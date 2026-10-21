# ora-controller

[English](README.en.md)

Cloud 模式只拨出到认证 gRPC，不持有 Cloud 业务 SQLite。每次新 clone/effect 派发重新取得 PostgreSQL 短期许可；关闭资格后只查询原执行。绑定确认、普通推进与独立强停分别恢复，所有结果保留稳定身份。

Substrate effect 在 journal 查询之后、实际 PUT 之前再次检查许可的 Controller epoch、有效期和当前本地租约。本地派发要求许可剩余有效期超过 2 秒，与 Sandbox Server 一致。生命周期 effect 在同一 epoch 内遇到许可过期时进入 RetryWait，下次尝试重新获取许可并沿原 effect 继续；epoch 不符时结束本轮，不写入 defer 状态。强停逐一交付 Cloud 提供的目标，单个目标失败不阻断其他目标；Cloud 不可用或租约丢失时停止派发。新 terminate PUT 前尽力关闭已有 Node 控制连接，沿用有界超时，不创建未经 Cloud 授权的 EndSession。只有 Substrate 的 `terminated` 和 `lateEnsureFenced` 双确认可作为终止证据；`sandbox_terminated` 墓碑拒绝使 ensure 保持 Blocked，不重发或另建代次。

## 验证与边界

模块测试覆盖真实协议、恢复日志、Git 或 TLS 的所属边界；cloud fixture 及回环程序不证明生产多人授权。真实部署用 cluster Compose，契约源为 third_party/cloud 固定提交 e48cc41，生成文件不手改。完整验收与缺口见根 [运行时控制说明](../../docs/controller/runtime-control.zh.md)。

会话派发、Thread 中继与命令投递见 [Agent 会话中继](../../docs/controller/agent-relay.zh.md)。
