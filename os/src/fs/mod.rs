//! File trait & inode(dir, file, pipe, stdin, stdout)

mod inode;
mod stdio;

use crate::mm::UserBuffer;
use easy_fs::Stat;

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
    /// retrieve the file metadata if available
    fn stat(&self) -> Option<Stat> {
        None
    }
}

pub use inode::{link_file, list_apps, open_file, unlink_file, OpenFlags};
pub use stdio::{Stdin, Stdout};
