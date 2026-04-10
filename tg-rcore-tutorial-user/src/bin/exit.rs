#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::shutdown_system;

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    println!("shutdown requested by exit app");
    shutdown_system()
}
