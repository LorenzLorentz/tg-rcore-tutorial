#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::vec::Vec;
use user_lib::{
    AtomicStats, busy_spin, exit, kernel_metrics, mutex_create, mutex_lock_contended,
    mutex_unlock, now_us, reset_kernel_metrics, sched_yield, thread_create, waittid,
};

static STATS: AtomicStats = AtomicStats::new();
static mut COUNTER: usize = 0;

const MUTEX_ID: usize = 0;
const THREADS: usize = 6;
const ITERATIONS: usize = 160;
const STARVATION_US: usize = 2_000_000;

unsafe fn worker(arg: *const usize) -> isize {
    let seed = *arg + 1;
    let mut local = seed;
    for round in 0..ITERATIONS {
        let wait_start = now_us();
        let contended = mutex_lock_contended(MUTEX_ID);
        let acquired = now_us();
        STATS.record_wait(acquired - wait_start, contended, STARVATION_US);

        let hold_start = acquired;
        let counter = (&raw mut COUNTER).read_volatile();
        local = busy_spin(3_000, local ^ round);
        (&raw mut COUNTER).write_volatile(counter + 1);
        let hold_end = now_us();
        mutex_unlock(MUTEX_ID);
        STATS.record_hold(hold_end - hold_start);

        if round % 16 == 0 {
            sched_yield();
        }
    }
    exit(local as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    assert_eq!(mutex_create(true), MUTEX_ID as isize);
    reset_kernel_metrics();
    let base = kernel_metrics();

    let seeds: Vec<_> = (0..THREADS).collect();
    let mut tids = Vec::new();
    for idx in 0..THREADS {
        tids.push(thread_create(
            worker as *const () as usize,
            &seeds[idx] as *const _ as usize,
        ) as usize);
    }
    for tid in tids {
        waittid(tid);
    }

    let summary = STATS.snapshot();
    let kernel = kernel_metrics().diff(base);
    let counter = unsafe { (&raw const COUNTER).read_volatile() };
    assert_eq!(counter, THREADS * ITERATIONS);

    println!(
        "[t2l5-summary] primitive=mutex variant=fifo_blocking ops={} contention={} avg_wait_us={} max_wait_us={} avg_hold_us={} max_hold_us={} ctx_switches={} blocked={} wakeups={} starvation={} bug_total={} bug_exact={} bug_heuristic={} bug_statistical={}",
        summary.acquisitions,
        summary.contentions,
        summary.avg_wait_us(),
        summary.max_wait_us,
        summary.avg_hold_us(),
        summary.max_hold_us,
        kernel.context_switches,
        kernel.blocked_sync_ops,
        kernel.wakeups,
        summary.starvation,
        kernel.bug_total,
        kernel.bug_exact,
        kernel.bug_heuristic,
        kernel.bug_statistical,
    );
    println!("t2l5 mutex stress passed!");
    0
}
