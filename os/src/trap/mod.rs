//! 陷阱处理功能
//!
//! 对于 rCore，我们有一个单一的陷阱入口点，即 `__alltraps`。在
//! [`init()`] 初始化时，我们将 `stvec` CSR 设置为指向它。
//!
//! 所有陷阱都通过 `__alltraps` 处理，它在 `trap.S` 中定义。汇编
//! 语言代码只做足够的工作来恢复内核空间上下文，确保 Rust 代码安全
//! 运行，并将控制权转移给 [`trap_handler()`]。
//!
//! 然后它根据具体的异常类型调用不同的功能。例如，定时器中断触发
//! 任务抢占，系统调用转到 [`syscall()`]。

mod context;

use crate::config::{TRAMPOLINE, TRAP_CONTEXT_BASE};
use crate::syscall::syscall;
use crate::task::{
    check_signals_error_of_current, current_add_signal, current_trap_cx, current_user_token,
    exit_current_and_run_next, handle_signals, suspend_current_and_run_next, SignalFlags,
};
use crate::timer::set_next_trigger;
use core::arch::{asm, global_asm};
use riscv::register::{
    mtvec::TrapMode,
    scause::{self, Exception, Interrupt, Trap},
    sie, stval, stvec,
};

global_asm!(include_str!("trap.S"));

/// 初始化陷阱处理
pub fn init() {
    // 设置内核陷阱入口点
    set_kernel_trap_entry();
}

fn set_kernel_trap_entry() {
    unsafe {
        // 设置stvec寄存器指向内核陷阱处理函数
        stvec::write(trap_from_kernel as usize, TrapMode::Direct);
    }
}

fn set_user_trap_entry() {
    unsafe {
        // 设置stvec寄存器指向用户陷阱处理入口（跳板页面）
        stvec::write(TRAMPOLINE, TrapMode::Direct);
    }
}

/// 在监管者模式下启用定时器中断
pub fn enable_timer_interrupt() {
    unsafe {
        // 设置sie寄存器的STIE位，启用监管者定时器中断
        sie::set_stimer();
    }
}

/// 陷阱处理程序
#[no_mangle]
pub fn trap_handler() -> ! {
    // 设置内核陷阱入口点
    set_kernel_trap_entry();
    let scause = scause::read();
    let stval = stval::read();
    // trace!("into {:?}", scause.cause());
    // 根据陷阱原因进行分发处理
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            // 处理用户态系统调用
            let mut cx = current_trap_cx();
            // 跳转到ecall指令的下一条指令
            cx.sepc += 4;
            // 执行系统调用，参数从寄存器中获取
            let result = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12], cx.x[13]]);
            // 重新获取陷阱上下文（可能在sys_exec中被修改）
            cx = current_trap_cx();
            // 将系统调用返回值存储到a0寄存器
            cx.x[10] = result as usize;
        }
        // 处理各种内存访问异常
        Trap::Exception(Exception::StoreFault)
        | Trap::Exception(Exception::StorePageFault)
        | Trap::Exception(Exception::InstructionFault)
        | Trap::Exception(Exception::InstructionPageFault)
        | Trap::Exception(Exception::LoadFault)
        | Trap::Exception(Exception::LoadPageFault) => {
            error!(
                "[kernel] trap_handler:  {:?} in application, bad addr = {:#x}, bad instruction = {:#x}, kernel killed it.",
                scause.cause(),
                stval,
                current_trap_cx().sepc,
            );
            // 向当前进程发送段错误信号
            current_add_signal(SignalFlags::SIGSEGV);
        }
        // 处理非法指令异常
        Trap::Exception(Exception::IllegalInstruction) => {
            // 向当前进程发送非法指令信号
            current_add_signal(SignalFlags::SIGILL);
        }
        // 处理监管者定时器中断
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            // 设置下一次定时器中断
            set_next_trigger();
            // 暂停当前任务，进行任务调度
            suspend_current_and_run_next();
        }
        _ => {
            panic!(
                "Unsupported trap {:?}, stval = {:#x}!",
                scause.cause(),
                stval
            );
        }
    }
    // 处理待处理的信号
    // trace!("[kernel] trap_handler:: handle_signals");
    handle_signals();

    // 检查是否有错误信号需要退出进程
    if let Some((errno, msg)) = check_signals_error_of_current() {
        trace!("[kernel] trap_handler: .. check signals {}", msg);
        exit_current_and_run_next(errno);
    }
    // 返回用户空间
    trap_return();
}

#[no_mangle]
/// 返回用户空间
/// 在 TRAMPOLINE 页面中设置 __restore 汇编函数的新地址，
/// 设置寄存器 a0 = trap_cx_ptr，寄存器 a1 = 用户页表的物理地址，
/// 最后，跳转到 __restore 汇编函数的新地址
pub fn trap_return() -> ! {
    // 设置用户陷阱入口点
    set_user_trap_entry();
    let trap_cx_ptr = TRAP_CONTEXT_BASE;
    let user_satp = current_user_token();
    extern "C" {
        fn __alltraps();
        fn __restore();
    }
    // 计算__restore函数在跳板页面中的虚拟地址
    let restore_va = __restore as usize - __alltraps as usize + TRAMPOLINE;
    // trace!("[kernel] trap_return: ..返回之前");
    unsafe {
        asm!(
            "fence.i",           // 指令缓存同步
            "jr {restore_va}",   // 跳转到__restore函数
            restore_va = in(reg) restore_va,
            in("a0") trap_cx_ptr,  // 传递陷阱上下文指针
            in("a1") user_satp,    // 传递用户页表令牌
            options(noreturn)
        )
    }
}

#[no_mangle]
/// 处理来自内核的陷阱
/// 未实现：来自内核模式的陷阱/中断/异常
/// 待办：第9章：I/O 设备
pub fn trap_from_kernel() -> ! {
    use riscv::register::sepc;
    trace!("stval = {:#x}, sepc = {:#x}", stval::read(), sepc::read());
    panic!("a trap {:?} from kernel!", scause::read().cause());
}

pub use context::TrapContext;
