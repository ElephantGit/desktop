# Node Agent 会话执行（计划）

本 PR 落地
[agent-runtime 根决策](../../specs/decisions/node/agent-runtime/0-shared-agent-runtime-crate-hosted-by-node.md)
的第二步：Node 用 `ora-agent-runtime` 运行一个 Agent
会话执行，只启动该执行指定版本的 agent 插件，把每条定型 记录先写入会话
JSONL、再交给账本成为 Thread 事件，并按受理顺序执行会话命令。最终文档为
`docs/node/agent-session.md` 与
`docs/node/agent-session.zh.md`；`docs/agent-runtime*.md` 同步记录新增的
宿主回调。

## 范围

- 做：D7 中会话一侧的全部接口与实现；`ora-agent-runtime` 为 Node
  增加的三个能力；Node 的插件生命周期组合； echo agent
  测试夹具；以内存账本、内存 checkout 与内存插件目录驱动的会话测试。
- 不做：`ora-node-db` 的执行、事件、命令表与 `SessionLedger` 的 SQLite
  实现；协议消息接线（`StartAgentSession`、 会话命令、`ThreadEvent`
  的发送与重放）；`PluginCatalog` 的安装一侧实现；Node 镜像中的 Deno。它们属于
  [协议后续决策](../../specs/decisions/node/protocol/20260928-streamed-thread-events-and-session-commands.md)
  与 plugin-runtime 根决策的落地。

## 与已批准决策的差异

| 决策                                                                                | 差异                                                                                                                                                                                 | 原因                                                                                                                                                                         |
| ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| agent-runtime D2：插件进程与 Agent CLI 在执行的进程 scope 内，受 host/guardian 纳管 | 本步 Node 与 Desktop 一样用 `ora-process` 直接启动 Deno（进程组整树终止）；会话结束、Node 正常停止时整树清理。Node 崩溃后遗留进程的回收与 host/guardian 纳管留给新的 process I/O ADR | guardian 当前不提供 stdin 与插件协议流（[process/io 根决策](../../specs/decisions/node/process/io/0-bounded-output-and-fenced-input.md)），Deno 插件依赖 stdio 上的 JSON-RPC |
| agent-runtime D7：`next_command` 返回最早的排队命令                                 | 改为 `queued_commands` 按受理顺序返回全部排队命令                                                                                                                                    | 轮次进行中必须看见排在用户轮次之后的 `EndSession`，才能按协议 D4 立即取消当前轮次并丢弃排队轮次                                                                              |

两处差异在 ADR 正文中补充说明，状态保持 `approved`。

## `ora-agent-runtime` 的新增能力

1. **记录已定型回调**：`RuntimeEvents::records_settled(session_id, lines)`。会话记录器每次成功追加
   JSONL 后， 在同一个 actor 内、按文件顺序同步调用，参数是刚写入的完整
   `HistoryLine`。`ora-history` 的 `HistoryWriter::append`
   因此返回它写入的行。Desktop 实现为空操作。回调在 JSONL 之后调用，保证 Thread
   中的 记录都存在于 JSONL（不变量 4）。
2. **指定 Session ID**：`start_session_with_id`，Node 以会话执行 ID 作为 Ora
   Session ID（D3），会话 JSONL 路径因此可以只由执行 ID 推出。
3. **用户消息的 turn 标识**：`prompt_session` 的请求可以携带用户消息
   ID，记录器把它写入 `user_message_chunk` 的 ACP `messageId`。这满足 D3“JSONL
   的用户消息记录带 `turn_id`”，同时不改变 `ora-history` 的记录 schema；Desktop
   不传该值，JSONL 与现在逐字节一致。

## 插件生命周期的共享改动

- 生成代通知的无损 tap 从 Backend 的 `BroadcastNotificationSink` 抽到
  `ora-plugin-lifecycle`，Backend 与 Node 共用同一实现。
- `ChildProcessEnvironmentProvider`
  增加插件进程自身的环境变量，`DenoPluginRuntimeLauncher` 把它设置到 Deno
  进程上。Node 以此为插件进程及其派生的全部进程设置
  `GIT_AUTHOR_*`/`GIT_COMMITTER_*`（D4）：Deno 直接派生的 进程继承插件环境，经
  `ora/childprocess/spawn` 由宿主派生的进程由原有的子进程环境接口注入。Desktop
  不提供 该值，行为不变。

## Node 会话执行

模块位于 `apps/ora-node/src/session`，与 `service` 一样只在 Linux 构建。

### D7 接口

| trait              | 提供方     | 本 PR                              |
| ------------------ | ---------- | ---------------------------------- |
| `SessionLedger`    | 账本       | 定义；测试用内存实现               |
| `CheckoutResolver` | clone 账本 | 定义；测试用内存实现               |
| `PluginCatalog`    | 插件安装   | 定义（含使用租约）；测试用内存实现 |
| `SessionHost`      | 会话执行   | 定义并由 `AgentSessions` 实现      |

