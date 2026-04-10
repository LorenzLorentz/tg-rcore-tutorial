#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::format;
use user_lib::{
    condvar_create, condvar_signal, condvar_wait, exit, mutex_create, mutex_lock, mutex_unlock,
    print_user_bug, sleep, thread_create, waittid,
};

const MUTEX_ID: usize = 0;
const CONDVAR_ID: usize = 0;

static mut READY_ITEMS: usize = 0;

unsafe fn producer() -> isize {
    sleep(20);
    mutex_lock(MUTEX_ID);
    READY_ITEMS = 1;
    condvar_signal(CONDVAR_ID);
    mutex_unlock(MUTEX_ID);

    sleep(20);
    mutex_lock(MUTEX_ID);
    condvar_signal(CONDVAR_ID);
    mutex_unlock(MUTEX_ID);
    exit(0)
}

unsafe fn consumer(id: usize) -> isize {
    mutex_lock(MUTEX_ID);
    if (&raw const READY_ITEMS).read_volatile() == 0 {
        condvar_wait(CONDVAR_ID, MUTEX_ID);
    }
    let ready = (&raw const READY_ITEMS).read_volatile();
    println!("[t2l5-control] consumer={} ready={}", id, ready);
    assert!(ready > 0, "condvar wait written with if instead of while");
    (&raw mut READY_ITEMS).write_volatile(ready - 1);
    mutex_unlock(MUTEX_ID);
    exit(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    assert_eq!(mutex_create(true), MUTEX_ID as isize);
    assert_eq!(condvar_create(), CONDVAR_ID as isize);

    let tids = [
        thread_create(consumer as *const () as usize, 0) as usize,
        thread_create(consumer as *const () as usize, 1) as usize,
        thread_create(producer as *const () as usize, 0) as usize,
    ];
    let mut failed = 0;
    for tid in tids {
        if waittid(tid) != 0 {
            failed += 1;
        }
    }
    println!("[t2l5-control] condvar if-bug failed_threads={failed}");
    if failed > 0 {
        print_user_bug(
            "exact",
            "condvar_if_misuse",
            "condvar",
            &format!("failed_threads={}", failed),
        );
        0
    } else {
        println!("condvar if-bug unexpectedly passed");
        1
    }
}
