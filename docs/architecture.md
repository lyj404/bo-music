# 项目结构

当前工作区保留 7 个 crate，按职责而非页面数量划分：

| Crate | 职责 |
| --- | --- |
| `bo_music-domain` | 与界面无关的共享领域类型 |
| `bo_music-api` | 接口协议、请求及会话管理 |
| `bo_music-storage` | 设置、凭据、队列和历史记录持久化 |
| `bo_music-playback` | libmpv、音效、频谱及平台音频输出 |
| `bo_music-app` | 业务协调、播放队列和后台服务 |
| `bo_music` | GPUI 页面、窗口和交互 |
| `bo_music-cli` | API 与播放诊断入口 |

## 界面状态

`bo_music/src/shell_state` 按状态归属组织，`MusicApp` 负责连接这些状态、GPUI 和后台服务：

- `navigation`：当前 Tab、详情来源、浏览数据和返回可用性。切换一级 Tab 时调用 `enter`；详情打开通知和浏览快照分别经 `open_detail`、`accept_browse` 更新。
- `search`：搜索输入、结果、请求代次及搜索建议。建议失效和面板关闭逻辑由该状态统一维护。
- `playlist_editor`：歌单编辑对话框、表单输入、分类和封面草稿。
- `comment_panel`：评论输入、草稿、提交状态及图片预览。
- `panels`：播放工具和侧面板的显示状态。

状态模块不发送后台命令。命令发送、界面渲染和 GPUI 资源操作仍由相应的 `shell_*`、页面模块负责。异步结果原有的请求代次检查保留在事件处理入口。

## 后台服务

`bo_music-app/src/desktop_service.rs` 保留原有公开入口，调用者仍使用 `desktop_service::{Command, Event, Service, ...}`。

- `desktop_service/messages.rs` 定义命令、事件及传递给界面的快照类型。
- `desktop_service/runtime.rs` 管理 Tokio 线程、运行时状态、任务取消和关闭顺序。
- `desktop_service/history.rs` 将历史记录的磁盘操作串行放在独立线程执行。
- `desktop_service/handlers` 按账号、目录、播放、歌单编辑、搜索与评论、设置与月报组织命令处理器。
- `desktop_service/audio.rs`、`batch.rs` 提供音效和批量操作辅助逻辑。

命令处理器通过宏组成同一个穷尽 `match`，保留原有处理顺序和 `continue` 语义，避免拆分后遗漏命令分支。

海报生成暂时保留在 `bo_music-app`。出现独立复用或可选构建需求后，再考虑提取 crate；增加 crate 本身不保证减少体积或提高性能。
