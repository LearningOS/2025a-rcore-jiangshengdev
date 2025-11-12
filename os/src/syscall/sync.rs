use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;

// 死锁检测失败时返回给用户态的特定错误码。
const DEADLOCK_ERR: isize = -0xDEAD;

/// 返回当前任务对应的线程 ID，用于死锁检测矩阵索引。
fn current_tid() -> usize {
    // 获取当前任务的引用。
    let task = current_task().unwrap();
    // 进入任务内部以读取线程标识。
    let guard = task.inner_exclusive_access();
    // 取得调度上下文保存的 tid。
    let tid = guard.res.as_ref().unwrap().tid;
    // 将 tid 返回给调用者。
    tid
}
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    // 在现有空槽或列表末尾申请互斥锁编号。
    let id = if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() - 1
    };
    // 初始化对应互斥锁的死锁检测记录。
    process_inner.deadlock.reset_mutex(id);
    id as isize
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    // 读取当前线程的 tid，用于死锁检测矩阵索引。
    let tid = current_tid();
    let process = current_process();
    let mutex = {
        // 进入进程内部执行死锁检测预登记。
        let mut process_inner = process.inner_exclusive_access();
        // 在 need 表中记录本次互斥锁请求。
        process_inner.deadlock.stage_mutex_request(tid, mutex_id, 1);
        // 若检测到潜在死锁，则回滚并直接返回错误码。
        if !process_inner.deadlock.check_mutex_safe() {
            process_inner
                .deadlock
                .unstage_mutex_request(tid, mutex_id, 1);
            return DEADLOCK_ERR;
        }
        // 死锁检测通过，克隆互斥锁引用准备加锁。
        Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap())
    };
    drop(process);
    // 真正尝试获取互斥锁。
    mutex.lock();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    // 加锁成功后提交分配信息。
    process_inner
        .deadlock
        .commit_mutex_allocation(tid, mutex_id, 1);
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    // 再次读取 tid，以便在释放记录时定位线程。
    let tid = current_tid();
    let process = current_process();
    let mutex = {
        // 克隆互斥锁引用以便在锁外操作。
        let process_inner = process.inner_exclusive_access();
        Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap())
    };
    drop(process);
    // 释放互斥锁本体。
    mutex.unlock();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    // 更新检测器中的 allocation 表，反映互斥锁释放。
    process_inner
        .deadlock
        .release_mutex_allocation(tid, mutex_id, 1);
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    // 复用空槽或向后追加以获取信号量编号。
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };
    // 重置对应信号量的资源总量与簿记信息。
    process_inner.deadlock.reset_semaphore(id, res_count);
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    // 读取当前线程 tid 以便更新检测器。
    let tid = current_tid();
    let process = current_process();
    let sem = {
        // 克隆信号量引用以便在锁外执行 up 操作。
        let process_inner = process.inner_exclusive_access();
        Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap())
    };
    drop(process);
    // 执行一次资源释放。
    sem.up();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    // 释放成功后在检测器中扣减 allocation。
    process_inner
        .deadlock
        .release_semaphore_allocation(tid, sem_id, 1);
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    // 获取当前线程 tid，作为死锁检测访问索引。
    let tid = current_tid();
    let process = current_process();
    let sem = {
        // 进入进程内部登记信号量请求。
        let mut process_inner = process.inner_exclusive_access();
        process_inner
            .deadlock
            .stage_semaphore_request(tid, sem_id, 1);
        // 若预测到死锁则立即回滚并返回错误码。
        if !process_inner.deadlock.check_semaphore_safe() {
            process_inner
                .deadlock
                .unstage_semaphore_request(tid, sem_id, 1);
            return DEADLOCK_ERR;
        }
        // 通过检测后克隆信号量句柄。
        Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap())
    };
    drop(process);
    // 可能阻塞的 down 操作在检测通过后执行。
    sem.down();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    // 将成功获得的资源数量写入 allocation 表。
    process_inner
        .deadlock
        .commit_semaphore_allocation(tid, sem_id, 1);
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// 启用或禁用死锁检测的系统调用
///
/// 说明：负责在进程级别更新检测开关，具体检测逻辑散布于互斥锁与信号量的调用路径。
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect");
    match enabled {
        0 | 1 => {
            // 获取当前进程以修改其死锁检测状态。
            let process = current_process();
            let mut process_inner = process.inner_exclusive_access();
            // 当参数为 1 时开启，为 0 时关闭检测。
            process_inner.deadlock.set_enabled(enabled == 1);
            0
        }
        _ => -1,
    }
}
