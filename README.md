# LocModel 网页版适配层

这个仓库只包含把 LocModel 桌面应用改成「服务器 + 浏览器」访问方式时，**自己新增**的代码：

| 文件 | 作用 |
|---|---|
| `src/http.rs` | axum 服务：`/api/invoke`（命令）、`/api/events`（SSE 事件）、`/api/raw`（文件下载）、`/api/upload`、`/api/dialog`、`/engine/*` 反向代理、口令鉴权 |
| `src/host.rs` | 把原来依赖 Tauri 的事件和窗口能力抽成一个 Host 对象 |
| `src/runtime.rs` | 共享的 Tokio 运行时 |
| `src/dialog.rs` | 网页版的文件选择对话框替代实现 |
| `frontend/web.js` | 前端垫片：提供 `window.__TAURI__.core.invoke` 和 `event.listen`，让原界面通过 HTTP 调用后端 |
| `tests/server.rs` | 服务层的基础测试 |

## 重要说明：这不是一个可以独立编译的项目

这些文件引用了桌面版主程序的模块，例如 `dispatch`、`Ctx`、`shutdown`、`engine`、`models`、`media` 等。
**你需要自备 LocModel 桌面版的 Rust 主程序代码**（即提供上述模块的那一份代码），再把这里的文件放进去，才能编译。

本仓库**不包含**：

- 桌面版的界面代码（`frontend/` 下除 `web.js` 以外的所有文件）
- 模型引擎、模型下载、画图、Agent、账号与支付等主程序模块
- 任何模型权重、访问口令、服务器地址或账号信息

## 使用方式（概述）

1. 准备一份提供上述模块接口的 LocModel 主程序代码。
2. 把本仓库的 `src/*.rs` 放入 `src/`，`frontend/web.js` 放入前端目录，`tests/server.rs` 放入 `tests/`。
3. 在 `src/lib.rs` 中声明新增的模块（`mod host; mod http; mod runtime; mod dialog;`），并在 `Cargo.toml` 中加入 `axum`、`include_dir`、`tokio-stream`、`tokio-util` 等依赖。
4. 在服务器上运行时设置 `LOCMODEL_BIND` 和 `LOCMODEL_DATA_DIR` 环境变量；如果监听地址不是本机回环地址，启动时会生成访问口令，保存在数据目录的 `access-token.txt` 中。

## 许可证

MIT License，见 [LICENSE](LICENSE)。仅适用于本仓库中的文件；桌面版主程序的版权和许可证不由本仓库授予。
