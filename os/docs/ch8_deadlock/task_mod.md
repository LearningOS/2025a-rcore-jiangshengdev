# task/mod.rs 改动详解

## 1. 改动内容

将原先的 `use process::ProcessControlBlock;` 修改为 `pub use process::ProcessControlBlock;`，即重新导出（re-export）`ProcessControlBlock` 类型。

## 2. 目的

- 允许其他模块通过 `crate::task::ProcessControlBlock` 直接引用类型，而无需深入到 `process` 子模块路径。
- 死锁检测逻辑位于 `syscall/sync.rs`，它需要访问 `ProcessControlBlock` 类型（尤其是内部标志 `deadlock_detect`），公开 re-export 提升可读性并减少耦合路径长度。

## 3. 影响

- 对现有调用方：旧代码仍可使用原路径（若之前显式引用子模块），新增路径提供使用便利。
- 风险：若未来需要隐藏 `ProcessControlBlock` 的内部实现细节，需再度封装访问 API，但当前实验场景可以接受。

## 4. 可选改进

- 提供专门的 getter：`fn current_process_deadlock_enabled() -> bool` 减少对 PCB 结构的直接依赖。

## 5. 小结

该改动是一个纯导出层优化，减少死锁检测相关模块引入 PCB 类型的认知成本，无功能性副作用。
