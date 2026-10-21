//! Backups of install targets.
//!
//! Before an install overwrites an existing target we copy the current target
//! into a timestamped backup folder. A configurable number of recent backups
//! is retained per skill.

use std::path::{Path, PathBuf};

use crate::errors::{Error, Result};
use crate::services::fs_util;

#[derive(Debug, Clone)]
pub struct BackupService {
    root: PathBuf,
    keep: usize,
}

impl BackupService {
    pub fn new(root: impl Into<PathBuf>, keep: usize) -> Self {
        BackupService {
            root: root.into(),
            keep: keep.max(1),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn set_keep(&mut self, keep: usize) {
        self.keep = keep.max(1);
    }

    /// Back up `target_dir` under `<root>/<skill_name>/<timestamp>`. Returns the
    /// backup directory.
    pub fn backup_target(
        &self,
        target_dir: &Path,
        skill_name: &str,
        timestamp: u64,
    ) -> Result<PathBuf> {
        if !target_dir.exists() {
            return Err(Error::Install(format!(
                "备份源不存在: {}",
                target_dir.display()
            )));
        }
        let safe_name = sanitize_segment(skill_name);
        let dest = self.root.join(&safe_name).join(timestamp.to_string());
        if dest.exists() {
            // Extremely unlikely collision; make it unique.
            let unique = self.root.join(&safe_name).join(format!("{timestamp}-1"));
            fs_util::copy_dir_contents(target_dir, &unique)?;
            self.prune(&safe_name)?;
            return Ok(unique);
        }
        fs_util::copy_dir_contents(target_dir, &dest)?;
        self.prune(&safe_name)?;
        Ok(dest)
    }

    /// List backup directories for a skill, newest first.
    pub fn list_backups(&self, skill_name: &str) -> Result<Vec<PathBuf>> {
        let dir = self.root.join(sanitize_segment(skill_name));
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| Error::io(&dir, e))? {
            let entry = entry.map_err(|e| Error::io(&dir, e))?;
            if entry.path().is_dir() {
                out.push(entry.path());
            }
        }
        out.sort();
        out.reverse();
        Ok(out)
    }

    /// Keep only the most recent `keep` backups for a skill.
    pub fn prune(&self, skill_name: &str) -> Result<()> {
        let backups = self.list_backups(skill_name)?;
        for old in backups.into_iter().skip(self.keep) {
            fs_util::remove_dir_all_safe(&old)?;
        }
        Ok(())
    }
}

/// Make a filesystem-safe single path segment out of a skill name.
fn sanitize_segment(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "skill".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backs_up_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("SKILL.md"), "v1").unwrap();

        let svc = BackupService::new(dir.path().join("backups"), 2);
        let b1 = svc.backup_target(&target, "demo", 1000).unwrap();
        assert!(b1.join("SKILL.md").is_file());

        // Three more backups -> only the newest two remain.
        for ts in [1001u64, 1002, 1003] {
            std::fs::write(target.join("SKILL.md"), ts.to_string()).unwrap();
            svc.backup_target(&target, "demo", ts).unwrap();
        }
        let backups = svc.list_backups("demo").unwrap();
        assert_eq!(backups.len(), 2);
        assert!(backups[0].ends_with("1003"));
        assert!(backups[1].ends_with("1002"));
    }

    #[test]
    fn missing_target_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let svc = BackupService::new(dir.path().join("backups"), 2);
        assert!(svc
            .backup_target(&dir.path().join("nope"), "demo", 1)
            .is_err());
    }
}
