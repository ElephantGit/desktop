# 工作流 orax 导入功能 — 实施计划

> **状态：A / B / C / D / E 全部完成，验证通过。**
>
> 本文件最初把全部条目预勾为已完成，但当时代码里一项都没落地。现已按真实代码重写计划、
> 逐段实施并逐段验证，实施过程的详细步骤记在第七节。
>
> **唯一未完成项在外部仓库**：`specs/` 里的 `plugin-packaging.md` / `.zh.md` 需要新增
> `workflow` kind 一节（该仓库未 checkout 到本工作区，改动须在其中按其自身约定进行）。
>
> 路径、命名与爆炸半径均已按真实代码校正。

## 一、已确定的设计决策

### 原始决策

1. **orax 存放方式**：新增 `PluginKind::Workflow`（`kind = "workflow"`），包内约定 `assets/workflows/*.json`
2. **导入交互**：一步导入——选 `.orax` → 后端解析全部工作流 → 逐个创建 + 自动发布，返回每个工作流的成败
3. **插件安装**：导入工作流的同时把 orax 作为插件安装（出现在插件列表）
4. **边界语义**：
   - 卸载插件不删除已导入的工作流（工作流是用户数据）
   - 单文件失败不连坐（一个工作流 JSON 坏不影响其他）

### 本轮定稿的决策

- **D1｜文件类型**：导入入口**只处理 `.orax`**。裸 `.json` 工作流导入**已存在**于工作流编辑器
  （`workflow-editor.tsx:1056` `importWorkflow`，经 `workflow-manager.tsx:52` 的 `onImport` 接线），
  不重复造这条路。
- **D2｜入口位置**：导入按钮放在**插件导入处**（`plugins-settings.tsx`），**工作流侧不新增按钮**。
- **D3｜包内 schema**：`assets/workflows/*.json` = **UI 导出格式原样**（`DemoWorkflow`）。
  含 `id`/`updatedAt`/React Flow 运行时字段（`measured`/`selected`/`dragging`/`zIndex`/`ariaRole`/
  `markerEnd`/`reconnectable`）——**可证明无害**，见第四节论证。
- **D4｜版本规则**：**显式 `version` 字段优先** → 回退文件名 stem 启发式 → 再回退后端自动生成
  `v{timestamp_millis}`。
- **D5｜契约形态**（D2 的推论）：行为**挂在 kind 上而非按钮上**。复用现有 `importPlugin` 操作，
  扩展 `ImportPluginResponse` 携带每个工作流的成败。**不新增 `importWorkflowPlugin` operation。**

> **D5 的关键理由**：若新增独立操作，同一个 `.orax` 走两个入口会有两种行为——用户在插件设置页
> 导入一个 workflow 包，插件装上了但工作流没进来，这是隐蔽的坑。行为必须由文件/kind 决定。

### D5 的范围收益

`plugin` namespace 无 Stream endpoint（全 Unary），`ImportPluginResponse` 是可加字段结构。
因此 **B 段（xtask 逻辑操作 + Desktop 绑定 + Tauri 命令）整体消失**，只剩契约 DTO 扩展。

---

## 二、代码现状（已核实）

### 现有 `.orax` 导入链路

```
选文件 → usePluginImport → client.plugin.import
  → tauri-transport invoke("import_plugin") → Tauri 命令宏
  → Plugins::import → PluginApi::import
  → Installer::install_local（解压 staging → 校验 sha256 → 按 kind 校验 → rename 提交到
                             <data>/plugins/installed/local/<name>/<version>）
  → lifecycle 重扫 + hook 命令冲突检测 → ImportPluginResponse { plugin_id, outcome }
```

| 环节          | 位置                                                                                                               |
| ------------- | ------------------------------------------------------------------------------------------------------------------ |
| 契约          | `crates/contracts/src/plugin.rs:663` `ImportPluginRequest`、`:672` `ImportPluginResponse`、`:631` `InstallOutcome` |
| 契约导出      | `crates/contracts/src/plugin.rs:753` `export()`，逐个 `Type::export(config)?`                                      |
| 逻辑操作      | `xtask/src/frontend/namespaces/plugin.rs:144-151`（`importPlugin`，Unary）— **D5 下无需改动**                      |
| Desktop 绑定  | `apps/desktop/src-tauri/bindings/plugin.rs:91-95` — **D5 下无需改动**                                              |
| Tauri 命令    | `apps/desktop/src-tauri/src/commands/plugin.rs:198-204` — **D5 下无需改动**                                        |
| Backend 编排  | `crates/backend/src/plugin/operations.rs:20-24` `Plugins`、`:246-253` `import()`                                   |
| Backend 实现  | `crates/backend/src/plugin.rs:164-193` `PluginApi`、`:615-651` `import()`、`:658-677` `finalize_new_install()`     |
| 安装器        | `crates/plugin-manager/src/install.rs:494-599` `install_local`                                                     |
| 前端入口      | `packages/app-shell/src/features/settings/plugins-settings.tsx:166-195` `handleImport`                             |
| 前端 mutation | `packages/app-shell/src/state/hooks/use-plugin-import.ts:9-17`                                                     |
| 前端按钮      | `packages/app-shell/src/features/settings/plugin-manager.tsx:57,135`                                               |

