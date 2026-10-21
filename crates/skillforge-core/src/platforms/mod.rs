//! Platform adapters.
//!
//! Each adapter owns the platform-specific knowledge (default paths, supported
//! frontmatter fields, discovered targets). Paths are based on official
//! documentation and are always overridable by the user. Verification status
//! is recorded per adapter and surfaced in the UI; nothing is claimed as
//! "verified" without a documented source and date.

pub mod claude_code;
pub mod codex;
pub mod common;
pub mod deepseek_harness;
pub mod deepseek_harness_desktop;
pub mod generic;
pub mod opencode;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::skill::SkillDocument;
use crate::domain::validation::ValidationIssue;

/// Installation scope of a target directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    User,
    Project,
    Custom,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Project => "project",
            Scope::Custom => "custom",
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Scope::User => "用户级",
            Scope::Project => "项目级",
            Scope::Custom => "自定义",
        }
    }
}

/// A directory a skill can be installed into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    pub platform_id: String,
    pub scope: Scope,
    /// Human label, e.g. "Claude Code · 用户级".
    pub label: String,
    /// The directory that holds skill subdirectories.
    pub path: PathBuf,
    pub exists: bool,
    /// Distinguishes several targets of the same scope, e.g. "Harness 专用"
    /// versus "跨 Agent 共享". `None` when a scope has a single target.
    pub variant: Option<&'static str>,
}

impl InstallTarget {
    pub fn display_path(&self) -> String {
        self.path.display().to_string()
    }
}

/// Result of probing whether the platform seems present on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformDetection {
    pub found: bool,
    pub detail: String,
}

/// How confident we are about an adapter's platform rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStatus {
    /// Documented rules read and tested against fixtures.
    Verified,
    /// Partially verified; some behavior is unconfirmed.
    PartiallyVerified,
    /// No confirmation yet.
    Unverified,
}

impl VerificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            VerificationStatus::Verified => "verified",
            VerificationStatus::PartiallyVerified => "warning",
            VerificationStatus::Unverified => "unverified",
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            VerificationStatus::Verified => "已验证",
            VerificationStatus::PartiallyVerified => "兼容但有警告",
            VerificationStatus::Unverified => "待验证",
        }
    }
}

/// What an adapter declares about its target platform.
#[derive(Debug, Clone)]
pub struct PlatformCapabilities {
    /// Top-level frontmatter keys the platform honors.
    pub supported_fields: Vec<&'static str>,
    /// Combined `description` (+ the `when_to_use` variant the platform reads)
    /// listing limit, 0 when unknown.
    pub listing_limit: usize,
    /// Whether supporting files are used.
    pub supports_resources: bool,
    /// Conventional resource directories.
    pub resource_dirs: Vec<&'static str>,
    pub notes: &'static str,
}

impl PlatformCapabilities {
    pub fn supports_field(&self, key: &str) -> bool {
        self.supported_fields.contains(&key)
    }
}

/// One deterministic field rename applied when installing for a platform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldRewrite {
    pub from: String,
    pub to: String,
    pub reason: String,
}

/// A platform-specific adaptation of `SKILL.md` for installation.
///
/// The source tree is never modified. The installer writes `content` into the
/// staged copy and records the final hashes, so the preview and the installed
/// file always agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillProjection {
    pub content: String,
    pub rewrites: Vec<FieldRewrite>,
}

/// Ambient environment used to discover targets. Passed in explicitly so tests
/// never depend on the developer's real home directory.
#[derive(Debug, Clone, Default)]
pub struct PlatformContext {
    pub home: Option<PathBuf>,
    /// Project root used for project-scoped targets.
    pub project_dir: Option<PathBuf>,
    /// User-configured override directories, keyed by platform id.
    pub custom_paths: BTreeMap<String, PathBuf>,
}

impl PlatformContext {
    pub fn custom(&self, platform_id: &str) -> Option<&PathBuf> {
        self.custom_paths.get(platform_id)
    }
}

/// The adapter interface. Installation itself lives in `InstallService`; an
/// adapter only describes and validates.
pub trait PlatformAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn docs_url(&self) -> &'static str;
    /// ISO date the rules were last checked against the docs.
    fn verified_on(&self) -> &'static str;
    fn verification(&self) -> VerificationStatus;
    fn capabilities(&self) -> PlatformCapabilities;

    /// Probe whether the platform appears installed.
    fn detect(&self, ctx: &PlatformContext) -> PlatformDetection;

    /// Candidate install targets for this platform.
    fn discover_targets(&self, ctx: &PlatformContext) -> Vec<InstallTarget>;

    /// Platform-specific validation for a document.
    ///
    /// This is per platform, not per target: every target of one platform has
    /// the same capabilities, so duplicating the same warning for each
    /// discovered root would only add noise.
    fn validate(&self, doc: &SkillDocument) -> Vec<ValidationIssue>;

    /// Adapt the on-disk `SKILL.md` for this platform before installation.
    ///
    /// Must be deterministic, must preserve unknown fields and the body, and
    /// must never touch the source file. Returning `None` means "install
    /// as-is". Implementations should only rewrite fields with a documented
    /// equivalent key.
    fn project(&self, _skill_md: &str) -> Option<SkillProjection> {
        None
    }
}

/// All adapters, in display order.
pub fn all_adapters() -> Vec<Box<dyn PlatformAdapter>> {
    vec![
        Box::new(claude_code::ClaudeCodeAdapter),
        Box::new(codex::CodexAdapter),
        Box::new(opencode::OpenCodeAdapter),
        Box::new(deepseek_harness::DeepSeekHarnessAdapter),
        Box::new(deepseek_harness_desktop::DeepSeekHarnessDesktopAdapter),
        Box::new(generic::GenericAdapter),
    ]
}

/// Look up an adapter by id.
pub fn adapter_by_id(id: &str) -> Option<Box<dyn PlatformAdapter>> {
    all_adapters().into_iter().find(|a| a.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_adapter_ids_are_unique() {
        let adapters = all_adapters();
        let mut ids: Vec<&str> = adapters.iter().map(|a| a.id()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "adapter ids must be unique");
    }

    #[test]
    fn lookup_by_id_works() {
        assert!(adapter_by_id("claude-code").is_some());
        assert!(adapter_by_id("does-not-exist").is_none());
    }
}
