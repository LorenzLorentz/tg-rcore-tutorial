#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicUsize, Ordering};
use user_lib::{busy_spin, exit, lab_tick, semaphore_create, semaphore_down, semaphore_up, thread_create};

const WORKER_COUNT: usize = 4;
const ROUNDS: usize = 8;

static SEM_IDS: [AtomicUsize; WORKER_COUNT] = [
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
];

struct IoArg {
    worker_id: usize,
    spin_rounds: usize,
}

fn io_worker(arg: *const IoArg) -> isize {
    let arg = unsafe { &*arg };
    let sem_id = SEM_IDS[arg.worker_id].load(Ordering::Relaxed);
    let mut seed = arg.worker_id + 1;
    for _ in 0..ROUNDS {
        semaphore_down(sem_id);
        seed = busy_spin(arg.spin_rounds, seed);
        lab_tick();
    }
    println!("[t2l4-user] io worker={} done", arg.worker_id);
    exit((seed & 0x7fff) as i32)
}

fn io_producer(_: usize) -> isize {
    let mut seed = 17usize;
    for round in 0..(WORKER_COUNT * ROUNDS) {
        seed = busy_spin(900, seed + round);
        lab_tick();
        let worker = round % WORKER_COUNT;
        semaphore_up(SEM_IDS[worker].load(Ordering::Relaxed));
    }
    println!("[t2l4-user] io producer done");
    exit((seed & 0x7fff) as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    println!("[t2l4-user] scenario=io");
    for (idx, slot) in SEM_IDS.iter().enumerate() {
        slot.store(semaphore_create(0) as usize, Ordering::Relaxed);
        let leaked = Box::leak(Box::new(IoArg {
            worker_id: idx,
            spin_rounds: 500 + idx * 120,
        }));
        thread_create(io_worker as *const () as usize, leaked as *const _ as usize);
    }
    thread_create(io_producer as *const () as usize, 0);
    0
}
