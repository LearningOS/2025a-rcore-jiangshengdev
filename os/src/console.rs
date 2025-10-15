//! SBI 控制台驱动程序，用于文本输出
use crate::sbi::console_putchar;
use core::fmt::{self, Write};

struct Stdout;

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // 逐字节输出字符串到控制台
        for b in s.bytes() {
            console_putchar(b as usize);
        }
        Ok(())
    }
}

pub fn print(args: fmt::Arguments) {
    // 使用Stdout结构体格式化并输出参数
    Stdout.write_fmt(args).unwrap();
}

/// 使用格式字符串和参数打印到主机控制台。
#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!($fmt $(, $($arg)+)?))
    }
}

/// 使用格式字符串和参数打印一行到主机控制台。
#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?))
    }
}
