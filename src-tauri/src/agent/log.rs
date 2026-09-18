//! `CTX-2`: the session log — the model's view of a conversation, append-only.
//!
//! The rule this file exists to keep: **anything the model saw must be
//! reconstructible from these rows.** A run writes what it sent; `replay` reads
//! it back as the identical message array. That is what makes resume possible
//! at all — an interrupted run has tool results that cost real time and real
//! money, and starting over throws every one of them away.
//!
//! ## Why a `prompt` row
//!
//! The plan's kind list has no `prompt`, because it assumes `CTX-3` has already
//! moved prompt assembly into Rust, at which point the opening system and user
//! turns are rows this loop writes itself. Assembly still lives in `store.ts`
//! today, so the loop is handed an array it did not build and cannot take
//! apart without guessing. It therefore stores that array whole, in one row,
//! and every message the run appends after it as its own row. Replay is the
//! same either way, and when `CTX-3` lands this row splits into the per-kind
//! rows the plan describes without changing a single reader.

use crate::db::Db;

/// Writes one run's view of the conversation. Cloneable-free by design: it
/// borrows the database and the ids, so there is exactly one of these per run
/// and no chance of two disagreeing about the sequence.
pub struct SessionLog<'a> {
    db: &'a Db,
    conversation_id: &'a str,
    run_id: &'a str,
}

impl<'a> SessionLog<'a> {
    pub fn new(db: &'a Db, conversation_id: &'a str, run_id: &'a str) -> Self {
        Self { db, conversation_id, run_id }
    }

    /// A logging failure must never take a run down with it: the log is a record
    /// of the work, not the work. Every write goes through here.
    fn write(&self, kind: &str, payload: &serde_json::Value) {
        let _ = self
            .db
            .append_session_event(self.conversation_id, Some(self.run_id), kind, payload);
    }

    /// The assembled array this run started from.
    pub fn prompt(&self, messages: &[serde_json::Value]) {
        self.write("prompt", &serde_json::Value::Array(messages.to_vec()));
    }

    /// One message appended to the live transcript, tagged by what it is. The
    /// kind is derived from the message rather than passed in, so a new push
    /// site cannot label itself wrongly.
    pub fn appended(&self, message: &serde_json::Value) {
        self.write(kind_of(message), message);
    }

    /// A steering instruction, recorded as itself as well as as the user message
    /// it becomes — the two are the same text but not the same event, and only
    /// the log can say the difference later.
    pub fn steered(&self, text: &str) {
        self.write("steer", &serde_json::json!({ "text": text }));
    }

    /// How the run ended. Its absence is meaningful: a run with no `stop` row
    /// was killed with the app, which is the case resume most wants to catch.
    pub fn stopped(&self, reason: &str, steps: usize) {
        self.write("stop", &serde_json::json!({ "reason": reason, "steps": steps }));
    }
}

/// `CTX-5`: the log's name for a compaction.
pub const COMPACTION_KIND: &str = "summary";

/// `PLN-4`: the log's name for a plan write.
pub const PLAN_KIND: &str = "plan";

/// Record the plan as it stands after a `plan` call. Best effort, like every
/// other write here.
///
/// Written on every change rather than once at the end, because the log is what
/// resume and fork read: a run killed with the app must come back holding the
/// plan it had, not the plan it started with.
pub fn record_plan(db: &Db, conversation_id: &str, run_id: &str, plan: &super::plan::Plan) {
    let Ok(payload) = serde_json::to_value(plan) else { return };
    let _ = db.append_session_event(conversation_id, Some(run_id), PLAN_KIND, &payload);
}

/// The plan this conversation's log ends with, if any.
///
/// Conversation-scoped rather than run-scoped on purpose: a fork copies the rows
/// with their original `run_id`, so asking by run id alone would find the
/// original's rows as well as the fork's. Asking a conversation for its own last
/// plan is the question both resume and a reopened fork actually have.
pub fn last_plan(db: &Db, conversation_id: &str) -> Option<super::plan::Plan> {
    let rows = db.session_events(conversation_id).ok()?;
    rows.into_iter()
        .rev()
        .find(|r| r.kind == PLAN_KIND)
        .and_then(|r| serde_json::from_str::<super::plan::Plan>(&r.payload_json).ok())
}

