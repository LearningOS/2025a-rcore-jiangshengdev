use super::{
    block_cache_sync_all, get_block_cache, Bitmap, BlockDevice, DiskInode, DiskInodeType, Inode,
    SuperBlock,
};
use crate::BLOCK_SZ;
use alloc::sync::Arc;
use core::fmt;
use spin::Mutex;

/// 位图块管理的数据块容量
const BITMAP_BLOCK_DATA_CAPACITY: u32 = 4096;
/// 位图块总数（数据容量 + 自身）
const BITMAP_BLOCK_TOTAL: u32 = 4097;
/// 基于块的简易文件系统，提供文件和目录的基本操作
pub struct EasyFileSystem {
    /// 底层块设备引用
    pub block_device: Arc<dyn BlockDevice>,
    /// inode分配位图，管理inode的分配和释放
    pub inode_bitmap: Bitmap,
    /// 数据块分配位图，管理数据块的分配和释放
    pub data_bitmap: Bitmap,
    /// inode区域起始块编号
    inode_area_start_block: u32,
    /// 数据区域起始块编号
    data_area_start_block: u32,
}

type DataBlock = [u8; BLOCK_SZ];

impl fmt::Debug for EasyFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            f.debug_struct("EasyFileSystem")
                .field("block_device", &"<BlockDevice>")
                .field("inode_bitmap", &self.inode_bitmap)
                .field("data_bitmap", &self.data_bitmap)
                .field("inode_area_start_block", &self.inode_area_start_block)
                .field("data_area_start_block", &self.data_area_start_block)
                .finish()
        } else {
            f.write_str("EasyFileSystem {\n")?;
            writeln!(f, "    block_device: \"<BlockDevice>\",")?;
            writeln!(f, "    inode_bitmap: {:?},", self.inode_bitmap)?;
            writeln!(f, "    data_bitmap: {:?},", self.data_bitmap)?;
            writeln!(
                f,
                "    inode_area_start_block: {},",
                self.inode_area_start_block
            )?;
            write!(
                f,
                "    data_area_start_block: {}\n}}",
                self.data_area_start_block
            )
        }
    }
}

