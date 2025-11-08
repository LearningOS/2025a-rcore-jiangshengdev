//! 聚合应用的运行时统计信息。

use crate::sync::UPSafeCell;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;

/// 唯一的进程实例标识
pub type ProcInstanceId = u64;

struct AppRunInstance {
    pid: usize,
    name: String,
    running_since_us: Option<usize>,
    accumulated_us: usize,
}

#[derive(Default)]
struct AppAggStat {
    total_cpu_ms: usize,
    last_finish_us: usize,
    runs: usize,
    last_exit_code: Option<i32>,
}

struct AppRuntimeTracker {
    instances: BTreeMap<ProcInstanceId, AppRunInstance>,
    stats: BTreeMap<String, AppAggStat>,
    order: Vec<String>,
}

impl AppRuntimeTracker {
    fn new() -> Self {
        Self {
            instances: BTreeMap::new(),
            stats: BTreeMap::new(),
            order: Vec::new(),
        }
    }
}

lazy_static! {
    static ref APP_RUNTIME: UPSafeCell<AppRuntimeTracker> =
        unsafe { UPSafeCell::new(AppRuntimeTracker::new()) };
}

static NEXT_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

#[inline]
/// 分配一个全局唯一的进程实例 ID。
pub fn alloc_instance_id() -> ProcInstanceId {
    NEXT_INSTANCE_ID.fetch_add(1, Ordering::Relaxed)
}

fn ensure_name_entry<'a>(tracker: &'a mut AppRuntimeTracker, name: &str) -> &'a mut AppAggStat {
    use alloc::collections::btree_map::Entry;
    match tracker.stats.entry(name.to_string()) {
        Entry::Vacant(entry) => {
            tracker.order.push(entry.key().clone());
            entry.insert(AppAggStat::default())
        }
        Entry::Occupied(entry) => entry.into_mut(),
    }
}

fn accumulate_until(instance: &mut AppRunInstance, timestamp_us: usize) -> usize {
    match instance.running_since_us.take() {
        Some(start) => {
            let delta = timestamp_us.saturating_sub(start);
            instance.accumulated_us = instance.accumulated_us.saturating_add(delta);
            delta
        }
        None => 0,
    }
}

fn record_segment(
    tracker: &mut AppRuntimeTracker,
    name: &str,
    duration_us: usize,
    timestamp_us: usize,
    exit_code: Option<i32>,
) {
    if duration_us == 0 {
        if let Some(code) = exit_code {
            let stat = ensure_name_entry(tracker, name);
            stat.last_finish_us = timestamp_us;
            stat.last_exit_code = Some(code);
        }
        return;
    }
    let stat = ensure_name_entry(tracker, name);
    stat.total_cpu_ms = stat.total_cpu_ms.saturating_add(duration_us / 1_000);
    stat.last_finish_us = timestamp_us;
    stat.runs = stat.runs.saturating_add(1);
    if let Some(code) = exit_code {
        stat.last_exit_code = Some(code);
    }
}

/// 注册新建进程的运行记录。
pub fn register_process(instance_id: ProcInstanceId, pid: usize, name: &str) {
    let mut tracker = APP_RUNTIME.exclusive_access();
    ensure_name_entry(&mut tracker, name);
    tracker.instances.insert(
        instance_id,
        AppRunInstance {
            pid,
            name: name.to_string(),
            running_since_us: None,
            accumulated_us: 0,
        },
    );
}

/// 记录 `fork` 派生出的子进程运行信息。
pub fn register_fork(parent_id: ProcInstanceId, child_id: ProcInstanceId, child_pid: usize) {
    let mut tracker = APP_RUNTIME.exclusive_access();
    let name = tracker
        .instances
        .get(&parent_id)
        .map(|parent| parent.name.clone())
        .unwrap_or_else(|| "<unknown>".to_string());
    ensure_name_entry(&mut tracker, &name);
    tracker.instances.insert(
        child_id,
        AppRunInstance {
            pid: child_pid,
            name,
            running_since_us: None,
            accumulated_us: 0,
        },
    );
}

