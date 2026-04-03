#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use user_lib::{busy_spin, current_hart_id, exit, sched_yield, thread_create, waittid};

const THREADS: usize = 8;
const SAMPLES: usize = 96;

static READY: AtomicUsize = AtomicUsize::new(0);
static START: AtomicUsize = AtomicUsize::new(0);
static HART_MASK: AtomicUsize = AtomicUsize::new(0);
static THREAD_MASKS: [AtomicUsize; THREADS] = [
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
];

unsafe fn worker(arg: *const usize) -> isize {
    let id = *arg;
    let mut seed = id + 1;

    READY.fetch_add(1, Ordering::SeqCst);
    while START.load(Ordering::SeqCst) == 0 {
        sched_yield();
    }

    let mut local_mask = 0usize;
    for round in 0..SAMPLES {
        let hart = current_hart_id();
        let hart_bit = 1usize << hart;
        local_mask |= hart_bit;
        HART_MASK.fetch_or(hart_bit, Ordering::SeqCst);

        seed = busy_spin(5_000 + (id % 4) * 700 + (round % 5) * 120, seed ^ round ^ hart);
        if round % 3 == 0 {
            sched_yield();
        }
    }

    THREAD_MASKS[id].store(local_mask, Ordering::SeqCst);
    exit((seed & 0x7fff) as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let ids: Vec<_> = (0..THREADS).collect();
    let mut tids = Vec::new();
    for id in &ids {
        tids.push(thread_create(worker as *const () as usize, id as *const _ as usize) as usize);
    }

    while READY.load(Ordering::SeqCst) != THREADS {
        sched_yield();
    }
    START.store(1, Ordering::SeqCst);

    for tid in tids {
        waittid(tid);
    }

    let hart_mask = HART_MASK.load(Ordering::SeqCst);
    let harts_seen = hart_mask.count_ones() as usize;
    let migrated_threads = THREAD_MASKS
        .iter()
        .filter(|mask| mask.load(Ordering::SeqCst).count_ones() > 1)
        .count();

    for (idx, mask) in THREAD_MASKS.iter().enumerate() {
        assert_ne!(mask.load(Ordering::SeqCst), 0, "thread {idx} never recorded a hart");
    }
    assert!(
        harts_seen >= 2,
        "expected at least 2 harts, saw mask=0x{:x}",
        hart_mask
    );

    println!(
        "[t4l45-summary] case=smp_probe threads={} samples={} harts_seen={} migrated_threads={} mask=0x{:x}",
        THREADS, SAMPLES, harts_seen, migrated_threads, hart_mask,
    );
    println!("t4l45 smp probe passed!");
    0
}
