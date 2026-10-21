//! Shared helpers for platform adapters.

use std::path::{Path, PathBuf};

use crate::domain::skill::SkillDocument;
use crate::domain::validation::{IssueSource, Severity, ValidationIssue};
use crate::platforms::{InstallTarget, PlatformCapabilities, PlatformContext, Scope};

/// Fields defined by the Agent Skills open specification.
pub const SPEC_FIELDS: &[&str] = &[
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
];

/// Build a target for a directory, computing its existence.
pub fn target(platform_id: &str, scope: Scope, label_prefix: &str, path: PathBuf) -> InstallTarget {
    target_with_variant(platform_id, scope, label_prefix, path, None)
}

/// Like [`target`], but tags a target that shares its scope with another one
/// (for platforms that scan more than one directory per scope).
pub fn target_with_variant(
    platform_id: &str,
    scope: Scope,
    label_prefix: &str,
    path: PathBuf,
    variant: Option<&'static str>,
) -> InstallTarget {
    let exists = path.is_dir();
    InstallTarget {
        platform_id: platform_id.to_string(),
        scope,
        label: format!("{label_prefix} · {}", scope.label_zh()),
        path,
        exists,
        variant,
    }
}

/// Project-scoped target, if a project dir is known.
pub fn project_target(
    ctx: &PlatformContext,
    platform_id: &str,
    label_prefix: &str,
    rel: &[&str],
) -> Option<InstallTarget> {
    project_target_with_variant(ctx, platform_id, label_prefix, rel, None)
}

pub fn project_target_with_variant(
    ctx: &PlatformContext,
    platform_id: &str,
    label_prefix: &str,
    rel: &[&str],
    variant: Option<&'static str>,
) -> Option<InstallTarget> {
    let project = ctx.project_dir.as_ref()?;
    let mut path = project.clone();
    for part in rel {
        path.push(part);
    }
    Some(target_with_variant(
        platform_id,
        Scope::Project,
        label_prefix,
        path,
        variant,
    ))
}

/// User-scoped target under the home directory.
pub fn user_target(
    ctx: &PlatformContext,
    platform_id: &str,
    label_prefix: &str,
    rel: &[&str],
) -> Option<InstallTarget> {
    user_target_with_variant(ctx, platform_id, label_prefix, rel, None)
}

pub fn user_target_with_variant(
    ctx: &PlatformContext,
    platform_id: &str,
    label_prefix: &str,
    rel: &[&str],
    variant: Option<&'static str>,
) -> Option<InstallTarget> {
    let home = ctx.home.as_ref()?;
    let mut path = home.clone();
    for part in rel {
        path.push(part);
    }
    Some(target_with_variant(
        platform_id,
        Scope::User,
        label_prefix,
        path,
        variant,
    ))
}

/// Custom override target, if the user configured one.
pub fn custom_target(
    ctx: &PlatformContext,
    platform_id: &str,
    label_prefix: &str,
) -> Option<InstallTarget> {
    let path = ctx.custom(platform_id)?.clone();
    Some(target(platform_id, Scope::Custom, label_prefix, path))
}

