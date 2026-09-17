# 无特权 Guardian 启动

[English](process-guardian.md) | 中文

Linux 已有真实独立的 `ora-process-guardian` app，以及 `ora-process-client` 的只读 Ready 查询和持久宿主会话绑定。
无需 root、helper 或服务安装。这是[已批准决策](../specs/decisions/node/process/recovery/20260917-rootless-guardian-bootstrap-and-reconnect.md)
的部分实现，不是 Run 持久执行或生产 Node 接入。

## 所有权与顺序

调用方向 [HostState](process-host-state.zh.md) 显式注入私有状态路径和可信 guardian 可执行文件。
可执行文件不能是 set-id；路径与凭据均不从 HOME 推导。创建意图和一次性启动记录先于 exec 提交。
子进程以空环境进入独立 session，不设置父进程死亡信号或 Drop-kill 策略；宿主先创建回收线程，再 exec。
宿主终止不终止 guardian，但外部服务管理器仍可能终止整个服务组。

命令行只有 `--bootstrap`。专用 socketpair 传递版本化 MessagePack bootstrap 帧；stdin 携带原持锁
打开文件描述，stdout 携带该 socket 而非日志。其他描述符在 exec 时关闭；子进程对继承的 bootstrap
描述符恢复 CLOEXEC。内核不支持所需 `close_range` 能力时拒绝启动。

`LinuxFileLock::adopt_inherited` 通过 procfs 验证继承描述自身的独占 flock；未加锁文件、重开的同 inode
和共享锁不能取得 bootstrap 资格。guardian 还验证私有路径、文件系统、规范 Scope 名称及锁的 device/inode。
已有日志或未知目录内容会被拒绝并保留。root 和同 UID 仍可信，不隔离恶意同 UID 程序。

只有 guardian 初始化 `guardian.sqlite`，通过 WAL/FULL 事务记录原身份、创建宿主绑定及凭据。
它同步私有 output 与 Scope 目录，再发布 `control.sock`、`events.sock`、`io.sock`。
因此 Ready 晚于初始化及端点绑定。Bootstrap EOF 不是存活租约；退出保留锁文件、日志和端点路径，
旧 Scope 不会重新开放初始化。

## Ready 不等于控制权接管

三个 socket 提供 Ready 和当前宿主会话查询；Control 另支持宿主绑定，没有 Run、业务修改、事件订阅或工作负载 I/O 消息。
每次交互验证内核 peer UID、凭据、协议版本、原意图和实际 socket 角色；客户端以同一个随机 session nonce
关联回复。MessagePack 使用长度前缀，限额 16 KiB、深度 16；畸形消息、未知字段和尾随对象均拒绝。
每次交互期限为 5 秒，独立 worker 限额避免停滞的 I/O 握手占用 control 名额。

Ready nonce 仍只是只读关联，**不是**持久管理会话或执行 fence。
`GuardianManagement::bind(host.binding())` 单独绑定通过 `HostState` 独占资格取得的宿主实例。
调用方必须是可信管理代码，不能转发 Node 自报的代次。guardian 在自己的 WAL/FULL 事务中提交
宿主代次、实例及新的会话凭据，再确认更高绑定；相同绑定重传返回同一凭据。
低代次、同代次不同实例、超出 SQLite 正 i64 范围的代次均拒绝。只有 Control 能绑定；原创建意图和恢复凭据不变。

`inspect(channel, session)` 在每个通道检查当前宿主绑定与会话凭据。已解码请求等待同一执行锁，
执行时重新读取 SQLite 当前权威，不沿用入口检查结果；锁内提交与检查之间没有异步空隙。
旧会话即使已有 socket 连接，接管后也无法继续查询会话状态。先前回复可能迟到，但不能绕过下一次执行检查。
接管回复丢失时重传相同绑定，不启动替代实例；存储错误不确认接管，下一请求重新读取真实持久绑定。
连接仍处于未结束事务时拒绝全部管理请求。日志所有者同时持有 Scope 锁，取消服务不会在剩余 worker
关闭 SQLite 之前释放独占资格。

该 fence 只处理宿主会话，不实现 Controller 授权、Scope 控制代次、租约、失联策略或业务执行。
这些检查与 Run 持久接受落实到实际副作用边界前，不开放 Run／stdin。持有可信恢复凭据的程序即使旧
宿主会话失效，仍能进行 Ready 发现。

新 guardian 创建 v2 日志，单独保存当前会话，原 bootstrap 事实保持不变。新 guardian 不升级已有 v1 日志。
存活旧二进制继续处理未改变的 Ready 格式，但拒绝接管消息；不支持接管时必须失败，不能回退为仅验证 UID、
改写旧日志或启动替代 guardian。

启动记录之后的所有失败均保留 `launch_unknown`，包括 exec 失败和调用方回复丢失。
只能查询原实例，不重试 exec、不替换锁、不删除旧 socket，也不重开旧 guardian 日志来收养存活工作负载。
guardian 死亡不证明清理完成；部分初始化保留给后续显式恢复，不自动修复。

## 验证

`cargo test -p ora-process-guardian --test bootstrap` 在私有临时部署中使用真实 app，验证日志先于 Ready、
独立 session、错误凭据／UID 拒绝、三通道关联、停滞 I/O 隔离、无效继承资格、旧日志保留、失败 exec
消耗尝试，以及外部 SIGKILL 启动方后的原实例发现。启动 fixture 使用生产 API，但不是生产宿主 app。

管理测试另验证启动方 SIGKILL 后接管、同绑定重传、代次冲突、错误凭据／UID、三个旧通道失效、
旧 socket 的迟到字节、绑定先提交再回复及主动丢弃回复后的恢复。Runtime 测试将已解码请求实际 poll
入执行锁队列再接管，并注入 SQLite 只读写失败后重试／重开。这些证据只证明宿主会话，不证明 Run／stdin
fencing 或完整崩溃窗口。

协议测试覆盖帧限额、规范身份、未知字段与凭据脱敏；utils 测试覆盖继承 OFD 资格和意外可继承描述符关闭。
宿主测试覆盖精确 v1 升级不替换身份或锁。任意崩溃窗口注入、物理断电、服务管理器部署、Windows／macOS
guardian 支持及 Run 持久恢复仍未验证。ADR 保持 approved，不标 implemented。
