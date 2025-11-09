use core::cmp::Ordering;

use alloc::{collections::BTreeMap, vec::Vec};
use lazy_static::lazy_static;

use crate::sync::UPSafeCell;

#[derive(Default)]
pub struct SyscallStats {
    pub total_ms: usize,
    pub max_ms: usize,
    pub calls: usize,
}

lazy_static! {
    static ref SYSCALL_STATS: UPSafeCell<BTreeMap<usize, SyscallStats>> =
        unsafe { UPSafeCell::new(BTreeMap::new()) };
}

const SYSCALL_NAMES: &[(usize, &str)] = &[
    (super::SYSCALL_DUP, "dup"),
    (super::SYSCALL_LINKAT, "linkat"),
    (super::SYSCALL_UNLINKAT, "unlinkat"),
    (super::SYSCALL_OPENAT, "openat"),
    (super::SYSCALL_CLOSE, "close"),
    (super::SYSCALL_PIPE, "pipe"),
    (super::SYSCALL_READ, "read"),
    (super::SYSCALL_WRITE, "write"),
    (super::SYSCALL_FSTAT, "fstat"),
    (super::SYSCALL_EXIT, "exit"),
    (super::SYSCALL_SLEEP, "sleep"),
    (super::SYSCALL_YIELD, "yield"),
    (super::SYSCALL_GETPID, "getpid"),
    (super::SYSCALL_GETTID, "gettid"),
    (super::SYSCALL_FORK, "fork"),
    (super::SYSCALL_EXEC, "exec"),
    (super::SYSCALL_WAITPID, "waitpid"),
    (super::SYSCALL_GETTIMEOFDAY, "gettimeofday"),
    (super::SYSCALL_MMAP, "mmap"),
    (super::SYSCALL_MUNMAP, "munmap"),
    (super::SYSCALL_SET_PRIORITY, "set_priority"),
    (super::SYSCALL_SPAWN, "spawn"),
    (super::SYSCALL_THREAD_CREATE, "thread_create"),
    (super::SYSCALL_WAITTID, "waittid"),
    (super::SYSCALL_MUTEX_CREATE, "mutex_create"),
    (super::SYSCALL_MUTEX_LOCK, "mutex_lock"),
    (super::SYSCALL_MUTEX_UNLOCK, "mutex_unlock"),
    (super::SYSCALL_SEMAPHORE_CREATE, "semaphore_create"),
    (super::SYSCALL_SEMAPHORE_UP, "semaphore_up"),
    (
        super::SYSCALL_ENABLE_DEADLOCK_DETECT,
        "enable_deadlock_detect",
    ),
    (super::SYSCALL_SEMAPHORE_DOWN, "semaphore_down"),
    (super::SYSCALL_CONDVAR_CREATE, "condvar_create"),
    (super::SYSCALL_CONDVAR_SIGNAL, "condvar_signal"),
    (super::SYSCALL_CONDVAR_WAIT, "condvar_wait"),
    (super::SYSCALL_KILL, "kill"),
];

fn syscall_name(id: usize) -> &'static str {
    SYSCALL_NAMES
        .iter()
        .find_map(|(num, name)| if *num == id { Some(*name) } else { None })
        .unwrap_or("unknown")
}

pub fn record_syscall_cost(syscall_id: usize, duration_ms: usize) {
    let mut stats_map = SYSCALL_STATS.exclusive_access();
    let entry = stats_map.entry(syscall_id).or_default();
    entry.calls += 1;
    entry.total_ms += duration_ms;
    if duration_ms > entry.max_ms {
        entry.max_ms = duration_ms;
    }
}

pub fn report_syscall_summary() {
    let stats_map = SYSCALL_STATS.exclusive_access();
    if stats_map.is_empty() {
        println!("[syscall]\ttime_summary\tno_records");
        return;
    }

    let mut rows: Vec<(usize, &SyscallStats)> =
        stats_map.iter().map(|(id, stats)| (*id, stats)).collect();
    rows.sort_by(|a, b| match b.1.total_ms.cmp(&a.1.total_ms) {
        Ordering::Equal => a.0.cmp(&b.0),
        other => other,
    });

    let mut name_width = rows
        .iter()
        .map(|(id, _)| syscall_name(*id).len())
        .max()
        .unwrap_or(0);
    name_width = name_width.max("syscall".len());

    let mut total_calls = 0usize;
    let mut total_ms = 0usize;
    let mut global_max = 0usize;

    println!("[syscall]\ttime_summary");
    println!(
        "[syscall]\t{:name_width$}\t{:>5}\t{:>12}\t{:>12}\t{:>12}",
        "syscall",
        "calls",
        "total_ms",
        "avg_ms",
        "max_ms",
        name_width = name_width,
    );

    for (id, stats) in rows.into_iter() {
        let avg = if stats.calls > 0 {
            stats.total_ms / stats.calls
        } else {
            0
        };
        total_calls += stats.calls;
        total_ms += stats.total_ms;
        if stats.max_ms > global_max {
            global_max = stats.max_ms;
        }
        println!(
            "[syscall]\t{:name_width$}\t{:>5}\t{:>12}\t{:>12}\t{:>12}",
            syscall_name(id),
            stats.calls,
            stats.total_ms,
            avg,
            stats.max_ms,
            name_width = name_width,
        );
    }

    if total_calls > 0 {
        let avg_total = total_ms / total_calls;
        println!(
            "[syscall]\t{:name_width$}\t{:>5}\t{:>12}\t{:>12}\t{:>12}",
            "total",
            total_calls,
            total_ms,
            avg_total,
            global_max,
            name_width = name_width,
        );
    }
}
