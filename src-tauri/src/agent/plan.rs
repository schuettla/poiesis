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

/// `WRK-4`: where the second look at an item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    /// Someone who never saw the work is looking at it now.
    Checking,
    Checked,
    /// Not done after all: `reason` says what is missing.
    SentBack,
}

/// `WRK-4`: the second look's verdict on an item. A verdict that sends an item
/// back always has its reason: one without a reason does not exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub state: ReviewState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `WRK-7`: a move a way of working made, kept with the plan so the board can
/// say why, now and after a reload. `item` is the 0-based place, when it was
/// about one item; `state` is `fired` or `skipped`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveNote {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<usize>,
    /// What it fired on (`WRK-17`), so a run's moves can be kept as a preset.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub when: String,
    #[serde(rename = "move")]
    pub mv: String,
    pub rule: String,
    /// The rule as a sentence: "When an item has failed twice, I try two ways."
    pub text: String,
    /// `always` | `preset` | `learned` | `trying`.
    pub from: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub since: String,
    pub state: String,
}

/// `WRK-6`: two ways were tried for this item, and one was kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branches {
    /// `a` | `b`.
    pub kept: String,
    /// Why the kept one was kept, in a sentence.
    pub reason: String,
    /// What each way did, in a sentence, `[a, b]`.
    pub ways: Vec<String>,
    /// The id the other way's changes are read back by (`branch_view_cmd`).
    pub id: String,
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
    /// `USE-5`: the decision model judged this item finished from the answer, and
    /// how sure it was. Set only with `status: Done`, and cleared the moment the
    /// model or the user moves the item. The screen draws these ticks as rings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judged: Option<f32>,
    /// The decision behind `judged`, so reopening the item can label it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    /// `WRK-2`: what has to be true for this item to count as done, in one
    /// sentence. Written with the item, shown under it, and what the final check
    /// is held against. Only plans written under a way of working carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_when: Option<String>,
    /// `WRK-4`: the second look, while it runs and after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<Review>,
    /// `WRK-6`: two ways were tried, and which one was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<Branches>,
    /// `WRK-14`: who is working on this item, when it is not the lead: "helper",
    /// or the name of an agent app. Set by the loop from a `delegate` call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

/// `WRK-10`: one thing that has to be true for the whole task to count as done,
/// agreed with the person before work starts. The final check ticks it: `met` is
/// `None` until then, and a criterion that was not met stays open with `why`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Criterion {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub met: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
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
    /// `PLF-3`: the run that wrote this was planning first, so nothing has
    /// changed yet and the user has to approve it. Persisted with the plan, so
    /// the approval row is still there after a reload.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub awaiting_approval: bool,
    /// `WRK-2`: the preset (its slug) the run was working under, when it was not
    /// Ad hoc. Persisted with the plan so the board's header survives a reload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// `WRK-7`: what the way of working did in this run, for the board's ⓘ.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moves: Vec<MoveNote>,
    /// `WRK-10`: what done means for the whole task, agreed before work starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub criteria: Vec<Criterion>,
}

/// `WRK-2`: what the run's way of working holds this plan to. Nothing here
/// authorises or refuses a tool call: it decides what the *plan* may be told.
/// The default holds the plan to nothing, which is every run that is not working
/// under a preset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discipline {
    /// "I finish one thing before the next": only the first open item may be
    /// started, and nothing is called done while an earlier item is open.
    pub one_at_a_time: bool,
    /// The line is stopped by this check. Nothing is called done and no other
    /// item is started until it passes.
    pub stopped_by: Option<String>,
    /// `WRK-22`: project files were changed and this check (the project's own) has
    /// not run since. Nothing is called done until it has: "done" is something a
    /// program says, not only a second look. Never set in Ad hoc.
    pub unchecked: Option<String>,
}

