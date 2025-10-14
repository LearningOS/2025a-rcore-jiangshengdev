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

mod action;
mod context;
mod id;
mod manager;
mod processor;
mod signal;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::fs::{open_file, OpenFlags};
use alloc::sync::Arc;
pub use context::TaskContext;
use lazy_static::*;
use manager::fetch_task;
use manager::remove_from_pid2task;
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use action::{SignalAction, SignalActions};
pub use id::{kstack_alloc, pid_alloc, KernelStack, PidHandle};
pub use manager::{add_task, pid2task};
pub use processor::{
    current_task, current_trap_cx, current_user_token, run_tasks, schedule, take_current_task,
};
pub use signal::{SignalFlags, MAX_SIG};

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

    // remove from pid2task
    remove_from_pid2task(task.getpid());
    // **** access current TCB exclusively
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
        let inode = open_file("ch7b_initproc", OpenFlags::RDONLY).unwrap();
        let v = inode.read_all();
        TaskControlBlock::new(v.as_slice())
    });
}

/// 将初始进程添加到管理器
pub fn add_initproc() {
    add_task(INITPROC.clone());
}

/// Check if the current task has any signal to handle
pub fn check_signals_error_of_current() -> Option<(i32, &'static str)> {
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    // println!(
    //     "[K] check_signals_error_of_current {:?}",
    //     task_inner.signals
    // );
    task_inner.signals.check_error()
}

/// Add signal to the current task
pub fn current_add_signal(signal: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    task_inner.signals |= signal;
    // println!(
    //     "[K] current_add_signal:: current task sigflag {:?}",
    //     task_inner.signals
    // );
}

/// call kernel signal handler
fn call_kernel_signal_handler(signal: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    match signal {
        SignalFlags::SIGSTOP => {
            task_inner.frozen = true;
            task_inner.signals ^= SignalFlags::SIGSTOP;
        }
        SignalFlags::SIGCONT => {
            if task_inner.signals.contains(SignalFlags::SIGCONT) {
                task_inner.signals ^= SignalFlags::SIGCONT;
                task_inner.frozen = false;
            }
        }
        _ => {
            // println!(
            //     "[K] call_kernel_signal_handler:: current task sigflag {:?}",
            //     task_inner.signals
            // );
            task_inner.killed = true;
        }
    }
}

/// call user signal handler
fn call_user_signal_handler(sig: usize, signal: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();

    let handler = task_inner.signal_actions.table[sig].handler;
    if handler != 0 {
        // user handler

        // handle flag
        task_inner.handling_sig = sig as isize;
        task_inner.signals ^= signal;

        // backup trapframe
        let trap_ctx = task_inner.get_trap_cx();
        task_inner.trap_ctx_backup = Some(*trap_ctx);

        // modify trapframe
        trap_ctx.sepc = handler;

        // put args (a0)
        trap_ctx.x[10] = sig;
    } else {
        // default action
        println!("[K] task/call_user_signal_handler: default action: ignore it or kill process");
    }
}

/// Check if the current task has any signal to handle
fn check_pending_signals() {
    for sig in 0..(MAX_SIG + 1) {
        let task = current_task().unwrap();
        let task_inner = task.inner_exclusive_access();
        let signal = SignalFlags::from_bits(1 << sig).unwrap();
        if task_inner.signals.contains(signal) && (!task_inner.signal_mask.contains(signal)) {
            let mut masked = true;
            let handling_sig = task_inner.handling_sig;
            if handling_sig == -1 {
                masked = false;
            } else {
                let handling_sig = handling_sig as usize;
                if !task_inner.signal_actions.table[handling_sig]
                    .mask
                    .contains(signal)
                {
                    masked = false;
                }
            }
            if !masked {
                drop(task_inner);
                drop(task);
                if signal == SignalFlags::SIGKILL
                    || signal == SignalFlags::SIGSTOP
                    || signal == SignalFlags::SIGCONT
                    || signal == SignalFlags::SIGDEF
                {
                    // signal is a kernel signal
                    call_kernel_signal_handler(signal);
                } else {
                    // signal is a user signal
                    call_user_signal_handler(sig, signal);
                    return;
                }
            }
        }
    }
}

/// Handle signals for the current process
pub fn handle_signals() {
    loop {
        check_pending_signals();
        let (frozen, killed) = {
            let task = current_task().unwrap();
            let task_inner = task.inner_exclusive_access();
            (task_inner.frozen, task_inner.killed)
        };
        if !frozen || killed {
            break;
        }
        suspend_current_and_run_next();
    }
}
