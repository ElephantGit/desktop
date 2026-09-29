# 工作流编辑器加载路径

[English](workflow-editor-load-path-fix.md) | 中文

打开一个工作流会让整个桌面窗口变白。本文记录故障现象、为什么一个坏节点能拖垮整页，以及加载路径是如何加固的。

## 现象

在编辑器里点开某个工作流后，窗口永久白屏：没有画布、没有界面框架、无法恢复。渲染进程报出：

```
Uncaught TypeError: Cannot read properties of undefined (reading 'label')
    at WorkflowFlowNodeView (node.tsx)
```

后端全程正常——草稿、版本、发布请求都正常返回，进程也没有退出。故障完全发生在前端渲染阶段。

## 根因

出问题的图里有一个 `data.kind` 为 `aggregator` 的节点——这个类型在当时没有任何 Ora 版本定义过。而每一步本该拦住它的环节，按设计都是宽容的：

1. **存储是不透明的。** `graph` 字段以字符串保存 React Flow 文档，工作流定义的处理器原样透传。工作流插件包里的文件原文会逐字成为存储的 graph，所以未知内容进入编辑器是设计使然，不是意外。
2. **编解码器刻意宽松。** `parseWorkflowGraph`（`packages/workflow-runtime/src/graph-codec.ts`）只归一化 viewport、过滤注解和全局变量，对节点类型不作判断——它的契约是让本版本不认识的字段原样往返。
3. **渲染目录是封闭集合。** 画布为每种节点类型注册一个组件，而 `createMockWorkflowNodeType`（`packages/workflow-mock/src/capabilities.ts`）用穷尽 `switch` 解析类型、没有 `default` 分支。联合类型之外的类型返回 `undefined`。
4. **节点视图立刻解引用。** 节点组件在首次渲染就读取 `createMockWorkflowNodeType(data.kind, locale).label`。
5. **没有任何兜底。** React 在渲染抛错时卸载整棵树，而 `packages/app-shell` 与 `packages/ui` 里都没有错误边界，于是一个节点视图失败就让整个窗口变白，而不只是画布。

结果是：对本版本语义非法的内容抵达了渲染层，而渲染层无法表达"我不认识这个节点"。

`aggregator` 后来已作为一等类型补进目录，所以那份具体的图现在能正常渲染。但故障类别没有变——任何目录之外的类型仍会复现。

## 修复方式

选定的规则是：**在所有加载路径共用的解析边界上做净化，并把被跳过的内容告诉用户**——而不是让每个渲染层消费者都对画不出来的类型做防御。

### 运行时（`packages/workflow-runtime`）

- `types.ts` 新增权威类型清单 `WORKFLOW_NODE_KINDS`，`WorkflowNodeKind` 改由它派生（`(typeof WORKFLOW_NODE_KINDS)[number]`），净化所用的清单与编译器检查的类型不会再各自漂移。
- `graph-codec.ts` 新增 `parseWorkflowGraphWithReport(graph)`，返回 `{ envelope, droppedNodeCount, droppedNodeKinds }`，并丢弃：
  - 不是可用记录的节点（缺少非空字符串 `id`，或 `data` 不是对象）；
  - `data.kind` 不在 `WORKFLOW_NODE_KINDS` 内的节点；
  - 引用了上述被丢弃节点的连线——指向已不存在节点的连线本来就画不出来。
- `parseWorkflowGraph` 变成它的薄封装，因此全部六处加载点——草稿加载、版本预览、导出预览、导出处理、运行视图——由同一条规则保护，无需逐个改造。
- 旧的 `prompt`/`model` 节点在类型判断**之前**升级为 `agent`，因此持久化的旧节点会落到受支持的替代类型上，而不是被当作未知类型丢弃。
- 编解码器的其余行为不变：非法 JSON 仍然加载为空画布，未知字段仍然在重新保存后保留，指向"从来就不存在"的 id 的连线仍然保留。

### 编辑器（`packages/app-shell`）

- 草稿加载走带报告的解析。由于 hydrate 发生在渲染阶段，报告经 ref 带出，由 effect 在草稿提交后触发提示，因此被丢弃的渲染不会产生提示。
- 版本预览就地提示，因为它本来就是异步处理器。
- 用户看到：`已跳过 1 个本版本无法渲染的节点（router）` / `Skipped 1 nodes this version cannot render (router)`——包含数量与具体类型，丢失的内容有解释而不是静默发生。因记录残缺而被丢弃的节点没有类型名可报，显示为未知类型。
- 运行视图与导出路径走同一个编解码器，自动受保护但不弹提示，因为它们是只读投影。

## 验证

- **单元测试（`packages/workflow-runtime/src/graph-codec.test.ts`）**——新增六个用例：渲染目录中的每一种类型都能保留；未知类型被丢弃并上报；指向被丢弃节点的连线被移除、可渲染节点之间的连线保留；残缺的节点记录被丢弃且不会让解析失败；旧类型在判断前完成升级；无法解析的图不上报任何丢弃。第一个用例把期望的类型**逐个写出**，而不是从 `WORKFLOW_NODE_KINDS` 派生——否则清单被误删时测试会跟着一起缩水。已通过删掉三种类型观察其失败来验证。
- **行为测试（`packages/app-shell/src/features/workflow-editor/workflow-editor.test.tsx`）**——携带 `router` 节点的草稿能打开画布、不渲染该节点、并弹出提示。把过滤逻辑临时关掉后重跑该用例会让 vitest worker 崩溃，证明它在未修复的代码上确实失败。

## 后果与取舍

- **被丢弃的节点会在下次保存时从草稿中消失。** 自动保存写入的是归一化后的图，本版本画不出来的节点不会被带过去。已发布快照保留原始字节，回滚或重新导入即可恢复。这是"原样存储、保存时归一化"的必然结果：另一种做法——保留任何组件都渲染不了的节点——会把问题推给图的每一个消费者。
- **这不是错误边界。** 加载路径已经修好，未知类型不会再让页面变白，但其他位置未预料的渲染错误仍然会。加错误边界仍是待办。
- **未知类型只上报、不解释。** 外来类型不会被自动映射到某个受支持类型。当包的意图明确时，应当从源头修正包的文档。
- **类型清单是与渲染目录的契约。** `createMockWorkflowNodeType` 有显式返回类型和穷尽 `switch`，因此"联合类型加了成员却没加 case"会被 TypeScript 直接拒绝。编译器看不见的是**数据里带着联合类型之外的类型**，那正是这个边界处理的情况。

## 相关文档

- [工作流](workflow.zh.md)——图存储与生命周期。
- [工作流插件（orax）导入](workflow-orax-import.zh.md)——把图 JSON 送进工作流库的打包约定。
