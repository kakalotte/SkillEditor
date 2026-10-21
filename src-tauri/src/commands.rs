//! Tauri commands.
//!
//! These wrappers do only three things: lock the state, show a native dialog
//! when a picker is needed, and map errors to readable strings. All behaviour
//! lives in [`crate::app`] so it can be tested without a window.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::app::{self, Core};
use crate::dto::{
    EditAck, FrontmatterGuide, InstallPreview, SettingsPatch, Snapshot, UninstallPreview,
};
use crate::shell;

/// The managed Tauri state.
pub struct AppState(Mutex<Core>);

impl AppState {
    pub fn new() -> Self {
        AppState(Mutex::new(Core::from_environment()))
    }

    /// True when the open document has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.0
            .lock()
            .map(|core| app::is_dirty(&core))
            .unwrap_or(false)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Run `f` with the locked core state.
fn with_core<T>(state: &State<'_, AppState>, f: impl FnOnce(&mut Core) -> T) -> Result<T, String> {
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "内部状态已损坏，请重启 SkillForge。".to_string())?;
    Ok(f(&mut guard))
}

// ---------------------------------------------------------------------------
// Boot and skills list
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn boot(state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, app::boot)
}

#[tauri::command]
pub fn refresh_skills(state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, app::refresh_skills)
}

#[tauri::command]
pub fn set_search(text: String, state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, |core| app::set_search(core, &text))
}

#[tauri::command]
pub fn new_skill(state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, app::new_skill)
}

#[tauri::command]
pub fn open_skill(id: String, state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, |core| app::open_skill(core, &id))
}

#[tauri::command]
pub fn delete_skill(
    id: String,
    confirm: bool,
    state: State<'_, AppState>,
) -> Result<Snapshot, String> {
    with_core(&state, |core| app::delete_skill(core, &id, confirm))
}

#[tauri::command]
pub fn reveal_skill(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let root = with_core(&state, |core| app::skill_root(core, &id))??;
    shell::reveal_in_file_manager(&root)
}

// ---------------------------------------------------------------------------
// Editor
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn set_name(value: String, state: State<'_, AppState>) -> Result<EditAck, String> {
    with_core(&state, |core| app::set_name(core, &value))
}

#[tauri::command]
pub fn set_description(value: String, state: State<'_, AppState>) -> Result<EditAck, String> {
    with_core(&state, |core| app::set_description(core, &value))
}

#[tauri::command]
pub fn set_body(value: String, state: State<'_, AppState>) -> Result<EditAck, String> {
    with_core(&state, |core| app::set_body(core, &value))
}

#[tauri::command]
pub fn preview_text(state: State<'_, AppState>) -> Result<String, String> {
    with_core(&state, |core| app::preview_text(core))?
}

/// Read-only frontmatter reference for the editor guide.
#[tauri::command]
pub fn frontmatter_guide(state: State<'_, AppState>) -> Result<FrontmatterGuide, String> {
    with_core(&state, |core| app::frontmatter_guide(core))
}

#[tauri::command]
pub fn save(state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, app::save)
}

#[tauri::command]
pub fn validate_now(state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, app::validate_now)
}

// ---------------------------------------------------------------------------
// Import / export / save-as (native pickers)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn import_skill(app: AppHandle, state: State<'_, AppState>) -> Result<Snapshot, String> {
    let picked = pick_import_path(&app);
    with_core(&state, |core| match picked {
        Some(path) => app::import_from(core, &path),
        None => app::snapshot(core),
    })
}

#[tauri::command]
pub async fn export_skill(app: AppHandle, state: State<'_, AppState>) -> Result<Snapshot, String> {
    let default_name = with_core(&state, |core| app::export_default_name(core))?;
    let picked = pick_save_file(&app, "导出为 ZIP", &default_name);
    with_core(&state, |core| match picked {
        Some(file) => app::export_to(core, &file),
        None => app::snapshot(core),
    })
}

#[tauri::command]
pub async fn save_skill_as(app: AppHandle, state: State<'_, AppState>) -> Result<Snapshot, String> {
    let picked = pick_folder(&app, "选择要另存到的目录");
    with_core(&state, |core| match picked {
        Some(parent) => app::save_as(core, &parent),
        None => app::snapshot(core),
    })
}