/// One compaction, as it goes into the log and comes back out.
///
/// The point of the row is that a summary can say **what it replaced**.
/// `conversations.summary` is a single column: it holds the newest summary and
/// the message it runs up to, and a second compaction overwrites the first, so
/// the conversation keeps no account of how it got compressed. The column stays
/// — it is what assembly reads, and it is the fast path — but it is now the
/// *latest* state of something the log records in full.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Compaction {
    /// The summary text, exactly as it was written into the conversation.
    pub text: String,
    /// The first message this summary covers: the message after the previous
    /// boundary, or the start of the conversation. `None` when the previous
    /// boundary could not be resolved — an older row, or a deleted message.
    pub from_message_id: Option<String>,
    /// The last message this summary covers. Turns after it are still sent word
    /// for word.
    pub upto_message_id: String,
    /// How many messages the summary stands in for.
    pub replaced: usize,
    /// Whether this summary folded an earlier one into itself. A `true` here is
    /// the honest warning: the text has been through a model twice, so detail
    /// the first pass dropped is not recoverable from the second.
    pub merged_earlier: bool,
}

/// Record a compaction. No `run_id`: compaction happens between runs, while the
/// next request is being assembled, and pinning it to a run would make it look
/// like something a run did.
///
/// Best effort, like every other write in this file. A conversation that
/// compacted but failed to log it is still correctly compacted.
pub fn record_compaction(db: &Db, conversation_id: &str, compaction: &Compaction) {
    let Ok(payload) = serde_json::to_value(compaction) else { return };
    let _ = db.append_session_event(conversation_id, None, COMPACTION_KIND, &payload);
}

/// Every compaction this conversation has been through, oldest first, each with
/// the moment it happened.
///
/// A row whose payload will not parse is skipped rather than failing the read:
/// one unreadable row must not hide the others.
pub fn compactions(db: &Db, conversation_id: &str) -> Vec<(i64, Compaction)> {
    let Ok(rows) = db.session_events(conversation_id) else {
        return Vec::new();
    };
    rows.into_iter()
        .filter(|r| r.kind == COMPACTION_KIND)
        .filter_map(|r| {
            serde_json::from_str::<Compaction>(&r.payload_json).ok().map(|c| (r.created_at, c))
        })
        .collect()
}

/// Classify a transcript message for the log. `system_note` rather than
/// `system` because by this point in the loop every system message is one the
/// loop itself inserted — the wrap-up prompt, a folder brief — never the
/// persona.
fn kind_of(message: &serde_json::Value) -> &'static str {
    if message.get("tool_call_id").is_some() {
        return "tool_result";
    }
    match message["role"].as_str() {
        Some("assistant") if message.get("tool_calls").is_some() => "tool_call",
        Some("assistant") => "assistant",
        Some("user") => "user",
        Some("tool") => "tool_result",
        _ => "system_note",
    }
}

/// `CTX-T2`: rebuild the exact message array a run sent, from its rows.
///
/// Rows that describe the run rather than what it sent — `steer`, `stop`,
/// `summary` and `plan` — are skipped. A `plan` row is state *about* the run,
/// not a turn in it: the resumed run is handed the plan itself (`RunContext`),
/// which then renders it into the transcript exactly as the original did, so
/// replaying the row as well would put the plan in twice and pin the older copy
/// there for good. The steering text is already in the log a second
/// time as the user message it became, and replaying it twice would put it in
/// the transcript twice. A `summary` row is a record *about* the conversation,
/// not a message in it; replaying one would hand the model a bookkeeping object
/// where a turn belongs. (It cannot reach this function today — a compaction
/// row carries no `run_id` — but replay is the one reader that must never guess
/// at a kind it does not know.)
pub fn replay(db: &Db, run_id: &str) -> Vec<serde_json::Value> {
    let Ok(rows) = db.session_events_for_run(run_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows {
        if matches!(row.kind.as_str(), "steer" | "stop" | COMPACTION_KIND | PLAN_KIND) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&row.payload_json) else {
            continue;
        };
        match value {
            serde_json::Value::Array(items) if row.kind == "prompt" => out.extend(items),
            other => out.push(other),
        }
    }
    out
}

