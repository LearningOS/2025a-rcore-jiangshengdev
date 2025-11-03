use super::{get_block_cache, BlockDevice, BLOCK_SZ};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::{Debug, Formatter, Result};
use core::ops::Range;

/// Magic number for sanity check
const EFS_MAGIC: u32 = 0x3b800001;
/// The max number of direct inodes
const INODE_DIRECT_COUNT: usize = 26;
/// The max length of inode name
const NAME_LENGTH_LIMIT: usize = 27;
/// The max number of indirect1 inodes
const INODE_INDIRECT1_COUNT: usize = BLOCK_SZ / 4;
/// The max number of indirect2 inodes
const INODE_INDIRECT2_COUNT: usize = INODE_INDIRECT1_COUNT * INODE_INDIRECT1_COUNT;
/// The max number of indirect3 inodes
const INODE_INDIRECT3_COUNT: usize = INODE_INDIRECT2_COUNT * INODE_INDIRECT1_COUNT;
/// The upper bound of direct inode index
const DIRECT_BOUND: usize = INODE_DIRECT_COUNT;
/// The upper bound of indirect1 inode index
const INDIRECT1_BOUND: usize = DIRECT_BOUND + INODE_INDIRECT1_COUNT;
/// The upper bound of indirect2 inode indexs
#[allow(unused)]
const INDIRECT2_BOUND: usize = INDIRECT1_BOUND + INODE_INDIRECT2_COUNT;
/// The upper bound of indirect3 inode indexs
#[allow(unused)]
const INDIRECT3_BOUND: usize = INDIRECT2_BOUND + INODE_INDIRECT3_COUNT;
/// Super block of a filesystem
#[repr(C)]
pub struct SuperBlock {
    magic: u32,
    pub total_blocks: u32,
    pub inode_bitmap_blocks: u32,
    pub inode_area_blocks: u32,
    pub data_bitmap_blocks: u32,
    pub data_area_blocks: u32,
}

impl Debug for SuperBlock {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("SuperBlock")
            .field("total_blocks", &self.total_blocks)
            .field("inode_bitmap_blocks", &self.inode_bitmap_blocks)
            .field("inode_area_blocks", &self.inode_area_blocks)
            .field("data_bitmap_blocks", &self.data_bitmap_blocks)
            .field("data_area_blocks", &self.data_area_blocks)
            .finish()
    }
}

impl SuperBlock {
    /// Initialize a super block
    pub fn initialize(
        &mut self,
        total_blocks: u32,
        inode_bitmap_blocks: u32,
        inode_area_blocks: u32,
        data_bitmap_blocks: u32,
        data_area_blocks: u32,
    ) {
        *self = Self {
            magic: EFS_MAGIC,
            total_blocks,
            inode_bitmap_blocks,
            inode_area_blocks,
            data_bitmap_blocks,
            data_area_blocks,
        }
    }
    /// Check if a super block is valid using efs magic
    pub fn is_valid(&self) -> bool {
        self.magic == EFS_MAGIC
    }
}
/// Type of a disk inode
#[derive(Clone, Copy, PartialEq)]
pub enum DiskInodeType {
    /// Regular file.
    File,
    /// Directory.
    Directory,
}

/// A indirect block
type IndirectBlock = [u32; BLOCK_SZ / 4];
/// A data block
type DataBlock = [u8; BLOCK_SZ];
/// Leaf interval within a specific indirect level.
#[derive(Clone, Copy)]
struct LevelSlice {
    start: usize,
    end: usize,
}

impl LevelSlice {
    fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    fn is_empty(&self) -> bool {
        self.start == self.end
    }
}
/// A disk inode
#[repr(C)]
pub struct DiskInode {
    pub size: u32,
    pub direct: [u32; INODE_DIRECT_COUNT],
    pub indirect1: u32,
    pub indirect2: u32,
    pub indirect3: u32,
    pub nlink: u32,
    type_: DiskInodeType,
}

fn access_indirect1_mut(inode: &mut DiskInode) -> &mut u32 {
    &mut inode.indirect1
}

