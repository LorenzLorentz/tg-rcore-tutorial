#![no_std]
#![no_main]
#![allow(clippy::needless_range_loop)]

//! 教学目标：
//! 两个线程各自独立完成矩阵乘法，与单线程串行执行对比，
//! 体现多核并行加速效果。

#[macro_use]
extern crate user_lib;
extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use user_lib::{exit, get_time, sched_yield, thread_create, waittid};

const N: usize = 32;
const P: i32 = 10007;
const ITERS: usize = 20;

type Mat = [[i32; N]; N];

static READY: AtomicUsize = AtomicUsize::new(0);
static START: AtomicUsize = AtomicUsize::new(0);

/// 每个线程的计算结果（校验和）
static CHECKSUM: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

/// 初始化矩阵：a[i][j] = (i * N + j + seed) % P
fn init_matrix(mat: &mut Mat, seed: usize) {
    for i in 0..N {
        for j in 0..N {
            mat[i][j] = ((i * N + j + seed) % (P as usize)) as i32;
        }
    }
}

/// 矩阵乘法 c = a * b (mod P)
fn mat_mul(a: &Mat, b: &Mat, c: &mut Mat) {
    for i in 0..N {
        for j in 0..N {
            let mut sum: i32 = 0;
            for k in 0..N {
                sum = (sum + (a[i][k] as i64 * b[k][j] as i64 % P as i64) as i32) % P;
            }
            c[i][j] = sum;
        }
    }
}

/// 计算矩阵所有元素的校验和
fn checksum(mat: &Mat) -> usize {
    let mut s: i64 = 0;
    for i in 0..N {
        for j in 0..N {
            s += mat[i][j] as i64;
        }
    }
    s as usize
}

/// 线程工作函数：反复进行矩阵乘法
unsafe fn worker(arg: *const usize) -> isize {
    let id = *arg;

    READY.fetch_add(1, Ordering::SeqCst);
    while START.load(Ordering::SeqCst) == 0 {
        sched_yield();
    }

    let mut a: Mat = [[0; N]; N];
    let mut b: Mat = [[0; N]; N];
    let mut c: Mat = [[0; N]; N];

    // 不同线程使用不同种子，确保各自独立计算
    init_matrix(&mut a, id * 100 + 1);
    init_matrix(&mut b, id * 100 + 2);

    for _ in 0..ITERS {
        mat_mul(&a, &b, &mut c);
        // 把结果作为下一轮输入
        for i in 0..N {
            for j in 0..N {
                a[i][j] = c[i][j];
            }
        }
    }

    let cs = checksum(&c);
    CHECKSUM[id].store(cs, Ordering::SeqCst);

    exit(0)
}

/// 串行执行相同工作量（两份矩阵乘法依次完成）
fn serial_work() -> (usize, usize) {
    let mut results = [0usize; 2];
    for id in 0..2 {
        let mut a: Mat = [[0; N]; N];
        let mut b: Mat = [[0; N]; N];
        let mut c: Mat = [[0; N]; N];

        init_matrix(&mut a, id * 100 + 1);
        init_matrix(&mut b, id * 100 + 2);

        for _ in 0..ITERS {
            mat_mul(&a, &b, &mut c);
            for i in 0..N {
                for j in 0..N {
                    a[i][j] = c[i][j];
                }
            }
        }
        results[id] = checksum(&c);
    }
    (results[0], results[1])
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    println!("=== t4l45 matrix smp test ===");
    println!("matrix size: {}x{}, iterations: {}, modulo: {}", N, N, ITERS, P);

    // ---- 阶段 1：串行基线 ----
    let t0 = get_time();
    let (serial_cs0, serial_cs1) = serial_work();
    let serial_ms = get_time() - t0;
    println!("serial: {}ms (checksum0={}, checksum1={})", serial_ms, serial_cs0, serial_cs1);

    // ---- 阶段 2：双线程并行 ----
    READY.store(0, Ordering::SeqCst);
    START.store(0, Ordering::SeqCst);

    let ids: Vec<usize> = (0..2).collect();
    let mut tids = Vec::new();
    for id in &ids {
        tids.push(thread_create(worker as *const () as usize, id as *const _ as usize) as usize);
    }

    // 等待两个线程就绪后同时放行
    while READY.load(Ordering::SeqCst) != 2 {
        sched_yield();
    }
    let t1 = get_time();
    START.store(1, Ordering::SeqCst);

    for tid in tids {
        waittid(tid);
    }
    let parallel_ms = get_time() - t1;

    let par_cs0 = CHECKSUM[0].load(Ordering::SeqCst);
    let par_cs1 = CHECKSUM[1].load(Ordering::SeqCst);
    println!(
        "parallel: {}ms (checksum0={}, checksum1={})",
        parallel_ms, par_cs0, par_cs1
    );

    // ---- 校验：并行结果必须与串行一致 ----
    assert_eq!(
        par_cs0, serial_cs0,
        "thread 0 checksum mismatch: parallel={} serial={}",
        par_cs0, serial_cs0
    );
    assert_eq!(
        par_cs1, serial_cs1,
        "thread 1 checksum mismatch: parallel={} serial={}",
        par_cs1, serial_cs1
    );

    // ---- 输出摘要 ----
    let speedup_x10 = if parallel_ms > 0 {
        serial_ms * 10 / parallel_ms
    } else {
        0
    };
    println!(
        "[t4l45-summary] case=matrix_smp threads=2 matrix={}x{} iters={} serial_ms={} parallel_ms={} speedup={}.{}x",
        N, N, ITERS, serial_ms, parallel_ms, speedup_x10 / 10, speedup_x10 % 10
    );

    if parallel_ms < serial_ms {
        println!("multi-core acceleration observed!");
    } else {
        println!("no speedup (might be single-core or scheduling overhead)");
    }

    println!("t4l45 matrix smp test passed!");
    0
}
