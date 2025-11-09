use super::BlockDevice;
use crate::mm::{
    frame_alloc, kernel_token, FrameTracker, PageTable, PhysAddr as KernelPhysAddr, PhysPageNum,
    VirtAddr,
};
use crate::sync::UPSafeCell;
use alloc::vec::Vec;
use core::ptr::NonNull;
use easy_fs::BLOCK_SZ;
use lazy_static::*;
use virtio_drivers::{
    device::blk::VirtIOBlk,
    transport::mmio::{MmioTransport, VirtIOHeader},
    BufferDirection, Hal, PhysAddr as VirtioPhysAddr,
};

#[allow(unused)]
const VIRTIO0: usize = 0x10001000;
/// VirtIOBlock device driver strcuture for virtio_blk device
pub struct VirtIOBlock(UPSafeCell<VirtIOBlk<VirtioHal, MmioTransport>>);

lazy_static! {
    /// The global io data queue for virtio_blk device
    static ref QUEUE_FRAMES: UPSafeCell<Vec<FrameTracker>> = unsafe { UPSafeCell::new(Vec::new()) };
}

const SECTOR_SIZE: usize = 512;
const SECTORS_PER_BLOCK: usize = BLOCK_SZ / SECTOR_SIZE;

impl BlockDevice for VirtIOBlock {
    /// Read a block from the virtio_blk device
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        debug_assert_eq!(buf.len(), BLOCK_SZ);
        let mut device = self.0.exclusive_access();
        device
            .read_blocks(block_id * SECTORS_PER_BLOCK, buf)
            .expect("Error when reading VirtIOBlk");
    }
    /// Write a block to the virtio_blk device
    fn write_block(&self, block_id: usize, buf: &[u8]) {
        debug_assert_eq!(buf.len(), BLOCK_SZ);
        let mut device = self.0.exclusive_access();
        device
            .write_blocks(block_id * SECTORS_PER_BLOCK, buf)
            .expect("Error when writing VirtIOBlk");
    }
}

impl Default for VirtIOBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtIOBlock {
    #[allow(unused)]
    /// Create a new VirtIOBlock driver with VIRTIO0 base_addr for virtio_blk device
    pub fn new() -> Self {
        unsafe {
            let header = NonNull::new(VIRTIO0 as *mut VirtIOHeader).unwrap();
            let transport =
                MmioTransport::new(header).expect("failed to create VirtIO MMIO transport");
            let blk = VirtIOBlk::<VirtioHal, _>::new(transport)
                .expect("failed to create VirtIO block device");
            Self(UPSafeCell::new(blk))
        }
    }
}

pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    /// allocate memory for virtio_blk device's io data queue
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (VirtioPhysAddr, NonNull<u8>) {
        assert!(pages > 0);
        let mut ppn_base = PhysPageNum(0);
        let mut queue_frames = QUEUE_FRAMES.exclusive_access();
        for i in 0..pages {
            let frame = frame_alloc().expect("frame allocation failed for virtio queue");
            if i == 0 {
                ppn_base = frame.ppn;
            } else {
                assert_eq!(frame.ppn.0, ppn_base.0 + i);
            }
            queue_frames.push(frame);
        }
        drop(queue_frames);
        let pa: KernelPhysAddr = ppn_base.into();
        let vaddr = pa.0;
        let paddr = vaddr as VirtioPhysAddr;
        let ptr = unsafe { NonNull::new_unchecked(vaddr as *mut u8) };
        (paddr, ptr)
    }
    /// free memory for virtio_blk device's io data queue
    unsafe fn dma_dealloc(paddr: VirtioPhysAddr, _vaddr: NonNull<u8>, pages: usize) -> i32 {
        let base_pa = KernelPhysAddr(paddr);
        let base_ppn: PhysPageNum = base_pa.into();
        let mut frames = QUEUE_FRAMES.exclusive_access();
        let before = frames.len();
        let start = base_ppn.0;
        let end = start + pages;
        frames.retain(|frame| {
            let ppn = frame.ppn.0;
            ppn < start || ppn >= end
        });
        debug_assert_eq!(before.saturating_sub(frames.len()), pages);
        0
    }
    /// translate physical address to virtual address for virtio_blk device
    unsafe fn mmio_phys_to_virt(paddr: VirtioPhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).unwrap()
    }
    /// translate virtual address to physical address for virtio_blk device
    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> VirtioPhysAddr {
        let ptr = buffer.cast::<u8>().as_ptr();
        virt_to_phys(ptr as usize)
    }
    unsafe fn unshare(_paddr: VirtioPhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {
        // Nothing to do, as memory is identity-mapped.
    }
}

fn virt_to_phys(vaddr: usize) -> VirtioPhysAddr {
    PageTable::from_token(kernel_token())
        .translate_va(VirtAddr::from(vaddr))
        .expect("virtual address translation failed for virtio share")
        .0 as VirtioPhysAddr
}
