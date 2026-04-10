#![no_std]
#![no_main]

extern crate alloc;
extern crate user_lib;

use core::str;
use user_lib::{OpenFlags, close, exec, fork, open, read, wait};

// 教学目标：
// 作为用户态“第一个进程”入口，按章节配置启动对应测试集或 shell。

#[unsafe(no_mangle)]
extern "C" fn main() -> i32 {
    if option_env!("CHAPTER").unwrap_or("0") == "9" {
        let target = match option_env!("T2L4_SCENARIO").unwrap_or("mixed") {
            "cpu" => "sched_lab_cpu",
            "io" => "sched_lab_io",
            "interactive" => "sched_lab_interactive",
            "mixed" => "sched_lab_mixed",
            _ => "sched_lab_mixed",
        };
        exec(target);
        return 0;
    }

    if option_env!("CHAPTER").unwrap_or("0") == "10" {
        let default = open("t2l5_default\0", OpenFlags::RDONLY);
        if default >= 0 {
            let fd = default as usize;
            let mut buf = [0u8; 64];
            let len = read(fd, &mut buf);
            close(fd);
            if len > 0 {
                if let Ok(target) = str::from_utf8(&buf[..len as usize]) {
                    let target = target.trim_matches(|ch| ch == '\0' || ch == '\n' || ch == '\r');
                    if !target.is_empty() {
                        exec(target);
                        return 0;
                    }
                }
            }
        }
        exec("t2l5_lab_menu");
        return 0;
    }

    if option_env!("CHAPTER").unwrap_or("0") == "45" {
        let scenario = option_env!("T4L45_SCENARIO").unwrap_or("mixed");
        let target = match scenario {
            "cpu" => "sched_lab_cpu",
            "io" => "sched_lab_io",
            "interactive" => "sched_lab_interactive",
            "mixed" => "sched_lab_mixed",
            other => other,
        };
        exec(target);
        return 0;
    }

    if fork() == 0 {
        // 子进程执行实际目标程序，父进程负责兜底回收孤儿退出。
        let target = match option_env!("CHAPTER").unwrap_or("0") {
            "5" => "ch5_usertest",
            "6" => "ch6_usertest",
            "8" => "ch8_usertest",
            "-5" => "ch5b_usertest",
            "-6" => "ch6b_usertest",
            "-7" => "ch7b_usertest",
            "-8" => "ch8b_usertest",
            _ => "user_shell",
        };
        exec(target);
    } else {
        loop {
            let mut exit_code: i32 = 0;
            let pid = wait(&mut exit_code);
            if pid == -1 {
                break;
            }
        }
    }
    0
}
