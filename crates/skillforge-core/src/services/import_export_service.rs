//! Import and export of skill directories and ZIP archives.
//!
//! ZIP import is hardened against Zip Slip: every entry name is validated with
//! the path-safety helpers and bounded by entry-count and size limits.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::domain::path_safety::normalize_relative;
use crate::errors::{Error, Result};
use crate::services::fs_util;

/// ZIP import limits (defensive defaults).
pub const MAX_ZIP_ENTRIES: usize = 2_000;
pub const MAX_ZIP_TOTAL_BYTES: u64 = 200 * 1024 * 1024;
pub const MAX_ZIP_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// Copy a skill directory to a new location.
pub fn copy_skill_dir(src_root: &Path, dst_root: &Path) -> Result<fs_util::CopyReport> {
    if !src_root.is_dir() {
        return Err(Error::ImportExport(format!(
            "源目录不存在: {}",
            src_root.display()
        )));
    }
    fs_util::copy_dir_contents(src_root, dst_root)
}

/// Export a skill directory to a `.zip` archive. Returns the number of files
/// written.
pub fn export_zip(skill_root: &Path, zip_path: &Path) -> Result<usize> {
    if !skill_root.is_dir() {
        return Err(Error::ImportExport(format!(
            "Skill 目录不存在: {}",
            skill_root.display()
        )));
    }
    if let Some(parent) = zip_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let file = std::fs::File::create(zip_path).map_err(|e| Error::io(zip_path, e))?;
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut count = 0usize;
    for entry in fs_util::walk(skill_root)? {
        if entry.is_dir || entry.is_symlink {
            continue;
        }
        let rel = &entry.relative_path;
        let rel_str = crate::domain::path_safety::to_slash(rel);
        let abs = skill_root.join(rel);
        let bytes = std::fs::read(&abs).map_err(|e| Error::io(&abs, e))?;
        writer
            .start_file(rel_str, options)
            .map_err(|e| Error::ImportExport(format!("写入 ZIP 条目失败: {e}")))?;
        writer
            .write_all(&bytes)
            .map_err(|e| Error::ImportExport(format!("写入 ZIP 数据失败: {e}")))?;
        count += 1;
    }
    writer
        .finish()
        .map_err(|e| Error::ImportExport(format!("完成 ZIP 失败: {e}")))?;
    Ok(count)
}

/// Extract a `.zip` archive into `dest_parent` under a single skill folder.
///
/// The archive must contain a `SKILL.md` at its top level (optionally nested in
/// exactly one directory, which is stripped). Returns the created directory.
pub fn import_zip(zip_path: &Path, dest_parent: &Path) -> Result<PathBuf> {
    let file = std::fs::File::open(zip_path).map_err(|e| Error::io(zip_path, e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| Error::ImportExport(format!("无法读取 ZIP: {e}")))?;

    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(Error::ImportExport(format!(
            "ZIP 条目过多（{} > {MAX_ZIP_ENTRIES}）",
            archive.len()
        )));
    }

    // Read every entry into memory first (bounded), validating names.
    let mut files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| Error::ImportExport(format!("读取 ZIP 条目失败: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| Error::ImportExport("ZIP 包含不安全的路径".to_string()))?
            .to_path_buf();
        let rel = match normalize_relative(&enclosed) {
            Ok(rel) => rel,
            Err(_) => {
                return Err(Error::ImportExport(format!(
                    "ZIP 条目路径不安全: {}",
                    enclosed.display()
                )))
            }
        };
        let mut buf = Vec::new();
        entry
            .by_ref()
            .take(MAX_ZIP_FILE_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|e| Error::ImportExport(format!("解压条目失败: {e}")))?;
        if buf.len() as u64 > MAX_ZIP_FILE_BYTES {
            return Err(Error::ImportExport(format!(
                "ZIP 条目过大: {}",
                rel.display()
            )));
        }
        total += buf.len() as u64;
        if total > MAX_ZIP_TOTAL_BYTES {
            return Err(Error::ImportExport("ZIP 解压后总体积过大".to_string()));
        }
        files.push((rel, buf));
    }

    // Strip a single common top-level directory if present, so archives that
    // wrap the skill in one folder still land directly in the skill root.
    let strip = common_prefix_dir(&files);
    let name = strip
        .as_ref()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| zip_base_name(zip_path));

    // Normalize entry paths relative to the stripped root.
    let mut staged: Vec<(PathBuf, &Vec<u8>)> = Vec::new();
    for (rel, bytes) in &files {
        let rel = match &strip {
            Some(prefix) => rel
                .strip_prefix(prefix)
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|_| rel.clone()),
            None => rel.clone(),
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        staged.push((rel, bytes));
    }

    if !staged.iter().any(|(rel, _)| rel == Path::new("SKILL.md")) {
        return Err(Error::ImportExport("ZIP 中没有找到 SKILL.md".to_string()));
    }

    std::fs::create_dir_all(dest_parent).map_err(|e| Error::io(dest_parent, e))?;
    let dest_root = unique_destination(dest_parent, &name);
    std::fs::create_dir_all(&dest_root).map_err(|e| Error::io(&dest_root, e))?;

    for (rel, bytes) in &staged {
        let out = crate::domain::path_safety::safe_join(&dest_root, rel)?;
        fs_util::atomic_write(&out, bytes)?;
    }

    Ok(dest_root)
}

fn zip_base_name(zip_path: &Path) -> String {
    zip_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "imported-skill".to_string())
}

/// If every entry shares the same first path component and it is a directory,
/// return it so it can be stripped when extracting.
fn common_prefix_dir(files: &[(PathBuf, Vec<u8>)]) -> Option<PathBuf> {
    let mut prefix: Option<PathBuf> = None;
    for (rel, _) in files {
        let first = rel.components().next()?;
        if rel.components().count() < 2 {
            // A top-level file means there is no wrapping directory.
            return None;
        }
        let dir = PathBuf::from(first.as_os_str());
        match &prefix {
            None => prefix = Some(dir),
            Some(existing) if existing == &dir => {}
            Some(_) => return None,
        }
    }
    prefix
}

fn unique_destination(parent: &Path, name: &str) -> PathBuf {
    let mut candidate = parent.join(name);
    let mut n = 1;
    while candidate.exists() {
        candidate = parent.join(format!("{name}-{n}"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(root: &Path) {
        std::fs::create_dir_all(root.join("references")).unwrap();
        std::fs::write(
            root.join("SKILL.md"),
            "---\nname: demo\ndescription: d\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(root.join("references/a.md"), "ref").unwrap();
    }

    #[test]
    fn zip_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("demo");
        make_skill(&skill);
        let zip_path = dir.path().join("demo.zip");
        let count = export_zip(&skill, &zip_path).unwrap();
        assert_eq!(count, 2);

        let out = dir.path().join("out");
        let extracted = import_zip(&zip_path, &out).unwrap();
        assert!(extracted.join("SKILL.md").is_file());
        assert!(extracted.join("references/a.md").is_file());
    }

    #[test]
    fn import_zip_rejects_zip_slip() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("evil.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("../evil.txt", options).unwrap();
            writer.write_all(b"pwned").unwrap();
            writer.finish().unwrap();
        }
        let out = dir.path().join("out");
        assert!(import_zip(&zip_path, &out).is_err());
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn import_zip_without_skill_md_fails() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("nope.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("readme.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"hi").unwrap();
            writer.finish().unwrap();
        }
        assert!(import_zip(&zip_path, &dir.path().join("out")).is_err());
    }
}
