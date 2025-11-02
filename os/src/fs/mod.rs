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
    /// Query file metadata if supported.
    fn stat(&self) -> Option<(u32, StatMode, u32)> {
        None
    }
}

/// The stat of a inode
#[repr(C)]
#[derive(Debug)]
pub struct Stat {
    /// ID of device containing file
    pub dev: u64,
    /// inode number
    pub ino: u64,
    /// file type and mode
    pub mode: StatMode,
    /// number of hard links
    pub nlink: u32,
    /// unused pad
    pad: [u64; 7],
}

impl Stat {
    /// Create a new stat structure with zeroed padding bytes.
    pub fn new(dev: u64, ino: u64, mode: StatMode, nlink: u32) -> Self {
        Self {
            dev,
            ino,
            mode,
            nlink,
            pad: [0; 7],
        }
    }
}

bitflags! {
    /// The mode of a inode
    /// whether a directory or a file
    pub struct StatMode: u32 {
        /// directory
        const DIR   = 0o040000;
        /// ordinary regular file
        const FILE  = 0o100000;
    }
}

impl StatMode {
    /// Mode field is empty.
    pub const NONE: Self = Self::empty();
}

pub use inode::{link_file, list_apps, open_file, unlink_file, OSInode, OpenFlags};
pub use stdio::{Stdin, Stdout};
