//! Path safety helpers.
//!
//! Every resource path that is used to read or write must pass through these
//! helpers. They reject absolute paths, `..` traversal, NUL bytes and any path
//! that would escape the skill root.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PathSafetyError {
    #[error("路径为空")]
    Empty,
    #[error("不允许绝对路径: {0}")]
    Absolute(String),
    #[error("路径越界（包含 `..`）: {0}")]
    Traversal(String),
    #[error("路径包含非法组件: {0}")]
    InvalidComponent(String),
    #[error("路径包含非法字符: {0}")]
    InvalidChars(String),
    #[error("路径逃逸出根目录: {0}")]
    EscapesRoot(String),
}

/// Normalize a *relative* path: resolve `.` segments, reject `..`, absolute
/// paths and empty results.
pub fn normalize_relative(rel: impl AsRef<Path>) -> Result<PathBuf, PathSafetyError> {
    let rel = rel.as_ref();
    if rel.as_os_str().is_empty() {
        return Err(PathSafetyError::Empty);
    }
    if rel.is_absolute() {
        return Err(PathSafetyError::Absolute(rel.display().to_string()));
    }

    let raw = rel.to_string_lossy();
    if raw.contains('\0') {
        return Err(PathSafetyError::InvalidChars(raw.into_owned()));
    }

    let mut out = PathBuf::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(PathSafetyError::Traversal(rel.display().to_string()))
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(PathSafetyError::Absolute(rel.display().to_string()))
            }
        }
    }

    if out.as_os_str().is_empty() {
        return Err(PathSafetyError::Empty);
    }
    Ok(out)
}

/// Join a validated relative path onto `root`, guaranteeing the result stays
/// lexically within `root`.
pub fn safe_join(
    root: impl AsRef<Path>,
    rel: impl AsRef<Path>,
) -> Result<PathBuf, PathSafetyError> {
    let root = root.as_ref();
    let rel = normalize_relative(rel)?;
    let joined = root.join(&rel);
    if !lexically_within(root, &joined) {
        return Err(PathSafetyError::EscapesRoot(joined.display().to_string()));
    }
    Ok(joined)
}

/// True when `candidate` is lexically inside `root` (no filesystem access).
pub fn lexically_within(root: &Path, candidate: &Path) -> bool {
    let root = normalized_lexical(root);
    let candidate = normalized_lexical(candidate);
    candidate.starts_with(&root)
}

/// Collapse `.` and `..` components lexically without touching the filesystem.
fn normalized_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Pop only if we can; otherwise keep it (conservative).
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve `candidate` through the filesystem and confirm it is still inside
/// `root`. Used right before writing so symlinks cannot redirect a write
/// outside the workspace. Returns `Ok(None)` when the path does not exist yet.
pub fn is_within_root_canonical(root: &Path, candidate: &Path) -> std::io::Result<bool> {
    let root = std::fs::canonicalize(root)?;
    match std::fs::canonicalize(candidate) {
        Ok(real) => Ok(real.starts_with(&root)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(err) => Err(err),
    }
}

/// Convert an OS path to a forward-slash string for stable display and hashing.
pub fn to_slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_absolute_and_traversal() {
        assert!(matches!(
            normalize_relative("/etc/passwd"),
            Err(PathSafetyError::Absolute(_))
        ));
        assert!(matches!(
            normalize_relative("../secrets"),
            Err(PathSafetyError::Traversal(_))
        ));
        assert!(matches!(
            normalize_relative("a/../../b"),
            Err(PathSafetyError::Traversal(_))
        ));
        assert!(matches!(
            normalize_relative(""),
            Err(PathSafetyError::Empty)
        ));
    }

    #[test]
    fn normalizes_dot_segments() {
        assert_eq!(
            normalize_relative("./a/./b/../c")
                .map_err(|_| ())
                .unwrap_err(),
            ()
        );
        assert_eq!(
            normalize_relative("references/./guide.md").unwrap(),
            PathBuf::from("references/guide.md")
        );
    }

    #[test]
    fn safe_join_stays_inside() {
        let root = Path::new("/tmp/skill");
        assert_eq!(
            safe_join(root, "scripts/run.sh").unwrap(),
            PathBuf::from("/tmp/skill/scripts/run.sh")
        );
        assert!(safe_join(root, "../../etc").is_err());
    }

    #[test]
    fn slash_conversion_is_stable() {
        assert_eq!(to_slash(Path::new("a/b/c.md")), "a/b/c.md");
    }
}
