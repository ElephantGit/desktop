# Controller 沙盒路由

[English](sandbox-router.md) | 中文

Cloud Controller 通过 `substrate.router_url` 连接所有沙盒 Node，以
`Ate-Target-Actor: <atespace>/<sandboxInstanceId>` 选择目标，不再根据 ID 推导 Node 直连地址。
部署配置需要移除旧 `direct_node_port` 字段；未知配置字段会被拒绝。

Cloud 可执行文件要求 HTTPS effects 和 WSS router。部署的管理 TLS 文件用于向 router 认证
Controller，router 的证书须匹配入口 URL。平台中继再独立连接 Node，因此不是 Controller–Node
端到端 TLS。hello 中的 Node 身份校验、Cloud 许可、目标绑定和运行时 fencing 保持不变。

Controller 与 Node 在联调时允许同网；同网可达不改变应用连接路径，也不启用直连 fallback。
Controller 不需要解析 Node 域名或访问其内部 IP。本机 cluster 与真实 sandbox adapter 提供
相同的目标路由行为；生产认证策略另由部署定义。

Controller 配置和 cluster Sandbox Server 需要协调升级。新 cluster Node 固定接受中继证书，
旧沙盒应先经正常生命周期收尾，再创建新实例。保留 Workspace 数据，不改 journal，也不放宽
证书检查来复用旧实例。协调升级步骤和验证方法见 cluster 的 `docs/sandbox-routing.md`。
