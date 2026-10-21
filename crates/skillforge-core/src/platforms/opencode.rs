//! OpenCode adapter.
//!
//! Sources (checked 2026-10-17):
//! * https://opencode.ai/docs/skills/
//!
//! Project: `.opencode/skills/<name>/SKILL.md`
//! Global:  `~/.config/opencode/skills/<name>/SKILL.md`
//! OpenCode also reads `.claude/skills/` and `.agents/skills/` (both project
//! and global). `name` must match `^[a-z0-9]+(-[a-z0-9]+)*$` and `description`
//! must be 1-1024 characters.

use crate::domain::skill::SkillDocument;
use crate::domain::validation::ValidationIssue;
use crate::platforms::common::{self, SPEC_FIELDS};
use crate::platforms::{
    InstallTarget, PlatformAdapter, PlatformCapabilities, PlatformContext, PlatformDetection,
    VerificationStatus,
};

pub struct OpenCodeAdapter;

impl PlatformAdapter for OpenCodeAdapter {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn docs_url(&self) -> &'static str {
        "https://opencode.ai/docs/skills/"
    }

    fn verified_on(&self) -> &'static str {
        "2026-10-17"
    }

    fn verification(&self) -> VerificationStatus {
        VerificationStatus::Verified
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            supported_fields: SPEC_FIELDS.to_vec(),
            listing_limit: 1024,
            supports_resources: true,
            resource_dirs: vec!["scripts", "references", "assets"],
            notes: "使用 Agent Skills 通用字段；平台专属行为放在 `metadata`（如 opencode/slash、opencode/autoinvoke）。description 上限 1024 字符。",
        }
    }

    fn detect(&self, ctx: &PlatformContext) -> PlatformDetection {
        let user_present = ctx
            .home
            .as_ref()
            .map(|h| h.join(".config").join("opencode").is_dir())
            .unwrap_or(false);
        let project_present = ctx
            .project_dir
            .as_ref()
            .map(|p| p.join(".opencode").is_dir())
            .unwrap_or(false);
        PlatformDetection {
            found: user_present || project_present,
            detail: if user_present {
                "发现 ~/.config/opencode 目录".to_string()
            } else if project_present {
                "发现项目内 .opencode 目录".to_string()
            } else {
                "未发现 OpenCode 配置目录（仍可手动指定目标）".to_string()
            },
        }
    }

    fn discover_targets(&self, ctx: &PlatformContext) -> Vec<InstallTarget> {
        let mut out = Vec::new();
        if let Some(t) = common::user_target(
            ctx,
            self.id(),
            self.display_name(),
            &[".config", "opencode", "skills"],
        ) {
            out.push(t);
        }
        if let Some(t) = common::project_target(
            ctx,
            self.id(),
            self.display_name(),
            &[".opencode", "skills"],
        ) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn discovers_opencode_paths() {
        let ctx = PlatformContext {
            home: Some(PathBuf::from("/home/u")),
            project_dir: Some(PathBuf::from("/work/p")),
            ..Default::default()
        };
        let paths: Vec<_> = OpenCodeAdapter
            .discover_targets(&ctx)
            .into_iter()
            .map(|t| t.path)
            .collect();
        assert!(paths.contains(&PathBuf::from("/home/u/.config/opencode/skills")));
        assert!(paths.contains(&PathBuf::from("/work/p/.opencode/skills")));
    }
}
