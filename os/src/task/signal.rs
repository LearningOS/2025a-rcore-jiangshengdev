//! 信号标志以及将信号标志转换为整数和字符串的工具函数

use bitflags::*;

bitflags! {
    /// 信号标志位
    pub struct SignalFlags: u32 {
        /// 中断
        const SIGINT    = 1 << 2;
        /// 非法指令
        const SIGILL    = 1 << 4;
        /// 异常终止
        const SIGABRT   = 1 << 6;
        /// 浮点异常
        const SIGFPE    = 1 << 8;
        /// 段错误
        const SIGSEGV   = 1 << 11;
    }
}

impl SignalFlags {
    /// 将信号标志转换为整数与字符串
    pub fn check_error(&self) -> Option<(i32, &'static str)> {
        if self.contains(Self::SIGINT) {
            Some((-2, "Killed, SIGINT=2"))
        } else if self.contains(Self::SIGILL) {
            Some((-4, "Illegal Instruction, SIGILL=4"))
        } else if self.contains(Self::SIGABRT) {
            Some((-6, "Aborted, SIGABRT=6"))
        } else if self.contains(Self::SIGFPE) {
            Some((-8, "Erroneous Arithmetic Operation, SIGFPE=8"))
        } else if self.contains(Self::SIGSEGV) {
            Some((-11, "Segmentation Fault, SIGSEGV=11"))
        } else {
            // warn!("[kernel] signalflags check_error  {:?}", self);
            None
        }
    }
}
