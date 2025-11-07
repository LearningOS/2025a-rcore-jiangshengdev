//! 通过简单回收策略实现 PID、任务用户资源与内核栈的分配。

use super::ProcessControlBlock;
use crate::config::{KERNEL_STACK_SIZE, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT_BASE, USER_STACK_SIZE};
use crate::mm::{MapPermission, PhysPageNum, VirtAddr, KERNEL_SPACE};
use crate::sync::UPSafeCell;
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use lazy_static::*;

/// 采用简单回收策略的分配器
pub struct RecycleAllocator {
    current: usize,
    recycled: Vec<usize>,
}

impl Default for RecycleAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl RecycleAllocator {
    /// 创建一个新的分配器
    pub fn new() -> Self {
        RecycleAllocator {
            current: 0,
            recycled: Vec::new(),
        }
    }
    /// 分配一个新的条目
    pub fn alloc(&mut self) -> usize {
        if let Some(id) = self.recycled.pop() {
            id
        } else {
            self.current += 1;
            self.current - 1
        }
    }
    /// 回收一个条目
    pub fn dealloc(&mut self, id: usize) {
        assert!(id < self.current);
        assert!(
            !self.recycled.iter().any(|i| *i == id),
            "id {} has been deallocated!",
            id
        );
        self.recycled.push(id);
    }
}

lazy_static! {
    /// 全局 PID 分配器
    static ref PID_ALLOCATOR: UPSafeCell<RecycleAllocator> =
        unsafe { UPSafeCell::new(RecycleAllocator::new()) };
    /// 全局内核栈分配器
    static ref KSTACK_ALLOCATOR: UPSafeCell<RecycleAllocator> =
        unsafe { UPSafeCell::new(RecycleAllocator::new()) };
}

/// 空闲任务的 PID 固定为 0
pub const IDLE_PID: usize = 0;

/// PID 句柄
pub struct PidHandle(pub usize);

/// 为进程分配 PID
pub fn pid_alloc() -> PidHandle {
    PidHandle(PID_ALLOCATOR.exclusive_access().alloc())
}

impl Drop for PidHandle {
    fn drop(&mut self) {
        // trace!("drop pid {}", self.0);
        PID_ALLOCATOR.exclusive_access().dealloc(self.0);
    }
}

/// 返回内核空间某个内核栈的（底部，顶部）地址。
pub fn kernel_stack_position(kstack_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - kstack_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}

/// 任务对应的内核栈
pub struct KernelStack(pub usize);

/// 为任务分配内核栈
pub fn kstack_alloc() -> KernelStack {
    let kstack_id = KSTACK_ALLOCATOR.exclusive_access().alloc();
    let (kstack_bottom, kstack_top) = kernel_stack_position(kstack_id);
    KERNEL_SPACE.exclusive_access().insert_framed_area(
        kstack_bottom.into(),
        kstack_top.into(),
        MapPermission::R | MapPermission::W,
    );
    KernelStack(kstack_id)
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let (kernel_stack_bottom, _) = kernel_stack_position(self.0);
        let kernel_stack_bottom_va: VirtAddr = kernel_stack_bottom.into();
        KERNEL_SPACE
            .exclusive_access()
            .remove_area_with_start_vpn(kernel_stack_bottom_va.into());
        KSTACK_ALLOCATOR.exclusive_access().dealloc(self.0);
    }
}

impl KernelStack {
    /// 将类型为 T 的变量压入内核栈顶并返回其裸指针
    #[allow(unused)]
    pub fn push_on_top<T>(&self, value: T) -> *mut T
    where
        T: Sized,
    {
        let kernel_stack_top = self.get_top();
        let ptr_mut = (kernel_stack_top - core::mem::size_of::<T>()) as *mut T;
        unsafe {
            *ptr_mut = value;
        }
        ptr_mut
    }
    /// 返回内核栈顶部地址
    pub fn get_top(&self) -> usize {
        let (_, kernel_stack_top) = kernel_stack_position(self.0);
        kernel_stack_top
    }
}

