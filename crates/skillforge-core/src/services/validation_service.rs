//! Validation engine.
//!
//! Rules are versioned and separated from the UI. Each rule has a stable id, a
//! severity, an origin (spec / platform / quality) and a documentation URL.
//! Recommended sections are only ever *quality* suggestions, never spec
//! errors.

use std::path::Path;

use crate::domain::naming;
use crate::domain::resource::SkillResource;
use crate::domain::skill::SkillDocument;
use crate::domain::validation::{IssueSource, Severity, ValidationIssue, ValidationReport};
use crate::formats::frontmatter::FrontmatterError;
use crate::formats::markdown;
use crate::formats::skill_md::{self, SkillMdError};
use crate::platforms::{PlatformAdapter, PlatformContext};

const SPEC_URL: &str = "https://agentskills.io/specification";
const CLAUDE_URL: &str = "https://code.claude.com/docs/en/skills";
const DSH_URL: &str =
    "https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/skills.md";

/// Maximum description length from the Agent Skills spec.
pub const MAX_DESCRIPTION_LEN: usize = 1024;
/// Maximum compatibility length from the Agent Skills spec.
pub const MAX_COMPATIBILITY_LEN: usize = 500;
/// Recommended maximum body length (lines) for progressive disclosure.
pub const RECOMMENDED_MAX_BODY_LINES: usize = 500;
/// Recommended maximum body size (approx tokens).
pub const RECOMMENDED_MAX_BODY_TOKENS: usize = 5000;

/// Validate a document (generic spec + quality). Platform rules are added by
/// `validate_with_context`.
pub fn validate(doc: &SkillDocument) -> ValidationReport {
    let mut report = ValidationReport::new();

    // --- Frontmatter parse health -------------------------------------
    if !doc.state.parse_ok {
        report.push(parse_error_issue(doc));
        return report;
    }

    if !doc.state.frontmatter_present {
        report.push(issue(
            "SPEC-FM-002",
            Severity::Error,
            IssueSource::Spec,
            "SKILL.md",
            None,
            "缺少 YAML frontmatter。Agent Skills 要求 SKILL.md 以 `---` 分隔的 frontmatter 开头。",
            "在文件开头添加 `---`、`name` 与 `description`，并以 `---` 结束。",
        ));
    }

    // --- name ----------------------------------------------------------
    let name = doc.name();
    if name.trim().is_empty() {
        report.push(issue(
            "SPEC-NAME-001",
            Severity::Error,
            IssueSource::Spec,
            "SKILL.md",
            None,
            "缺少必需字段 `name`。",
            "添加 `name` 字段，例如 `name: my-skill`。",
        ));
    } else if let Err(err) = naming::validate_skill_name(&name) {
        report.push(issue(
            "SPEC-NAME-002",
            Severity::Error,
            IssueSource::Spec,
            "SKILL.md",
            None,
            format!("`name` 不符合命名规范: {err}"),
            "使用小写字母、数字和单个连字符，长度 1-64，且不以连字符开头/结尾。",
        ));
    } else if let Some(dir) = doc.dir_name() {
        if !dir.is_empty() && dir != name {
            report.push(issue(
                "SPEC-NAME-003",
                Severity::Error,
                IssueSource::Spec,
                "SKILL.md",
                None,
                format!("`name` ({name}) 与所在目录名 ({dir}) 不一致。"),
                "让 `name` 与目录名保持一致，或重命名目录。",
            ));
        }
    }

    // --- description ---------------------------------------------------
    let description = doc.description();
    if description.trim().is_empty() {
        report.push(issue(
            "SPEC-DESC-001",
            Severity::Error,
            IssueSource::Spec,
            "SKILL.md",
            None,
            "缺少必需字段 `description`。",
            "添加 `description`，说明 Skill 做什么以及何时使用。",
        ));
    } else {
        let len = description.chars().count();
        if len > MAX_DESCRIPTION_LEN {
            report.push(issue(
                "SPEC-DESC-002",
                Severity::Error,
                IssueSource::Spec,
                "SKILL.md",
                None,
                format!("`description` 长度 {len} 超过上限 {MAX_DESCRIPTION_LEN}。"),
                "精简 description，把细节放入正文或 references/。",
            ));
        }
        if len < 30 {
            report.push(issue(
                "QUAL-DESC-001",
                Severity::Info,
                IssueSource::Quality,
                "SKILL.md",
                None,
                "description 比较简短，Agent 可能不易判断何时使用。",
                "建议说明「做什么」和「何时使用」，并包含关键触发词。",
            ));
        }
        let lower = description.to_lowercase();
        let says_when = lower.contains("use when")
            || lower.contains("when ")
            || description.contains("何时")
            || description.contains("使用")
            || description.contains("场景");
        if !says_when {
            report.push(issue(
                "QUAL-DESC-002",
                Severity::Info,
                IssueSource::Quality,
                "SKILL.md",
                None,
                "description 未明确说明「何时使用」。",
                "补充触发场景，例如「当用户需要…时使用」。",
            ));
        }
    }

    // --- compatibility --------------------------------
    if let Some(compat) = doc.frontmatter.get_str_lossy("compatibility") {
        let len = compat.chars().count();
        if len > MAX_COMPATIBILITY_LEN {
            report.push(issue(
                "SPEC-COMP-001",
                Severity::Error,
                IssueSource::Spec,
                "SKILL.md",
                None,
                format!("`compatibility` 长度 {len} 超过上限 {MAX_COMPATIBILITY_LEN}。"),
                "精简 compatibility，只保留必要的环境要求。",
            ));
        }
    }

    // --- metadata must be a mapping ------------------------------------
    if let Some(value) = doc.frontmatter.get("metadata") {
        if !matches!(
            value,
            serde_yaml_ng::Value::Mapping(_) | serde_yaml_ng::Value::Null
        ) {
            report.push(issue(
                "SPEC-META-001",
                Severity::Warning,
                IssueSource::Spec,
                "SKILL.md",
                None,
                "`metadata` 应为键值映射（string -> string）。",
                "将 `metadata` 改为 YAML 映射，例如 `metadata:\\n  author: me`。",
            ));
        }
    }

    // --- resource path safety ------------------------------------------
    report.extend(validate_resource_paths(&doc.resources));

    // --- body quality ---------------------------------------------------
    report.extend(validate_body(&doc.instructions));

    report
}

