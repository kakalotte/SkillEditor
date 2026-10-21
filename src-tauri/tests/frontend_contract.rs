//! Contract tests between the front end and the Rust command registry.
//!
//! The UI is plain JavaScript with no build step, so nothing would catch a typo
//! in a command name at compile time. These tests close that gap:
//!
//! * every command the front end invokes must be registered in
//!   `generate_handler!`, and every registered command must be used,
//! * the files `index.html` references must exist,
//! * `frontendDist` must point at the real front-end directory,
//! * the capability file must grant the dialog permissions the pickers need.
//!
//! They run with `cargo test` and need no Node.js.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn package_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("无法读取 {}: {e}", path.display()))
}

/// Command names listed in `generate_handler![ ... ]`.
fn registered_commands(main_rs: &str) -> BTreeSet<String> {
    let start = main_rs
        .find("generate_handler![")
        .expect("main.rs must contain a generate_handler! block");
    let rest = &main_rs[start..];
    let end = rest
        .find("])")
        .expect("generate_handler! block must be closed");
    let mut names = BTreeSet::new();
    for line in rest[..end].lines() {
        let line = line.trim().trim_end_matches(',');
        if let Some(name) = line.strip_prefix("commands::") {
            names.insert(name.to_string());
        }
    }
    names
}

/// Command names the front end passes to a wrapper such as `run(`.
fn commands_invoked_with(prefix: &str, app_js: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut search = app_js;
    while let Some(index) = search.find(prefix) {
        let after = &search[index + prefix.len()..];
        if let Some(close) = after.find('"') {
            let name = &after[..close];
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                names.insert(name.to_string());
            }
        }
        search = &search[index + prefix.len()..];
    }
    names
}

/// Command names the front end calls through `call`, `run`, `runAfterFlush` or
/// `invoke` (the raw wrapper used by the diagnostic logger).
fn invoked_commands(app_js: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for prefix in ["call(\"", "run(\"", "runAfterFlush(\"", "invoke(\""] {
        names.extend(commands_invoked_with(prefix, app_js));
    }
    names
}

/// Return type of every `#[tauri::command]`, keyed by function name.
///
/// Multi-line signatures (`preview_install`, `uninstall`, `delete_skill`, …)
/// are read up to the body's opening brace, so wrapping does not matter.
fn command_return_types(commands_rs: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for chunk in commands_rs.split("#[tauri::command]").skip(1) {
        let Some(fn_index) = chunk.find("fn ") else {
            continue;
        };
        let after = &chunk[fn_index + 3..];
        let name_end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        let name = after[..name_end].to_string();

        let signature = &chunk[..chunk.find('{').unwrap_or(chunk.len())];
        let Some(arrow) = signature.find("-> Result<") else {
            // `log_frontend` returns `()`: never driven by `run`.
            continue;
        };
        let ret = &signature[arrow + "-> Result<".len()..];
        let ret_end = ret.find(',').unwrap_or(ret.len());
        map.insert(name, ret[..ret_end].trim().to_string());
    }
    map
}

#[test]
fn every_invoked_command_is_registered() {
    let main_rs = read(&package_dir().join("src/main.rs"));
    let app_js = read(&package_dir().join("../ui/app.js"));
    let registered = registered_commands(&main_rs);
    let invoked = invoked_commands(&app_js);

    assert!(
        !invoked.is_empty(),
        "front-end command scan found nothing; the test is no longer meaningful"
    );

    let missing: Vec<_> = invoked.difference(&registered).collect();
    assert!(
        missing.is_empty(),
        "前端调用了未注册的命令: {missing:?}（已注册: {registered:?}）"
    );

    let unused: Vec<_> = registered.difference(&invoked).collect();
    assert!(
        unused.is_empty(),
        "已注册但前端未使用的命令: {unused:?}（调用: {invoked:?}）"
    );
}

#[test]
fn front_end_assets_exist_and_are_referenced() {
    let ui = package_dir().join("../ui");
    let html = read(&ui.join("index.html"));
    for asset in ["app.js", "styles.css"] {
        assert!(html.contains(asset), "index.html 未引用 {asset}");
        assert!(ui.join(asset).is_file(), "缺少 ui/{asset}");
    }
}