fn access_indirect1_ref(inode: &DiskInode) -> &u32 {
    &inode.indirect1
}

fn access_indirect2_mut(inode: &mut DiskInode) -> &mut u32 {
    &mut inode.indirect2
}

fn access_indirect2_ref(inode: &DiskInode) -> &u32 {
    &inode.indirect2
}

fn access_indirect3_mut(inode: &mut DiskInode) -> &mut u32 {
    &mut inode.indirect3
}

fn access_indirect3_ref(inode: &DiskInode) -> &u32 {
    &inode.indirect3
}

/// Static description of an indirect level in the inode tree.
#[derive(Clone, Copy)]
struct LevelSpec {
    base: usize,
    capacity: usize,
    depth: usize,
    accessor_mut: fn(&mut DiskInode) -> &mut u32,
    accessor_ref: fn(&DiskInode) -> &u32,
}

impl LevelSpec {
    const fn new(
        base: usize,
        capacity: usize,
        depth: usize,
        accessor_mut: fn(&mut DiskInode) -> &mut u32,
        accessor_ref: fn(&DiskInode) -> &u32,
    ) -> Self {
        Self {
            base,
            capacity,
            depth,
            accessor_mut,
            accessor_ref,
        }
    }

    fn pointer_mut<'a>(&self, inode: &'a mut DiskInode) -> &'a mut u32 {
        (self.accessor_mut)(inode)
    }

    fn pointer_ref<'a>(&self, inode: &'a DiskInode) -> &'a u32 {
        (self.accessor_ref)(inode)
    }

    const fn upper_bound(&self) -> usize {
        self.base + self.capacity
    }
}

/// Metadata for every indirect layer, ordered from shallow to deep.
const LEVEL_SPECS: [LevelSpec; 3] = [
    LevelSpec::new(
        DIRECT_BOUND,
        INODE_INDIRECT1_COUNT,
        1,
        access_indirect1_mut,
        access_indirect1_ref,
    ),
    LevelSpec::new(
        INDIRECT1_BOUND,
        INODE_INDIRECT2_COUNT,
        2,
        access_indirect2_mut,
        access_indirect2_ref,
    ),
    LevelSpec::new(
        INDIRECT2_BOUND,
        INODE_INDIRECT3_COUNT,
        3,
        access_indirect3_mut,
        access_indirect3_ref,
    ),
];

impl DiskInode {
    /// Initialize a disk inode, as well as all direct inodes under it
    /// indirect1 and indirect2 block are allocated only when they are needed
    pub fn initialize(&mut self, type_: DiskInodeType) {
        self.size = 0;
        self.direct.iter_mut().for_each(|v| *v = 0);
        self.indirect1 = 0;
        self.indirect2 = 0;
        self.indirect3 = 0;
        self.nlink = 1;
        self.type_ = type_;
    }
    /// Whether this inode is a directory
    pub fn is_dir(&self) -> bool {
        self.type_ == DiskInodeType::Directory
    }
    /// Whether this inode is a file
    #[allow(unused)]
    pub fn is_file(&self) -> bool {
        self.type_ == DiskInodeType::File
    }
    /// Increase hard link reference count.
    pub fn inc_nlink(&mut self) -> u32 {
        self.nlink += 1;
        self.nlink
    }
    /// Decrease hard link reference count and return the new value.
    pub fn dec_nlink(&mut self) -> u32 {
        assert!(self.nlink > 0);
        self.nlink -= 1;
        self.nlink
    }
    /// Get current hard link count.
    pub fn nlink(&self) -> u32 {
        self.nlink
    }
    /// Get inode type.
    pub fn inode_type(&self) -> DiskInodeType {
        self.type_
    }
    /// Return block number correspond to size.
    pub fn data_blocks(&self) -> u32 {
        Self::_data_blocks(self.size)
    }
    fn _data_blocks(size: u32) -> u32 {
        (size + BLOCK_SZ as u32 - 1) / BLOCK_SZ as u32
    }
    /// Return number of blocks needed include indirect1/2.
    pub fn total_blocks(size: u32) -> u32 {
        let data_blocks = Self::_data_blocks(size) as usize;
        let mut total = data_blocks;
        for level in LEVEL_SPECS.iter() {
            let leaves = Self::level_usage(data_blocks, level.base, level.capacity);
            total += Self::metadata_blocks_for(leaves, level.depth);
        }
        total as u32
    }

