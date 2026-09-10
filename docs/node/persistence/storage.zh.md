# Node 本地存储

[English](storage.md) | 中文

调用方注入 `NodeConfig.home_directory`。部署时赋值为 `~/.ora/node`，单元测试使用临时目录。
数据库固定为 `home_directory/ora-node.sqlite3`。library 不读取 `HOME`，不启动进程或 IPC 服务。

数据库打开期间持有独占 OS 文件锁。SQLite 使用默认 rollback journal 和 FULL 同步写入。
新库的 application ID 为 `0x4f52414e`，schema version 为 1。已有空文件、其他数据库、不支持的版本、
目录和损坏数据库均拒绝打开，不自动重建。重开保留 NodeId，每个 Node 运行实例生成新的
NodeIncarnationId；显式配置身份不匹配时初始化失败。

`ora-node-db` 管理四张表：`node_metadata`、`executions`、`resources`、`outbox`。
完整命令与解析后的目标分别存储；目标冻结规范路径绑定、授权根、任务路径、分支和 base commit。
operation／execution 唯一约束阻止身份改绑。Git 开始前预留 active 资源的 Workspace、路径和仓库内分支。
删除引用既有归属，完成后保留 tombstone。

带前置检查的转换保存 Accepted、Running、Unknown 和 Completed 证据。完成事务一起提交资源事实、
终态结果和原始事件。状态读取不确认事件。确认必须精确匹配 Node、operation、execution 和 sequence 1，
只删除投递记录；结果和执行去重在确认后仍保留。

`WriteGuard` 提供真实 SQLite 事务的故障注入点。`ora-node-db` 单元测试覆盖独占归属、文件保护、去重、
资源预留、事务回滚、重开和确认。测试入口：`cargo test -p ora-node-db -p ora-node`。
