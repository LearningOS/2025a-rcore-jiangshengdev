use super::BlockDevice;
use crate::config::PAGE_SIZE;
use crate::mm::{
    frame_alloc, frame_dealloc, kernel_token, FrameTracker, PageTable, PhysAddr, PhysPageNum,
    VirtAddr,
};
use crate::sync::UPSafeCell;
use alloc::vec::Vec;
use core::ptr::NonNull;
use easy_fs::BLOCK_SZ;
use lazy_static::*;
use virtio_drivers::device::blk::{VirtIOBlk, SECTOR_SIZE};
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::{BufferDirection, Hal};

type DmaFrame = (usize, FrameTracker);

/// Virtio_Block 设备中控制寄存器的基地址
#[allow(unused)]
const VIRTIO0: usize = 0x10001000;
/// virtio_blk 设备的 VirtIOBlock 设备驱动结构
pub struct VirtIOBlock(UPSafeCell<VirtIOBlk<VirtioHal, MmioTransport>>);

unsafe impl Send for VirtIOBlock {}

lazy_static! {
    static ref DMA_FRAMES: UPSafeCell<Vec<DmaFrame>> = unsafe { UPSafeCell::new(Vec::new()) };
}

impl BlockDevice for VirtIOBlock {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        // 从指定块ID读取数据到缓冲区
        let sectors_per_block = BLOCK_SZ / SECTOR_SIZE;
        let sector_id = block_id * sectors_per_block;
        self.0
            .exclusive_access()
            .read_blocks(sector_id, buf)
            .expect("Error when reading VirtIOBlk");
    }
    fn write_block(&self, block_id: usize, buf: &[u8]) {
        // 将缓冲区数据写入到指定块ID
        let sectors_per_block = BLOCK_SZ / SECTOR_SIZE;
        let sector_id = block_id * sectors_per_block;
        self.0
            .exclusive_access()
            .write_blocks(sector_id, buf)
            .expect("Error when writing VirtIOBlk");
    }
}

impl VirtIOBlock {
    #[allow(unused)]
    /// 使用 VIRTIO0 基地址为 virtio_blk 设备创建新的 VirtIOBlock 驱动
    pub fn new() -> Self {
        unsafe {
            // 初始化VirtIO块设备驱动，使用自定义的HAL实现
            let header = NonNull::new(VIRTIO0 as *mut VirtIOHeader).unwrap();
            let transport = MmioTransport::new(header).expect("无法初始化 VirtIO MMIO 传输");
            Self(UPSafeCell::new(
                VirtIOBlk::<VirtioHal, _>::new(transport).expect("初始化 VirtIOBlk 驱动失败"),
            ))
        }
    }
}

pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (usize, NonNull<u8>) {
        assert!(pages > 0, "请求的 DMA 页数必须大于 0");
        let mut allocated = Vec::with_capacity(pages);
        for i in 0..pages {
            let frame = frame_alloc().expect("DMA 内存分配失败");
            if i == 0 {
                allocated.push(frame);
                continue;
            }
            let base_ppn = allocated[0].ppn;
            assert_eq!(frame.ppn.0, base_ppn.0 + i);
            allocated.push(frame);
        }
        let base_ppn = allocated[0].ppn;
        let base_pa: PhysAddr = base_ppn.into();
        let vaddr = NonNull::new(base_pa.0 as *mut u8).unwrap();
        let mut frames = DMA_FRAMES.exclusive_access();
        for frame in allocated.into_iter() {
            let pa: PhysAddr = frame.ppn.into();
            frames.push((pa.0, frame));
        }
        (base_pa.0, vaddr)
    }

    unsafe fn dma_dealloc(paddr: usize, _vaddr: NonNull<u8>, pages: usize) -> i32 {
        let mut frames = DMA_FRAMES.exclusive_access();
        for i in 0..pages {
            let target = paddr + i * PAGE_SIZE;
            if let Some(index) = frames.iter().position(|(addr, _)| *addr == target) {
                let _ = frames.swap_remove(index);
            } else {
                let pa = PhysAddr::from(target);
                let ppn: PhysPageNum = pa.into();
                frame_dealloc(ppn);
            }
        }
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: usize, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).unwrap()
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> usize {
        let vaddr = buffer.cast::<u8>().as_ptr() as usize;
        virt_to_phys(vaddr)
    }

    unsafe fn unshare(_paddr: usize, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}

fn virt_to_phys(vaddr: usize) -> usize {
    PageTable::from_token(kernel_token())
        .translate_va(VirtAddr::from(vaddr))
        .unwrap()
        .0
}
