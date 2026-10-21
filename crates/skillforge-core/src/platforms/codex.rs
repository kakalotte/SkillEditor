//! OpenAI Codex adapter.
//!
//! Sources (checked 2026-10-17):
//! * Community + official repository evidence: personal skills live in
//!   `~/.codex/skills/<name>/SKILL.md`; project skills in
//!   `.codex/skills/<name>/SKILL.md`.
//! * https://github.com/openai/skills
//!
//! The canonical docs page (`https://developers.openai.com/codex/skills/`)
//! could not be fetched by automated tooling at verification time, so this
//! adapter is marked `PartiallyVerified` and the user is encouraged to confirm
//! the path in the platform page before installing.

use crate::domain::skill::SkillDocument;
use crate::domain::validation::ValidationIssue;
use crate::platforms::common::{self, SPEC_FIELDS};
use crate::platforms::{
    InstallTarget, PlatformAdapter, PlatformCapabilities, PlatformContext, PlatformDetection,
    VerificationStatus,
};

pub struct CodexAdapter;

impl PlatformAdapter for CodexAdapter {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "OpenAI Codex"
    }

    fn docs_url(&self) -> &'static str {
        "https://developers.openai.com/codex/skills/"
    }

    fn verified_on(&self) -> &'static str {
        "2026-10-17"
    }

    fn verification(&self) -> VerificationStatus {
        // The docs site blocked automated fetching; paths come from the
        // official skills repository and community reports.
        VerificationStatus::PartiallyVerified
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            supported_fields: SPEC_FIELDS.to_vec(),
            listing_limit: 0,
            supports_resources: true,
            resource_dirs: vec!["scripts", "references", "assets"],
            notes: "以 Agent Skills 通用字段为准。官方文档页面在核验时无法自动抓取，路径依据官方 skills 仓库与社区资料，请在安装页确认后再使用。",
        }
    }

    fn detect(&self, ctx: &PlatformContext) -> PlatformDetection {
        let user_present = ctx
            .home
            .as_ref()
            .map(|h| h.join(".codex").is_dir())
            .unwrap_or(false);
        let project_present = ctx
            .project_dir
            .as_ref()
            .map(|p| p.join(".codex").is_dir())
            .unwrap_or(false);
        PlatformDetection {
            found: user_present || project_present,
            detail: if user_present {
                "发现 ~/.codex 目录".to_string()
            } else if project_present {
                "发现项目内 .codex 目录".to_string()
            } else {
                "未发现 ~/.codex 目录（仍可手动指定目标）".to_string()
            },
        }
    }

    fn discover_targets(&self, ctx: &PlatformContext) -> Vec<InstallTarget> {
        let mut out = Vec::new();
        if let Some(t) =
            common::user_target(ctx, self.id(), self.display_name(), &[".codex", "skills"])
        {
            out.push(t);
        }
        if let Some(t) =
            common::project_target(ctx, self.id(), self.display_name(), &[".codex", "skills"])
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn discovers_codex_paths() {
        let ctx = PlatformContext {
            home: Some(PathBuf::from("/home/u")),
            project_dir: Some(PathBuf::from("/work/p")),
            ..Default::default()
        };
        let paths: Vec<_> = CodexAdapter
            .discover_targets(&ctx)
            .into_iter()
            .map(|t| t.path)
            .collect();
        assert!(paths.contains(&PathBuf::from("/home/u/.codex/skills")));
        assert!(paths.contains(&PathBuf::from("/work/p/.codex/skills")));
    }

    #[test]
    fn is_marked_partially_verified() {
        assert_eq!(
            CodexAdapter.verification(),
            VerificationStatus::PartiallyVerified
        );
    }
}
