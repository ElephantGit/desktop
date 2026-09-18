# Node 最小闭环：clone 指定仓库与分支

[English](minimal-loop.md) | 中文

## 当前方向

2026-09-18 起，最小闭环从“已有 Main Workspace 上创建／删除 task worktree”调整为
**clone 指定仓库的指定分支**。沿用本机 Desktop–Controller–Node 分工：调用方指定仓库和分支，
Controller 协调，Node 在执行环境完成 clone，结果回到调用方可观察的状态。

此文记录方向，不是可运行 clone 接口说明。最小执行契约已获批，具体 wire 和存储设计仍待完成；本次不改代码。

## 已有基础与差距

- [独立 Node](runtime.zh.md) 已有启动恢复、停止和显式数据目录；Linux host／guardian 可执行受管 Git。
- [Worktree 持久执行](persistence/worktree-execution.zh.md)已实现，但这是保留能力，不是新的首版目标。
- 当前协议能力、终态结果及 Node 数据模型含 Worktree 专用约束。clone 需要按实际需求适配，
  不能直接套用已有 Main Workspace 前提或将 clone 伪装为 EnsureWorktree。
- Node 对外 IPC、Controller 持久协调及 Client 新入口尚未接通。已有测试不构成 clone 闭环验收。

保留稳定执行身份、派发前持久责任、进程恢复交接、结果可查询及持久接管后确认等可靠性原则。
信任体系和 Strong 继续延期；私有仓库访问使用 Node 可信部署提供的非交互凭据。
现有 Backend、Worktree 数据与文件布局保持不变。

## 已批准边界与后续设计

| 主题     | 已批准政策／剩余工作                                                      |
| -------- | ------------------------------------------------------------------------- |
| 输入     | HTTPS 与显式 SSH、部署凭据、明确分支；返回实际获取的 commit               |
| 本地资源 | Node 在注入根下分配独占目标，保留失败／未知残留                           |
| Git 范围 | 单分支完整历史并 checkout；禁用 hooks，不递归初始化 submodule 或下载 LFS  |
| 协调     | 重放原执行；具体消息、存储迁移、Controller 接管和 Client 入口仍需实现设计 |

已讨论接受的 Worktree 管理命令禁用 hooks 和清理未知时的仓库级门禁尚未实现；
不能直接把其讨论结论当作当前代码行为或完整 clone 故障模型。

## 规格入口

[clone 根决策](../../specs/decisions/node/repository/0-clone-selected-repository-branch.md)与
[最小执行契约](../../specs/decisions/node/repository/20260918-minimal-clone-execution-contract.md)已于 2026-09-18
获批（approved），确认范围、输入、目录、内容和恢复政策。
核心测试义务已登记，clone 实现证据仍为 Missing；批准不表示功能已实现。
本次不启动 clone，不迁移用户目录，不接通 IPC，也不切换 Backend 写入入口。
