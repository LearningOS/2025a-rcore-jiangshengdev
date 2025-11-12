# syscall/sync.rs 改动详解

## 1. 改动综述

该文件包含死锁检测的开关与检测实现：

- 常量 `DEADLOCK_ERR = -0xDEAD` 作为统一错误码；
- 系统调用 `sys_enable_deadlock_detect(enabled)` 控制进程级开关；
- 在 `sys_mutex_lock` / `sys_semaphore_down` 前依据开关执行检测，若检测到潜在环，则拒绝请求并返回 `DEADLOCK_ERR`；
- 检测实现由等待图构建 + DFS 路径搜索组成（涉及函数：`is_mutex_deadlock`、`is_semaphore_deadlock`、`add_edge`、`detect_cycle`、`has_path`）。

## 2. 接口与行为

### 2.1 `sys_enable_deadlock_detect(enabled: usize) -> isize`

- 参数：`0` 关闭、`1` 开启；其他值返回 `-1`。
- 作用：设置当前进程 `ProcessControlBlockInner.deadlock_detect` 标志；该标志会在 fork 时继承。

### 2.2 在加锁/降信号量前的检测

- `sys_mutex_lock(mutex_id)`：
  1. 若进程未开启检测，直接加锁；
  2. 若开启且目标可下转为 `MutexBlocking`：
     - 读取 `owner_tid`；若为 `None`（无人持有）则无需检测；
     - 若 `owner_tid == requester_tid`（同线程重复获取），直接视为死锁返回 `DEADLOCK_ERR`；
     - 构建等待图（详见 3），加入“本次请求”边 `requester -> owner`；
     - 若检测到 `owner ->* requester`，返回 `DEADLOCK_ERR`；否则执行 `lock()`。
- `sys_semaphore_down(sem_id)`：
  1. 若进程未开启检测，直接 `down()`；
  2. 若开启：
     - 读取目标信号量的 `available` 与 `holders`；若 `available > 0` 或无持有者，说明无需检测；
     - 构建等待图（详见 3），加入“本次请求”边 `requester -> each(holder_of_target)`；
     - 对每个 holder 检测 `holder ->* requester` 是否成立；若任一成立，返回 `DEADLOCK_ERR`，否则执行 `down()`。

## 3. 等待图构建

### 3.1 互斥锁

- 遍历进程内所有 `mutex_list`，仅考虑 `MutexBlocking`：
  - 若 `owner_tid = Some(owner)`，则对其 `wait_queue` 中的每个 `waiter` 加边 `waiter -> owner`；
- 对当前请求再额外加边 `requester -> owner_of_target`。

### 3.2 信号量

- 遍历进程内所有 `semaphore_list`：
  - 获取 `holders`（`count>0` 的 tid 列表）；
  - 对等待队列中的每个 `waiter`，对每个 `holder` 加边 `waiter -> holder`；
- 对当前请求再额外加边 `requester -> each(holder_of_target)`。

## 4. 环检测算法

- 函数列表：
  - `add_edge(graph, from, to)`：无自环，去重添加；
  - `detect_cycle(graph, start, target)`：判断从 `start` 是否存在到 `target` 的路径；
  - `has_path(graph, node, target, visited)`：DFS。
- 判定规则：
  - 互斥锁：检测 `owner_of_target ->* requester` 是否成立；
  - 信号量：对每个 `holder_of_target` 判定 `holder ->* requester` 是否成立。

## 5. 依赖与可见性

- 需要访问 `ProcessControlBlock`（通过 `pub use` 暴露）；
- 使用 `BTreeMap` / `BTreeSet` 存储图与访问集合；
- 通过 `Mutex::as_any()` 下转型，识别 `MutexBlocking`；
- 引用了 `TaskControlBlock::tid()` 方便提取线程标识。

## 6. 返回值与错误码

- 检测到潜在死锁：返回 `-0xDEAD`；
- 参数非法：返回 `-1`；
- 正常路径：在实际 `lock()` / `down()` 调用后返回 `0`。

## 7. 设计权衡与局限

- 仅在“可能阻塞”的路径上做检测（如互斥锁被持有、信号量无可用且有人持有），降低无谓开销；
- 未考虑不同同步原语的交叉（如 condvar/waittid 与锁/信号量混合），按题目要求可忽略；
- 图为每次临时构建，代码更简单，代价是重复开销，若后续性能不够可做增量缓存。

## 8. 小结

`syscall/sync.rs` 中的改动提供了死锁检测的开关和完整检测路径，利用等待图 + DFS 的方式在阻塞前拦截潜在环等待，确保系统在开启检测模式时避免进入不可恢复的死锁状态。
