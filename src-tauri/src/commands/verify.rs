//! `USE-10`: the commands behind the Verify action on an answer. The work is
//! `agent::verify`; this file gates it on the experiment, finds the answer, runs
//! one check at a time for a chat, reports progress and keeps the result.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;

use crate::agent::verify::{self, Claim, Live, Progress, Steps};
use crate::commands::agent::{resolve_turn, ChatTarget};
use crate::db::verifications::StoredCheck;
use crate::db::Db;
use crate::runtime::proxy::CancelFlag;
use crate::runtime::{EmbedManager, RuntimeManager};
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

const OFF: &str = "Checking answers is part of Fast & slow thinking. Turn it on in Settings \u{2192} Experiments.";

/// `SEE-5`: the deep check belongs to `fast-slow`, as the check on every answer does.
fn allowed(db: &Db) -> bool {
    crate::experiments::is_on(db, "fast-slow")
}

/// The checks running now, one a chat: the controller keeps its counts by chat, so
/// two at once would share them.
#[derive(Default)]
pub struct VerifyRuns(Mutex<HashMap<String, CancelFlag>>);

/// Takes the chat's place in [`VerifyRuns`] and gives it back, however the check ends.
struct Slot<'a> {
    runs: &'a VerifyRuns,
    conversation_id: String,
}

impl<'a> Slot<'a> {
    fn take(runs: &'a VerifyRuns, conversation_id: &str, cancel: &CancelFlag) -> Option<Slot<'a>> {
        let mut map = runs.0.lock().unwrap();
        if map.contains_key(conversation_id) {
            return None;
        }
        map.insert(conversation_id.to_string(), cancel.clone());
        Some(Slot { runs, conversation_id: conversation_id.to_string() })
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.runs.0.lock().unwrap().remove(&self.conversation_id);
    }
}

/// A stored check, as the screen reads it.
#[derive(Debug, Clone, Serialize)]
pub struct CheckView {
    pub message_id: String,
    pub conversation_id: String,
    pub created_at: i64,
    pub claims: Vec<Claim>,
    pub note: Option<String>,
    /// Opinions left out.
    pub skipped: u32,
    /// `auto` or `deep`.
    pub mode: String,
    /// What an automatic check counted (`agent::autocheck::Summary`); `None` for a deep one.
    pub summary: Option<crate::agent::autocheck::Summary>,
}

impl From<StoredCheck> for CheckView {
    fn from(s: StoredCheck) -> Self {
        CheckView {
            claims: verify::claims_from_json(&s.claims_json),
            message_id: s.message_id,
            conversation_id: s.conversation_id,
            created_at: s.created_at,
            note: s.note,
            skipped: s.skipped,
            summary: s.summary.as_deref().and_then(|j| serde_json::from_str(j).ok()),
            mode: s.mode,
        }
    }
}

/// What a check tells the screen while it runs.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum VerifyEvent {
    Progress {
        #[serde(flatten)]
        progress: Progress,
    },
    Done { check: CheckView },
    Cancelled,
    Error { message: String },
}

