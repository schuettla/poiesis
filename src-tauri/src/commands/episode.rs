//! "Last time" (`EPI-UI-1`, `COGNITION_PLAN` IV.1).
//!
//! While the first message of a new conversation is typed, find a finished
//! conversation of the same kind of work and offer to do it the same way. This
//! is the one place an episode is ever shown back, and never silently: the
//! composer says it in one line and the user takes it or leaves it.
//!
//! A match has to be a conversation that ended `done`, of a known task kind,
//! under 180 days old, close enough in meaning to what is being typed. A wrong
//! "last time" is worse than none, so the bar is above the one for lessons.

use serde::Serialize;
use tauri::State;

use crate::commands::embedgen::embed_texts_or_none;
use crate::db::episodes::{Episode, EPISODE_SCOPE};
use crate::db::vectors::{NewVector, ScopeSearch};
use crate::db::Db;
use crate::runtime::{EmbedManager, RuntimeManager};
use crate::PoiesisError;

/// How close in meaning. Above the lesson floor (`SEM_FLOOR`, 0.58).
const LAST_TIME_FLOOR: f32 = 0.62;
/// Older than this is not "last time" any more.
const MAX_AGE_MS: i64 = 180 * 24 * 60 * 60 * 1000;
/// How many episodes one call may embed that were not embedded yet.
const BACKFILL: usize = 16;
/// Candidates looked at before giving up.
const CANDIDATES: usize = 5;
/// The line is one line.
const LINE_CLIP: usize = 140;
/// The fewest characters worth reading, as the composer's other hints.
const MIN_CHARS: usize = 12;

/// What the composer shows and, when taken, carries into the turn.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LastTime {
    pub conversation_id: String,
    /// When that conversation was last worked on, in ms.
    pub at: i64,
    pub goal: String,
    /// The first sentence of the digest: what I did.
    pub line: String,
    /// The whole digest, for the turn.
    pub digest: String,
    /// The skill that conversation used, if any.
    pub skill: Option<String>,
}

/// The first sentence of a digest, clipped to one line.
pub(crate) fn first_sentence(digest: &str) -> String {
    let digest = digest.trim();
    let end = digest
        .char_indices()
        .find(|(i, c)| matches!(c, '.' | '!' | '?') && digest[*i + c.len_utf8()..].starts_with([' ', '\n']))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(digest.len());
    let sentence = digest[..end].trim();
    if sentence.chars().count() <= LINE_CLIP {
        return sentence.to_string();
    }
    let mut out: String = sentence.chars().take(LINE_CLIP).collect();
    out.push('…');
    out
}

/// Is this episode one to offer back? `age_ms` is how long since the chat was
/// last worked on.
pub(crate) fn offerable(e: &Episode, age_ms: i64, score: f32) -> bool {
    e.outcome.as_deref() == Some("done")
        && e.task_kind.as_deref().is_some_and(|k| k != "unknown")
        && e.digest.as_deref().is_some_and(|d| !d.trim().is_empty())
        && age_ms <= MAX_AGE_MS
        && score >= LAST_TIME_FLOOR
}

/// The best offerable episode among scored hits, best first. `current` is the
/// chat being typed in, which is never its own last time.
pub(crate) fn choose(db: &Db, hits: &[(String, f32)], current: &str, now: i64) -> Option<LastTime> {
    for (id, score) in hits {
        if id == current {
            continue;
        }
        let Ok(Some(episode)) = db.episode(id) else { continue };
        let Ok(Some(conversation)) = db.get_conversation(id) else { continue };
        if !offerable(&episode, now - conversation.updated_at, *score) {
            continue;
        }
        let digest = episode.digest.clone().unwrap_or_default();
        return Some(LastTime {
            conversation_id: id.clone(),
            at: conversation.updated_at,
            goal: episode.goal.clone().unwrap_or_default(),
            line: first_sentence(&digest),
            digest,
            skill: db.skills_used_in(id).ok().and_then(|s| s.into_iter().next()),
        });
    }
    None
}

