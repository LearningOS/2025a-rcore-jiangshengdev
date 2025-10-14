//! 与任务管理相关的类型和完全更改 TCB 的函数

use super::{
    kstack_alloc, pid_alloc, KernelStack, PidHandle, SignalActions, SignalFlags, TaskContext,
};
use crate::{
    config::TRAP_CONTEXT_BASE,
    fs::{File, Stdin, Stdout},
    mm::{translated_refmut, MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE},
    sync::UPSafeCell,
    trap::{trap_handler, TrapContext},
};
use alloc::{
    string::String,
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::cell::RefMut;

/// 任务控制块结构
///
/// 直接保存运行期间不会改变的内容
pub struct TaskControlBlock {
    // 不可变
    /// 进程标识符
    pub pid: PidHandle,

    /// 对应 PID 的内核栈
    pub kernel_stack: KernelStack,

    /// 可变
    inner: UPSafeCell<TaskControlBlockInner>,
}

impl TaskControlBlock {
    /// 获取内部 TCB 的可变引用
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// 获取应用程序页表的地址
    pub fn get_user_token(&self) -> usize {
        let inner = self.inner_exclusive_access();
        inner.memory_set.token()
    }
}

pub struct TaskControlBlockInner {
    /// 放置陷阱上下文的帧的物理页号
    pub trap_cx_ppn: PhysPageNum,

    /// 应用程序数据只能出现在应用程序地址空间
    /// 低于 base_size 的区域中
    pub base_size: usize,

    /// 保存任务上下文
    pub task_cx: TaskContext,

    /// 维护当前进程的执行状态
    pub task_status: TaskStatus,

    /// 应用程序地址空间
    pub memory_set: MemorySet,

    /// 当前进程的父进程。
    /// Weak 不会影响父进程的引用计数
    pub parent: Option<Weak<TaskControlBlock>>,

    /// 包含当前进程所有子进程 TCB 的向量
    pub children: Vec<Arc<TaskControlBlock>>,

    /// 当主动退出或执行错误发生时设置
    pub exit_code: i32,
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    pub signals: SignalFlags,
    pub signal_mask: SignalFlags,
    // the signal which is being handling
    pub handling_sig: isize,
    // Signal actions
    pub signal_actions: SignalActions,
    // if the task is killed
    pub killed: bool,
    // if the task is frozen by a signal
    pub frozen: bool,
    pub trap_ctx_backup: Option<TrapContext>,

    /// 堆底部
    pub heap_bottom: usize,

    /// 程序中断点
    pub program_brk: usize,
}

impl TaskControlBlockInner {
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
    pub fn is_zombie(&self) -> bool {
        self.get_status() == TaskStatus::Zombie
    }
    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }
}

