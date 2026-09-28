# node-transport

[English](README.en.md)

网络管理通道通过部署 CA、双方证书及可信 Controller 叶证书固定建立身份。TLS 失败没有明文回退。WebSocket 只承载消息，用户权限由 Cloud 决定并由 Node 绑定检查，传输连接本身不授予用户操作资格。

## 验证与边界

模块测试覆盖真实协议、恢复日志、Git 或 TLS 的所属边界；cloud fixture 及回环程序不证明生产多人授权。真实部署用 cluster Compose，契约源为 third_party/cloud 固定提交 e48cc41，生成文件不手改。完整验收与缺口见根 [运行时控制说明](../../docs/runtime-control.zh.md)。
