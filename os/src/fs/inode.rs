//! `Arc<Inode>` -> `OSInodeInner`: 为了并发打开文件
//! 我们需要将 `Inode` 包装到 `Arc` 中，但 `Inode` 中的 `Mutex` 阻止
//! 文件系统被同时访问
//!
//! `UPSafeCell<OSInodeInner>` -> `OSInode`: 对于静态的 `ROOT_INODE`，我们
//! 需要将 `OSInodeInner` 包装到 `UPSafeCell` 中
use super::File;
use crate::drivers::BLOCK_DEVICE;
use crate::mm::UserBuffer;
use crate::sync::UPSafeCell;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;
use core::cmp::min;
use easy_fs::{EasyFileSystem, Inode, BLOCK_SZ};
use lazy_static::*;

/// 内存中的 inode
/// 文件系统 inode 的包装器
/// 用于在其上实现 File trait
pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
}
/// 'UPSafeCell' 中的 OS inode 内部结构
pub struct OSInodeInner {
    offset: usize,
    inode: Arc<Inode>,
}

impl OSInode {
    /// 在内存中创建新的 inode
    pub fn new(readable: bool, writable: bool, inode: Arc<Inode>) -> Self {
        Self {
            readable,
            writable,
            // 初始化内部结构，文件偏移量从0开始
            inner: unsafe { UPSafeCell::new(OSInodeInner { offset: 0, inode }) },
        }
    }
    /// 从 inode 读取所有数据
    pub fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.exclusive_access();
        // 创建与文件系统块大小一致的缓冲区用于分块读取
        let mut buffer: Vec<u8> = vec![0; BLOCK_SZ];
        let mut v: Vec<u8> = Vec::new();
        // 循环读取文件的所有内容
        loop {
            let len = inner.inode.read_at(inner.offset, &mut buffer);
            if len == 0 {
                break;
            }
            // 更新文件偏移量
            inner.offset += len;
            // 将读取的数据添加到结果向量中
            v.extend_from_slice(&buffer[..len]);
        }
        v
    }
}

lazy_static! {
    pub static ref ROOT_INODE: Arc<Inode> = {
        let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
        Arc::new(EasyFileSystem::root_inode(&efs))
    };
}

/// 列出根目录中的所有应用程序
pub fn list_apps() {
    println!("/**** APPS ****");
    let mut apps = ROOT_INODE.ls();
    apps.sort();
    // 遍历根目录中的所有文件并打印文件名
    for app in apps {
        println!("{}", app);
    }
    println!("**************/");
}

/// 在内核态执行的 EasyFileSystem 自检，覆盖多块写入、偏移写入与清空逻辑
pub fn run_internal_fs_test() {
    println!("[kernel] Running internal fs smoke test");
    const TEST_FILE: &str = "kernel-fs-selftest";
    let inode = ROOT_INODE
        .find(TEST_FILE)
        .or_else(|| ROOT_INODE.create(TEST_FILE))
        .expect("failed to prepare test file");

    const TEST_LENGTHS: &[usize] = &[BLOCK_SZ / 2, BLOCK_SZ * 3, BLOCK_SZ * 5 + 211, BLOCK_SZ * 8];

    for &len in TEST_LENGTHS {
        inode.clear();
        let mut baseline = build_pattern(len, 0x37);
        assert_eq!(inode.write_at(0, baseline.as_slice()), baseline.len());

        let read_back = read_full(&inode, len);
        assert_eq!(read_back.as_slice(), baseline.as_slice());

        if len > BLOCK_SZ {
            let mid_offset = len / 3;
            let patch_len = min(BLOCK_SZ / 2, len - mid_offset);
            let patch = build_pattern(patch_len, 0xA5);
            assert_eq!(inode.write_at(mid_offset, patch.as_slice()), patch.len());
            baseline[mid_offset..mid_offset + patch_len].copy_from_slice(&patch);

            let read_back = read_full(&inode, baseline.len());
            assert_eq!(read_back.as_slice(), baseline.as_slice());
        }
    }

    inode.clear();
    let mut scratch = [0u8; 32];
    assert_eq!(inode.read_at(0, &mut scratch), 0);
    println!("[kernel] internal fs smoke test passed");
}

bitflags! {
    /// open() 系统调用的 flags 参数通过将以下零个或多个值进行 OR 运算构造：
    pub struct OpenFlags: u32 {
        /// 只写
        const WRONLY = 1 << 0;
        /// 读写
        const RDWR = 1 << 1;
        /// 创建新文件
        const CREATE = 1 << 9;
        /// 将文件大小截断为 0
        const TRUNC = 1 << 10;
    }
}

impl OpenFlags {
    /// 只读
    pub const RDONLY: OpenFlags = OpenFlags::empty();
    /// 为简单起见不检查有效性
    /// 返回 (readable, writable)
    pub fn read_write(&self) -> (bool, bool) {
        // 根据打开标志确定文件的读写权限
        if self.is_empty() {
            // 默认为只读
            (true, false)
        } else if self.contains(Self::WRONLY) {
            // 只写模式
            (false, true)
        } else {
            // 读写模式
            (true, true)
        }
    }
}

/// 打开文件
pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    // 根据标志确定文件的读写权限
    let (readable, writable) = flags.read_write();
    if flags.contains(OpenFlags::CREATE) {
        // 如果设置了CREATE标志
        if let Some(inode) = ROOT_INODE.find(name) {
            // 文件已存在，清空文件内容
            inode.clear();
            Some(Arc::new(OSInode::new(readable, writable, inode)))
        } else {
            // 文件不存在，创建新文件
            ROOT_INODE
                .create(name)
                .map(|inode| Arc::new(OSInode::new(readable, writable, inode)))
        }
    } else {
        // 打开已存在的文件
        ROOT_INODE.find(name).map(|inode| {
            // 如果设置了TRUNC标志，清空文件内容
            if flags.contains(OpenFlags::TRUNC) {
                inode.clear();
            }
            Arc::new(OSInode::new(readable, writable, inode))
        })
    }
}

impl File for OSInode {
    fn readable(&self) -> bool {
        self.readable
    }
    fn writable(&self) -> bool {
        self.writable
    }
    fn read(&self, mut buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_read_size = 0usize;
        // 遍历用户缓冲区的所有片段
        for slice in buf.buffers.iter_mut() {
            let read_size = inner.inode.read_at(inner.offset, slice);
            if read_size == 0 {
                break;
            }
            // 更新文件偏移量
            inner.offset += read_size;
            total_read_size += read_size;
        }
        total_read_size
    }
    fn write(&self, buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_write_size = 0usize;
        // 遍历用户缓冲区的所有片段
        for slice in buf.buffers.iter() {
            let write_size = inner.inode.write_at(inner.offset, slice);
            // 确保写入的字节数等于片段大小
            assert_eq!(write_size, slice.len());
            // 更新文件偏移量
            inner.offset += write_size;
            total_write_size += write_size;
        }
        total_write_size
    }
}

fn build_pattern(len: usize, seed: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity(len);
    for i in 0..len {
        data.push(seed.wrapping_add((i as u8).wrapping_mul(13))); // 简单的确定性模式
    }
    data
}

fn read_full(inode: &Arc<Inode>, expected_size: usize) -> Vec<u8> {
    let mut collected = Vec::with_capacity(expected_size);
    let mut offset = 0usize;
    let mut buffer = [0u8; 256];
    loop {
        let read = inode.read_at(offset, &mut buffer);
        if read == 0 {
            break;
        }
        collected.extend_from_slice(&buffer[..read]);
        offset += read;
    }
    assert_eq!(offset, expected_size);
    collected
}
