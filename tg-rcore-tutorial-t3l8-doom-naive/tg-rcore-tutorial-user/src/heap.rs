use alloc::alloc::handle_alloc_error;
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

/// 图形应用会长期持有一块 frame buffer，因此这里直接使用一个简单稳定的 bump heap。
const HEAP_BYTES: usize = 512 << 10;

#[repr(align(16))]
struct AlignedHeap([u8; HEAP_BYTES]);

struct HeapSpace(UnsafeCell<AlignedHeap>);

unsafe impl Sync for HeapSpace {}

static HEAP_SPACE: HeapSpace = HeapSpace(UnsafeCell::new(AlignedHeap([0; HEAP_BYTES])));
static NEXT: AtomicUsize = AtomicUsize::new(0);

pub fn init() {
    NEXT.store(0, Ordering::SeqCst);
}

struct Global;

#[global_allocator]
static GLOBAL: Global = Global;

unsafe impl GlobalAlloc for Global {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(1);
        let size = layout.size();
        let mut current = NEXT.load(Ordering::Relaxed);
        loop {
            let aligned = (current + align - 1) & !(align - 1);
            let next = aligned.saturating_add(size);
            if next > HEAP_BYTES {
                handle_alloc_error(layout);
            }
            match NEXT.compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst) {
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
