//! [`TaskManager`] 的实现
//!
//! 它仅用于管理进程并基于就绪队列调度进程。
//! 其他CPU进程监控功能在Processor中实现。

use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use lazy_static::*;
/// 线程安全的 `TaskControlBlock` 数组
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// 一个简单的 FIFO 调度器
impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    /// 创建一个空的 TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// 将进程添加回就绪队列
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        // 将任务添加到就绪队列的末尾，实现FIFO调度
        self.ready_queue.push_back(task);
    }
    /// 从就绪队列中取出一个进程
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        // 从就绪队列的前端取出任务，实现FIFO调度
        self.ready_queue.pop_front()
    }
}

lazy_static! {
    /// 通过 lazy_static! 创建的 TASK_MANAGER 实例
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
    /// PID到PCB的映射表实例（进程ID到进程控制块的映射）
    pub static ref PID2TCB: UPSafeCell<BTreeMap<usize, Arc<TaskControlBlock>>> =
        unsafe { UPSafeCell::new(BTreeMap::new()) };
}

/// 将进程添加到就绪队列
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    // 将任务添加到PID到任务控制块的映射表中
    PID2TCB
        .exclusive_access()
        .insert(task.getpid(), Arc::clone(&task));
    // 将任务添加到就绪队列中等待调度
    TASK_MANAGER.exclusive_access().add(task);
}

/// 从就绪队列中取出一个进程
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    // 从任务管理器的就绪队列中获取下一个要运行的任务
    TASK_MANAGER.exclusive_access().fetch()
}

/// 根据PID获取进程
pub fn pid2task(pid: usize) -> Option<Arc<TaskControlBlock>> {
    // 根据PID从映射表中查找对应的任务控制块
    let map = PID2TCB.exclusive_access();
    map.get(&pid).map(Arc::clone)
}

/// 从PID到PCB映射表中移除项目（由exit_current_and_run_next调用）
pub fn remove_from_pid2task(pid: usize) {
    // 从PID到任务控制块的映射表中移除指定的任务
    let mut map = PID2TCB.exclusive_access();
    if map.remove(&pid).is_none() {
        panic!("cannot find pid {} in pid2task!", pid);
    }
}
