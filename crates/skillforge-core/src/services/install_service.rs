//! Skill installation: planning, diffing, safe apply and uninstall.
//!
//! Safety rules implemented here:
//! * every source path is validated before it is written,
//! * an existing target is always backed up before being replaced,
//! * the new tree is staged in a temporary directory on the same filesystem
//!   and verified before the swap,
//! * a swap failure rolls back to the previous tree,
//! * a manifest (relative path -> content hash) records exactly which files
//!   this app installed, so uninstall only removes files it owns and detects
//!   external modifications.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::path_safety::normalize_relative;
use crate::errors::{Error, Result};
use crate::platforms::{FieldRewrite, Scope};
use crate::services::backup_service::BackupService;
use crate::services::fs_util;

/// What happens to one file during an install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Unchanged,
    Removed,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Modified => "modified",
            ChangeKind::Unchanged => "unchanged",
            ChangeKind::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanEntry {
    pub relative_path: PathBuf,
    pub change: ChangeKind,
    pub detail: String,
}

/// A record of everything this app installed into a target directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub target_path: String,
    pub skill_name: String,
    pub source_path: String,
    pub platform_id: String,
    pub scope: String,
    pub installed_at: u64,
    pub app_version: String,
    /// relative path -> sha256 hex
    pub files: BTreeMap<String, String>,
    /// Field rewrites applied to the installed copy (empty when installed
    /// as-is). `#[serde(default)]` keeps manifests written by older versions
    /// readable.
    #[serde(default)]
    pub rewrites: Vec<FieldRewrite>,
}

/// Stores manifests in a central directory, keyed by a hash of the target path.
#[derive(Debug, Clone)]
pub struct ManifestStore {
    dir: PathBuf,
}

impl ManifestStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        ManifestStore { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn key(target: &Path) -> String {
        fs_util::hash_bytes(target.to_string_lossy().as_bytes())[..16].to_string()
    }

    fn path_for(&self, target: &Path) -> PathBuf {
        self.dir.join(format!("{}.json", Self::key(target)))
    }

    pub fn load(&self, target: &Path) -> Option<Manifest> {
        let path = self.path_for(target);
        let bytes = std::fs::read(&path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self, manifest: &Manifest) -> Result<()> {
        let path = self.path_for(Path::new(&manifest.target_path));
        let bytes = serde_json::to_vec_pretty(manifest)
            .map_err(|e| Error::Install(format!("序列化清单失败: {e}")))?;
        fs_util::atomic_write(&path, &bytes)
    }

    pub fn remove(&self, target: &Path) -> Result<()> {
        let path = self.path_for(target);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
        }
        Ok(())
    }
}

/// A complete install plan produced by `plan_install`.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub platform_id: String,
    pub scope: Scope,
    pub source_root: PathBuf,
    pub target_parent: PathBuf,
    pub target_root: PathBuf,
    pub skill_name: String,
    pub entries: Vec<PlanEntry>,
    pub creates_target: bool,
    /// Target files were changed since we installed them.
    pub has_external_modification: bool,
    pub will_backup: bool,
    pub previous: Option<Manifest>,
    /// Platform-adapted file contents (relative path -> bytes). The staged copy
    /// is overwritten with these after copying the source tree, so the diff and
    /// the installed hashes describe the real result. The source is untouched.
    pub overrides: BTreeMap<String, Vec<u8>>,
    /// Field rewrites that produced `overrides`, recorded in the manifest.
    pub rewrites: Vec<FieldRewrite>,
}

impl InstallPlan {
    pub fn added(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.change == ChangeKind::Added)
            .count()
    }
    pub fn modified(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.change == ChangeKind::Modified)
            .count()
    }
    pub fn removed(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.change == ChangeKind::Removed)
            .count()
    }
    pub fn unchanged(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.change == ChangeKind::Unchanged)
            .count()
    }
}

/// Build the plan for installing `source_root` as `skill_name` into
/// `target_parent`.
/// Options controlling how a plan is built.
#[derive(Debug, Clone, Copy)]
pub struct PlanOptions {
    /// Compare files already in the target against the last recorded manifest,
    /// so a third-party edit is flagged before it is overwritten.
    ///
    /// Turning this off skips the comparison entirely (no hashing of the
    /// target), which is what the `detect_external_changes` setting does. The
    /// backup still happens; only the *detection* is skipped.
    pub detect_external_modification: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        PlanOptions {
            detect_external_modification: true,
        }
    }
}

