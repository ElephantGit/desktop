# 进程宿主创建意图日志

[English](process-host-state.md) | 中文

Linux `ora_process_runtime::HostState` 已负责持久化宿主创建意图与一次性
[独立 guardian 启动](process-guardian.zh.md)，不是生产宿主 app 或 Run 启动器。
它是已批准[guardian 启动决策](../specs/decisions/node/process/recovery/20260917-rootless-guardian-bootstrap-and-reconnect.md)
的部分实现，无需 root、helper、cgroup 委派或服务安装。现有 Git／插件入口及业务数据库政策不变。

## 显式定位与所有权

调用方提供绝对路径、专用的 `state_dir`；`HostState` 不读取 HOME，也不使用业务 cwd。
应用组合层应从显式注入的 Node／home 定位选择该目录；生产组合尚未接入。

- `HostState::create(&state_dir)` 要求目标目录不存在，已有空目录也拒绝。只创建
  `host.lock`、`host.sqlite`、SQLite 辅助文件及 `scopes/`。
- `HostState::recover(&state_dir)` 要求原稳定锁、数据库和 scopes 目录存在。缺文件、未知根目录条目、
  畸形身份及不兼容日志均失败，不重置状态；恢复失败后不会自动改走创建入口。
- 目录仅所有者可访问；文件必须是仅所有者可访问、只有一个硬链接的普通 inode。复用
  `ora-utils::path` 拒绝符号链接及组／其他用户可写的祖先目录，不修改已有权限。
  root 和指定 UID 仍属于可信范围，不隔离恶意同 UID 程序。
- 首批文件系统允许 ext 家族、XFS 和 Btrfs，拒绝网络、内存、overlay 及未知文件系统。
  分类本身不证明挂载选项、存储硬件或断电行为；本地测试目前只覆盖 ext 家族存储。
- 完整的规范 Scope ID 与 `control.sock` 后缀必须满足 Linux 路径型 socket 长度上限。
  超长路径拒绝，不截短、不换目录。组可写的 checkout 和 `/tmp` 不属于支持的状态父目录。

创建中断可能留下不完整的专用目录；恢复会保留并拒绝该目录，不自动修复；仅支持下述精确 v1 日志的增量迁移。
持有期间不得删除锁文件、替换目录，或删除记录来绕过失败。

## 持久事实不等于启动权限

宿主持有原 `host.lock`，直到 SQLite 连接关闭。恢复先非阻塞获取原锁，再只读检查日志兼容性，
最后才提交新的宿主实例。锁竞争返回错误，不授权替换锁或 endpoint。

版本 2 日志使用 application ID `0x4f524148` 和 `user_version=2`，检查精确 schema、完整性及
持久身份。记录正数宿主代次及宿主实例 ID，以及各 Scope 的原 guardian 实例、创建时宿主绑定和
`intent_recorded` 阶段。代次溢出拒绝；恢复不改写意图中的创建者。
已有 Scope 目录必须具有规范 ID、私有目录元数据和匹配的宿主意图。本切片不检查或管理其内部
journal 与 endpoint。

`record_scope_intent(scope)` 提交新的原始意图，或原样返回已有记录；`scope_intent(scope)` 查询
该责任。查询不存在不代表可以重建 guardian。此登记调用不创建 Scope 目录、guardian journal、启动票据、
进程、凭据或 Ready 事实。没有意图的已有 Scope 路径会阻止登记，原文件保留。

`start_guardian(scope, executable)` 要求已有意图及显式传入的可信可执行文件。
它先提交凭据和 `launch_unknown` 记录，再创建私有 Scope 目录、获取其锁并 exec guardian。
此后的错误或取消均消耗这次尝试；即使证明 exec 失败，也不能再次启动。
`guardian_access(scope)` 在宿主恢复后取回原发现材料，不启动进程，也不转移控制权。
Ready 由 `ora-process-client` 另行查询。

恢复接受精确的 v1 仅意图 schema，在同一事务中添加启动表并推进宿主身份；原意图、路径与锁 inode 不变。
旧版本不能启动 guardian，因此没有需要推断或回填的启动记录。未知 schema 拒绝；旧二进制拒绝 v2，
不会重置它。宿主恢复不写 guardian.sqlite。

日志独立启用并验证 WAL＋`synchronous=FULL`，要求实际链接的 SQLite 主线版本包含 WAL-reset 修复
（不低于 3.51.3）。新事实提交事务后才返回，并同步所在目录条目。因此，提交后的文件系统失败
可能使调用报错但记录已存在：应查询原身份，不能从错误推导“没有接受”。SQLite 的耐久语义见
[WAL](https://sqlite.org/wal.html) 和 [synchronous](https://sqlite.org/pragma.html#pragma_synchronous)
文档；物理断电耐久仍未验证。

## 验证与剩余工作

`cargo test -p ora-process-runtime --test host_state` 覆盖并发创建、锁竞争、重启去重、锁身份不变、
外部 SIGKILL 后调用者内存丢失、缺失／外来文件、路径长度、权限、链接、版本／schema／身份损坏、
代次耗尽、冲突修复及精确 v1 升级。强杀 fixture 改变 child 的 HOME 和 cwd，仍使用同一显式状态路径。
测试在测试用户 home 下建立私有临时目录；生产代码不会从该环境变量推导路径。

真实 app 的启动、拒绝、启动方强杀及发现证据见 [guardian 启动](process-guardian.zh.md)。
其中已包含持久宿主会话接管。Controller 授权、Run 接受、生产宿主组合及平台清理仍待实现；没有 ADR 被标为 implemented。
