# 仓库 clone

[English](repository-clone.md) | 中文

Node 把指定仓库的指定分支 clone 到它分配的目录中，并报告实际获取到的 commit。clone 是独立的执行族，有自己的记录；
它不需要已有的 Main Workspace。相关决策是
[clone 根决策](https://github.com/ora-space/specs/blob/main/decisions/node/repository/0-clone-selected-repository-branch.md)、
[最小 clone 执行契约](https://github.com/ora-space/specs/blob/main/decisions/node/repository/20260918-minimal-clone-execution-contract.md)
和[被终止的尝试以 interrupted 失败](https://github.com/ora-space/specs/blob/main/decisions/node/repository/20260925-terminated-clone-attempt-fails-as-interrupted.md)。

## 部署

```json
{
  "clone": {
    "repository_root": "/home/node/repositories",
    "git_config": "/home/node/deployment/clone.gitconfig",
    "search_path": ["/usr/bin"],
    "ssh": { "kind": "disabled" }
  }
}
```

- 启动前准备好根目录和配置；Node 从不创建它们，也不修改它们的权限。每个路径都是绝对路径，属于当前用户或 root，
  不含符号链接组件；release 构建拒绝组或其他用户可写的路径。根目录不能与 Node 或进程状态、已配置的 worktree
  checkout 重叠。
- 文件系统必须报告创建时间：设备、inode 和创建时间共同标识根目录和每个新创建的叶子目录。缺失或被替换的归属证据
  保持 `Unknown`；只有数据库里的占用从不意味着拥有一个目录。这是 rootless 下针对合作用户的保护，不能隔离以同一
  用户运行的恶意代码。
- `git_config` 可以选择 CA 证书和非交互的部署凭据助手。只有配置路径会被复制到 Run 的环境中；不继承外部的 `HOME`，
  clone 的输出被丢弃，所以认证诊断信息从不进入日志。Cloud 部署在这里只允许非机密的 CA 信任，见
  [运行时控制](../controller/runtime-control.zh.md)。
- 默认关闭 SSH，除非配置为
  `{ "kind": "configured", "program": "/usr/bin/ssh", "config": "/home/node/deployment/ssh_config" }`。
  该文件选择身份和 known hosts；Node 强制使用 batch 模式和严格的主机密钥检查，所以未知主机必须由部署预先提供。
- `process.command_timeout_ms` 限制本地 Git 命令；网络 clone、fetch 和 push 使用 `process.network_timeout_ms`
  （默认 600000，允许 1..1200000）。网络时间预算耗尽后，仍要先关闭原来的 Scope，并拿到宿主和 guardian 的清理证据，
  才能报告 interrupted。重试策略属于 Cloud，由它限制 Workspace 的 clone 尝试次数；Node 从不自己发明重试。

嵌入方使用 `configure_clone`、`submit_clone` 和 `recover_clones`；可执行文件在启动时恢复已受理的 clone，只从控制会话
接收新的 clone。

## 执行

每个受理的请求拥有一个新的、以排他方式创建的目录，以及最多一个持久的变更 Run。Git 执行非本地、完整历史、单分支的
clone，不用模板，不递归 submodule，不使用 LFS smudge 或 process 过滤器；每条命令都禁用 hook 和交互提示。同名的 tag
不能满足请求的分支。校验要求得到一个正常的、非 shallow 的 checkout，对象独立，来源和分支与请求一致，HEAD 等于获取到的
分支 commit，且已跟踪的文件是干净的。

每次尝试结束时（Git 退出、停止宽限或截止时间之后的清理，或重启后的恢复），都先关闭原来的受管 Scope，再按宿主看到的情况
分类：

- 有退出码且清理已完成：继续校验；
- 被信号终止且清理已完成：以 `interrupted` 失败并保留目录，因为 Git 从未得出结论；
- 缺少退出或清理事实：保持 `Unknown`。

正常停止会在退出前保存 interrupted 结果，被 kill 之后的恢复会得到同样的结果。缺少进程证据从不成为再启动一个 Run 的
理由。没有 Run 的占用只有在它持久化的阶段能证明从未派发时才会继续。已知的失败保留它的目录；重试需要新的 operation 和
execution 身份，并得到一个不同的目录。终止结果的重放总是使用原来的持久结果和事件，无论离线还是用户编辑过，都不重新检查，
也不重新 clone。

## 记录

clone 记录保存在 Node 数据库中（schema v3 及以后）：

- `accept_clone` 原子地保存原始命令、生成的仓库身份、根目录和路径，以及初始的占用；它从不创建目录。以相同输入重放返回
  原来的目标；输入改变或跨执行族复用都会失败。目标位置不能与任何活跃的 worktree 目标或保留的 clone 目标重叠，失败也从不
  释放它的占用。
- 进度记录 `Reserved`、`DirectoryCreated` 或 `Dispatched`。目录身份在创建后冻结，`Unknown` 保持它的阶段。进程日志只有
  在有了持久的目录创建证据之后才启动受管 Run。
- 终止结果和它的事件在一个事务中提交，并且必须与原始输入、资源和路径一致。一次已派发的尝试需要恰好一个观察到的结束，
  且没有未清理的 Run：退出码（`process_outcomes`）支持成功或 Git 失败，信号终止（`process_terminations`）只支持
  `interrupted`，成功要求退出码为零。
- 查询从不确认；`pending_events` 覆盖所有执行族，确认只删除投递记录，结果、输入和占用都会保留。
