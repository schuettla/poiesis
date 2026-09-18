//! `PLN-1`/`PLN-2`: the plan a run is working to.
//!
//! A run meter that reads `step 1 of 12` names a budget, not an intention. This
//! module is the intention: a short list of the work the run means to do, that
//! the model writes through a tool, that the user can see, and that the run is
//! then measured against.
//!
//! Three rules hold the whole design up:
//!
//! - **The plan the UI shows is the plan the model is working against.** It is
//!   rendered back into the transcript on every turn (`plan_message`), so there
//!   is exactly one plan and both sides are looking at it. A plan the UI knows
//!   about and the model does not is a checklist that lies.
//! - **A dropped item stays visible, with its reason.** A plan that quietly
//!   loses items is the same lie told by omission.
//! - **The plan never gates execution.** Nothing in this file can refuse a tool
//!   call, and a step that is not on the list still runs. The plan describes
//!   intent; it does not authorise.
//!
//! ## Why this is not a `Toolset`
//!
//! `PLN-1` calls for a toolset. It is not one, for the same reason `results.rs`
//! is not: its state belongs to the *run*, not to the database or the machine,
//! so it is dispatched by the loop itself (`TurnCtx::dispatch`) against the run's
//! own `Plan`. Making it a `Toolset` would have put a second on/off switch in
//! Settings beside `PLN-UI-4`'s three-way setting, and the two could disagree —
//! "planning: on" next to "plan the work first: never". The setting is the
//! control; this file has no other.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::db::Db;

/// Where an item stands. `Dropped` is a real state rather than a deletion: an
/// item that turned out to be unnecessary is information about the run, and a
/// plan that silently shrinks cannot be trusted to have been the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Todo,
    Doing,
    Done,
    Dropped,
}

impl PlanStatus {
    fn parse(s: &str) -> Option<PlanStatus> {
        match s.trim().to_ascii_lowercase().as_str() {
            "todo" | "to do" | "pending" => Some(PlanStatus::Todo),
            "doing" | "in_progress" | "in progress" | "running" => Some(PlanStatus::Doing),
            "done" | "complete" | "completed" => Some(PlanStatus::Done),
            "dropped" | "skipped" | "cancelled" | "canceled" => Some(PlanStatus::Dropped),
            _ => None,
        }
    }

    /// How the transcript says it. First person is `PRES-0`'s job in the UI;
    /// what the model reads is plainer than that.
    fn as_note(self) -> &'static str {
        match self {
            PlanStatus::Todo => "to do",
            PlanStatus::Doing => "doing now",
            PlanStatus::Done => "done",
            PlanStatus::Dropped => "dropped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub text: String,
    pub status: PlanStatus,
    /// Why a dropped item was dropped. Only ever set for `Dropped`, and shown
    /// on the same line as the item it explains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// True for an item added after the plan was first written. A plan that grew
    /// mid-run is a different thing from one that was right to begin with, and
    /// the difference is worth seeing.
    #[serde(default)]
    pub added: bool,
}

/// One run's plan. `revisions` counts *rewrites* — a `set` over an existing
/// plan — not status changes, because rewriting the plan is the event worth
/// announcing (`PLN-UI-3`) and ticking an item off is not.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Plan {
    #[serde(default)]
    pub items: Vec<PlanItem>,
    #[serde(default)]
    pub revisions: usize,
    /// Every earlier version of the list, oldest first — what the disclosure in
    /// `PLN-UI-3` opens onto. Kept as plain text per item: the point is to show
    /// what the plan used to say, not to replay its statuses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous: Vec<Vec<String>>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Replace the list. The first `set` of a run is the plan being written; a
    /// later one is a revision, and says so.
    fn set(&mut self, texts: Vec<String>) {
        if !self.items.is_empty() {
            self.previous.push(self.items.iter().map(|i| i.text.clone()).collect());
            self.revisions += 1;
        }
        self.items = texts
            .into_iter()
            .map(|text| PlanItem { text, status: PlanStatus::Todo, why: None, added: false })
            .collect();
    }

