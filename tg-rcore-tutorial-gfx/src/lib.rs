#![no_std]
#![cfg_attr(target_arch = "riscv64", deny(warnings, missing_docs))]
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

//! Minimal graphics component backed by VirtIO-GPU framebuffer.
//!
//! The crate intentionally exposes only a tiny drawing surface API:
//! - `Display` for GPU-backed framebuffer ownership and flushing
//! - `Canvas` for software rasterization
//! - `Point` / `Color` for geometry and color
//!
//! The public API is small, but the separation makes it reusable by later
//! tutorial chapters without tangling game logic with device setup.

#[cfg(not(target_arch = "riscv64"))]
use core::marker::PhantomData;
#[cfg(target_arch = "riscv64")]
use core::{
    cell::UnsafeCell,
    ptr::NonNull,
    slice,
    sync::atomic::{AtomicUsize, Ordering},
};
use virtio_drivers::{Error as VirtIoError, MmioError};
#[cfg(target_arch = "riscv64")]
use virtio_drivers::{Hal, MmioTransport, VirtIOGpu, VirtIOHeader};

/// Default MMIO base of `virtio-mmio-bus.0` on QEMU `virt`.
pub const VIRTIO_GPU_MMIO_BASE: usize = 0x1000_1000;

#[cfg(target_arch = "riscv64")]
const PAGE_SIZE: usize = 4096;
#[cfg(target_arch = "riscv64")]
const DMA_POOL_BYTES: usize = 8 * 1024 * 1024;

/// The result type returned by this crate.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors returned by the graphics component.
#[derive(Debug)]
pub enum Error {
    /// The target is unsupported by this crate.
    UnsupportedPlatform,
    /// VirtIO MMIO transport initialization failed.
    Mmio(MmioError),
    /// VirtIO GPU operation failed.
    VirtIo(VirtIoError),
}

impl From<MmioError> for Error {
    fn from(value: MmioError) -> Self {
        Self::Mmio(value)
    }
}

impl From<VirtIoError> for Error {
    fn from(value: VirtIoError) -> Self {
        Self::VirtIo(value)
    }
}

/// A point in screen space.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Point {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate.
    pub y: i32,
}

impl Point {
    /// Creates a point.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A BGRA8888 color stored in little-endian memory as `0xAARRGGBB`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Color(u32);

impl Color {
    /// Creates an opaque RGB color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(0xff00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    /// Creates a color from raw framebuffer bits.
    pub const fn raw(bits: u32) -> Self {
        Self(bits)
    }

    /// Returns the raw framebuffer bits.
    pub const fn bits(self) -> u32 {
        self.0
    }
}

/// A software drawing surface over a framebuffer.
pub struct Canvas<'a> {
    width: usize,
    height: usize,
    pixels: &'a mut [u32],
}

impl<'a> Canvas<'a> {
    fn new(width: usize, height: usize, pixels: &'a mut [u32]) -> Self {
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Returns the width in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Returns the height in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Fills the full framebuffer with a single color.
    pub fn clear(&mut self, color: Color) {
        self.pixels.fill(color.bits());
    }

    /// Writes a single pixel when it lies inside bounds.
    pub fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.width || y >= self.height {
            return;
        }
        self.pixels[y * self.width + x] = color.bits();
    }

