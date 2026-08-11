//! The organism made visible (ORG-1) — one read-only snapshot of how Poiesis
//! is doing, plus the Health-tab actions that go with it.
//!
//! Deliberately counts and words, never gauges or green/red: this is a page
//! about a self, not a server dashboard.

use serde::Serialize;
use tauri::State;

use crate::db::{Db, ToolStatRow};
use crate::memory::MemoryStore;
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

/// Everything the Self view's vitality strip and Health tab need, in one call.
#[derive(Debug, Serialize)]
pub struct Vitality {
    pub facts: usize,
    pub lessons: usize,
    /// Agent Skills currently discoverable and switched on (`SKL-5`).
    pub skills: usize,
    /// How many times a skill has been read this install — the same honesty
    /// the recipe use count carried, now sourced from the activity log.
    pub skill_uses: u32,
    pub quarantined: Vec<String>,
    /// Engine restarts the watchdog performed since launch (HEAL-1).
    pub engine_restarts_session: u32,
    pub pending_proposals: usize,
    pub last_reflection: Option<i64>,
    /// 7-day per-tool reliability for the model in play (HEAL-2).
    pub tool_health: Vec<ToolStatRow>,
}

#[tauri::command]
pub async fn get_vitality_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    mgr: State<'_, RuntimeManager>,
    model_name: Option<String>,
) -> Result<Vitality, PoiesisError> {
    // Reading the Health tab is also the moment to notice damaged files.
    mem.quarantine_scan(&db);

    let skills = crate::agent::skillpack::discover(mgr.app_data_dir(), None);
    let skills_on = skills.iter().filter(|p| crate::agent::skillpack::is_enabled(&db, p)).count();
    let model = match model_name {
        Some(m) if !m.trim().is_empty() => Some(m),
        _ => mgr.engine_model_name().await,
    };
    let tool_health = model
        .and_then(|m| db.tool_health(&m, 7).ok())
        .unwrap_or_default();

    Ok(Vitality {
        facts: mem.list().len(),
        lessons: mem.list_lessons().len(),
        skills: skills_on,
        skill_uses: db.count_activity("skill").unwrap_or(0),
        quarantined: mem.quarantined(),
        engine_restarts_session: mgr.restarts_session(),
        pending_proposals: db.pending_proposal_count().unwrap_or(0),
        last_reflection: db.last_reflection().ok().flatten(),
        tool_health,
    })
}

/// The Health tab's Golden section (`GLD-UI-1`): the last recorded run,
/// without running a fresh one — matches how the rest of the tab is read
/// passively on open.
#[tauri::command]
pub fn get_golden_status_cmd(db: State<'_, Db>) -> Option<crate::agent::golden::GoldenStatus> {
    crate::agent::golden::load_status(&db)
}

/// "Check me now" (`GLD-UI-1`): always runs a fresh pass over the golden set,
/// independent of `golden.enabled` — that setting only gates the automatic
/// guard around a self-change, never the user's own button.
#[tauri::command]
pub async fn check_golden_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    mgr: State<'_, RuntimeManager>,
    target: Option<crate::commands::agent::ChatTarget>,
) -> Result<crate::agent::golden::GoldenStatus, PoiesisError> {
    crate::agent::golden::check_now(&mgr.client, &mgr, &db, &mem, target.as_ref())
        .await
        .map_err(PoiesisError::Message)
}

/// 7-day per-tool reliability for one model (HEAL-2). Separate from `Vitality`
/// because the frontend refreshes it on every model change and after every turn
/// that used a tool — this touches one indexed table and nothing on disk.
///
/// `model_name` is omitted for local models, whose name only the manager knows.
#[tauri::command]
pub async fn get_tool_health_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    model_name: Option<String>,
) -> Result<Vec<ToolStatRow>, PoiesisError> {
    let model = match model_name {
        Some(m) if !m.trim().is_empty() => Some(m),
        _ => mgr.engine_model_name().await,
    };
    Ok(model
        .and_then(|m| db.tool_health(&m, 7).ok())
        .unwrap_or_default())
}

/// Put a quarantined file back (HEAL-3) — the user has presumably repaired it.
#[tauri::command]
pub fn restore_quarantined_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    file: String,
) -> Result<(), PoiesisError> {
    mem.restore_quarantined(&db, &file).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "heal", &format!("restored {file}"));
    Ok(())
}

#[tauri::command]
pub fn delete_quarantined_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    file: String,
) -> Result<(), PoiesisError> {
    mem.delete_quarantined(&file).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "heal", &format!("discarded {file}"));
    Ok(())
}

/// Seed a conversation's workspace surface from a skill's bundled
/// `assets/surface.json` (`SKL-5`, carrying `RCP-UI-2` forward). Writes the
/// reserved surface row exactly the way `render_ui` does, so the template
/// renders through the ordinary surface path with nothing special about
/// having come from a skill.
#[tauri::command]
pub fn set_surface_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    tree_json: String,
) -> Result<String, PoiesisError> {
    serde_json::from_str::<serde_json::Value>(&tree_json)
        .map_err(|e| PoiesisError::Message(format!("that surface template isn't valid JSON: {e}")))?;
    crate::agent::present::write_surface(&db, &conversation_id, &tree_json)
        .map_err(PoiesisError::Message)
}
