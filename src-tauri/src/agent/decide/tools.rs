//! `USE-4`: which groups of tools a message needs. Before the first request of a
//! user turn, one decision asks, for every group the run could use, whether this
//! message needs it. A group the model is sure it does not need is left off the
//! menu for this turn, which makes the prompt shorter and a small model less
//! likely to wander. `need_tools` brings a group back; nothing is lost.
//!
//! A *unit* is the thing that is held back whole: an optional toolset, Composio's
//! meta-tools as "Connected apps", one MCP connector, or `delegate` as "Other
//! agents". This file holds the parts that need no engine and no run: building the
//! units, the question, reading the answer, and the bookkeeping of what is held
//! and what came back. `run.rs` applies it, after prompt assembly and after every
//! deterministic narrowing, so a decision only ever sees what the run could use
//! (decision 13).

use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

use super::control::{self, Act, Gate};
use super::policy::Purpose;
use super::{Answer, Ask, Question, Verdict};
use crate::agent::toolsets::Toolset;

/// Toolsets that are never held back, so a decision can never take them away.
/// File access, Recall and Memory are how a run reads and keeps things; Skills
/// are named in the system prompt; Workspace UI is the canvas of a workspace chat;
/// `classify` is on the menu only when the decision model is there, and its own
/// description says when it fits, so a second judgment of it would add a step.
const ALWAYS: [Toolset; 6] =
    [Toolset::FileSystem, Toolset::Recall, Toolset::Memory, Toolset::Skills, Toolset::Present, Toolset::Decide];

/// The name of the tool that brings a held-back unit back.
pub const NEED_TOOLS: &str = "need_tools";

pub fn handles(name: &str) -> bool {
    name == NEED_TOOLS
}

/// How much of the user's message and of the last reply a decision reads.
const CLIP_CHARS: usize = 2_000;
/// How many tool names a unit's description lists.
const NAMES_SHOWN: usize = 5;

/// Whose tool a spec is. Built by the caller from the run's tool table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    Builtin(Toolset),
    /// Composio's meta-tools.
    Apps,
    /// One MCP connector.
    Connector { id: String, name: String },
}

/// One group of tools, held back or kept as a whole.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    /// Stable within a run and what `need_tools` names: `web_search`, `apps`,
    /// `mcp:<connector id>`.
    pub id: String,
    /// The words the screen uses.
    pub label: String,
    /// What the decision model reads about it.
    pub about: String,
    /// The tool names it owns.
    pub tools: Vec<String>,
}

fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n).collect();
    out.push('…');
    out
}

fn list(names: &[String], shown: usize) -> String {
    let mut out = names.iter().take(shown).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > shown {
        out.push_str(", …");
    }
    out
}

/// The units of a run, in the order their first tool appears. `offers` is every
/// advertised tool with its owner; a tool of a toolset that is always kept, or
/// one that belongs to nobody (the loop's own `plan`, `harness`, `ask_user`), is
/// not part of any unit. `apps` is the granted app names, `agents` the agent
/// types `delegate` would list.
pub fn units(offers: &[(String, Owner)], apps: &str, agents: &[String]) -> Vec<Unit> {
    let mut out: Vec<Unit> = Vec::new();
    for (tool, owner) in offers {
        let (id, label) = match owner {
            Owner::Builtin(t) if ALWAYS.contains(t) => continue,
            Owner::Builtin(Toolset::Subagents) => ("subagents".to_string(), "Other agents".to_string()),
            Owner::Builtin(t) => (t.id().to_string(), t.label().to_string()),
            Owner::Apps => ("apps".to_string(), "Connected apps".to_string()),
            Owner::Connector { id, name } => (format!("mcp:{id}"), name.clone()),
        };
        match out.iter_mut().find(|u| u.id == id) {
            Some(unit) => unit.tools.push(tool.clone()),
            None => out.push(Unit { id, label, about: String::new(), tools: vec![tool.clone()] }),
        }
    }
    for unit in &mut out {
        unit.about = match unit.id.as_str() {
            "apps" => format!("Connected apps: {apps}."),
            "subagents" if agents.is_empty() => "Hand a task to another agent.".to_string(),
            "subagents" => format!("Hand a task to another agent: {}.", list(agents, 12)),
            _ => format!("{}: {}.", unit.label, list(&unit.tools, NAMES_SHOWN)),
        };
    }
    out
}