/// Validate every resource path for traversal/absolute-path problems.
pub fn validate_resource_paths(resources: &[SkillResource]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    for res in resources {
        if let Err(err) = crate::domain::path_safety::normalize_relative(&res.relative_path) {
            issues.push(issue(
                "SPEC-RES-001",
                Severity::Error,
                IssueSource::Spec,
                crate::domain::path_safety::to_slash(&res.relative_path),
                None,
                format!("资源路径不安全: {err}"),
                "资源必须位于 Skill 根目录内，使用相对路径且不包含 `..`。",
            ));
        }
    }
    issues
}

fn validate_body(body: &str) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let facts = markdown::analyze(body);

    if body.trim().is_empty() {
        issues.push(issue(
            "QUAL-BODY-001",
            Severity::Warning,
            IssueSource::Quality,
            "SKILL.md",
            None,
            "正文为空，Skill 没有任何指令内容。",
            "在 frontmatter 之后写下 Agent 应遵循的步骤或规则。",
        ));
        return issues;
    }

    if facts.characters < 40 {
        issues.push(issue(
            "QUAL-BODY-002",
            Severity::Info,
            IssueSource::Quality,
            "SKILL.md",
            None,
            "正文较短，可能不足以让 Agent 稳定复现流程。",
            "补充清晰的步骤、边界与示例。",
        ));
    }

    if facts.lines > RECOMMENDED_MAX_BODY_LINES {
        issues.push(issue(
            "QUAL-BODY-003",
            Severity::Info,
            IssueSource::Quality,
            "SKILL.md",
            None,
            format!(
                "正文共 {} 行，超过建议的 {} 行。",
                facts.lines, RECOMMENDED_MAX_BODY_LINES
            ),
            "把细节拆分到 references/ 中，正文保持精简（渐进式披露）。",
        ));
    }

    if facts.approx_tokens() > RECOMMENDED_MAX_BODY_TOKENS {
        issues.push(issue(
            "QUAL-BODY-004",
            Severity::Info,
            IssueSource::Quality,
            "SKILL.md",
            None,
            format!(
                "正文约 {} tokens，可能超出建议的 {} tokens。",
                facts.approx_tokens(),
                RECOMMENDED_MAX_BODY_TOKENS
            ),
            "考虑把长篇参考内容移到独立文件，按需加载。",
        ));
    }

    if !facts.has_steps && facts.headings == 0 {
        issues.push(issue(
            "QUAL-STRUCT-001",
            Severity::Info,
            IssueSource::Quality,
            "SKILL.md",
            None,
            "正文缺少明显的步骤或章节结构。",
            "使用标题或编号步骤组织流程（这是建议，不是强制规范）。",
        ));
    }

    if !facts.has_examples {
        issues.push(issue(
            "QUAL-STRUCT-002",
            Severity::Info,
            IssueSource::Quality,
            "SKILL.md",
            None,
            "正文中未发现示例。",
            "添加输入/输出示例或边界情况说明，帮助 Agent 正确执行。",
        ));
    }

    issues
}

