# Node 插件

[English](plugins.md) | 中文

在 Cloud 模式下，Cloud 冻结一个 Workspace 的插件计划，Controller 原样派发，Node 在自己的 home 中安装或删除插件。Controller
既不解析市场版本，也不创建插件的 Substrate effect。相关决策是
[插件安装作为 Node 执行](https://github.com/ora-space/specs/blob/main/decisions/controller/plugin/0-plugin-installs-as-node-executions.md)、
[Node 把插件安装到 Workspace home](https://github.com/ora-space/specs/blob/main/decisions/node/plugin-runtime/0-node-installs-plugins-into-workspace-home.md)
和[有界的部署下载超时](https://github.com/ora-space/specs/blob/main/decisions/node/plugin-runtime/20261010-bounded-deployment-download-timeout.md)。

## 派发

Node 必须声明 `plugin_install`。Cloud 部署发送带 `InstallPlugins` 或 `RemovePlugins` 以及执行许可的 `ControlledPlugins`；
Node 在受理时和开始执行时各检查一次授权，持久化原始输入，并在单独的线程上下载和解压，所以心跳和查询保持响应。只要该 home
从未启用过运行时控制，私有本地 IPC 也接受裸命令。Cloud 接受终止证据（包括单项失败）之后，Controller 推进 Workspace 操作的
`plugin` 步骤；整个执行失败时，以 `PLUGIN_EXECUTION_FAILED` 推迟该步骤以便重试。

## 安装

包放在 `node.home_directory` 下的 `plugins/installed/<namespace>/<name>/<version>/`。下载和解压使用发现范围之外的
`plugins/.node-installs/install-*`。安装复用 `ora-plugin-manager` 的校验，并在发布之前检查计划中的 SHA-256、身份、版本和
目标平台。它从不启动插件代码。

- 已经安装了有效的精确版本时直接成功，不下载。升级会撤下旧版本。删除不存在的插件也算成功，删除保留插件数据和配置。已存在的
  符号链接目录会被拒绝。
- 每次尝试有时间限制（默认 60 秒），最多三次尝试以应对暂时性失败；建立连接限制在 10 秒内，制品限制在 512 MiB。失败的尝试会
  从头开始传输。
- 对慢速链路，可以用 `"plugins": { "download_timeout_seconds": 600 }` 提高每次尝试的上限（10 到 1200 的整数；无效值让启动
  失败）。总预算是上限的三倍再加 70 秒。远程命令不能选择这个策略，它也不改变 TLS 校验以及冻结的 URL 和 SHA-256 检查。

`PluginInstaller::catalog()` 提供 [Agent 会话](agent-sessions.zh.md)共享的 `DirectoryPluginCatalog`：会话先获取使用租约，
再解析精确版本；只要有会话持有某个插件的租约，替换或删除它就会返回 `plugin_in_use`。

## 持久化与恢复

插件执行和它的 outbox 保存在 Node 数据库中（schema v7）。以不同的输入复用同一个身份会被拒绝。终止结果和它唯一的序号 1
事件原子地提交；查询和重复的命令不消耗任何东西，只有精确的 ACK 才清除事件，结果仍可查询。Controller 在确认事件之前先提交
Cloud 的接管，从不确认只从查询得知的证据。

启动时删除本执行器的临时目录。未设栅栏的本地未完成输入可以重新运行。Cloud 中的 Node 重启后，旧 incarnation 的许可失效，所以
保留的未完成执行以 `interrupted` 结算，Cloud 以新的授权登记新的执行。已完成但未确认的事件以原来的身份重放。
