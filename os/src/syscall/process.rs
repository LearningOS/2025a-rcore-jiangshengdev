//! Process management syscalls
use crate::{
    config::PAGE_SIZE,
    mm::{PageTable, PTEFlags, VirtAddr},
    task::{
        change_program_brk, current_syscall_count, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next,
    },
    timer::get_time_us,
};
use core::{mem, slice};

#[repr(C)]
#[derive(Debug)]
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
    let bytes =
        unsafe { slice::from_raw_parts(&time_val as *const TimeVal as *const u8, mem::size_of::<TimeVal>()) };
    if write_user_bytes(current_user_token(), ts as usize, bytes) {
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
        0 => {
            let mut byte = [0u8; 1];
            if read_user_bytes(token, id, &mut byte) {
                byte[0] as isize
            } else {
                -1
            }
        }
        1 => {
            let byte = [data as u8];
            if write_user_bytes(token, id, &byte) {
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
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    -1
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    -1
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

fn read_user_bytes(token: usize, mut ptr: usize, buf: &mut [u8]) -> bool {
    if buf.is_empty() {
        return true;
    }
    let page_table = PageTable::from_token(token);
    let mut processed = 0;
    while processed < buf.len() {
        let va = VirtAddr::from(ptr);
        let vpn = va.floor();
        let offset = va.page_offset();
        let len = (PAGE_SIZE - offset).min(buf.len() - processed);
        let pte = match page_table.translate(vpn) {
            Some(pte) => pte,
            None => return false,
        };
        let flags = pte.flags();
        if !flags.contains(PTEFlags::U) || !flags.contains(PTEFlags::R) {
            return false;
        }
        let page_data = pte.ppn().get_bytes_array();
        buf[processed..processed + len]
            .copy_from_slice(&page_data[offset..offset + len]);
        processed += len;
        ptr += len;
    }
    true
}

fn write_user_bytes(token: usize, mut ptr: usize, buf: &[u8]) -> bool {
    if buf.is_empty() {
        return true;
    }
    let page_table = PageTable::from_token(token);
    let mut processed = 0;
    while processed < buf.len() {
        let va = VirtAddr::from(ptr);
        let vpn = va.floor();
        let offset = va.page_offset();
        let len = (PAGE_SIZE - offset).min(buf.len() - processed);
        let pte = match page_table.translate(vpn) {
            Some(pte) => pte,
            None => return false,
        };
        let flags = pte.flags();
        if !flags.contains(PTEFlags::U) || !flags.contains(PTEFlags::W) {
            return false;
        }
        let page_data = pte.ppn().get_bytes_array();
        page_data[offset..offset + len]
            .copy_from_slice(&buf[processed..processed + len]);
        processed += len;
        ptr += len;
    }
    true
}
