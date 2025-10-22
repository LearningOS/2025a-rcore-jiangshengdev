use super::{BlockDevice, BLOCK_SZ};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{compiler_fence, Ordering};
use lazy_static::*;
use spin::Mutex;
/// 内存中的缓存块，用于缓存磁盘块数据以提高访问性能
pub struct BlockCache {
    /// 缓存的块数据，大小为BLOCK_SZ字节
    cache: Vec<u8>,
    /// 对应的磁盘块编号
    block_id: usize,
    /// 底层块设备的引用
    block_device: Arc<dyn BlockDevice>,
    /// 标记块数据是否被修改过，用于决定是否需要写回磁盘
    modified: bool,
}

impl BlockCache {
    /// 从磁盘加载新的块缓存
    ///
    /// # 参数
    /// * `block_id` - 要加载的块编号
    /// * `block_device` - 块设备引用
    ///
    /// # 返回
    /// 新创建的块缓存实例
    pub fn new(block_id: usize, block_device: Arc<dyn BlockDevice>) -> Self {
        // 创建空的缓存数组
        let mut cache = vec![0u8; BLOCK_SZ];
        // 从块设备读取数据到缓存
        block_device.read_block(block_id, &mut cache);
        // 创建新的块缓存实例，初始状态为未修改
        Self {
            cache,
            block_id,
            block_device,
            modified: false,
        }
    }
    /// 获取缓存块数据中某偏移的地址
    ///
    /// # 参数
    /// * `offset` - 偏移量
    ///
    /// # 返回
    /// 对应偏移位置的内存地址
    fn addr_of_offset(&self, offset: usize) -> usize {
        &self.cache[offset] as *const _ as usize
    }

    /// 获取指定偏移处的只读引用
    ///
    /// # 参数
    /// * `offset` - 偏移量
    ///
    /// # 返回
    /// 指定类型T的只读引用
    pub fn get_ref<T>(&self, offset: usize) -> &T
    where
        T: Sized,
    {
        // 获取类型T的大小
        let type_size = core::mem::size_of::<T>();
        // 确保偏移量和类型大小不会超出块边界
        assert!(offset + type_size <= BLOCK_SZ);
        // 获取偏移位置的内存地址
        let addr = self.addr_of_offset(offset);
        // 将地址转换为类型T的引用
        unsafe { &*(addr as *const T) }
    }

    /// 获取指定偏移处的可变引用
    ///
    /// # 参数
    /// * `offset` - 偏移量
    ///
    /// # 返回
    /// 指定类型T的可变引用
    pub fn get_mut<T>(&mut self, offset: usize) -> &mut T
    where
        T: Sized,
    {
        // 获取类型T的大小
        let type_size = core::mem::size_of::<T>();
        // 确保偏移量和类型大小不会超出块边界
        assert!(offset + type_size <= BLOCK_SZ);
        // 标记块已被修改，需要写回磁盘
        self.modified = true;
        // 获取偏移位置的内存地址
        let addr = self.addr_of_offset(offset);
        // 将地址转换为类型T的可变引用
        unsafe { &mut *(addr as *mut T) }
    }

    /// 只读访问指定偏移处的数据
    ///
    /// # 参数
    /// * `offset` - 偏移量
    /// * `f` - 访问回调函数
    ///
    /// # 返回
    /// 回调函数的返回值
    pub fn read<T, V>(&self, offset: usize, f: impl FnOnce(&T) -> V) -> V {
        f(self.get_ref(offset))
    }

    /// 可写访问指定偏移处的数据
    ///
    /// # 参数
    /// * `offset` - 偏移量
    /// * `f` - 修改回调函数
    ///
    /// # 返回
    /// 回调函数的返回值
    pub fn modify<T, V>(&mut self, offset: usize, f: impl FnOnce(&mut T) -> V) -> V {
        f(self.get_mut(offset))
    }

