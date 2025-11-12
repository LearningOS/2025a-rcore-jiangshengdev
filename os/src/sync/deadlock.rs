//! Deadlock detection bookkeeping based on the Banker's algorithm.

use alloc::vec;
use alloc::vec::Vec;

/// Tracks mutex and semaphore resources for a single process.
#[derive(Default)]
pub struct DeadlockDetector {
    enabled: bool,
    mutex: ResourceState,
    semaphore: ResourceState,
}

impl DeadlockDetector {
    /// Enable or disable deadlock detection for the process.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns whether detection is currently enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Reset bookkeeping for a mutex resource.
    pub fn reset_mutex(&mut self, id: usize) {
        self.mutex.reset_resource(id, 1);
    }

    /// Reset bookkeeping for a semaphore resource with the given capacity.
    pub fn reset_semaphore(&mut self, id: usize, total: usize) {
        self.semaphore.reset_resource(id, total);
    }

    /// Record a staged mutex request from thread `tid`.
    pub fn stage_mutex_request(&mut self, tid: usize, id: usize, amount: usize) {
        self.mutex.stage_request(tid, id, amount);
    }

    /// Record a staged semaphore request from thread `tid`.
    pub fn stage_semaphore_request(&mut self, tid: usize, id: usize, amount: usize) {
        self.semaphore.stage_request(tid, id, amount);
    }

    /// Undo a previously staged mutex request.
    pub fn unstage_mutex_request(&mut self, tid: usize, id: usize, amount: usize) {
        self.mutex.unstage_request(tid, id, amount);
    }

    /// Undo a previously staged semaphore request.
    pub fn unstage_semaphore_request(&mut self, tid: usize, id: usize, amount: usize) {
        self.semaphore.unstage_request(tid, id, amount);
    }

    /// Check whether the mutex state remains safe.
    pub fn check_mutex_safe(&self) -> bool {
        !self.enabled || self.mutex.check_safe()
    }

    /// Check whether the semaphore state remains safe.
    pub fn check_semaphore_safe(&self) -> bool {
        !self.enabled || self.semaphore.check_safe()
    }

    /// Commit an acquired mutex to the allocation table.
    pub fn commit_mutex_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        self.mutex.commit_allocation(tid, id, amount);
    }

    /// Commit an acquired semaphore to the allocation table.
    pub fn commit_semaphore_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        self.semaphore.commit_allocation(tid, id, amount);
    }

    /// Release a mutex from the allocation table.
    pub fn release_mutex_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        self.mutex.release_allocation(tid, id, amount);
    }

    /// Release a semaphore from the allocation table.
    pub fn release_semaphore_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        self.semaphore.release_allocation(tid, id, amount);
    }
}

#[derive(Default)]
struct ResourceState {
    total: Vec<usize>,
    allocation: Vec<Vec<usize>>,
    need: Vec<Vec<usize>>,
}

impl ResourceState {
    fn reset_resource(&mut self, id: usize, total: usize) {
        self.ensure_resource(id);
        self.total[id] = total;
        for row in &mut self.allocation {
            row[id] = 0;
        }
        for row in &mut self.need {
            row[id] = 0;
        }
    }

    fn stage_request(&mut self, tid: usize, id: usize, amount: usize) {
        self.ensure_entries(tid, id);
        self.need[tid][id] += amount;
    }

    fn unstage_request(&mut self, tid: usize, id: usize, amount: usize) {
        self.ensure_entries(tid, id);
        let slot = &mut self.need[tid][id];
        debug_assert!(*slot >= amount);
        *slot -= amount;
    }

    fn commit_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        self.ensure_entries(tid, id);
        let need_entry = &mut self.need[tid][id];
        debug_assert!(*need_entry >= amount);
        *need_entry -= amount;
        self.allocation[tid][id] += amount;
    }

    fn release_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        self.ensure_entries(tid, id);
        let alloc = &mut self.allocation[tid][id];
        debug_assert!(*alloc >= amount);
        *alloc -= amount;
    }

    fn check_safe(&self) -> bool {
        if self.total.is_empty() {
            return true;
        }
        let mut work = self.available();
        let mut finish = vec![false; self.allocation.len()];
        let resource_cnt = self.total.len();
        loop {
            let mut progressed = false;
            #[allow(clippy::needless_range_loop)]
            for i in 0..self.allocation.len() {
                if finish[i] {
                    continue;
                }
                if (0..resource_cnt).all(|j| self.need[i][j] <= work[j]) {
                    #[allow(clippy::needless_range_loop)]
                    for j in 0..resource_cnt {
                        work[j] += self.allocation[i][j];
                    }
                    finish[i] = true;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
        finish.iter().enumerate().all(|(idx, done)| {
            if *done {
                true
            } else {
                self.allocation[idx].iter().all(|&alloc| alloc == 0)
                    && self.need[idx].iter().all(|&req| req == 0)
            }
        })
    }

    fn available(&self) -> Vec<usize> {
        let mut remaining = self.total.clone();
        for row in &self.allocation {
            for (res_idx, &alloc) in row.iter().enumerate() {
                if res_idx < remaining.len() {
                    if remaining[res_idx] >= alloc {
                        remaining[res_idx] -= alloc;
                    } else {
                        remaining[res_idx] = 0;
                    }
                }
            }
        }
        remaining
    }

    fn ensure_entries(&mut self, tid: usize, id: usize) {
        self.ensure_resource(id);
        self.ensure_thread(tid);
    }

    fn ensure_resource(&mut self, id: usize) {
        if self.total.len() <= id {
            let new_len = id + 1;
            self.total.resize(new_len, 0);
            for row in &mut self.allocation {
                row.resize(new_len, 0);
            }
            for row in &mut self.need {
                row.resize(new_len, 0);
            }
        }
    }

    fn ensure_thread(&mut self, tid: usize) {
        if self.allocation.len() <= tid {
            let resource_len = self.total.len();
            let new_len = tid + 1;
            while self.allocation.len() < new_len {
                self.allocation.push(vec![0; resource_len]);
                self.need.push(vec![0; resource_len]);
            }
        }
    }
}