#[test]
fn tauri_config_points_at_the_real_front_end() {
    let raw = read(&package_dir().join("tauri.conf.json"));
    let config: serde_json::Value =
        serde_json::from_str(&raw).expect("tauri.conf.json 必须是合法 JSON");

    let dist = config["build"]["frontendDist"]
        .as_str()
        .expect("build.frontendDist 必须存在");
    let dist_path = package_dir().join(dist);
    assert!(
        dist_path.join("index.html").is_file(),
        "frontendDist {dist} 中没有 index.html"
    );

    assert_eq!(
        config["app"]["withGlobalTauri"], true,
        "前端依赖 window.__TAURI__（无 npm 构建），必须开启 withGlobalTauri"
    );

    assert_eq!(
        config["identifier"].as_str(),
        Some("io.skillforge.desktop"),
        "bundle identifier 改变会影响 macOS 数据目录"
    );

    let window = &config["app"]["windows"][0];
    assert_eq!(window["label"].as_str(), Some("main"));
    assert_eq!(window["title"].as_str(), Some("SkillForge"));
}

#[test]
fn workspace_and_tauri_versions_match() {
    // The release tag, Cargo metadata and the bundle version must not drift:
    // Tauri writes `tauri.conf.json`'s version into the installers, while the
    // package metadata comes from Cargo.toml.
    let cargo_toml = read(&package_dir().join("../Cargo.toml"));
    let mut in_workspace_package = false;
    let workspace_version = cargo_toml
        .lines()
        .find_map(|line| {
            let line = line.trim();
            if line == "[workspace.package]" {
                in_workspace_package = true;
                return None;
            }
            if line.starts_with('[') {
                in_workspace_package = false;
                return None;
            }
            if in_workspace_package {
                line.strip_prefix("version = \"")
                    .and_then(|value| value.strip_suffix('"'))
                    .map(str::to_owned)
            } else {
                None
            }
        })
        .expect("Cargo.toml 的 [workspace.package] 必须有 version");

    let raw = read(&package_dir().join("tauri.conf.json"));
    let config: serde_json::Value =
        serde_json::from_str(&raw).expect("tauri.conf.json 必须是合法 JSON");
    let tauri_version = config["version"]
        .as_str()
        .expect("tauri.conf.json 必须有字符串 version");

    assert_eq!(
        workspace_version, tauri_version,
        "Cargo.toml 与 tauri.conf.json 的版本必须一致"
    );
}

#[test]
fn capability_grants_the_dialog_permissions_the_pickers_need() {
    let raw = read(&package_dir().join("capabilities/default.json"));
    let capability: serde_json::Value =
        serde_json::from_str(&raw).expect("capabilities/default.json 必须是合法 JSON");

    let permissions: Vec<&str> = capability["permissions"]
        .as_array()
        .expect("permissions 必须是数组")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    assert!(permissions.contains(&"core:default"), "缺少 core:default");
    assert!(
        permissions.contains(&"dialog:default"),
        "缺少 dialog:default：原生文件选择器会失败"
    );

    let windows: Vec<&str> = capability["windows"]
        .as_array()
        .expect("windows 必须是数组")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(windows, vec!["main"]);
}

