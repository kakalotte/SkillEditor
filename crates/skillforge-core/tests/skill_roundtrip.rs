//! Round-trip and workspace integration tests.
//!
//! These exercise the public library API (no GUI) against on-disk fixtures and
//! assert the core product guarantee: editing a known field never loses
//! unknown frontmatter fields, the body, or resource files.

use std::path::PathBuf;

use skillforge_core::domain::resource::ResourceKind;
use skillforge_core::domain::skill::SkillDocument;
use skillforge_core::formats::skill_md;
use skillforge_core::services::{scan_resources, WorkspaceService};

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn copy_fixture(name: &str, dest: &std::path::Path) {
    copy_dir(&fixture_dir(name), dest);
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn fixture_parses_and_round_trips_byte_for_byte() {
    let content = std::fs::read_to_string(fixture_dir("valid-skill").join("SKILL.md")).unwrap();
    let parsed = skill_md::parse(&content).unwrap();
    assert!(parsed.had_block);
    let out = skill_md::serialize(&parsed.frontmatter, &parsed.body).unwrap();
    assert_eq!(
        out, content,
        "untouched document must round-trip byte for byte"
    );
}

#[test]
fn editing_known_field_preserves_unknown_fields_and_cjk() {
    let content = std::fs::read_to_string(fixture_dir("valid-skill").join("SKILL.md")).unwrap();
    let mut doc = SkillDocument::from_skill_md(fixture_dir("valid-skill"), &content);

    assert_eq!(doc.name(), "valid-skill");
    doc.set_description("新的中文描述，用于验证未知字段不会丢失。");
    doc.set_instructions("# 新的正文\n\n内容。\n");
    doc.mark_dirty();

    let out = doc.to_skill_md().unwrap();
    assert!(
        out.contains("x-custom-field: keep-me"),
        "unknown field lost"
    );
    assert!(out.contains("author: skillforge-tests"), "metadata lost");
    assert!(out.contains("新的中文描述"), "CJK lost");

    let reparsed = skill_md::parse(&out).unwrap();
    let keys = reparsed.frontmatter.keys();
    assert_eq!(keys[0], "name");
    assert!(keys.contains(&"x-custom-field".to_string()));
    assert_eq!(
        reparsed.frontmatter.get_str("x-custom-field").as_deref(),
        Some("keep-me")
    );
}

#[test]
fn malformed_frontmatter_is_recoverable_as_raw_text() {
    let content =
        std::fs::read_to_string(fixture_dir("invalid-frontmatter").join("SKILL.md")).unwrap();
    let doc = SkillDocument::from_skill_md(fixture_dir("invalid-frontmatter"), &content);
    assert!(!doc.state.parse_ok);
    assert_eq!(doc.state.raw_fallback.as_deref(), Some(content.as_str()));
    // Serializing the raw-fallback document must not lose the original bytes.
    let out = doc.to_skill_md().unwrap();
    assert_eq!(out, content);
}

#[test]
fn workspace_save_reload_preserves_resources() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    let root = ws.join("valid-skill");
    copy_fixture("valid-skill", &root);

    let workspace = WorkspaceService::new(&ws);
    let mut doc = workspace.load(&root).unwrap();

    // Resources are scanned and classified, and scripts are never executed.
    let kinds: Vec<ResourceKind> = doc.resources.iter().map(|r| r.kind).collect();
    assert!(kinds.contains(&ResourceKind::Script));
    assert!(kinds.contains(&ResourceKind::Markdown));

    doc.set_instructions("# 重新保存\n\n正文。\n");
    workspace.save(&mut doc).unwrap();

    let reloaded = workspace.load(&root).unwrap();
    assert_eq!(reloaded.instructions, "# 重新保存\n\n正文。\n");
    assert!(root.join("references/guide.md").is_file());
    assert!(root.join("scripts/helper.sh").is_file());
    assert_eq!(reloaded.resources.len(), 2);
}

#[test]
fn import_into_workspace_copies_all_files() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    let workspace = WorkspaceService::new(&ws);

    let imported = workspace
        .import_into_workspace(&fixture_dir("valid-skill"))
        .unwrap();
    assert!(imported.join("SKILL.md").is_file());
    assert!(imported.join("references/guide.md").is_file());
    assert!(imported.join("scripts/helper.sh").is_file());

    // Importing again must not overwrite the first copy.
    let again = workspace
        .import_into_workspace(&fixture_dir("valid-skill"))
        .unwrap();
    assert_ne!(imported, again);

    let entries = workspace.list_entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(scan_resources(&imported).unwrap().len(), 2);
}
