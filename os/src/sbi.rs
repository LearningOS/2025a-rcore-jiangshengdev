//! SBI 调用包装器

#![allow(unused)]

use core::arch::asm;

const SBI_SET_TIMER: usize = 0;
const SBI_CONSOLE_PUTCHAR: usize = 1;
const SBI_CONSOLE_GETCHAR: usize = 2;
const SBI_SHUTDOWN: usize = 8;

/// 通用 sbi 调用
#[inline(always)]
fn sbi_call(which: usize, arg0: usize, arg1: usize, arg2: usize) -> usize {
    let mut ret;
    unsafe {
        // 使用ecall指令调用SBI服务
        // x17寄存器存放SBI功能号，x10-x12存放参数
        asm!(
            "ecall",
            inlateout("x10") arg0 => ret,
            in("x11") arg1,
            in("x12") arg2,
            in("x16") 0,
            in("x17") which,
        );
    }
    ret
}

/// 使用 sbi 调用设置定时器
pub fn set_timer(timer: usize) {
    // 调用SBI设置定时器服务，参数为定时器触发时间
    sbi_call(SBI_SET_TIMER, timer, 0, 0);
}

/// 使用 sbi 调用在控制台输出字符（qemu uart 处理程序）
pub fn console_putchar(c: usize) {
    // 调用SBI控制台输出字符服务
    sbi_call(SBI_CONSOLE_PUTCHAR, c, 0, 0);
}

/// 使用 sbi 调用从控制台获取字符（qemu uart 处理程序）
pub fn console_getchar() -> usize {
    // 调用SBI控制台获取字符服务，返回字符或0（无字符可读）
    sbi_call(SBI_CONSOLE_GETCHAR, 0, 0, 0)
}

/// 使用 sbi 调用关闭内核
pub fn shutdown() -> ! {
    // 调用SBI关机服务
    sbi_call(SBI_SHUTDOWN, 0, 0, 0);
    panic!("It should shutdown!");
}
