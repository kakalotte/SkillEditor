//! Generic Agent Skills directory adapter.
//!
//! This is the "any directory the user chooses" target. It applies only the
//! open specification rules and never assumes a product-specific location.
//!
//! Sources (checked 2026-10-17):
//! * https://agentskills.io/specification

use crate::domain::skill::SkillDocument;
use crate::domain::validation::ValidationIssue;
use crate::platforms::common::{self, SPEC_FIELDS};
use crate::platforms::{
    InstallTarget, PlatformAdapter, PlatformCapabilities, PlatformContext, PlatformDetection,
    VerificationStatus,
};

pub struct GenericAdapter;

impl PlatformAdapter for GenericAdapter {
    fn id(&self) -> &'static str {
        "generic"
    }

    fn display_name(&self) -> &'static str {
        "通用目录（手动选择）"
    }

    fn docs_url(&self) -> &'static str {
        "https://agentskills.io/specification"
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
            notes: "仅套用 Agent Skills 开放规范，不假设任何产品专属目录。",
        }
    }

    fn detect(&self, ctx: &PlatformContext) -> PlatformDetection {
        PlatformDetection {
            found: ctx.custom(self.id()).is_some(),
            detail: if ctx.custom(self.id()).is_some() {
                "已配置自定义目标目录".to_string()
            } else {
                "未配置自定义目录，请在下方选择或输入目标目录".to_string()
            },
        }
    }

    fn discover_targets(&self, ctx: &PlatformContext) -> Vec<InstallTarget> {
        common::custom_target(ctx, self.id(), self.display_name())
            .into_iter()
            .collect()
    }

    fn validate(&self, doc: &SkillDocument) -> Vec<ValidationIssue> {
        common::field_compat_issues(doc, &self.capabilities(), self.id(), self.docs_url())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn no_targets_without_configuration() {
        let ctx = PlatformContext::default();
        assert!(GenericAdapter.discover_targets(&ctx).is_empty());
        assert!(!GenericAdapter.detect(&ctx).found);
    }

    #[test]
    fn uses_custom_path_when_present() {
        let mut custom = BTreeMap::new();
        custom.insert("generic".to_string(), PathBuf::from("/tmp/skills"));
        let ctx = PlatformContext {
            custom_paths: custom,
            ..Default::default()
        };
        let targets = GenericAdapter.discover_targets(&ctx);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, PathBuf::from("/tmp/skills"));
    }
}
