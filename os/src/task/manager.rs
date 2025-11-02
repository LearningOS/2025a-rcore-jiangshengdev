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

/// Wrapper for TaskControlBlock that implements Ord for min-heap behavior
/// based on stride values with overflow handling
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
        // Reverse ordering for min-heap: if self < other, return Greater
        // This makes BinaryHeap (max-heap) behave as min-heap
        StrideComparator::partial_cmp(other.0.stride(), self.0.stride())
    }
}

///An array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: BinaryHeap<TaskWrapper>,
}

/// A stride scheduler using priority queue.
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
