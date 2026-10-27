//! Encoding by rules (`EPI-2`, `COGNITION_PLAN` III.1 steps 1 to 3).
//!
//! What a finished conversation was, found without a model: the signals it
//! left, how it ended, and what kind of task it was. The reflection pass writes
//! the result to the conversation row, runs credit on it (`agent::credit`), and
//! only then asks a model for the words (goal, digest, candidates).
//!
//! Every rule here reads rows that already exist. None of it guesses at what
//! the user meant: a conversation that fits no rule is `unknown`, and `unknown`
//! is never shown back to the user.

use serde::Serialize;

use crate::db::Db;

/// How a conversation ended.
pub const DONE: &str = "done";
pub const PARTIAL: &str = "partial";
pub const FAILED: &str = "failed";
pub const ABANDONED: &str = "abandoned";
pub const UNKNOWN: &str = "unknown";

/// What the rules found in one conversation. This is `signals_json`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Signals {
    /// Messages in the conversation.
    pub messages: usize,
    /// Tools used, most used first.
    pub tools: Vec<(String, i64)>,
    /// Tools that failed, most failed first.
    pub tool_failures: Vec<(String, i64)>,
    /// Failed calls that a later call of the same tool fixed (`tool_fixes`).
    pub fixes: usize,
    /// What the user did to correct me (`agent::log::user_signals`).
    pub user_signals: Vec<String>,
    pub rewound: bool,
    /// The last tool call, or the last check of an answer, failed.
    pub ended_on_failure: bool,
    /// The last message is the user's and is not a thanks: nothing answered it.
    pub unanswered: bool,
    /// The last user turn is a short thanks or "that works".
    pub last_user_accepts: bool,
    /// Claims of the latest check of an answer, by finding.
    pub claims_supported: usize,
    pub claims_refuted: usize,
    /// Kinds of the artifacts the conversation made.
    pub artifacts: Vec<String>,
    /// Scripts and commands that ran and came back clean.
    pub clean_runs: i64,
    /// A working folder is attached.
    pub folder: bool,
    /// From the durable session state (`remember` tool).
    pub entities: Vec<String>,
    pub constraints: usize,
    pub decisions: usize,
}

/// Run the rules over one conversation. Every read that fails reads as empty:
/// a signal that could not be read is a signal not found.
pub fn gather(db: &Db, conversation_id: &str) -> Signals {
    let messages = db.list_messages(conversation_id).unwrap_or_default();
    let tools = db.tools_used_in(conversation_id).unwrap_or_default();
    let clean_runs = ["run_code", "run_command", "run_task"]
        .iter()
        .map(|t| db.tool_results_between(conversation_id, t, 0, None).map(|(ok, _)| ok).unwrap_or(0))
        .sum();

    let last_check = db.verifications_for(conversation_id).ok().and_then(|mut v| v.pop());
    let (claims_supported, claims_refuted) = last_check
        .as_ref()
        .map(|c| count_findings(&c.claims_json))
        .unwrap_or((0, 0));

    let (entities, constraints, decisions) = db
        .get_session_state(conversation_id)
        .ok()
        .flatten()
        .map(|s| read_state(&s))
        .unwrap_or_default();

    let last_user_accepts =
        messages.iter().rev().find(|m| m.role == "user").is_some_and(|m| accepts(&m.content));

    Signals {
        messages: messages.len(),
        tools,
        tool_failures: db.tool_failures_in(conversation_id).unwrap_or_default(),
        fixes: db.tool_fixes_in(conversation_id).map(|f| f.len()).unwrap_or(0),
        user_signals: super::log::user_signals(db, conversation_id),
        rewound: super::log::was_rewound(db, conversation_id),
        ended_on_failure: db.last_tool_ok(conversation_id).ok().flatten() == Some(false) || claims_refuted > 0,
        // A thanks is the end of a conversation, not a question left hanging.
        unanswered: messages.last().is_some_and(|m| m.role == "user") && !last_user_accepts,
        last_user_accepts,
        claims_supported,
        claims_refuted,
        artifacts: db
            .list_artifacts(conversation_id)
            .unwrap_or_default()
            .into_iter()
            .map(|a| a.kind)
            .collect(),
        clean_runs,
        folder: db.conversation_folder(conversation_id).ok().and_then(|(f, _)| f).is_some(),
        entities,
        constraints,
        decisions,
    }
}

