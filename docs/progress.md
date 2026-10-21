# 进度与验证记录

本文件记录**实际执行过的命令与结果**。所有「通过」都有对应命令；
所有未执行的事情都在「待验证 / 未实现」中标明。

## 0. 环境

```
macOS 15.8.1 (24H32), Apple Silicon M4
rustc 1.99.1 / cargo 1.99.1
tauri 2.12.4, tauri-build 2.7.3, tauri-plugin-dialog 2.8.3
node v24.16.0（仅用于人工语法检查，构建不需要）
python3 + Pillow（仅用于生成图标）
```

工作目录：`skillforge-tauri/`（仓库根；远程 `origin` 指向
<https://github.com/kakalotte/SkillEditor>）。

## 1. 阶段 A：工具链验证与最小窗口

目标：先证明「Rust + Tauri 在这台机器上能构建、能起窗口」，再写业务代码。

| 步骤 | 命令 | 结果 |
| --- | --- | --- |
| crates.io 可达 | `cargo search tauri` | 可达 |
| 最小窗口构建 | `cargo build -p skillforge` | 干净构建 **2m06s** |
| 直接运行 | `./target/debug/skillforge` | 进程存活，stdout/stderr 为空 |
| 窗口确实创建 | `lsappinfo list` | `type="Foreground"`；WebKit 的 `Networking`/`GPU`/`Web Content` XPC 服务在运行 |
| 干净退出 | `kill <pid>` | 无残留进程 |

**结论**：Tauri 在 macOS 上无需 CLI 即可构建与启动。

同时验证了 `tauri-macros` 2.7.3 的命令参数默认转成 camelCase
（`~/.cargo/registry/.../tauri-macros-2.7.3/src/command/wrapper.rs`），
这决定了前端必须用 `platformId` 而不是 `platform_id`。

### 阶段 A 的踩坑记录

1. **`tauri-build` 需要图标**，否则构建失败 → 写 `packaging/make-icons.py` 生成。
2. **`dirs` 需要作为 `src-tauri` 的直接依赖**（`app.rs` 用 `home_dir()`）。
3. **`with_core` 不能直接接收 `fn(&Core) -> T`**（签名要求 `FnOnce(&mut Core)`）
   → 一律用闭包 `|core| app::config_root(core)`。
4. **bin crate 没有 lib target** → `Core::from_roots` 标 `#[cfg(test)]`，
   所有命令层测试放在模块内。
5. **不用 Tauri CLI 就必须自己开 `custom-protocol` 特性**，否则嵌入的前端资源
   不会被提供 → `src-tauri/Cargo.toml` 设 `default = ["custom-protocol"]`。

## 2. 阶段 B：核心库（`crates/skillforge-core`）

核心库是独立的纯 Rust crate，不包含桌面 UI 入口；包名统一为
`skillforge_core`，供 Tauri 命令层调用。

```
$ cargo test -p skillforge-core
70 passed (unit)
 5 passed (skill_roundtrip)
 7 passed (platform_paths)
 5 passed (install_safety)     # 后续增加到 6
 6 passed (validation)
= 93 passed
```

零警告（工作区 `warnings = "deny"`）。

同时补齐了 fixtures（`tests/fixtures/valid-skill`、`invalid-frontmatter`）。

## 3. 阶段 C：命令层（`src-tauri`）

新增文件：

| 文件 | 内容 |
| --- | --- |
| `src/dto.rs` | 全部可序列化视图模型（camelCase），325 行 |
| `src/app.rs` | 全部业务桥接逻辑 + 模块内测试，1513 行 |
| `src/shell.rs` | 打开文件管理器 / 浏览器链接，125 行 |
| `src/commands.rs` | 29 个 `#[tauri::command]` 薄壳，310 行 |
| `src/main.rs` | builder、插件、状态、`invoke_handler!`、关闭拦截，65 行 |

```
$ cargo test -p skillforge            # 26 passed
$ cargo clippy --workspace --all-targets   # 无输出
```

设计要点（详见 `docs/decisions/0004-frontend-and-ipc.md`）：
非文本命令返回完整 `Snapshot`，文本命令返回 `EditAck`。

### 本实现刻意增加的安全措施

| 措施 | 位置 |
| --- | --- |
| `delete_skill(id, confirm)` / `uninstall(platform, scope, confirm)` 未确认即拒绝 | `app.rs` |
| `save` 校验名称并拒绝写入已存在的目录 | `app.rs` |
| `save_skill_as` 拒绝覆盖已存在的 `SKILL.md`，并复制资源文件 | `app.rs` |
| 坏 frontmatter 可在应用内修复（`set_body` 重新解析原始文本） | `app.rs::repair_raw` |
| `open_url` 只接受 http/https | `shell.rs` |

