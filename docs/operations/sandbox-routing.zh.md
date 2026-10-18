# 沙盒路由

[English](sandbox-routing.md) | 中文

cloud 形态的 Controller 通过 `substrate.router_url` 访问每个沙盒 Node，用 header `ate-target-actor: <atespace>/<sandboxInstanceId>`
选择目标；它从不从 id 推导出直接的 Node 地址，也不需要 Node 的 DNS 名称或 IP。未知的部署字段会被拒绝，所以过时的字段必须从配置
中删除。

- 可执行文件要求 HTTPS 的 effects 和 WSS 的路由器。Controller 的管理 TLS 文件用于向路由器认证自己，路由器的证书必须与路由器 URL
  匹配。
- 平台中继另外连接 Node 的双向 TLS 监听，Node 固定中继的证书。这不是端到端的 Controller–Node TLS：握手中的 Node 身份校验、
  Cloud 许可、目标绑定和运行时栅栏仍然有效，见[运行时控制](../controller/runtime-control.zh.md)。
- 集成测试中 Controller 和 Node 可以共享一个网络；共享的可达性既不改变配置的路由，也不启用任何后备路径。
- 本地集群适配器和真实的沙盒适配器暴露同样的目标路由；它们在生产中的认证策略属于部署的范畴。

Controller 配置和 Sandbox Server 要一起升级。因为 Node 固定了中继证书，在创建替代实例之前，先通过正常的生命周期操作排空旧沙盒；
保留 Workspace 数据，永远不要为了复用旧实例而改写日志或放松证书检查。协调升级的步骤见 cluster 仓库的 `docs/sandbox-routing.md`。
