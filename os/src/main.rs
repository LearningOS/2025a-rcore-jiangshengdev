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

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate log;

extern crate alloc;

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
    extern "C" {
        fn sbss();
        fn ebss();
    }
    unsafe {
        core::slice::from_raw_parts_mut(sbss as usize as *mut u8, ebss as usize - sbss as usize)
            .fill(0);
    }
}

#[no_mangle]
/// 操作系统的 Rust 入口点
pub fn rust_main() -> ! {
    clear_bss();
    println!("[kernel] Hello, world!");
    logging::init();
    mm::init();
    mm::remap_test();
    trap::init();
    trap::enable_timer_interrupt();
    timer::set_next_trigger();
    fs::list_apps();
    task::add_initproc();
    task::run_tasks();
    panic!("Unreachable in rust_main!");
}