## 4. 阶段 D：前端

`ui/index.html`（骨架）+ `ui/styles.css`（浅/深两套变量）+ `ui/app.js`（渲染与 IPC）。

关键约束与做法：

- 不用 `innerHTML`（SKILL.md 是不可信输入），全部 `textContent`。
- 文本编辑防抖 160 ms，任何读取文档的命令前先 `await flushEdits()`。
- 页面「建一次 + 只更新」，避免重建正在输入的控件而丢光标。
- 主题：`system` 解析为 `prefers-color-scheme` → `data-theme`。

### 契约测试（`src-tauri/tests/frontend_contract.rs`）

前端没有类型系统，用静态分析补上 6 条断言：

1. 前端调用的每个命令都已注册。
2. 每个已注册的命令都被使用。
3. `index.html` 引用的 `app.js` / `styles.css` 存在。
4. `frontendDist` 指向真实目录且 `withGlobalTauri = true`，identifier/窗口配置符合预期。
5. capabilities 授予 `core:default` 与 `dialog:default`。
6. 前端不用 `innerHTML` 等解析 HTML 的 API；命令参数是 camelCase。

**这套测试当场抓到了 3 个真实问题**：

- `refs.editorBody_. = null;` —— 语法错误（`node --check` 与测试都报错）。
- 用 `hidden` 隐藏 `.badge` / `.editor-grid` / `.modal` 无效
  —— 它们有 `display: flex|inline-flex|grid`，需要 `[hidden] { display: none !important }`。
- `log_frontend` 通过 `invoke("…")` 直接调用，漏在命令扫描之外
  → 扩展扫描前缀后通过。

### 前端诊断通道

新增 `log_frontend(level, message)`：前端错误/就绪信息写 stderr。
这既是产品功能（无开发者工具时的诊断手段），也是本环境下唯一可获得的
「界面确实渲染了」的证据。见 `docs/decisions/0005-diagnostics.md`。

## 5. 阶段 E：打包与启动验证

`packaging/make-macos-app.sh`：手工组装 `Contents/`、写 `Info.plist`、
拷图标、ad-hoc 签名（不使用 Tauri CLI）。

```
$ ./packaging/make-macos-app.sh
==> cargo build (profile=release, version=0.1.0, id=io.skillforge.desktop)
==> assembling .../dist/SkillForge.app
==> ad-hoc signing
==> done: .../dist/SkillForge.app

$ codesign -dv dist/SkillForge.app
Identifier=io.skillforge.desktop
Format=app bundle with Mach-O thin (arm64)
flags=0x2(adhoc)
```

### 启动验证（隔离 HOME，避免污染真实配置）

```
$ HOME=$(mktemp -d) ./dist/SkillForge.app/Contents/MacOS/skillforge 2>&1
[前端/信息] 界面渲染完成：技能 0，平台 4，目标 3，主题 system
[前端/信息] 已就绪：0 个 Skill，工作区 <HOME>/Library/Application Support/SkillForge/workspace
```

### 有真实数据的启动验证

在工作区预置 1 个正常 Skill（`pdf-tools`，含 `references/guide.md`）
与 1 个 frontmatter 损坏的 Skill（`summarize`）：

```
[前端/信息] 界面渲染完成：技能 2，平台 4，目标 3，主题 system
[前端/信息] 已就绪：2 个 Skill，工作区 …
```

两个 Skill **都被列出**（包括坏的那个），没有静默丢弃。

### 用 `open` 启动 .app

```
$ open dist/SkillForge.app
$ lsappinfo list | grep -A3 SkillForge.app
    bundleID="io.skillforge.desktop"
    bundle path=".../dist/SkillForge.app"
    pid = 42244 type="Foreground" flavor=3 Version="0.1.0" fileType="APPL" Arch=ARM64
```

关闭后无残留进程，测试期间创建的配置目录已清理。

**证据链**：窗口创建 → WebView 加载嵌入的 `index.html` → `app.js` 执行到
`main()` → `boot` IPC 成功 → 4 个渲染函数全部无异常 → stderr 出现
「界面渲染完成」+「已就绪」→ 工作区目录真实落盘。

## 6. 最终验证（一次完整运行）

```
$ cargo fmt --all --check                 # 无输出
$ cargo test --workspace                  # 128 passed
$ cargo clippy --workspace --all-targets  # 无输出
```

| 测试目标 | 数量 |
| --- | --- |
| `skillforge-core` 单元测试 | 70 |
| `install_safety` | 6 |
| `platform_paths` | 7 |
| `skill_roundtrip` | 5 |
| `validation` | 6 |
| `skillforge`（`app.rs`） | 27 |
| `frontend_contract` | 7 |
| **合计** | **128** |

