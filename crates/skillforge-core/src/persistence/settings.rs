//! The settings model and platform directory resolution.
//!
//! Settings are stored as JSON. They never contain skill bodies, API keys or
//! telemetry. If the file is corrupt, the settings service moves it aside and
//! falls back to defaults rather than deleting anything.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Theme preference. `System` follows the OS when the toolkit can report it,
/// otherwise it falls back to light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub fn as_index(self) -> i32 {
        match self {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        }
    }

    pub fn from_index(index: i32) -> Self {
        match index {
            1 => Theme::Light,
            2 => Theme::Dark,
            _ => Theme::System,
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Theme::Dark)
    }
}

/// One entry in the recent-skills list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentEntry {
    pub path: PathBuf,
    pub name: String,
    pub opened_at: u64,
}

/// User settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// Default workspace directory.
    pub workspace: PathBuf,
    pub theme: Theme,
    /// How many backups to keep per skill.
    pub backup_count: usize,
    /// Whether to check installed targets for external changes.
    pub detect_external_changes: bool,
    /// Recently opened skills, newest first.
    pub recent: Vec<RecentEntry>,
    /// User overrides for platform target directories (platform id -> dir).
    pub custom_paths: BTreeMap<String, PathBuf>,
    /// Last known project directory used for project-scoped targets.
    pub project_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            workspace: default_workspace(),
            theme: Theme::System,
            backup_count: 5,
            detect_external_changes: true,
            recent: Vec::new(),
            custom_paths: BTreeMap::new(),
            project_dir: None,
        }
    }
}

impl Settings {
    /// Record a recently opened skill, de-duplicating by path.
    pub fn touch_recent(&mut self, path: PathBuf, name: String, now: u64) {
        self.recent.retain(|r| r.path != path);
        self.recent.insert(
            0,
            RecentEntry {
                path,
                name,
                opened_at: now,
            },
        );
        self.recent.truncate(20);
    }
}

/// Root directory for configuration (`.../SkillForge`).
pub fn config_root() -> PathBuf {
    dirs::config_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SkillForge")
}

/// Root directory for application data (backups, manifests, logs).
pub fn data_root() -> PathBuf {
    dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SkillForge")
}

/// Default workspace directory for new skills.
pub fn default_workspace() -> PathBuf {
    data_root().join("workspace")
}

/// Default log directory.
pub fn default_log_dir() -> PathBuf {
    data_root().join("logs")
}

/// Default directory for install backups.
pub fn default_backup_dir() -> PathBuf {
    data_root().join("backups")
}

/// Default directory for install manifests.
pub fn default_manifest_dir() -> PathBuf {
    data_root().join("installs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_index_round_trips() {
        for theme in [Theme::System, Theme::Light, Theme::Dark] {
            assert_eq!(Theme::from_index(theme.as_index()), theme);
        }
        assert_eq!(Theme::from_index(99), Theme::System);
    }

    #[test]
    fn recent_is_deduped_and_capped() {
        let mut s = Settings::default();
        s.touch_recent(PathBuf::from("/a"), "a".into(), 1);
        s.touch_recent(PathBuf::from("/b"), "b".into(), 2);
        s.touch_recent(PathBuf::from("/a"), "a".into(), 3);
        assert_eq!(s.recent.len(), 2);
        assert_eq!(s.recent[0].path, PathBuf::from("/a"));
    }

    #[test]
    fn settings_json_round_trips() {
        let mut s = Settings {
            theme: Theme::Dark,
            ..Default::default()
        };
        s.custom_paths
            .insert("generic".into(), PathBuf::from("/tmp/skills"));
        let text = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(s, back);
    }
}
