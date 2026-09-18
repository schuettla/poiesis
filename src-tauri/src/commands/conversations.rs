//! Conversation, message, and settings commands (Phase 2, CHT-3/CHT-4).

use tauri::State;

use crate::cloud::{drive_turn, ChatEndpoint};
use crate::commands::agent::{build_remote_endpoint, ChatTarget};
use crate::db::{Artifact, Block, Conversation, Db, Message, MessageHit, NewAttachment, NewMessage};
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

    // `HRN-8`: tool results kept on disk are this conversation's, and nothing
    // else refers to them, so they go with it.
    let _ = std::fs::remove_dir_all(mgr.app_data_dir().join("results").join(&id));

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
#[allow(clippy::too_many_arguments)]
pub fn finalize_message_cmd(
    db: State<'_, Db>,
    id: String,
    content: String,
    steps_json: Option<String>,
    context_json: Option<String>,
    stop_reason: Option<String>,
    // `PLN-UI-5`: the plan the run worked to, so reopening the conversation
    // brings it back with the timeline rather than losing it.
    plan_json: Option<String>,
) -> Cmd<()> {
    db.finalize_message(
        &id,
        &content,
        steps_json.as_deref(),
        context_json.as_deref(),
        stop_reason.as_deref(),
        plan_json.as_deref(),
    )
    .map_err(err)
}

/// Each word becomes a quoted prefix term, so punctuation the user types
/// (`-`, `:`, `(`) is searched for rather than parsed as FTS5 syntax — which
/// used to fail the whole query and read as "no results".
fn fts_prefix_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

#[tauri::command]
pub fn search_messages_cmd(db: State<'_, Db>, query: String) -> Cmd<Vec<MessageHit>> {
    match fts_prefix_query(&query) {
        Some(fts) => db.search_messages(&fts, 30).map_err(err),
        None => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::fts_prefix_query;

    #[test]
    fn quotes_every_term_as_a_prefix() {
        assert_eq!(fts_prefix_query("  cache  bug "), Some("\"cache\"* \"bug\"*".into()));
        assert_eq!(fts_prefix_query("foo-bar (x):"), Some("\"foo-bar\"* \"(x):\"*".into()));
        assert_eq!(fts_prefix_query("say \"hi\""), Some("\"say\"* \"hi\"*".into()));
        assert_eq!(fts_prefix_query("   "), None);
        assert_eq!(fts_prefix_query("\"\""), None);
    }
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
    let endpoint = match build_remote_endpoint(&db, &target).map_err(PoiesisError::Message)? {
        Some(ep) => ep,
        None => {
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

    let outcome = drive_turn(&mgr.client, &endpoint, &msgs, &[], 0.2, crate::cloud::Effort::Off, &CancelFlag::new(), |_| {})
        .await
        .map_err(err)?;

    let summary = match outcome {
        TurnOutcome::Final { content, .. } => content.trim().to_string(),
        TurnOutcome::ToolCalls { .. } => {
            return Err(PoiesisError::Message("The model tried to use a tool while summarizing.".into()))
        }
        TurnOutcome::Cancelled => return Err(PoiesisError::Message("Summarizing was cancelled.".into())),
    };
    if summary.is_empty() {
        return Err(PoiesisError::Message("The model returned an empty summary.".into()));
    }

    db.set_conversation_summary(&conversation_id, &summary, &upto_message_id)
        .map_err(err)?;

    // `CTX-5`: the column above holds only the newest summary, so a second
    // compaction erases the first. The log keeps every one, with the stretch of
    // conversation it stands in for, so "what happened to the beginning of this
    // chat" has an answer that survives being compacted again.
    let previous = conv.summary_upto_message_id.as_deref();
    let covered: Vec<&crate::db::Message> = match previous {
        // Everything after the last boundary, up to this one.
        Some(prev) => messages
            .iter()
            .skip_while(|m| m.id != prev)
            .skip(1)
            .collect(),
        None => messages.iter().collect(),
    };
    crate::agent::log::record_compaction(
        &db,
        &conversation_id,
        &crate::agent::log::Compaction {
            text: summary.clone(),
            from_message_id: covered.first().map(|m| m.id.clone()),
            upto_message_id: upto_message_id.clone(),
            replaced: covered.len(),
            merged_earlier: conv.summary.is_some(),
        },
    );
    Ok(summary)
}

/// `CTX-5`: every compaction this conversation has been through, oldest first.
///
/// Read separately from the conversation itself because it is history, not
/// state: the chat list loads on every launch and nothing there needs it.
#[tauri::command]
pub fn conversation_summaries_cmd(
    db: State<'_, Db>,
    conversation_id: String,
) -> Cmd<Vec<CompactionView>> {
    Ok(crate::agent::log::compactions(&db, &conversation_id)
        .into_iter()
        .map(|(at, c)| CompactionView {
            at,
            text: c.text,
            from_message_id: c.from_message_id,
            upto_message_id: c.upto_message_id,
            replaced: c.replaced,
            merged_earlier: c.merged_earlier,
        })
        .collect())
}

/// One compaction as the UI shows it: the summary, when it happened, and how
/// much of the conversation it stands in for.
#[derive(serde::Serialize)]
pub struct CompactionView {
    pub at: i64,
    pub text: String,
    pub from_message_id: Option<String>,
    pub upto_message_id: String,
    pub replaced: usize,
    pub merged_earlier: bool,
}
