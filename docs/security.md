# 安全、隐私与可靠性

本文只描述**已实现并已测试**的机制。未实测项明确标注「待验证」。

## 1. 隐私：本地优先

| 承诺 | 实现方式 | 证据 |
| --- | --- | --- |
| 不需要登录 | 没有账号、没有 token、没有 OAuth | `.env`、密钥、账号相关代码为零 |
| 默认不联网 | 唯一的出网入口是用户在平台页/问题卡上点击的「官方文档」链接 | `shell::open_url` 只接受 `http`/`https`，且仅在用户点击时调用 |
| 不创建本地 HTTP 服务 | 没有 `axum`/`hyper`/`actix` 依赖，没有监听端口 | 依赖清单；前端由 Tauri 自定义协议从可执行文件内提供 |
| 不上传 Skill 内容 | 没有任何上传代码路径 | — |
| 不在日志中记录正文 | `log_frontend` 只接受前端主动给出的短消息；Rust 侧诊断只写路径与校验信息 | `commands.rs::log_frontend` |
| 无遥测 | 没有分析 SDK | 依赖清单 |

`Cargo.lock` 中的网络相关依赖只来自 Tauri/`wry`/`zip` 的构建期与运行时需要，
其中唯一在运行时可能出网的是系统 WebView 自身——而本应用的页面不加载任何远程资源
（CSP `default-src 'self'`）。

## 2. 不执行 Skill 内容

**规格 §10.2 的硬性要求：不自动运行脚本或执行外部命令。**

实现：

- 核心库没有任何 `Command::new` 调用去执行 Skill 内的东西。
- 唯一的进程启动在 `src-tauri/src/shell.rs`，只有两种用途：
  1. 打开系统文件管理器并定位一个路径（`open -R` / `explorer /select,` / `xdg-open`）；
  2. 用默认浏览器打开一个**经校验的 http(s)** 文档链接。
- 这两者都只在用户显式点击时发生；`shell.rs` 的文档注释把这一约束写死。
- 导入 ZIP 只是解压，从不执行其中的任何文件；`scripts/` 目录被当作普通资源
  复制与保留，从不运行。

测试：`shell.rs::tests::open_url_rejects_unsafe_schemes_without_spawning`
断言 `file:///etc/passwd` 被拒绝且**没有 spawn 进程**。

## 3. 路径安全

全部在 `crates/skillforge-core/src/domain/path_safety.rs`：

| 函数 | 作用 |
| --- | --- |
| `normalize_relative` | 拒绝绝对路径与 `..` 段，规范化（`a/./b` → `a/b`） |
| `safe_join` | 在 root 下拼接相对路径，越界即报错 |
| `lexically_within` | 纯词法判断候选是否在 root 内 |
| `is_within_root_canonical` | 解析符号链接后的真实路径判断 |
| `to_slash` | 统一成 `/` 分隔用于清单与显示 |

应用点：

- **安装**：`apply_install` 对每个源相对路径调用 `normalize_relative`，
  并断言 `target_root` 词法上仍在用户选择的 `target_parent` 内
  （否则返回「目标路径逃逸出所选目录」）。平台投影产生的 `overrides`
  键也走 `normalize_relative`，写入时再用 `safe_join` 限制在暂存目录内。
- **导入**：ZIP 条目全部经路径校验，拒绝 Zip Slip
  （`../../etc/...` 之类的条目不会写到目标之外）。
- **卸载**：`apply_uninstall` 用 `safe_join(&target_root, rel)` 得到绝对路径，
  再删除。

## 4. 符号链接策略

**默认不跟随符号链接写入根目录之外。**

- `fs_util::walk` 用 `symlink_metadata` 判断，遇到符号链接时**记录但不遍历**。
- `fs_util::copy_dir_contents` **跳过**符号链接并报告
  （`CopyReport.skipped_symlinks` → `InstallReport.skipped_symlinks`）。
- `fs_util::remove_dir_all_safe` 对符号链接只删除链接本身，不递归其目标。
- 清单记录会跳过符号链接条目。

**待验证**：`save_skill_as` 复制资源文件时的符号链接行为**没有**命令层测试覆盖
（核心层的 `copy_dir_contents` 有测试）。这是已知的测试空白，不是已知缺陷。

## 5. 覆盖与删除必须确认

- **后端强制**：`delete_skill(id, confirm)` 与 `uninstall(platform, scope, confirm)`
  在 `confirm != true` 时**直接拒绝并返回警告状态**，不做任何写入。
  即使前端有 bug 或有人直接调 IPC 也删不掉东西。
- **前端二次确认**：删除与卸载都弹出模态确认，写清将删除的绝对路径与文件数。
- **覆盖前备份**：安装时若目标已存在，先复制到
  `<备份根>/<skill 名>/<时间戳>`，再落盘。
  - 备份数量由设置控制（1–50，默认见 `Settings`），超出后修剪最旧的。
  - `BackupService::new` 强制 `keep >= 1`，不可能配成「不保留任何备份」。
- **保存 Skill**：`save` 在创建新目录前校验名称，并**拒绝**写入已存在的目录
  （不会静默接管同名目录）。`save_skill_as` 同样拒绝已存在的 `SKILL.md`。
- **字段投影**：安装时只改写暂存副本（`plan.overrides` → staging），
  工作区源文件不写；预览会列出映射，有兼容提醒时需显式点「仍然安装」。
  清单记录投影后内容的哈希与 `from` / `to`，后续外部修改检测不会误报。

