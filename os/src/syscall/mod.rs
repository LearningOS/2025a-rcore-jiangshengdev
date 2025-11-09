//! 系统调用的实现。
//!
//! 所有系统调用的唯一入口是 [`syscall()`]，当用户态通过 `ecall` 指令发起系统调用时会触发该函数。
//! 此时处理器会产生“U 模式环境调用”异常，由 [`crate::trap::trap_handler`] 统一处理。
//!
//! 为了清晰起见，每个系统调用都单独实现为以 `sys_` 开头的函数，相应实现位于各子模块中，新的系统调用也应遵循此约定。

/// openat 系统调用
pub const SYSCALL_OPENAT: usize = 56;
/// close 系统调用
pub const SYSCALL_CLOSE: usize = 57;
/// read 系统调用
pub const SYSCALL_READ: usize = 63;
/// write 系统调用
pub const SYSCALL_WRITE: usize = 64;
/// unlinkat 系统调用
pub const SYSCALL_UNLINKAT: usize = 35;
/// linkat 系统调用
pub const SYSCALL_LINKAT: usize = 37;
/// fstat 系统调用
pub const SYSCALL_FSTAT: usize = 80;
/// exit 系统调用
pub const SYSCALL_EXIT: usize = 93;
/// sleep 系统调用
pub const SYSCALL_SLEEP: usize = 101;
/// yield 系统调用
pub const SYSCALL_YIELD: usize = 124;
/// kill 系统调用
pub const SYSCALL_KILL: usize = 129;
/*
/// sigaction 系统调用
pub const SYSCALL_SIGACTION: usize = 134;
/// sigprocmask 系统调用
pub const SYSCALL_SIGPROCMASK: usize = 135;
/// sigreturn 系统调用
pub const SYSCALL_SIGRETURN: usize = 139;
*/
/// gettimeofday 系统调用
pub const SYSCALL_GETTIMEOFDAY: usize = 169;
/// getpid 系统调用
pub const SYSCALL_GETPID: usize = 172;
/// gettid 系统调用
pub const SYSCALL_GETTID: usize = 178;
/// fork 系统调用
pub const SYSCALL_FORK: usize = 220;
/// exec 系统调用
pub const SYSCALL_EXEC: usize = 221;
/// waitpid 系统调用
pub const SYSCALL_WAITPID: usize = 260;
/// 设置优先级系统调用
pub const SYSCALL_SET_PRIORITY: usize = 140;
/*
/// sbrk 系统调用
pub const SYSCALL_SBRK: usize = 214;
*/
/// munmap 系统调用
pub const SYSCALL_MUNMAP: usize = 215;
/// mmap 系统调用
pub const SYSCALL_MMAP: usize = 222;
/// spawn 系统调用
pub const SYSCALL_SPAWN: usize = 400;
/*
/// mail read 系统调用
pub const SYSCALL_MAIL_READ: usize = 401;
/// mail write 系统调用
pub const SYSCALL_MAIL_WRITE: usize = 402;
*/
/// dup 系统调用
pub const SYSCALL_DUP: usize = 24;
/// pipe 系统调用
pub const SYSCALL_PIPE: usize = 59;
/// thread_create 系统调用
pub const SYSCALL_THREAD_CREATE: usize = 460;
/// waittid 系统调用
pub const SYSCALL_WAITTID: usize = 462;
/// mutex_create 系统调用
pub const SYSCALL_MUTEX_CREATE: usize = 463;
/// mutex_lock 系统调用
pub const SYSCALL_MUTEX_LOCK: usize = 464;
/// mutex_unlock 系统调用
pub const SYSCALL_MUTEX_UNLOCK: usize = 466;
/// semaphore_create 系统调用
pub const SYSCALL_SEMAPHORE_CREATE: usize = 467;
/// semaphore_up 系统调用
pub const SYSCALL_SEMAPHORE_UP: usize = 468;
/// 启用死锁检测系统调用
pub const SYSCALL_ENABLE_DEADLOCK_DETECT: usize = 469;
/// semaphore_down 系统调用
pub const SYSCALL_SEMAPHORE_DOWN: usize = 470;
/// condvar_create 系统调用
pub const SYSCALL_CONDVAR_CREATE: usize = 471;
/// condvar_signal 系统调用
pub const SYSCALL_CONDVAR_SIGNAL: usize = 472;
/// condvar_wait 系统调用
pub const SYSCALL_CONDVAR_WAIT: usize = 473;