    /// Fills an axis-aligned rectangle.
    pub fn fill_rect(&mut self, origin: Point, width: i32, height: i32, color: Color) {
        let x0 = origin.x.max(0).min(self.width as i32);
        let y0 = origin.y.max(0).min(self.height as i32);
        let x1 = (origin.x + width).max(0).min(self.width as i32);
        let y1 = (origin.y + height).max(0).min(self.height as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for y in y0..y1 {
            let row = y as usize * self.width;
            for x in x0..x1 {
                self.pixels[row + x as usize] = color.bits();
            }
        }
    }

    /// Draws the outline of an axis-aligned rectangle.
    pub fn stroke_rect(
        &mut self,
        origin: Point,
        width: i32,
        height: i32,
        thickness: i32,
        color: Color,
    ) {
        if width <= 0 || height <= 0 || thickness <= 0 {
            return;
        }
        self.fill_rect(origin, width, thickness, color);
        self.fill_rect(
            Point::new(origin.x, origin.y + height - thickness),
            width,
            thickness,
            color,
        );
        self.fill_rect(origin, thickness, height, color);
        self.fill_rect(
            Point::new(origin.x + width - thickness, origin.y),
            thickness,
            height,
            color,
        );
    }

    /// Fills a circle centered at `center`.
    pub fn fill_circle(&mut self, center: Point, radius: i32, color: Color) {
        if radius <= 0 {
            return;
        }
        let min_x = (center.x - radius).max(0);
        let max_x = (center.x + radius).min(self.width as i32 - 1);
        let min_y = (center.y - radius).max(0);
        let max_y = (center.y + radius).min(self.height as i32 - 1);
        let radius_sq = (radius as i64) * (radius as i64);

        for y in min_y..=max_y {
            let dy = (y - center.y) as i64;
            let row = y as usize * self.width;
            for x in min_x..=max_x {
                let dx = (x - center.x) as i64;
                if dx * dx + dy * dy <= radius_sq {
                    self.pixels[row + x as usize] = color.bits();
                }
            }
        }
    }

    /// Fills a triangle using edge functions.
    pub fn fill_triangle(&mut self, a: Point, b: Point, c: Point, color: Color) {
        let min_x = a.x.min(b.x).min(c.x).max(0);
        let max_x = a.x.max(b.x).max(c.x).min(self.width as i32 - 1);
        let min_y = a.y.min(b.y).min(c.y).max(0);
        let max_y = a.y.max(b.y).max(c.y).min(self.height as i32 - 1);
        if min_x > max_x || min_y > max_y {
            return;
        }

        let area = edge(a, b, c);
        if area == 0 {
            return;
        }

        for y in min_y..=max_y {
            let row = y as usize * self.width;
            for x in min_x..=max_x {
                let p = Point::new(x, y);
                let w0 = edge(b, c, p);
                let w1 = edge(c, a, p);
                let w2 = edge(a, b, p);
                if same_sign(area, w0) && same_sign(area, w1) && same_sign(area, w2) {
                    self.pixels[row + x as usize] = color.bits();
                }
            }
        }
    }

    /// Fills a convex polygon by fan-triangulating from the first vertex.
    pub fn fill_convex_polygon(&mut self, points: &[Point], color: Color) {
        if points.len() < 3 {
            return;
        }
        for tri in points[1..].windows(2) {
            self.fill_triangle(points[0], tri[0], tri[1], color);
        }
    }
}

fn edge(a: Point, b: Point, p: Point) -> i64 {
    (p.x - a.x) as i64 * (b.y - a.y) as i64 - (p.y - a.y) as i64 * (b.x - a.x) as i64
}

fn same_sign(area: i64, v: i64) -> bool {
    if area > 0 { v >= 0 } else { v <= 0 }
}

#[cfg(target_arch = "riscv64")]
#[repr(align(4096))]
struct Aligned<const N: usize>([u8; N]);

#[cfg(target_arch = "riscv64")]
struct DmaPool(UnsafeCell<Aligned<DMA_POOL_BYTES>>);

#[cfg(target_arch = "riscv64")]
unsafe impl Sync for DmaPool {}

#[cfg(target_arch = "riscv64")]
static DMA_POOL: DmaPool = DmaPool(UnsafeCell::new(Aligned([0; DMA_POOL_BYTES])));
#[cfg(target_arch = "riscv64")]
static DMA_NEXT: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_arch = "riscv64")]
/// Default HAL used by the crate's `Display` type alias.
///
/// It assumes the caller runs in an identity-mapped environment and uses the
/// crate-internal DMA pool.
pub struct StaticHal;

