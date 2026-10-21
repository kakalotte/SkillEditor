//! SkillForge desktop entry point (Tauri shell).
//!
//! All business logic lives in the `skillforge-core` crate. This binary owns
//! the application state, the Tauri commands and the window.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod commands;
mod dto;
mod shell;

use tauri::{Emitter, Manager, WindowEvent};

use commands::AppState;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::boot,
            commands::refresh_skills,
            commands::set_search,
            commands::new_skill,
            commands::open_skill,
            commands::delete_skill,
            commands::reveal_skill,
            commands::set_name,
            commands::set_description,
            commands::set_body,
            commands::preview_text,
            commands::frontmatter_guide,
            commands::save,
            commands::save_skill_as,
            commands::export_skill,
            commands::import_skill,
            commands::validate_now,
            commands::refresh_platforms,
            commands::preview_install,
            commands::confirm_install,
            commands::cancel_install,
            commands::preview_uninstall,
            commands::uninstall,
            commands::open_target,
            commands::choose_workspace,
            commands::update_settings,
            commands::save_settings,
            commands::open_config_dir,
            commands::open_url,
            commands::log_frontend,
        ])
        // Unsaved edits block the window from closing; the front end shows the
        // reason.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app_state = window.state::<AppState>();
                if app_state.is_dirty() {
                    api.prevent_close();
                    let _ = window.emit("skillforge://unsaved-edits", ());
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("无法启动 SkillForge");
}
