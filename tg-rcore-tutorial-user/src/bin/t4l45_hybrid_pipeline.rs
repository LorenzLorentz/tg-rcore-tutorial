#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::vec::Vec;
use core::cmp::max;
use core::sync::atomic::{AtomicUsize, Ordering};
use user_lib::{
    AtomicStats, busy_spin, condvar_create, condvar_signal, condvar_wait_contended, exit,
    kernel_metrics, lab_tick, mutex_create, mutex_lock_contended, mutex_unlock, now_us,
    reset_kernel_metrics, sched_yield, thread_create, waittid,
};

const MUTEX_ID: usize = 0;
const COND_NOT_EMPTY: usize = 0;
const COND_NOT_FULL: usize = 1;
const BUFFER_SIZE: usize = 4;
const PRODUCERS: usize = 2;
const CONSUMERS: usize = 2;
const CPU_HOGS: usize = 1;
const ITEMS_PER_PRODUCER: usize = 32;
const ITEMS_PER_CONSUMER: usize = PRODUCERS * ITEMS_PER_PRODUCER / CONSUMERS;
const CPU_ROUNDS: usize = 32;
const STARVATION_US: usize = 3_000_000;

static LOCK_STATS: AtomicStats = AtomicStats::new();
static WAIT_STATS: AtomicStats = AtomicStats::new();
static HOLD_STATS: AtomicStats = AtomicStats::new();
static PRODUCED_COUNT: AtomicUsize = AtomicUsize::new(0);
static CONSUMED_COUNT: AtomicUsize = AtomicUsize::new(0);
static PRODUCED_SUM: AtomicUsize = AtomicUsize::new(0);
static CONSUMED_SUM: AtomicUsize = AtomicUsize::new(0);

static mut BUFFER: [usize; BUFFER_SIZE] = [0; BUFFER_SIZE];
static mut HEAD: usize = 0;
static mut TAIL: usize = 0;
static mut COUNT: usize = 0;

struct CpuArg {
    id: usize,
}

unsafe fn producer(arg: *const usize) -> isize {
    let id = *arg;
    let mut local = id + 11;
    for round in 0..ITEMS_PER_PRODUCER {
        let wait_start = now_us();
        let contended = mutex_lock_contended(MUTEX_ID);
        let acquired = now_us();
        LOCK_STATS.record_wait(acquired - wait_start, contended, STARVATION_US);

        while (&raw const COUNT).read_volatile() == BUFFER_SIZE {
            let blocked_start = now_us();
            let blocked = condvar_wait_contended(COND_NOT_FULL, MUTEX_ID);
            WAIT_STATS.record_wait(now_us() - blocked_start, blocked, STARVATION_US);
        }

        let hold_start = now_us();
        let item = id * 10_000 + round;
        BUFFER[HEAD] = item;
        HEAD = (HEAD + 1) % BUFFER_SIZE;
        COUNT += 1;
        PRODUCED_COUNT.fetch_add(1, Ordering::Relaxed);
        PRODUCED_SUM.fetch_add(item, Ordering::Relaxed);
        local = busy_spin(280 + (round % 5) * 40, local ^ item);
        HOLD_STATS.record_hold_sample(now_us() - hold_start);
        condvar_signal(COND_NOT_EMPTY);
        mutex_unlock(MUTEX_ID);

        local = busy_spin(420 + id * 70, local ^ round);
        lab_tick();
        if round % 8 == 0 {
            sched_yield();
        }
    }
    exit((local & 0x7fff) as i32)
}

unsafe fn consumer(arg: *const usize) -> isize {
    let id = *arg;
    let mut local = id + 101;
    for round in 0..ITEMS_PER_CONSUMER {
        let wait_start = now_us();
        let contended = mutex_lock_contended(MUTEX_ID);
        let acquired = now_us();
        LOCK_STATS.record_wait(acquired - wait_start, contended, STARVATION_US);

        while (&raw const COUNT).read_volatile() == 0 {
            let blocked_start = now_us();
            let blocked = condvar_wait_contended(COND_NOT_EMPTY, MUTEX_ID);
            WAIT_STATS.record_wait(now_us() - blocked_start, blocked, STARVATION_US);
        }

        let hold_start = now_us();
        let item = BUFFER[TAIL];
        TAIL = (TAIL + 1) % BUFFER_SIZE;
        COUNT -= 1;
        CONSUMED_COUNT.fetch_add(1, Ordering::Relaxed);
        CONSUMED_SUM.fetch_add(item, Ordering::Relaxed);
        local = busy_spin(320 + (round % 7) * 30, local ^ item);
        HOLD_STATS.record_hold_sample(now_us() - hold_start);
        condvar_signal(COND_NOT_FULL);
        mutex_unlock(MUTEX_ID);

        local = busy_spin(520 + id * 80, local ^ round);
        lab_tick();
        if round % 6 == 0 {
            sched_yield();
        }
    }
    exit((local & 0x7fff) as i32)
}

