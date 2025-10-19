use bitflags::*;

/// 最大信号编号
///
/// 定义了系统支持的最大信号编号。UNIX系统通常支持31个信号，
/// 编号从1到31。信号0是保留的，不作为实际信号使用。
pub const MAX_SIG: usize = 31;

bitflags! {
    /// 信号标志位集合
    ///
    /// 使用位标志实现信号集合，每个位代表一个信号。
    /// 这种设计允许高效的信号集合操作，如并集、交集、差集等。
    ///
    /// 信号分类：
    /// - 程序错误信号：SIGILL, SIGSEGV, SIGFPE等
    /// - 作业控制信号：SIGSTOP, SIGCONT, SIGTSTP等
    /// - 终端信号：SIGINT, SIGQUIT, SIGHUP等
    /// - 用户定义信号：SIGUSR1, SIGUSR2
    /// - 系统信号：SIGKILL, SIGTERM, SIGABRT等
    pub struct SignalFlags: u32 {
        /// 默认信号处理
        const SIGDEF = 1;
        /// 挂起信号 - 终端连接断开
        const SIGHUP = 1 << 1;
        /// 中断信号 - 通常由Ctrl+C产生
        const SIGINT = 1 << 2;
        /// 退出信号 - 通常由Ctrl+\产生
        const SIGQUIT = 1 << 3;
        /// 非法指令信号 - 执行了无效的机器指令
        const SIGILL = 1 << 4;
        /// 跟踪/断点陷阱信号 - 调试器使用
        const SIGTRAP = 1 << 5;
        /// 中止信号 - 程序异常终止
        const SIGABRT = 1 << 6;
        /// 总线错误信号 - 内存访问错误
        const SIGBUS = 1 << 7;
        /// 浮点异常信号 - 浮点运算错误
        const SIGFPE = 1 << 8;
        /// 强制终止信号 - 不可捕获和忽略
        const SIGKILL = 1 << 9;
        /// 用户自定义信号1 - 应用程序可自由使用
        const SIGUSR1 = 1 << 10;
        /// 段错误信号 - 内存访问违规
        const SIGSEGV = 1 << 11;
        /// 用户自定义信号2 - 应用程序可自由使用
        const SIGUSR2 = 1 << 12;
        /// 管道破裂信号 - 写入已关闭的管道
        const SIGPIPE = 1 << 13;
        /// 闹钟信号 - 定时器到期
        const SIGALRM = 1 << 14;
        /// 终止信号 - 请求程序正常终止
        const SIGTERM = 1 << 15;
        /// 栈错误信号 - 栈溢出或其他栈相关错误
        const SIGSTKFLT = 1 << 16;
        /// 子进程状态改变信号 - 子进程停止或终止
        const SIGCHLD = 1 << 17;
        /// 继续执行信号 - 恢复被停止的进程
        const SIGCONT = 1 << 18;
        /// 停止信号 - 不可阻塞的进程停止
        const SIGSTOP = 1 << 19;
        /// 终端停止信号 - 通常由Ctrl+Z产生
        const SIGTSTP = 1 << 20;
        /// 后台进程读终端信号 - 后台进程尝试读取终端
        const SIGTTIN = 1 << 21;
        /// 后台进程写终端信号 - 后台进程尝试写入终端
        const SIGTTOU = 1 << 22;
        /// 套接字紧急数据信号 - 套接字上有紧急数据
        const SIGURG = 1 << 23;
        /// CPU时间限制超出信号 - 进程CPU使用时间超限
        const SIGXCPU = 1 << 24;
        /// 文件大小限制超出信号 - 文件大小超出限制
        const SIGXFSZ = 1 << 25;
        /// 虚拟定时器到期信号 - 虚拟时间定时器到期
        const SIGVTALRM = 1 << 26;
        /// 性能分析定时器到期信号 - 用于程序性能分析
        const SIGPROF = 1 << 27;
        /// 窗口大小改变信号 - 终端窗口大小发生变化
        const SIGWINCH = 1 << 28;
        /// I/O可用信号 - 文件描述符可进行I/O操作
        const SIGIO = 1 << 29;
        /// 电源故障信号 - 系统电源出现问题
        const SIGPWR = 1 << 30;
        /// 错误系统调用信号 - 执行了无效的系统调用
        const SIGSYS = 1 << 31;
    }
}

impl SignalFlags {
    /// 检查信号标志中是否包含错误信号
    ///
    /// 错误信号是指那些通常导致进程异常终止的信号。
    /// 这个函数用于在信号处理过程中检测是否有致命错误发生。
    ///
    /// 检查的错误信号类型：
    /// - SIGINT：中断信号，通常由用户按Ctrl+C产生
    /// - SIGILL：非法指令，程序执行了无效的机器指令
    /// - SIGABRT：中止信号，程序调用abort()函数
    /// - SIGFPE：浮点异常，除零或其他算术错误
    /// - SIGKILL：强制终止，不可捕获的终止信号
    /// - SIGSEGV：段错误，内存访问违规
    ///
    /// 返回值：
    /// - Some((错误码, 错误描述))：如果检测到错误信号
    /// - None：如果没有检测到错误信号
    ///
    /// 错误码采用负数，遵循UNIX约定：
    /// - 正数：正常退出码
    /// - 负数：信号导致的异常退出
    pub fn check_error(&self) -> Option<(i32, &'static str)> {
        // 按优先级检查各种错误信号
        if self.contains(Self::SIGINT) {
            Some((-2, "Killed, SIGINT=2"))
        } else if self.contains(Self::SIGILL) {
            Some((-4, "Illegal Instruction, SIGILL=4"))
        } else if self.contains(Self::SIGABRT) {
            Some((-6, "Aborted, SIGABRT=6"))
        } else if self.contains(Self::SIGFPE) {
            Some((-8, "Erroneous Arithmetic Operation, SIGFPE=8"))
        } else if self.contains(Self::SIGKILL) {
            Some((-9, "Killed, SIGKILL=9"))
        } else if self.contains(Self::SIGSEGV) {
            Some((-11, "Segmentation Fault, SIGSEGV=11"))
        } else {
            // 没有检测到错误信号
            // warn!("[kernel] signalflags check_error  {:?}", self);
            None
        }
    }
}
