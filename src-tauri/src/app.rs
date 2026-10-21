//! The bridge between the Tauri command layer and `skillforge-core`.
//!
//! Every operation is implemented here as a plain function over `&Core` /
//! `&mut Core`. The `#[tauri::command]` wrappers in `commands.rs` only deal
//! with dialogs, locking and error conversion, which keeps all real behaviour
//! testable without starting a window.
//!
//! State model: [`Core`] owns the settings, the workspace service, the install
//! services and the currently open document. Every non-text operation returns a
//! complete [`Snapshot`] so the front end never reconciles partial updates.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use skillforge_core::domain::naming;
use skillforge_core::domain::path_safety;
use skillforge_core::domain::skill::{SkillDocument, SkillId};
use skillforge_core::domain::validation::{
    IssueSource, Severity, ValidationIssue, ValidationReport,
};
use skillforge_core::formats::markdown;
use skillforge_core::guide;
use skillforge_core::persistence::settings::{self, Settings, Theme};
use skillforge_core::platforms::{self, FieldRewrite, InstallTarget, PlatformContext, Scope};
use skillforge_core::services::install_service::{
    self, InstallPlan, Manifest, ManifestStore, UninstallPlan,
};
use skillforge_core::services::{
    self, fs_util, validation_service, BackupService, SettingsService, SkillEntry, WorkspaceService,
};

use crate::dto::{
    Counts, DiffItem, EditAck, EditorState, FrontmatterGuide, GuideField, GuideLimits,
    GuidePlatform, GuideRule, InstallPreview, IssueItem, MarkdownStats, PlatformItem, ResourceItem,
    RewriteItem, SettingsDto, SettingsPatch, SkillItem, Snapshot, Status, TargetItem,
    UninstallPreview,
};

/// Mutable application state owned by the Tauri process.
pub struct Core {
    settings_service: SettingsService,
    settings: Settings,
    workspace: WorkspaceService,
    backups: BackupService,
    manifests: ManifestStore,
    project_dir: Option<PathBuf>,
    adapters: Vec<Box<dyn platforms::PlatformAdapter>>,
    entries: Vec<SkillEntry>,
    open: Option<SkillDocument>,
    search: String,
    pending_plan: Option<InstallPlan>,
    next_draft_seq: u64,
    status: Status,
}

impl Core {
    /// Build the state from a settings service. Kept explicit so tests can point
    /// the config and data roots at a temporary directory.
    pub fn new(settings_service: SettingsService) -> Self {
        let loaded = settings_service.load();
        let settings = loaded.settings;
        let project_dir = settings.project_dir.clone().or_else(detect_project_dir);
        let backups = BackupService::new(settings_service.backup_dir(), settings.backup_count);
        let manifests = ManifestStore::new(settings_service.manifest_dir());
        let workspace = WorkspaceService::new(settings.workspace.clone());
        let status = if loaded.warnings.is_empty() {
            Status::info("就绪")
        } else {
            Status::warning(loaded.warnings.join("；"))
        };

        Core {
            settings_service,
            settings,
            workspace,
            backups,
            manifests,
            project_dir,
            adapters: platforms::all_adapters(),
            entries: Vec::new(),
            open: None,
            search: String::new(),
            pending_plan: None,
            next_draft_seq: 1,
            status,
        }
    }

    /// Build the state from explicit roots. Test-only: the app always uses the
    /// real user profile, tests never touch it.
    #[cfg(test)]
    pub fn from_roots(config_root: impl Into<PathBuf>, data_root: impl Into<PathBuf>) -> Self {
        Core::new(SettingsService::new(config_root, data_root))
    }

    /// Build the state from the real user profile, creating nothing yet.
    pub fn from_environment() -> Self {
        Core::new(SettingsService::default())
    }