/// The user's last message and the assistant's reply before it, as a decision
/// reads them (each clipped to 2k characters). Empty when there is no user turn.
pub fn last_exchange(messages: &[Value]) -> (String, String) {
    let text = |m: &Value| -> String {
        match m.get("content") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    };
    let role = |m: &Value| m.get("role").and_then(Value::as_str).unwrap_or("").to_string();
    let Some(at) = messages.iter().rposition(|m| role(m) == "user") else { return (String::new(), String::new()) };
    let previous = messages[..at]
        .iter()
        .rev()
        .find(|m| role(m) == "assistant" && !text(m).trim().is_empty())
        .map(text)
        .unwrap_or_default();
    (clip(&text(&messages[at]), CLIP_CHARS), clip(&previous, CLIP_CHARS))
}

/// A request with one yes-or-no question per unit. Owns its text so the caller
/// can hand out a borrowed [`Ask`].
#[derive(Debug, Clone)]
pub struct ToolAsk {
    pub state: String,
    pub questions: Vec<(String, Question)>,
}

impl ToolAsk {
    pub fn as_ask(&self) -> Ask<'_> {
        Ask {
            state: &self.state,
            images: Vec::new(),
            questions: self.questions.iter().map(|(id, q)| (id.as_str(), q.clone())).collect(),
        }
    }
}

pub fn question(unit: &Unit) -> Question {
    Question::Noul {
        instructions: format!(
            "Would the assistant need these tools to do what the user's message asks, even for one step? \
             Say true when any of them is likely to be used. Say false only when the message is plain \
             conversation or about something these tools do not touch.\n{}",
            unit.about
        ),
        when_true: format!("The message needs {}.", unit.label),
        when_false: format!("The message does not need {}.", unit.label),
    }
}

/// `None` when there is nothing to ask: no user message, or no unit.
pub fn ask_for(message: &str, previous: &str, units: &[Unit]) -> Option<ToolAsk> {
    if message.trim().is_empty() || units.is_empty() {
        return None;
    }
    let mut state = format!("The user's message:\n{message}");
    if !previous.trim().is_empty() {
        state.push_str(&format!("\n\nThe assistant's last reply:\n{previous}"));
    }
    Some(ToolAsk { state, questions: units.iter().map(|u| (u.id.clone(), question(u))).collect() })
}

/// A unit that a decision left off the menu, with how sure the model was that it
/// is needed. The number is after calibration, when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub unit: Unit,
    pub p_needed: f32,
}

/// What the decision came to: the units to hold back, and what to tell the
/// controller about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub held: Vec<Held>,
    pub act: Act,
}

/// Hold a unit back when its answer reaches the row's bar. A unit with no usable
/// answer stays on the menu.
pub fn pick(units: &[Unit], verdict: &Verdict) -> Pick {
    let mut held = Vec::new();
    for unit in units {
        let Some(answer @ Answer::Noul { p_true }) = verdict.get(&unit.id) else { continue };
        if control::evaluate(Purpose::ToolSelect, answer, verdict.calibrated, Gate::Open) == Act::Go {
            held.push(Held { unit: unit.clone(), p_needed: *p_true });
        }
    }
    let act = if held.is_empty() { Act::Below } else { Act::Go };
    Pick { held, act }
}

/// Whether a run may be narrowed at all. A delegated child, a resumed run, an
/// unattended run and a run with no tools are never narrowed.
pub fn may_narrow(child: bool, resumed: bool, headless: bool, tools_enabled: bool) -> bool {
    tools_enabled && !child && !resumed && !headless
}

/// The units that stay on the menu whatever a decision says, because the user's
/// own act already settled it: they named an agent with `/delegate`, or a sign-in
/// card is waiting and the turn continues once it is done (`CAP-6`).
pub fn forced(delegate_asked: bool, sign_in_waiting: bool) -> Vec<&'static str> {
    let mut out = Vec::new();
    if delegate_asked {
        out.push("subagents");
    }
    if sign_in_waiting {
        out.push("apps");
    }
    out
}

/// How a held-back unit came back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Back {
    /// The model asked for it with `need_tools`.
    Asked,
    /// The model called one of its tools without asking first.
    Called,
    /// The user added it back from the run bar.
    Chip,
}