（上表是 2026-10-17 的基线；2026-10-20 新增 DeepSeek Harness 支持后为 140，
再加安装兼容门槛与字段投影后为 157，见本节末尾的两个 2026-10-20 小节。）

### 本轮修复的真实缺陷

`detect_external_changes` 设置**之前只被存储、从未被使用**——
UI 上是一个「死开关」。已修复：新增
`install_service::PlanOptions { detect_external_modification }` 与
`plan_install_with(...)`，`preview_install` 用该设置决定是否做外部修改比对；
`plan_install` 保持原签名（默认开启），因此既有测试未受影响。
新增两个测试：核心层 `external_change_detection_can_be_turned_off`、
命令层 `external_change_detection_follows_the_setting`。

**安装预览报 `snapshot.skills.length` 未定义**（2026-10-20 报告并修复）：

- 现象：打开 Skill → 平台页 → 点「安装预览」，界面报
  「未处理的异步错误：undefined is not an object (evaluating 'snapshot.skills.length')」，
  预览弹窗不出现；且此后技能列表/平台页/设置页都渲染不出来。
- 根因：`ui/app.js` 用 `run("preview_install", …)` 调用该命令，而 `run()` 会把返回值
  无条件交给 `applySnapshot()`。`preview_install` 返回的是 `InstallPreview`
  （`title/targetRoot/added/…`，没有 `skills`），于是 `state.snapshot` 被覆盖成
  preview 对象，`renderTop()` 读 `snapshot.skills.length` 时抛出。
  同类命令 `preview_uninstall` 用的是 `call()`，所以只有安装预览坏了。
- 修复：调用改为 `call("preview_install", …)`；`run()` 增加返回值形状检查
  （无 `skills` 数组时报错并保持 `state.snapshot` 不变）。
- 回归测试：`frontend_contract::run_is_only_used_for_snapshot_commands`
  解析 `commands.rs` 的返回类型与 `app.js` 的 `run()` 调用点并配对。
  已用「把调用改回 `run(`」的方式验证该测试确实失败
  （报 `preview_install -> InstallPreview`）。
- 重新打包：`packaging/make-macos-app.sh` 成功（release 34s，ad-hoc 签名）。
  `ui/app.js` 是唯一在 10-17 13:03 之后改动的构建输入，因此这次 release 重编译
  即由资源变化触发，`dist/SkillForge.app` 已包含新前端。
- 启动验证（`HOME` 指向临时目录，工作区放 1 个正常 + 1 个 frontmatter 损坏的 Skill）：
  `[前端/信息] 界面渲染完成：技能 2，平台 4，目标 3，主题 system`
  （当时尚未加入 DeepSeek Harness；加入后为平台 6、目标 7，见下文），
  无未处理异常，退出后无残留进程。
- 待人工确认：GUI 点击路径未自动化，修复后需在界面上点一次「安装预览」。

### 新增 DeepSeek Harness / DeepSeek Harness Desktop（2026-10-20）

需求：让 SkillForge 能往 DeepSeek Harness（CLI `dsh`）和带桌面壳的
DeepSeek Harness Desktop 安装 Skill。

核实到的官方规则（`docs/subsystems/skills.md`、`docs/config-catalog.md`）：

- 本地技能 provider 会扫描 `~/.dsh/skills`（rank 400）与
  `~/.agents/skills`（rank 500），项目级为 `<项目>/.dsh/skills`（100）与
  `<项目>/.agents/skills`（200）。
- `dshHome` 默认 `$DSH_HOME` 或 `~/.dsh`，`agentsHome` 默认
  `$DSH_AGENTS_HOME` 或 `~/.agents`。
- 只读 `name`、`description`、`whenToUse`（注意是 camelCase）、`metadata`、
  `disable-model-invocation`、`user-invocable`；其余字段被忽略。
- 技能名必须 kebab-case，非法名字会被跳过（记日志，不报错）。

注意：本应用按默认位置发现目标，**不读取** `DSH_HOME` / `DSH_AGENTS_HOME`；
改过这两个变量的用户需要用「设置 → 自定义路径」覆盖（UI 会显示真实路径）。

实现：

- 新增 `platforms/deepseek_harness.rs`（`Verified`）与
  `platforms/deepseek_harness_desktop.rs`（`PartiallyVerified`）。
  Desktop 运行的是同一套 Harness 核心，因此两者目标目录相同；
  Desktop 仅额外用 `~/.dsh/profiles/desktop` 做检测。