    /// Ambient environment used for target discovery.
    fn platform_context(&self) -> PlatformContext {
        PlatformContext {
            home: dirs::home_dir(),
            project_dir: self.project_dir.clone(),
            custom_paths: self.settings.custom_paths.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Bootstrap and snapshot
// ---------------------------------------------------------------------------

/// Create the workspace directory if needed and build the first snapshot.
pub fn boot(core: &mut Core) -> Snapshot {
    if let Err(err) = core.workspace.ensure_workspace() {
        core.status = Status::error(format!("无法创建工作区: {err}"));
    }
    reload_entries(core);
    snapshot(core)
}

/// Render everything the front end needs for the current screen.
pub fn snapshot(core: &Core) -> Snapshot {
    let query = core.search.to_lowercase();
    let open_id = core.open.as_ref().map(|d| d.id.to_string());
    let skills = core
        .entries
        .iter()
        .filter(|e| {
            query.is_empty()
                || e.name.to_lowercase().contains(&query)
                || e.description.to_lowercase().contains(&query)
        })
        .map(|e| {
            let dirty = open_id.as_deref() == Some(e.id().as_str())
                && core.open.as_ref().map(|d| d.state.dirty).unwrap_or(false);
            SkillItem {
                id: e.id(),
                name: e.name.clone(),
                description: e.description.clone(),
                path: e.root.display().to_string(),
                modified: format_time(e.modified),
                status: install_status(core, &e.name),
                dirty,
                parse_ok: e.parse_ok,
            }
        })
        .collect();

    let report = current_report(core);

    Snapshot {
        app_version: skillforge_core::VERSION.to_string(),
        workspace: core.workspace.workspace().display().to_string(),
        project_dir: core
            .project_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        status: core.status.clone(),
        theme: theme_name(core.settings.theme).to_string(),
        search: core.search.clone(),
        skills,
        editor: core.open.as_ref().map(editor_state),
        issues: issue_items(&report),
        counts: counts_of(&report),
        platforms: platform_items(core),
        targets: target_items(core),
        settings: settings_dto(core),
    }
}

fn settings_dto(core: &Core) -> SettingsDto {
    SettingsDto {
        workspace: core.settings.workspace.display().to_string(),
        theme: theme_name(core.settings.theme).to_string(),
        backup_count: core.settings.backup_count,
        detect_external_changes: core.settings.detect_external_changes,
        config_path: core.settings_service.config_root().display().to_string(),
        log_dir: core.settings_service.log_dir().display().to_string(),
        backup_dir: core.settings_service.backup_dir().display().to_string(),
        manifest_dir: core.settings_service.manifest_dir().display().to_string(),
    }
}

/// Read-only frontmatter rules for the editor guide.
///
/// This deliberately returns a dedicated DTO instead of a [`Snapshot`]: opening
/// the guide must not change application state or mark the document dirty.
pub fn frontmatter_guide(core: &Core) -> FrontmatterGuide {
    let fields = guide::FIELDS
        .iter()
        .map(|field| GuideField {
            key: field.key.to_string(),
            label: field.label.to_string(),
            value_type: field.value_type.to_string(),
            required: field.required,
            category: field.category.as_str().to_string(),
            summary: field.summary.to_string(),
            rules: field.rules.iter().map(|s| (*s).to_string()).collect(),
            example: field.example.to_string(),
            common_mistake: field.common_mistake.to_string(),
            doc_url: field.docs_url.to_string(),
            aliases: field.aliases.iter().map(|s| (*s).to_string()).collect(),
            platform_ids: core
                .adapters
                .iter()
                .filter(|adapter| adapter.capabilities().supports_field(field.key))
                .map(|adapter| adapter.id().to_string())
                .collect(),
        })
        .collect();

    let rules = guide::RULES
        .iter()
        .map(|rule| GuideRule {
            id: rule.id.to_string(),
            severity: rule.severity.to_string(),
            source: rule.source.to_string(),
            source_label: rule.source_label.to_string(),
            summary: rule.summary.to_string(),
            fix: rule.fix.to_string(),
            doc_url: rule.docs_url.to_string(),
        })
        .collect();

    let platforms = core
        .adapters
        .iter()
        .map(|adapter| {
            let caps = adapter.capabilities();
            GuidePlatform {
                id: adapter.id().to_string(),
                name: adapter.display_name().to_string(),
                listing_limit: caps.listing_limit,
                notes: caps.notes.to_string(),
                docs_url: adapter.docs_url().to_string(),
            }
        })
        .collect();

    FrontmatterGuide {
        fields,
        rules,
        platforms,
        directory_rules: guide::DIRECTORY_RULES
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        body_rules: guide::BODY_RULES.iter().map(|s| (*s).to_string()).collect(),
        example: guide::FULL_EXAMPLE.to_string(),
        limits: GuideLimits {
            name_max: guide::NAME_MAX_LEN,
            description_max: guide::DESCRIPTION_MAX_LEN,
            compatibility_max: guide::COMPATIBILITY_MAX_LEN,
            body_max_lines: guide::BODY_MAX_LINES,
            body_max_tokens: guide::BODY_MAX_TOKENS,
        },
    }
}

fn editor_state(doc: &SkillDocument) -> EditorState {
    EditorState {
        id: doc.id.to_string(),
        name: doc.name(),
        description: doc.description(),
        body: doc.instructions.clone(),
        path: doc.root.display().to_string(),
        dirty: doc.state.dirty,
        read_only: !doc.state.parse_ok,
        parse_ok: doc.state.parse_ok,
        frontmatter_present: doc.state.frontmatter_present,
        frontmatter_keys: doc.frontmatter.keys(),
        resources: doc.resources.iter().map(resource_item).collect(),
        stats: stats_of(&markdown::analyze(&doc.instructions)),
    }
}

fn resource_item(resource: &skillforge_core::domain::resource::SkillResource) -> ResourceItem {
    ResourceItem {
        path: path_safety::to_slash(&resource.relative_path),
        kind: resource.kind.as_str().to_string(),
        size: resource.human_size(),
    }
}

fn stats_of(facts: &markdown::MarkdownFacts) -> MarkdownStats {
    MarkdownStats {
        lines: facts.lines,
        characters: facts.characters,
        headings: facts.headings,
        list_items: facts.list_items,
        code_fences: facts.code_fences,
        links: facts.links,
        approx_tokens: facts.approx_tokens(),
    }
}

fn counts_of(report: &ValidationReport) -> Counts {
    Counts {
        errors: report.errors(),
        warnings: report.warnings(),
        infos: report.infos(),
    }
}

fn issue_items(report: &ValidationReport) -> Vec<IssueItem> {
    report.sorted().iter().map(issue_item).collect()
}

fn issue_item(issue: &ValidationIssue) -> IssueItem {
    IssueItem {
        rule_id: issue.rule_id.clone(),
        severity: issue.severity.as_str().to_string(),
        source: source_key(&issue.source).to_string(),
        source_label: issue.source.label_zh(),
        file: issue.file.clone(),
        line: issue.line.map(|l| l.to_string()).unwrap_or_default(),
        message: issue.message.clone(),
        suggestion: issue.suggestion.clone(),
        doc_url: issue.doc_url.clone().unwrap_or_default(),
    }
}

fn source_key(source: &IssueSource) -> &'static str {
    match source {
        IssueSource::Spec => "spec",
        IssueSource::Quality => "quality",
        IssueSource::Platform(_) => "platform",
    }
}

fn current_report(core: &Core) -> ValidationReport {
    match core.open.as_ref() {
        Some(doc) => {
            validation_service::validate_with_context(doc, &core.adapters, &core.platform_context())
        }
        None => ValidationReport::new(),
    }
}

fn platform_items(core: &Core) -> Vec<PlatformItem> {
    let ctx = core.platform_context();
    // Projection is a pure text transform. For a clean document, read exactly
    // what the installer would copy; for unsaved edits, fall back to the
    // in-memory serialization (installation itself is blocked while dirty).
    let skill_md = core.open.as_ref().and_then(|doc| {
        if doc.state.dirty {
            doc.to_skill_md().ok()
        } else {
            std::fs::read_to_string(doc.root.join("SKILL.md"))
                .ok()
                .or_else(|| doc.to_skill_md().ok())
        }
    });
    core.adapters
        .iter()
        .map(|adapter| {
            let caps = adapter.capabilities();
            let detection = adapter.detect(&ctx);
            let (compat_status, compat_warnings) = match core.open.as_ref() {
                None => ("unknown", 0),
                Some(doc) if !doc.state.parse_ok => ("unknown", 0),
                Some(doc) => {
                    let warnings = adapter
                        .validate(doc)
                        .iter()
                        .filter(|i| i.severity == Severity::Warning)
                        .count();
                    (if warnings > 0 { "warning" } else { "ok" }, warnings)
                }
            };
            let compat_rewrites = skill_md
                .as_deref()
                .and_then(|md| adapter.project(md))
                .map(|p| p.rewrites.len())
                .unwrap_or(0);
            PlatformItem {
                id: adapter.id().to_string(),
                name: adapter.display_name().to_string(),
                verification: adapter.verification().as_str().to_string(),
                verification_label: adapter.verification().label_zh().to_string(),
                docs_url: adapter.docs_url().to_string(),
                verified_on: adapter.verified_on().to_string(),
                detected: detection.found,
                detect_detail: detection.detail,
                supported_fields: caps
                    .supported_fields
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
                listing_limit: caps.listing_limit,
                supports_resources: caps.supports_resources,
                resource_dirs: caps
                    .resource_dirs
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
                notes: caps.notes.to_string(),
                compat_status: compat_status.to_string(),
                compat_warnings,
                compat_rewrites,
            }
        })
        .collect()
}

fn target_items(core: &Core) -> Vec<TargetItem> {
    let ctx = core.platform_context();
    let name = core
        .open
        .as_ref()
        .map(|d| d.name())
        .filter(|n| !n.is_empty());
    let mut items = Vec::new();
    for adapter in &core.adapters {
        for target in adapter.discover_targets(&ctx) {
            let installed = name
                .as_ref()
                .map(|n| core.manifests.load(&target.path.join(n)).is_some())
                .unwrap_or(false);
            items.push(target_item(&target, installed));
        }
    }
    items
}

fn target_item(target: &InstallTarget, installed: bool) -> TargetItem {
    TargetItem {
        platform_id: target.platform_id.clone(),
        scope: target.scope.as_str().to_string(),
        scope_label: scope_label(target),
        label: target.label.clone(),
        path: target.display_path(),
        exists: target.exists,
        installed,
    }
}

fn scope_label(target: &InstallTarget) -> String {
    match target.variant {
        Some(variant) => format!("{} · {variant}", target.scope.label_zh()),
        None => target.scope.label_zh().to_string(),
    }
}

fn rewrite_item(rewrite: &FieldRewrite) -> RewriteItem {
    RewriteItem {
        from: rewrite.from.clone(),
        to: rewrite.to.clone(),
        reason: rewrite.reason.clone(),
    }
}

/// How a skill relates to the known install targets.
fn install_status(core: &Core, name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let ctx = core.platform_context();
    for adapter in &core.adapters {
        for target in adapter.discover_targets(&ctx) {
            let target_dir = target.path.join(name);
            if let Some(manifest) = core.manifests.load(&target_dir) {
                if manifest.skill_name != name {
                    continue;
                }
                if compare_to_manifest(&target_dir, &manifest) {
                    return "installed".to_string();
                }
                return "outdated".to_string();
            }
            if target_dir.join("SKILL.md").is_file() {
                return "attention".to_string();
            }
        }
    }
    String::new()
}

/// True when every file we installed still matches its recorded hash.
fn compare_to_manifest(dir: &Path, manifest: &Manifest) -> bool {
    for (rel, expected) in &manifest.files {
        match fs_util::hash_file(&dir.join(rel)) {
            Ok(hash) if &hash == expected => {}
            _ => return false,
        }
    }
    true
}

fn reload_entries(core: &mut Core) {
    match core.workspace.list_entries() {
        Ok(entries) => core.entries = entries,
        Err(err) => {
            core.entries.clear();
            core.status = Status::error(format!("读取工作区失败: {err}"));
        }
    }
}

fn find_entry(core: &Core, id: &str) -> Option<SkillEntry> {
    core.entries.iter().find(|e| e.id() == id).cloned()
}

// ---------------------------------------------------------------------------
// Skills list
// ---------------------------------------------------------------------------

pub fn refresh_skills(core: &mut Core) -> Snapshot {
    reload_entries(core);
    core.status = Status::ok("已刷新");
    snapshot(core)
}

pub fn set_search(core: &mut Core, text: &str) -> Snapshot {
    core.search = text.to_string();
    snapshot(core)
}

pub fn new_skill(core: &mut Core) -> Snapshot {
    let seq = core.next_draft_seq;
    core.next_draft_seq += 1;
    let root = core.workspace.workspace().join(format!("new-skill-{seq}"));
    let doc = SkillDocument::new_draft(SkillId::draft(seq), root, "", "");
    core.open = Some(doc);
    core.status = Status::info("新建 Skill：填写名称与描述后保存。");
    snapshot(core)
}

pub fn open_skill(core: &mut Core, id: &str) -> Snapshot {
    let Some(entry) = find_entry(core, id) else {
        core.status = Status::warning("未找到该 Skill，可能已被移除。");
        return snapshot(core);
    };
    match core.workspace.load(&entry.root) {
        Ok(doc) => {
            let name = doc.name();
            core.settings
                .touch_recent(entry.root.clone(), name, services::now_secs());
            core.open = Some(doc);
        }
        Err(err) => core.status = Status::error(format!("打开失败: {err}")),
    }
    snapshot(core)
}

/// Delete a skill directory. `confirm` must be true: the front end shows a
/// confirmation dialog, and this guard keeps the destructive path explicit.
pub fn delete_skill(core: &mut Core, id: &str, confirm: bool) -> Snapshot {
    if !confirm {
        core.status = Status::warning("删除操作未确认，已取消。");
        return snapshot(core);
    }
    let Some(entry) = find_entry(core, id) else {
        core.status = Status::warning("未找到该 Skill，可能已被移除。");
        return snapshot(core);
    };
    match core.workspace.delete(&entry.root) {
        Ok(()) => {
            if core
                .open
                .as_ref()
                .map(|d| d.root == entry.root)
                .unwrap_or(false)
            {
                core.open = None;
            }
            reload_entries(core);
            core.status = Status::ok(format!("已删除 {}", entry.name));
        }
        Err(err) => core.status = Status::error(format!("删除失败: {err}")),
    }
    snapshot(core)
}

/// Directory of a skill, for the "reveal in file manager" command.
pub fn skill_root(core: &Core, id: &str) -> Result<PathBuf, String> {
    find_entry(core, id)
        .map(|e| e.root)
        .ok_or_else(|| "未找到该 Skill".to_string())
}

// ---------------------------------------------------------------------------
// Editor
// ---------------------------------------------------------------------------

/// Rename the skill. Ignored while the frontmatter is unparseable, because the
/// raw text is the only representation that can be preserved verbatim.
pub fn set_name(core: &mut Core, value: &str) -> EditAck {
    if let Some(doc) = core.open.as_mut() {
        if doc.state.parse_ok {
            doc.set_name(value);
            doc.mark_dirty();
        }
    }
    edit_ack(core)
}

pub fn set_description(core: &mut Core, value: &str) -> EditAck {
    if let Some(doc) = core.open.as_mut() {
        if doc.state.parse_ok {
            doc.set_description(value);
            doc.mark_dirty();
        }
    }
    edit_ack(core)
}

/// Replace the body. When the document is in raw-text fallback mode the text is
/// the whole file, so it is re-parsed and the document recovers automatically
/// as soon as the frontmatter becomes valid again.
pub fn set_body(core: &mut Core, value: &str) -> EditAck {
    if let Some(doc) = core.open.as_mut() {
        if doc.state.parse_ok {
            doc.set_instructions(value);
        } else {
            repair_raw(doc, value);
        }
    }
    edit_ack(core)
}

/// Re-parse raw text, keeping the draft identity and the resource list.
fn repair_raw(doc: &mut SkillDocument, text: &str) {
    let root = doc.root.clone();
    let id = doc.id.clone();
    let mut resources = std::mem::take(&mut doc.resources);
    let mut reparsed = SkillDocument::from_skill_md(root, text);
    std::mem::swap(&mut reparsed.resources, &mut resources);
    reparsed.id = id;
    reparsed.state.dirty = true;
    *doc = reparsed;
}

fn edit_ack(core: &Core) -> EditAck {
    let report = current_report(core);
    let doc = core.open.as_ref();
    let facts = doc
        .map(|d| markdown::analyze(&d.instructions))
        .unwrap_or_default();
    EditAck {
        dirty: doc.map(|d| d.state.dirty).unwrap_or(false),
        parse_ok: doc.map(|d| d.state.parse_ok).unwrap_or(true),
        read_only: doc.map(|d| !d.state.parse_ok).unwrap_or(false),
        counts: counts_of(&report),
        issues: issue_items(&report),
        stats: stats_of(&facts),
        frontmatter_keys: doc.map(|d| d.frontmatter.keys()).unwrap_or_default(),
    }
}

/// The rendered `SKILL.md` for the preview tab (never a rich Markdown renderer;
/// see docs/decisions/0004-frontend-and-ipc.md).
pub fn preview_text(core: &Core) -> Result<String, String> {
    let doc = core
        .open
        .as_ref()
        .ok_or_else(|| "没有打开的 Skill".to_string())?;
    doc.to_skill_md().map_err(|e| e.to_string())
}

pub fn save(core: &mut Core) -> Snapshot {
    let Some(mut doc) = core.open.take() else {
        core.status = Status::warning("没有打开的 Skill。");
        return snapshot(core);
    };

    if !doc.state.parse_ok {
        core.open = Some(doc);
        core.status = Status::error("frontmatter 无法解析，请先修复后再保存。");
        return snapshot(core);
    }

    let name = doc.name();
    if name.is_empty() {
        core.open = Some(doc);
        core.status = Status::error("请先填写有效的 name 再保存。");
        return snapshot(core);
    }

    if !doc.root.exists() {
        // A new skill derives its directory from `name`, so the name must be
        // validated and must not silently take over an existing directory.
        if let Err(err) = naming::validate_skill_name(&name) {
            core.open = Some(doc);
            core.status = Status::error(format!("name 不合法，无法保存: {err}"));
            return snapshot(core);
        }
        let candidate = core.workspace.workspace().join(&name);
        if candidate.exists() {
            core.open = Some(doc);
            core.status = Status::error(format!(
                "工作区中已存在 “{name}”，请换一个名称或使用「另存为」。"
            ));
            return snapshot(core);
        }
        doc.root = candidate;
        doc.id = SkillId::from_root(&doc.root);
    }

    match core.workspace.save(&mut doc) {
        Ok(()) => {
            core.settings
                .touch_recent(doc.root.clone(), name.clone(), services::now_secs());
            core.open = Some(doc);
            reload_entries(core);
            core.status = Status::ok(format!("已保存 {name}"));
        }
        Err(err) => {
            core.open = Some(doc);
            core.status = Status::error(format!("保存失败: {err}"));
        }
    }
    snapshot(core)
}

/// Save the open skill as a complete copy under `parent/<name>`, including its
/// resource files. Refuses to overwrite an existing `SKILL.md`.
pub fn save_as(core: &mut Core, parent: &Path) -> Snapshot {
    let Some(doc) = core.open.clone() else {
        core.status = Status::warning("没有打开的 Skill。");
        return snapshot(core);
    };
    if !doc.state.parse_ok {
        core.status = Status::error("frontmatter 无法解析，请先修复后再另存为。");
        return snapshot(core);
    }
    let name = doc.name();
    if let Err(err) = naming::validate_skill_name(&name) {
        core.status = Status::error(format!("name 不合法，无法另存为: {err}"));
        return snapshot(core);
    }
    let root = parent.join(&name);
    if root.join("SKILL.md").exists() {
        core.status = Status::error(format!(
            "目标目录已存在 SKILL.md，请换一个位置: {}",
            root.display()
        ));
        return snapshot(core);
    }

    match core.workspace.save_as(&doc, &root).and_then(|new_doc| {
        copy_resources(&doc.root, &root)?;
        Ok(new_doc)
    }) {
        Ok(new_doc) => {
            core.settings
                .touch_recent(new_doc.root.clone(), name, services::now_secs());
            core.open = Some(new_doc);
            reload_entries(core);
            core.status = Status::ok(format!("已另存为 {}", root.display()));
        }
        Err(err) => core.status = Status::error(format!("另存为失败: {err}")),
    }
    snapshot(core)
}

/// Copy every resource file (everything except `SKILL.md`) from one skill root
/// to another. Symlinks are never followed.
fn copy_resources(from_root: &Path, to_root: &Path) -> Result<(), skillforge_core::errors::Error> {
    for resource in services::scan_resources(from_root)? {
        let source = from_root.join(&resource.relative_path);
        if source.is_symlink() {
            continue;
        }
        let bytes =
            std::fs::read(&source).map_err(|e| skillforge_core::errors::Error::io(&source, e))?;
        let destination = path_safety::safe_join(to_root, &resource.relative_path)?;
        fs_util::atomic_write(&destination, &bytes)?;
    }
    Ok(())
}

pub fn export_to(core: &mut Core, file: &Path) -> Snapshot {
    let Some(doc) = core.open.as_ref() else {
        core.status = Status::warning("没有打开的 Skill。");
        return snapshot(core);
    };
    if !doc.root.exists() {
        core.status = Status::error("请先保存 Skill，再导出 ZIP。");
        return snapshot(core);
    }
    let root = doc.root.clone();
    match services::export_zip(&root, file) {
        Ok(count) => {
            core.status = Status::ok(format!("已导出 {count} 个文件到 {}", file.display()));
        }
        Err(err) => core.status = Status::error(format!("导出失败: {err}")),
    }
    snapshot(core)
}

/// Import a skill directory, a `SKILL.md` or a `.zip` and open the result.
pub fn import_from(core: &mut Core, path: &Path) -> Snapshot {
    let result: Result<PathBuf, String> = if path.is_dir() {
        core.workspace
            .import_into_workspace(path)
            .map_err(|e| e.to_string())
    } else if is_zip(path) {
        services::import_zip(path, core.workspace.workspace()).map_err(|e| e.to_string())
    } else if path.file_name().map(|n| n == "SKILL.md").unwrap_or(false) {
        match path.parent() {
            Some(parent) => core
                .workspace
                .import_into_workspace(parent)
                .map_err(|e| e.to_string()),
            None => Err("无法确定 SKILL.md 所在目录".to_string()),
        }
    } else {
        Err("请选择 Skill 目录、SKILL.md 或 .zip 文件".to_string())
    };

    match result {
        Ok(root) => {
            reload_entries(core);
            match core.workspace.load(&root) {
                Ok(doc) => {
                    let name = doc.name();
                    core.settings
                        .touch_recent(root.clone(), name, services::now_secs());
                    core.open = Some(doc);
                    core.status = Status::ok(format!("已导入到 {}", root.display()));
                }
                Err(err) => core.status = Status::error(format!("导入的文件无法读取: {err}")),
            }
        }
        Err(err) => core.status = Status::error(format!("导入失败: {err}")),
    }
    snapshot(core)
}

fn is_zip(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false)
}

pub fn validate_now(core: &mut Core) -> Snapshot {
    core.status = Status::ok("校验完成");
    snapshot(core)
}

// ---------------------------------------------------------------------------
// Platforms and installation
// ---------------------------------------------------------------------------

pub fn refresh_platforms(core: &mut Core) -> Snapshot {
    core.status = Status::ok("已重新检测平台目录");
    snapshot(core)
}

/// Resolve an install target.
///
/// `path` is the target the user actually clicked. It matters on platforms that
/// scan more than one directory per scope (DeepSeek Harness has both
/// `~/.dsh/skills` and `~/.agents/skills`): resolving by scope alone would
/// always pick the first one. An unknown path is rejected rather than trusted.
fn resolve_target(
    core: &Core,
    platform_id: &str,
    scope: &str,
    path: Option<&str>,
) -> Option<InstallTarget> {
    let ctx = core.platform_context();
    let adapter = core.adapters.iter().find(|a| a.id() == platform_id)?;
    let targets = adapter.discover_targets(&ctx);

    if let Some(path) = path {
        let wanted = Path::new(path);
        return targets.into_iter().find(|t| t.path == wanted);
    }

    // Legacy callers (and tests) pass only the scope: prefer the unlabelled
    // target, then the first one found.
    let scope = scope_from_str(scope);
    let mut fallback = None;
    for target in targets {
        if target.scope != scope {
            continue;
        }
        if target.variant.is_none() {
            return Some(target);
        }
        fallback.get_or_insert(target);
    }
    fallback
}

fn scope_from_str(scope: &str) -> Scope {
    match scope {
        "project" => Scope::Project,
        "custom" => Scope::Custom,
        _ => Scope::User,
    }
}

fn platform_label(core: &Core, platform_id: &str) -> String {
    core.adapters
        .iter()
        .find(|a| a.id() == platform_id)
        .map(|a| a.display_name().to_string())
        .unwrap_or_else(|| platform_id.to_string())
}

/// Build an install plan and keep it for `confirm_install`.
pub fn preview_install(
    core: &mut Core,
    platform_id: &str,
    scope: &str,
    path: Option<&str>,
) -> Result<InstallPreview, String> {
    let Some(doc) = core.open.as_ref() else {
        return Err("请先打开一个 Skill 再安装。".to_string());
    };
    let name = doc.name();
    if name.is_empty() {
        return Err("Skill 没有有效的 name，无法安装。".to_string());
    }
    if doc.state.dirty {
        return Err("有未保存的修改，请先保存再安装。".to_string());
    }
    let Some(target) = resolve_target(core, platform_id, scope, path) else {
        return Err("未找到该安装目标。".to_string());
    };
    let Some(adapter) = core.adapters.iter().find(|a| a.id() == platform_id) else {
        return Err("未知平台。".to_string());
    };

    // Projection is computed from the on-disk file, which is exactly what the
    // installer copies (unsaved edits were rejected above).
    let skill_md = std::fs::read_to_string(doc.root.join("SKILL.md"))
        .map_err(|e| format!("无法读取源 SKILL.md（请先保存 Skill 再安装）: {e}"))?;
    let projection = adapter.project(&skill_md);

    // Warn about what will actually be installed, not about fields that the
    // projection already resolved (for example `when_to_use` on DSH).
    let projected_doc = projection
        .as_ref()
        .map(|p| SkillDocument::from_skill_md(doc.root.clone(), &p.content));
    let validate_doc = projected_doc.as_ref().unwrap_or(doc);
    let warnings: Vec<IssueItem> = adapter
        .validate(validate_doc)
        .iter()
        .filter(|i| i.severity == Severity::Warning)
        .map(issue_item)
        .collect();

    let (overrides, core_rewrites, rewrites) = match projection {
        Some(projection) => {
            let preview_rewrites: Vec<RewriteItem> =
                projection.rewrites.iter().map(rewrite_item).collect();
            (
                BTreeMap::from([("SKILL.md".to_string(), projection.content.into_bytes())]),
                projection.rewrites,
                preview_rewrites,
            )
        }
        None => (BTreeMap::new(), Vec::new(), Vec::new()),
    };

    let source_root = doc.root.clone();
    let selected_scope = target.scope;
    let target_scope_label = scope_label(&target);
    let platform_label = adapter.display_name().to_string();

    let plan = install_service::plan_install_with_overrides(
        &source_root,
        &name,
        &target.path,
        platform_id,
        selected_scope,
        &core.manifests,
        install_service::PlanOptions {
            detect_external_modification: core.settings.detect_external_changes,
        },
        overrides,
        core_rewrites,
    )
    .map_err(|e| e.to_string())?;

    let preview = InstallPreview {
        title: format!(
            "安装 {} → {} · {}",
            name, platform_label, target_scope_label
        ),
        target_root: plan.target_root.display().to_string(),
        platform_label,
        added: plan.added(),
        modified: plan.modified(),
        removed: plan.removed(),
        unchanged: plan.unchanged(),
        creates_target: plan.creates_target,
        has_external_modification: plan.has_external_modification,
        will_backup: plan.will_backup,
        warnings,
        rewrites,
        entries: plan
            .entries
            .iter()
            .map(|e| DiffItem {
                path: e.relative_path.display().to_string(),
                change: e.change.as_str().to_string(),
                detail: e.detail.clone(),
            })
            .collect(),
    };
    core.pending_plan = Some(plan);
    Ok(preview)
}

pub fn cancel_install(core: &mut Core) -> Snapshot {
    core.pending_plan = None;
    core.status = Status::info("已取消安装。");
    snapshot(core)
}

pub fn confirm_install(core: &mut Core) -> Snapshot {
    let Some(plan) = core.pending_plan.take() else {
        core.status = Status::warning("没有待确认的安装计划。");
        return snapshot(core);
    };
    match install_service::apply_install(
        &plan,
        &core.backups,
        &core.manifests,
        services::now_secs(),
        skillforge_core::VERSION,
    ) {
        Ok(report) => {
            let backup_note = report
                .backup
                .as_ref()
                .map(|b| format!("，已备份到 {}", b.display()))
                .unwrap_or_default();
            let rewrite_note = if plan.rewrites.is_empty() {
                String::new()
            } else {
                format!("，已适配 {} 个字段", plan.rewrites.len())
            };
            reload_entries(core);
            core.status = Status::ok(format!(
                "已安装 {} 个文件到 {}{}{}",
                report.copied_files,
                report.target_root.display(),
                rewrite_note,
                backup_note
            ));
        }
        Err(err) => core.status = Status::error(format!("安装失败（已尝试恢复）: {err}")),
    }
    snapshot(core)
}

/// Describe what an uninstall would remove, so the front end can confirm it.
pub fn preview_uninstall(
    core: &Core,
    platform_id: &str,
    scope: &str,
    path: Option<&str>,
) -> Result<UninstallPreview, String> {
    let (target, target_dir, plan) = uninstall_plan(core, platform_id, scope, path)?;
    Ok(UninstallPreview {
        title: format!(
            "从 {} 卸载 “{}”",
            platform_label(core, platform_id),
            plan.manifest.skill_name
        ),
        target_root: target_dir.display().to_string(),
        platform_id: platform_id.to_string(),
        scope: target.scope.as_str().to_string(),
        scope_label: scope_label(&target),
        managed_files: plan.managed_files.len(),
        has_external_changes: plan.has_external_changes,
    })
}

/// Remove a skill from a target. `confirm` must be true.
pub fn uninstall(
    core: &mut Core,
    platform_id: &str,
    scope: &str,
    path: Option<&str>,
    confirm: bool,
) -> Snapshot {
    if !confirm {
        core.status = Status::warning("卸载操作未确认，已取消。");
        return snapshot(core);
    }
    let (_target, target_dir, plan) = match uninstall_plan(core, platform_id, scope, path) {
        Ok(pair) => pair,
        Err(err) => {
            core.status = Status::error(err);
            return snapshot(core);
        }
    };
    match install_service::apply_uninstall(&plan, &core.manifests) {
        Ok(()) => {
            reload_entries(core);
            core.status = Status::ok(format!("已从 {} 卸载", target_dir.display()));
        }
        Err(err) => core.status = Status::error(format!("卸载失败: {err}")),
    }
    snapshot(core)
}

fn uninstall_plan(
    core: &Core,
    platform_id: &str,
    scope: &str,
    path: Option<&str>,
) -> Result<(InstallTarget, PathBuf, UninstallPlan), String> {
    let target = resolve_target(core, platform_id, scope, path)
        .ok_or_else(|| "未找到该安装目标。".to_string())?;
    let name = core
        .open
        .as_ref()
        .map(|d| d.name())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| "请先打开要卸载的 Skill。".to_string())?;
    let target_dir = target.path.join(&name);
    let manifest = core
        .manifests
        .load(&target_dir)
        .ok_or_else(|| "该目标不是由本应用安装的，未找到安装清单。".to_string())?;
    let plan = install_service::plan_uninstall(&target_dir, manifest).map_err(|e| e.to_string())?;
    Ok((target, target_dir, plan))
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// Point the workspace at `dir` and re-list it (used by the folder picker).
pub fn set_workspace(core: &mut Core, dir: &Path) -> Snapshot {
    core.settings.workspace = dir.to_path_buf();
    core.workspace.set_workspace(dir.to_path_buf());
    if let Err(err) = core.workspace.ensure_workspace() {
        core.status = Status::error(format!("无法创建工作区: {err}"));
    }
    reload_entries(core);
    snapshot(core)
}

/// Suggested file name for the ZIP export.
pub fn export_default_name(core: &Core) -> String {
    let name = core.open.as_ref().map(|d| d.name()).unwrap_or_default();
    if name.is_empty() {
        "skill.zip".to_string()
    } else {
        format!("{name}.zip")
    }
}

/// True when the open document has unsaved edits (used by the close guard).
pub fn is_dirty(core: &Core) -> bool {
    core.open.as_ref().map(|d| d.state.dirty).unwrap_or(false)
}

pub fn update_settings(core: &mut Core, patch: &SettingsPatch) -> Snapshot {
    if let Some(workspace) = &patch.workspace {
        core.settings.workspace = PathBuf::from(workspace);
    }
    if let Some(theme) = &patch.theme {
        core.settings.theme = theme_from_name(theme);
    }
    if let Some(count) = patch.backup_count {
        core.settings.backup_count = count.clamp(1, 50);
        core.backups.set_keep(core.settings.backup_count);
    }
    if let Some(detect) = patch.detect_external_changes {
        core.settings.detect_external_changes = detect;
    }
    snapshot(core)
}

pub fn save_settings(core: &mut Core) -> Snapshot {
    if core.settings.workspace.as_os_str().is_empty() {
        core.settings.workspace = settings::default_workspace();
    }
    core.settings.backup_count = core.settings.backup_count.clamp(1, 50);
    core.backups.set_keep(core.settings.backup_count);
    core.workspace
        .set_workspace(core.settings.workspace.clone());
    let _ = core.workspace.ensure_workspace();

    match core.settings_service.save(&core.settings) {
        Ok(()) => {
            reload_entries(core);
            core.status = Status::ok("设置已保存");
        }
        Err(err) => core.status = Status::error(format!("保存设置失败: {err}")),
    }
    snapshot(core)
}

pub fn config_root(core: &Core) -> PathBuf {
    core.settings_service.config_root().to_path_buf()
}

fn theme_name(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "system",
        Theme::Light => "light",
        Theme::Dark => "dark",
    }
}