/// 基于块设备的简易文件系统
impl EasyFileSystem {
    /// 在块设备上创建新的文件系统
    ///
    /// # 参数
    /// * `block_device` - 块设备引用
    /// * `total_blocks` - 总块数
    /// * `inode_bitmap_blocks` - inode位图占用的块数
    ///
    /// # 返回
    /// 新创建的文件系统实例
    pub fn create(
        block_device: Arc<dyn BlockDevice>,
        total_blocks: u32,
        inode_bitmap_blocks: u32,
    ) -> Arc<Mutex<Self>> {
        // 计算文件系统各区域的布局
        // 创建inode位图，从块1开始（块0是超级块）
        let inode_bitmap = Bitmap::new(1, inode_bitmap_blocks as usize);
        // 计算inode位图能管理的最大inode数量
        let inode_num = inode_bitmap.maximum();
        // 获取单个inode的大小
        let inode_size = core::mem::size_of::<DiskInode>();
        // 计算存储所有inode需要的块数
        // 使用向上取整：(inode_num * inode_size + BLOCK_SZ - 1) / BLOCK_SZ
        let inode_area_blocks = ((inode_num * inode_size + BLOCK_SZ - 1) / BLOCK_SZ) as u32;
        // inode相关区域的总块数 = inode位图块数 + inode存储区块数
        let inode_total_blocks = inode_bitmap_blocks + inode_area_blocks;
        // 数据相关区域的总块数 = 总块数 - 超级块(1) - inode相关块数
        let data_total_blocks = total_blocks - 1 - inode_total_blocks;
        // 计算数据位图需要的块数
        // 每个位图块可以管理4096个数据块，所以需要 (data_total_blocks + 4096) / 4097 个位图块
        // 4097 = 4096 + 1，其中1是位图块本身
        let data_bitmap_blocks =
            (data_total_blocks + BITMAP_BLOCK_DATA_CAPACITY) / BITMAP_BLOCK_TOTAL;
        // 实际可用的数据块数 = 数据总块数 - 数据位图块数
        let data_area_blocks = data_total_blocks - data_bitmap_blocks;
        // 数据位图起始块编号
        let data_bitmap_start = (1 + inode_bitmap_blocks + inode_area_blocks) as usize;
        let data_bitmap = Bitmap::new(data_bitmap_start, data_bitmap_blocks as usize);
        // 数据区域起始块编号
        let data_area_start_block = 1 + inode_total_blocks + data_bitmap_blocks;
        let mut efs = Self {
            block_device: Arc::clone(&block_device),
            inode_bitmap,
            data_bitmap,
            inode_area_start_block: 1 + inode_bitmap_blocks,
            data_area_start_block,
        };
        // 只初始化元数据区域的块（超级块、位图、inode区）
        // 数据区的块在实际分配时再清零，避免不必要的初始化
        let metadata_blocks = 1 + inode_total_blocks + data_bitmap_blocks;
        for i in 0..metadata_blocks {
            get_block_cache(i as usize, Arc::clone(&block_device))
                .lock()
                .modify(0, |data_block: &mut DataBlock| {
                    // 使用 fill 方法批量清零，比逐字节赋值快得多
                    data_block.fill(0);
                });
        }
        // 初始化超级块
        get_block_cache(0, Arc::clone(&block_device)).lock().modify(
            0,
            |super_block: &mut SuperBlock| {
                super_block.initialize(
                    total_blocks,
                    inode_bitmap_blocks,
                    inode_area_blocks,
                    data_bitmap_blocks,
                    data_area_blocks,
                );
            },
        );
        // 立即写回超级块到磁盘
        // 为根节点"/"创建inode，确保分配到编号0
        assert_eq!(efs.alloc_inode(), 0);
        // 获取根inode在磁盘上的位置
        let (root_inode_block_id, root_inode_offset) = efs.get_disk_inode_pos(0);
        // 初始化根inode为目录类型
        get_block_cache(root_inode_block_id as usize, Arc::clone(&block_device))
            .lock()
            .modify(root_inode_offset, |disk_inode: &mut DiskInode| {
                disk_inode.initialize(DiskInodeType::Directory);
            });
        // 确保所有初始化数据都写入磁盘
        block_cache_sync_all();
        // 返回新创建的文件系统实例
        Arc::new(Mutex::new(efs))
    }
    /// 将现有块设备作为文件系统打开
    ///
    /// # 参数
    /// * `block_device` - 包含文件系统的块设备引用
    ///
    /// # 返回
    /// 打开的文件系统实例
    pub fn open(block_device: Arc<dyn BlockDevice>) -> Arc<Mutex<Self>> {
        // 从块0读取超级块信息
        get_block_cache(0, Arc::clone(&block_device))
            .lock()
            .read(0, |super_block: &SuperBlock| {
                // 验证文件系统的有效性
                assert!(super_block.is_valid(), "Error loading EFS!");
                // 计算inode相关区域的总块数
                let inode_total_blocks =
                    super_block.inode_bitmap_blocks + super_block.inode_area_blocks;
                // 根据超级块信息重建文件系统结构
                let data_bitmap_start = (1 + inode_total_blocks) as usize;
                let data_area_start_block = 1 + inode_total_blocks + super_block.data_bitmap_blocks;
                let efs = Self {
                    block_device,
                    // 重建inode位图，从块1开始
                    inode_bitmap: Bitmap::new(1, super_block.inode_bitmap_blocks as usize),
                    // 重建数据位图，从inode区域之后开始
                    data_bitmap: Bitmap::new(
                        data_bitmap_start,
                        super_block.data_bitmap_blocks as usize,
                    ),
                    // 设置各区域的起始位置
                    inode_area_start_block: 1 + super_block.inode_bitmap_blocks,
                    data_area_start_block,
                };
                Arc::new(Mutex::new(efs))
            })
    }
    /// 获取文件系统的根inode
    ///
    /// # 参数
    /// * `efs` - 文件系统引用
    ///
    /// # 返回
    /// 根目录的inode
    pub fn root_inode(efs: &Arc<Mutex<Self>>) -> Inode {
        // 获取块设备引用
        let block_device = Arc::clone(&efs.lock().block_device);
        // 临时获取efs锁，查询根inode（编号0）的位置
        let (block_id, block_offset) = efs.lock().get_disk_inode_pos(0);
        // 释放efs锁，创建VFS层的根inode
        Inode::new(block_id, block_offset, Arc::clone(efs), block_device)
    }
    /// 根据inode编号获取其在磁盘上的位置
    ///
    /// # 参数
    /// * `inode_id` - inode编号
    ///
    /// # 返回
    /// 元组(块编号, 块内偏移)
    pub fn get_disk_inode_pos(&self, inode_id: u32) -> (u32, usize) {
        // 获取单个inode的大小
        let inode_size = core::mem::size_of::<DiskInode>();
        // 计算每个块能容纳多少个inode
        let inodes_per_block = (BLOCK_SZ / inode_size) as u32;
        // 计算inode所在的块编号
        let block_id = self.inode_area_start_block + inode_id / inodes_per_block;
        // 计算块内偏移
        let offset = (inode_id % inodes_per_block) as usize * inode_size;
        // 返回块编号和块内偏移
        (block_id, offset)
    }
    /// 根据数据块编号获取其在磁盘上的实际块编号
    ///
    /// # 参数
    /// * `data_block_id` - 数据块编号
    ///
    /// # 返回
    /// 实际的磁盘块编号
    pub fn get_data_block_id(&self, data_block_id: u32) -> u32 {
        self.data_area_start_block + data_block_id
    }
    /// 分配新的inode
    ///
    /// # 返回
    /// 新分配的inode编号
    pub fn alloc_inode(&mut self) -> u32 {
        self.inode_bitmap.alloc(&self.block_device).unwrap() as u32
    }

    /// 分配新的数据块
    ///
    /// # 返回
    /// 新分配的数据块编号
    pub fn alloc_data(&mut self) -> u32 {
        // 从数据位图分配一个数据块
        let data_block_id = self.data_bitmap.alloc(&self.block_device).unwrap() as u32;
        // 转换为实际的磁盘块编号
        let block_id = data_block_id + self.data_area_start_block;
        // 清零新分配的数据块（延迟初始化策略）
        get_block_cache(block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(0, |data_block: &mut DataBlock| {
                data_block.fill(0);
            });
        block_id
    }
    /// 释放指定的数据块
    ///
    /// # 参数
    /// * `block_id` - 要释放的数据块编号
    pub fn dealloc_data(&mut self, block_id: u32) {
        // 首先清空数据块的内容
        get_block_cache(block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(0, |data_block: &mut DataBlock| {
                // 使用 fill 方法批量清零，比逐字节赋值快得多
                data_block.fill(0);
            });
        // 计算数据块在位图中的索引
        let data_block_index = (block_id - self.data_area_start_block) as usize;
        // 然后在数据位图中标记该块为可用
        self.data_bitmap
            .dealloc(&self.block_device, data_block_index)
    }
}
