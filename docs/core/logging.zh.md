# 日志

[English](logging.md) | 中文

Desktop、Controller 和 Node 通过 `ora-logging` 写结构化日志。插件进程例外：插件自己的诊断写入由宿主持久化的独立插件日志，见[插件日志](../plugins/plugin-logging.zh.md)。

## 职责划分

- `ora-logging` 负责进程级 subscriber、JSON 事件格式、sink 选择、文件轮转、保留期清理和不可变的进程时区。
- 组装根读取配置，以显式的 `LoggingConfig` 调用 `ora_logging::init_logging`，在进程生命周期内持有返回的 `LoggingGuard`，并把可克隆的 `LogLevelControl` 与偏好存储组合起来。过早丢弃 guard 会丢失缓冲中的输出。stdout 和文件 sink 不丢数据：写入通道满时对调用方施加背压，而不是丢弃日志行。
- 请求入口和基础设施 crate 只发出 `tracing` 事件，从不配置 sink 或读取环境变量。handler 和仓储不发通用的完成事件，也不为单纯传播而记录失败事件。

初始化是进程级的，时区只能设置一次，所以必须在访问任何 `ora_logging::clock` 之前完成。无法创建的文件 sink 会以类型化的 `LoggingInitError` 让初始化失败，而不是退回到别的 sink。

## Desktop 配置

Desktop 在打开存储之前先以 `info` 初始化临时日志，然后恢复 `user_config.log_level`；未设置的偏好同样表示 `info`。存储读取失败或值格式错误会中止启动。不读取 `ORA_LOG_LEVEL`，也没有启动时的覆盖方式。应用内设置及其持久化偏好是唯一的配置入口；编译期日志上限不变。

`ora-runtime-settings` 管理器串行化实时的级别变更：先重载进程过滤器再持久化，持久化失败时回滚过滤器，即使发起请求的 Tauri future 被取消，已开始的提交或补偿也会完成。见[设置](../desktop/settings.zh.md)。

| 项目 | 取值 |
|---|---|
| 文件 | `app_data_dir/logs/ora.log` |
| 轮转 | 每天一个文件，保留三个（文件在 UTC 边界轮转） |
| stdout | 仅 debug 构建 |
| 时区 | 启动时读取的操作系统 IANA 时区，在进程内固定；读取失败时使用 UTC 并记录警告 |

带有 request id 的错误提示提供"下载日志"，开发者模式开启时 设置 → 开发者选项 里也有同样的导出。Desktop 把当天的日志复制到原生保存对话框中选定的位置，不暴露应用数据路径。

## JSON 事件契约

每个 sink 每行写一个 JSON 对象，包含 `timestamp`、`level`、`target` 和 `message`。`method`、`span`、`trace_id` 和 `request_id` 只在附加时出现。业务元数据放在 `context` 下，失败细节放在 `error` 下。按字段前缀路由：`error.kind` 进入 `error`，`context.operation` 进入 `context`，其他无法识别的字段都进入 `context`。空的 `context` 和 `error` 对象会省略。

```json
{
  "timestamp": "2026-05-09T20:00:00+08:00",
  "level": "INFO",
  "target": "ora_backend::request_lifecycle",
  "message": "request completed",
  "method": "complete_success",
  "request_id": "550e8400-e29b-41d4-a716-446655440000",
  "context": { "operation": "create_project", "outcome": "success", "duration_ms": 12 }
}
```

RFC 3339 时间戳使用进程时区并带偏移量。当本地日期与文件后缀不一致时，以事件时间戳为准。

## 请求完成

前端请求在 Tauri 或流入口获得规范的 UUID v4；客户端提供的 id 从不作为规范 id。同一个 id 关联请求 span、公开错误载荷或流错误帧，以及完成事件。

每个请求恰好记录一条完成事件，包含 `operation`、`request_id`、`outcome` 和 `duration_ms`。失败时再加上公开的 `error.code`，以及来自 `ErrorReport::from_error` 的 `error.message`、`error.chain` 和 `error.chain_depth`。