/// 任务的用户态资源
pub struct TaskUserRes {
    /// 任务 ID
    pub tid: usize,
    /// 用户栈基址
    pub ustack_base: usize,
    /// 所属进程
    pub process: Weak<ProcessControlBlock>,
}
/// 返回任务陷阱上下文的低地址（底部）
fn trap_cx_bottom_from_tid(tid: usize) -> usize {
    TRAP_CONTEXT_BASE - tid * PAGE_SIZE
}
/// 返回任务用户栈的底部地址（高地址）
fn ustack_bottom_from_tid(ustack_base: usize, tid: usize) -> usize {
    ustack_base + tid * (PAGE_SIZE + USER_STACK_SIZE)
}

impl TaskUserRes {
    /// 创建新的 TaskUserRes（任务用户资源）
    pub fn new(
        process: Arc<ProcessControlBlock>,
        ustack_base: usize,
        alloc_user_res: bool,
    ) -> Self {
        let tid = process.inner_exclusive_access().alloc_tid();
        let task_user_res = Self {
            tid,
            ustack_base,
            process: Arc::downgrade(&process),
        };
        if alloc_user_res {
            task_user_res.alloc_user_res();
        }
        task_user_res
    }
    /// 为任务分配用户态资源
    pub fn alloc_user_res(&self) {
        let process = self.process.upgrade().unwrap();
        let mut process_inner = process.inner_exclusive_access();
        // 分配用户栈
        let ustack_bottom = ustack_bottom_from_tid(self.ustack_base, self.tid);
        let ustack_top = ustack_bottom + USER_STACK_SIZE;
        process_inner.memory_set.insert_framed_area(
            ustack_bottom.into(),
            ustack_top.into(),
            MapPermission::R | MapPermission::W | MapPermission::U,
        );
        // 分配 trap_cx
        let trap_cx_bottom = trap_cx_bottom_from_tid(self.tid);
        let trap_cx_top = trap_cx_bottom + PAGE_SIZE;
        process_inner.memory_set.insert_framed_area(
            trap_cx_bottom.into(),
            trap_cx_top.into(),
            MapPermission::R | MapPermission::W,
        );
    }
    /// 回收任务的用户态资源
    fn dealloc_user_res(&self) {
        // 回收 tid
        let process = self.process.upgrade().unwrap();
        let mut process_inner = process.inner_exclusive_access();
        // 手动回收用户栈
        let ustack_bottom_va: VirtAddr = ustack_bottom_from_tid(self.ustack_base, self.tid).into();
        process_inner
            .memory_set
            .remove_area_with_start_vpn(ustack_bottom_va.into());
        // 手动回收 trap_cx
        let trap_cx_bottom_va: VirtAddr = trap_cx_bottom_from_tid(self.tid).into();
        process_inner
            .memory_set
            .remove_area_with_start_vpn(trap_cx_bottom_va.into());
    }

    #[allow(unused)]
    /// 分配任务 ID
    pub fn alloc_tid(&mut self) {
        self.tid = self
            .process
            .upgrade()
            .unwrap()
            .inner_exclusive_access()
            .alloc_tid();
    }
    /// 回收任务 ID
    pub fn dealloc_tid(&self) {
        let process = self.process.upgrade().unwrap();
        let mut process_inner = process.inner_exclusive_access();
        process_inner.dealloc_tid(self.tid);
    }
    /// 返回指定 tid 任务陷阱上下文的用户虚拟地址（底部）
    pub fn trap_cx_user_va(&self) -> usize {
        trap_cx_bottom_from_tid(self.tid)
    }
    /// 返回指定 tid 任务陷阱上下文所在的物理页号
    pub fn trap_cx_ppn(&self) -> PhysPageNum {
        let process = self.process.upgrade().unwrap();
        let process_inner = process.inner_exclusive_access();
        let trap_cx_bottom_va: VirtAddr = trap_cx_bottom_from_tid(self.tid).into();
        process_inner
            .memory_set
            .translate(trap_cx_bottom_va.into())
            .unwrap()
            .ppn()
    }
    /// 返回任务用户栈的底部地址
    pub fn ustack_base(&self) -> usize {
        self.ustack_base
    }
    /// 返回任务用户栈的顶部地址
    pub fn ustack_top(&self) -> usize {
        ustack_bottom_from_tid(self.ustack_base, self.tid) + USER_STACK_SIZE
    }
}

impl Drop for TaskUserRes {
    fn drop(&mut self) {
        self.dealloc_tid();
        self.dealloc_user_res();
    }
}