/// Full validation: generic rules plus platform rules for every adapter.
///
/// Platform capabilities do not vary by target, so each adapter contributes
/// its warnings once even when it discovers several roots (for example the
/// DeepSeek Harness user and shared directories).
pub fn validate_with_context(
    doc: &SkillDocument,
    adapters: &[Box<dyn PlatformAdapter>],
    _ctx: &PlatformContext,
) -> ValidationReport {
    let mut report = validate(doc);
    for adapter in adapters {
        report.extend(adapter.validate(doc));
    }
    report
}

fn parse_error_issue(doc: &SkillDocument) -> ValidationIssue {
    if let Some(raw) = &doc.state.raw_fallback {
        if let Err(err) = skill_md::parse(raw) {
            if let SkillMdError::Frontmatter(FrontmatterError::Yaml { line, message }) = &err {
                return issue(
                    "SPEC-FM-001",
                    Severity::Error,
                    IssueSource::Spec,
                    "SKILL.md",
                    Some(*line),
                    format!("frontmatter 无法解析: {message}"),
                    "修正 YAML 语法；可先切换到源码模式手动编辑。",
                );
            }
            if let SkillMdError::Frontmatter(FrontmatterError::NotMapping) = &err {
                return issue(
                    "SPEC-FM-001",
                    Severity::Error,
                    IssueSource::Spec,
                    "SKILL.md",
                    None,
                    "frontmatter 必须是一个 YAML 映射。",
                    "把 frontmatter 写成 `key: value` 的键值对形式。",
                );
            }
        }
    }
    issue(
        "SPEC-FM-001",
        Severity::Error,
        IssueSource::Spec,
        "SKILL.md",
        None,
        "frontmatter 无法解析。",
        "检查 `---` 之间的 YAML 语法。当前以原始文本模式打开。",
    )
}

/// Build an issue and attach the rule's documentation URL.
fn issue(
    rule_id: &str,
    severity: Severity,
    source: IssueSource,
    file: impl Into<String>,
    line: Option<usize>,
    message: impl Into<String>,
    suggestion: impl Into<String>,
) -> ValidationIssue {
    let mut i = ValidationIssue::new(rule_id, severity, source, file, message, suggestion);
    i.line = line;
    i.doc_url = Some(rule_doc_url(rule_id).to_string());
    i
}

/// Documentation URL for a rule id.
pub fn rule_doc_url(rule_id: &str) -> &'static str {
    if rule_id.starts_with("PLAT-CLAUDE") {
        CLAUDE_URL
    } else if rule_id.starts_with("PLAT-DEEPSEEK-HARNESS") {
        DSH_URL
    } else {
        SPEC_URL
    }
}

/// True when the skill's name matches every discovered target's naming rules.
pub fn name_matches_dir(doc: &SkillDocument) -> bool {
    match doc.dir_name() {
        Some(dir) => dir == doc.name() || (doc.name().is_empty() && dir.is_empty()),
        None => true,
    }
}

