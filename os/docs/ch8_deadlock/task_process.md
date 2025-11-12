# task/process.rs 改动详解

## 1. 新增字段

在 `ProcessControlBlockInner` 中加入：

```rust
pub deadlock_detect: bool,
```

用于标记该进程是否开启死锁检测。默认值为 `false`（在 `new()` 构造时初始化）。

## 2. fork 行为

创建子进程时：

```rust
let detect_enabled = parent.deadlock_detect;
...
child_inner.deadlock_detect = detect_enabled;
```

即子进程继承父进程的死锁检测状态，确保行为一致（避免父、子进程在共享同步资源时出现策略不一致）。

## 3. 与系统调用的交互

- `sys_enable_deadlock_detect(enabled)` 会直接修改当前进程的该字段。
- `sys_mutex_lock` / `sys_semaphore_down` 在检测前读取该布尔值，决定是否进行图构建与环检测。

## 4. 设计理由

- 进程级而非线程级：死锁检测的开销和策略对整个资源图一致，线程级开关会导致图构建语义复杂（部分线程参与检测，部分不参与）。
- 继承行为防止出现父进程开启检测、子进程未开启而二者共享互斥锁/信号量导致“不一致边建模”。

## 5. 可扩展方向

- 统计字段：如 `deadlock_detect_attempts`、`deadlock_detect_hits` 用于诊断实际死锁情况和性能影响。
- 允许运行时动态调整策略级别（例如仅检测互斥锁 / 检测互斥锁与信号量）。

## 6. 小结

该文件的改动为死锁检测提供了生命周期管理与继承语义，开关由进程粒度控制，保持实现简洁并避免资源建模不一致。
