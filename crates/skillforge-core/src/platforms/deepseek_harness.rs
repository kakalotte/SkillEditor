//! DeepSeek Harness (`dsh`) adapter.
//!
//! Sources (checked 2026-10-20):
//! * Skill subsystem:
//!   https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/skills.md
//! * Config catalog (`dshHome` / `agentsHome` defaults):
//!   https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/config-catalog.md
//!
//! The shipped local provider scans these roots, in rank order:
//!
//! | Rank | Root |
//! | --- | --- |
//! | 100 | `<project>/.dsh/skills` |
//! | 200 | `<project>/.agents/skills` |
//! | 400 | `<dshHome>/skills` |
//! | 500 | `<agentsHome>/skills` |
//!
//! `dshHome` defaults to `$DSH_HOME` or `~/.dsh`; `agentsHome` defaults to
//! `$DSH_AGENTS_HOME` or `~/.agents`. The project root is the nearest ancestor
//! containing `.git`, falling back to the current directory.
//!
//! A skill is either a directory bundle (`<name>/SKILL.md`) or a flat
//! `<name>.md` file; this app always writes directory bundles. Only `name` and
//! `description` are required, and a skill whose name is not kebab-case is
//! ignored (logged, not an error).

use crate::domain::skill::SkillDocument;
use crate::domain::validation::ValidationIssue;
use crate::platforms::common;
use crate::platforms::{
    FieldRewrite, InstallTarget, PlatformAdapter, PlatformCapabilities, PlatformContext,
    PlatformDetection, SkillProjection, VerificationStatus,
};

/// Frontmatter keys the local provider actually reads.
///
/// `whenToUse` is camelCase because the provider reads that exact key; the two
/// invocation switches are kebab-case. Other keys (including the generic
/// `license` / `compatibility` / `allowed-tools`) are ignored silently.
pub const HARNESS_FIELDS: &[&str] = &[
    "name",
    "description",
    "whenToUse",
    "metadata",
    "disable-model-invocation",
    "user-invocable",
];

/// Label for the Harness-only root (`~/.dsh/skills`).
pub const VARIANT_HARNESS: &str = "Harness 专用";
/// Label for the shared root (`~/.agents/skills`) also read by other agents.
pub const VARIANT_SHARED: &str = "跨 Agent 共享";

/// Build the capability matrix shared by the CLI and Desktop adapters.
pub fn harness_capabilities(notes: &'static str) -> PlatformCapabilities {
    PlatformCapabilities {
        supported_fields: HARNESS_FIELDS.to_vec(),
        // The model-facing catalog truncates descriptions at
        // `catalogDescriptionMaxLength`, 500 by default.
        listing_limit: 500,
        supports_resources: true,
        resource_dirs: vec!["scripts", "references", "assets"],
        notes,
    }
}

/// All roots the DSH local provider scans, for any front end that runs it.
pub fn harness_targets(
    ctx: &PlatformContext,
    platform_id: &str,
    label_prefix: &str,
) -> Vec<InstallTarget> {
    let mut out = Vec::new();
    if let Some(t) = common::user_target_with_variant(
        ctx,
        platform_id,
        label_prefix,
        &[".dsh", "skills"],
        Some(VARIANT_HARNESS),
    ) {
        out.push(t);
    }
    if let Some(t) = common::user_target_with_variant(
        ctx,
        platform_id,
        label_prefix,
        &[".agents", "skills"],
        Some(VARIANT_SHARED),
    ) {
        out.push(t);
    }
    if let Some(t) = common::project_target_with_variant(
        ctx,
        platform_id,
        label_prefix,
        &[".dsh", "skills"],
        Some(VARIANT_HARNESS),
    ) {
        out.push(t);
    }
    if let Some(t) = common::project_target_with_variant(
        ctx,
        platform_id,
        label_prefix,
        &[".agents", "skills"],
        Some(VARIANT_SHARED),
    ) {
        out.push(t);
    }
    if let Some(t) = common::custom_target(ctx, platform_id, label_prefix) {
        out.push(t);
    }
    out
}

/// Generic name used by other Agent Skills consumers.
pub const WHEN_TO_USE_SNAKE: &str = "when_to_use";
/// Exact key the Harness reads (camelCase).
pub const WHEN_TO_USE_CAMEL: &str = "whenToUse";