impl TaskControlBlock {
    /// 创建一个新进程
    ///
    /// 目前仅用于创建 initproc
    pub fn new(elf_data: &[u8]) -> Self {
        // 包含 elf 程序头/跳板/陷阱上下文/用户栈的内存集
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        // 在内核空间中分配一个 PID 和一个内核栈
        let pid_handle = pid_alloc();
        let kernel_stack = kstack_alloc();
        let kernel_stack_top = kernel_stack.get_top();
        // 将一个跳转到 trap_return 的任务上下文推入内核栈顶部
        let task_control_block = Self {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: user_sp,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: vec![
                        // 0 -> 标准输入
                        Some(Arc::new(Stdin)),
                        // 1 -> 标准输出
                        Some(Arc::new(Stdout)),
                        // 2 -> 标准错误
                        Some(Arc::new(Stdout)),
                    ],
                    signals: SignalFlags::empty(),
                    signal_mask: SignalFlags::empty(),
                    handling_sig: -1,
                    signal_actions: SignalActions::default(),
                    killed: false,
                    frozen: false,
                    trap_ctx_backup: None,
                    heap_bottom: user_sp,
                    program_brk: user_sp,
                })
            },
        };
        // 在用户空间中准备陷阱上下文
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }

    /// 加载新的 elf 文件替换原始应用程序地址空间并开始执行
    pub fn exec(&self, elf_data: &[u8], args: Vec<String>) {
        // 包含 elf 程序头/跳板/陷阱上下文/用户栈的内存集
        let (memory_set, mut user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        // push arguments on user stack
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_refmut(
                    memory_set.token(),
                    (argv_base + arg * core::mem::size_of::<usize>()) as *mut usize,
                )
            })
            .collect();
        *argv[args.len()] = 0;
        for i in 0..args.len() {
            user_sp -= args[i].len() + 1;
            *argv[i] = user_sp;
            let mut p = user_sp;
            for c in args[i].as_bytes() {
                *translated_refmut(memory_set.token(), p as *mut u8) = *c;
                p += 1;
            }
            *translated_refmut(memory_set.token(), p as *mut u8) = 0;
        }
        // make the user_sp aligned to 8B for k210 platform
        user_sp -= user_sp % core::mem::size_of::<usize>();

        // **** access current TCB exclusively
        let mut inner = self.inner_exclusive_access();
        // 替换内存集
        inner.memory_set = memory_set;
        // 更新陷阱上下文物理页号
        inner.trap_cx_ppn = trap_cx_ppn;
        // 初始化陷阱上下文
        let mut trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            self.kernel_stack.get_top(),
            trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *inner.get_trap_cx() = trap_cx;
        // **** release current PCB
    }

    /// 父进程 fork 子进程
    pub fn fork(self: &Arc<TaskControlBlock>) -> Arc<TaskControlBlock> {
        // ---- 持有父进程 PCB 锁
        let mut parent_inner = self.inner_exclusive_access();
        // 复制用户空间（包括陷阱上下文）
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        // 在内核空间中分配一个 PID 和一个内核栈
        let pid_handle = pid_alloc();
        let kernel_stack = kstack_alloc();
        let kernel_stack_top = kernel_stack.get_top();
        // 复制文件描述符表
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent_inner.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: parent_inner.base_size,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: new_fd_table,
                    signals: SignalFlags::empty(),
                    // inherit the signal_mask and signal_action
                    signal_mask: parent_inner.signal_mask,
                    handling_sig: -1,
                    signal_actions: parent_inner.signal_actions.clone(),
                    killed: false,
                    frozen: false,
                    trap_ctx_backup: None,
                    heap_bottom: parent_inner.heap_bottom,
                    program_brk: parent_inner.program_brk,
                })
            },
        });
        // 添加子进程
        parent_inner.children.push(task_control_block.clone());
        // 修改陷阱上下文中的内核栈指针
        // **** 独占访问子进程 PCB
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        trap_cx.kernel_sp = kernel_stack_top;
        // 返回
        task_control_block
        // **** 释放子进程 PCB
        // ---- release parent PCB
    }

    /// 获取进程的 PID
    pub fn getpid(&self) -> usize {
        self.pid.0
    }

    /// 更改程序中断点的位置。如果失败则返回 None
    pub fn change_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner_exclusive_access();
        let heap_bottom = inner.heap_bottom;
        let old_break = inner.program_brk;
        let new_brk = inner.program_brk as isize + size as isize;
        if new_brk < heap_bottom as isize {
            return None;
        }
        let result = if size < 0 {
            inner
                .memory_set
                .shrink_to(VirtAddr(heap_bottom), VirtAddr(new_brk as usize))
        } else {
            inner
                .memory_set
                .append_to(VirtAddr(heap_bottom), VirtAddr(new_brk as usize))
        };
        if result {
            inner.program_brk = new_brk as usize;
            Some(old_break)
        } else {
            None
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
/// 任务状态：未初始化、就绪、运行中、已退出
pub enum TaskStatus {
    /// 未初始化
    UnInit,
    /// 准备运行
    Ready,
    /// 运行中
    Running,
    /// 已退出
    Zombie,
}
