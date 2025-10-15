//! [`FrameAllocator`] 的实现，
//! 控制操作系统中的所有帧。
use super::{PhysAddr, PhysPageNum};
use crate::config::MEMORY_END;
use crate::sync::UPSafeCell;
use alloc::vec::Vec;
use core::fmt::{self, Debug, Formatter};
use lazy_static::*;

/// 物理页帧分配和释放的跟踪器
pub struct FrameTracker {
    /// 物理页号
    pub ppn: PhysPageNum,
}

impl FrameTracker {
    /// 创建一个新的 FrameTracker
    pub fn new(ppn: PhysPageNum) -> Self {
        // 将新分配的物理页面清零，确保安全性
        let bytes_array = ppn.get_bytes_array();
        for i in bytes_array {
            *i = 0;
        }
        Self { ppn }
    }
}

impl Debug for FrameTracker {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("FrameTracker:PPN={:#x}", self.ppn.0))
    }
}

impl Drop for FrameTracker {
    fn drop(&mut self) {
        // 当FrameTracker被销毁时，自动释放对应的物理页面
        frame_dealloc(self.ppn);
    }
}

trait FrameAllocator {
    fn new() -> Self;
    fn alloc(&mut self) -> Option<PhysPageNum>;
    fn dealloc(&mut self, ppn: PhysPageNum);
}
/// 帧分配器的一个实现
pub struct StackFrameAllocator {
    current: usize,
    end: usize,
    recycled: Vec<usize>,
}

impl StackFrameAllocator {
    pub fn init(&mut self, l: PhysPageNum, r: PhysPageNum) {
        // 设置可分配的物理页面范围
        self.current = l.0;
        self.end = r.0;
        // trace!("剩余 {} 个物理帧。", self.end - self.current);
    }
}
impl FrameAllocator for StackFrameAllocator {
    fn new() -> Self {
        Self {
            current: 0,
            end: 0,
            recycled: Vec::new(),
        }
    }
    fn alloc(&mut self) -> Option<PhysPageNum> {
        // 优先从回收栈中分配页面
        if let Some(ppn) = self.recycled.pop() {
            Some(ppn.into())
        } else if self.current == self.end {
            // 没有可用页面
            None
        } else {
            // 从连续区域分配新页面
            self.current += 1;
            Some((self.current - 1).into())
        }
    }
    fn dealloc(&mut self, ppn: PhysPageNum) {
        let ppn = ppn.0;
        // 检查页面是否有效且已被分配
        if ppn >= self.current || self.recycled.iter().any(|&v| v == ppn) {
            panic!("帧 ppn={:#x} 尚未被分配！", ppn);
        }
        // 将页面添加到回收栈中以供重用
        self.recycled.push(ppn);
    }
}

type FrameAllocatorImpl = StackFrameAllocator;

lazy_static! {
    /// 通过 lazy_static! 创建的帧分配器实例
    pub static ref FRAME_ALLOCATOR: UPSafeCell<FrameAllocatorImpl> =
        unsafe { UPSafeCell::new(FrameAllocatorImpl::new()) };
}
/// 使用 `ekernel` 和 `MEMORY_END` 初始化帧分配器
pub fn init_frame_allocator() {
    extern "C" {
        fn ekernel();
    }
    // 初始化帧分配器，管理内核结束地址到内存结束地址之间的物理页面
    FRAME_ALLOCATOR.exclusive_access().init(
        PhysAddr::from(ekernel as usize).ceil(),
        PhysAddr::from(MEMORY_END).floor(),
    );
}

/// 以 FrameTracker 风格分配一个物理页帧
pub fn frame_alloc() -> Option<FrameTracker> {
    // 从全局帧分配器分配页面并包装为FrameTracker
    FRAME_ALLOCATOR
        .exclusive_access()
        .alloc()
        .map(FrameTracker::new)
}

/// 释放给定 ppn 的物理页帧
pub fn frame_dealloc(ppn: PhysPageNum) {
    // 将物理页面归还给全局帧分配器
    FRAME_ALLOCATOR.exclusive_access().dealloc(ppn);
}

#[allow(unused)]
/// 帧分配器的简单测试
pub fn frame_allocator_test() {
    let mut v: Vec<FrameTracker> = Vec::new();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        println!("{:?}", frame);
        v.push(frame);
    }
    v.clear();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        println!("{:?}", frame);
        v.push(frame);
    }
    drop(v);
    println!("帧分配器测试通过！");
}
