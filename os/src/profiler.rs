//! 内核性能测量模块
//!
//! 提供简洁的性能测量功能，支持嵌套测量，用于分析内核启动时间和各个初始化阶段的耗时。

extern crate alloc;
use crate::sync::UPSafeCell;
use crate::timer::get_time_us;
use alloc::collections::BTreeMap;
use alloc::string::String;
use lazy_static::*;

/// 性能测量器
struct Profiler {
    active_timings: BTreeMap<String, usize>,
    timing_results: BTreeMap<String, usize>,
}

impl Profiler {
    pub fn new() -> Self {
        Self {
            active_timings: BTreeMap::new(),
            timing_results: BTreeMap::new(),
        }
    }
}

/// 早期测量器
struct EarlyProfiler {
    measurements: [(Option<&'static str>, usize, usize); 8],
    count: usize,
}

impl EarlyProfiler {
    pub fn new() -> Self {
        Self {
            measurements: [(None, 0, 0); 8],
            count: 0,
        }
    }
}

lazy_static! {
    /// 全局性能测量器实例
    static ref PROFILER: UPSafeCell<Profiler> = unsafe {
        UPSafeCell::new(Profiler::new())
    };

    /// 早期测量器实例
    static ref EARLY_PROFILER: UPSafeCell<EarlyProfiler> = unsafe {
        UPSafeCell::new(EarlyProfiler::new())
    };
}

/// 早期测量开始（在堆分配器初始化之前使用）
pub fn early_start_timing(name: &'static str) {
    let mut early = EARLY_PROFILER.exclusive_access();
    if early.count < early.measurements.len() {
        let current_count = early.count;
        early.measurements[current_count] = (Some(name), get_time_us(), 0);
    }
}

/// 早期测量结束（在堆分配器初始化之前使用）
pub fn early_end_timing(name: &'static str) {
    let mut early = EARLY_PROFILER.exclusive_access();
    // 找到对应的开始记录
    let current_count = early.count;
    for i in 0..current_count + 1 {
        if let (Some(recorded_name), start_time, 0) = early.measurements[i] {
            if recorded_name == name {
                let end_time = get_time_us();
                let duration = end_time - start_time;
                early.measurements[i] = (Some(name), start_time, duration);
                if i == current_count {
                    early.count += 1;
                }
                break;
            }
        }
    }
}

/// 初始化性能测量器
pub fn init_profiler() {
    let mut profiler = PROFILER.exclusive_access();
    let early = EARLY_PROFILER.exclusive_access();

    // 将早期测量结果添加到主结果中
    for i in 0..early.count {
        if let (Some(name), _, duration) = early.measurements[i] {
            if duration > 0 {
                profiler.timing_results.insert(String::from(name), duration);
            }
        }
    }
}

/// 开始测量
pub fn start_timing(name: &str) {
    let mut profiler = PROFILER.exclusive_access();
    let start_time = get_time_us();
    profiler
        .active_timings
        .insert(String::from(name), start_time);
}

/// 结束测量并记录
pub fn end_timing(name: &str) {
    let mut profiler = PROFILER.exclusive_access();
    if let Some(start_time) = profiler.active_timings.remove(name) {
        let end_time = get_time_us();
        let duration = end_time - start_time;
        profiler.timing_results.insert(String::from(name), duration);
    }
}

/// 输出所有测量结果
pub fn print_timing_results() {
    println!("[计时] 内核启动性能:");

    // 先输出早期测量结果
    let early = EARLY_PROFILER.exclusive_access();
    for i in 0..early.count {
        if let (Some(name), _, duration) = early.measurements[i] {
            if duration > 0 {
                println!("[计时] {:15} : {:>6} μs(微秒)", name, duration);
            }
        }
    }

    // 再输出正常测量结果
    let profiler = PROFILER.exclusive_access();
    for (name, duration) in &profiler.timing_results {
        // 跳过早期测量结果（避免重复）
        let mut is_early = false;
        for j in 0..early.count {
            if let (Some(early_name), _, _) = early.measurements[j] {
                if name == early_name {
                    is_early = true;
                    break;
                }
            }
        }

        if !is_early {
            println!("[计时] {:15} : {:>6} μs(微秒)", name, duration);
        }
    }

    println!("[计时] 测量完成");
}

/// 便捷宏：测量代码块执行时间
#[macro_export]
macro_rules! time_it {
    ($name:expr, $code:block) => {{
        $crate::profiler::start_timing($name);
        let result = $code;
        $crate::profiler::end_timing($name);
        result
    }};
}

/// 早期测量宏：在堆分配器初始化之前使用
#[macro_export]
macro_rules! early_time_it {
    ($name:expr, $code:block) => {{
        $crate::profiler::early_start_timing($name);
        let result = $code;
        $crate::profiler::early_end_timing($name);
        result
    }};
}
