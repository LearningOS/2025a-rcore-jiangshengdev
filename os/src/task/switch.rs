//! 提供用于在两个任务上下文之间切换的 __switch 汇编函数 [`TaskContext`]
use super::TaskContext;
use core::arch::global_asm;

global_asm!(include_str!("switch.S"));

extern "C" {
    /// 切换到 `next_task_cx_ptr` 指向的上下文，并将当前上下文保存到 `current_task_cx_ptr`。
    pub fn __switch(current_task_cx_ptr: *mut TaskContext, next_task_cx_ptr: *const TaskContext);
}