/// Offer a past conversation for the message being typed, or nothing. Never an
/// error: with no embedder, no match, or a conversation that is not new, the
/// composer simply shows nothing.
#[tauri::command]
pub async fn last_time_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    db: State<'_, Db>,
    conversation_id: String,
    text: String,
) -> Result<Option<LastTime>, PoiesisError> {
    let text = text.trim().to_string();
    if text.chars().count() < MIN_CHARS || text.starts_with('/') {
        return Ok(None);
    }
    // Only the first message of a conversation: after that the user is already
    // working with me, and the offer would be noise.
    if !db.list_messages_window(&conversation_id, 1).unwrap_or_default().is_empty() {
        return Ok(None);
    }
    let Ok(Some(model)) = db.default_model_by_role("embed") else { return Ok(None) };

    // The same round trip embeds the query and whatever episodes have no vector
    // yet, as recall does for memory.
    let missing: Vec<(String, String)> =
        db.episodes_missing_vector(&model.name).unwrap_or_default().into_iter().take(BACKFILL).collect();
    let mut texts = vec![text];
    texts.extend(missing.iter().map(|(_, t)| t.clone()));
    let Some((vectors, model_name, dim)) = embed_texts_or_none(&mgr, &embed_mgr, &db, &texts).await else {
        return Ok(None);
    };
    if !missing.is_empty() {
        let rows: Vec<NewVector> = missing
            .iter()
            .zip(&vectors[1..])
            .map(|((id, text), v)| NewVector {
                owner_kind: "episode".into(),
                scope_key: EPISODE_SCOPE.into(),
                ref_key: id.clone(),
                chunk_ix: 0,
                text: text.clone(),
                model: model_name.clone(),
                dim,
                vec: v.clone(),
                mtime: None,
            })
            .collect();
        let _ = db.insert_vectors(&rows);
    }

    let hits = match db.search_vectors("episode", EPISODE_SCOPE, &model_name, dim, &vectors[0], CANDIDATES) {
        Ok(ScopeSearch::Hits(hits)) => hits,
        Ok(ScopeSearch::Stale) => {
            // A model switched under the vectors: rebuilt on the next ask.
            let _ = db.delete_vectors_for_scope("episode", EPISODE_SCOPE);
            return Ok(None);
        }
        Err(_) => return Ok(None),
    };
    let scored: Vec<(String, f32)> = hits.into_iter().map(|h| (h.ref_key, h.score)).collect();
    Ok(choose(&db, &scored, &conversation_id, crate::db::now_ms()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done_episode(db: &Db, title: &str, kind: &str, digest: &str) -> String {
        let id = db.create_conversation(title, None, false).unwrap().id;
        db.save_episode_rules(&id, "done", kind, "{}").unwrap();
        db.save_episode_draft(&id, "expense summary", digest, "", "", "[]").unwrap();
        id
    }

    #[test]
    fn the_line_is_the_first_sentence_and_one_line() {
        assert_eq!(first_sentence("I made the summary. Then I checked it."), "I made the summary.");
        assert_eq!(first_sentence("No full stop here"), "No full stop here");
        assert_eq!(first_sentence("Version 1.5 is out. Done."), "Version 1.5 is out.");
        let long = format!("I {}.", "did a lot of things ".repeat(20));
        assert_eq!(first_sentence(&long).chars().count(), LINE_CLIP + 1);
    }

    #[test]
    fn only_a_recent_close_finished_episode_of_a_known_kind_is_offered() {
        let db = Db::open_in_memory().unwrap();
        let id = done_episode(&db, "Expenses", "report", "I made the expense summary.");
        let e = db.episode(&id).unwrap().unwrap();
        let day = 24 * 60 * 60 * 1000;
        assert!(offerable(&e, day, 0.7));
        assert!(!offerable(&e, 200 * day, 0.7), "older than 180 days");
        assert!(!offerable(&e, day, 0.5), "not close enough");

        let unknown = done_episode(&db, "Chat", "unknown", "I chatted.");
        assert!(!offerable(&db.episode(&unknown).unwrap().unwrap(), day, 0.9), "unknown is never shown");

        let failed = db.create_conversation("Failed", None, false).unwrap().id;
        db.save_episode_rules(&failed, "failed", "report", "{}").unwrap();
        db.save_episode_draft(&failed, "g", "I tried.", "", "", "[]").unwrap();
        assert!(!offerable(&db.episode(&failed).unwrap().unwrap(), day, 0.9), "only a done one");
    }

    #[test]
    fn the_best_offerable_hit_wins_and_the_current_chat_is_never_its_own_last_time() {
        let db = Db::open_in_memory().unwrap();
        let old_failed = db.create_conversation("Old", None, false).unwrap().id;
        db.save_episode_rules(&old_failed, "failed", "report", "{}").unwrap();
        db.save_episode_draft(&old_failed, "g", "I tried.", "", "", "[]").unwrap();
        let good = done_episode(&db, "Expenses", "report", "I made the expense summary per category. It was long.");
        let current = done_episode(&db, "Now", "report", "I am the current chat.");
        let now = crate::db::now_ms();

        let hits = vec![(current.clone(), 0.99), (old_failed, 0.9), (good.clone(), 0.7)];
        let offer = choose(&db, &hits, &current, now).unwrap();
        assert_eq!(offer.conversation_id, good);
        assert_eq!(offer.line, "I made the expense summary per category.");
        assert_eq!(offer.goal, "expense summary");
        assert!(offer.skill.is_none());
        assert!(choose(&db, &[(current.clone(), 0.99)], &current, now).is_none());
    }

    #[test]
    fn the_skill_that_conversation_used_goes_with_the_offer() {
        let db = Db::open_in_memory().unwrap();
        let id = done_episode(&db, "Expenses", "report", "I made the expense summary.");
        db.record_skill_run("expense-report", &id);
        let offer = choose(&db, &[(id.clone(), 0.8)], "other", crate::db::now_ms()).unwrap();
        assert_eq!(offer.skill.as_deref(), Some("expense-report"));
    }
}
