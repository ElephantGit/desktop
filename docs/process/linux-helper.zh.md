# Linux helper

[English](linux-helper.md) | 中文

`ora-process-helper`（`apps/ora-process-helper`）是独立的特权 helper，[Linux helper ADR](https://github.com/ora-space/specs/blob/main/decisions/node/process/containment/linux/20260917-independent-privileged-helper.md)
（`approved`）用它来描述基于 cgroup v2 的强隔离。目前已有的是部署预检、一个经过认证的**只做检查**的监听，以及一个不通过 IPC 暴露
的底层启动闸门。没有启动 API、服务安装程序或 guardian 集成，也没有任何东西声明强隔离。Node 部署改用
[rootless 适配器](linux-rootless.zh.md)，不需要特权部署。

## 配置与预检

用 `cargo build -p ora-process-helper` 构建。管理员显式地以单独的 root 进程运行
`ora-process-helper --check /etc/ora/process-helper.json`；永远不要以 setuid 安装。`--check` 和 `--serve` 都不改变 cgroup
成员关系，也没有任何东西会自动安装二进制、创建账户或启动特权服务。

```json
{
  "version": 1,
  "manager_uid": 1000,
  "workload_uid": 2000,
  "workload_gid": 2000,
  "cgroup_root": "/sys/fs/cgroup/ora-workloads"
}
```

管理 UID 和工作负载 UID 必须不同且非零，GID 必须非零，三者都拒绝 `4294967295`。未知字段和版本会被拒绝。文件最大 16 KiB，必须
是由 root 控制的普通文件，祖先目录也由 root 控制，组和其他用户都不可写，且不含符号链接；`ora-utils::path::open_trusted_path`
固定检查过的描述符，而不是重新跟随路径。

cgroup 根必须已经存在，由 root 控制，使用 `domain` 类型的 cgroup v2，没有直接挂在上面的进程，并提供 `cgroup.events` 和可写的
`cgroup.kill`；helper 自己必须在它之外。预检从不写这些文件，只在这些前提满足时成功；它既不证明后代进程已经消失，也不证明允许
启动。

## 检查服务

`ora-process-helper --serve /etc/ora/process-helper.json /run/ora-helper/control.sock` 在一个属于 `manager_uid`、模式为 `0600`
的端点上监听，父目录必须已存在并由 root 控制。从不替换已有的文件或 socket；`SIGINT`/`SIGTERM` 只删除本服务创建的 socket，崩溃
留下的文件需要管理员删除后才能重启。

每个连接在读取任何载荷之前都通过内核的对端 UID 认证（连接时的身份，所以管理方不能把 socket 交给工作负载），并且只承载一个请求
和一个回复：四字节大端长度加 UTF-8 JSON。请求是 `{"version":1,"operation":"inspect"}`；回复是 `launch_unavailable`、
`unauthorized`、`invalid_request` 或 `unsupported_version`。请求最大 16 KiB，交换有五秒截止时间，最多同时进行 16 个交换。管理方
可以查询可用性，但永远不能选择命令、UID、PID 或 cgroup。

## 启动闸门

`ora_process_runtime::spawn_linux_helper_workload` 是给受信 helper 代码用的构建块，不是 Run API。它重新加载 root 拥有的配置，
只接受已存在的 `cgroup_root/scope/run` 目标；它不创建目录、不保存日志，也不对调用去重，所以将来的生命周期 owner 必须负责准入
和清理责任。

它检查由 root 控制的迁移祖先目录、为空的直接成员、为空且未冻结的 Run 子树，以及可写的成员文件。exec 之前，fork 出的子进程向
固定的成员描述符写入 `0`，开启 session，给非 stdio 描述符设置 close-on-exec，设置 `no_new_privs`，清空附加组，设置真实、有效和
保存的 GID 与 UID，清空 capability，并以工作负载身份切换到 cwd；任何一步失败都会中止 exec。只使用调用方提供的环境，返回的
`Child` 拥有三个 pipe，所以由调用方读空输出、回收子进程并执行后代进程策略。

特权验收测试被显式标记为 ignored，只在一次性的、由管理员准备的环境中运行：设置 `ORA_PROCESS_HELPER_TEST_CONFIG`，以 root 运行
`cargo test -p ora-process-runtime --test helper_launch_privileged -- --ignored`。它只在配置的根下创建和删除新的 Scope 和 Run
目录。
