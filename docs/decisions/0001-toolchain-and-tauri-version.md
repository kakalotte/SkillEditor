# 0001 — 工具链与 Tauri 版本

- 状态：已接受（2026-10-21 修订：发布 CI 使用 Tauri CLI）
- 日期：2026-10-17

## 背景

本实现交付 SkillForge 的 **Rust + Tauri** 实现：界面层使用 Tauri v2，业务逻辑位于
独立的纯 Rust 核心库。

主规格的桌面框架条款按 Tauri 技术栈落地；其中与 Tauri 冲突的条款
（§7.1「不引入 WebView」等）不作为验收项。逐条适用性见
`0006-spec-applicability.md`。

在动手写代码之前，先核实了依赖与平台版本（规格 §1：编码前验证版本）。

## 决策

### 1. 版本固定

| 组件 | 版本 | 说明 |
| --- | --- | --- |
| Rust | 1.99.1（实测工具链） | `rust-version = "1.82"`（MSRV，故意压低） |
| edition | 2021 | 工作区统一 |
| `tauri` | 2.12.4 | v2 稳定线 |
| `tauri-build` | 2.7.3 | 构建期校验 `tauri.conf.json` 并嵌入前端资源 |
| `tauri-plugin-dialog` | 2.8.3 | **唯一**使用的 Tauri 插件 |
| `wry` | 0.57.0（传递依赖） | WebView 抽象层 |
| `tao` | 0.37.1（传递依赖） | 窗口/事件循环 |
| `serde` / `serde_json` | 1.0.229 / 1.0.154 | |
| `serde_yaml_ng` | 0.10.0 | 见 0002 |
| `thiserror` | 2.0.21 | 核心库的 `thiserror = "2"` 解析结果 |
| `dirs` | 7.0.0 | 平台目录解析 |
| `sha2` / `hex` | 0.10.9 / 0.4.3 | 安装清单内容哈希 |
| `zip` | 9.0.4（仅 `deflate`） | 导入/导出 |
| `tempfile` | 3.27.0 | 安装暂存目录与测试 |

工作区统一用 `[workspace.dependencies]` 声明版本，成员仅写 `.workspace = true`。

工具链由 `rust-toolchain.toml` 固定（`channel = "1.99.1"`，含 `rustfmt` 与 `clippy`），
以保证「新克隆的仓库用固定的编译器构建」。MSRV 仍声明为 1.82：
删掉该文件后，任意 ≥ 1.82 的稳定版都应能构建（**仅在 1.99.1 上实测过**）。

### 2. 本地不使用 Tauri CLI；发布 CI 使用

日常开发与本地复现不安装 `@tauri-apps/cli`：

- 构建只用 `cargo build` / `cargo run`。
- `tauri.conf.json` 由 `tauri-build`（`build.rs`）在编译期读取校验，
  前端资源由 `tauri::generate_context!()` 在编译期嵌入可执行文件。
- macOS `.app` 由 `packaging/make-macos-app.sh` 手工组装（写 `Info.plist`、
  拷图标、ad-hoc 签名），用于本机开发启动。

**发布例外**：GitHub Actions 的发布工作流会安装 `tauri-cli`，并用
`cargo tauri build` 生成正式安装包。这样做是因为 CLI 负责：

- macOS `.dmg` 组装、Developer ID 签名与公证；
- Windows NSIS `.exe` 与 MSI `.msi`；
- Linux `.deb` 与 `.AppImage`；
- 把四个平台的产物上传到同一个 Draft Release。

本地的 `packaging/make-macos-app.sh` 不承担分发职责，只做 ad-hoc 签名。
发布流程、Secrets 与验收步骤见 `docs/releasing.md`。

**关于 `custom-protocol`**：Tauri CLI 在 `tauri build` 时会自动加
`--features custom-protocol`（即 `tauri/custom-protocol`，启用从可执行文件内
提供前端资源的自定义协议）。本地不用 CLI，因此本实现自己在
`src-tauri/Cargo.toml` 中默认开启：

```toml
[features]
default = ["custom-protocol"]
custom-protocol = ["tauri/custom-protocol"]
```

**收益**：本地开发仍然只需要 Rust 工具链，没有 Node/npm/npx 依赖；
CI 才下载 Tauri CLI，且只用于发布。

### 3. **不使用 npm、打包器或前端框架**

前端是手写的 `ui/index.html` + `ui/styles.css` + `ui/app.js`，无构建步骤。这依赖
`tauri.conf.json` 中：

```json
"app": { "withGlobalTauri": true }
```

它把 `@tauri-apps/api` 注入为 `window.__TAURI__`，因此不需要 `import`、
不需要打包器。详见 0004。

### 4. 工作区结构

```
skillforge-tauri/
├── Cargo.toml                 # workspace：版本、依赖、lint、release profile
├── crates/skillforge-core/    # 纯 Rust 核心（UI 无关）
├── src-tauri/                 # Tauri 壳（bin crate: skillforge）
├── ui/                        # 前端资源（编译期嵌入）
└── packaging/                 # 图标生成 + macOS 打包
```

核心 crate 是纯 Rust 库（`skillforge_core`），只包含领域模型、格式、校验、平台适配
与安装逻辑，不依赖 Tauri 或任何 GUI 框架。

**刻意不做的抽象**：核心库不感知界面层，也不为其他界面方案预留 trait 或 feature。
当前只实现 Tauri；额外构建矩阵会带来远超收益的复杂度（规格 §13：避免过度架构）。

### 5. 质量门禁

```toml
[workspace.lints.rust]
warnings = "deny"
unsafe_code = "deny"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }

[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

`unsafe_code = "deny"` 对本项目尤其重要：Tauri 与 `wry` 自身有大量 unsafe，
但那是依赖内部的事；我们自己的代码零 unsafe。

## 后果

- ✅ 本地单一工具链；`git clone` 后 `cargo build --release` 即可产出可执行文件。
- ✅ 发布由 CI 使用 Tauri CLI 完成，能覆盖 macOS 公证与 Windows/Linux 安装包。
- ✅ 依赖面小：只有 1 个 Tauri 插件（dialog），WebView 能力一律不用。
- ⚠️ 本地 ad-hoc 打包脚本不使用 CLI，因此它必须与 `tauri.conf.json` 的
  identifier、版本号保持一致；CI 发布则直接使用 CLI 读取这些配置。已用
  `src-tauri/tests/frontend_contract.rs` 断言 identifier 与版本一致性，并在
  打包脚本中从 `Cargo.toml` / `tauri.conf.json` 读取，避免多处硬编码漂移。
- ℹ️ 渲染层是系统 WebView。这是 Tauri 的选型结果；§7.1「不引入 WebView」
  按 Tauri 方案实现（见 `0006`），「不运行本地服务端」仍适用且已满足。
- ⚠️ 前端没有任何类型检查，命令名写错只会在运行时炸。已用
  `src-tauri/tests/frontend_contract.rs` 做前后端契约测试弥补。

## 验证记录

```
$ cargo build -p skillforge            # 干净构建 2m06s
$ ./target/debug/skillforge            # 进程存活，lsappinfo 报 type="Foreground"
$ cargo test --workspace               # 127 passed
$ cargo clippy --workspace --all-targets   # 无输出
$ cargo fmt --all --check              # 无输出
```