mod fs;
mod process;
pub(crate) mod stats;
mod sync;
mod thread;

use fs::*;
use process::*;
use sync::*;
use thread::*;

use crate::fs::Stat;
use crate::timer::get_time_ms;

/// 根据 `syscall_id` 及其参数处理系统调用异常
pub fn syscall(syscall_id: usize, args: [usize; 4]) -> isize {
    let start = get_time_ms();
    let result = match syscall_id {
        SYSCALL_DUP => sys_dup(args[0]),
        SYSCALL_LINKAT => sys_linkat(args[1] as *const u8, args[3] as *const u8),
        SYSCALL_UNLINKAT => sys_unlinkat(args[1] as *const u8),
        SYSCALL_OPENAT => sys_open(args[1] as *const u8, args[2] as u32),
        SYSCALL_CLOSE => sys_close(args[0]),
        SYSCALL_PIPE => sys_pipe(args[0] as *mut usize),
        SYSCALL_READ => sys_read(args[0], args[1] as *const u8, args[2]),
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_FSTAT => sys_fstat(args[0], args[1] as *mut Stat),
        SYSCALL_EXIT => {
            // sys_exit returns !, so record the elapsed time before we leave this scope.
            let elapsed = get_time_ms().saturating_sub(start);
            stats::record_syscall_cost(syscall_id, elapsed);
            sys_exit(args[0] as i32);
        }
        SYSCALL_SLEEP => sys_sleep(args[0]),
        SYSCALL_YIELD => sys_yield(),
        SYSCALL_GETPID => sys_getpid(),
        SYSCALL_GETTID => sys_gettid(),
        SYSCALL_FORK => sys_fork(),
        SYSCALL_EXEC => sys_exec(args[0] as *const u8, args[1] as *const usize),
        SYSCALL_WAITPID => sys_waitpid(args[0] as isize, args[1] as *mut i32),
        SYSCALL_GETTIMEOFDAY => sys_get_time(args[0] as *mut TimeVal, args[1]),
        SYSCALL_MMAP => sys_mmap(args[0], args[1], args[2]),
        SYSCALL_MUNMAP => sys_munmap(args[0], args[1]),
        SYSCALL_SET_PRIORITY => sys_set_priority(args[0] as isize),
        SYSCALL_SPAWN => sys_spawn(args[0] as *const u8),
        SYSCALL_THREAD_CREATE => sys_thread_create(args[0], args[1]),
        SYSCALL_WAITTID => sys_waittid(args[0]) as isize,
        SYSCALL_MUTEX_CREATE => sys_mutex_create(args[0] == 1),
        SYSCALL_MUTEX_LOCK => sys_mutex_lock(args[0]),
        SYSCALL_MUTEX_UNLOCK => sys_mutex_unlock(args[0]),
        SYSCALL_SEMAPHORE_CREATE => sys_semaphore_create(args[0]),
        SYSCALL_SEMAPHORE_UP => sys_semaphore_up(args[0]),
        SYSCALL_ENABLE_DEADLOCK_DETECT => sys_enable_deadlock_detect(args[0]),
        SYSCALL_SEMAPHORE_DOWN => sys_semaphore_down(args[0]),
        SYSCALL_CONDVAR_CREATE => sys_condvar_create(),
        SYSCALL_CONDVAR_SIGNAL => sys_condvar_signal(args[0]),
        SYSCALL_CONDVAR_WAIT => sys_condvar_wait(args[0], args[1]),
        SYSCALL_KILL => sys_kill(args[0], args[1] as u32),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    };
    let elapsed = get_time_ms().saturating_sub(start);
    stats::record_syscall_cost(syscall_id, elapsed);
    result
}
