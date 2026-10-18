# 进程运行时

[English](process-runtime.md) | 中文

进程运行时运行 Node 的受管程序（目前是 Git），让它们的启动、退出和清理成为持久的事实，而不是内存中的假设。它由三个相互协作
的部分组成，都只支持 Linux，并信任本地环境：

```text
ora-node ──IPC──▶ ora-process-host ──IPC──▶ ora-process-guardian（每个 Scope 一个）──▶ 工作负载 Run
           host.sock / host-io.sock         control.sock / events.sock / io.sock
```

- **宿主**（`apps/ora-process-host`）拥有持久的 Scope 和 Run 意图，并协调各个 guardian，见[宿主](host.zh.md)。
- 每个 **guardian**（`apps/ora-process-guardian`）是一个独立的进程，拥有一个 Scope 的 Run 及其观测结果，比宿主和 Node 活得
  更久，见 [Guardian](guardian.zh.md)。
- **生命周期内核**（`ora-process-runtime::ScopeRuntime`）和平台适配器判定每个观测证明了什么；在 Linux 上，适配器是
  [rootless 尽力而为](linux-rootless.zh.md)。

Desktop 不使用这套组件：它的子进程经过 `ora-process` 和 [reaper](../desktop/desktop-host.zh.md#进程清理)。设计决策是
[进程相关 ADR](https://github.com/ora-space/specs/blob/main/decisions/node/process/README.md)，核心测试用例在
[`test-cases/node/process/`](https://github.com/ora-space/specs/blob/main/test-cases/node/process/README.md) 下。

## Crate

| Crate | 拥有 |
|---|---|
| `ora-process-protocol` | Run 身份、精确的启动规格（`RunSpec`）、隔离方式选择、停止意图、退出事实和清理证据；有界的 MessagePack 宿主与 guardian 消息；helper 的检查用线上类型 |
| `ora-process-client` | 宿主和 guardian 的客户端，只依赖协议类型 |
| `ora-process-runtime` | `ScopeRuntime`、Linux 适配器、`HostState`、`HostCoordinator` 和 guardian 内部实现 |

## 生命周期内核

`ScopeRuntime<P>` 拥有一个 Scope 的准入、Run 记录和停止截止时间。`Platform` 提供经过验证的能力、创建时的隔离、观测和对单个
Run 的信号。

- 创建 Scope 时冻结实际的保证。要求强隔离但不可用时直接拒绝，请求尽力而为时也从不悄悄升级。
- 以相同参数重放一个 RunId 返回它当前的事实；参数改变则冲突。未知的启动从不重试。
- 关闭先封住准入，再安排清理；停止一个 Run 不影响它的 Scope 和同级 Run。等待、先通知再等待、强制这几种请求只会收紧截止时间
  或升级。
- 直接退出和后代进程清理是两个独立的事实。清理策略先通知后代进程，宽限期之后强制结束它们；wait-for-all 会一直管理它们，
  直到它们退出或被停止。
- 只送达信号从不证明清理完成，失败的观测保留责任。退出状态可以变得更精确，但不会变得更模糊；相互矛盾的观测保持阻塞。

调用通过可变所有权串行化，由 owner 用单调的 `Instant` 驱动 `reconcile`：内核没有后台任务、定时器或基于 `Drop` 的清理，丢弃
它也不是崩溃恢复。平台方法必须有界，并保持稳定的尝试身份，包括在一次结果不确定的启动之后。

## 保证什么，不保证什么

- 在 Linux 上隔离是**尽力而为**的：在被发现之前逃逸的后代进程可能存活下来，完成状态是 `BestEffortComplete`，从不是经过确认的
  静默。能提供强 cgroup 隔离的特权 helper 路径目前只提供部署检查和检查服务，见 [Linux helper](linux-helper.zh.md)；cgroup、
  macOS 和 Windows 的隔离决策都是 `approved`，但这里还没有实现。
- 部署信任本地的 Node 和管理程序：这些通道上没有秘密 token、签名密钥或 Controller 授权。私有路径和内核的对端 UID 检查防止
  意外的跨用户访问，但防不住同一 UID 下的恶意代码。宿主和 guardian 的 epoch 让过期的协作实例在被接管后无法再行动。
- 输出以有界、不持久化的前缀形式捕获；stdin 是关闭的。插件进程还没有在这些 Scope 中运行，见
  [Node Agent 会话](../node/agent-sessions.zh.md#记录顺序与崩溃)。
- SQLite 日志（宿主、guardian）启用 WAL 和 `synchronous=FULL`，并要求链接的 SQLite 至少为 3.51.3，其中包含 WAL 重置的修复；
  物理断电下的行为没有验证。

## 文件锁

`ora_utils::fs::LinuxFileLock` 是 guardian 启动引导所用的 flock 原语。它以不等待的方式排他地获取一个已打开的普通文件（有竞争时
返回 `WouldBlock`）；clone 会复制已加锁的打开描述，`into_file()` 把它交给子进程，中间不解锁再加锁。描述符默认设置
close-on-exec。按照 Linux flock 的生命周期语义，`Drop` 只关闭描述符，从不显式解锁，因为显式解锁可能释放子进程共享的锁。调用方
必须保持 inode 不变，解析受信路径并确认文件系统合格；这个锁既不证明持久性，也不证明工作负载已被清理。
