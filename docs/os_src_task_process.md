# os/src/task/process.rs 变更说明

## 结构体调整

- `ProcessControlBlockInner` 新增字段 `deadlock: DeadlockDetector`，用于存放每个进程的资源分配状态。
- 在内核进程与用户进程的初始化路径中，均以 `DeadlockDetector::default()` 填充该字段，确保所有进程在创建时具备独立的检测上下文。

## 影响

- 进程在生命周期内可持续追踪自身线程对互斥锁、信号量的请求状况，为系统调用中的死锁检测提供数据支撑。
