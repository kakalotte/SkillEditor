# 实际架构

本文描述**已经实现并测试通过**的架构，不是设想。

## 1. 全局结构

```
skillforge-tauri/
├── Cargo.toml                     # workspace：成员、版本、依赖、lint、release profile
├── Cargo.lock                     # 已提交
├── crates/skillforge-core/        # 纯 Rust 核心库（UI 无关）—— 6527 行
│   ├── src/
│   │   ├── lib.rs
│   │   ├── errors.rs
│   │   ├── guide.rs               # 只读 frontmatter 字段/规则目录 + 示例
│   │   ├── domain/                # naming, path_safety, resource, skill, validation
│   │   ├── formats/               # frontmatter, markdown, skill_md
│   │   ├── persistence/           # settings
│   │   ├── platforms/             # adapter trait + 6 个实现 + common
│   │   └── services/              # workspace, validation, install, backup, import_export, settings, fs_util
│   └── tests/                     # 4 个集成测试（772 行）+ fixtures
├── .github/workflows/             # CI + 三平台 Draft Release（Tauri CLI 仅用于发布）
├── src-tauri/                     # Tauri 壳（bin crate `skillforge`）
│   ├── Cargo.toml                 # default = ["custom-protocol"]
│   ├── build.rs                   # tauri_build::build()
│   ├── tauri.conf.json            # 窗口、CSP、frontendDist、bundle
│   ├── capabilities/default.json  # core:default + dialog:default
│   ├── icons/                     # 由 packaging/make-icons.py 生成
│   ├── src/                       # 2981 行
│   │   ├── main.rs                # builder、插件、状态、invoke_handler、关闭拦截
│   │   ├── app.rs                 # 全部业务桥接逻辑 + 32 个测试（2053 行）
│   │   ├── commands.rs            # 30 个 #[tauri::command] 薄壳（321 行）
│   │   ├── dto.rs                 # 可序列化视图模型（416 行）
│   │   └── shell.rs               # 打开文件管理器 / URL（125 行）
│   └── tests/frontend_contract.rs # 前后端契约测试（444 行）
├── ui/                            # 前端资源，编译期嵌入（2810 行）
│   ├── index.html                 # 静态骨架（95 行）
│   ├── styles.css                 # 浅色/深色两套变量（1014 行）
│   └── app.js                     # 渲染 + IPC（1701 行）
├── packaging/
│   ├── make-icons.py              # 可复现图标生成
│   └── make-macos-app.sh          # 手工组装 .app（不用 Tauri CLI）
├── rust-toolchain.toml            # 固定工具链为 1.99.1
└── docs/
```

约 **13600 行**（Rust + JS + CSS + HTML + 脚本，不含 `target/`）。
其中核心库（含测试）5475 行。

## 2. 三层职责

```
┌───────────────────────────────────────────────────────────────┐
│ 前端 ui/app.js                                                │
│    · 只做渲染；不持有业务状态（4 个页面 + 3 个模态框）        │
│    · 文本编辑防抖 + 冲刷；所有数据用 textContent 插入         │
└───────────────────────────┬───────────────────────────────────┘
                            │ window.__TAURI__.core.invoke
                            │ 非文本命令 → Snapshot；文本命令 → EditAck
┌───────────────────────────┴───────────────────────────────────┐
│ src-tauri                                                     │
│   commands.rs  薄壳：取锁、弹原生对话框、错误转字符串          │
│   app.rs       全部业务逻辑（纯函数，接收 &Core / &mut Core） │
│   dto.rs       序列化边界（camelCase）                         │
│   shell.rs     打开文件管理器 / 浏览器链接                     │
└───────────────────────────┬───────────────────────────────────┘
                            │ 直接函数调用（同一个进程）
┌───────────────────────────┴───────────────────────────────────┐
│ crates/skillforge-core                                        │
│   domain    模型 + 校验规则 + 路径安全                        │
│   formats   frontmatter(YAML) / skill_md / markdown 统计       │
│   platforms adapter trait + 6 实现                            │
│   services  工作区扫描 / 校验 / 安装 / 备份 / 导入导出         │
│   persistence  设置读写                                        │
└───────────────────────────────────────────────────────────────┘
```

**关键边界**：核心库不依赖 Tauri、WebView 或任何 GUI 工具包（`lib.rs` 的文档注释明确了
这一点），因此可以在没有显示服务器的情况下测试。前端不直接访问文件系统。

## 3. 状态与所有权

