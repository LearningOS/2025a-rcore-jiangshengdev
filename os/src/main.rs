//! The main module and entrypoint
//!
//! Various facilities of the kernels are implemented as submodules. The most
//! important ones are:
//!
//! - [`trap`]: Handles all cases of switching from userspace to the kernel
//! - [`task`]: Task management
//! - [`syscall`]: System call handling and implementation
//! - [`mm`]: Address map using SV39
//! - [`sync`]: Wrap a static data structure inside it so that we are able to access it without any `unsafe`.
//! - [`fs`]: Separate user from file system with some structures
//!
//! The operating system also starts in this module. Kernel code starts
//! executing from `entry.asm`, after which [`rust_main()`] is called to
//! initialize various pieces of functionality. (See its source code for
//! details.)
//!
//! We then call [`task::run_tasks()`] and for the first time go to
//! userspace.

#![deny(missing_docs)]
#![deny(warnings)]
#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

#[macro_use]
extern crate log;

extern crate alloc;

#[macro_use]
extern crate bitflags;

#[path = "boards/qemu.rs"]
mod board;

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

/// A tiny no-op helper that temporarily borrows a value.
///
/// Use this in debugging to keep a variable "alive" (i.e., used)
/// until the call site, so debuggers can reliably inspect the
/// most recent writes before optimization elides intermediate states.
///
/// It takes a shared reference and passes it to `core::hint::black_box`
/// to discourage the compiler from optimizing the usage away.
#[inline(always)]
pub fn dbg_hold<T: ?Sized>(value: &T) {
    // Prevent aggressive optimization from eliminating the use.
    core::hint::black_box(value);
}

fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    unsafe {
        core::ptr::write_bytes(sbss as usize as *mut u8, 0, ebss as usize - sbss as usize);
    }
}

#[no_mangle]
/// the rust entry-point of os
pub fn rust_main() -> ! {
    time_call!("clear_bss", clear_bss());
    time_call!("boot_banner", println!("[kernel] Hello, world!"));
    time_call!("logging::init", logging::init());
    time_call!("mm::init", mm::init());
    time_call!("mm::remap_test", mm::remap_test());
    time_call!("trap::init", trap::init());
    time_call!(
        "trap::enable_timer_interrupt",
        trap::enable_timer_interrupt()
    );
    time_call!("timer::set_next_trigger", timer::set_next_trigger());
    time_call!("fs::list_apps", fs::list_apps());
    time_call!("task::add_initproc", task::add_initproc());
    timer::log_instant("task::run_tasks start");
    task::run_tasks();
    panic!("Unreachable in rust_main!");
}
