//! Semaphore

use crate::sync::UPSafeCell;
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;

/// semaphore structure
pub struct Semaphore {
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
    pub holders: BTreeMap<usize, usize>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                    holders: BTreeMap::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self) {
        trace!("kernel: Semaphore::up");
        let current_tid = current_task().map(|task| task.tid());
        let mut inner = self.inner.exclusive_access();
        if let Some(tid) = current_tid {
            if let Some(entry) = inner.holders.get_mut(&tid) {
                if *entry > 0 {
                    *entry -= 1;
                    if *entry == 0 {
                        inner.holders.remove(&tid);
                    }
                }
            }
        }
        inner.count += 1;
        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                let tid = task.tid();
                *inner.holders.entry(tid).or_insert(0) += 1;
                drop(inner);
                wakeup_task(task);
                return;
            }
        }
        drop(inner);
    }

    /// down operation of semaphore
    pub fn down(&self) {
        trace!("kernel: Semaphore::down");
        let task = current_task().unwrap();
        let tid = task.tid();
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            inner.wait_queue.push_back(task);
            drop(inner);
            block_current_and_run_next();
        } else {
            *inner.holders.entry(tid).or_insert(0) += 1;
            drop(inner);
        }
    }
}