    /// Helper to build tree recursively
    /// extend number of leaves within `leaf_range`
    fn build_tree(
        blocks: &mut alloc::vec::IntoIter<u32>,
        block_id: u32,
        mut cur_leaf: usize,
        leaf_range: Range<usize>,
        depth: Range<usize>,
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        if depth.start == depth.end {
            return cur_leaf + 1;
        }
        let next_depth = depth.start + 1..depth.end;
        get_block_cache(block_id as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect_block: &mut IndirectBlock| {
                let mut i = 0;
                while i < INODE_INDIRECT1_COUNT && cur_leaf < leaf_range.end {
                    if cur_leaf >= leaf_range.start {
                        indirect_block[i] = blocks.next().unwrap();
                    }
                    cur_leaf = Self::build_tree(
                        blocks,
                        indirect_block[i],
                        cur_leaf,
                        leaf_range.clone(),
                        next_depth.clone(),
                        block_device,
                    );
                    i += 1;
                }
            });
        cur_leaf
    }

    /// Compute the leaf range that should be updated at a specific level.
    fn level_range(prev: usize, target: usize, base: usize, capacity: usize) -> LevelSlice {
        let start = prev.saturating_sub(base).min(capacity);
        let end = target.saturating_sub(base).min(capacity);
        LevelSlice::new(start, end)
    }

    /// Return number of leaves from `total` that belong to a level starting at `base`.
    fn level_usage(total: usize, base: usize, capacity: usize) -> usize {
        total.saturating_sub(base).min(capacity)
    }

    /// Expand an indirect subtree so that it covers the desired leaf range.
    fn expand_indirect_level(
        pointer: &mut u32,
        slice: LevelSlice,
        capacity: usize,
        depth: usize,
        blocks: &mut alloc::vec::IntoIter<u32>,
        block_device: &Arc<dyn BlockDevice>,
    ) {
        debug_assert!(slice.start <= slice.end);
        debug_assert!(slice.end <= capacity);
        if slice.is_empty() {
            if slice.end > 0 {
                assert_ne!(*pointer, 0);
            }
            return;
        }
        if *pointer == 0 {
            assert_eq!(slice.start, 0);
            *pointer = blocks
                .next()
                .expect("no available block for indirect root allocation");
        }
        Self::build_tree(
            blocks,
            *pointer,
            0,
            slice.start..slice.end,
            0..depth,
            block_device,
        );
    }

    /// Reclaim an indirect subtree covering the given number of leaves.
    fn shrink_indirect_level(
        pointer: &mut u32,
        leaves: usize,
        capacity: usize,
        depth: usize,
        collected: &mut Vec<u32>,
        block_device: &Arc<dyn BlockDevice>,
    ) {
        debug_assert!(leaves <= capacity);
        if leaves == 0 {
            assert_eq!(*pointer, 0);
            return;
        }
        assert_ne!(*pointer, 0);
        collected.push(*pointer);
        Self::collect_tree_blocks(collected, *pointer, 0, 0..leaves, 0..depth, block_device);
        *pointer = 0;
    }

    /// Number of leaves covered by one child pointer at the given depth.
    fn subtree_span(depth: usize) -> usize {
        if depth == 0 {
            1
        } else {
            INODE_INDIRECT1_COUNT.pow((depth - 1) as u32)
        }
    }

    /// Traverse indirect levels to locate the data block at `offset`.
    fn descend_indirect(
        mut block_id: u32,
        mut offset: usize,
        mut depth: usize,
        block_device: &Arc<dyn BlockDevice>,
    ) -> u32 {
        while depth > 0 {
            assert_ne!(block_id, 0);
            let span = Self::subtree_span(depth);
            let index = offset / span;
            offset %= span;
            block_id = get_block_cache(block_id as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect: &IndirectBlock| indirect[index]);
            depth -= 1;
        }
        block_id
    }

