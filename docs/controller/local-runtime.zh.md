# 本机 Controller 运行时

[English](local-runtime.md) | 中文

`ora-controller` 负责本机 clone 意图的持久接受与结果接管，其可执行入口同时承载
[minicloud](../minicloud/runtime.zh.md) 调用的过渡 clone API。它不执行 Git、不替换 Backend 写入入口，
也不充当 Cloud 权威存储。Linux 会话使用现有 [Node IPC](../node/local-ipc.zh.md)
和长度前缀 JSON 消息，不增加应用凭据。

## 接受与存储

协调逻辑只通过 `CoordinationStore` 接口读写持久状态：接口按完整原子业务操作定义
（`accept_request`、`take_over_node_event`、`record_queried_result`、`original_dispatch`、
`pending_dispatches`、`result`、`operations`、`operation`），异步形态，不暴露事务、连接或表。本机唯一实现是
`SqliteStore::open(home, controller_id)`；它的每个操作在 blocking pool 上执行，SQLite 的 fsync 不占用
承载 Node 会话与 API 的异步运行时。云端部署将以 Cloud RPC 适配器实现同一接口，见
[Controller–Cloud 契约](../protocols/controller-cloud-contract.zh.md)；适配器在部署期选定，不互为后备。

`accept_request(request_id, spec)` 返回的命令包含稳定 operation／execution。完整输入及目标 Node 落盘后
才返回；相同请求返回原命令，改变输入则拒绝。`result(execution_id)` 查询持久终态，没有结果不表示失败。
`pending_dispatches(node)` 只列出尚无持久结果的执行：它们是重连后会话周期查询的对象；已完成执行
重放的事件仍经 `original_dispatch` 校验。

显式注入的私有目录保存 `ora-controller.sqlite3`，与 Node／process 状态独立。
application ID 为 `0x4f524143`、schema version 为 1；精确结构／完整性校验和同级文件
`ora-controller.sqlite3.lock` 上的 OS 租约保护重开。租约放在数据库旁边，避免在 macOS 或 Windows 上
与 SQLite 自身的文件锁冲突。
不同 ControllerId 或未知已有文件会被拒绝。不从 HOME 推导目录，不清库，不导入历史任务或自动重绑定。

`clone_operations` 保存接受记录和不可变终态，`clone_receipts` 保存 Node 原事件精确身份与内容。
查询完成和事件交付使用同一接管事务；`take_over` 只对实际收到的事件、且在 `take_over_node_event`
返回后才构造 Ack，查询结果经 `record_queried_result` 保存但不产生 Ack 依据。
相同内容幂等，冲突输入／结果／请求关联不确认。历史结果保留原 Node incarnation，
查询报告者和心跳则必须匹配当前会话。

## 独立可执行入口

`ControllerRuntime::open(RuntimeConfig)` 支持内嵌。`handle()` 提供持久 clone 接受、操作列表和查询；
`run(shutdown)` 拥有重连循环，不安装进程信号。查询不存在与操作已接受但尚无终态明确区分。
库本身不依赖任何监听器；`Service::start(DeploymentConfig, Transport, NodeHosting)` 为可执行入口和测试
组合 API 监听、唯一运行时所有者以及可选托管的 Node。

构建 `cargo build -p ora-controller -p ora-node -p ora-process-host -p ora-process-guardian`。
部署状态放在一个配置文件里，本次进程的组合方式由命令行给出：

```text
ora-controller --config /absolute/path/controller.json [--single-node]
               [--transport tcp|unix] [--host 127.0.0.1] [--port 4820] [--socket /path/api.sock]
```

| 参数 | 规则 |
|---|---|
| `--transport tcp`（默认） | `--host` 默认 `127.0.0.1`，`--port` 默认 `4820`。非回环地址允许启动但会记录警告：API 没有认证，回环只是部署约束而不是安全保证。 |
| `--transport unix` | 需要 `--socket`，必须是直接位于 `home_directory` 内的绝对路径，按与 Node endpoint 相同的私有 socket 规则创建；不接受 `--host`／`--port`。 |
| `--single-node` | 按 `single_node` 段启动配置的 Node，正常关停时停止它，见下文。 |

非法参数组合与配置都在获取数据库租约前拒绝。

