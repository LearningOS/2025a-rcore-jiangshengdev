# PERF 开关速览

- 作用：统一控制内核与 `easy-fs-fuse` 的性能日志。设置 `PERF` 且值非空即开启，未设置或值为空串则关闭。
- 开启例子：`make run PERF=1`、`PERF=1 cargo run`。
- 关闭例子：`make run` 或显式 `make run PERF=`。
- 影响范围：
  - 内核：`time_call!`、`timer::log_instant`、程序/系统调用统计、`exec` 分解日志。
  - easy-fs-fuse：打包阶段的 `[time]` 输出。
- 切换开关后需要重新编译受影响的二进制。
