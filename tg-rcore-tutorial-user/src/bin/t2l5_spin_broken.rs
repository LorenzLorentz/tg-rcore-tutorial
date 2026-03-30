#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{TicketSpinLock, busy_spin, exit, thread_create, waittid};

static LOCK: TicketSpinLock = TicketSpinLock::new();

unsafe fn worker() -> isize {
    let _ = LOCK.lock();
    busy_spin(10_000, 1);
    LOCK.unlock();
    exit(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let _ = LOCK.lock();
    println!("[t2l5-control] main thread keeps spinlock forever");
    let tid = thread_create(worker as *const () as usize, 0) as usize;
    waittid(tid);
    println!("broken spinlock unexpectedly finished");
    0
}
