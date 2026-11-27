//! `TEAM-19`, `TEAM-20`: what waits for the person, and which sessions a run changed while
//! nobody watched.
//!
//! The entries are `change_proposals` rows (`db::add_waiting`). The inbox shows them; these
//! commands answer them. Saying yes to a change makes exactly that change; answering a question
//! writes the answer as a post to the one who asked. The returned sentence is what the session is
//! told when the person has answered everything that waited and the work goes on.

use tauri::State;

use crate::agent::team::{KIND_ANSWER, YOU};
use crate::db::{ChangeProposal, Db, NewPost};
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err(e: impl std::fmt::Display) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// Everything that waits for the person, newest first.
#[tauri::command]
pub fn list_waiting_cmd(db: State<'_, Db>) -> Cmd<Vec<ChangeProposal>> {
    db.list_waiting().map_err(err)
}

/// The sessions a run changed while nobody watched, that the person has not opened since.
#[tauri::command]
pub fn unread_sessions_cmd(db: State<'_, Db>) -> Cmd<Vec<String>> {
    db.unread_conversations().map_err(err)
}

/// The person opened a session.
#[tauri::command]
pub fn mark_seen_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<()> {
    db.mark_seen(&conversation_id).map_err(err)
}

/// `PAR-10`: a chat that is not on screen asks something. See `waiting::tell`.
#[tauri::command]
pub fn tell_waiting_cmd(db: State<'_, Db>, conversation_id: String, id: String, agent: Option<String>, line: String) {
    crate::waiting::tell(&db, &conversation_id, &id, agent.as_deref(), &line);
}

/// A team's session finished a run while the person was looking somewhere else. A chat they
/// started themselves never gets a dot, so only a session a team leads is marked.
#[tauri::command]
pub fn mark_away_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<()> {
    let leader = db.get_conversation(&conversation_id).map_err(err)?.and_then(|c| c.leader);
    if leader.as_deref().is_some_and(|l| l.starts_with("team:")) {
        db.mark_away(&conversation_id).map_err(err)?;
    }
    Ok(())
}

/// Answer one waiting entry. `accept` is yes for a change and "I will answer" for a question
/// (with the words in `text`); not accepting leaves the change undone, or the question
/// unanswered. Returns one sentence for the session. An error leaves the entry waiting.
#[tauri::command]
pub fn resolve_waiting_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    id: String,
    accept: bool,
    text: Option<String>,
) -> Cmd<String> {
    answer(&db, mgr.app_data_dir(), &id, accept, text.as_deref()).map_err(PoiesisError::Message)
}