impl Back {
    /// The `decision_outcomes.source` stem for this way back.
    pub fn source(self) -> &'static str {
        match self {
            Back::Asked => "need-tools",
            Back::Called => "called-held",
            Back::Chip => "chip",
        }
    }

    pub fn by(self) -> &'static str {
        match self {
            Back::Chip => "you",
            _ => "model",
        }
    }
}

#[derive(Debug, Clone)]
struct Slot {
    held: Held,
    back: Option<Back>,
    /// Said to the screen and written to the log.
    reported: bool,
}

/// What one run has held back, and what has come back since. Lives on the run's
/// handle so a click on the run bar, which arrives from outside the loop, and the
/// loop itself see the same state.
#[derive(Debug, Default)]
pub struct HeldSet {
    decision_id: Option<String>,
    slots: Vec<Slot>,
}

/// One held unit as the screen shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeldView {
    pub id: String,
    pub label: String,
    /// The probability the model gave that it was needed.
    pub p: f32,
}

impl HeldSet {
    pub fn hold(&mut self, decision_id: &str, held: Vec<Held>) {
        self.decision_id = Some(decision_id.to_string());
        self.slots = held.into_iter().map(|held| Slot { held, back: None, reported: false }).collect();
    }

    pub fn decision_id(&self) -> Option<&str> {
        self.decision_id.as_deref()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn views(&self) -> Vec<HeldView> {
        self.slots
            .iter()
            .map(|s| HeldView { id: s.held.unit.id.clone(), label: s.held.unit.label.clone(), p: s.held.p_needed })
            .collect()
    }

    /// Units off the menu right now.
    pub fn out(&self) -> impl Iterator<Item = &Held> {
        self.slots.iter().filter(|s| s.back.is_none()).map(|s| &s.held)
    }

    /// The tool names that are off the menu right now.
    pub fn out_tools(&self) -> HashSet<&str> {
        self.out().flat_map(|h| h.unit.tools.iter().map(String::as_str)).collect()
    }

    /// Bring these units back. Returns the ones that were held and are now back.
    pub fn restore(&mut self, ids: &[String], how: Back) -> Vec<String> {
        let mut back = Vec::new();
        for slot in &mut self.slots {
            if slot.back.is_none() && ids.iter().any(|i| i == &slot.held.unit.id) {
                slot.back = Some(how);
                back.push(slot.held.unit.id.clone());
            }
        }
        back
    }

    /// The model called a tool that is off the menu: its unit comes back too.
    pub fn restore_for_tool(&mut self, tool: &str, how: Back) -> Option<String> {
        let id = self.out().find(|h| h.unit.tools.iter().any(|t| t == tool))?.unit.id.clone();
        self.restore(std::slice::from_ref(&id), how);
        Some(id)
    }

    /// Units that came back and have not been reported yet, marked as reported.
    pub fn take_unreported(&mut self) -> Vec<(HeldView, Back)> {
        let mut out = Vec::new();
        for slot in &mut self.slots {
            if let (Some(back), false) = (slot.back, slot.reported) {
                slot.reported = true;
                out.push((
                    HeldView { id: slot.held.unit.id.clone(), label: slot.held.unit.label.clone(), p: slot.held.p_needed },
                    back,
                ));
            }
        }
        out
    }

    /// Units that never came back, for the labels written when a run completes.
    pub fn never_back(&self) -> Vec<&Held> {
        self.slots.iter().filter(|s| s.back.is_none()).map(|s| &s.held).collect()
    }
}

/// The tool that brings a unit back. Offered only while something is off the
/// menu, and it can only name what was held (`USE-4`).
pub fn need_tools_spec(out: &[&Held]) -> Value {
    let ids: Vec<&str> = out.iter().map(|h| h.unit.id.as_str()).collect();
    let held = out.iter().map(|h| format!("{} ({})", h.unit.label, h.unit.id)).collect::<Vec<_>>().join(", ");
    serde_json::json!({
        "type": "function",
        "function": {
            "name": NEED_TOOLS,
            "description": format!(
                "Some tools were left off this turn's menu because the message did not seem to need them. \
                 If you do need one, name it here and it is on the menu from your next step. Left off: {held}."
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "units": {
                        "type": "array",
                        "items": { "type": "string", "enum": ids },
                        "description": "The ids of the tool groups you need."
                    }
                },
                "required": ["units"]
            }
        }
    })
}

