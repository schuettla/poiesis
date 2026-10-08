//! Tauri command handlers — the IPC surface exposed to the React frontend.
//! Grouped by subsystem; each module is registered in `lib::run`.

pub mod agent;
pub mod attachments;
pub mod browser;
pub mod cloud;
pub mod connectors;
pub mod conversations;
pub mod embedgen;
pub mod endpoints;
pub mod files;
pub mod imagegen;
pub mod index;
pub mod mail;
pub mod media;
pub mod memory;
pub mod models;
pub mod permissions;
pub mod organism;
pub mod personas;
pub mod projects;
pub mod reflect;
pub mod rerankgen;
pub mod runtime;
pub mod scheduler;
pub mod skills;
pub mod subagents;
pub mod updater;
pub mod usage;
#[cfg(feature = "voice")]
pub mod voice;

/// Returns the running application version. Smoke-test of the IPC bridge and a
/// real datum for Settings/About. Read from `tauri.conf.json` rather than
/// Cargo.toml: that is the version the updater compares and the installer is
/// named for (`REL-4`), so there is one source of truth.
#[tauri::command]
pub fn app_version(app: tauri::AppHandle) -> String {
    updater::running_version(&app)
}

/// The three version facts a problem report carries (`PUB-3`) — nothing from
/// the user's conversations, files or settings.
#[derive(serde::Serialize)]
pub struct ProblemFacts {
    pub app_version: String,
    pub windows: String,
    pub schema_version: i64,
}

#[tauri::command]
pub fn problem_facts_cmd(app: tauri::AppHandle) -> ProblemFacts {
    let os = sysinfo::System::long_os_version().unwrap_or_else(|| "unknown".into());
    let build = sysinfo::System::kernel_version().unwrap_or_default();
    ProblemFacts {
        app_version: updater::running_version(&app),
        windows: if build.is_empty() { os } else { format!("{os} (build {build})") },
        schema_version: crate::db::SCHEMA_VERSION,
    }
}
