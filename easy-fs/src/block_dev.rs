use core::any::Any;
/// 块设备特征，定义块级读写接口
/// 以块为单位读写数据的抽象接口
pub trait BlockDevice: Send + Sync + Any {
    /// 从指定块读取数据到缓冲区
    ///
    /// # 参数
    /// * `block_id` - 要读取的块编号
    /// * `buf` - 目标缓冲区，长度应为BLOCK_SZ
    fn read_block(&self, block_id: usize, buf: &mut [u8]);

    /// 从缓冲区写数据到指定块
    ///
    /// # 参数
    /// * `block_id` - 要写入的块编号
    /// * `buf` - 源缓冲区，长度应为BLOCK_SZ
    fn write_block(&self, block_id: usize, buf: &[u8]);
}