- 同一平台出现两个用户级目录后，原有的 `(平台, 范围)` 反查只会命中第一个。
  为此 `InstallTarget` 增加 `variant`，`preview_install` /
  `preview_uninstall` / `uninstall` 增加可选 `path` 参数：
  只接受该 adapter 已发现的路径，未知路径直接报「未找到该安装目标」。
  前端在三个调用点回传 `target.path`。
- `PLAT-DEEPSEEK-HARNESS-*` 的规则文档链接指向 DSH 的 skills 文档。

验证：

```
$ cargo fmt --all --check
$ cargo clippy --workspace --all-targets      # 无输出
$ cargo test --workspace
test result: ok. 28 passed   # 命令层
test result: ok. 7 passed    # 前后端契约
test result: ok. 79 passed   # core 单元
test result: ok. 6 passed    # install_safety
test result: ok. 9 passed    # platform_paths
test result: ok. 5 passed    # skill_roundtrip
test result: ok. 6 passed    # validation
```

合计 **140** 个测试通过（原 128 + 12）。

重新打包并启动（`HOME` 指向临时目录，预置 1 个 Skill、
`~/.dsh/profiles/desktop` 与 `~/.agents/skills`）：

```
$ packaging/make-macos-app.sh
    Finished `release` profile [optimized] target(s) in 36.90s
==> ad-hoc signing
==> done: dist/SkillForge.app

$ HOME=<tmp> ./dist/SkillForge.app/Contents/MacOS/skillforge 2>&1
[前端/信息] 界面渲染完成：技能 1，平台 6，目标 7，主题 system
[前端/信息] 已就绪：1 个 Skill，工作区 <tmp>/.../workspace
```

（平台从 4 增至 6；目标从 3 增至 7：CLI/Desktop 各贡献
`~/.dsh/skills` 与 `~/.agents/skills` 两条用户级目标。）退出后无残留进程。

人工/GUI：本环境仍无法自动点击 WebView，两个新平台卡片与双用户级目标的
实际渲染、以及按 `path` 安装/卸载的点击路径，需在打包后人工确认。
另：没有在装有 DSH Desktop 的机器上确认它是否覆盖 `DSH_HOME`，
因此 Desktop 保持 `PartiallyVerified`。

### 安装兼容门槛与字段投影（2026-10-20）

**动机**：安装预览此前只显示文件差异，不显示平台字段兼容提醒；DSH 读取
`whenToUse`，而工作区常用的 `when_to_use` 会被静默忽略。用户会看到「安装成功」，
但 Harness 实际读不到触发说明。

实现：

- `PlatformAdapter::validate(doc)` 去掉未使用的 `target` 参数，平台校验改为
  **每个 adapter 一次**。`validate_with_context` 不再按目标目录循环。
  DSH 有四个根目录，旧逻辑会把同一条警告重复四次。
- 新增 `PlatformAdapter::project(&str) -> Option<SkillProjection>`，默认 `None`。
  DSH / Desktop 把 YAML 顶层的 `when_to_use` 重命名为 `whenToUse`：
  - 文本级替换，保留注释、键顺序、缩进、CRLF、BOM、未知字段与正文；
  - 嵌套键与正文中的同名文本不动；
  - 已有 `whenToUse` 或两键并存时不改；源文件从不写入。
- `plan_install_with_overrides(...)` 把投影内容放进 `InstallPlan.overrides`，
  参与差异哈希；`apply_install` 复制源树后只写暂存副本，清单按最终内容记录，并保存投影 `from` / `to`（旧版清单兼容）。
  overrides 的键同样经过 `normalize_relative` + `safe_join`。
- `InstallPreview` 增加 `warnings` / `rewrites`；平台卡片增加
  `compatStatus` / `compatWarnings` / `compatRewrites`。安装模态框显示提醒与
  `from → to` 映射，有提醒时按钮为「仍然安装」。预览的 warnings 基于**投影后的文档**校验：`when_to_use` 已自动适配时不再重复报「会被忽略」，仍不受支持的 `license` 等字段继续报警。
- `preview_install` 拒绝有未保存修改的文档，避免预览/安装磁盘上的旧内容。
- 安装预览标题改用带变体的范围标签（如「用户级 · Harness 专用」）。

新增测试（17 个）：

