# 平台支持与验证状态

所有平台知识都集中在 `crates/skillforge-core/src/platforms/` 的 adapter 中，
UI 代码不包含任何路径字符串。核实日期统一为 **2026-10-17**。

## 1. 主机平台（应用自身能在哪运行）

| 平台 | 构建 | 运行 / 启动 | 原生对话框 | 在文件管理器中显示 | 状态 |
| --- | --- | --- | --- | --- | --- |
| macOS 15.8.1 / arm64 | 实测通过 | 实测通过（直接运行 + `.app` + `open`） | 未逐一点击验证 | 调用系统 `open -R`，未逐一点击验证 | **已实测（构建/运行）** |
| Windows 10/11 | 未构建 | **待验证** | **待验证** | 代码分支存在（`explorer /select,`），**待验证** | **待验证** |
| Linux（GTK 桌面） | 未构建 | **待验证**（需 WebKitGTK） | **待验证** | 代码分支存在（`xdg-open`），**待验证** | **待验证** |

### 1.1 构建与发布流水线

`.github/workflows/ci.yml` 在 macOS、Windows、Ubuntu 22.04 上运行
`cargo fmt`、`cargo clippy`、`cargo test` 与前端语法检查。
`.github/workflows/release.yml` 由 `v*` tag 触发，在原生 runner 上生成：

| 平台 | 产物 | 签名 |
| --- | --- | --- |
| macOS arm64 | `.dmg` | Developer ID + 公证 |
| macOS x86_64 | `.dmg` | Developer ID + 公证 |
| Windows x64 | NSIS `.exe`、`.msi` | Authenticode |
| Linux x64 | `.deb`、`.AppImage` | 未签名 |

工作流已经写入仓库，但 Windows/Linux 的首次 CI 运行尚未发生；在 Actions
真正跑通之前，下表的「待验证」状态不改变。签名需要仓库 Secrets，步骤见
`docs/releasing.md`。

实测环境：

```
macOS 15.8.1 (24H32), Apple Silicon M4
rustc/cargo 1.99.1
tauri 2.12.4, tauri-build 2.7.3, tauri-plugin-dialog 2.8.3
```

### macOS 上的实测证据

```
$ cargo build --release -p skillforge
    Finished `release` profile [optimized] target(s)
$ packaging/make-macos-app.sh
==> ad-hoc signing
==> done: dist/SkillForge.app
$ open dist/SkillForge.app
$ lsappinfo list | grep -A3 SkillForge.app
    bundleID="io.skillforge.desktop"
    bundle path=".../dist/SkillForge.app"
    pid = 42244 type="Foreground" flavor=3 Version="0.1.0" fileType="APPL" Arch=ARM64
```

界面确实渲染的证据（`docs/decisions/0005-diagnostics.md` 描述的通道）：

```
[前端/信息] 界面渲染完成：技能 1，平台 6，目标 7，主题 system
[前端/信息] 已就绪：1 个 Skill，工作区 <HOME>/Library/Application Support/SkillForge/workspace
```

### 未验证的部分（诚实声明）

- Windows / Linux 的**构建与运行从未执行**。代码里有 `cfg(target_os)` 分支，
  但**没有在真实系统上跑过**，因此不声称支持。
- 原生文件/目录选择器的实际弹窗行为未逐一点击验证（需要人工交互）。
- 「在文件管理器中显示」未逐一点击验证。
- 本地 `packaging/make-macos-app.sh` 只做 ad-hoc 签名；正式发布工作流已接入
  Developer ID 签名、公证与 Windows Authenticode 签名，但首次签名发布仍需配置
  GitHub Secrets 并在 Draft Release 中验收。
- 应用图标在 Finder/Dock 中的实际显示未截图验证。

## 2. 目标平台 adapter（Skill 装到哪个工具）

