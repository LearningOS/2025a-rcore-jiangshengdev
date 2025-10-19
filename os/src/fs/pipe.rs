use super::File;
use crate::mm::UserBuffer;
use crate::sync::UPSafeCell;
use alloc::sync::{Arc, Weak};

use crate::task::suspend_current_and_run_next;

/// 进程间通信管道结构
///
/// 管道是一种单向通信机制，用于在进程间传递数据。
/// 每个管道有两端：读端和写端。数据从写端写入，从读端读出。
/// 管道使用环形缓冲区来存储数据，支持阻塞式读写操作。
pub struct Pipe {
    /// 标识此管道端是否可读
    readable: bool,
    /// 标识此管道端是否可写
    writable: bool,
    /// 指向共享环形缓冲区的引用，读端和写端共享同一个缓冲区
    buffer: Arc<UPSafeCell<PipeRingBuffer>>,
}

impl Pipe {
    /// 创建管道的读端
    ///
    /// 读端只能进行读操作，不能写入数据。
    /// 当读端尝试读取数据时，如果缓冲区为空且写端仍然存在，
    /// 读操作会阻塞等待数据到达。
    pub fn read_end_with_buffer(buffer: Arc<UPSafeCell<PipeRingBuffer>>) -> Self {
        Self {
            readable: true,
            writable: false,
            buffer,
        }
    }

    /// 创建管道的写端
    ///
    /// 写端只能进行写操作，不能读取数据。
    /// 当写端尝试写入数据时，如果缓冲区已满，
    /// 写操作会阻塞等待缓冲区有空间。
    pub fn write_end_with_buffer(buffer: Arc<UPSafeCell<PipeRingBuffer>>) -> Self {
        Self {
            readable: false,
            writable: true,
            buffer,
        }
    }
}

/// 管道环形缓冲区的大小（字节）
///
/// 32字节的缓冲区大小是一个合理的选择，既不会占用太多内存，
/// 又能提供足够的缓冲空间来减少进程间的阻塞等待。
const RING_BUFFER_SIZE: usize = 32;

/// 环形缓冲区的状态枚举
///
/// 用于快速判断缓冲区的当前状态，避免每次都计算可用空间。
#[derive(Copy, Clone, PartialEq)]
enum RingBufferStatus {
    /// 缓冲区已满，无法写入更多数据
    Full,
    /// 缓冲区为空，无数据可读
    Empty,
    /// 缓冲区处于正常状态，既不满也不空
    Normal,
}

/// 管道的环形缓冲区实现
///
/// 环形缓冲区是一种高效的FIFO数据结构，使用固定大小的数组
/// 和两个指针（head和tail）来实现。当指针到达数组末尾时，
/// 会回绕到数组开头，形成"环形"结构。
///
/// 这种设计的优点：
/// 1. 内存使用固定，不会动态增长
/// 2. 读写操作都是O(1)时间复杂度
/// 3. 天然支持生产者-消费者模式
pub struct PipeRingBuffer {
    /// 存储数据的固定大小数组
    arr: [u8; RING_BUFFER_SIZE],
    /// 读指针，指向下一个要读取的数据位置
    head: usize,
    /// 写指针，指向下一个要写入的数据位置
    tail: usize,
    /// 缓冲区当前状态，用于快速判断满/空状态
    status: RingBufferStatus,
    /// 写端的弱引用，用于检测写端是否已关闭
    /// 使用弱引用避免循环引用导致的内存泄漏
    write_end: Option<Weak<Pipe>>,
}

impl Default for PipeRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl PipeRingBuffer {
    /// 创建新的空环形缓冲区
    ///
    /// 初始状态下，head和tail都指向0，缓冲区为空。
    /// 数组内容初始化为0，但这不是必需的，因为只有在
    /// 写入数据后才会读取对应位置的内容。
    pub fn new() -> Self {
        Self {
            arr: [0; RING_BUFFER_SIZE],
            head: 0,
            tail: 0,
            status: RingBufferStatus::Empty,
            write_end: None,
        }
    }

    /// 设置写端的弱引用
    ///
    /// 这个弱引用用于检测写端是否已经被关闭。当所有写端都被
    /// 关闭时，读端可以知道不会再有新数据写入，从而可以在
    /// 缓冲区为空时立即返回EOF，而不是继续阻塞等待。
    ///
    /// 使用弱引用而不是强引用的原因：
    /// 1. 避免循环引用：缓冲区引用写端，写端也引用缓冲区
    /// 2. 允许写端正常释放，不会因为缓冲区持有引用而无法释放
    pub fn set_write_end(&mut self, write_end: &Arc<Pipe>) {
        self.write_end = Some(Arc::downgrade(write_end));
    }
    /// 向环形缓冲区写入一个字节
    ///
    /// 写入过程：
    /// 1. 将状态设置为Normal（除非写入后变满）
    /// 2. 在tail位置写入数据
    /// 3. tail指针前进一位，使用模运算实现环形回绕
    /// 4. 检查是否已满：当tail追上head时，缓冲区满
    ///
    /// 注意：调用此函数前应确保缓冲区未满，否则会覆盖未读数据
    pub fn write_byte(&mut self, byte: u8) {
        self.status = RingBufferStatus::Normal;
        self.arr[self.tail] = byte;
        self.tail = (self.tail + 1) % RING_BUFFER_SIZE;
        // 当tail追上head时，说明缓冲区已满
        // 这时所有RING_BUFFER_SIZE个位置都有数据
        if self.tail == self.head {
            self.status = RingBufferStatus::Full;
        }
    }