    /// Return the number of metadata blocks needed to index `leaves` leaves.
    fn metadata_blocks_for(leaves: usize, depth: usize) -> usize {
        if depth == 0 || leaves == 0 {
            return 0;
        }
        let mut total = 1usize;
        let mut span = INODE_INDIRECT1_COUNT;
        for _ in 1..depth {
            let nodes = (leaves + span - 1) / span;
            total += nodes;
            span = span.saturating_mul(INODE_INDIRECT1_COUNT);
        }
        total
    }
    /// Get the number of data blocks that have to be allocated given the new size of data
    pub fn blocks_num_needed(&self, new_size: u32) -> u32 {
        assert!(new_size >= self.size);
        Self::total_blocks(new_size) - Self::total_blocks(self.size)
    }
    /// Get id of block given inner id
    pub fn get_block_id(&self, inner_id: u32, block_device: &Arc<dyn BlockDevice>) -> u32 {
        let inner_id = inner_id as usize;
        if inner_id < INODE_DIRECT_COUNT {
            self.direct[inner_id]
        } else {
            for level in LEVEL_SPECS.iter() {
                if inner_id < level.upper_bound() {
                    let root = *level.pointer_ref(self);
                    let offset = inner_id - level.base;
                    return Self::descend_indirect(root, offset, level.depth, block_device);
                }
            }
            panic!("inner_id {} out of range", inner_id);
        }
    }
    /// Inncrease the size of current disk inode
    pub fn increase_size(
        &mut self,
        new_size: u32,
        new_blocks: Vec<u32>,
        block_device: &Arc<dyn BlockDevice>,
    ) {
        let prev_blocks = self.data_blocks() as usize;
        self.size = new_size;
        let target_blocks = self.data_blocks() as usize;
        let mut blocks_iter = new_blocks.into_iter();

        let direct_start = prev_blocks.min(INODE_DIRECT_COUNT);
        let direct_end = target_blocks.min(INODE_DIRECT_COUNT);
        for idx in direct_start..direct_end {
            self.direct[idx] = blocks_iter.next().unwrap();
        }
        if target_blocks <= DIRECT_BOUND {
            return;
        }
        for level in LEVEL_SPECS.iter() {
            if target_blocks <= level.base {
                break;
            }
            let slice = Self::level_range(prev_blocks, target_blocks, level.base, level.capacity);
            Self::expand_indirect_level(
                level.pointer_mut(self),
                slice,
                level.capacity,
                level.depth,
                &mut blocks_iter,
                block_device,
            );
            if target_blocks <= level.upper_bound() {
                return;
            }
        }
        debug_assert!(target_blocks <= LEVEL_SPECS.last().unwrap().upper_bound());
    }

    /// Helper to recycle blocks recursively
    /// collect leaves covered by `leaf_range`
    fn collect_tree_blocks(
        collected: &mut Vec<u32>,
        block_id: u32,
        mut cur_leaf: usize,
        leaf_range: Range<usize>,
        depth: Range<usize>,
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        if depth.start == depth.end {
            return cur_leaf + 1;
        }
        let next_depth = depth.start + 1..depth.end;
        get_block_cache(block_id as usize, Arc::clone(block_device))
            .lock()
            .read(0, |indirect_block: &IndirectBlock| {
                let mut i = 0;
                while i < INODE_INDIRECT1_COUNT && cur_leaf < leaf_range.end {
                    if cur_leaf >= leaf_range.start {
                        collected.push(indirect_block[i]);
                    }
                    cur_leaf = Self::collect_tree_blocks(
                        collected,
                        indirect_block[i],
                        cur_leaf,
                        leaf_range.clone(),
                        next_depth.clone(),
                        block_device,
                    );
                    i += 1;
                }
            });
        cur_leaf
    }