unsafe fn cpu_hog(arg: *const CpuArg) -> isize {
    let arg = &*arg;
    let mut local = arg.id + 1_001;
    for round in 0..CPU_ROUNDS {
        local = busy_spin(4_000 + arg.id * 500, local ^ round);
        lab_tick();
        if round % 4 == 0 {
            sched_yield();
        }
    }
    exit((local & 0x7fff) as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    assert_eq!(mutex_create(true), MUTEX_ID as isize);
    assert_eq!(condvar_create(), COND_NOT_EMPTY as isize);
    assert_eq!(condvar_create(), COND_NOT_FULL as isize);

    reset_kernel_metrics();
    let base = kernel_metrics();
    let start = now_us();

    let ids: Vec<_> = (0..PRODUCERS.max(CONSUMERS)).collect();
    let cpu_args: Vec<_> = (0..CPU_HOGS).map(|id| CpuArg { id }).collect();
    let mut tids = Vec::new();
    for idx in 0..PRODUCERS {
        tids.push(thread_create(
            producer as *const () as usize,
            &ids[idx] as *const _ as usize,
        ) as usize);
    }
    for idx in 0..CONSUMERS {
        tids.push(thread_create(
            consumer as *const () as usize,
            &ids[idx] as *const _ as usize,
        ) as usize);
    }
    for arg in &cpu_args {
        tids.push(thread_create(cpu_hog as *const () as usize, arg as *const _ as usize) as usize);
    }
    for tid in tids {
        waittid(tid);
    }

    let elapsed_us = now_us() - start;
    let lock_summary = LOCK_STATS.snapshot();
    let wait_summary = WAIT_STATS.snapshot();
    let hold_summary = HOLD_STATS.snapshot();
    let kernel = kernel_metrics().diff(base);

    let produced_count = PRODUCED_COUNT.load(Ordering::Relaxed);
    let consumed_count = CONSUMED_COUNT.load(Ordering::Relaxed);
    let produced_sum = PRODUCED_SUM.load(Ordering::Relaxed);
    let consumed_sum = CONSUMED_SUM.load(Ordering::Relaxed);
    let remaining = unsafe { (&raw const COUNT).read_volatile() };

    assert_eq!(produced_count, PRODUCERS * ITEMS_PER_PRODUCER);
    assert_eq!(consumed_count, PRODUCERS * ITEMS_PER_PRODUCER);
    assert_eq!(produced_sum, consumed_sum);
    assert_eq!(remaining, 0);
    assert!(kernel.blocked_sync_ops > 0);
    assert!(kernel.wakeups > 0);

    let wait_ops = lock_summary.acquisitions + wait_summary.acquisitions;
    let total_wait_us = lock_summary.total_wait_us + wait_summary.total_wait_us;
    let avg_wait_us = if wait_ops == 0 {
        0
    } else {
        total_wait_us / wait_ops
    };
    let max_wait_us = max(lock_summary.max_wait_us, wait_summary.max_wait_us);
    let starvation = lock_summary.starvation + wait_summary.starvation;
    let throughput_ops_per_sec = consumed_count * 1_000_000 / elapsed_us.max(1);

    println!(
        "[t4l45-summary] case=hybrid_pipeline ops={} contention={} avg_wait_us={} max_wait_us={} avg_hold_us={} max_hold_us={} elapsed_us={} throughput_ops_per_sec={} ctx_switches={} blocked={} wakeups={} starvation={}",
        consumed_count,
        lock_summary.contentions + wait_summary.contentions,
        avg_wait_us,
        max_wait_us,
        hold_summary.avg_hold_us(),
        hold_summary.max_hold_us,
        elapsed_us,
        throughput_ops_per_sec,
        kernel.context_switches,
        kernel.blocked_sync_ops,
        kernel.wakeups,
        starvation,
    );
    println!("t4l45 hybrid pipeline passed!");
    0
}
