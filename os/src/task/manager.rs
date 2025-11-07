//! [`TaskManager`] 的实现。
//!
//! 仅负责基于就绪队列管理与调度进程，其他 CPU 进程监控相关功能由 `Processor` 提供。

use super::{ProcessControlBlock, TaskControlBlock, TaskStatus};
use crate::sync::UPSafeCell;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use lazy_static::*;
/// 线程安全的 `TaskControlBlock` 队列
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,

    /// 停止中的任务，保留引用以避免切换任务时回收其内核栈
    stop_task: Option<Arc<TaskControlBlock>>,
}

/// 简单的 FIFO 调度器。
impl TaskManager {
    /// 创建一个空的 `TaskManager`
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            stop_task: None,
        }
    }
    /// 将任务重新加入就绪队列
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// 从就绪队列取出一个任务
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop_front()
    }
    pub fn remove(&mut self, task: Arc<TaskControlBlock>) {
        if let Some((id, _)) = self
            .ready_queue
            .iter()
            .enumerate()
            .find(|(_, t)| Arc::as_ptr(t) == Arc::as_ptr(&task))
        {
            self.ready_queue.remove(id);
        }
    }
    /// 记录一个停止中的任务
    pub fn add_stop(&mut self, task: Arc<TaskControlBlock>) {
        // 注意：上一条停止任务已经完全结束（至少在单核场景中不再使用内核栈），因此可以直接替换。
        self.stop_task = Some(task);
    }
}

lazy_static! {
    /// 通过 lazy_static! 创建的 TASK_MANAGER 实例
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
    /// PID2PCB 实例（PID 到 PCB 的映射）
    pub static ref PID2PCB: UPSafeCell<BTreeMap<usize, Arc<ProcessControlBlock>>> =
        unsafe { UPSafeCell::new(BTreeMap::new()) };
}

/// 将任务加入就绪队列
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// 唤醒一个任务
pub fn wakeup_task(task: Arc<TaskControlBlock>) {
    trace!("kernel: TaskManager::wakeup_task");
    let mut task_inner = task.inner_exclusive_access();
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);
    add_task(task);
}

/// 从就绪队列移除任务
pub fn remove_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::remove_task");
    TASK_MANAGER.exclusive_access().remove(task);
}

/// 从就绪队列获取一个任务
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}

/// 将任务置于等待停止状态，确保其内核栈彻底闲置。
pub fn add_stopping_task(task: Arc<TaskControlBlock>) {
    TASK_MANAGER.exclusive_access().add_stop(task);
}

/// 根据 PID 查询进程
pub fn pid2process(pid: usize) -> Option<Arc<ProcessControlBlock>> {
    let map = PID2PCB.exclusive_access();
    map.get(&pid).map(Arc::clone)
}

/// 将 (pid, pcb) 插入 PID2PCB 映射（由 `do_fork` 与 `ProcessControlBlock::new` 调用）
pub fn insert_into_pid2process(pid: usize, process: Arc<ProcessControlBlock>) {
    PID2PCB.exclusive_access().insert(pid, process);
}

/// 从 PID2PCB 映射中移除指定 pid（由 `exit_current_and_run_next` 调用）
pub fn remove_from_pid2process(pid: usize) {
    let mut map = PID2PCB.exclusive_access();
    if map.remove(&pid).is_none() {
        panic!("cannot find pid {} in pid2task!", pid);
    }
}
