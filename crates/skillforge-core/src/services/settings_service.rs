//! Load and save settings, with corruption recovery.
//!
//! On a corrupt settings file the original is renamed to
//! `config.json.corrupt-<timestamp>` and defaults are returned. Skill files are
//! never touched by this service.

use std::path::{Path, PathBuf};

use crate::errors::{Error, Result};
use crate::persistence::settings::{self, Settings};
use crate::services::fs_util;

#[derive(Debug, Clone)]
pub struct LoadedSettings {
    pub settings: Settings,
    /// Human-readable warnings (e.g. recovered from a corrupt file).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SettingsService {
    config_root: PathBuf,
    data_root: PathBuf,
}

impl Default for SettingsService {
    fn default() -> Self {
        Self::new(settings::config_root(), settings::data_root())
    }
}

impl SettingsService {
    pub fn new(config_root: impl Into<PathBuf>, data_root: impl Into<PathBuf>) -> Self {
        SettingsService {
            config_root: config_root.into(),
            data_root: data_root.into(),
        }
    }

    pub fn config_root(&self) -> &Path {
        &self.config_root
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_root.join("config.json")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_root.join("logs")
    }

    pub fn backup_dir(&self) -> PathBuf {
        self.data_root.join("backups")
    }

    pub fn manifest_dir(&self) -> PathBuf {
        self.data_root.join("installs")
    }

    /// Load settings, recovering from a missing or corrupt file.
    pub fn load(&self) -> LoadedSettings {
        let path = self.config_path();
        let mut warnings = Vec::new();
        let settings = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(mut s) => {
                    if s.workspace.as_os_str().is_empty() {
                        s.workspace = settings::default_workspace();
                    }
                    s
                }
                Err(err) => {
                    warnings.push(format!("配置文件损坏，已恢复默认设置: {err}"));
                    self.quarantine(&path);
                    Settings::default()
                }
            },
            Err(err) if fs_util::is_not_found(&err) => Settings::default(),
            Err(err) => {
                warnings.push(format!("无法读取配置，使用默认设置: {err}"));
                Settings::default()
            }
        };
        LoadedSettings { settings, warnings }
    }

    /// Persist settings atomically.
    pub fn save(&self, settings: &Settings) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(settings)
            .map_err(|e| Error::Settings(format!("序列化设置失败: {e}")))?;
        fs_util::atomic_write(&self.config_path(), &bytes)
    }

    /// Move a corrupt config aside so the user can inspect it later.
    fn quarantine(&self, path: &Path) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let dest = path.with_extension(format!("json.corrupt-{ts}"));
        let _ = std::fs::rename(path, dest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_settings() {
        let dir = tempfile::tempdir().unwrap();
        let svc = SettingsService::new(dir.path().join("cfg"), dir.path().join("data"));
        let mut s = svc.load().settings;
        s.backup_count = 9;
        svc.save(&s).unwrap();
        let loaded = svc.load();
        assert_eq!(loaded.settings.backup_count, 9);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn corrupt_config_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let svc = SettingsService::new(dir.path().join("cfg"), dir.path().join("data"));
        std::fs::create_dir_all(svc.config_root()).unwrap();
        std::fs::write(svc.config_path(), b"{ not json").unwrap();
        let loaded = svc.load();
        assert_eq!(loaded.settings, Settings::default());
        assert!(!loaded.warnings.is_empty());
        // Original preserved under a .corrupt-* name.
        let recovered = std::fs::read_dir(svc.config_root())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(recovered);
    }

    #[test]
    fn missing_config_uses_defaults_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let svc = SettingsService::new(dir.path().join("cfg"), dir.path().join("data"));
        let loaded = svc.load();
        assert!(loaded.warnings.is_empty());
    }
}