    /// Append work the plan did not foresee, marked as an addition.
    fn add(&mut self, texts: Vec<String>) {
        for text in texts {
            self.items.push(PlanItem {
                text,
                status: PlanStatus::Todo,
                why: None,
                added: true,
            });
        }
    }

    /// Move one item. `index` is 1-based, as the tool advertises and as the
    /// transcript numbers them — the model reads the list before it writes to it.
    fn update(
        &mut self,
        index: usize,
        status: PlanStatus,
        why: Option<String>,
    ) -> Result<(), String> {
        if index == 0 || index > self.items.len() {
            return Err(format!(
                "There is no item {index} in the plan — it has {} item{}, numbered 1 to {}.",
                self.items.len(),
                if self.items.len() == 1 { "" } else { "s" },
                self.items.len().max(1),
            ));
        }
        let item = &mut self.items[index - 1];
        item.status = status;
        // A reason belongs to a drop. Carrying one onto `done` would put an
        // excuse beside finished work; clearing it on the way back out of
        // `dropped` keeps a stale reason from outliving the drop it explained.
        item.why = if status == PlanStatus::Dropped {
            why.filter(|w| !w.trim().is_empty())
        } else {
            None
        };
        Ok(())
    }

    /// The item the run is on, for `PLN-UI-2`'s meter. The first `Doing`, else
    /// the first thing still to do — a model that never marks `doing` still gets
    /// a truthful label rather than none.
    pub fn current(&self) -> Option<&PlanItem> {
        self.items
            .iter()
            .find(|i| i.status == PlanStatus::Doing)
            .or_else(|| self.items.iter().find(|i| i.status == PlanStatus::Todo))
    }

    /// `PLN-5`: what the run never got to. This is the honest half of "I stopped
    /// at my step limit" — not just that it stopped, but what is left.
    pub fn unreached(&self) -> Vec<&PlanItem> {
        self.items
            .iter()
            .filter(|i| matches!(i.status, PlanStatus::Todo | PlanStatus::Doing))
            .collect()
    }

    /// The list as the model reads it back.
    pub fn render(&self) -> String {
        let mut lines = Vec::with_capacity(self.items.len());
        for (n, item) in self.items.iter().enumerate() {
            let mut line = format!("{}. {} — {}", n + 1, item.text, item.status.as_note());
            if let Some(why) = &item.why {
                line.push_str(": ");
                line.push_str(why);
            }
            if item.added {
                line.push_str(" (added later)");
            }
            lines.push(line);
        }
        lines.join("\n")
    }
}

/// `PLN-2`: the plan, as one system turn, rendered fresh for every request.
///
/// `None` for a run with no plan — deliberately, and `PLN-T2` pins it: an empty
/// scaffold ("your plan: (none)") is an invitation to fill it in, and most
/// requests should not have a plan at all.
///
/// The caller pushes this onto the transcript for the length of one request and
/// takes it off again, which is why it is a function rather than a message the
/// loop appends. Appending would leave a trail of contradicting copies — turn
/// three's plan still saying `doing` under turn six's saying `done` — and that
/// stale copy is exactly the lying checklist this feature exists to avoid.
pub fn plan_message(plan: &Plan) -> Option<serde_json::Value> {
    if plan.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "role": "system",
        "content": format!(
            "## Your plan for this run\n{}\n\
             Keep it true: call `plan` with `update` when you start an item and when you finish one. \
             If you are about to do something that is not on the list, add it or rewrite the plan first.",
            plan.render()
        ),
    }))
}

/// `PLN-5`: what the wrap-up turn is told, when a run out of budget had a plan.
///
/// A separate line from `plan_message` because the wrap-up turn has no tools:
/// telling it to call `plan` there would be an instruction it cannot follow.
/// What it needs instead is the one thing only the plan knows — which items
/// never happened — so the answer can say so in words.
pub fn wrap_up_message(plan: &Plan) -> Option<serde_json::Value> {
    let left = plan.unreached();
    if left.is_empty() {
        return None;
    }
    let list = left.iter().map(|i| format!("- {}", i.text)).collect::<Vec<_>>().join("\n");
    Some(serde_json::json!({
        "role": "system",
        "content": format!(
            "You planned this run and did not reach every item. Say plainly, at the end of your answer, \
             what is still outstanding:\n{list}",
        ),
    }))
}