/// Convenience: validate a raw `SKILL.md` string with a synthetic root.
pub fn validate_raw(root: &Path, content: &str) -> ValidationReport {
    let doc = SkillDocument::from_skill_md(root.to_path_buf(), content);
    validate(&doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn doc_from(content: &str) -> SkillDocument {
        SkillDocument::from_skill_md(PathBuf::from("/ws/my-skill"), content)
    }

    #[test]
    fn valid_skill_has_no_errors() {
        let content = "---\nname: my-skill\ndescription: Does a thing and explains when to use it clearly.\n---\n# 步骤\n\n1. one\n2. two\n\n```\nexample\n```\n";
        let report = validate(&doc_from(content));
        assert!(!report.has_errors(), "{:?}", report.issues);
    }

    #[test]
    fn missing_description_is_error() {
        let content = "---\nname: my-skill\n---\nbody\n";
        let report = validate(&doc_from(content));
        assert!(report.issues.iter().any(|i| i.rule_id == "SPEC-DESC-001"));
    }

    #[test]
    fn missing_frontmatter_is_error() {
        let report = validate(&doc_from("# just body\n"));
        assert!(report.issues.iter().any(|i| i.rule_id == "SPEC-FM-002"));
    }

    #[test]
    fn invalid_name_is_error() {
        let content = "---\nname: Bad Name\ndescription: long enough description here for the check.\n---\n1. step\n";
        let report = validate(&doc_from(content));
        assert!(report.issues.iter().any(|i| i.rule_id == "SPEC-NAME-002"));
    }

    #[test]
    fn name_dir_mismatch_is_error() {
        let content =
            "---\nname: other-name\ndescription: long enough description here for the check.\n---\n1. step\n";
        let report = validate(&doc_from(content));
        assert!(report.issues.iter().any(|i| i.rule_id == "SPEC-NAME-003"));
    }

    #[test]
    fn invalid_frontmatter_reports_line() {
        let report = validate(&doc_from("---\nname: [oops\ndescription: x\n---\nbody"));
        let err = report
            .issues
            .iter()
            .find(|i| i.rule_id == "SPEC-FM-001")
            .expect("parse error issue");
        assert!(err.line.is_some());
    }

    #[test]
    fn recommended_sections_are_quality_not_spec_errors() {
        let content = "---\nname: my-skill\ndescription: A description long enough to pass the vague check.\n---\nA single paragraph without structure.\n";
        let report = validate(&doc_from(content));
        let struct_issue = report
            .issues
            .iter()
            .find(|i| i.rule_id == "QUAL-STRUCT-001")
            .expect("structure suggestion");
        assert_eq!(struct_issue.severity, Severity::Info);
        assert_eq!(struct_issue.source, IssueSource::Quality);
        assert!(!report.has_errors());
    }

    #[test]
    fn unsafe_resource_path_is_error() {
        let resources = vec![SkillResource::new(
            PathBuf::from("../escape.md"),
            crate::domain::resource::ResourceKind::Markdown,
            1,
        )];
        let issues = validate_resource_paths(&resources);
        assert!(issues.iter().any(|i| i.rule_id == "SPEC-RES-001"));
    }

    #[test]
    fn platform_rules_flag_unknown_fields() {
        use crate::platforms::adapter_by_id;
        let content = "---\nname: my-skill\ndescription: A description long enough to pass the vague check.\nwhen_to_use: only claude knows this\n---\n1. step\n";
        let doc = doc_from(content);
        let opencode = adapter_by_id("opencode").unwrap();
        let issues = opencode.validate(&doc);
        assert!(issues.iter().any(|i| i.rule_id.contains("FIELD")));
    }

    #[test]
    fn platform_rules_link_to_the_right_docs() {
        assert!(rule_doc_url("PLAT-CLAUDE-CODE-FIELD").contains("claude.com"));
        assert!(rule_doc_url("PLAT-DEEPSEEK-HARNESS-FIELD").contains("deepseek-harness"));
        assert!(rule_doc_url("PLAT-DEEPSEEK-HARNESS-DESC").contains("deepseek-harness"));
        assert_eq!(rule_doc_url("PLAT-OPENCODE-FIELD"), SPEC_URL);
    }
}
