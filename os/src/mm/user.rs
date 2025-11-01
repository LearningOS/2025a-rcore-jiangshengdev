use super::{PTEFlags, PageTable, VirtAddr};
use crate::config::PAGE_SIZE;
use core::{mem, slice};

/// Copy bytes from user space into `buf`.
/// Returns `true` when the entire buffer is successfully filled, `false` otherwise.
pub fn read_user_bytes(token: usize, mut ptr: usize, buf: &mut [u8]) -> bool {
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
        let Some(pte) = page_table.translate(vpn) else {
            return false;
        };
        let flags = pte.flags();
        if !flags.contains(PTEFlags::U) || !flags.contains(PTEFlags::R) {
            return false;
        }
        let page_data = pte.ppn().get_bytes_array();
        buf[processed..processed + len].copy_from_slice(&page_data[offset..offset + len]);
        processed += len;
        ptr += len;
    }
    true
}

/// Copy bytes from kernel buffer into user space.
/// Returns `true` when the entire buffer is successfully written, `false` otherwise.
pub fn write_user_bytes(token: usize, mut ptr: usize, buf: &[u8]) -> bool {
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
        let Some(pte) = page_table.translate(vpn) else {
            return false;
        };
        let flags = pte.flags();
        if !flags.contains(PTEFlags::U) || !flags.contains(PTEFlags::W) {
            return false;
        }
        let page_data = pte.ppn().get_bytes_array();
        page_data[offset..offset + len].copy_from_slice(&buf[processed..processed + len]);
        processed += len;
        ptr += len;
    }
    true
}

/// Read a single byte from user space. Returns `None` if the address is invalid.
pub fn read_user_u8(token: usize, addr: usize) -> Option<u8> {
    let mut byte = [0u8; 1];
    if read_user_bytes(token, addr, &mut byte) {
        Some(byte[0])
    } else {
        None
    }
}

/// Write a single byte into user space.
pub fn write_user_u8(token: usize, addr: usize, value: u8) -> bool {
    write_user_bytes(token, addr, &[value])
}

/// Copy a plain-old-data value into user space.
pub fn write_user_value<T: Copy>(token: usize, ptr: *mut T, value: &T) -> bool {
    if ptr.is_null() {
        return false;
    }
    let size = mem::size_of::<T>();
    let bytes = unsafe { slice::from_raw_parts(value as *const T as *const u8, size) };
    write_user_bytes(token, ptr as usize, bytes)
}
