# Agent 中继

[English](agent-relay.md) | 中文

在 cloud 形态下，Controller 在 Cloud 和 Workspace 的 Node 之间搬运 Agent 会话工作、会话命令、Revision 交付及其会话历史的上传授权。Cloud 拥有 IssueRun 状态和所有持久回执；Controller 只保留连接内的队列，从不创建 Cloud 状态的持久副本。它原样转交 Cloud 在每份输入中固定的 Git Server 仓库和 ref，从不持有、申请或转交任何 Git 访问凭据：Node 用自己的 git-access 身份访问 Git Server。SQLite 形态只做 clone。决策是 [Controller 中继 Agent 会话](https://github.com/ora-space/specs/blob/main/decisions/controller/session/0-controller-relays-agent-sessions.md)和[快照 clone 输入与执行 ref 授权](https://github.com/ora-space/specs/blob/main/decisions/cloud/controller-integration/20261016-snapshot-clone-input-and-execution-ref-grants.md)两份 ADR；Node 一侧见 [Node Agent 会话](../node/agent-sessions.zh.md)和 [Revision](../node/revisions.zh.md)。

## 会话

`ClaimWork` 的会话条目指明它的 Workspace 沙盒和 Node。会话输入中的 Workspace 目录（`workspace_directory`，由 [`ora-git list`](../node/ora-git.zh.md) 显示）原样送达 Node。Controller 等待该沙盒完成经过校验的握手并具备 `agent_session` 能力，然后在持有沙盒准入闸门的同时把精确的输入记录到 Cloud；重连会唤醒排队的工作。Node 的 operation id 从登记的记录中读回，而不是假定等于 IssueRun id。绑定了模型的启动还需要 Node 的 `model_proxy` 能力。

待处理的执行会被查询。`Unknown` 状态在每个连接上最多允许重传一次原始命令，使用新的执行许可和 `ControlledStartAgentSession`；缺失或已关闭的许可永远不能授权一次启动。判断沙盒是否空闲时，未完成的 Agent 执行算作工作。

## Thread 事件

每个执行有自己的有序接管 worker。每批最多 64 条记录，从第一条开始最多等 100 毫秒，编码后的载荷达到约 1 MiB 时刷出，以保持在 gRPC 限制之内。Cloud 必须确认整批（`TakeOverThreadEvents`），之后才向 Node 发送任何精确的事件 ACK。终止信封排在之前所有批次之后；`Completed` 的状态回复不能结算会话，因为它不带能证明之前记录已送达的序号。

确定不可用的写入在 250 毫秒后重试，不发 ACK。回复丢失的写入用 RPC 适配器有界的重试和同一个提交 id。冲突或耗尽的不确定写入会关闭连接且不发 ACK，重连后从 Node 的持久 outbox 重放；仅仅清空队列不会触发重放，还可能让满的窗口卡住。Thread worker、命令投递、对账和轮询都在传输帧泵之外运行，所以一个慢的执行从不阻塞其他执行或心跳。连接拆除会取消它的 worker。

## 命令

`ThreadCommandAvailable` 唤醒投递，周期性轮询作为兜底。命令必须匹配登记的运行和执行，以及当前的沙盒和运行时范围；只有一个运行的第一条未投递命令才有资格，而且 Node 必须先报告它知道这个执行。匹配的 `SessionCommandAccepted` 或 `SessionCommandRejected(session_ended)` 才允许 `RecordThreadCommandDelivered`；只是发送出去从不算数。重试保持命令 id 和载荷不变；重启后 Cloud 会再次提供未确认的队首，由 Node 去重。

## Revision 交付

`DeliverRevision` 条目走同样的认领、登记和派发路径。登记要求目标 Node 完成经过校验的握手并具备 `revision_delivery`；否则条目留在 Cloud 中排队。登记的输入会补上目标 Node（Cloud 的 spec 不带它）重新构建，Git Server 仓库、`delivery_ref` 和 `workspace_ref` 保持不变，并且只以 `ControlledDeliverRevision` 在新的许可下发送。交付有自己的待处理列表，所以它们从不进入 clone 的轮询，未完成的交付也会让 quiesce 不报告空闲。

在 `TakeOverNodeEvent` 之前，Controller 把终止的 `RevisionResult` 与派发的输入核对：`base_commit`、`revision_ref` 和 `history.key` 必须一致，未改变的最终 commit 必须是基础 commit 或输入中上一个 Revision 的最终 commit，已交付的则两者都不能是。核对不通过的结果既不接管也不确认。交付 ref 是否指向最终 commit 只由 Cloud 经 Git Server 的 ref 查询核验；Controller 从不访问 Git Server。结果只在 Cloud 提交之后才确认；Cloud 在这次接管中校验声明的会话历史对象。`Completed` 的状态回复不结算任何东西，因为 Cloud 只接受带 Node 序号回执的交付结果。Controller 从不自己写 `RevisionFailed`，也从不报告只能由 Cloud 记录的 `VERIFICATION_FAILED`。关闭、丢失租约和断线都让交付保持没有结果；新进程通过状态查询以及（对 `Unknown`）原始命令继续它。

### 上传授权

交付只上传它的会话历史（`history_key` 处的 JSONL）；提交都推送到 Git Server。只为这个 key、只用 Node 冻结的 SHA-256 摘要向 `GrantRevisionUpload` 请求授权，这样 Cloud 会同时签名 `x-amz-checksum-sha256` 和 `If-None-Match: *`。指明其他 key 的请求从不转发给 Cloud，也得不到授权，但不会让交付失败：

- Node 发送 `UploadGrantNeeded` 时，原样转发它的校验和映射。
- 新连接第一次看到交付处于 `Accepted` 或 `Running` 时，用该 Node 最后一次报告给本进程的摘要再次请求。
- 派发之后不立即请求，因为 Node 还没有冻结它的对象；重启后的 Controller 不知道任何摘要，会等 Node 来请求。

授权以 `UploadGrant` 转发，header 和过期时间原样保留，只针对请求过的 key。Cloud 不可用或丢失租约时保留请求稍后重试；`CONFLICT`（交付已有结果，或它的运行已经不再交付）会丢弃请求，不让任何东西失败。授权只在发往传输层的途中存在于内存中，从不持久化、记录日志或格式化进诊断信息。

## 恢复会话

带有 `prior_revision` 的会话输入会恢复该 Issue 最新的 Revision（[Revision 恢复契约](https://github.com/ora-space/specs/blob/main/decisions/cloud/controller-integration/20261010-revision-restore-contract.md)，交付方式按 [Git 原生交付 ref 与续作](https://github.com/ora-space/specs/blob/main/decisions/cloud/revision/20261017-git-native-delivery-refs-and-resume.md)）。它的上一个 Revision 映射进 Node 会话 spec 时带有 Git Server 仓库、交付 ref 和基础 commit（Node 协议拒绝的输入会被拒绝），映射进交付 spec 时只带身份和最终 commit（指明了仓库、交付 ref 或基础 commit 的交付输入会被拒绝）。Node 自己从 Git Server 拉取上一个最终 commit；没有下载授权。只有当 Node 的实时握手也声明了 `revision_restore` 时才登记这样的会话；否则它留在 Cloud 中排队，所以它从不会在无法恢复的 Node 的新 clone 上运行。
