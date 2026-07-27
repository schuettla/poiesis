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
use crate::NexusError;

/// Everything the Self view's vitality strip and Health tab need, in one call.
#[derive(Debug, Serialize)]
pub struct Vitality {
    pub facts: usize,
    pub lessons: usize,
    pub recipes: usize,
    /// Sum of every recipe's use count — how much the procedures earned.
    pub recipe_uses: u32,
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
) -> Result<Vitality, NexusError> {
    // Reading the Health tab is also the moment to notice damaged files.
    mem.quarantine_scan(&db);

    let recipes = mem.list_recipes();
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
        recipes: recipes.len(),
        recipe_uses: recipes.iter().map(|r| r.used).sum(),
        quarantined: mem.quarantined(),
        engine_restarts_session: mgr.restarts_session(),
        pending_proposals: db.pending_proposal_count().unwrap_or(0),
        last_reflection: db.last_reflection().ok().flatten(),
        tool_health,
    })
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
) -> Result<Vec<ToolStatRow>, NexusError> {
    let model = match model_name {
        Some(m) if !m.trim().is_empty() => Some(m),
        _ => mgr.engine_model_name().await,
    };
    Ok(model
        .and_then(|m| db.tool_health(&m, 7).ok())
        .unwrap_or_default())
}

#[tauri::command]
pub fn list_recipes_cmd(mem: State<'_, MemoryStore>) -> Vec<crate::memory::Recipe> {
    mem.list_recipes()
}

#[tauri::command]
pub fn forget_recipe_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    name: String,
) -> Result<String, NexusError> {
    let file = mem.forget_recipe(&db, &name).map_err(NexusError::Message)?;
    let _ = db.log_activity(None, "memory", &format!("dropped the recipe {name}"));
    Ok(file)
}

/// Put a quarantined file back (HEAL-3) — the user has presumably repaired it.
#[tauri::command]
pub fn restore_quarantined_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    file: String,
) -> Result<(), NexusError> {
    mem.restore_quarantined(&db, &file).map_err(NexusError::Message)?;
    let _ = db.log_activity(None, "heal", &format!("restored {file}"));
    Ok(())
}

#[tauri::command]
pub fn delete_quarantined_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    file: String,
) -> Result<(), NexusError> {
    mem.delete_quarantined(&file).map_err(NexusError::Message)?;
    let _ = db.log_activity(None, "heal", &format!("discarded {file}"));
    Ok(())
}

/// Seed a conversation's workspace surface from a recipe template (RCP-UI-2).
/// Writes the reserved surface row exactly the way `render_ui` does, so the
/// template renders through the ordinary surface path with nothing special
/// about having come from a recipe.
#[tauri::command]
pub fn set_surface_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    tree_json: String,
) -> Result<String, NexusError> {
    serde_json::from_str::<serde_json::Value>(&tree_json)
        .map_err(|e| NexusError::Message(format!("that surface template isn't valid JSON: {e}")))?;
    crate::agent::present::write_surface(&db, &conversation_id, &tree_json)
        .map_err(NexusError::Message)
}
