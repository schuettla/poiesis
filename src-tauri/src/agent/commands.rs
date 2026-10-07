//! `REG-2` and `AGC-1..3`: the agent's side of the command registry.
//!
//! The manifest (`shared/commands.json`) is the one list of commands; the
//! frontend reads it for the `/` menu and this file reads it for what the agent
//! may do with it. Two loop-owned tools come out of it:
//!
//! - `harness` controls the agent's *own session*: clear old tool output, suggest
//!   one of the user's slash commands, ask to switch a mode, propose a schedule.
//!   It is not named `command` or `run`: `run_command` is a shell, and small
//!   models conflate the two.
//! - `ask_user` puts a real fork to the user and waits for the answer.
//!
//! Neither belongs to a toolset. Both touch state that lives on the run, so the
//! loop dispatches them the way it dispatches `plan`.
//!
//! Guidance on *when* to use them is in their tool descriptions and nowhere else
//! (Rule 8): the system prompt is behind the golden gate and does not change.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::autonomy::{autonomy_gate, Rung};
use crate::db::Db;

static MANIFEST: &str = include_str!("../../../shared/commands.json");

/// One manifest entry, as far as this side needs it. The frontend reads the
/// rest (`args`, `summary`) and unknown fields are ignored here.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSpec {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub section: String,
    pub kind: String,
    pub who: String,
    #[serde(default)]
    pub agent_class: Option<String>,
    #[serde(default)]
    pub suggestible: bool,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub trace: bool,
}

/// Every built-in command, parsed once. A manifest that does not parse is a
/// build-time mistake, so it panics in the first test rather than limping on.
pub fn manifest() -> &'static [CommandSpec] {
    static PARSED: OnceLock<Vec<CommandSpec>> = OnceLock::new();
    PARSED.get_or_init(|| {
        serde_json::from_str(MANIFEST).expect("shared/commands.json is not valid")
    })
}

/// What `harness{name}` can be, and the autonomy class that governs each.
/// Anything not here is user-only, whatever the manifest says.
pub const HARNESS: &[(&str, &str)] = &[
    ("compact", "context"),
    ("suggest", "suggest"),
    ("switch_mode", "modes"),
    ("schedule", "schedule"),
];

/// `harness_class`: which rung decides this call.
pub fn harness_class(name: &str) -> Option<&'static str> {
    HARNESS.iter().find(|(n, _)| *n == name).map(|(_, class)| *class)
}

/// `AGC-1`: the names `harness` may take in this run. A class set to `Off`
/// withdraws its name, so the model is never told about something it cannot do.
/// A run nobody is watching keeps only `compact`: a suggestion, a mode switch
/// and a schedule proposal all need a person to answer them.
pub fn agent_callable(db: &Db, headless: bool) -> Vec<&'static str> {
    HARNESS
        .iter()
        .filter(|(name, class)| (!headless || *name == "compact") && autonomy_gate(db, class) != Rung::Off)
        .map(|(name, _)| *name)
        .collect()
}

/// `AGC-2`: the commands the agent may *suggest*.
pub fn suggestible() -> Vec<&'static str> {
    manifest().iter().filter(|c| c.suggestible).map(|c| c.name.as_str()).collect()
}

/// The manifest name a suggested `/command` stands for, if it may be suggested.
pub fn suggestible_name(command: &str) -> Option<&'static str> {
    let word = command.trim().trim_start_matches('/').to_lowercase();
    manifest()
        .iter()
        .find(|c| c.suggestible && (c.name == word || c.aliases.iter().any(|a| *a == word)))
        .map(|c| c.name.as_str())
}

/// Is this one of the two loop-owned tools of this file?
pub fn handles(name: &str) -> bool {
    matches!(name, "harness" | "ask_user")
}

/// `AGC-1`: the `harness` spec for the names this run offers. `None` when there
/// are none: a tool that cannot do anything is not offered.
pub fn harness_spec(names: &[&str]) -> Option<serde_json::Value> {
    if names.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "type": "function",
        "function": {
            "name": "harness",
            "description": "Control my own working session, not the user's computer. compact: clear old tool output I no longer need word for word, to make room. suggest: offer the user one of their slash commands when it would clearly help them (at most once per command per conversation). switch_mode: ask the user to turn a mode on (they decide). schedule: propose running a task later or on a repeat (they decide). Never use this to do the task itself.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "enum": names },
                    "reason": { "type": "string", "description": "One sentence the user will read, in first person." },
                    "command": { "type": "string", "description": "suggest: the slash command, e.g. /skillify" },
                    "mode": { "type": "string", "enum": ["workspace", "plan_first"], "description": "switch_mode" },
                    "when": { "type": "string", "description": "schedule: e.g. 'every weekday at 9'" },
                    "task": { "type": "string", "description": "schedule: what to do" }
                },
                "required": ["name", "reason"]
            }
        }
    }))
}

