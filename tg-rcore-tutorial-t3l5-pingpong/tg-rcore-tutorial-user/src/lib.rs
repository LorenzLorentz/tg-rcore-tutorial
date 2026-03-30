#![no_std]
//!
//! 教程阅读建议：
//!
//! - `_start` 展示用户程序最小运行时（初始化控制台、堆、调用 main）；
//! - 其余辅助函数（sleep/pipe_*）展示了常见 syscall 组合用法。

mod heap;

extern crate alloc;

use tg_console::log;

pub use tg_console::{print, println};
pub use tg_syscall::*;

/// pingpong host 角色。
pub const PINGPONG_HOST: usize = 0;
/// pingpong 左侧玩家角色。
pub const PINGPONG_LEFT: usize = 1;
/// pingpong 右侧玩家角色。
pub const PINGPONG_RIGHT: usize = 2;

/// pingpong lobby 阶段：等待双方玩家就位。
pub const PINGPONG_LOBBY: isize = 0;
/// pingpong running 阶段：正常游戏中。
pub const PINGPONG_RUNNING: isize = 1;
/// pingpong game-over 阶段：等待重开或退出。
pub const PINGPONG_GAME_OVER: isize = 2;
/// pingpong quit 阶段：当前会话结束。
pub const PINGPONG_QUIT: isize = 3;

const SYSCALL_PINGPONG_ATTACH: SyscallId = SyscallId(0x500);
const SYSCALL_PINGPONG_TICK: SyscallId = SyscallId(0x501);

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    // 用户态运行时初始化顺序与内核类似：先 I/O，再堆，再进入 main。
    tg_console::init_console(&Console);
    tg_console::set_log_level(option_env!("LOG"));
    heap::init();

    unsafe extern "C" {
        fn main() -> i32;
    }

    // SAFETY: main 函数由用户程序提供，链接器保证其存在且符合 C ABI
    exit(unsafe { main() });
    unreachable!()
}

#[panic_handler]
fn panic_handler(panic_info: &core::panic::PanicInfo) -> ! {
    let err = panic_info.message();
    if let Some(location) = panic_info.location() {
        log::error!("Panicked at {}:{}, {err}", location.file(), location.line());
    } else {
        log::error!("Panicked: {err}");
    }
    exit(1);
    unreachable!()
}

pub fn getchar() -> u8 {
    let mut c = [0u8; 1];
    read(STDIN, &mut c);
    c[0]
}

struct Console;

impl tg_console::Console for Console {
    #[inline]
    fn put_char(&self, c: u8) {
        tg_syscall::write(STDOUT, &[c]);
    }

    #[inline]
    fn put_str(&self, s: &str) {
        tg_syscall::write(STDOUT, s.as_bytes());
    }
}

pub fn sleep(period_ms: usize) {
    // 轮询时钟 + 主动让出 CPU 的教学实现，便于理解 time/yield 系统调用协作。
    let mut time: TimeSpec = TimeSpec::ZERO;
    clock_gettime(ClockId::CLOCK_MONOTONIC, &mut time as *mut _ as _);
    let time = time + TimeSpec::from_millsecond(period_ms);
    loop {
        let mut now: TimeSpec = TimeSpec::ZERO;
        clock_gettime(ClockId::CLOCK_MONOTONIC, &mut now as *mut _ as _);
        if now > time {
            break;
        }
        sched_yield();
    }
}

pub fn get_time() -> isize {
    let mut time: TimeSpec = TimeSpec::ZERO;
    clock_gettime(ClockId::CLOCK_MONOTONIC, &mut time as *mut _ as _);
    (time.tv_sec * 1000 + time.tv_nsec / 1_000_000) as isize
}

pub fn trace_read(ptr: *const u8) -> Option<u8> {
    let ret = trace(0, ptr as usize, 0);
    if ret >= 0 && ret <= 255 {
        Some(ret as u8)
    } else {
        None
    }
}

pub fn trace_write(ptr: *const u8, value: u8) -> isize {
    trace(1, ptr as usize, value as usize)
}

pub fn count_syscall(syscall_id: usize) -> isize {
    trace(2, syscall_id, 0)
}

/// 注册当前进程为 pingpong host。
pub fn pingpong_attach_host() -> isize {
    unsafe { tg_syscall::native::syscall1(SYSCALL_PINGPONG_ATTACH, PINGPONG_HOST) }
}

/// 注册当前进程为 pingpong 玩家，并绑定上下移动按键。
pub fn pingpong_attach_player(role: usize, up_key: u8, down_key: u8) -> isize {
    unsafe {
        tg_syscall::native::syscall3(
            SYSCALL_PINGPONG_ATTACH,
            role,
            up_key as usize,
            down_key as usize,
        )
    }
}

/// 推进 host 的一帧逻辑并返回当前游戏阶段。
pub fn pingpong_tick_host() -> isize {
    unsafe { tg_syscall::native::syscall1(SYSCALL_PINGPONG_TICK, PINGPONG_HOST) }
}

/// 推进指定玩家的一次输入处理并返回当前游戏阶段。
pub fn pingpong_tick_player(role: usize) -> isize {
    unsafe { tg_syscall::native::syscall1(SYSCALL_PINGPONG_TICK, role) }
}

/// 从管道读取数据
/// 返回实际读取的总字节数，负数表示错误
pub fn pipe_read(pipe_fd: usize, buffer: &mut [u8]) -> isize {
    let mut total_read = 0usize;
    let len = buffer.len();
    loop {
        if total_read >= len {
            return total_read as isize;
        }
        let ret = read(pipe_fd, &mut buffer[total_read..]);
        if ret == -2 {
            // 暂时无数据，让出 CPU 后重试
            sched_yield();
            continue;
        } else if ret == 0 {
            // EOF，写端关闭
            return total_read as isize;
        } else if ret < 0 {
            // 其他错误
            return ret;
        } else {
            total_read += ret as usize;
        }
    }
}

/// 向管道写入数据
/// 返回实际写入的总字节数，负数表示错误
pub fn pipe_write(pipe_fd: usize, buffer: &[u8]) -> isize {
    let mut total_write = 0usize;
    let len = buffer.len();
    loop {
        if total_write >= len {
            return total_write as isize;
        }
        let ret = write(pipe_fd, &buffer[total_write..]);
        if ret == -2 {
            // 缓冲区满，让出 CPU 后重试
            sched_yield();
            continue;
        } else if ret < 0 {
            // 其他错误
            return ret;
        } else {
            total_write += ret as usize;
        }
    }
}
