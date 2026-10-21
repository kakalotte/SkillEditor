//! Application services: everything that touches the filesystem or the
//! platform environment. The UI only talks to these through `app_state`.

pub mod backup_service;
pub mod fs_util;
pub mod import_export_service;
pub mod install_service;
pub mod settings_service;
pub mod validation_service;
pub mod workspace_service;

pub use backup_service::BackupService;
pub use import_export_service::{export_zip, import_zip};
pub use install_service::{
    ChangeKind, InstallPlan, InstallReport, Manifest, ManifestStore, PlanOptions,
};
pub use settings_service::SettingsService;
pub use validation_service::{validate, validate_with_context};
pub use workspace_service::{now_secs, scan_resources, SkillEntry, WorkspaceService};
