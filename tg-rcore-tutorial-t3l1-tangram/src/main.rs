//! ch1 tangram application.
//!
//! This crate keeps the chapter-1 bare-metal startup path, but replaces
//! the serial `Hello, world!` demo with a VirtIO-GPU framebuffer renderer
//! that draws a tangram-inspired `OS` logo.

// 不使用标准库，因为裸机环境没有操作系统提供系统调用支持
#![no_std]
// 不使用标准入口，因为裸机环境没有 C runtime 进行初始化
#![no_main]
// RISC-V64 架构下启用严格警告和文档检查
#![cfg_attr(target_arch = "riscv64", deny(warnings, missing_docs))]
// 非 RISC-V64 架构允许死代码（用于 cargo publish --dry-run 在主机上通过编译）
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};
use tg_rcore_tutorial_gfx::{Color, Display, Point, VIRTIO_GPU_MMIO_BASE};
use tg_sbi::{console_putchar, shutdown};

const HEAP_BYTES: usize = 512 * 1024;

#[repr(align(4096))]
struct AlignedHeap([u8; HEAP_BYTES]);

struct HeapSpace(UnsafeCell<AlignedHeap>);

unsafe impl Sync for HeapSpace {}

static HEAP_SPACE: HeapSpace = HeapSpace(UnsafeCell::new(AlignedHeap([0; HEAP_BYTES])));

struct BumpAllocator {
    next: AtomicUsize,
}

