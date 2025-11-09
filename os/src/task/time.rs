use super::{processor::current_task, TaskControlBlock};
use crate::sync::UPSafeCell;
use crate::timer::{get_time_ms, perf_enabled};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use lazy_static::*;

struct StopWatch {
    last_time_ms: usize,
    initialized: bool,
}

impl StopWatch {
    const fn new() -> Self {
        Self {
            last_time_ms: 0,
            initialized: false,
        }
    }

    fn refresh(&mut self) -> usize {
        let now = get_time_ms();
        if !self.initialized {
            self.initialized = true;
            self.last_time_ms = now;
            return 0;
        }
        let diff = now.saturating_sub(self.last_time_ms);
        self.last_time_ms = now;
        diff
    }

    fn mark(&mut self) {
        self.last_time_ms = get_time_ms();
        self.initialized = true;
    }
}

#[derive(Default)]
struct ProgramStats {
    total_user_ms: usize,
    total_kernel_ms: usize,
    runs: usize,
}

lazy_static! {
    static ref STOP_WATCH: UPSafeCell<StopWatch> = unsafe { UPSafeCell::new(StopWatch::new()) };
    static ref PROGRAM_TIME: UPSafeCell<BTreeMap<String, ProgramStats>> =
        unsafe { UPSafeCell::new(BTreeMap::new()) };
}

fn refresh_stop_watch() -> usize {
    STOP_WATCH.exclusive_access().refresh()
}

fn mark_stop_watch() {
    STOP_WATCH.exclusive_access().mark();
}

fn accumulate_task_user_time(task: &Arc<TaskControlBlock>, diff: usize) {
    let mut task_inner = task.inner_exclusive_access();
    task_inner.user_time_ms += diff;
    drop(task_inner);
    if let Some(process) = task.process.upgrade() {
        let mut process_inner = process.inner_exclusive_access();
        process_inner.user_time_ms += diff;
    }
}

fn accumulate_task_kernel_time(task: &Arc<TaskControlBlock>, diff: usize) {
    let mut task_inner = task.inner_exclusive_access();
    task_inner.kernel_time_ms += diff;
    drop(task_inner);
    if let Some(process) = task.process.upgrade() {
        let mut process_inner = process.inner_exclusive_access();
        process_inner.kernel_time_ms += diff;
    }
}

/// 初始化停表，需在调度循环启动前调用。
pub fn init() {
    if perf_enabled() {
        mark_stop_watch();
    }
}

/// 用户态陷入内核时调用，累计当前任务的用户态时间。
pub fn user_time_end() {
    if !perf_enabled() {
        return;
    }
    let diff = refresh_stop_watch();
    if diff == 0 {
        return;
    }
    if let Some(task) = current_task() {
        accumulate_task_user_time(&task, diff);
    } else {
        mark_stop_watch();
    }
}

/// 返回用户态前调用，累计当前任务的内核态时间。
pub fn user_time_start() {
    if !perf_enabled() {
        return;
    }
    let diff = refresh_stop_watch();
    if diff == 0 {
        return;
    }
    if let Some(task) = current_task() {
        accumulate_task_kernel_time(&task, diff);
    } else {
        mark_stop_watch();
    }
}

/// 任务在内核中被切换出去时调用，统计最近一次内核段。
pub fn record_kernel_time_for(task: &Arc<TaskControlBlock>) {
    if !perf_enabled() {
        return;
    }
    let diff = refresh_stop_watch();
    if diff == 0 {
        return;
    }
    accumulate_task_kernel_time(task, diff);
}

/// 新任务即将在内核态恢复执行时调用，避免将调度间隙计入其他任务。
pub fn on_task_switch_in(_task: &Arc<TaskControlBlock>) {
    if perf_enabled() {
        mark_stop_watch();
    }
}

/// 调度器暂时没有任务可运行时调用，重置停表避免空闲时间计入下一次统计。
pub fn on_idle() {
    if perf_enabled() {
        mark_stop_watch();
    }
}

/// 记录单个进程（按程序名称）的累计时间。
pub fn accumulate_program_time(name: &str, user_ms: usize, kernel_ms: usize) {
    if !perf_enabled() {
        return;
    }
    if user_ms == 0 && kernel_ms == 0 {
        return;
    }
    let mut summary = PROGRAM_TIME.exclusive_access();
    let stats = summary.entry(String::from(name)).or_default();
    stats.total_user_ms += user_ms;
    stats.total_kernel_ms += kernel_ms;
    stats.runs += 1;
}

/// 系统关闭前输出所有程序名称的时间汇总。
pub fn report_program_summary() {
    if !perf_enabled() {
        return;
    }
    let summary = PROGRAM_TIME.exclusive_access();
    if summary.is_empty() {
        println!("[time]\tprogram_time_summary\tno_records");
        return;
    }
    let mut rows: Vec<(&String, &ProgramStats)> = summary.iter().collect();
    let mut name_width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    name_width = name_width.max("program".len());
    rows.sort_by(|a, b| {
        let total_a = a.1.total_user_ms + a.1.total_kernel_ms;
        let total_b = b.1.total_user_ms + b.1.total_kernel_ms;
        total_b.cmp(&total_a)
    });
    let mut total_runs = 0usize;
    let mut total_user = 0usize;
    let mut total_kernel = 0usize;
    println!("[time]\tprogram_time_summary");
    println!(
        "[time]\t{:name_width$}\t{:>4}\t{:>8}\t{:>9}\t{:>8}",
        "program",
        "runs",
        "user_ms",
        "kernel_ms",
        "total_ms",
        name_width = name_width
    );
    for (name, stats) in rows.into_iter() {
        let total_ms = stats.total_user_ms + stats.total_kernel_ms;
        total_runs += stats.runs;
        total_user += stats.total_user_ms;
        total_kernel += stats.total_kernel_ms;
        println!(
            "[time]\t{:name_width$}\t{:>4}\t{:>8}\t{:>9}\t{:>8}",
            name,
            stats.runs,
            stats.total_user_ms,
            stats.total_kernel_ms,
            total_ms,
            name_width = name_width
        );
    }
    if total_runs > 0 {
        println!(
            "[time]\t{:name_width$}\t{:>4}\t{:>8}\t{:>9}\t{:>8}",
            "total",
            total_runs,
            total_user,
            total_kernel,
            total_user + total_kernel,
            name_width = name_width
        );
    }
}
