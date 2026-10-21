//! Install/uninstall safety integration tests.
//!
//! Uses real temp directories on the local filesystem. Confirms backups,
//! external-change detection, conflict handling and that uninstall only
//! removes files the app installed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use skillforge_core::platforms::Scope;
use skillforge_core::services::install_service::{
    apply_install, apply_uninstall, plan_install, plan_install_with, plan_install_with_overrides,
    plan_uninstall, ManifestStore, PlanOptions,
};
use skillforge_core::services::BackupService;

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) {
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

struct Harness {
    _tmp: tempfile::TempDir,
    source: PathBuf,
    target_parent: PathBuf,
    backups: BackupService,
    manifests: ManifestStore,
}

impl Harness {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source/valid-skill");
        copy_dir(&fixture_dir("valid-skill"), &source);
        let target_parent = tmp.path().join("targets");
        let backups = BackupService::new(tmp.path().join("backups"), 5);
        let manifests = ManifestStore::new(tmp.path().join("manifests"));
        Harness {
            _tmp: tmp,
            source,
            target_parent,
            backups,
            manifests,
        }
    }

    fn install(&self, ts: u64) -> PathBuf {
        let plan = plan_install(
            &self.source,
            "valid-skill",
            &self.target_parent,
            "generic",
            Scope::Custom,
            &self.manifests,
        )
        .unwrap();
        apply_install(&plan, &self.backups, &self.manifests, ts, "0.1.0")
            .unwrap()
            .target_root
    }
}

#[test]
fn planning_alone_writes_nothing() {
    let h = Harness::new();
    let plan = plan_install(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "generic",
        Scope::Custom,
        &h.manifests,
    )
    .unwrap();
    assert!(plan.creates_target);
    // Cancelling after a preview (i.e. never applying) must leave no trace.
    assert!(!h.target_parent.exists());
    assert!(!plan.target_root.exists());
}

#[test]
fn install_then_external_change_is_detected() {
    let h = Harness::new();
    let target = h.install(1000);
    assert!(target.join("SKILL.md").is_file());
    assert!(target.join("references/guide.md").is_file());
    assert!(h.manifests.load(&target).is_some());

    // A clean re-plan reports no external modification.
    let plan = plan_install(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "generic",
        Scope::Custom,
        &h.manifests,
    )
    .unwrap();
    assert!(!plan.has_external_modification);

    // Simulate a third-party edit in the target.
    std::fs::write(target.join("references/guide.md"), "edited by someone else").unwrap();
    let plan = plan_install(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "generic",
        Scope::Custom,
        &h.manifests,
    )
    .unwrap();
    assert!(
        plan.has_external_modification,
        "external edit must be detected before overwriting"
    );
}

#[test]
fn external_change_detection_can_be_turned_off() {
    let h = Harness::new();
    let target = h.install(1000);

    // Someone edits a file the app installed.
    std::fs::write(target.join("references/guide.md"), "edited by someone else").unwrap();

    // Detection on (the default) reports it.
    let detected = plan_install(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "generic",
        Scope::Custom,
        &h.manifests,
    )
    .unwrap();
    assert!(detected.has_external_modification);

    // Detection off skips the comparison, but the rest of the plan is unchanged:
    // the file is still seen as modified and the target is still backed up.
    let ignored = plan_install_with(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "generic",
        Scope::Custom,
        &h.manifests,
        PlanOptions {
            detect_external_modification: false,
        },
    )
    .unwrap();
    assert!(!ignored.has_external_modification);
    assert_eq!(ignored.modified(), detected.modified());
    assert!(ignored.will_backup);
}

#[test]
fn reinstall_creates_a_backup() {
    let h = Harness::new();
    let target = h.install(1000);
    // Change the source body and reinstall.
    let skill_md = h.source.join("SKILL.md");
    let mut content = std::fs::read_to_string(&skill_md).unwrap();
    content.push_str("\n追加一行。\n");
    std::fs::write(&skill_md, content).unwrap();

    let plan = plan_install(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "generic",
        Scope::Custom,
        &h.manifests,
    )
    .unwrap();
    assert!(plan.will_backup);
    let report = apply_install(&plan, &h.backups, &h.manifests, 1001, "0.1.0").unwrap();
    let backup = report.backup.expect("a backup directory should exist");
    assert!(backup.join("SKILL.md").is_file());
    assert!(report.target_root.join("SKILL.md").is_file());
    let _ = target;
}