/// `AGC-3`: the `ask_user` spec.
pub fn ask_user_spec() -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": "ask_user",
            "description": "Ask the user to decide something you cannot sensibly decide yourself: a real fork where the options lead to different work. Do not use it to confirm something you could just do, or to ask permission (permissions are handled for you). The run pauses until they answer. Offer 2-4 concrete options; they can always write their own.",
            "parameters": {
                "type": "object",
                "properties": {
                    "question": { "type": "string" },
                    "options": {
                        "type": "array",
                        "minItems": 2,
                        "maxItems": 4,
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": { "type": "string" },
                                "detail": { "type": "string" }
                            },
                            "required": ["label"]
                        }
                    },
                    "multi": { "type": "boolean", "description": "true if more than one option may be chosen" }
                },
                "required": ["question", "options"]
            }
        }
    })
}

/// What the model said in a `harness` call, with every field optional but the
/// two the spec requires, so a sloppy small model gets an answer, not a crash.
#[derive(Debug, Default, Deserialize)]
pub struct HarnessCall {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub when: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
}

/// What a `harness` call comes to once the rung, the manifest and this
/// conversation's history have been consulted. The loop only has to carry it
/// out, which is why the rules can be tested without a run.
#[derive(Debug, PartialEq)]
pub enum Act {
    /// Clear old tool output before the next turn (`CLR-4`).
    Compact,
    /// Nothing happens; the model is told this.
    Say(String),
    /// Offer one suggestion chip (`AGC-2`), and trace it.
    Suggest { command: String, reason: String },
    /// Ask the user (`AGC-4`), and trace it. `reply` is what the model reads.
    Propose { name: String, args: String, reason: String, payload: serde_json::Value, note: String, reply: String },
}

impl Act {
    /// `PLF-2`: while planning first, nothing changes on its own. A mode switch
    /// the `Auto` rung would have made becomes a question, like the `Ask` rung.
    /// Clearing old output stays: it touches only this run's own transcript.
    pub fn asked_only(self) -> Act {
        match self {
            Act::Propose { name, args, reason, mut payload, note, reply: _ }
                if payload.get("auto").and_then(|a| a.as_bool()) == Some(true) =>
            {
                payload["auto"] = serde_json::Value::Bool(false);
                Act::Propose {
                    name,
                    args,
                    reason,
                    payload,
                    note,
                    reply: "I'm planning first, so I've asked the user instead; carry on without waiting for the answer."
                        .into(),
                }
            }
            other => other,
        }
    }
}

