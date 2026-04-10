#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{enable_deadlock_detect, mutex_create, mutex_lock, mutex_unlock};

const MUTEX_ID: usize = 0;

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    enable_deadlock_detect(true);
    assert_eq!(mutex_create(true), MUTEX_ID as isize);
    assert_eq!(mutex_lock(MUTEX_ID), 0);
    let ret = mutex_lock(MUTEX_ID);
    if ret == -0xdead {
        println!("mutex self-deadlock exposed as expected");
        mutex_unlock(MUTEX_ID);
        return 0;
    }
    println!("mutex self-deadlock unexpectedly returned {}", ret);
    1
}
