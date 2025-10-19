//! 主模块和入口点
//!
//! 内核的各种功能都作为子模块实现。最重要的包括：
//!
//! - [`trap`]: 处理从用户空间切换到内核的所有情况
//! - [`task`]: 任务管理
//! - [`syscall`]: 系统调用处理和实现
//! - [`mm`]: 使用 SV39 的地址映射
//! - [`sync`]: 将静态数据结构包装在其中，以便我们能够在没有任何 `unsafe` 的情况下访问它。
//! - [`fs`]: 通过一些结构将用户与文件系统分离
//!
//! 操作系统也从这个模块开始。内核代码从 `entry.asm` 开始执行，
//! 之后调用 [`rust_main()`] 来初始化各种功能。（详见其源代码。）
//!
//! 然后我们调用 [`task::run_tasks()`] 并第一次进入用户空间。

#![deny(missing_docs)]
#![deny(warnings)]
#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

extern crate alloc;
#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate log;

#[macro_use]
mod console;
pub mod config;
pub mod drivers;
pub mod fs;
pub mod lang_items;
pub mod logging;
pub mod mm;
pub mod sbi;
pub mod sync;
pub mod syscall;
pub mod task;
pub mod timer;
pub mod trap;

use core::arch::global_asm;

global_asm!(include_str!("entry.asm"));
/// 清空 BSS 段
fn clear_bss() {
    // 声明外部符号，这些符号由链接器脚本定义
    // sbss: BSS段的起始地址
    // ebss: BSS段的结束地址
    extern "C" {
        fn sbss();
        fn ebss();
    }
    // 使用unsafe代码将BSS段内存区域清零
    // 这是必要的，因为BSS段包含未初始化的全局变量，需要被初始化为0
    unsafe {
        core::slice::from_raw_parts_mut(sbss as usize as *mut u8, ebss as usize - sbss as usize)
            .fill(0);
    }
}

#[no_mangle]
/// 操作系统的 Rust 入口点
pub fn rust_main() -> ! {
    // 清空BSS段，为内核运行准备干净的内存环境
    clear_bss();
    println!("[kernel] Hello, world!");

    // 初始化日志系统，用于内核调试和信息输出
    logging::init();

    // 初始化内存管理子系统
    mm::init();
    // 测试内存重映射功能是否正常工作
    mm::remap_test();

    // 初始化陷阱处理机制，处理异常和中断
    trap::init();
    // 启用定时器中断，用于任务调度
    trap::enable_timer_interrupt();
    // 设置下一次定时器中断的触发时间
    timer::set_next_trigger();

    // 列出可用的应用程序
    fs::list_apps();
    // 添加初始进程到任务队列
    task::add_initproc();
    // 开始运行任务调度器，进入多任务环境
    task::run_tasks();
    panic!("Unreachable in rust_main!");
}
