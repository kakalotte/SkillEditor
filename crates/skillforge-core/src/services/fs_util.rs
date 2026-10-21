//! Filesystem helpers shared by the service layer.
//!
//! All writes go through `atomic_write` (temp file in the same directory +
//! rename) so a crash cannot leave a half-written file. Directory copies never
//! follow symlinks outside the skill root.

use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::domain::path_safety::PathSafetyError;
use crate::errors::{Error, Result};

/// SHA-256 of a byte slice, hex encoded.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// SHA-256 of a file's contents, hex encoded.
pub fn hash_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    Ok(hash_bytes(&bytes))
}

/// Write `bytes` to `path` atomically.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;

    let mut tmp = tempfile::Builder::new()
        .prefix(".skillforge-tmp-")
        .tempfile_in(parent)
        .map_err(|e| Error::io(parent, e))?;
    tmp.write_all(bytes).map_err(|e| Error::io(path, e))?;
    tmp.flush().map_err(|e| Error::io(path, e))?;
    tmp.as_file().sync_all().map_err(|e| Error::io(path, e))?;
    // Persist replaces the destination atomically on POSIX; on Windows it
    // fails if the destination exists, so remove it first.
    match tmp.persist(path) {
        Ok(_) => Ok(()),
        Err(_) => {
            let _ = std::fs::remove_file(path);
            // Recreate and retry once.
            let mut tmp = tempfile::Builder::new()
                .prefix(".skillforge-tmp-")
                .tempfile_in(parent)
                .map_err(|e| Error::io(parent, e))?;
            tmp.write_all(bytes).map_err(|e| Error::io(path, e))?;
            tmp.flush().map_err(|e| Error::io(path, e))?;
            tmp.persist(path)
                .map(|_| ())
                .map_err(|e| Error::io(path, e.error))
        }
    }
}

/// One entry discovered while walking a directory tree.
#[derive(Debug, Clone)]
pub struct WalkEntry {
    pub relative_path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
}

/// Recursively list the contents of `root` (excluding `root` itself), without
/// following symlinks. Sorted for deterministic output.
pub fn walk(root: &Path) -> Result<Vec<WalkEntry>> {
    let mut out = Vec::new();
    walk_inner(root, root, &mut out)?;
    out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(out)
}

fn walk_inner(root: &Path, dir: &Path, out: &mut Vec<WalkEntry>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| path.clone());
        let meta = std::fs::symlink_metadata(&path).map_err(|e| Error::io(&path, e))?;
        let is_symlink = meta.file_type().is_symlink();
        if is_symlink {
            // Record the symlink itself; we never traverse or follow it.
            out.push(WalkEntry {
                relative_path: rel,
                is_dir: false,
                is_symlink: true,
                size: 0,
            });
            continue;
        }
        if meta.is_dir() {
            out.push(WalkEntry {
                relative_path: rel.clone(),
                is_dir: true,
                is_symlink: false,
                size: 0,
            });
            walk_inner(root, &path, out)?;
        } else {
            out.push(WalkEntry {
                relative_path: rel,
                is_dir: false,
                is_symlink: false,
                size: meta.len(),
            });
        }
    }
    Ok(())
}

/// Outcome of a directory copy.
#[derive(Debug, Default, Clone)]
pub struct CopyReport {
    pub copied_files: usize,
    pub skipped_symlinks: Vec<PathBuf>,
}

/// Copy the *contents* of `src` into `dst`. Symlinks are skipped (never
/// followed) and reported.
pub fn copy_dir_contents(src: &Path, dst: &Path) -> Result<CopyReport> {
    let mut report = CopyReport::default();
    std::fs::create_dir_all(dst).map_err(|e| Error::io(dst, e))?;
    let entries = std::fs::read_dir(src).map_err(|e| Error::io(src, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(src, e))?;
        let path = entry.path();
        let name = entry.file_name();
        let dest = dst.join(&name);
        let meta = std::fs::symlink_metadata(&path).map_err(|e| Error::io(&path, e))?;
        if meta.file_type().is_symlink() {
            report.skipped_symlinks.push(PathBuf::from(&name));
            continue;
        }
        if meta.is_dir() {
            let child = copy_dir_contents(&path, &dest)?;
            report.copied_files += child.copied_files;
            report.skipped_symlinks.extend(
                child
                    .skipped_symlinks
                    .into_iter()
                    .map(|p| PathBuf::from(&name).join(p)),
            );
        } else {
            std::fs::copy(&path, &dest).map_err(|e| Error::io(&dest, e))?;
            report.copied_files += 1;
        }
    }
    Ok(report)
}

/// Remove a directory tree, refusing to follow symlinks.
pub fn remove_dir_all_safe(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(dir).map_err(|e| Error::io(dir, e))?;
    if meta.file_type().is_symlink() {
        return std::fs::remove_file(dir).map_err(|e| Error::io(dir, e));
    }
    std::fs::remove_dir_all(dir).map_err(|e| Error::io(dir, e))
}

/// True when `err` reports a missing path.
pub fn is_not_found(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::NotFound
}

/// Validate that a relative path is safe, mapping the error into `Error`.
pub fn require_safe(rel: &Path) -> Result<PathBuf> {
    crate::domain::path_safety::normalize_relative(rel)
        .map_err(|e: PathSafetyError| Error::UnsafePath(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        atomic_write(&file, b"one").unwrap();
        atomic_write(&file, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "two");
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(hash_bytes(b"abc"), hash_bytes(b"abc"));
        assert_ne!(hash_bytes(b"abc"), hash_bytes(b"abd"));
    }

    #[test]
    fn copy_and_walk() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("references")).unwrap();
        std::fs::write(src.join("SKILL.md"), "x").unwrap();
        std::fs::write(src.join("references/a.md"), "y").unwrap();

        let dst = dir.path().join("dst");
        let report = copy_dir_contents(&src, &dst).unwrap();
        assert_eq!(report.copied_files, 2);
        assert!(dst.join("references/a.md").is_file());

        let walked = walk(&src).unwrap();
        let names: Vec<String> = walked
            .iter()
            .map(|e| crate::domain::path_safety::to_slash(&e.relative_path))
            .collect();
        assert!(names.contains(&"SKILL.md".to_string()));
        assert!(names.contains(&"references".to_string()));
        assert!(names.contains(&"references/a.md".to_string()));
    }
}