### 每个执行一套运行时

每个会话执行拥有自己的 `PluginLifecycle` 与 `AgentRuntimeManager`：插件根仍是
Node 数据目录下的 `plugins/`，但该生命周期只报告、只启动本执行的 agent 插件，Git
身份也只属于这个执行。这样“只启动指定插件”
与“身份只进入该执行的进程树”由组合方式保证，而不是由共享实例上的过滤保证。Node
的宿主适配：

| 接口                 | Node 实现                                                                                       |
| -------------------- | ----------------------------------------------------------------------------------------------- |
| `SessionStore`       | 内存会话行。Node 重启不恢复会话（协议 D6），会话行不需要跨进程保存                              |
| `AgentAttach`        | 本执行的 `PluginLifecycle`，只含指定插件，并确认发现的包目录就是 `PluginCatalog` 返回的版本目录 |
| `SessionSetup`       | 空 MCP 集合（D5）                                                                               |
| `RuntimeEvents`      | `records_settled` 转为 `append_thread_event`；其余事件丢弃                                      |
| `WorkspaceDirectory` | `CheckoutResolver` 解析出的 checkout                                                            |

### 执行流程

1. `start` 立即返回，后台任务解析 checkout；失败以
   `agent_failed{checkout_unavailable}` 结束。
2. 查询 `PluginCatalog::installed(id, version)`；不存在或发现的包不是该版本时以
   `agent_failed{agent_plugin_unavailable}`
   结束，不启动任何插件进程。随后取得租约，会话结束后释放。
3. 以执行 ID 创建会话，发送 `initial_turn`。
4. 每个轮次期间，定型记录经 `records_settled` 追加为 Thread 事件，`turn_id`
   取当前轮次。单条记录超过 256 KiB 时事件携带截断版本（保留
   `at`、`seq`、`type`）并标注 `truncated`，JSONL 保留原文。
5. `command_arrived` 唤醒会话读取 `queued_commands`：
   - `SubmitUserTurn` 在当前轮次结束后按受理顺序执行，执行时结算为 `executed`；
   - `EndSession` 取消当前轮次，把排在前面的用户轮次结算为
     `discarded`，停止会话与插件，结算自身为 `executed`，最后
     `end_session(reason)`。
6. 插件不可用或连接熔断（运行时报告 `Failing`）时以 `agent_failed`
   结束；轮次失败只记录 `TurnEnded`，会话保持。
7. 结束时依次：停止会话（支持时
   `session/close`）、停止插件整树、释放租约、写入终态。

### 恢复与交付

- `recover_interrupted(execution)`：没有存活的会话运行时，Node 不重建
  Agent；确认 JSONL 可读后返回 `interrupted`。JSONL 中已写入但没有成为 Thread
  事件的最后一条记录保留在 JSONL 中（D3）。
- `sealed_history(execution)`：会话仍在运行时返回
  `history_unavailable`；否则返回 `ora-history` 规定路径下的 JSONL。

## echo agent 夹具

`apps/ora-node` 增加 `ora-node-echo-agent` 测试二进制，与 E2E 的 `fake-agent`
一样顶替 `deno`：实现 agent 插件 契约与最小
ACP（`initialize`、`session/new`、`session/prompt`、`session/cancel`、`session/close`）。prompt
原样 回显；含 `[commit]` 时经 `ora/childprocess/spawn` 在 checkout 中执行
`git commit --allow-empty`；含 `[hold]` 时保持轮次直到取消。Node 镜像只复制
`ora-node`，不包含该夹具。

## 测试

| 场景       | 验证                                                                                                                   |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| 记录顺序   | 内存账本中每个 Thread 事件都能在 JSONL 找到，顺序一致，用户消息记录的 `messageId` 与 Thread 的 `turn_id` 都等于轮次 ID |
| 命令排队   | 轮次进行中到达的两个 `SubmitUserTurn` 在其后按受理顺序执行，重复唤醒不重复执行                                         |
| 结束       | 轮次进行中的 `EndSession` 取消轮次、丢弃排队轮次、写入 `user_ended` 终态，插件进程已退出，租约已释放                   |
| 中断       | JSONL 已写入、Thread 追加失败时，重开后 `recover_interrupted` 返回 `interrupted`，JSONL 比 Thread 多一条               |
| 插件选择   | 版本不符或未安装时以 `agent_plugin_unavailable` 结束且不启动插件进程                                                   |
| Git 身份   | echo agent 提交的 author/committer 为输入身份，checkout 的 `.git/config` 不变                                          |
| 超大记录   | 超过 256 KiB 的记录以截断版本进入 Thread，JSONL 保留原文                                                               |
| 运行时回调 | `ora-agent-runtime` 单元测试：`records_settled` 与 JSONL 行一致；指定 Session ID 与用户消息 ID 生效                    |

核心测试用例 `specs/test-cases/node/agent-runtime/session-execution.md`
在实现后更新证据状态。