### `PluginKind` 的爆炸半径 = 4 个文件

1. `crates/plugin-manifest/src/enums.rs` — 枚举本体及三个 match
2. `crates/plugin-manifest/src/manifest.rs` — `validate_kind_sections`
3. `crates/plugin-manager/src/validation.rs` — `PluginContribution` 及分发
4. `crates/plugin-manifest/src/tests.rs` — 测试

补充事实：

- **契约里没有 kind 枚举可扩展**——`crates/contracts/src/plugin.rs:244` 是 `pub kind: String`。
- **`ora-plugin-lifecycle` 完全不 match `PluginKind`**（只在测试 fixture 里出现 kind 字符串）。
- `crates/plugin-manager/src/install.rs:579` 只有一处 `matches!(manifest.kind(), PluginKind::Hook)`，
  Workflow 不受影响。

### 契约命名已有占用

`ImportPluginRequest` / `ImportPluginResponse` / `InstallOutcome` 已被插件导入使用。

### `crates/application` 下没有 plugin 模块

插件的用例跳过 application 层，住在 `crates/backend/src/plugin/`。workflow 的 handler 确实在 application：

- `crates/application/src/workflow/handlers.rs:35` `CreateWorkflowHandler<Repository, IdGenerator, ClockSource>`
- `crates/application/src/workflow/handlers.rs:388` `PublishWorkflowHandler<Repository, IdGenerator, ClockSource>`

（该目录是 `handlers.rs / id_generator.rs / mapper.rs / ports.rs / tests.rs`，**没有 `workflow.rs`**。）

### `PluginApi` / `WorkflowApi` 都是 concrete struct，不是 trait

- `crates/backend/src/plugin.rs:164-193` `PluginApi`
- `crates/backend/src/workflow/definition.rs:23-38` `WorkflowApi`（`create:65`、`publish:117`）

加方法即可，不要按 trait 设计。

### `install_local` 的重复导入行为

- 落在**保留的 `local` namespace**
- 同一 `<name>/<version>` 已存在时返回 `AlreadyInstalled`（`install.rs:555`）

### `sha256` 自述摘要不可满足（文档 bug）

`install_local` 把清单自述的 `sha256` 与**归档文件本身**的摘要比对（`install.rs:537-546`），
而摘要写在归档内部——要满足 `sha256(包含 H 的归档) == H` 需密码学不动点，**不可能收敛**。

`specs` 侧 `plugin-packaging.md` §10.3 的「迭代到稳定」是错的。

实测：installed 形态在 `manifest.rs:477-484` 直接 `return Ok(None)`，`url`/`sha256` 的配对规则
**只对 release 形态生效**，因此**省略该字段完全合法**（release 形态无此问题——摘要在
`registry/**/orax.toml` 里，在归档外部）。

---

## 三、D3 的可行性论证（为什么原样放导出格式是安全的）

前端 `normalizeWorkflowDefinition`（`packages/workflow-runtime/src/definition.ts:54`）做三件事，
**三件在 Rust 侧都已有对应物**，无需移植：

| 它做的事                                        | Rust 现状                                                                                                                                                                                                   |
| ----------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 节点/边字段白名单（剥掉 React Flow 运行时字段） | **不需要**。Rust 所有 graph wire 类型都没有 `deny_unknown_fields`；`graph.rs:178-181` 明确注释「Unknown top-level metadata fields are ignored by serde; only `nodes` and `edges` participate in execution」 |
| Start 节点 `instruction`→`input` 迁移           | **已有**。`graph.rs:598-604`：`NodeType::Start => data.input.or(data.instruction)`，两端规则一致                                                                                                            |
| `validateWorkflowDefinition`（DAG 校验）        | **已有**。`WorkflowGraph::parse`（`graph.rs:561-696`）：重复 id、悬空边、环检测、多 Start、全局变量类型                                                                                                     |

