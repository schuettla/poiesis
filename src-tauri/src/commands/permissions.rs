//! Permission whitelist + activity-log commands (Phase 4, §6.1).

use tauri::State;

use crate::db::{ActivityEntry, Db, Grant};
use crate::NexusError;

type Cmd<T> = Result<T, NexusError>;

fn err<E: std::fmt::Display>(e: E) -> NexusError {
    NexusError::Message(e.to_string())
}

/// Granted folders, listed + revocable in Settings (§5.4.4).
#[tauri::command]
pub fn list_permissions_cmd(db: State<'_, Db>) -> Cmd<Vec<Grant>> {
    db.list_permissions().map_err(err)
}

/// Manually grant a folder (e.g. picked from a folder dialog). `mode` is
/// "read" or "read-write".
#[tauri::command]
pub fn add_permission_cmd(db: State<'_, Db>, path: String, mode: String) -> Cmd<Grant> {
    db.add_permission(&path, &mode).map_err(err)
}

#[tauri::command]
pub fn revoke_permission_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    db.delete_permission(&id).map_err(err)
}

/// The visible activity log of what the agent did (§6.1, §6.3).
#[tauri::command]
pub fn list_activity_cmd(db: State<'_, Db>, limit: Option<i64>) -> Cmd<Vec<ActivityEntry>> {
    db.list_activity(limit.unwrap_or(100)).map_err(err)
}
