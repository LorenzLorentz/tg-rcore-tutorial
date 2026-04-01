#![no_std]
#![no_main]

extern crate user_lib;

use user_lib::{
    PINGPONG_GAME_OVER, PINGPONG_LOBBY, PINGPONG_QUIT, PINGPONG_RUNNING,
    pingpong_attach_host, pingpong_tick_host, println, sleep, spawn, waitpid,
};

#[unsafe(no_mangle)]
extern "C" fn main() -> i32 {
    println!("PingPong controls: left=W/S right=O/L restart=R quit=Q");
    println!("PingPong window input: click the QEMU window before playing");
    if pingpong_attach_host() < 0 {
        println!("pingpong host attach failed");
        return 1;
    }

    let left_pid = spawn("pingpong_left");
    let right_pid = spawn("pingpong_right");
    if left_pid < 0 || right_pid < 0 {
        println!("failed to spawn pingpong players");
        return 1;
    }

    let mut last_phase = -1;
    loop {
        let phase = pingpong_tick_host();
        if phase != last_phase {
            match phase {
                PINGPONG_LOBBY => println!("waiting for both players..."),
                PINGPONG_RUNNING => println!("game on"),
                PINGPONG_GAME_OVER => println!("game over: press R to replay or Q to quit"),
                PINGPONG_QUIT => println!("leaving pingpong"),
                _ => {}
            }
            last_phase = phase;
        }

        if phase == PINGPONG_QUIT {
            break;
        }

        sleep(if phase == PINGPONG_RUNNING { 16 } else { 24 });
    }

    let mut exit_code = 0;
    waitpid(left_pid, &mut exit_code);
    waitpid(right_pid, &mut exit_code);
    0
}
