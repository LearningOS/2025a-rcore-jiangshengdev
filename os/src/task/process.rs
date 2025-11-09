//! [`ProcessControlBlock`] 的实现。

use super::id::RecycleAllocator;
use super::manager::insert_into_pid2process;
use super::TaskControlBlock;
use super::{add_task, SignalFlags};
use super::{pid_alloc, PidHandle};
use crate::fs::{File, Stdin, Stdout};
use crate::mm::{translated_refmut, MemorySet, KERNEL_SPACE};
use crate::sync::{Condvar, Mutex, Semaphore, UPSafeCell};
use crate::timer::get_time_us;
use crate::trap::{trap_handler, TrapContext};
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefMut;

fn normalize_program_name(path: &str) -> String {
    String::from(path.rsplit('/').next().unwrap_or(path))
}

/// 进程控制块
pub struct ProcessControlBlock {
    /// 不可变部分
    pub pid: PidHandle,
    /// 可变部分
    inner: UPSafeCell<ProcessControlBlockInner>,
}

/// `exec` 流程中各阶段的耗时细分（微秒）
pub struct ExecPerfDetail {
    pub reset_us: usize,
    pub memory_set_us: usize,
    pub install_us: usize,
    pub user_res_us: usize,
    pub argv_us: usize,
    pub trap_us: usize,
}

/// 进程控制块的内部状态
pub struct ProcessControlBlockInner {
    /// 是否为僵尸进程
    pub is_zombie: bool,
    /// 地址空间（内存集合）
    pub memory_set: MemorySet,
    /// 父进程
    pub parent: Option<Weak<ProcessControlBlock>>,
    /// 子进程列表
    pub children: Vec<Arc<ProcessControlBlock>>,
    /// 退出码
    pub exit_code: i32,
    /// 文件描述符表
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    /// 信号标志位
    pub signals: SignalFlags,
    /// 任务（线程）列表
    pub tasks: Vec<Option<Arc<TaskControlBlock>>>,
    /// 任务资源分配器
    pub task_res_allocator: RecycleAllocator,
    /// 互斥锁列表
    pub mutex_list: Vec<Option<Arc<dyn Mutex>>>,
    /// 信号量列表
    pub semaphore_list: Vec<Option<Arc<Semaphore>>>,
    /// 条件变量列表
    pub condvar_list: Vec<Option<Arc<Condvar>>>,
    /// 进程对应的程序名称
    pub name: String,
    /// 进程累计的用户态时间（毫秒）
    pub user_time_ms: usize,
    /// 进程累计的内核态时间（毫秒）
    pub kernel_time_ms: usize,
}

impl ProcessControlBlockInner {
    #[allow(unused)]
    /// 获取应用页表地址
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// 分配新的文件描述符
    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }
    /// 分配新的任务 ID
    pub fn alloc_tid(&mut self) -> usize {
        self.task_res_allocator.alloc()
    }
    /// 回收任务 ID
    pub fn dealloc_tid(&mut self, tid: usize) {
        self.task_res_allocator.dealloc(tid)
    }
    /// 当前进程中的任务（线程）数量
    pub fn thread_count(&self) -> usize {
        self.tasks.len()
    }
    /// 根据 tid 获取进程内的任务
    pub fn get_task(&self, tid: usize) -> Arc<TaskControlBlock> {
        self.tasks[tid].as_ref().unwrap().clone()
    }
}

