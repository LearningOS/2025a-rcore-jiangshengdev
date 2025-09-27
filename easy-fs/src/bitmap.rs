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
    let block_pos = bit / BLOCK_BITS;
    bit %= BLOCK_BITS;
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
        for block_id in 0..self.blocks {
            let pos = get_block_cache(
                block_id + self.start_block_id as usize,
                Arc::clone(block_device),
            )
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                if let Some((bits64_pos, inner_pos)) = bitmap_block
                    .iter()
                    .enumerate()
                    .find(|(_, bits64)| **bits64 != u64::MAX)
                    .map(|(bits64_pos, bits64)| (bits64_pos, bits64.trailing_ones() as usize))
                {
                    // 修改缓存
                    bitmap_block[bits64_pos] |= 1u64 << inner_pos;
                    Some(block_id * BLOCK_BITS + bits64_pos * 64 + inner_pos as usize)
                } else {
                    None
                }
            });
            if pos.is_some() {
                return pos;
            }
        }
        None
    }
    /// 释放一个块
    ///
    /// # 参数
    /// * `block_device` - 块设备引用
    /// * `bit` - 要释放的块编号
    pub fn dealloc(&self, block_device: &Arc<dyn BlockDevice>, bit: usize) {
        let (block_pos, bits64_pos, inner_pos) = decomposition(bit);
        get_block_cache(block_pos + self.start_block_id, Arc::clone(block_device))
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                assert!(bitmap_block[bits64_pos] & (1u64 << inner_pos) > 0);
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
