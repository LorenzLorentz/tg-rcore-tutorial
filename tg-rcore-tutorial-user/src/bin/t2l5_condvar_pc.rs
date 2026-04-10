#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use user_lib::{
    AtomicStats, busy_spin, condvar_create, condvar_signal, condvar_wait_contended, exit,
    kernel_metrics, mutex_create, mutex_lock_contended, mutex_unlock, now_us,
    reset_kernel_metrics, sleep, thread_create, waittid,
};

const MUTEX_ID: usize = 0;
const COND_NOT_EMPTY: usize = 0;
const COND_NOT_FULL: usize = 1;
const BUFFER_SIZE: usize = 2;
const PRODUCER_COUNT: usize = 2;
const CONSUMER_COUNT: usize = 1;
const ITEMS_PER_PRODUCER: usize = 32;
const ITEMS_PER_CONSUMER: usize = PRODUCER_COUNT * ITEMS_PER_PRODUCER / CONSUMER_COUNT;
const STARVATION_US: usize = 2_000_000;

static WAIT_STATS: AtomicStats = AtomicStats::new();
static HOLD_STATS: AtomicStats = AtomicStats::new();
static PRODUCED_SUM: AtomicUsize = AtomicUsize::new(0);
static CONSUMED_SUM: AtomicUsize = AtomicUsize::new(0);

static mut BUFFER: [usize; BUFFER_SIZE] = [0; BUFFER_SIZE];
static mut HEAD: usize = 0;
static mut TAIL: usize = 0;
static mut COUNT: usize = 0;

unsafe fn producer(arg: *const usize) -> isize {
    let id = *arg;
    let mut sum = 0usize;
    for round in 0..ITEMS_PER_PRODUCER {
        let _ = mutex_lock_contended(MUTEX_ID);
        while (&raw const COUNT).read_volatile() == BUFFER_SIZE {
            let wait_start = now_us();
            let contended = condvar_wait_contended(COND_NOT_FULL, MUTEX_ID);
            WAIT_STATS.record_wait(now_us() - wait_start, contended, STARVATION_US);
        }

        let hold_start = now_us();
        let item = id * 2_000 + round;
        BUFFER[HEAD] = item;
        HEAD = (HEAD + 1) % BUFFER_SIZE;
        COUNT += 1;
        sum = sum.wrapping_add(item);
        let hold_end = now_us();
        HOLD_STATS.record_hold_sample(hold_end - hold_start);
        condvar_signal(COND_NOT_EMPTY);
        mutex_unlock(MUTEX_ID);
        busy_spin(200, item + 3);
    }
    PRODUCED_SUM.fetch_add(sum, Ordering::Relaxed);
    exit(0)
}

unsafe fn consumer(arg: *const usize) -> isize {
    let _id = *arg;
    let mut sum = 0usize;
    for _ in 0..ITEMS_PER_CONSUMER {
        let _ = mutex_lock_contended(MUTEX_ID);
        while (&raw const COUNT).read_volatile() == 0 {
            let wait_start = now_us();
            let contended = condvar_wait_contended(COND_NOT_EMPTY, MUTEX_ID);
            WAIT_STATS.record_wait(now_us() - wait_start, contended, STARVATION_US);
        }

        let hold_start = now_us();
        let item = BUFFER[TAIL];
        TAIL = (TAIL + 1) % BUFFER_SIZE;
        COUNT -= 1;
        sum = sum.wrapping_add(item);
        let hold_end = now_us();
        HOLD_STATS.record_hold_sample(hold_end - hold_start);
        condvar_signal(COND_NOT_FULL);
        mutex_unlock(MUTEX_ID);
        busy_spin(500, item + 11);
        sleep(5);
    }
    CONSUMED_SUM.fetch_add(sum, Ordering::Relaxed);
    exit(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    assert_eq!(mutex_create(true), MUTEX_ID as isize);
    assert_eq!(condvar_create(), COND_NOT_EMPTY as isize);
    assert_eq!(condvar_create(), COND_NOT_FULL as isize);

    reset_kernel_metrics();
    let base = kernel_metrics();

    let ids: Vec<_> = (0..PRODUCER_COUNT.max(CONSUMER_COUNT)).collect();
    let mut tids = Vec::new();
    for idx in 0..PRODUCER_COUNT {
        tids.push(thread_create(
            producer as *const () as usize,
            &ids[idx] as *const _ as usize,
        ) as usize);
    }
    for idx in 0..CONSUMER_COUNT {
        tids.push(thread_create(
            consumer as *const () as usize,
            &ids[idx] as *const _ as usize,
        ) as usize);
    }
    for tid in tids {
        waittid(tid);
    }

    let wait_summary = WAIT_STATS.snapshot();
    let hold_summary = HOLD_STATS.snapshot();
    let kernel = kernel_metrics().diff(base);
    let produced = PRODUCED_SUM.load(Ordering::Relaxed);
    let consumed = CONSUMED_SUM.load(Ordering::Relaxed);
    assert_eq!(produced, consumed);

    println!(
        "[t2l5-summary] primitive=condvar variant=producer_consumer ops={} contention={} avg_wait_us={} max_wait_us={} avg_hold_us={} max_hold_us={} ctx_switches={} blocked={} wakeups={} starvation={} bug_total={} bug_exact={} bug_heuristic={} bug_statistical={}",
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
        kernel.bug_total,
        kernel.bug_exact,
        kernel.bug_heuristic,
        kernel.bug_statistical,
    );
    println!("t2l5 condvar producer-consumer passed!");
    0
}
