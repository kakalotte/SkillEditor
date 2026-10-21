# 0003 — 平台适配层与「核实状态」

- 状态：已接受
- 日期：2026-10-17
- 修订：2026-10-20（新增 DeepSeek Harness 两个 adapter；同一范围多目录的定位规则）

## 背景

不同 Agent 工具读取 Skill 的目录不同、支持的 frontmatter 字段不同、
描述长度上限也不同。规格 §1 要求：

> 平台路径、能力、校验、安装逻辑必须放在 adapter 里，不能写进 UI 代码。
> 不得宣称未实测的兼容性。

## 决策

### 1. `PlatformAdapter` trait 是唯一的平台知识入口

```rust
pub trait PlatformAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn docs_url(&self) -> &'static str;
    fn verified_on(&self) -> &'static str;          // 核实日期
    fn verification(&self) -> VerificationStatus;   // 三级核实状态
    fn capabilities(&self) -> PlatformCapabilities;
    fn detect(&self, ctx: &PlatformContext) -> PlatformDetection;
    fn discover_targets(&self, ctx: &PlatformContext) -> Vec<InstallTarget>;
    fn validate(&self, doc: &SkillDocument) -> Vec<ValidationIssue>;
    fn project(&self, skill_md: &str) -> Option<SkillProjection>;  // 默认 None
}
```

`PlatformContext` 只携带 `home`、`project_dir`、`custom`（用户覆盖路径）。
trait 里**没有**任何文件系统写操作——adapter 只负责「说」与「纯文本转换」，
安装由 `services/install_service.rs` 负责「做」。`validate` 不再接收 target：
同一平台的所有目标能力相同，按目标重复校验只会产生重复警告。

### 2. 三级核实状态

```rust
pub enum VerificationStatus {
    Verified,            // 官方文档可直接核实（绿）
    PartiallyVerified,   // 依据官方仓库/社区资料，文档页无法自动抓取（黄）
    Unverified,          // 未核实（灰）
}
```

UI 上分别显示「已验证 / 兼容但有警告 / 待验证」徽章，并显示核实日期与官方文档链接。

`VerificationStatus` 本身不实现 `Serialize`（核心库不带 UI 词汇），
由 DTO 层通过 `as_str()` / `label_zh()` 映射成稳定的字符串：

| 变体 | `verification`（前端用于上色） | `verificationLabel`（显示文本） |
| --- | --- | --- |
| `Verified` | `verified` | 已验证 |
| `PartiallyVerified` | `warning` | 兼容但有警告 |
| `Unverified` | `unverified` | 待验证 |

契约测试断言 Codex 为 `warning`，防止有人「顺手改绿」。

**这一条是对「不许吹牛」的工程化约束**：adapter 必须显式声明自己的可信度，
而不是让文档去补。

### 3. 六个 adapter

| id | 显示名 | 用户目标 | 项目目标 | 列表上限 | 核实 |
| --- | --- | --- | --- | --- | --- |
| `claude-code` | Claude Code | `~/.claude/skills` | `.claude/skills` | 1536 | `Verified`（已验证） |
| `codex` | OpenAI Codex | `~/.codex/skills` | `.codex/skills` | 无 | `PartiallyVerified`（兼容但有警告） |
| `opencode` | OpenCode | `~/.config/opencode/skills` | `.opencode/skills` | 1024 | `Verified`（已验证） |
| `deepseek-harness` | DeepSeek Harness | `~/.dsh/skills`、`~/.agents/skills` | `.dsh/skills`、`.agents/skills` | 500 | `Verified`（已验证） |
| `deepseek-harness-desktop` | DeepSeek Harness Desktop | 同上（同一套 Harness 核心） | 同上 | 500 | `PartiallyVerified`（兼容但有警告） |
| `generic` | 通用目录（手动选择） | — | — | 1024 | `Verified`（已验证） |

