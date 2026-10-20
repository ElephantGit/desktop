# Node

[English](node.md) | 中文

`ora-node`（`apps/ora-node`）是在执行环境中运行工作的 Linux 可执行文件：在 Cloud 模式下，每个
Workspace 沙盒中运行一个 Node。它拥有一个显式配置的 Node 数据库，执行 clone、插件安装、Agent 会话和
Revision 交付，在重启后恢复未完成的执行，并通过[进程宿主和 guardian](../process/process-runtime.zh.md)
运行每一条 Git 命令。它只通过[控制会话](control-session.zh.md)接受来自自己 Controller 的命令；配置文件和
stdin 都不是命令通道。

## 部署

用 `cargo build -p ora-node -p ora-process-host -p ora-process-guardian` 构建，启动
[进程宿主](../process/host.zh.md)，然后运行 `ora-node /absolute/path/node-config.json`。最小配置：

```json
{
  "node": { "home_directory": "/home/alice/.ora/node", "identity": "Discover", "repositories": [] },
  "process": {
    "host_directory": "/home/alice/.ora/process",
    "expected_uid": 1000,
    "git_program": "/usr/bin/git",
    "environment": { "PATH": "/usr/bin:/bin", "HOME": "/home/alice" },
    "command_timeout_ms": 30000,
    "cleanup_timeout_ms": 5000,
    "shutdown_grace_ms": 2000
  },
  "timezone": "Asia/Shanghai",
  "recovery_interval_ms": 1000
}
```

更多的配置段用来启用能力：`clone`（[仓库 clone](repository-clone.zh.md)）、`control`
（[控制会话](control-session.zh.md)）、`agent`（[Agent 会话](agent-sessions.zh.md)）、`plugins`
（[插件](plugins.zh.md)）和 `git_access`（[Revision](revisions.zh.md#git-server-访问材料)），
clone、Revision 交付和 [ora-git](ora-git.zh.md) 都需要后者；Cloud 模式的 Node 没有它时拒绝启动。
`process.workload_uid` 让 Git 工作负载和 Agent 以单独的用户运行。

- `"Discover"` 保留已有的 NodeId 或生成第一个；`{ "Require": "id" }` 强制使用一个已登记的身份。每次启动都
  生成新的 NodeIncarnationId。
- 两个状态目录都是绝对路径并由外部注入，从不从 `HOME` 推导（`environment` 中的 `HOME` 只用于配置 Git）。
  新目录是私有的；release 构建拒绝已存在但不私有的目录，debug 构建跳过权限位检查。两者都拒绝错误的 owner、
  穿过受信路径的符号链接，以及与宿主状态互为别名的路径。
- `node.repositories` 保存显式的 `RepositoryBinding`（仓库引用、Main Workspace 身份和路径、授权根目录、
  worktree 根目录），供 [worktree 执行](worktree-execution.zh.md)使用。不会 clone 任何东西，也不从 cwd 推断。

## 执行与恢复

所有 Git 运行都通过宿主和 guardian 使用配置的可执行文件和环境。只读的预检共享有界的读取 Scope。每次变更之前，
Node 先在自己的数据库中提交它的执行、宿主、Scope 和 Run 关联，宿主和 guardian 也各自保存自己的意图和事实；
丢失的回复从不授权一次新的尝试。输出有界且不持久化，被截断、失败或缺失的输出从不被当作完整的 Git 事实解析。

恢复时，Node 先封住每个未完成的变更 Scope，等观察到它关闭之后，才检查或修复它的资源。guardian 不可用或清理
未经证实时，该执行保持 `Unknown`，保留它的占用，而不冲突的工作仍可继续。

guardian 在 exec 之前固定发起请求的 Node 的进程身份，所以即使宿主断开，Node 退出也会触发 Run 的清理。这是
rootless 下的尽力而为，不是强隔离：逃逸的后代进程或死掉的 guardian 都可能让清理无法得到证实，owner 存活与否
只用来触发清理。

`SIGTERM` 和 `SIGINT` 关闭准入，给正在运行的 Git 命令留出宽限时间，然后请求清理。关闭过程核实待处理的 Scope，
报告失败而不是声称完成了未经证实的清理；业务对账可以留到下一次启动。

## 存储

数据库固定是 `home_directory/ora-node.sqlite3`。Node 拥有业务执行、资源和事件记录；宿主和 guardian 拥有
各自独立的日志。

- 打开时在同级的 `ora-node.sqlite3.lock` 上持有排他的操作系统租约，持续到数据库关闭，而不是锁数据库文件本身，
  否则在 macOS 和 Windows 上 SQLite 自己的锁会与整文件租约冲突。租约排除另一个 Node owner，但不证明较早的
  Git 进程已经停止。
- SQLite 使用默认的回滚日志和 `synchronous=FULL`。数据库的 application id 为 `0x4f52414e`。打开时先检查身份、
  完整性、精确的表和索引定义以及外键；空文件、其他数据库、不支持的版本、目录和损坏的数据库都会被拒绝，从不重建。
- 旧的精确 schema 在事务中迁移，保留执行、结果和待处理事件；旧的二进制拒绝更新的版本，而不是去读它。

| 版本 | 新增 |
|---|---|
| 1 | `node_metadata`、`executions`、`resources`、`outbox`（Worktree 执行） |
| 2 | `managed_executions`、`process_attempts`：进程关联 |
| 3 | `execution_identities`、`clone_executions`、`clone_outbox`、`process_outcomes` |
| 4 | `controller_binding`、`execution_controllers`：与部署绑定的 ControllerId |
| 5 | `process_terminations`：在确认清理之后被信号结束的 Run |
| 6 | 运行时绑定、执行状态和执行许可 |
| 7 | `plugin_executions`、`plugin_outbox` |
| 8 | `node_executions`、`execution_events`、`session_commands`：Agent 会话 |
| 9 | `revision_deliveries`、`revision_outbox` |
| 10 | `workspace_branch_heads`：`ora-git` 推送时声明的自己 Workspace 分支的值 |
| 11 | `session_restores`、`restore_process_attempts`：恢复阶段、原进程身份与停止确认时钟 |

所有执行族共享 `execution_identities`，防止不同族之间的 key 冲突。完成时，资源事实、终止结果和它的原始事件
一起提交；状态读取从不确认。确认必须匹配 Node、operation、execution 和序号，并且只删除投递记录，结果和去重
信息保留下来。迁移从不为旧的直接 Git 执行编造进程证据，所以这样的进行中执行会保持阻塞。存储的 `RunSpec`
中的环境变量值是私有数据，从不进入日志。

`WriteGuard` 为真实 SQLite 故障测试暴露事务失败点；`cargo test -p ora-node-db -p ora-node` 会运行它们。

恢复的 Git 准入阶段与原命令意图在同一事务记录。停止确认前不能提交会话终态；上限耗尽只保留命名残留，不删除原身份。升级前缺少关联的恢复保持 `legacy_unknown`：旧终态继续保留，但不会因此解除 checkout 和运行时准入保护。见 [Revision 恢复](revisions.zh.md)。
