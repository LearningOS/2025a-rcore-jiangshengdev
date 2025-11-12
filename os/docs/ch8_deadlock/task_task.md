# task/task.rs 改动详解

## 1. 新增方法

```rust
pub fn tid(&self) -> usize
```

返回该 `TaskControlBlock` 的线程标识符 `tid`。

## 2. 实现细节

- 通过 `self.inner.exclusive_access().res.as_ref().expect(...).tid` 取得当前任务的 tid；
- 若任务已被回收（`res` 为 `None`），则触发 `expect` 中的调试信息，这在正常调度路径下不应发生。

## 3. 引入目的

- 提供简洁、统一的 `tid` 访问方式，避免在不同模块重复展开内部结构；
- 死锁检测中广泛需要使用 `tid`（构建等待图时的节点），例如：
  - 读取等待队列任务的 `tid()`；
  - 标记互斥锁 `owner_tid = Some(tid)`；
  - 信号量 `holders` 的键即为 `tid`。

## 4. 影响范围

- 外部模块可直接调用 `tcb.tid()`，减少对内部结构的耦合；
- 提升可读性与可维护性。

## 5. 小结

新增的 `tid()` 是一个小而关键的辅助接口，使等待图构建与日志输出更自然，降低模块间的结构依赖。
