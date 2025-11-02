//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cmp::Ordering;
use lazy_static::*;
/// Stride comparator that handles overflow correctly
struct StrideComparator;

impl StrideComparator {
    /// Compare two stride values handling potential overflow.
    /// Returns Ordering::Less if stride1 should be scheduled before stride2.
    /// 
    /// This uses signed difference comparison to handle wraparound:
    /// If (stride1 - stride2) when interpreted as signed is negative,
    /// then stride1 < stride2 in the circular stride space.
    fn partial_cmp(stride1: usize, stride2: usize) -> Ordering {
        let diff = stride1.wrapping_sub(stride2) as isize;
        diff.cmp(&0)
    }
}

///An array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: Vec<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Create an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: Vec::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push(task);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        if self.ready_queue.is_empty() {
            return None;
        }
        let mut best_idx = 0;
        let mut best_stride = self.ready_queue[0].stride();
        for idx in 1..self.ready_queue.len() {
            let stride = self.ready_queue[idx].stride();
            if StrideComparator::partial_cmp(stride, best_stride) == Ordering::Less {
                best_stride = stride;
                best_idx = idx;
            }
        }
        Some(self.ready_queue.remove(best_idx))
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
