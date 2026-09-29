# Node Agent 会话执行

[English](agent-session.md) | 中文

Node 用 [`ora-agent-runtime`](../agent-runtime.zh.md#运行时-crate-与宿主) 运行
Agent 会话执行：在一次 clone 留下的 checkout 中只启动该执行指定版本的 agent
插件，把每条定型记录先写入会话 JSONL、再交给 Node 账本成为 Thread
事件，并按受理顺序执行会话命令。决策依据见
[agent-runtime 根决策](../../specs/decisions/node/agent-runtime/0-shared-agent-runtime-crate-hosted-by-node.md)
与[协议后续决策](../../specs/decisions/node/protocol/20260928-streamed-thread-events-and-session-commands.md)。

代码位于 `apps/ora-node/src/session`，只在 Linux
构建。账本表、协议消息接线与插件安装不在这里；它们只经下文的
接口与会话执行交互。

## 接口

| trait              | 提供方     | 会话执行用它做什么                                                                           |
| ------------------ | ---------- | -------------------------------------------------------------------------------------------- |
| `SessionLedger`    | Node 账本  | 追加 Thread 事件、写入终态、读取排队命令（按受理顺序的全部）、结算命令                       |
| `CheckoutResolver` | clone 账本 | 由 clone 执行 ID 解析 checkout；会话从不自行拼接路径                                         |
| `PluginCatalog`    | 插件安装   | 取得使用租约，查询指定版本的安装目录                                                         |
| `SessionHost`      | 会话执行   | 由 `AgentSessions` 实现：`start`、`command_arrived`、`recover_interrupted`、`sealed_history` |

`queued_commands`
返回全部排队命令而不是只有队首：轮次进行中必须能看见排在用户轮次之后的
`EndSession`，才能 立即取消当前轮次并丢弃排在它前面的轮次。

## 每个执行一套运行时

每个会话执行组合自己的插件生命周期与 agent 运行时。插件根仍是 Node 数据目录下的
`plugins/`，会话 history 位于同一目录下的
`sessions/`，但这个生命周期只报告、只启动本执行的 agent 插件，并以本执行的 Git
身份启动它。
“只启动指定插件”和“身份只进入该执行的进程树”因此由组合方式保证，而不是由共享实例上的过滤保证。

| 运行时接口           | Node 实现                                                                       |
| -------------------- | ------------------------------------------------------------------------------- |
| `SessionStore`       | `MemorySessionStore`：Node 重启不恢复会话，会话行不需要跨进程保存               |
| `AgentAttach`        | 本执行的插件生命周期，只含一个插件；不登记 Effect consumer（Node 不投影 Skill） |
| `SessionSetup`       | `NoSessionMcp`：MCP 的配置与密钥还没有下发路径                                  |
| `RuntimeEvents`      | 每一行 history 写成一个 Thread 事件；标题与模型目录事件丢弃                     |
| `WorkspaceDirectory` | checkout                                                                        |

Git 身份以 `GIT_AUTHOR_*`/`GIT_COMMITTER_*`
同时设置在插件进程上（插件直接派生的进程继承它）和宿主经
`ora/childprocess/spawn` 为插件派生的进程上（这些进程继承的是 Node
的环境）。Node 不写任何 Git 配置。

## 执行过程

1. `start` 立即返回；会话在后台解析 checkout，找不到时以
   `agent_failed{checkout_unavailable}` 结束。
2. 先取得插件租约，再查询指定版本。目录中没有该版本，或插件根里发现的包不在该版本目录、不是
   agent 时，以 `agent_failed{agent_plugin_unavailable}`
   结束，此时没有启动任何插件进程。租约持有到插件进程整树退出之后。
3. 等待 agent 连接就绪（有上限，超时或监管放弃为
   `agent_failed{agent_unavailable}`），以执行 ID 作为 Ora Session ID
   创建会话（失败为 `agent_failed{agent_start_failed}`），然后发送
   `initial_turn`。
4. 轮次进行中定型的每一行 history 都以当前轮次的 `turn_id` 成为 Thread
   事件；用户消息在 JSONL 中也以 ACP `messageId` 带着同一个标识。超过 256 KiB
   的记录在 Thread 中只保留 `at`、`seq`、`type` 并标注 `truncated`，JSONL
   保留原文。
5. `command_arrived` 唤醒会话读取队列。`SubmitUserTurn`
   在当前轮次结束后按受理顺序执行，开始执行时结算为
   `executed`；重复唤醒不会重复执行。`EndSession` 把排在它之前的用户轮次结算为
   `discarded`，停止会话（取消 进行中的轮次并记录 `TurnEnded{cancelled}`，支持时
   `session/close`），再结算自身为 `executed`。
6. 结束时：停止会话，释放运行时（连接监管随之停止重连），停止插件并等待整树退出，释放租约；把仍排队的命令结算
   为
   `discarded`，移除存活记录，最后写入终态。终态最后写入，所以交付看到会话结束时
   history 已不再被写入。

Agent 轮次失败或超时只记录 `TurnEnded`，会话保持。轮次无法被接纳（agent
无法连接）时以 `agent_failed{agent_unavailable}` 结束。

## 记录顺序与崩溃

运行时逐行写 history，每行写入文件后、写下一行之前同步调用
`record_settled`，Node 在其中完成 `append_thread_event`。因此 Thread
中的每条记录都在 JSONL 里且顺序一致；两次写入之间崩溃时，JSONL 最多比 Thread
多一行。

`append_thread_event` 一旦失败，镜像永久停止（之后的行不再进入 Thread，避免
Thread 出现无法解释的空洞），会话以 `agent_failed{thread_unavailable}` 结束。

Node 重启后，没有终态的会话执行由 `recover_interrupted` 以 `interrupted`
结束：history 唯一的写入者已随旧进程
结束，文件内容就是最终内容。`sealed_history` 在会话仍在本进程运行时返回
`history_unavailable`，否则返回 `ora-history` 规定路径下的 JSONL。

## 已知差距

插件进程与 Agent CLI 由 Node 像 Desktop
一样直接启动（进程组整树终止），会话结束与 Node 正常停止时整树清理；
它们还没有纳入 host/guardian 的执行进程 scope，因为 guardian
目前不提供插件所需的 stdin 与协议流。Node 崩溃时， 插件会因 stdio
关闭而退出，但忽略这一点的后代进程不会被回收，直到新的 process I/O
决策覆盖插件。

## 测试

`apps/ora-node/tests/agent_session.rs` 用内存账本、固定 checkout
与带租约计数的插件目录，通过 `SessionHost` 驱动真实的 echo agent
插件进程（`ora-node-echo-agent`，与 E2E 的 `fake-agent` 一样顶替
`deno`）。覆盖记录顺序
与轮次归属、命令排队、结束时的取消与丢弃、崩溃窗口与中断恢复、插件版本不符、Git
身份与超大记录。夹具只用于 测试；Node 镜像只复制 `ora-node`。

持久化适配见[会话账本](session-ledger.zh.md)。该实现已可供组合使用；生产协议入口和启动恢复接线仍属于后续步骤。
