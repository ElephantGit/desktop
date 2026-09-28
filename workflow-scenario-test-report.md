# 工作流功能用户级场景测试报告

- 测试对象:`fix/workflow-catalog-order-and-windows-tooling` 分支(含 JSON Start 变量类型修复)
- 测试时间:2026-09-28
- 测试方式:通过生产 Backend 接口(创建 → 发布 → 创建运行 → 填写启动输入 → 启动 → 轮询终态)驱动真实运行引擎,使用 fake OpenCode Agent 插件执行 Agent 节点(它把渲染后的完整提示词原样回显为输出,因此提示词模板同时充当数据流断言)
- 测试代码:`e2e/tests/desktop/core/session/tests/workflow_scenarios.rs`(15 个场景,约 9 秒跑完)
- 总体结果:**15/15 场景通过**。工作流的核心执行语义(调度、类型系统、数据流、复合节点)全部正确;发现的问题集中在**错误信息呈现**与**Agent 输出语义**两处,共 3 个缺陷 + 1 个设计确认项。

## 场景覆盖矩阵

| #   | 场景                    | 覆盖点                                                                                                                                                                         | 结果 |
| --- | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---- |
| 1   | 全数据类型 Start 输入   | string / number / boolean / select / array[string] / array[object] / object / any(含 json 控件声明数组类型,即本 PR 修复路径)渲染进 Agent 提示词;变量池携带类型化值             | ✅   |
| 2   | 运行输入类型校验        | number 收字符串、object 收数组、array[string] 收对象、未声明变量 → 全部拒绝;合法值通过并成功执行                                                                               | ✅   |
| 3   | 必填 Start 值缺失       | 不填必填值直接启动 → 启动被拒,零节点执行                                                                                                                                       | ✅   |
| 4   | 文件类型 Start 输入     | file / array[file] 以工作区相对引用渲染;绝对路径、`..` 穿越路径在输入边界被拒                                                                                                  | ✅   |
| 5   | 条件:字符串相等路由     | 命中分支执行、未命中走隐式 else;未激活分支零会话;输出节点绑定各自分支                                                                                                          | ✅   |
| 6   | 条件:数字/布尔/多分支   | greater_than / less_than / equals / is 组合 and、or 逻辑;首个人选分支优先;else 兜底                                                                                            | ✅   |
| 7   | 迭代:对象数组           | array[object] 逐轮绑定 item 为对象;嵌套路径 `{{#iter.item.note#}}` 渲染;区域内条件可比较 `iter.item.id`                                                                        | ✅   |
| 8   | 迭代:绕过收集目标       | continue 策略下条件分支绕过 collectSelector 的轮次计为失败:运行成功、failed_count=1、仅收集已执行轮次                                                                          | ✅   |
| 9   | 迭代:空数组             | 空源合法:零轮次、零会话、立即以空输出完成                                                                                                                                      | ✅   |
| 10  | 迭代:超上限             | 源长度 > maxIterations:启动边界即失败,零会话,错误含 "exceeding maxIterations" 与调整指引                                                                                       | ✅   |
| 11  | 循环:until 满足即退出   | 第 1 轮满足 until → 成功,1 会话,loopConfig.outputs 绑定暴露                                                                                                                    | ✅   |
| 12  | 循环:到达上限未终止     | until 永不满足 + maxIterations=2 → 失败,恰好 2 会话,错误含 "did not terminate within 2 rounds"                                                                                 | ✅   |
| 13  | 循环:类型化反馈跨轮携带 | loop 变量 draft 每轮回写 writer.output;第 2 轮提示词嵌入第 1 轮输出;until 用 contains 检测到携带值后退出                                                                       | ✅   |
| 14  | 聚合器                  | 互斥分支取第一个已赋值选择器;已赋值的 `false` 是值而非缺失(false 直通)                                                                                                         | ✅   |
| 15  | 组合流水线              | 门条件(mode equals + threshold ≥)→ 迭代区域(条件过滤 beta 轮,continue)→ 循环(反馈携带,until contains)→ 输出;对象/数组 Start 值跨容器流入循环体;skip 模式走 else 分支零会话完成 | ✅   |

## 发现的问题

### 问题 1(P1,错误信息):运行输入的类型校验失败报告为无细节的内部错误

**现象**:在运行启动页填写与声明类型不符的值(或拼写错误的变量名、不安全的文件路径)时,用户看到的错误一律是:

```
application operation failed
```

四种完全不同的错误——number 变量给了字符串、object 变量给了数组、array[string] 变量给了对象、变量名不存在(测试 2),以及绝对路径/`..` 穿越(测试 4)——全部显示同一句话。变量名、期望类型、实际值全部丢失,用户无从知道自己填错了什么。

