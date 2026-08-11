//! Browser panel + capability-grant commands (`BRW-UI-1`, `BRW-3`/`SYS-1`).

use tauri::State;

use crate::agent::browser::{BrowserPanelState, BrowserPool};
use crate::db::{CapabilityGrant, Db};
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// What this conversation is browsing, or last browsed.
///
/// A live session wins; otherwise the stored record comes back marked
/// `closed`, so re-opening a chat still shows where the agent went instead of
/// an empty panel beside a transcript full of visits. `None` only when this
/// conversation has never browsed.
#[tauri::command]
pub async fn browser_state_cmd(
    pool: State<'_, BrowserPool>,
    db: State<'_, Db>,
    conversation_id: String,
) -> Cmd<Option<BrowserPanelState>> {
    if let Some(live) = pool.snapshot(&conversation_id).await {
        return Ok(Some(live));
    }
    let Some((domain, title, screenshot, trail)) = db.browser_session(&conversation_id) else {
        return Ok(None);
    };
    Ok(Some(BrowserPanelState {
        domain,
        title,
        // The image ages out on its own (`prune_screenshots`); a path that no
        // longer resolves would render as a broken thumbnail, so it's dropped
        // here and the record stands on its trail alone.
        screenshot: screenshot.filter(|p| std::path::Path::new(p).exists()),
        trail,
        closed: true,
    }))
}

/// `BRW-UI-1`'s "Stop browsing" — drops the session and its Chrome process.
/// The stored record stays: the browsing still happened.
#[tauri::command]
pub async fn stop_browser_cmd(pool: State<'_, BrowserPool>, conversation_id: String) -> Cmd<()> {
    pool.stop(&conversation_id).await;
    Ok(())
}

/// The panel's "Dismiss" — the user is done looking at a finished session, so
/// forget it rather than showing it again on every re-open.
#[tauri::command]
pub fn forget_browser_session_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<()> {
    db.delete_browser_session(&conversation_id);
    Ok(())
}

/// Every "Always allow" answer to a domain/app consent prompt, revocable in
/// Settings like a folder grant.
#[tauri::command]
pub fn list_capability_grants_cmd(db: State<'_, Db>) -> Cmd<Vec<CapabilityGrant>> {
    db.list_capability_grants().map_err(err)
}

#[tauri::command]
pub fn revoke_capability_grant_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    db.delete_capability_grant(&id).map_err(err)
}
