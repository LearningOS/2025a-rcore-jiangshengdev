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
    // 从处理器中取出当前正在运行的任务
    let task = take_current_task().unwrap();

    // 获取任务控制块的独占访问权限
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    // 将任务状态从运行中改为就绪状态
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);

    // 将任务重新加入就绪队列等待调度
    add_task(task);
    // 切换到调度器，选择下一个任务运行
    unsafe {
        schedule(task_cx_ptr);
    }
}

/// make run TEST=1 中 usertests 应用程序的 PID
pub const IDLE_PID: usize = 0;

/// 退出当前"运行中"的任务并运行任务列表中的下一个任务
pub fn exit_current_and_run_next(exit_code: i32) {
    // 从处理器中取出当前正在运行的任务
    let task = take_current_task().unwrap();

    let pid = task.getpid();
    // 检查是否是空闲进程退出，如果是则表示所有应用程序都已完成
    if pid == IDLE_PID {
        println!(
            "[kernel] Idle process exit with exit_code {} ...",
            exit_code
        );
        panic!("All applications completed!");
    }

    // 从PID到任务的映射表中移除该任务
    remove_from_pid2task(task.getpid());
    // 获取任务控制块的独占访问权限
    let mut inner = task.inner_exclusive_access();
    // 将任务状态设置为僵尸状态，等待父进程回收
    inner.task_status = TaskStatus::Zombie;
    // 保存任务的退出代码
    inner.exit_code = exit_code;
    // 将所有子进程的父进程设置为init进程，避免孤儿进程

    // 获取init进程的独占访问权限
    {
        let mut initproc_inner = INITPROC.inner_exclusive_access();
        // 遍历当前进程的所有子进程
        for child in inner.children.iter() {
            // 将子进程的父进程指针指向init进程
            child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
            // 将子进程添加到init进程的子进程列表中
            initproc_inner.children.push(child.clone());
        }
    }

    // 清空当前进程的子进程列表
    inner.children.clear();
    // 回收进程的用户空间内存页面
    inner.memory_set.recycle_data_pages();
    // 关闭所有打开的文件描述符
    inner.fd_table.clear();
    drop(inner);
    // 手动释放任务控制块以正确维护引用计数
    drop(task);
    // 创建一个未使用的任务上下文，因为退出的任务不需要保存上下文
    let mut _unused = TaskContext::zero_init();
    // 切换到调度器选择下一个任务运行
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
    // 将初始进程克隆并添加到任务管理器的就绪队列中
    add_task(INITPROC.clone());
}

/// 检查当前任务是否有需要处理的信号
pub fn check_signals_error_of_current() -> Option<(i32, &'static str)> {
    // 获取当前任务并检查是否有错误信号需要处理
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    // println!(
    //     "[K] check_signals_error_of_current {:?}",
    //     task_inner.signals
    // );
    task_inner.signals.check_error()
}

/// 向当前任务添加信号
pub fn current_add_signal(signal: SignalFlags) {
    // 获取当前任务并向其信号集合中添加指定信号
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    task_inner.signals |= signal;
    // println!(
    //     "[K] current_add_signal:: current task sigflag {:?}",
    //     task_inner.signals
    // );
}

/// 调用内核信号处理函数
fn call_kernel_signal_handler(signal: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    // 根据信号类型执行相应的内核信号处理
    match signal {
        // SIGSTOP信号：暂停进程执行
        SignalFlags::SIGSTOP => {
            task_inner.frozen = true;
            task_inner.signals ^= SignalFlags::SIGSTOP;
        }
        // SIGCONT信号：继续暂停的进程执行
        SignalFlags::SIGCONT => {
            if task_inner.signals.contains(SignalFlags::SIGCONT) {
                task_inner.signals ^= SignalFlags::SIGCONT;
                task_inner.frozen = false;
            }
        }
        // 其他信号：标记进程为被杀死状态
        _ => {
            // println!(
            //     "[K] call_kernel_signal_handler:: current task sigflag {:?}",
            //     task_inner.signals
            // );
            task_inner.killed = true;
        }
    }
}

/// 调用用户信号处理函数
fn call_user_signal_handler(sig: usize, signal: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();

    let handler = task_inner.signal_actions.table[sig].handler;
    if handler != 0 {
        // 如果用户设置了自定义信号处理函数

        // 设置当前正在处理的信号标志
        task_inner.handling_sig = sig as isize;
        task_inner.signals ^= signal;

        // 备份当前的陷阱上下文
        let trap_ctx = task_inner.get_trap_cx();
        task_inner.trap_ctx_backup = Some(*trap_ctx);

        // 修改陷阱上下文，跳转到用户信号处理函数
        trap_ctx.sepc = handler;

        // 设置信号处理函数的参数（信号编号）
        trap_ctx.x[10] = sig;
    } else {
        // 使用默认的信号处理动作
        println!("[K] task/call_user_signal_handler: default action: ignore it or kill process");
    }
}

/// 检查当前任务是否有待处理的信号
fn check_pending_signals() {
    // 遍历所有可能的信号编号
    for sig in 0..(MAX_SIG + 1) {
        let task = current_task().unwrap();
        let task_inner = task.inner_exclusive_access();
        let signal = SignalFlags::from_bits(1 << sig).unwrap();
        // 检查任务是否有该信号且该信号未被屏蔽
        if task_inner.signals.contains(signal) && (!task_inner.signal_mask.contains(signal)) {
            let mut masked = true;
            let handling_sig = task_inner.handling_sig;
            // 检查信号是否被当前正在处理的信号屏蔽
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
            // 如果信号未被屏蔽，则处理该信号
            if !masked {
                drop(task_inner);
                drop(task);
                // 判断是内核信号还是用户信号
                if signal == SignalFlags::SIGKILL
                    || signal == SignalFlags::SIGSTOP
                    || signal == SignalFlags::SIGCONT
                    || signal == SignalFlags::SIGDEF
                {
                    // 内核信号，由内核直接处理
                    call_kernel_signal_handler(signal);
                } else {
                    // 用户信号，调用用户信号处理函数
                    call_user_signal_handler(sig, signal);
                    return;
                }
            }
        }
    }
}

/// 处理当前进程的信号
pub fn handle_signals() {
    loop {
        // 检查并处理待处理的信号
        check_pending_signals();
        let (frozen, killed) = {
            let task = current_task().unwrap();
            let task_inner = task.inner_exclusive_access();
            (task_inner.frozen, task_inner.killed)
        };
        // 如果进程未被冻结或已被杀死，则退出信号处理循环
        if !frozen || killed {
            break;
        }
        // 如果进程被冻结，暂停当前任务并运行下一个任务
        suspend_current_and_run_next();
    }
}
