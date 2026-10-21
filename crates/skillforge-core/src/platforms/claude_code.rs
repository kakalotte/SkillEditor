//! Claude Code adapter.
//!
//! Sources (checked 2026-10-17):
//! * https://code.claude.com/docs/en/skills
//!
//! Personal skills live in `~/.claude/skills/<name>/SKILL.md`; project skills
//! in `.claude/skills/<name>/SKILL.md`. Claude Code accepts the Agent Skills
//! spec fields plus a set of Claude-specific extensions.

use crate::domain::skill::SkillDocument;
use crate::domain::validation::ValidationIssue;
use crate::platforms::common::{self, SPEC_FIELDS};
use crate::platforms::{
    InstallTarget, PlatformAdapter, PlatformCapabilities, PlatformContext, PlatformDetection,
    VerificationStatus,
};

pub struct ClaudeCodeAdapter;

const CLAUDE_FIELDS: &[&str] = &[
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
    // Claude Code extensions:
    "when_to_use",
    "argument-hint",
    "arguments",
    "disable-model-invocation",
    "user-invocable",
    "disallowed-tools",
    "model",
    "effort",
    "context",
    "agent",
    "background",
    "hooks",
    "paths",
    "shell",
];

impl PlatformAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn docs_url(&self) -> &'static str {
        "https://code.claude.com/docs/en/skills"
    }

    fn verified_on(&self) -> &'static str {
        "2026-10-17"
    }

    fn verification(&self) -> VerificationStatus {
        VerificationStatus::Verified
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            supported_fields: CLAUDE_FIELDS.to_vec(),
            listing_limit: 1536,
            supports_resources: true,
            resource_dirs: vec!["scripts", "references", "assets"],
            notes: "支持 Agent Skills 通用字段及 Claude Code 扩展字段；description 与 when_to_use 在列表中合并截断到 1536 字符。",
        }
    }

    fn detect(&self, ctx: &PlatformContext) -> PlatformDetection {
        let user_present = ctx
            .home
            .as_ref()
            .map(|h| h.join(".claude").is_dir())
            .unwrap_or(false);
        let project_present = ctx
            .project_dir
            .as_ref()
            .map(|p| p.join(".claude").is_dir())
            .unwrap_or(false);
        PlatformDetection {
            found: user_present || project_present,
            detail: if user_present {
                "发现 ~/.claude 目录".to_string()
            } else if project_present {
                "发现项目内 .claude 目录".to_string()
            } else {
                "未发现 ~/.claude 目录（仍可手动指定目标）".to_string()
            },
        }
    }

    fn discover_targets(&self, ctx: &PlatformContext) -> Vec<InstallTarget> {
        let mut out = Vec::new();
        if let Some(t) =
            common::user_target(ctx, self.id(), self.display_name(), &[".claude", "skills"])
        {
            out.push(t);
        }
        if let Some(t) =
            common::project_target(ctx, self.id(), self.display_name(), &[".claude", "skills"])
        {
            out.push(t);
        }
        if let Some(t) = common::custom_target(ctx, self.id(), self.display_name()) {
            out.push(t);
        }
        out
    }

    fn validate(&self, doc: &SkillDocument) -> Vec<ValidationIssue> {
        common::field_compat_issues(doc, &self.capabilities(), self.id(), self.docs_url())
    }
}

/// Fields unique to Claude Code (used by docs/tests).
pub fn claude_extension_fields() -> &'static [&'static str] {
    CLAUDE_FIELDS
}

/// The subset that is part of the open spec.
pub fn spec_fields() -> &'static [&'static str] {
    SPEC_FIELDS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx() -> PlatformContext {
        PlatformContext {
            home: Some(PathBuf::from("/home/u")),
            project_dir: Some(PathBuf::from("/work/proj")),
            ..Default::default()
        }
    }

    #[test]
    fn discovers_user_and_project_targets() {
        let targets = ClaudeCodeAdapter.discover_targets(&ctx());
        let paths: Vec<_> = targets.iter().map(|t| t.path.clone()).collect();
        assert!(paths.contains(&PathBuf::from("/home/u/.claude/skills")));
        assert!(paths.contains(&PathBuf::from("/work/proj/.claude/skills")));
    }

    #[test]
    fn spec_fields_are_all_supported() {
        let caps = ClaudeCodeAdapter.capabilities();
        for field in SPEC_FIELDS {
            assert!(caps.supports_field(field), "missing {field}");
        }
        assert!(caps.supports_field("when_to_use"));
    }
}