impl Discipline {
    /// May the item at `index` (1-based) be moved to `status` now? An error is
    /// written for the model to read and act on, like every other one here.
    fn check(&self, plan: &Plan, index: usize, status: PlanStatus) -> Result<(), String> {
        let Some(item) = plan.items.get(index.wrapping_sub(1)) else {
            // Out of range: `Plan::update` words that mistake.
            return Ok(());
        };
        let starting = status == PlanStatus::Doing && item.status != PlanStatus::Doing;
        let finishing = status == PlanStatus::Done && item.status != PlanStatus::Done;
        if let Some(check) = &self.stopped_by {
            if starting || finishing {
                return Err(format!(
                    "Stopped: `{check}` is failing. Fix that before anything else. Item {index} can't be                      {} until it passes.",
                    if starting { "started" } else { "called done" }
                ));
            }
        }
        if let Some(check) = &self.unchecked {
            if finishing {
                return Err(format!(
                    "You changed files and `{check}` has not run since. Run it with run_task; item {index} can be called done once it passes."
                ));
            }
        }
        if self.one_at_a_time && (starting || finishing) {
            // The first item still open, other than this one, that comes before it.
            let earlier = plan
                .items
                .iter()
                .take(index - 1)
                .position(|i| matches!(i.status, PlanStatus::Todo | PlanStatus::Doing));
            if let Some(j) = earlier {
                return Err(format!(
                    "Item {} comes first. Finish it, or drop it with a reason, before {} item {index}.",
                    j + 1,
                    if starting { "starting" } else { "finishing" },
                ));
            }
            if starting {
                if let Some(j) = plan
                    .items
                    .iter()
                    .position(|i| i.status == PlanStatus::Doing)
                {
                    return Err(format!(
                        "Item {} is still in hand. Finish it, or drop it with a reason, before starting item {index}.",
                        j + 1
                    ));
                }
            }
        }
        Ok(())
    }
}

