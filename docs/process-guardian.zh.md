# 无特权 Guardian 与可信本机管理

[English](process-guardian.md) | 中文

按 Eric 的明确要求，当前信任本机 Node 和管理程序，不使用秘密令牌、签名密钥或 Controller 授权。
私有路径与内核 peer UID 检查防止意外跨用户访问，不隔离恶意同 UID 程序。
宿主实例身份和持久代次仍保留，用于拒绝接管后迟到的旧实例请求。

## 独立所有权

调用方向 [HostState](process-host-state.zh.md) 显式提供专用状态目录和可信可执行文件。
不读取 HOME，不要求 root helper 或服务安装。宿主先提交创建意图和已消耗的启动记录，再 exec。
专用 socketpair 传递 bootstrap 身份；原 Scope 独占 flock 直接继承，不先解锁再重抢。
子进程进入独立 session，清空环境，exec 时关闭非显式描述符；命令行只有 --bootstrap。
不安装父进程死亡或 Drop-kill 策略。

Guardian 验证继承的打开描述、Scope 锁 inode、私有路径及本地文件系统。
只有它初始化 WAL/FULL 的 guardian.sqlite，再发布 control.sock、events.sock、io.sock。
Ready 晚于初始化，不证明 Run 已启动或清理完成。启动错误、回复丢失、guardian 死亡都不授权
为同一 Scope 再启动一个 guardian。旧日志、锁和端点保留，不自动修复部分初始化。

## 基于身份的接管

GuardianManagement::bind 接收通过 HostState 独占资格提交的绑定。可信调用方必须遵守该所有者，
不自行编造更高代次。更高绑定先提交再确认；相同绑定幂等，低代次或同代次不同实例拒绝。
宿主会话只包含公开的宿主绑定，不包含秘密凭据。

请求在执行锁内按日志当前绑定检查，接管前排队的请求也不能沿用旧资格。
Scope 锁保持到剩余 worker 关闭 SQLite；未结束事务或存储失败不能产生成功接管确认。
Ready 发现独立于当前宿主会话查询。

消息使用有界、长度前缀 MessagePack（16 KiB、深度 16），协议版本为 2；每次交互有 5 秒 I/O
期限，各通道 worker 独立限额。当前提供 Ready、宿主绑定和会话查询；下一切片接业务执行。

## 已有文件与版本

新 host 和 guardian 日志版本为 3，不再有凭据列。Host 恢复事务化迁移精确 v1/v2 布局，
保留原 Scope、代次、锁 inode 与已消耗启动尝试。删除旧令牌列不承诺安全擦除 SQLite 空闲页
或备份中的历史值。未知布局拒绝，不自动修复。

存活旧 guardian 使用旧令牌协议，与新客户端不兼容；必要时使用其兼容管理版本，
不改写它的日志、不覆盖仍有责任的二进制，也不重启旧 Scope。本批不收养旧活进程。

## 验证与边界

真实 app 测试覆盖独立 session、启动方 SIGKILL 后存续与接管、原实例发现、失败 exec 去重、
错误 Scope／UID、继承锁资格、旧日志拒绝、三个旧通道失效、迟到字节及接管回复丢失。
Runtime 测试覆盖执行队列检查、持久化失败与锁生命周期；Host 测试覆盖精确 v1/v2 迁移。

完整 Controller 授权、租约、生产宿主 app、业务 IPC、stdin、持久输出、Node／Git／插件接入
和 guardian 死亡恢复仍未完成。强纳管、服务管理器下存续、物理断电与其他平台支持尚未证明。
