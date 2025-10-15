//! RISC-V 定时器相关功能

use crate::config::CLOCK_FREQ;
use crate::sbi::set_timer;
use riscv::register::time;
/// 每秒的时钟周期数
const TICKS_PER_SEC: usize = 100;
/// 每秒的毫秒数
const MSEC_PER_SEC: usize = 1000;
/// 每秒的微秒数
const MICRO_PER_SEC: usize = 1_000_000;

/// 获取当前时间（时钟周期）
pub fn get_time() -> usize {
    // 读取RISC-V的time寄存器获取当前时钟周期数
    time::read()
}

/// 获取当前时间（毫秒）
pub fn get_time_ms() -> usize {
    // 将时钟周期转换为毫秒
    time::read() * MSEC_PER_SEC / CLOCK_FREQ
}

/// 获取当前时间（微秒）
pub fn get_time_us() -> usize {
    // 将时钟周期转换为微秒
    time::read() * MICRO_PER_SEC / CLOCK_FREQ
}

/// 设置下一次定时器中断
pub fn set_next_trigger() {
    // 设置定时器在当前时间加上一个时间片后触发中断
    set_timer(get_time() + CLOCK_FREQ / TICKS_PER_SEC);
}