pub fn answer(db: &Db, data_dir: &std::path::Path, id: &str, accept: bool, text: Option<&str>) -> Result<String, String> {
    let entry = db
        .get_change_proposal(id)
        .map_err(|e| e.to_string())?
        .filter(|p| p.target.starts_with("waiting-"))
        .ok_or("That is no longer waiting.")?;
    if entry.status != "pending" {
        return Err("That was already answered.".to_string());
    }
    let (conversation, session) = (entry.conversation_id.clone().unwrap_or_default(), entry.session_id.clone().unwrap_or_default());
    let who = entry.author.clone().unwrap_or_else(|| "The run".to_string());
    let note = match entry.target.as_str() {
        "waiting-file" => {
            if !accept {
                db.resolve_change_proposal(id, "dismissed").map_err(|e| e.to_string())?;
                format!("The person said no to this change, so it was not made: {}.", entry.rationale)
            } else {
                let made = crate::agent::filesystem::apply_parked(db, data_dir, &conversation, &entry.proposed_text)?;
                db.resolve_change_proposal(id, "applied").map_err(|e| e.to_string())?;
                format!("The person said yes to this change, and it is made: {}. {made}.", entry.rationale)
            }
        }
        "waiting-ask" => {
            let said = text.map(str::trim).filter(|t| !t.is_empty());
            let payload: serde_json::Value = serde_json::from_str(&entry.proposed_text).unwrap_or_default();
            let role = payload["from"].as_str().unwrap_or("coordinator").to_string();
            match (accept, said) {
                (true, Some(said)) => {
                    let asked = payload["post"].as_str();
                    let item = payload["item"].as_u64().map(|n| n as usize);
                    // Left for the one who asked: it reads it when the coordinator next hands work
                    // over or collects, as any post for a member that is not working.
                    let kind = KIND_ANSWER;
                    db.add_post(
                        &session,
                        &NewPost { from: YOU, to: &role, kind, item, reply_to: asked, text: said, ..Default::default() },
                    )
                    .map_err(|e| e.to_string())?;
                    db.resolve_change_proposal(id, "applied").map_err(|e| e.to_string())?;
                    format!("The person answered {who}: {said}")
                }
                (true, None) => return Err("Write the answer first.".to_string()),
                (false, _) => {
                    db.resolve_change_proposal(id, "dismissed").map_err(|e| e.to_string())?;
                    format!("The person chose not to answer {who} for now: {}", entry.rationale)
                }
            }
        }
        other => return Err(format!("{other} is not something I can answer here.")),
    };
    crate::waiting::changed(&session);
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(db: &Db) -> String {
        db.create_conversation("Morning run", None, false).unwrap().id
    }

    #[test]
    fn a_change_that_waits_is_made_only_when_the_person_says_yes() {
        let db = Db::open_in_memory().unwrap();
        let conv = session(&db);
        let dir = std::env::temp_dir().join(format!("poiesis_waiting_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        db.set_conversation_folder(&conv, Some(dir.to_str().unwrap())).unwrap();
        std::fs::write(dir.join("old.txt"), "keep me").unwrap();
        let payload = serde_json::json!({ "tool": "delete_file", "args": { "path": "old.txt" }, "shown": "old.txt" });
        let entry = db.add_waiting("waiting-file", &conv, Some("api"), &payload.to_string(), "Delete old.txt").unwrap();
        assert_eq!(db.list_waiting().unwrap().len(), 1);
        assert!(dir.join("old.txt").exists(), "parked, not made");

        // Not yet answered by a yes: saying no leaves the file and closes the entry.
        let no = answer(&db, &dir, &entry.id, false, None).unwrap();
        assert!(no.contains("said no"), "{no}");
        assert!(dir.join("old.txt").exists());
        assert!(db.list_waiting().unwrap().is_empty());
        assert!(answer(&db, &dir, &entry.id, true, None).is_err(), "an answered entry is not answered twice");

        let again = db.add_waiting("waiting-file", &conv, Some("api"), &payload.to_string(), "Delete old.txt").unwrap();
        let yes = answer(&db, &dir, &again.id, true, None).unwrap();
        assert!(yes.contains("said yes"), "{yes}");
        assert!(!dir.join("old.txt").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_edit_that_no_longer_fits_is_not_made_and_keeps_waiting() {
        let db = Db::open_in_memory().unwrap();
        let conv = session(&db);
        let dir = std::env::temp_dir().join(format!("poiesis_waiting_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        db.set_conversation_folder(&conv, Some(dir.to_str().unwrap())).unwrap();
        std::fs::write(dir.join("a.txt"), "changed since").unwrap();
        let payload = serde_json::json!({
            "tool": "edit_file", "args": { "path": "a.txt", "old_string": "original text", "new_string": "new" }, "shown": "a.txt"
        });
        let entry = db.add_waiting("waiting-file", &conv, None, &payload.to_string(), "Edit a.txt").unwrap();
        let why = answer(&db, &dir, &entry.id, true, None).unwrap_err();
        assert!(why.contains("changed since"), "{why}");
        assert_eq!(db.list_waiting().unwrap().len(), 1, "still waiting, so the person can say no");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_answer_is_written_to_the_one_who_asked() {
        let db = Db::open_in_memory().unwrap();
        let conv = session(&db);
        let payload = serde_json::json!({ "post": "p_1", "from": "api", "item": 2 });
        let entry = db.add_waiting("waiting-ask", &conv, Some("api"), &payload.to_string(), "api asks: Which table?").unwrap();
        assert!(answer(&db, std::path::Path::new("."), &entry.id, true, Some("  ")).is_err(), "an empty answer is no answer");
        let note = answer(&db, std::path::Path::new("."), &entry.id, true, Some("orders")).unwrap();
        assert!(note.contains("orders"), "{note}");
        let posts = db.list_posts(&conv).unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!((posts[0].from_member.as_str(), posts[0].to_member.as_str(), posts[0].kind.as_str()), ("you", "api", "answer"));
        assert_eq!(posts[0].item, Some(2));
        assert!(posts[0].delivered_at.is_none(), "it waits until api is woken for it");
        assert!(db.list_waiting().unwrap().is_empty());
    }

    #[test]
    fn what_waits_marks_the_session_and_opening_it_clears_the_dot() {
        let db = Db::open_in_memory().unwrap();
        let conv = session(&db);
        let child = db.create_conversation("api", None, false).unwrap().id;
        db.set_conversation_parent(&child, &conv).ok();
        assert!(db.unread_conversations().unwrap().is_empty(), "a chat the person started has no dot");
        db.add_waiting("waiting-ask", &child, Some("api"), "{}", "api asks: Which table?").unwrap();
        let waiting = db.list_waiting().unwrap();
        assert_eq!(waiting[0].conversation_id.as_deref(), Some(child.as_str()));
        db.mark_away(&conv).unwrap();
        assert_eq!(db.unread_conversations().unwrap(), vec![conv.clone()]);
        std::thread::sleep(std::time::Duration::from_millis(3));
        db.mark_seen(&conv).unwrap();
        assert!(db.unread_conversations().unwrap().is_empty());
        std::thread::sleep(std::time::Duration::from_millis(3));
        db.mark_away(&conv).unwrap();
        assert_eq!(db.unread_conversations().unwrap(), vec![conv], "a later run makes it unread again");
    }
}