| adapter id | 显示名 | 用户级目标 | 项目级目标 | 核实状态 |
| --- | --- | --- | --- | --- |
| `claude-code` | Claude Code | `~/.claude/skills` | `<项目>/.claude/skills` | 已验证 |
| `codex` | OpenAI Codex | `~/.codex/skills` | `<项目>/.codex/skills` | **兼容但有警告** |
| `opencode` | OpenCode | `~/.config/opencode/skills` | `<项目>/.opencode/skills` | 已验证 |
| `deepseek-harness` | DeepSeek Harness | `~/.dsh/skills`（Harness 专用）<br>`~/.agents/skills`（跨 Agent 共享） | `<项目>/.dsh/skills`<br>`<项目>/.agents/skills` | 已验证 |
| `deepseek-harness-desktop` | DeepSeek Harness Desktop | 同上（Desktop 运行同一套 Harness 核心） | 同上 | **兼容但有警告** |
| `generic` | 通用目录（手动选择） | — | — | 已验证（仅自定义目录） |

每个 adapter 都可以通过「设置 → 自定义路径」额外增加一个目标目录
（`PlatformContext.custom_paths`），UI 上显示为「自定义」范围。

**同一范围多个目录**：DeepSeek Harness 官方文档明确会扫描两个用户级目录
（`~/.dsh/skills` 排 400、`~/.agents/skills` 排 500），所以它有两个「用户级」目标。
UI 用徽章区分（「用户级 · Harness 专用」/「用户级 · 跨 Agent 共享」），
安装/卸载命令会带上用户点击的**具体路径**，而不是只靠 (平台, 范围) 反查，
否则永远只会命中第一个目录。

`DSH_HOME` / `DSH_AGENTS_HOME` 是 DSH 自己的覆盖机制。本应用不做环境变量
推断，始终按默认位置（`~/.dsh`、`~/.agents`）发现目标；改过这两个变量的用户
需要在「设置 → 自定义路径」里为对应平台指向实际目录。UI 会显示解析后的真实路径。

### 2.1 能力对比

| adapter | 支持的 frontmatter 字段 | 列表长度上限 | 资源目录 |
| --- | --- | --- | --- |
| `claude-code` | Agent Skills 规范字段 + Claude Code 扩展 | **1536** 字符 | `scripts`、`references`、`assets` |
| `codex` | Agent Skills 规范字段 | 未声明（不限） | `scripts`、`references`、`assets` |
| `opencode` | Agent Skills 规范字段 | **1024** 字符 | `scripts`、`references`、`assets` |
| `deepseek-harness` | DSH 实际读取的 6 个字段（见下文） | **500** 字符（`catalogDescriptionMaxLength` 默认值） | `scripts`、`references`、`assets` |
| `deepseek-harness-desktop` | 同 `deepseek-harness` | **500** 字符 | 同 `deepseek-harness` |
| `generic` | Agent Skills 规范字段 | 1024 字符 | `scripts`、`references`、`assets` |

Agent Skills 规范字段（`SPEC_FIELDS`）：
`name`、`description`、`license`、`compatibility`、`metadata`、`allowed-tools`。

Claude Code 额外声明（`CLAUDE_FIELDS`）：
`when_to_use`、`argument-hint`、`arguments`、`disable-model-invocation`、
`user-invocable`、`disallowed-tools`、`model`、`effort`、`context`、`agent`、
`background`、`hooks`、`paths`、`shell`。

DeepSeek Harness 声明（`HARNESS_FIELDS`）：
`name`、`description`、`whenToUse`、`metadata`、`disable-model-invocation`、
`user-invocable`。注意 `whenToUse` 是 **camelCase**（DSH 读取的就是这个键），
`when_to_use` 在 DSH 中会被忽略；本应用安装到 DSH 时会把它映射为
`whenToUse`（只改安装副本，见 2.2）。其余字段（含 `license`、`compatibility`、
`allowed-tools`）也会被忽略，因此只报提示性警告，不影响安装。

