use core::any::Any;
/// 块设备特征
/// 以块为单位读写数据
pub trait BlockDevice: Send + Sync + Any {
    /// 从块读取数据到缓冲区
    fn read_block(&self, block_id: usize, buf: &mut [u8]);
    /// 从缓冲区写数据到块
    fn write_block(&self, block_id: usize, buf: &[u8]);
}
