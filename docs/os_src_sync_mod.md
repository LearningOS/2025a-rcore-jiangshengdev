# os/src/sync/mod.rs 变更说明

## 调整摘要

- 新引入的 `deadlock` 子模块在此完成注册，并向外导出了 `DeadlockDetector` 类型。

## 影响

- 允许其他模块通过 `crate::sync::DeadlockDetector` 访问死锁检测逻辑，与已有的互斥锁、信号量等同步原语保持同一命名空间。
