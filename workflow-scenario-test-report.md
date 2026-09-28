# 工作流功能用户级场景测试报告

- 测试对象:`fix/workflow-catalog-order-and-windows-tooling` 分支(含 JSON Start 变量类型修复 + 本轮三组缺陷修复)
- 测试方式:通过生产 Backend 接口(创建 → 发布 → 创建运行 → 填写启动输入 → 启动 → 轮询终态)驱动真实运行引擎,使用 fake OpenCode Agent 插件执行 Agent 节点。fake Agent 的回答语义与真实 Agent 对齐:默认把任务指令(变量已渲染)作为最终回答回显,提示词中的注入块(workspace 边界、工作流上下文)只进会话不进输出;任务指令里的 `FAKE_REPLY:` 指令让场景可以指定精确回答(含结构化输出的 JSON)。
- 测试代码:`e2e/tests/desktop/core/session/tests/workflow_scenarios.rs`(20 个场景,约 4 秒跑完)
- 总体结果:**20/20 场景通过**。首轮测试发现的 3 个缺陷已全部修复并回归验证;新增 5 个场景覆盖 equals 级条件/循环判定、skills 插件、MCP 插件与结构化输出 × 迭代组合。

## 场景覆盖矩阵

| #   | 场景                    | 覆盖点                                                                                                                                                                         | 结果 |
| --- | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---- |
| 1   | 全数据类型 Start 输入   | string / number / boolean / select / array[string] / array[object] / object / any(含 json 控件声明数组类型,即本 PR 修复路径)渲染进 Agent 提示词;变量池携带类型化值             | ✅   |
| 2   | 运行输入类型校验        | number 收字符串、object 收数组、array[string] 收对象、未声明变量 → 全部拒绝,且每个拒绝都以 `workflow_run_input_invalid` 携带变量名与期望类型(本轮修复);合法值通过并成功执行    | ✅   |
| 3   | 必填 Start 值缺失       | 不填必填值直接启动 → 启动被拒,零节点执行,错误携带缺失变量名(本轮修复)                                                                                                          | ✅   |
| 4   | 文件类型 Start 输入     | file / array[file] 以工作区相对引用渲染;绝对路径、`..` 穿越路径在输入边界被拒并按坏请求携带变量名透出                                                                          | ✅   |
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
| 16  | 精确输出语义(新增)      | Agent 节点 output 恰为最终回答:默认回显不含 `<workspace_boundary>`/`<current_workflow_step>`/`<workflow_context>`;equals 条件按精确输出路由;输出绑定原样传递                   | ✅   |
| 17  | 循环 until equals(新增) | until equals 精确匹配 Agent 回答第 1 轮即退出;反馈变量只携带回答本身                                                                                                           | ✅   |
| 18  | skills 插件(新增)       | 安装 Skill 插件 → 部署冻结物化回执 → 提示词携带 `/code-review` 强制调用契约与 worktree 物化路径;禁用绑定不阻断;启用不存在的技能在创建运行时以 `workflow_skill_not_found` 拒绝  | ✅   |
| 19  | mcp 插件(新增)          | 安装两个 MCP 插件,节点启用其一禁用其一 → 迭代每轮会话的 session/new 只携带冻结允许列表 `["official/tools"]`;禁用插件绝不进入会话                                               | ✅   |
| 20  | 结构化输出 × 迭代(新增) | 结构化输出契约把 Agent 的 JSON 回答解析为 `body.structured_output`;collectSelector 收集为类型化 array[object];运行输出按对象深相等断言                                         | ✅   |

## 首轮发现问题的处置结果

### 问题 1(P1,已修复):运行输入的类型校验失败报告为无细节的内部错误

**处置**:`workflow_run_input_invalid` 公开错误贯穿全链——DB 层把变量池的类型拒绝映射为类型化拒绝结果(变量名 + 期望类型 + 原因),引擎经端口结果透出,应用层映射为 `ApplicationError::WorkflowRunInputInvalid`,后端按 `InvalidRequest` 分类携带 `WorkflowRunInputInvalidParams { variable, reason }`;前端新增中英文翻译并改用契约错误 toast 展示。场景 2、4 断言变量名与原因(如 "value does not match the declared type number")。

### 问题 2(P1,已修复):缺必填 Start 值启动时不告知是哪个变量

**处置**:`WorkflowValidationError::MissingRequiredStartVariable` / `InvalidStartVariableOption` 在后端映射层拆出,同样以 `workflow_run_input_invalid` 携带变量名与原因("required value is missing")。场景 3 断言错误携带缺失变量名。

### 问题 3(P2,已定位为测试基建伪影并修复):Agent 节点输出"混入"注入上下文

**结论**:产品语义本就正确——`AssistantOutputAccumulator` 只保留最终助手消息(非交互路径),交互完成路径读取最后一条落定的助手消息;首轮观察到的"输出包含 `<workspace_boundary>` 等注入块"是 fake Agent 把整个提示词回显为回答造成的伪影,真实 Agent 的最终回答不含这些块。**修复方向因此落在测试基建**:fake Agent 改为按任务指令回答(与真实 Agent 行为对齐),并新增提示词日志(`acp_prompts.jsonl`)承接提示词级断言(skills 契约、MCP 选择等)。场景 16、17、20 证明:equals 条件/until 可用、收集值干净、反馈不滚雪球。

### 问题 4(P3,设计确认项):continue 策略下绕过收集目标的轮次计为"失败"

**结论**:保持迭代 ADR D3 语义不变;`docs/workflow.md` / `workflow.zh.md` 已明确记载该行为("分支绕开收集目标的轮次按失败结算……而不是读取上一轮的陈旧池值"),无需代码变更。场景 8 持续钉死该语义。

## 维护注意(测试基础设施)

三处在本套件开发中实际踩过的坑,后续新增场景时务必遵守:

1. **在 current-thread tokio runtime 中轮询必须用 `tokio::time::sleep(...).await`**(本文件 `WorkflowHarness::poll`),不能用 `std::thread::sleep`——阻塞运行时线程会饿死后端派生的 Agent 会话泵,表现为所有含 Agent 会话的测试超时(swift 节点不受影响)。现有 `iteration.rs` 也遵循此约定。
2. **`DesktopTestSetup` 必须存活到测试结束**(本文件 `WorkflowHarness::_setup` 字段)。它的 `TempDir` 在 Drop 时删除沙箱:Unix 上删除立即生效,运行中的 Agent 会话因 workspace 目录消失而以 "workspace is unavailable" 失败;Windows 上因 SQLite 句柄占用删除静默失败、目录侥幸保留,于是出现"本机全绿、Linux CI 全红"的假象。若把 setup 的构建封装进 harness/辅助函数,必须把 setup 本体存进存活期覆盖整个测试的结构体。
3. **fake Agent 的回答语义**(`e2e/fake-agent/acp.rs::fake_reply`):默认回答 = `Fake agent received: {任务指令}`(注入块只进 `acp_prompts.jsonl` 日志,不进回答);任务指令含 `FAKE_REPLY: ` 前缀时精确回答其后文本(JSON、equals 目标值皆可);提示词含上一轮失败块时回答 `{"ok":true}`(结构化输出重试路径依赖此行为)。新增需要精确回答或提示词级断言的场景时分别使用 `FAKE_REPLY:` 与 `ScenarioRun::prompts()`。
