//! 简单的性能测量模块，参考os/profiler.rs设计

use std::collections::BTreeMap;
use std::time::Instant;

/// 性能测量器
pub struct PackingProfiler {
    /// 活跃的测量（测量名称 -> 开始时间）
    active_timings: BTreeMap<String, Instant>,
    /// 完成的测量结果（测量名称 -> 耗时微秒）
    timing_results: BTreeMap<String, u64>,
}

impl PackingProfiler {
    /// 创建新的性能测量器
    pub fn new() -> Self {
        Self {
            active_timings: BTreeMap::new(),
            timing_results: BTreeMap::new(),
        }
    }

    /// 开始测量
    pub fn start_timing(&mut self, name: &str) {
        let start_time = Instant::now();
        self.active_timings.insert(name.to_string(), start_time);
    }

    /// 结束测量并记录
    pub fn end_timing(&mut self, name: &str) -> Option<u64> {
        if let Some(start_time) = self.active_timings.remove(name) {
            let duration = start_time.elapsed().as_micros() as u64;
            self.timing_results.insert(name.to_string(), duration);
            Some(duration)
        } else {
            None
        }
    }

    /// 输出性能测量结果（类似os风格）
    pub fn print_timing_results(&self) {
        println!("[计时] 文件系统打包性能:");

        for (name, duration) in &self.timing_results {
            println!("[计时] {:15} : {:>6} μs(微秒)", name, duration);
        }

        println!("[计时] 测量完成");
    }
}

/// 便捷宏：测量代码块执行时间（类似os::profiler的time_it!）
#[macro_export]
macro_rules! pack_time_it {
    ($profiler:expr, $name:expr, $code:block) => {{
        $profiler.start_timing($name);
        let result = $code;
        $profiler.end_timing($name);
        result
    }};
}
