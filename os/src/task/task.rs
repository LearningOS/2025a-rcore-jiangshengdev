//! Types related to task management & Functions for completely changing TCB
use super::TaskContext;
use super::{kstack_alloc, pid_alloc, KernelStack, PidHandle};
use crate::config::TRAP_CONTEXT_BASE;
use crate::mm::{MapError, MapPermission, MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE};
use crate::sync::UPSafeCell;
use crate::trap::{trap_handler, TrapContext};
use alloc::collections::BTreeMap;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::cell::RefMut;

const BIG_STRIDE: usize = 1 << 20;
const DEFAULT_PRIORITY: usize = 16;

/// Task control block structure
///
/// Directly save the contents that will not change during running
pub struct TaskControlBlock {
    // Immutable
    /// Process identifier
    pub pid: PidHandle,

    /// Kernel stack corresponding to PID
    pub kernel_stack: KernelStack,

    /// Mutable
    inner: UPSafeCell<TaskControlBlockInner>,
}

impl TaskControlBlock {
    /// Get the mutable reference of the inner TCB
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// Get the address of app's page table
    pub fn get_user_token(&self) -> usize {
        let inner = self.inner_exclusive_access();
        inner.memory_set.token()
    }
}

pub struct TaskControlBlockInner {
    /// The physical page number of the frame where the trap context is placed
    pub trap_cx_ppn: PhysPageNum,

    /// Application data can only appear in areas
    /// where the application address space is lower than base_size
    pub base_size: usize,

    /// Save task context
    pub task_cx: TaskContext,

    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,

    /// Application address space
    pub memory_set: MemorySet,

    /// Parent process of the current process.
    /// Weak will not affect the reference count of the parent
    pub parent: Option<Weak<TaskControlBlock>>,

    /// A vector containing TCBs of all child processes of the current process
    pub children: Vec<Arc<TaskControlBlock>>,

    /// It is set when active exit or execution error occurs
    pub exit_code: i32,

    /// Heap bottom
    pub heap_bottom: usize,

    /// Program break
    pub program_brk: usize,

    /// Regions created via mmap, keyed by start address
    pub mmap_regions: BTreeMap<usize, usize>,

    /// Current accumulated stride for stride scheduling
    pub stride: usize,

    /// Scheduling priority (higher value means lower share)
    pub priority: usize,

    /// Increment added to stride each time the task is scheduled
    pub pass: usize,
}

impl TaskControlBlockInner {
    /// get the trap context
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    /// get the user token
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
    pub fn is_zombie(&self) -> bool {
        self.get_status() == TaskStatus::Zombie
    }

    /// Map a new anonymous memory region for the task.
    pub fn mmap(&mut self, start: usize, len: usize, perm: MapPermission) -> Result<(), MapError> {
        if len == 0 {
            return Ok(());
        }
        let end = start.checked_add(len).ok_or(MapError::AddressOverflow)?;
        if self.mmap_regions.contains_key(&start) {
            return Err(MapError::AlreadyMapped);
        }
        let start_va = VirtAddr::from(start);
        let end_va = VirtAddr::from(end);
        self.memory_set
            .mmap(start_va, end_va, perm | MapPermission::U)?;
        self.mmap_regions.insert(start, len);
        Ok(())
    }

    /// Unmap a previously mapped anonymous memory region.
    pub fn munmap(&mut self, start: usize, len: usize) -> Result<(), MapError> {
        if len == 0 {
            return Ok(());
        }
        let end = start.checked_add(len).ok_or(MapError::AddressOverflow)?;
        let recorded_len = self
            .mmap_regions
            .get(&start)
            .ok_or(MapError::RegionNotFound)?;
        if *recorded_len != len {
            return Err(MapError::RangeMismatch);
        }
        let start_va = VirtAddr::from(start);
        let end_va = VirtAddr::from(end);
        self.memory_set.munmap(start_va, end_va)?;
        self.mmap_regions.remove(&start);
        Ok(())
    }