/// Adapt a `SKILL.md` for the Harness by renaming the generic `when_to_use`
/// key to `whenToUse`.
///
/// This is a text-level rewrite of the top-level key only: comments, key
/// order, unknown fields, the body and the source file are left untouched.
/// Returns `None` when there is nothing to do (already adapted, no such key,
/// or unparseable frontmatter).
pub fn project_harness_skill_md(content: &str) -> Option<SkillProjection> {
    let parsed = crate::formats::skill_md::parse(content).ok()?;
    if !parsed.had_block {
        return None;
    }
    if parsed.frontmatter.contains_key(WHEN_TO_USE_CAMEL)
        || !parsed.frontmatter.contains_key(WHEN_TO_USE_SNAKE)
    {
        return None;
    }

    let (yaml_range, _) = crate::formats::skill_md::frontmatter_ranges(content)?;
    let yaml = &content[yaml_range.clone()];
    let (new_yaml, count) = rename_top_level_key(yaml, WHEN_TO_USE_SNAKE, WHEN_TO_USE_CAMEL);
    if count == 0 {
        return None;
    }

    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..yaml_range.start]);
    out.push_str(&new_yaml);
    out.push_str(&content[yaml_range.end..]);
    Some(SkillProjection {
        content: out,
        rewrites: vec![FieldRewrite {
            from: WHEN_TO_USE_SNAKE.to_string(),
            to: WHEN_TO_USE_CAMEL.to_string(),
            reason: "Harness 读取 camelCase 的 whenToUse；仅修改安装副本，源文件保持不变。"
                .to_string(),
        }],
    })
}

/// Rename a top-level YAML key, preserving leading indentation, comments, line
/// endings and everything after the key. Returns the new YAML and how many
/// lines were changed.
fn rename_top_level_key(yaml: &str, from: &str, to: &str) -> (String, usize) {
    // A root block mapping is normally at column 0, but YAML also permits the
    // whole mapping to be indented. Use the smallest indentation of a
    // non-blank, non-comment line as the top-level indentation; nested keys are
    // always deeper, so they cannot be mistaken for the top-level key.
    let top_indent = yaml
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);

    let mut out = String::with_capacity(yaml.len());
    let mut count = 0;
    for line in yaml.split_inclusive('\n') {
        let (body, eol) = match line.strip_suffix('\n') {
            Some(rest) => match rest.strip_suffix('\r') {
                Some(rest) => (rest, "\r\n"),
                None => (rest, "\n"),
            },
            None => (line, ""),
        };
        let mut replaced = false;
        let indent = body.len() - body.trim_start().len();
        if indent == top_indent {
            let trimmed = &body[indent..];
            if let Some(rest) = trimmed.strip_prefix(from) {
                // A top-level key is followed by a colon. This rejects
                // `when_to_use_extra` and keys with a non-colon suffix.
                if rest.trim_start().starts_with(':') {
                    out.push_str(&body[..indent]);
                    out.push_str(to);
                    out.push_str(rest);
                    out.push_str(eol);
                    count += 1;
                    replaced = true;
                }
            }
        }
        if !replaced {
            out.push_str(line);
        }
    }
    (out, count)
}

pub struct DeepSeekHarnessAdapter;