后三者的 `supported_fields` 为 Agent Skills 规范字段
（`name`/`description`/`license`/`compatibility`/`metadata`/`allowed-tools`）；
Claude Code 额外声明了一批扩展字段；DeepSeek Harness 只声明它**实际读取**的
6 个键（`name`、`description`、`whenToUse`、`metadata`、
`disable-model-invocation`、`user-invocable`），其余字段会被忽略。

### 3.1 同一范围多个目录

DSH 官方文档明确会扫描两个用户级目录（`~/.dsh/skills` 与 `~/.agents/skills`），
所以「一个平台、一个范围、一个目录」的旧假设不再成立。

- `InstallTarget` 增加 `variant: Option<&'static str>`，由
  `common::*_with_variant` 设置；UI 徽章显示「用户级 · Harness 专用」/
  「用户级 · 跨 Agent 共享」。
- 安装/卸载命令新增可选的 `path` 参数：前端回传用户点击的那一行路径，
  后端只接受**该 adapter 已发现的路径**（防止任意路径注入）。
  未传 `path` 时退回按范围查找，保持旧调用与测试可用。
- 清单仍以目标目录路径为键（`ManifestStore::key`），所以同一目录从 CLI 或
  Desktop 安装不会产生两份记录。

### 4. 校验问题按来源分级

`IssueSource` 三值，与严重级别正交，避免把「建议」当成「规范错误」：

- `Spec`：Agent Skills 规范（`SPEC-*`）
- `Platform`：平台限制（`PLAT-*`）
- `Quality`：写作质量建议（`QUAL-*`）

规则 id 是稳定字符串（如 `SPEC-NAME-002`、`QUAL-STRUCT-001`），
每条规则有 `rule_doc_url()` 映射到文档链接，UI 里每条问题都能点开查看规则。

**推荐章节缺失只报「建议」，绝不报错**（规格 §9）。
平台字段不兼容报「警告」，因为规范本身允许扩展字段。

平台规则**每个 adapter 只跑一次**，不按目标目录重复。DSH 有四个根目录，
按旧逻辑会把同一条 `license` 警告显示四次。去重放在
`validation_service::validate_with_context`，adapter 只描述一次能力。

### 5. 自定义覆盖

每个 adapter 都可通过 `PlatformContext.custom(id)` 追加一个自定义目标目录
（在「设置」中配置）。`generic` adapter 只有自定义目标——它不假设任何产品目录。

### 6. 安装时投影（`project`）

**问题**：Agent Skills 的通用前端字段与各平台的读取字段并不完全一致。
DeepSeek Harness 只读 camelCase 的 `whenToUse`，而 Claude Code 等使用
`when_to_use`。如果只在 UI 里警告，用户会以为「安装成功」，但 Harness
实际看不到触发说明。

**决策**：adapter 可以提供一个**纯文本、确定性**的 `project` 变换。
默认实现返回 `None`，即原样安装；目前只有 DSH CLI / Desktop 实现：
把 YAML 顶层的 `when_to_use` 重命名为 `whenToUse`。

约束：

1. **只改安装副本**。`project` 接收字符串、返回新字符串，不接触文件系统；
   源 `SKILL.md` 字节不变。
2. **只做有明确等价关系的重命名**。不做语义转换、不删字段、不猜。
3. **文本级重写**：只替换顶层的键名，保留注释、键顺序、缩进（含缩进的根映射）、
   CRLF/BOM、未知字段与正文；嵌套键（`metadata.when_to_use`）不匹配。
   当前只处理未加引号的键；无法安全改写时返回 `None`，预览不显示映射。
4. 源中已有 `whenToUse` 或两个键并存时，不投影（保持原样）。
5. 投影结果进入 `InstallPlan.overrides`，**参与哈希**；
   `apply_install` 在复制源树后把 overrides 写入暂存目录，清单按最终内容记录，并保存投影 `from` / `to`（旧版清单靠 `serde(default)` 兼容）。
   因此第二次安装显示「无变化」，卸载与外部修改检测仍然准确。
