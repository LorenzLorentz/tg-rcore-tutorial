#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicUsize, Ordering};
use user_lib::{busy_spin, exit, lab_tick, semaphore_create, semaphore_down, semaphore_up, thread_create};

const INTERACTIVE_WORKERS: usize = 3;
const INTERACTIVE_ROUNDS: usize = 10;

static SEM_IDS: [AtomicUsize; INTERACTIVE_WORKERS] = [
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
    AtomicUsize::new(usize::MAX),
];

struct CpuArg {
    spin_rounds: usize,
    tick_count: usize,
    seed: usize,
}

struct InteractiveArg {
    worker_id: usize,
}

fn cpu_worker(arg: *const CpuArg) -> isize {
    let arg = unsafe { &*arg };
    let mut seed = arg.seed;
    for _ in 0..arg.tick_count {
        seed = busy_spin(arg.spin_rounds, seed);
        lab_tick();
    }
    println!("[t2l4-user] mixed cpu seed={} done", arg.seed);
    exit((seed & 0x7fff) as i32)
}

fn interactive_worker(arg: *const InteractiveArg) -> isize {
    let arg = unsafe { &*arg };
    let sem_id = SEM_IDS[arg.worker_id].load(Ordering::Relaxed);
    let mut seed = arg.worker_id + 41;
    for _ in 0..INTERACTIVE_ROUNDS {
        semaphore_down(sem_id);
        seed = busy_spin(220 + arg.worker_id * 40, seed);
        lab_tick();
    }
    println!("[t2l4-user] mixed interactive worker={} done", arg.worker_id);
    exit((seed & 0x7fff) as i32)
}

fn event_loop(_: usize) -> isize {
    let mut seed = 73usize;
    for round in 0..(INTERACTIVE_WORKERS * INTERACTIVE_ROUNDS) {
        seed = busy_spin(320 + (round % 4) * 60, seed + round);
        lab_tick();
        semaphore_up(SEM_IDS[round % INTERACTIVE_WORKERS].load(Ordering::Relaxed));
    }
    println!("[t2l4-user] mixed producer done");
    exit((seed & 0x7fff) as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    println!("[t2l4-user] scenario=mixed");

    let cpu_specs = [
        CpuArg {
            spin_rounds: 5_000,
            tick_count: 7,
            seed: 101,
        },
        CpuArg {
            spin_rounds: 2_800,
            tick_count: 7,
            seed: 202,
        },
    ];
    for spec in cpu_specs {
        let leaked = Box::leak(Box::new(spec));
        thread_create(cpu_worker as *const () as usize, leaked as *const _ as usize);
    }

    for idx in 0..INTERACTIVE_WORKERS {
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
