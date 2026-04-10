#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use core::ptr;
use user_lib::{
    AtomicStats, FairRwLock, busy_spin, exit, kernel_metrics, now_us, reset_kernel_metrics,
    sched_yield, sleep, thread_create, waittid,
};

static READ_STATS: AtomicStats = AtomicStats::new();
static WRITE_STATS: AtomicStats = AtomicStats::new();
static mut VALUE: usize = 0;
static mut LOCK_PTR: *const FairRwLock = ptr::null();

const READERS: usize = 5;
const WRITERS: usize = 2;
const READER_LOOPS: usize = 120;
const WRITER_LOOPS: usize = 36;
const READER_STARVATION_US: usize = 120_000;
const WRITER_STARVATION_US: usize = 120_000;

unsafe fn read_worker(arg: *const usize) -> isize {
    let id = *arg;
    let lock = &*LOCK_PTR;
    for round in 0..READER_LOOPS {
        let wait_start = now_us();
        let contended = lock.read_lock();
        let acquired = now_us();
        READ_STATS.record_wait(acquired - wait_start, contended, READER_STARVATION_US);

        let hold_start = acquired;
        let value = (&raw const VALUE).read_volatile();
        busy_spin(1_200, value ^ id ^ round);
        let hold_end = now_us();
        lock.read_unlock();
        READ_STATS.record_hold(hold_end - hold_start);

        if round % 24 == 0 {
            sched_yield();
        }
    }
    exit(0)
}

unsafe fn write_worker(arg: *const usize) -> isize {
    let id = *arg;
    let lock = &*LOCK_PTR;
    sleep(5 + id * 2);
    for round in 0..WRITER_LOOPS {
        let wait_start = now_us();
        let contended = lock.write_lock();
        let acquired = now_us();
        WRITE_STATS.record_wait(acquired - wait_start, contended, WRITER_STARVATION_US);

        let hold_start = acquired;
        let value = (&raw const VALUE).read_volatile();
        (&raw mut VALUE).write_volatile(value + 1);
        busy_spin(2_000, value ^ round ^ id);
        let hold_end = now_us();
        lock.write_unlock();
        WRITE_STATS.record_hold(hold_end - hold_start);
        sched_yield();
    }
    exit(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let lock = Box::leak(Box::new(FairRwLock::new()));
    unsafe {
        LOCK_PTR = lock as *const _;
    }

    reset_kernel_metrics();
    let base = kernel_metrics();

    let ids: Vec<_> = (0..READERS.max(WRITERS)).collect();
    let mut tids = Vec::new();
    for idx in 0..READERS {
        tids.push(thread_create(
            read_worker as *const () as usize,
            &ids[idx] as *const _ as usize,
        ) as usize);
    }
    for idx in 0..WRITERS {
        tids.push(thread_create(
            write_worker as *const () as usize,
            &ids[idx] as *const _ as usize,
        ) as usize);
    }
    for tid in tids {
        waittid(tid);
    }

    let read_summary = READ_STATS.snapshot();
    let write_summary = WRITE_STATS.snapshot();
    let kernel = kernel_metrics().diff(base);
    let final_value = unsafe { (&raw const VALUE).read_volatile() };
    assert_eq!(final_value, WRITERS * WRITER_LOOPS);
    assert_eq!(write_summary.starvation, 0);

    println!(
        "[t2l5-summary] primitive=rwlock variant=fair read_ops={} write_ops={} contention={} avg_wait_us={} max_wait_us={} avg_hold_us={} max_hold_us={} ctx_switches={} blocked={} wakeups={} starvation={} bug_total={} bug_exact={} bug_heuristic={} bug_statistical={}",
        read_summary.acquisitions,
        write_summary.acquisitions,
        read_summary.contentions + write_summary.contentions,
        write_summary.avg_wait_us(),
        write_summary.max_wait_us,
        write_summary.avg_hold_us(),
        write_summary.max_hold_us,
        kernel.context_switches,
        kernel.blocked_sync_ops,
        kernel.wakeups,
        write_summary.starvation,
        kernel.bug_total,
        kernel.bug_exact,
        kernel.bug_heuristic,
        kernel.bug_statistical,
    );
    println!("t2l5 fair rwlock passed!");
    0
}
