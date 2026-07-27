//! Tauri command handlers — the IPC surface exposed to the React frontend.
//! Grouped by subsystem; each module is registered in `lib::run`.

pub mod agent;
pub mod attachments;
pub mod cloud;
pub mod connectors;
pub mod conversations;
pub mod files;
pub mod imagegen;
pub mod memory;
pub mod models;
pub mod permissions;
pub mod organism;
pub mod personas;
pub mod reflect;
pub mod runtime;

/// Returns the running application version. Smoke-test of the IPC bridge and a
/// real datum for Settings/About.
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
