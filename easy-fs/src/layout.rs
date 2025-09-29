use super::{get_block_cache, BlockDevice, BLOCK_SZ};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::{Debug, Formatter, Result};
use core::sync::atomic::{compiler_fence, Ordering};

/// 用于完整性检查的魔数
const EFS_MAGIC: u32 = 0x3b800001;
/// 直接inode的最大数量
const INODE_DIRECT_COUNT: usize = 28;
/// inode名称的最大长度
const NAME_LENGTH_LIMIT: usize = 27;
/// 一级间接inode的最大数量
const INODE_INDIRECT1_COUNT: usize = BLOCK_SZ / 4;
/// 二级间接inode的最大数量
const INODE_INDIRECT2_COUNT: usize = INODE_INDIRECT1_COUNT * INODE_INDIRECT1_COUNT;
/// 直接inode索引的上界
const DIRECT_BOUND: usize = INODE_DIRECT_COUNT;
/// 一级间接inode索引的上界
const INDIRECT1_BOUND: usize = DIRECT_BOUND + INODE_INDIRECT1_COUNT;
/// 二级间接inode索引的上界
#[allow(unused)]
const INDIRECT2_BOUND: usize = INDIRECT1_BOUND + INODE_INDIRECT2_COUNT;
/// 文件系统的超级块，存储文件系统的元数据信息
#[repr(C)]
pub struct SuperBlock {
    /// 魔数，用于验证文件系统的有效性
    magic: u32,
    /// 文件系统总块数
    pub total_blocks: u32,
    /// inode位图占用的块数
    pub inode_bitmap_blocks: u32,
    /// inode区域占用的块数
    pub inode_area_blocks: u32,
    /// 数据位图占用的块数
    pub data_bitmap_blocks: u32,
    /// 数据区域占用的块数
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
    /// 初始化超级块
    ///
    /// # 参数
    /// * `total_blocks` - 文件系统总块数
    /// * `inode_bitmap_blocks` - inode位图占用的块数
    /// * `inode_area_blocks` - inode区域占用的块数
    /// * `data_bitmap_blocks` - 数据位图占用的块数
    /// * `data_area_blocks` - 数据区域占用的块数
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
    /// 使用efs魔数检查超级块是否有效
    ///
    /// # 返回
    /// 如果魔数匹配则返回true，否则返回false
    pub fn is_valid(&self) -> bool {
        self.magic == EFS_MAGIC
    }
}
/// 磁盘inode的类型
#[derive(PartialEq)]
pub enum DiskInodeType {
    File,
    Directory,
}

/// 间接块
type IndirectBlock = [u32; BLOCK_SZ / 4];
/// 数据块
type DataBlock = [u8; BLOCK_SZ];
/// 磁盘inode，存储文件或目录的元数据和数据块索引
#[repr(C)]
pub struct DiskInode {
    /// 文件或目录的大小（字节数）
    pub size: u32,
    /// 直接数据块索引数组
    pub direct: [u32; INODE_DIRECT_COUNT],
    /// 一级间接块索引
    pub indirect1: u32,
    /// 二级间接块索引
    pub indirect2: u32,
    /// inode类型（文件或目录）
    type_: DiskInodeType,
}

