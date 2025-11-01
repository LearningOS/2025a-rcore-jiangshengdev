//! Process management syscalls
use crate::{
    config::PAGE_SIZE,
    mm::{read_user_u8, write_user_u8, write_user_value, MapPermission},
    task::{
        change_program_brk, current_syscall_count, current_user_token, exit_current_and_run_next,
        mmap_current, munmap_current, suspend_current_and_run_next,
    },
    timer::get_time_us,
};
use bitflags::bitflags;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    if ts.is_null() {
        return -1;
    }
    let us = get_time_us();
    let time_val = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    if write_user_value(current_user_token(), ts, &time_val) {
        0
    } else {
        -1
    }
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    let token = current_user_token();
    match trace_request {
        0 => read_user_u8(token, id)
            .map(|byte| byte as isize)
            .unwrap_or(-1),
        1 => {
            if write_user_u8(token, id, data as u8) {
                0
            } else {
                -1
            }
        }
        2 => current_syscall_count(id)
            .map(|count| count as isize)
            .unwrap_or(-1),
        _ => -1,
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    if !is_page_aligned(start) {
        return -1;
    }
    let Some(len_aligned) = align_len(len) else {
        return -1;
    };
    let Some(perm) = prot_to_perm(prot) else {
        return -1;
    };
    if len_aligned == 0 {
        return 0;
    }
    if start.checked_add(len_aligned).is_none() {
        return -1;
    }
    mmap_current(start, len_aligned, perm).map_or(-1, |_| 0)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if !is_page_aligned(start) {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    if !is_page_aligned(len) {
        return -1;
    }
    if start.checked_add(len).is_none() {
        return -1;
    }
    munmap_current(start, len).map_or(-1, |_| 0)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

fn is_page_aligned(addr: usize) -> bool {
    addr % PAGE_SIZE == 0
}

fn align_len(len: usize) -> Option<usize> {
    if len == 0 {
        Some(0)
    } else {
        let pages = ((len - 1) / PAGE_SIZE).checked_add(1)?;
        pages.checked_mul(PAGE_SIZE)
    }
}

fn prot_to_perm(prot: usize) -> Option<MapPermission> {
    let flags = ProtFlags::from_bits(prot)?;
    if flags.is_empty() {
        return None;
    }
    let mut perm = MapPermission::U;
    if flags.contains(ProtFlags::READ) {
        perm |= MapPermission::R;
    }
    if flags.contains(ProtFlags::WRITE) {
        perm |= MapPermission::W;
    }
    if flags.contains(ProtFlags::EXECUTE) {
        perm |= MapPermission::X;
    }
    Some(perm)
}

bitflags! {
    struct ProtFlags: usize {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
    }
}