/// `AGC-1`/`AGC-2`: decide one `harness` call.
///
/// `names` is what this run offered. `Auto` does it and shows it, `Ask` turns
/// it into a proposal the run does not wait on, and `Off` is refused here as
/// well as being absent from the enum, because a setting can change mid-run.
pub fn decide(
    db: &Db,
    conversation_id: &str,
    names: &[&str],
    call: &HarnessCall,
) -> Result<Act, String> {
    if !names.contains(&call.name.as_str()) {
        return Err(format!("I can't use \"{}\" here.", call.name));
    }
    let class = harness_class(&call.name).unwrap_or("modes");
    let rung = autonomy_gate(db, class);
    if rung == Rung::Off {
        return Err("The user has turned that off.".to_string());
    }
    let reason = call.reason.trim().to_string();
    let asked = "I've asked the user; carry on without waiting for the answer.".to_string();

    match call.name.as_str() {
        "compact" => {
            if rung == Rung::Ask {
                return Ok(Act::Propose {
                    name: "compact".into(),
                    args: String::new(),
                    note: format!("I asked to make room: {reason}"),
                    reason,
                    payload: serde_json::json!({}),
                    reply: asked,
                });
            }
            Ok(Act::Compact)
        }
        "suggest" => {
            let typed = call.command.as_deref().unwrap_or("");
            let Some(name) = suggestible_name(typed) else {
                return Err(format!("I can't suggest {} here.", typed.trim()));
            };
            // One per command per conversation (`AGC-2`).
            let already = super::log::commands(db, conversation_id)
                .iter()
                .any(|r| r.trace.name == "suggest" && r.trace.args == name);
            if already {
                return Ok(Act::Say("Already suggested; don't repeat it.".into()));
            }
            // `CPX-5`: twice declined, in any conversation, is a no.
            if super::log::declined_suggestions(db, name) >= 2 {
                return Ok(Act::Say(
                    "The user has said not now to that twice. Don't suggest it again.".into(),
                ));
            }
            Ok(Act::Suggest { command: name.to_string(), reason })
        }
        "switch_mode" => {
            let (mode, label) = match call.mode.as_deref() {
                Some("workspace") => ("workspace", "Workspace"),
                Some("plan_first") => ("plan_first", "Plan first"),
                _ => return Err("mode must be \"workspace\" or \"plan_first\".".to_string()),
            };
            let auto = rung == Rung::Auto;
            Ok(Act::Propose {
                name: "switch_mode".into(),
                args: mode.into(),
                note: format!("I asked to switch to {label}: {reason}"),
                reason,
                payload: serde_json::json!({ "mode": mode, "auto": auto }),
                reply: if auto {
                    "Done. It takes effect from the user's next message.".into()
                } else {
                    asked
                },
            })
        }
        "schedule" => {
            let task = call
                .task
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .ok_or_else(|| "`task` is required.".to_string())?;
            let when = call.when.as_deref().map(str::trim).filter(|w| !w.is_empty()).unwrap_or("later");
            Ok(Act::Propose {
                name: "schedule".into(),
                args: task.into(),
                note: format!("I asked to run this {when}: {task}"),
                reason,
                payload: serde_json::json!({ "when": when, "task": task }),
                reply: asked,
            })
        }
        other => Err(format!("I can't use \"{other}\" here.")),
    }
}

/// `CPX-3`: a `harness` call is never a bare tool row. It reads as the first
/// person doing something, in the run's own timeline.
pub fn describe_harness(args: &serde_json::Value) -> (String, String) {
    let call: HarnessCall = serde_json::from_value(args.clone()).unwrap_or_default();
    match call.name.as_str() {
        "compact" => ("made room".into(), String::new()),
        "suggest" => {
            let command = call.command.unwrap_or_default();
            ("suggested".into(), format!("/{}", command.trim().trim_start_matches('/')))
        }
        "switch_mode" => {
            let mode = if call.mode.as_deref() == Some("plan_first") { "Plan first" } else { "Workspace" };
            ("asked to switch to".into(), mode.into())
        }
        "schedule" => ("asked to schedule".into(), call.task.unwrap_or_default()),
        other => ("asked to".into(), other.to_string()),
    }
}

