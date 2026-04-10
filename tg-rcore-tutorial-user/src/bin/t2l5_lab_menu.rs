#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::exec;

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    println!("t2l5 sync bug lab");
    println!();
    println!("correct demos:");
    println!("  t2l5_spin_ticket");
    println!("  t2l5_mutex_stress");
    println!("  t2l5_semaphore_pc");
    println!("  t2l5_condvar_pc");
    println!("  t2l5_rwlock_fair");
    println!("  t2l5_phil_mutex");
    println!("  exit                (shutdown qemu)");
    println!();
    println!("bug demos:");
    println!("  t2l5_bug_deadlock_mutex   (exact)");
    println!("  t2l5_spin_broken          (heuristic)");
    println!("  t2l5_condvar_if_bug       (exact, user misuse)");
    println!("  t2l5_rwlock_reader_pref   (statistical)");
    println!();
    println!("type a program name and press enter:");
    exec("user_shell");
    0
}
