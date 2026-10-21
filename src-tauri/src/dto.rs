//! Serializable view data handed to the front end.
//!
//! These types are a *view* of the core model, never the model itself: the core
//! keeps `PathBuf`, enums and typed errors, while the front end receives plain
//! JSON-friendly values (camelCase strings and numbers).
//!
//! The whole screen is described by one [`Snapshot`]. Every non-text command
//! returns a fresh snapshot, so the front end never has to reconcile partial
//! updates. Text edits are the exception: they return an [`EditAck`] so the
//! textarea is not re-rendered (and the caret does not jump) on every keystroke.

use serde::{Deserialize, Serialize};

/// Severity/status colour token used by both the status bar and issue rows.
pub const KIND_INFO: &str = "info";
pub const KIND_OK: &str = "ok";
pub const KIND_WARNING: &str = "warning";
pub const KIND_ERROR: &str = "error";

/// A one-line status message shown in the bottom bar.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub kind: String,
    pub message: String,
}

impl Status {
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        Status {
            kind: kind.to_string(),
            message: message.into(),
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self::new(KIND_INFO, message)
    }

    pub fn ok(message: impl Into<String>) -> Self {
        Self::new(KIND_OK, message)
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(KIND_WARNING, message)
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::new(KIND_ERROR, message)
    }
}

/// One row in the skills list.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub path: String,
    pub modified: String,
    /// `""`, `installed`, `outdated` or `attention`.
    pub status: String,
    pub dirty: bool,
    /// False when the frontmatter could not be parsed (raw-text fallback).
    pub parse_ok: bool,
}

/// A bundled resource file next to `SKILL.md`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceItem {
    pub path: String,
    pub kind: String,
    pub size: String,
}

/// Structural statistics for the body, used by the editor side panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownStats {
    pub lines: usize,
    pub characters: usize,
    pub headings: usize,
    pub list_items: usize,
    pub code_fences: usize,
    pub links: usize,
    pub approx_tokens: usize,
}

impl Serialize for MarkdownStats {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("MarkdownStats", 7)?;
        s.serialize_field("lines", &self.lines)?;
        s.serialize_field("characters", &self.characters)?;
        s.serialize_field("headings", &self.headings)?;
        s.serialize_field("listItems", &self.list_items)?;
        s.serialize_field("codeFences", &self.code_fences)?;
        s.serialize_field("links", &self.links)?;
        s.serialize_field("approxTokens", &self.approx_tokens)?;
        s.end()
    }
}

/// A single validation finding.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IssueItem {
    pub rule_id: String,
    /// `error` | `warning` | `info`.
    pub severity: String,
    /// `spec` | `quality` | `platform`.
    pub source: String,
    pub source_label: String,
    pub file: String,
    /// Empty when the rule has no line.
    pub line: String,
    pub message: String,
    pub suggestion: String,
    /// Empty when the rule has no documentation URL.
    pub doc_url: String,
}

/// Issue counters shown in the status bar.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
}

/// The open document.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditorState {
    pub id: String,
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: String,
    pub dirty: bool,
    /// True when the frontmatter failed to parse: editing is limited to raw text.
    pub read_only: bool,
    pub parse_ok: bool,
    pub frontmatter_present: bool,
    /// Top-level frontmatter keys, in document order (unknown ones included).
    pub frontmatter_keys: Vec<String>,
    pub resources: Vec<ResourceItem>,
    pub stats: MarkdownStats,
}

/// What a text edit changed (no snapshot, so the editor keeps its caret).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditAck {
    pub dirty: bool,
    pub parse_ok: bool,
    pub read_only: bool,
    pub counts: Counts,
    pub issues: Vec<IssueItem>,
    pub stats: MarkdownStats,
    pub frontmatter_keys: Vec<String>,
}

/// A platform adapter and what it declares.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlatformItem {
    pub id: String,
    pub name: String,
    /// `verified` | `warning` | `unverified`.
    pub verification: String,
    pub verification_label: String,
    pub docs_url: String,
    pub verified_on: String,
    pub detected: bool,
    pub detect_detail: String,
    pub supported_fields: Vec<String>,
    /// Combined listing length limit, 0 when unknown.
    pub listing_limit: usize,
    pub supports_resources: bool,
    pub resource_dirs: Vec<String>,
    pub notes: String,
    /// `ok` | `warning` | `unknown` (no skill open).
    pub compat_status: String,
    /// Number of platform compatibility warnings for the open skill.
    pub compat_warnings: usize,
    /// Number of fields the installer will adapt for this platform.
    pub compat_rewrites: usize,
}

/// Static limits shown in the read-only frontmatter guide.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GuideLimits {
    pub name_max: usize,
    pub description_max: usize,
    pub compatibility_max: usize,
    pub body_max_lines: usize,
    pub body_max_tokens: usize,
}