fn theme_from_name(name: &str) -> Theme {
    match name {
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        _ => Theme::System,
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn format_time(time: std::time::SystemTime) -> String {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => {
            let secs = d.as_secs();
            let (year, month, day) = civil_from_days((secs / 86_400) as i64);
            let rem = secs % 86_400;
            format!(
                "{year:04}-{month:02}-{day:02} {:02}:{:02}",
                rem / 3600,
                (rem % 3600) / 60
            )
        }
        Err(_) => "—".to_string(),
    }
}

/// Days since the Unix epoch -> (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Walk up from the current directory looking for a project root.
fn detect_project_dir() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A fully isolated app state: temp config/data/workspace roots and a
    /// "generic" install target inside the temp directory, so tests never read
    /// or write the developer's real profile.
    struct Fixture {
        _dir: tempfile::TempDir,
        core: Core,
        workspace: PathBuf,
        targets: PathBuf,
        config_root: PathBuf,
    }

    fn setup() -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        let config_root = dir.path().join("config");
        let data_root = dir.path().join("data");
        let targets = dir.path().join("targets");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::create_dir_all(&targets).expect("targets");
        std::fs::create_dir_all(&config_root).expect("config root");

        let settings = Settings {
            workspace: workspace.clone(),
            custom_paths: BTreeMap::from([("generic".to_string(), targets.clone())]),
            ..Settings::default()
        };
        std::fs::write(
            config_root.join("config.json"),
            serde_json::to_vec_pretty(&settings).expect("serialize settings"),
        )
        .expect("write settings");

        let core = Core::from_roots(&config_root, &data_root);
        Fixture {
            _dir: dir,
            core,
            workspace,
            targets,
            config_root,
        }
    }

    fn write_skill(root: &Path, body: &str) {
        std::fs::create_dir_all(root).expect("create skill dir");
        std::fs::write(root.join("SKILL.md"), body).expect("write SKILL.md");
    }

    fn valid_skill(name: &str) -> String {
        format!(
            "---\nname: {name}\ndescription: 用于测试应用层流程的 Skill，描述足够长以通过检查。\n---\n\n# 步骤\n\n1. 第一步\n2. 第二步\n"
        )
    }

    #[test]
    fn boot_lists_skills_from_the_workspace() {
        let mut fixture = setup();
        write_skill(&fixture.workspace.join("alpha"), &valid_skill("alpha"));
        write_skill(&fixture.workspace.join("beta"), &valid_skill("beta"));

        let snapshot = boot(&mut fixture.core);

        assert_eq!(snapshot.skills.len(), 2);
        assert_eq!(snapshot.app_version, skillforge_core::VERSION);
        assert!(snapshot.workspace.contains("workspace"));
        assert_eq!(snapshot.status.kind, "info");
    }

    #[test]
    fn create_save_and_reload_round_trip() {
        let mut fixture = setup();
        let created = new_skill(&mut fixture.core);
        assert!(created.editor.is_some());

        set_name(&mut fixture.core, "demo-skill");
        set_description(
            &mut fixture.core,
            "一个用于演示保存流程的 Skill，描述足够长。",
        );
        set_body(&mut fixture.core, "# 步骤\n\n1. 做一件事\n");
        let saved = save(&mut fixture.core);

        assert_eq!(saved.status.kind, "ok", "{}", saved.status.message);
        let path = fixture.workspace.join("demo-skill/SKILL.md");
        assert!(path.is_file(), "SKILL.md was not written");
        let text = std::fs::read_to_string(&path).expect("read back");
        assert!(text.contains("name: demo-skill"));
        assert!(text.contains("做一件事"));

        // Reloading from disk sees the same content.
        let opened = open_skill(&mut fixture.core, &saved.skills[0].id);
        let editor = opened.editor.expect("editor state");
        assert_eq!(editor.name, "demo-skill");
        assert!(!editor.dirty);
        assert!(!editor.read_only);
    }

    #[test]
    fn save_rejects_an_invalid_name_without_writing() {
        let mut fixture = setup();
        new_skill(&mut fixture.core);
        set_name(&mut fixture.core, "Bad Name");

        let snapshot = save(&mut fixture.core);

        assert_eq!(snapshot.status.kind, "error");
        assert!(
            snapshot.status.message.contains("name 不合法"),
            "{}",
            snapshot.status.message
        );
        assert!(!fixture.workspace.join("Bad Name").exists());
    }

    #[test]
    fn save_never_silently_takes_over_an_existing_directory() {
        let mut fixture = setup();
        write_skill(&fixture.workspace.join("taken"), &valid_skill("taken"));
        let untouched = std::fs::read_to_string(fixture.workspace.join("taken/SKILL.md"))
            .expect("read original");

        new_skill(&mut fixture.core);
        set_name(&mut fixture.core, "taken");
        set_body(&mut fixture.core, "overwritten\n");
        let snapshot = save(&mut fixture.core);

        assert_eq!(snapshot.status.kind, "error");
        assert!(
            snapshot.status.message.contains("已存在"),
            "{}",
            snapshot.status.message
        );
        assert_eq!(
            std::fs::read_to_string(fixture.workspace.join("taken/SKILL.md")).expect("read again"),
            untouched
        );
    }

    #[test]
    fn editing_through_the_app_layer_preserves_unknown_fields() {
        let mut fixture = setup();
        let root = fixture.workspace.join("custom");
        write_skill(
            &root,
            "---\nname: custom\ndescription: 保留未知字段的测试，描述足够长以通过检查。\nx-extra: 7\n---\n\nbody\n",
        );

        let booted = boot(&mut fixture.core);
        let id = booted.skills[0].id.clone();
        open_skill(&mut fixture.core, &id);
        set_description(&mut fixture.core, "更新后的描述，长度依旧足够通过检查。");
        let snapshot = save(&mut fixture.core);
        assert_eq!(snapshot.status.kind, "ok", "{}", snapshot.status.message);

        let text = std::fs::read_to_string(root.join("SKILL.md")).expect("read back");
        assert!(text.contains("x-extra: 7"), "{text}");
        assert!(text.contains("更新后的描述"), "{text}");
    }

    #[test]
    fn broken_frontmatter_can_be_repaired_inside_the_app() {
        let mut fixture = setup();
        let root = fixture.workspace.join("broken");
        write_skill(&root, "---\nname: [unclosed\n---\nbody\n");

        let booted = boot(&mut fixture.core);
        assert!(!booted.skills[0].parse_ok);
        let id = booted.skills[0].id.clone();

        let opened = open_skill(&mut fixture.core, &id);
        let editor = opened.editor.expect("editor state");
        assert!(editor.read_only, "raw mode must be flagged read-only");

        // The user repairs the YAML in the body textarea.
        let ack = set_body(
            &mut fixture.core,
            "---\nname: broken\ndescription: 修复后的描述，长度足够通过检查。\n---\nbody\n",
        );
        assert!(ack.parse_ok, "frontmatter should parse again");
        assert!(!ack.read_only);

        let saved = save(&mut fixture.core);
        assert_eq!(saved.status.kind, "ok", "{}", saved.status.message);
    }

    #[test]
    fn search_filters_the_visible_list() {
        let mut fixture = setup();
        write_skill(&fixture.workspace.join("alpha"), &valid_skill("alpha"));
        write_skill(&fixture.workspace.join("beta"), &valid_skill("beta"));

        boot(&mut fixture.core);
        assert_eq!(set_search(&mut fixture.core, "alp").skills.len(), 1);
        assert_eq!(set_search(&mut fixture.core, "zzz").skills.len(), 0);
        assert_eq!(set_search(&mut fixture.core, "").skills.len(), 2);
    }

    #[test]
    fn delete_requires_explicit_confirmation() {
        let mut fixture = setup();
        write_skill(&fixture.workspace.join("alpha"), &valid_skill("alpha"));
        let booted = boot(&mut fixture.core);
        let id = booted.skills[0].id.clone();

        let refused = delete_skill(&mut fixture.core, &id, false);
        assert_eq!(refused.status.kind, "warning");
        assert!(fixture.workspace.join("alpha").is_dir());

        let deleted = delete_skill(&mut fixture.core, &id, true);
        assert_eq!(deleted.status.kind, "ok");
        assert!(!fixture.workspace.join("alpha").exists());
        assert!(deleted.skills.is_empty());
    }

    #[test]
    fn dirty_flag_tracks_edits_and_clears_on_save() {
        let mut fixture = setup();
        write_skill(&fixture.workspace.join("alpha"), &valid_skill("alpha"));
        let booted = boot(&mut fixture.core);
        let id = booted.skills[0].id.clone();
        open_skill(&mut fixture.core, &id);
        assert!(!is_dirty(&fixture.core));

        let ack = set_body(&mut fixture.core, "changed\n");
        assert!(ack.dirty);
        assert!(is_dirty(&fixture.core));

        save(&mut fixture.core);
        assert!(!is_dirty(&fixture.core));
    }

    #[test]
    fn install_preview_confirm_and_uninstall_round_trip() {
        let mut fixture = setup();
        write_skill(
            &fixture.workspace.join("demo-skill"),
            &valid_skill("demo-skill"),
        );
        let booted = boot(&mut fixture.core);
        let id = booted.skills[0].id.clone();
        open_skill(&mut fixture.core, &id);

        let preview =
            preview_install(&mut fixture.core, "generic", "custom", None).expect("preview");
        assert!(preview.creates_target);
        assert!(preview.added >= 1);
        assert!(preview.entries.iter().any(|e| e.path.ends_with("SKILL.md")));

        let installed = confirm_install(&mut fixture.core);
        assert_eq!(installed.status.kind, "ok", "{}", installed.status.message);
        let target_dir = fixture.targets.join("demo-skill");
        assert!(target_dir.join("SKILL.md").is_file());

        // The target list reports the install, and the plan is consumed.
        let target = installed
            .targets
            .iter()
            .find(|t| t.path == fixture.targets.display().to_string())
            .expect("custom target is listed");
        assert!(target.installed);
        assert!(cancel_install(&mut fixture.core)
            .status
            .message
            .contains("已取消"));

        // Uninstall needs confirmation and then removes only managed files.
        let plan =
            preview_uninstall(&fixture.core, "generic", "custom", None).expect("uninstall plan");
        assert_eq!(plan.managed_files, 1);
        let refused = uninstall(&mut fixture.core, "generic", "custom", None, false);
        assert_eq!(refused.status.kind, "warning");
        assert!(target_dir.join("SKILL.md").is_file());

        let removed = uninstall(&mut fixture.core, "generic", "custom", None, true);
        assert_eq!(removed.status.kind, "ok", "{}", removed.status.message);
        assert!(!target_dir.join("SKILL.md").exists());
    }

    #[test]
    fn install_targets_can_be_selected_by_path() {
        // DeepSeek Harness scans two user roots. Resolving by (platform, scope)
        // alone would always pick the first one, so the front end sends the
        // path of the row the user clicked.
        let fixture = setup();
        let ctx = fixture.core.platform_context();
        let adapter = platforms::adapter_by_id("deepseek-harness").expect("adapter");
        let users: Vec<_> = adapter
            .discover_targets(&ctx)
            .into_iter()
            .filter(|t| t.scope == Scope::User)
            .collect();
        assert_eq!(users.len(), 2, "两条用户级目标必须都保留");

        for target in &users {
            let path = target.path.display().to_string();
            let resolved = resolve_target(&fixture.core, "deepseek-harness", "user", Some(&path))
                .expect("resolve by path");
            assert_eq!(resolved.path, target.path);
            assert_eq!(resolved.variant, target.variant);
        }

        // A path that is not one of the discovered targets is rejected.
        assert!(resolve_target(
            &fixture.core,
            "deepseek-harness",
            "user",
            Some("/definitely/not/a/target")
        )
        .is_none());

        // The UI label distinguishes the two rows.
        let items = target_items(&fixture.core);
        let labels: Vec<&str> = items
            .iter()
            .filter(|i| i.platform_id == "deepseek-harness" && i.scope == "user")
            .map(|i| i.scope_label.as_str())
            .collect();
        assert!(labels.iter().any(|l| l.contains("Harness 专用")));
        assert!(labels.iter().any(|l| l.contains("跨 Agent 共享")));
    }

    #[test]
    fn deepseek_install_projects_when_to_use_and_keeps_the_source() {
        let mut fixture = setup();
        let root = fixture.workspace.join("harness-skill");
        write_skill(
            &root,
            "---\nname: harness-skill\ndescription: 用于测试 DeepSeek Harness 安装适配的 Skill，描述足够长。\nlicense: MIT\nwhen_to_use: 用户需要处理 PDF 时。\n---\n\n# 步骤\n\n1. 做一件事\n",
        );
        // Point the DSH custom target into the temp dir so the test never
        // touches the real ~/.dsh.
        fixture
            .core
            .settings
            .custom_paths
            .insert("deepseek-harness".to_string(), fixture.targets.clone());

        let booted = boot(&mut fixture.core);
        let id = booted.skills[0].id.clone();
        open_skill(&mut fixture.core, &id);

        let target_path = fixture.targets.display().to_string();
        let preview = preview_install(
            &mut fixture.core,
            "deepseek-harness",
            "custom",
            Some(&target_path),
        )
        .expect("preview");

        assert_eq!(preview.rewrites.len(), 1);
        assert_eq!(preview.rewrites[0].from, "when_to_use");
        assert_eq!(preview.rewrites[0].to, "whenToUse");

        // The front end consumes these exact camelCase keys.
        let preview_json = serde_json::to_value(&preview).expect("serialize preview");
        assert_eq!(preview_json["rewrites"][0]["from"], "when_to_use");
        assert_eq!(preview_json["rewrites"][0]["to"], "whenToUse");
        assert!(preview_json["warnings"].is_array());
        assert!(preview_json.get("targetRoot").is_some());
        assert!(
            preview
                .warnings
                .iter()
                .all(|w| !w.message.contains("when_to_use")),
            "a field resolved by the projection must not also be reported as ignored: {:?}",
            preview.warnings
        );
        assert!(
            preview
                .warnings
                .iter()
                .any(|w| w.message.contains("license")),
            "remaining unsupported fields must still be reported: {:?}",
            preview.warnings
        );
        assert!(preview
            .entries
            .iter()
            .any(|e| e.path.ends_with("SKILL.md") && e.detail.contains("平台适配")));
        assert!(
            !fixture.targets.join("harness-skill").exists(),
            "preview must not write"
        );

        let source_before = std::fs::read_to_string(root.join("SKILL.md")).unwrap();
        assert!(source_before.contains("when_to_use:"));

        let installed = confirm_install(&mut fixture.core);
        assert_eq!(installed.status.kind, "ok", "{}", installed.status.message);
        assert!(
            installed.status.message.contains("已适配 1 个字段"),
            "{}",
            installed.status.message
        );

        let target_text =
            std::fs::read_to_string(fixture.targets.join("harness-skill/SKILL.md")).unwrap();
        assert!(target_text.contains("whenToUse: 用户需要处理 PDF 时。"));
        assert!(!target_text.contains("when_to_use:"));
        assert!(
            target_text.contains("license: MIT"),
            "unsupported fields must be preserved in the installed copy"
        );
        assert!(target_text.contains("# 步骤"));
        assert_eq!(
            std::fs::read_to_string(root.join("SKILL.md")).unwrap(),
            source_before,
            "installation must never modify the workspace source"
        );

        let manifest = fixture
            .core
            .manifests
            .load(&fixture.targets.join("harness-skill"))
            .expect("manifest");
        assert_eq!(manifest.rewrites.len(), 1);
        assert_eq!(manifest.rewrites[0].from, "when_to_use");
        assert_eq!(manifest.rewrites[0].to, "whenToUse");

        // Reinstalling the same projection is idempotent: the target already
        // has the adapted bytes, so SKILL.md is unchanged.
        let again = preview_install(
            &mut fixture.core,
            "deepseek-harness",
            "custom",
            Some(&target_path),
        )
        .expect("second preview");
        assert_eq!(again.modified, 0);
        assert!(again.unchanged >= 1);
        cancel_install(&mut fixture.core);
    }

    #[test]
    fn install_preview_refuses_unsaved_edits() {
        let mut fixture = setup();
        write_skill(
            &fixture.workspace.join("demo-skill"),
            &valid_skill("demo-skill"),
        );
        let booted = boot(&mut fixture.core);
        let id = booted.skills[0].id.clone();
        open_skill(&mut fixture.core, &id);
        set_body(&mut fixture.core, "changed but not saved\n");

        let err = preview_install(&mut fixture.core, "generic", "custom", None)
            .expect_err("unsaved edits must not be installed silently");
        assert!(err.contains("未保存"), "{err}");
        assert!(!fixture.targets.join("demo-skill").exists());
    }

    #[test]
    fn platform_cards_report_compatibility_for_the_open_skill() {
        let mut fixture = setup();
        write_skill(
            &fixture.workspace.join("harness-skill"),
            "---\nname: harness-skill\ndescription: 用于测试兼容性徽标的 Skill，描述足够长。\nwhen_to_use: 处理 PDF 时。\n---\n\n# 步骤\n\n1. 做一件事\n",
        );

        let booted = boot(&mut fixture.core);
        let dsh = booted
            .platforms
            .iter()
            .find(|p| p.id == "deepseek-harness")
            .expect("dsh platform");
        assert_eq!(dsh.compat_status, "unknown");

        let id = booted.skills[0].id.clone();
        let opened = open_skill(&mut fixture.core, &id);
        for id in ["deepseek-harness", "deepseek-harness-desktop"] {
            let platform = opened
                .platforms
                .iter()
                .find(|p| p.id == id)
                .expect("platform card");
            assert_eq!(platform.compat_status, "warning", "{id}");
            assert!(platform.compat_warnings >= 1, "{id}");
            assert_eq!(platform.compat_rewrites, 1, "{id}");
        }
    }

    #[test]
    fn external_change_detection_follows_the_setting() {
        let mut fixture = setup();
        write_skill(
            &fixture.workspace.join("demo-skill"),
            &valid_skill("demo-skill"),
        );
        let booted = boot(&mut fixture.core);
        let id = booted.skills[0].id.clone();
        open_skill(&mut fixture.core, &id);

        preview_install(&mut fixture.core, "generic", "custom", None).expect("preview");
        confirm_install(&mut fixture.core);

        // A third-party edit inside the target.
        let target_skill_md = fixture.targets.join("demo-skill/SKILL.md");
        std::fs::write(&target_skill_md, "tampered\n").expect("tamper");

        // Detection is on by default.
        let detected =
            preview_install(&mut fixture.core, "generic", "custom", None).expect("preview");
        assert!(
            detected.has_external_modification,
            "an external edit must be reported by default"
        );

        // Turning the setting off skips the comparison.
        update_settings(
            &mut fixture.core,
            &SettingsPatch {
                detect_external_changes: Some(false),
                ..SettingsPatch::default()
            },
        );
        let ignored =
            preview_install(&mut fixture.core, "generic", "custom", None).expect("preview");
        assert!(
            !ignored.has_external_modification,
            "the setting must reach the install plan"
        );
        // The file is still reported as modified and the target is still backed up.
        assert!(ignored.modified >= 1);
        assert!(ignored.will_backup);
    }

    #[test]
    fn uninstall_without_a_manifest_is_refused() {
        let mut fixture = setup();
        write_skill(
            &fixture.workspace.join("demo-skill"),
            &valid_skill("demo-skill"),
        );
        // A foreign file that we did not install.
        write_skill(
            &fixture.targets.join("demo-skill"),
            &valid_skill("demo-skill"),
        );
        let booted = boot(&mut fixture.core);
        let id = booted.skills[0].id.clone();
        open_skill(&mut fixture.core, &id);

        let err =
            preview_uninstall(&fixture.core, "generic", "custom", None).expect_err("must fail");
        assert!(err.contains("未找到安装清单"), "{err}");
        assert!(fixture.targets.join("demo-skill/SKILL.md").is_file());
    }

    #[test]
    fn settings_patch_clamps_and_persists() {
        let mut fixture = setup();
        let patched = update_settings(
            &mut fixture.core,
            &SettingsPatch {
                theme: Some("dark".to_string()),
                backup_count: Some(999),
                detect_external_changes: Some(false),
                ..SettingsPatch::default()
            },
        );
        assert_eq!(patched.settings.theme, "dark");
        assert_eq!(patched.settings.backup_count, 50);
        assert!(!patched.settings.detect_external_changes);

        let saved = save_settings(&mut fixture.core);
        assert_eq!(saved.status.kind, "ok", "{}", saved.status.message);

        let reloaded = Core::from_roots(&fixture.config_root, fixture.config_root.join("data"));
        assert_eq!(reloaded.settings.theme, Theme::Dark);
        assert_eq!(reloaded.settings.backup_count, 50);
    }

    #[test]
    fn save_as_copies_resources_and_refuses_to_overwrite() {
        let mut fixture = setup();
        let root = fixture.workspace.join("source-skill");
        write_skill(&root, &valid_skill("source-skill"));
        std::fs::create_dir_all(root.join("references")).expect("references dir");
        std::fs::write(root.join("references/guide.md"), "reference body").expect("write resource");

        let booted = boot(&mut fixture.core);
        let id = booted.skills[0].id.clone();
        open_skill(&mut fixture.core, &id);

        let destination = fixture._dir.path().join("elsewhere");
        std::fs::create_dir_all(&destination).expect("destination");
        let copied = save_as(&mut fixture.core, &destination);
        assert_eq!(copied.status.kind, "ok", "{}", copied.status.message);
        assert!(destination.join("source-skill/SKILL.md").is_file());
        assert_eq!(
            std::fs::read_to_string(destination.join("source-skill/references/guide.md"))
                .expect("copied resource"),
            "reference body"
        );

        // Saving again onto the same destination asks the user to pick another.
        let again = save_as(&mut fixture.core, &destination);
        assert_eq!(again.status.kind, "error");
        assert!(again.status.message.contains("已存在 SKILL.md"));
    }

    #[test]
    fn import_directory_and_zip_land_in_the_workspace() {
        let mut fixture = setup();
        let external = fixture._dir.path().join("external/imported-skill");
        write_skill(&external, &valid_skill("imported-skill"));

        let imported = import_from(&mut fixture.core, &external);
        assert_eq!(imported.status.kind, "ok", "{}", imported.status.message);
        assert!(fixture.workspace.join("imported-skill/SKILL.md").is_file());
        assert_eq!(imported.skills.len(), 1);
        assert!(imported.editor.is_some());

        // Export then re-import through ZIP.
        let zip_path = fixture._dir.path().join("export.zip");
        let source_root = fixture.workspace.join("imported-skill");
        services::export_zip(&source_root, &zip_path).expect("export");
        let zipped = import_from(&mut fixture.core, &zip_path);
        assert_eq!(zipped.status.kind, "ok", "{}", zipped.status.message);
        // A second copy is created rather than overwriting the first.
        assert_eq!(zipped.skills.len(), 2);
    }

    #[test]
    fn unknown_import_targets_are_reported() {
        let mut fixture = setup();
        let text = fixture._dir.path().join("notes.txt");
        std::fs::write(&text, "hello").expect("write");
        let snapshot = import_from(&mut fixture.core, &text);
        assert_eq!(snapshot.status.kind, "error");
        assert!(snapshot.status.message.contains("请选择 Skill 目录"));
    }

    #[test]
    fn preview_text_renders_skill_md_and_reports_missing_documents() {
        let mut fixture = setup();
        assert!(preview_text(&fixture.core).is_err());

        new_skill(&mut fixture.core);
        set_name(&mut fixture.core, "preview-demo");
        let text = preview_text(&fixture.core).expect("preview");
        assert!(text.starts_with("---\n"));
        assert!(text.contains("name: preview-demo"));
    }

    #[test]
    fn export_requires_a_saved_skill() {
        let mut fixture = setup();
        new_skill(&mut fixture.core);
        set_name(&mut fixture.core, "unsaved");
        let file = fixture._dir.path().join("out.zip");
        let snapshot = export_to(&mut fixture.core, &file);
        assert_eq!(snapshot.status.kind, "error");
        assert!(!file.exists());
    }

    #[test]
    fn platform_items_expose_capabilities_and_verification() {
        let mut fixture = setup();
        let snapshot = boot(&mut fixture.core);
        let codex = snapshot
            .platforms
            .iter()
            .find(|p| p.id == "codex")
            .expect("codex adapter");
        assert_eq!(codex.verification, "warning");
        assert_eq!(codex.verification_label, "兼容但有警告");
        assert!(!codex.docs_url.is_empty());
        assert!(!codex.verified_on.is_empty());
        assert!(codex.supported_fields.contains(&"name".to_string()));

        let generic = snapshot
            .platforms
            .iter()
            .find(|p| p.id == "generic")
            .expect("generic adapter");
        assert_eq!(generic.listing_limit, 1024);
        assert!(generic.detected, "the fixture configures a custom target");
        assert_eq!(
            generic.resource_dirs,
            vec!["scripts", "references", "assets"]
        );
        assert!(snapshot
            .targets
            .iter()
            .any(|t| t.platform_id == "generic" && t.scope == "custom"));
    }

    #[test]
    fn frontmatter_guide_is_read_only_and_platform_aware() {
        let mut fixture = setup();
        let root = fixture.workspace.join("alpha");
        write_skill(&root, &valid_skill("alpha"));
        let snapshot = boot(&mut fixture.core);
        open_skill(&mut fixture.core, &snapshot.skills[0].id);
        assert!(!fixture.core.open.as_ref().expect("open").state.dirty);
        frontmatter_guide(&fixture.core);
        assert!(
            !fixture.core.open.as_ref().expect("open").state.dirty,
            "读取指南不应把干净文档标脏"
        );

        set_description(
            &mut fixture.core,
            "阅读指南时不应丢失的未保存描述，长度足够并通过质量检查。",
        );
        assert!(fixture.core.open.as_ref().expect("open").state.dirty);
        let source_before = std::fs::read_to_string(root.join("SKILL.md")).expect("read source");

        let guide = frontmatter_guide(&fixture.core);
        assert_eq!(guide.limits.name_max, 64);
        assert_eq!(guide.limits.description_max, 1024);
        assert_eq!(guide.limits.compatibility_max, 500);
        assert_eq!(guide.limits.body_max_lines, 500);
        assert_eq!(guide.limits.body_max_tokens, 5000);
        assert!(!guide.example.is_empty());
        assert!(guide.fields.iter().any(|f| f.key == "name" && f.required));
        assert!(guide
            .fields
            .iter()
            .any(|f| f.key == "description" && f.category == "required"));

        let when_to_use = guide
            .fields
            .iter()
            .find(|f| f.key == "when_to_use")
            .expect("when_to_use");
        assert!(when_to_use
            .platform_ids
            .contains(&"claude-code".to_string()));
        let when_to_use_camel = guide
            .fields
            .iter()
            .find(|f| f.key == "whenToUse")
            .expect("whenToUse");
        assert!(when_to_use_camel
            .platform_ids
            .contains(&"deepseek-harness".to_string()));

        assert!(guide.rules.iter().any(|r| r.id == "SPEC-DESC-002"));
        assert!(guide
            .platforms
            .iter()
            .any(|p| p.id == "generic" && p.listing_limit == 1024));
        assert!(
            fixture.core.open.as_ref().expect("open").state.dirty,
            "读取指南不应清除或改变未保存状态"
        );
        let source_after = std::fs::read_to_string(root.join("SKILL.md")).expect("read source");
        assert_eq!(source_before, source_after, "读取指南不应写文件");

        let json = serde_json::to_string(&guide).expect("serialize guide");
        for key in [
            "\"platformIds\"",
            "\"listingLimit\"",
            "\"bodyMaxLines\"",
            "\"commonMistake\"",
        ] {
            assert!(json.contains(key), "缺少 camelCase 字段 {key}: {json}");
        }
    }

    #[test]
    fn every_snapshot_is_json_serializable() {
        let mut fixture = setup();
        write_skill(&fixture.workspace.join("alpha"), &valid_skill("alpha"));
        let snapshot = boot(&mut fixture.core);
        let json = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(json.contains("\"appVersion\""));
        assert!(json.contains("\"skills\""));
        // Fields consumed by the platform cards.
        assert!(json.contains("\"compatStatus\""));
        assert!(json.contains("\"compatWarnings\""));
        assert!(json.contains("\"compatRewrites\""));
    }

    #[test]
    fn reveal_reports_unknown_ids() {
        let fixture = setup();
        assert!(skill_root(&fixture.core, "missing").is_err());
    }
}
