//! 标准输入和标准输出
use super::File;
use crate::mm::UserBuffer;
use crate::sbi::console_getchar;
use crate::task::suspend_current_and_run_next;

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
        // 从控制台读取一个字符
        let mut c: usize;
        loop {
            c = console_getchar();
            if c == 0 {
                // 没有字符可读，暂停当前任务等待输入
                suspend_current_and_run_next();
                continue;
            } else {
                break;
            }
        }
        let ch = c as u8;
        // 将读取的字符写入用户缓冲区
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
        // 遍历用户缓冲区的所有片段并输出到控制台
        for buffer in user_buf.buffers.iter() {
            print!("{}", core::str::from_utf8(buffer).unwrap());
        }
        user_buf.len()
    }
}
