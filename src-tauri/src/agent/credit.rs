//! Credit (`CRD-2`, `COGNITION_PLAN` III.3): did a remembered thing help?
//!
//! `messages.context_json` says which facts and lessons reached each answer.
//! At the end of encoding, the answers of one conversation are read once and
//! each item is given what it earned, on the `memory_usage` row it already has:
//! `chances` (answers it reached), `helped`, `hurt`. Nothing here asks a model.
//!
//! What counts, with the evidence this build has:
//!
//! - **A lesson cued to a tool** (`cue: tool:<name>`), or one that fires at a tool
//!   (`fires_on`, `HAB-1`). The tool ran after the
//!   answer and did not fail: helped. It failed: hurt. The turn was gone back
//!   on: hurt. The tool did not run: no effect.
//! - **Anything the user pressed Outdated or Not relevant on** (a command trace
//!   in the session log): hurt, on the last answer that carried it.
//! - **Surprise** (`MET-2`): a failure after a turn whose decisions were sure counts
//!   double, and so does a success after a turn whose decisions were unsure.
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

/// A turn's decisions were this sure or surer on average: a failure after them is
/// a surprise (`MET-2`, III.7).
pub const SURE: f32 = 0.85;
/// ...and this unsure or less: a success after them is a surprise.
pub const UNSURE: f32 = 0.6;

/// How sure the decision model was in `[from, to)` of this conversation: the
/// mean of the top probability of each answered decision. `None` when it was
/// not asked, and then nothing is a surprise.
fn sureness(db: &Db, conversation_id: &str, from: i64, to: i64) -> Option<f32> {
    let tops: Vec<f32> = db
        .decisions_between(conversation_id, from, to)
        .unwrap_or_default()
        .iter()
        .filter_map(|d| crate::agent::decide::calibration::probs_of(&d.answers))
        .filter_map(|(_, probs)| probs.into_iter().reduce(f32::max))
        .collect();
    (!tops.is_empty()).then(|| tops.iter().sum::<f32>() / tops.len() as f32)
}

/// What one `hurt` and one `helped` count for after a turn the model was this
/// sure about: a failure it was sure about counts double, and so does a success
/// it was unsure about. Everything else counts once.
fn surprise(sure: Option<f32>) -> (i64, i64) {
    match sure {
        Some(s) if s >= SURE => (2, 1),
        Some(s) if s <= UNSURE => (1, 2),
        _ => (1, 1),
    }
}

/// The tool a lesson is cued to, when it is.
fn tool_cue(cue: Option<&str>) -> Option<&str> {
    cue?.trim().strip_prefix("tool:").map(str::trim).filter(|t| !t.is_empty())
}

/// The settings key that says how far this conversation has been credited: the
/// time of the newest answer or press already counted.
fn credited_key(conversation_id: &str) -> String {
    format!("credit.upto.{conversation_id}")
}