且 `graph` 列在 domain / contracts / DB 三层**全是不透明 `String`**
（`domain/workflow.rs:41`、`contracts/workflow.rs:18`、`db/repository/workflow.rs:668`），
`create` / `update_draft` / `publish` **都不解析它**。

**结论**：死重字段可证明无害。DB 里会存下它们，但编辑器首次保存时被前端归一化掉，**自愈**。

### 导入时校验的落点

**调 Rust 已有的 `WorkflowGraph::parse`**，它是图的语义校验的唯一所有者。每文件 parse 一次，
失败只记该文件的 outcome —— 这就是 D4「单文件失败不连坐」的实现方式。

（实现时需确认 `WorkflowGraph::parse` 从 `workflow_run::engine::graph` 到 `workflow::import`
的可见性，必要时提升为 `pub(crate)`。）

---

## 四、实施计划

### A. orax 格式支持（plugin-manifest + plugin-manager）✅ 已完成

- [x] `crates/plugin-manifest/src/enums.rs`
  - [x] 枚举加 `Workflow` 变体 + 更新 doc comment
  - [x] `may_ship_targeted_artifact()` → 归入 `false` 组
  - [x] `as_str()` / `from_str()` → `"workflow"`
- [x] `crates/plugin-manifest/src/manifest.rs`
  - [x] `validate_kind_sections()` 补 Workflow arm：`[workbench]` 与 `[webview]` 都拒绝
  - [x] `[artifact]` 拒绝由 `may_ship_targeted_artifact` 自动覆盖（已加测试验证）
- [x] 新建 `crates/plugin-manager/src/workflow.rs`（206 行）：`validate_workflow`
  - [x] `InstalledWorkflowDescriptor { files: Vec<PortableRelativePath> }`
  - [x] 校验 `assets/workflows/`：目录存在、至少 1 个 `*.json`、数量 ≤ 上限
  - [x] 非 `*.json`、子目录、符号链接一律**忽略**（不拒绝整包）
- [x] `crates/plugin-manager/src/limits.rs`：`MAX_WORKFLOWS_PER_PACKAGE = 256`
- [x] `crates/plugin-manager/src/validation.rs`：`PluginContribution::Workflow` +
      `kind()` + `entrypoint()` + 分发 arm
- [x] `crates/plugin-manager/src/lib.rs`：导出 `InstalledWorkflowDescriptor` /
      `WORKFLOW_ASSET_DIRECTORY` / `WORKFLOW_FILE_EXTENSION`
- [x] **计划外但必需**：`PluginContribution` 新增变体触发了全仓穷尽匹配点，共 6 处——
      `crates/surface/src/definition.rs`（无 surface → `None`）、
      `crates/plugin-lifecycle/src/{permissions,registration,state}.rs`、
      `crates/backend/src/plugin.rs:690`（hook 命令冲突检测）、
      `crates/plugin-manager/src/kind_tests.rs` ×2
- [x] 契约：`InstalledPluginContribution` 加 unit 变体 `Workflow`（`contracts/src/plugin.rs`）
- [x] 契约生成产物：`packages/contracts/src/dto/plugin.ts` 已重新生成
- [x] 测试：9 个新增用例全部通过

**偏离计划一处（有意）**：计划写校验"存在、可解析、数量上限"，实际**不做 JSON 可解析性校验**。
理由是边界语义第 4 条「一个工作流 JSON 坏不影响其他」——若包校验因某个 JSON 损坏而拒绝整包，
其余工作流就一个都进不来，恰好违反该语义。因此包校验只管**布局**（目录/数量/路径安全），
坏 JSON 留给导入时逐文件报告。副作用是 `plugin-manager` 不需要新增 `serde_json` 依赖。

**前端零改动**：已安装列表直接渲染 `plugin.kind` 字符串（`plugin-manager.tsx:213`），
workflow 包会正常显示为 `0.1.0 · workflow · stopped`。
`MARKETPLACE_KIND_ORDER`（`plugins-settings.tsx:45`）未加 `workflow`——该 kind 走本地导入，
不经 marketplace，故暂不列入。

### B. 契约（D5 下只改 DTO）✅ 已完成（并入 C 段）

> 契约 DTO 是 application 层导入 handler 的输入/输出类型，所以它在 C 段开头一并落地。