impl ProcessControlBlock {
    /// 独占访问内部状态
    pub fn inner_exclusive_access(&self) -> RefMut<'_, ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// 根据 ELF 文件创建新进程
    pub fn new(elf_data: &[u8], name: &str) -> Arc<Self> {
        trace!("kernel: ProcessControlBlock::new");
        // 根据 ELF 生成的内存集合，包含程序头、trampoline、trap 上下文与用户栈
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        // 分配一个 PID
        let pid_handle = pid_alloc();
        let program_name = normalize_program_name(name);
        let process = Arc::new(Self {
            pid: pid_handle,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
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
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    name: program_name,
                    user_time_ms: 0,
                    kernel_time_ms: 0,
                })
            },
        });
        // 创建主线程，此处需分配用户栈与 trap_cx
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&process),
            ustack_base,
            true,
        ));
        // 准备主线程的 trap_cx
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        let ustack_top = task_inner.res.as_ref().unwrap().ustack_top();
        let kstack_top = task.kstack.get_top();
        drop(task_inner);
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            ustack_top,
            KERNEL_SPACE.exclusive_access().token(),
            kstack_top,
            trap_handler as usize,
        );
        crate::dbg_hold(trap_cx);
        // 将主线程加入进程
        let mut process_inner = process.inner_exclusive_access();
        process_inner.tasks.push(Some(Arc::clone(&task)));
        drop(process_inner);
        insert_into_pid2process(process.getpid(), Arc::clone(&process));
        // 将主线程加入调度器
        add_task(task);
        process
    }

    /// 仅支持单线程进程。
    pub fn exec(
        self: &Arc<Self>,
        elf_data: &[u8],
        args: Vec<String>,
        path: &str,
    ) -> ExecPerfDetail {
        trace!("kernel: exec");
        {
            let inner = self.inner_exclusive_access();
            assert_eq!(inner.thread_count(), 1);
        }
        let reset_start = get_time_us();
        let new_program_name = normalize_program_name(path);
        let (old_name, old_user_time_ms, old_kernel_time_ms, tasks_to_reset) = {
            let mut inner = self.inner_exclusive_access();
            let old_name = inner.name.clone();
            let old_user_time_ms = inner.user_time_ms;
            let old_kernel_time_ms = inner.kernel_time_ms;
            inner.name = new_program_name;
            inner.user_time_ms = 0;
            inner.kernel_time_ms = 0;
            let tasks = inner
                .tasks
                .iter()
                .filter_map(|t| t.as_ref().map(Arc::clone))
                .collect::<Vec<_>>();
            (old_name, old_user_time_ms, old_kernel_time_ms, tasks)
        };
        if old_user_time_ms > 0 || old_kernel_time_ms > 0 {
            super::time::accumulate_program_time(&old_name, old_user_time_ms, old_kernel_time_ms);
        }
        for task in tasks_to_reset {
            let mut task_inner = task.inner_exclusive_access();
            task_inner.user_time_ms = 0;
            task_inner.kernel_time_ms = 0;
        }
        let reset_us = get_time_us().saturating_sub(reset_start);
        // 根据 ELF 构建新的内存集合（程序头 / trampoline / trap 上下文 / 用户栈）
        trace!("kernel: exec .. MemorySet::from_elf");
        let memory_start = get_time_us();
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        let memory_set_us = get_time_us().saturating_sub(memory_start);
        let new_token = memory_set.token();
        // 替换原有内存集合
        trace!("kernel: exec .. substitute memory_set");
        let install_start = get_time_us();
        self.inner_exclusive_access().memory_set = memory_set;
        let install_us = get_time_us().saturating_sub(install_start);
        // 由于内存集合已改变，需要重新为主线程分配用户资源
        trace!("kernel: exec .. alloc user resource for main thread again");
        let task = self.inner_exclusive_access().get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        let user_res_start = get_time_us();
        task_inner.res.as_mut().unwrap().ustack_base = ustack_base;
        task_inner.res.as_mut().unwrap().alloc_user_res();
        task_inner.trap_cx_ppn = task_inner.res.as_mut().unwrap().trap_cx_ppn();
        task_inner.user_time_ms = 0;
        task_inner.kernel_time_ms = 0;
        let user_res_us = get_time_us().saturating_sub(user_res_start);
        // 将参数压入用户栈
        trace!("kernel: exec .. push arguments on user stack");
        let argv_start = get_time_us();
        let mut user_sp = task_inner.res.as_mut().unwrap().ustack_top();
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_refmut(
                    new_token,
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
                *translated_refmut(new_token, p as *mut u8) = *c;
                p += 1;
            }
            *translated_refmut(new_token, p as *mut u8) = 0;
        }
        // 对齐栈指针到 8 字节（适配 k210 平台）
        user_sp -= user_sp % core::mem::size_of::<usize>();
        let argv_us = get_time_us().saturating_sub(argv_start);
        // 初始化 trap_cx
        trace!("kernel: exec .. initialize trap_cx");
        let trap_start = get_time_us();
        let mut trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            task.kstack.get_top(),
            trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *task_inner.get_trap_cx() = trap_cx;
        crate::dbg_hold(task_inner.get_trap_cx());
        let trap_us = get_time_us().saturating_sub(trap_start);
        ExecPerfDetail {
            reset_us,
            memory_set_us,
            install_us,
            user_res_us,
            argv_us,
            trap_us,
        }
    }

    /// 仅支持单线程进程。
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        trace!("kernel: fork");
        let mut parent = self.inner_exclusive_access();
        assert_eq!(parent.thread_count(), 1);
        // 完整克隆父进程的内存集合（含 trampoline、用户栈、trap_cx）
        let memory_set = MemorySet::from_existed_user(&parent.memory_set);
        // 分配 PID
        let pid = pid_alloc();
        // 拷贝文件描述符表
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        // 创建子进程 PCB
        let child_program_name = parent.name.clone();
        let child = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: new_fd_table,
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    name: child_program_name,
                    user_time_ms: 0,
                    kernel_time_ms: 0,
                })
            },
        });
        // 将子进程挂到父进程名下
        parent.children.push(Arc::clone(&child));
        // 为子进程创建主线程
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&child),
            parent
                .get_task(0)
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .ustack_base(),
            // 此处不重新分配 trap_cx 或用户栈，但会分配新的内核栈
            false,
        ));
        // 将线程挂入子进程
        let mut child_inner = child.inner_exclusive_access();
        child_inner.tasks.push(Some(Arc::clone(&task)));
        drop(child_inner);
        // 更新该线程 trap_cx 中的内核栈顶指针
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        trap_cx.kernel_sp = task.kstack.get_top();
        crate::dbg_hold(trap_cx);
        drop(task_inner);
        insert_into_pid2process(child.getpid(), Arc::clone(&child));
        // 将线程加入调度器
        add_task(task);
        child
    }
    /// 获取 PID
    pub fn getpid(&self) -> usize {
        self.pid.0
    }
}
