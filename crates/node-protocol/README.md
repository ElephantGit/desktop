# node-protocol

[English](README.en.md)

RuntimeControl 能力代表用户控制约束；单连接和心跳不能替代它。RuntimeBinding 显式区分用户代次、Controller 代次、运行时代次、Node incarnation、业务 operation、Node operation 和 execution。Cloud 旧组件不支持时拒绝，不忽略字段。

## 验证与边界

模块测试覆盖真实协议、恢复日志、Git 或 TLS 的所属边界；cloud fixture 及回环程序不证明生产多人授权。真实部署用 cluster Compose，契约源为 third_party/cloud 固定提交 e48cc41，生成文件不手改。完整验收与缺口见根 [运行时控制说明](../../docs/runtime-control.zh.md)。