/// What resume tells the model when it picks a run back up. It says the run was
/// interrupted rather than pretending it flowed on, because the transcript the
/// model is looking at ends mid-thought and an unexplained gap invites it to
/// start the whole task again.
pub const RESUME_PROMPT: &str = "You were interrupted before you finished. Everything above is your own work so far, including every tool result. Carry on from where you stopped — do not start again, and do not repeat work that is already above.";

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().expect("in-memory db")
    }

    fn conv(db: &Db) -> String {
        db.create_conversation("A chat", None, false).unwrap().id
    }

    #[test]
    fn a_message_is_labelled_by_what_it_is() {
        assert_eq!(kind_of(&serde_json::json!({ "role": "user", "content": "hi" })), "user");
        assert_eq!(
            kind_of(&serde_json::json!({ "role": "assistant", "content": "hi" })),
            "assistant"
        );
        assert_eq!(
            kind_of(&serde_json::json!({ "role": "assistant", "tool_calls": [] })),
            "tool_call"
        );
        assert_eq!(
            kind_of(&serde_json::json!({ "role": "tool", "tool_call_id": "c1", "content": "ok" })),
            "tool_result"
        );
        assert_eq!(kind_of(&serde_json::json!({ "role": "system", "content": "x" })), "system_note");
    }

    /// `CTX-T2`: what went in comes back out, in order, unchanged.
    #[test]
    fn replaying_a_run_reproduces_the_exact_message_array() {
        let db = db();
        let conversation_id = conv(&db);
        let log = SessionLog::new(&db, &conversation_id, "run-1");

        let prompt = vec![
            serde_json::json!({ "role": "system", "content": "You are Poiesis." }),
            serde_json::json!({ "role": "user", "content": "read the file" }),
        ];
        let call = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{ "id": "c1", "type": "function",
                             "function": { "name": "read_file", "arguments": "{}" } }]
        });
        let result = serde_json::json!({ "role": "tool", "tool_call_id": "c1", "content": "40 lines" });

        log.prompt(&prompt);
        log.appended(&call);
        log.appended(&result);
        log.steered("also check the changelog");
        log.appended(&serde_json::json!({ "role": "user", "content": "also check the changelog" }));
        log.stopped("aborted", 3);

        let mut expected = prompt.clone();
        expected.push(call);
        expected.push(result);
        expected.push(serde_json::json!({ "role": "user", "content": "also check the changelog" }));

        assert_eq!(replay(&db, "run-1"), expected);
    }

    #[test]
    fn a_run_that_never_wrote_a_stop_row_is_the_one_resume_is_for() {
        let db = db();
        let conversation_id = conv(&db);
        SessionLog::new(&db, &conversation_id, "run-1")
            .prompt(&[serde_json::json!({ "role": "user", "content": "go" })]);
        assert_eq!(
            db.last_logged_run(&conversation_id).unwrap(),
            Some(("run-1".to_string(), None))
        );

        SessionLog::new(&db, &conversation_id, "run-1").stopped("timeout", 12);
        assert_eq!(
            db.last_logged_run(&conversation_id).unwrap(),
            Some(("run-1".to_string(), Some("timeout".to_string())))
        );
    }

    fn compaction(text: &str, upto: &str, replaced: usize, merged: bool) -> Compaction {
        Compaction {
            text: text.into(),
            from_message_id: Some("m1".into()),
            upto_message_id: upto.into(),
            replaced,
            merged_earlier: merged,
        }
    }

    /// `CTX-5`: every compaction is kept, in order, with what it replaced.
    ///
    /// This is the whole point of the row. The `conversations.summary` column
    /// holds one summary; compacting twice overwrites the first with a summary
    /// of a summary, and the original wording is gone. The log keeps both, so
    /// the second one can be read next to the first it folded in.
    #[test]
    fn every_compaction_is_kept_not_just_the_latest() {
        let db = db();
        let conversation_id = conv(&db);

        record_compaction(&db, &conversation_id, &compaction("FACTS: the first ten turns", "m10", 10, false));
        record_compaction(&db, &conversation_id, &compaction("FACTS: the first thirty", "m30", 20, true));

        let found = compactions(&db, &conversation_id);
        assert_eq!(found.len(), 2, "the second did not overwrite the first");

        let (_, first) = &found[0];
        assert_eq!(first.upto_message_id, "m10");
        assert_eq!(first.replaced, 10);
        assert!(!first.merged_earlier, "there was nothing to fold in yet");

        let (_, second) = &found[1];
        assert_eq!(second.replaced, 20, "what this pass covered, not the running total");
        assert!(second.merged_earlier, "it swallowed the first, and says so");
    }

    /// A conversation that has never been compacted has no history to show,
    /// rather than an empty-looking one.
    #[test]
    fn a_conversation_that_was_never_compacted_has_no_summaries() {
        let db = db();
        let conversation_id = conv(&db);
        SessionLog::new(&db, &conversation_id, "run-1")
            .prompt(&[serde_json::json!({ "role": "user", "content": "go" })]);
        assert!(compactions(&db, &conversation_id).is_empty());
    }

    /// A compaction row is a note *about* the conversation, not a turn in it.
    /// Replaying one into a resumed run would hand the model a bookkeeping
    /// object where a message belongs.
    #[test]
    fn a_summary_row_is_never_replayed_as_a_message() {
        let db = db();
        let conversation_id = conv(&db);
        let log = SessionLog::new(&db, &conversation_id, "run-1");
        log.prompt(&[serde_json::json!({ "role": "user", "content": "go" })]);
        // Written against the run on purpose: the real writer never does this,
        // and replay must not depend on that staying true.
        let _ = db.append_session_event(
            &conversation_id,
            Some("run-1"),
            COMPACTION_KIND,
            &serde_json::to_value(compaction("FACTS: …", "m3", 3, false)).unwrap(),
        );
        log.appended(&serde_json::json!({ "role": "assistant", "content": "done" }));

        let replayed = replay(&db, "run-1");
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[1]["content"], "done");
    }

    /// `PLN-T3`: a plan round-trips through the log, and `replay` skips it — the
    /// same shape as the summary test above, for the same reason.
    #[test]
    fn a_plan_round_trips_through_the_log_and_is_never_replayed_as_a_message() {
        let db = db();
        let conversation_id = conv(&db);
        let log = SessionLog::new(&db, &conversation_id, "run-1");
        log.prompt(&[serde_json::json!({ "role": "user", "content": "build it" })]);

        let held = std::sync::Mutex::new(super::super::plan::Plan::default());
        super::super::plan::execute(
            &held,
            &serde_json::json!({ "items": ["read the spec", "write the file"] }),
        )
        .unwrap();
        super::super::plan::execute(
            &held,
            &serde_json::json!({ "update": { "index": 1, "status": "done" } }),
        )
        .unwrap();
        record_plan(&db, &conversation_id, "run-1", &held.lock().unwrap());
        log.appended(&serde_json::json!({ "role": "assistant", "content": "done" }));

        let back = last_plan(&db, &conversation_id).expect("the plan comes back out");
        assert_eq!(back.items.len(), 2);
        assert_eq!(back.items[0].status, super::super::plan::PlanStatus::Done);
        assert_eq!(back.items[1].text, "write the file");

        let replayed = replay(&db, "run-1");
        assert_eq!(replayed.len(), 2, "the plan row is not a message");
        assert_eq!(replayed[1]["content"], "done");
    }

    /// A conversation that never planned has no plan, rather than an empty one
    /// that would render as a plan with no items.
    #[test]
    fn a_conversation_that_never_planned_has_no_plan() {
        let db = db();
        let conversation_id = conv(&db);
        SessionLog::new(&db, &conversation_id, "run-1")
            .prompt(&[serde_json::json!({ "role": "user", "content": "hi" })]);
        assert!(last_plan(&db, &conversation_id).is_none());
    }

    /// `PLN-T4`: fork and resume both carry the plan.
    ///
    /// This is deliberately the same test the summary-dropping bug in
    /// `fork_conversation` needed and did not have. Resume reads the plan back
    /// through `last_plan`; a fork copies the rows, so the branch answers the
    /// same question with the same plan — and the original is untouched by it.
    #[test]
    fn a_fork_and_a_resume_both_carry_the_plan() {
        let db = db();
        let conversation_id = conv(&db);
        let branch = conv(&db);
        let log = SessionLog::new(&db, &conversation_id, "run-1");
        log.prompt(&[serde_json::json!({ "role": "user", "content": "build it" })]);

        let held = std::sync::Mutex::new(super::super::plan::Plan::default());
        super::super::plan::execute(&held, &serde_json::json!({ "items": ["a", "b", "c"] })).unwrap();
        super::super::plan::execute(
            &held,
            &serde_json::json!({ "update": { "index": 1, "status": "done" } }),
        )
        .unwrap();
        record_plan(&db, &conversation_id, "run-1", &held.lock().unwrap());

        // Resume: the run was killed before it wrote a stop row, and the plan it
        // had is the plan it picks back up.
        let (run_id, reason) = db.last_logged_run(&conversation_id).unwrap().unwrap();
        assert_eq!(run_id, "run-1");
        assert_eq!(reason, None);
        let resumed = last_plan(&db, &conversation_id).unwrap();
        assert_eq!(resumed.items.len(), 3);
        assert_eq!(resumed.items[0].status, super::super::plan::PlanStatus::Done);

        // Fork: everything up to the cut, the plan row included.
        let upto = db.session_events(&conversation_id).unwrap().len() as i64;
        db.fork_session_events(&conversation_id, &branch, upto).unwrap();
        let forked = last_plan(&db, &branch).expect("the branch has the plan too");
        assert_eq!(forked.items.len(), 3);
        assert_eq!(forked.items[0].status, super::super::plan::PlanStatus::Done);
        assert!(last_plan(&db, &conversation_id).is_some(), "the original keeps its own");
    }

    /// `CTX-T3`: a fork takes the log up to the cut and nothing past it.
    #[test]
    fn forking_copies_events_up_to_a_point_and_no_further() {
        let db = db();
        let conversation_id = conv(&db);
        let other = conv(&db);
        let log = SessionLog::new(&db, &conversation_id, "run-1");
        for n in 0..5 {
            log.appended(&serde_json::json!({ "role": "user", "content": n.to_string() }));
        }

        let copied = db.fork_session_events(&conversation_id, &other, 3).unwrap();
        assert_eq!(copied, 3);

        let forked = db.session_events(&other).unwrap();
        assert_eq!(forked.len(), 3, "up to the cut, and nothing after it");
        assert_eq!(
            forked.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "renumbered from 1 — the fork owns its own history"
        );
        let contents: Vec<String> = forked
            .iter()
            .map(|e| serde_json::from_str::<serde_json::Value>(&e.payload_json).unwrap()["content"]
                .as_str()
                .unwrap()
                .to_string())
            .collect();
        assert_eq!(contents, vec!["0", "1", "2"]);
        assert_eq!(
            db.session_events(&conversation_id).unwrap().len(),
            5,
            "the source log is untouched by a fork"
        );
    }
}