impl DiskInode {
    /// 初始化磁盘inode以及其下的所有直接inode
    /// 一级和二级间接块仅在需要时分配
    ///
    /// # 参数
    /// * `type_` - inode类型（文件或目录）
    pub fn initialize(&mut self, type_: DiskInodeType) {
        // 初始化文件大小为0
        self.size = 0;
        // 清空所有直接块索引
        self.direct.iter_mut().for_each(|v| {
            compiler_fence(Ordering::SeqCst);
            *v = 0;
        });
        // 清空间接块索引
        self.indirect1 = 0;
        self.indirect2 = 0;
        // 设置inode类型
        self.type_ = type_;
    }
    /// 检查此inode是否为目录
    ///
    /// # 返回
    /// 如果是目录返回true，否则返回false
    pub fn is_dir(&self) -> bool {
        self.type_ == DiskInodeType::Directory
    }
    /// 检查此inode是否为文件
    ///
    /// # 返回
    /// 如果是文件返回true，否则返回false
    #[allow(unused)]
    pub fn is_file(&self) -> bool {
        self.type_ == DiskInodeType::File
    }
    /// 返回当前inode大小对应的数据块数
    ///
    /// # 返回
    /// 需要的数据块数量
    pub fn data_blocks(&self) -> u32 {
        Self::_data_blocks(self.size)
    }
    fn _data_blocks(size: u32) -> u32 {
        // 使用向上取整计算需要的数据块数：(size + BLOCK_SZ - 1) / BLOCK_SZ
        (size + BLOCK_SZ as u32 - 1) / BLOCK_SZ as u32
    }
    /// 返回指定大小所需的总块数，包括一级/二级间接块
    ///
    /// # 参数
    /// * `size` - 文件大小（字节）
    ///
    /// # 返回
    /// 总共需要的块数（包括间接块）
    pub fn total_blocks(size: u32) -> u32 {
        // 计算需要的数据块数
        let data_blocks = Self::_data_blocks(size) as usize;
        let mut total = data_blocks as usize;
        // 如果数据块数超过直接索引范围，需要一级间接块
        if data_blocks > INODE_DIRECT_COUNT {
            total += 1;
        }
        // 如果数据块数超过一级间接索引范围，需要二级间接块
        if data_blocks > INDIRECT1_BOUND {
            total += 1;
            // 计算需要的子一级间接块数量
            let indirect1_blocks_needed =
                (data_blocks - INDIRECT1_BOUND + INODE_INDIRECT1_COUNT - 1) / INODE_INDIRECT1_COUNT;
            total += indirect1_blocks_needed;
        }
        total as u32
    }
    /// 根据新的数据大小计算需要额外分配的块数
    ///
    /// # 参数
    /// * `new_size` - 新的文件大小
    ///
    /// # 返回
    /// 需要额外分配的块数
    pub fn blocks_num_needed(&self, new_size: u32) -> u32 {
        assert!(new_size >= self.size);
        Self::total_blocks(new_size) - Self::total_blocks(self.size)
    }
    /// 根据内部块编号获取实际的磁盘块编号
    ///
    /// # 参数
    /// * `inner_id` - 文件内部的块编号
    /// * `block_device` - 块设备引用
    ///
    /// # 返回
    /// 实际的磁盘块编号
    pub fn get_block_id(&self, inner_id: u32, block_device: &Arc<dyn BlockDevice>) -> u32 {
        let inner_id = inner_id as usize;
        // 根据块编号范围确定使用哪种索引方式
        if inner_id < INODE_DIRECT_COUNT {
            // 直接索引：块编号在直接索引范围内，直接从direct数组获取
            self.direct[inner_id]
        } else if inner_id < INDIRECT1_BOUND {
            // 一级间接索引：需要通过一级间接块查找
            get_block_cache(self.indirect1 as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect_block: &IndirectBlock| {
                    // 计算在一级间接块中的索引位置
                    indirect_block[inner_id - INODE_DIRECT_COUNT]
                })
        } else {
            // 二级间接索引：需要通过二级间接块查找
            // 计算在二级间接索引范围内的相对位置
            let last = inner_id - INDIRECT1_BOUND;
            // 首先从二级间接块中获取对应的一级间接块编号
            let indirect1 = get_block_cache(self.indirect2 as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect2: &IndirectBlock| {
                    // 计算应该使用哪个一级间接块
                    indirect2[last / INODE_INDIRECT1_COUNT]
                });
            // 然后从对应的一级间接块中获取实际的数据块编号
            get_block_cache(indirect1 as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect1: &IndirectBlock| {
                    // 计算在一级间接块中的索引位置
                    indirect1[last % INODE_INDIRECT1_COUNT]
                })
        }
    }
    /// 增加当前磁盘inode的大小并分配相应的数据块
    ///
    /// # 参数
    /// * `new_size` - 新的文件大小
    /// * `new_blocks` - 新分配的数据块编号列表
    /// * `block_device` - 块设备引用
    pub fn increase_size(
        &mut self,
        new_size: u32,
        new_blocks: Vec<u32>,
        block_device: &Arc<dyn BlockDevice>,
    ) {
        // 记录当前已分配的数据块数
        let mut current_blocks = self.data_blocks();
        // 更新文件大小
        self.size = new_size;
        // 计算新大小需要的总数据块数
        let mut total_blocks = self.data_blocks();
        // 创建新块的迭代器
        let mut new_blocks = new_blocks.into_iter();
        // 首先填充直接块索引
        while current_blocks < total_blocks.min(INODE_DIRECT_COUNT as u32) {
            // 将新分配的块编号存入直接索引数组
            self.direct[current_blocks as usize] = new_blocks.next().unwrap();
            current_blocks += 1;
        }
        // 如果需要一级间接块
        if total_blocks > INODE_DIRECT_COUNT as u32 {
            // 如果是第一次需要一级间接块，分配一个新块作为间接块
            if current_blocks == INODE_DIRECT_COUNT as u32 {
                self.indirect1 = new_blocks.next().unwrap();
            }
            // 调整计数器，转换到一级间接块的索引空间
            current_blocks -= INODE_DIRECT_COUNT as u32;
            total_blocks -= INODE_DIRECT_COUNT as u32;
        } else {
            // 不需要间接块，直接返回
            return;
        }
        // 填充一级间接块中的数据块索引
        get_block_cache(self.indirect1 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect1: &mut IndirectBlock| {
                // 在一级间接块中填充数据块索引
                while current_blocks < total_blocks.min(INODE_INDIRECT1_COUNT as u32) {
                    indirect1[current_blocks as usize] = new_blocks.next().unwrap();
                    current_blocks += 1;
                }
            });
        // 如果需要二级间接块
        if total_blocks > INODE_INDIRECT1_COUNT as u32 {
            // 如果是第一次需要二级间接块，分配一个新块
            if current_blocks == INODE_INDIRECT1_COUNT as u32 {
                self.indirect2 = new_blocks.next().unwrap();
            }
            // 调整计数器，转换到二级间接块的索引空间
            current_blocks -= INODE_INDIRECT1_COUNT as u32;
            total_blocks -= INODE_INDIRECT1_COUNT as u32;
        } else {
            // 不需要二级间接块，直接返回
            return;
        }
        // 处理二级间接块的复杂分配逻辑
        // 使用二维坐标系统：(a, b) 其中a是一级间接块索引，b是块内索引
        // 起始一级间接块索引
        let mut a0 = current_blocks as usize / INODE_INDIRECT1_COUNT;
        // 起始块内索引
        let mut b0 = current_blocks as usize % INODE_INDIRECT1_COUNT;
        // 结束一级间接块索引
        let a1 = total_blocks as usize / INODE_INDIRECT1_COUNT;
        // 结束块内索引
        let b1 = total_blocks as usize % INODE_INDIRECT1_COUNT;

        // 在二级间接块中分配一级间接块和数据块
        get_block_cache(self.indirect2 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect2: &mut IndirectBlock| {
                // 遍历从(a0,b0)到(a1,b1)的所有位置
                while (a0 < a1) || (a0 == a1 && b0 < b1) {
                    // 如果是新的一级间接块的开始位置，需要分配新的一级间接块
                    if b0 == 0 {
                        indirect2[a0] = new_blocks.next().unwrap();
                    }
                    // 在当前一级间接块中分配数据块
                    get_block_cache(indirect2[a0] as usize, Arc::clone(block_device))
                        .lock()
                        .modify(0, |indirect1: &mut IndirectBlock| {
                            indirect1[b0] = new_blocks.next().unwrap();
                        });
                    // 移动到下一个位置
                    b0 += 1;
                    // 如果当前一级间接块已满，移动到下一个一级间接块
                    if b0 == INODE_INDIRECT1_COUNT {
                        b0 = 0;
                        a0 += 1;
                    }
                }
            });
    }

