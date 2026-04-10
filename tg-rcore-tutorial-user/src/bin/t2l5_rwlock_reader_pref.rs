#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::{boxed::Box, format, vec::Vec};
use core::ptr;
use user_lib::{
    AtomicStats, ReaderPreferRwLock, busy_spin, exit, now_us, print_user_bug, sched_yield, sleep,
    thread_create, waittid,
};

static READ_STATS: AtomicStats = AtomicStats::new();
static WRITE_STATS: AtomicStats = AtomicStats::new();
static mut VALUE: usize = 0;
static mut LOCK_PTR: *const ReaderPreferRwLock = ptr::null();

const READERS: usize = 7;
const WRITERS: usize = 1;
const READER_LOOPS: usize = 200;
const WRITER_LOOPS: usize = 12;
const WRITER_STARVATION_US: usize = 8_000;

unsafe fn read_worker(arg: *const usize) -> isize {
    let id = *arg;
    let lock = &*LOCK_PTR;
    for round in 0..READER_LOOPS {
        let wait_start = now_us();
        let contended = lock.read_lock();
        let acquired = now_us();
        READ_STATS.record_wait(acquired - wait_start, contended, 0);

        let hold_start = acquired;
        let value = (&raw const VALUE).read_volatile();
        busy_spin(2_000, value ^ id ^ round);
        let hold_end = now_us();
        lock.read_unlock();
        READ_STATS.record_hold(hold_end - hold_start);

        if round % 8 == 0 {
            sched_yield();
        }
    }
    exit(0)
}

unsafe fn write_worker(arg: *const usize) -> isize {
    let id = *arg;
    let lock = &*LOCK_PTR;
    sleep(5 + id);
    for round in 0..WRITER_LOOPS {
        let wait_start = now_us();
        let contended = lock.write_lock();
        let acquired = now_us();
        WRITE_STATS.record_wait(acquired - wait_start, contended, WRITER_STARVATION_US);

        let hold_start = acquired;
        let value = (&raw const VALUE).read_volatile();
        (&raw mut VALUE).write_volatile(value + 1);
        busy_spin(1_500, value ^ round ^ id);
        let hold_end = now_us();
        lock.write_unlock();
        WRITE_STATS.record_hold(hold_end - hold_start);
    }
    exit(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let lock = Box::leak(Box::new(ReaderPreferRwLock::new()));
    unsafe {
        LOCK_PTR = lock as *const _;
    }

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

    let writer_summary = WRITE_STATS.snapshot();
    println!(
        "[t2l5-control] reader_prefer writer_max_wait_us={} starvation={}",
        writer_summary.max_wait_us,
        writer_summary.starvation,
    );
    if writer_summary.starvation > 0 {
        print_user_bug(
            "statistical",
            "starvation",
            "rwlock",
            &format!(
                "variant=reader_prefer writer_max_wait_us={} starvation={}",
                writer_summary.max_wait_us, writer_summary.starvation
            ),
        );
        println!("reader-prefer rwlock starved writer as expected");
        0
    } else {
        println!("reader-prefer rwlock unexpectedly passed");
        1
    }
}
