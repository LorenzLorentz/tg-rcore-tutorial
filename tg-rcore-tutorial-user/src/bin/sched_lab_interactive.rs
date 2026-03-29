#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicUsize, Ordering};
use user_lib::{busy_spin, exit, lab_tick, semaphore_create, semaphore_down, semaphore_up, thread_create};

const WORKER_COUNT: usize = 4;
const ROUNDS: usize = 14;

static SEM_IDS: [AtomicUsize; WORKER_COUNT] = [
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
];

struct InteractiveArg {
    worker_id: usize,
}

fn interactive_worker(arg: *const InteractiveArg) -> isize {
    let arg = unsafe { &*arg };
    let sem_id = SEM_IDS[arg.worker_id].load(Ordering::Relaxed);
    let mut seed = arg.worker_id + 11;
    for _ in 0..ROUNDS {
        semaphore_down(sem_id);
        seed = busy_spin(180 + arg.worker_id * 30, seed);
        lab_tick();
    }
    println!("[t2l4-user] interactive worker={} done", arg.worker_id);
    exit((seed & 0x7fff) as i32)
}

fn event_loop(_: usize) -> isize {
    let mut seed = 29usize;
    for round in 0..(WORKER_COUNT * ROUNDS) {
        seed = busy_spin(260 + (round % 3) * 40, seed + round);
        lab_tick();
        semaphore_up(SEM_IDS[round % WORKER_COUNT].load(Ordering::Relaxed));
    }
    println!("[t2l4-user] interactive producer done");
    exit((seed & 0x7fff) as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    println!("[t2l4-user] scenario=interactive");
    for idx in 0..WORKER_COUNT {
        SEM_IDS[idx].store(semaphore_create(0) as usize, Ordering::Relaxed);
        let leaked = Box::leak(Box::new(InteractiveArg { worker_id: idx }));
        thread_create(
            interactive_worker as *const () as usize,
            leaked as *const _ as usize,
        );
    }
    thread_create(event_loop as *const () as usize, 0);
    0
}