/// Remove JavaScript comments so the checks below match real code only.
fn strip_comments(source: &str) -> String {
    enum Mode {
        Code,
        Line,
        Block,
        Str(char),
    }

    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut mode = Mode::Code;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match mode {
            Mode::Line => {
                if c == '\n' {
                    mode = Mode::Code;
                    out.push(c);
                }
                i += 1;
            }
            Mode::Block => {
                if c == '*' && next == Some('/') {
                    mode = Mode::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Mode::Str(quote) => {
                out.push(c);
                if c == '\\' {
                    if let Some(escaped) = next {
                        out.push(escaped);
                    }
                    i += 2;
                    continue;
                }
                if c == quote {
                    mode = Mode::Code;
                }
                i += 1;
            }
            Mode::Code => {
                if c == '/' && next == Some('/') {
                    mode = Mode::Line;
                    i += 2;
                } else if c == '/' && next == Some('*') {
                    mode = Mode::Block;
                    i += 2;
                } else {
                    if c == '"' || c == '\'' || c == '`' {
                        mode = Mode::Str(c);
                    }
                    out.push(c);
                    i += 1;
                }
            }
        }
    }
    out
}

#[test]
fn front_end_never_uses_inner_html_with_core_data() {
    // Skill names, descriptions and paths are untrusted file content. The UI
    // must build nodes with `textContent` instead of interpolating markup.
    let app_js = strip_comments(&read(&package_dir().join("../ui/app.js")));
    assert!(
        !app_js.contains("innerHTML"),
        "app.js 不得使用 innerHTML：SKILL.md 内容是不可信输入"
    );
    assert!(
        !app_js.contains("insertAdjacentHTML") && !app_js.contains("document.write"),
        "app.js 不得使用会解析 HTML 的 DOM API"
    );
}

#[test]
fn front_end_arguments_are_camel_case() {
    // Tauri renames command arguments to camelCase on the JavaScript side; a
    // snake_case call site compiles in Rust but fails at runtime.
    let app_js = strip_comments(&read(&package_dir().join("../ui/app.js")));
    for needle in ["platformId:", "backupCount:", "detectExternalChanges:"] {
        assert!(
            app_js.contains(needle),
            "app.js 应使用 camelCase 参数 {needle}"
        );
    }
    for needle in ["platform_id:", "backup_count:", "detect_external_changes:"] {
        assert!(
            !app_js.contains(needle),
            "app.js 使用了 snake_case 参数 {needle}，Tauri 会拒绝"
        );
    }
}

/// `run()` in `app.js` feeds its result straight into `applySnapshot()`. A
/// command returning anything other than `Snapshot` (e.g. `preview_install`,
/// which returns `InstallPreview`) would overwrite `state.snapshot` with an
/// object the renderers cannot read, breaking every later render.
///
/// This is the regression test for that bug: it pairs the Rust return types
/// with the JavaScript call sites, which the other contract tests never did.
#[test]
fn run_is_only_used_for_snapshot_commands() {
    let commands_rs = read(&package_dir().join("src/commands.rs"));
    let app_js = strip_comments(&read(&package_dir().join("../ui/app.js")));
    let returns = command_return_types(&commands_rs);

    assert!(
        !returns.is_empty(),
        "未能解析任何命令的返回类型；测试已失效"
    );

    let snapshot_commands: BTreeSet<String> = returns
        .iter()
        .filter(|(_, ret)| ret.as_str() == "Snapshot")
        .map(|(name, _)| name.clone())
        .collect();

    let mut driven = commands_invoked_with("run(\"", &app_js);
    driven.extend(commands_invoked_with("runAfterFlush(\"", &app_js));
    assert!(
        !driven.is_empty(),
        "未找到 run()/runAfterFlush() 调用；测试已失效"
    );

    let wrong: Vec<String> = driven
        .iter()
        .filter(|name| !snapshot_commands.contains(*name))
        .map(|name| match returns.get(name) {
            Some(ret) => format!("{name} -> {ret}"),
            None => format!("{name} -> (未注册)"),
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "run()/runAfterFlush() 会把返回值当作 Snapshot 渲染，只能用于返回 Snapshot 的命令；\
         返回其他类型的命令请改用 call()。违规项: {wrong:?}"
    );

    // Reverse direction: a command that returns a Snapshot but is invoked with
    // `call()` silently skips rendering. `boot` is the one allowed exception —
    // `main()` awaits it and applies the snapshot by hand.
    let unrendered: Vec<_> = snapshot_commands
        .difference(&driven)
        .filter(|name| name.as_str() != "boot")
        .collect();
    assert!(
        unrendered.is_empty(),
        "这些命令返回 Snapshot 却没有通过 run()/runAfterFlush() 应用: {unrendered:?}"
    );
}

#[test]
fn frontmatter_guide_is_a_read_only_modal() {
    let html = read(&package_dir().join("../ui/index.html"));
    let start = html
        .find("id=\"modal-guide\"")
        .expect("index.html 必须包含 Frontmatter 规则模态框");
    let end = html[start..]
        .find("<script")
        .map(|offset| start + offset)
        .unwrap_or(html.len());
    let modal = &html[start..end];

    for id in ["guide-search", "guide-tabs", "guide-status", "guide-body"] {
        assert!(modal.contains(&format!("id=\"{id}\"")), "指南缺少 {id}");
    }
    assert!(
        modal.contains("只读参考"),
        "指南必须说明它是只读参考，不能暗示会编辑文档"
    );
    assert!(
        !modal.contains("<textarea") && !modal.contains("contenteditable"),
        "只读指南不应包含字段/YAML 编辑控件"
    );

    let app_js = strip_comments(&read(&package_dir().join("../ui/app.js")));
    assert!(
        app_js.contains("call(\"frontmatter_guide\")"),
        "指南必须通过 call() 读取，不能把非 Snapshot 结果交给 run()"
    );
    assert!(
        app_js.contains("function openGuide"),
        "前端缺少打开指南的入口"
    );
    assert!(
        app_js.contains("renderGuideStatus(guide, !query && state.guideTab === \"required\")"),
        "当前文档状态只应在「必填字段」页且无搜索时显示"
    );
    assert!(
        !app_js.contains("平台列表上限"),
        "平台列表上限只放在「平台扩展」页，不应重复出现在状态栏"
    );

    let css = read(&package_dir().join("../ui/styles.css"));
    assert!(
        css.contains(".guide-status[hidden]"),
        "状态栏隐藏时需要显式的 display:none 规则"
    );
}