    /// 将缓存同步到块设备
    /// 如果块被修改过，则将数据写回到磁盘
    pub fn sync(&mut self) {
        // 只有在块被修改过的情况下才需要写回
        if self.modified {
            // 重置修改标志
            self.modified = false;
            // 将缓存数据写回到块设备
            self.block_device.write_block(self.block_id, &self.cache);
        }
    }
}

impl Drop for BlockCache {
    /// 在BlockCache被销毁时自动同步到块设备
    fn drop(&mut self) {
        self.sync()
    }
}
/// 使用16个块的块缓存
const BLOCK_CACHE_SIZE: usize = 16;

/// 块缓存管理器，使用LRU策略管理多个块缓存
pub struct BlockCacheManager {
    /// 缓存队列，存储(块编号, 块缓存)对
    queue: VecDeque<(usize, Arc<Mutex<BlockCache>>)>,
}

impl BlockCacheManager {
    /// 创建新的块缓存管理器
    ///
    /// # 返回
    /// 新的块缓存管理器实例
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    /// 获取指定块编号和设备的块缓存
    ///
    /// # 参数
    /// * `block_id` - 块编号
    /// * `block_device` - 块设备引用
    ///
    /// # 返回
    /// 块缓存的Arc<Mutex<BlockCache>>引用
    pub fn get_block_cache(
        &mut self,
        block_id: usize,
        block_device: Arc<dyn BlockDevice>,
    ) -> Arc<Mutex<BlockCache>> {
        // 首先检查缓存中是否已存在该块
        if let Some(pair) = self.queue.iter().find(|pair| {
            compiler_fence(Ordering::SeqCst);
            pair.0 == block_id
        }) {
            // 缓存命中，直接返回现有的块缓存
            Arc::clone(&pair.1)
        } else {
            // 缓存未命中，需要加载新块
            // 检查缓存是否已满
            if self.queue.len() == BLOCK_CACHE_SIZE {
                // 缓存已满，需要使用LRU策略替换
                // 查找引用计数为1的块缓存（只被管理器持有，没有外部引用）
                if let Some((idx, _)) = self.queue.iter().enumerate().find(|(_, pair)| {
                    compiler_fence(Ordering::SeqCst);
                    Arc::strong_count(&pair.1) == 1
                }) {
                    // 找到可替换的块，将其从队列中移除
                    // 由于Arc的Drop特性，块缓存会自动同步到磁盘
                    self.queue.drain(idx..=idx);
                } else {
                    // 所有块都有外部引用，无法替换，这是一个错误情况
                    panic!("Run out of BlockCache!");
                }
            }
            // 创建新的块缓存并从磁盘加载数据
            let block_cache = Arc::new(Mutex::new(BlockCache::new(
                block_id,
                Arc::clone(&block_device),
            )));
            // 将新块缓存添加到队列末尾（最近使用）
            self.queue.push_back((block_id, Arc::clone(&block_cache)));
            block_cache
        }
    }
}

lazy_static! {
    /// 全局块缓存管理器
    pub static ref BLOCK_CACHE_MANAGER: Mutex<BlockCacheManager> =
        Mutex::new(BlockCacheManager::new());
}
/// 获取给定块编号和块设备对应的块缓存
///
/// # 参数
/// * `block_id` - 块编号
/// * `block_device` - 块设备引用
///
/// # 返回
/// 块缓存的Arc<Mutex<BlockCache>>引用
pub fn get_block_cache(
    block_id: usize,
    block_device: Arc<dyn BlockDevice>,
) -> Arc<Mutex<BlockCache>> {
    BLOCK_CACHE_MANAGER
        .lock()
        .get_block_cache(block_id, block_device)
}
/// 将所有块缓存同步到块设备
pub fn block_cache_sync_all() {
    // 获取全局缓存管理器的锁
    let manager = BLOCK_CACHE_MANAGER.lock();
    // 遍历所有缓存的块
    for (_, cache) in manager.queue.iter() {
        // 同步每个块缓存到磁盘
        cache.lock().sync();
    }
}
