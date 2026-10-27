//! Credit (`CRD-2`, `COGNITION_PLAN` III.3): did a remembered thing help?
//!
//! `messages.context_json` says which facts and lessons reached each answer.
//! At the end of encoding, the answers of one conversation are read once and
//! each item is given what it earned, on the `memory_usage` row it already has:
//! `chances` (answers it reached), `helped`, `hurt`. Nothing here asks a model.
//!
//! What counts, with the evidence this build has:
//!
//! - **A lesson cued to a tool** (`cue: tool:<name>`). The tool ran after the
//!   answer and did not fail: helped. It failed: hurt. The turn was gone back
//!   on: hurt. The tool did not run: no effect.
//! - **Anything the user pressed Outdated or Not relevant on** (a command trace
//!   in the session log): hurt, on the last answer that carried it.
//! - Nothing else. A fact that was merely in the prompt is a chance and not a
//!   help: this build cannot tell that the answer used it, and a guess here
//!   would rank memory by noise.

use std::collections::HashMap;

use crate::commands::memory::ContextRefs;
use crate::db::{Db, Message};
use crate::memory::{MemoryStore, FACTS, LESSONS};

/// The command traces the chip's two buttons write (`CUE-UI-1`). The argument
/// is the entry's name.
pub const OUTDATED: &str = "outdated";
pub const NOT_RELEVANT: &str = "not-relevant";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Tally {
    chances: i64,
    helped: i64,
    hurt: i64,
    last_at: i64,
}

/// The tool a lesson is cued to, when it is.
fn tool_cue(cue: Option<&str>) -> Option<&str> {
    cue?.trim().strip_prefix("tool:").map(str::trim).filter(|t| !t.is_empty())
}

/// Credit every item that reached an answer of this conversation. Returns how
/// many items were credited. Safe to call on a conversation without any
/// recorded context: it credits nothing.
pub fn credit_conversation(db: &Db, mem: &MemoryStore, conversation_id: &str) -> usize {
    let messages = db.list_messages(conversation_id).unwrap_or_default();
    let answers: Vec<(usize, ContextRefs)> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "assistant")
        .filter_map(|(i, m)| {
            let raw = db.message_context_json(&m.id).ok().flatten()?;
            Some((i, serde_json::from_str::<ContextRefs>(&raw).ok()?))
        })
        .collect();
    if answers.is_empty() {
        return 0;
    }

    // Answers after the point the user went back to were abandoned.
    let rewound_from = rewind_point(db, conversation_id, &messages);
    // Entries the user pressed a button on, with the time of the press.
    let pressed: Vec<(String, i64)> = crate::agent::log::commands(db, conversation_id)
        .into_iter()
        .filter(|r| r.trace.name == OUTDATED || r.trace.name == NOT_RELEVANT)
        .map(|r| (r.trace.args.trim().to_string(), r.at))
        .collect();

    let mut tallies: HashMap<(&'static str, String), Tally> = HashMap::new();
    for (n, (i, refs)) in answers.iter().enumerate() {
        let msg = &messages[*i];
        let until = answers.get(n + 1).map(|(next, _)| messages[*next].created_at);

        let items = refs
            .facts
            .iter()
            .map(|name| (FACTS, name))
            .chain(refs.lessons.iter().map(|name| (LESSONS, name)));
        for (collection, name) in items {
            let Some(entry) = mem.read_in(collection, name) else { continue };
            let t = tallies.entry((collection, name.clone())).or_default();
            t.chances += 1;
            t.last_at = t.last_at.max(msg.created_at);

            if collection == LESSONS {
                if let Some(tool) = tool_cue(entry.cue.as_deref()) {
                    let (ok, failed) =
                        db.tool_results_between(conversation_id, tool, msg.created_at, until).unwrap_or((0, 0));
                    let gone_back = rewound_from.is_some_and(|from| *i > from);
                    if failed > 0 || gone_back {
                        t.hurt += 1;
                    } else if ok > 0 {
                        t.helped += 1;
                    }
                }
            }

            // The press belongs to the last answer that carried the entry.
            let carried_after = answers
                .iter()
                .skip(n + 1)
                .any(|(_, later)| later.facts.contains(name) || later.lessons.contains(name));
            let hurt_by_press = pressed
                .iter()
                .any(|(arg, at)| arg == name && *at >= msg.created_at && until.is_none_or(|u| *at < u) && !carried_after);
            if hurt_by_press {
                t.hurt += 1;
            }
        }
    }

    let count = tallies.len();
    for ((collection, name), t) in tallies {
        let _ = db.add_memory_credit(collection, &name, t.chances, t.helped, t.hurt, t.last_at);
    }
    count
}

