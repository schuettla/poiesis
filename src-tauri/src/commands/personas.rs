//! Persona commands (CHT-4) + per-conversation persona/override linkage (CHT-7).

use tauri::State;

use crate::agent::leader::{self, Led, Leader};
use crate::db::{Db, NewPersona, Persona};
use crate::memory::MemoryStore;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

#[tauri::command]
pub fn list_personas_cmd(db: State<'_, Db>) -> Cmd<Vec<Persona>> {
    db.list_personas().map_err(err)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn create_persona_cmd(
    db: State<'_, Db>,
    name: String,
    system_prompt: String,
    model_id: Option<String>,
    params_json: Option<String>,
    tools_json: Option<String>,
    skills_json: Option<String>,
    description: Option<String>,
    spawnable: Option<bool>,
) -> Cmd<Persona> {
    db.create_persona(&NewPersona {
        name,
        system_prompt,
        model_id,
        params_json,
        tools_json,
        skills_json,
        description,
        spawnable: spawnable.unwrap_or(false),
    })
    .map_err(err)
}

#[tauri::command]
pub fn update_persona_cmd(db: State<'_, Db>, persona: Persona) -> Cmd<()> {
    db.update_persona(&persona).map_err(err)
}

#[tauri::command]
pub fn delete_persona_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, id: String) -> Cmd<()> {
    // `TEAM-9`: where it played a member of a team, the place becomes a helper with the same job.
    // First, so a team that cannot be saved leaves the persona where it is.
    let name = db.get_persona(&id).map_err(err)?.map_or_else(|| "A persona".to_string(), |p| p.name);
    mem.release_persona_from_teams(&id, &name).map_err(err)?;
    db.delete_persona(&id).map_err(err)?;
    // `TEAM-5`: what only this persona knew goes to the memory trash, where it can be restored.
    let gone = mem.forget_persona(&db, &id);
    if gone > 0 {
        let _ = db.log_activity(None, "memory", &format!("set {gone} entries aside with a deleted persona"));
    }
    // `TEAM-1`: a place that started its chats with this persona starts them with Poiesis again.
    mem.drop_default_leaders_for(&format!("persona:{id}")).map_err(err)
}

/// "Make default": new chats everywhere are led by this persona (`TEAM-1`). The flag
/// on the row stays so the editor can still show it.
#[tauri::command]
pub fn set_default_persona_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, id: String) -> Cmd<()> {
    db.set_default_persona(&id).map_err(err)?;
    mem.set_default_leader("global", Some(&format!("persona:{id}"))).map_err(err)
}

/// `TEAM-1`: who leads this chat, and whether somebody chose it for this chat or
/// where chats start says so.
#[tauri::command]
pub fn conversation_leader_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, id: String) -> Option<Led> {
    let conv = db.get_conversation(&id).ok().flatten()?;
    Some(leader::effective(&db, &mem, &conv))
}

/// Hand this chat to a leader (`poiesis`, `persona:<id>`, `team:<slug>`), or (`None`) give the
/// choice back so where chats start decides again. A team (`team:<slug>`) is led by its coordinator.
#[tauri::command]
pub fn set_conversation_leader_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, id: String, leader: Option<String>) -> Cmd<()> {
    let who = match leader.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        None => None,
        Some(text) => Some(Leader::parse(text).ok_or_else(|| err("I don't know who that is."))?),
    };
    leader::set_leader(&db, &mem, &id, who.as_ref()).map_err(err)
}

/// Say who leads the chats that start at a place (`global`, `project:<id>`,
/// `kind:<kind>`), or (`None`) take it away.
#[tauri::command]
pub fn leader_default_set_cmd(mem: State<'_, MemoryStore>, scope: String, leader: Option<String>) -> Cmd<()> {
    mem.set_default_leader(&scope, leader.as_deref()).map_err(err)
}

/// Attach (or clear) a conversation's persona and one-off overrides (CHT-4/CHT-7).
#[tauri::command]
pub fn set_conversation_persona_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    persona_id: Option<String>,
    overrides_json: Option<String>,
) -> Cmd<()> {
    db.set_conversation_persona(&conversation_id, persona_id.as_deref(), overrides_json.as_deref())
        .map_err(err)
}