pub fn plan_install(
    source_root: &Path,
    skill_name: &str,
    target_parent: &Path,
    platform_id: &str,
    scope: Scope,
    manifests: &ManifestStore,
) -> Result<InstallPlan> {
    plan_install_with(
        source_root,
        skill_name,
        target_parent,
        platform_id,
        scope,
        manifests,
        PlanOptions::default(),
    )
}

/// Build a plan with explicit options. See [`plan_install`].
#[allow(clippy::too_many_arguments)]
pub fn plan_install_with(
    source_root: &Path,
    skill_name: &str,
    target_parent: &Path,
    platform_id: &str,
    scope: Scope,
    manifests: &ManifestStore,
    options: PlanOptions,
) -> Result<InstallPlan> {
    plan_install_with_overrides(
        source_root,
        skill_name,
        target_parent,
        platform_id,
        scope,
        manifests,
        options,
        BTreeMap::new(),
        Vec::new(),
    )
}

/// Build a plan that writes platform-adapted contents for some source files.
///
/// Overrides are keyed by slash-separated relative path and must point at
/// existing source files. Hashing uses the override bytes, so an install whose
/// only difference is a projection is reported as `Modified`, and a re-install
/// of the same projection is reported as `Unchanged`.
#[allow(clippy::too_many_arguments)]
pub fn plan_install_with_overrides(
    source_root: &Path,
    skill_name: &str,
    target_parent: &Path,
    platform_id: &str,
    scope: Scope,
    manifests: &ManifestStore,
    options: PlanOptions,
    overrides: BTreeMap<String, Vec<u8>>,
    rewrites: Vec<FieldRewrite>,
) -> Result<InstallPlan> {
    if !source_root.is_dir() {
        return Err(Error::Install(format!(
            "源目录不存在: {}",
            source_root.display()
        )));
    }
    let target_root = target_parent.join(skill_name);

    // Validate and normalize override keys up front: they end up as write
    // destinations, so they go through the same path safety rules as sources.
    let mut overrides_normalized = BTreeMap::new();
    for (rel, bytes) in overrides {
        let rel = normalize_relative(Path::new(&rel))?;
        overrides_normalized.insert(crate::domain::path_safety::to_slash(&rel), bytes);
    }

    // Collect source files (skip directories).
    let walked = fs_util::walk(source_root)?;
    let mut source_hashes: BTreeMap<String, String> = BTreeMap::new();
    let mut skipped_symlinks = Vec::new();
    for entry in &walked {
        if entry.is_symlink {
            skipped_symlinks.push(entry.relative_path.clone());
            continue;
        }
        if entry.is_dir {
            continue;
        }
        let rel = normalize_relative(&entry.relative_path)?;
        let key = crate::domain::path_safety::to_slash(&rel);
        let abs = source_root.join(&rel);
        let hash = match overrides_normalized.get(&key) {
            Some(bytes) => fs_util::hash_bytes(bytes),
            None => fs_util::hash_file(&abs)?,
        };
        source_hashes.insert(key, hash);
    }

    for rel in overrides_normalized.keys() {
        if !source_hashes.contains_key(rel) {
            return Err(Error::Install(format!(
                "平台适配的目标文件不在源目录中: {rel}"
            )));
        }
    }

    let previous = manifests.load(&target_root);

    let mut entries = Vec::new();
    let mut has_external_modification = false;

    for (rel, hash) in &source_hashes {
        let abs = target_root.join(rel);
        let adapted = if overrides_normalized.contains_key(rel) {
            "将写入平台适配版本"
        } else {
            ""
        };
        if !abs.exists() {
            entries.push(PlanEntry {
                relative_path: PathBuf::from(rel),
                change: ChangeKind::Added,
                detail: adapted.to_string(),
            });
        } else {
            let existing = fs_util::hash_file(&abs)?;
            if &existing == hash {
                entries.push(PlanEntry {
                    relative_path: PathBuf::from(rel),
                    change: ChangeKind::Unchanged,
                    detail: String::new(),
                });
            } else {
                // Compare against what we last installed to detect a third-party
                // edit in the target. Skipped entirely when the user turns
                // external-change detection off.
                if options.detect_external_modification {
                    if let Some(prev) = &previous {
                        match prev.files.get(rel) {
                            Some(known) if known == &existing => {}
                            Some(_) => has_external_modification = true,
                            None => has_external_modification = true,
                        }
                    }
                }
                entries.push(PlanEntry {
                    relative_path: PathBuf::from(rel),
                    change: ChangeKind::Modified,
                    detail: if adapted.is_empty() {
                        "目标文件将被覆盖".to_string()
                    } else {
                        adapted.to_string()
                    },
                });
            }
        }
    }

    // Files we previously installed that no longer exist in the source.
    if let Some(prev) = &previous {
        for rel in prev.files.keys() {
            if !source_hashes.contains_key(rel) {
                entries.push(PlanEntry {
                    relative_path: PathBuf::from(rel),
                    change: ChangeKind::Removed,
                    detail: "源中已删除".to_string(),
                });
            }
        }
    }

    for symlink in &skipped_symlinks {
        entries.push(PlanEntry {
            relative_path: symlink.clone(),
            change: ChangeKind::Unchanged,
            detail: "符号链接已跳过（不会跟随）".to_string(),
        });
    }

    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    let creates_target = !target_root.exists();
    Ok(InstallPlan {
        platform_id: platform_id.to_string(),
        scope,
        source_root: source_root.to_path_buf(),
        target_parent: target_parent.to_path_buf(),
        target_root,
        skill_name: skill_name.to_string(),
        entries,
        creates_target,
        has_external_modification,
        will_backup: !creates_target,
        previous,
        overrides: overrides_normalized,
        rewrites,
    })
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub target_root: PathBuf,
    pub backup: Option<PathBuf>,
    pub copied_files: usize,
    pub skipped_symlinks: Vec<PathBuf>,
    pub manifest: Manifest,
}