/// What a `need_tools` call answers, and the groups it brings back. A group that
/// is held comes back from the next step. One that was never held is refused:
/// `why_not` says why it is not on the menu, in the words Settings -> Tools uses
/// for a toolset that is off or not ready, so this cannot route around a switch,
/// an account or a grant.
pub fn bring_back(held: &mut HeldSet, asked: &[String], why_not: impl Fn(&str) -> Option<String>) -> String {
    let back = held.restore(asked, Back::Asked);
    let views = held.views();
    let label = |id: &str| views.iter().find(|v| v.id == id).map_or(id.to_string(), |v| v.label.clone());
    let mut lines = Vec::new();
    if !back.is_empty() {
        let names: Vec<String> = back.iter().map(|id| label(id)).collect();
        lines.push(format!("Back on your menu from your next step: {}.", names.join(", ")));
    }
    for id in asked.iter().filter(|id| !back.contains(id)) {
        lines.push(if views.iter().any(|v| &v.id == id) {
            format!("{} is already on your menu.", label(id))
        } else {
            why_not(id).unwrap_or_else(|| format!("Nothing was held back called {id}."))
        });
    }
    if lines.is_empty() {
        lines.push("Name at least one group from the list.".to_string());
    }
    lines.join(" ")
}

/// The ids a `need_tools` call names. Anything else in the call is ignored.
pub fn requested(args: &Value) -> Vec<String> {
    args.get("units")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn offer(tool: &str, owner: Owner) -> (String, Owner) {
        (tool.to_string(), owner)
    }

    fn fixture() -> Vec<(String, Owner)> {
        vec![
            offer("read_file", Owner::Builtin(Toolset::FileSystem)),
            offer("web_search", Owner::Builtin(Toolset::WebSearch)),
            offer("fetch_url", Owner::Builtin(Toolset::WebSearch)),
            offer("send_mail", Owner::Builtin(Toolset::Mail)),
            offer("delegate", Owner::Builtin(Toolset::Subagents)),
            offer("check_agents", Owner::Builtin(Toolset::Subagents)),
            offer("COMPOSIO_SEARCH_TOOLS", Owner::Apps),
            offer("COMPOSIO_MULTI_EXECUTE_TOOL", Owner::Apps),
            offer("list_notes", Owner::Connector { id: "c1".into(), name: "Notes".into() }),
            offer("recall", Owner::Builtin(Toolset::Recall)),
        ]
    }

    fn the_units() -> Vec<Unit> {
        units(&fixture(), "Google Calendar, Slack", &["researcher".to_string(), "Claude Code".to_string()])
    }

    fn verdict(answers: &[(&str, f32)], calibrated: bool) -> Verdict {
        Verdict {
            id: "d1".into(),
            answers: answers.iter().map(|(id, p)| ((*id).to_string(), Answer::Noul { p_true: *p })).collect::<BTreeMap<_, _>>(),
            backend: super::super::BackendId::Local { model: "Clef-flash".into() },
            calibrated,
            ms: 20,
        }
    }

    /// `USE-T-1`: the groups are the optional toolsets, Connected apps, each MCP
    /// connector and Other agents, and File access and Recall are never among them.
    #[test]
    fn the_menu_is_made_of_optional_toolsets_apps_connectors_and_other_agents() {
        let u = the_units();
        let ids: Vec<&str> = u.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids, ["web_search", "mail", "subagents", "apps", "mcp:c1"]);
        assert!(!ids.contains(&"filesystem") && !ids.contains(&"recall"));
        assert_eq!(u[0].tools, ["web_search", "fetch_url"]);
        assert_eq!(u[2].tools, ["delegate", "check_agents"], "delegate and its two companions are one unit");
    }

    #[test]
    fn every_toolset_that_must_stay_is_kept_whatever_it_offers() {
        for t in ALWAYS {
            let offers = vec![offer("some_tool", Owner::Builtin(t))];
            assert!(units(&offers, "", &[]).is_empty(), "{} must never be held back", t.id());
        }
    }

    #[test]
    fn a_unit_is_described_by_names_never_by_schemas() {
        let u = the_units();
        assert_eq!(u[3].about, "Connected apps: Google Calendar, Slack.");
        assert_eq!(u[2].about, "Hand a task to another agent: researcher, Claude Code.");
        assert_eq!(u[0].about, "Web search: web_search, fetch_url.");
        let many: Vec<(String, Owner)> = (1..=8)
            .map(|i| offer(&format!("tool_{i}"), Owner::Connector { id: "x".into(), name: "Big".into() }))
            .collect();
        assert_eq!(units(&many, "", &[])[0].about, "Big: tool_1, tool_2, tool_3, tool_4, tool_5, ….");
    }

    #[test]
    fn the_question_asks_one_yes_or_no_per_unit_over_the_message_and_the_last_reply() {
        let u = the_units();
        let ask = ask_for("what is on my calendar tomorrow?", "Sure.", &u).unwrap();
        assert_eq!(ask.questions.len(), u.len());
        assert!(ask.state.contains("what is on my calendar tomorrow?") && ask.state.contains("Sure."));
        assert!(ask.questions.iter().all(|(_, q)| matches!(q, Question::Noul { .. })));
        assert_eq!(ask.as_ask().questions.len(), 5);
        let Question::Noul { instructions, .. } = &ask.questions[3].1 else { panic!() };
        assert!(instructions.contains("Google Calendar"));
        assert!(!ask_for("   ", "", &u).is_some(), "no message, no question");
        assert!(ask_for("hi", "", &[]).is_none(), "no unit, no question");
        assert!(!ask_for("hi", "", &u).unwrap().state.contains("last reply"), "no reply to show");
    }

    #[test]
    fn the_last_exchange_skips_tool_turns_and_clips_each_side() {
        let long = "x".repeat(5_000);
        let messages = vec![
            serde_json::json!({ "role": "system", "content": "rules" }),
            serde_json::json!({ "role": "user", "content": "first" }),
            serde_json::json!({ "role": "assistant", "content": long }),
            serde_json::json!({ "role": "assistant", "content": null, "tool_calls": [] }),
            serde_json::json!({ "role": "tool", "content": "result" }),
            serde_json::json!({ "role": "user", "content": [{ "type": "text", "text": "second" }] }),
            serde_json::json!({ "role": "system", "content": "note" }),
        ];
        let (user, previous) = last_exchange(&messages);
        assert_eq!(user, "second");
        assert_eq!(previous.chars().count(), 2_001, "2000 characters and an ellipsis");
        assert_eq!(last_exchange(&[serde_json::json!({ "role": "system", "content": "x" })]), (String::new(), String::new()));
    }

    /// `CTL-T-1` for this row: a unit goes only on a confident no. Uncalibrated the
    /// bar is p(needed) <= 0.05, calibrated <= 0.15.
    #[test]
    fn a_unit_is_held_back_only_when_the_model_is_sure_it_is_not_needed() {
        let u = the_units();
        let answers = [("web_search", 0.03), ("mail", 0.10), ("subagents", 0.5), ("apps", 0.9), ("mcp:c1", 0.04)];
        let got = pick(&u, &verdict(&answers, false));
        let ids: Vec<&str> = got.held.iter().map(|h| h.unit.id.as_str()).collect();
        assert_eq!(ids, ["web_search", "mcp:c1"], "0.10 is not sure enough without a fit");
        assert_eq!(got.act, Act::Go);
        assert_eq!(got.held[0].p_needed, 0.03);

        let fitted = pick(&u, &verdict(&answers, true));
        let ids: Vec<&str> = fitted.held.iter().map(|h| h.unit.id.as_str()).collect();
        assert_eq!(ids, ["web_search", "mail", "mcp:c1"], "with a fit, 0.10 is under 0.15");
    }

    #[test]
    fn a_unit_with_no_answer_stays_and_nothing_held_is_a_default() {
        let u = the_units();
        let got = pick(&u, &verdict(&[("web_search", 0.5)], true));
        assert!(got.held.is_empty());
        assert_eq!(got.act, Act::Below, "settles as `default`");
        assert!(pick(&u, &verdict(&[], true)).held.is_empty(), "a missing answer keeps the unit");
    }

    /// `USE-T-1`: children, resumed runs, scheduled runs and a run with no tools
    /// are never narrowed.
    #[test]
    fn only_a_users_own_fresh_turn_with_tools_is_narrowed() {
        assert!(may_narrow(false, false, false, true));
        assert!(!may_narrow(true, false, false, true), "a delegated child");
        assert!(!may_narrow(false, true, false, true), "a resumed run");
        assert!(!may_narrow(false, false, true, true), "a scheduled task");
        assert!(!may_narrow(false, false, false, false), "no tools");
    }

    #[test]
    fn what_the_user_already_settled_is_forced_onto_the_menu() {
        assert_eq!(forced(true, false), ["subagents"]);
        assert_eq!(forced(false, true), ["apps"]);
        assert_eq!(forced(true, true), ["subagents", "apps"]);
        assert!(forced(false, false).is_empty());
    }

    fn held_two() -> HeldSet {
        let u = the_units();
        let mut set = HeldSet::default();
        set.hold(
            "d1",
            vec![Held { unit: u[0].clone(), p_needed: 0.03 }, Held { unit: u[3].clone(), p_needed: 0.04 }],
        );
        set
    }

    #[test]
    fn need_tools_brings_back_only_what_was_held() {
        let mut set = held_two();
        assert_eq!(set.out_tools().len(), 4);
        // A unit that was never held (mail was kept) is not returned.
        let back = set.restore(&["mail".into(), "web_search".into()], Back::Asked);
        assert_eq!(back, ["web_search"]);
        assert!(!set.out_tools().contains("web_search") && set.out_tools().contains("COMPOSIO_SEARCH_TOOLS"));
        // Asking again for the same one brings nothing back twice.
        assert!(set.restore(&["web_search".into()], Back::Asked).is_empty());
    }

    #[test]
    fn calling_a_held_tool_brings_its_unit_back() {
        let mut set = held_two();
        assert_eq!(set.restore_for_tool("fetch_url", Back::Called).as_deref(), Some("web_search"));
        assert_eq!(set.restore_for_tool("fetch_url", Back::Called), None, "already back");
        assert_eq!(set.restore_for_tool("read_file", Back::Called), None, "never held");
    }

    #[test]
    fn a_unit_that_came_back_is_reported_once_with_how() {
        let mut set = held_two();
        set.restore(&["apps".into()], Back::Chip);
        let first = set.take_unreported();
        assert_eq!(first.len(), 1);
        assert_eq!((first[0].0.id.as_str(), first[0].1), ("apps", Back::Chip));
        assert!(set.take_unreported().is_empty(), "said once");
        assert_eq!(Back::Chip.by(), "you");
        assert_eq!(Back::Asked.by(), "model");
        assert_eq!(set.never_back().len(), 1, "web_search never came back");
        assert_eq!(set.views().len(), 2, "the chip keeps showing what was held");
    }

    #[test]
    fn the_need_tools_spec_names_only_what_is_out_and_reads_the_call_back() {
        let set = held_two();
        let out: Vec<&Held> = set.out().collect();
        let spec = need_tools_spec(&out);
        assert_eq!(spec["function"]["name"], NEED_TOOLS);
        assert_eq!(spec["function"]["parameters"]["properties"]["units"]["items"]["enum"], serde_json::json!(["web_search", "apps"]));
        assert!(spec["function"]["description"].as_str().unwrap().contains("Connected apps (apps)"));
        assert_eq!(requested(&serde_json::json!({ "units": ["apps", 3, "x"] })), ["apps", "x"]);
        assert!(requested(&serde_json::json!({})).is_empty());
        assert!(handles(NEED_TOOLS) && !handles("plan"));
    }

    #[test]
    fn need_tools_says_what_came_back_and_why_a_refused_group_is_not_on_the_menu() {
        let mut set = held_two();
        let why = |id: &str| (id == "mail").then(|| "Mail: I can't use this yet. Add a mail account in Settings \u{2192} Mail.".to_string());
        let reply = bring_back(&mut set, &["web_search".into(), "mail".into(), "nope".into()], why);
        assert_eq!(
            reply,
            "Back on your menu from your next step: Web search. \
             Mail: I can't use this yet. Add a mail account in Settings \u{2192} Mail. \
             Nothing was held back called nope."
        );
        // Asking for what is already back does not bring it twice.
        assert_eq!(bring_back(&mut set, &["web_search".into()], |_| None), "Web search is already on your menu.");
        assert_eq!(bring_back(&mut set, &[], |_| None), "Name at least one group from the list.");
        assert!(!set.out_tools().contains("web_search"), "and it is on the menu again");
    }

    /// `USE-T-1`: the refusal never opens a group a gate closed, even when the
    /// model names one that does exist elsewhere in the app.
    #[test]
    fn need_tools_cannot_add_a_group_that_was_never_held() {
        let mut set = held_two();
        let reply = bring_back(&mut set, &["mail".into()], |_| None);
        assert_eq!(reply, "Nothing was held back called mail.");
        assert_eq!(set.out().count(), 2, "nothing changed");
    }
}
