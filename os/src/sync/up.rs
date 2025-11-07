//! 面向单核处理器（单个 CPU 核心）的安全数据封装
//!
//! UPSafeCell 用于包裹静态数据结构，以确保访问安全。
//!
//! 注意：仅应在单核（单个 CPU 核心）环境中使用，且内核在内核态下不支持任务抢占或陷入。

use core::cell::{RefCell, RefMut};

/// 包裹一个静态数据结构，使我们无需编写 `unsafe` 即可访问。
///
/// 仅应在单核环境中使用。
///
/// 通过调用 `exclusive_access` 获取内部数据的可变引用。
pub struct UPSafeCell<T> {
    /// 内部数据
    inner: RefCell<T>,
}

unsafe impl<T> Sync for UPSafeCell<T> {}

impl<T> UPSafeCell<T> {
    /// 调用者需确保内部结构仅在单核环境下使用。
    ///
    /// # Safety
    /// 调用者必须确保受保护的数据只会在单个 CPU 核心上访问，且内核态不会被抢占。
    pub unsafe fn new(value: T) -> Self {
        Self {
            inner: RefCell::new(value),
        }
    }
    /// 如数据已被借用则会 panic。
    pub fn exclusive_access(&self) -> RefMut<'_, T> {
        self.inner.borrow_mut()
    }
}