/// The index, in `messages`, of the user turn this conversation was gone back
/// to; answers after it were abandoned.
fn rewind_point(db: &Db, conversation_id: &str, messages: &[Message]) -> Option<usize> {
    let rows = db.session_events(conversation_id).ok()?;
    rows.iter()
        .filter(|r| r.kind == crate::agent::log::REWOUND_KIND)
        .filter_map(|r| serde_json::from_str::<serde_json::Value>(&r.payload_json).ok())
        .filter_map(|v| v["messageId"].as_str().map(str::to_string))
        .filter_map(|id| messages.iter().position(|m| m.id == id))
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewMessage;
    use crate::memory::Fact;

    struct World {
        db: Db,
        mem: MemoryStore,
        conv: String,
        _dir: tempfile::TempDir,
    }

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let mem = MemoryStore::new(dir.path()).unwrap();
        let conv = db.create_conversation("A chat", None, false).unwrap().id;
        World { db, mem, conv, _dir: dir }
    }

    fn lesson(w: &World, name: &str, cue: Option<&str>) {
        w.mem
            .save_lesson(
                &w.db,
                &Fact {
                    name: name.into(),
                    description: format!("about {name}"),
                    kind: "lesson".into(),
                    created: String::new(),
                    source_conversation: None,
                    body: "Do it carefully.".into(),
                    scope: None,
                    recurrence: None,
                    last_seen: None,
                    expires_at: None,
                    origin: None,
                    project: None,
                    cue: cue.map(str::to_string),
                    confidence: None,
                    status: None,
                    supersedes: None,
                    superseded_by: None,
                    evidence: Vec::new(),
                    valid_from: None,
                },
            )
            .unwrap();
    }

    /// An answer that carried these lessons and facts, made at `at`.
    fn answer(w: &World, lessons: &[&str], facts: &[&str], at: i64) -> String {
        let msg = w
            .db
            .append_message(
                &w.conv,
                &NewMessage {
                    role: "assistant".into(),
                    content: "ok".into(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        let refs = ContextRefs {
            lessons: lessons.iter().map(|s| s.to_string()).collect(),
            facts: facts.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        w.db.finalize_message(&msg.id, "ok", None, Some(&serde_json::to_string(&refs).unwrap()), None, None).unwrap();
        w.db.exec_for_tests("UPDATE messages SET created_at = ?2 WHERE id = ?1", rusqlite::params![msg.id, at]);
        msg.id
    }

    fn tool(w: &World, name: &str, ok: bool, at: i64) {
        w.db.add_tool_stat("m", name, &w.conv, ok);
        w.db.exec_for_tests(
            "UPDATE tool_stats SET created_at = ?1 WHERE rowid = (SELECT MAX(rowid) FROM tool_stats)",
            [at],
        );
    }

    fn credit_of(w: &World, collection: &str, name: &str) -> crate::db::episodes::Credit {
        w.db.memory_credit_map(collection).unwrap()[name]
    }

    #[test]
    fn a_lesson_cued_to_a_tool_that_then_runs_clean_helped() {
        let w = world();
        lesson(&w, "check-header", Some("tool:read_file"));
        answer(&w, &["check-header"], &[], 100);
        tool(&w, "read_file", true, 120);
        assert_eq!(credit_conversation(&w.db, &w.mem, &w.conv), 1);
        let c = credit_of(&w, "lessons", "check-header");
        assert_eq!((c.chances, c.helped, c.hurt), (1, 1, 0));
    }

    #[test]
    fn the_same_failure_again_hurt() {
        let w = world();
        lesson(&w, "check-header", Some("tool:read_file"));
        answer(&w, &["check-header"], &[], 100);
        tool(&w, "read_file", false, 120);
        credit_conversation(&w.db, &w.mem, &w.conv);
        let c = credit_of(&w, "lessons", "check-header");
        assert_eq!((c.chances, c.helped, c.hurt), (1, 0, 1));
    }

    #[test]
    fn a_tool_that_never_ran_is_a_chance_and_nothing_more() {
        let w = world();
        lesson(&w, "check-header", Some("tool:read_file"));
        lesson(&w, "be-brief", None);
        answer(&w, &["check-header", "be-brief"], &[], 100);
        tool(&w, "write_file", true, 120);
        credit_conversation(&w.db, &w.mem, &w.conv);
        assert_eq!(credit_of(&w, "lessons", "check-header").helped, 0);
        assert_eq!(credit_of(&w, "lessons", "be-brief").chances, 1);
        assert_eq!(credit_of(&w, "lessons", "be-brief").hurt, 0);
    }

    #[test]
    fn a_tool_call_after_the_next_answer_belongs_to_the_next_answer() {
        let w = world();
        lesson(&w, "check-header", Some("tool:read_file"));
        answer(&w, &["check-header"], &[], 100);
        answer(&w, &[], &[], 200);
        tool(&w, "read_file", false, 250);
        credit_conversation(&w.db, &w.mem, &w.conv);
        let c = credit_of(&w, "lessons", "check-header");
        assert_eq!((c.helped, c.hurt), (0, 0), "the failure came in a turn it was not part of");
    }

    #[test]
    fn a_rewound_turn_hurts_a_cued_lesson() {
        let w = world();
        lesson(&w, "check-header", Some("tool:read_file"));
        let first = w
            .db
            .append_message(
                &w.conv,
                &NewMessage {
                    role: "user".into(),
                    content: "do it".into(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        w.db.exec_for_tests("UPDATE messages SET created_at = 50 WHERE id = ?1", [&first.id]);
        answer(&w, &["check-header"], &[], 100);
        let _ = w.db.append_session_event(
            &w.conv,
            None,
            crate::agent::log::REWOUND_KIND,
            &serde_json::json!({ "messageId": first.id, "preview": "do it" }),
        );
        credit_conversation(&w.db, &w.mem, &w.conv);
        assert_eq!(credit_of(&w, "lessons", "check-header").hurt, 1);
    }

    #[test]
    fn a_press_of_outdated_hurts_the_fact_on_the_answer_that_carried_it() {
        let w = world();
        w.mem
            .save(
                &w.db,
                &Fact {
                    name: "lives-in-hamburg".into(),
                    description: "Lives in Hamburg".into(),
                    kind: "fact".into(),
                    created: String::new(),
                    source_conversation: None,
                    body: "Lives in Hamburg.".into(),
                    scope: None,
                    recurrence: None,
                    last_seen: None,
                    expires_at: None,
                    origin: None,
                    project: None,
                    cue: None,
                    confidence: None,
                    status: None,
                    supersedes: None,
                    superseded_by: None,
                    evidence: Vec::new(),
                    valid_from: None,
                },
            )
            .unwrap();
        answer(&w, &[], &["lives-in-hamburg"], 100);
        let _ = w.db.append_session_event(
            &w.conv,
            None,
            crate::agent::log::COMMAND_KIND,
            &serde_json::json!({ "name": OUTDATED, "args": "lives-in-hamburg", "by": "user", "outcome": "done" }),
        );
        w.db.exec_for_tests("UPDATE session_events SET created_at = 150", []);
        credit_conversation(&w.db, &w.mem, &w.conv);
        let c = credit_of(&w, "facts", "lives-in-hamburg");
        assert_eq!((c.chances, c.helped, c.hurt), (1, 0, 1));
    }

    #[test]
    fn a_forgotten_entry_is_not_given_a_row_back() {
        let w = world();
        answer(&w, &["gone"], &["also-gone"], 100);
        assert_eq!(credit_conversation(&w.db, &w.mem, &w.conv), 0);
        assert!(w.db.memory_credit_map("lessons").unwrap().is_empty());
    }

    #[test]
    fn a_conversation_without_recorded_context_credits_nothing() {
        let w = world();
        assert_eq!(credit_conversation(&w.db, &w.mem, &w.conv), 0);
    }
}
