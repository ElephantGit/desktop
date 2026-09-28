# Cloud 运行时控制交付

[English](runtime-control.md)

权威规则链接 [Cloud ADR](../specs/decisions/cloud/controller-integration/20260927-fenced-runtime-control-delivery.md)，Controller/Node 不复制租户角色政策。Cloud proto 固定 e48cc41；Controller 只拨出、无过渡 HTTP 监听，云端不另建 SQLite。

执行入口携带独立控制代次、Controller 租约、运行时代次、Node incarnation 与稳定 execution ID。Node 绑定持久后才能确认；接受及首次真实变更都检查旧资格，关闭在同代次不可恢复。Controller 新派发前取得 Cloud 短期许可；到期或断线后查询原责任，不把未知当未执行。

生产 Cloud CLI 要求 HTTPS gRPC、管理证书、HTTPS Substrate 及直接认证 Node 通道。Node 镜像 root 管理程序与 UID/GID 1000 工作负载分开；/run/ora-management 私密，代码目录由工作负载拥有。静态 clone.gitconfig 仅允许无秘密的 CA 信任设置，不接受个人 helper/SSH 回退。

升级旧 UID 1000 管理 home 不自动修复可信性；旧未完成责任阻止新绑定。需要部署停机、核对旧进程及受控迁移，不因换代次推断旧进程已死。Node 证书当前 24 小时，有效期刷新机制尚未交付；长运行部署需受控重启/轮换，不承诺无中断。此前延期的完整本机宿主信任体系仍未实施。

ora-node/examples/loopback-transport-fixture 仅用于保留原传输、真实 Git 和崩溃回归，拒绝 Cloud target 与非回环地址，不进入运行镜像。它不证明生产权限域隔离。真实 Node/Docker 验收见 cluster。云端文件/终端/Agent 产品、真实插件执行器与项目凭据提供方保持关闭。完整接受响应丢失、组件重启和升级/回滚组合仍须补齐，ADR 不自动标 implemented。
