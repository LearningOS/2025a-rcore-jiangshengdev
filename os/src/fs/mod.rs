//! File trait 和 inode（目录、文件、管道、标准输入、标准输出）

mod inode;
mod pipe;
mod stdio;

use crate::mm::UserBuffer;

/// 所有文件类型的 File trait
pub trait File: Send + Sync {
    /// 文件是否可读？
    fn readable(&self) -> bool;
    /// 文件是否可写？
    fn writable(&self) -> bool;
    /// 从文件读取到缓冲区，返回读取的字节数
    fn read(&self, buf: UserBuffer) -> usize;
    /// 从缓冲区写入到文件，返回写入的字节数
    fn write(&self, buf: UserBuffer) -> usize;
}

/// inode 的状态信息
#[repr(C)]
#[derive(Debug)]
pub struct Stat {
    /// 包含文件的设备 ID
    pub dev: u64,
    /// inode 编号
    pub ino: u64,
    /// 文件类型和模式
    pub mode: StatMode,
    /// 硬链接数量
    pub nlink: u32,
    /// 未使用的填充
    pad: [u64; 7],
}

bitflags! {
    /// inode 的模式
    /// 是目录还是文件
    pub struct StatMode: u32 {
        /// 目录
        const DIR   = 0o040000;
        /// 普通常规文件
        const FILE  = 0o100000;
    }
}

impl StatMode {
    /// 空
    pub const NULL: StatMode = StatMode::empty();
}

pub use inode::{list_apps, open_file, run_internal_fs_test, OSInode, OpenFlags};
pub use pipe::{make_pipe, Pipe};
pub use stdio::{Stdin, Stdout};
