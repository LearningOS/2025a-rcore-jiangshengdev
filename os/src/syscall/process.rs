use crate::mm::translated_byte_buffer;
use crate::timer::{get_time_us, perf_enabled};
use crate::{
    fs::{open_file, OpenFlags},
    mm::{translated_ref, translated_refmut, translated_str},
    task::{
        current_process, current_task, current_user_token, exit_current_and_run_next, pid2process,
        suspend_current_and_run_next, SignalFlags,
    },
};
use alloc::{format, string::String, sync::Arc, vec::Vec};
use core::cmp::max;
use core::mem::size_of;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// exit 系统调用
///
/// 退出当前任务并运行任务队列中的下一个任务
pub fn sys_exit(exit_code: i32) -> ! {
    trace!(
        "kernel:pid[{}] sys_exit",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}
/// yield 系统调用
pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}
/// getpid 系统调用
pub fn sys_getpid() -> isize {
    trace!(
        "kernel: sys_getpid pid:{}",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    current_task().unwrap().process.upgrade().unwrap().getpid() as isize
}
/// fork 子进程系统调用
pub fn sys_fork() -> isize {
    trace!(
        "kernel:pid[{}] sys_fork",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    let current_process = current_process();
    let new_process = current_process.fork();
    let new_pid = new_process.getpid();
    // 修改新线程的 trap 上下文，因为切换后会立即返回
    let new_process_inner = new_process.inner_exclusive_access();
    let task = new_process_inner.tasks[0].as_ref().unwrap();
    let trap_cx = task.inner_exclusive_access().get_trap_cx();
    // 无需再次调整到下一条指令；对子进程而言，fork 返回 0
    trap_cx.x[10] = 0;
    crate::dbg_hold(trap_cx);
    new_pid as isize
}
/// exec 系统调用
pub fn sys_exec(path: *const u8, mut args: *const usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_exec",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    let total_start = get_time_us();
    let arg_collect_start = total_start;
    let token = current_user_token();
    let path = translated_str(token, path);
    let mut args_vec: Vec<String> = Vec::new();
    loop {
        let arg_str_ptr = *translated_ref(token, args);
        if arg_str_ptr == 0 {
            break;
        }
        args_vec.push(translated_str(token, arg_str_ptr as *const u8));
        unsafe {
            args = args.add(1);
        }
    }
    let arg_collect_us = get_time_us().saturating_sub(arg_collect_start);
    let open_start = get_time_us();
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let open_us = get_time_us().saturating_sub(open_start);
        let read_start = get_time_us();
        let all_data = app_inode.read_all();
        let read_us = get_time_us().saturating_sub(read_start);
        let process = current_process();
        let argc = args_vec.len();
        let exec_profile = process.exec(all_data.as_slice(), args_vec, path.as_str());
        if perf_enabled() {
            let total_us = get_time_us().saturating_sub(total_start);
            let total_ms = total_us / 1_000;
            let name_width = max(path.len(), 41);
            let aligned_name = format!("{:width$}", path, width = name_width);
            const LABEL_PAD: usize = 9;
            let fmt_ms = |label: &str, value_ms: usize| {
                format!("{:>width$}= {:>6}ms", label, value_ms, width = LABEL_PAD)
            };
            let fmt_us = |label: &str, value_us: usize| {
                format!("{:>width$}= {:>6}us", label, value_us, width = LABEL_PAD)
            };
            println!("[exec-prof] {} {}", aligned_name, fmt_ms("total", total_ms));
            println!(
                "[exec-prof]   {} {} {} {} {}",
                fmt_us("args", arg_collect_us),
                fmt_us("open", open_us),
                fmt_us("read", read_us),
                fmt_us("reset", exec_profile.reset_us),
                fmt_us("mem", exec_profile.memory_set_us)
            );
            println!(
                "[exec-prof]   {} {} {} {}",
                fmt_us("install", exec_profile.install_us),
                fmt_us("user_res", exec_profile.user_res_us),
                fmt_us("argv", exec_profile.argv_us),
                fmt_us("trap", exec_profile.trap_us)
            );
        }
        // 返回 argc，因为稍后会覆盖到 cx.x[10]
        argc as isize
    } else {
        -1
    }
}

/// waitpid 系统调用
///
/// 若不存在指定 pid 的子进程，返回 -1；若子进程仍在运行，返回 -2。
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    //trace!("kernel: sys_waitpid");
    let process = current_process();
    // 查找目标子进程

    let mut inner = process.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- 释放当前 PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ 临时独占访问子进程 PCB
        p.inner_exclusive_access().is_zombie && (pid == -1 || pid as usize == p.getpid())
        // ++++ 释放子进程 PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // 确认移出子进程列表后会被释放
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ 临时独占访问子进程 PCB
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ 释放子进程 PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- 当前 PCB 会自动释放
}

/// kill 系统调用
pub fn sys_kill(pid: usize, signal: u32) -> isize {
    trace!(
        "kernel:pid[{}] sys_kill",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    if let Some(process) = pid2process(pid) {
        if let Some(flag) = SignalFlags::from_bits(signal) {
            process.inner_exclusive_access().signals |= flag;
            0
        } else {
            -1
        }
    } else {
        -1
    }
}

/// get_time 系统调用
///
/// TODO：返回以秒和微秒计的时间。
/// 提示：可以结合虚拟内存管理重新实现；若 [`TimeVal`] 跨越两个页面需如何处理？
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_get_time",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    if ts.is_null() {
        return -1;
    }
    let micros = get_time_us();
    let time_val = TimeVal {
        sec: micros / 1_000_000,
        usec: micros % 1_000_000,
    };
    let token = current_user_token();
    let len = size_of::<TimeVal>();
    let raw_bytes =
        unsafe { core::slice::from_raw_parts(&time_val as *const TimeVal as *const u8, len) };
    let mut offset = 0;
    for buf in translated_byte_buffer(token, ts as *const u8, len) {
        let end = (offset + buf.len()).min(len);
        let copy_len = end - offset;
        if copy_len == 0 {
            continue;
        }
        buf[..copy_len].copy_from_slice(&raw_bytes[offset..offset + copy_len]);
        offset += copy_len;
        if offset >= len {
            break;
        }
    }
    if offset == len {
        0
    } else {
        -1
    }
}

/// mmap 系统调用
///
/// TODO：实现 mmap。
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mmap NOT IMPLEMENTED",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    -1
}

/// munmap 系统调用
///
/// TODO：实现 munmap。
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_munmap NOT IMPLEMENTED",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    -1
}

/// 调整数据段大小
// pub fn sys_sbrk(size: i32) -> isize {
//     trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().process.upgrade().unwrap().getpid());
//     if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
//         old_brk as isize
//     } else {
//     -1
// }

/// spawn 系统调用
/// TODO：实现 spawn。
/// 提示：fork + exec 不等于 spawn
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn NOT IMPLEMENTED",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    -1
}

/// 设置优先级的系统调用
///
/// TODO：实现任务优先级设定
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    -1
}
