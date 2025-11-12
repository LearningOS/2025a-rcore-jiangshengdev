//! 基于银行家算法的死锁检测簿记逻辑。

// 引入向量宏以便快速构造动态数组。
use alloc::vec;
// 引入 `Vec` 支持动态维护资源矩阵。
use alloc::vec::Vec;

/// 跟踪单个进程的互斥锁与信号量资源占用情况。
// 提供默认实现以便在进程构造时直接创建检测器实例。
#[derive(Default)]
pub struct DeadlockDetector {
    // 记录死锁检测是否启用。
    enabled: bool,
    // 保存互斥锁相关的资源矩阵。
    mutex: ResourceState,
    // 保存信号量相关的资源矩阵。
    semaphore: ResourceState,
}

impl DeadlockDetector {
    /// 为当前进程开启或关闭死锁检测。
    pub fn set_enabled(&mut self, enabled: bool) {
        // 将开关状态同步为调用方传入的布尔值。
        self.enabled = enabled;
    }

    /// 返回死锁检测是否处于启用状态。
    pub fn enabled(&self) -> bool {
        // 直接读取布尔开关。
        self.enabled
    }

    /// 重置指定互斥锁资源的簿记信息。
    pub fn reset_mutex(&mut self, id: usize) {
        // 互斥锁每次最多仅持有一个单位的资源。
        self.mutex.reset_resource(id, 1);
    }

    /// 按照信号量总容量重置簿记信息。
    pub fn reset_semaphore(&mut self, id: usize, total: usize) {
        // 将信号量的总可用量注入检测器。
        self.semaphore.reset_resource(id, total);
    }

    /// 记录线程 `tid` 发起的互斥锁资源请求。
    pub fn stage_mutex_request(&mut self, tid: usize, id: usize, amount: usize) {
        // 将需求先写入 need 表以便后续安全性评估。
        self.mutex.stage_request(tid, id, amount);
    }

    /// 记录线程 `tid` 发起的信号量资源请求。
    pub fn stage_semaphore_request(&mut self, tid: usize, id: usize, amount: usize) {
        // 对应信号量资源执行与互斥锁相同的预登记。
        self.semaphore.stage_request(tid, id, amount);
    }

    /// 撤销互斥锁资源请求的预登记。
    pub fn unstage_mutex_request(&mut self, tid: usize, id: usize, amount: usize) {
        // 在请求被拒绝时回滚 need 表。
        self.mutex.unstage_request(tid, id, amount);
    }

    /// 撤销信号量资源请求的预登记。
    pub fn unstage_semaphore_request(&mut self, tid: usize, id: usize, amount: usize) {
        // 与互斥锁逻辑一致地回滚。
        self.semaphore.unstage_request(tid, id, amount);
    }

    /// 检查互斥锁资源状态是否仍然安全。
    pub fn check_mutex_safe(&self) -> bool {
        // 未开启检测时直接放行，否则运行安全性算法。
        !self.enabled || self.mutex.check_safe()
    }

    /// 检查信号量资源状态是否仍然安全。
    pub fn check_semaphore_safe(&self) -> bool {
        // 与互斥锁同理，依据开关决定是否执行算法。
        !self.enabled || self.semaphore.check_safe()
    }

    /// 提交互斥锁资源的成功分配。
    pub fn commit_mutex_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        // 将需求从 need 表扣除并记录到 allocation。
        self.mutex.commit_allocation(tid, id, amount);
    }

    /// 提交信号量资源的成功分配。
    pub fn commit_semaphore_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        // 信号量分配沿用统一的提交流程。
        self.semaphore.commit_allocation(tid, id, amount);
    }

    /// 释放互斥锁资源的分配记录。
    pub fn release_mutex_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        // 从 allocation 表中归还指定数量的资源。
        self.mutex.release_allocation(tid, id, amount);
    }

    /// 释放信号量资源的分配记录。
    pub fn release_semaphore_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        // 信号量释放同样直接影响 allocation 表。
        self.semaphore.release_allocation(tid, id, amount);
    }
}

// 提供默认实现以简化状态结构的初始化流程。
#[derive(Default)]
struct ResourceState {
    // 记录每类资源的总量。
    total: Vec<usize>,
    // 记录各线程实际占用的资源量。
    allocation: Vec<Vec<usize>>,
    // 记录各线程尚需的资源量。
    need: Vec<Vec<usize>>,
}

impl ResourceState {
    /// 按照给定总量重置指定资源列的簿记数据。
    fn reset_resource(&mut self, id: usize, total: usize) {
        // 确保资源列存在，避免索引越界。
        self.ensure_resource(id);
        // 更新资源总量。
        self.total[id] = total;
        // 重置所有线程对该资源的分配记录。
        for row in &mut self.allocation {
            row[id] = 0;
        }
        // 重置所有线程对该资源的需求记录。
        for row in &mut self.need {
            row[id] = 0;
        }
    }

    /// 记录线程对资源的临时需求，用于安全性评估。
    fn stage_request(&mut self, tid: usize, id: usize, amount: usize) {
        // 确保线程与资源的矩阵项已准备好。
        self.ensure_entries(tid, id);
        // 增加当前线程对资源的需求量。
        self.need[tid][id] += amount;
    }

