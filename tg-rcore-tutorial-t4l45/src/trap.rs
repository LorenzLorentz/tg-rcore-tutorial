use crate::processor::{MAX_HARTS, current_hart};
use core::sync::atomic::{AtomicUsize, Ordering};

struct HartTrapState {
    pending_timer_ticks: AtomicUsize,
}

impl HartTrapState {
    const fn new() -> Self {
        Self {
            pending_timer_ticks: AtomicUsize::new(0),
        }
    }
}

static HART_TRAP_STATE: [HartTrapState; MAX_HARTS] = [const { HartTrapState::new() }; MAX_HARTS];

#[inline]
fn hart_state(hart_id: usize) -> &'static HartTrapState {
    assert!(hart_id < MAX_HARTS);
    &HART_TRAP_STATE[hart_id]
}

#[cfg(target_arch = "riscv64")]
#[inline]
fn set_kernel_trap_vector() {
    unsafe {
        core::arch::asm!(
            "csrw stvec, {}",
            in(reg) kernel_trap_entry as *const () as usize
        );
    }
}

#[cfg(not(target_arch = "riscv64"))]
#[inline]
fn set_kernel_trap_vector() {}

/// 初始化当前 hart 的常驻内核 trap 状态。
pub(crate) fn init_hart(hart_id: usize) {
    hart_state(hart_id)
        .pending_timer_ticks
        .store(0, Ordering::Release);
    set_kernel_trap_vector();
}

/// 取走当前 hart 上延迟到安全点处理的 timer ticks。
pub(crate) fn take_pending_timer_ticks(hart_id: usize) -> usize {
    hart_state(hart_id)
        .pending_timer_ticks
        .swap(0, Ordering::AcqRel)
}

#[cfg(target_arch = "riscv64")]
/// 在当前作用域内临时打开 S 态中断，并在退出时恢复原状态。
pub(crate) fn with_interrupts_enabled<R>(f: impl FnOnce() -> R) -> R {
    use riscv::register::sstatus;

    struct RestoreInterruptState {
        was_enabled: bool,
    }

    impl Drop for RestoreInterruptState {
        fn drop(&mut self) {
            if !self.was_enabled {
                unsafe {
                    sstatus::clear_sie();
                }
            }
        }
    }

    let was_enabled = sstatus::read().sie();
    if !was_enabled {
        unsafe {
            sstatus::set_sie();
        }
    }
    let restore = RestoreInterruptState { was_enabled };
    let result = f();
    drop(restore);
    result
}

#[cfg(not(target_arch = "riscv64"))]
/// 在非 RISC-V 目标上直接执行闭包。
pub(crate) fn with_interrupts_enabled<R>(f: impl FnOnce() -> R) -> R {
    f()
}

#[cfg(target_arch = "riscv64")]
#[allow(dead_code)]
#[repr(C)]
struct KernelTrapFrame {
    x: [usize; 31],
    sstatus: usize,
    sepc: usize,
    scause: usize,
    stval: usize,
    padding: usize,
}

#[cfg(target_arch = "riscv64")]
extern "C" fn kernel_trap_rust(frame: &mut KernelTrapFrame) {
    use riscv::register::scause;

    let hart_id = current_hart();
    match scause::read().cause() {
        scause::Trap::Interrupt(scause::Interrupt::SupervisorTimer) => {
            crate::arm_next_timer();
            hart_state(hart_id)
                .pending_timer_ticks
                .fetch_add(1, Ordering::AcqRel);
        }
        trap => {
            panic!(
                "unexpected kernel trap on hart {}: {:?}, scause={:#x}, sepc={:#x}, stval={:#x}, sstatus={:#x}",
                hart_id, trap, frame.scause, frame.sepc, frame.stval, frame.sstatus
            );
        }
    }
}

#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
unsafe extern "C" fn kernel_trap_entry() {
    const FRAME_SIZE: usize = 36 * core::mem::size_of::<usize>();
    core::arch::naked_asm!(
        r"  .altmacro
            .macro SAVE n
                sd x\n, (\n-1)*8(sp)
            .endm
            .macro SAVE_ALL
                sd x1, 0*8(sp)
                .set n, 3
                .rept 29
                    SAVE %n
                    .set n, n+1
                .endr
            .endm

            .macro LOAD n
                ld x\n, (\n-1)*8(sp)
            .endm
            .macro LOAD_ALL
                ld x1, 0*8(sp)
                .set n, 3
                .rept 29
                    LOAD %n
                    .set n, n+1
                .endr
            .endm
        ",
        "   .option push
            .option nopic
        ",
        "   addi sp, sp, -{frame_size}
            SAVE_ALL
            addi t0, sp, {frame_size}
            sd   t0, 1*8(sp)
            csrr t0, sstatus
            sd   t0, 31*8(sp)
            csrr t0, sepc
            sd   t0, 32*8(sp)
            csrr t0, scause
            sd   t0, 33*8(sp)
            csrr t0, stval
            sd   t0, 34*8(sp)
            mv   a0, sp
            call {kernel_trap_rust}
            ld   t0, 31*8(sp)
            csrw sstatus, t0
            ld   t0, 32*8(sp)
            csrw sepc, t0
            LOAD_ALL
            ld   sp, 1*8(sp)
            sret
        ",
        "   .option pop",
        frame_size = const FRAME_SIZE,
        kernel_trap_rust = sym kernel_trap_rust,
    )
}

#[cfg(not(target_arch = "riscv64"))]
unsafe extern "C" fn kernel_trap_entry() {
    unimplemented!("kernel_trap_entry() is only supported on riscv64")
}
