use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task, ProcessControlBlock};
use crate::timer::{add_timer, get_time_ms};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::sync::Arc;
use alloc::vec::Vec;

const DEADLOCK_ERR: isize = -(0xDEAD as isize);
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let requester_tid = current_task().unwrap().tid();
    let (mutex, detect_enabled) = {
        let process_inner = process.inner_exclusive_access();
        let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
        let enabled = process_inner.deadlock_detect;
        (mutex, enabled)
    };
    if detect_enabled {
        if let Some(blocking_mutex) = mutex.as_ref().as_any().downcast_ref::<MutexBlocking>() {
            if is_mutex_deadlock(&process, blocking_mutex, requester_tid) {
                return DEADLOCK_ERR;
            }
        }
    }
    mutex.lock();
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let requester_tid = current_task().unwrap().tid();
    let (sem, detect_enabled) = {
        let process_inner = process.inner_exclusive_access();
        let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
        let enabled = process_inner.deadlock_detect;
        (sem, enabled)
    };
    if detect_enabled && is_semaphore_deadlock(&process, sem.as_ref(), requester_tid) {
        return DEADLOCK_ERR;
    }
    sem.down();
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect {}", enabled);
    let flag = match enabled {
        0 => false,
        1 => true,
        _ => return -1,
    };
    let process = current_process();
    process.inner_exclusive_access().deadlock_detect = flag;
    0
}

fn is_mutex_deadlock(
    process: &Arc<ProcessControlBlock>,
    target_mutex: &MutexBlocking,
    requester_tid: usize,
) -> bool {
    let owner_tid = {
        let inner = target_mutex.inner.exclusive_access();
        inner.owner_tid
    };
    let owner_tid = match owner_tid {
        Some(tid) => tid,
        None => return false,
    };
    if owner_tid == requester_tid {
        return true;
    }

    let mutexes: Vec<Arc<dyn Mutex>> = {
        let process_inner = process.inner_exclusive_access();
        process_inner
            .mutex_list
            .iter()
            .filter_map(|mutex| mutex.as_ref().map(Arc::clone))
            .collect()
    };

    let mut graph: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for mutex in mutexes.iter() {
        if let Some(blocking) = mutex.as_ref().as_any().downcast_ref::<MutexBlocking>() {
            let inner = blocking.inner.exclusive_access();
            if let Some(owner) = inner.owner_tid {
                for task in inner.wait_queue.iter() {
                    let waiter_tid = task.tid();
                    add_edge(&mut graph, waiter_tid, owner);
                }
            }
        }
    }

    add_edge(&mut graph, requester_tid, owner_tid);
    detect_cycle(&graph, owner_tid, requester_tid)
}

fn is_semaphore_deadlock(
    process: &Arc<ProcessControlBlock>,
    target_sem: &Semaphore,
    requester_tid: usize,
) -> bool {
    let (owners, available) = {
        let inner = target_sem.inner.exclusive_access();
        let owners = inner
            .holders
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(&tid, _)| tid)
            .collect::<Vec<_>>();
        (owners, inner.count)
    };

    if available > 0 || owners.is_empty() {
        return false;
    }

    let semaphores: Vec<Arc<Semaphore>> = {
        let process_inner = process.inner_exclusive_access();
        process_inner
            .semaphore_list
            .iter()
            .filter_map(|sem| sem.as_ref().map(Arc::clone))
            .collect()
    };

    let mut graph: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for sem in semaphores.iter() {
        let inner = sem.inner.exclusive_access();
        let holders: Vec<usize> = inner
            .holders
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(&tid, _)| tid)
            .collect();
        if holders.is_empty() {
            continue;
        }
        for task in inner.wait_queue.iter() {
            let waiter_tid = task.tid();
            for owner in holders.iter().copied() {
                add_edge(&mut graph, waiter_tid, owner);
            }
        }
    }

    for owner in owners.iter().copied() {
        add_edge(&mut graph, requester_tid, owner);
    }

    owners
        .iter()
        .copied()
        .any(|owner| detect_cycle(&graph, owner, requester_tid))
}

fn add_edge(graph: &mut BTreeMap<usize, Vec<usize>>, from: usize, to: usize) {
    if from == to {
        return;
    }
    let entry = graph.entry(from).or_insert_with(Vec::new);
    if !entry.contains(&to) {
        entry.push(to);
    }
}

fn detect_cycle(graph: &BTreeMap<usize, Vec<usize>>, start: usize, target: usize) -> bool {
    let mut visited = BTreeSet::new();
    has_path(graph, start, target, &mut visited)
}

fn has_path(
    graph: &BTreeMap<usize, Vec<usize>>,
    node: usize,
    target: usize,
    visited: &mut BTreeSet<usize>,
) -> bool {
    if node == target {
        return true;
    }
    if !visited.insert(node) {
        return false;
    }
    if let Some(neighbors) = graph.get(&node) {
        for &next in neighbors {
            if has_path(graph, next, target, visited) {
                return true;
            }
        }
    }
    false
}
