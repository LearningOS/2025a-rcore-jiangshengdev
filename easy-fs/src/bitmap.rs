//! Disk layout & data structure layer: about bitmaps
//!
//! There are two different types of [`Bitmap`] in the easy-fs layout that manage inodes and
//! blocks, respectively. Each bitmap consists of several logical blocks with size [`BLOCK_SZ`].
//! Each bit represents the allocation status of an inode/data block, 0 means unallocated, and 1
//! means allocated. Bitmap allocation simply finds a zero bit and flips it to one; deallocation
//! resets the bit back to zero.
use super::{get_block_cache, BlockDevice, BLOCK_SZ};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};
/// A bitmap block stored on disk
type BitmapBlock = [u64; BLOCK_SZ / core::mem::size_of::<u64>()];
/// Number of bits represented by one bitmap block
pub const BLOCK_BITS: usize = BLOCK_SZ * 8;
/// bitmap struct for disk block management
pub struct Bitmap {
    start_block_id: usize,
    blocks: usize,
    next_hint: AtomicUsize,
}

/// Decompose bits into (block_pos, bits64_pos, inner_pos)
fn decomposition(mut bit: usize) -> (usize, usize, usize) {
    let block_pos = bit / BLOCK_BITS;
    bit %= BLOCK_BITS;
    (block_pos, bit / 64, bit % 64)
}

impl Bitmap {
    /// Create new bitmap blocks
    pub fn new(start_block_id: usize, blocks: usize) -> Self {
        Self {
            start_block_id,
            blocks,
            next_hint: AtomicUsize::new(0),
        }
    }
    /// Allocate a block according to the bitmap info
    pub fn alloc(&self, block_device: &Arc<dyn BlockDevice>) -> Option<usize> {
        let start = self.next_hint.load(Ordering::Relaxed) % self.blocks;
        for offset in 0..self.blocks {
            let block_id = (start + offset) % self.blocks;
            let pos = get_block_cache(block_id + self.start_block_id, Arc::clone(block_device))
                .lock()
                .modify(0, |bitmap_block: &mut BitmapBlock| {
                    if let Some((bits64_pos, inner_pos)) = bitmap_block
                        .iter()
                        .enumerate()
                        .find(|(_, bits64)| **bits64 != u64::MAX)
                        .map(|(bits64_pos, bits64)| (bits64_pos, bits64.trailing_ones() as usize))
                    {
                        // modify cache
                        bitmap_block[bits64_pos] |= 1u64 << inner_pos;
                        Some(block_id * BLOCK_BITS + bits64_pos * 64 + inner_pos)
                    } else {
                        None
                    }
                });
            if let Some(bit_pos) = pos {
                let next = (block_id + 1) % self.blocks;
                self.next_hint.store(next, Ordering::Relaxed);
                return Some(bit_pos);
            }
        }
        None
    }
    /// Deallocate a block according to the bitmap info
    pub fn dealloc(&self, block_device: &Arc<dyn BlockDevice>, bit: usize) {
        let (block_pos, bits64_pos, inner_pos) = decomposition(bit);
        get_block_cache(block_pos + self.start_block_id, Arc::clone(block_device))
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                assert!(bitmap_block[bits64_pos] & (1u64 << inner_pos) > 0);
                bitmap_block[bits64_pos] -= 1u64 << inner_pos;
            });
    }
    /// bitmap max size in bits(the max number of blocks according to the bitmap size)
    pub fn maximum(&self) -> usize {
        self.blocks * BLOCK_BITS
    }
}