/// The mode is the user's override of the model's judgement (`PLN-3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanMode {
    /// Plan every run that can use tools.
    Always,
    /// The model decides, guided by one sentence of the system prompt. The
    /// default, and what an unset setting means.
    #[default]
    Auto,
    /// No tool, no prompt sentence, no plan.
    Never,
}

pub const PLAN_MODE_KEY: &str = "agent.plan_mode";

impl PlanMode {
    pub fn parse(s: &str) -> PlanMode {
        match s.trim() {
            "always" => PlanMode::Always,
            "never" => PlanMode::Never,
            _ => PlanMode::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PlanMode::Always => "always",
            PlanMode::Auto => "auto",
            PlanMode::Never => "never",
        }
    }

    /// What the user chose, or *when it helps* if they never said.
    pub fn current(db: &Db) -> PlanMode {
        match db.get_setting(PLAN_MODE_KEY).ok().flatten() {
            Some(v) => PlanMode::parse(&v),
            None => PlanMode::Auto,
        }
    }

    /// Is the `plan` tool offered at all this run?
    pub fn offers_tool(self) -> bool {
        self != PlanMode::Never
    }

    /// `PLN-3`: the one sentence of system prompt. One sentence is the whole
    /// budget — a model that writes junk plans will not be argued out of it by
    /// a paragraph, and *never* is the right answer for that model.
    pub fn guidance(self) -> &'static str {
        match self {
            PlanMode::Always => {
                "Before your first tool call, write the plan with the `plan` tool, then keep it up to date as you work."
            }
            PlanMode::Auto => {
                "When a request has several distinct parts, or will take more than a few steps, write a short plan with the `plan` tool before you start and keep it up to date as you work; otherwise just do the work."
            }
            PlanMode::Never => "",
        }
    }
}

pub fn handles(name: &str) -> bool {
    name == "plan"
}

pub fn tool_specs() -> Vec<serde_json::Value> {
    // Deliberately plain JSON Schema: no `$ref`, no `oneOf`. The engines this
    // has to survive include free-tier hosts and small local servers, and an
    // exotic schema is refused by more of them than it helps. `update` and
    // `updates` are two spellings of one thing for the same reason — the
    // executor takes either, and either shape, rather than betting on the model
    // reading the schema closely.
    let one_update = serde_json::json!({
        "type": "object",
        "properties": {
            "index": { "type": "integer", "description": "Which item, numbered from 1." },
            "status": { "type": "string", "enum": ["todo", "doing", "done", "dropped"] },
            "why": {
                "type": "string",
                "description": "Required for `dropped`: why this item is not being done. It stays visible next to the item."
            }
        },
        "required": ["index", "status"]
    });
    vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": "plan",
            "description": "Write or revise the short plan for this run, and keep it current. Pass `items` to write the whole plan (a handful of short phrases, in order). Pass `update` to move an item, or `updates` to move several at once — items are numbered from 1, as shown when the plan is read back. Pass `add` for work you did not foresee. The plan is shown to the user as you work, so it must stay true: drop an item with a reason rather than leaving it hanging. Do not spend a whole turn on bookkeeping — call this in the same turn as the work it describes, and finish one item and start the next in a single call: updates: [{index:2,status:\"done\"},{index:3,status:\"doing\"}].",
            "parameters": {
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "The whole plan, replacing any earlier one. Short phrases, in the order you mean to do them."
                    },
                    "add": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Extra items appended to the current plan, marked as later additions."
                    },
                    "update": one_update,
                    "updates": {
                        "type": "array",
                        "items": one_update,
                        "description": "Several moves in one call — finishing one item and starting the next is one thought, not two turns."
                    }
                }
            }
        }
    })]
}

