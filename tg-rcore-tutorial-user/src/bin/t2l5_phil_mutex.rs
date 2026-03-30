#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::vec::Vec;
use user_lib::{
    AtomicStats, exit, kernel_metrics, mutex_create, mutex_lock_contended, mutex_unlock, now_us,
    reset_kernel_metrics, sleep, thread_create, waittid,
};

const N: usize = 5;
const ROUND: usize = 4;
const STARVATION_US: usize = 2_000_000;
const ARR: [[usize; ROUND * 2]; N] = [
    [20, 30, 35, 18, 26, 22, 14, 20],
    [12, 22, 10, 30, 26, 14, 10, 24],
    [18, 14, 28, 12, 20, 18, 34, 16],
    [16, 28, 22, 14, 24, 16, 12, 28],
    [20, 10, 18, 20, 12, 16, 22, 12],
];

static WAIT_STATS: AtomicStats = AtomicStats::new();
static HOLD_STATS: AtomicStats = AtomicStats::new();

unsafe fn philosopher(arg: *const usize) -> isize {
    let id = *arg;
    let left = id;
    let right = if id + 1 == N { 0 } else { id + 1 };
    let min = left.min(right);
    let max = left.max(right);

    for round in 0..ROUND {
        sleep(ARR[id][2 * round]);

        let wait_start = now_us();
        let contended_left = mutex_lock_contended(min);
        let contended_right = mutex_lock_contended(max);
        let acquired = now_us();
        WAIT_STATS.record_wait(
            acquired - wait_start,
            contended_left || contended_right,
            STARVATION_US,
        );

        let hold_start = acquired;
        sleep(ARR[id][2 * round + 1]);
        let hold_end = now_us();
        HOLD_STATS.record_hold_sample(hold_end - hold_start);

        mutex_unlock(max);
        mutex_unlock(min);
    }
    exit(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    for idx in 0..N {
        assert_eq!(mutex_create(true), idx as isize);
    }

    reset_kernel_metrics();
    let base = kernel_metrics();

    let ids: Vec<_> = (0..N).collect();
    let mut tids = Vec::new();
    for idx in 0..N {
        tids.push(thread_create(
            philosopher as *const () as usize,
            &ids[idx] as *const _ as usize,
        ) as usize);
    }
    for tid in tids {
        waittid(tid);
    }

    let wait_summary = WAIT_STATS.snapshot();
    let hold_summary = HOLD_STATS.snapshot();
    let kernel = kernel_metrics().diff(base);

    println!(
        "[t2l5-summary] primitive=mutex variant=philosophers ops={} contention={} avg_wait_us={} max_wait_us={} avg_hold_us={} max_hold_us={} ctx_switches={} blocked={} wakeups={} starvation={}",
        wait_summary.acquisitions,
        wait_summary.contentions,
        wait_summary.avg_wait_us(),
        wait_summary.max_wait_us,
        hold_summary.avg_hold_us(),
        hold_summary.max_hold_us,
        kernel.context_switches,
        kernel.blocked_sync_ops,
        kernel.wakeups,
        wait_summary.starvation,
    );
    println!("t2l5 philosophers passed!");
    0
}
