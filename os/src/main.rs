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
pub mod profiler;
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
    // 测量 BSS 段清零时间（单独统计）
    let bss_start = timer::get_time_us();
    clear_bss();
    let bss_end = timer::get_time_us();
    let bss_duration = bss_end - bss_start;

    println!("[kernel] Hello, world!");
    println!("[计时] bss_clear       : {:>6} μs(微秒)", bss_duration);

    // 测量日志系统初始化时间（早期测量）
    early_time_it!("logging_init", {
        logging::init();
    });

    // 测量内存管理初始化时间（早期测量）
    early_time_it!("mm_init", {
        mm::init();
    });

    // 初始化性能测量器（需要堆分配，所以放在 mm::init 之后）
    profiler::init_profiler();

    // 测量内存重映射测试时间
    time_it!("mm_remap_test", {
        mm::remap_test();
    });

    // // 临时验证块设备读写路径，确保新版 virtio 驱动正常
    // drivers::block::block_device_test();
    //
    // // 执行内核态文件系统自检，验证 EasyFileSystem 读写路径
    // fs::run_internal_fs_test();

    // 测量陷阱处理机制初始化时间
    time_it!("trap_init", {
        trap::init();
    });

    // 测量定时器中断启用时间
    time_it!("timer_enable", {
        trap::enable_timer_interrupt();
    });

    // 测量定时器触发设置时间
    time_it!("timer_trigger", {
        timer::set_next_trigger();
    });

    // 测量文件系统应用列表时间
    time_it!("fs_list_apps", {
        fs::list_apps();
    });

    println!("[kernel] Finished listing apps, preparing to add initproc");

    // 测量初始进程添加时间
    time_it!("task_initproc", {
        task::add_initproc();
    });

    println!("[kernel] Added initproc, entering scheduler");

    // 输出性能测量结果
    profiler::print_timing_results();

    // 开始运行任务调度器，进入多任务环境
    task::run_tasks();
    panic!("Unreachable in rust_main!");
}