| 分类 | 级别 |
|---|---|
| `Internal` | `ERROR` |
| `Conflict`、`Forbidden`、`HostUnavailable` | `WARN` |
| `InvalidRequest`、`NotFound`、`PayloadTooLarge`、`Unprocessable` | `INFO` |
| 取消、成功的健康检查和就绪检查 | `DEBUG` |

`Forbidden`（宿主拒绝访问，例如文件不可读）高于 `INFO`，避免被淹没在成功请求中；它也不放进 `Conflict`，好让后者保持"状态争用"的含义。`HostUnavailable`（宿主环境没有完成委托给它的操作，例如打开一个未安装的编辑器）不进入 `ERROR`。路由任务时遇到的存储或 Git 故障属于 `Internal`，绝不是 `Conflict`；只有 Git 确认不存在的 worktree 才是真正的冲突。

长生命周期的流把完成推迟到流结束、报告错误帧、被取消或丢失 channel 时；channel 被丢弃时以 `cancelled` 完成。终止帧在发送之前就认领完成，所以即使那一帧没有到达，记录也描述了 backend 流是如何结束的。`RequestLifecycle` 在结构上保证有收尾记录：最后一个句柄在没有显式完成的情况下被丢弃时，会在 `DEBUG` 级别发出 `outcome = "abandoned"`。

`ErrorReport` 遍历 `Error::source()` 链，在 debug 和 release 构建中都保留原始的链文本；遍历在 1,024 个节点处停止。清洗器和脱敏规则只供显式使用，`ErrorReport::from_error` 不会调用它们，所以调用方只能附加经过批准的字段：绝对路径、完整的 Git 参数或远程地址、SQL 值、prompt、环境变量值、凭据和不限长的 stderr 都不是合法的日志字段。

持久化的后台工作独立于触发它的请求记录日志。例如 Git 清理以 `operation = "git_cleanup"` 记录事件，带有自己的重试状态，所以后续清理的结果永远不会改变原请求的错误。启动、迁移和状态转换事件同样是独立的生命周期事实。

## 发出日志的辅助宏

使用 `ora_trace!`、`ora_debug!`、`ora_info!`、`ora_warn!` 和 `ora_error!`，不要直接用 `tracing` 宏；它们会把当前函数作为 `method` 附加上去。`runtime_span`、`span_with_correlation`、`span_with_trace_id` 和 `span_with_request_id` 创建的 span 会把保留字段传递给嵌套事件（显式的事件字段优先）。使用 `ora_logging::clock::now_local`，而不是 `OffsetDateTime::now_local()`。

## Git 命令日志

`gitlancer` 保持与框架无关：它定义 `GitlancerLogger` 和只能写一次的 `logging::register`；没有注册 logger 时，调用点没有任何副作用。`CliGitRunner::run` 在启动进程前调用 `log_command`，在进程退出后以耗时、退出码和是否成功调用 `log_result`（启动失败以零耗时报告失败）。

`ora-logging` 提供桥接实现 `OraGitlancerLogger`；Desktop 在 `init_logging` 之后立即用 `register_gitlancer_logger()` 安装它。它把字母组成的 Git 子命令记录为 `operation`；对于 `git config`，还记录识别出的动作、作用域和安全的 `user.*` key，同时丢弃 cwd 和其他所有参数（提交信息、路径、远程地址、凭据）。结果只带 `duration_ms` 和 `exit_code`。

## 部署边界

本地 JSONL 和 Logdy（`logdy.config.json`）是本地查看工具。生产环境的日志收集（OTLP、Loki、Tempo、Grafana）属于部署，见 cluster 仓库。

## 测试

`with_trace_logging` 和 `with_recorded_trace_logging` 安装线程作用域的 `TRACE` dispatcher。对结构化输出做断言的测试要用它们；只是碰到同一批调用点的普通测试也要用，因为 `tracing` 会缓存调用点的 interest。Desktop 启动日志在独立的子进程里测试，因为 `init_logging` 拥有进程全局的时钟和 subscriber。
