//! SkillForge core library.
//!
//! The library contains all domain logic, file formats, validation and
//! platform adapters. It has no dependency on the desktop shell (Tauri), on a
//! web view or on any GUI toolkit, so it can be unit and integration tested
//! without a display server or an event loop.
//!
//! The boundary is deliberate: the Tauri command layer in `src-tauri` owns the
//! application state and calls into these services; the JavaScript front end
//! never touches the filesystem directly.

pub mod domain;
pub mod errors;
pub mod formats;
pub mod guide;
pub mod persistence;
pub mod platforms;
pub mod services;

/// Product version, sourced from Cargo metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