/// Check the frontmatter against the platform's supported fields and listing
/// limit. Returns warnings only (never hard spec errors).
pub fn field_compat_issues(
    doc: &SkillDocument,
    caps: &PlatformCapabilities,
    platform_id: &str,
    docs_url: &str,
) -> Vec<ValidationIssue> {
    if !doc.state.parse_ok {
        return Vec::new();
    }
    let mut issues = Vec::new();
    for key in doc.frontmatter.keys() {
        if !caps.supports_field(&key) {
            // A few platforms have a documented equivalent key. The installer
            // projects those fields, so the warning must describe the actual
            // result instead of claiming the field will simply be ignored.
            // Invariant: an adapter that declares `whenToUse` must implement
            // `project` for the snake_case alias (both DSH adapters do).
            let camel_supported = caps.supports_field("whenToUse");
            let both_aliases = key == "when_to_use"
                && camel_supported
                && doc.frontmatter.contains_key("whenToUse");
            let projectable = key == "when_to_use" && camel_supported && !both_aliases;
            let message = if projectable {
                format!(
                    "字段 `when_to_use` 不是 {} 识别的字段（该平台读取 `whenToUse`）；安装时会自动尝试改名，预览会显示实际结果（只改安装副本，源文件不变）。",
                    caps_list_name(platform_id)
                )
            } else if both_aliases {
                format!(
                    "字段 `when_to_use` 不是 {} 识别的字段；源中已有 `whenToUse`，安装时不会自动合并，`when_to_use` 会被忽略。",
                    caps_list_name(platform_id)
                )
            } else {
                format!(
                    "字段 `{key}` 不是 {} 识别的字段，安装后该字段会被忽略（不会影响安装）。",
                    caps_list_name(platform_id)
                )
            };
            let suggestion = if projectable {
                "安装时会自动适配；也可以在工作区直接改用 `whenToUse`，让源文件更明确。".to_string()
            } else if both_aliases {
                "保留 `whenToUse` 即可；如需保留说明，可把 `when_to_use` 的内容移入正文或 `metadata`。".to_string()
            } else {
                format!("如需跨平台通用，请把 `{key}` 移入 `metadata`，或从 frontmatter 中移除。")
            };
            issues.push(
                ValidationIssue::new(
                    format!("PLAT-{}-FIELD", platform_id.to_uppercase()),
                    Severity::Warning,
                    IssueSource::Platform(platform_id.to_string()),
                    "SKILL.md",
                    message,
                    suggestion,
                )
                .with_doc(docs_url),
            );
        }
    }

    if caps.listing_limit > 0 {
        // Count only the value the platform actually reads. DSH reads the
        // camelCase key; when it is absent, the installer projects the
        // snake_case value into it. Other platforms either read snake_case or
        // ignore both keys.
        let when_value = if caps.supports_field("whenToUse") {
            doc.frontmatter
                .get_str_lossy("whenToUse")
                .or_else(|| doc.frontmatter.get_str_lossy("when_to_use"))
        } else if caps.supports_field("when_to_use") {
            doc.frontmatter.get_str_lossy("when_to_use")
        } else {
            None
        };
        let combined =
            doc.description().chars().count() + when_value.map(|s| s.chars().count()).unwrap_or(0);
        if combined > caps.listing_limit {
            issues.push(
                ValidationIssue::new(
                    format!("PLAT-{}-DESC", platform_id.to_uppercase()),
                    Severity::Warning,
                    IssueSource::Platform(platform_id.to_string()),
                    "SKILL.md",
                    format!(
                        "description（含 when_to_use / whenToUse）共 {combined} 字符，超过 {} 的 {}-字符列表上限，超出部分在识别时会被截断。",
                        caps_list_name(platform_id),
                        caps.listing_limit
                    ),
                    "精简 description，把细节放到正文或 references/。",
                )
                .with_doc(docs_url),
            );
        }
    }

    issues
}

fn caps_list_name(platform_id: &str) -> &str {
    match platform_id {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        "deepseek-harness" => "DeepSeek Harness",
        "deepseek-harness-desktop" => "DeepSeek Harness Desktop",
        _ => "目标平台",
    }
}

/// Convenience: does `dir` exist as a directory?
pub fn dir_exists(dir: &Path) -> bool {
    dir.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::skill::SkillDocument;
    use crate::platforms::PlatformCapabilities;
    use std::path::PathBuf;

    fn has_desc_issue(content: &str, fields: &[&'static str], limit: usize) -> bool {
        let doc = SkillDocument::from_skill_md(PathBuf::from("demo"), content);
        let caps = PlatformCapabilities {
            supported_fields: fields.to_vec(),
            listing_limit: limit,
            supports_resources: false,
            resource_dirs: Vec::new(),
            notes: "",
        };
        field_compat_issues(&doc, &caps, "demo", "https://example.invalid")
            .iter()
            .any(|issue| issue.rule_id == "PLAT-DEMO-DESC")
    }

    #[test]
    fn listing_limit_counts_only_the_alias_the_platform_reads() {
        let snake_only =
            "---\nname: demo\ndescription: short\nwhen_to_use: 1234567890\n---\nbody\n";
        // Ignored field: it must not consume the description budget.
        assert!(!has_desc_issue(snake_only, &["name", "description"], 12));
        // Claude Code reads `when_to_use`.
        assert!(has_desc_issue(
            snake_only,
            &["name", "description", "when_to_use"],
            12
        ));
        // DSH reads the projected camelCase value.
        assert!(has_desc_issue(
            snake_only,
            &["name", "description", "whenToUse"],
            12
        ));

        // When both exist, the projection is a no-op and DSH reads only the
        // camelCase value; the ignored snake_case value must not be counted.
        let both = "---\nname: demo\ndescription: short\nwhen_to_use: 1234567890\nwhenToUse: 123\n---\nbody\n";
        assert!(!has_desc_issue(
            both,
            &["name", "description", "whenToUse"],
            12
        ));
    }
}
