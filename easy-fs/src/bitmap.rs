use super::{get_block_cache, BlockDevice, BLOCK_SZ};
use alloc::sync::Arc;
/// 位图块
type BitmapBlock = [u64; 64];
/// 一个块中的位数
const BLOCK_BITS: usize = BLOCK_SZ * 8;
/// 位图结构，用于管理块的分配和释放
pub struct Bitmap {
    /// 位图起始块编号
    start_block_id: usize,
    /// 位图占用的块数
    blocks: usize,
}

/// 将位分解为 (block_pos, bits64_pos, inner_pos)
///
/// # 参数
/// * `bit` - 要分解的位索引
///
/// # 返回
/// 返回元组 (块位置, 64位段位置, 段内位置)
fn decomposition(mut bit: usize) -> (usize, usize, usize) {
    // 计算位所在的位图块编号
    let block_pos = bit / BLOCK_BITS;
    // 计算位在块内的偏移
    bit %= BLOCK_BITS;
    // 返回：(位图块位置, 64位段位置, 段内位位置)
    (block_pos, bit / 64, bit % 64)
}

impl Bitmap {
    /// 根据起始块编号和块数创建新的位图
    ///
    /// # 参数
    /// * `start_block_id` - 位图起始块编号
    /// * `blocks` - 位图占用的块数
    ///
    /// # 返回
    /// 新创建的位图实例
    pub fn new(start_block_id: usize, blocks: usize) -> Self {
        Self {
            start_block_id,
            blocks,
        }
    }
    /// 从块设备分配新块
    ///
    /// # 参数
    /// * `block_device` - 块设备引用
    ///
    /// # 返回
    /// 成功时返回分配的块编号，失败时返回None
    pub fn alloc(&self, block_device: &Arc<dyn BlockDevice>) -> Option<usize> {
        // 遍历位图的每个块，寻找可用的位
        for block_id in 0..self.blocks {
            // 获取当前位图块的缓存
            let pos = get_block_cache(
                block_id + self.start_block_id as usize,
                Arc::clone(block_device),
            )
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                // 在位图块中查找第一个未满的64位段
                // u64::MAX表示所有位都被占用
                if let Some((bits64_pos, inner_pos)) = bitmap_block
                    .iter()
                    .enumerate()
                    .find(|(_, bits64)| **bits64 != u64::MAX)
                    // 使用trailing_ones()找到第一个0位的位置
                    // trailing_ones()返回从最低位开始连续1的个数
                    .map(|(bits64_pos, bits64)| (bits64_pos, bits64.trailing_ones() as usize))
                {
                    // 将找到的位设置为1，表示已分配
                    bitmap_block[bits64_pos] |= 1u64 << inner_pos;
                    // 计算全局位索引：块偏移 + 64位段偏移 + 段内偏移
                    let global_bit_index = block_id * BLOCK_BITS + bits64_pos * 64 + inner_pos;
                    Some(global_bit_index)
                } else {
                    // 当前块已满，返回None继续查找下一个块
                    None
                }
            });
            // 如果在当前块中找到了可用位，直接返回
            if pos.is_some() {
                return pos;
            }
        }
        // 所有块都已满，分配失败
        None
    }
    /// 释放一个块
    ///
    /// # 参数
    /// * `block_device` - 块设备引用
    /// * `bit` - 要释放的块编号
    pub fn dealloc(&self, block_device: &Arc<dyn BlockDevice>, bit: usize) {
        // 将全局位索引分解为具体的位置信息
        let (block_pos, bits64_pos, inner_pos) = decomposition(bit);
        // 计算位图块的块编号
        let bitmap_block_id = block_pos + self.start_block_id;
        // 获取对应位图块的缓存
        get_block_cache(bitmap_block_id, Arc::clone(block_device))
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                // 确保要释放的位确实是已分配状态（值为1）
                assert!(bitmap_block[bits64_pos] & (1u64 << inner_pos) > 0);
                // 将对应位清零，表示释放该块
                // 使用减法而不是异或，因为我们确定该位为1
                bitmap_block[bits64_pos] -= 1u64 << inner_pos;
            });
    }
    /// 获取最大可分配块数
    ///
    /// # 返回
    /// 位图能管理的最大块数
    pub fn maximum(&self) -> usize {
        self.blocks * BLOCK_BITS
    }
}