/// What a plan write did to an item the board draws a divider for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Started,
    Done,
    Dropped,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.criteria.is_empty()
    }

    /// Replace the list. The first `set` of a run is the plan being written; a
    /// later one is a revision, and says so.
    fn set(&mut self, texts: Vec<(String, Option<String>)>) {
        // The same list written again is not a revision: nothing about it changed,
        // and starting its items over would undo work already marked.
        let same = texts.len() == self.items.len()
            && texts.iter().zip(&self.items).all(|((t, d), i)| *t == i.text && *d == i.done_when);
        if same {
            return;
        }
        if !self.items.is_empty() {
            self.previous.push(self.items.iter().map(|i| i.text.clone()).collect());
            self.revisions += 1;
        }
        self.items = texts
            .into_iter()
            .map(|(text, done_when)| PlanItem {
                text,
                status: PlanStatus::Todo,
                why: None,
                added: false,
                judged: None,
                decision: None,
                done_when,
                review: None,
                branches: None,
                by: None,
            })
            .collect();
    }

    /// `WRK-14`: tasks handed to helpers say which plan item they are (`item`,
    /// numbered from 1). Each such item is marked as being worked on by that helper:
    /// the agent's own name, or "helper" for a general one. Returns the items
    /// assigned (0-based) and whether each went to a named agent. An item that is
    /// done or dropped, or a number that is not in the plan, is left alone.
    pub fn assign(&mut self, tasks: &serde_json::Value) -> Vec<(usize, bool)> {
        let mut out = Vec::new();
        for task in tasks.as_array().into_iter().flatten() {
            let Some(n) = task.get("item").and_then(|v| v.as_u64()).and_then(|n| usize::try_from(n).ok()) else {
                continue;
            };
            let Some(item) = n.checked_sub(1).and_then(|i| self.items.get_mut(i)) else { continue };
            if matches!(item.status, PlanStatus::Done | PlanStatus::Dropped) {
                continue;
            }
            let agent = task.get("agent").and_then(|a| a.as_str()).map(str::trim).filter(|a| !a.is_empty());
            item.by = Some(agent.unwrap_or("helper").to_string());
            if item.status == PlanStatus::Todo {
                item.status = PlanStatus::Doing;
            }
            out.push((n - 1, agent.is_some()));
        }
        out
    }

    /// `WRK-10`: the criteria the person is asked to agree to. Writing them again
    /// replaces them: an agreement the person has not given yet is a draft.
    pub fn set_criteria(&mut self, texts: Vec<String>) {
        self.criteria = texts.into_iter().map(|text| Criterion { text, met: None, why: None }).collect();
    }

    /// `WRK-10`: what the final check found, one entry per criterion in order.
    /// Fewer entries than criteria leaves the rest as they were.
    pub fn tick_criteria(&mut self, found: &[(bool, Option<String>)]) {
        for (c, (met, why)) in self.criteria.iter_mut().zip(found) {
            c.met = Some(*met);
            c.why = if *met { None } else { why.clone().filter(|w| !w.trim().is_empty()) };
        }
    }

    /// `WRK-10`: criteria the final check has not met (or not looked at yet).
    pub fn criteria_open(&self) -> Vec<&Criterion> {
        self.criteria.iter().filter(|c| c.met != Some(true)).collect()
    }

    /// Append work the plan did not foresee, marked as an addition.
    fn add(&mut self, texts: Vec<(String, Option<String>)>) {
        for (text, done_when) in texts {
            self.items.push(PlanItem {
                text,
                status: PlanStatus::Todo,
                why: None,
                added: true,
                judged: None,
                decision: None,
                done_when,
                review: None,
                branches: None,
                by: None,
            });
        }
    }

    /// `WRK-2`: the loop starts the next item itself. When nothing is being
    /// worked on and something is still to do, the first such item becomes the
    /// one in hand, so the board is true without the model spending a call on it.
    /// Returns the place (0-based) of the item it started.
    pub fn start_next(&mut self) -> Option<usize> {
        if self.items.iter().any(|i| i.status == PlanStatus::Doing) {
            return None;
        }
        let n = self.items.iter().position(|i| i.status == PlanStatus::Todo)?;
        self.items[n].status = PlanStatus::Doing;
        Some(n)
    }

    /// `WRK-2`: the statuses as they stand, to compare a write against.
    pub fn statuses(&self) -> Vec<PlanStatus> {
        self.items.iter().map(|i| i.status).collect()
    }

    /// `WRK-2`: what a write changed, item by item, against the statuses taken
    /// before it. An item the earlier list did not have counts as `Todo`.
    pub fn changes_since(&self, before: &[PlanStatus]) -> Vec<(usize, Change)> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(n, item)| {
                let was = before.get(n).copied().unwrap_or(PlanStatus::Todo);
                if was == item.status {
                    return None;
                }
                match item.status {
                    PlanStatus::Doing => Some((n, Change::Started)),
                    PlanStatus::Done => Some((n, Change::Done)),
                    PlanStatus::Dropped => Some((n, Change::Dropped)),
                    PlanStatus::Todo => None,
                }
            })
            .collect()
    }

    /// The items still open, with their 0-based place in the list.
    pub fn open_items(&self) -> Vec<(usize, &PlanItem)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| matches!(i.status, PlanStatus::Todo | PlanStatus::Doing))
            .collect()
    }

    /// `USE-5`: tick an open item off because the decision model judged the
    /// answer to finish it. Returns whether it was open: an item the model closed
    /// or dropped in the meantime is left as it is.
    pub fn mark_judged(&mut self, index: usize, p: f32, decision: &str) -> bool {
        let Some(item) = self.items.get_mut(index) else { return false };
        if !matches!(item.status, PlanStatus::Todo | PlanStatus::Doing) {
            return false;
        }
        item.status = PlanStatus::Done;
        item.judged = Some(p);
        item.decision = Some(decision.to_string());
        true
    }

    /// `USE-5`: the user reopened a judged tick. Returns the decision it came
    /// from, which is now known to be wrong; `None` when the item was not judged
    /// (a tick the model gave itself is not this feature's to undo).
    pub fn reopen(&mut self, index: usize) -> Option<String> {
        let item = self.items.get_mut(index)?;
        item.judged?;
        item.status = PlanStatus::Todo;
        item.judged = None;
        item.decision.take()
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
        // A verdict belongs to the work it judged: moving the item ends it. An item
        // that stays where it is keeps what the second look said about it.
        if item.status != status {
            item.review = None;
        }
        item.status = status;
        // The model has now spoken for the item itself, which replaces the guess.
        item.judged = None;
        item.decision = None;
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
        let mut lines = Vec::with_capacity(self.items.len() + self.criteria.len() + 1);
        if !self.criteria.is_empty() {
            lines.push("Done means:".to_string());
            for c in &self.criteria {
                let mark = match c.met {
                    Some(true) => "met",
                    Some(false) => "not met",
                    None => "not checked yet",
                };
                lines.push(format!("- {} ({mark})", c.text));
            }
        }
        for (n, item) in self.items.iter().enumerate() {
            let mut line = format!("{}. {} — {}", n + 1, item.text, item.status.as_note());
            if let Some(why) = &item.why {
                line.push_str(": ");
                line.push_str(why);
            }
            if item.added {
                line.push_str(" (added later)");
            }
            if item.judged.is_some() {
                line.push_str(" (judged finished from your answer)");
            }
            if let Some(done_when) = &item.done_when {
                line.push_str(&format!(" (done when: {done_when})"));
            }
            if let Some(Review { state: ReviewState::SentBack, reason }) = &item.review {
                line.push_str(&format!(" (sent back by the second look: {})", reason.as_deref().unwrap_or("not done")));
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
    tool_specs_with(false)
}

/// How many criteria one task is agreed on. More than this is a list nobody
/// reads, and a final check that cannot be held to any of them.
pub const MAX_CRITERIA: usize = 5;

/// `WRK-2`: with `done_when`, each item of `items` is an object that says what
/// has to be true for it to count as done. Plain JSON Schema either way.
pub fn tool_specs_with(done_when: bool) -> Vec<serde_json::Value> {
    tool_specs_for(done_when, false)
}

/// `WRK-10`: with `criteria`, the plan also says what done means for the whole
/// task, which the person is asked to agree to before anything changes.
pub fn tool_specs_for(done_when: bool, criteria: bool) -> Vec<serde_json::Value> {
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
    let items = if done_when {
        serde_json::json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "The item, as a short phrase." },
                    "done_when": {
                        "type": "string",
                        "description": "One sentence: what has to be true for this item to count as done, so it can be checked."
                    }
                },
                "required": ["text", "done_when"]
            },
            "description": "The whole plan, replacing any earlier one, in the order you mean to do it. Each item says what done means for it."
        })
    } else {
        serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
            "description": "The whole plan, replacing any earlier one. Short phrases, in the order you mean to do them."
        })
    };
    let mut properties = serde_json::json!({
        "items": items,
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
    });
    if criteria {
        properties["criteria"] = serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
            "description": "What done means for the whole task: two to four statements, each one something that can be seen to be true or false when the work is over. The person agrees to these before anything changes, and they are checked against your answer at the end."
        });
    }
    vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": "plan",
            "description": "Write or revise the short plan for this run, and keep it current. Pass `items` to write the whole plan (a handful of short phrases, in order). Pass `update` to move an item, or `updates` to move several at once — items are numbered from 1, as shown when the plan is read back. Pass `add` for work you did not foresee. The plan is shown to the user as you work, so it must stay true: drop an item with a reason rather than leaving it hanging. Do not spend a whole turn on bookkeeping — call this in the same turn as the work it describes, and finish one item and start the next in a single call: updates: [{index:2,status:\"done\"},{index:3,status:\"doing\"}].",
            "parameters": {
                "type": "object",
                "properties": properties
            }
        }
    })]
}