/// `(supported, refuted)` among the claims of one stored check. The JSON is
/// either the list of claims or an object holding it under `claims`.
fn count_findings(claims_json: &str) -> (usize, usize) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(claims_json) else { return (0, 0) };
    let list = v.as_array().or_else(|| v.get("claims").and_then(|c| c.as_array()));
    let Some(list) = list else { return (0, 0) };
    let count = |finding: &str| list.iter().filter(|c| c["finding"].as_str() == Some(finding)).count();
    (count("supported"), count("refuted"))
}

/// The entity names the session state holds (up to 8).
pub(crate) fn state_entities(state_json: &str) -> Vec<String> {
    read_state(state_json).0
}

/// Entity names (up to 8), and the number of constraints and decisions, from the
/// session state as the `remember` tool wrote it.
fn read_state(state_json: &str) -> (Vec<String>, usize, usize) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(state_json) else { return Default::default() };
    let size = |key: &str| match &v[key] {
        serde_json::Value::Array(a) => a.len(),
        serde_json::Value::Object(o) => o.len(),
        _ => 0,
    };
    let entities = match &v["entities"] {
        serde_json::Value::Array(a) => a.iter().filter_map(|e| e.as_str().map(str::to_string)).collect(),
        serde_json::Value::Object(o) => o.keys().cloned().collect(),
        _ => Vec::new(),
    };
    let entities: Vec<String> = entities.into_iter().take(8).collect();
    (entities, size("constraints"), size("decisions"))
}

/// Is this user turn a short thanks or "that works"? A long message is a new
/// request even when it starts with "thanks", and a turn that says "but" or "not"
/// is a correction even when it says "good".
pub fn accepts(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    if t.is_empty() || t.chars().count() > 80 {
        return false;
    }
    let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).collect();
    const AGAINST: &[&str] = &[
        "but", "not", "no", "wrong", "still", "doesn't", "didn't", "isn't", "don't", "nope", "aber", "nicht", "falsch",
        "noch", "immer", "kein", "keine",
    ];
    if words.iter().any(|w| AGAINST.contains(w)) {
        return false;
    }
    const FOR: &[&str] = &[
        "thanks", "thank", "thx", "perfect", "great", "good", "ok", "okay", "works", "worked", "nice", "awesome",
        "done", "danke", "passt", "super", "top", "prima", "gut", "perfekt", "klasse",
    ];
    words.iter().any(|w| FOR.contains(w))
}

/// How the conversation ended, by the plan's rules (III.1 step 2).
pub fn outcome(s: &Signals) -> &'static str {
    if s.rewound || s.unanswered {
        return ABANDONED;
    }
    if s.messages < 2 {
        return UNKNOWN;
    }
    if s.ended_on_failure {
        return FAILED;
    }
    if !s.user_signals.is_empty() && !s.last_user_accepts {
        return PARTIAL;
    }
    DONE
}

/// What kind of work it was, by the plan's rules (III.1 step 3): mail tools are
/// `mail`, code tools are `code`, a document made is a `report`, a working
/// folder is `code`, and anything else is `unknown`.
pub fn task_kind(s: &Signals) -> &'static str {
    let used = |names: &[&str]| s.tools.iter().any(|(t, _)| names.contains(&t.as_str()));
    if used(&["list_mail", "search_mail", "read_mail", "send_mail", "reply_mail"]) {
        "mail"
    } else if used(&["run_code", "run_command", "run_task", "find_symbol"]) {
        "code"
    } else if s.artifacts.iter().any(|k| k == "markdown" || k == "html") {
        "report"
    } else if s.folder {
        "code"
    } else {
        UNKNOWN
    }
}

