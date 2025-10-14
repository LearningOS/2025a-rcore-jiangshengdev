use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{compiler_fence, Ordering};
use spin::{Mutex, MutexGuard};
/// easy-fs上的虚拟文件系统层，提供文件和目录操作的高级接口
pub struct Inode {
    /// inode所在的块编号
    block_id: usize,
    /// inode在块内的偏移量
    block_offset: usize,
    /// 文件系统引用
    fs: Arc<Mutex<EasyFileSystem>>,
    /// 块设备引用
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// 创建新的VFS inode实例
    ///
    /// # 参数
    /// * `block_id` - inode所在的块编号
    /// * `block_offset` - inode在块内的偏移量
    /// * `fs` - 文件系统引用
    /// * `block_device` - 块设备引用
    ///
    /// # 返回
    /// 新的Inode实例
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// 在磁盘inode上调用函数来读取数据
    ///
    /// # 参数
    /// * `f` - 读取回调函数
    ///
    /// # 返回
    /// 回调函数的返回值
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// 在磁盘inode上调用函数来修改数据
    ///
    /// # 参数
    /// * `f` - 修改回调函数
    ///
    /// # 返回
    /// 回调函数的返回值
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// 在目录中根据名称查找inode编号
    ///
    /// # 参数
    /// * `name` - 要查找的文件或目录名
    /// * `disk_inode` - 目录的磁盘inode
    ///
    /// # 返回
    /// 找到时返回inode编号，否则返回None
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // 确保当前inode是目录类型
        assert!(disk_inode.is_dir());
        // 计算目录中的文件数量：目录大小 / 目录项大小
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        // 创建临时目录项用于读取数据
        let mut dirent = DirEntry::empty();
        // 遍历目录中的每个目录项
        for i in 0..file_count {
            // 计算目录项在磁盘中的偏移
            let offset = DIRENT_SZ * i;
            // 从磁盘读取第i个目录项的数据
            assert_eq!(
                disk_inode.read_at(offset, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            // 比较目录项名称与目标名称
            if dirent.name() == name {
                // 找到匹配的文件，返回其inode编号
                return Some(dirent.inode_id());
            }
        }
        // 遍历完所有目录项都没找到，返回None
        None
    }
    /// 在当前目录中根据名称查找子文件或子目录
    ///
    /// # 参数
    /// * `name` - 要查找的文件或目录名
    ///
    /// # 返回
    /// 找到时返回对应的Inode，否则返回None
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        // 获取文件系统锁
        let fs = self.fs.lock();
        // 在当前目录中查找指定名称的文件
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                // 获取找到的inode在磁盘上的位置
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                // 创建新的VFS inode实例
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }
    /// 增加磁盘inode的大小并分配必要的数据块
    ///
    /// # 参数
    /// * `new_size` - 新的文件大小
    /// * `disk_inode` - 要修改的磁盘inode
    /// * `fs` - 文件系统的可变引用
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        // 如果新大小不大于当前大小，无需扩展
        if new_size < disk_inode.size {
            return;
        }
        // 计算需要额外分配的块数
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        // 预分配所需的数据块
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            // 从文件系统分配新的数据块
            v.push(fs.alloc_data());
        }
        // 将新分配的块添加到inode的索引结构中
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// 在当前目录中创建新文件
    ///
    /// # 参数
    /// * `name` - 新文件的名称
    ///
    /// # 返回
    /// 成功时返回新文件的Inode，如果文件已存在则返回None
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        // 获取文件系统的可变引用
        let mut fs = self.fs.lock();

        // 检查文件是否已存在
        let op = |root_inode: &DiskInode| {
            // 确保当前inode是目录
            assert!(root_inode.is_dir());
            // 在目录中查找同名文件
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            // 文件已存在，创建失败
            return None;
        }

        // 开始创建新文件的流程
        // 第一步：分配新的inode
        let new_inode_id = fs.alloc_inode();

        // 第二步：初始化新分配的inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                // 将新inode初始化为文件类型
                new_inode.initialize(DiskInodeType::File);
            });

        // 第三步：在父目录中添加新文件的目录项
        self.modify_disk_inode(|root_inode| {
            // 计算当前目录中的文件数量
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            // 计算添加新目录项后的目录大小
            let new_size = (file_count + 1) * DIRENT_SZ;
            // 如果需要，扩展目录的大小以容纳新的目录项
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // 创建新的目录项
            let dirent = DirEntry::new(name, new_inode_id);
            // 计算写入位置的偏移
            let write_offset = file_count * DIRENT_SZ;
            // 将目录项写入目录的末尾
            root_inode.write_at(write_offset, dirent.as_bytes(), &self.block_device);
        });

        // 第四步：获取新文件inode的位置信息并创建VFS inode
        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        // 确保所有修改都写入磁盘
        block_cache_sync_all();
        // 创建并返回新文件的VFS inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // 文件系统锁在此处自动释放
    }
    /// 列出当前目录中的所有文件和子目录
    ///
    /// # 返回
    /// 包含所有文件和目录名称的字符串向量
    pub fn ls(&self) -> Vec<String> {
        // 获取文件系统锁（只读）
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            // 计算目录中的文件数量
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            // 创建结果向量
            let mut v: Vec<String> = Vec::new();
            // 遍历每个目录项
            for i in 0..file_count {
                // 创建临时目录项
                let mut dirent = DirEntry::empty();
                // 计算目录项偏移
                let offset = i * DIRENT_SZ;
                // 读取目录项数据
                assert_eq!(
                    disk_inode.read_at(offset, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                // 将文件名添加到结果中
                v.push(String::from(dirent.name()));
            }
            v
        })
    }
    /// 从当前文件的指定偏移处读取数据
    ///
    /// # 参数
    /// * `offset` - 读取起始偏移量
    /// * `buf` - 目标缓冲区
    ///
    /// # 返回
    /// 实际读取的字节数
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        // 获取文件系统锁（只读）
        let _fs = self.fs.lock();
        // 委托给磁盘inode的读取方法
        self.read_disk_inode(|disk_inode| {
            compiler_fence(Ordering::SeqCst);
            disk_inode.read_at(offset, buf, &self.block_device)
        })
    }
    /// 向当前文件的指定偏移处写入数据
    ///
    /// # 参数
    /// * `offset` - 写入起始偏移量
    /// * `buf` - 源数据缓冲区
    ///
    /// # 返回
    /// 实际写入的字节数
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        // 获取文件系统的可变锁
        let mut fs = self.fs.lock();
        // 修改磁盘inode并执行写入操作
        let size = self.modify_disk_inode(|disk_inode| {
            // 如果需要，扩展文件大小以容纳新数据
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            // 执行实际的写入操作
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        // 确保所有修改都写入磁盘
        block_cache_sync_all();
        size
    }
    /// 清除当前文件中的所有数据并释放相关的数据块
    pub fn clear(&self) {
        // 获取文件系统的可变锁
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            // 记录原始文件大小用于验证
            let size = disk_inode.size;
            // 清空inode并获取需要释放的所有块
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            // 验证释放的块数是否正确
            assert_eq!(
                data_blocks_dealloc.len(),
                DiskInode::total_blocks(size) as usize
            );
            // 逐个释放所有数据块
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        // 确保所有修改都写入磁盘
        block_cache_sync_all();
    }
}
