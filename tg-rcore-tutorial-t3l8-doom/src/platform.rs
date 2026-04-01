use crate::{KERNEL_SPACE, Sv39, build_flags};
use alloc::alloc::{Layout, alloc_zeroed, dealloc};
use core::ptr::NonNull;
use spin::Mutex;
use tg_console::log;
use tg_gfx::Display;
use tg_kernel_vm::page_table::{MmuMeta, VAddr, VmFlags};
use virtio_drivers::{Hal, MmioTransport, VirtIOHeader, VirtIOInput};

pub(crate) const VIRTIO_GPU_MMIO_BASE: usize = 0x1000_2000;
pub(crate) const VIRTIO_INPUT_MMIO_BASE: usize = 0x1000_3000;
/// task3 Doom 使用的 framebuffer 信息查询 syscall 号。
pub(crate) const FRAMEBUFFER_GETINFO_SYSCALL: usize = 5000;
/// task3 Doom 使用的 framebuffer present syscall 号。
pub(crate) const FRAMEBUFFER_PRESENT_SYSCALL: usize = 5001;
/// task3 Doom 使用的输入轮询 syscall 号。
pub(crate) const INPUT_POLL_SYSCALL: usize = 5002;
/// BGRA8888 帧缓冲格式常量。
pub(crate) const FRAMEBUFFER_FORMAT_BGRA8888: u32 = 1;

/// 用户态可见的帧缓冲信息结构。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct FramebufferInfo {
    /// 帧缓冲宽度。
    pub width: u32,
    /// 帧缓冲高度。
    pub height: u32,
    /// 每行像素数。
    pub stride: u32,
    /// 像素格式。
    pub format: u32,
}

/// 用户态可见的输入事件结构。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct InputEvent {
    /// 事件类型。
    pub event_type: u16,
    /// 按键码。
    pub code: u16,
    /// 事件值。
    pub value: u32,
}

static DISPLAY: Mutex<Option<Display>> = Mutex::new(None);
static INPUT: Mutex<Option<Keyboard>> = Mutex::new(None);

struct Keyboard {
    inner: VirtIOInput<PlatformHal, MmioTransport>,
}

unsafe impl Send for Keyboard {}
unsafe impl Sync for Keyboard {}

struct PlatformHal;

impl Hal for PlatformHal {
    fn dma_alloc(pages: usize) -> usize {
        unsafe {
            alloc_zeroed(Layout::from_size_align_unchecked(
                pages << Sv39::PAGE_BITS,
                1 << Sv39::PAGE_BITS,
            )) as _
        }
    }

    fn dma_dealloc(paddr: usize, pages: usize) -> i32 {
        unsafe {
            dealloc(
                paddr as _,
                Layout::from_size_align_unchecked(pages << Sv39::PAGE_BITS, 1 << Sv39::PAGE_BITS),
            )
        }
        0
    }

    fn phys_to_virt(paddr: usize) -> usize {
        paddr
    }

    fn virt_to_phys(vaddr: usize) -> usize {
        const VALID: VmFlags<Sv39> = build_flags("__V");
        let ptr = unsafe {
            KERNEL_SPACE
                .assume_init_ref()
                .translate::<u8>(VAddr::new(vaddr), VALID)
                .unwrap()
        };
        ptr.as_ptr() as usize
    }
}

pub(crate) fn init() {
    init_display();
    init_input();
}

pub(crate) fn framebuffer_info() -> Option<FramebufferInfo> {
    let display = DISPLAY.lock();
    display.as_ref().map(|display| FramebufferInfo {
        width: display.width() as u32,
        height: display.height() as u32,
        stride: display.width() as u32,
        format: FRAMEBUFFER_FORMAT_BGRA8888,
    })
}

pub(crate) fn present_bgra8888(pixels: &[u32], width: usize, height: usize) -> isize {
    let mut display = DISPLAY.lock();
    match display.as_mut() {
        Some(display) => display
            .present_bgra8888(pixels, width, height)
            .map(|_| 0)
            .unwrap_or(-1),
        None => -1,
    }
}

pub(crate) fn poll_input() -> Option<InputEvent> {
    let mut input = INPUT.lock();
    input.as_mut().and_then(|keyboard| {
        keyboard.inner.ack_interrupt();
        keyboard.inner.pop_pending_event().map(|event| InputEvent {
            event_type: event.event_type,
            code: event.code,
            value: event.value,
        })
    })
}

fn init_display() {
    match Display::new(VIRTIO_GPU_MMIO_BASE) {
        Ok(display) => {
            log::info!(
                "platform: gpu ready at {:#x} ({}x{})",
                VIRTIO_GPU_MMIO_BASE,
                display.width(),
                display.height()
            );
            *DISPLAY.lock() = Some(display);
        }
        Err(err) => {
            log::warn!("platform: gpu init failed: {:?}", err);
        }
    }
}

fn init_input() {
    let header = match NonNull::new(VIRTIO_INPUT_MMIO_BASE as *mut VirtIOHeader) {
        Some(header) => header,
        None => return,
    };
    let transport = match unsafe { MmioTransport::new(header) } {
        Ok(transport) => transport,
        Err(err) => {
            log::warn!("platform: keyboard transport init failed: {:?}", err);
            return;
        }
    };
    match VirtIOInput::<PlatformHal, _>::new(transport) {
        Ok(input) => {
            log::info!("platform: keyboard ready at {:#x}", VIRTIO_INPUT_MMIO_BASE);
            *INPUT.lock() = Some(Keyboard { inner: input });
        }
        Err(err) => {
            log::warn!("platform: keyboard init failed: {:?}", err);
        }
    }
}
