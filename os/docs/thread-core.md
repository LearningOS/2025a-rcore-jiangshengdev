# 线程实现核心（极简版）

只讲最核心：线程如何被创建、被调度、如何完成上下文切换并进入/继续运行用户态。忽略信号量、条件变量等同步细节。

## 核心概念
- 线程=一个可调度实体，关键由两份“快照”组成：
  - TaskContext（内核态切换用）：保存 `ra/sp` 与 `s0~s11`，用于在内核中切换线程。
  - TrapContext（用户态恢复用）：保存用户态寄存器、`sepc/sstatus`、以及返回到用户态所需的信息。
- 每个线程属于某个进程（PCB），共享同一地址空间与资源，但拥有独立的用户栈与内核栈。

## 关键结构（放在哪）
- TaskControlBlock：`src/task/task.rs`
  - `kstack`：该线程的内核栈。
  - `inner.task_cx: TaskContext`：内核态切换保存点（下一次 `__switch` ret 的位置）。
  - `inner.trap_cx_ppn`：存放 TrapContext 的物理页号。
  - `inner.task_status`：就绪/运行/阻塞。
- TaskContext：`src/task/context.rs`
  - 字段：`ra`, `sp`, `s[12]`。
  - `goto_trap_return(kstack_top)`：令 `ra=trap_return`、`sp=kstack_top`，保证“第一次被调度”时会跳到 `trap_return` 走用户态恢复。
- TrapContext：`src/trap/context.rs`
  - `app_init_context(entry, ustack_top, kernel_satp, kstack_top, trap_handler)`：设置用户入口 `sepc=entry`、用户栈 SP、以及返回到内核的入口 `trap_handler` 等。

## 唯一关键路径：线程切换如何发生
- 触发调度的时机：
  - `sys_yield()`：`src/syscall/process.rs` → `suspend_current_and_run_next()`。
  - 定时器中断：`src/trap/mod.rs` 的 `trap_handler()` 中调用 `suspend_current_and_run_next()`。
- `suspend_current_and_run_next()`：`src/task/mod.rs`
  - 将当前线程状态改为 Ready，放回就绪队列，然后调用 `schedule(task_cx_ptr)`。
- `schedule()`：`src/task/processor.rs`
  - 通过 `__switch(switched_task_cx_ptr, idle_task_cx_ptr)` 切回处理器的空闲上下文（idle），让出 CPU，回到调度循环。
- 调度循环 `run_tasks()`：`src/task/processor.rs`
  - 从就绪队列取下一个任务，置为 Running，然后 `__switch(idle_task_cx_ptr, next_task_cx_ptr)` 切入目标线程。
- `__switch`（汇编真切换）：`src/task/switch.S`
  - 保存当前的 `ra/sp/s0~s11` 到 `current_task_cx`。
  - 加载 `next_task_cx` 的 `ra/sp/s0~s11`。
  - `ret` → 跳到下一个线程 TaskContext 中的 `ra`（首次为 `trap_return`）。

## 首次运行 vs 继续运行
- 新线程创建：`sys_thread_create(entry, arg)`：`src/syscall/thread.rs`
  - 分配 TCB 与内核栈、用户栈，创建 `TaskContext::goto_trap_return(kstack_top)`。
  - 初始化 `TrapContext = app_init_context(entry, ustack_top, kernel_satp, kstack_top, trap_handler)`；设置 `a0=arg`。
  - 放入就绪队列。
- 首次被调度：
  - `__switch(..., next_task_cx)` 后 `ret` 到 `trap_return`（因为 `TaskContext.ra=trap_return`）。
  - `trap_return()`：`src/trap/mod.rs` → 跳到 `__restore`，用 TrapContext 恢复用户寄存器/`sepc`/`sstatus`，进入用户态从 `entry` 执行。
- 后续再次运行：
  - 用户态执行过程中发生 trap（系统调用/中断）进入内核；再次让出时保存 TaskContext/TrapContext。
  - 下次再被选中时，`__switch` 恢复 TaskContext，随后 `trap_return` 基于 TrapContext 回到用户态，继续先前的执行点。

## 退出（只看主线）
- `sys_exit(code)` 或信号致命错误：`exit_current_and_run_next(code)`：`src/syscall/process.rs`/`src/task/mod.rs`
  - 记录退出码；若为主线程，额外做进程回收（不展开）。
  - 最后调用 `schedule()` 放弃 CPU，调度下一任务。

## 调度器极简模型
- `TaskManager`：`src/task/manager.rs`
  - FIFO 就绪队列：`add()` 放入、`fetch()` 取出。
- `Processor`：`src/task/processor.rs`
  - 保存 `current` 与一个 `idle_task_cx`。
  - 两个切换点：
    - `run_tasks()`：`idle -> next`。
    - `schedule()`：`current -> idle`。

## 一句话总览
线程切换 = 在内核用 `__switch` 保存当前 TaskContext、选择下一个、加载下一个 TaskContext；若是首次运行，`ret` 跳到 `trap_return`，通过 TrapContext 恢复用户态并从 `sepc` 开始执行。

## 代码速查
- TaskContext：`src/task/context.rs`
- __switch：`src/task/switch.S`（声明于 `src/task/switch.rs`）
- 调度主循环/让出：`src/task/processor.rs`（`run_tasks`/`schedule`）
- 就绪队列：`src/task/manager.rs`
- TCB/状态：`src/task/task.rs`
- 线程创建：`src/syscall/thread.rs`（`sys_thread_create`）
- 返回用户态：`src/trap/mod.rs`（`trap_return`）
