use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
    inode_id: u32,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        inode_id: u32,
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
            inode_id,
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
                return Some(dirent.inode_id());
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
                    inode_id,
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
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
            let mut reused_slot = None;
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                root_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device);
                if dirent.inode_id() == 0 {
                    reused_slot = Some(i);
                    break;
                }
            }
            let (slot, needs_expand) = if let Some(idx) = reused_slot {
                (idx, false)
            } else {
                (file_count, true)
            };
            if needs_expand {
                let new_size = (file_count + 1) * DIRENT_SZ;
                self.increase_size(new_size as u32, root_inode, &mut fs);
            }
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(slot * DIRENT_SZ, dirent.as_bytes(), &self.block_device);
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            new_inode_id,
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
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
                if dirent.inode_id() != 0 {
                    v.push(String::from(dirent.name()));
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

    /// Return inode number.
    pub fn inode_id(&self) -> u32 {
        self.inode_id
    }

    /// Check whether current inode is a directory.
    pub fn is_dir(&self) -> bool {
        self.read_disk_inode(|disk_inode| disk_inode.is_dir())
    }

    /// Check whether current inode is a regular file.
    pub fn is_file(&self) -> bool {
        self.read_disk_inode(|disk_inode| disk_inode.is_file())
    }

    /// Get file type and hard link count.
    pub fn stat_info(&self) -> (DiskInodeType, u32) {
        self.read_disk_inode(|disk_inode| (disk_inode.inode_type(), disk_inode.nlink()))
    }

    /// Increase hard link count.
    pub fn inc_nlink(&self) -> u32 {
        self.modify_disk_inode(|disk_inode| disk_inode.inc_nlink())
    }

    /// Decrease hard link count.
    pub fn dec_nlink(&self) -> u32 {
        self.modify_disk_inode(|disk_inode| disk_inode.dec_nlink())
    }

    /// Append a directory entry that points to the provided inode.
    pub fn add_link(&self, name: &str, target: &Arc<Inode>) -> bool {
        if !self.is_dir() {
            return false;
        }
        if self.find(name).is_some() {
            return false;
        }
        let mut fs = self.fs.lock();
        let inserted = self.modify_disk_inode(|dir_inode| {
            assert!(dir_inode.is_dir());
            let file_count = (dir_inode.size as usize) / DIRENT_SZ;
            let mut reused_slot = None;
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                dir_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device);
                if dirent.inode_id() == 0 {
                    reused_slot = Some(i);
                    break;
                }
            }
            let slot = if let Some(idx) = reused_slot {
                idx
            } else {
                let new_size = (file_count + 1) * DIRENT_SZ;
                self.increase_size(new_size as u32, dir_inode, &mut fs);
                file_count
            };
            let dirent = DirEntry::new(name, target.inode_id());
            dir_inode.write_at(slot * DIRENT_SZ, dirent.as_bytes(), &self.block_device);
            true
        });
        drop(fs);
        if inserted {
            target.modify_disk_inode(|disk_inode| {
                disk_inode.inc_nlink();
            });
            block_cache_sync_all();
            true
        } else {
            false
        }
    }

    /// Remove a directory entry and return the inode id it referenced.
    pub fn remove_dirent(&self, name: &str) -> Option<u32> {
        if !self.is_dir() {
            return None;
        }
        let removed = self.modify_disk_inode(|dir_inode| {
            assert!(dir_inode.is_dir());
            let file_count = (dir_inode.size as usize) / DIRENT_SZ;
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                dir_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device);
                if dirent.inode_id() != 0 && dirent.name() == name {
                    let inode_id = dirent.inode_id();
                    let empty = DirEntry::empty();
                    dir_inode.write_at(i * DIRENT_SZ, empty.as_bytes(), &self.block_device);
                    return Some(inode_id);
                }
            }
            None
        });
        if removed.is_some() {
            block_cache_sync_all();
        }
        removed
    }

    /// Deallocate current inode from filesystem metadata.
    pub fn dealloc(&self) {
        let mut fs = self.fs.lock();
        fs.dealloc_inode(self.inode_id);
        block_cache_sync_all();
    }
}
