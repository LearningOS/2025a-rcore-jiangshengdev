//! 全局分配器
use crate::config::KERNEL_HEAP_SIZE;
use buddy_system_allocator::LockedHeap;

#[global_allocator]
/// 堆分配器实例
static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

#[alloc_error_handler]
/// 当堆分配错误发生时触发 panic
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    // 堆分配失败时的错误处理，打印布局信息并终止程序
    panic!("堆分配错误，layout = {:?}", layout);
}
/// 堆空间 ([u8; KERNEL_HEAP_SIZE])
static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];
/// 初始化堆分配器
pub fn init_heap() {
    unsafe {
        // 使用静态分配的堆空间初始化buddy系统分配器
        HEAP_ALLOCATOR
            .lock()
            .init(HEAP_SPACE.as_ptr() as usize, KERNEL_HEAP_SIZE);
    }
}

#[allow(unused)]
pub fn heap_test() {
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    extern "C" {
        fn sbss();
        fn ebss();
    }
    // 获取BSS段地址范围，用于验证堆分配的内存位置
    let bss_range = sbss as usize..ebss as usize;
    // 测试Box分配
    let a = Box::new(5);
    assert_eq!(*a, 5);
    assert!(bss_range.contains(&(a.as_ref() as *const _ as usize)));
    drop(a);
    // 测试Vec分配和扩容
    let mut v: Vec<usize> = Vec::new();
    for i in 0..500 {
        v.push(i);
    }
    // 验证数据正确性
    for (i, val) in v.iter().take(500).enumerate() {
        assert_eq!(*val, i);
    }
    // 验证内存分配位置
    assert!(bss_range.contains(&(v.as_ptr() as usize)));
    drop(v);
    println!("堆测试通过！");
}
