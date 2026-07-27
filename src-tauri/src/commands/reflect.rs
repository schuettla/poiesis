//! Reflection (REF-2): the agent learning from its own finished work.
//!
//! A *lesson* is the one kind of memory Poiesis writes about **itself** — a
//! generalizable correction drawn from a conversation that is over ("check a
//! directory exists before writing into it"). Three commitments shape this:
//!
//! - **Idempotent.** `reflected_at` is stamped before the model runs, so a
//!   hung or nonsense turn can never put the app in a retry loop.
//! - **Strict.** The output is parsed as JSON or discarded. A model that
//!   rambles teaches nothing; guessing at its intent would write junk into the
//!   prompt of every future conversation.
//! - **Gated.** Saving obeys `autonomy_gate("lessons")` — the user can turn
//!   self-teaching down to proposals, or off.

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::autonomy::{autonomy_gate, Rung};
use crate::cloud::{drive_turn, ChatEndpoint};
use crate::commands::agent::{build_cloud_endpoint, ChatTarget};
use crate::db::Db;
use crate::memory::{Fact, MemoryStore, LESSONS};
use crate::runtime::proxy::{CancelFlag, TurnOutcome};
use crate::runtime::RuntimeManager;
use crate::NexusError;

/// How many turns of the finished conversation reflection reads.
const WINDOW: usize = 30;
/// Per-turn clip. Reflection needs the shape of what happened, not the text.
const TURN_CLIP: usize = 400;
/// Never write more than this from one pass, whatever the model returns.
const MAX_LESSONS: usize = 3;

/// One lesson the model proposed. `confidence` decides whether it is worth
/// acting on at all — low-confidence drafts are dropped, not queued, because
/// v1 would rather learn nothing than fill the review queue with noise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LessonDraft {
    pub name: String,
    pub description: String,
    pub body: String,
    /// "high" | "low"
    #[serde(default)]
    pub confidence: String,
}

#[derive(Debug, Default, Deserialize)]
struct LessonBatch {
    #[serde(default)]
    lessons: Vec<LessonDraft>,
}

/// What one reflection pass produced. `saved` is what went to disk and is in
/// effect now; `proposed` is waiting on the user. They are counted separately
/// because the UI says different things about them — claiming to have learned
/// something that is still pending approval would be a lie.
#[derive(Debug, Default, Serialize)]
pub struct Reflection {
    pub saved: Vec<LessonDraft>,
    pub proposed: Vec<LessonDraft>,
}