/// Check one answer of the assistant against evidence. Answers through `on_event`:
/// progress while it runs and then `done`, `cancelled` or `error`. The command
/// itself returns once the check is over.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn verify_message_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    db: State<'_, Db>,
    runs: State<'_, VerifyRuns>,
    conversation_id: String,
    message_id: String,
    target: Option<ChatTarget>,
    on_event: Channel<VerifyEvent>,
) -> Cmd<()> {
    if !allowed(&db) {
        return Err(err(OFF));
    }
    let answer = db
        .list_messages(&conversation_id)
        .map_err(err)?
        .into_iter()
        .find(|m| m.id == message_id && m.role == "assistant")
        .map(|m| m.content)
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| err("There is no answer there to check."))?;

    let cancel = CancelFlag::new();
    let Some(_slot) = Slot::take(&runs, &conversation_id, &cancel) else {
        return Err(err("A check is already running in this chat. Wait for it, or cancel it."));
    };
    let say = |event: VerifyEvent| {
        let _ = on_event.send(event);
    };

    let setup = match resolve_turn(&mgr, &db, &target.unwrap_or_default()).await {
        Ok(setup) => setup,
        Err(message) => {
            say(VerifyEvent::Error { message });
            return Ok(());
        }
    };
    // This check's counts begin again: 25 claims and 4 reviews are for one check.
    // The chat turn's own counts are not touched.
    crate::agent::decide::begin_check(&conversation_id);

    let live = Live {
        client: &mgr.client,
        endpoint: &setup.endpoint,
        db: &db,
        mgr: &mgr,
        embed_mgr: &embed_mgr,
        conversation_id: &conversation_id,
        cancel: &cancel,
        web: crate::agent::toolsets::Toolset::WebSearch.is_enabled(&db),
        state: Default::default(),
    };
    let mut progress = |p: Progress| say(VerifyEvent::Progress { progress: p });
    let result = verify::run(&live, &answer, &cancel, &mut progress).await;

    // A cancel ends the model call it interrupted with an error of its own, so the
    // flag is what says which it was.
    if cancel.is_cancelled() {
        say(VerifyEvent::Cancelled);
        return Ok(());
    }
    match result {
        Ok(Some(outcome)) => match verify::save(&db, &conversation_id, &message_id, &outcome) {
            Ok(true) => match db.verification(&message_id) {
                Ok(Some(stored)) => say(VerifyEvent::Done { check: stored.into() }),
                _ => say(VerifyEvent::Error { message: "The check finished but could not be read back.".into() }),
            },
            Ok(false) => say(VerifyEvent::Error { message: "That answer is gone, so the check was not kept.".into() }),
            Err(message) => say(VerifyEvent::Error { message }),
        },
        Ok(None) => say(VerifyEvent::Cancelled),
        Err(message) => say(VerifyEvent::Error { message }),
    }
    Ok(())
}

/// Stop the check running in a chat. Nothing is kept of a stopped check.
#[tauri::command]
pub fn verify_cancel_cmd(runs: State<'_, VerifyRuns>, conversation_id: String) {
    if let Some(flag) = runs.0.lock().unwrap().get(&conversation_id) {
        flag.cancel();
    }
}

/// Every check kept for a chat. Empty while `fast-slow` is off.
#[tauri::command]
pub fn verify_list_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<Vec<CheckView>> {
    if !allowed(&db) {
        return Ok(Vec::new());
    }
    Ok(db.verifications_for(&conversation_id).map_err(err)?.into_iter().map(CheckView::from).collect())
}

/// "This is right" or "This is wrong" on one claim. It is kept with the claim and
/// is a label for the decision that judged it.
#[tauri::command]
pub fn verify_mark_cmd(db: State<'_, Db>, message_id: String, claim_id: String, mark: String) -> Cmd<Claim> {
    verify::mark(&db, &message_id, &claim_id, &mark).map_err(err)
}

/// `SEE-1`: **Review this**. The slow model reads one marked sentence against the passages
/// it was judged on, without being told the quick verdict, and the sentence keeps what it
/// says. A mark that code matched against the run's own steps has no passages to review.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn verify_review_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    db: State<'_, Db>,
    conversation_id: String,
    message_id: String,
    claim_id: String,
    target: Option<ChatTarget>,
) -> Cmd<Claim> {
    if !allowed(&db) {
        return Err(err(OFF));
    }
    let stored = db.verification(&message_id).map_err(err)?.ok_or_else(|| err("That answer has no check."))?;
    let mut all = verify::claims_from_json(&stored.claims_json);
    let at = all.iter().position(|c| c.id == claim_id).ok_or_else(|| err("That claim is no longer in the check."))?;
    if all[at].how == "reviewed" {
        return Ok(all[at].clone());
    }
    if all[at].passages.is_empty() || all[at].how == crate::agent::autocheck::LEDGER {
        return Err(err("There is nothing to review it against: no passage was found for it."));
    }
    crate::agent::decide::control::controller().review(Some(&conversation_id)).map_err(err)?;
    let setup = resolve_turn(&mgr, &db, &target.unwrap_or_default()).await.map_err(err)?;
    let cancel = CancelFlag::new();
    let live = Live {
        client: &mgr.client,
        endpoint: &setup.endpoint,
        db: &db,
        mgr: &mgr,
        embed_mgr: &embed_mgr,
        conversation_id: &conversation_id,
        cancel: &cancel,
        web: false,
        state: Default::default(),
    };
    let reply = live.review(&all[at], &all[at].passages).await;
    let (finding, reason) = reply
        .as_deref()
        .and_then(verify::parse_review)
        .ok_or_else(|| err("The review gave no usable answer."))?;
    let claim = &mut all[at];
    claim.slow = Some(finding.clone());
    claim.finding = finding;
    claim.how = "reviewed".into();
    claim.settled = true;
    if !reason.is_empty() {
        claim.reason = Some(reason);
    }
    let updated = claim.clone();
    db.set_verification_claims(&message_id, &serde_json::to_string(&all).map_err(err)?).map_err(err)?;
    Ok(updated)
}

