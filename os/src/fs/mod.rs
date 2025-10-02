//! File trait & inode(dir, file, pipe, stdin, stdout)

mod inode;
mod stdio;

use crate::mm::UserBuffer;

/// trait File for all file types
pub trait File: Send + Sync {
    /// the file readable?
    fn readable(&self) -> bool;
    /// the file writable?
    fn writable(&self) -> bool;
    /// read from the file to buf, return the number of bytes read
    fn read(&self, buf: UserBuffer) -> usize;
    /// write to the file from buf, return the number of bytes written
    fn write(&self, buf: UserBuffer) -> usize;
    /// 获取文件的可选 stat 信息
    fn stat(&self) -> Option<StatInfo> {
        // 默认实现不返回任何元数据
        None
    }
}

/// The stat of a inode
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Stat {
    /// 所在的设备编号，实验中固定为 0
    pub dev: u64,
    /// 文件对应的 inode 编号
    pub ino: u64,
    /// 文件类型及权限模式
    pub mode: StatMode,
    /// 当前的硬链接数量
    pub nlink: u32,
    /// unused pad
    pad: [u64; 7],
}

impl Stat {
    /// 创建带有默认填充字段的 Stat 对象
    pub fn new(dev: u64, ino: u64, mode: StatMode, nlink: u32) -> Self {
        // 按照 POSIX stat 布局初始化结构体
        Self {
            dev,
            ino,
            mode,
            nlink,
            pad: [0; 7],
        }
    }
}

/// stat 系统调用使用的简化元数据描述
#[derive(Clone, Copy, Debug)]
pub struct StatInfo {
    /// inode 编号
    pub ino: u64,
    /// 文件类型对应的模式位
    pub mode: StatMode,
    /// 硬链接数量
    pub nlink: u32,
}

bitflags! {
    /// The mode of a inode
    /// whether a directory or a file
    pub struct StatMode: u32 {
        /// null
        const NULL  = 0;
        /// directory
        const DIR   = 0o040000;
        /// ordinary regular file
        const FILE  = 0o100000;
    }
}

pub use inode::{link_file, list_apps, open_file, unlink_file, OpenFlags};
pub use stdio::{Stdin, Stdout};