    fn set_priority(&mut self, priority: usize) {
        self.priority = priority;
        self.pass = calc_pass(priority);
    }

    pub fn bump_stride(&mut self) {
        let increment = self.pass.max(1);
        self.stride = self.stride.saturating_add(increment);
    }
}

impl TaskControlBlock {
    /// Create a new process
    ///
    /// At present, it is only used for the creation of initproc
    pub fn new(elf_data: &[u8]) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = kstack_alloc();
        let kernel_stack_top = kernel_stack.get_top();
        // push a task context which goes to trap_return to the top of kernel stack
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
                    heap_bottom: user_sp,
                    program_brk: user_sp,
                    mmap_regions: BTreeMap::new(),
                    stride: 0,
                    priority: DEFAULT_PRIORITY,
                    pass: calc_pass(DEFAULT_PRIORITY),
                })
            },
        };
        // prepare TrapContext in user space
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

    /// Load a new elf to replace the original application address space and start execution
    pub fn exec(&self, elf_data: &[u8]) {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();

        // **** access current TCB exclusively
        let mut inner = self.inner_exclusive_access();
        // substitute memory_set
        inner.memory_set = memory_set;
        // update trap_cx ppn
        inner.trap_cx_ppn = trap_cx_ppn;
        // initialize base_size
        inner.base_size = user_sp;
        inner.mmap_regions.clear();
        // initialize trap_cx
        let trap_cx = inner.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            self.kernel_stack.get_top(),
            trap_handler as usize,
        );
        // **** release inner automatically
    }

    /// parent process fork the child process
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        // ---- access parent PCB exclusively
        let mut parent_inner = self.inner_exclusive_access();
        // copy user space(include trap context)
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = kstack_alloc();
        let kernel_stack_top = kernel_stack.get_top();
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
                    heap_bottom: parent_inner.heap_bottom,
                    program_brk: parent_inner.program_brk,
                    mmap_regions: parent_inner.mmap_regions.clone(),
                    stride: parent_inner.stride,
                    priority: parent_inner.priority,
                    pass: parent_inner.pass,
                })
            },
        });
        // add child
        parent_inner.children.push(task_control_block.clone());
        // modify kernel_sp in trap_cx
        // **** access child PCB exclusively
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        trap_cx.kernel_sp = kernel_stack_top;
        // return
        task_control_block
        // **** release child PCB
        // ---- release parent PCB
    }

    /// get pid of process
    pub fn getpid(&self) -> usize {
        self.pid.0
    }

    /// change the location of the program break. return None if failed.
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

    /// Map a new anonymous region for the task.
    pub fn mmap(&self, start: usize, len: usize, perm: MapPermission) -> Result<(), MapError> {
        self.inner_exclusive_access().mmap(start, len, perm)
    }

    /// Unmap a previously mapped anonymous region for the task.
    pub fn munmap(&self, start: usize, len: usize) -> Result<(), MapError> {
        self.inner_exclusive_access().munmap(start, len)
    }

    /// Get current stride value used by the stride scheduler.
    pub fn stride(&self) -> usize {
        let inner = self.inner_exclusive_access();
        inner.stride
    }

    /// Increase stride when the scheduler selects this task.
    pub fn bump_stride(&self) {
        let mut inner = self.inner_exclusive_access();
        inner.bump_stride();
    }

    /// Adjust task priority and return the updated value.
    pub fn set_priority(&self, priority: usize) -> usize {
        let mut inner = self.inner_exclusive_access();
        inner.set_priority(priority);
        inner.priority
    }

    /// Query the current task priority.
    pub fn priority(&self) -> usize {
        let inner = self.inner_exclusive_access();
        inner.priority
    }
}

#[derive(Copy, Clone, PartialEq)]
/// task status: UnInit, Ready, Running, Exited
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Zombie,
}

fn calc_pass(priority: usize) -> usize {
    let priority = priority.max(1);
    let pass = BIG_STRIDE / priority;
    pass.max(1)
}
