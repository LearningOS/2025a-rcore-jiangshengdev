//! RISC-V 计时器相关功能

use core::cmp::Ordering;

use crate::config::CLOCK_FREQ;
use crate::sbi::set_timer;
use crate::sync::UPSafeCell;
use crate::task::{current_task, wakeup_task, TaskControlBlock};
use alloc::collections::BinaryHeap;
use alloc::sync::Arc;
use lazy_static::*;
use riscv::register::time;
/// 每秒的节拍数
const TICKS_PER_SEC: usize = 100;
/// 每秒的毫秒数
const MSEC_PER_SEC: usize = 1000;
/// 每秒的微秒数
#[allow(dead_code)]
const MICRO_PER_SEC: usize = 1_000_000;
/// 输出标签对齐宽度
const LABEL_WIDTH: usize = 28;

/// 以节拍为单位获取当前时间
pub fn get_time() -> usize {
    time::read()
}

/// 以毫秒为单位获取当前时间
pub fn get_time_ms() -> usize {
    time::read() * MSEC_PER_SEC / CLOCK_FREQ
}

/// 以微秒为单位获取当前时间
pub fn get_time_us() -> usize {
    time::read() * MICRO_PER_SEC / CLOCK_FREQ
}

/// 设置下一次定时器中断
pub fn set_next_trigger() {
    set_timer(get_time() + CLOCK_FREQ / TICKS_PER_SEC);
}

/// 定时器的条件变量
pub struct TimerCondVar {
    /// 定时器触发的毫秒时间戳
    pub expire_ms: usize,
    /// 定时器触发时要唤醒的任务
    pub task: Arc<TaskControlBlock>,
}

impl PartialEq for TimerCondVar {
    fn eq(&self, other: &Self) -> bool {
        self.expire_ms == other.expire_ms
    }
}
impl Eq for TimerCondVar {}
impl PartialOrd for TimerCondVar {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimerCondVar {
    fn cmp(&self, other: &Self) -> Ordering {
        other.expire_ms.cmp(&self.expire_ms)
    }
}

lazy_static! {
    /// TIMERS：定时器条件变量的全局集合
    static ref TIMERS: UPSafeCell<BinaryHeap<TimerCondVar>> =
        unsafe { UPSafeCell::new(BinaryHeap::<TimerCondVar>::new()) };
}

/// 添加定时器
pub fn add_timer(expire_ms: usize, task: Arc<TaskControlBlock>) {
    trace!(
        "kernel:pid[{}] add_timer",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    let mut timers = TIMERS.exclusive_access();
    timers.push(TimerCondVar { expire_ms, task });
}

/// 移除定时器
pub fn remove_timer(task: Arc<TaskControlBlock>) {
    //trace!("kernel:pid[{}] remove_timer", current_task().unwrap().process.upgrade().unwrap().getpid());
    trace!("kernel: remove_timer");
    let mut timers = TIMERS.exclusive_access();
    let mut temp = BinaryHeap::<TimerCondVar>::new();
    for condvar in timers.drain() {
        if Arc::as_ptr(&task) != Arc::as_ptr(&condvar.task) {
            temp.push(condvar);
        }
    }
    timers.clear();
    timers.append(&mut temp);
    trace!("kernel: remove_timer END");
}

/// 检查是否有定时器到期
pub fn check_timer() {
    trace!(
        "kernel:pid[{}] check_timer",
        current_task().unwrap().process.upgrade().unwrap().getpid()
    );
    let current_ms = get_time_ms();
    let mut timers = TIMERS.exclusive_access();
    while let Some(timer) = timers.peek() {
        if timer.expire_ms <= current_ms {
            wakeup_task(Arc::clone(&timer.task));
            timers.pop();
        } else {
            break;
        }
    }
}

/// 打印带标签的耗时信息
#[inline(always)]
pub fn report_duration(label: &str, duration_us: usize) {
    let duration_ms = duration_us / MSEC_PER_SEC;
    if duration_ms > 0 {
        println!(
            "[time]\t{:width$}\t{} us ({} ms)",
            label,
            duration_us,
            duration_ms,
            width = LABEL_WIDTH
        );
    } else {
        println!(
            "[time]\t{:width$}\t{} us",
            label,
            duration_us,
            width = LABEL_WIDTH
        );
    }
}

/// 记录当前时间点，便于无返回函数的追踪
#[inline(always)]
pub fn log_instant(label: &str) {
    let current_us = get_time_us();
    let current_ms = current_us / MSEC_PER_SEC;
    if current_ms > 0 {
        println!(
            "[time]\t{:width$}\tat {} us ({} ms)",
            label,
            current_us,
            current_ms,
            width = LABEL_WIDTH
        );
    } else {
        println!(
            "[time]\t{:width$}\tat {} us",
            label,
            current_us,
            width = LABEL_WIDTH
        );
    }
}

/// 为函数调用统计执行时间
#[macro_export]
macro_rules! time_call {
    ($label:expr, $expr:expr) => {{
        let __start = $crate::timer::get_time_us();
        let __result = { $expr };
        let __end = $crate::timer::get_time_us();
        $crate::timer::report_duration($label, __end.saturating_sub(__start));
        __result
    }};
}
