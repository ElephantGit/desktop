# Linux rootless 适配器

[English](linux-rootless.md) | 中文

`ora_process_runtime::LinuxBestEffort` 是 `ScopeRuntime` 的 Linux 平台适配器。它不需要 root、sudo、特权 helper、cgroup 委派、
服务安装或单独的账户；工作负载保持调用方的身份（Node 还可以把工作负载降到 `process.workload_uid`）。特权的替代方案见
[Linux helper](linux-helper.zh.md)。

## 准入

适配器只声明 `BestEffortOnly`：`RequireStrong` 在启动之前被拒绝，`PreferStrong` 选择尽力而为，完成状态是 `BestEffortComplete`，
从不是 `ConfirmedQuiescence`。构造时检查可读的 procfs（描述调用方的 PID namespace）、pidfd 操作和 `waitid(P_PIDFD)`，并拒绝自动
回收子进程（忽略 `SIGCHLD` 或设置 `SA_NOCLDWAIT`）。受限的内核、procfs 挂载或系统调用策略可能让构造失败，所以并非所有容器或
gVisor 配置都受支持。

每个 Run 在 exec 之前开启新的 session，环境恰好是 `RunSpec.env`，stdin 关闭。`with_discarded_io()` 丢弃全部三个流并拒绝捕获
请求；`with_bounded_output()` 接受 `OutputPolicy::Capture { stdout_limit, stderr_limit }`。

## 有界输出

上限是每个流的字节数，也是精确 `RunSpec` 重放身份的一部分；为零时只接受空输出，恰好达到上限不算溢出。每个被捕获的 pipe 有一个
独立的读取器，即使没有消费者也持续读空，只保留有界的前缀。溢出或读取器失败会在下一次 `reconcile` 时请求强制清理，即使在
`WaitForAll` 下也是如此。

`ScopeRuntime::read_output(run, stream, offset, max_bytes)` 只复制不消费，报告保留的长度、一旦发生就保持的截断标志，以及
`Open`、`Eof` 或 `Failed`；超出前缀的 offset 是错误。带截断的 EOF 是不完整的，读取失败也不是 EOF。直接退出、跟踪的清理和 pipe
EOF 彼此独立：直接子进程退出后，后代进程仍可能持有 pipe。字节不持久化，也没有 stdin、持久化、背压或轮转。

## 发现、停止与证据

- 适配器扫描原来的 session 寻找成员，在获取 pidfd 时固定它们的 proc 目录，成员脱离或被重新挂到别的父进程时保留已获取的 pidfd。
  从不根据历史的 PPID 推断归属，也从不向数字进程组广播信号。
- 通知通过 pidfd 发送 `SIGTERM`，强制发送 `SIGKILL`；停止意图会保留，所以之后发现的成员也会收到。只有自己拥有的、尚未回收的
  直接子进程可以退回到 `Child::kill`。
- 只有当直接退出和每个已获取的退出都发生在一次新的、成功的且没有发现新身份的扫描之前，才报告尽力而为的完成。直接子进程只在这时
  才被回收，所以在清理完成之前它的 session id 保持它的身份。扫描或信号出错以及仍然活着的成员都会阻止完成。
- 调用方必须驱动 `reconcile` 并独占子进程回收；不能有其他线程或处理器回收这些子进程。启动后获取 pidfd 失败时保留归属并报告
  不确定的启动，之后的观测会重试获取，而不是再启动一次。
- 丢弃适配器会尝试强制清理，并启动一个回收直接子进程的 reaper；这不是清理完成的证明，也不能在 owner 崩溃或被 `SIGKILL` 后
  保留。

在被发现之前创建另一个 session 或在原 session 之外诞生的后代进程可能逃逸。PID 句柄防止向被复用的 PID 发信号，但不能让发现变得
完备。这里有意不作为针对同一用户工作负载的安全边界。

## 测试

以普通用户运行 `cargo test -p ora-process-runtime --test linux_best_effort`、`--test linux_output` 和
`cargo test -p ora-utils --test linux_process --test pipe_capture`。它们使用带就绪握手的真实子进程和后代进程，从不修改测试
运行器的环境。