```rust
// src-tauri/src/commands.rs
pub struct AppState(Mutex<Core>);

// src-tauri/src/app.rs
pub struct Core {
    settings_service: SettingsService,   // 读写 config.json
    settings: Settings,                  // 已解析的配置
    workspace: WorkspaceService,         // 工作区扫描
    backups: BackupService,              // 覆盖前备份（保留 N 份）
    manifests: ManifestStore,            // 安装清单（按目标路径哈希索引）
    project_dir: Option<PathBuf>,        // 当前项目目录（用于项目级目标）
    adapters: Vec<Box<dyn PlatformAdapter>>,
    entries: Vec<SkillEntry>,            // 列表项的缓存
    open: Option<SkillDocument>,         // 当前打开的文档（含脏标记）
    search: String,
    pending_plan: Option<InstallPlan>,   // 「预览 → 确认」之间的暂存
    next_draft_seq: u64,                 // 未保存草稿的 id 序号
    status: Status,                      // 状态栏消息（kind + text）
}
```

- 单个 `Mutex<Core>`：命令都在主线程或短任务里执行，不会成为瓶颈；
  唯一可能长时间阻塞的是原生对话框，因此**选择器命令是 `async`**，
  用 `blocking_pick_*`（插件内部会切回主线程）先取得路径，再短暂持锁。
- `pending_plan` 是「安装预览 → 确认」之间的唯一暂存；`cancel_install`
  只是清掉它，因此**取消永远不会产生半成品**。
  卸载不缓存计划：`preview_uninstall` 与 `uninstall` 都从清单
  （`ManifestStore`）重新计算，避免出现「预览时与执行时目标已变化」的窗口。
- `Core::from_roots(config_root, data_root)` 是测试专用的构造入口，
  接收显式根目录；生产走 `Core::from_environment()`（`dirs` 解析真实用户目录）。

## 4. IPC 契约

共 **30 个命令**，按返回类型分四类。

### 4.1 返回完整 `Snapshot`（18 个）

`boot`、`refresh_skills`、`set_search`、`new_skill`、`open_skill`、`delete_skill`、
`save`、`validate_now`、`import_skill`、`export_skill`、`save_skill_as`、
`choose_workspace`、`refresh_platforms`、`confirm_install`、`cancel_install`、
`uninstall`、`update_settings`、`save_settings`。

```rust
pub struct Snapshot {
    app_version: String, workspace: String, project_dir: String,
    status: Status, theme: String, search: String,
    skills: Vec<SkillItem>, editor: Option<EditorItem>,
    issues: Vec<IssueItem>, counts: Counts,
    platforms: Vec<PlatformItem>, targets: Vec<TargetItem>,
    settings: SettingsDto,
}
```

前端 `applySnapshot()` 是纯渲染，不做任何推导。

### 4.2 返回轻量 `EditAck`（3 个文本命令）

`set_name`、`set_description`、`set_body`：

```rust
pub struct EditAck {
    dirty: bool, parse_ok: bool, read_only: bool,
    frontmatter_keys: Vec<String>, stats: MarkdownStats,
    issues: Vec<IssueItem>, counts: Counts,
}
```

前端 `applyAck()` 只更新侧栏，**不重建正在输入的控件**，从而不丢光标。

### 4.3 返回其它具体类型（4 个）

| 命令 | 返回 | 说明 |
| --- | --- | --- |
| `preview_install` | `InstallPreview` | 差异预览 + 平台兼容提醒 + 字段映射，前端弹模态框 |
| `preview_uninstall` | `UninstallPreview` | 卸载预览 |
| `preview_text` | `String` | 将要写入磁盘的 `SKILL.md` 全文 |
| `frontmatter_guide` | `FrontmatterGuide` | 只读字段/规则指南；不返回 `Snapshot`，不改变文档或脏标记 |

`frontmatter_guide` 的字段与规则文本来自 `crates/skillforge-core/src/guide.rs`；
app 层再用当前 adapter 的 `supports_field` / `listing_limit` 补上平台支持集合与
列表上限。因此 JavaScript 不维护第二份平台字段知识。前端只通过 `call()` 读取它，
避免把非 `Snapshot` 结果交给 `applySnapshot()`。

`preview_install` / `preview_uninstall` 以及 `uninstall` 都接受可选的 `path`：
前端回传用户点击的目标行路径。后端只用它在该平台已发现的目录中匹配，
不会接受任意路径；未传时退回按 `scope` 解析（兼容旧调用与测试）。