#[cfg(target_arch = "riscv64")]
impl Hal for StaticHal {
    fn dma_alloc(pages: usize) -> usize {
        let size = pages * PAGE_SIZE;
        let offset = DMA_NEXT.fetch_add(size, Ordering::SeqCst);
        if offset + size > DMA_POOL_BYTES {
            return 0;
        }
        let base = unsafe { (*DMA_POOL.0.get()).0.as_ptr() as usize };
        base + offset
    }

    fn dma_dealloc(_paddr: usize, _pages: usize) -> i32 {
        0
    }

    fn phys_to_virt(paddr: usize) -> usize {
        paddr
    }

    fn virt_to_phys(vaddr: usize) -> usize {
        vaddr
    }
}

/// A GPU-backed display with a software canvas.
#[cfg(target_arch = "riscv64")]
pub struct DisplayDriver<H: Hal> {
    width: usize,
    height: usize,
    gpu: VirtIOGpu<'static, H, MmioTransport>,
    framebuffer_ptr: *mut u8,
    framebuffer_len: usize,
}

#[cfg(target_arch = "riscv64")]
unsafe impl<H: Hal> Send for DisplayDriver<H> {}

#[cfg(target_arch = "riscv64")]
impl<H: Hal> DisplayDriver<H> {
    /// Opens the GPU device at the given MMIO base and maps a framebuffer.
    pub fn new(mmio_base: usize) -> Result<Self> {
        let header =
            NonNull::new(mmio_base as *mut VirtIOHeader).ok_or(Error::UnsupportedPlatform)?;
        let transport = unsafe { MmioTransport::new(header) }?;
        let mut gpu = VirtIOGpu::<H, MmioTransport>::new(transport)?;
        let (width, height) = gpu.resolution()?;
        let (framebuffer_ptr, framebuffer_len) = {
            let framebuffer = gpu.setup_framebuffer()?;
            (framebuffer.as_mut_ptr(), framebuffer.len())
        };
        Ok(Self {
            width: width as usize,
            height: height as usize,
            gpu,
            framebuffer_ptr,
            framebuffer_len,
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Borrows the framebuffer as a software canvas.
    pub fn canvas(&mut self) -> Canvas<'_> {
        let pixels = unsafe {
            slice::from_raw_parts_mut(
                self.framebuffer_ptr as *mut u32,
                self.framebuffer_len / core::mem::size_of::<u32>(),
            )
        };
        Canvas::new(self.width, self.height, pixels)
    }

    /// Flushes the framebuffer contents to the host display.
    pub fn present(&mut self) -> Result<()> {
        self.gpu.flush()?;
        Ok(())
    }
}

/// GPU-backed display using the crate's default HAL.
#[cfg(target_arch = "riscv64")]
pub type Display = DisplayDriver<StaticHal>;

/// Host-side stub.
#[cfg(not(target_arch = "riscv64"))]
pub struct DisplayDriver<H> {
    pixel: u32,
    _phantom: PhantomData<H>,
}

#[cfg(not(target_arch = "riscv64"))]
impl<H> DisplayDriver<H> {
    /// Host-side stub.
    pub fn new(_mmio_base: usize) -> Result<Self> {
        Err(Error::UnsupportedPlatform)
    }

    /// Host-side stub width.
    pub fn width(&self) -> usize {
        1
    }

    /// Host-side stub height.
    pub fn height(&self) -> usize {
        1
    }

    /// Host-side stub canvas.
    pub fn canvas(&mut self) -> Canvas<'_> {
        Canvas::new(1, 1, core::slice::from_mut(&mut self.pixel))
    }

    /// Host-side stub present.
    pub fn present(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Host-side stub alias mirroring the RISC-V default type.
#[cfg(not(target_arch = "riscv64"))]
pub type Display = DisplayDriver<StaticHal>;

/// Default host-side HAL marker.
#[cfg(not(target_arch = "riscv64"))]
pub struct StaticHal;
