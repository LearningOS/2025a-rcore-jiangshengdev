# os/src/syscall/process.rs 变更说明

## 核心改动

- 实现 `sys_get_time`：对用户态传入的 `TimeVal` 指针进行合法性检查，通过 `translated_refmut` 映射到内核可写的引用。
- 读取 `get_time_us` 返回的微秒级时间，拆分为秒与微秒两个部分回写到 `TimeVal`。

## 额外细节

- 在调用日志中保留了原有的 `trace!`，方便调试。
- 函数遇到空指针直接返回 `-1`，保持与其他系统调用一致的错误处理策略。
