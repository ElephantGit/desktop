# 定时任务

[English](scheduled-tasks.md) | 中文

定时任务是一个持久化、带 revision 的定义，按计划对某个项目运行一段 prompt：每日汇报、周期性的代码质量检查、待办整理。相关决策是 [local 与 cloud 执行归属](https://github.com/ora-space/specs/blob/main/decisions/desktop/core/scheduled-task/0-local-and-cloud-execution-ownership.md) ADR（`approved`）。代码在 `crates/domain/src/scheduled_task.rs` 和 `scheduled_run.rs`、`crates/application/src/scheduled_task`、`crates/backend/src/scheduled_task.rs`，以及 `scheduled_tasks`、`scheduled_runs` 和 `scheduled_occurrences` 表（迁移 `0015`）。

## 已交付的部分

定义用例和历史读取已接入 Desktop。触发点准入（`AdmitScheduledOccurrenceHandler`）和离线缺口记录（`RecordOfflineMissedOccurrenceHandler`）已在 `ora-application` 中实现，但还没有组装进 Backend，所以 Desktop 目前不会触发定时运行。创建或更新 `cloud` 任务会作为无效请求被拒绝。

| 操作 | 效果 |
|---|---|
| `createScheduledTask` | 校验并持久化一个新的 `local` 任务 |
| `getScheduledTask`、`listScheduledTasks` | 读取可见的定义 |
| `updateScheduledTask` | 替换可编辑字段，以被替换的精确快照为条件 |
| `deleteScheduledTask` | 软删除定义；运行和错过的历史保留 |
| `revokeScheduledTaskApproval` | 撤回无人值守的授权并停用任务 |
| `listScheduledTaskHistory` | 按最新在前列出运行和错过的触发点 |

## 定义

`ScheduledTask` 包含名称、`enabled` 标志、计划、执行位置、目标项目、Agent（`AgentRef`）及可选的模型、prompt、revision 和可选的授权。

- **计划**：`EveryHours { interval }`（至少 1）、`Daily { hour, minute }` 或 `Weekly { weekdays, hour, minute }`（至少一个工作日），都按本地墙钟时间。
- **执行位置**：`local` 或 `cloud`，创建时确定；要换位置，复制任务并停用旧的。
- **目标**：启用的任务所在项目需要一个可接纳的本地 Main Workspace，并且是 Git 仓库，检查方式与交互式创建任务相同。校验失败是无效请求，并发修改返回 `scheduled_task_conflict`。
- **授权**：无人值守的本地执行需要对唯一支持的策略 `full_auto_in_worktree` 的显式同意，并记录同意时间。只有带着这份同意，任务才能被启用；撤回同意会停用任务。
- **Revision**：每次改变冻结输入的更新都会递增 `revision`，所以运行总能指明它执行的是哪个定义。

创建和更新先校验并冻结定义（`prepare`），再单独持久化，这样适配层可以在提交之前检查实时的运行时资格（例如 Agent 是否可用）。

## 运行与历史

领域层已经确定了调度器将遵循的语义：

- 每个计划的触发是一个 `ScheduledOccurrence`，由任务和计划时间标识，与产生它的 revision 无关。准入它是幂等的，并冻结一个 `ScheduledRunSnapshot`（revision、项目、Agent、模型、prompt、授权）。
- 实时观测最多可以迟到十分钟。更晚的观测，或者 Desktop 没有运行期间经过的时间，记为错过的触发点（`late_trigger`、`app_not_running`），而不是失败的运行。长时间关机之后，每个任务只记录最近的一个到期时间；启动时十分钟窗口仍未关闭的时间留给实时调度器处理。
- 运行只向前推进：`accepted`、`running`、`succeeded`、`failed` 或 `cancelled`。因为任务仍在运行或已有四个本地运行处于活跃状态而无法开始的触发点，变为 `skipped_busy` 或 `skipped_capacity`。失败原因有 `target_unavailable`、`model_unavailable`、`worktree_failed`、`approval_policy_invalid`、`timeout` 和 `execution_error`。
- 每个任务的历史最多保留 100 条运行和错过的条目，删除任务不会删除它们。

## 旧版本遗留的 worktree 任务

在 [worktree 原子归属提交](https://github.com/ora-space/specs/blob/main/decisions/desktop/core/scheduled-task/20261020-worktree-ownership-atomic-commit.md) ADR（`approved`）之前，创建与运行启动之间崩溃可能留下一个隔离的 worktree 任务，任何定时清理路径都无法回收它。这类任务会以普通任务的身份出现在任务列表里，且永远不会被自动删除。要手动清理：

1. 找一个标题形如 `Scheduled run <UUID>`、历史里没有任何关联定时运行的任务。
2. 像删除普通任务一样从任务列表删除它；常规的任务级联会一并移除它的 worktree 和分支。

标题只是识别线索，不是归属证明，所以不确定来源时不要仅凭这个规律删除任务。新的运行不再产生这类遗留；如果这类任务在 UI 里看不到或删不掉，请另开 issue 报告。
