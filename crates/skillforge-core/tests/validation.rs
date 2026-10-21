//! Validation integration tests.
//!
//! Verifies the three-tier model: spec errors, platform compatibility warnings,
//! and quality suggestions. Recommended sections must never be reported as
//! spec errors.

use std::path::PathBuf;

use skillforge_core::domain::skill::SkillDocument;
use skillforge_core::domain::validation::{IssueSource, Severity};
use skillforge_core::platforms::{self, PlatformContext};
use skillforge_core::services::validation_service;

fn fixture_doc(name: &str) -> SkillDocument {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    let content = std::fs::read_to_string(root.join("SKILL.md")).unwrap();
    SkillDocument::from_skill_md(root, &content)
}

#[test]
fn valid_fixture_has_no_spec_errors() {
    let report = validation_service::validate(&fixture_doc("valid-skill"));
    assert!(!report.has_errors(), "issues: {:?}", report.issues);
}

#[test]
fn broken_frontmatter_reports_a_spec_error_with_a_line() {
    let report = validation_service::validate(&fixture_doc("invalid-frontmatter"));
    let err = report
        .issues
        .iter()
        .find(|i| i.rule_id == "SPEC-FM-001")
        .expect("frontmatter parse error");
    assert_eq!(err.severity, Severity::Error);
    assert_eq!(err.source, IssueSource::Spec);
    assert!(err.line.is_some(), "parse error should point at a line");
}

#[test]
fn missing_frontmatter_is_a_spec_error() {
    let doc = SkillDocument::from_skill_md(PathBuf::from("/tmp/no-fm"), "# just a body\n");
    let report = validation_service::validate(&doc);
    assert!(report.issues.iter().any(|i| i.rule_id == "SPEC-FM-002"));
}

#[test]
fn invalid_name_and_dir_mismatch_are_errors() {
    let bad = SkillDocument::from_skill_md(
        PathBuf::from("/ws/Bad Name"),
        "---\nname: Bad Name\ndescription: long enough description here for the check.\n---\n1. step\n",
    );
    let report = validation_service::validate(&bad);
    assert!(report.issues.iter().any(|i| i.rule_id == "SPEC-NAME-002"));

    let mismatch = SkillDocument::from_skill_md(
        PathBuf::from("/ws/other-dir"),
        "---\nname: my-name\ndescription: long enough description here for the check.\n---\n1. step\n",
    );
    let report = validation_service::validate(&mismatch);
    assert!(report.issues.iter().any(|i| i.rule_id == "SPEC-NAME-003"));
}

#[test]
fn recommended_structure_is_quality_not_spec_error() {
    let doc = SkillDocument::from_skill_md(
        PathBuf::from("/ws/plain-skill"),
        "---\nname: plain-skill\ndescription: A description long enough to pass the vague check.\n---\nA single paragraph without structure.\n",
    );
    let report = validation_service::validate(&doc);
    let suggestion = report
        .issues
        .iter()
        .find(|i| i.rule_id == "QUAL-STRUCT-001")
        .expect("structure suggestion");
    assert_eq!(suggestion.severity, Severity::Info);
    assert_eq!(suggestion.source, IssueSource::Quality);
    assert!(!report.has_errors());
}

#[test]
fn platform_validation_warns_about_unknown_fields() {
    let doc = fixture_doc("valid-skill"); // contains x-custom-field
    let adapters = platforms::all_adapters();
    let ctx = PlatformContext {
        home: Some(PathBuf::from("/home/tester")),
        project_dir: None,
        ..Default::default()
    };
    let report = validation_service::validate_with_context(&doc, &adapters, &ctx);

    assert!(!report.has_errors(), "unknown fields must warn, not error");
    let platform_warning = report
        .issues
        .iter()
        .find(|i| matches!(i.source, IssueSource::Platform(_)))
        .expect("expected a platform compatibility warning");
    assert_eq!(platform_warning.severity, Severity::Warning);
}

#[test]
fn platform_warnings_are_not_duplicated_per_target() {
    // DeepSeek Harness discovers four roots, but its capabilities are the same
    // for every root: one unsupported field must produce one warning.
    let doc = SkillDocument::from_skill_md(
        PathBuf::from("/home/tester/.dsh/skills/my-skill"),
        "---\nname: my-skill\ndescription: A description long enough to pass the vague check.\nlicense: MIT\n---\n1. step\n",
    );
    let adapters = platforms::all_adapters();
    let ctx = PlatformContext {
        home: Some(PathBuf::from("/home/tester")),
        project_dir: Some(PathBuf::from("/work/project")),
        ..Default::default()
    };
    let report = validation_service::validate_with_context(&doc, &adapters, &ctx);
    let count = report
        .issues
        .iter()
        .filter(|i| i.rule_id == "PLAT-DEEPSEEK-HARNESS-FIELD" && i.message.contains("license"))
        .count();
    assert_eq!(count, 1, "issues: {:?}", report.issues);
}