/// The timeline line for a `plan` call. Past tense, like every other step.
pub fn describe(_name: &str, args: &serde_json::Value) -> (String, String) {
    if let Some(items) = args.get("items").and_then(|v| v.as_array()) {
        return ("planned".to_string(), format!("{} steps", items.len()));
    }
    if args.get("criteria").is_some() {
        return ("agreed what done means".to_string(), String::new());
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
    execute_with(plan, args, &Discipline::default())
}

/// [`execute`], with the plan held to a way of working (`WRK-2`).
pub fn execute_with(plan: &Mutex<Plan>, args: &serde_json::Value, rules: &Discipline) -> Result<String, String> {
    let mut plan = plan.lock().unwrap();

    // An item is a phrase, or an object with the phrase and what done means for
    // it. Either is accepted whatever the schema offered: small models guess.
    let strings = |v: Option<&serde_json::Value>| -> Vec<(String, Option<String>)> {
        v.and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| match x {
                        serde_json::Value::String(s) => Some((s.trim().to_string(), None)),
                        serde_json::Value::Object(o) => {
                            let text = o.get("text")?.as_str()?.trim().to_string();
                            let done_when = o
                                .get("done_when")
                                .and_then(|d| d.as_str())
                                .map(|d| d.trim().to_string())
                                .filter(|d| !d.is_empty());
                            Some((text, done_when))
                        }
                        _ => None,
                    })
                    .filter(|(text, _)| !text.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };

    let mut did_something = false;

    // `WRK-10`: what done means for the whole task. Strings, or objects with a
    // `text`: small models guess here too.
    if let Some(list) = args.get("criteria").and_then(|v| v.as_array()) {
        let texts: Vec<String> = list
            .iter()
            .filter_map(|x| match x {
                serde_json::Value::String(s) => Some(s.trim().to_string()),
                serde_json::Value::Object(o) => o.get("text").and_then(|t| t.as_str()).map(|t| t.trim().to_string()),
                _ => None,
            })
            .filter(|t| !t.is_empty())
            .take(MAX_CRITERIA)
            .collect();
        if texts.is_empty() {
            return Err("`criteria` needs at least one statement that can be seen to be true or false.".into());
        }
        plan.set_criteria(texts);
        did_something = true;
    }

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
        rules.check(&plan, index, status)?;
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

    /// `USE-5`: a judged tick keeps its probability and decision, is not left
    /// open for the nudge, and a reopen hands the decision back for labelling.
    #[test]
    fn a_judged_tick_can_be_reopened_and_names_its_decision() {
        let m = plan_with(&["one", "two", "three"]);
        execute(&m, &serde_json::json!({ "update": { "index": 3, "status": "dropped", "why": "no" } })).unwrap();
        let mut plan = m.lock().unwrap();
        assert_eq!(plan.open_items().iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![0, 1]);

        assert!(plan.mark_judged(0, 0.88, "d1"));
        assert!(!plan.mark_judged(2, 0.99, "d2"), "a dropped item is not ticked");
        assert!(!plan.mark_judged(9, 0.99, "d3"), "no such item");
        assert_eq!((plan.items[0].status, plan.items[0].judged), (PlanStatus::Done, Some(0.88)));
        assert_eq!(plan.unreached().len(), 1, "only item two is still open for the nudge");
        assert!(plan.render().contains("1. one — done (judged finished from your answer)"));

        assert_eq!(plan.reopen(0).as_deref(), Some("d1"));
        assert_eq!((plan.items[0].status, plan.items[0].judged, plan.items[0].decision.clone()), (PlanStatus::Todo, None, None));
        assert_eq!(plan.reopen(0), None, "nothing left to undo");
        // A tick the model gave itself is not a judgement, so there is nothing to label.
        plan.update(2, PlanStatus::Done, None).unwrap();
        assert_eq!(plan.reopen(1), None);
        assert_eq!(plan.items[1].status, PlanStatus::Done);
    }

    /// The model moving an item itself replaces the guess about it.
    #[test]
    fn the_models_own_update_clears_a_judgement() {
        let m = plan_with(&["one"]);
        m.lock().unwrap().mark_judged(0, 0.9, "d1");
        execute(&m, &serde_json::json!({ "update": { "index": 1, "status": "doing" } })).unwrap();
        let plan = m.lock().unwrap();
        assert_eq!((plan.items[0].status, plan.items[0].judged, plan.items[0].decision.clone()), (PlanStatus::Doing, None, None));
        // A judgement is not written for a plan that never had one.
        let json = serde_json::to_string(&plan.items[0]).unwrap();
        assert!(!json.contains("judged") && !json.contains("decision"), "{json}");
    }

    /// `WRK-2`: an item can say what done means for it, and the model reads that
    /// back with the item. A phrase with no such sentence still works.
    #[test]
    fn an_item_can_say_what_done_means_and_the_model_reads_it_back() {
        let m = Mutex::new(Plan::default());
        execute(
            &m,
            &serde_json::json!({
                "items": [{ "text": "export", "done_when": "the file opens in Excel" }, "plain"]
            }),
        )
        .unwrap();
        let plan = m.lock().unwrap();
        assert_eq!(plan.items[0].done_when.as_deref(), Some("the file opens in Excel"));
        assert_eq!(plan.items[1].done_when, None);
        assert!(
            plan.render().contains("1. export \u{2014} to do (done when: the file opens in Excel)"),
            "{}",
            plan.render()
        );
        assert!(!serde_json::to_string(&plan.items[1]).unwrap().contains("done_when"), "nothing written for none");
    }

    #[test]
    fn the_work_schema_asks_for_objects_and_the_plain_one_for_phrases() {
        let plain = tool_specs();
        let work = tool_specs_with(true);
        let items = |spec: &Vec<serde_json::Value>| {
            spec[0].pointer("/function/parameters/properties/items/items/type").cloned()
        };
        assert_eq!(items(&plain), Some(serde_json::json!("string")));
        assert_eq!(items(&work), Some(serde_json::json!("object")));
        assert!(work[0].pointer("/function/parameters/properties/items/items/properties/done_when").is_some());
    }

    /// `WRK-2`: what a write changed, so the board can draw a divider for it.
    #[test]
    fn the_changes_since_a_write_name_what_started_and_what_ended() {
        let m = plan_with(&["one", "two", "three"]);
        let mut plan = m.lock().unwrap();
        let before = plan.statuses();
        assert_eq!(plan.start_next(), Some(0));
        assert_eq!(plan.start_next(), None, "one is already in hand");
        assert_eq!(plan.changes_since(&before), vec![(0, Change::Started)]);

        let before = plan.statuses();
        plan.items[0].status = PlanStatus::Done;
        plan.items[1].status = PlanStatus::Dropped;
        assert_eq!(plan.changes_since(&before), vec![(0, Change::Done), (1, Change::Dropped)]);

        // An item the earlier list did not have counts as one still to do.
        let before = plan.statuses();
        plan.add(vec![("four".into(), None)]);
        assert!(plan.changes_since(&before).is_empty());
    }

    /// `WRK-10`: criteria ride on the plan, are read back to the model, are ticked
    /// one by one, and a met criterion carries no excuse.
    #[test]
    fn criteria_are_written_read_back_and_ticked() {
        let m = Mutex::new(Plan::default());
        execute(
            &m,
            &serde_json::json!({ "items": ["export"], "criteria": ["Fleet has an Export button", { "text": "Excel opens the file" }, "  "] }),
        )
        .unwrap();
        let mut plan = m.lock().unwrap();
        assert_eq!(plan.criteria.len(), 2, "a blank one is not a criterion");
        assert!(plan.render().contains("- Fleet has an Export button (not checked yet)"));

        plan.tick_criteria(&[(true, Some("ignored".into())), (false, Some("it opens with one column".into()))]);
        assert_eq!(plan.criteria[0], Criterion { text: "Fleet has an Export button".into(), met: Some(true), why: None });
        assert_eq!(plan.criteria[1].why.as_deref(), Some("it opens with one column"));
        assert_eq!(plan.criteria_open().len(), 1);
        assert!(plan.render().contains("(not met)"));
    }

    /// `WRK-10`: criteria alone are a plan write, and an empty list is not.
    #[test]
    fn criteria_alone_count_and_an_empty_list_does_not() {
        let m = Mutex::new(Plan::default());
        assert!(execute(&m, &serde_json::json!({ "criteria": ["it works"] })).is_ok());
        assert!(execute(&m, &serde_json::json!({ "criteria": [] })).is_err());
        let spec = tool_specs_for(true, true);
        assert!(spec[0].pointer("/function/parameters/properties/criteria").is_some());
        assert!(tool_specs_for(true, false)[0].pointer("/function/parameters/properties/criteria").is_none());
    }

    /// `WRK-14`: tasks handed to helpers mark the item they are, and nothing else.
    #[test]
    fn handing_an_item_to_a_helper_marks_who_has_it_and_leaves_the_rest_alone() {
        let m = plan_with(&["export", "button", "docs", "done already"]);
        let mut plan = m.lock().unwrap();
        plan.items[3].status = PlanStatus::Done;
        let tasks = serde_json::json!([
            { "task": "a", "item": 1 },
            { "task": "b", "item": 2, "agent": " Claude Code " },
            { "task": "c", "item": 4 },
            { "task": "d", "item": 9 },
            { "task": "e", "item": 0 },
            { "task": "no item" },
        ]);
        let assigned = plan.assign(&tasks);
        assert_eq!(assigned, vec![(0, false), (1, true)], "done work and numbers outside the plan are left alone");
        assert_eq!((plan.items[0].by.as_deref(), plan.items[0].status), (Some("helper"), PlanStatus::Doing));
        assert_eq!(plan.items[1].by.as_deref(), Some("Claude Code"));
        assert!(plan.items[2].by.is_none() && plan.items[2].status == PlanStatus::Todo);
        assert!(plan.items[3].by.is_none());
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