`persistence` 在部署期选定持久适配器：`{ "kind": "sqlite" }` 用 `home_directory` 内的 SQLite 与文件租约；
`{ "kind": "cloud", "endpoint": "..." }` 让每个持久操作成为对 Cloud 内部控制契约的调用，不在本机开库。
运行中不切换，两者互不为后备；当前构建尚未提供 Cloud 适配器，`cloud` 在打开任何本机状态之前被拒绝。

```json
{
  "controller": {
    "home_directory": "/home/node/controller",
    "persistence": { "kind": "sqlite" },
    "controller_id": "deployment-controller",
    "protected_state_directories": ["/home/node/state", "/home/node/process"],
    "nodes": [
      {
        "node_id": "deployment-node",
        "endpoint": "/home/node/state/control.sock"
      }
    ],
    "session": { "io_timeout_ms": 10000, "query_interval_ms": 1000 },
    "reconnect_ms": 1000,
    "timezone": "Asia/Shanghai"
  },
  "api": { "node_id": "deployment-node" },
  "single_node": {
    "node_executable": "/opt/ora/bin/ora-node",
    "node_config": "/home/node/config/node.json",
    "ready_timeout_ms": 30000,
    "stop_timeout_ms": 30000
  }
}
```

`api.node_id` 指定已接受 clone 派发到的 Node，调用方不选择 Node。`protected_state_directories`
须列出所有 Node／host／guardian 状态根；配置的 endpoint 父目录也受保护。Controller 数据目录与它们
重叠时，在开库前拒绝。独立程序恢复已接受记录，配置文件和 stdin 不是业务命令通道。
不托管 Node 时分别部署 host 和 Node，Node 配置的归属须匹配 ControllerId。

`--single-node` 要求 `nodes` 恰好包含 `api.node_id` 这一个 Node。开库前，程序只读读取 `node_config`，
其 `ipc.controller_id` 或 `ipc.endpoint` 不匹配、或 endpoint 上已有进程接受连接时拒绝启动。随后在
自身进程组内（不新建会话）启动 `node_executable <node_config>`，在 `ready_timeout_ms` 内等待 endpoint
可连接，然后才绑定 API。process host 与 guardian 是前置条件，程序不部署也不启动它们。Controller
单独退出不会向 Node 发送任何信号，已接受的 clone 继续执行；运维或启动器按进程组停止时两者都会收到。
托管的 Node 自行退出时，Controller 关停并以失败退出，而不是继续受理无法派发的请求。

正常关停顺序固定为：API 受理（有限等待在途请求）→ Node 会话 → 托管 Node（`SIGTERM`，最多等待
`stop_timeout_ms`，不升级为 `SIGKILL`）→ 数据库租约。本进程停止从不取消 Node 已接受的执行。

JSON 接口是 [minicloud](../minicloud/runtime.zh.md#http-接口) 文档描述的过渡 clone API，
DTO 位于 `ora-contracts::controller_api`。面向 Cloud 的契约由 Cloud 仓库的 proto 定义，Controller 作为
客户端拨出（见 [Controller–Cloud 契约](../protocols/controller-cloud-contract.zh.md)）；该监听器只服务
JSON 接口与本机调用方。

## 验证与保留范围

真实 SQLite 测试经 `CoordinationStore` 接口覆盖接受、独占、事务失败、查询／事件乱序、重复接管和冲突事实；
framed 会话测试覆盖 Unknown 重传有界，以及错误 Node 身份或缺少 clone 能力时在派发前拒绝。
独立 Controller–Node–host／guardian 测试执行真实 HTTPS clone，
截住 Ack 后在持久接管之后强杀 Controller，再离线重启，检查原结果、精确 Ack、Node outbox 清空和唯一变更 Run。
Node 自身 IPC 测试另覆盖 Node 重启与事件重放。

另有独立子进程运行生产 `run_session` 与真实 SQLite 所有者，仅注入提交前暂停点。
父进程确认没有 Ack，在接管事务仍打开时发送 SIGKILL，再重开数据库验证回滚及原意图不变。
随后由正常 Controller 可执行程序在 HTTPS 拒绝访问时接管 Node 重放的结果。
暂停点是持久化测试依赖（`WritePoint::Commit`），不是部署选项或协议扩展。

这不代表 Client／UI、Cloud、多 Controller 或恶意对端保证完成；全部队列压力、崩溃边界和部署组合
仍在 approved ADR 核心用例中跟踪。既有 Backend 入口及 Worktree 协调保持不变。