`InstallPreview` 还包含 `warnings`（该平台的 `PLAT-*` 兼容提醒）与
`rewrites`（安装时将执行的字段映射）。前端据此显示提醒，并把确认按钮
改为「仍然安装」。平台卡片上的 `compatStatus` / `compatWarnings` /
`compatRewrites` 来自同一套 adapter 方法，避免两处判断不一致。

### 4.4 只执行副作用、不返回界面状态（5 个）

| 命令 | 返回 | 说明 |
| --- | --- | --- |
| `reveal_skill` | `()` | 在文件管理器中定位 Skill 目录 |
| `open_target` | `()` | 打开一个目标目录 |
| `open_config_dir` | `()` | 打开配置目录 |
| `open_url` | `()` | 用浏览器打开经校验的 http(s) 链接 |
| `log_frontend` | `()` | 把前端诊断写到 stderr |

这些命令不改动应用状态，因此不返回快照；失败时返回可读错误字符串。

### 4.5 一个特例：`preview_text`

预览视图需要正文全文。如果把它塞进每次 `EditAck`，每个按键都要传一遍全文，
浪费且会破坏「不重建控件」。因此单独一个 `preview_text() -> Result<String, String>`，
只在切到预览标签时按需调用。

### 4.6 事件

窗口关闭被拦截时，`main.rs` 向 WebView 发 `skillforge://unsaved-edits`
事件，前端显示警告（不阻止关闭，由用户决定）。

## 5. 核心服务

| 模块 | 职责 | 要点 |
| --- | --- | --- |
| `services/workspace_service.rs` | 扫描工作区、列出资源、时间戳 | 只看一层目录；`SKILL.md` 缺失的目录不算 Skill |
| `services/validation_service.rs` | 生成分来源分级的校验报告 | 规则 id 稳定；推荐章节只报建议；平台规则每个 adapter 只跑一次 |
| `services/install_service.rs` | 规划、应用、卸载安装 | `InstallPlan` / `Manifest` / `ManifestStore`；支持平台适配后的文件覆盖（`overrides`），哈希以最终内容为准，清单记录投影 `from` / `to` |
| `services/backup_service.rs` | 覆盖前备份，保留 N 份 | `<root>/<skill>/<timestamp>`，按数量修剪 |
| `services/import_export_service.rs` | ZIP 导入导出 | 条目数/单文件/总大小上限；防 Zip Slip |
| `services/fs_util.rs` | 原子写入、遍历、目录复制 | `atomic_write` 临时文件 + persist；`walk` 不跟随符号链接 |
| `services/settings_service.rs` | 配置读写 | JSON；未知字段忽略（向前兼容） |
| `domain/path_safety.rs` | 相对路径规范化与越界判定 | `normalize_relative`、`safe_join`、`lexically_within` |

## 6. 安装流水线

```
preview_install(platform, scope, path?)
  └─ 有未保存修改 → 拒绝（避免安装磁盘上的旧内容）
  └─ 解析目标：有 path 时只接受该平台已发现的目录（多目录平台用）
  └─ 读磁盘 SKILL.md → adapter.project(text) → overrides + rewrites
       └─ 只改有明确等价关系的顶层键；源文件不写
  └─ adapter.validate(投影后的文档) → 剩余平台兼容提醒（每平台一次）
  └─ 构建 InstallPlan（overrides 参与哈希）
       ├─ 源文件树 walk（不跟随符号链接）
       ├─ 与目标现有文件按 sha256 比较 → ChangeKind
       ├─ 读旧清单，检测「安装后被外部修改」
       └─ 判断是否需要备份（目标已存在且不同）

confirm_install()
  └─ apply_install(plan, backups, manifests, timestamp, app_version)
       1. 校验每个源相对路径（拒绝 `..` 与绝对路径）
       2. 校验目标仍在所选父目录内（防逃逸）
       3. 目标已存在 → 备份到 <backup>/<skill>/<ts>
       4. 在同级文件系统上建暂存目录，复制全部内容
       5. 把 overrides 写入暂存副本（只影响副本）
       6. 校验暂存目录里有 SKILL.md，否则中止（此时目标未被触碰）
       7. 交换：目标改名为 .skillforge-old-<ts> → 暂存改名到目标位置
          → 删除旧目录；第二步失败时把旧目录改回（可恢复）
       8. 遍历最终内容，记录 Manifest（相对路径 → sha256，以及投影 from/to）
```

`preview_install` 阶段的 `has_external_modification` 与 `will_backup`
会显示在差异预览里；确认后才落盘。暂存目录与目标目录的交换发生在同一文件系统内，
因此 `rename` 是有效操作。

