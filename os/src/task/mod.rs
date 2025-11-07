//! 进程 [`ProcessControlBlock`] 与任务（线程）[`TaskControlBlock`] 管理机制的实现。
//!
//! 这里提供其他模块（如系统调用或时钟中断）所需的调度入口。
//! 通过挂起或退出当前任务，可以修改任务状态，经由 `TASK_MANAGER`（位于 `task/manager.rs`）管理任务队列，
//! 并利用 `PROCESSOR`（位于 `task/processor.rs`）完成控制流切换。
//!
//! 注意 [`__switch`] 的使用，该函数附近的控制流可能与直觉不符。

mod context;
mod id;
mod manager;
mod process;
mod processor;
mod signal;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use self::id::TaskUserRes;
use crate::fs::{open_file, OpenFlags};
use crate::task::manager::add_stopping_task;
use crate::timer::remove_timer;
use alloc::{sync::Arc, vec::Vec};
use lazy_static::*;
use manager::fetch_task;
use process::ProcessControlBlock;
use switch::__switch;

pub use context::TaskContext;
pub use id::{kstack_alloc, pid_alloc, KernelStack, PidHandle, IDLE_PID};
pub use manager::{add_task, pid2process, remove_from_pid2process, remove_task, wakeup_task};
pub use processor::{
    current_kstack_top, current_process, current_task, current_trap_cx, current_trap_cx_user_va,
    current_user_token, run_tasks, schedule, take_current_task,
};
pub use signal::SignalFlags;
pub use task::{TaskControlBlock, TaskStatus};

/// 挂起当前任务并切换到下一个任务
pub fn suspend_current_and_run_next() {
    // 此时必定有一个应用正在运行。
    let task = take_current_task().unwrap();

    // ---- 独占访问当前任务控制块
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    // 将状态置为 Ready
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);
    // ---- 释放当前任务控制块

    // 重新加入就绪队列。
    add_task(task);
    // 进入调度循环
    unsafe {
        schedule(task_cx_ptr);
    }
}

/// 阻塞当前任务并切换到下一个任务。
pub fn block_current_and_run_next() {
    let task = take_current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    task_inner.task_status = TaskStatus::Blocked;
    drop(task_inner);
    unsafe {
        schedule(task_cx_ptr);
    }
}

use crate::board::QEMUExit;

/// 结束当前处于 Running 状态的任务并运行任务队列中的下一个任务。
pub fn exit_current_and_run_next(exit_code: i32) {
    trace!(
        "kernel: pid[{}] exit_current_and_run_next",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    // 从 Processor 中取出当前任务
    let task = take_current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    let process = task.process.upgrade().unwrap();
    let tid = task_inner.res.as_ref().unwrap().tid;
    // 记录退出码
    task_inner.exit_code = Some(exit_code);
    task_inner.res = None;
    // 此处不移除线程，因为仍在使用其内核栈；在 sys_waittid 调用时再回收
    drop(task_inner);

    // 将任务转入等待停止状态，避免内核栈被提前回收
    if tid == 0 {
        add_stopping_task(task);
    } else {
        drop(task);
    }
    // 但若该任务是当前进程的主线程，进程需要立刻终止
    if tid == 0 {
        let pid = process.getpid();
        if pid == IDLE_PID {
            println!(
                "[kernel] Idle process exit with exit_code {} ...",
                exit_code
            );
            if exit_code != 0 {
                //crate::sbi::shutdown(255); //255 == -1 表示错误提示
                crate::board::QEMU_EXIT_HANDLE.exit_failure();
            } else {
                //crate::sbi::shutdown(0); //0 表示成功提示
                crate::board::QEMU_EXIT_HANDLE.exit_success();
            }
        }
        remove_from_pid2process(pid);
        let mut process_inner = process.inner_exclusive_access();
        // 将进程标记为僵尸进程
        process_inner.is_zombie = true;
        // 记录主进程退出码
        process_inner.exit_code = exit_code;

        {
            // 将所有子进程挂到 init 进程名下
            let mut initproc_inner = INITPROC.inner_exclusive_access();
            for child in process_inner.children.iter() {
                child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
                initproc_inner.children.push(child.clone());
            }
        }

        // 回收所有线程的用户资源（tid/trap_cx/ustack）
        // 必须在释放整个 memory_set 之前完成，否则会重复回收
        let mut recycle_res = Vec::<TaskUserRes>::new();
        for task in process_inner
            .tasks
            .iter()
            .filter_map(|t| t.as_ref().map(Arc::clone))
        {
            // 若其他任务仍在 TaskManager 的就绪队列或等待定时器到期，需要一并移除。
            //
            // 不必额外处理互斥锁/信号量，因为它们仅限于单个进程，PCB 回收时会移除被阻塞的任务。
            trace!("kernel: exit_current_and_run_next .. remove_inactive_task");
            remove_inactive_task(task.clone());
            let mut task_inner = task.inner_exclusive_access();
            if let Some(res) = task_inner.res.take() {
                recycle_res.push(res);
            }
        }
        // dealloc_tid 与 dealloc_user_res 需要访问 PCB 内部数据，
        // 因此需先收集用户资源后再暂时释放 process_inner，避免死锁或重复借用。
        drop(process_inner);
        recycle_res.clear();

        let mut process_inner = process.inner_exclusive_access();
        process_inner.children.clear();
        // 回收用户空间的其他数据（代码段 / 数据段）
        process_inner.memory_set.recycle_data_pages();
        // 关闭文件描述符
        process_inner.fd_table.clear();
        // 移除所有任务
        process_inner.tasks.clear();
    }
    drop(process);
    // 此时无需保存任务上下文
    let mut _unused = TaskContext::zero_init();
    unsafe {
        schedule(&mut _unused as *mut _);
    }
}

lazy_static! {
    /// 初始进程的创建
    ///
    /// 名称 "initproc" 可以替换为其他应用（如 "usertests"），
    /// 但由于存在 user_shell，因此无需修改。
    pub static ref INITPROC: Arc<ProcessControlBlock> = {
        let inode = open_file("ch8b_initproc", OpenFlags::RDONLY).unwrap();
        let v = inode.read_all();
        ProcessControlBlock::new(v.as_slice())
    };
}

/// 将 init 进程加入任务管理器
pub fn add_initproc() {
    let _initproc = INITPROC.clone();
}

/// 检查当前任务是否有待处理的信号
pub fn check_signals_of_current() -> Option<(i32, &'static str)> {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    process_inner.signals.check_error()
}

/// 为当前任务添加信号
pub fn current_add_signal(signal: SignalFlags) {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.signals |= signal;
}

/// 在 PCB 回收时移除处于非活动（阻塞）状态的任务（由 `exit_current_and_run_next` 调用）
pub fn remove_inactive_task(task: Arc<TaskControlBlock>) {
    remove_task(Arc::clone(&task));
    trace!("kernel: remove_inactive_task .. remove_timer");
    remove_timer(Arc::clone(&task));
}
