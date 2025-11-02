//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::BinaryHeap;
use alloc::sync::Arc;
use core::cmp::Ordering;
use lazy_static::*;

/// Stride comparator that handles overflow correctly
struct StrideComparator;

impl StrideComparator {
    /// Compare two stride values while handling potential overflow.
    /// Returns `Ordering::Less` if `lhs` should be scheduled before `rhs`.
    fn cmp(lhs: usize, rhs: usize) -> Ordering {
        let diff = lhs.wrapping_sub(rhs) as isize;
        diff.cmp(&0)
    }
}

/// Wrapper to provide `Ord` implementation for [`BinaryHeap`].
struct TaskWrapper(Arc<TaskControlBlock>);

impl PartialEq for TaskWrapper {
    fn eq(&self, other: &Self) -> bool {
        self.0.stride() == other.0.stride()
    }
}

impl Eq for TaskWrapper {}

impl PartialOrd for TaskWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TaskWrapper {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap behavior on [`BinaryHeap`].
        StrideComparator::cmp(other.0.stride(), self.0.stride())
    }
}

///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: BinaryHeap<TaskWrapper>,
}

impl TaskManager {
    ///Create an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: BinaryHeap::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push(TaskWrapper(task));
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop().map(|wrapper| wrapper.0)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    TASK_MANAGER.exclusive_access().fetch()
}
