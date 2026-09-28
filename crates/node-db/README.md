# node-db

[English](README.en.md)

SQLite 是 Node 本地恢复日志，不是 Cloud 业务权威。schema v6 记录控制绑定及强制围栏模式。关闭标记在同代次不可撤回；更高代次不能覆盖未完成责任，未知旧未绑定执行不能被空绑定接管。同一运行时冲突 clone 互斥；保留稳定 operation/execution 和进程证据。

## 验证与边界

模块测试覆盖真实协议、恢复日志、Git 或 TLS 的所属边界；cloud fixture 及回环程序不证明生产多人授权。真实部署用 cluster Compose，契约源为 third_party/cloud 固定提交 e48cc41，生成文件不手改。完整验收与缺口见根 [运行时控制说明](../../docs/runtime-control.zh.md)。
