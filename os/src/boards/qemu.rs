//参考:: https://github.com/andre-richter/qemu-exit
use core::arch::asm;

const EXIT_SUCCESS: u32 = 0x5555; // 等同于 `exit(0)`。qemu 成功退出

const EXIT_FAILURE_FLAG: u32 = 0x3333;
const EXIT_FAILURE: u32 = exit_code_encode(1); // 等同于 `exit(1)`。qemu 失败退出
const EXIT_RESET: u32 = 0x7777; // qemu 重置

pub trait QEMUExit {
    /// 使用指定的返回码退出。
    ///
    /// 注意：对于 `X86`，代码在 QEMU 内部会与 `0x1` 进行二进制或运算。
    fn exit(&self, code: u32) -> !;

    /// 如果可能，使用 `EXIT_SUCCESS`（即 `0`）退出 QEMU。
    ///
    /// 注意：对于 `X86` 不可用。
    fn exit_success(&self) -> !;

    /// 使用 `EXIT_FAILURE`（即 `1`）退出 QEMU。
    fn exit_failure(&self) -> !;
}

/// RISCV64 配置
pub struct RISCV64 {
    /// sifive_test 映射设备的地址。
    addr: u64,
}

/// 使用 EXIT_FAILURE_FLAG 编码退出代码。
const fn exit_code_encode(code: u32) -> u32 {
    (code << 16) | EXIT_FAILURE_FLAG
}

impl RISCV64 {
    /// 创建一个实例。
    pub const fn new(addr: u64) -> Self {
        // 创建RISCV64退出处理器实例，指定sifive_test设备地址
        RISCV64 { addr }
    }
}

impl QEMUExit for RISCV64 {
    /// 使用指定的退出代码退出 qemu。
    fn exit(&self, code: u32) -> ! {
        // 对非特殊退出代码进行编码处理
        let code_new = match code {
            EXIT_SUCCESS | EXIT_FAILURE | EXIT_RESET => code,
            _ => exit_code_encode(code),
        };

        unsafe {
            // 向sifive_test设备写入退出代码
            asm!(
                "sw {0}, 0({1})",
                in(reg)code_new, in(reg)self.addr
            );

            // 如果QEMU退出失败，进入无限循环等待
            // 在这里调用 `panic!()` 是不可行的，因为很有可能
            // 这个函数本身就是 `panic!()` 处理程序中的最后一个表达式。
            // 这可以防止可能的无限循环。
            loop {
                asm!("wfi", options(nomem, nostack));
            }
        }
    }

    fn exit_success(&self) -> ! {
        // 以成功状态退出QEMU
        self.exit(EXIT_SUCCESS);
    }

    fn exit_failure(&self) -> ! {
        // 以失败状态退出QEMU
        self.exit(EXIT_FAILURE);
    }
}

const VIRT_TEST: u64 = 0x100000;

pub const QEMU_EXIT_HANDLE: RISCV64 = RISCV64::new(VIRT_TEST);
