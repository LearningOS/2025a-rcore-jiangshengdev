//! 单处理器内部可变性原语
use core::cell::{RefCell, RefMut};

/// 将静态数据结构包装在其中，这样我们就能够
/// 在不使用任何 `unsafe` 的情况下访问它。
///
/// 我们应该只在单处理器中使用它。
///
/// 为了获取内部数据的可变引用，调用
/// `exclusive_access`。
pub struct UPSafeCell<T> {
    /// 内部数据
    inner: RefCell<T>,
}

unsafe impl<T> Sync for UPSafeCell<T> {}

impl<T> UPSafeCell<T> {
    /// # Safety
    /// 用户有责任保证内部结构只在
    /// 单处理器中使用。
    pub unsafe fn new(value: T) -> Self {
        Self {
            // 使用RefCell提供内部可变性
            inner: RefCell::new(value),
        }
    }
    /// 如果数据已被借用则会 panic。
    pub fn exclusive_access(&self) -> RefMut<'_, T> {
        // 获取内部数据的可变引用，如果已被借用则panic
        self.inner.borrow_mut()
    }
}
