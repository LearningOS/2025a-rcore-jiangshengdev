//! 任务管理相关类型与用于切换 TCB 的函数。

use super::id::TaskUserRes;
use super::{kstack_alloc, KernelStack, ProcessControlBlock, TaskContext};
use crate::trap::TrapContext;
use crate::{mm::PhysPageNum, sync::UPSafeCell};
use alloc::sync::{Arc, Weak};
use core::cell::RefMut;

/// 任务控制块结构体
pub struct TaskControlBlock {
    /// 不可变字段
    pub process: Weak<ProcessControlBlock>,
    /// 与 PID 对应的内核栈
    pub kstack: KernelStack,
    /// 可变字段
    inner: UPSafeCell<TaskControlBlockInner>,
}

impl TaskControlBlock {
    /// 获取内部 TCB 的可变引用
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// 获取应用页表地址
    pub fn get_user_token(&self) -> usize {
        let process = self.process.upgrade().unwrap();
        let inner = process.inner_exclusive_access();
        inner.memory_set.token()
    }
}

pub struct TaskControlBlockInner {
    pub res: Option<TaskUserRes>,
    /// 存放 trap 上下文的物理页号
    pub trap_cx_ppn: PhysPageNum,
    /// 保存任务上下文
    pub task_cx: TaskContext,

    /// 维护当前任务的运行状态
    pub task_status: TaskStatus,
    /// 主动退出或执行错误时记录退出码
    pub exit_code: Option<i32>,
    /// 记录任务累计的用户态运行时间（毫秒）
    pub user_time_ms: usize,
    /// 记录任务累计的内核态运行时间（毫秒）
    pub kernel_time_ms: usize,
}

impl TaskControlBlockInner {
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }

    #[allow(unused)]
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
}

impl TaskControlBlock {
    /// 创建新任务
    pub fn new(
        process: Arc<ProcessControlBlock>,
        ustack_base: usize,
        alloc_user_res: bool,
    ) -> Self {
        let res = TaskUserRes::new(Arc::clone(&process), ustack_base, alloc_user_res);
        let trap_cx_ppn = res.trap_cx_ppn();
        let kstack = kstack_alloc();
        let kstack_top = kstack.get_top();
        Self {
            process: Arc::downgrade(&process),
            kstack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    res: Some(res),
                    trap_cx_ppn,
                    task_cx: TaskContext::goto_trap_return(kstack_top),
                    task_status: TaskStatus::Ready,
                    exit_code: None,
                    user_time_ms: 0,
                    kernel_time_ms: 0,
                })
            },
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
/// 任务的执行状态
pub enum TaskStatus {
    /// 就绪
    Ready,
    /// 运行中
    Running,
    /// 阻塞
    Blocked,
}