/// Apply a plan: backup, stage, verify, swap, record manifest.
pub fn apply_install(
    plan: &InstallPlan,
    backups: &BackupService,
    manifests: &ManifestStore,
    timestamp: u64,
    app_version: &str,
) -> Result<InstallReport> {
    // 1. Validate every source relative path.
    for entry in &plan.entries {
        normalize_relative(&entry.relative_path)?;
    }

    // The target must stay inside the chosen parent.
    if !crate::domain::path_safety::lexically_within(&plan.target_parent, &plan.target_root) {
        return Err(Error::Install("目标路径逃逸出所选目录".to_string()));
    }

    // 2. Back up an existing target.
    let backup = if plan.target_root.exists() {
        Some(backups.backup_target(&plan.target_root, &plan.skill_name, timestamp)?)
    } else {
        None
    };

    // 3. Stage into a temp dir on the same filesystem as the target.
    std::fs::create_dir_all(&plan.target_parent).map_err(|e| Error::io(&plan.target_parent, e))?;
    let staging = tempfile::Builder::new()
        .prefix(".skillforge-staging-")
        .tempdir_in(&plan.target_parent)
        .map_err(|e| Error::io(&plan.target_parent, e))?;
    let copy = fs_util::copy_dir_contents(&plan.source_root, staging.path())?;

    // 3b. Apply platform adaptations to the staged copy only. The source tree
    //     is never modified; the manifest below hashes what was actually
    //     written, so uninstall and external-change detection stay accurate.
    for (rel, bytes) in &plan.overrides {
        let rel = normalize_relative(Path::new(rel))?;
        let destination = crate::domain::path_safety::safe_join(staging.path(), &rel)?;
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        // `fs::copy` preserves the source mode; a read-only SKILL.md would
        // otherwise make the projected copy unwritable. Add owner-write only
        // (never world-write), then restore the source mode after writing.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&destination) {
                let mut permissions = meta.permissions();
                permissions.set_mode(permissions.mode() | 0o200);
                let _ = std::fs::set_permissions(&destination, permissions);
            }
        }
        #[cfg(not(unix))]
        {
            if let Ok(meta) = std::fs::metadata(&destination) {
                let mut permissions = meta.permissions();
                // This block only compiles on non-Unix targets (in practice
                // Windows), where clearing read-only does not imply the
                // world-writable mode the lint warns about. The source mode is
                // restored right after the override is written.
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                let _ = std::fs::set_permissions(&destination, permissions);
            }
        }
        std::fs::write(&destination, bytes).map_err(|e| Error::io(&destination, e))?;
        if let Ok(meta) = std::fs::metadata(plan.source_root.join(&rel)) {
            let _ = std::fs::set_permissions(&destination, meta.permissions());
        }
    }

    // 4. Verify the staged tree.
    if !staging.path().join("SKILL.md").is_file() {
        return Err(Error::Install(
            "暂存目录中缺少 SKILL.md，已中止安装".to_string(),
        ));
    }

    // 5. Swap. Move the old target aside, move staging into place, then delete
    //    the old copy. Restore on failure.
    let old_path = plan
        .target_parent
        .join(format!(".skillforge-old-{timestamp}"));
    let mut moved_old = false;
    if plan.target_root.exists() {
        std::fs::rename(&plan.target_root, &old_path)
            .map_err(|e| Error::io(&plan.target_root, e))?;
        moved_old = true;
    }

    // `TempDir::keep` prevents the Drop cleanup from deleting our staged tree.
    let staged_path = staging.keep();
    match std::fs::rename(&staged_path, &plan.target_root) {
        Ok(()) => {}
        Err(e) => {
            if moved_old {
                let _ = std::fs::rename(&old_path, &plan.target_root);
            }
            let _ = fs_util::remove_dir_all_safe(&staged_path);
            return Err(Error::io(&plan.target_root, e));
        }
    }

    if moved_old {
        let _ = fs_util::remove_dir_all_safe(&old_path);
    }

    // 6. Record the manifest from the (post-swap) target contents.
    let mut files = BTreeMap::new();
    for entry in fs_util::walk(&plan.target_root)? {
        if entry.is_dir || entry.is_symlink {
            continue;
        }
        let rel = crate::domain::path_safety::to_slash(&entry.relative_path);
        let hash = fs_util::hash_file(&plan.target_root.join(&entry.relative_path))?;
        files.insert(rel, hash);
    }

    let manifest = Manifest {
        target_path: plan.target_root.to_string_lossy().into_owned(),
        skill_name: plan.skill_name.clone(),
        source_path: plan.source_root.to_string_lossy().into_owned(),
        platform_id: plan.platform_id.clone(),
        scope: plan.scope.as_str().to_string(),
        installed_at: timestamp,
        app_version: app_version.to_string(),
        files,
        rewrites: plan.rewrites.clone(),
    };
    manifests.save(&manifest)?;

    Ok(InstallReport {
        target_root: plan.target_root.clone(),
        backup,
        copied_files: copy.copied_files,
        skipped_symlinks: copy.skipped_symlinks,
        manifest,
    })
}

