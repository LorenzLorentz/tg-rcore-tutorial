//! 内存分配。
//!
//! 教程阅读建议：
//!
//! - 先看 `init` 与 `transfer`：理解“先初始化，再把可用内存交给分配器”；
//! - 再看 `HEAP` / `GlobalAlloc`：理解 Rust `alloc` 如何落到内核堆实现。

#![no_std]
#![deny(missing_docs)]

extern crate alloc;

use alloc::alloc::handle_alloc_error;
use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
};
use customizable_buddy::{BuddyAllocator, LinkedListBuddy, UsizeBuddy};
use spin::Mutex;

/// 初始化内存分配。
///
/// 参数 `base_address` 表示动态内存区域的起始位置。
///
/// # 注意
///
/// 此函数必须在使用任何堆分配之前调用，且只能调用一次。
#[inline]
pub fn init(base_address: usize) {
    // 启动阶段由 hart0 初始化，其他 hart 此时还不会进入分配路径。
    HEAP.lock().init(
        core::mem::size_of::<usize>().trailing_zeros() as _,
        NonNull::new(base_address as *mut u8).unwrap(),
    );
}

/// 将一个内存块托管到内存分配器。
///
/// # Safety
///
/// 调用者必须确保：
/// - `region` 内存块与已经转移到分配器的内存块都不重叠
/// - `region` 未被其他对象引用
/// - `region` 必须位于初始化时传入的起始位置之后
/// - 内存块的所有权将转移到分配器
#[inline]
pub unsafe fn transfer(region: &'static mut [u8]) {
    // 将一段“现成内存”并入堆。常用于把启动后可回收区域纳入分配器管理。
    let ptr = NonNull::new(region.as_mut_ptr()).unwrap();
    // SAFETY: 由调用者保证内存块有效且不重叠
    unsafe { HEAP.lock().transfer(ptr, region.len()) };
}

/// 堆分配器。
///
/// 最大容量：6 + 21 + 3 = 30 -> 1 GiB。
/// 多核环境下通过自旋锁串行化对 buddy 元数据的访问。
static HEAP: Mutex<BuddyAllocator<21, UsizeBuddy, LinkedListBuddy>> =
    Mutex::new(BuddyAllocator::new());

struct Global;

#[global_allocator]
static GLOBAL: Global = Global;

// SAFETY: GlobalAlloc 的实现必须是 unsafe 的。
unsafe impl GlobalAlloc for Global {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut heap = HEAP.lock();
        if let Ok((ptr, _)) = heap.allocate_layout::<u8>(layout) {
            ptr.as_ptr()
        } else {
            drop(heap);
            handle_alloc_error(layout)
        }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            HEAP.lock()
                .deallocate_layout(NonNull::new(ptr).unwrap(), layout)
        }
    }
}