    /// 在请求被拒绝时回滚线程的临时需求。
    fn unstage_request(&mut self, tid: usize, id: usize, amount: usize) {
        // 同样先确保索引有效。
        self.ensure_entries(tid, id);
        // 取出对应需求槽位。
        let slot = &mut self.need[tid][id];
        // 调试构建下确保回滚不会下溢。
        debug_assert!(*slot >= amount);
        // 扣减已登记的需求量。
        *slot -= amount;
    }

    /// 在成功分配资源后更新分配表与剩余需求。
    fn commit_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        // 确保矩阵尺寸满足访问需求。
        self.ensure_entries(tid, id);
        // 找出该线程尚需的资源计数。
        let need_entry = &mut self.need[tid][id];
        // 调试时确认需求足够扣减。
        debug_assert!(*need_entry >= amount);
        // 扣除需求量，表示已经被满足。
        *need_entry -= amount;
        // 将数量累加至分配表。
        self.allocation[tid][id] += amount;
    }

    /// 释放线程持有的资源占用记录。
    fn release_allocation(&mut self, tid: usize, id: usize, amount: usize) {
        // 保证索引有效。
        self.ensure_entries(tid, id);
        // 获取当前分配量。
        let alloc = &mut self.allocation[tid][id];
        // 调试阶段确认释放不会导致负值。
        debug_assert!(*alloc >= amount);
        // 减去释放的资源量。
        *alloc -= amount;
    }

    /// 利用银行家算法判断当前状态是否安全。
    fn check_safe(&self) -> bool {
        // 若尚未记录任何资源，则必然安全。
        if self.total.is_empty() {
            return true;
        }
        // 计算当前可用资源向量。
        let mut work = self.available();
        // 初始化每个线程的完成标记。
        let mut finish = vec![false; self.allocation.len()];
        // 缓存资源种类数量。
        let resource_cnt = self.total.len();
        loop {
            // 标记本轮是否找到可以推进的线程。
            let mut progressed = false;
            #[allow(clippy::needless_range_loop)]
            for i in 0..self.allocation.len() {
                // 已完成的线程无需重复判断。
                if finish[i] {
                    continue;
                }
                // 判断该线程需求是否均可被当前 work 满足。
                if (0..resource_cnt).all(|j| self.need[i][j] <= work[j]) {
                    #[allow(clippy::needless_range_loop)]
                    for j in 0..resource_cnt {
                        // 将线程占用的资源归还给 work。
                        work[j] += self.allocation[i][j];
                    }
                    // 标记该线程可顺利完成。
                    finish[i] = true;
                    // 指示本轮取得进展，继续尝试更多线程。
                    progressed = true;
                }
            }
            // 若一轮中没有线程完成，算法终止。
            if !progressed {
                break;
            }
        }
        // 检查所有未标记完成的线程是否确实未占用资源。
        finish.iter().enumerate().all(|(idx, done)| {
            if *done {
                true
            } else {
                self.allocation[idx].iter().all(|&alloc| alloc == 0)
                    && self.need[idx].iter().all(|&req| req == 0)
            }
        })
    }

    /// 计算仍可提供给线程使用的资源数量。
    fn available(&self) -> Vec<usize> {
        // 拷贝总资源量作为剩余资源的初始值。
        let mut remaining = self.total.clone();
        for row in &self.allocation {
            for (res_idx, &alloc) in row.iter().enumerate() {
                // 确保资源索引有效再进行扣减。
                if res_idx < remaining.len() {
                    if remaining[res_idx] >= alloc {
                        // 正常扣除已经分配的资源。
                        remaining[res_idx] -= alloc;
                    } else {
                        // 防止数值下溢，直接置零。
                        remaining[res_idx] = 0;
                    }
                }
            }
        }
        // 返回剩余可用的资源向量。
        remaining
    }

    /// 确保访问矩阵时线程和资源索引均有效。
    fn ensure_entries(&mut self, tid: usize, id: usize) {
        // 先确保资源列存在。
        self.ensure_resource(id);
        // 再确保线程行存在。
        self.ensure_thread(tid);
    }

    /// 扩展资源矩阵的列以容纳新的资源编号。
    fn ensure_resource(&mut self, id: usize) {
        // 当请求的资源编号超出当前容量时扩展矩阵。
        if self.total.len() <= id {
            // 计算新的容量大小。
            let new_len = id + 1;
            // 将总量数组扩展并用零填充。
            self.total.resize(new_len, 0);
            for row in &mut self.allocation {
                // 对每一行分配表补齐新列。
                row.resize(new_len, 0);
            }
            for row in &mut self.need {
                // 对每一行需求表同样补齐。
                row.resize(new_len, 0);
            }
        }
    }

    /// 扩展资源矩阵的行以容纳新的线程编号。
    fn ensure_thread(&mut self, tid: usize) {
        // 若线程编号超出当前记录范围则扩展行。
        if self.allocation.len() <= tid {
            // 当前资源列数为新行的长度。
            let resource_len = self.total.len();
            // 目标行数为线程编号加一。
            let new_len = tid + 1;
            while self.allocation.len() < new_len {
                // 为 allocation 新增一行并填充零。
                self.allocation.push(vec![0; resource_len]);
                // 为 need 新增一行保持尺寸一致。
                self.need.push(vec![0; resource_len]);
            }
        }
    }
}