/// Plan for removing a previously installed skill.
#[derive(Debug, Clone)]
pub struct UninstallPlan {
    pub target_root: PathBuf,
    pub managed_files: Vec<PathBuf>,
    pub has_external_changes: bool,
    pub manifest: Manifest,
}

pub fn plan_uninstall(target_root: &Path, manifest: Manifest) -> Result<UninstallPlan> {
    let mut managed_files = Vec::new();
    let mut has_external_changes = false;
    for (rel, hash) in &manifest.files {
        let abs = target_root.join(rel);
        if !abs.exists() {
            continue;
        }
        let current = fs_util::hash_file(&abs)?;
        if &current != hash {
            has_external_changes = true;
        }
        managed_files.push(PathBuf::from(rel));
    }
    Ok(UninstallPlan {
        target_root: target_root.to_path_buf(),
        managed_files,
        has_external_changes,
        manifest,
    })
}

/// Remove the files recorded in the manifest, then prune now-empty dirs and
/// delete the target directory if it is empty. Never touches unmanaged files:
/// if anything remains the directory is left in place.
pub fn apply_uninstall(plan: &UninstallPlan, manifests: &ManifestStore) -> Result<()> {
    for rel in &plan.managed_files {
        let abs = crate::domain::path_safety::safe_join(&plan.target_root, rel)?;
        if abs.exists() {
            std::fs::remove_file(&abs).map_err(|e| Error::io(&abs, e))?;
        }
    }

    // Remove empty directories bottom-up, but never escape the target root.
    let mut dirs: Vec<PathBuf> = fs_util::walk(&plan.target_root)?
        .into_iter()
        .filter(|e| e.is_dir)
        .map(|e| e.relative_path)
        .collect();
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for rel in dirs {
        let abs = plan.target_root.join(&rel);
        if abs.is_dir() && dir_is_empty(&abs) {
            let _ = std::fs::remove_dir(&abs);
        }
    }

    if plan.target_root.is_dir() && dir_is_empty(&plan.target_root) {
        let _ = std::fs::remove_dir(&plan.target_root);
    }

    manifests.remove(&plan.target_root)?;
    Ok(())
}

