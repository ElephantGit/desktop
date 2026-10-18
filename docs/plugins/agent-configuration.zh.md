# Agent 配置声明

[English](agent-configuration.md) | 中文

`ora-plugin-config::compile_agent_configuration` 校验不可变 Agent 声明，不读取用户配置、启动进程
或发送网络请求。这是独立编译入口。安装配置分发器也识别该形态，包校验会为 `agent` kind 编译它；
本地配置存储及其编辑器服务 v1 形态。编译成功不等于安装成功、获得授权或运行就绪。

## 输入

```json
{
  "schemaVersion": 2,
  "agent": {
    "modelAccess": {
      "contractVersion": 1,
      "protocols": ["anthropic-messages"],
      "modelCardinality": "single"
    }
  },
  "settings": {
    "verbose": {
      "type": "boolean",
      "title": "Verbose output",
      "description": "Include additional output detail",
      "default": false
    }
  }
}
```

- `schemaVersion` 必须为 `2`，模型访问的 `contractVersion` 必须为 `1`。
- 协议列表非空且不重复，精确名称为 `openai-completions`、`anthropic-messages`。`openai` 这类
  provider 名称和 `openai-responses` 等未支持协议会被拒绝，不会被归一为另一协议。
- `modelCardinality` 必须为 `single`，声明不能授予第二个模型的调用权。
- `settings` 可以省略；出现时必须是非空的现有标量 Setting 声明对象，显式 `null` 不算省略。
  类型仍为 string、number、boolean。
- 连接选择、API Key、运行认证头和上游 URL 不属于 `agent.modelAccess`，各层未知成员均拒绝。
  Agent 声明不能混入 MCP `transport` 或 Hook contribution。
- 复用源文件 256 KiB、最多 128 个 Setting、Setting ID、文本长度、必填标志、顺序及默认值类型
  规则。重复 JSON 键在反序列化折叠之前就被拒绝。

这里的 Settings 是作者声明的元数据，不是秘密存储或运行表达式语言。编译器不推断任意文本是否为
凭据；凭据交付及受支持非秘密选项的解释属于另行授权的消费端。

## 编译结果、身份与错误

结果包含定型模型能力与确定排序的 Setting 声明。`fingerprint` 是原始输入字节的 SHA-256 小写
十六进制；空白变化也改变身份，发布副本与包核对前不能重新序列化。既有 v1 声明指纹保持原来的
规范化语义。

错误区分超限、非法 JSON、重复键、结构/Settings 无效、schema/contract 版本不支持，以及协议
列表为空、重复或未支持。错误文本不包含被拒字符串或源片段。编译不证明来源 namespace、发行
版本/target、包摘要、包内路径、个人归属或实际模型访问能力。

## 契约样例

[共享样例](../../crates/plugin-config/tests/fixtures/agent-configuration-contract.json) 保存字面的
`source` 字符串以及编译结果或错误分类，LF/CRLF checkout 不改变 source 字节，消费端可以跨平台
比对同一声明的身份。

## 发行关联与安装核对

市场 release 清单仅在 release 形态携带 `[agent_configuration]` 发行关联：`schema_version`、
`declaration_sha256`（该 release 每个包必须包含的原始声明字节 SHA-256）和 `copy`（静态声明
副本在条目内的相对路径）。只有声明了 release 来源的 `agent` 清单可以携带；installed 形态或
section 内的未知键都会被拒绝。副本路径由 `/` 分隔的 ASCII 段组成，每段匹配
`[A-Za-z0-9][A-Za-z0-9._-]*`；空段、父目录段、反斜杠、转义和绝对路径均被拒绝。

两条安装路径都在提交安装前用包自身字节核对关联：市场路径从清单取期望值，Node 路径接收安装
计划冻结的关联。Controller 把 Cloud 契约映射为 Node 协议时，原样保留计划中的声明版本和摘要。
包内 `assets/config.json` 必须编译到关联的 schema 版本且原始字节摘要一致，否则安装器在 staging
阶段拒绝包。Agent 包可以携带没有发行关联的惰性声明元数据；声明本身不授予个人配置或模型访问权。

## 决策

- [Agent 配置声明](https://github.com/ora-space/specs/blob/main/decisions/desktop/plugin/manifest/20261017-agent-configuration-declarations.md)