impl PlatformAdapter for DeepSeekHarnessAdapter {
    fn id(&self) -> &'static str {
        "deepseek-harness"
    }

    fn display_name(&self) -> &'static str {
        "DeepSeek Harness"
    }

    fn docs_url(&self) -> &'static str {
        "https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/skills.md"
    }

    fn verified_on(&self) -> &'static str {
        "2026-10-20"
    }

    fn verification(&self) -> VerificationStatus {
        // Paths, frontmatter keys and limits come from the official
        // `docs/subsystems/skills.md` and `docs/config-catalog.md`.
        VerificationStatus::Verified
    }

    fn capabilities(&self) -> PlatformCapabilities {
        harness_capabilities(
            "DSH 读取 `name`、`description`、`whenToUse`（camelCase）、`metadata`、\
             `disable-model-invocation`、`user-invocable`，其余字段会被忽略。\
             用户技能放在 `~/.dsh/skills`（Harness 专用）或 `~/.agents/skills`（跨 Agent 共享），\
             两者都会被扫描。本应用按这两个默认位置发现目标；如果你用 \
             `DSH_HOME` / `DSH_AGENTS_HOME` 改过位置，请在设置里为该平台配置自定义路径。",
        )
    }

    fn detect(&self, ctx: &PlatformContext) -> PlatformDetection {
        let user_present = ctx
            .home
            .as_ref()
            .map(|h| h.join(".dsh").is_dir())
            .unwrap_or(false);
        let agents_present = ctx
            .home
            .as_ref()
            .map(|h| h.join(".agents").is_dir())
            .unwrap_or(false);
        let project_present = ctx
            .project_dir
            .as_ref()
            .map(|p| p.join(".dsh").is_dir() || p.join(".agents").is_dir())
            .unwrap_or(false);
        PlatformDetection {
            found: user_present || agents_present || project_present,
            detail: if user_present {
                "发现 ~/.dsh 目录".to_string()
            } else if agents_present {
                "发现 ~/.agents 目录".to_string()
            } else if project_present {
                "发现项目内 .dsh 或 .agents 目录".to_string()
            } else {
                "未发现 ~/.dsh 或 ~/.agents 目录（仍可手动指定目标）".to_string()
            },
        }
    }

    fn discover_targets(&self, ctx: &PlatformContext) -> Vec<InstallTarget> {
        harness_targets(ctx, self.id(), self.display_name())
    }

    fn validate(&self, doc: &SkillDocument) -> Vec<ValidationIssue> {
        common::field_compat_issues(doc, &self.capabilities(), self.id(), self.docs_url())
    }

    fn project(&self, skill_md: &str) -> Option<SkillProjection> {
        project_harness_skill_md(skill_md)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::skill::SkillDocument;
    use std::path::PathBuf;

    fn ctx() -> PlatformContext {
        PlatformContext {
            home: Some(PathBuf::from("/home/u")),
            project_dir: Some(PathBuf::from("/work/p")),
            ..Default::default()
        }
    }

    #[test]
    fn discovers_the_four_documented_roots() {
        let paths: Vec<_> = DeepSeekHarnessAdapter
            .discover_targets(&ctx())
            .into_iter()
            .map(|t| t.path)
            .collect();
        assert!(paths.contains(&PathBuf::from("/home/u/.dsh/skills")));
        assert!(paths.contains(&PathBuf::from("/home/u/.agents/skills")));
        assert!(paths.contains(&PathBuf::from("/work/p/.dsh/skills")));
        assert!(paths.contains(&PathBuf::from("/work/p/.agents/skills")));
    }

    #[test]
    fn the_two_user_roots_carry_distinct_variants() {
        let targets = DeepSeekHarnessAdapter.discover_targets(&ctx());
        let user: Vec<_> = targets
            .iter()
            .filter(|t| t.scope == crate::platforms::Scope::User)
            .collect();
        assert_eq!(user.len(), 2);
        let labels: Vec<_> = user.iter().map(|t| t.variant).collect();
        assert!(labels.contains(&Some(VARIANT_HARNESS)));
        assert!(labels.contains(&Some(VARIANT_SHARED)));
    }

    #[test]
    fn declares_the_invocation_switches_as_supported() {
        let caps = DeepSeekHarnessAdapter.capabilities();
        assert!(caps.supports_field("name"));
        assert!(caps.supports_field("description"));
        assert!(caps.supports_field("whenToUse"));
        assert!(caps.supports_field("disable-model-invocation"));
        assert!(caps.supports_field("user-invocable"));
        assert!(!caps.supports_field("when_to_use"));
        assert_eq!(caps.listing_limit, 500);
    }

    #[test]
    fn compatibility_warnings_link_to_the_dsh_docs() {
        let doc = SkillDocument::from_skill_md(
            PathBuf::from("/home/u/.dsh/skills/my-skill"),
            "---\nname: my-skill\ndescription: A description long enough to pass the vague check.\nlicense: MIT\n---\n1. step\n",
        );
        let issues = DeepSeekHarnessAdapter.validate(&doc);
        let field = issues
            .iter()
            .find(|i| i.rule_id == "PLAT-DEEPSEEK-HARNESS-FIELD")
            .expect("license must be reported as unsupported");
        assert_eq!(
            field.doc_url.as_deref(),
            Some(DeepSeekHarnessAdapter.docs_url())
        );
    }

    #[test]
    fn when_to_use_warning_describes_the_projection() {
        let doc = SkillDocument::from_skill_md(
            PathBuf::from("/home/u/.dsh/skills/my-skill"),
            "---\nname: my-skill\ndescription: A description long enough to pass the vague check.\nwhen_to_use: Use for PDFs.\n---\nbody\n",
        );
        let issues = DeepSeekHarnessAdapter.validate(&doc);
        let field = issues
            .iter()
            .find(|i| i.rule_id == "PLAT-DEEPSEEK-HARNESS-FIELD")
            .expect("field warning");
        assert!(field.message.contains("自动尝试改名"), "{}", field.message);
        assert!(field.suggestion.contains("whenToUse"));
    }

    #[test]
    fn is_marked_verified() {
        assert_eq!(
            DeepSeekHarnessAdapter.verification(),
            VerificationStatus::Verified
        );
    }

    #[test]
    fn projection_renames_only_the_top_level_alias() {
        let source = "---\nname: my-skill\ndescription: A description long enough to pass the vague check.\nwhen_to_use: Use for PDFs. # keep this comment\nmetadata:\n  when_to_use: nested value\nx-extra: keep\n---\n\n# Body\n\nwhen_to_use: not frontmatter\n";
        let projection = project_harness_skill_md(source).expect("projection");

        assert!(projection
            .content
            .contains("whenToUse: Use for PDFs. # keep this comment"));
        assert!(projection
            .content
            .contains("metadata:\n  when_to_use: nested value"));
        assert!(projection
            .content
            .contains("# Body\n\nwhen_to_use: not frontmatter"));
        assert!(projection.content.contains("x-extra: keep"));
        assert_eq!(projection.rewrites.len(), 1);
        assert_eq!(projection.rewrites[0].from, WHEN_TO_USE_SNAKE);
        assert_eq!(projection.rewrites[0].to, WHEN_TO_USE_CAMEL);
        // The source string itself is never mutated.
        assert!(source.contains("\nwhen_to_use: Use for PDFs."));
    }

    #[test]
    fn projection_handles_an_indented_root_mapping() {
        let source = "---\n  name: my-skill\n  description: A description long enough to pass the vague check.\n  when_to_use: x\n  metadata:\n    when_to_use: nested\n---\nbody\n";
        let projection = project_harness_skill_md(source).expect("projection");
        assert!(projection.content.contains("  whenToUse: x"));
        assert!(projection
            .content
            .contains("  metadata:\n    when_to_use: nested"));
    }

    #[test]
    fn projection_is_a_noop_when_already_adapted() {
        let source = "---\nname: my-skill\ndescription: A description long enough to pass the vague check.\nwhenToUse: Use for PDFs.\n---\nbody\n";
        assert!(project_harness_skill_md(source).is_none());
    }

    #[test]
    fn projection_is_a_noop_when_both_keys_exist() {
        let source = "---\nname: my-skill\ndescription: A description long enough to pass the vague check.\nwhen_to_use: old\nwhenToUse: new\n---\nbody\n";
        assert!(project_harness_skill_md(source).is_none());

        let doc = SkillDocument::from_skill_md(PathBuf::from("demo"), source);
        let issues = DeepSeekHarnessAdapter.validate(&doc);
        let field = issues
            .iter()
            .find(|i| i.rule_id == "PLAT-DEEPSEEK-HARNESS-FIELD")
            .expect("field warning");
        assert!(field.message.contains("不会自动合并"), "{}", field.message);
        assert!(field.suggestion.contains("whenToUse"));
    }

    #[test]
    fn projection_preserves_a_leading_bom() {
        let source = "\u{feff}---\nname: my-skill\ndescription: A description long enough to pass the vague check.\nwhen_to_use: x\n---\nbody\n";
        let projection = project_harness_skill_md(source).expect("projection");
        assert!(projection.content.starts_with("\u{feff}---\n"));
        assert!(projection.content.contains("whenToUse: x"));
        assert!(projection.content.ends_with("body\n"));
    }

    #[test]
    fn projection_preserves_crlf_and_returns_none_without_frontmatter() {
        let crlf = "---\r\nname: my-skill\r\ndescription: A description long enough.\r\nwhen_to_use: x\r\n---\r\nbody\r\n";
        let projection = project_harness_skill_md(crlf).expect("projection");
        assert!(projection.content.contains("whenToUse: x\r\n"));
        assert!(projection.content.contains("body\r\n"));

        assert!(project_harness_skill_md("# just a body\n").is_none());
        assert!(project_harness_skill_md("---\nname: broken\n  bad: [\n---\nbody\n").is_none());
    }
}