**根因**:`crates/backend/src/error.rs:476` 附近,`ApplicationError::WorkflowRunRepository { source }` 被归类为 `ErrorClassification::Internal` 并映射到固定文案 `"application operation failed"`。DB 层 `variable_pool.set` 抛出的 `TypeMismatch { selector, value_type }`(含变量名与类型)在 `crates/db/src/repository/workflow_run_engine/payload.rs:67` 被 `ToSqlConversionFailure` 包装后逐层丢弃。

**影响**:类型填错的用户会以为遇到了系统 bug 而不是自己的输入问题;且 Internal 分类意味着前端也无法给出可操作的提示。

**建议**:为运行输入校验增加带参数的错误映射(类似 `WorkflowNameBlank` 的 `InvalidRequest` 分类),至少携带变量名与期望类型;或让 `WorkflowRunRepository` 对 `TypeMismatch`/undeclared 来源做细分透出。

### 问题 2(P1,错误信息):缺必填 Start 值启动时不告知是哪个变量

**现象**:不填必填 Start 值直接点启动(测试 3),错误为:

```
workflow run is not executable
```

引擎层明明有 `WorkflowValidationError::MissingRequiredStartVariable { name }`(`crates/application/src/workflow_run/engine/engine.rs:726`),但变量名在 `crates/backend/src/error.rs:589` 的 `WorkflowRunValidation(_)` 映射中被丢弃。

**影响**:有多个必填变量的工作流,用户只能逐个猜哪个没填。

**建议**:同问题 1,增加携带 `name` 的 PublicError 变体。

### 问题 3(P2,输出语义):Agent 节点输出始终混入注入的工作区边界与工作流上下文

**现象**:每个 Agent 节点的 `output`(也就是变量池里 `{node}.output`、迭代 collect 值、loop 反馈值、Output 节点绑定值)都是「回显 + `<workspace_boundary>`(含本机绝对路径的 8 条规则)+ `<current_workflow_step>` + `<workflow_context>`(拓扑与状态)」的全文,而不是 Agent 的最终回答本身。测试 5、11、14、15 的断言都因这一点从精确匹配改为标记匹配。

对用户的三层影响:

1. **`equals` 类条件/until 几乎不可用**:对 Agent 输出做整串相等比较永远不成立(测试 11 首版 `until equals` 三轮全空转后触顶失败,改 `contains` 才通过)。用户在编辑器里无从得知输出含大量注入前缀。
2. **迭代收集的不是"结果"**:collectSelector 收集到的是含上下文的全文(测试 7、15 中 collected 数组每项都带完整注入块),下游消费与结果展示都会带着这些噪音。
3. **循环反馈滚雪球**:loop 反馈变量每轮携带全文,第 2 轮提示词嵌入第 1 轮全文(含 workspace 绝对路径),N 轮后提示词线性膨胀,浪费 token 且可能干扰 Agent(测试 13、15 已观察到两轮嵌套)。

**建议**:将 `{node}.output` 的语义收敛为「Agent 的最终回答」,注入上下文只进会话不进输出;或至少为 collect/反馈/条件提供"纯回答"视图。这是行为变更,需要单独评审。

### 问题 4(P3,设计确认):continue 策略下绕过收集目标的轮次计为"失败"

测试 8 验证:区域内条件把某轮路由到非 collectSelector 目标(或无边可走)时,该轮结算为 failed——运行成功、`failed_count` +1、该轮不收集。这是迭代 ADR D3 的明确语义(防止读到上一轮的陈旧池值),但对预期"跳过该轮"的用户,`failed_count` 上升可能显得意外。建议在编辑器/运行文档中显式说明该语义(非代码缺陷)。

## 维护注意（测试基础设施）

两处在本套件开发中实际踩过的坑，后续新增场景时务必遵守：

1. **在 current-thread tokio runtime 中轮询必须用 `tokio::time::sleep(...).await`**（本文件 `WorkflowHarness::poll`），不能用 `std::thread::sleep`——阻塞运行时线程会饿死后端派生的 Agent 会话泵，表现为所有含 Agent 会话的测试超时（swift 节点不受影响）。现有 `iteration.rs` 也遵循此约定。
2. **`DesktopTestSetup` 必须存活到测试结束**（本文件 `WorkflowHarness::_setup` 字段）。它的 `TempDir` 在 Drop 时删除沙箱：Unix 上删除立即生效，运行中的 Agent 会话因 workspace 目录消失而以 "workspace is unavailable" 失败；Windows 上因 SQLite 句柄占用删除静默失败、目录侥幸保留，于是出现“本机全绿、Linux CI 全红”的假象。若把 setup 的构建封装进 harness/辅助函数，必须把 setup 本体存进存活期覆盖整个测试的结构体。
