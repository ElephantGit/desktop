# minicloud 本机运行时

[English](runtime.md) | 中文

Linux HTTP 程序内嵌与 `ora-controller` 相同的运行时。先按既有
[部署配置](../node/repository-clone.zh.md)启动 Node 和 host／guardian，
再构建 `cargo build -p ora-minicloud-server`，运行 `ora-minicloud-server /absolute/path/minicloud.json`：

```json
{
  "listen": "127.0.0.1:4317",
  "node_id": "deployment-node",
  "controller": {
    "home_directory": "/home/node/controller",
    "protected_state_directories": ["/home/node/state", "/home/node/process"],
    "controller_id": "deployment-controller",
    "nodes": [
      {
        "node_id": "deployment-node",
        "endpoint": "/home/node/state/control.sock"
      }
    ],
    "session": { "io_timeout_ms": 10000, "query_interval_ms": 1000 },
    "reconnect_ms": 1000,
    "timezone": "Asia/Shanghai"
  }
}
```

Node 配置的归属必须匹配 ControllerId。启动 minicloud 前，停止使用同一状态目录的独立 Controller。
各状态目录继续显式注入并拒绝重叠；非回环监听或未配置的目标 Node 会在打开 Controller 状态前拒绝。
这是非生产应用，不增加认证或额外安全体系。

## HTTP 接口

先运行 `deno install`，再从仓库根执行 `deno task --filter @ora/minicloud-client dev`，
打开 `http://127.0.0.1:5174`。Vite 将 `/api` 代理到 `http://127.0.0.1:4317`；
不同 server 端口可通过 `MINICLOUD_SERVER_URL` 配置。
页面使用共享 shadcn 组件及 React 19，轮询 Controller 操作。
提交前把未确认请求写入当前标签页 session storage；回复丢失或刷新后，“重试原请求”复用原身份与输入。
这不是操作历史数据库。关闭页面终止 HTTP 和轮询，不取消 Node 执行。

- `POST /api/clones`：`{ "requestId": "stable-client-id", "repository": "https://host/repo.git", "branch": "main" }`。
  持久接受后返回 HTTP 202，包含 `requestId`、`operationId`、`executionId`。
- `GET /api/clones`：按接受顺序倒序列出操作，Node 离线时仍能读取待协调记录。
- `GET /api/clones/{executionId}`：读取单个操作；404 表示没有该身份的接受记录。

状态为 `pending`、`succeeded`（Node 路径及 commit）、`failed`（明确原因及残留路径）。
pending 不声明 Git 是否正在运行。HTTP 400 表示输入无效，409 表示身份／输入冲突，503 表示暂时不可用；
HTTP 错误不是 clone 终态。浏览器 DTO 从 `ora-contracts::minicloud` 生成，与 Node 消息协议分开。

未确认接受结果时使用相同请求身份和输入重传。关闭浏览器或停止 server 不取消 Node 执行；
重启使用原状态目录恢复事实。不另建 minicloud 任务数据库、不提供清理命令或自动新执行重试。

真实 HTTP 测试覆盖接受、冲突、无效输入、离线列表、不存在的执行身份、独占、回环限制及正常重启。
下层 Controller 强杀测试与 minicloud 自身端到端证据分别记录。

前端检查：`deno task --filter @ora/minicloud-client lint`、`test`、`build`。
`task test:minicloud` 另验证真实 HTTP 及 Vite proxy → 独立 minicloud server → Node → HTTPS Git，
包含 server SIGKILL／重启和唯一变更 Run；要求 Linux 且已安装前端依赖。
Vite 用例在纯 Rust CI 中显式跳过，通过专项任务执行。
真实链路还通过 SQLite writer lock 注入接受写失败，验证 HTTP 503 且没有接受记录，释放后仍只有一个 Run。
真实 TCP 代理截断接受响应 body，server 重启后重传仍保留原执行；clone 期间正常关闭 server，
固定身份的 Git 进程仍存活，最终返回同一结果且只有一个 Run。server 入口验证重叠目录拒绝、
未知文件保留，并复用独立 Controller 所有者接受的记录。
DOM 测试验证页面行为、轮询自动恢复及刷新身份恢复；完整浏览器引擎交互验收明确不在本次范围内。
