//! Workspace: listing, loading, saving and resource scanning for skills on
//! disk.
//!
//! The workspace is a plain directory of skill folders. It is never the only
//! way to use a skill: any directory containing a `SKILL.md` is valid.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::path_safety::to_slash;
use crate::domain::resource::{self, ResourceKind, SkillResource};
use crate::domain::skill::{SkillDocument, SkillId};
use crate::errors::{Error, Result};
use crate::services::fs_util;

/// A lightweight entry for the skills list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEntry {
    pub root: PathBuf,
    pub name: String,
    pub description: String,
    pub modified: SystemTime,
    pub parse_ok: bool,
}

impl SkillEntry {
    pub fn id(&self) -> String {
        to_slash(&self.root)
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceService {
    workspace: PathBuf,
    next_draft_seq: u64,
}

impl WorkspaceService {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        WorkspaceService {
            workspace: workspace.into(),
            next_draft_seq: 1,
        }
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn set_workspace(&mut self, workspace: impl Into<PathBuf>) {
        self.workspace = workspace.into();
    }

    /// Create the workspace directory if it does not exist.
    pub fn ensure_workspace(&self) -> Result<()> {
        if !self.workspace.exists() {
            std::fs::create_dir_all(&self.workspace).map_err(|e| Error::io(&self.workspace, e))?;
        }
        Ok(())
    }

    /// List skill folders in the workspace. Unreadable folders are skipped
    /// rather than failing the whole listing.
    pub fn list_entries(&self) -> Result<Vec<SkillEntry>> {
        let mut out = Vec::new();
        let read = match std::fs::read_dir(&self.workspace) {
            Ok(read) => read,
            Err(err) if fs_util::is_not_found(&err) => return Ok(out),
            Err(err) => return Err(Error::io(&self.workspace, err)),
        };
        for entry in read {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let root = entry.path();
            if !root.is_dir() {
                continue;
            }
            let skill_md = root.join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }
            let content = match std::fs::read_to_string(&skill_md) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let doc = SkillDocument::from_skill_md(root.clone(), &content);
            let name = {
                let n = doc.name();
                if n.is_empty() {
                    doc.dir_name().unwrap_or_else(|| "未命名".to_string())
                } else {
                    n
                }
            };
            let modified = file_mtime(&skill_md).unwrap_or(UNIX_EPOCH);
            out.push(SkillEntry {
                root,
                name,
                description: doc.description(),
                modified,
                parse_ok: doc.state.parse_ok,
            });
        }
        out.sort_by_key(|a| a.name.to_lowercase());
        Ok(out)
    }

    /// Load a skill from its root directory (reads `SKILL.md` and scans
    /// resources).
    pub fn load(&self, root: &Path) -> Result<SkillDocument> {
        let skill_md = root.join("SKILL.md");
        let content = std::fs::read_to_string(&skill_md).map_err(|e| {
            if fs_util::is_not_found(&e) {
                Error::Workspace(format!("目录中没有 SKILL.md: {}", root.display()))
            } else {
                Error::io(&skill_md, e)
            }
        })?;
        let mut doc = SkillDocument::from_skill_md(root.to_path_buf(), &content);
        doc.resources = scan_resources(root)?;
        Ok(doc)
    }

    /// Create an in-memory draft whose root is `<workspace>/<name>`.
    pub fn create_draft(&mut self, name: &str, description: &str) -> SkillDocument {
        let seq = self.next_draft_seq;
        self.next_draft_seq += 1;
        let root = if name.is_empty() {
            self.workspace.join(format!("new-skill-{seq}"))
        } else {
            self.workspace.join(name)
        };
        SkillDocument::new_draft(SkillId::draft(seq), root, name, description)
    }

    /// Write the document's `SKILL.md` atomically and refresh its resource
    /// list. Only `SKILL.md` is written; resource files are never touched.
    pub fn save(&self, doc: &mut SkillDocument) -> Result<()> {
        std::fs::create_dir_all(&doc.root).map_err(|e| Error::io(&doc.root, e))?;
        let content = doc.to_skill_md()?;
        let skill_md = doc.root.join("SKILL.md");
        fs_util::atomic_write(&skill_md, content.as_bytes())?;
        doc.resources = scan_resources(&doc.root)?;
        doc.state.dirty = false;
        doc.state.parse_ok = true;
        Ok(())
    }

    /// Save the document under a new root directory.
    pub fn save_as(&self, doc: &SkillDocument, new_root: &Path) -> Result<SkillDocument> {
        std::fs::create_dir_all(new_root).map_err(|e| Error::io(new_root, e))?;
        let content = doc.to_skill_md()?;
        fs_util::atomic_write(&new_root.join("SKILL.md"), content.as_bytes())?;
        let mut new_doc = SkillDocument::from_skill_md(new_root.to_path_buf(), &content);
        new_doc.resources = scan_resources(new_root)?;
        Ok(new_doc)
    }

    /// Delete a skill directory after the caller has obtained confirmation.
    /// Refuses obviously dangerous paths.
    pub fn delete(&self, root: &Path) -> Result<()> {
        if !root.exists() {
            return Err(Error::Workspace(format!("目录不存在: {}", root.display())));
        }
        if root.parent().is_none() {
            return Err(Error::Workspace("拒绝删除文件系统根目录".to_string()));
        }
        if let Some(home) = dirs::home_dir() {
            if root == home {
                return Err(Error::Workspace("拒绝删除用户主目录".to_string()));
            }
        }
        fs_util::remove_dir_all_safe(root)
    }

    /// Copy another directory into the workspace as a skill.
    pub fn import_into_workspace(&self, source_root: &Path) -> Result<PathBuf> {
        self.ensure_workspace()?;
        let name = source_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "imported-skill".to_string());
        let dest = unique_destination(&self.workspace, &name);
        fs_util::copy_dir_contents(source_root, &dest)?;
        Ok(dest)
    }
}