/// One field in the frontmatter guide, with platform support resolved by the
/// app layer from the live adapters.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GuideField {
    pub key: String,
    pub label: String,
    pub value_type: String,
    pub required: bool,
    /// `required` | `spec-optional` | `platform`.
    pub category: String,
    pub summary: String,
    pub rules: Vec<String>,
    pub example: String,
    pub common_mistake: String,
    pub doc_url: String,
    pub aliases: Vec<String>,
    pub platform_ids: Vec<String>,
}

/// One validation rule in the guide.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GuideRule {
    pub id: String,
    /// `error` | `warning` | `info`.
    pub severity: String,
    /// `spec` | `quality` | `platform`.
    pub source: String,
    pub source_label: String,
    pub summary: String,
    pub fix: String,
    pub doc_url: String,
}

/// A platform row in the guide (support and listing limits).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GuidePlatform {
    pub id: String,
    pub name: String,
    /// Combined listing limit, 0 when unknown.
    pub listing_limit: usize,
    pub notes: String,
    pub docs_url: String,
}

/// Read-only content for the "Frontmatter 规则" modal.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FrontmatterGuide {
    pub fields: Vec<GuideField>,
    pub rules: Vec<GuideRule>,
    pub platforms: Vec<GuidePlatform>,
    pub directory_rules: Vec<String>,
    pub body_rules: Vec<String>,
    pub example: String,
    pub limits: GuideLimits,
}

/// A discoverable install directory.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetItem {
    pub platform_id: String,
    pub scope: String,
    pub scope_label: String,
    pub label: String,
    pub path: String,
    pub exists: bool,
    /// True when the currently open skill is installed here.
    pub installed: bool,
}

/// One file in the install diff.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffItem {
    pub path: String,
    /// `added` | `modified` | `unchanged` | `removed`.
    pub change: String,
    pub detail: String,
}

/// One field rename the installer will apply to its copy.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RewriteItem {
    pub from: String,
    pub to: String,
    pub reason: String,
}

/// The install confirmation dialog contents.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallPreview {
    pub title: String,
    pub target_root: String,
    pub platform_label: String,
    pub added: usize,
    pub modified: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub creates_target: bool,
    /// Target files changed outside the app since we installed them.
    pub has_external_modification: bool,
    pub will_backup: bool,
    /// Compatibility warnings that remain after projection (what will actually
    /// be installed).
    pub warnings: Vec<IssueItem>,
    /// Field renames applied to the installed copy (source stays untouched).
    pub rewrites: Vec<RewriteItem>,
    pub entries: Vec<DiffItem>,
}

/// The uninstall confirmation dialog contents.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UninstallPreview {
    pub title: String,
    pub target_root: String,
    pub platform_id: String,
    pub scope: String,
    pub scope_label: String,
    pub managed_files: usize,
    /// True when a managed file was edited outside the app.
    pub has_external_changes: bool,
}

/// Editable settings.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub workspace: String,
    /// `system` | `light` | `dark`.
    pub theme: String,
    pub backup_count: usize,
    pub detect_external_changes: bool,
    pub config_path: String,
    pub log_dir: String,
    pub backup_dir: String,
    pub manifest_dir: String,
}

/// A settings update from the front end. Every field is optional so the UI can
/// send only what changed.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub workspace: Option<String>,
    pub theme: Option<String>,
    pub backup_count: Option<usize>,
    pub detect_external_changes: Option<bool>,
}

/// Everything the front end needs to render the current screen.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub app_version: String,
    pub workspace: String,
    pub project_dir: String,
    pub status: Status,
    pub theme: String,
    pub search: String,
    pub skills: Vec<SkillItem>,
    pub editor: Option<EditorState>,
    pub issues: Vec<IssueItem>,
    pub counts: Counts,
    pub platforms: Vec<PlatformItem>,
    pub targets: Vec<TargetItem>,
    pub settings: SettingsDto,
}

// NOTE: `Snapshot::page` is deliberately absent. Which page is visible is pure
// view state and lives in the front end; the back end only answers "what does
// the skills list look like" and "what is open".

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_serialize_with_camel_case_keys() {
        let stats = MarkdownStats {
            lines: 3,
            characters: 40,
            headings: 1,
            list_items: 2,
            code_fences: 1,
            links: 0,
            approx_tokens: 10,
        };
        let json = serde_json::to_string(&stats).expect("serialize");
        assert!(json.contains("\"listItems\":2"), "{json}");
        assert!(json.contains("\"approxTokens\":10"), "{json}");
    }

    #[test]
    fn settings_patch_ignores_unknown_fields() {
        let patch: SettingsPatch =
            serde_json::from_str("{\"theme\":\"dark\",\"nope\":1}").expect("parse");
        assert_eq!(patch.theme.as_deref(), Some("dark"));
        assert_eq!(patch.backup_count, None);
    }

    #[test]
    fn status_helpers_set_the_kind() {
        assert_eq!(Status::error("x").kind, KIND_ERROR);
        assert_eq!(Status::ok("x").kind, KIND_OK);
    }
}
