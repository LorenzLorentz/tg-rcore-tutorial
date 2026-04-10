#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::format;
use core::sync::atomic::{AtomicBool, Ordering};
use user_lib::{
    TicketSpinLock, busy_spin, exit, print_user_bug, sleep, thread_create, waittid,
};

static LOCK: TicketSpinLock = TicketSpinLock::new();
static ENTERED_CRITICAL: AtomicBool = AtomicBool::new(false);

unsafe fn worker() -> isize {
    let _ = LOCK.lock();
    ENTERED_CRITICAL.store(true, Ordering::Relaxed);
    busy_spin(10_000, 1);
    LOCK.unlock();
    exit(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let _ = LOCK.lock();
    println!("[t2l5-control] main thread keeps spinlock forever");
    let tid = thread_create(worker as *const () as usize, 0) as usize;
    sleep(50);
    if !ENTERED_CRITICAL.load(Ordering::Relaxed) {
        print_user_bug(
            "heuristic",
            "stuck_spin",
            "spinlock",
            &format!("worker_tid={} main_holds_lock=1", tid),
        );
        LOCK.unlock();
        waittid(tid);
        return 0;
    }
    LOCK.unlock();
    waittid(tid);
    println!("broken spinlock unexpectedly finished");
    1
}
