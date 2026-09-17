# 无需 root 的 Linux 进程跟踪

[English](process-linux-rootless.md) | 中文

`ora_process_runtime::LinuxBestEffort::with_discarded_io()` 是接入 `ScopeRuntime` 的首个真实 Linux
adapter。不需要 root、sudo、特权 helper、cgroup 委派、服务安装或独立业务账号；业务沿用调用方身份。
[特权 helper 路线](process-helper.zh.md) 保留，但当前不优先推进。

## 准入与所有权

adapter 只报告 `BestEffortOnly`。`RequireStrong` 在启动前拒绝；`PreferStrong` 在接受运行前选择
`BestEffort`。收尾结果只能是 `BestEffortComplete`，不能是 `ConfirmedQuiescence`。

构造时检查 procfs 可读性、pidfd 操作和 `waitid(P_PIDFD)` 支持，并拒绝通过忽略 `SIGCHLD` 或
`SA_NOCLDWAIT` 自动回收子进程的环境。受限内核、procfs 挂载或系统调用策略可能导致构造失败；
无需 root 不代表支持所有容器或 gVisor 配置。procfs 必须对应调用方的 PID namespace。

每个 Run 在 exec 前建立独立 session。环境严格来自 `RunSpec.env`，不会隐式继承宿主环境。
此构造入口明确丢弃三个标准流，尚不能直接用于需要输入输出的 Git／插件集成。

调用方必须驱动 `ScopeRuntime::reconcile`，并独占子进程回收权。跟踪期间，其他线程或信号处理器
不得回收这些子进程，也不得启用自动回收。直接子进程直到已跟踪对象完成清理才被 reap，
即使已退出也保留 session ID 的身份锚点。启动后获取 pidfd 失败仍保留所有权并报告启动未知；
后续观测重试获取，不再次启动业务。

## 发现、停止与证据

- 扫描原 session 成员，在获取 pidfd 期间固定 proc 目录；已捕获的 pidfd 在成员后续脱离 session
  或被重新托管后仍保留。不从历史 PPID 猜测归属，不向数字进程组广播信号。
- 通知发送 `SIGTERM`，强制发送 `SIGKILL`。持续保留停止意图，后续发现的成员也收到信号。
  发现失败不阻止向已捕获成员发送信号。
- 通过 pidfd 发送信号。只有独占、尚未 reap 的直接子进程，在获取 pidfd 或发信号失败时，
  才允许以 `Child::kill` 兜底强制停止；原失败仍保持可见。
- 只有直接进程和已捕获成员的退出先于一次成功的新扫描、扫描没有发现新身份、已捕获成员仍全部
  退出时，才报告尽力收尾完成并 reap 直接子进程。扫描／信号错误及存活成员均阻止完成。
- Drop 尝试强制清理并启动直接子进程回收线程。这不是完成证明，也不能应对所有者崩溃或被 `SIGKILL`。

在**被发现之前**新建 session 的后代，以及在原 session 外新生的后代，可能逃过跟踪。
pidfd 避免向复用的 PID 错发信号，但不能让发现完备，也不能阻止逃离。这是尽力清理，
不是隔离同用户业务的安全边界。

## 验证与待完成项

以普通 Linux 用户运行 `cargo test -p ora-process-runtime --test linux_best_effort` 和
`cargo test -p ora-utils --test linux_process`。真实进程测试覆盖准入、重传不重复执行、直接退出后后代
存活、Run 隔离、通知／强制升级、已捕获成员 `setsid`，以及 Drop 对直接子进程的清理。
工具测试覆盖 pidfd 退出与回收、过期 proc 观测和非 UTF-8 进程名；没有通过强制数字 PID 复用或
耗尽描述符验证身份复用及启动后获取失败恢复的完整路径。

持久宿主／guardian 所有权、崩溃恢复、I/O 交接和生产入口仍未实现，详见[运行体系状态](process-runtime.zh.md)。
这些测试不完成强纳管 ADR，也不能从 Linux 结果推导 Windows／macOS 支持。