    /// 将inode大小清零并返回应该释放的所有块
    /// 稍后调用者需要将这些块的内容清零
    ///
    /// # 参数
    /// * `block_device` - 块设备引用
    ///
    /// # 返回
    /// 需要释放的所有块编号列表
    pub fn clear_size(&mut self, block_device: &Arc<dyn BlockDevice>) -> Vec<u32> {
        // 创建用于存储需要释放的块编号的向量
        let mut v: Vec<u32> = Vec::new();
        // 获取当前文件使用的数据块数
        let mut data_blocks = self.data_blocks() as usize;
        // 将文件大小设置为0
        self.size = 0;
        // 初始化当前处理的块计数器
        let mut current_blocks = 0usize;
        // 处理直接块的释放
        while current_blocks < data_blocks.min(INODE_DIRECT_COUNT) {
            // 将直接块编号加入释放列表
            v.push(self.direct[current_blocks]);
            // 清空直接块索引
            self.direct[current_blocks] = 0;
            current_blocks += 1;
        }
        // 检查是否需要处理一级间接块
        if data_blocks > INODE_DIRECT_COUNT {
            // 将一级间接块本身加入释放列表
            v.push(self.indirect1);
            // 调整剩余数据块数和计数器
            data_blocks -= INODE_DIRECT_COUNT;
            current_blocks = 0;
        } else {
            // 只有直接块，直接返回
            return v;
        }
        // 处理一级间接块中的数据块
        get_block_cache(self.indirect1 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect1: &mut IndirectBlock| {
                // 遍历一级间接块中的所有数据块索引
                while current_blocks < data_blocks.min(INODE_INDIRECT1_COUNT) {
                    // 将数据块编号加入释放列表
                    v.push(indirect1[current_blocks]);
                    // 注意：这里不清零索引，因为整个间接块都会被释放
                    current_blocks += 1;
                }
            });
        // 清空一级间接块索引
        self.indirect1 = 0;
        // 检查是否需要处理二级间接块
        if data_blocks > INODE_INDIRECT1_COUNT {
            // 将二级间接块本身加入释放列表
            v.push(self.indirect2);
            // 调整剩余数据块数
            data_blocks -= INODE_INDIRECT1_COUNT;
        } else {
            // 只需要处理到一级间接块，直接返回
            return v;
        }
        // 处理二级间接块的释放
        assert!(data_blocks <= INODE_INDIRECT2_COUNT);
        // 计算需要释放的一级间接块数量和最后一个块的部分大小
        // 完整的一级间接块数量
        let a1 = data_blocks / INODE_INDIRECT1_COUNT;
        // 最后一个一级间接块中的数据块数量
        let b1 = data_blocks % INODE_INDIRECT1_COUNT;

        get_block_cache(self.indirect2 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect2: &mut IndirectBlock| {
                // 释放所有完整的一级间接块及其包含的数据块
                for entry in indirect2.iter_mut().take(a1) {
                    // 将一级间接块本身加入释放列表
                    v.push(*entry);
                    // 释放该一级间接块中的所有数据块
                    get_block_cache(*entry as usize, Arc::clone(block_device))
                        .lock()
                        .modify(0, |indirect1: &mut IndirectBlock| {
                            // 遍历一级间接块中的所有数据块索引
                            for entry in indirect1.iter() {
                                v.push(*entry);
                            }
                        });
                }
                // 处理最后一个部分填充的一级间接块
                if b1 > 0 {
                    // 将最后一个一级间接块本身加入释放列表
                    v.push(indirect2[a1]);
                    // 只释放该块中实际使用的数据块
                    get_block_cache(indirect2[a1] as usize, Arc::clone(block_device))
                        .lock()
                        .modify(0, |indirect1: &mut IndirectBlock| {
                            // 只遍历前b1个数据块索引
                            for entry in indirect1.iter().take(b1) {
                                v.push(*entry);
                            }
                        });
                }
            });
        // 清空二级间接块索引
        self.indirect2 = 0;
        // 返回所有需要释放的块编号列表
        v
    }
    /// 从当前磁盘inode的指定偏移处读取数据
    ///
    /// # 参数
    /// * `offset` - 读取起始偏移量
    /// * `buf` - 目标缓冲区
    /// * `block_device` - 块设备引用
    ///
    /// # 返回
    /// 实际读取的字节数
    pub fn read_at(
        &self,
        offset: usize,
        buf: &mut [u8],
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        // 初始化读取范围
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        // 如果读取范围无效，直接返回0
        if start >= end {
            return 0;
        }
        // 计算起始块编号和已读取字节数
        let mut start_block = start / BLOCK_SZ;
        let mut read_size = 0usize;
        loop {
            // 计算当前块的结束位置
            let end_current_block = ((start / BLOCK_SZ + 1) * BLOCK_SZ).min(end);
            // 计算当前块需要读取的字节数
            let block_read_size = end_current_block - start;
            // 获取目标缓冲区的切片
            let dst = &mut buf[read_size..read_size + block_read_size];
            // 获取数据块并读取数据
            get_block_cache(
                self.get_block_id(start_block as u32, block_device) as usize,
                Arc::clone(block_device),
            )
            .lock()
            .read(0, |data_block: &DataBlock| {
                // 计算源数据在块内的位置和范围
                let src = &data_block[start % BLOCK_SZ..start % BLOCK_SZ + block_read_size];
                // 复制数据到目标缓冲区
                dst.copy_from_slice(src);
            });
            // 更新已读取的字节数
            read_size += block_read_size;
            // 检查是否已完成所有读取
            if end_current_block == end {
                break;
            }
            // 移动到下一个块
            start_block += 1;
            start = end_current_block;
        }
        read_size
    }
    /// 向当前磁盘inode的指定偏移处写入数据
    /// 注意：inode的大小必须事先正确调整
    ///
    /// # 参数
    /// * `offset` - 写入起始偏移量
    /// * `buf` - 源数据缓冲区
    /// * `block_device` - 块设备引用
    ///
    /// # 返回
    /// 实际写入的字节数
    pub fn write_at(
        &mut self,
        offset: usize,
        buf: &[u8],
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        // 初始化写入范围
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        // 确保写入范围有效
        assert!(start <= end);
        // 计算起始块编号和已写入字节数
        let mut start_block = start / BLOCK_SZ;
        let mut write_size = 0usize;
        loop {
            // 计算当前块的结束位置
            let end_current_block = ((start / BLOCK_SZ + 1) * BLOCK_SZ).min(end);
            // 计算当前块需要写入的字节数
            let block_write_size = end_current_block - start;
            // 获取数据块并写入数据
            get_block_cache(
                self.get_block_id(start_block as u32, block_device) as usize,
                Arc::clone(block_device),
            )
            .lock()
            .modify(0, |data_block: &mut DataBlock| {
                // 获取源数据切片
                let src = &buf[write_size..write_size + block_write_size];
                // 计算目标位置在块内的范围
                let dst = &mut data_block[start % BLOCK_SZ..start % BLOCK_SZ + block_write_size];
                // 复制数据到目标位置
                dst.copy_from_slice(src);
            });
            // 更新已写入的字节数
            write_size += block_write_size;
            // 检查是否已完成所有写入
            if end_current_block == end {
                break;
            }
            // 移动到下一个块
            start_block += 1;
            start = end_current_block;
        }
        write_size
    }
}
/// 目录项，存储目录中文件或子目录的信息
#[repr(C)]
pub struct DirEntry {
    /// 文件或目录名称，以null结尾的字符串
    name: [u8; NAME_LENGTH_LIMIT + 1],
    /// 对应的inode编号
    inode_id: u32,
}
/// 目录项的大小
pub const DIRENT_SZ: usize = 32;

