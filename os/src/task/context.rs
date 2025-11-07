//! [`TaskContext`] 的实现
use crate::trap::trap_return;

#[repr(C)]
/// 任务上下文结构，保存若干寄存器
pub struct TaskContext {
    /// 任务切换后的返回地址
    ra: usize,
    /// 栈指针
    sp: usize,
    /// s0-s11 寄存器（被调用者保存）
    s: [usize; 12],
}

impl TaskContext {
    /// 创建一个空的任务上下文
    pub fn zero_init() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s: [0; 12],
        }
    }
    /// 使用 trap 返回地址与内核栈指针创建任务上下文
    pub fn goto_trap_return(kstack_ptr: usize) -> Self {
        Self {
            ra: trap_return as usize,
            sp: kstack_ptr,
            s: [0; 12],
        }
    }
}
