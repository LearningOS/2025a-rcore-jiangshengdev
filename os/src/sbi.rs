//! SBI call wrappers

#![allow(unused)]

use core::arch::asm;

/// SBI call return payload following the v0.2 calling convention
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SbiRet {
    /// SBI error code (0 on success, negative on failure)
    pub error: isize,
    /// SBI return value for successful calls
    pub value: usize,
}

const SBI_SUCCESS: isize = 0;

const SBI_EXT_TIME: usize = 0x5449_4d45;
const SBI_EXT_DBCN: usize = 0x4442_434e;
const SBI_EXT_SRST: usize = 0x5352_5354;

const SBI_FID_SET_TIMER: usize = 0;
const SBI_FID_CONSOLE_READ: usize = 1;
const SBI_FID_CONSOLE_WRITE_BYTE: usize = 2;
const SBI_FID_SYSTEM_RESET: usize = 0;

const SBI_SRST_TYPE_SHUTDOWN: usize = 0;
const SBI_SRST_REASON_NO_REASON: usize = 0;

// Single-byte staging area for debug console reads.
static mut CONSOLE_READ_BUF: [u8; 1] = [0; 1];

/// Number of argument registers available for SBI calls (a0-a5)
const SBI_NUM_ARGS: usize = 6;

/// General SBI call entry that follows the v0.2+ calling convention
#[inline(always)]
fn sbi_call(extension_id: usize, function_id: usize, args: [usize; SBI_NUM_ARGS]) -> SbiRet {
    let [mut a0, mut a1, a2, a3, a4, a5] = args;
    unsafe {
        asm!(
            "ecall",
            inlateout("a0") a0,
            inlateout("a1") a1,
            in("a2") a2,
            in("a3") a3,
            in("a4") a4,
            in("a5") a5,
            in("a6") function_id,
            in("a7") extension_id,
            options(nostack),
        );
    }
    SbiRet {
        error: a0 as isize,
        value: a1,
    }
}

/// Use SBI TIME extension to set timer
pub fn set_timer(timer: usize) {
    let ret = sbi_call(SBI_EXT_TIME, SBI_FID_SET_TIMER, [timer, 0, 0, 0, 0, 0]);
    debug_assert_eq!(ret.error, SBI_SUCCESS);
}

/// Use SBI debug console extension to putchar in console (QEMU UART handler)
pub fn console_putchar(c: usize) {
    let ret = sbi_call(
        SBI_EXT_DBCN,
        SBI_FID_CONSOLE_WRITE_BYTE,
        [c & 0xff, 0, 0, 0, 0, 0],
    );
    debug_assert_eq!(ret.error, SBI_SUCCESS);
}

/// Use SBI debug console extension to getchar from console (QEMU UART handler)
pub fn console_getchar() -> usize {
    let ret = unsafe {
        let buf_addr = CONSOLE_READ_BUF.as_mut_ptr() as usize;
        sbi_call(
            SBI_EXT_DBCN,
            SBI_FID_CONSOLE_READ,
            [1, buf_addr, 0, 0, 0, 0],
        )
    };
    if ret.error != SBI_SUCCESS || ret.value == 0 {
        return 0;
    }
    unsafe { usize::from(CONSOLE_READ_BUF[0]) }
}

/// Use SBI system reset extension to shutdown the kernel
pub fn shutdown() -> ! {
    let ret = sbi_call(
        SBI_EXT_SRST,
        SBI_FID_SYSTEM_RESET,
        [
            SBI_SRST_TYPE_SHUTDOWN,
            SBI_SRST_REASON_NO_REASON,
            0,
            0,
            0,
            0,
        ],
    );
    if ret.error == SBI_SUCCESS {
        unreachable!("sbi_system_reset should not return on success");
    }
    panic!("sbi_system_reset failed: error {:#x}", ret.error);
}
