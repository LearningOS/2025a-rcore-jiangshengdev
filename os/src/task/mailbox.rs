use alloc::collections::VecDeque;

/// 每个邮箱允许同时缓存的报文上限。
pub const MAILBOX_CAPACITY: usize = 16;
/// 单条报文允许的最大字节长度。
pub const MAILBOX_MAX_MSG_LEN: usize = 256;

/// 固定大小的报文负载容器。
#[derive(Clone)]
pub struct MailMessage {
    len: usize,
    data: [u8; MAILBOX_MAX_MSG_LEN],
}

/// 邮箱写入失败时的错误类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailboxFull;

impl MailMessage {
    /// 通过指定长度与数据构造报文，超长会触发断言。
    pub fn from_parts(len: usize, data: [u8; MAILBOX_MAX_MSG_LEN]) -> Self {
        assert!(len <= MAILBOX_MAX_MSG_LEN);
        Self { len, data }
    }

    /// 返回报文实际长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 判断报文是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 获取报文负载的只读切片。
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

/// 简单的 FIFO 邮箱，容量受限。
pub struct MailBox {
    queue: VecDeque<MailMessage>,
}

impl Default for MailBox {
    fn default() -> Self {
        Self::new()
    }
}

impl MailBox {
    /// 创建空邮箱。
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    /// 判断邮箱是否为空。
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// 判断邮箱是否已满。
    pub fn is_full(&self) -> bool {
        self.queue.len() >= MAILBOX_CAPACITY
    }

    /// 入队报文；若邮箱已满则返回错误。
    pub fn push(&mut self, message: MailMessage) -> Result<(), MailboxFull> {
        if self.is_full() {
            Err(MailboxFull)
        } else {
            self.queue.push_back(message);
            Ok(())
        }
    }

    /// 取出最早入队的报文。
    pub fn pop(&mut self) -> Option<MailMessage> {
        self.queue.pop_front()
    }

    /// 查看队首报文长度但不出队。
    pub fn peek_len(&self) -> Option<usize> {
        self.queue.front().map(MailMessage::len)
    }
}