impl BumpAllocator {
    const fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
        }
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator::new();

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(1);
        let size = layout.size();
        let mut current = self.next.load(Ordering::Relaxed);
        loop {
            let aligned = (current + align - 1) & !(align - 1);
            let next = aligned.saturating_add(size);
            if next > HEAP_BYTES {
                return core::ptr::null_mut();
            }
            match self
                .next
                .compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => {
                    let base = unsafe { (*HEAP_SPACE.0.get()).0.as_mut_ptr() as usize };
                    return (base + aligned) as *mut u8;
                }
                Err(observed) => current = observed,
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[derive(Copy, Clone)]
struct Piece {
    color: Color,
    points: [Point; 4],
    len: usize,
}

const fn tri(color: Color, a: Point, b: Point, c: Point) -> Piece {
    Piece {
        color,
        points: [a, b, c, Point::new(0, 0)],
        len: 3,
    }
}

const fn quad(color: Color, a: Point, b: Point, c: Point, d: Point) -> Piece {
    Piece {
        color,
        points: [a, b, c, d],
        len: 4,
    }
}

const BG: Color = Color::rgb(14, 22, 36);
const PANEL: Color = Color::rgb(26, 38, 58);
const SHADOW: Color = Color::rgb(8, 12, 20);

const O_PIECES: [Piece; 7] = [
    quad(
        Color::rgb(245, 94, 69),
        Point::new(110, 170),
        Point::new(225, 80),
        Point::new(310, 165),
        Point::new(225, 245),
    ),
    quad(
        Color::rgb(254, 156, 78),
        Point::new(310, 165),
        Point::new(395, 80),
        Point::new(510, 170),
        Point::new(395, 250),
    ),
    quad(
        Color::rgb(255, 210, 94),
        Point::new(110, 170),
        Point::new(225, 245),
        Point::new(225, 355),
        Point::new(110, 430),
    ),
    quad(
        Color::rgb(94, 197, 133),
        Point::new(510, 170),
        Point::new(395, 250),
        Point::new(395, 355),
        Point::new(510, 430),
    ),
    quad(
        Color::rgb(82, 180, 255),
        Point::new(110, 430),
        Point::new(225, 355),
        Point::new(310, 440),
        Point::new(225, 520),
    ),
    quad(
        Color::rgb(101, 126, 255),
        Point::new(310, 440),
        Point::new(395, 355),
        Point::new(510, 430),
        Point::new(395, 520),
    ),
    quad(
        Color::rgb(206, 118, 255),
        Point::new(225, 245),
        Point::new(310, 165),
        Point::new(395, 250),
        Point::new(310, 335),
    ),
];

const S_PIECES: [Piece; 7] = [
    quad(
        Color::rgb(255, 132, 118),
        Point::new(575, 110),
        Point::new(805, 110),
        Point::new(760, 180),
        Point::new(620, 180),
    ),
    tri(
        Color::rgb(255, 179, 82),
        Point::new(575, 110),
        Point::new(620, 180),
        Point::new(520, 250),
    ),
    quad(
        Color::rgb(255, 221, 94),
        Point::new(620, 180),
        Point::new(760, 180),
        Point::new(825, 255),
        Point::new(690, 305),
    ),
    quad(
        Color::rgb(86, 191, 150),
        Point::new(565, 250),
        Point::new(690, 250),
        Point::new(750, 330),
        Point::new(625, 330),
    ),
    quad(
        Color::rgb(84, 173, 255),
        Point::new(625, 330),
        Point::new(750, 330),
        Point::new(695, 410),
        Point::new(565, 410),
    ),
    tri(
        Color::rgb(105, 129, 255),
        Point::new(695, 410),
        Point::new(815, 355),
        Point::new(755, 480),
    ),
    quad(
        Color::rgb(206, 118, 255),
        Point::new(525, 430),
        Point::new(755, 430),
        Point::new(700, 500),
        Point::new(570, 500),
    ),
];

fn put_str(s: &str) {
    for byte in s.bytes() {
        console_putchar(byte);
    }
}

fn project(p: Point, origin: Point, scale_num: i32, scale_den: i32) -> Point {
    Point::new(
        origin.x + p.x * scale_num / scale_den,
        origin.y + p.y * scale_num / scale_den,
    )
}

fn draw_piece(
    canvas: &mut tg_rcore_tutorial_gfx::Canvas<'_>,
    piece: &Piece,
    origin: Point,
    scale_num: i32,
    scale_den: i32,
    shadow_offset: Point,
    shadow: bool,
) {
    let mut points = [Point::new(0, 0); 4];
    for (idx, point) in piece.points[..piece.len].iter().enumerate() {
        let mut mapped = project(*point, origin, scale_num, scale_den);
        if shadow {
            mapped.x += shadow_offset.x;
            mapped.y += shadow_offset.y;
        }
        points[idx] = mapped;
    }
    let color = if shadow { SHADOW } else { piece.color };
    canvas.fill_convex_polygon(&points[..piece.len], color);
}

fn draw_scene(display: &mut Display) {
    let width = display.width() as i32;
    let height = display.height() as i32;
    let scale_num = core::cmp::min(width / 1000, height / 620).max(1);
    let scale_den = 1;
    let scene_w = 1000 * scale_num / scale_den;
    let scene_h = 620 * scale_num / scale_den;
    let origin = Point::new((width - scene_w) / 2, (height - scene_h) / 2);
    let shadow_offset = Point::new(scale_num * 8, scale_num * 8);

    let mut canvas = display.canvas();
    canvas.clear(BG);
    canvas.fill_rect(
        Point::new(origin.x - 30 * scale_num, origin.y - 30 * scale_num),
        scene_w + 60 * scale_num,
        scene_h + 60 * scale_num,
        PANEL,
    );

    for piece in O_PIECES.iter().chain(S_PIECES.iter()) {
        draw_piece(
            &mut canvas,
            piece,
            origin,
            scale_num,
            scale_den,
            shadow_offset,
            true,
        );
    }
    for piece in O_PIECES.iter().chain(S_PIECES.iter()) {
        draw_piece(
            &mut canvas,
            piece,
            origin,
            scale_num,
            scale_den,
            shadow_offset,
            false,
        );
    }

    let hole = [
        project(Point::new(255, 255), origin, scale_num, scale_den),
        project(Point::new(310, 205), origin, scale_num, scale_den),
        project(Point::new(365, 255), origin, scale_num, scale_den),
        project(Point::new(365, 345), origin, scale_num, scale_den),
        project(Point::new(310, 395), origin, scale_num, scale_den),
        project(Point::new(255, 345), origin, scale_num, scale_den),
    ];
    canvas.fill_convex_polygon(&hole, PANEL);
}

/// S 态程序入口点。
///
/// 这是一个裸函数（naked function），放置在 `.text.entry` 段，
/// 链接脚本将其安排在地址 `0x80200000`。
///
/// 裸函数不生成函数序言和尾声，因此可以在没有栈的情况下执行。
/// 它完成两件事：
/// 1. 设置栈指针 `sp`，指向栈顶（栈从高地址向低地址增长）
/// 2. 跳转到 Rust 主函数 `rust_main`
#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn _start() -> ! {
    // 栈大小：4 KiB
    const STACK_SIZE: usize = 4096;

    // 在 .bss.uninit 段中分配栈空间
    #[unsafe(link_section = ".bss.uninit")]
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::naked_asm!(
        "la sp, {stack} + {stack_size}", // 将 sp 设置为栈顶地址
        "j  {main}",                      // 跳转到 rust_main
        stack_size = const STACK_SIZE,
        stack      =   sym STACK,
        main       =   sym rust_main,
    )
}

extern "C" fn rust_main() -> ! {
    put_str("tg tangram: init gpu\n");
    let mut display = match Display::new(VIRTIO_GPU_MMIO_BASE) {
        Ok(display) => display,
        Err(_) => panic!("failed to initialize VirtIO-GPU"),
    };
    draw_scene(&mut display);
    if display.present().is_err() {
        panic!("failed to flush framebuffer");
    }
    put_str("tg tangram: frame flushed\n");
    loop {
        core::hint::spin_loop();
    }
}

/// panic 处理函数。
///
/// `#![no_std]` 环境下必须自行实现。发生 panic 时以异常状态关机。
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    shutdown(true) // true 表示异常关机
}

/// 非 RISC-V64 架构的占位模块。
///
/// 提供 `main` 等符号，使得在主机平台（如 x86_64）上也能通过编译，
/// 满足 `cargo publish --dry-run` 和 `cargo test` 的需求。
#[cfg(not(target_arch = "riscv64"))]
mod stub {
    /// 主机平台占位入口
    #[unsafe(no_mangle)]
    pub extern "C" fn main() -> i32 {
        0
    }

    /// C 运行时占位
    #[unsafe(no_mangle)]
    pub extern "C" fn __libc_start_main() -> i32 {
        0
    }

    /// Rust 异常处理人格占位
    #[unsafe(no_mangle)]
    pub extern "C" fn rust_eh_personality() {}
}
