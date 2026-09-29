# Dify 循环终止条件参考

[English](dify-loop-termination-reference.md) | 中文

调研日期：2026-09-24。范围：Ora 本次需要提供的循环终止条件编辑器；本文记录参考依据，不代表承诺完全兼容 Dify 循环行为。

## 一手来源结论

- Dify 文档列出三种终止方式：条件满足、达到最大次数、执行退出循环节点。允许不配置条件，此时由最大次数限制执行。[官方循环文档](https://docs.dify.ai/en/cloud/use-dify/nodes/loop)
- 面板分别显示循环变量、循环终止条件、最大循环次数。每条条件由变量、运算符、比较值组成，支持新增和删除；多条条件显示 AND/OR 切换。默认条件列表为空，组合方式为 AND。[面板](https://github.com/langgenius/dify/blob/7feebe3405094482e42dcbdf407cbe17c262cdd9/web/app/components/workflow/nodes/loop/panel.tsx)、[条件列表](https://github.com/langgenius/dify/blob/7feebe3405094482e42dcbdf407cbe17c262cdd9/web/app/components/workflow/nodes/loop/components/condition-list/index.tsx)、[默认配置](https://github.com/langgenius/dify/blob/7feebe3405094482e42dcbdf407cbe17c262cdd9/web/app/components/workflow/nodes/loop/default.ts)
- 运算符随变量类型变化：字符串支持相等、包含、前后缀、空值判断；数字支持大小比较和空值判断；布尔值支持相等和空值判断。一元运算符隐藏比较值输入；数字可选常量或变量，布尔值使用专用控件。切换变量会重置运算符和比较值。[运算符定义](https://github.com/langgenius/dify/blob/7feebe3405094482e42dcbdf407cbe17c262cdd9/web/app/components/workflow/nodes/loop/utils.ts)、[条件行](https://github.com/langgenius/dify/blob/7feebe3405094482e42dcbdf407cbe17c262cdd9/web/app/components/workflow/nodes/loop/components/condition-list/condition-item.tsx)、[数字输入](https://github.com/langgenius/dify/blob/7feebe3405094482e42dcbdf407cbe17c262cdd9/web/app/components/workflow/nodes/loop/components/condition-number-input.tsx)
- 该版本 Dify 依赖 `graphon==0.7.0`。其处理器在开始执行循环体前及执行完成后检查条件，前置检查忽略 `ValueError`；空条件列表不会命中。达到次数上限按成功结束，并使用不同于条件命中的结束原因。[依赖声明](https://github.com/langgenius/dify/blob/7feebe3405094482e42dcbdf407cbe17c262cdd9/api/pyproject.toml)、[Graphon v0.7.0 处理器](https://github.com/langgenius/graphon/blob/11e2dee8cbd6dc2e6bf1c2059d9bbf4d0437ebe5/src/graphon/graph_engine/loop_container_handler.py)

## Ora 建议范围

参考条件行交互：选择可用变量、选择受支持的运算符、输入对应类型的常量或在支持时选择变量，支持增删条件，以及全部满足／任一满足（AND/OR）。隐藏不需要的比较值输入，保留最大次数设置并说明其行为。复用 Ora 现有条件契约、求值器和变量作用域规则。

Ora 当前要求 `until` 非空，每轮结束后求值，达到上限但未命中条件时失败，这些语义与 Dify 不同。仅增加编辑器不应隐式改变已保存工作流，也不应声称完全兼容；可选条件、达到上限成功结束、循环体前置检查和独立退出循环节点需要单独的运行时决策及行为验证。参见 [Ora 工作流行为](workflow.zh.md)。
