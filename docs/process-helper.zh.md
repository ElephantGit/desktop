# Linux 进程 Helper 部署预检

[English](process-helper.md) | 中文

独立特权 helper 方向已记录在
[Linux 后续 ADR](../specs/decisions/node/process/containment/linux/20260917-independent-privileged-helper.md)。
当前可执行文件**仅实现部署预检**，没有启动 API、监听服务、服务安装器或 guardian 接入，
不宣称强纳管能力。

## 构建与配置

使用 `cargo build -p ora-process-helper` 构建。部署管理员可显式以独立 root 进程运行
`ora-process-helper --check /etc/ora/process-helper.json`，不要安装为 setuid 程序。
本次改动不安装二进制、不创建账号、不修改 cgroup，也不启动特权服务。

版本 1 配置如下，数字身份仅为示例，不是默认值：

```json
{
  "version": 1,
  "manager_uid": 1000,
  "workload_uid": 2000,
  "workload_gid": 2000,
  "cgroup_root": "/sys/fs/cgroup/ora-workloads"
}
```

管理与业务 UID 必须非零且不同，业务 GID 必须非零；拒绝未知字段和版本。配置限制为 16 KiB，
必须是 root 控制的普通文件；各级祖先也必须由 root 控制，禁止组／其他用户写入，禁止符号链接。
共享的 `ora-utils::path::open_trusted_path` 使用文件描述符固定逐级验证过的目录和目标，
不采用检查路径后再跟随可能被替换的链接的方式。

Cgroup 根必须已存在、由 root 控制，实际位于 cgroup v2 文件系统，类型为 `domain`，
没有直接附着的进程，并提供 `cgroup.events` 与可写的 `cgroup.kill`。预检不会写入控制文件。
Helper 本身必须位于工作负载子树外。检查成功只证明这些前提，不证明全部后代已退出、
不冻结后续权限、不验证账号配置，也不授予启动资格。

## 待实现边界

管理 IPC 认证、降权前后的执行边界、禁止重新提权、创建时纳管、描述符交接、业务身份下的工作区访问、
guardian 存续与 helper 恢复仍待实现。Root helper 不能成为任意 root 命令执行或任意 PID 迁移接口。

当前测试在不提权的条件下验证配置拒绝、非 cgroup 文件系统拒绝、CLI 失败和受信任路径处理。
Root／cgroup 正向测试仍需显式配置的环境。Crates CI 已覆盖 Linux、macOS、Windows 任务矩阵；
新增矩阵本身不构成纳管证明。
