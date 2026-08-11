//! Conversation, message, and settings commands (Phase 2, CHT-3/CHT-4).

use tauri::State;

use crate::cloud::{drive_turn, ChatEndpoint};
use crate::commands::agent::{build_cloud_endpoint, ChatTarget};
use crate::db::{Artifact, Block, Conversation, Db, Message, NewAttachment, NewMessage};
use crate::runtime::proxy::{CancelFlag, TurnOutcome};
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
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
pub fn delete_conversation_cmd(mgr: State<'_, RuntimeManager>, db: State<'_, Db>, id: String) -> Cmd<()> {
    // Collect generated media before the cascade removes the rows that would
    // otherwise be the only record these files ever existed (`FIX-2`).
    let media_dir = mgr.generated_media_dir();
    let candidates: Vec<String> = db
        .list_artifacts(&id)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| matches!(a.kind.as_str(), "image" | "video"))
        .filter(|a| std::path::Path::new(&a.content).starts_with(&media_dir))
        .map(|a| a.content)
        .collect();

    db.delete_conversation(&id).map_err(err)?;

    for path in candidates {
        let still_referenced = db.is_known_attachment(&path).unwrap_or(true)
            || db.is_known_artifact_content(&path).unwrap_or(true);
        if !still_referenced {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
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
    context_json: Option<String>,
) -> Cmd<()> {
    db.finalize_message(&id, &content, steps_json.as_deref(), context_json.as_deref())
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

// ---- context compaction (CTX-3) ----

/// Clip a message body for the summarization prompt. Compaction is about the
/// shape of the conversation, not its every word.
fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

/// Summarize every message up to and including `upto_message_id` into
/// `conversations.summary`, merging any existing summary, and return the result.
///
/// Runs on the same endpoint the chat uses — a local model summarizes locally.
/// This changes only what is *sent* to the model on later turns: no message is
/// ever deleted, hidden, or altered.
#[tauri::command]
pub async fn compact_conversation_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    conversation_id: String,
    upto_message_id: String,
    target: Option<ChatTarget>,
) -> Cmd<String> {
    let target = target.unwrap_or_default();
    let endpoint = if target.provenance.as_deref() == Some("cloud") {
        build_cloud_endpoint(&target).map_err(PoiesisError::Message)?
    } else {
        let Some((base_url, token)) = mgr.engine_endpoint().await else {
            return Err(PoiesisError::Message(
                "No model is loaded, so older turns can't be summarized.".into(),
            ));
        };
        ChatEndpoint::OpenAi {
            base_url,
            api_key: Some(token),
            model: None,
        }
    };

    let conv = db
        .list_conversations()
        .map_err(err)?
        .into_iter()
        .find(|c| c.id == conversation_id)
        .ok_or_else(|| PoiesisError::Message("That conversation no longer exists.".into()))?;

    let messages = db.list_messages_until(&conversation_id, &upto_message_id).map_err(err)?;
    if messages.is_empty() {
        return Err(PoiesisError::Message("Nothing to summarize yet.".into()));
    }

    let transcript = messages
        .iter()
        .map(|m| format!("{}: {}", m.role, clip(&m.content, 500)))
        .collect::<Vec<_>>()
        .join("\n");
    let existing = conv.summary.as_deref().unwrap_or("none");

    // In workspace mode the live surface holds the task state, so restating it
    // in the summary would only duplicate (and can contradict) what's on screen.
    let workspace_rule = if conv.workspace {
        "\nThe live workspace surface is authoritative; do not restate its contents."
    } else {
        ""
    };

    let prompt = format!(
        "Summarize this conversation so a colleague can continue it.\n\
         Use exactly these sections, plain text, max 300 words total:\n\
         FACTS: (stable facts, names, numbers)\n\
         DECISIONS: (settled choices)\n\
         OPEN: (unresolved threads, next steps){workspace_rule}\n\
         Existing summary to merge in:\n{existing}\n\
         Conversation:\n{transcript}"
    );

    let msgs = vec![
        serde_json::json!({
            "role": "system",
            "content": "You compress conversation history. Output ONLY the summary, no preamble.",
        }),
        serde_json::json!({ "role": "user", "content": prompt }),
    ];

    let outcome = drive_turn(&mgr.client, &endpoint, &msgs, &[], 0.2, &CancelFlag::new(), |_| {})
        .await
        .map_err(err)?;

    let summary = match outcome {
        TurnOutcome::Final { content } => content.trim().to_string(),
        TurnOutcome::ToolCalls(_) => {
            return Err(PoiesisError::Message("The model tried to use a tool while summarizing.".into()))
        }
        TurnOutcome::Cancelled => return Err(PoiesisError::Message("Summarizing was cancelled.".into())),
    };
    if summary.is_empty() {
        return Err(PoiesisError::Message("The model returned an empty summary.".into()));
    }

    db.set_conversation_summary(&conversation_id, &summary, &upto_message_id)
        .map_err(err)?;
    Ok(summary)
}