6. 预览必须展示映射（`from → to`），不能静默改写。
7. 预览中的兼容警告按**投影后的副本**重新校验：已被投影解决的字段只出现在
   映射区，仍不支持的字段继续警告。平台卡片仍显示源文档的兼容状态与将适配的
   字段数，两者不混淆。

**为什么不在编辑器里双向统一**：工作区源文件是跨平台资产，编辑器不应为了
某一个安装目标改动它；保存时自动翻译也会破坏「无损保持」与用户预期。
投影把平台差异限制在安装边界内。

**为什么不用临时目录当 source_root**：`InstallPlan` 在「预览 → 确认」之间
缓存，临时目录的生命周期与清单里的 `source_path` 都会变得不可靠。
overrides 让计划保持自包含。

## 后果

- ✅ 新增一个平台 = 新增一个 adapter 文件 + 在 `all_adapters()` 注册，UI 不动。
- ✅ 「待验证」是数据（`VerificationStatus`）而不是文档里的一句话。
- ✅ 字段差异在安装边界内解决；源文件保持跨平台中立，投影可预览、可重复。
- ⚠️ 投影只覆盖有文档等价关系的字段。其他平台的扩展字段仍只报警告，
  不自动改写（例如 `license` 在 DSH 中被忽略，但没有安全的目标字段可映射）。
- ⚠️ Codex 的官方文档站对自动抓取返回 403，因此路径依据官方 skills 仓库与社区资料，
  标为 `PartiallyVerified` 并在 UI 中显示黄色徽章与说明。**没有把它写成 `Verified`。**
- ⚠️ DeepSeek Harness Desktop 的技能目录继承自同一套 Harness 核心，
  但本机没有运行中的 Desktop 可供确认它不会覆盖 `DSH_HOME`，因此
  标为 `PartiallyVerified`；CLI adapter 依据官方文档标为 `Verified`。
- ⚠️ Windows/Linux 上的实际目录未实测（macOS 上这些目录本就无关），
  `docs/platform-support.md` 中逐项标注。

## 测试

- `crates/skillforge-core/tests/platform_paths.rs`：用临时 HOME/项目目录断言
  六个 adapter 的发现结果，不依赖开发者真实配置；并断言 Desktop 与 CLI 的
  技能根目录完全一致。
- 断言 Codex 与 DeepSeek Harness Desktop 的核实状态为 `PartiallyVerified`
  （防止有人「顺手改绿」）。
- `app.rs::install_targets_can_be_selected_by_path`：两个用户级目标都能按
  `path` 正确解析，未知路径被拒绝。
- `src-tauri/tests/frontend_contract.rs::capability_grants_*` 与
  `app.rs::platform_items_expose_capabilities_and_verification`：
  确认能力字段与核实状态确实到达 UI。
- `deepseek_harness.rs`：投影保留注释、缩进根映射、嵌套键、正文、CRLF 与 BOM；
  已适配或两键并存时不动；Desktop 共用同一规则。
- `install_safety.rs::projected_content_is_installed_without_touching_the_source`：
  overrides 参与哈希、只写安装副本、源文件不变、重复安装为「无变化」。
- `install_safety.rs::projected_content_can_replace_a_read_only_source_file`：
  Unix 只读源文件仍可投影，权限保留，清单记录 `from` / `to`。
- `install_service.rs::legacy_manifest_without_rewrites_still_deserializes`：
  旧清单没有 `rewrites` 字段仍可读取。
- `common.rs::listing_limit_counts_only_the_alias_the_platform_reads`：
  列表长度只计平台实际读取的别名（DSH 用 camelCase，Claude 用 snake_case）。
- `validation.rs::platform_warnings_are_not_duplicated_per_target`：
  DSH 四个根目录只产生一条字段警告。
- `app.rs::deepseek_install_projects_when_to_use_and_keeps_the_source`：
  预览显示映射与警告、确认后目标为 `whenToUse`、源文件不变、清单记录投影、再次预览无变化。
