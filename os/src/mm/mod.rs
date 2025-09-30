//! 内存管理实现
//!
//! 针对 RV64 系统的 SV39 基于页面的虚拟内存架构，
//! 以及关于内存管理的所有内容，如帧分配器、页表、
//! 映射区域和内存集合，都在这里实现。
//!
//! 每个任务或进程都有一个 memory_set 来控制其虚拟内存。

mod address;
mod frame_allocator;
mod heap_allocator;
mod memory_set;
mod page_table;

use address::VPNRange;
pub use address::{PhysAddr, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
pub use frame_allocator::{frame_alloc, frame_dealloc, FrameTracker};
pub use memory_set::remap_test;
pub use memory_set::{kernel_token, MapPermission, MemorySet, KERNEL_SPACE};
use page_table::PTEFlags;
pub use page_table::{
    translated_byte_buffer, translated_ref, translated_refmut, translated_str, PageTable,
    PageTableEntry, UserBuffer, UserBufferIterator,
};

/// 初始化堆分配器、帧分配器和内核空间
pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}
