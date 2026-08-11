//! Persona commands (CHT-4) + per-conversation persona/override linkage (CHT-7).

use tauri::State;

use crate::db::{Db, NewPersona, Persona};
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
pub fn create_persona_cmd(
    db: State<'_, Db>,
    name: String,
    system_prompt: String,
    model_id: Option<String>,
    params_json: Option<String>,
    tools_json: Option<String>,
    skills_json: Option<String>,
) -> Cmd<Persona> {
    db.create_persona(&NewPersona {
        name,
        system_prompt,
        model_id,
        params_json,
        tools_json,
        skills_json,
    })
    .map_err(err)
}

#[tauri::command]
pub fn update_persona_cmd(db: State<'_, Db>, persona: Persona) -> Cmd<()> {
    db.update_persona(&persona).map_err(err)
}

#[tauri::command]
pub fn delete_persona_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    db.delete_persona(&id).map_err(err)
}

#[tauri::command]
pub fn set_default_persona_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    db.set_default_persona(&id).map_err(err)
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
