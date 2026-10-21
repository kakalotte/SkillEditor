# 0006 — 规格适用性与 Tauri 技术栈确认

- 状态：已接受
- 日期：2026-10-17

## 决策

**本实现的技术栈确定为 Rust + Tauri，已获确认。**
主规格的桌面框架条款按 Tauri 技术栈落地；**与 Tauri 冲突的条款
不作为本实现的验收项**。

因此不再把 WebView 等事项描述为「偏离」「违反规格」——
它们是技术选型的正常结果，而非需要辩护的缺陷。

## 逐条适用性

| 主规格条款 | 本实现 |
| --- | --- |
| §7.1「不引入 WebView」 | **不适用**。Tauri 的渲染层就是系统 WebView（macOS WKWebView / Windows WebView2 / Linux WebKitGTK） |
| §7.1「不运行本地服务端」 | **适用且已满足**。没有 HTTP 服务端、没有监听端口；前端资源由 Tauri 自定义协议（`tauri://`，Windows 上为 `http://tauri.localhost`）从可执行文件内直接提供 |
| §7「桌面技术架构」 | **以 Tauri 等价架构实现**（见 `architecture.md`） |
| §12 阶段 A「初始化 Rust 项目与最小窗口」 | **以 Tauri 最小窗口实现**（见 `progress.md` 阶段 A） |
| §12 阶段 F「建立 Windows/macOS/Linux 构建流程」 | **仍适用**；三平台 CI 与发布工作流已添加，Windows/Linux 首次运行待验证（见 `progress.md` §7 第 2 条与 §12） |
| §14「可运行的 Rust 桌面项目」 | **以 Rust + Tauri 桌面项目实现** |
| §1 执行要求、§6 文件规范、§9 提示、§10 安全、§11 测试与验收、§13 代码质量 | **全部适用并已遵循** |

「不引入庞大前端工具链」不是规格条款，而是本实现自行附加的约束（见 `0001`）；
它通过「手写 HTML/CSS/JS + `withGlobalTauri`」满足，无需 npm。

## 后果

- `0001` 与 `0004` 中关于「偏离规格 §7.1」的表述据此更新为「以 Tauri 方案实现」。
- 仍然保留 §10（安全）与 §11（测试/验收）的全部要求——**不作为验收项的是技术选型条款，
  不是安全与质量条款**。
- 本实现是单一 Rust 工作区：核心库与 Tauri 命令层分层，安全要求不因渲染层选型而降低。

## 与其他决策的关系

- `0001-toolchain-and-tauri-version.md`：Tauri 版本、本地/CI 的 Tauri CLI 边界、`custom-protocol`。
- `0004-frontend-and-ipc.md`：前端与 IPC 设计。
- 本文只负责「哪些规格条款适用」，不重复技术细节。
