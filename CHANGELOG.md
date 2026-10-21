# 变更记录

本文件记录 SkillForge（Rust + Tauri）的变更。
格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [0.1.0] — 2026-10-21

首个 GitHub 发布版本。以下为相对 2026-10-17 内部基线的新增内容。

### 新增

- **只读 Frontmatter 规则指南**：编辑器工具栏与侧栏新增入口，按
  「必填字段 / 规范可选 / 平台扩展 / 正文与目录 / 校验规则」分页展示
  21 个字段的类型、必填性、约束、YAML 示例、常见错误、支持平台与官方链接，
  以及 20 条校验规则、目录/正文规则和完整 `SKILL.md` 示例。
  - 支持跨字段/规则搜索；「必填字段」页顶部显示一条紧凑的当前文档状态
    （必填完整性、已用字段、缺失必填项和 `description` 字符数），
    其他分页与搜索时不显示；
  - 平台支持与列表上限由核心库 + 实时 adapter 提供，不在 JavaScript 中硬编码；
    列表上限不重复显示在状态栏，只出现在「平台扩展」页；
  - 只读：不提供字段/YAML/整份源码编辑控件，打开时不写文件、不改变未保存状态；
  - 编辑器 `name` / `description` 下方新增规范提示与实时字符计数。
- **安装兼容门槛**：安装预览现在显示**安装副本**在该平台剩余的兼容提醒；
  有提醒时确认按钮变为「仍然安装」，用户必须显式确认。
- **平台卡片兼容状态**：每个平台卡片显示「当前 Skill：兼容 / N 项兼容提醒 /
  未打开」，并显示安装时将适配的字段数量。
- **安装时字段投影**：DeepSeek Harness 与 DeepSeek Harness Desktop 安装时，
  自动把通用的 `when_to_use` 重命名为 Harness 读取的 `whenToUse`。
  - 只修改安装副本，工作区源文件字节不变；
  - 只重命名顶层键，保留注释、键顺序、未知字段与正文；
  - 源中已有 `whenToUse` 或两者并存时不改动；
  - 预览列出映射内容；安装清单按实际写入内容记录哈希，并记录 `from` / `to`；
    旧版清单（没有该字段）仍可读取。

### 变更

- 平台兼容校验改为**每个平台一次**，不再按发现的目标目录重复
  （DeepSeek Harness 有四个根目录，此前会重复四次警告）。
- `preview_install` 拒绝有未保存修改的文档，避免安装磁盘上的旧内容。
- 安装预览的兼容提醒按**投影后的安装副本**校验：已被自动适配的字段不再同时报「会被忽略」，其他不支持字段继续显示。
- 安装预览标题包含目标变体（如「用户级 · Harness 专用」），不再只显示范围；
  安装成功后的状态栏显示已适配字段数。
- `InstallPreview` JSON 增加 `warnings` 与 `rewrites`；`PlatformItem` 增加
  `compatStatus` / `compatWarnings` / `compatRewrites`。

### 测试

- 165 个测试：core 124（单元 94 + 集成 30）、命令层 32、前后端契约 9。
- 新增指南测试：字段/规则唯一性、规范字段与 adapter 字段覆盖、YAML 示例可解析、
  完整示例可解析为 `SKILL.md`、规则常量一致性、平台支持与 camelCase 序列化；
  命令层验证打开指南不会改变未保存状态或写入文件；契约测试验证指南是只读模态框。
- 新增投影（BOM、CRLF、缩进根映射、嵌套键、已适配）、安装副本哈希、兼容去重、
  列表长度按实际读取字段计算、
  安装前保存门槛与命令层端到端投影测试。

### 发布

- 新增三平台 GitHub Actions CI（macOS、Windows、Ubuntu 22.04）。
- 新增 `v*` tag 触发的发布工作流：两个 macOS DMG、Windows NSIS + MSI、
  Linux `.deb` + `.AppImage`，统一进入 Draft Release。
- macOS 接入 Developer ID 签名与公证；Windows 在 CI 中导入 PFX 并签名
  NSIS/MSI。Linux 版本未签名。
- 新增 `docs/releasing.md`，记录 GitHub Secrets、发布步骤与签名验收命令。
- 本地开发仍不依赖 Node.js 或 Tauri CLI；Tauri CLI 仅用于 CI 发布。

## 初始实现 — 2026-10-17

首个内部版本。核心逻辑（领域模型、`SKILL.md` 格式、校验规则、平台适配、安装安全）
位于纯 Rust 库，界面层使用 Tauri v2 + WebView。

### 新增

