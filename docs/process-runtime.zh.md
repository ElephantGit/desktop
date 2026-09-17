# 进程运行体系实现状态

[English](process-runtime.md) | 中文

[已批准的进程 ADR](../specs/decisions/node/process/README.md) 正在分批实施。
当前增量提供内存态生命周期内核，**不是生产进程启动器**。
现有 `ora-process`、`ora-reaper`、Git 和插件入口保持不变。

Linux 另已加入独立 [Helper 部署预检](process-helper.zh.md)，检查不启用工作负载启动，也不构成平台 adapter。

## 所有权与行为

- `ora-process-protocol` 拥有本地域类型：运行身份、精确启动参数、纳管选择、停止意图、直接退出事实和清理证据，尚未定义线协议编码。
- `ora-process-runtime::ScopeRuntime<P>` 拥有单个 Scope 的准入、运行记录与停止期限。
  `Platform` 提供已验证能力、创建时纳管、观测和单 Run 信号。本批尚无 OS adapter；测试通过这一边界注入平台事实。
- 创建 Scope 时冻结实际保证。必须强但能力不足时拒绝；明确要求尽力时不能静默提升为强模式。
- 同 RunId 同参数重传返回当前事实，变更参数产生冲突。未知启动不会重试；当前对已证明未启动的尝试也仅重放，不续跑。
- 关闭先封闭准入，再安排收尾；停止单个 Run 不关闭 Scope，也不停止相邻 Run。
  等待、通知后等待、立即强制三类请求只能收紧期限或升级动作。
- 直接退出与后代清理独立。收尾策略通知后代，并在显式宽限期后强制结束；等待全部策略持续管理后代，直到退出或收到停止请求。
- 信号发送成功不证明清理完成。观测或信号失败仍保留责任；直接运行／退出证据可确认未知启动，不再次 spawn。
  退出结果可以变得更精确，但不能被较弱证据覆盖；矛盾的启动／退出观测保持阻塞。

## 调用方责任与待实现边界

调用由可变所有权串行化。调用方必须用单调递增的 `Instant` 驱动 `reconcile`；内核没有后台任务、
定时器、重试退避或基于 Drop 的清理。平台方法必须有界，并在启动未知时仍保留稳定尝试身份。
丢弃内核不提供崩溃恢复。

持久接受、授权、租约、宿主／guardian 进程、平台 adapter、I/O、恢复、资源交接和生产接入均待实现。
本批不改变文件系统布局，不代表阶段 1 完成，也不证明任何 OS 级纳管保证。

## 验证

运行 `cargo test -p ora-process-runtime` 和
`cargo clippy -p ora-process-protocol -p ora-process-runtime --all-targets -- -D warnings`。
集成测试通过公开运行时接口注入受控平台事实与时间，不依赖 sleep 或环境变量修改。
[核心用例索引](../specs/test-cases/node/process/README.md) 将这些证据记为 `Partial`；
真实后代、崩溃、持久化、跨进程竞争和平台权限边界仍需直接验证。
