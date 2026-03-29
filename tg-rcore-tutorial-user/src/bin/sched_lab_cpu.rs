#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::boxed::Box;
use user_lib::{busy_spin, exit, lab_tick, thread_create};

struct CpuArg {
    spin_rounds: usize,
    tick_count: usize,
    seed: usize,
}

fn cpu_worker(arg: *const CpuArg) -> isize {
    let arg = unsafe { &*arg };
    let mut seed = arg.seed;
    for _ in 0..arg.tick_count {
        seed = busy_spin(arg.spin_rounds, seed);
        lab_tick();
    }
    println!("[t2l4-user] cpu worker seed={} done", arg.seed);
    exit((seed & 0x7fff) as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    println!("[t2l4-user] scenario=cpu");
    let specs = [
        CpuArg {
            spin_rounds: 6_000,
            tick_count: 6,
            seed: 1,
        },
        CpuArg {
            spin_rounds: 4_500,
            tick_count: 6,
            seed: 2,
        },
        CpuArg {
            spin_rounds: 3_200,
            tick_count: 6,
            seed: 3,
        },
        CpuArg {
            spin_rounds: 2_200,
            tick_count: 6,
            seed: 4,
        },
        CpuArg {
            spin_rounds: 1_200,
            tick_count: 6,
            seed: 5,
        },
    ];

    for spec in specs {
        let leaked = Box::leak(Box::new(spec));
        thread_create(cpu_worker as *const () as usize, leaked as *const _ as usize);
    }
    0
}
