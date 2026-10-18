# Workspace 文件

[English](workspace-files.md) | 中文

Files 面板可以浏览 checkout、查看文本、按名称和内容搜索、随原生文件变化刷新，并在资源管理器树里新建、剪切/复制/粘贴、重命名和经确认后删除。查看器本身是只读的。输入框里的 `@` 文件引用也使用同一组操作。

## 职责划分

客户端发送任务或项目 id，需要时再带上相对于 checkout 的路径，从不提供根目录。Desktop 根据持久化的身份解析权威的 checkout。

| 层 | 负责 |
|---|---|
| `ora-utils` | 可移植的路径校验和规范化的包含检查 |
| `ora-fs` | 把它们用在 checkout 根目录上，读取和搜索的上限、ripgrep 执行、原生监听 |
| `apps/desktop/src-tauri/src/commands/files.rs` | Tauri 参数提取、根目录解析、命令和 channel 边界 |
| `apps/desktop/src-tauri/src/workspace_files.rs` | 把文件系统结果映射成契约值，保留类型化错误 |
| `packages/app-shell/src/features/files` | 目录树、查看器、搜索 UI 和按行引用 |
| `packages/app-shell/src/state/data/files.ts` | 查询标识、失效和刷新范围 |

通用文件系统访问刻意放在 `ora-backend` 之外。

## 作用域：任务 worktree 或项目 checkout

选中任务时，Files 解析到该任务的 worktree。选中了项目但还没有任务时（空白或草稿输入框），Explorer、Search 和 `@` 引用解析到项目的 Main Workspace checkout，也就是模型发现和第一个会话使用的目录。两个 id 都有时以任务 worktree 为准。

## 操作

| 任务作用域 | 项目作用域 | 请求 |
|---|---|---|
| `listWorkspaceDirectory` | `listProjectDirectory` | id，可选的相对 `path` |
| `readWorkspaceFile` | `readProjectFile` | id，相对 `path` |
| `searchWorkspace` | `searchProject` | id，有上限的查询 |
| `watchWorkspace` | `watchProject` | id；是一个流 |
| `createWorkspaceEntry` | `createProjectEntry` | id，相对 `path`，`kind`（文件或目录） |
| `copyWorkspaceEntry` | `copyProjectEntry` | id，`from`，目标 `path` |
| `moveWorkspaceEntry` | `moveProjectEntry` | id，`from`，目标 `path` |
| `deleteWorkspaceEntry` | `deleteProjectEntry` | id，相对 `path` |

返回的路径都用斜杠分隔，相对于解析出的 checkout。契约在 `crates/contracts/src/file_system.rs`。

- 新建不会替换已有路径（`file_system_path_already_exists`），并要求父目录已存在。资源管理器在文件夹里或文件的父目录里新建；在树的空白处新建则落在根目录。
- 复制和粘贴作用于条目本身；在同一文件夹内复制会加上 `name copy` 后缀。重命名是对文件名部分的就地移动。删除先确认，然后永久删除该路径。
- 监听流发出 `data`、`error` 和 `end` 帧。错误帧使用共享的 `{ code, params, requestId }` 形状；关闭期间已排队的终止错误以 `error` 发出，而不是成功的 `end`。变化以使缓存失效的批次到达，而不是事件日志。关闭 watcher 会等待原生资源释放，见[流与取消](../core/streams.zh.md)。

## 安全

路径会被校验为相对路径，在包含检查之前先规范化，并在读取、搜索或新建之前设定上限。新建是受限的 `create_new` 或 `create_dir`；复制、移动和删除拒绝逃出 checkout，并且从不跟随符号链接（删除会删掉未被跟随的符号链接本身）。Backend 拒绝带根的路径。

## 刷新

新建、复制、移动和删除成功后，都会等待 Files 数据 owner 对目录、文件和搜索查询的范围刷新，即使 watcher 事件还没到。活跃查询会重新拉取，不活跃的查询标记为过期；其他任务和 checkout 互不影响。改名事件会让新旧路径、它们的父目录和搜索都失效；重新扫描会让整个选中的作用域失效。切换作用域会中止旧 watcher，卸载会中止当前 watcher。

## 查看器与聊天集成

- 行号旁的 `+`（点击、拖动，或在聚焦的行号上按 Ctrl/Cmd+Enter）把这些行作为一个紧凑的标签引用到输入框。prompt 中携带的是反引号包裹的 `path:range` 引用，由 Agent 自己读取内容；聊天历史把它显示回同样的标签。diff 行号旁的引用是例外，会展开成一个小的 `diff --git` patch，因为那些修改还不在磁盘上。
- 聊天中的内联制品链接通过 `openWorkspaceFile` 和 `WorkspaceFileRequest`（`path`、`requestId`，可选的行、列和结束行）打开面板。视图会去掉绝对 ACP 路径中的任务 cwd 前缀，展开上级目录，并高亮引用的行或范围。新的 `requestId` 会让该路径的查询失效，所以再次打开会重新读磁盘。任务 cwd 之外的路径不会被打开；文件不存在时显示本地化的"路径不存在"文案，而不是传输错误。
- 400 行以内的文件挂载所有行。更大的文件只渲染视口附近的窗口；超过大约 512 KiB 字符时，内容处理推迟到首次绘制之后，期间显示加载遮罩，所以切换会话从不等待预览。语法高亮的上限是 512 KiB。
- 搜索和读取都可以通过契约客户端取消。

"复制路径""复制相对路径"和"在文件管理器中显示"使用原生的 `open_location` 命令，见 [Desktop 宿主](../desktop/desktop-host.zh.md#原生命令)。