    /// 从环形缓冲区读取一个字节
    ///
    /// 读取过程：
    /// 1. 将状态设置为Normal（除非读取后变空）
    /// 2. 从head位置读取数据
    /// 3. head指针前进一位，使用模运算实现环形回绕
    /// 4. 检查是否已空：当head追上tail时，缓冲区空
    ///
    /// 注意：调用此函数前应确保缓冲区非空，否则会读取到未定义数据
    pub fn read_byte(&mut self) -> u8 {
        self.status = RingBufferStatus::Normal;
        let c = self.arr[self.head];
        self.head = (self.head + 1) % RING_BUFFER_SIZE;
        // 当head追上tail时，说明缓冲区已空
        // 这时所有数据都已被读取
        if self.head == self.tail {
            self.status = RingBufferStatus::Empty;
        }
        c
    }
    /// 计算当前可读取的字节数
    ///
    /// 环形缓冲区中数据量的计算需要考虑两种情况：
    /// 1. tail >= head：数据连续存储，直接相减
    /// 2. tail < head：数据跨越了数组边界，需要分两段计算
    ///
    /// 特殊情况处理：
    /// - Empty状态：head == tail且无数据，返回0
    /// - Full状态：head == tail但有数据，返回RING_BUFFER_SIZE
    pub fn available_read(&self) -> usize {
        if self.status == RingBufferStatus::Empty {
            0
        } else if self.tail > self.head {
            // 数据连续存储在[head, tail)区间
            self.tail - self.head
        } else {
            // 数据分布在两段：[head, RING_BUFFER_SIZE) 和 [0, tail)
            self.tail + RING_BUFFER_SIZE - self.head
        }
    }

    /// 计算当前可写入的字节数
    ///
    /// 可写入空间 = 总空间 - 已使用空间
    /// 这个计算基于available_read()的结果，确保一致性。
    pub fn available_write(&self) -> usize {
        if self.status == RingBufferStatus::Full {
            0
        } else {
            RING_BUFFER_SIZE - self.available_read()
        }
    }

    /// 检查所有写端是否都已关闭
    ///
    /// 通过尝试将弱引用升级为强引用来检测写端是否仍然存在：
    /// - 如果upgrade()返回Some，说明写端仍然存在
    /// - 如果upgrade()返回None，说明写端已被释放
    ///
    /// 这个检查对于读端的阻塞行为很重要：
    /// - 如果写端仍存在，读端在无数据时应该阻塞等待
    /// - 如果写端已关闭，读端在无数据时应该返回EOF
    pub fn all_write_ends_closed(&self) -> bool {
        self.write_end.as_ref().unwrap().upgrade().is_none()
    }
}

/// 创建一个新的管道，返回(读端, 写端)
///
/// 管道创建过程：
/// 1. 创建共享的环形缓冲区，使用Arc包装以支持多个引用
/// 2. 创建读端和写端，它们都引用同一个缓冲区
/// 3. 在缓冲区中设置写端的弱引用，用于检测写端关闭
///
/// 返回值：
/// - 第一个元素：管道的读端，只能读取数据
/// - 第二个元素：管道的写端，只能写入数据
///
/// 典型用法：
/// ```rust
/// let (read_end, write_end) = make_pipe();
/// // 将read_end给一个进程，write_end给另一个进程
/// ```
pub fn make_pipe() -> (Arc<Pipe>, Arc<Pipe>) {
    // 创建共享的环形缓冲区，使用UPSafeCell提供内部可变性
    let buffer = Arc::new(unsafe { UPSafeCell::new(PipeRingBuffer::new()) });

    // 创建管道的两端，它们共享同一个缓冲区
    let read_end = Arc::new(Pipe::read_end_with_buffer(buffer.clone()));
    let write_end = Arc::new(Pipe::write_end_with_buffer(buffer.clone()));

    // 在缓冲区中保存写端的弱引用，用于检测写端是否关闭
    // 这样当写端被释放时，读端可以检测到并停止阻塞等待
    buffer.exclusive_access().set_write_end(&write_end);

    (read_end, write_end)
}

impl File for Pipe {
    /// 检查管道是否可读
    ///
    /// 只有管道的读端才返回true，写端始终返回false
    fn readable(&self) -> bool {
        self.readable
    }

