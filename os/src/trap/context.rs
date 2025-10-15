//! [`TrapContext`] 的实现
use riscv::register::sstatus::{self, Sstatus, SPP};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// 陷阱上下文结构，包含 sstatus、sepc 和寄存器
pub struct TrapContext {
    /// 通用寄存器 x0-31
    pub x: [usize; 32],
    /// 监管者状态寄存器
    pub sstatus: Sstatus,
    /// 监管者异常程序计数器
    pub sepc: usize,
    /// 内核地址空间的令牌
    pub kernel_satp: usize,
    /// 当前应用程序的内核栈指针
    pub kernel_sp: usize,
    /// 内核中陷阱处理程序入口点的虚拟地址
    pub trap_handler: usize,
}

impl TrapContext {
    /// 将 sp（栈指针）放入 TrapContext 的 x[2] 字段
    pub fn set_sp(&mut self, sp: usize) {
        // x[2]寄存器对应RISC-V的sp（栈指针）寄存器
        self.x[2] = sp;
    }
    /// 初始化应用程序的陷阱上下文
    pub fn app_init_context(
        entry: usize,
        sp: usize,
        kernel_satp: usize,
        kernel_sp: usize,
        trap_handler: usize,
    ) -> Self {
        let mut sstatus = sstatus::read();
        // 设置陷阱返回后的特权级为用户态
        sstatus.set_spp(SPP::User);
        let mut cx = Self {
            // 初始化所有通用寄存器为0
            x: [0; 32],
            sstatus,
            // 设置程序计数器为应用程序入口点
            sepc: entry,
            // 设置内核页表令牌
            kernel_satp,
            // 设置内核栈指针
            kernel_sp,
            // 设置陷阱处理函数地址
            trap_handler,
        };
        // 设置用户栈指针
        cx.set_sp(sp);
        // 返回初始化完成的陷阱上下文
        cx
    }
}
