//! `RWD`: go back to before a turn, in the same chat.
//!
//! Rewind removes the turn and everything after it from the conversation, with an
//! optional file undo first. Nothing keeps the removed turns: `/fork` is how a
//! chat is kept. What stays is one `rewound` row saying that the user went back
//! and abandoned what followed. That row is the strongest negative signal there
//! is about a piece of work, and reflection reads it (`CPX-5`).
//!
//! "Try again" is the same cut followed by the same message sent again. It
//! writes no row and credits nothing.

use serde::Serialize;

use crate::db::Db;
use crate::memory::MemoryStore;

use super::changes;
use super::log::REWOUND_KIND;

/// What a rewind did.
#[derive(Debug, Serialize)]
pub struct Rewound {
    /// What the user had written in that turn, to put back in the box.
    pub prompt: String,
    /// How many files were put back (`0` when the user chose to keep them).
    pub files_undone: usize,
    /// Files that could not be put back, by name. The rewind still happens: a
    /// half-done undo with the turns still there would leave the user with neither.
    pub files_failed: Vec<String>,
}

/// The turn being gone back to: it must be one of the user's own messages.
fn user_turn(db: &Db, conversation_id: &str, message_id: &str) -> Result<crate::db::Message, String> {
    let message = db
        .list_messages(conversation_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|m| m.id == message_id)
        .ok_or_else(|| "I can't find that turn any more.".to_string())?;
    if message.role != "user" {
        return Err("I can only go back to before something you wrote.".to_string());
    }
    Ok(message)
}

/// `RWD-2`: how many files a rewind to before this turn would put back, so the
/// dialog can say so before the user commits to it.
pub fn files_since(db: &Db, conversation_id: &str, message_id: &str) -> Result<usize, String> {
    let turn = user_turn(db, conversation_id, message_id)?;
    Ok(changes::change_set(db, conversation_id, turn.created_at).files.len())
}

/// A short, one-line stand-in for a turn, for a title or a note.
fn preview(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    crate::media::ellipsize(first, 48)
}

