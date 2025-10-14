//! 系统调用的实现
//!
//! 所有系统调用的单一入口点 [`syscall()`] 在用户空间希望使用 `ecall`
//! 指令执行系统调用时被调用。在这种情况下，处理器会引发一个
//! "来自 U 模式的环境调用"异常，这作为 [`crate::trap::trap_handler`]
//! 中的一种情况来处理。
//!
//! 为了清晰起见，每个单独的系统调用都作为自己的函数实现，命名为
//! `sys_` 加上系统调用的名称。你可以在子模块中找到这样的函数，
//! 你也应该以这种方式实现系统调用。

/// dup syscall
const SYSCALL_DUP: usize = 24;
/// unlinkat 系统调用
const SYSCALL_UNLINKAT: usize = 35;
/// linkat 系统调用
const SYSCALL_LINKAT: usize = 37;
/// open 系统调用
const SYSCALL_OPEN: usize = 56;
/// close 系统调用
const SYSCALL_CLOSE: usize = 57;
/// pipe syscall
const SYSCALL_PIPE: usize = 59;
/// read 系统调用
const SYSCALL_READ: usize = 63;
/// write 系统调用
const SYSCALL_WRITE: usize = 64;
/// fstat 系统调用
const SYSCALL_FSTAT: usize = 80;
/// exit 系统调用
const SYSCALL_EXIT: usize = 93;
/// yield 系统调用
const SYSCALL_YIELD: usize = 124;
/// kill syscall
const SYSCALL_KILL: usize = 129;
/// sigaction syscall
const SYSCALL_SIGACTION: usize = 134;
/// sigprocmask syscall
const SYSCALL_SIGPROCMASK: usize = 135;
/// sigreturn syscall
const SYSCALL_SIGRETURN: usize = 139;
/// setpriority 系统调用
const SYSCALL_SET_PRIORITY: usize = 140;
/// gettime 系统调用
const SYSCALL_GET_TIME: usize = 169;
/// getpid 系统调用
const SYSCALL_GETPID: usize = 172;
/// sbrk 系统调用
const SYSCALL_SBRK: usize = 214;
/// munmap 系统调用
const SYSCALL_MUNMAP: usize = 215;
/// fork 系统调用
const SYSCALL_FORK: usize = 220;
/// exec 系统调用
const SYSCALL_EXEC: usize = 221;
/// mmap 系统调用
const SYSCALL_MMAP: usize = 222;
/// waitpid 系统调用
const SYSCALL_WAITPID: usize = 260;
/// spawn 系统调用
const SYSCALL_SPAWN: usize = 400;

mod fs;
mod process;

use fs::*;
use process::*;

use crate::{fs::Stat, task::SignalAction};

/// 使用 `syscall_id` 和其他参数处理系统调用异常
pub fn syscall(syscall_id: usize, args: [usize; 4]) -> isize {
    match syscall_id {
        SYSCALL_DUP => sys_dup(args[0]),
        SYSCALL_OPEN => sys_open(args[1] as *const u8, args[2] as u32),
        SYSCALL_CLOSE => sys_close(args[0]),
        SYSCALL_PIPE => sys_pipe(args[0] as *mut usize),
        SYSCALL_LINKAT => sys_linkat(args[1] as *const u8, args[3] as *const u8),
        SYSCALL_UNLINKAT => sys_unlinkat(args[1] as *const u8),
        SYSCALL_READ => sys_read(args[0], args[1] as *const u8, args[2]),
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_FSTAT => sys_fstat(args[0], args[1] as *mut Stat),
        SYSCALL_EXIT => sys_exit(args[0] as i32),
        SYSCALL_YIELD => sys_yield(),
        SYSCALL_KILL => sys_kill(args[0], args[1] as i32),
        SYSCALL_SIGACTION => sys_sigaction(
            args[0] as i32,
            args[1] as *const SignalAction,
            args[2] as *mut SignalAction,
        ),
        SYSCALL_SIGPROCMASK => sys_sigprocmask(args[0] as u32),
        SYSCALL_SIGRETURN => sys_sigreturn(),
        SYSCALL_GETPID => sys_getpid(),
        SYSCALL_FORK => sys_fork(),
        SYSCALL_EXEC => sys_exec(args[0] as *const u8, args[1] as *const usize),
        SYSCALL_WAITPID => sys_waitpid(args[0] as isize, args[1] as *mut i32),
        SYSCALL_GET_TIME => sys_get_time(args[0] as *mut TimeVal, args[1]),
        SYSCALL_MMAP => sys_mmap(args[0], args[1], args[2]),
        SYSCALL_MUNMAP => sys_munmap(args[0], args[1]),
        SYSCALL_SBRK => sys_sbrk(args[0] as i32),
        SYSCALL_SPAWN => sys_spawn(args[0] as *const u8),
        SYSCALL_SET_PRIORITY => sys_set_priority(args[0] as isize),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}