impl DirEntry {
    /// 创建一个空的目录项
    ///
    /// # 返回
    /// 新的空目录项实例
    pub fn empty() -> Self {
        Self {
            name: [0u8; NAME_LENGTH_LIMIT + 1],
            inode_id: 0,
        }
    }
    /// 根据文件名和inode编号创建目录项
    ///
    /// # 参数
    /// * `name` - 文件或目录名称
    /// * `inode_id` - 对应的inode编号
    ///
    /// # 返回
    /// 新的目录项实例
    pub fn new(name: &str, inode_id: u32) -> Self {
        // 创建名称字节数组，初始化为0
        let mut bytes = [0u8; NAME_LENGTH_LIMIT + 1];
        // 将文件名复制到字节数组中
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        // 创建新的目录项
        Self {
            name: bytes,
            inode_id,
        }
    }
    /// 获取目录项的字节表示（只读）
    ///
    /// # 返回
    /// 目录项的字节切片
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const _ as usize as *const u8, DIRENT_SZ) }
    }
    /// 获取目录项的可变字节表示
    ///
    /// # 返回
    /// 目录项的可变字节切片
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self as *mut _ as usize as *mut u8, DIRENT_SZ) }
    }
    /// 获取目录项的文件名
    ///
    /// # 返回
    /// 文件或目录名称的字符串切片
    pub fn name(&self) -> &str {
        // 查找字符串结束位置（第一个null字节）
        let len = (0usize..)
            .find(|i| {
                compiler_fence(Ordering::SeqCst);
                self.name[*i] == 0
            })
            .unwrap();
        // 将字节数组转换为字符串切片
        core::str::from_utf8(&self.name[..len]).unwrap()
    }
    /// 获取目录项对应的inode编号
    ///
    /// # 返回
    /// inode编号
    pub fn inode_id(&self) -> u32 {
        self.inode_id
    }
}