- [x] `crates/contracts/src/plugin.rs`
  - [x] 新增 `ImportedWorkflowOutcome`，形状对齐 `InstallOutcome`（`tag = "state"` /
        `rename_all = "snake_case"` / `rename_all_fields = "camelCase"`），两个 arm 分别是
        `Imported { source_file, workflow_id, name, version }` 与 `Failed { source_file, reason }`
  - [x] `ImportPluginResponse` 加 `workflows: Vec<ImportedWorkflowOutcome>` 字段（可加，不破坏现有消费者）
  - [x] 注册进 `export()`
  - [x] 契约测试：更新 `serializes_import_plugin_contract`（补空 `workflows`）+ 新增
        `serializes_imported_workflow_outcomes`（两个 arm + camelCase）
- [x] **确认不需改动**：xtask 逻辑操作、Desktop 绑定、Tauri 命令（D5 的范围收益，已核实）

### C. 业务实现 ✅ 已完成

- [x] 新建 `crates/application/src/workflow/version.rs`（**计划外**，见下）
  - [x] `MAX_VERSION_BYTES` / `DRAFT_VERSION` / `is_valid_user_version` / `is_publishable_version`
  - [x] 把 `PublishWorkflowHandler` 里**内联**的版本校验抽成共用谓词，供 publish 与 import 共用
- [x] 新建 `crates/application/src/workflow/import.rs`
  - [x] `WorkflowDocument { source_file, contents }`
  - [x] `derive_publish_version(explicit, source_file, workflow_name) -> Option<String>`（D4 规则）
  - [x] `ImportWorkflowsHandler { create, publish }`：组合两个既有 handler，不重实现规则
  - [x] 每个文件：解析 JSON → 取 `name`/`version` → **调 `WorkflowGraph::parse` 校验** →
        `CreateWorkflowHandler` → `PublishWorkflowHandler`
  - [x] 逐文件收集 outcome，**永不中断批次**
- [x] `crates/backend/src/plugin.rs`：`PluginApi::import()` 返回 `ImportedPlugin`（含未解释的
      `workflow_documents`），**plugin 层不掺工作流领域逻辑**
- [x] 新建 `crates/backend/src/plugin/workflow_documents.rs`（**计划外**，见下）
- [x] `crates/backend/src/plugin/operations.rs`：`Plugins` 持 `Arc<WorkflowImport>`，
      `import()` 编排「装包 → 同步 agent → 导工作流 → 组装响应」
- [x] `crates/backend/src/workflow/definition.rs`：`WorkflowImport` 类型别名 + `workflow_import()`
- [x] `crates/backend/src/bootstrap.rs`：`Backend::open` 处接线
- [x] 顺序语义实现：插件先提交落盘再导工作流

**两处计划外的改动，都是被检查规则逼出来的：**

1. **`version.rs`**：`derive_publish_version` 需要判断候选版本能否发布，而
   `PublishWorkflowHandler` 把同一套规则（非空 / ≤128 字节 / 非 `.`/`..` / 无分隔符与控制字符）
   内联在函数体里。直接复制会造成两处规则漂移，所以抽成共用谓词。
   `draft` 保留字单独处理：publish 需要区分 `WorkflowVersionReserved` 与
   `WorkflowVersionInvalid` 两个错误，所以 `is_valid_user_version` 不含它，而
   `is_publishable_version` 加上它——import 侧不能再修复候选，必须走自动版本回退。

2. **`workflow_documents.rs`**：新增代码把 `crates/backend/src/plugin.rs` 顶到 803 行，
   触发 `check:rust-size` 的 800 行上限。按 AGENTS.md「超过约 800 行应在新模块添加功能」，
   把读取逻辑拆成独立模块。

**graph 存储**（已按 D3 + 你的决定实现）：**原样存整份导出文档**，不做投影或裁剪。
包内 JSON 的字节内容经校验后直接作为 `graph` 字符串写入。`id`/`name`/`updatedAt`/React Flow
运行时字段都跟着存——Rust 三层都不解析它们，前端 `parseWorkflowGraph` 容忍未知字段，
用户首次编辑保存时被前端归一化掉，自愈。Workflow 记录的 `name` 从文档顶层读一次。

### D. 前端 ✅ 已完成

