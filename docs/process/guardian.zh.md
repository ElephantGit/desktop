# Guardian

[English](guardian.md) | 中文

`ora-process-guardian`（`apps/ora-process-guardian`）是拥有一个 Scope 的 Run 的独立进程。[宿主](host.zh.md)为每个 Scope 启动它
一次；之后它比宿主和 Node 活得更久，下一个宿主实例会重新绑定到它，而不是重新启动它。这个很薄的应用把启动引导、接管和 Run 操作
都委托给 `ora-process-runtime`。

## 启动引导

宿主在 exec 之前提交 Scope 的创建意图和一条已消耗的启动记录。一个专用的 socketpair 投递启动引导身份，原来排他的 Scope flock
被继承下来，中间不解锁再加锁。子进程进入自己的 session，清空环境，并在 exec 时关闭不该继承的描述符；argv 中只出现
`--bootstrap`，不安装父进程死亡或基于 `Drop` 的 kill 策略。

guardian 校验继承的打开描述、它的 Scope inode、私有路径和本地文件系统。只有它初始化 `guardian.sqlite`（WAL，
`synchronous=FULL`，`user_version = 4`），然后发布 `control.sock`、`events.sock` 和 `io.sock`。初始化之后才是 `Ready`；它既不
证明某个 Run 已经启动，也不证明清理已经完成。启动错误、丢失的回复和 guardian 死亡从不授权为同一个 Scope 启动另一个 guardian，
已有的日志、锁和端点也从不被自动修复。

## 接管

`GuardianManagement::bind` 接收在 `HostState` 下提交的宿主绑定。更高的绑定在确认之前提交，同一绑定的重试是幂等的，更低的 epoch
或同一 epoch 下不同的实例会被拒绝。宿主会话只携带公开的绑定，而不是 bearer 凭据。请求在执行互斥锁下根据当前的日志权威检查，包括
接管之前排队的请求；未完成的事务或存储失败从不产生成功的接管确认。

消息使用有界的 MessagePack（16 KiB，深度 16），guardian 线上协议版本 3，每次交换有五秒的 I/O 截止时间，各通道的 worker 分别有
界。guardian 每 50 毫秒对账一次生命周期状态，与宿主连接无关。

## Run

`Ready` 并绑定之后，宿主使用 `GuardianRuns::execute`：

- **Start** 要求一个从未用过的 RunId、精确的 `RunSpec` 和显式的宿主断开策略（`KeepRunning`）。guardian 在 exec 之前提交 spec
  和一个不确定的初始快照。相同的 id 和 spec 返回原来的尝试；spec 改变则冲突；即使证明了 exec 失败，也不能在这个 id 下重试。
  环境变量值原样保存在私有日志中。
- **Query** 返回日志中的快照，把直接退出和尽力而为的清理分开。丢失的响应不是没有被接受的证据。
- **Stop** 强制清理一个 Run；**Close** 先持久地封住 Scope，再强制清理。两者都可以安全地重复，关闭之后仍可以查询。送达信号不是
  终止结果。
- **Output** 通过 `io.sock` 读取独立的 stdout 或 stderr 前缀。捕获由每个 `RunSpec` 选择启用：每个流最多 1 MiB，每次读取最多
  4096 字节，每个 Scope 最多接受 64 次尝试。截断和 EOF 都是显式的，字节随 guardian 一起消失。

`RunSpec` 的存活策略默认是 `Independent`。`TerminateOnOwnerExit` 在 exec 之前固定观察到的 owner 进程，在那个 pidfd 退出时强制
清理 Run，与宿主连接无关；Node 使用它，让自己的死亡触发清理。owner 身份只是触发条件，从不是发信号的依据。

工作负载在单独的 session 中使用 [Linux 尽力而为适配器](linux-rootless.zh.md)。在被发现之前逃逸的后代进程可能存活。如果
guardian 被 kill，工作负载可能仍然活着：旧的数字 PID 从不成为发信号的依据，存储的 `Running` 事实变成历史，guardian 和 Run 都
不会被重新启动。

## 兼容性

线上协议版本 3 拒绝更旧的活跃对端。活着的旧 guardian 使用更早的 token 协议，需要与它兼容的管理版本：永远不要改写它们的日志、在它们
仍承担责任时替换它们的二进制，或重启旧的 Scope。运行中的工作负载不会跨版本接管。