| 测试 | 覆盖 |
| --- | --- |
| `deepseek_harness::projection_*`（6 个） | 顶层重命名、缩进根映射、嵌套/正文不动、已适配、两键并存、CRLF、BOM、坏 frontmatter |
| `deepseek_harness::when_to_use_warning_describes_the_projection` | 警告说明会尝试自动改名 |
| `deepseek_harness_desktop::projects_with_the_same_rule_as_the_cli` | Desktop 共用规则 |
| `install_safety::projected_content_is_installed_without_touching_the_source` | overrides 哈希、只写副本、幂等 |
| `install_safety::projected_content_can_replace_a_read_only_source_file` | 只读源文件的权限保留（Unix）、清单记录投影 |
| `install_service::legacy_manifest_without_rewrites_still_deserializes` | 旧清单兼容 |
| `common::listing_limit_counts_only_the_alias_the_platform_reads` | 列表长度只算平台实际读取的别名 |
| `install_safety::overrides_for_unknown_files_are_rejected` | 路径校验 |
| `validation::platform_warnings_are_not_duplicated_per_target` | 每平台一次 |
| `app::deepseek_install_projects_when_to_use_and_keeps_the_source` | 预览→安装→源不变→清单记录投影→幂等 |
| `app::install_preview_refuses_unsaved_edits` | 脏文档门槛 |
| `app::platform_cards_report_compatibility_for_the_open_skill` | 平台卡片状态与适配数 |

验证：

```
$ cargo fmt --all --check                     # 无输出
$ cargo clippy --workspace --all-targets     # 无输出
$ cargo test --workspace
test result: ok. 31 passed   # 命令层
test result: ok. 7 passed    # 前后端契约
test result: ok. 89 passed   # core 单元
test result: ok. 9 passed    # install_safety
test result: ok. 9 passed    # platform_paths
test result: ok. 5 passed    # skill_roundtrip
test result: ok. 7 passed    # validation
```

合计 **157** 个测试通过（原 140 + 17）。

重新打包并启动（`HOME` 指向临时目录，预置 1 个含 `when_to_use` 的 Skill、
`~/.dsh/profiles/desktop` 与 `~/.agents/skills`）：

```
$ packaging/make-macos-app.sh
    Finished `release` profile [optimized] target(s) in 38.76s
==> assembling .../dist/SkillForge.app
==> ad-hoc signing
==> done: .../dist/SkillForge.app

$ HOME=<tmp> ./dist/SkillForge.app/Contents/MacOS/skillforge 2>&1
[前端/信息] 界面渲染完成：技能 1，平台 6，目标 7，主题 system
[前端/信息] 已就绪：1 个 Skill，工作区 <tmp>/.../workspace
```

退出后无残留进程。启动日志只能证明界面渲染与命令注册未破坏，
**不能证明弹窗内容**。GUI 点击路径仍需人工确认：打开含 `when_to_use` 的
Skill → 平台页查看兼容/适配徽章 → 安装预览查看提醒与映射 → 确认后检查目标
`SKILL.md` 为 `whenToUse`、工作区源文件仍为 `when_to_use`。
另：没有 DSH 运行时可实测它是否正确读取该字段，规则依据官方
`docs/subsystems/skills.md`。

### 新增只读 Frontmatter 规则指南（2026-10-21）

需求：让用户在不离开编辑器的情况下查看支持的 frontmatter 字段、约束、示例、
平台支持与校验规则；本阶段**只读**，不做字段编辑器、YAML 编辑器或整份源码编辑器。

实现：

- 核心库新增 `crates/skillforge-core/src/guide.rs`：21 个字段、20 条校验规则、
  目录/正文规则、完整 `SKILL.md` 示例与限制常量。字段覆盖 `SPEC_FIELDS` 与所有
  adapter 的 `supported_fields`。
- 新增 `FrontmatterGuide` / `GuideField` / `GuideRule` / `GuidePlatform` /
  `GuideLimits` DTO（camelCase）与 `frontmatter_guide` 命令。返回专用 DTO 而不是
  `Snapshot`，因此前端必须用 `call()`，不会触发全量重渲染。
- app 层用当前 adapter 的 `supports_field` 与 `listing_limit` 计算平台支持集合和
  列表上限；平台知识不在 JavaScript 中硬编码。
- UI 新增宽模态框：搜索、五个分页（必填/规范可选/平台扩展/正文与目录/校验规则）、
  「必填字段」页顶部一条紧凑的当前文档状态（必填完整性、已用字段数、缺失必填项、
  description 字符数；其他分页与搜索时不显示）、字段卡片、规则卡片、官方文档链接
  与完整示例。平台列表上限只在「平台扩展」页展示。编辑器工具栏和侧栏提供入口，
  `name`/`description` 下方增加提示与实时字符计数。
- 只读保证：指南没有 `textarea` / `contenteditable` 或字段编辑控件；命令不写文件。
  命令层测试先打开 Skill 并制造未保存修改，再调用指南，断言脏标记仍在、磁盘字节不变；
  契约测试断言模态框结构、无编辑控件且使用 `call("frontmatter_guide")`。

验证：