/// 标记指定进程实例开始占用 CPU。
pub fn start_running(instance_id: ProcInstanceId, timestamp_us: usize) {
    let mut tracker = APP_RUNTIME.exclusive_access();
    if let Some(instance) = tracker.instances.get_mut(&instance_id) {
        if instance.running_since_us.is_none() {
            instance.running_since_us = Some(timestamp_us);
        }
    }
}

/// 标记指定进程实例停止占用 CPU，并累加本段耗时。
pub fn stop_running(instance_id: ProcInstanceId, timestamp_us: usize) {
    let mut tracker = APP_RUNTIME.exclusive_access();
    if let Some(instance) = tracker.instances.get_mut(&instance_id) {
        accumulate_until(instance, timestamp_us);
    }
}

/// 在 `exec` 后为进程切换到新的应用名称并结算旧阶段耗时。
pub fn mark_exec(instance_id: ProcInstanceId, new_name: &str, timestamp_us: usize) {
    let mut tracker = APP_RUNTIME.exclusive_access();
    if let Some((old_name, duration)) = tracker.instances.get_mut(&instance_id).map(|instance| {
        let old_name = instance.name.clone();
        accumulate_until(instance, timestamp_us);
        let duration = instance.accumulated_us;
        instance.accumulated_us = 0;
        instance.name = new_name.to_string();
        instance.running_since_us = Some(timestamp_us);
        (old_name, duration)
    }) {
        ensure_name_entry(&mut tracker, new_name);
        record_segment(&mut tracker, &old_name, duration, timestamp_us, None);
    }
}

/// 进程退出时结算其累计耗时并更新聚合统计。
pub fn mark_exit(instance_id: ProcInstanceId, exit_code: i32, timestamp_us: usize) {
    let mut tracker = APP_RUNTIME.exclusive_access();
    if let Some(mut instance) = tracker.instances.remove(&instance_id) {
        accumulate_until(&mut instance, timestamp_us);
        let duration = instance.accumulated_us;
        record_segment(
            &mut tracker,
            &instance.name,
            duration,
            timestamp_us,
            Some(exit_code),
        );
    }
}

/// 打印所有应用名称的聚合耗时统计。
pub fn print_summary() {
    let tracker = APP_RUNTIME.exclusive_access();
    let mut entries: Vec<_> = tracker.stats.iter().collect();
    entries.sort_by(|(name_a, stat_a), (name_b, stat_b)| {
        stat_b
            .total_cpu_ms
            .cmp(&stat_a.total_cpu_ms)
            .then_with(|| name_a.cmp(name_b))
    });
    let max_name_len = entries
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    for (name, stat) in entries {
        match stat.last_exit_code {
            Some(code) => println!(
                "[app]\t{:<width$}\tcpu_ms={}\truns={}\tlast_finish_us={}\tlast_exit={}",
                name,
                stat.total_cpu_ms,
                stat.runs,
                stat.last_finish_us,
                code,
                width = max_name_len
            ),
            None => println!(
                "[app]\t{:<width$}\tcpu_ms={}\truns={}\tlast_finish_us={}",
                name,
                stat.total_cpu_ms,
                stat.runs,
                stat.last_finish_us,
                width = max_name_len
            ),
        }
    }
    if !tracker.instances.is_empty() {
        println!(
            "[app-runtime]\tunresolved_instances={}",
            tracker.instances.len()
        );
        for (instance_id, instance) in tracker.instances.iter() {
            println!(
                "[app-runtime]\tinstance={}\tpid={}\tname={}\trunning_since={:?}\taccumulated_us={}",
                instance_id,
                instance.pid,
                instance.name,
                instance.running_since_us,
                instance.accumulated_us
            );
        }
    }
}
