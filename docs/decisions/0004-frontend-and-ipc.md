# 0004 — 前端方案与前后端 IPC 契约

- 状态：已接受
- 日期：2026-10-17

## 背景

本实现的技术栈是 **Rust + Tauri**（见 `0006-spec-applicability.md`），
因此渲染层是系统 WebView（macOS 为 WKWebView，Windows 为 WebView2，
Linux 为 WebKitGTK）。主规格 §7.1 的「不引入 WebView」与 Tauri 的渲染方式冲突，
**按 Tauri 方案实现**；其中的「不运行本地服务端」仍适用且已满足。

规格 §5 要求界面有 4 个页面、编辑器源码/预览双视图、校验结果列表、
平台适配页与安装差异预览；§13 要求避免过度架构。

要在满足 §5 的前提下，又不引入庞大前端工具链。

## 决策

### 1. 技术栈与渲染层

| 事项 | 本实现 |
| --- | --- |
| 渲染层 | 系统 WebView（Tauri 的前提） |
| 本地服务端 | **没有**。前端资源由 Tauri 的 `tauri://`（Windows 上为 `http://tauri.localhost`）自定义协议直接从可执行文件内提供，没有监听端口、没有额外进程 |

即：有 WebView，但没有网络服务、没有端口。

### 2. 前端不用框架、不用 npm、不用打包器

`ui/` 下只有三个文件：

- `index.html`：静态骨架，只声明元素 id/class，不含任何逻辑。
- `styles.css`：浅色/深色两套 CSS 自定义属性（`data-theme` 决定用哪套）。
- `app.js`：一个 IIFE，`"use strict"`，无依赖。

**理由**：本实现的核心卖点是「单一 Rust 工具链、一条 `cargo build` 命令」。
引入 Vite/React 会立刻带来 Node.js、npm 锁文件、前端构建产物的版本管理，
与 `0001` 的技术选型相矛盾。

**代价**：没有类型检查、没有组件复用、DOM 代码是手写的。用下面的契约测试补偿。

启用 `window.__TAURI__` 只需 `tauri.conf.json` 里一行 `withGlobalTauri: true`，
不需要 `import`，因此也不需要打包器。

### 3. 快照式 IPC：一个命令 → 一份完整界面状态

这是本设计最关键的约定。

```rust
// 每一个会改变状态的非文本命令，都返回完整的 Snapshot
#[tauri::command] fn save(...)      -> Result<Snapshot, String>
#[tauri::command] fn open_skill(...)-> Result<Snapshot, String>
#[tauri::command] fn confirm_install(...) -> Result<Snapshot, String>
```

`Snapshot` 包含：应用版本、工作区路径、项目目录、状态栏消息、主题、搜索词、
技能列表、当前打开的文档、校验问题、计数、平台列表、安装目标、设置。

前端 `applySnapshot()` 是**纯渲染**——不做任何状态推导、不缓存业务状态。
所有业务逻辑都在 Rust 侧，前端只是显示器。这样：

- 不存在「前端状态与后端不一致」这类 bug。
- Rust 侧可以用临时目录直接测试完整的用户流程（`app.rs` 里 27 个测试），
  不需要窗口、不需要 WebView。

**包装函数必须与返回类型配对**（这是实际踩过的坑）：

| 前端函数 | 用于 | 行为 |
| --- | --- | --- |
| `run()` / `runAfterFlush()` | **只能**用于返回 `Snapshot` 的命令 | 把返回值交给 `applySnapshot()` |
| `call()` | 其他所有命令 | 只返回结果，不渲染 |

`run()` 还带形状检查：返回值没有 `skills` 数组时报错并**保持 `state.snapshot` 不变**，
避免一次调用错误把整个界面污染成不可渲染状态。
`frontend_contract.rs` 的 `run_is_only_used_for_snapshot_commands` 静态校验这条规则：
它把 `commands.rs` 的返回类型与 `app.js` 的调用点配对，用错包装函数会让测试失败。

### 4. 文本编辑例外：`EditAck` 而不是 `Snapshot`

如果 `set_body` 也返回 `Snapshot`，前端会重建 `<textarea>`，
光标位置和输入法状态会丢失——这是不可接受的。

因此三个文本命令返回**轻量确认**：

```rust
pub struct EditAck {
    pub dirty: bool,
    pub parse_ok: bool,
    pub read_only: bool,
    pub frontmatter_keys: Vec<String>,
    pub stats: MarkdownStats,
    pub issues: Vec<IssueItem>,
    pub counts: Counts,
}
```

前端 `applyAck()` **只更新侧栏**（校验结果、结构统计、frontmatter 键、脏标记），
绝不触碰正在输入的元素。