## 6. 外部修改检测

「安装之后就没人动过」是备份与卸载正确性的前提，因此显式检测：

- `Manifest` 记录每个文件的 **sha256**。
- 安装预览时：若目标现存文件与清单哈希不一致 → `has_external_modification = true`
  → 差异预览里显示黄色警告「…在本应用之外被修改过；继续安装会覆盖它们（已保留备份）」。
- 卸载预览时：若文件哈希与清单不一致 → `has_external_changes = true`
  → 确认框里警告「仍会删除它们」。
- 清单缺失时不会误删：卸载计划只包含**清单里记录过**的文件。

测试：`ban` 核心集成测试 `install_safety.rs` 与命令层测试覆盖了
「外部修改被检测到」的路径。

## 7. 原子写入与数据完整性

`fs_util::atomic_write`：

1. 在**目标同目录**创建临时文件（`tempfile`，前缀 `.skillforge-tmp-`）；
2. 写入 → `flush` → `sync_all`（落盘）；
3. `persist` 原子替换目标；
4. Windows 上若因目标存在而失败，先删除目标再重建重试一次。

因此写 `SKILL.md` / `config.json` / 清单时，**不会出现半写入的文件**。

安装的整体切换也用「先构建完整、再整体改名」的策略（见 `architecture.md` §6），
失败时把旧目录改回。

## 8. ZIP 导入的防御性上限

`services/import_export_service.rs`：

| 常量 | 值 | 作用 |
| --- | --- | --- |
| `MAX_ZIP_ENTRIES` | 2000 | 条目数上限，防「解压炸弹」的条目维度 |
| `MAX_ZIP_TOTAL_BYTES` | 200 MiB | 解压后总大小上限 |
| `MAX_ZIP_FILE_BYTES` | 50 MiB | 单个条目上限 |

读取时按 `MAX_ZIP_FILE_BYTES + 1` 读取以判断是否超限；累计大小超过总上限立即中止。
每条相对路径都经路径安全校验。

## 9. 面板与依赖面

- 只使用一个 Tauri 插件：`tauri-plugin-dialog`（原生选择器）。
- 能力文件（`capabilities/default.json`）只授予
  `core:default` 与 `dialog:default`——**没有**授予任意文件系统访问
  （不引入 `tauri-plugin-fs`）。
- CSP：`default-src 'self'`，`script-src 'self'`，无 `'unsafe-eval'`。
- `unsafe_code = "deny"`：应用自身代码零 `unsafe`（Tauri/`wry` 内部的 unsafe
  属于依赖，不在本项目代码范围内）。
- 白名单方式：`generate_handler!` 显式列出 30 个命令，
  契约测试保证前端调用的命令都已注册、已注册的命令都被使用。
- 只读的 `frontmatter_guide` 只返回内存中的字段/规则目录与 adapter 能力，
  不读写技能文件；前端不得用它打开字段/YAML 编辑器。

## 10. 前端 XSS 防护

Skill 的 `name`、`description`、正文、路径**都是不可信输入**（用户可以导入任意
`SKILL.md`）。因此：

- `ui/app.js` 所有插入一律用 `document.createTextNode` / `textContent`；
  不存在 `innerHTML` / `insertAdjacentHTML` / `document.write`。
- 由 `src-tauri/tests/frontend_contract.rs` 机械校验（先剥离注释避免误报）。
- `open_url` 只接受 `http`/`https`，因此即使 `doc_url` 被伪造也无法触发
  任意 URI scheme（如 `javascript:`）。

**待验证**：未做真实的恶意 `SKILL.md` 端到端渗透测试；
上述是静态约束与单元测试层面的保证。

## 11. 可靠性与错误信息

- 所有可预期的失败（文件不存在、权限不足、磁盘空间、YAML 语法错误、
  目标逃逸、名称非法）都返回**可读的中文错误**，写到状态栏，不 panic。
- 未保存的修改在关闭窗口时被拦截并提示（`skillforge://unsaved-edits` 事件）。
- 外部文件变化不会静默覆盖内存中的编辑：`save` 只在用户点击时写盘，
  且写前重新校验；文件被外部删除时会得到明确错误。
- **没有**实现文件系统监听（watch）。因此在应用外部修改文件后，
  需要用户点「刷新」才能看到——这是刻意的取舍（避免引入 watcher 依赖与竞态）。

## 12. 已知安全相关的空白（如实声明）

| 空白 | 影响 | 状态 |
| --- | --- | --- |
| Windows/Linux 上的路径与 shell 分支从未运行 | 可能在该平台上行为不符预期 | 待验证 |
| 本地打包未做正式代码签名/公证 | 本地 `.app` 仍会触发 Gatekeeper 提示 | 发布工作流已接入签名/公证；首次签名发布待验收 |
| `save_skill_as` 的符号链接行为无命令层测试 | 理论上有拷贝链接目标的风险 | 待验证（核心层已跳过符号链接） |
| 没有文件系统监听 | 外部改动需手动刷新 | 设计取舍 |
| 未做渗透测试 | 只能保证已实现的静态约束 | 待验证 |

本项目**不做安全审计**，也不声称做了。界面上的校验是**语法/规范校验**，
不能当作安全性保证（规格 §10.2 明确要求不得把语法校验描述为安全审计）。
