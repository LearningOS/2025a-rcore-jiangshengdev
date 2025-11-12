# mutex.rs 改动详解

## 1. 改动综述

补丁在 `os/src/sync/mutex.rs` 中：

1. 为阻塞型互斥锁新增持有者跟踪字段 `owner_tid: Option<usize>`；
2. 将其 `inner`、`wait_queue`、`owner_tid` 暂时公开以方便死锁检测读取；
3. 在 `Mutex` trait 中新增 `as_any()` 以支持对 `Arc<dyn Mutex>` 进行运行时类型识别；
4. 在加锁/解锁路径维护持有者转移；
   帮助死锁检测构建等待图（waiter -> owner）。

## 2. 新增内容

| 元素                                          | 类型/签名        | 目的                                                              |
| --------------------------------------------- | ---------------- | ----------------------------------------------------------------- |
| `use core::any::Any`                          | 引入标准库 trait | 支持动态类型转换                                                  |
| `Mutex::as_any(&self) -> &dyn Any`            | trait 方法       | 允许对 `Arc<dyn Mutex>` 做 `downcast_ref::<MutexBlocking>()` 检测 |
| `MutexBlockingInner.owner_tid: Option<usize>` | 字段             | 记录当前锁的拥有线程 id（tid），无锁或无持有者时为 `None`         |
| `pub inner` / `pub wait_queue` 暂时公开       | 字段可见性调整   | 死锁检测模块读取内部状态（简化实现）                              |

## 3. 逻辑调整

### 3.1 `lock()`

- 原逻辑：若已加锁则入等待队列阻塞；否则设置 `locked = true`。
- 现逻辑：在成功获取锁时除设置 `locked = true` 外，额外写入 `owner_tid = Some(current_tid)`。

### 3.2 `unlock()`

- 原逻辑：若等待队列非空就唤醒一个并保持 `locked = true`；否则置 `locked = false`。
- 现逻辑：
  - 若唤醒等待者：在保持锁定状态的同时把 `owner_tid` 设为唤醒任务的 tid（提前标记其未来持有权）。
  - 无等待者：释放锁并清空 `owner_tid`。

### 3.3 `as_any()` 实现

`MutexSpin` 与 `MutexBlocking` 均实现 `as_any()`，检测逻辑通过 `downcast_ref::<MutexBlocking>()` 筛选出阻塞型互斥锁（自旋锁不参与等待队列图）。

## 4. 设计权衡

- 公开内部字段（`pub`）是为了减少额外 Getter 编写工作（实验性质），牺牲一定封装性；后续可引入只读访问器以恢复封装。
- 所有权转移提前在 `unlock()` 中完成而非等待者重新进入 `lock()` 流程后再写入，能让等待图更及时、避免图构建后需特殊处理“即将被切换的持有者”情形；潜在误差（被唤醒任务在抢占前被 kill）在当前实验范围可忽略。

## 5. 对死锁检测的支持点

- `owner_tid` 与 `wait_queue` 搭配生成边：`waiter_tid -> owner_tid`。
- 通过 `as_any()` 判断是否为阻塞型互斥锁，避免对自旋锁进行无意义检测。

## 6. 潜在改进

- 进一步抽象：提供 `fn owner()` / `fn waiters()` 只读接口而不暴露结构。
- 支持递归锁：可扩展 `owner_tid` 为 `(tid, depth)`。

## 7. 小结

该文件的改动是互斥锁死锁检测的数据基础：新增持有者跟踪 + 类型识别接口，保持原有 API 不变，侵入性较低。
