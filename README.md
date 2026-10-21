# SkillForge（Rust + Tauri）

[![CI](https://github.com/kakalotte/SkillEditor/actions/workflows/ci.yml/badge.svg)](https://github.com/kakalotte/SkillEditor/actions/workflows/ci.yml)

把个人经验整理成标准 **Agent Skills** 的本地优先桌面编辑器。

SkillForge 采用 **Rust + Tauri v2 + WebView**：领域模型、`SKILL.md` 格式、校验规则、
平台适配与安装安全位于独立的纯 Rust 核心库，界面层只负责展示与调用命令。

- GitHub 仓库：<https://github.com/kakalotte/SkillEditor>
- 产品与实现方案（主规格）：`../SkillForge_Rust_Slint_Product_and_Implementation_Plan.md`
- 本实现的技术决策记录：`docs/decisions/`
- 逐阶段进度与实测记录：`docs/progress.md`
- 发布、签名与 Secrets：`docs/releasing.md`

### 文档阅读顺序

| 文档 | 内容 |
| --- | --- |
| `README.md` | 本文：功能、环境、构建、运行、测试、打包 |
| `docs/product-spec.md` | 本实现实际交付的产品需求，以及与主规格的差异 |
| `docs/architecture.md` | 实际架构、IPC 契约、服务划分、测试分层 |
| `docs/platform-support.md` | 平台路径、能力、来源与验证状态（含待验证清单） |
| `docs/security.md` | 文件操作、路径安全与安全策略 |
| `docs/progress.md` | 逐阶段命令与结果、验收对照、未实现项 |
| `docs/releasing.md` | GitHub Actions 发布、macOS/Windows 签名与验收 |
| `docs/decisions/0001` | 工具链与 Tauri 版本、本地/CI 的 CLI 边界 |
| `docs/decisions/0002` | YAML 解析与 frontmatter 无损往返 |
| `docs/decisions/0003` | 平台适配层与三级「核实状态」 |
| `docs/decisions/0004` | 前端方案与前后端 IPC 契约 |
| `docs/decisions/0005` | 诊断：不引入日志框架，统一写 stderr |
| `docs/decisions/0006` | 技术栈与规格适用性：Rust + Tauri 为既定实现 |

## 功能

- **工作区**：列出任意目录下的 Skill 目录；搜索、刷新、显示修改时间。
- **创建 / 编辑**：表单编辑 `name`、`description`，源码视图编辑正文；预览视图显示实际要写入磁盘的
  `SKILL.md` 全文。
- **无损保持**：未知 frontmatter 字段、注释、中文与资源文件在保存后原样保留；frontmatter
  解析失败时按原始文本打开，并可**在应用内直接修复**，修复成功前禁止保存。
- **校验**：分三类问题（`spec` 规范错误 / `platform` 平台兼容 / `quality` 质量建议），
  每条规则有稳定 id、严重级别、来源、可选的文件与行号、建议和规则文档链接。
  推荐章节缺失只报「建议」，绝不当作规范错误。平台卡片会显示当前 Skill
  在该平台的兼容状态，同一平台无论发现多少个目录都只报一次。
- **Frontmatter 规则参考（只读）**：编辑器工具栏与侧栏可打开规则指南，按
  「必填字段 / 规范可选 / 平台扩展 / 正文与目录 / 校验规则」分页，显示字段类型、
  必填性、约束、YAML 示例、常见错误、平台支持与官方链接，并附完整 `SKILL.md` 示例。
  平台支持与列表上限来自核心库与 adapter，不在 JavaScript 中硬编码；支持搜索。
  「必填字段」页顶部显示一条紧凑的当前文档状态（已用字段、缺失必填项与
  `description` 字符数），其他分页与搜索时不显示。打开指南不会修改文件、
  不会清除未保存状态。
- **平台适配**：Claude Code、OpenAI Codex、OpenCode、DeepSeek Harness、
  DeepSeek Harness Desktop、通用目录六个 adapter，
  各自声明默认路径、支持字段、列表长度上限、资源目录、核实日期与验证状态。
  对有明确等价关系的字段，安装时只修改**安装副本**（当前为 DeepSeek Harness 的
  `when_to_use` → `whenToUse`）；工作区源文件字节不变。
- **安装**：先显示**差异预览**（新增/更新/删除/无变化、是否需要新建目录、是否需要备份、
  是否检测到外部修改），确认后才落盘；覆盖前自动备份。预览同时列出投影后仍存在的
  平台兼容提醒与将要执行的字段映射，有提醒时确认按钮变为「仍然安装」；存在未保存修改时拒绝安装，
  避免安装磁盘上的旧内容。安装清单记录实际写入内容的哈希与投影
  `from` / `to`，便于事后核对；旧版清单仍可读取。
- **卸载**：只删除由本应用写入且记录在安装清单中的文件；外部修改会被提示。
  未确认的删除/卸载会被后端直接拒绝（`confirm` 参数）。
- **导入 / 导出**：导入目录、`SKILL.md` 或 `.zip`（防 Zip Slip、条目数与体积上限）；
  导出为 `.zip`；另存为会连同资源文件一起复制。
- **安全**：不执行 Skill 内的任何脚本；不联网；不记录正文。

## 环境要求

| 组件 | 版本（实测） | 说明 |
| --- | --- | --- |
| Rust | 1.99.1（`rust-toolchain.toml` 固定） | 最低声明 MSRV 为 1.82，见 `Cargo.toml` |
| Tauri | 2.12.4（`tauri-build` 2.7.3） | v2 稳定版 |
| `tauri-plugin-dialog` | 2.8.3 | 唯一使用的插件：原生文件/目录选择器 |
| 系统 WebView | macOS 自带 WKWebView | Windows 需 WebView2，Linux 需 WebKitGTK（未实测） |
| Node.js | **不需要** | 前端是手写 HTML/CSS/JS，无构建步骤 |
| Tauri CLI | 本地**不需要** | 本地直接 `cargo build`；CI 发布使用 `cargo tauri` |

完整依赖清单与选择理由见 `docs/decisions/0001-toolchain-and-tauri-version.md`。

`rust-toolchain.toml` 把工具链固定为 **1.99.1**：`cargo build` 会用该版本
（rustup 会在缺失时自动安装）。若不想下载，可删掉该文件，用任意
≥ 1.82 的稳定版即可。

## 构建与运行

```bash
# 构建
cargo build --release -p skillforge

# 直接运行（开发用；macOS 上窗口正常，但建议用下面的 .app 启动以获得完整 GUI 会话）
./target/release/skillforge
```

前端 `ui/` 由 `tauri-build` 在编译期嵌入可执行文件，因此运行时不依赖任何外部文件。

### macOS 打包

```bash
packaging/make-macos-app.sh     # 生成并 ad-hoc 签名 dist/SkillForge.app
open dist/SkillForge.app
```

这个脚本只用于本机开发：它做 ad-hoc 签名，不满足分发要求。正式安装包由
GitHub Actions 使用 Tauri CLI 构建，macOS 版本会做 Developer ID 签名与公证。

### GitHub Releases（正式安装包）

发布由 `v*` tag 触发，产物先进入 **Draft Release**，人工验收后再发布：

| 平台 | 产物 | 签名 |
| --- | --- | --- |
| macOS Apple Silicon | `aarch64` `.dmg` | Developer ID + 公证 |
| macOS Intel | `x86_64` `.dmg` | Developer ID + 公证 |
| Windows | NSIS `.exe`、`.msi` | Authenticode |
| Linux | `.deb`、`.AppImage` | 未签名 |

签名所需的 GitHub Secrets、发布步骤与验收命令见 `docs/releasing.md`。
本地开发仍然不需要 Node.js 或 Tauri CLI。

图标可由脚本重新生成（可复现，不手改二进制）：

```bash
python3 packaging/make-icons.py   # 需要 Pillow 与（可选）macOS 的 iconutil
```

## 测试与质量检查

```bash
cargo fmt --all --check                        # 格式
cargo clippy --workspace --all-targets         # 静态检查（工作区把 clippy 警告视为错误）
cargo test --workspace                         # 全部测试
```

当前结果：**165 个测试全部通过**（core 124：单元 94 + 集成 30；桌面命令层 32；
前后端契约 9），`cargo fmt --check` 与 `cargo clippy` 均无输出。

测试分层：

- `crates/skillforge-core/tests/`：核心库的集成测试（fixtures 位于
  `crates/skillforge-core/tests/fixtures/`），不依赖显示环境与开发者真实配置（使用临时 HOME）。
- `src-tauri/src/app.rs` 内的 `#[cfg(test)]`：命令层全流程测试（新建→保存→重载、
  导入/导出、安装预览→确认→卸载、外部修改检测、路径安全拒绝等），使用临时 config/data 根目录。
- `src-tauri/tests/frontend_contract.rs`：前后端契约测试——前端调用的每个命令都必须已注册、
  已注册的命令都必须被使用、`index.html` 引用的资源必须存在、能力文件必须授予对话框权限、
  前端不得使用 `innerHTML` 渲染 Skill 内容、命令参数必须是 camelCase；
  `Cargo.toml` 与 `tauri.conf.json` 的版本必须一致；Frontmatter 指南
  必须保持只读模态框形态，并通过 `call()` 读取非 `Snapshot` 返回值。

## 常见问题

**前端报错怎么排查？** 应用没有日志框架，前端的错误会通过 `log_frontend` 命令写到
stderr（`[前端/错误] …`）。从终端启动即可看到：

```bash
./target/release/skillforge 2>&1 | grep '前端'
```

**窗口起不来？** 在 macOS 上请用 `packaging/make-macos-app.sh` 生成 `.app` 后用
`open` 启动；如果显示器处于休眠状态，先执行 `caffeinate -u -t 1`。

**数据存在哪？** 见「设置 → 本地路径」。默认：

| 内容 | macOS |
| --- | --- |
| 配置 | `~/Library/Application Support/SkillForge/config.json` |
| 工作区 | `~/Library/Application Support/SkillForge/workspace` |
| 安装清单 | `~/Library/Application Support/SkillForge/manifests` |
| 备份 | `~/Library/Application Support/SkillForge/backups` |
| 日志 | `~/Library/Application Support/SkillForge/logs` |

实际路径由 `dirs` 按平台解析（Windows 为 `%APPDATA%`，Linux 为 `$XDG_*`）。

## 规格适用性

本实现的技术栈是 **Rust + Tauri**。主规格的桌面框架章节按 Tauri 技术栈落地
（逐条适用性见 `docs/decisions/0006-spec-applicability.md`）：

- §7.1「不引入 WebView」：不适用 —— Tauri 的渲染层就是系统 WebView。
- §7.1「不运行本地服务端」：**仍然适用且已满足** —— 没有 HTTP 服务端、没有监听端口，
  前端资源由自定义协议从可执行文件内直接提供。
- §7 的桌面技术架构、§12 阶段 A 的最小窗口、§14 的「可运行的 Rust 桌面项目」：
  以 Tauri 等价物实现。
- **§10（安全）、§11（测试与验收）、§13（代码质量）全部适用并已遵循** ——
  不适用的只是技术选型条款，不是安全与质量条款。

另外三条是本实现自行附加的约束，不是规格要求：

- 本地不使用 Tauri CLI、不使用 npm/打包器，因此不需要 Node.js 即可构建；
  发布 CI 例外：它安装 Tauri CLI 以生成签名安装包。
- 没有日志框架，诊断信息统一输出到 stderr（`docs/decisions/0005-diagnostics.md`）。
- 预览视图不做富文本 Markdown 渲染，而是显示将要写入磁盘的 `SKILL.md` 全文
  加结构统计（`docs/decisions/0004-frontend-and-ipc.md`）。

## 已知限制与待验证

详见 `docs/progress.md` 与 `docs/platform-support.md`。要点：

- 仅在 **macOS 15.8.1 / arm64** 上实测构建、运行、打包与启动。
  Windows 与 Linux 的 CI 构建矩阵已经添加，但首次 Actions 运行前仍视为**待验证**；
  原生对话框与「在文件管理器中显示」也仍待人工点击验证。
- OpenCode 的默认路径与字段依据官方文档核实；Codex 的官方文档页在核验时无法自动抓取，
  标记为「兼容但有警告」（`PartiallyVerified`）。
- DeepSeek Harness 的路径/字段依据官方文档核实；DeepSeek Harness Desktop
  运行同一套 Harness 核心、目录相同，但未在装有 Desktop 的机器上确认它是否覆盖
  `DSH_HOME`，因此标记为「兼容但有警告」。
- 正式发布工作流已接入 macOS Developer ID 签名/公证与 Windows Authenticode
  签名，但首次签名发布仍需要在 GitHub 配置 Secrets 并完成一次 Draft Release
  验收；本地 `packaging/make-macos-app.sh` 仍然只做 ad-hoc 签名。
- 主题「跟随系统」依赖 WebView 的 `prefers-color-scheme`；浅色/深色外观未做像素级截图验证。

## 许可证

MIT OR Apache-2.0，见 `LICENSE-MIT` 与 `LICENSE-APACHE`。
仓库根目录的 `LICENSE` 是 GitHub 初始化仓库时生成的 MIT 文本。
