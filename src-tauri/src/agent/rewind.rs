//! `RWD`: go back to before a turn, without losing anything.
//!
//! Rewind is a branch plus an optional file undo, which is what makes it safe:
//! the original conversation is never edited. Its messages stay exactly as they
//! were, and it gets one `rewound` row saying that the user went back and
//! abandoned what followed. That row is the strongest negative signal there is
//! about a piece of work, and reflection reads it (`CPX-5`).

use serde::Serialize;

use crate::db::{Conversation, Db};

use super::changes;
use super::log::REWOUND_KIND;

/// What a rewind did.
#[derive(Debug, Serialize)]
pub struct Rewound {
    /// The new conversation, holding everything before the turn.
    pub conversation: Conversation,
    /// What the user had written in that turn, to put back in the box.
    pub prompt: String,
    /// How many files were put back (`0` when the user chose to keep them).
    pub files_undone: usize,
    /// Files that could not be put back, by name. The rewind still happens: a
    /// half-done undo with no branch would leave the user with neither.
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
/// 2. Branch the conversation just before the turn. The branch has everything
///    that came before it and nothing that came after.
/// 3. Write the `rewound` row into the *original*.
pub fn rewind(db: &Db, conversation_id: &str, message_id: &str, undo_files: bool) -> Result<Rewound, String> {
    let turn = user_turn(db, conversation_id, message_id)?;
    let shown = preview(&turn.content);

    let mut files_undone = 0;
    let mut files_failed = Vec::new();
    if undo_files {
        let set = changes::change_set(db, conversation_id, turn.created_at);
        let mut files: Vec<&changes::FileChange> = set.files.iter().collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.last_at));
        // Every file is tried. Stopping at the first failure would leave the
        // earlier ones put back with no branch and no record of it.
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

    let (conversation, _resend) = db
        .fork_conversation_at(conversation_id, message_id, false)
        .map_err(|e| e.to_string())?;
    // "(again)" says a question was asked twice; this is a different thing.
    let original = db
        .get_conversation(conversation_id)
        .map_err(|e| e.to_string())?
        .map(|c| c.title)
        .unwrap_or_default();
    let title = format!("{original} (before \u{201c}{shown}\u{201d})");
    let _ = db.rename_conversation(&conversation.id, &title);

    let _ = db.append_session_event(
        conversation_id,
        None,
        REWOUND_KIND,
        &serde_json::json!({
            "messageId": message_id,
            "preview": shown,
            "branchId": conversation.id,
            "filesUndone": files_undone,
            "filesFailed": files_failed,
        }),
    );

    Ok(Rewound {
        conversation: Conversation { title, ..conversation },
        prompt: turn.content,
        files_undone,
        files_failed,
    })
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
        World { db, conv, second_question, file, root, data }
    }

    fn done(w: &World) {
        std::fs::remove_dir_all(&w.root).ok();
        std::fs::remove_dir_all(&w.data).ok();
    }

    /// `RWD-T1`: with undo, the bytes come back, the branch stops before the turn,
    /// the original is untouched and says it was gone back from.
    #[test]
    fn rewinding_with_undo_restores_the_files_and_branches_before_the_turn() {
        let w = world();
        let before = w.db.list_messages(&w.conv).unwrap().len();

        let out = rewind(&w.db, &w.conv, &w.second_question, true).unwrap();

        assert_eq!(out.files_undone, 1);
        assert_eq!(std::fs::read_to_string(&w.file).unwrap(), "before", "the file is as it was");
        assert_eq!(out.prompt, "now change a.txt", "the words go back in the box");
        let branch = w.db.list_messages(&out.conversation.id).unwrap();
        assert_eq!(
            branch.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
            ["first question", "first answer"],
            "the branch holds what came before the turn and nothing after"
        );
        assert_eq!(w.db.list_messages(&w.conv).unwrap().len(), before, "the original is untouched");
        assert!(out.conversation.title.contains("before"), "{}", out.conversation.title);

        let rows = w.db.session_events(&w.conv).unwrap();
        let rewound: Vec<_> = rows.iter().filter(|r| r.kind == REWOUND_KIND).collect();
        assert_eq!(rewound.len(), 1, "one row, in the original");
        assert!(rewound[0].payload_json.contains("now change a.txt"));
        assert!(rewound[0].payload_json.contains(&out.conversation.id));
        done(&w);
    }

    #[test]
    fn rewinding_without_undo_leaves_the_files_alone() {
        let w = world();
        let out = rewind(&w.db, &w.conv, &w.second_question, false).unwrap();
        assert_eq!(out.files_undone, 0);
        assert_eq!(std::fs::read_to_string(&w.file).unwrap(), "after");
        done(&w);
    }

    /// A file that cannot be put back does not stop the rewind: the branch is
    /// still made, and the file is named so the user can see it was not undone.
    #[test]
    fn a_file_that_cannot_be_put_back_is_named_and_the_branch_is_still_made() {
        let w = world();
        // The saved copy is gone, so there is nothing to put back.
        std::fs::remove_dir_all(&w.data).unwrap();
        let out = rewind(&w.db, &w.conv, &w.second_question, true).unwrap();
        assert_eq!(out.files_undone, 0);
        assert_eq!(out.files_failed.len(), 1);
        assert!(out.files_failed[0].contains("a.txt"), "{:?}", out.files_failed);
        assert_eq!(w.db.list_messages(&out.conversation.id).unwrap().len(), 2, "the branch is made");
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
        assert!(rewind(&w.db, &w.conv, &answer, false).is_err());
        assert!(rewind(&w.db, &w.conv, "no-such-message", false).is_err());
        done(&w);
    }
}