- [x] **不新增按钮、不新增操作**（D2 + D5）—— 复用插件设置页的「导入插件」
- [x] `state/data/workflows.ts`：
  - [x] 把三个 key 常量改为从 `workflowQueriesPrefix` 派生（前缀只有一个来源）
  - [x] 新增 `invalidateWorkflowQueries(queryClient)`（**失效归属数据所有者**，UI 不复制 key）
  - [x] 导出 `workflowKeys`（对齐 `plugins.ts` 的 `pluginKeys` 约定，供测试引用而非复制字面量）
- [x] `state/hooks/use-plugin-import.ts`：`onSettled` 里 `Promise.all` 同时失效 plugin + workflow 查询
- [x] `features/settings/plugins-settings.tsx`：
  - [x] 新增 `workflowImportSummary(outcomes, t)`，报告「导入 N 个 / 拒绝 M 个」
  - [x] toast 成功消息带 `description` —— 但**只在包确实带工作流文档时**才传第二个参数
- [x] `features/settings/translations/plugins.ts`：`importWorkflowsImported` /
      `importWorkflowsSummary` 中英双语
- [x] 测试：`plugins-settings.test.tsx` 2 个用例（全成功 / 部分拒绝）+ 新建
      `use-plugin-import.test.tsx` 1 个用例（断言 workflow 库被失效）

**踩到的坑（已修）**：最初写成 `toast.success(message, response.workflows.length === 0 ? undefined : {...})`，
这会让**普通插件导入**也以**两个参数**调用 toast，导致既有断言
`expect(successToast).toHaveBeenCalledWith(stringMatching(...))` 因参数个数不符而失败
（32 个测试挂 1 个）。改为 `if (length === 0) { toast.success(message); return; }` 分支，
常见路径保持单参数调用。

**验证测试不是摆设**：临时移掉 `invalidateWorkflowQueries` 后，
`use-plugin-import.test.tsx` **确实失败**，确认它真的在断言失效行为，而不是恒真。

### E. 生成 + 测试 + 文档 ✅ 已完成

- [x] `task export-contracts` → `task check:contracts`（A 段契约改动后已生成并校验）
- [x] plugin-manager：workflow kind 校验测试（A 段完成）
- [x] application：`derive_publish_version` 表驱动测试（9 组）+ `version.rs` 谓词测试 3 个 + import 测试 6 个（成功／重名／坏 JSON／无名／悬空边／多文档不连坐）
- [x] 契约序列化测试：`serializes_imported_workflow_outcomes`（两个 arm + camelCase），
      并更新 `serializes_import_plugin_contract` 覆盖空的 `workflows`
- [x] backend 端到端：`imports_workflow_package_documents_alongside_the_plugin`
      （真 `.orax` 装包 → 读文档 → 建工作流并发布 → 坏文档单独失败，其余照常）
- [x] 前端 memory handler：`importedWorkflows` 状态 + 响应带 `workflows`
- [x] 前端用例：`plugins-settings.test.tsx` 2 个 + `use-plugin-import.test.tsx` 1 个（D 段完成）
- [x] `docs/workflow-orax-import.md` + `.zh.md`（新建，中英互链）+ `README.md` 索引条目
- [x] `task lint` + 全量测试收尾（见下）

**产物文档**：`docs/workflow-orax-import.md` / `.zh.md`，六节——包布局、文档格式、
导入链路、版本推导、失败语义、边界。中英互链，并加入 `README.md` 的 Architecture Docs 索引。
交叉链接指向 `workflow.md` / `desktop-runtime.md` 与 `../specs/plugin-packaging.md`
（后者与 `docs/effect-skill-state.md` 的既有写法一致；`/specs` 在 `.gitignore` 中，是独立仓库）。

### 最终验证（E 段收尾）

> `task test` 会停在 `test:crates` 的 `rg` precondition 上（本机无真 `ripgrep`，见第六节）。
> 因此按它的**每个子步骤**手动执行。

| `task test` 的子步骤                                                       | 结果                        |
| -------------------------------------------------------------------------- | --------------------------- |
| `task test:frontend`（含 `check:contracts` + `lint:frontend`）             | ✅ 166 文件 / 1443 测试全过 |
| `task lint:crates`                                                         | ✅ 通过                     |
| `cargo test --workspace --exclude ora-desktop --exclude ora-desktop-tests` | ✅ **70/70 二进制全过**     |
| `task test:tauri`                                                          | ✅ 83 通过                  |
| `task test:e2e`                                                            | ✅ 12 通过                  |
| `task lint`（全部 lint）                                                   | ✅ 通过                     |
| `task check:contracts`                                                     | ✅ 无漂移                   |

