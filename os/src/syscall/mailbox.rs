//! 邮箱相关系统调用实现

use crate::{
    mm::translated_byte_buffer,
    task::{current_task, current_user_token, pid2task, MailMessage, MAILBOX_MAX_MSG_LEN},
};

/// 从当前进程邮箱读取一条报文。
pub fn sys_mail_read(buf: *mut u8, len: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mail_read",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    // 限制读取长度，保证单次读取不超过协议上限。
    let read_limit = len.min(MAILBOX_MAX_MSG_LEN);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    // len==0 作为探测，可直接返回状态而不消费报文。
    if read_limit == 0 {
        return if inner.mailbox.is_empty() { -1 } else { 0 };
    }
    // 出队一条报文，邮箱为空则直接返回失败。
    let message = match inner.mailbox.pop() {
        Some(msg) => msg,
        None => return -1,
    };
    let message_len = message.len();
    let copy_len = core::cmp::min(message_len, read_limit);
    let payload = message.as_slice();
    drop(inner);
    if copy_len > 0 {
        let mut offset = 0usize;
        // 分页复制到用户缓冲区，截断到用户给定长度。
        for chunk in translated_byte_buffer(token, buf as *const u8, copy_len) {
            if offset >= copy_len {
                break;
            }
            let step = core::cmp::min(chunk.len(), copy_len - offset);
            chunk[..step].copy_from_slice(&payload[offset..offset + step]);
            offset += step;
        }
    }
    message_len as isize
}

/// 向指定进程邮箱发送一条报文。
pub fn sys_mail_write(pid: usize, buf: *const u8, len: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mail_write",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let target = match pid2task(pid) {
        Some(task) => task,
        None => return -1,
    };
    // 同样对写入长度做上限限制。
    let write_len = len.min(MAILBOX_MAX_MSG_LEN);
    // len==0 时作为探测，直接查看目标邮箱是否还有空间。
    if write_len == 0 {
        let inner = target.inner_exclusive_access();
        return if inner.mailbox.is_full() { -1 } else { 0 };
    }
    let mut payload = [0u8; MAILBOX_MAX_MSG_LEN];
    let mut offset = 0usize;
    // 从用户空间分段复制报文内容，直到达到写入长度。
    for chunk in translated_byte_buffer(token, buf, write_len) {
        if offset >= write_len {
            break;
        }
        let step = core::cmp::min(chunk.len(), write_len - offset);
        payload[offset..offset + step].copy_from_slice(&chunk[..step]);
        offset += step;
    }
    // 如果用户缓冲区不足，则视为失败。
    if offset < write_len {
        return -1;
    }
    let message = MailMessage::from_parts(write_len, payload);
    let mut inner = target.inner_exclusive_access();
    // 邮箱满或 push 失败都直接返回错误。
    if inner.mailbox.is_full() {
        return -1;
    }
    if inner.mailbox.push(message).is_err() {
        return -1;
    }
    write_len as isize
}