/// The step row for `ask_user`: `asked you: {question}`; `— you chose …` is
/// added by the result note once they answer.
pub fn describe_ask(args: &serde_json::Value) -> (String, String) {
    let question = args.get("question").and_then(|q| q.as_str()).unwrap_or("");
    ("asked you:".into(), question.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_parses_with_unique_names_and_aliases() {
        let mut seen = std::collections::HashSet::new();
        for c in manifest() {
            assert!(seen.insert(c.name.clone()), "duplicate name {}", c.name);
            for a in &c.aliases {
                assert!(seen.insert(a.clone()), "alias {a} repeats a name");
            }
        }
        assert!(seen.len() > 20);
    }

    /// A command the agent can call has to say which class governs it, and the
    /// class has to be one the harness tool actually reads.
    #[test]
    fn every_agent_command_names_a_class_the_harness_reads() {
        for c in manifest().iter().filter(|c| c.who == "both") {
            let class = c.agent_class.as_deref().unwrap_or_else(|| panic!("{} has no agentClass", c.name));
            assert!(
                HARNESS.iter().any(|(_, k)| *k == class),
                "{} names class {class}, which no harness name reads",
                c.name
            );
        }
    }

    /// `REG-T2`: `Off` withdraws a name, and a run nobody watches keeps only `compact`.
    #[test]
    fn agent_callable_drops_off_classes_and_everything_but_compact_when_headless() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(agent_callable(&db, false), vec!["compact", "suggest", "switch_mode", "schedule"]);
        assert_eq!(agent_callable(&db, true), vec!["compact"]);

        db.set_setting("autonomy.suggest", "off").unwrap();
        assert_eq!(agent_callable(&db, false), vec!["compact", "switch_mode", "schedule"]);
        db.set_setting("autonomy.context", "off").unwrap();
        assert!(agent_callable(&db, true).is_empty(), "headless with context off has nothing");
        // Off everywhere: no names, so no tool.
        db.set_setting("autonomy.modes", "off").unwrap();
        db.set_setting("autonomy.schedule", "off").unwrap();
        assert!(agent_callable(&db, false).is_empty());
        assert!(harness_spec(&agent_callable(&db, false)).is_none());
    }

    /// `AGC-T1`: the enum in the spec is exactly the names offered.
    #[test]
    fn the_spec_lists_only_the_names_on_offer() {
        let spec = harness_spec(&["compact", "schedule"]).unwrap();
        assert_eq!(
            spec.pointer("/function/parameters/properties/name/enum").unwrap(),
            &serde_json::json!(["compact", "schedule"])
        );
        // The description has to say this is the agent's own session.
        let text = spec.pointer("/function/description").and_then(|d| d.as_str()).unwrap();
        assert!(text.contains("not the user's computer"));
    }

    fn call(name: &str) -> HarnessCall {
        HarnessCall { name: name.into(), reason: "because".into(), ..Default::default() }
    }

    const ALL: [&str; 4] = ["compact", "suggest", "switch_mode", "schedule"];

    /// `AGC-T1`: `Auto` does it, `Ask` proposes and does not wait.
    #[test]
    fn the_rung_decides_whether_compact_runs_or_is_asked() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(decide(&db, "c", &ALL, &call("compact")), Ok(Act::Compact));
        db.set_setting("autonomy.context", "ask").unwrap();
        match decide(&db, "c", &ALL, &call("compact")).unwrap() {
            Act::Propose { name, reply, .. } => {
                assert_eq!(name, "compact");
                assert!(reply.contains("without waiting"), "the run must not block: {reply}");
            }
            other => panic!("expected a proposal, got {other:?}"),
        }
    }

    /// `PLF-2`: while planning first, an `Auto` mode switch is only asked, and
    /// clearing old output (which touches nothing outside the run) still runs.
    #[test]
    fn planning_first_turns_an_automatic_switch_into_a_question() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting("autonomy.modes", "auto").unwrap();
        let mut switch = call("switch_mode");
        switch.mode = Some("workspace".into());
        let auto = decide(&db, "c", &ALL, &switch).unwrap();
        assert!(matches!(&auto, Act::Propose { payload, .. } if payload["auto"] == true));
        match auto.asked_only() {
            Act::Propose { payload, reply, .. } => {
                assert_eq!(payload["auto"], false);
                assert!(reply.contains("without waiting"), "{reply}");
            }
            other => panic!("expected a proposal, got {other:?}"),
        }
        assert_eq!(Act::Compact.asked_only(), Act::Compact);
    }

    /// A name this run did not offer, or one switched off since, is refused.
    #[test]
    fn a_name_that_is_not_on_offer_is_refused() {
        let db = Db::open_in_memory().unwrap();
        assert!(decide(&db, "c", &["compact"], &call("schedule")).is_err());
        assert!(decide(&db, "c", &ALL, &call("nonsense")).is_err());
        db.set_setting("autonomy.suggest", "off").unwrap();
        let mut suggest = call("suggest");
        suggest.command = Some("/skillify".into());
        assert_eq!(
            decide(&db, "c", &ALL, &suggest),
            Err("The user has turned that off.".to_string()),
            "off mid-run is still off"
        );
    }

    /// `AGC-T1`: one suggestion per command per conversation, and only for a
    /// command the manifest says may be suggested.
    #[test]
    fn a_command_is_suggested_once_per_conversation() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("A chat", None, false).unwrap().id;
        let mut suggest = call("suggest");
        suggest.command = Some("/skillify".into());
        assert_eq!(
            decide(&db, &conv, &ALL, &suggest),
            Ok(Act::Suggest { command: "skillify".into(), reason: "because".into() })
        );
        super::super::log::record_command(
            &db,
            &conv,
            None,
            &super::super::log::CommandTrace {
                name: "suggest".into(),
                args: "skillify".into(),
                by: "agent".into(),
                outcome: "proposed".into(),
                note: None,
                message_id: None,
            },
        );
        match decide(&db, &conv, &ALL, &suggest).unwrap() {
            Act::Say(text) => assert!(text.starts_with("Already suggested")),
            other => panic!("expected the repeat to be refused, got {other:?}"),
        }
        // Another conversation has not been told yet.
        let other = db.create_conversation("Another", None, false).unwrap().id;
        assert!(matches!(decide(&db, &other, &ALL, &suggest), Ok(Act::Suggest { .. })));

        suggest.command = Some("/rename".into());
        assert!(decide(&db, &conv, &ALL, &suggest).is_err(), "not a command that may be suggested");
    }

    /// `CPX-5`: a suggestion the user has said no to twice, anywhere, is not offered.
    #[test]
    fn a_suggestion_declined_twice_in_any_chat_is_not_offered_again() {
        let db = Db::open_in_memory().unwrap();
        let first = db.create_conversation("One", None, false).unwrap().id;
        let second = db.create_conversation("Two", None, false).unwrap().id;
        let fresh = db.create_conversation("Three", None, false).unwrap().id;
        let no = |conv: &str| {
            super::super::log::record_command(
                &db,
                conv,
                None,
                &super::super::log::CommandTrace {
                    name: "suggest".into(),
                    args: "skillify".into(),
                    by: "user".into(),
                    outcome: "declined".into(),
                    note: None,
                    message_id: None,
                },
            );
        };
        let mut suggest = call("suggest");
        suggest.command = Some("/skillify".into());

        no(&first);
        assert!(matches!(decide(&db, &fresh, &ALL, &suggest), Ok(Act::Suggest { .. })), "once is not a pattern");
        no(&second);
        match decide(&db, &fresh, &ALL, &suggest).unwrap() {
            Act::Say(text) => assert!(text.contains("twice"), "{text}"),
            other => panic!("expected it to be withheld, got {other:?}"),
        }
    }

    /// `AGC-T1`/`AGC-T3`: a mode switch asks by default, can be made automatic,
    /// and a schedule is asked about even when someone wrote `auto` by hand.
    #[test]
    fn a_mode_switch_asks_and_a_schedule_always_asks() {
        let db = Db::open_in_memory().unwrap();
        let mut switch = call("switch_mode");
        switch.mode = Some("workspace".into());
        match decide(&db, "c", &ALL, &switch).unwrap() {
            Act::Propose { payload, note, .. } => {
                assert_eq!(payload["mode"], "workspace");
                assert_eq!(payload["auto"], false);
                assert!(note.starts_with("I asked to switch to Workspace"));
            }
            other => panic!("{other:?}"),
        }
        db.set_setting("autonomy.modes", "auto").unwrap();
        match decide(&db, "c", &ALL, &switch).unwrap() {
            Act::Propose { payload, .. } => assert_eq!(payload["auto"], true),
            other => panic!("{other:?}"),
        }
        switch.mode = Some("zen".into());
        assert!(decide(&db, "c", &ALL, &switch).is_err());

        db.set_setting("autonomy.schedule", "auto").unwrap();
        let mut schedule = call("schedule");
        assert!(decide(&db, "c", &ALL, &schedule).is_err(), "a schedule needs a task");
        schedule.task = Some("summarise my mail".into());
        schedule.when = Some("every weekday at 9".into());
        match decide(&db, "c", &ALL, &schedule).unwrap() {
            Act::Propose { name, payload, reply, .. } => {
                assert_eq!(name, "schedule");
                assert_eq!(payload["when"], "every weekday at 9");
                assert!(reply.contains("asked the user"), "auto is read as ask");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn only_manifest_commands_marked_suggestible_can_be_suggested() {
        assert_eq!(suggestible_name("/skillify"), Some("skillify"));
        assert_eq!(suggestible_name("make-skill"), Some("skillify"), "an alias resolves to the command");
        assert_eq!(suggestible_name("/reflect"), Some("reflect"));
        assert_eq!(suggestible_name("/rename"), None, "not suggestible");
        assert_eq!(suggestible_name("/nonsense"), None);
    }

    /// `CPX-3`: no raw tool name reaches the timeline.
    #[test]
    fn a_harness_call_reads_as_the_first_person_acting() {
        let v = |name: &str, extra: serde_json::Value| {
            let mut args = serde_json::json!({ "name": name, "reason": "r" });
            args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            describe_harness(&args)
        };
        assert_eq!(v("compact", serde_json::json!({})), ("made room".into(), String::new()));
        assert_eq!(v("suggest", serde_json::json!({ "command": "skillify" })), ("suggested".into(), "/skillify".into()));
        assert_eq!(v("suggest", serde_json::json!({ "command": "/skillify" })).1, "/skillify");
        assert_eq!(v("switch_mode", serde_json::json!({ "mode": "plan_first" })).1, "Plan first");
        assert_eq!(v("schedule", serde_json::json!({ "task": "mail" })).0, "asked to schedule");
        assert_eq!(describe_ask(&serde_json::json!({ "question": "Which one?" })).0, "asked you:");
    }
}
