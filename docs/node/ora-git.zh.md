# ora-git

[English](ora-git.md) | 中文

`ora-git`（`apps/ora-git`）是 Agent 在沙盒里从平台 Git Server 拉取和向它推送时使用的命令。它从不对网络运行 Git，也不持有
任何凭据：它把几个固定动词之一变成一行 JSON 请求，发到会话的 Unix socket 上，由 Node 用自己的 git-access 身份执行。
相关决策是
[从 Git Server 快照 clone 与 ora-git](https://github.com/ora-space/specs/blob/main/decisions/node/repository/20261016-clone-from-git-server-snapshot-and-ora-git.md)
（`approved`，端到端验收通过后改为 `implemented`）；
Git Server 允许各执行写什么，见
[快照 clone 输入与执行 ref 授权](https://github.com/ora-space/specs/blob/main/decisions/cloud/controller-integration/20261016-snapshot-clone-input-and-execution-ref-grants.md)。

## 部署

```json
"agent": { "deno_path": "/opt/ora/bin/deno", "ready_timeout_ms": 30000, "ora_git_directory": "/run/ora-git" }
```

- `agent.ora_git_directory` 启用 `ora-git`；没有它，或没有 `git_access`，会话就没有 socket。目录归 Node 所有（`0711`，
  其他用户可以穿过但不能列出），Node 启动时清空它。
- Node 镜像把可执行文件安装在 workload `PATH` 上的 `/usr/local/bin/ora-git`，见 [容器镜像](../operations/container-images.zh.md)。
  socket 客户端只支持 Unix；为其他平台构建时，`ora-git` 回答 `internal`。

## 会话与 socket

每个运行中的 Agent 会话在该目录中有自己的、随机命名的 socket，路径以 `ORA_GIT_SOCKET` 交给 Agent 插件和它启动的进程。
socket 无法打开的会话以 `agent_failed{ora_git_unavailable}` 结束。

- 请求属于它到达的 socket 所属的会话，而不是请求里声明的任何东西：请求不携带 execution、Workspace 或仓库。对端
  （`SO_PEERCRED`）必须是工作负载用户，否则回答 `forbidden`。
- Node 正在停止、已接受 `EndSession`（在会话处理它之前）或会话已结束后，请求以 `no_session` 失败。
- 每个会话同一时间只运行一个请求；并发的请求得到 `busy`。
- 会话离开运行时时，socket 立即拒绝新请求，但会等待 Git 已在运行的请求，所以会话被记为结束、交付可能开始之后，它请求的
  任何操作都不会再写它的 checkout 或 Git Server。随后删除 socket 文件。连接在得到回答之前关闭时，客户端报告 `no_session`。

请求以会话执行的 `Ora-Execution-Id` 访问 Git Server；Git Server 根据 Cloud 当前状态判定该执行能读写什么，并以它为准。
第一阶段只授权 IssueRun 的会话：WorkflowRun 的 Agent 会话中，凡是访问 Git Server 的动词都得到 `denied`。

## 动词

```text
ora-git info | list | fetch upstream | fetch workspace <workspaceId> [<branch>] | push [<branch>]
```

| 动词 | 作用 |
|---|---|
| `info` | 打印本 Workspace 的 id、分支和基础提交，以及拉取的 ref 落在哪里；不访问网络 |
| `list` | 列出 Binding 的上游分支和 Git Server 上所有 Workspace 分支，并按会话的 Workspace 目录（Cloud 在会话开始时冻结）标注各 Workspace 的类型、Issue、run、run 状态和标题 |
| `fetch upstream` | 把 Binding 的上游分支拉到 `refs/remotes/ora/upstream/` |
| `fetch workspace <id> [<branch>]` | 把某个 Workspace 的全部分支或其中一个拉到 `refs/remotes/ora/workspaces/<id>/` |
| `push [<branch>]` | 把本地分支（默认是 Workspace 自己的分支）推送到 `refs/heads/workspaces/<自己的 id>/<branch>` |

拉取只写 `refs/remotes/ora/**`；本地分支、HEAD、index、工作区文件和仓库配置都保持不变。命令行不接受 URL、refspec、选项
或强制参数，分支参数是普通分支名（不能是 `HEAD`，不能以 `-` 开头，不含 `:`、`+` 或 `*`，最多 200 字节）。进度行写到 stderr，
结果写到 stdout。

## 推送

推送只写本 Workspace 的同名分支。它声明的旧值（`--force-with-lease`）来自 Node 账本（`workspace_branch_heads`），在本
checkout 自己成功推送或拉取自己 Workspace 之后记录；`refs/remotes/ora/**` 这类 Agent 可写的 ref 从不提供这个值。

- 如果账本中的值在本地已知，而推送的提交不包含它，推送在发出之前就以 `not_fast_forward` 拒绝。
- lease 过期是 `conflict`，除非 Git Server 已经持有推送的提交（回答丢失的推送被重放），这种情况算成功。
- 更新被拒绝是 `denied`。

推送成功后，账本和 `refs/remotes/ora/workspaces/<自己的 id>/<branch>` 记录新值。

## 退出码

失败时在 stderr 打印 `ora-git: <code>: <hint>`；提示只提及 ref 和提交。

| 代码 | 退出码 | 含义 |
|---|---|---|
| `usage` | 2 | 命令行或请求格式错误 |
| `no_session` | 3 | socket 背后没有运行中的会话 |
| `forbidden` | 4 | 调用者不是工作负载用户 |
| `busy` | 5 | 会话的另一个请求正在运行 |
| `not_found` | 6 | 没有匹配的 ref 或本地分支 |
| `conflict` | 7 | Workspace 分支在本 checkout 上次看到之后移动过 |
| `not_fast_forward` | 8 | 推送的提交不包含该分支最后已知的值 |
| `denied` | 9 | Git Server 拒绝了请求或 Node 的 git-access 身份（401、403） |
| `unavailable` | 10 | 无法访问 Git Server |
| `internal` | 11 | Node 本地失败；原因记在 Node 日志中 |