```
$ cargo fmt --all --check                          # 无输出
$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile ... (0 warnings)
$ cargo test --workspace
    Running unittests src/main.rs (target/debug/deps/skillforge-...)
test result: ok. 32 passed; 0 failed
    Running tests/frontend_contract.rs (...)
test result: ok. 8 passed; 0 failed
    Running unittests src/lib.rs (...)
test result: ok. 94 passed; 0 failed
    Running tests/install_safety.rs (...)
test result: ok. 9 passed; 0 failed
    Running tests/platform_paths.rs (...)
test result: ok. 9 passed; 0 failed
    Running tests/skill_roundtrip.rs (...)
test result: ok. 5 passed; 0 failed
    Running tests/validation.rs (...)
test result: ok. 7 passed; 0 failed
$ node --check ui/app.js                           # 通过（仅人工语法检查）
```

合计 **165** 个测试通过：core 124（单元 94 + 集成 30）+ 命令层 32 + 契约 9
（契约层新增版本一致性测试）。
指南相关新增测试 7 个（核心 5 + 命令层 1 + 契约 1），命令层测试后来扩展为
「干净文档保持干净 + 脏文档保持脏 + 磁盘字节不变」。

重新打包并启动（`HOME` 指向临时目录，预置 1 个 Skill）：

```
$ packaging/make-macos-app.sh
    Finished `release` profile [optimized] target(s) in 34.78s
==> assembling .../dist/SkillForge.app
==> ad-hoc signing
==> done: .../dist/SkillForge.app

$ HOME=<tmp> ./dist/SkillForge.app/Contents/MacOS/skillforge 2>&1
[前端/信息] 界面渲染完成：技能 1，平台 6，目标 7，主题 system
[前端/信息] 已就绪：1 个 Skill，工作区 <tmp>/.../workspace
```

退出后无残留进程。启动日志只能证明界面渲染与命令注册未破坏，**不能证明弹窗内容**。

为验证弹窗本身，另外用同一份 `ui/index.html` / `app.js` / `styles.css` 在浏览器中
加载临时 mock IPC 数据（未提交），实际点击检查：

- 五个分页均出现；必填页只显示 `name` / `description`，规范可选页显示 `license`，
  平台扩展页显示字段与两个平台的列表上限，正文页显示完整示例，规则页显示规则；
- 当前文档状态只在「必填字段」页显示一条紧凑摘要
  「当前文档：必填完整 · 已用 2 个字段 · description 31 / 1024」；
- 状态栏不含平台列表上限；平台扩展页仍显示两个平台的列表上限；
- 切到其他四个分页或输入搜索词时，状态栏隐藏；
- 搜索 `description` 同时命中字段与 `SPEC-DESC-001`；Esc 可关闭；
- 在 `description` 输入后等待防抖结束，实时字符数保持 `8 / 1024`，没有回退到旧值。

这验证了前端 DOM 渲染与交互，但**不能替代 Tauri 原生窗口的点击验收**。

待人工确认：GUI 点击路径未自动化。需要打开一个 Skill，点击「Frontmatter 规则」，
检查分页、搜索、当前文档状态、官方链接、Esc/背景关闭，并确认关闭后未保存状态不变。

### 指南状态栏收窄（2026-10-21）

需求：状态栏此前在每个分页和搜索时都显示，且重复平台列表上限，信息噪音大。
调整为只在「必填字段」页且无搜索时显示一条紧凑摘要，平台上限只保留在
「平台扩展」页。

实现：

- `renderGuide()` 以 `!query && state.guideTab === "required"` 控制
  `renderGuideStatus(guide, visible)`；不可见时清空内容并设置 `hidden`。
- 状态栏改为单行「当前文档：<必填徽章> · 已用 N 个字段 ·
  description N / max」，删除平台上限计算与第二行；缺失必填项仍附在摘要末尾。
- `styles.css` 增加 `.guide-status[hidden] { display: none; }`，避免
  `.guide-status { display: flex; }` 覆盖 `hidden`。
- 契约测试新增条件渲染、状态栏不出现「平台列表上限」及隐藏 CSS 的断言。

验证：

```
$ node --check ui/app.js                           # 通过
$ cargo fmt --all --check                          # 无输出
$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile ... (0 warnings)
$ cargo test --workspace
    32 + 9 + 94 + 9 + 9 + 5 + 7 = 165 passed
```

浏览器 mock IPC 复测（必填页 description 为 53 字符）：

- 「必填字段」页显示「当前文档：必填完整 · 已用 2 个字段 ·
  description 53 / 1024」；
- 其他四个分页及搜索时 `#guide-status` 的 `hidden` 为 true，计算样式为
  `display: none`；
