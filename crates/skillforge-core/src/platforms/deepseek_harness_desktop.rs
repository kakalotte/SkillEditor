//! DeepSeek Harness Desktop adapter.
//!
//! Sources (checked 2026-10-20):
//! * Official desktop (Electron shell around the same Harness):
//!   https://github.com/deepseek-ai/deepseek-harness/tree/master/apps/desktop
//! * Community desktop that bundles the same upstream:
//!   https://github.com/anywhere-labs/dsh-desktop
//!
//! Desktop does not introduce a separate skill store: it starts the same Harness
//! core, so the local skill provider reads the exact same roots as the CLI
//! (`~/.dsh/skills`, `~/.agents/skills`, and the project-level equivalents) and
//! `DSH_HOME` can move them. The desktop application additionally owns
//! `$DSH_HOME/profiles/desktop`, which is what this adapter probes for.
//!
//! Marked `PartiallyVerified` on purpose: the skill roots are inherited from the
//! Harness docs, but a running Desktop installation was not available to confirm
//! that neither front end overrides `DSH_HOME`.

use crate::domain::skill::SkillDocument;
use crate::domain::validation::ValidationIssue;
use crate::platforms::common;
use crate::platforms::deepseek_harness::{
    harness_capabilities, harness_targets, project_harness_skill_md,
};
use crate::platforms::{
    InstallTarget, PlatformAdapter, PlatformCapabilities, PlatformContext, PlatformDetection,
    SkillProjection, VerificationStatus,
};

pub struct DeepSeekHarnessDesktopAdapter;

impl PlatformAdapter for DeepSeekHarnessDesktopAdapter {
    fn id(&self) -> &'static str {
        "deepseek-harness-desktop"
    }

    fn display_name(&self) -> &'static str {
        "DeepSeek Harness Desktop"
    }

    fn docs_url(&self) -> &'static str {
        "https://github.com/deepseek-ai/deepseek-harness/tree/master/apps/desktop"
    }

    fn verified_on(&self) -> &'static str {
        "2026-10-20"
    }

    fn verification(&self) -> VerificationStatus {
        VerificationStatus::PartiallyVerified
    }

    fn capabilities(&self) -> PlatformCapabilities {
        harness_capabilities(
            "Desktop 运行的是同一套 Harness 核心，技能目录与 CLI 完全相同\
             （`~/.dsh/skills`、`~/.agents/skills`，以及项目级同名目录）；\
             从这里安装与从 DeepSeek Harness 安装结果一致。桌面壳私有数据在 \
             `$DSH_HOME/profiles/desktop`，技能不存放在那里。若用 `DSH_HOME` \
             改过位置，请在设置里配置自定义路径。",
        )
    }

    fn detect(&self, ctx: &PlatformContext) -> PlatformDetection {
        let home = ctx.home.as_ref();
        let profile_present = home
            .map(|h| h.join(".dsh").join("profiles").join("desktop").is_dir())
            .unwrap_or(false);
        let home_present = home.map(|h| h.join(".dsh").is_dir()).unwrap_or(false);
        PlatformDetection {
            found: profile_present,
            detail: if profile_present {
                "发现 ~/.dsh/profiles/desktop（桌面端已初始化）".to_string()
            } else if home_present {
                "发现 ~/.dsh，但未见 desktop profile；桌面端可能尚未启动过".to_string()
            } else {
                "未发现 ~/.dsh（仍可手动指定目标）".to_string()
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
        // Desktop runs the same Harness core and reads the same fields.
        project_harness_skill_md(skill_md)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx() -> PlatformContext {
        PlatformContext {
            home: Some(PathBuf::from("/home/u")),
            project_dir: Some(PathBuf::from("/work/p")),
            ..Default::default()
        }
    }

    #[test]
    fn shares_the_harness_roots() {
        let paths: Vec<_> = DeepSeekHarnessDesktopAdapter
            .discover_targets(&ctx())
            .into_iter()
            .map(|t| t.path)
            .collect();
        assert!(paths.contains(&PathBuf::from("/home/u/.dsh/skills")));
        assert!(paths.contains(&PathBuf::from("/home/u/.agents/skills")));
    }

    #[test]
    fn detects_an_initialized_desktop_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().to_path_buf();
        let profile = home.join(".dsh").join("profiles").join("desktop");
        std::fs::create_dir_all(&profile).expect("create profile");
        let ctx = PlatformContext {
            home: Some(home),
            ..Default::default()
        };

        let detection = DeepSeekHarnessDesktopAdapter.detect(&ctx);
        assert!(detection.found);
        assert!(detection.detail.contains("profiles/desktop"));
    }

    #[test]
    fn is_marked_partially_verified() {
        assert_eq!(
            DeepSeekHarnessDesktopAdapter.verification(),
            VerificationStatus::PartiallyVerified
        );
    }

    #[test]
    fn projects_with_the_same_rule_as_the_cli() {
        let source = "---\nname: my-skill\ndescription: A description long enough to pass the vague check.\nwhen_to_use: x\n---\nbody\n";
        let projection = DeepSeekHarnessDesktopAdapter
            .project(source)
            .expect("desktop projection");
        assert!(projection.content.contains("whenToUse: x"));
        assert_eq!(projection.rewrites.len(), 1);
    }
}