/// Scan a skill root for resource files (everything except `SKILL.md`).
pub fn scan_resources(root: &Path) -> Result<Vec<SkillResource>> {
    let mut out = Vec::new();
    let entries = match fs_util::walk(root) {
        Ok(entries) => entries,
        Err(err) if fs_util::is_not_found(&std::io::Error::other(err.to_string())) => {
            return Ok(out)
        }
        Err(err) => return Err(err),
    };
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let rel = &entry.relative_path;
        if rel == Path::new("SKILL.md") {
            continue;
        }
        if entry.is_symlink {
            // List the symlink but never load its contents.
            out.push(SkillResource::new(rel.clone(), ResourceKind::Unknown, 0));
            continue;
        }
        let kind = ResourceKind::from_path(rel);
        let abs = root.join(rel);
        let size = entry.size;
        let mut resource = SkillResource::new(rel.clone(), kind, size);
        resource.content_state = resource::load_state(&abs, kind, size);
        out.push(resource);
    }
    Ok(out)
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

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Current UNIX time in seconds.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::skill_md;

    fn write_skill(root: &Path, content: &str) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn lists_and_loads_skills() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        write_skill(
            &ws.join("alpha"),
            "---\nname: alpha\ndescription: first\n---\nbody\n",
        );
        write_skill(
            &ws.join("beta"),
            "---\nname: beta\ndescription: second\n---\nbody\n",
        );
        let svc = WorkspaceService::new(&ws);
        let entries = svc.list_entries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "alpha");
        let doc = svc.load(&ws.join("alpha")).unwrap();
        assert_eq!(doc.description(), "first");
    }

    #[test]
    fn create_save_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        let mut svc = WorkspaceService::new(&ws);
        let mut doc = svc.create_draft("demo", "A demo skill");
        assert!(doc.state.dirty);
        svc.save(&mut doc).unwrap();
        assert!(!doc.state.dirty);
        let reloaded = svc.load(&doc.root).unwrap();
        assert_eq!(reloaded.name(), "demo");
        assert_eq!(reloaded.description(), "A demo skill");
    }

    #[test]
    fn save_preserves_resources_and_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        let root = ws.join("demo");
        std::fs::create_dir_all(root.join("references")).unwrap();
        std::fs::write(root.join("references/a.md"), "keep").unwrap();
        write_skill(
            &root,
            "---\nname: demo\ndescription: d\nx-extra: 7\n---\nbody\n",
        );
        let svc = WorkspaceService::new(&ws);
        let mut doc = svc.load(&root).unwrap();
        doc.set_description("updated");
        svc.save(&mut doc).unwrap();

        let out = std::fs::read_to_string(root.join("SKILL.md")).unwrap();
        let parsed = skill_md::parse(&out).unwrap();
        assert_eq!(parsed.frontmatter.get_str("x-extra").as_deref(), Some("7"));
        assert_eq!(
            parsed.frontmatter.get_str("description").as_deref(),
            Some("updated")
        );
        assert!(root.join("references/a.md").is_file());
        let res = scan_resources(&root).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(to_slash(&res[0].relative_path), "references/a.md");
    }

    #[test]
    fn import_into_workspace_renames_on_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        let external = dir.path().join("ext/demo");
        write_skill(&external, "---\nname: demo\ndescription: d\n---\nb\n");
        let svc = WorkspaceService::new(&ws);
        let dest1 = svc.import_into_workspace(&external).unwrap();
        let dest2 = svc.import_into_workspace(&external).unwrap();
        assert_ne!(dest1, dest2);
        assert!(dest1.join("SKILL.md").is_file());
        assert!(dest2.join("SKILL.md").is_file());
    }
}
