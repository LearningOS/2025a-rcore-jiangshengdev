//! 现代SBI调用包装器
//!
//! 支持以下现代SBI扩展：
//! - Timer Extension (TIME) - 定时器管理
//! - System Reset Extension (SRST) - 系统重置
//! - Debug Console Extension (DBCN) - 调试控制台

#![allow(unused)]

use crate::mm::{kernel_token, PageTable, PhysAddr, VirtAddr};
use core::arch::asm;

// 现代SBI扩展常量
// Timer Extension (TIME) - EID #0x54494D45 "TIME"
const SBI_EXT_TIME: usize = 0x54494D45;
const SBI_TIME_SET_TIMER: usize = 0x0;

// System Reset Extension (SRST) - EID #0x53525354 "SRST"
const SBI_EXT_SRST: usize = 0x53525354;
const SBI_SRST_SYSTEM_RESET: usize = 0x0;

// DBCN (Debug Console) 扩展 - 现代SBI接口
const SBI_EXT_DBCN: usize = 0x4442434E; // "DBCN"
const SBI_DBCN_CONSOLE_WRITE: usize = 0x0;
const SBI_DBCN_CONSOLE_READ: usize = 0x1;
const SBI_DBCN_CONSOLE_WRITE_BYTE: usize = 0x2;

// System Reset 类型常量
const SBI_SRST_RESET_TYPE_SHUTDOWN: usize = 0x00000000;
const SBI_SRST_RESET_TYPE_COLD_REBOOT: usize = 0x00000001;
const SBI_SRST_RESET_TYPE_WARM_REBOOT: usize = 0x00000002;

// System Reset 原因常量
const SBI_SRST_RESET_REASON_NO_REASON: usize = 0x00000000;
const SBI_SRST_RESET_REASON_SYSTEM_FAILURE: usize = 0x00000001;

/// SBI 返回值结构
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SbiRet {
    /// 错误码，0表示成功
    pub error: isize,
    /// 返回值
    pub value: usize,
}

impl SbiRet {
    /// 创建成功的返回值
    pub const fn success(value: usize) -> Self {
        Self { error: 0, value }
    }

    /// 创建失败的返回值
    pub const fn error(error: isize) -> Self {
        Self { error, value: 0 }
    }

    /// 检查调用是否成功
    pub fn is_ok(&self) -> bool {
        self.error == 0
    }

    /// 检查调用是否失败
    pub fn is_err(&self) -> bool {
        self.error != 0
    }

    /// 获取成功时的值，失败时返回0
    pub fn value_or_zero(&self) -> usize {
        if self.is_ok() {
            self.value
        } else {
            0
        }
    }

    /// 获取成功时的值，失败时返回默认值
    pub fn value_or(&self, default: usize) -> usize {
        if self.is_ok() {
            self.value
        } else {
            default
        }
    }

    /// 如果失败则panic，成功则返回值
    pub fn expect(self, msg: &str) -> usize {
        if self.is_ok() {
            self.value
        } else {
            panic!("{}: SBI error {}", msg, self.error);
        }
    }
}

/// 现代 SBI 调用 (返回 SbiRet 结构)
#[inline(always)]
fn sbi_call(eid: usize, fid: usize, arg0: usize, arg1: usize, arg2: usize) -> SbiRet {
    let error: isize;
    let value: usize;
    unsafe {
        asm!(
            "ecall",
            in("a0") arg0,
            in("a1") arg1,
            in("a2") arg2,
            in("a6") fid,
            in("a7") eid,
            lateout("a0") error,
            lateout("a1") value,
            options(nostack)
        );
    }
    SbiRet { error, value }
}

/// 将虚拟地址转换为物理地址，复用现有的地址转换逻辑
#[inline]
fn translate_vaddr(vaddr: VirtAddr) -> Option<PhysAddr> {
    let page_table = PageTable::from_token(kernel_token());
    page_table.translate_va(vaddr)
}

/// 将物理地址分解为高低32位，用于SBI调用
#[inline]
fn split_paddr(paddr: PhysAddr) -> (usize, usize) {
    let paddr_val = paddr.0;
    (paddr_val & 0xFFFFFFFF, (paddr_val >> 32) & 0xFFFFFFFF)
}

/// 通用的DBCN缓冲区操作函数
/// 封装地址转换和SBI调用的通用逻辑
#[inline]
fn dbcn_buffer_operation(buffer_ptr: *const u8, len: usize, fid: usize) -> SbiRet {
    if len == 0 {
        return SbiRet::success(0);
    }

    let vaddr = VirtAddr::from(buffer_ptr as usize);

    if let Some(paddr) = translate_vaddr(vaddr) {
        let (base_addr_lo, base_addr_hi) = split_paddr(paddr);
        sbi_call(SBI_EXT_DBCN, fid, len, base_addr_lo, base_addr_hi)
    } else {
        SbiRet::error(-1)
    }
}

/// 使用现代Timer扩展设置定时器
pub fn set_timer(stime_value: u64) -> SbiRet {
    sbi_call(SBI_EXT_TIME, SBI_TIME_SET_TIMER, stime_value as usize, 0, 0)
}

/// 清除定时器中断（设置为无限远的未来）
pub fn clear_timer() -> SbiRet {
    set_timer(u64::MAX)
}

/// 现代控制台写入 (使用DBCN扩展)
pub fn console_write(data: &[u8]) -> SbiRet {
    dbcn_buffer_operation(data.as_ptr(), data.len(), SBI_DBCN_CONSOLE_WRITE)
}

/// 现代控制台读取 (使用DBCN扩展，非阻塞)
pub fn console_read(buffer: &mut [u8]) -> SbiRet {
    dbcn_buffer_operation(buffer.as_mut_ptr(), buffer.len(), SBI_DBCN_CONSOLE_READ)
}

/// 现代控制台单字节写入
pub fn console_write_byte(byte: u8) -> SbiRet {
    sbi_call(
        SBI_EXT_DBCN,
        SBI_DBCN_CONSOLE_WRITE_BYTE,
        byte as usize,
        0,
        0,
    )
}

/// 输出字符 (兼容旧接口)
pub fn console_putchar(c: usize) {
    let _ = console_write_byte(c as u8);
}

/// 获取字符 (兼容旧接口，非阻塞)
pub fn console_getchar() -> usize {
    let mut buffer = [0u8; 1];
    let ret = console_read(&mut buffer);

    if ret.is_ok() && ret.value > 0 {
        buffer[0] as usize
    } else {
        0
    }
}

/// 系统重置 - 使用现代SRST扩展
pub fn system_reset(reset_type: usize, reset_reason: usize) -> SbiRet {
    sbi_call(
        SBI_EXT_SRST,
        SBI_SRST_SYSTEM_RESET,
        reset_type,
        reset_reason,
        0,
    )
}

/// 系统关机
pub fn shutdown() -> ! {
    let _ = system_reset(
        SBI_SRST_RESET_TYPE_SHUTDOWN,
        SBI_SRST_RESET_REASON_NO_REASON,
    );
    panic!("System shutdown failed!");
}

/// 系统冷重启
pub fn cold_reboot() -> ! {
    let _ = system_reset(
        SBI_SRST_RESET_TYPE_COLD_REBOOT,
        SBI_SRST_RESET_REASON_NO_REASON,
    );
    panic!("System cold reboot failed!");
}

/// 系统热重启
pub fn warm_reboot() -> ! {
    let _ = system_reset(
        SBI_SRST_RESET_TYPE_WARM_REBOOT,
        SBI_SRST_RESET_REASON_NO_REASON,
    );
    panic!("System warm reboot failed!");
}