---

## 五、需要外部仓库配合

`specs/` 是独立 Git 仓库，**当前工作区没有 checkout**。打包格式文档
（`plugin-packaging.md` / `.zh.md`）在其中，需要新增 `workflow` kind 一节：

- `kind` 封闭集合从 6 种改为 7 种
- §6 各 kind 包内容要求新增 `workflow` 小节（`assets/workflows/*.json` 布局）
- §10.3 修正 `sha256` 的不可满足描述（见第二节）
- §12 自检清单补 `workflow` 行

改动须在 `specs/` 仓库内按其自身约定进行，不能作为本仓库的附带修改。

---

## 六、验证命令

```bash
task lint:crates        # Rust lint
task test:crates        # Rust 测试
task lint:frontend      # 前端 lint
task test:frontend      # 前端测试（受 scripts/run-with-clean-stderr.ts 约束）
task test:tauri         # Desktop 变更必跑
task export-contracts   # 生成
task check:contracts    # 校验生成产物无漂移
task lint               # 全部 lint
task test               # 收尾全量（耗时长）
```

### 本机环境注意事项

这三条是这台机器上实测踩到的，换台机器可能不同：

1. **`cargo` 和 `deno` 不在 PATH**。分别在 `C:\Users\kyber\.cargo\bin` 和
   `C:\Users\kyber\.deno\bin`。跑 task 前先补：

   ```bash
   export PATH="/c/Users/kyber/.cargo/bin:/c/Users/kyber/.deno/bin:$PATH"
   ```

   `task export-contracts` 的第二段是 `deno task --filter @ora/contracts generate:error-schema`，
   没有 deno 会在 Rust 导出成功之后才失败，看起来像"生成失败"其实只是 PATH 问题。

2. **`node_modules` 需要先装**。`task install:frontend` 装（需要 deno 在 PATH）。
   没装的话 `check:contracts` 的 `scripts/check-contract-schema.ts` 会报
   `Could not find "ts-to-zod" in a node_modules folder`。

3. **系统没有真正的 `ripgrep`**，导致 `task test:crates` 直接拒绝运行：

   ```
   task: Ripgrep (rg) is not available on PATH. Install it before running Rust tests.
   ```

   `test:crates` 的 precondition 会新起一个 `sh` 检查 `command -v rg`，因此**解析不到 shell
   函数形式的 `rg`**（交互式 shell 里的 `rg` 是函数，不是可执行文件）。
   而 `crates/fs` 的测试确实需要真二进制（`WorkspaceFileSystem::new("rg".into(), ...)`）。
   绕过办法是直接跑 `cargo test --workspace --no-fail-fast`。

4. **`ora-utils` 的 `probe_reports_a_refused_connection` 是 flaky**。它探
   `http://127.0.0.1:1/` 并在 2 秒内断言 `DownloadError::Network`；满载全量跑时会走超时分支而失败，
   单独跑必然通过。失败时先单独复现一次再判断：

   ```bash
   cargo test -p ora-utils --features http-reqwest --lib probe_reports_a_refused_connection
   ```

   （**注意必须带 `--features http-reqwest`**，该测试在该 feature 门控之后，不带会显示 0 个测试。）

---

## 七、实施记录（A / B / C / D 逐段详细步骤）

### A 段：orax 格式支持

1. **读现有 per-kind 校验器的形状**：`plugin-manager/src/skill.rs`（最近的类比——
   它校验 `assets/<name>/SKILL.md` 树，与 `assets/workflows/*.json` 同构）。
   归纳出公共形状：`pub(crate) fn validate_<kind>(package_root, ...) -> Result<Installed<Kind>Descriptor, ManifestValidationError>`，
   用 `invalid(field_path, message)` 报错、`CanonicalPathRoot::resolve_existing` 做符号链接安全校验。
2. **改枚举** `plugin-manifest/src/enums.rs`：加 `Workflow` 变体，并在 `may_ship_targeted_artifact`
   （归 `false` 组）、`as_str`、`from_str` 三处补 arm；更新枚举 doc comment 说明 Workflow 是
   processless 且无专属 section。
3. **改清单 section 配对** `plugin-manifest/src/manifest.rs` 的 `validate_kind_sections`：
   四个 arm 的 kind 列表都加上 `Workflow`（该函数是穷尽 match，不加会编译失败）。
