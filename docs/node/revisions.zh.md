# Revision

[English](revisions.md) | 中文

Agent 会话结束时，Node 把它的 checkout 和历史保存为一个 **Revision**：把最终提交推送到平台 Git Server 上的交付 ref，
对象存储只经 Cloud 签发的授权上传定稿的会话 JSONL。同一个 Issue 之后的运行在 Agent 启动之前从 Git Server 恢复这个
Revision。不生成、不上传也不下载 bundle。相关决策是
[内部快照根决策](https://github.com/ora-space/specs/blob/main/decisions/node/revision/0-internal-snapshot-and-incremental-bundle-upload.md)
（其中的快照提交仍然适用），以及
[推送交付 ref 并从 Git Server 恢复](https://github.com/ora-space/specs/blob/main/decisions/node/revision/20261017-push-delivery-ref-and-restore-from-git-server.md)
（`approved`，端到端验收通过后改为 `implemented`），
后者取代了根决策的 bundle 上传和
[会话之前恢复上一个 Revision](https://github.com/ora-space/specs/blob/main/decisions/node/revision/20261010-restore-prior-revision-before-session.md)
的 bundle 恢复。Controller 一侧见 [Agent 中继](../controller/agent-relay.zh.md)，授权契约见
[Cloud 契约](../controller/cloud-contract.zh.md#revision-上传校验和)。

Node 同时有 `clone` 和 `git_access` 配置段时声明 `revision_delivery`，因为交付在 clone 的 checkout 中使用 clone 加固过的
Git 环境运行 Git，并以 git-access 身份推送；当且仅当它同时声明 `agent_session` 和 `revision_delivery` 时声明
`revision_restore`。

## 受理

- `controlled_deliver_revision` 是生产入口：它的运行时许可必须与本 Node、incarnation、operation 和 execution 匹配，在
  受理时检查。裸的 `deliver_revision` 只在从未启用运行时控制的 home 中、经私有 IPC 接受。
- 输入指明已结束的会话、它的 clone 执行、基础提交、`refs/ora/revisions/` 下的本地 Revision ref、Git Server 的
  `git_repository_url`、`refs/ora/deliveries/` 下的 `delivery_ref`、`workspace_ref` 和会话历史的对象键。Workspace ref
  必须等于 clone 的 Workspace ref。
- 输入在任何工作之前持久化，交付记为 `Running`。重复相同输入只报告状态；同一身份下的不同输入是协议错误。
- 交付存放在 `revision_deliveries`，唯一的终态事件在 `revision_outbox`，与会话分开，因此会话恢复永远看不到它们。

## 准备

准备只做一次：

1. 会话必须已有终态结果且没有存活的 actor（`failed{session_not_settled}`），它的 checkout 必须仍在交付的基础提交上
   （`failed{checkout_unavailable}`），它的 JSONL 必须存在（`failed{history_unavailable}`）。
2. 定稿的 JSONL 复制到 `<home>/revision-deliveries/<sha256(execution)>/history.jsonl`。
3. 以工作负载用户身份，在 checkout 的 Git 目录中用临时 index 给工作区做快照（`read-tree HEAD`、`add -A`、`write-tree`）。
   如果树等于 `HEAD^{tree}`，最终提交就是 `HEAD`；否则 `commit-tree` 追加一个提交，作者是会话的 Git 身份，提交者是
   `Ora <revision@ora.invalid>`。hook、fsmonitor 和签名都被禁用，真正的 index、工作区文件和分支都不受影响。
   `update-ref` 让本地 Revision ref 指向最终提交。
4. 最终提交等于基础提交，或者（续接的会话）等于上一个 Revision 的最终提交时，声明为 `unchanged`；否则为 `delivered`。
   两种情况都会推送自己的交付 ref。
5. 在第一次网络写入之前，最终提交、会话历史的大小和 SHA-256 以及声明作为冻结计划写入账本。在此之前重启会重新准备；
   之后重启则复用同一份计划。

## 推送与上传

使用 Node git-access 身份的 Git 从不在 checkout 中运行。每次推送使用 `<repository_root>/.ora-transport/` 下的私有传输仓库，
它只借用 checkout 的对象库（拒绝嵌套的 alternates 和共享目录，并禁用 alternate ref 发现）。证书、私钥、CA 和交付执行的
`Ora-Execution-Id` 请求头只交给每条 HTTPS 命令，所以 checkout 的配置无法重定向它，也无法装入 credential helper。

1. 把冻结的提交推送到 `delivery_ref`。新建它，或发现它已指向该提交，都算成功；更新被拒绝是 `failed{push_rejected}`，
   网络或 5xx 失败重试用尽是 `failed{push_failed}`。
2. 尝试把 `workspace_ref` 快进到同一提交，声明 Node 账本上次看到的值。重放算成功；旧值过期、被拒绝或重试用尽只记日志，
   从不让交付失败。
3. 上传会话历史：没有有效授权时（尚未收到、5 秒内过期、被 403 拒绝或因重启丢失），Node 发送带会话历史冻结 SHA-256 的
   `upload_grant_needed`，每 2 秒重复一次，每次新连接时立即再发，交付保持 `Running`。PUT 以流式发送冻结的文件，原样带上
   授权的每个请求头（包括 `If-None-Match: *` 和 `x-amz-checksum-sha256`），从不跟随重定向。2xx 和 412 都算已上传；失败三次
   以 `failed{upload_failed}` 结束交付。授权只存在于内存，从不写日志。

Git Server 的暂时性失败最多尝试三次，间隔 5 秒和 30 秒；401、403 和 429 以外的其他 4xx，以及被拒绝的 ref 更新不重试，以 `push_rejected` 失败。每次推送尝试最长 30 分钟，
与 `network_timeout_ms` 无关，并在一分钟内低于 1 KiB/s 时中止。Cloud 在登记 Revision 之前，于事务外核验交付 ref 和会话历史。

终态 `revision_result`（序号 1）会重放直到被确认；它持久化后冻结的文件即被删除，残留在下次启动时清扫。关闭、Controller
断开和取消从不写入失败；重启后交付从冻结计划继续。

## 恢复上一个 Revision

带 `prior_revision`（其身份、最终提交与基础提交、`git_repository_url` 和 `delivery_ref`）的会话，在定位 checkout 并检查
插件之后、把 checkout 交给工作负载用户或启动任何插件之前恢复它。仓库必须是 checkout 当初 clone 所用的仓库；`git_repository_url`
是其他仓库时，会话以 `agent_failed{prior_revision_unavailable}` 结束，不访问 Git Server。

1. 以 Node 身份、使用会话执行的身份，把 `delivery_ref` 拉进一个借用 checkout 对象的私有传输仓库，只以 clone 的基础提交
   协商。拉到的提交必须等于上一个 Revision 的最终提交。
2. 以工作负载用户身份，把对象从传输仓库取入 checkout，并把 Workspace 的本地分支移到上一个 Revision 的最终提交。clone 的
   基础提交、仓库配置和 `refs/remotes/ora/**` 都不变。
3. 如果上一个 Revision 的基础提交不在当前快照的历史中，就在第一轮输入后追加一段固定的英文说明，写明恢复的提交、它的基础
   提交和分支，并建议运行 `ora-git fetch upstream`。

拉取和导入共享一个 5 分钟的截止时间，重试也计算在内。交付 ref 不存在、403 或 401 与 429 以外的其他 4xx 时，会话以
`agent_failed{prior_revision_base_unavailable}` 结束：前序 Revision 的 Git 内容永久取不到。401 只说明 Node 自己的
git-access 身份没有通过认证，所以会话以可由之后的运行再续的 `agent_failed{prior_revision_unavailable}` 结束；同一材料重试
不会通过，时限内也不重试。其他拉取、校验或检出失败，以及截止时间到期，同样以 `agent_failed{prior_revision_unavailable}` 结束。两种情况都不会启动插件进程。恢复期间 Node 停止或收到 `EndSession`，
恢复会在下一条 Git 命令之前停下并等待正在运行的那条，所以会话只在恢复不再写 checkout 之后才结束。恢复期间重启，会话
以 `interrupted` 结束。传输仓库用完即删，启动时也会清扫。

## Git Server 访问材料

启用平台 Git Server 的部署增加顶层 `git_access` 配置段，以绝对路径给出 `ca_cert`、`client_cert` 和 `client_key`，这是为
运行时的租户、Workspace 和代次签发的客户端证书，与 WSS 和模型访问的身份分开。入口脚本把这些文件放在只有 root 可访问的
`/run/ora` 中。Node 在启动时校验这些材料，不可用时拒绝启动；Cloud 模式（设置了 `control.target`）的 Node 没有这一段时以
`git_access_required` 拒绝启动。证书只承载身份：Git Server 根据 Cloud 当前状态，按请求头 `Ora-Execution-Id` 中的执行
判定每个请求能读写什么，服务器地址由执行输入给出。同一份材料用于 [clone](repository-clone.zh.md)、交付、恢复和
[ora-git](ora-git.zh.md)。

## 限制

- Agent 插件运行在宿主 Scope 之外，所以“进程 scope 已关闭”指会话结果已写入、会话运行时已停止其插件进程树。
- 交付的 Git Run 由宿主管理，但不作为业务变更记入日志；被中断的准备只是重新准备。
- 仓库配置仍可以选择由 `git add` 运行的 clean 或 smudge 过滤器，它们和 Agent 本身一样以工作负载身份运行。
- 在传输仓库中解析借用的 checkout 对象没有单独的资源限制。
- 旧版 Node 在 Node home 中留下的 `revision-restores/` 目录不会被删除。
