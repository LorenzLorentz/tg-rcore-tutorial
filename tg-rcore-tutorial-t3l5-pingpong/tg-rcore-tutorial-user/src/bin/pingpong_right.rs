#![no_std]
#![no_main]

extern crate user_lib;

use user_lib::{
    PINGPONG_QUIT, PINGPONG_RIGHT, PINGPONG_RUNNING, pingpong_attach_player,
    pingpong_tick_player, sleep,
};

#[unsafe(no_mangle)]
extern "C" fn main() -> i32 {
    if pingpong_attach_player(PINGPONG_RIGHT, b'o', b'l') < 0 {
        return 1;
    }

    loop {
        let phase = pingpong_tick_player(PINGPONG_RIGHT);
        if phase == PINGPONG_QUIT {
            break;
        }
        sleep(if phase == PINGPONG_RUNNING { 8 } else { 16 });
    }
    0
}