/// The signals as `signals_json`.
pub fn signals_json(s: &Signals) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewMessage;

    fn say(db: &Db, conv: &str, role: &str, text: &str) {
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
        .unwrap();
    }

    fn chat(db: &Db) -> String {
        db.create_conversation("A chat", None, false).unwrap().id
    }

    #[test]
    fn a_short_thanks_accepts_and_a_correction_does_not() {
        assert!(accepts("Thanks, that works!"));
        assert!(accepts("perfekt, danke"));
        assert!(!accepts("Thanks, but the totals are still wrong"));
        assert!(!accepts("Nicht ganz, aber gut"));
        assert!(!accepts(&format!("thanks. {}", "Now please also do the other report. ".repeat(4))));
        assert!(!accepts(""));
    }

    #[test]
    fn a_fix_in_the_middle_and_a_thanks_at_the_end_is_done() {
        let db = Db::open_in_memory().unwrap();
        let id = chat(&db);
        say(&db, &id, "user", "Summarise the CSV");
        say(&db, &id, "assistant", "Done.");
        say(&db, &id, "user", "Thanks, perfect");
        db.add_tool_stat("m", "read_file", &id, false);
        db.add_tool_stat("m", "read_file", &id, true);
        let s = gather(&db, &id);
        assert_eq!(s.tool_failures, vec![("read_file".to_string(), 1)]);
        assert!(!s.ended_on_failure, "the last call succeeded");
        assert!(s.last_user_accepts);
        assert_eq!(outcome(&s), DONE);
    }

    #[test]
    fn a_run_that_ends_on_a_failed_tool_is_failed() {
        let db = Db::open_in_memory().unwrap();
        let id = chat(&db);
        say(&db, &id, "user", "Write the file");
        say(&db, &id, "assistant", "I could not.");
        db.add_tool_stat("m", "write_file", &id, false);
        assert_eq!(outcome(&gather(&db, &id)), FAILED);
    }

    #[test]
    fn a_rewound_or_unanswered_chat_is_abandoned() {
        let db = Db::open_in_memory().unwrap();
        let rewound = chat(&db);
        say(&db, &rewound, "user", "Do it");
        say(&db, &rewound, "assistant", "Done");
        let _ = db.append_session_event(
            &rewound,
            None,
            super::super::log::REWOUND_KIND,
            &serde_json::json!({ "preview": "Do it" }),
        );
        assert_eq!(outcome(&gather(&db, &rewound)), ABANDONED);

        let left = chat(&db);
        say(&db, &left, "user", "First");
        say(&db, &left, "assistant", "Answer");
        say(&db, &left, "user", "Second, unanswered");
        assert_eq!(outcome(&gather(&db, &left)), ABANDONED);
    }

    #[test]
    fn a_correction_without_a_thanks_is_partial_and_a_one_liner_is_unknown() {
        let db = Db::open_in_memory().unwrap();
        let id = chat(&db);
        say(&db, &id, "user", "Plan the trip");
        say(&db, &id, "assistant", "Here it is.");
        super::super::log::record_command(
            &db,
            &id,
            None,
            &super::super::log::CommandTrace {
                name: "forget".into(),
                args: "tea".into(),
                by: "user".into(),
                outcome: "done".into(),
                note: Some("forget that I like tea".into()),
                message_id: None,
            },
        );
        assert_eq!(outcome(&gather(&db, &id)), PARTIAL);

        let one = chat(&db);
        say(&db, &one, "user", "hi");
        assert_eq!(outcome(&gather(&db, &one)), ABANDONED, "an unanswered one-liner was left");
        let two = chat(&db);
        say(&db, &two, "assistant", "hello");
        assert_eq!(outcome(&gather(&db, &two)), UNKNOWN);
    }

    #[test]
    fn the_task_kind_follows_the_tools_the_artifacts_and_the_folder() {
        let db = Db::open_in_memory().unwrap();
        let mail = chat(&db);
        db.add_tool_stat("m", "search_mail", &mail, true);
        assert_eq!(task_kind(&gather(&db, &mail)), "mail");

        let code = chat(&db);
        db.add_tool_stat("m", "run_command", &code, true);
        let s = gather(&db, &code);
        assert_eq!(task_kind(&s), "code");
        assert_eq!(s.clean_runs, 1);

        let report = chat(&db);
        db.add_artifact(Some(&report), "Expenses", "markdown", "# Expenses", None).unwrap();
        assert_eq!(task_kind(&gather(&db, &report)), "report");

        let folder = chat(&db);
        db.set_conversation_folder(&folder, Some("/tmp/x")).unwrap();
        assert_eq!(task_kind(&gather(&db, &folder)), "code");

        assert_eq!(task_kind(&gather(&db, &chat(&db))), UNKNOWN);
    }

    #[test]
    fn the_session_state_and_the_last_check_are_read() {
        let (entities, constraints, decisions) =
            read_state(r#"{"entities":["Anna","Sparkasse"],"constraints":["EUR only"],"decisions":{"a":1,"b":2}}"#);
        assert_eq!(entities, vec!["Anna", "Sparkasse"]);
        assert_eq!((constraints, decisions), (1, 2));
        assert_eq!(count_findings(r#"[{"finding":"supported"},{"finding":"refuted"},{"finding":"refuted"}]"#), (1, 2));
        assert_eq!(count_findings(r#"{"claims":[{"finding":"supported"}]}"#), (1, 0));
        assert_eq!(count_findings("not json"), (0, 0));
    }
}
