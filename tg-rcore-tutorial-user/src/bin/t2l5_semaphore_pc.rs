#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use user_lib::{
    AtomicStats, busy_spin, exit, kernel_metrics, now_us, reset_kernel_metrics, semaphore_create,
    semaphore_down_contended, semaphore_up, thread_create, waittid,
};

const SEM_EMPTY: usize = 0;
const SEM_FULL: usize = 1;
const SEM_MUTEX: usize = 2;
const BUFFER_SIZE: usize = 8;
const PRODUCER_COUNT: usize = 3;
const CONSUMER_COUNT: usize = 3;
const ITEMS_PER_PRODUCER: usize = 40;
const ITEMS_PER_CONSUMER: usize = PRODUCER_COUNT * ITEMS_PER_PRODUCER / CONSUMER_COUNT;
const STARVATION_US: usize = 2_000_000;

static MUTEX_STATS: AtomicStats = AtomicStats::new();
static GATE_BLOCKS: AtomicUsize = AtomicUsize::new(0);
static PRODUCED_SUM: AtomicUsize = AtomicUsize::new(0);
static CONSUMED_SUM: AtomicUsize = AtomicUsize::new(0);

static mut BUFFER: [usize; BUFFER_SIZE] = [0; BUFFER_SIZE];
static mut HEAD: usize = 0;
static mut TAIL: usize = 0;

unsafe fn producer(arg: *const usize) -> isize {
    let id = *arg;
    let mut sum = 0usize;
    for round in 0..ITEMS_PER_PRODUCER {
        if semaphore_down_contended(SEM_EMPTY) {
            GATE_BLOCKS.fetch_add(1, Ordering::Relaxed);
        }

        let wait_start = now_us();
        let contended = semaphore_down_contended(SEM_MUTEX);
        let acquired = now_us();
        MUTEX_STATS.record_wait(acquired - wait_start, contended, STARVATION_US);

        let hold_start = acquired;
        let item = id * 1_000 + round;
        BUFFER[HEAD] = item;
        HEAD = (HEAD + 1) % BUFFER_SIZE;
        sum = sum.wrapping_add(item);
        busy_spin(1_000, item + 1);
        let hold_end = now_us();
        semaphore_up(SEM_MUTEX);
        MUTEX_STATS.record_hold(hold_end - hold_start);

        semaphore_up(SEM_FULL);
    }
    PRODUCED_SUM.fetch_add(sum, Ordering::Relaxed);
    exit(0)
}

unsafe fn consumer(arg: *const usize) -> isize {
    let _id = *arg;
    let mut sum = 0usize;
    for _ in 0..ITEMS_PER_CONSUMER {
        if semaphore_down_contended(SEM_FULL) {
            GATE_BLOCKS.fetch_add(1, Ordering::Relaxed);
        }

        let wait_start = now_us();
        let contended = semaphore_down_contended(SEM_MUTEX);
        let acquired = now_us();
        MUTEX_STATS.record_wait(acquired - wait_start, contended, STARVATION_US);

        let hold_start = acquired;
        let item = BUFFER[TAIL];
        TAIL = (TAIL + 1) % BUFFER_SIZE;
        sum = sum.wrapping_add(item);
        busy_spin(800, item + 7);
        let hold_end = now_us();
        semaphore_up(SEM_MUTEX);
        MUTEX_STATS.record_hold(hold_end - hold_start);

        semaphore_up(SEM_EMPTY);
    }
    CONSUMED_SUM.fetch_add(sum, Ordering::Relaxed);
    exit(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    assert_eq!(semaphore_create(BUFFER_SIZE), SEM_EMPTY as isize);
    assert_eq!(semaphore_create(0), SEM_FULL as isize);
    assert_eq!(semaphore_create(1), SEM_MUTEX as isize);

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

    let summary = MUTEX_STATS.snapshot();
    let kernel = kernel_metrics().diff(base);
    let produced = PRODUCED_SUM.load(Ordering::Relaxed);
    let consumed = CONSUMED_SUM.load(Ordering::Relaxed);
    assert_eq!(produced, consumed);

    println!(
        "[t2l5-summary] primitive=semaphore variant=producer_consumer ops={} contention={} avg_wait_us={} max_wait_us={} avg_hold_us={} max_hold_us={} ctx_switches={} blocked={} wakeups={} starvation={} gate_blocks={}",
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
        GATE_BLOCKS.load(Ordering::Relaxed),
    );
    println!("t2l5 semaphore producer-consumer passed!");
    0
}