4. **写校验器** 新建 `plugin-manager/src/workflow.rs`：
   - 解析 `assets/workflows` 为 `PortableRelativePath`，`CanonicalPathRoot` 解析并确认是目录
   - 枚举条目，**只收普通文件 + `.json` 后缀**（用 `entry.file_type()`，不跟随符号链接）
   - 空目录报错、超过上限报错；名字排序保证确定性
   - descriptor 只存**包内相对路径**（不读文件内容，解析留给导入时逐文件做）
5. **加上限常量** `plugin-manager/src/limits.rs`：`MAX_WORKFLOWS_PER_PACKAGE = 256`。
6. **接到分发器** `plugin-manager/src/validation.rs`：`PluginContribution` 加变体、
   `kind()`、`entrypoint()`（归 `None` 组）、`match manifest.kind()` 加 arm。
7. **导出** `plugin-manager/src/lib.rs`。
8. **编译，按报错补穷尽匹配点**（6 处，见 A 段清单）——这一步是编译器驱动的，不用猜。
9. **契约** `contracts/src/plugin.rs`：`InstalledPluginContribution` 加 unit 变体 `Workflow`。
10. **补测试**：`plugin-manager/src/workflow.rs` 4 个、`kind_tests.rs` 2 个、`plugin-manifest/src/tests.rs` 3 个。
11. **格式化 → 重新生成契约 → 全量验证**：
    `task format:crates` → `task lint:crates` → `task export-contracts` →
    `task check:contracts` → `task lint:frontend` → `cargo test --workspace --no-fail-fast`。

### B 段：契约 DTO（并入 C 段完成）

D5 决议让 B 段只剩契约改动，而 application 的导入 handler 需要 outcome 类型，
所以在 C 段开头一并做了：

1. `contracts/src/plugin.rs` 新增 `ImportedWorkflowOutcome`，形状对齐既有的 `InstallOutcome`
   （`tag = "state"` / `rename_all = "snake_case"` / `rename_all_fields = "camelCase"`）。
2. `ImportPluginResponse` 加 `workflows: Vec<ImportedWorkflowOutcome>`（可加字段）。
3. 注册进 `export()`。
4. 更新 `serializes_import_plugin_contract`（补空 `workflows`），新增
   `serializes_imported_workflow_outcomes`（两个 arm + camelCase）。

### C 段：业务实现

1. **先确认可见性**：`WorkflowGraph::parse` 已经是 `pub`，且 `WorkflowGraph` 从
   `crate::workflow_run` 重导出 —— **不需要提可见性**（计划里预判的这点没发生）。
2. **读三个 handler 的签名**：`CreateWorkflowHandler::new(repository, id_generator, clock)`（仓库按值）、
   `PublishWorkflowHandler::new(Arc<Repository>, ...)`（仓库共享）。
   `WorkflowApi::new` 里用 `(*repository).clone()` 满足前者 —— 我的 handler 照抄这个模式。
3. **抽出共用版本谓词**（计划外）新建 `application/src/workflow/version.rs`：
   `MAX_VERSION_BYTES` / `DRAFT_VERSION` / `is_valid_user_version` / `is_publishable_version`。
   把 `PublishWorkflowHandler` 内联的那套判断换成调用。
   **分层原因**：publish 需要区分 `WorkflowVersionReserved` 与 `WorkflowVersionInvalid`，
   所以基础谓词不含 `draft`；import 无法修复候选、只能回退，需要合并答案。
4. **写导入用例** 新建 `application/src/workflow/import.rs`：
   - `WorkflowDocument { source_file, contents }`
   - `derive_publish_version`：显式 `version` → 文件名 stem（去 `.reactflow.json`/`.json`，
     忽略大小写，最长后缀优先）→ 工作流名 → `None`
   - `ImportWorkflowsHandler` 组合两个既有 handler，逐文档 `try_import`，
     失败转成该文档的 `Failed` outcome，**永不中断批次**
5. **写测试**（6 个）+ fake 仓库。**踩到的两个坑**：
   - fake 必须让内层状态共享（`Arc<Mutex<...>>`），因为 create 持有克隆而 publish 持有 `Arc`，
     深拷贝会让两半看不见彼此写入
   - 第二个工作流的 id 是 `workflow-4` 而非 `workflow-3` —— 一篇文档消耗 **3 个** id
     （create 的 workflow + snapshot，publish 的 snapshot），我漏算了 publish 那次