/// Run one self-reflection pass over a finished conversation.
#[tauri::command]
pub async fn reflect_conversation_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    app: tauri::AppHandle,
    conversation_id: String,
    target: Option<ChatTarget>,
) -> Result<Reflection, NexusError> {
    // First thing, before anything can fail: this conversation has had its turn.
    let _ = db.set_conversation_reflected(&conversation_id, crate::db::now_ms());

    // Nothing to learn from, and nothing to route through — bail quietly rather
    // than surfacing an error for a background process the user didn't ask for.
    let rung = autonomy_gate(&db, "lessons");
    if rung == Rung::Off {
        return Ok(Reflection::default());
    }

    let target = target.unwrap_or_default();
    let endpoint = if target.provenance.as_deref() == Some("cloud") {
        build_cloud_endpoint(&target).map_err(NexusError::Message)?
    } else {
        let Some((base_url, token)) = mgr.engine_endpoint().await else {
            return Ok(Reflection::default());
        };
        ChatEndpoint::OpenAi {
            base_url,
            api_key: Some(token),
            model: None,
        }
    };

    let turns = db
        .list_messages_window(&conversation_id, WINDOW)
        .unwrap_or_default();
    if turns.is_empty() {
        return Ok(Reflection::default());
    }
    let transcript = turns
        .iter()
        .map(|m| {
            let mut text: String = m.content.chars().take(TURN_CLIP).collect();
            if m.content.chars().count() > TURN_CLIP {
                text.push('…');
            }
            format!("{}: {text}", m.role)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let failures = db.tool_failures_in(&conversation_id).unwrap_or_default();
    let failure_text = if failures.is_empty() {
        "none".to_string()
    } else {
        failures
            .iter()
            .map(|(tool, n)| format!("{tool} failed {n}×"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let prompt = format!(
        "Below is a finished conversation and the assistant's tool-failure counts. \
         Extract AT MOST {MAX_LESSONS} lessons about how the assistant should work better next time.\n\
         A lesson must be: (a) generalizable beyond this one conversation, (b) actionable as a \
         behavior change, (c) grounded in an observed mistake, failure, or user correction. \
         Style rules the assistant was merely following are NOT lessons. If nothing qualifies, \
         return {{\"lessons\":[]}}.\n\
         JSON schema:\n\
         {{\"lessons\":[{{\"name\":\"kebab-case-slug\",\"description\":\"one line\",\
         \"body\":\"2-4 sentences, imperative voice\",\"confidence\":\"high|low\"}}]}}\n\
         Tool failures: {failure_text}\n\
         Conversation:\n{transcript}"
    );
    let msgs = vec![
        serde_json::json!({
            "role": "system",
            "content": "You are the self-reflection process of a local AI assistant. Output ONLY JSON, no preamble.",
        }),
        serde_json::json!({ "role": "user", "content": prompt }),
    ];

    let outcome = drive_turn(&mgr.client, &endpoint, &msgs, &[], 0.2, &CancelFlag::new(), |_| {})
        .await
        .map_err(|e| NexusError::Message(e.to_string()))?;
    let TurnOutcome::Final { content } = outcome else {
        return Ok(Reflection::default());
    };

    // Parse strictly: unreadable output means this conversation taught nothing.
    let batch: LessonBatch = serde_json::from_str(strip_fence(&content)).unwrap_or_default();

    let existing: Vec<String> = mem.list_lessons().into_iter().map(|l| l.name).collect();
    let mut out = Reflection::default();
    for draft in batch.lessons.into_iter().take(MAX_LESSONS) {
        if draft.confidence.trim() != "high" {
            continue;
        }
        let Ok(slug) = crate::memory::slugify(&draft.name) else { continue };
        if existing.contains(&slug) {
            continue; // already learned; re-learning it isn't news
        }
        match rung {
            Rung::Off => break,
            Rung::Ask => {
                if db
                    .add_change_proposal("lesson", Some(&slug), &draft.body, &draft.description)
                    .is_ok()
                {
                    out.proposed.push(draft);
                }
            }
            Rung::Auto => {
                let fact = Fact {
                    name: slug.clone(),
                    description: draft.description.clone(),
                    kind: "lesson".to_string(),
                    created: String::new(),
                    source_conversation: Some(conversation_id.clone()),
                    body: draft.body.clone(),
                };
                if mem.save_lesson(&db, &fact).is_err() {
                    continue;
                }
                // Reflection runs outside any chat stream, so the toast is
                // driven by an app-level event rather than the agent sink.
                let _ = app.emit(
                    "poiesis-memory-write",
                    serde_json::json!({
                        "op": "save",
                        "name": slug,
                        "description": draft.description,
                        "collection": LESSONS,
                        "undo_token": "",
                    }),
                );
                let _ = db.log_activity(
                    Some(&conversation_id),
                    "reflect",
                    &format!("learned {slug}"),
                );
                out.saved.push(draft);
            }
        }
    }
    Ok(out)
}

/// Strip a ```json fence if the model wrapped its answer in one.
fn strip_fence(s: &str) -> &str {
    let s = s.trim();
    let Some(rest) = s.strip_prefix("```") else { return s };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.trim_start_matches('\n').trim_end_matches('`').trim()
}

/// Delete a lesson by hand (REF-UI-1). Returns the trash token that undoes it.
#[tauri::command]
pub fn forget_lesson_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    name: String,
) -> Result<String, NexusError> {
    let file = mem.forget_lesson(&db, &name).map_err(NexusError::Message)?;
    let _ = db.log_activity(None, "memory", &format!("forgot lesson {name}"));
    Ok(file)
}

#[tauri::command]
pub fn list_lessons_cmd(mem: State<'_, MemoryStore>) -> Vec<Fact> {
    mem.list_lessons()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_high_confidence_drafts_survive_parsing() {
        let raw = r#"```json
        {"lessons":[
          {"name":"Verify Paths","description":"check first","body":"Do it.","confidence":"high"},
          {"name":"maybe","description":"unsure","body":"Hmm.","confidence":"low"}
        ]}
        ```"#;
        let batch: LessonBatch = serde_json::from_str(strip_fence(raw)).unwrap();
        assert_eq!(batch.lessons.len(), 2);
        let kept: Vec<_> = batch
            .lessons
            .iter()
            .filter(|l| l.confidence == "high")
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(crate::memory::slugify(&kept[0].name).unwrap(), "verify-paths");
    }

    #[test]
    fn prose_teaches_nothing_rather_than_erroring() {
        let batch: LessonBatch =
            serde_json::from_str(strip_fence("Sure! Here are some lessons:")).unwrap_or_default();
        assert!(batch.lessons.is_empty());
        // A well-formed empty answer is also fine.
        let batch: LessonBatch = serde_json::from_str(r#"{"lessons":[]}"#).unwrap();
        assert!(batch.lessons.is_empty());
    }
}