    /// 检查管道是否可写
    ///
    /// 只有管道的写端才返回true，读端始终返回false
    fn writable(&self) -> bool {
        self.writable
    }

    /// 从管道读取数据
    ///
    /// 管道读取的特点：
    /// 1. 阻塞式读取：如果没有数据且写端仍存在，会阻塞等待
    /// 2. EOF检测：如果没有数据且写端已关闭，返回已读取的字节数
    /// 3. 部分读取：可能读取少于请求的字节数
    ///
    /// 读取流程：
    /// 1. 检查缓冲区中的可用数据
    /// 2. 如果无数据可读：
    ///    - 检查写端是否关闭，如果关闭则返回EOF
    ///    - 如果写端仍存在，阻塞等待新数据
    /// 3. 如果有数据可读，尽可能多地读取数据
    /// 4. 重复上述过程直到读满用户缓冲区或遇到EOF
    ///
    /// 参数：
    /// - buf: 用户提供的缓冲区，可能跨越多个物理页面
    ///
    /// 返回值：
    /// - 实际读取的字节数，可能小于请求的字节数
    fn read(&self, buf: UserBuffer) -> usize {
        // 确保这是管道的读端
        assert!(self.readable());

        let want_to_read = buf.len();
        let mut buf_iter = buf.into_iter();
        let mut already_read = 0usize;

        // 循环读取，直到读满缓冲区或遇到EOF
        loop {
            let mut ring_buffer = self.buffer.exclusive_access();
            let loop_read = ring_buffer.available_read();

            if loop_read == 0 {
                // 缓冲区为空，检查是否应该阻塞等待
                if ring_buffer.all_write_ends_closed() {
                    // 写端已关闭，不会再有新数据，返回EOF
                    return already_read;
                }
                // 写端仍存在，释放锁并阻塞等待新数据
                drop(ring_buffer);
                suspend_current_and_run_next();
                continue;
            }

            // 有数据可读，尽可能多地读取
            for _ in 0..loop_read {
                if let Some(byte_ref) = buf_iter.next() {
                    // 从环形缓冲区读取一个字节到用户缓冲区
                    unsafe {
                        *byte_ref = ring_buffer.read_byte();
                    }
                    already_read += 1;

                    // 检查是否已读满用户缓冲区
                    if already_read == want_to_read {
                        return want_to_read;
                    }
                } else {
                    // 用户缓冲区已满，返回已读取的字节数
                    return already_read;
                }
            }
            // 继续循环，尝试读取更多数据
        }
    }
    /// 向管道写入数据
    ///
    /// 管道写入的特点：
    /// 1. 阻塞式写入：如果缓冲区满，会阻塞等待空间
    /// 2. 原子性：写入操作是原子的，不会与其他写入操作交错
    /// 3. 部分写入：可能写入少于请求的字节数（当用户缓冲区耗尽时）
    ///
    /// 写入流程：
    /// 1. 检查缓冲区中的可用空间
    /// 2. 如果无空间可写：
    ///    - 阻塞等待读端消费数据，释放空间
    /// 3. 如果有空间可写，尽可能多地写入数据
    /// 4. 重复上述过程直到写完用户缓冲区中的所有数据
    ///
    /// 注意事项：
    /// - 写入操作不会检查读端是否关闭
    /// - 如果读端关闭但缓冲区未满，写入仍可继续
    /// - 只有当缓冲区满且读端关闭时，写入才会永久阻塞
    ///
    /// 参数：
    /// - buf: 用户提供的数据缓冲区，可能跨越多个物理页面
    ///
    /// 返回值：
    /// - 实际写入的字节数，通常等于请求的字节数
    fn write(&self, buf: UserBuffer) -> usize {
        // 确保这是管道的写端
        assert!(self.writable());

        let want_to_write = buf.len();
        let mut buf_iter = buf.into_iter();
        let mut already_write = 0usize;

        // 循环写入，直到写完所有数据
        loop {
            let mut ring_buffer = self.buffer.exclusive_access();
            let loop_write = ring_buffer.available_write();

            if loop_write == 0 {
                // 缓冲区已满，释放锁并阻塞等待空间
                // 当读端消费数据后，会有空间可写
                drop(ring_buffer);
                suspend_current_and_run_next();
                continue;
            }

            // 有空间可写，尽可能多地写入数据
            // 最多写入loop_write个字节，避免超出可用空间
            for _ in 0..loop_write {
                if let Some(byte_ref) = buf_iter.next() {
                    // 从用户缓冲区读取一个字节写入环形缓冲区
                    ring_buffer.write_byte(unsafe { *byte_ref });
                    already_write += 1;

                    // 检查是否已写完所有数据
                    if already_write == want_to_write {
                        return want_to_write;
                    }
                } else {
                    // 用户缓冲区已空，返回已写入的字节数
                    return already_write;
                }
            }
            // 继续循环，尝试写入更多数据
        }
    }
}
