//! 标准输入和标准输出 - 使用现代DBCN接口
use super::File;
use crate::mm::UserBuffer;
use crate::sbi::{console_read, console_write, console_write_byte};
use crate::task::suspend_current_and_run_next;

/// 写入缓冲区，失败时回退到逐字节写入
fn write_buffer_with_fallback(buffer: &[u8]) -> usize {
    let ret = console_write(buffer);
    let written = ret.value_or_zero();

    if written > 0 {
        written
    } else {
        // 批量写入失败，回退到逐字节写入
        buffer.iter().for_each(|&byte| {
            let _ = console_write_byte(byte);
        });
        buffer.len()
    }
}

/// 从控制台读取单个字符，阻塞直到有输入
fn read_char_blocking() -> u8 {
    loop {
        let mut kernel_buf = [0u8; 1];
        let ret = console_read(&mut kernel_buf);

        if ret.is_ok() && ret.value > 0 {
            return kernel_buf[0];
        } else {
            // 没有字符可读，使用WFI降低CPU占用并切换任务
            unsafe {
                core::arch::asm!("wfi", options(nomem, nostack));
            }
            suspend_current_and_run_next();
        }
    }
}

/// 从控制台获取字符的标准输入文件
pub struct Stdin;

/// 向控制台输出字符的标准输出文件
pub struct Stdout;

impl File for Stdin {
    fn readable(&self) -> bool {
        true
    }
    fn writable(&self) -> bool {
        false
    }
    fn read(&self, mut user_buf: UserBuffer) -> usize {
        assert_eq!(user_buf.len(), 1);

        let ch = read_char_blocking();
        unsafe {
            user_buf.buffers[0].as_mut_ptr().write_volatile(ch);
        }
        1
    }
    fn write(&self, _user_buf: UserBuffer) -> usize {
        panic!("Cannot write to stdin!");
    }
}

impl File for Stdout {
    fn readable(&self) -> bool {
        false
    }
    fn writable(&self) -> bool {
        true
    }
    fn read(&self, _user_buf: UserBuffer) -> usize {
        panic!("Cannot read from stdout!");
    }
    fn write(&self, user_buf: UserBuffer) -> usize {
        user_buf
            .buffers
            .iter()
            .map(|buffer| write_buffer_with_fallback(buffer))
            .sum()
    }
}
