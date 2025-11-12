//! Synchronization and interior mutability primitives

mod condvar;
// 引入死锁检测模块以供同步原语使用。
mod deadlock;
mod mutex;
mod semaphore;
mod up;

pub use condvar::Condvar;
// 对外暴露死锁检测器，供进程控制和系统调用引用。
pub use deadlock::DeadlockDetector;
pub use mutex::{Mutex, MutexBlocking, MutexSpin};
pub use semaphore::Semaphore;
pub use up::UPSafeCell;
