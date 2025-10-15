//! 进程管理系统调用
//!

use crate::{
    fs::{open_file, OpenFlags},
    mm::{translated_ref, translated_refmut, translated_str},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next, pid2task,
        suspend_current_and_run_next, SignalAction, SignalFlags, MAX_SIG,
    },
};
use alloc::{string::String, sync::Arc, vec::Vec};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    // 退出当前进程并运行下一个任务
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
    // 主动让出CPU，暂停当前任务并运行下一个任务
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    // 返回当前进程的进程ID
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    // 创建当前进程的副本（子进程）
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // 设置子进程的返回值为0（fork在子进程中返回0）
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // 设置子进程的系统调用返回值寄存器
    trap_cx.x[10] = 0;
    // 将子进程添加到调度器的就绪队列中
    add_task(new_task);
    // 父进程返回子进程的PID
    new_pid as isize
}

pub fn sys_exec(path: *const u8, mut args: *const usize) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    // 翻译程序路径字符串
    let path = translated_str(token, path);
    let mut args_vec: Vec<String> = Vec::new();
    // 解析命令行参数数组
    loop {
        let arg_str_ptr = *translated_ref(token, args);
        if arg_str_ptr == 0 {
            break;
        }
        // 翻译每个参数字符串
        args_vec.push(translated_str(token, arg_str_ptr as *const u8));
        unsafe {
            args = args.add(1);
        }
    }
    // 尝试打开要执行的程序文件
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        let argc = args_vec.len();
        // 用新程序替换当前进程的地址空间
        task.exec(all_data.as_slice(), args_vec);
        // 返回参数个数，这将成为新程序的argc
        argc as isize
    } else {
        -1
    }
}

/// 如果没有 pid 与给定值相同的子进程，返回 -1。
/// 否则如果有子进程但它仍在运行，返回 -2。
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    //trace!("kernel: sys_waitpid");
    let task = current_task().unwrap();

    // 获取当前进程控制块的独占访问权限
    let mut inner = task.inner_exclusive_access();
    // 检查是否存在指定PID的子进程（-1表示等待任意子进程）
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
    }
    // 查找已经退出（僵尸状态）的子进程
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
    });
    if let Some((idx, _)) = pair {
        // 从子进程列表中移除已退出的子进程
        let child = inner.children.remove(idx);
        // 确认子进程的引用计数为1，即将被释放
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // 获取子进程的退出代码
        let exit_code = child.inner_exclusive_access().exit_code;
        // 将退出代码写入用户空间
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        // 子进程存在但仍在运行
        -2
    }
}

pub fn sys_kill(pid: usize, signum: i32) -> isize {
    trace!("kernel:pid[{}] sys_kill", current_task().unwrap().pid.0);
    // 根据PID查找目标进程
    if let Some(task) = pid2task(pid) {
        if let Some(flag) = SignalFlags::from_bits(1 << signum) {
            // 向目标进程发送信号
            let mut task_ref = task.inner_exclusive_access();
            // 检查信号是否已经存在
            if task_ref.signals.contains(flag) {
                return -1;
            }
            // 添加信号到目标进程的信号集合中
            task_ref.signals.insert(flag);
            0
        } else {
            -1
        }
    } else {
        -1
    }
}

/// 你的任务：获取以秒和微秒为单位的时间
/// 提示：你可能需要用虚拟内存管理重新实现它。
/// 提示：如果 [`TimeVal`] 被两个页面分割怎么办？
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_get_time NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    -1
}

/// 你的任务：实现 mmap。
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    -1
}

/// 你的任务：实现 munmap。
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_munmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    -1
}

/// 改变数据段大小
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    // 尝试调整程序的堆空间大小
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        // 返回调整前的程序中断点地址
        old_brk as isize
    } else {
        -1
    }
}

/// 你的任务：实现 spawn。
/// 提示：fork + exec =/= spawn
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    -1
}

// 你的任务：设置任务优先级。
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    -1
}

pub fn sys_sigprocmask(mask: u32) -> isize {
    trace!(
        "kernel:pid[{}] sys_sigprocmask",
        current_task().unwrap().pid.0
    );
    if let Some(task) = current_task() {
        let mut inner = task.inner_exclusive_access();
        let old_mask = inner.signal_mask;
        // 设置新的信号屏蔽掩码
        if let Some(flag) = SignalFlags::from_bits(mask) {
            inner.signal_mask = flag;
            // 返回旧的信号屏蔽掩码
            old_mask.bits() as isize
        } else {
            -1
        }
    } else {
        -1
    }
}

pub fn sys_sigreturn() -> isize {
    trace!(
        "kernel:pid[{}] sys_sigreturn",
        current_task().unwrap().pid.0
    );
    if let Some(task) = current_task() {
        let mut inner = task.inner_exclusive_access();
        // 清除正在处理的信号标志
        inner.handling_sig = -1;
        // 恢复信号处理前的陷阱上下文
        let trap_ctx = inner.get_trap_cx();
        *trap_ctx = inner.trap_ctx_backup.unwrap();
        // 返回a0寄存器的值，避免被系统调用返回值覆盖
        trap_ctx.x[10] as isize
    } else {
        -1
    }
}

fn check_sigaction_error(signal: SignalFlags, action: usize, old_action: usize) -> bool {
    // 检查sigaction系统调用的参数是否有错误
    if action == 0
        || old_action == 0
        || signal == SignalFlags::SIGKILL
        || signal == SignalFlags::SIGSTOP
    {
        // SIGKILL和SIGSTOP信号不能被捕获或忽略
        true
    } else {
        false
    }
}

pub fn sys_sigaction(
    signum: i32,
    action: *const SignalAction,
    old_action: *mut SignalAction,
) -> isize {
    trace!(
        "kernel:pid[{}] sys_sigaction",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    // 检查信号编号是否有效
    if signum as usize > MAX_SIG {
        return -1;
    }
    if let Some(flag) = SignalFlags::from_bits(1 << signum) {
        // 检查参数是否有错误
        if check_sigaction_error(flag, action as usize, old_action as usize) {
            return -1;
        }
        // 获取当前的信号处理动作
        let prev_action = inner.signal_actions.table[signum as usize];
        // 将旧的信号处理动作写入用户空间
        *translated_refmut(token, old_action) = prev_action;
        // 设置新的信号处理动作
        inner.signal_actions.table[signum as usize] = *translated_ref(token, action);
        0
    } else {
        -1
    }
}