列表长度上限指平台在**列表/索引**中显示 description 时的截断长度；
超限只报 `PLAT-<ID>-DESC` **警告**，并说明会被截断。计入长度的是平台
实际读取的别名：Claude Code 用 `when_to_use`，DSH 用 `whenToUse`
（源中只有 snake_case 时按投影后的值计算），其他平台忽略别名，只计
description。

兼容警告按 **adapter** 去重：同一平台即使发现多个目标目录
（例如 DSH 的四个根目录），同一字段也只报一次。

### 2.2 安装时字段适配（投影）

只有**文档明确等价**的字段才会被改写，而且只改写**安装副本**：

| 平台 | 工作区源字段 | 安装副本字段 | 依据 |
| --- | --- | --- | --- |
| `deepseek-harness` | `when_to_use` | `whenToUse` | DSH 只读 camelCase 键 |
| `deepseek-harness-desktop` | `when_to_use` | `whenToUse` | 同一套 Harness 核心 |

约束（有测试保护）：

- 只重命名 YAML 顶层的键；嵌套键（如 `metadata.when_to_use`）和正文中的同名文本
  不受影响。
- 当前只匹配未加引号的顶层键；带引号的键保持原样，预览不会显示映射。
  （本应用保存的 `SKILL.md` 总是未加引号的顶层键。）
- 保留注释、键顺序、缩进、换行风格（含 CRLF）与 BOM；不删除任何字段。
- 源文件已经是 `whenToUse`，或同时存在两个键时，不做改写；两者并存时
  警告会说明不会自动合并，`when_to_use` 仍会被忽略。
- `SKILL.md` 之外的资源文件不参与投影。
- 预览弹窗列出 `from → to`；兼容提醒按**投影后的副本**重新校验，
  已被自动适配的字段不会同时报「会被忽略」。
- 安装清单按**实际写入**的副本计算哈希，同时记录投影的 `from` / `to`
  （旧版清单没有该字段也能读取），因此再次安装同一投影会显示「无变化」，
  卸载与外部修改检测也仍然准确。

投影规则由 `PlatformAdapter::project(&str)` 实现，默认返回 `None`（原样安装）。
新增平台时，只有在官方文档给出明确等价关系时才应实现它，不要为语义不同的字段
强行造适配层。

### 2.3 检测逻辑

| adapter | 检测方式 |
| --- | --- |
| `claude-code` | 存在 `~/.claude` 或 `<项目>/.claude` |
| `codex` | 存在 `~/.codex` 或 `<项目>/.codex` |
| `opencode` | 存在 `~/.config/opencode` 或 `<项目>/.opencode` |
| `deepseek-harness` | 存在 `~/.dsh`、`~/.agents` 或项目内 `.dsh` / `.agents` |
| `deepseek-harness-desktop` | 存在 `~/.dsh/profiles/desktop`（桌面端初始化后创建） |
| `generic` | 已配置自定义目录 |

未检测到**不影响使用**：仍可手动指定目标或通过设置添加自定义路径。
UI 上「未检测到」只是一个中性徽章，不是错误。

## 3. 核实状态的含义

| 状态 | `verification` 字符串 | UI 标签 | 含义 |
| --- | --- | --- | --- |
| `Verified` | `verified` | 已验证 | 规则已读官方文档，并有基于规则的测试 |
| `PartiallyVerified` | `warning` | 兼容但有警告 | 部分行为未经确认 |
| `Unverified` | `unverified` | 待验证 | 尚无确认 |

**Codex 为什么是「兼容但有警告」**：核验时
`https://developers.openai.com/codex/skills/` 对自动抓取返回 403，
路径依据官方 skills 仓库与社区资料得出。因此它显示黄色徽章，
`capabilities().notes` 和 UI 都明确提示「请在安装页确认后再使用」。

