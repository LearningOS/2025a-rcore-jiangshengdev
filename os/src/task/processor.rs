//! [`Processor`] 的实现以及控制流的交汇点。
//!
//! 负责保持用户应用在 CPU 上的连续运行，记录当前 CPU 的运行状态，
//! 并负责不同应用控制流的切换与转移。

use super::__switch;
use super::{fetch_task, TaskStatus};
use super::{time, ProcessControlBlock, TaskContext, TaskControlBlock};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;

/// 处理器管理结构
pub struct Processor {
    current: Option<Arc<TaskControlBlock>>,

    /// 各核心的基本控制流，用于选择并切换进程
    idle_task_cx: TaskContext,
}

impl Processor {
    pub fn new() -> Self {
        Self {
            current: None,
            idle_task_cx: TaskContext::zero_init(),
        }
    }

    /// 获取 `idle_task_cx` 的可变引用指针
    fn get_idle_task_cx_ptr(&mut self) -> *mut TaskContext {
        &mut self.idle_task_cx as *mut _
    }

    /// 以移动语义取出当前任务
    pub fn take_current(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.current.take()
    }

    /// 以克隆语义获取当前任务
    pub fn current(&self) -> Option<Arc<TaskControlBlock>> {
        self.current.as_ref().map(Arc::clone)
    }
}

lazy_static! {
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe { UPSafeCell::new(Processor::new()) };
}

/// 进程执行与调度的主体逻辑
/// 循环调用 `fetch_task` 获取待运行的任务，并通过 `__switch` 完成切换
pub fn run_tasks() {
    time::init();
    loop {
        let mut processor = PROCESSOR.exclusive_access();
        if let Some(task) = fetch_task() {
            let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
            // 独占访问即将运行的任务控制块
            let mut task_inner = task.inner_exclusive_access();
            let next_task_cx_ptr = &task_inner.task_cx as *const TaskContext;
            task_inner.task_status = TaskStatus::Running;
            // 手动释放任务内部引用
            drop(task_inner);
            // 手动释放任务控制块引用
            let task_for_time = Arc::clone(&task);
            processor.current = Some(task);
            time::on_task_switch_in(&task_for_time);
            // 手动释放处理器锁
            drop(processor);
            unsafe {
                __switch(idle_task_cx_ptr, next_task_cx_ptr);
            }
        } else {
            warn!("no tasks available in run_tasks");
            time::on_idle();
        }
    }
}

/// 以移动方式取出当前任务，并将内部状态置为 `None`
pub fn take_current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().take_current()
}

/// 克隆获取当前任务
pub fn current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

/// 获取当前进程
pub fn current_process() -> Arc<ProcessControlBlock> {
    current_task().unwrap().process.upgrade().unwrap()
}

/// 获取当前任务的用户态页表地址
pub fn current_user_token() -> usize {
    let task = current_task().unwrap();
    task.get_user_token()
}

/// 获取当前任务 trap 上下文的可变引用
pub fn current_trap_cx() -> &'static mut TrapContext {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .get_trap_cx()
}

/// 获取当前任务 trap 上下文的用户虚拟地址
pub fn current_trap_cx_user_va() -> usize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .trap_cx_user_va()
}

/// 获取当前任务内核栈顶部地址
pub fn current_kstack_top() -> usize {
    current_task().unwrap().kstack.get_top()
}

/// 返回空闲控制流以执行新的调度循环。
///
/// # Safety
/// 调用方必须确保 `switched_task_cx_ptr` 指向的任务上下文内存在整个切换期间保持可写且有效。
pub unsafe fn schedule(switched_task_cx_ptr: *mut TaskContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
    drop(processor);
    __switch(switched_task_cx_ptr, idle_task_cx_ptr);
}