- 状态栏文本不含平台列表上限；「平台扩展」页仍显示「列表上限 777」；
- 切回必填页后状态恢复，仍是同一行摘要。

重新打包并启动：

```
$ packaging/make-macos-app.sh
    Finished `release` profile [optimized] target(s) in 40.69s
==> done: .../dist/SkillForge.app

$ HOME=<tmp> ./dist/SkillForge.app/Contents/MacOS/skillforge
[前端/信息] 界面渲染完成：技能 1，平台 6，目标 7，主题 system
[前端/信息] 已就绪：1 个 Skill，工作区 <tmp>/.../workspace
```

退出后无残留进程。

## 7. 验收标准对照（规格 §11.3）

| # | 标准 | 状态 | 证据 |
| --- | --- | --- | --- |
| 1 | macOS 开发环境可构建和启动 | ✅ | 阶段 A + 阶段 E 的启动验证 |
| 2 | Windows/Linux 的 CI 构建可用；无法实测的 GUI 细节必须记录 | ⚠️ **工作流已添加，待首次运行** | `.github/workflows/ci.yml`；平台限制仍记录在 `platform-support.md` §1 |
| 3 | 可创建、编辑、导入、保存、导出 Skill | ✅ | `app.rs`：`create_save_and_reload_round_trip`、导入/导出测试 |
| 4 | 对无效 frontmatter 给出可理解错误 | ✅ | `broken_frontmatter_can_be_repaired_inside_the_app`（含行号） |
| 5 | 已知字段修改后，未知字段与资源文件不丢失 | ✅ | `editing_through_the_app_layer_preserves_unknown_fields`、`save_as_copies_resources_*` |
| 6 | 至少两个 adapter 有基于官方规则的测试 | ✅ | `platform_paths.rs` 覆盖 6 个 adapter |
| 7 | 安装前显示差异并提供备份 | ✅ | `install_preview_confirm_and_uninstall_round_trip`、`reinstall_creates_a_backup`、`deepseek_install_projects_when_to_use_and_keeps_the_source`（同时展示兼容提醒与投影） |
| 8 | 不执行 Skill 内脚本 | ✅ | 核心库无执行路径；`open_url_rejects_unsafe_schemes_without_spawning` |
| 9 | README 提供安装、运行、测试和打包步骤 | ✅ | `README.md` |
| 10 | `cargo fmt --check`、`cargo clippy`、`cargo test` 通过 | ✅ | 阶段 6 的完整运行 |

**第 2 条仍待验证**：Windows/Linux/macOS 的 CI 工作流已经写入仓库，
但首次 GitHub Actions 运行尚未发生。在它跑通之前，本实现仍只声称在 macOS 上
实测可构建、可运行。

## 8. 待验证（未实测）

| 项 | 说明 |
| --- | --- |
| Windows 构建 / 运行 | 从未执行；`cfg(target_os = "windows")` 分支未运行过 |
| Linux 构建 / 运行 | 从未执行；需要 WebKitGTK，未验证 |
| Windows/Linux 的原生对话框 | 未验证 |
| Windows/Linux 的「在文件管理器中显示」 | 未验证（`explorer /select,` / `xdg-open` 分支） |
| 代码签名与公证 | 发布工作流已接入；本地 `.app` 仍为 ad-hoc，首次签名发布待验收 |
| 应用图标在 Finder/Dock 的实际显示 | 未截图验证 |
| 浅色/深色主题的实际视觉效果 | 未截图验证；`system` → `prefers-color-scheme` 的逻辑已实现 |
| 中文输入法（IME）行为 | 未人工验证；防抖 160 ms 与「不重建输入控件」是为其设计 |
| 窗口缩放 / 长文本 / 超大 Skill | 未人工验证；CSS 有 `min-width`/滚动与 `@media` 断点 |
| `save_skill_as` 的符号链接行为 | 命令层无测试（核心层 `copy_dir_contents` 跳过符号链接并有测试） |
| 恶意 `SKILL.md` 的端到端渗透 | 只做了静态约束（不用 `innerHTML`）与单元测试，未做渗透测试 |
| 原生选择器的实际弹窗 | 未逐一点击验证（需人工交互） |
| 平台兼容徽章 / 安装提醒 / 映射预览的实际渲染 | 后端与契约测试已覆盖，WebView 点击路径未人工确认 |
| DeepSeek Harness 运行时读取 `whenToUse` | 没有可用的 DSH 运行时；规则依据官方文档，未做端到端实测 |

## 9. 未实现（对照规格 §12）