/// Credit every item that reached an answer of this conversation, from where
/// the last call stopped. Returns how many items were credited. Safe to call
/// again: answers and presses already counted are not counted twice, and the
/// ones that came since are (a person who comes back to a chat after its first
/// reflection still teaches me). Safe on a conversation without any recorded
/// context: it credits nothing.
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
    let done_upto: Option<i64> =
        db.get_setting(&credited_key(conversation_id)).ok().flatten().and_then(|v| v.parse().ok());
    let is_new = |at: i64| done_upto.is_none_or(|w| at > w);
    let mut newest = done_upto.unwrap_or(i64::MIN);

    // Answers after the point the user went back to were abandoned.
    let rewound_from = rewind_point(db, conversation_id, &messages);
    let carries = |refs: &ContextRefs, name: &str| {
        refs.facts.iter().any(|n| n == name) || refs.lessons.iter().any(|n| n == name) || refs.fired.iter().any(|n| n == name)
    };
    // Entries the user pressed a button on, each with the answer it belongs to:
    // the last answer before the press that carried the entry, however many
    // answers came between.
    let pressed: Vec<(String, i64, usize)> = crate::agent::log::commands(db, conversation_id)
        .into_iter()
        .filter(|r| r.trace.name == OUTDATED || r.trace.name == NOT_RELEVANT)
        .filter_map(|r| {
            let name = r.trace.args.trim().to_string();
            let owner = answers
                .iter()
                .enumerate()
                .rev()
                .find(|(_, (i, refs))| messages[*i].created_at <= r.at && carries(refs, &name))
                .map(|(n, _)| n)?;
            Some((name, r.at, owner))
        })
        .collect();

    let mut tallies: HashMap<(&'static str, String), Tally> = HashMap::new();
    for (n, (i, refs)) in answers.iter().enumerate() {
        let msg = &messages[*i];
        let fresh = is_new(msg.created_at);
        let presses: Vec<&(String, i64, usize)> = pressed.iter().filter(|(_, at, owner)| *owner == n && is_new(*at)).collect();
        if !fresh && presses.is_empty() {
            continue;
        }
        let until = answers.get(n + 1).map(|(next, _)| messages[*next].created_at);
        let (hurt_w, helped_w) = surprise(sureness(db, conversation_id, msg.created_at, until.unwrap_or(i64::MAX)));

        let items = refs
            .facts
            .iter()
            .map(|name| (FACTS, name))
            .chain(refs.lessons.iter().map(|name| (LESSONS, name)))
            // `HAB-1`: a lesson that fired at a tool call reached the answer as surely
            // as one in the index. It is listed once even if both say so.
            .chain(refs.fired.iter().filter(|name| !refs.lessons.contains(name)).map(|name| (LESSONS, name)));
        for (collection, name) in items {
            let Some(entry) = mem.read_in(collection, name) else { continue };
            let hurt_by_press = presses.iter().any(|(arg, _, _)| arg == name);
            if !fresh && !hurt_by_press {
                continue;
            }
            let t = tallies.entry((collection, name.clone())).or_default();
            t.last_at = t.last_at.max(msg.created_at);

            if fresh {
                t.chances += 1;
                if collection == LESSONS {
                    let tool = tool_cue(entry.cue.as_deref()).or(entry.fires.as_ref().map(|f| f.on.as_str()));
                    if let Some(tool) = tool {
                        let (ok, failed) =
                            db.tool_results_between(conversation_id, tool, msg.created_at, until).unwrap_or((0, 0));
                        let gone_back = rewound_from.is_some_and(|from| *i > from);
                        if failed > 0 || gone_back {
                            t.hurt += hurt_w;
                        } else if ok > 0 {
                            t.helped += helped_w;
                        }
                    }
                }
            }
            if hurt_by_press {
                t.hurt += hurt_w;
            }
        }
        newest = newest.max(msg.created_at);
        for (_, at, _) in &presses {
            newest = newest.max(*at);
        }
    }

    let count = tallies.len();
    for ((collection, name), t) in tallies {
        let _ = db.add_memory_credit(collection, &name, t.chances, t.helped, t.hurt, t.last_at);
    }
    if newest > i64::MIN {
        let _ = db.set_setting(&credited_key(conversation_id), &newest.to_string());
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
                    fires: None,
                    trigger: None,
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

    /// `HAB-1`: a lesson that fires is not in the index, so only `fired` carries it. It
    /// is credited by the tool it fires on.
    #[test]
    fn a_lesson_that_fired_at_a_tool_is_credited_by_that_tool() {
        use crate::memory::Fires;
        let w = world();
        let mut l = w.mem.read_in(LESSONS, "x").unwrap_or_default();
        l.name = "check-header".into();
        l.description = "check the header".into();
        l.kind = "lesson".into();
        l.body = "Look at the first row.".into();
        l.fires = Some(Fires { on: "read_file".into(), when: None, check: None });
        w.mem.save_lesson(&w.db, &l).unwrap();
        let msg = w
            .db
            .append_message(
                &w.conv,
                &NewMessage {
                    role: "assistant".into(),
                    content: String::new(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        let refs = ContextRefs { fired: vec!["check-header".into()], ..Default::default() };
        w.db.finalize_message(&msg.id, "ok", None, Some(&serde_json::to_string(&refs).unwrap()), None, None).unwrap();
        w.db.exec_for_tests("UPDATE messages SET created_at = ?2 WHERE id = ?1", rusqlite::params![msg.id, 100]);
        tool(&w, "read_file", true, 120);
        credit_conversation(&w.db, &w.mem, &w.conv);
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

    /// A decision the model made in this conversation at `at`, with `p` for its yes.
    fn decided(w: &World, id: &str, p: f64, at: i64) {
        let answers = serde_json::json!({ "goal": { "p": p } });
        w.db.add_decision(&crate::db::decisions::NewDecision {
            id,
            conversation_id: Some(&w.conv),
            run_id: None,
            purpose: "goal-met",
            backend: "local",
            model: "m",
            ms: 1,
            calibrated: false,
            answers: &answers,
            input_tokens: 1,
            state_preview: "",
            state_hash: id,
            action: None,
        })
        .unwrap();
        w.db.set_decision_at(id, at);
    }

    /// `MET-2`: credit of a lesson cued to `read_file`, after a turn whose decision
    /// said `p`, when the tool then ran `ok` or failed.
    fn after_decision(p: Option<f64>, ok: bool) -> (i64, i64) {
        let w = world();
        lesson(&w, "check-header", Some("tool:read_file"));
        answer(&w, &["check-header"], &[], 100);
        if let Some(p) = p {
            decided(&w, "d1", p, 110);
        }
        tool(&w, "read_file", ok, 120);
        credit_conversation(&w.db, &w.mem, &w.conv);
        let c = credit_of(&w, "lessons", "check-header");
        (c.helped, c.hurt)
    }

    #[test]
    fn a_failure_the_decision_model_was_sure_about_counts_double() {
        assert_eq!(after_decision(Some(0.95), false), (0, 2));
        assert_eq!(after_decision(Some(0.7), false), (0, 1), "not sure enough to be a surprise");
        assert_eq!(after_decision(None, false), (0, 1), "no decision, no surprise");
    }

    #[test]
    fn a_success_the_decision_model_was_unsure_about_counts_double() {
        assert_eq!(after_decision(Some(0.5), true), (2, 0));
        assert_eq!(after_decision(Some(0.95), true), (1, 0), "a sure success is no surprise");
        assert_eq!(after_decision(None, true), (1, 0));
    }

    #[test]
    fn a_decision_from_another_turn_is_not_this_turns_surprise() {
        let w = world();
        lesson(&w, "check-header", Some("tool:read_file"));
        answer(&w, &["check-header"], &[], 100);
        answer(&w, &[], &[], 200);
        decided(&w, "later", 0.95, 250);
        tool(&w, "read_file", false, 120);
        credit_conversation(&w.db, &w.mem, &w.conv);
        assert_eq!(credit_of(&w, "lessons", "check-header").hurt, 1);
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
                    fires: None,
                    trigger: None,
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

    /// A press on an older answer's chip, after a newer answer that did not carry
    /// the entry, still counts against it.
    #[test]
    fn a_press_after_a_later_answer_still_hurts_the_entry_it_was_on() {
        let w = world();
        lesson(&w, "be-brief", None);
        answer(&w, &["be-brief"], &[], 100);
        answer(&w, &[], &[], 200);
        let _ = w.db.append_session_event(
            &w.conv,
            None,
            crate::agent::log::COMMAND_KIND,
            &serde_json::json!({ "name": NOT_RELEVANT, "args": "be-brief", "by": "user", "outcome": "done" }),
        );
        w.db.exec_for_tests("UPDATE session_events SET created_at = 250", []);
        credit_conversation(&w.db, &w.mem, &w.conv);
        let c = credit_of(&w, "lessons", "be-brief");
        assert_eq!((c.chances, c.hurt), (1, 1));
    }

    /// Credit picks up where it stopped: what came after the first reflection is
    /// counted, and nothing is counted twice.
    #[test]
    fn credit_counts_new_answers_and_presses_once_after_the_first_pass() {
        let w = world();
        lesson(&w, "check-header", Some("tool:read_file"));
        answer(&w, &["check-header"], &[], 100);
        tool(&w, "read_file", true, 120);
        credit_conversation(&w.db, &w.mem, &w.conv);
        credit_conversation(&w.db, &w.mem, &w.conv);
        let c = credit_of(&w, "lessons", "check-header");
        assert_eq!((c.chances, c.helped, c.hurt), (1, 1, 0), "asking again counts nothing twice");

        // The person comes back: a new answer that fails, and a press on it.
        answer(&w, &["check-header"], &[], 300);
        tool(&w, "read_file", false, 320);
        let _ = w.db.append_session_event(
            &w.conv,
            None,
            crate::agent::log::COMMAND_KIND,
            &serde_json::json!({ "name": OUTDATED, "args": "check-header", "by": "user", "outcome": "done" }),
        );
        w.db.exec_for_tests("UPDATE session_events SET created_at = 350", []);
        credit_conversation(&w.db, &w.mem, &w.conv);
        let c = credit_of(&w, "lessons", "check-header");
        assert_eq!((c.chances, c.helped, c.hurt), (2, 1, 2));
        credit_conversation(&w.db, &w.mem, &w.conv);
        assert_eq!(credit_of(&w, "lessons", "check-header").hurt, 2);
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
