//! Skill resources: files and folders bundled next to `SKILL.md`.
//!
//! Resources are never deleted just because the app cannot classify them.
//! Binary and oversized files are listed but not loaded into memory.

use std::path::{Path, PathBuf};

/// Upper bound for reading a text resource into memory (1 MiB).
pub const MAX_TEXT_PREVIEW: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Markdown,
    Text,
    Script,
    Image,
    Binary,
    Unknown,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ResourceKind::Markdown => "markdown",
            ResourceKind::Text => "text",
            ResourceKind::Script => "script",
            ResourceKind::Image => "image",
            ResourceKind::Binary => "binary",
            ResourceKind::Unknown => "unknown",
        }
    }

    /// Classify a path by extension only (content sniffing happens separately).
    pub fn from_path(path: &Path) -> ResourceKind {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "md" | "markdown" => ResourceKind::Markdown,
            "txt" | "text" | "rst" => ResourceKind::Text,
            "sh" | "bash" | "zsh" | "fish" | "py" | "rb" | "js" | "mjs" | "cjs" | "ts" | "ps1"
            | "bat" | "cmd" => ResourceKind::Script,
            "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico" => ResourceKind::Image,
            "" => ResourceKind::Unknown,
            _ => ResourceKind::Binary,
        }
    }
}

/// How much of a resource we hold in memory. Most resources stay unloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceContentState {
    NotLoaded,
    Text(String),
    Binary,
    TooLarge,
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillResource {
    pub relative_path: PathBuf,
    pub kind: ResourceKind,
    pub size: u64,
    pub content_state: ResourceContentState,
}

impl SkillResource {
    pub fn new(relative_path: PathBuf, kind: ResourceKind, size: u64) -> Self {
        SkillResource {
            relative_path,
            kind,
            size,
            content_state: ResourceContentState::NotLoaded,
        }
    }

    pub fn is_textual(&self) -> bool {
        matches!(
            self.kind,
            ResourceKind::Markdown | ResourceKind::Text | ResourceKind::Script
        )
    }

    pub fn human_size(&self) -> String {
        human_size(self.size)
    }
}

/// Format a byte count for display.
pub fn human_size(size: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let s = size as f64;
    if s < KB {
        format!("{size} B")
    } else if s < MB {
        format!("{:.1} KB", s / KB)
    } else {
        format!("{:.1} MB", s / MB)
    }
}

/// Decide content state for a freshly discovered file, reading small text
/// files (and validating UTF-8) but leaving binary/large files untouched.
pub fn load_state(path: &Path, kind: ResourceKind, size: u64) -> ResourceContentState {
    if !matches!(
        kind,
        ResourceKind::Markdown | ResourceKind::Text | ResourceKind::Script
    ) {
        return ResourceContentState::Binary;
    }
    if size > MAX_TEXT_PREVIEW {
        return ResourceContentState::TooLarge;
    }
    match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => ResourceContentState::Text(text),
            Err(_) => ResourceContentState::Binary,
        },
        Err(err) => ResourceContentState::Unreadable(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_extension() {
        assert_eq!(
            ResourceKind::from_path(Path::new("references/guide.md")),
            ResourceKind::Markdown
        );
        assert_eq!(
            ResourceKind::from_path(Path::new("scripts/run.sh")),
            ResourceKind::Script
        );
        assert_eq!(
            ResourceKind::from_path(Path::new("assets/logo.png")),
            ResourceKind::Image
        );
        assert_eq!(
            ResourceKind::from_path(Path::new("LICENSE")),
            ResourceKind::Unknown
        );
        assert_eq!(
            ResourceKind::from_path(Path::new("data.bin")),
            ResourceKind::Binary
        );
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }
}