| 规格条目 | 状态 |
| --- | --- |
| 阶段 D 第 5 条「实现安全的自动修复机制（只修复确定安全的项目）」 | **未实现**。目前提供的是「校验 + 在源码视图手动修复 + 修复前禁止保存」，没有一键自动修复命令。 |
| 阶段 F 第 3 条「建立 Windows/macOS/Linux 构建流程」 | CI 与发布工作流已添加；Windows/Linux 首次运行待验证 |
| 阶段 F 第 6 条「更新文档、截图」 | 文档已写；**没有截图**（本环境无法可靠截取窗口） |

## 10. 交付物对照（规格 §14）

| 交付物 | 位置 | 状态 |
| --- | --- | --- |
| 可运行的 Rust + Tauri 桌面项目 | `skillforge-tauri/` | ✅ 已实测启动 |
| `README.md` | 项目根 | ✅ |
| `docs/product-spec.md` | | ✅ |
| `docs/architecture.md` | | ✅ |
| `docs/platform-support.md` | | ✅ |
| `docs/security.md` | | ✅ |
| `docs/progress.md` | | ✅（本文件） |
| `docs/decisions/` | 0001–0006 | ✅ |
| 单元与集成测试 | 165 个 | ✅ |
| 可测试的 Skill fixtures | `crates/skillforge-core/tests/fixtures/` | ✅ |
| 开发环境运行方式 + 可复现打包说明 | `README.md` + `packaging/` | ✅（macOS） |

## 11. 规格适用性确认（2026-10-17）

本实现的技术栈是 **Rust + Tauri**。主规格的桌面框架章节按 Tauri 技术栈落地，
不再作为验收项，也不再被描述为「偏离 / 违反规格」：

| 主规格条款 | 本实现 |
| --- | --- |
| §7.1「不引入 WebView」 | 不适用（Tauri 的渲染层就是系统 WebView） |
| §7.1「不运行本地服务端」 | 适用且已满足（无服务端、无端口） |
| §7 桌面技术架构 / §12 阶段 A 最小窗口 / §14「可运行的 Rust 桌面项目」 | 以 Tauri 等价物实现 |
| §1 / §6 / §9 / §10 / §11 / §13 | 全部适用并已遵循 |

逐条对照与后果见 `decisions/0006-spec-applicability.md`；
`README.md`、`CHANGELOG.md`、`docs/product-spec.md`、
`decisions/0001`、`decisions/0004` 中的相关表述已同步更新。
**未改动的部分**：§10（安全）与 §11（测试/验收）的全部要求仍然适用。

## 12. GitHub 发布流水线（2026-10-21）

目标：把 `skillforge-tauri/` 作为仓库根推送到
<https://github.com/kakalotte/SkillEditor>，由原生 GitHub Actions runner 生成
macOS、Windows、Linux 安装包，先进 Draft Release，人工验收后再发布。

新增与调整：

- `.github/workflows/ci.yml`：macOS / Windows / Ubuntu 22.04 三平台
  `cargo fmt`、`cargo clippy`、`cargo test`、`node --check`。
- `.github/workflows/release.yml`：`v*` tag 触发；先由 `create_release` 创建
  或复用 Draft Release，再由四个矩阵任务上传产物，避免多个任务并发创建 release。
  - macOS arm64 / x86_64 各一个 DMG，Developer ID 签名 + 公证；
  - Windows NSIS `.exe` + MSI `.msi`，CI 导入 PFX 后签名；
  - Linux `.deb` + `.AppImage`，不签名。
- `docs/releasing.md`：必需的 GitHub Secrets、Apple/Windows 证书准备、
  tag 发布、Draft Release 验收与重试步骤。
- `Cargo.toml` 的 `repository` 改为真实仓库地址。
- `frontend_contract.rs` 新增版本一致性测试：`Cargo.toml` 与
  `tauri.conf.json` 的版本必须相同。
- 本地 `packaging/make-macos-app.sh` 保持 ad-hoc 签名，不承担分发职责。

本地验证（未运行 GitHub Actions）：

```
$ cargo fmt --all --check                          # 无输出
$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile ... (0 warnings)
$ cargo test --workspace
    32 + 9 + 94 + 9 + 9 + 5 + 7 = 165 passed
$ node --check ui/app.js                           # 通过
```

### 仍需在 GitHub 上验证

- 首次三平台 CI 是否全绿；Windows/Linux 的平台分支、原生对话框与
  「在文件管理器中显示」仍需人工验证。
- GitHub Secrets 是否已配置：Apple 证书/公证凭据与 Windows PFX。
  未配置时发布工作流会**主动失败**，不会静默产出未签名安装包。
- `v0.1.0` tag 与 Draft Release 的四类产物是否齐全、签名是否有效。
- Draft Release 下载安装后的启动、编辑、保存、导入/导出与卸载流程。