- **工作区**：扫描目录下的 Skill，搜索（防抖）、刷新、按修改时间与状态徽章展示。
- **编辑器**：表单编辑 `name` / `description`，源码视图编辑正文，
  预览视图显示将要写入磁盘的 `SKILL.md` 全文；侧栏显示校验结果、结构统计、
  资源文件与 frontmatter 字段；提供保存、另存为、导出 ZIP、校验、
  在文件管理器中显示。
- **无损保持**：未知 frontmatter 字段、中文与 Unicode、资源文件在保存后保留；
  frontmatter 解析失败时以原始文本打开，**可在应用内修复**，修复前禁止保存。
- **新建 / 导入 / 导出**：新建 Skill；导入目录、`SKILL.md` 或 `.zip`；
  导出 `.zip`；另存为会连同资源文件一起复制。
- **校验系统**：`SPEC-*`（规范错误）/ `PLAT-*`（平台兼容警告）/
  `QUAL-*`（质量建议）三类共 20 条规则，带严重级别、来源、可选行号、
  建议与规则文档链接。
- **平台适配**：`claude-code`、`codex`、`opencode`、`deepseek-harness`、
  `deepseek-harness-desktop`、`generic` 六个 adapter，
  各自声明默认路径、支持字段、列表长度上限、资源目录、核实日期与三级验证状态；
  支持自定义路径覆盖。DeepSeek Harness 官方会扫描两个用户级目录
  （`~/.dsh/skills` 与 `~/.agents/skills`），因此目标行带「Harness 专用」/
  「跨 Agent 共享」徽章，安装与卸载按用户点击的具体路径定位。
- **安装**：差异预览（新增/更新/删除/无变化、是否新建目录、是否备份、
  是否检测到外部修改）→ 确认 → 备份 → 暂存 → 整体切换 → 记录 `sha256` 清单。
- **卸载**：只删除清单内文件，清理空目录，检测并提示外部修改。
- **主题**：浅色 / 深色 / 跟随系统（`prefers-color-scheme`）。
- **设置**：工作区、主题、备份数量（1–50）、外部修改检测开关、
  配置/清单/备份/日志路径显示。
- **诊断**：`log_frontend` 命令把前端错误与就绪信息写 stderr，无需日志框架。
- **打包**：`packaging/make-icons.py`（可复现图标）与
  `packaging/make-macos-app.sh`（手工组装 `.app` + ad-hoc 签名，不使用 Tauri CLI）。

### 安全

- 删除与卸载**后端强制要求 `confirm: true`**，未确认即拒绝。
- 不执行 Skill 内的任何脚本；唯一的进程启动是打开文件管理器与
  **经 http/https 校验**的文档链接。
- 所有资源路径经 `normalize_relative` / `safe_join` 校验，安装目标经
  `lexically_within` 防逃逸；ZIP 导入设条目数（2000）、单文件（50 MiB）、
  总量（200 MiB）上限并防 Zip Slip。
- 保存采用临时文件 + 原子替换；覆盖前自动备份，备份数下限为 1。
- 前端**不使用 `innerHTML`**（Skill 内容是不可信输入），由契约测试强制。
- 只授予 `core:default` 与 `dialog:default` 权限，不开放任意文件系统访问。

### 测试

- 140 个测试：核心单元 79、核心集成 26、命令层 28、前后端契约 7。
- 前后端契约测试会在编译后校验：命令注册表与前端调用一致、
  前端资源存在、配置与能力正确、无 `innerHTML`、参数为 camelCase。

### 规格适用性

- 本实现技术栈为 **Rust + Tauri**（已确认）。主规格的桌面框架条款按
  Tauri 技术栈落地；§7.1 的「不运行本地服务端」仍适用且已满足
  （无服务端、无端口）。见 `docs/decisions/0006-spec-applicability.md`。
- 本地不使用 Tauri CLI，因此不需要 Node.js / npm；发布 CI 例外，使用
  `cargo tauri` 生成安装包。本地需显式启用 `custom-protocol` 特性
  （见 `docs/decisions/0001-toolchain-and-tauri-version.md`）。
- 预览视图不做富文本 Markdown 渲染，而是显示将要写入的全文加结构统计。
- §10（安全）、§11（测试与验收）、§13（代码质量）全部适用并已遵循。

### 未实现 / 待验证

- Windows 与 Linux 的构建、运行、原生对话框与文件管理器集成**从未执行**。
- 初始版本未建立 CI、未做代码签名与公证；已在 2026-10-21 的发布流水线中补齐
  （见上方 `[0.1.0]` 的「发布」段）。
- 规格 §12 阶段 D 第 5 条「安全的自动修复」未实现（三版一致）。
- 无截图；浅色/深色外观与中文输入法行为未人工验证。

详见 `docs/progress.md` 的「待验证」与「未实现」两节。
