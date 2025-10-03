//! 任务管理实现
//!
//! 关于任务管理的所有内容，如启动和切换任务都在这里实现。
//!
//! 一个名为 `TASK_MANAGER` 的 [`TaskManager`] 全局实例控制整个操作系统中的所有任务。
//!
//! 一个名为 `PROCESSOR` 的 [`Processor`] 全局实例监控每个核心的运行任务。
//!
//! 一个名为 `PID_ALLOCATOR` 的全局实例为用户应用程序分配 PID。
//!
//! 当你看到 `switch.S` 中的 `__switch` 汇编函数时要小心。围绕此函数的控制流可能不是你期望的。
mod context;
mod id;
mod manager;
mod processor;
mod switch;
#[allow(clippy::module_inception)]
#[allow(rustdoc::private_intra_doc_links)]
mod task;

use crate::fs::{open_file, OpenFlags};
use alloc::sync::Arc;
pub use context::TaskContext;
use lazy_static::*;
pub use manager::{fetch_task, TaskManager};
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use id::{kstack_alloc, pid_alloc, KernelStack, PidHandle};
pub use manager::add_task;
pub use processor::{
    current_task, current_trap_cx, current_user_token, run_tasks, schedule, take_current_task,
    Processor,
};
/// 暂停当前"运行中"的任务并运行任务列表中的下一个任务
pub fn suspend_current_and_run_next() {
    // 必须有一个应用程序正在运行
    let task = take_current_task().unwrap();

    // ---- 独占访问当前 TCB
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    // 将状态更改为就绪
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);
    // ---- 释放当前 PCB

    // 推回就绪队列
    add_task(task);
    // 跳转到调度循环
    unsafe {
        schedule(task_cx_ptr);
    }
}

/// make run TEST=1 中 usertests 应用程序的 PID
pub const IDLE_PID: usize = 0;

/// 退出当前"运行中"的任务并运行任务列表中的下一个任务
pub fn exit_current_and_run_next(exit_code: i32) {
    // 从处理器中取出
    let task = take_current_task().unwrap();

    let pid = task.getpid();
    if pid == IDLE_PID {
        println!(
            "[kernel] Idle process exit with exit_code {} ...",
            exit_code
        );
        panic!("All applications completed!");
    }

    // **** 独占访问当前 TCB
    let mut inner = task.inner_exclusive_access();
    // 将状态更改为僵尸
    inner.task_status = TaskStatus::Zombie;
    // 记录退出代码
    inner.exit_code = exit_code;
    // 不移动到其父进程，而是移动到 initproc 下

    // ++++++ 独占访问 initproc TCB
    {
        let mut initproc_inner = INITPROC.inner_exclusive_access();
        for child in inner.children.iter() {
            child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
            initproc_inner.children.push(child.clone());
        }
    }
    // ++++++ 释放父进程 PCB

    inner.children.clear();
    // 释放用户空间
    inner.memory_set.recycle_data_pages();
    // 丢弃文件描述符
    inner.fd_table.clear();
    drop(inner);
    // **** 释放当前 PCB
    // 手动丢弃任务以正确维护引用计数
    drop(task);
    // 我们不需要保存任务上下文
    let mut _unused = TaskContext::zero_init();
    unsafe {
        schedule(&mut _unused as *mut _);
    }
}

lazy_static! {
    /// 初始进程的创建
    ///
    /// 名称 "initproc" 可以更改为任何其他应用程序名称，如 "usertests"，
    /// 但我们有 user_shell，所以不需要更改它。
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new({
        let inode = open_file("ch6b_initproc", OpenFlags::RDONLY).unwrap();
        let v = inode.read_all();
        TaskControlBlock::new(v.as_slice())
    });
}

/// 将初始进程添加到管理器
pub fn add_initproc() {
    add_task(INITPROC.clone());
}