### 5. 防抖 + 强制冲刷

- 输入防抖 160 ms（`queueEdit`）。
- **任何读取文档的命令都先 `await flushEdits()`**（保存、另存为、导出、校验、
  安装预览、卸载预览）。
- 因此「打字后立刻点保存」不会丢最后几个字符。

后端不跟踪「当前页面」，页面切换（技能/编辑器/平台/设置）完全由前端
`state.page` 决定——这是纯视图状态，放后端没有意义。

### 6. 安全：只用 `textContent`，永不 `innerHTML`

Skill 的 `name`/`description`/正文/路径全部来自用户的 `SKILL.md`，
**是不可信输入**。前端所有插入都用 `document.createTextNode` / `textContent`，
`app.js` 里不存在 `innerHTML`、`insertAdjacentHTML`、`document.write`。

这条由契约测试机械校验（会先剥离注释，避免注释里的字样误报）。

### 7. 真正的逻辑放在 `app.rs`，`commands.rs` 只是薄壳

```rust
// app.rs —— 纯函数，接收 &Core / &mut Core，可测试
pub fn save(core: &mut Core) -> Result<Snapshot, String>

// commands.rs —— 只做三件事：取锁、弹原生对话框、把错误转成字符串
#[tauri::command]
pub fn save(state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, |core| app::save(core))
}
```

`app.rs` 里的 27 个测试用临时 config/data 根目录跑完整流程
（新建→保存→重载、导入导出、安装→备份→卸载、外部修改检测、路径穿越拒绝），
不启动窗口。

### 8. 只用一个插件

只依赖 `tauri-plugin-dialog`（原生文件/目录选择器）。其余两处需要「打开外部东西」
的地方用自定义 Rust 命令：

- `open_target` / `reveal_skill`：`std::process::Command` 调用 `open -R` /
  `explorer /select,` / `xdg-open`（跨平台分支见 `shell.rs`）。
- `open_url`：同上的 URL 分支，并且**只接受 `http`/`https`**，
  由 `shell::open_url` 校验（测试覆盖：`javascript:`、`file:` 被拒且不 spawn 进程）。

没有引入 `tauri-plugin-opener`，把依赖面压在最小。

## 后果

- ✅ 前端零依赖、零构建；整个仓库仍然只需 Rust 工具链。
- ✅ 业务逻辑 100% 可测试，且已经有 27 个命令层测试 + 6 个契约测试。
- ✅ 文本输入不会因重渲染丢光标。
- ℹ️ 渲染层是系统 WebView（Tauri 的选型结果，见 `0006`）。
- ⚠️ 前端无类型系统。命令名/参数名写错只有运行时才炸。缓解：
  `src-tauri/tests/frontend_contract.rs` 校验
  前端调用的命令都已注册、已注册的命令都被使用、参数为 camelCase、
  资源存在、能力文件已授权。
- ⚠️ 预览视图不做富文本 Markdown 渲染：`preview_text` 返回将要写入磁盘的
  `SKILL.md` 全文，配结构统计（行数/标题/列表项/代码块/链接/≈tokens）。
  **这是刻意的**——「所见即所存」比渲染得好看更重要，也避免再引入一个 Markdown 库。
- ⚠️ `tauri.conf.json` 的 `withGlobalTauri: true` 会把 API 挂到 `window` 上。
  截断的 CSP 见下；因为不加载任何远程资源，风险面很小。

## CSP

```json
"csp": "default-src 'self'; img-src 'self' asset: data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self' ipc: http://ipc.localhost"
```

`style-src` 需要 `'unsafe-inline'` 是因为部分布局用内联 `style` 属性
（例如 `margin-top:10px`）。没有 `'unsafe-eval'`，`script-src` 只有 `'self'`。
`connect-src` 里的 `ipc:` / `http://ipc.localhost` 是 Tauri 自身 IPC 通道所需，
不是外部网络访问——`default-src 'self'` 保证不加载任何远程资源。

## 前端能力对用户的价值

界面上的每一项都对应一条真实能力，没有装饰性控件：

- 技能页：搜索、新建、导入、刷新、打开、在文件管理器中显示、删除（带确认）。
- 编辑器：name/description 表单、源码/预览切换、保存、另存为、导出 ZIP、校验、
  在文件管理器中显示、结构化侧栏（问题/统计/资源/frontmatter 键）。
- 平台页：检测状态、核实徽章、官方文档链接、支持字段、列表上限、资源目录、
  每个目标的安装预览 / 卸载 / 打开目录。
- 设置页：工作区、主题、备份数量、外部修改检测开关、配置/清单/备份/日志路径。
