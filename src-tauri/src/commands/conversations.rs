//! Conversation, message, and settings commands (Phase 2, CHT-3/CHT-4).

use tauri::State;

use crate::db::{Artifact, Block, Conversation, Db, Message, NewAttachment, NewMessage};
use crate::NexusError;

type Cmd<T> = Result<T, NexusError>;

fn err<E: std::fmt::Display>(e: E) -> NexusError {
    NexusError::Message(e.to_string())
}

#[tauri::command]
pub fn list_conversations_cmd(db: State<'_, Db>) -> Cmd<Vec<Conversation>> {
    db.list_conversations().map_err(err)
}

#[tauri::command]
pub fn create_conversation_cmd(
    db: State<'_, Db>,
    title: Option<String>,
    model_id: Option<String>,
    workspace: Option<bool>,
) -> Cmd<Conversation> {
    db.create_conversation(
        title.as_deref().unwrap_or("New chat"),
        model_id.as_deref(),
        workspace.unwrap_or(false),
    )
    .map_err(err)
}

#[tauri::command]
pub fn set_conversation_workspace_cmd(
    db: State<'_, Db>,
    id: String,
    workspace: bool,
) -> Cmd<()> {
    db.set_conversation_workspace(&id, workspace).map_err(err)
}

#[tauri::command]
pub fn rename_conversation_cmd(db: State<'_, Db>, id: String, title: String) -> Cmd<()> {
    db.rename_conversation(&id, &title).map_err(err)
}

#[tauri::command]
pub fn delete_conversation_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    db.delete_conversation(&id).map_err(err)
}

#[tauri::command]
pub fn list_messages_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<Vec<Message>> {
    db.list_messages(&conversation_id).map_err(err)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn append_message_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    role: String,
    content: String,
    model_name: Option<String>,
    model_provenance: Option<String>,
    steps_json: Option<String>,
    attachments: Option<Vec<NewAttachment>>,
) -> Cmd<Message> {
    db.append_message(
        &conversation_id,
        &NewMessage {
            role,
            content,
            model_name,
            model_provenance,
            steps_json,
            attachments: attachments.unwrap_or_default(),
        },
    )
    .map_err(err)
}

#[tauri::command]
pub fn finalize_message_cmd(
    db: State<'_, Db>,
    id: String,
    content: String,
    steps_json: Option<String>,
) -> Cmd<()> {
    db.finalize_message(&id, &content, steps_json.as_deref())
        .map_err(err)
}

#[tauri::command]
pub fn search_conversations_cmd(db: State<'_, Db>, query: String) -> Cmd<Vec<Conversation>> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return db.list_conversations().map_err(err);
    }
    // Treat the user's text as a prefix-OR query so partial words match.
    let fts = format!("{}*", trimmed.replace('"', " "));
    db.search_conversations(&fts).map_err(err)
}

#[tauri::command]
pub fn list_artifacts_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<Vec<Artifact>> {
    db.list_artifacts(&conversation_id).map_err(err)
}

#[tauri::command]
pub fn list_all_artifacts_cmd(db: State<'_, Db>) -> Cmd<Vec<Artifact>> {
    db.list_all_artifacts().map_err(err)
}

#[tauri::command]
pub fn list_blocks_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<Vec<Block>> {
    db.list_blocks(&conversation_id).map_err(err)
}

#[tauri::command]
pub fn update_block_state_cmd(db: State<'_, Db>, id: String, state_json: String) -> Cmd<()> {
    db.update_block_state(&id, &state_json).map_err(err)
}

#[tauri::command]
pub fn get_session_state_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<Option<String>> {
    db.get_session_state(&conversation_id).map_err(err)
}

#[tauri::command]
pub fn set_session_state_cmd(db: State<'_, Db>, conversation_id: String, state_json: String) -> Cmd<()> {
    db.set_session_state(&conversation_id, &state_json).map_err(err)
}

#[tauri::command]
pub fn get_setting_cmd(db: State<'_, Db>, key: String) -> Cmd<Option<String>> {
    db.get_setting(&key).map_err(err)
}

#[tauri::command]
pub fn set_setting_cmd(db: State<'_, Db>, key: String, value: String) -> Cmd<()> {
    db.set_setting(&key, &value).map_err(err)
}