fn dir_is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut it| it.next().is_none())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platforms::Scope;

    fn write_skill(root: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(
            root.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: d\n---\n{body}"),
        )
        .unwrap();
        std::fs::create_dir_all(root.join("references")).unwrap();
        std::fs::write(root.join("references/a.md"), "ref").unwrap();
    }

    #[test]
    fn installs_backs_up_and_detects_external_edit() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source/demo");
        write_skill(&source, "demo", "v1\n");

        let target_parent = dir.path().join("targets");
        let backups = BackupService::new(dir.path().join("backups"), 5);
        let manifests = ManifestStore::new(dir.path().join("manifests"));

        // First install creates the target.
        let plan = plan_install(
            &source,
            "demo",
            &target_parent,
            "generic",
            Scope::Custom,
            &manifests,
        )
        .unwrap();
        assert!(plan.creates_target);
        assert_eq!(plan.added(), 2);
        let report = apply_install(&plan, &backups, &manifests, 1000, "0.1.0").unwrap();
        assert!(report.target_root.join("SKILL.md").is_file());
        assert!(manifests.load(&report.target_root).is_some());

        // Reinstall with a change -> modified + backup.
        std::fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: d\n---\nv2\n",
        )
        .unwrap();
        let plan = plan_install(
            &source,
            "demo",
            &target_parent,
            "generic",
            Scope::Custom,
            &manifests,
        )
        .unwrap();
        assert_eq!(plan.modified(), 1);
        assert!(!plan.creates_target);
        let report = apply_install(&plan, &backups, &manifests, 1001, "0.1.0").unwrap();
        assert!(report.backup.is_some());

        // External edit to the target -> detected.
        std::fs::write(
            report.target_root.join("references/a.md"),
            "externally changed",
        )
        .unwrap();
        let plan = plan_install(
            &source,
            "demo",
            &target_parent,
            "generic",
            Scope::Custom,
            &manifests,
        )
        .unwrap();
        assert!(plan.has_external_modification);
    }

    #[test]
    fn uninstall_removes_managed_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source/demo");
        write_skill(&source, "demo", "body");
        let target_parent = dir.path().join("targets");
        let backups = BackupService::new(dir.path().join("backups"), 5);
        let manifests = ManifestStore::new(dir.path().join("manifests"));

        let plan = plan_install(
            &source,
            "demo",
            &target_parent,
            "generic",
            Scope::Custom,
            &manifests,
        )
        .unwrap();
        let report = apply_install(&plan, &backups, &manifests, 1, "0.1.0").unwrap();

        // Add an unmanaged file; uninstall must leave it and the directory.
        std::fs::write(report.target_root.join("local-note.txt"), "keep me").unwrap();

        let manifest = manifests.load(&report.target_root).unwrap();
        let up = plan_uninstall(&report.target_root, manifest).unwrap();
        apply_uninstall(&up, &manifests).unwrap();

        assert!(!report.target_root.join("SKILL.md").exists());
        assert!(report.target_root.join("local-note.txt").exists());
        assert!(manifests.load(&report.target_root).is_none());
    }

    #[test]
    fn legacy_manifest_without_rewrites_still_deserializes() {
        let json = r#"{
            "target_path": "/tmp/target",
            "skill_name": "demo",
            "source_path": "/tmp/source",
            "platform_id": "generic",
            "scope": "custom",
            "installed_at": 1,
            "app_version": "0.1.0",
            "files": {"SKILL.md": "abc"}
        }"#;
        let manifest: Manifest = serde_json::from_str(json).unwrap();
        assert!(manifest.rewrites.is_empty());
    }

    #[test]
    fn uninstall_flags_external_changes() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source/demo");
        write_skill(&source, "demo", "body");
        let target_parent = dir.path().join("targets");
        let backups = BackupService::new(dir.path().join("backups"), 5);
        let manifests = ManifestStore::new(dir.path().join("manifests"));
        let plan = plan_install(
            &source,
            "demo",
            &target_parent,
            "generic",
            Scope::Custom,
            &manifests,
        )
        .unwrap();
        let report = apply_install(&plan, &backups, &manifests, 1, "0.1.0").unwrap();
        std::fs::write(report.target_root.join("SKILL.md"), "changed").unwrap();
        let manifest = manifests.load(&report.target_root).unwrap();
        let up = plan_uninstall(&report.target_root, manifest).unwrap();
        assert!(up.has_external_changes);
    }
}