#[test]
fn projected_content_is_installed_without_touching_the_source() {
    let h = Harness::new();
    let source_text = std::fs::read_to_string(h.source.join("SKILL.md")).unwrap();
    let projected = format!("{source_text}\n<!-- platform projection -->\n");
    let overrides = BTreeMap::from([("SKILL.md".to_string(), projected.clone().into_bytes())]);

    let plan = plan_install_with_overrides(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "deepseek-harness",
        Scope::Custom,
        &h.manifests,
        PlanOptions::default(),
        overrides.clone(),
        Vec::new(),
    )
    .unwrap();

    let entry = plan
        .entries
        .iter()
        .find(|e| e.relative_path == Path::new("SKILL.md"))
        .expect("SKILL.md plan entry");
    assert_eq!(entry.change.as_str(), "added");
    assert_eq!(entry.detail, "将写入平台适配版本");
    assert_eq!(
        plan.overrides.get("SKILL.md").map(Vec::as_slice),
        Some(projected.as_bytes())
    );

    let report = apply_install(&plan, &h.backups, &h.manifests, 1000, "0.1.0").unwrap();
    assert_eq!(
        std::fs::read_to_string(report.target_root.join("SKILL.md")).unwrap(),
        projected
    );
    // The workspace source must be byte-for-byte unchanged.
    assert_eq!(
        std::fs::read_to_string(h.source.join("SKILL.md")).unwrap(),
        source_text
    );

    // Re-planning with the same projection sees the target as unchanged;
    // without it, the target differs from the source.
    let same = plan_install_with_overrides(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "deepseek-harness",
        Scope::Custom,
        &h.manifests,
        PlanOptions::default(),
        overrides,
        Vec::new(),
    )
    .unwrap();
    assert!(same
        .entries
        .iter()
        .any(|e| e.relative_path == Path::new("SKILL.md") && e.change.as_str() == "unchanged"));

    let without = plan_install(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "deepseek-harness",
        Scope::Custom,
        &h.manifests,
    )
    .unwrap();
    assert!(without
        .entries
        .iter()
        .any(|e| e.relative_path == Path::new("SKILL.md") && e.change.as_str() == "modified"));
}

#[cfg(unix)]
#[test]
fn projected_content_can_replace_a_read_only_source_file() {
    use std::os::unix::fs::PermissionsExt;

    let h = Harness::new();
    let skill_md = h.source.join("SKILL.md");
    let mut permissions = std::fs::metadata(&skill_md).unwrap().permissions();
    permissions.set_mode(0o444);
    std::fs::set_permissions(&skill_md, permissions).unwrap();

    let projected = b"---\nname: valid-skill\ndescription: projected\n---\nbody\n".to_vec();
    let plan = plan_install_with_overrides(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "deepseek-harness",
        Scope::Custom,
        &h.manifests,
        PlanOptions::default(),
        BTreeMap::from([("SKILL.md".to_string(), projected.clone())]),
        vec![skillforge_core::platforms::FieldRewrite {
            from: "when_to_use".to_string(),
            to: "whenToUse".to_string(),
            reason: "test projection".to_string(),
        }],
    )
    .unwrap();
    let report = apply_install(&plan, &h.backups, &h.manifests, 1000, "0.1.0").unwrap();

    let target = report.target_root.join("SKILL.md");
    assert_eq!(std::fs::read(&target).unwrap(), projected);
    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o444, "installed copy must keep the source mode");
    let source_mode = std::fs::metadata(&skill_md).unwrap().permissions().mode() & 0o777;
    assert_eq!(source_mode, 0o444, "the source must stay read-only");
    assert_eq!(report.manifest.rewrites.len(), 1);
    assert_eq!(report.manifest.rewrites[0].from, "when_to_use");
    assert_eq!(report.manifest.rewrites[0].to, "whenToUse");
}

#[test]
fn overrides_for_unknown_files_are_rejected() {
    let h = Harness::new();
    let overrides = BTreeMap::from([("missing.md".to_string(), b"x".to_vec())]);
    let err = plan_install_with_overrides(
        &h.source,
        "valid-skill",
        &h.target_parent,
        "deepseek-harness",
        Scope::Custom,
        &h.manifests,
        PlanOptions::default(),
        overrides,
        Vec::new(),
    )
    .expect_err("unknown override path must fail");
    assert!(err.to_string().contains("不在源目录"), "{err}");
}

#[test]
fn uninstall_preserves_unmanaged_files_and_directory() {
    let h = Harness::new();
    let target = h.install(1000);

    // A file the app did not install.
    std::fs::write(target.join("local-note.txt"), "keep me").unwrap();

    let manifest = h.manifests.load(&target).unwrap();
    let plan = plan_uninstall(&target, manifest).unwrap();
    assert!(!plan.has_external_changes);
    apply_uninstall(&plan, &h.manifests).unwrap();

    assert!(!target.join("SKILL.md").exists());
    assert!(
        target.join("local-note.txt").exists(),
        "unmanaged file was deleted"
    );
    assert!(target.is_dir(), "directory with leftover files was removed");
    assert!(h.manifests.load(&target).is_none());
}

#[test]
fn uninstall_flags_files_modified_after_install() {
    let h = Harness::new();
    let target = h.install(1000);
    std::fs::write(target.join("SKILL.md"), "changed outside the app").unwrap();

    let manifest = h.manifests.load(&target).unwrap();
    let plan = plan_uninstall(&target, manifest).unwrap();
    assert!(plan.has_external_changes);
}
