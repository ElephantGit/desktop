# 流与取消

[English](streams.md) | 中文

会话的 load 和 prompt、历史回放、Workspace 监听和 `watchAppEvents` 都是流。本文说明 Desktop 如何通过 Tauri 运行它们，以及一次取消实际确认了什么。

## 流的生命周期

流使用共享的 `stream_contract` 命令，通过 Tauri Channel 按顺序转发 `data`、`error` 和 `end` 帧。私有的 call id 让 `AbortSignal` 只取消这一条流，另一个 request id 则为日志关联整条流。

流注册表（`apps/desktop/src-tauri/src/stream_registry.rs`）在领域启动之前认领 call id，并保留到拥有它的注册被丢弃为止；取消是通知所有者，而不是释放这个 id。已经在进行的启动会被允许完成，因为放弃任意的领域工作可能让 actor 的副作用失去归属；取消之后才创建出来的资源会立即丢弃。前端即使在启动尚未完成时也会发出取消，并在启动完成后再做一次清理。应用退出时取消所有正在启动或运行的注册，并拒绝新的注册。领域模块只提供事件源，从不重复实现转发或请求完成。

## 取消的三个阶段

1. **已请求**：Desktop 取消注册 token，或者 backend 流发送一条针对该操作的 actor 命令。此时什么都没有确认。
2. **已触发**：启动完成或转发退出，资源所有者开始清理。`Drop` 仍然是尽力而为，不提供任何确认。
3. **已确认**：操作所有者确认已停止，事件发布方释放了资源，附带的工作流恢复也成功返回。只有这时 `cancel_contract_stream` 才返回成功。

Backend 调用方使用 `SessionEventStream::cancel_and_wait(&mut self)`。取消只发起一次，重复调用返回相同结果；丢弃正在等待的 future 不会中止清理。调用方用 `tokio::time::timeout` 施加自己的期限。

## 等待确认

`cancel_contract_stream` 最多等待 30 秒。超时只结束这一个等待者，从不影响启动、清理或其他等待者；清理进行期间注册仍归属原所有者，并发的等待者看到同一个注册，之后的调用仍然可以取回最终回执。已完成的回执在注册表 256 条的修剪阈值内缓存，活跃的注册从不被修剪。未知或已被淘汰的 id 会报错，而不是推断为成功；新的操作要使用新的 call id。

在启动期间，如果在源被轮询之前就取消，不会创建任何资源。一旦开始创建，Desktop 会等它完成，再等待由此产生的流的清理。启动和清理的错误对启动命令和取消等待者都会保留。运行中的流会把终止帧和生命周期完成推迟到清理结束。

原生文件系统 watcher 在启动和转发两个阶段都会 await `ora_fs::WorkspaceWatcher::close()`，它在原生句柄和回调捕获都释放之后，等待原生事件处理器的退役回执。watcher 的 `Drop` 和宿主 worker 退出都不确认任何事。这个回执依赖精确锁定的 `notify` 8.2.0 的析构顺序；平台审计记录在 `crates/fs/src/watch/retirement.rs`，升级 `notify` 时必须重做。

## 谁确认什么

actor 的确认属于某一代操作。prompt 需要一个终止的 provider 响应；取消宽限期过期或 provider 失败，即使在本地隔离之后也仍然是未确认，之后一轮成功也不能证明之前失败的操作。actor 最多保留 256 条操作回执，证据缺失时保守地判为失败。

- 取消一次 load 会分离并 join 它的中继，但不会取消它所跟随的 prompt。
- 历史回放和应用事件 worker 会确认自己的任务已结束。
- 工作流清理会 await `end_human_turn`，包括它的持久化结果。
- 共享的 actor、provider 连接、其他流和用户文件仍归各自的所有者。

## 错误

原始的业务失败在终止帧和请求生命周期中优先：例如即使清理同时返回了 `agent_runtime_unavailable`，`session_history_degraded` 也保持不变，取消回执另外报告清理错误。只有在没有业务失败时，清理错误才会成为终止错误。

清理失败、缺少所有者回执、清理 worker panic 和等待超时都会以现有的契约形状返回错误：确认失败使用 `internal_error`，actor 不可用使用 `agent_runtime_unavailable`，恢复错误保留自己的 code。诊断信息会指明是恢复失败、证据缺失或过期，还是 `stream cleanup wait timed out; cleanup remains active`。错误从不证明清理已经完成；中止信号或被放弃的迭代器也不是清理回执。