字段投影只作用于安装副本：源 `SKILL.md` 不写、不改，安装清单记录的是
**投影后**内容的哈希。这样再次安装同一投影会显示「无变化」，
卸载与外部修改检测也不会误报。没有投影的平台 `overrides` 为空，
流水线与旧版本完全一致。

## 7. 卸载流水线

```
preview_uninstall → 读清单，统计将被删除的文件数，检测外部修改
uninstall(confirm=true) → 只删除清单内的文件 → 清理空目录 → 删除清单
```

**后端强制要求 `confirm: true`**：即使前端出现 bug 或有人直接调 IPC，
未确认的删除/卸载都会被拒绝。删除 Skill 目录同样如此。

## 8. 前端结构

`app.js` 是一个 IIFE，按职责分块：

| 区块 | 内容 |
| --- | --- |
| DOM helpers | `el` / `append` / `clear` / `badge` / `field` / `button`——只用 `textContent` |
| IPC | `call` / `run` / `runAfterFlush` / `log` |
| 主题 | `applyTheme` 解析 `system` → `data-theme`，监听 `prefers-color-scheme` |
| 快照 | `applySnapshot` / `applyAck` |
| 防抖编辑 | `queueEdit` / `flushEdits` |
| 页面 | `buildXShell`（只建一次）+ `renderX`（只更新内容） |
| 模态框 | 安装差异预览、通用确认（删除/卸载）、只读 Frontmatter 指南 |
| 装配 | `wireChrome` / `wireCloseGuard` / `main` |

**「建一次 + 只更新」的模式**是为避免输入控件被重建而丢焦点：
搜索框、name/description、正文、设置项都建一次，后续只改值。

### 契约测试（`src-tauri/tests/frontend_contract.rs`）

前端没有类型系统，这些测试用静态分析补上：

1. 前端调用的每个命令都必须在 `generate_handler!` 中注册。
2. 每个已注册的命令都必须被前端调用（防止死命令）。
3. `index.html` 引用的 `app.js` / `styles.css` 必须存在。
4. `frontendDist` 必须指向含 `index.html` 的真实目录。
5. `withGlobalTauri` 必须为 `true`；identifier 与窗口 label/title 必须符合预期。
6. capabilities 必须授予 `core:default` 与 `dialog:default`。
7. `app.js` 不得使用 `innerHTML` / `insertAdjacentHTML` / `document.write`
   （先剥离注释，避免注释误报）。
8. 命令参数必须是 camelCase（`platformId:` 而非 `platform_id:`）。
9. Frontmatter 指南必须是只读模态框：包含搜索/分页/状态/内容容器，不得含
   `textarea` 或 `contenteditable`，且必须通过 `call("frontmatter_guide")` 读取。
   状态容器只在「必填字段」页且无搜索时显示；平台列表上限只放在「平台扩展」页。

## 9. 测试分层

| 层 | 位置 | 数量 | 特点 |
| --- | --- | --- | --- |
| 核心单元 | `crates/skillforge-core/src/**` 内 `#[cfg(test)]` | 94 | 纯逻辑，无 I/O |
| 核心集成 | `crates/skillforge-core/tests/` | 30 | 用临时 HOME/目录与 fixtures |
| 命令层 | `src-tauri/src/app.rs` 内 `#[cfg(test)]` | 32 | 完整用户流程，用临时 config/data 根目录 |
| 前后端契约 | `src-tauri/tests/frontend_contract.rs` | 9 | 静态分析前端资源、配置与版本 |
| **合计** | | **165** | |

所有测试都**不依赖开发者真实配置**：`Core::from_roots`（测试专用）接收显式的
home/config/data 根目录，`PlatformContext` 由它构造。

## 10. 构建与打包

```bash
cargo build --release -p skillforge        # 前端在编译期嵌入
packaging/make-macos-app.sh                # 组装 .app + ad-hoc 签名（仅本地）
open dist/SkillForge.app
```

正式安装包由 `.github/workflows/release.yml` 在原生 runner 上使用 Tauri CLI
生成：两个 macOS DMG（签名 + 公证）、Windows NSIS/MSI（签名）、Linux
`.deb`/`.AppImage`。本地与 CI 的 CLI 边界、`custom-protocol` 特性见
`docs/decisions/0001-toolchain-and-tauri-version.md`，发布步骤见
`docs/releasing.md`。

`[profile.release]` 开启 `lto = "thin"`、`codegen-units = 1`、`strip = true`。
