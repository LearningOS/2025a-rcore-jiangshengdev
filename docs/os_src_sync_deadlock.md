# os/src/sync/deadlock.rs 变更说明

## 新增内容

- 新建 `DeadlockDetector` 结构，封装进程级的死锁检测开关及互斥锁、信号量两类资源的状态。
- 新建内部工具结构 `ResourceState`，维护银行家算法需要的 `total`、`allocation`、`need` 三类表格，并提供若干辅助方法。

## 主要逻辑

- `set_enabled` / `enabled`：允许进程开启或关闭检测功能。
- `reset_mutex` / `reset_semaphore`：在资源创建时重置对应资源列，同时清零所有线程的分配与需求数据。
- `stage_*_request`：在真正发起阻塞之前先记录线程的资源需求，作为安全性评估的输入。
- `check_*_safe`：依次对互斥锁、信号量执行银行家算法，如果未开启检测或能够找到安全序列则返回 `true`。
- `commit_*_allocation` / `release_*_allocation`：在成功获取或释放资源后更新分配表，保持数据一致性。

## 关键实现细节

- 银行家算法通过 `available` 计算当前剩余资源，循环尝试找到可满足需求的线程，只要所有未完成线程都没有实际占用资源，即视为安全。
- `ensure_resource` 与 `ensure_thread` 动态扩展矩阵规模，保证资源 ID 与线程 ID 稀疏分布时也能按需增长。
- 所有 `unstage`、`commit`、`release` 操作都带有 `debug_assert!`，在调试构建中协助捕捉不匹配的资源计数。