/// The words that ask for a corrected answer, written from what the check found.
/// The screen sends them as an ordinary message, so the result is a new answer and
/// the old one is not touched.
#[tauri::command]
pub fn verify_revise_cmd(db: State<'_, Db>, message_id: String) -> Cmd<String> {
    let stored = db.verification(&message_id).map_err(err)?.ok_or_else(|| err("That answer has no check."))?;
    verify::revise_request(&verify::claims_from_json(&stored.claims_json))
        .ok_or_else(|| err("There is nothing to revise: no claim was refuted, in conflict or unconfirmed."))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SEE-T-9`: the deep check follows `fast-slow`, not the old Verify switch.
    #[test]
    fn the_deep_check_is_gated_on_fast_slow_only() {
        let db = Db::open_in_memory().unwrap();
        assert!(!allowed(&db));
        db.set_setting(&crate::experiments::setting_key("verify"), "on").unwrap();
        assert!(!allowed(&db), "the old key means nothing now");
        db.set_setting(&crate::experiments::setting_key("fast-slow"), "on").unwrap();
        assert!(allowed(&db));
        assert!(OFF.contains("Fast & slow thinking"));
    }

    #[test]
    fn one_check_runs_in_a_chat_at_a_time_and_the_slot_comes_back() {
        let runs = VerifyRuns::default();
        let flag = CancelFlag::new();
        let first = Slot::take(&runs, "conv", &flag).expect("the chat is free");
        assert!(Slot::take(&runs, "conv", &flag).is_none(), "a second check waits");
        assert!(Slot::take(&runs, "other", &flag).is_some(), "another chat is free");
        drop(first);
        assert!(Slot::take(&runs, "conv", &flag).is_some(), "the slot is given back");
    }

    #[test]
    fn the_progress_event_reads_flat_for_the_screen() {
        let json = serde_json::to_value(VerifyEvent::Progress { progress: Progress::Checking { done: 2, total: 9 } }).unwrap();
        assert_eq!(json, serde_json::json!({ "type": "progress", "phase": "checking", "done": 2, "total": 9 }));
        let reading = serde_json::to_value(VerifyEvent::Progress { progress: Progress::Reading }).unwrap();
        assert_eq!(reading, serde_json::json!({ "type": "progress", "phase": "reading" }));
        assert_eq!(serde_json::to_value(VerifyEvent::Cancelled).unwrap(), serde_json::json!({ "type": "cancelled" }));
        let error = serde_json::to_value(VerifyEvent::Error { message: "x".into() }).unwrap();
        assert_eq!(error, serde_json::json!({ "type": "error", "message": "x" }));
    }

    #[test]
    fn nothing_to_revise_says_so_and_a_stored_check_reads_back_as_a_view() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("Facts", None, false).unwrap();
        let msg = db
            .append_message(
                &conv.id,
                &crate::db::NewMessage {
                    role: "assistant".into(),
                    content: "Paris is in France.".into(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: vec![],
                },
            )
            .unwrap();
        db.save_verification(&StoredCheck {
            message_id: msg.id.clone(),
            conversation_id: conv.id.clone(),
            claims_json: "[]".into(),
            note: Some("n".into()),
            skipped: 2,
            created_at: 5,
            mode: "deep".into(),
            summary: None,
        })
        .unwrap();
        let view: CheckView = db.verification(&msg.id).unwrap().unwrap().into();
        assert_eq!((view.claims.len(), view.skipped, view.note.as_deref()), (0, 2, Some("n")));
        assert!(verify::revise_request(&view.claims).is_none());
    }
}
