# os/src/syscall/sync.rs 变更说明

## 新增常量与辅助函数

- 定义 `DEADLOCK_ERR = -0xDEAD`，作为死锁检测失败时返回给用户态的统一错误码。
- 新增 `current_tid()`，从当前任务的 `TaskControlBlock` 中抽取 `tid`，供资源记账使用。

## 互斥锁相关调整

- `sys_mutex_create` 在分配或复用条目后调用 `deadlock.reset_mutex`，初始化每个互斥锁对应的资源列。
- `sys_mutex_lock` 先在进程的 `DeadlockDetector` 中 `stage_mutex_request`，银行家算法确认安全后才真正调用 `mutex.lock()`；成功持有后再 `commit_mutex_allocation`。
- `sys_mutex_unlock` 在释放底层互斥锁后调用 `release_mutex_allocation`，同步释放资源。

## 信号量相关调整

- `sys_semaphore_create` 记录信号量的总容量至 `deadlock.reset_semaphore`。
- `sys_semaphore_down` 与互斥锁加锁路径一致，先预演资源请求并检测安全性，失败立即返回 `DEADLOCK_ERR`；成功后提交分配。
- `sys_semaphore_up` 在唤醒等待线程后，通过 `release_semaphore_allocation` 更新可用资源。

## 死锁检测开关

- `sys_enable_deadlock_detect` 实现参数校验：仅接受 `0` 或 `1`，并据此更新进程内的 `DeadlockDetector` 开关状态。
