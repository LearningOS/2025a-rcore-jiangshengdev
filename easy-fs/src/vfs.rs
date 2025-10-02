use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
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
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }

    /// 在持有可写文件系统锁的情况下修改磁盘 inode
    fn modify_disk_inode_with_fs<V>(
        &self,
        f: impl FnOnce(&mut DiskInode, &mut EasyFileSystem) -> V,
    ) -> V {
        // 获取文件系统互斥锁以进行修改
        let mut fs = self.fs.lock();
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, |disk_inode: &mut DiskInode| {
                // 将磁盘 inode 可变引用与文件系统一起传入回调
                f(disk_inode, &mut fs)
            })
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }

    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }
    /// Increase the size of a disk inode
    fn increase_size(&self, new_size: u32, disk_inode: &mut DiskInode, fs: &mut EasyFileSystem) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }

    /// 返回当前 inode 在文件系统中的编号
    pub fn inode_id(&self) -> u32 {
        // 锁定文件系统以查询 inode 位置信息
        let fs = self.fs.lock();
        fs.get_inode_id(self.block_id as u32, self.block_offset)
    }

    /// 判断当前 inode 是否代表目录
    pub fn is_dir(&self) -> bool {
        let _fs = self.fs.lock();
        // 读取磁盘 inode 判断类型
        self.read_disk_inode(|disk_inode| disk_inode.is_dir())
    }

    /// 获取当前 inode 的硬链接数量
    pub fn nlink(&self) -> u32 {
        let _fs = self.fs.lock();
        // 读取磁盘 inode 中的引用计数
        self.read_disk_inode(|disk_inode| disk_inode.nlink())
    }

    /// 在当前目录下为目标 inode 创建新的硬链接目录项
    pub fn hard_link(&self, name: &str, target: &Arc<Inode>) -> bool {
        // 目录自身不能作为硬链接目标
        if target.is_dir() {
            return false;
        }
        // 记录目标 inode 的编号
        let target_id = target.inode_id();
        let appended = self.modify_disk_inode_with_fs(|disk_inode, fs| {
            // 非目录节点不支持写入目录项
            if !disk_inode.is_dir() {
                return false;
            }
            // 检查重复的文件名
            if self.find_inode_id(name, disk_inode).is_some() {
                return false;
            }
            // 计算目录项数量并扩展目录空间
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            self.increase_size(new_size as u32, disk_inode, fs);
            // 写入新的目录项链接到目标 inode
            let dirent = DirEntry::new(name, target_id);
            disk_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
            true
        });
        if appended {
            // 成功写入目录项后增加目标文件的硬链接计数
            target.modify_disk_inode(|disk_inode| {
                disk_inode.increase_nlink();
            });
            // 同步块缓存确保元数据落盘
            block_cache_sync_all();
            true
        } else {
            false
        }
    }

    /// 按名称移除目录项并返回被删除 inode 的编号
    pub fn remove_dirent(&self, name: &str) -> Option<u32> {
        let removed = self.modify_disk_inode_with_fs(|disk_inode, fs| {
            // 仅目录 inode 才能维护目录项
            if !disk_inode.is_dir() {
                return None;
            }
            // 统计目录项数量并判断空目录
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            if file_count == 0 {
                return None;
            }
            // 遍历目录项寻找匹配名称
            let mut target_index = None;
            let mut dirent = DirEntry::empty();
            for i in 0..file_count {
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                if dirent.name() == name {
                    target_index = Some(i);
                    break;
                }
            }
            let Some(idx) = target_index else {
                return None;
            };
            // 记录被删除目录项的 inode 编号
            let inode_id = dirent.inode_id();
            let last_index = file_count - 1;
            if idx != last_index {
                // 用末尾目录项覆盖空洞
                let mut tail_dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(
                        last_index * DIRENT_SZ,
                        tail_dirent.as_bytes_mut(),
                        &self.block_device,
                    ),
                    DIRENT_SZ,
                );
                disk_inode.write_at(idx * DIRENT_SZ, tail_dirent.as_bytes(), &self.block_device);
            }
            // 将末尾目录项清零
            let empty = DirEntry::empty();
            disk_inode.write_at(last_index * DIRENT_SZ, empty.as_bytes(), &self.block_device);
            // 调整文件大小并释放不再使用的块
            let new_size = (file_count - 1) * DIRENT_SZ;
            let freed_blocks = disk_inode.truncate(new_size as u32, &self.block_device);
            for block in freed_blocks {
                fs.dealloc_data(block);
            }
            Some(inode_id)
        });
        if removed.is_some() {
            // 刷新缓存确保目录更改生效
            block_cache_sync_all();
        }
        removed
    }

    /// 回收 inode 的元数据资源
    pub fn dealloc(&self) {
        // 获取当前 inode 的编号用于位图回收
        let inode_id = self.inode_id();
        let mut fs = self.fs.lock();
        fs.dealloc_inode(inode_id);
        // 同步缓存以持久化位图更新
        block_cache_sync_all();
    }

    /// 将硬链接数量减一并返回剩余的链接数
    pub fn decrease_nlink(&self) -> u32 {
        // 修改磁盘 inode 的引用计数
        self.modify_disk_inode(|disk_inode| disk_inode.decrease_nlink())
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                let name = dirent.name();
                if !name.is_empty() {
                    v.push(String::from(name));
                }
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}
