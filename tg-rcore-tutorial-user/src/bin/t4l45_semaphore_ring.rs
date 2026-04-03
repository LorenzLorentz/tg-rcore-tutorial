#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use user_lib::{
    AtomicStats, busy_spin, exit, kernel_metrics, lab_tick, now_us, reset_kernel_metrics,
    sched_yield, semaphore_create, semaphore_down_contended, semaphore_up, thread_create, waittid,
};

const THREADS: usize = 6;
const ROUNDS: usize = 72;
const STARVATION_US: usize = 2_000_000;

static WAIT_STATS: AtomicStats = AtomicStats::new();
static HOLD_STATS: AtomicStats = AtomicStats::new();
static HANDOFFS: AtomicUsize = AtomicUsize::new(0);

static SEM_IDS: [AtomicUsize; THREADS] = [
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
];

struct WorkerArg {
    id: usize,
    next: usize,
}

unsafe fn worker(arg: *const WorkerArg) -> isize {
    let arg = &*arg;
    let sem_id = SEM_IDS[arg.id].load(Ordering::Relaxed);
    let next_sem = SEM_IDS[arg.next].load(Ordering::Relaxed);
    let mut local = arg.id + 1;

    for round in 0..ROUNDS {
        let wait_start = now_us();
        let contended = semaphore_down_contended(sem_id);
        let acquired = now_us();
        WAIT_STATS.record_wait(acquired - wait_start, contended, STARVATION_US);

        let hold_start = acquired;
        let handoff = HANDOFFS.fetch_add(1, Ordering::SeqCst) + 1;
        local = busy_spin(850 + (arg.id + round % 4) * 70, local ^ handoff);
        lab_tick();
        HOLD_STATS.record_hold_sample(now_us() - hold_start);

        semaphore_up(next_sem);
        if round % 5 == 0 {
            sched_yield();
        }
    }

    exit((local & 0x7fff) as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    for idx in 0..THREADS {
        let initial = if idx == 0 { 1 } else { 0 };
        SEM_IDS[idx].store(semaphore_create(initial) as usize, Ordering::Relaxed);
    }

    reset_kernel_metrics();
    let base = kernel_metrics();
    let start = now_us();

    let args: Vec<_> = (0..THREADS)
        .map(|id| WorkerArg {
            id,
            next: (id + 1) % THREADS,
        })
        .collect();
    let mut tids = Vec::new();
    for arg in &args {
        tids.push(thread_create(worker as *const () as usize, arg as *const _ as usize) as usize);
    }
    for tid in tids {
        waittid(tid);
    }

    let elapsed_us = now_us() - start;
    let wait_summary = WAIT_STATS.snapshot();
    let hold_summary = HOLD_STATS.snapshot();
    let kernel = kernel_metrics().diff(base);
    let handoffs = HANDOFFS.load(Ordering::Relaxed);

    assert_eq!(handoffs, THREADS * ROUNDS);
    assert!(kernel.blocked_sync_ops > 0);
    assert!(kernel.wakeups > 0);

    println!(
        "[t4l45-summary] case=semaphore_ring ops={} contention={} avg_wait_us={} max_wait_us={} avg_hold_us={} max_hold_us={} elapsed_us={} throughput_ops_per_sec={} ctx_switches={} blocked={} wakeups={} starvation={}",
        handoffs,
        wait_summary.contentions,
        wait_summary.avg_wait_us(),
        wait_summary.max_wait_us,
        hold_summary.avg_hold_us(),
        hold_summary.max_hold_us,
        elapsed_us,
        handoffs * 1_000_000 / elapsed_us.max(1),
        kernel.context_switches,
        kernel.blocked_sync_ops,
        kernel.wakeups,
        wait_summary.starvation,
    );
    println!("t4l45 semaphore ring passed!");
    0
}