/// The timeline line for a `plan` call. Past tense, like every other step.
pub fn describe(_name: &str, args: &serde_json::Value) -> (String, String) {
    if let Some(items) = args.get("items").and_then(|v| v.as_array()) {
        return ("planned".to_string(), format!("{} steps", items.len()));
    }
    if let Some(items) = args.get("add").and_then(|v| v.as_array()) {
        return (
            "added to the plan".to_string(),
            format!("{} more", items.len()),
        );
    }
    if let Some(update) = args.get("update") {
        let status = update["status"].as_str().unwrap_or("changed");
        let index = update["index"].as_i64().unwrap_or(0);
        return ("updated the plan".to_string(), format!("item {index} — {status}"));
    }
    ("planned".to_string(), String::new())
}

/// Run a `plan` call against this run's plan.
///
/// Errors are written for the model to read and correct, never as a panic: an
/// out-of-range index is the single most likely mistake, and a run must not die
/// of a bad plan edit.
pub fn execute(plan: &Mutex<Plan>, args: &serde_json::Value) -> Result<String, String> {
    let mut plan = plan.lock().unwrap();

    let strings = |v: Option<&serde_json::Value>| -> Vec<String> {
        v.and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };

    let mut did_something = false;

    if args.get("items").is_some() {
        let items = strings(args.get("items"));
        if items.is_empty() {
            return Err("A plan needs at least one item. Pass `items` as a list of short phrases.".into());
        }
        plan.set(items);
        did_something = true;
    }

    if args.get("add").is_some() {
        let items = strings(args.get("add"));
        if items.is_empty() {
            return Err("`add` needs at least one item.".into());
        }
        if plan.is_empty() {
            // Adding to nothing is writing the plan; treat it as that rather
            // than producing a plan whose every item is marked "added later".
            plan.set(items);
        } else {
            plan.add(items);
        }
        did_something = true;
    }

    // One update or several. Several matters more than it looks: finishing one
    // item and starting the next is *one* thought, and making the model spend a
    // whole turn on each half of it is how a six-item plan ate a third of a
    // twelve-step budget on bookkeeping.
    let updates = match args.get("updates").or_else(|| args.get("update")) {
        Some(serde_json::Value::Array(items)) => items.clone(),
        Some(one) => vec![one.clone()],
        None => Vec::new(),
    };
    for update in &updates {
        let index = update["index"]
            .as_i64()
            .ok_or("`update` needs an `index`, numbered from 1.")?;
        let status_text = update["status"]
            .as_str()
            .ok_or("`update` needs a `status`: todo, doing, done or dropped.")?;
        let status = PlanStatus::parse(status_text).ok_or_else(|| {
            format!("'{status_text}' is not a status. Use todo, doing, done or dropped.")
        })?;
        let why = update["why"].as_str().map(str::to_string);
        if status == PlanStatus::Dropped && why.as_deref().map(str::trim).unwrap_or("").is_empty() {
            return Err(
                "Dropping an item needs a `why` — it stays visible next to the item.".into(),
            );
        }
        let index = usize::try_from(index).unwrap_or(0);
        plan.update(index, status, why)?;
        did_something = true;
    }

    if !did_something {
        return Err(
            "Nothing to do: pass `items` to write the plan, `add` to extend it, or `update` to move one item."
                .into(),
        );
    }

    Ok(format!("The plan now reads:\n{}", plan.render()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_with(items: &[&str]) -> Mutex<Plan> {
        let m = Mutex::new(Plan::default());
        execute(&m, &serde_json::json!({ "items": items })).unwrap();
        m
    }

    /// `PLN-T1`: `set` replaces rather than appends, and the replacement counts
    /// as a revision — that count is what `PLN-UI-3` renders.
    #[test]
    fn writing_the_plan_again_replaces_it_and_says_it_was_a_revision() {
        let m = plan_with(&["read the spec", "sketch the layout"]);
        assert_eq!(m.lock().unwrap().revisions, 0, "the first plan is not a revision");

        execute(&m, &serde_json::json!({ "items": ["start over"] })).unwrap();
        let plan = m.lock().unwrap();
        assert_eq!(plan.items.len(), 1, "set replaces; it does not append");
        assert_eq!(plan.revisions, 1);
        assert_eq!(
            plan.previous,
            vec![vec!["read the spec".to_string(), "sketch the layout".to_string()]],
            "the version it replaced is kept, for the disclosure"
        );
    }

    /// `PLN-T1`: an update moves exactly one item and leaves the rest alone.
    #[test]
    fn an_update_moves_one_item_and_only_that_one() {
        let m = plan_with(&["read the spec", "sketch the layout", "write the file"]);
        execute(&m, &serde_json::json!({ "update": { "index": 2, "status": "doing" } })).unwrap();

        let plan = m.lock().unwrap();
        assert_eq!(plan.items[0].status, PlanStatus::Todo);
        assert_eq!(plan.items[1].status, PlanStatus::Doing);
        assert_eq!(plan.items[2].status, PlanStatus::Todo);
        assert_eq!(plan.current().unwrap().text, "sketch the layout");
    }

    /// Finishing one item and starting the next is one thought, so it must be
    /// one call. Two calls meant two turns, and two turns off a twelve-step
    /// budget is how a six-item plan ran out of room at item four.
    #[test]
    fn finishing_one_item_and_starting_the_next_is_a_single_call() {
        let m = plan_with(&["read the spec", "sketch the layout", "write the file"]);
        execute(
            &m,
            &serde_json::json!({
                "updates": [
                    { "index": 1, "status": "done" },
                    { "index": 2, "status": "doing" }
                ]
            }),
        )
        .unwrap();

        let plan = m.lock().unwrap();
        assert_eq!(plan.items[0].status, PlanStatus::Done);
        assert_eq!(plan.items[1].status, PlanStatus::Doing);
        assert_eq!(plan.items[2].status, PlanStatus::Todo);
    }

    /// `update` and `updates`, object or list: four spellings of one thing. The
    /// model should not lose a turn to an error because it guessed the other
    /// name — small models guess, and the cost of being lenient here is nothing.
    #[test]
    fn either_spelling_and_either_shape_is_accepted() {
        for args in [
            serde_json::json!({ "update": { "index": 1, "status": "done" } }),
            serde_json::json!({ "update": [{ "index": 1, "status": "done" }] }),
            serde_json::json!({ "updates": { "index": 1, "status": "done" } }),
            serde_json::json!({ "updates": [{ "index": 1, "status": "done" }] }),
        ] {
            let m = plan_with(&["read the spec"]);
            execute(&m, &args).unwrap();
            assert_eq!(m.lock().unwrap().items[0].status, PlanStatus::Done, "{args}");
        }
    }

    /// `PLN-T1`: a dropped item stays in the plan, with its reason. Losing it
    /// would make the plan a record of what happened to succeed.
    #[test]
    fn a_dropped_item_stays_visible_with_its_reason() {
        let m = plan_with(&["read the spec", "sketch the layout"]);
        execute(
            &m,
            &serde_json::json!({
                "update": { "index": 2, "status": "dropped", "why": "the spec already had it" }
            }),
        )
        .unwrap();

        let plan = m.lock().unwrap();
        assert_eq!(plan.items.len(), 2, "a drop is not a delete");
        assert_eq!(plan.items[1].status, PlanStatus::Dropped);
        assert_eq!(plan.items[1].why.as_deref(), Some("the spec already had it"));
        assert!(plan.render().contains("the spec already had it"));
    }

    /// A drop with no reason is refused rather than recorded as an unexplained
    /// disappearance — the reason is the whole point of keeping the item.
    #[test]
    fn dropping_an_item_without_a_reason_is_refused() {
        let m = plan_with(&["read the spec"]);
        let err = execute(&m, &serde_json::json!({ "update": { "index": 1, "status": "dropped" } }))
            .unwrap_err();
        assert!(err.contains("why"), "{err}");
        assert_eq!(m.lock().unwrap().items[0].status, PlanStatus::Todo, "and nothing moved");
    }

    /// `PLN-T1`: `add` appends and the addition is marked as one, so a plan that
    /// grew is distinguishable from one that was right.
    #[test]
    fn an_added_item_is_marked_as_an_addition() {
        let m = plan_with(&["read the spec"]);
        execute(&m, &serde_json::json!({ "add": ["fix the test it broke"] })).unwrap();

        let plan = m.lock().unwrap();
        assert_eq!(plan.items.len(), 2);
        assert!(!plan.items[0].added);
        assert!(plan.items[1].added);
        assert_eq!(plan.revisions, 0, "growing a plan is not rewriting it");
        assert!(plan.render().contains("(added later)"));
    }

    /// `PLN-T1`: an out-of-range index is an error the model can read and fix,
    /// not a panic that takes the run down.
    #[test]
    fn an_index_off_the_end_is_an_error_the_model_can_read() {
        let m = plan_with(&["read the spec", "sketch the layout"]);
        let err = execute(&m, &serde_json::json!({ "update": { "index": 7, "status": "done" } }))
            .unwrap_err();
        assert!(err.contains("no item 7"), "{err}");
        assert!(err.contains("1 to 2"), "it says what the valid range is: {err}");

        // Zero is the other half of the same mistake: a model counting from 0.
        assert!(execute(&m, &serde_json::json!({ "update": { "index": 0, "status": "done" } })).is_err());
    }

    /// `PLN-T2`'s first half at the unit that produces it: a plan renders as one
    /// system turn, and a run with no plan renders nothing at all.
    #[test]
    fn a_run_with_no_plan_renders_no_scaffolding() {
        assert!(plan_message(&Plan::default()).is_none());

        let m = plan_with(&["read the spec"]);
        let msg = plan_message(&m.lock().unwrap()).expect("a plan renders");
        assert_eq!(msg["role"], "system");
        let content = msg["content"].as_str().unwrap();
        assert!(content.contains("1. read the spec — to do"), "{content}");
    }

    /// `PLN-5`'s input: what a run that stopped early never reached.
    #[test]
    fn unreached_items_are_the_ones_not_finished_or_dropped() {
        let m = plan_with(&["one", "two", "three", "four"]);
        execute(&m, &serde_json::json!({ "update": { "index": 1, "status": "done" } })).unwrap();
        execute(&m, &serde_json::json!({ "update": { "index": 2, "status": "doing" } })).unwrap();
        execute(
            &m,
            &serde_json::json!({ "update": { "index": 4, "status": "dropped", "why": "not needed" } }),
        )
        .unwrap();

        let plan = m.lock().unwrap();
        let left: Vec<&str> = plan.unreached().iter().map(|i| i.text.as_str()).collect();
        assert_eq!(left, vec!["two", "three"], "done and dropped are both reached");
    }

    /// A call with no recognised argument says what the tool wants instead of
    /// silently succeeding — a silent no-op would leave the model believing it
    /// had written a plan.
    #[test]
    fn a_call_that_asks_for_nothing_says_so() {
        let m = Mutex::new(Plan::default());
        assert!(execute(&m, &serde_json::json!({})).is_err());
        assert!(m.lock().unwrap().is_empty());
    }

    /// `PLN-3`: *never* means no tool and no sentence, so nothing in the prompt
    /// invites a plan the run cannot write.
    #[test]
    fn never_offers_neither_the_tool_nor_the_sentence() {
        assert!(!PlanMode::Never.offers_tool());
        assert!(PlanMode::Never.guidance().is_empty());
        assert!(PlanMode::Auto.offers_tool());
        assert!(!PlanMode::Auto.guidance().is_empty());
    }

    #[test]
    fn an_unset_or_unknown_mode_is_when_it_helps() {
        assert_eq!(PlanMode::parse("auto"), PlanMode::Auto);
        assert_eq!(PlanMode::parse("nonsense"), PlanMode::Auto);
        assert_eq!(PlanMode::parse("always"), PlanMode::Always);
        assert_eq!(PlanMode::parse("never"), PlanMode::Never);

        let db = Db::open_in_memory().unwrap();
        assert_eq!(PlanMode::current(&db), PlanMode::Auto);
        db.set_setting(PLAN_MODE_KEY, "never").unwrap();
        assert_eq!(PlanMode::current(&db), PlanMode::Never);
    }
}