6. **后端接线**：
   - `PluginApi::import` 改返回 `ImportedPlugin`（带未解释的 `workflow_documents`）
   - 新建 `backend/src/plugin/workflow_documents.rs`（计划外，因为 800 行上限）
   - `Plugins` 加 `Arc<WorkflowImport>` 字段并编排「装包 → 同步 agent → 导工作流 → 组装响应」
   - `backend/src/workflow/definition.rs` 加 `WorkflowImport` 别名与 `workflow_import()` 构造函数
   - `backend/src/bootstrap.rs` 接线；`install_tests.rs` 的 helper 跟进
7. **补端到端测试**：`install_tests.rs` 新增真 `.orax`（含一好一坏两个文档）走 `Plugins::import`。
8. **格式化 → lint → 全量验证**。lint 报 800 行超限 → 拆模块 → 再 lint。

### D 段：前端

1. **先找失效归属**：`workflowLibraryKey` 是 `state/data/workflows.ts` 的模块私有常量。
   按 feature-change-guide「查询身份与失效归数据所有者，UI 不得复制 query-key 字符串」，
   失效函数必须加在那里。
2. 把三个 key 常量改为从 `workflowQueriesPrefix` 派生（前缀单一来源），
   新增 `invalidateWorkflowQueries`，导出 `workflowKeys`（对齐 `plugins.ts` 的 `pluginKeys`）。
3. `use-plugin-import.ts` 的 `onSettled` 用 `Promise.all` 同时失效两组查询。
4. `plugins-settings.tsx` 加 `workflowImportSummary` 并接到 toast 的 `description`。
5. 翻译：`translations/plugins.ts` 加两个 key 的 `zh-CN` / `en-US` 两份。
6. **写测试时发现回归**：`toast.success(message, undefined)` 会让既有断言（单参数）失败。
   改成条件分支后常见路径保持单参数。**这个坑很隐蔽**：`toHaveBeenCalledWith` 对参数个数敏感。
7. **验证测试有效性**：临时移掉 `invalidateWorkflowQueries`，确认新 hook 测试**会失败**，
   证明它断言的是真实行为而非恒真。
8. **格式化 → lint → 全量前端测试**。

### E 段：生成 + 测试 + 文档

1. **文档结构**：先读 `docs/session-mcp.md` / `.zh.md` 与 `README.md` 的索引，确定既有约定
   （标题下 `English | [中文](x.zh.md)` / `[English](x.md) | 中文`；中文版按惯例比英文版凝练）。
2. **确认交叉链接写法**：`docs/effect-skill-state.md` 已有 `../specs/...` 的引用，
   `.gitignore` 第 33 行有 `/specs` —— 确认 specs 是独立 clone 在该位置、且链接路径正确。
3. **写英文版** `docs/workflow-orax-import.md`：包布局 / 文档格式 / 导入链路 / 版本推导 /
   失败语义 / 边界，六节。
4. **写中文版** `docs/workflow-orax-import.zh.md`：同六节，按现有中文档的凝练风格。
5. **加入 README 索引**：`README.md` 的 Architecture Docs 段、紧随 Workflow 条目。
6. **验证链接与配对**：逐个确认引用的文档文件存在、中英互链方向正确。
7. **全量收尾**：按 `task test` 的每个子步骤手动执行（`task test` 本身会停在 `test:crates`
   的 `rg` precondition 上），结果见 E 段末尾的表格。

### 各段结束时的验证结果

| 阶段 | 验证                                                                                            |
| ---- | ----------------------------------------------------------------------------------------------- |
| A    | `cargo test --workspace --no-fail-fast` **74/74 二进制通过**；lint/contracts/frontend lint 全过 |
| C    | 同上 74/74；`test:frontend` 165 文件 / 1440 测试；`test:tauri` 83 通过                          |
| D    | `test:frontend` **166 文件 / 1443 测试**全过；lint 通过                                         |
| E    | 见「最终验证（E 段收尾）」表格：全部通过，`check:contracts` 无漂移                              |

### 三个值得记住的教训

1. **契约加字段会波及前端测试夹具**。`ImportPluginResponse` 加 `workflows` 后，
   memory handler 立即缺失字段而编译失败。改契同时要搜 `test/memory/` 下的夹具。
2. **`toHaveBeenCalledWith` 对参数个数敏感**。给已有调用加第二个参数（哪怕是 `undefined`）
   会打破既有断言。要么保持单参数，要么同步改断言。
3. **模块尺寸检查是硬门槛**。`check:rust-size` 的 800 行上限不认「只多了 3 行」，
   新增功能前先看目标文件当前行数。