/// `RWD-1`: go back to before `message_id`.
///
/// 1. If asked, put back every file the agent changed since that turn, newest
///    first so a file moved and then edited unwinds in order.
/// 2. Unless this is a `retry`, write the `rewound` row and credit what reached
///    the answers, while the abandoned ones can still be read.
/// 3. Remove the turn and everything after it.
pub fn rewind(
    db: &Db,
    mem: &MemoryStore,
    conversation_id: &str,
    message_id: &str,
    undo_files: bool,
    retry: bool,
) -> Result<Rewound, String> {
    let turn = user_turn(db, conversation_id, message_id)?;
    let shown = preview(&turn.content);
    if db.work_running_since(conversation_id, turn.created_at).map_err(|e| e.to_string())? {
        return Err("Something started after that message is still working. Stop it first.".to_string());
    }

    let mut files_undone = 0;
    let mut files_failed = Vec::new();
    if undo_files {
        let set = changes::change_set(db, conversation_id, turn.created_at);
        let mut files: Vec<&changes::FileChange> = set.files.iter().collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.last_at));
        // Every file is tried. Stopping at the first failure would leave the
        // earlier ones put back with the turns that explain them still there.
        for file in files {
            match changes::undo_file(db, file) {
                Ok(()) => files_undone += 1,
                Err(_) => files_failed.push(file.display.clone()),
            }
        }
        if files_undone > 0 {
            let _ = db.log_activity(
                Some(conversation_id),
                "file",
                &format!("undid my changes since \u{201c}{shown}\u{201d}"),
            );
        }
    }

    if !retry {
        let _ = db.append_session_event(
            conversation_id,
            None,
            REWOUND_KIND,
            &serde_json::json!({
                "messageId": message_id,
                "preview": shown,
                "filesUndone": files_undone,
                "filesFailed": files_failed,
            }),
        );
        super::credit::credit_conversation(db, mem, conversation_id);
    }

    db.cut_conversation(conversation_id, turn.created_at).map_err(|e| e.to_string())?;

    Ok(Rewound { prompt: turn.content, files_undone, files_failed })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewMessage;

    fn msg(db: &Db, conv: &str, role: &str, text: &str) -> String {
        db.append_message(
            conv,
            &NewMessage {
                role: role.into(),
                content: text.into(),
                model_name: None,
                model_provenance: None,
                steps_json: None,
                attachments: Vec::new(),
            },
        )
        .unwrap()
        .id
    }

    fn pause() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = crate::permissions::canonicalize_lenient(&std::env::temp_dir())
            .join(format!("poiesis_rewind_{name}_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A conversation in a folder: two exchanges, and a file the agent changed
    /// during the second.
    struct World {
        db: Db,
        mem: MemoryStore,
        conv: String,
        second_question: String,
        file: std::path::PathBuf,
        root: std::path::PathBuf,
        data: std::path::PathBuf,
    }

    fn world() -> World {
        let db = Db::open_in_memory().unwrap();
        let root = scratch("root");
        let data = scratch("data");
        let mem = MemoryStore::new(&root.join(".memory")).unwrap();
        let conv = db.create_conversation("Refactor", None, false).unwrap().id;
        db.set_conversation_folder(&conv, Some(&root.to_string_lossy())).unwrap();
        let file = root.join("a.txt");
        std::fs::write(&file, "before").unwrap();

        msg(&db, &conv, "user", "first question");
        pause();
        msg(&db, &conv, "assistant", "first answer");
        pause();
        let second_question = msg(&db, &conv, "user", "now change a.txt");
        pause();
        super::super::trash::record(&db, &data, &conv, "edit", &file, None);
        std::fs::write(&file, "after").unwrap();
        pause();
        msg(&db, &conv, "assistant", "second answer");
        World { db, mem, conv, second_question, file, root, data }
    }

    fn done(w: &World) {
        std::fs::remove_dir_all(&w.root).ok();
        std::fs::remove_dir_all(&w.data).ok();
    }

    fn said(w: &World) -> Vec<String> {
        w.db.list_messages(&w.conv).unwrap().into_iter().map(|m| m.content).collect()
    }

    fn go_back(w: &World, undo_files: bool) -> Rewound {
        rewind(&w.db, &w.mem, &w.conv, &w.second_question, undo_files, false).unwrap()
    }

    /// `RWD-T1`: with undo, the bytes come back, the chat stops before the turn
    /// and says it was gone back on. No second chat is made.
    #[test]
    fn rewinding_with_undo_restores_the_files_and_cuts_the_chat_before_the_turn() {
        let w = world();
        let chats = w.db.list_conversations().unwrap().len();

        let out = go_back(&w, true);

        assert_eq!(out.files_undone, 1);
        assert_eq!(std::fs::read_to_string(&w.file).unwrap(), "before", "the file is as it was");
        assert_eq!(out.prompt, "now change a.txt", "the words go back in the box");
        assert_eq!(said(&w), ["first question", "first answer"], "the turn and what followed are gone");
        assert_eq!(w.db.list_conversations().unwrap().len(), chats, "no branch");

        let rows = w.db.session_events(&w.conv).unwrap();
        let rewound: Vec<_> = rows.iter().filter(|r| r.kind == REWOUND_KIND).collect();
        assert_eq!(rewound.len(), 1);
        assert!(rewound[0].payload_json.contains("now change a.txt"));
        done(&w);
    }

    #[test]
    fn rewinding_without_undo_leaves_the_files_alone() {
        let w = world();
        let out = go_back(&w, false);
        assert_eq!(out.files_undone, 0);
        assert_eq!(std::fs::read_to_string(&w.file).unwrap(), "after");
        done(&w);
    }

    /// A file that cannot be put back does not stop the rewind: the chat is
    /// still cut, and the file is named so the user can see it was not undone.
    #[test]
    fn a_file_that_cannot_be_put_back_is_named_and_the_chat_is_still_cut() {
        let w = world();
        // The saved copy is gone, so there is nothing to put back.
        std::fs::remove_dir_all(&w.data).unwrap();
        let out = go_back(&w, true);
        assert_eq!(out.files_undone, 0);
        assert_eq!(out.files_failed.len(), 1);
        assert!(out.files_failed[0].contains("a.txt"), "{:?}", out.files_failed);
        assert_eq!(said(&w).len(), 2);
        done(&w);
    }

    /// What the model would resume from goes with the turns; what came before,
    /// and an earlier compaction of it, stays.
    #[test]
    fn the_log_and_the_summary_of_removed_turns_go_with_them() {
        let w = world();
        let first = w.db.list_messages(&w.conv).unwrap()[0].id.clone();
        let removed_answer = w.db.list_messages(&w.conv).unwrap()[3].id.clone();
        w.db.append_session_event(&w.conv, Some("run-2"), "assistant", &serde_json::json!({ "role": "assistant" }))
            .unwrap();
        for upto in [&first, &removed_answer] {
            w.db.append_session_event(
                &w.conv,
                None,
                super::super::log::COMPACTION_KIND,
                &serde_json::json!({ "upto_message_id": upto }),
            )
            .unwrap();
        }
        w.db.set_conversation_summary(&w.conv, "all of it", &removed_answer).unwrap();

        go_back(&w, false);

        assert!(w.db.session_events_for_run("run-2").unwrap().is_empty());
        let kept: Vec<String> = w
            .db
            .session_events(&w.conv)
            .unwrap()
            .into_iter()
            .filter(|r| r.kind == super::super::log::COMPACTION_KIND)
            .map(|r| r.payload_json)
            .collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!(kept[0].contains(&first));
        let conv = w.db.get_conversation(&w.conv).unwrap().unwrap();
        assert!(conv.summary.is_none() && conv.summary_upto_message_id.is_none());
        done(&w);
    }

    #[test]
    fn a_summary_of_turns_that_stay_is_kept() {
        let w = world();
        let first = w.db.list_messages(&w.conv).unwrap()[0].id.clone();
        w.db.set_conversation_summary(&w.conv, "the beginning", &first).unwrap();
        go_back(&w, false);
        let conv = w.db.get_conversation(&w.conv).unwrap().unwrap();
        assert_eq!(conv.summary.as_deref(), Some("the beginning"));
        done(&w);
    }

    /// "Try again" is the same cut, but it is not a verdict on the work.
    #[test]
    fn a_retry_cuts_the_turn_without_saying_the_user_went_back() {
        let w = world();
        rewind(&w.db, &w.mem, &w.conv, &w.second_question, false, true).unwrap();
        assert_eq!(said(&w), ["first question", "first answer"]);
        assert!(!super::super::log::was_rewound(&w.db, &w.conv));
        done(&w);
    }

    #[test]
    fn a_helper_still_working_for_a_removed_turn_stops_the_rewind() {
        let w = world();
        let child = w.db.create_conversation("helper", None, false).unwrap().id;
        w.db.create_subagent_run("run-h", &w.conv, None, &child, "general", "look").unwrap();
        assert!(rewind(&w.db, &w.mem, &w.conv, &w.second_question, true, false).is_err());
        assert_eq!(said(&w).len(), 4, "nothing was cut");
        assert_eq!(std::fs::read_to_string(&w.file).unwrap(), "after", "and no file was touched");
        done(&w);
    }

    /// `RWD-2`: the dialog is told how many files before the user commits.
    #[test]
    fn the_dialog_can_ask_how_many_files_would_be_put_back() {
        let w = world();
        assert_eq!(files_since(&w.db, &w.conv, &w.second_question).unwrap(), 1);
        done(&w);
    }

    #[test]
    fn only_something_the_user_wrote_can_be_gone_back_to() {
        let w = world();
        let answer = w
            .db
            .list_messages(&w.conv)
            .unwrap()
            .into_iter()
            .find(|m| m.role == "assistant")
            .unwrap()
            .id;
        assert!(rewind(&w.db, &w.mem, &w.conv, &answer, false, false).is_err());
        assert!(rewind(&w.db, &w.mem, &w.conv, "no-such-message", false, false).is_err());
        done(&w);
    }
}
