# Node 会话账本

[English](session-ledger.md) | 中文

`ora-node-db` 用 schema v8 持久化 Agent 会话输入、生命周期、命令和待发送事件。
`apps/ora-node/src/session/ledger.rs` 为 `SessionJournal` 实现现有的 `SessionLedger` 与
`CheckoutResolver` 接口，数据库不反向依赖 Node 应用。

`NodeDatabase::session_journal` 创建供 actor 使用的窄连接，共享原有 Node 独占租约。
克隆句柄通过互斥锁串行访问，不同句柄的写入通过 SQLite immediate 事务协调。
写入采用 FULL synchronous；即使受理连接关闭，最后一个 journal 仍持有租约。

- 受理保存完整、不可变的输入及两个身份键。受控受理同时保存 runtime permit，并检查未完成责任。
  启动重新检查 runtime 权限，只允许 Accepted 转为 Running，不能重新启动 Running 会话。
- Thread 追加要求 Running。事务分配序号、保存完整事件并推进 `last_sequence`，提交后才返回。
  精确 ACK 只删指定事件；重复 ACK 无副作用，未来序号被拒绝。全部事件删除也不会重置序号。
- 命令保留完整消息、唯一命令 ID 和受理顺序。相同重发不重新入队，内容变化被拒绝；查询返回全部排队命令。
  结算只能单向进行；重复相同结算无副作用，改变结算结果被拒绝。
- 结束 Accepted 或 Running 会话时，终态、最后一个事件及全部排队命令的丢弃在同一事务提交。
  后续追加失败，后续命令返回 `SessionEnded` 且不入队；事务失败不会留下部分更新。
- checkout 查询只返回成功完成的 clone 原先保存的目标路径。不存在、失败、未完成及非 clone 执行均无
  checkout；运行时适配器遇到存储错误也拒绝提供路径，不根据 ID 重建路径。

`recoverable_sessions` 返回待结算为 interrupted 的未终态输入，不用于自动恢复运行。
生产协议入口、启动时调用 `SessionHost::recover_interrupted`、256 事件窗口和安装器共享插件目录的组合
属于下一步接线。本次账本变更本身不会在生产协议中声明或启用 Agent 会话能力。

测试通过数据库 API 和运行时 trait 使用真实 SQLite：
`cargo test -p ora-node-db` 与 `cargo test -p ora-node --test session_ledger`。
覆盖迁移、回滚、精确 ACK、重启、并发分配与终态受理竞态、命令顺序、runtime 隔离、checkout 证据和共享租约寿命。