#[tauri::command]
pub async fn choose_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Snapshot, String> {
    let picked = pick_folder(&app, "选择默认工作区目录");
    with_core(&state, |core| match picked {
        Some(dir) => app::set_workspace(core, &dir),
        None => app::snapshot(core),
    })
}

// ---------------------------------------------------------------------------
// Platforms and installation
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn refresh_platforms(state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, app::refresh_platforms)
}

#[tauri::command]
pub fn preview_install(
    platform_id: String,
    scope: String,
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<InstallPreview, String> {
    with_core(&state, |core| {
        app::preview_install(core, &platform_id, &scope, path.as_deref())
    })?
}

#[tauri::command]
pub fn confirm_install(state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, app::confirm_install)
}

#[tauri::command]
pub fn cancel_install(state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, app::cancel_install)
}

#[tauri::command]
pub fn preview_uninstall(
    platform_id: String,
    scope: String,
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<UninstallPreview, String> {
    with_core(&state, |core| {
        app::preview_uninstall(core, &platform_id, &scope, path.as_deref())
    })?
}

#[tauri::command]
pub fn uninstall(
    platform_id: String,
    scope: String,
    path: Option<String>,
    confirm: bool,
    state: State<'_, AppState>,
) -> Result<Snapshot, String> {
    with_core(&state, |core| {
        app::uninstall(core, &platform_id, &scope, path.as_deref(), confirm)
    })
}

#[tauri::command]
pub fn open_target(path: String) -> Result<(), String> {
    shell::reveal_in_file_manager(std::path::Path::new(&path))
}

// ---------------------------------------------------------------------------
// Settings and shell
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn update_settings(
    patch: SettingsPatch,
    state: State<'_, AppState>,
) -> Result<Snapshot, String> {
    with_core(&state, |core| app::update_settings(core, &patch))
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>) -> Result<Snapshot, String> {
    with_core(&state, app::save_settings)
}

#[tauri::command]
pub fn open_config_dir(state: State<'_, AppState>) -> Result<(), String> {
    let root = with_core(&state, |core| app::config_root(core))?;
    shell::reveal_in_file_manager(&root)
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    shell::open_url(&url)
}

/// Write a front-end diagnostic line to stderr.
///
/// The desktop shell deliberately has no logging framework (see
/// docs/decisions/0005-diagnostics.md). This command is how a JavaScript-side
/// failure becomes visible when the app is started from a terminal, which is
/// also the only practical way to diagnose a broken render in a packaged build.
#[tauri::command]
pub fn log_frontend(level: String, message: String) {
    let label = match level.as_str() {
        "error" => "错误",
        "warn" => "警告",
        _ => "信息",
    };
    eprintln!("[前端/{label}] {message}");
}

// ---------------------------------------------------------------------------
// Native dialog helpers
//
// The blocking variants are safe here because async commands run on the async
// worker pool, never on the main thread (the plugin dispatches the panel to the
// main thread itself).
// ---------------------------------------------------------------------------

fn pick_folder(app: &AppHandle, title: &str) -> Option<PathBuf> {
    app.dialog()
        .file()
        .set_title(title)
        .blocking_pick_folder()
        .and_then(|path| path.into_path().ok())
}

/// Pick a skill directory, a `SKILL.md` or a `.zip`.
fn pick_import_path(app: &AppHandle) -> Option<PathBuf> {
    if let Some(dir) = pick_folder(app, "导入 Skill（目录 / SKILL.md / .zip）") {
        return Some(dir);
    }
    app.dialog()
        .file()
        .set_title("导入 Skill（SKILL.md 或 .zip）")
        .add_filter("Skill 或 ZIP", &["md", "zip"])
        .blocking_pick_file()
        .and_then(|path| path.into_path().ok())
}

fn pick_save_file(app: &AppHandle, title: &str, default_name: &str) -> Option<PathBuf> {
    app.dialog()
        .file()
        .set_title(title)
        .set_file_name(default_name)
        .add_filter("ZIP", &["zip"])
        .blocking_save_file()
        .and_then(|path| path.into_path().ok())
}
