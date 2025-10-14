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
    set_kernel_trap_entry();
}

fn set_kernel_trap_entry() {
    unsafe {
        stvec::write(trap_from_kernel as usize, TrapMode::Direct);
    }
}

fn set_user_trap_entry() {
    unsafe {
        stvec::write(TRAMPOLINE, TrapMode::Direct);
    }
}

/// 在监管者模式下启用定时器中断
pub fn enable_timer_interrupt() {
    unsafe {
        sie::set_stimer();
    }
}

/// 陷阱处理程序
#[no_mangle]
pub fn trap_handler() -> ! {
    set_kernel_trap_entry();
    let scause = scause::read();
    let stval = stval::read();
    // trace!("into {:?}", scause.cause());
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            // 无论如何都跳转到下一条指令
            let mut cx = current_trap_cx();
            cx.sepc += 4;
            // 获取系统调用返回值
            let result = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12], cx.x[13]]);
            // cx 在 sys_exec 期间被改变，所以我们必须再次调用它
            cx = current_trap_cx();
            cx.x[10] = result as usize;
        }
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
            current_add_signal(SignalFlags::SIGSEGV);
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            current_add_signal(SignalFlags::SIGILL);
        }
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            set_next_trigger();
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
    // handle signals (handle the sent signal)
    // trace!("[kernel] trap_handler:: handle_signals");
    handle_signals();

    // check error signals (if error then exit)
    if let Some((errno, msg)) = check_signals_error_of_current() {
        trace!("[kernel] trap_handler: .. check signals {}", msg);
        exit_current_and_run_next(errno);
    }
    trap_return();
}

#[no_mangle]
/// 返回用户空间
/// 在 TRAMPOLINE 页面中设置 __restore 汇编函数的新地址，
/// 设置寄存器 a0 = trap_cx_ptr，寄存器 a1 = 用户页表的物理地址，
/// 最后，跳转到 __restore 汇编函数的新地址
pub fn trap_return() -> ! {
    set_user_trap_entry();
    let trap_cx_ptr = TRAP_CONTEXT_BASE;
    let user_satp = current_user_token();
    extern "C" {
        fn __alltraps();
        fn __restore();
    }
    let restore_va = __restore as usize - __alltraps as usize + TRAMPOLINE;
    // trace!("[kernel] trap_return: ..返回之前");
    unsafe {
        asm!(
            "fence.i",
            "jr {restore_va}",
            restore_va = in(reg) restore_va,
            in("a0") trap_cx_ptr,
            in("a1") user_satp,
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