    /// Clear size to zero and return blocks that should be deallocated.
    /// We will clear the block contents to zero later.
    pub fn clear_size(&mut self, block_device: &Arc<dyn BlockDevice>) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let data_blocks = self.data_blocks() as usize;
        self.size = 0;
        let direct_end = data_blocks.min(INODE_DIRECT_COUNT);
        for idx in 0..direct_end {
            v.push(self.direct[idx]);
            self.direct[idx] = 0;
        }

        for level in LEVEL_SPECS.iter() {
            let leaves = Self::level_usage(data_blocks, level.base, level.capacity);
            Self::shrink_indirect_level(
                level.pointer_mut(self),
                leaves,
                level.capacity,
                level.depth,
                &mut v,
                block_device,
            );
        }

        v
    }
    /// Read data from current disk inode
    pub fn read_at(
        &self,
        offset: usize,
        buf: &mut [u8],
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        if start >= end {
            return 0;
        }
        let mut start_block = start / BLOCK_SZ;
        let mut read_size = 0usize;
        loop {
            // calculate end of current block
            let mut end_current_block = (start / BLOCK_SZ + 1) * BLOCK_SZ;
            end_current_block = end_current_block.min(end);
            // read and update read size
            let block_read_size = end_current_block - start;
            let dst = &mut buf[read_size..read_size + block_read_size];
            get_block_cache(
                self.get_block_id(start_block as u32, block_device) as usize,
                Arc::clone(block_device),
            )
            .lock()
            .read(0, |data_block: &DataBlock| {
                let src = &data_block[start % BLOCK_SZ..start % BLOCK_SZ + block_read_size];
                dst.copy_from_slice(src);
            });
            read_size += block_read_size;
            // move to next block
            if end_current_block == end {
                break;
            }
            start_block += 1;
            start = end_current_block;
        }
        read_size
    }
    /// Write data into current disk inode
    /// size must be adjusted properly beforehand
    pub fn write_at(
        &mut self,
        offset: usize,
        buf: &[u8],
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        assert!(start <= end);
        let mut start_block = start / BLOCK_SZ;
        let mut write_size = 0usize;
        loop {
            // calculate end of current block
            let mut end_current_block = (start / BLOCK_SZ + 1) * BLOCK_SZ;
            end_current_block = end_current_block.min(end);
            // write and update write size
            let block_write_size = end_current_block - start;
            get_block_cache(
                self.get_block_id(start_block as u32, block_device) as usize,
                Arc::clone(block_device),
            )
            .lock()
            .modify(0, |data_block: &mut DataBlock| {
                let src = &buf[write_size..write_size + block_write_size];
                let dst = &mut data_block[start % BLOCK_SZ..start % BLOCK_SZ + block_write_size];
                dst.copy_from_slice(src);
            });
            write_size += block_write_size;
            // move to next block
            if end_current_block == end {
                break;
            }
            start_block += 1;
            start = end_current_block;
        }
        write_size
    }
}
/// A directory entry
#[repr(C)]
pub struct DirEntry {
    name: [u8; NAME_LENGTH_LIMIT + 1],
    inode_id: u32,
}
/// Size of a directory entry
pub const DIRENT_SZ: usize = 32;

impl DirEntry {
    /// Create an empty directory entry
    pub fn empty() -> Self {
        Self {
            name: [0u8; NAME_LENGTH_LIMIT + 1],
            inode_id: 0,
        }
    }
    /// Crate a directory entry from name and inode number
    pub fn new(name: &str, inode_id: u32) -> Self {
        let mut bytes = [0u8; NAME_LENGTH_LIMIT + 1];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        Self {
            name: bytes,
            inode_id,
        }
    }
    /// Serialize into bytes
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const _ as usize as *const u8, DIRENT_SZ) }
    }
    /// Serialize into mutable bytes
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self as *mut _ as usize as *mut u8, DIRENT_SZ) }
    }
    /// Get name of the entry
    pub fn name(&self) -> &str {
        let len = (0usize..).find(|i| self.name[*i] == 0).unwrap();
        core::str::from_utf8(&self.name[..len]).unwrap()
    }
    /// Get inode number of the entry
    pub fn inode_id(&self) -> u32 {
        self.inode_id
    }
}
