//! Platform path discovery tests.
//!
//! Uses a synthetic HOME and project directory so results never depend on the
//! developer's real configuration. Also pins the honest verification status of
//! each adapter.

use std::collections::BTreeMap;
use std::path::PathBuf;

use skillforge_core::platforms::{
    adapter_by_id, all_adapters, PlatformContext, VerificationStatus,
};

fn ctx() -> PlatformContext {
    PlatformContext {
        home: Some(PathBuf::from("/home/tester")),
        project_dir: Some(PathBuf::from("/work/project")),
        custom_paths: BTreeMap::new(),
    }
}

fn paths_of(id: &str, ctx: &PlatformContext) -> Vec<PathBuf> {
    adapter_by_id(id)
        .unwrap()
        .discover_targets(ctx)
        .into_iter()
        .map(|t| t.path)
        .collect()
}

#[test]
fn claude_code_discovers_user_and_project_targets() {
    let paths = paths_of("claude-code", &ctx());
    assert!(paths.contains(&PathBuf::from("/home/tester/.claude/skills")));
    assert!(paths.contains(&PathBuf::from("/work/project/.claude/skills")));
}

#[test]
fn opencode_discovers_user_and_project_targets() {
    let paths = paths_of("opencode", &ctx());
    assert!(paths.contains(&PathBuf::from("/home/tester/.config/opencode/skills")));
    assert!(paths.contains(&PathBuf::from("/work/project/.opencode/skills")));
}

#[test]
fn codex_discovers_user_and_project_targets() {
    let paths = paths_of("codex", &ctx());
    assert!(paths.contains(&PathBuf::from("/home/tester/.codex/skills")));
    assert!(paths.contains(&PathBuf::from("/work/project/.codex/skills")));
}

#[test]
fn deepseek_harness_discovers_harness_specific_and_shared_roots() {
    let paths = paths_of("deepseek-harness", &ctx());
    assert!(paths.contains(&PathBuf::from("/home/tester/.dsh/skills")));
    assert!(paths.contains(&PathBuf::from("/home/tester/.agents/skills")));
    assert!(paths.contains(&PathBuf::from("/work/project/.dsh/skills")));
    assert!(paths.contains(&PathBuf::from("/work/project/.agents/skills")));
}

#[test]
fn deepseek_harness_desktop_shares_the_same_roots() {
    // Desktop runs the same Harness core, so it must not invent a separate
    // skill store. If this ever changes, the docs note must change too.
    assert_eq!(
        paths_of("deepseek-harness-desktop", &ctx()),
        paths_of("deepseek-harness", &ctx())
    );
}

#[test]
fn generic_has_no_default_target_but_honors_overrides() {
    assert!(paths_of("generic", &ctx()).is_empty());

    let mut custom = BTreeMap::new();
    custom.insert("generic".to_string(), PathBuf::from("/opt/agent-skills"));
    let ctx = PlatformContext {
        custom_paths: custom,
        ..ctx()
    };
    assert_eq!(
        paths_of("generic", &ctx),
        vec![PathBuf::from("/opt/agent-skills")]
    );
}

#[test]
fn custom_override_is_appended_for_every_adapter() {
    for id in [
        "claude-code",
        "codex",
        "opencode",
        "deepseek-harness",
        "deepseek-harness-desktop",
    ] {
        let mut custom = BTreeMap::new();
        custom.insert(id.to_string(), PathBuf::from("/custom/skills"));
        let ctx = PlatformContext {
            custom_paths: custom,
            ..ctx()
        };
        let paths = paths_of(id, &ctx);
        assert!(
            paths.contains(&PathBuf::from("/custom/skills")),
            "{id} should expose the custom override"
        );
    }
}

#[test]
fn every_adapter_has_a_docs_url_and_a_verification_date() {
    for adapter in all_adapters() {
        assert!(
            !adapter.docs_url().is_empty(),
            "{} has no docs url",
            adapter.id()
        );
        assert!(
            !adapter.verified_on().is_empty(),
            "{} has no verification date",
            adapter.id()
        );
    }
}

#[test]
fn advisory_verification_status_is_pinned() {
    let codex = adapter_by_id("codex").unwrap();
    assert_eq!(codex.verification(), VerificationStatus::PartiallyVerified);

    let claude = adapter_by_id("claude-code").unwrap();
    assert_eq!(claude.verification(), VerificationStatus::Verified);

    let dsh = adapter_by_id("deepseek-harness").unwrap();
    assert_eq!(dsh.verification(), VerificationStatus::Verified);

    // Desktop is a shell over the same Harness; its roots are inherited and
    // were not confirmed on a running installation.
    let desktop = adapter_by_id("deepseek-harness-desktop").unwrap();
    assert_eq!(
        desktop.verification(),
        VerificationStatus::PartiallyVerified
    );
}