**DeepSeek Harness Desktop 为什么是「兼容但有警告」**：技能目录是继承来的
（Desktop 启动的就是同一套 Harness 核心，官方文档也写明技能根目录来自
`dshHome` / `agentsHome`），但本机没有运行中的 Desktop 可供确认它是否会
覆盖 `DSH_HOME`，因此不写「已验证」。CLI 的 `deepseek-harness` 依据官方
`docs/subsystems/skills.md` 与 `docs/config-catalog.md`，标记为已验证。

这条有测试保护：`crates/skillforge-core/tests/platform_paths.rs` 与
`src-tauri/src/app.rs::platform_items_expose_capabilities_and_verification`
都断言 Codex 的状态是 `warning`，防止有人「顺手改绿」；
`platform_paths.rs` 同时钉住 DSH Desktop 的 `PartiallyVerified`。

## 4. 来源

| adapter | 官方文档 | 核实日期 |
| --- | --- | --- |
| `claude-code` | https://code.claude.com/docs/en/skills | 2026-10-17 |
| `codex` | https://developers.openai.com/codex/skills/ | 2026-10-17 |
| `opencode` | https://opencode.ai/docs/skills/ | 2026-10-17 |
| `deepseek-harness` | https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/skills.md | 2026-10-20 |
| `deepseek-harness-desktop` | https://github.com/deepseek-ai/deepseek-harness/tree/master/apps/desktop | 2026-10-20 |
| `generic` | https://agentskills.io/specification | 2026-10-17 |

DSH 的默认 home 来自 `docs/config-catalog.md`：
`dshHome` = `$DSH_HOME` 或 `~/.dsh`，`agentsHome` = `$DSH_AGENTS_HOME` 或 `~/.agents`。

平台兼容警告（`PLAT-*`）的文档链接来自对应 adapter 的 `docs_url()`
（Claude Code → Claude 文档、DSH → DSH skills 文档、其余 → 各自官方文档）；
规范与质量规则由 `services/validation_service.rs::rule_doc_url` 指向
Agent Skills 规范（`https://agentskills.io/specification`）。

## 5. 路径解析

应用自身的配置/数据根目录由 `dirs` crate 解析：

| 内容 | macOS | Windows | Linux |
| --- | --- | --- | --- |
| 配置 | `~/Library/Application Support/SkillForge/config.json` | `%APPDATA%\SkillForge\config.json`（未实测） | `$XDG_CONFIG_HOME/skillforge/config.json`（未实测） |
| 工作区（默认） | `~/Library/Application Support/SkillForge/workspace` | 同级的 `workspace`（未实测） | 同级的 `workspace`（未实测） |
| 安装清单 | `…/SkillForge/manifests` | 未实测 | 未实测 |
| 备份 | `…/SkillForge/backups` | 未实测 | 未实测 |
| 日志 | `…/SkillForge/logs` | 未实测 | 未实测 |

只有 macOS 一列是实测的。UI 的「设置 → 本地路径」会实时显示**当前机器上解析出的
真实路径**，因此用户不需要依赖这张表。

## 6. 新增一个平台 adapter 的步骤

1. 在 `crates/skillforge-core/src/platforms/` 新增 `xxx.rs`，实现 `PlatformAdapter`。
2. 在 `platforms/mod.rs` 的 `all_adapters()` 注册。
3. 在 `tests/platform_paths.rs` 增加用临时 HOME/项目目录的路径断言
   （不要依赖开发者真实配置）。
4. 如实填写 `verification()` 与 `verified_on()`——**无法核实就写
   `PartiallyVerified` 或 `Unverified`，不要写 `Verified`**。
5. 如果平台在**同一范围**下扫描多个目录，用
   `common::*_with_variant(..., Some("说明"))` 给每个目录加 `variant`；
   UI 会显示成「用户级 · 说明」，安装/卸载时前端回传 `path` 精确定位。
   只靠 `(平台, 范围)` 反查会永远命中第一个目录。

UI 无需改动：平台页会按 `PlatformItem` 自动渲染新卡片。
