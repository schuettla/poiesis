//! `WRK-1`/`WRK-2`: the way a run works, enforced by the loop.
//!
//! A way of working is a rule: *when* something happens, *I do* something
//! (`memory::ways`). The rules a run holds are the Always ones, what was learned
//! in scope, and the chat's preset on top. This file is the part that carries
//! them out, and it is deliberately small: it watches what the loop already
//! knows (how checks ended, what a tool said, which item is in hand) and answers
//! with a `Reaction`: something to tell the model, something to show the person,
//! or a signal to write down.
//!
//! Three rules hold the whole design up:
//!
//! - **Enforced, not hinted.** A model that is told to fix a failing check first
//!   can ignore it. Here the plan refuses to call an item done while the line is
//!   stopped (`plan::Discipline`), and the loop asks again before it lets an
//!   answer go out over a failing check. Work tools are never refused: the plan
//!   describes intent, it does not authorise (`plan.rs`).
//! - **The chat stays quiet.** The person sees the board, the run bar and, for a
//!   repeated problem, one question. Nothing here writes a line into the answer.
//! - **Every way of working leaves a signal.** What fired, on which rule, is
//!   written to the session log (`way` rows) from the first day, so there is
//!   evidence when the organization starts to evolve.
//!
//! Nothing in this file decides *which* rules a run has. `MemoryStore::rules_for`
//! does, and a rule whose move this loop cannot carry out yet is simply inert:
//! the fixed lists in `memory::ways` are the whole vocabulary, and the later
//! steps of `ORG_LOOP_PLAN` add the moves behind them one by one.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, RwLock};

use crate::agent::aside::Look;
use crate::agent::ledger::CheckResult;
use crate::agent::plan::{Change, Discipline, MoveNote, Plan, PlanStatus, Review, ReviewState};
use crate::agent::AgentEvent;
use crate::db::Db;
use crate::memory::ways::{Move, Rule, ScopeCtx, Trigger, AD_HOC};
use crate::memory::MemoryStore;

/// Files changed inside one item before "I've changed many files" holds.
pub const MANY_FILES: usize = 6;
/// How often a run is asked to deal with a stopped line before it may answer.
/// A model that cannot fix it will not on the third ask either, and every ask
/// costs the person a turn.
const MAX_LINE_NUDGES: usize = 2;
/// The first `ask_user` that does not come is asked for once more.
const MAX_ASK_MISSES: usize = 1;
/// How often one item is sent back before the run is let go on. A second look
/// that keeps saying no to a model that cannot do better is a loop, not a check.
const MAX_SENDBACKS: u32 = 2;
/// What the second look is told was done in an item, newest last.
const TRAIL_LINES: usize = 40;
/// My head is getting full past this share of the window. Clearing old results
/// starts at three quarters, so a fresh start comes first where there is a way
/// for it.
const HEAD_FULL: (usize, usize) = (2, 3);
/// How often one run forgets on purpose. A run that needs a fourth has a task
/// that does not fit, which a person should hear about.
const MAX_FRESH_STARTS: usize = 3;
/// Turns a "not this time" holds a fresh start off for.
const FRESH_PAUSE: usize = 8;

/// What a way of working did, for the loop to carry out.
#[derive(Debug, Clone)]
pub enum Reaction {
    /// Said to the model, as a system turn.
    Tell(String),
    /// Said to the screen.
    Show(AgentEvent),
    /// Written to the session log, for the evolution to come.
    Record(Signal),
}

/// One fired way of working, as the log keeps it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Signal {
    pub rule: String,
    pub trigger: String,
    #[serde(rename = "move")]
    pub mv: String,
    /// What happened, in a few words: the check, the problem.
    pub what: String,
    /// The item it happened in, 0-based, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<usize>,
}

#[derive(Debug, Clone)]
struct Stopped {
    check: String,
}

#[derive(Default)]
struct State {
    stopped: Option<Stopped>,
    /// Per check or tool: how it last failed and how many times in a row.
    last_failure: HashMap<String, (String, u32)>,
    /// Problems already put to the person, so one is asked about once.
    asked: HashSet<String>,
    /// The loop owes the person a question about this.
    ask_pending: Option<String>,
    ask_misses: usize,
    /// The item in hand (0-based) and how many files had changed when it began.
    current_item: Option<usize>,
    item_base: usize,
    /// The item already told to split, so it is told once.
    split_told: Option<usize>,
    line_nudges: usize,
    /// `WRK-4`: what was done in the item in hand, in outline, for the second look.
    trail: Vec<String>,
    /// When the item in hand began (epoch ms), so its patch can be told from the rest.
    item_started_ms: i64,
    /// How often each item was sent back.
    sendbacks: HashMap<usize, u32>,
    /// The latest word on each check, `label -> "passed" | "failing: ..."`.
    checks: BTreeMap<String, String>,
    /// `WRK-7`: rules the person said "not this time" to. Each is skipped the next
    /// time it would fire, and then fires again.
    skip_once: HashSet<String>,
    /// `WRK-5`: how often the run forgot on purpose, whether it may again, and the
    /// turn a "not this time" holds it off until.
    fresh_starts: usize,
    fresh_blocked: bool,
    fresh_pause_until: usize,
    /// `WRK-6`: how often each item failed (a stopped line, a send-back), the
    /// way asked for and what it was about, the items already branched, and
    /// whether branching stopped working in this run.
    item_fails: HashMap<usize, u32>,
    branch_wanted: Option<(usize, String)>,
    branched: HashSet<usize>,
    branch_blocked: bool,
    /// `WRK-10`: the run is looking around first and changes nothing, until the
    /// plan is written or a question about what is wanted is answered.
    looking: bool,
    /// `WRK-10`: the run is agreeing what done means; nothing changes until the
    /// person says yes.
    criteria_phase: bool,
    /// `WRK-10`: how often the criteria were checked against an answer, and how
    /// often that sent the answer back.
    criteria_looks: u32,
    criteria_sendbacks: u32,
    /// `WRK-10`: items already checked in about.
    checked_in: HashSet<usize>,
    /// `WRK-10`: the person said they will take it from here.
    taken_over: bool,
    /// `WRK-13`: a preset the person asked for in the middle of the run, applied
    /// when no item is in hand.
    pending_switch: Option<String>,
    /// `WRK-13`: a switch was offered, or the run was told it has grown. Once each.
    bigger_told: bool,
    switch_offered: bool,
}

/// `ScopeCtx` with its own strings.
struct OwnedScope {
    project: Option<String>,
    persona: Option<String>,
    kind: Option<String>,
}

impl OwnedScope {
    fn as_ctx(&self) -> ScopeCtx<'_> {
        ScopeCtx { project: self.project.as_deref(), persona: self.persona.as_deref(), kind: self.kind.as_deref() }
    }
}

/// What the second look is given besides the patch.
pub struct Evidence {
    pub trail: Vec<String>,
    pub checks: Vec<String>,
    pub since_ms: i64,
}

/// How the run works right now. Apart from the rest because `WRK-13` can change
/// it in the middle of a run, at the end of an item.
struct Live {
    /// The preset's slug.
    preset: String,
    name: String,
    /// `chat` | `message` | `switch`.
    source: &'static str,
    rules: Vec<Rule>,
}

pub struct Work {
    live: RwLock<Live>,
    run_id: String,
    /// Whether the person can be asked anything: a lead run somebody watches.
    can_ask: bool,
    /// `WRK-7`: every move made so far, for the board. Apart from `state` so a
    /// move can be written down from anywhere without taking its lock.
    moves: Mutex<Vec<MoveNote>>,
    /// Whether this run has a `plan` tool to write its items with. A model too
    /// small to plan cannot work item by item, and must not be told to.
    pub can_plan: bool,
    /// `WRK-6`: whether this run can start helpers in copies of its folder: a lead
    /// run that may delegate and works in a folder.
    pub can_branch: bool,
    state: Mutex<State>,
}

/// Lowercase, digits folded, spaces collapsed, cut short: two failures that read
/// the same to a person are the same here, whatever line number they name.
fn normalize(s: &str) -> String {
    let mut out = String::new();
    let mut last_digit = false;
    for c in s.to_lowercase().chars() {
        if c.is_ascii_digit() {
            if !last_digit {
                out.push('#');
            }
            last_digit = true;
        } else {
            last_digit = false;
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(100).collect()
}

impl Work {
    pub fn new(run_id: &str, preset: &str, name: &str, source: &'static str, rules: Vec<Rule>, can_ask: bool) -> Work {
        Work {
            live: RwLock::new(Live { preset: preset.to_string(), name: name.to_string(), source, rules }),
            run_id: run_id.to_string(),
            can_ask,
            moves: Mutex::new(Vec::new()),
            can_plan: true,
            can_branch: false,
            state: Mutex::new(State::default()),
        }
    }

    /// Which way this run works, and the rules that come with it.
    ///
    /// `message_style` is a `/style` for this message alone; otherwise the chat's
    /// own setting, and failing that where the chat starts (`MemoryStore::default_for`:
    /// kind of task, persona, project, everywhere). Only a lead run somebody watches
    /// is told by the person or by those defaults. A delegated child or an unattended
    /// task has the preset its own conversation was given (`WRK-18`: the lead's choice,
    /// Lean otherwise; the task's), and works under the base rules without one. What
    /// needs a person (criteria, check-ins, looking around first) waits for `can_ask`.
    pub fn resolve(
        db: &Db,
        memory: &MemoryStore,
        conversation_id: &str,
        run_id: &str,
        message_style: Option<&str>,
        carried: Option<&str>,
        lead_and_watched: bool,
    ) -> Work {
        let conv = db.get_conversation(conversation_id).ok().flatten();
        // A plan that carries on (approved, or resumed) keeps the way it was written
        // under, whatever the chat says now: "Go ahead" is a message with no style.
        let (asked, source) = match message_style.filter(|s| !s.trim().is_empty()) {
            Some(s) if lead_and_watched => (Some(s.trim().to_string()), "message"),
            _ => match carried.filter(|s| !s.trim().is_empty() && lead_and_watched) {
                Some(s) => (Some(s.trim().to_string()), "chat"),
                None => {
                    let own = conv.as_ref().and_then(|c| c.style.clone());
                    // Where the chat starts, for a chat nobody chose a preset for.
                    let started = own.or_else(|| {
                        if !lead_and_watched {
                            return None;
                        }
                        let ctx = Work::scope_of(db, conversation_id);
                        memory.default_for(&ctx.as_ctx()).map(|p| p.slug)
                    });
                    (started, "chat")
                }
            },
        };
        // A preset that no longer exists (deleted, or from a newer version) is the
        // default, not an error: the run still has to run.
        let preset = asked.and_then(|typed| memory.find_preset(&typed));
        let (slug, name, source) = match &preset {
            Some(p) => (p.slug.clone(), p.name.clone(), source),
            None => (AD_HOC.to_string(), "Ad hoc".to_string(), "chat"),
        };
        let rules = Work::rules_in_scope(db, memory, conversation_id, &slug);
        Work::new(run_id, &slug, &name, source, rules, lead_and_watched)
    }

    /// What a preset holds in this conversation: the Always rules, what was learned
    /// for this project and persona, and the preset on top.
    fn rules_in_scope(db: &Db, memory: &MemoryStore, conversation_id: &str, slug: &str) -> Vec<Rule> {
        let scope = Work::scope_of(db, conversation_id);
        memory.rules_for(slug, &scope.as_ctx())
    }

    /// Where this conversation is, for narrowing a way of working or choosing where a
    /// chat starts: its project, its persona and the kind of work it is in so far.
    fn scope_of(db: &Db, conversation_id: &str) -> OwnedScope {
        let conv = db.get_conversation(conversation_id).ok().flatten();
        OwnedScope {
            project: db.conversation_project(conversation_id).ok().flatten().map(|p| p.id),
            persona: conv.and_then(|c| c.persona_id),
            kind: crate::agent::selfmodel::kind_now(db, conversation_id).map(str::to_string),
        }
    }

    /// `WRK-13`: the name and rules of a preset asked for in the middle of a run.
    /// `None` when it no longer exists.
    pub fn rules_of(db: &Db, memory: &MemoryStore, conversation_id: &str, slug: &str) -> Option<(String, Vec<Rule>)> {
        let preset = memory.find_preset(slug)?;
        Some((preset.name.clone(), Work::rules_in_scope(db, memory, conversation_id, &preset.slug)))
    }

    /// What the way of working did so far, as the board keeps it.
    pub fn moves(&self) -> Vec<MoveNote> {
        self.moves.lock().unwrap().clone()
    }

    pub fn is_ad_hoc(&self) -> bool {
        self.live.read().unwrap().preset == AD_HOC
    }

    pub fn preset(&self) -> String {
        self.live.read().unwrap().preset.clone()
    }

    pub fn rule(&self, when: Trigger, mv: Move) -> Option<Rule> {
        self.live.read().unwrap().rules.iter().find(|r| r.when == when && r.mv == mv).cloned()
    }

    fn has_move(&self, mv: Move) -> bool {
        self.live.read().unwrap().rules.iter().any(|r| r.mv == mv)
    }

    /// "I finish one thing before the next": the run works its plan item by item.
    pub fn rounds(&self) -> bool {
        self.can_plan && self.has_move(Move::OneAtATime)
    }

    /// Does the way this run works write a plan: item by item, or with criteria?
    /// Asked before `can_plan` is known, to decide whether the plan tool is offered.
    pub fn plans(&self) -> bool {
        self.has_move(Move::OneAtATime) || self.has_move(Move::Criteria) || self.hands_out()
    }

    /// `WRK-14`: does this way of working hand items to helpers?
    pub fn hands_out(&self) -> bool {
        self.has_move(Move::Parallel) || self.has_move(Move::HandOff)
    }

    /// What the plan is held to right now.
    pub fn discipline(&self) -> Discipline {
        Discipline {
            one_at_a_time: self.rounds(),
            stopped_by: self.state.lock().unwrap().stopped.as_ref().map(|s| s.check.clone()),
            unchecked: None,
        }
    }

    /// `WRK-22`: `discipline`, plus the project's check when files were changed
    /// and it has not run since (`unchecked`, from the ledger and the project card).
    /// Ad hoc plans stay held to nothing but a stopped line.
    pub fn discipline_with(&self, unchecked: Option<String>) -> Discipline {
        Discipline { unchecked: unchecked.filter(|_| !self.is_ad_hoc()), ..self.discipline() }
    }

    /// Sent once, before the first request.
    pub fn style_event(&self) -> AgentEvent {
        let live = self.live.read().unwrap();
        AgentEvent::StyleSet {
            run_id: self.run_id.clone(),
            preset: live.preset.clone(),
            name: live.name.clone(),
            source: live.source.to_string(),
        }
    }

    /// `WRK-19`: the preset this run works under, for the log. The chat may not say
    /// (a default chose it), so evolution reads this to know how a chat really went.
    /// A switch in the middle of the run writes its own (`switch_to`).
    pub fn style_signal(&self) -> Signal {
        let live = self.live.read().unwrap();
        Signal {
            rule: STYLE_RULE.into(),
            trigger: "run".into(),
            mv: live.preset.clone(),
            what: live.source.to_string(),
            item: None,
        }
    }

    fn signal(&self, rule: &Rule, what: &str, item: Option<usize>) -> Reaction {
        Reaction::Record(Signal {
            rule: rule.id.clone(),
            trigger: rule.when.slug().to_string(),
            mv: rule.mv.slug().to_string(),
            what: what.to_string(),
            item,
        })
    }

    fn moved(&self, rule: &Rule, item: Option<usize>) -> Reaction {
        self.moved_as(&rule, item, "fired")
    }

    fn moved_as(&self, rule: &Rule, item: Option<usize>, state: &str) -> Reaction {
        let note = MoveNote {
            item,
            when: rule.when.slug().to_string(),
            mv: rule.mv.slug().to_string(),
            rule: rule.id.clone(),
            text: rule.text.clone(),
            from: rule.from.clone(),
            since: rule.since.clone(),
            state: state.to_string(),
        };
        {
            let mut moves = self.moves.lock().unwrap();
            // The same move on the same item says itself once.
            if !moves.iter().any(|m| m.rule == note.rule && m.item == note.item && m.state == note.state) {
                moves.push(note);
            }
        }
        Reaction::Show(AgentEvent::Move {
            run_id: self.run_id.clone(),
            item,
            mv: rule.mv.slug().to_string(),
            rule: rule.id.clone(),
            text: rule.text.clone(),
            from: rule.from.clone(),
            since: rule.since.clone(),
            state: state.into(),
        })
    }

    /// `WRK-7`: the rule for this trigger and move, unless the person said "not
    /// this time" to it. A skip is used up here and shown as such, so what the
    /// board says is what happened; it also leaves its own signal, because a rule
    /// that keeps being skipped is something the organization should hear about.
    fn due(&self, when: Trigger, mv: Move, item: Option<usize>) -> (Option<Rule>, Vec<Reaction>) {
        let Some(rule) = self.rule(when, mv) else { return (None, Vec::new()) };
        if self.state.lock().unwrap().skip_once.remove(&rule.id) {
            let out = vec![self.moved_as(&rule, item, "skipped"), self.signal(&rule, "not this time", item)];
            return (None, out);
        }
        (Some(rule), Vec::new())
    }

    /// `WRK-7`: "Not this time" on a rule: skip it the next time it would fire.
    pub fn skip_next(&self, rule_id: &str) -> bool {
        if !self.live.read().unwrap().rules.iter().any(|r| r.id == rule_id) {
            return false;
        }
        self.state.lock().unwrap().skip_once.insert(rule_id.to_string());
        true
    }

    /// The plan was written. Starts the next item when the run works item by
    /// item, and says which items began and ended. `edited` is how many files
    /// the run has changed so far, so "many" can be counted from where an item
    /// began.
    pub fn after_plan_write(&self, plan: &mut Plan, before: &[PlanStatus], edited: usize, auto_start: bool) -> Vec<AgentEvent> {
        let ad_hoc = self.is_ad_hoc();
        // A run switched to Ad hoc (`WRK-13`) still ends the item it had in hand.
        if ad_hoc && self.state.lock().unwrap().current_item.is_none() {
            return Vec::new();
        }
        plan.style = (!ad_hoc).then(|| self.preset());
        if auto_start && self.rounds() {
            plan.start_next();
        }
        let mut out = Vec::new();
        for (n, change) in plan.changes_since(before) {
            let item = &plan.items[n];
            match change {
                Change::Started => {
                    let mut st = self.state.lock().unwrap();
                    st.current_item = Some(n);
                    st.item_base = edited;
                    st.split_told = None;
                    st.trail.clear();
                    st.item_started_ms = crate::db::now_ms();
                    out.push(AgentEvent::ItemStarted {
                        run_id: self.run_id.clone(),
                        item: n,
                        text: item.text.clone(),
                        done_when: item.done_when.clone(),
                    });
                }
                Change::Done | Change::Dropped => {
                    let mut st = self.state.lock().unwrap();
                    if st.current_item == Some(n) {
                        st.current_item = None;
                    }
                    out.push(AgentEvent::ItemEnded {
                        run_id: self.run_id.clone(),
                        item: n,
                        text: item.text.clone(),
                        outcome: if change == Change::Done { "done" } else { "dropped" }.into(),
                        why: item.why.clone(),
                    });
                }
            }
        }
        out
    }

    /// A tool came back with an error. Two of the same in a row, with nothing
    /// in between that worked, is the same problem twice.
    pub fn failed(&self, key: &str, what: &str) -> Vec<Reaction> {
        let sig = normalize(what);
        let mut st = self.state.lock().unwrap();
        let count = match st.last_failure.get_mut(key) {
            Some((last, n)) if *last == sig => {
                *n += 1;
                *n
            }
            _ => {
                st.last_failure.insert(key.to_string(), (sig.clone(), 1));
                1
            }
        };
        if count < 2 {
            return Vec::new();
        }
        let Some(rule) = self.rule(Trigger::SameProblemTwice, Move::CheckIn) else { return Vec::new() };
        if !st.asked.insert(format!("{key}|{sig}")) {
            return Vec::new();
        }
        let what: String = what.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect();
        if self.can_ask {
            st.ask_pending = Some(what.clone());
            st.ask_misses = 0;
        }
        let item = st.current_item;
        drop(st);
        let tell = if self.can_ask {
            format!(
                "You have hit the same problem twice: {what}. Stop and ask the person before you try again. \
                 Call ask_user now with 2 or 3 concrete ways to go on, and begin the question with \
                 \"I've hit the same problem twice:\"."
            )
        } else {
            format!(
                "You have hit the same problem twice: {what}. Do not try the same thing a third time. \
                 Either do something different, or say plainly what is wrong and what you would try."
            )
        };
        vec![Reaction::Tell(tell), self.moved(&rule, item), self.signal(&rule, &what, item)]
    }

    /// A tool of this name worked: whatever it had been failing at is over.
    pub fn succeeded(&self, key: &str) {
        self.state.lock().unwrap().last_failure.remove(key);
    }

    /// How checks ended since the loop last looked: stop the line on a failure,
    /// start it again when the same check passes. Also counts a failing check as
    /// a problem, so one that fails the same way twice is asked about.
    pub fn after_checks(&self, results: Vec<CheckResult>) -> Vec<Reaction> {
        let mut out = Vec::new();
        for r in results {
            let key = format!("check:{}", r.label);
            self.state.lock().unwrap().checks.insert(
                r.label.clone(),
                if r.passed { "passed".to_string() } else { format!("failing ({})", r.signature) },
            );
            if r.passed {
                self.succeeded(&key);
                // The check that stopped the line passes. Another that is still failing
                // keeps it stopped: the line runs only when every check it knows passes.
                let rule = self.rule(Trigger::CheckFails, Move::StopTheLine);
                let cleared = {
                    let mut st = self.state.lock().unwrap();
                    match &st.stopped {
                        Some(s) if s.check == r.label => {
                            let still = st
                                .checks
                                .iter()
                                .find(|(label, said)| **label != r.label && said.starts_with("failing"))
                                .map(|(label, _)| label.clone())
                                .filter(|_| rule.is_some());
                            st.stopped = still.clone().map(|check| Stopped { check });
                            Some(still)
                        }
                        _ => None,
                    }
                };
                match (cleared, rule) {
                    (Some(Some(other)), Some(rule)) => {
                        out.push(Reaction::Show(AgentEvent::LineStopped {
                            run_id: self.run_id.clone(),
                            check: other.clone(),
                            rule: rule.id.clone(),
                            text: rule.text.clone(),
                        }));
                        out.push(Reaction::Tell(format!(
                            "`{}` passes again, but `{other}` is still failing. Fix that before anything else.",
                            r.label
                        )));
                    }
                    (Some(_), _) => {
                        out.push(Reaction::Show(AgentEvent::LineCleared {
                            run_id: self.run_id.clone(),
                            check: r.label.clone(),
                        }));
                        out.push(Reaction::Tell(format!(
                            "`{}` passes again. The line is running: carry on with the item in hand.",
                            r.label
                        )));
                    }
                    (None, _) => {}
                }
                continue;
            }
            if let Some(rule) = self.rule(Trigger::CheckFails, Move::StopTheLine) {
                let already = self.state.lock().unwrap().stopped.as_ref().is_some_and(|s| s.check == r.label);
                if !already {
                    let item = {
                        let mut st = self.state.lock().unwrap();
                        st.stopped = Some(Stopped { check: r.label.clone() });
                        st.current_item
                    };
                    out.push(Reaction::Show(AgentEvent::LineStopped {
                        run_id: self.run_id.clone(),
                        check: r.label.clone(),
                        rule: rule.id.clone(),
                        text: rule.text.clone(),
                    }));
                    out.push(Reaction::Tell(format!(
                        "Stopped: `{}` is failing ({}). Fix that before anything else. Do not call any item done \
                         or start another until it passes; run it again to find out.",
                        r.label, r.signature
                    )));
                    out.push(self.signal(&rule, &r.label, item));
                    if let Some(n) = item {
                        out.extend(self.item_failed(n, &format!("{} failed: {}", r.label, r.signature)));
                    }
                }
            }
            out.extend(self.failed(&key, &r.signature));
        }
        out
    }

    /// After a batch of tool calls: has the item in hand grown past what one
    /// item should hold? Told once per item.
    pub fn after_changes(&self, edited: usize) -> Vec<Reaction> {
        if self.rule(Trigger::ManyChanges, Move::Split).is_none() {
            return Vec::new();
        }
        let (n, changed) = {
            let st = self.state.lock().unwrap();
            let Some(n) = st.current_item else { return Vec::new() };
            let changed = edited.saturating_sub(st.item_base);
            if st.split_told == Some(n) || changed < MANY_FILES {
                return Vec::new();
            }
            (n, changed)
        };
        let (rule, mut out) = self.due(Trigger::ManyChanges, Move::Split, Some(n));
        self.state.lock().unwrap().split_told = Some(n);
        let Some(rule) = rule else { return out };
        let what = format!("{changed} files in one item");
        out.extend(vec![
            Reaction::Tell(format!(
                "This item has changed {changed} files, which is a lot for one item. Split the rest into smaller \
                 items now: call plan with `add`, each item saying what done means for it, and finish the one in \
                 hand first."
            )),
            self.moved(&rule, Some(n)),
            self.signal(&rule, &what, Some(n)),
        ]);
        out
    }

    // ---- WRK-4: the second look ----

    /// What a tool did in the item in hand, in a few words, for the second look.
    pub fn note_step(&self, verb: &str, target: &str, ok: bool) {
        let mut st = self.state.lock().unwrap();
        if st.current_item.is_none() {
            return;
        }
        let what = format!("{verb} {target}{}", if ok { "" } else { " (it failed)" });
        st.trail.push(what.split_whitespace().collect::<Vec<_>>().join(" "));
        if st.trail.len() > TRAIL_LINES {
            st.trail.remove(0);
        }
    }

    /// What the second look is shown besides the patch.
    pub fn evidence(&self) -> Evidence {
        let st = self.state.lock().unwrap();
        Evidence {
            trail: st.trail.clone(),
            checks: st.checks.iter().map(|(l, r)| format!("{l}: {r}")).collect(),
            since_ms: st.item_started_ms,
        }
    }

    /// Items a write just called done that are owed a second look. For each, the
    /// item goes back to in hand and anything the same write started after it goes
    /// back to waiting: nothing counts until the look is over. Returns their
    /// places, and what the look skipped.
    pub fn owe_looks(&self, plan: &mut Plan, before: &[PlanStatus]) -> (Vec<usize>, Vec<Reaction>) {
        if plan.awaiting_approval || self.rule(Trigger::BeforeDone, Move::SecondLook).is_none() {
            return (Vec::new(), Vec::new());
        }
        let writes = plan.changes_since(before);
        let finished: Vec<usize> = writes.iter().filter(|(_, c)| *c == Change::Done).map(|(n, _)| *n).collect();
        let mut owed = Vec::new();
        let mut out = Vec::new();
        for n in finished {
            // The loop's own ticks (`USE-5`) are not the model's claim.
            if plan.items[n].judged.is_some() {
                continue;
            }
            let (rule, skipped) = self.due(Trigger::BeforeDone, Move::SecondLook, Some(n));
            out.extend(skipped);
            if rule.is_none() {
                continue;
            }
            plan.items[n].status = PlanStatus::Doing;
            owed.push(n);
        }
        if let Some(&first) = owed.first() {
            for (m, c) in &writes {
                if *c == Change::Started && *m > first {
                    plan.items[*m].status = PlanStatus::Todo;
                }
            }
        }
        (owed, out)
    }

    /// The look at item `n` begins: the board says so.
    pub fn begin_look(&self, plan: &mut Plan, n: usize) -> Vec<Reaction> {
        plan.items[n].review = Some(Review { state: ReviewState::Checking, reason: None });
        let rule = self.rule(Trigger::BeforeDone, Move::SecondLook);
        let mut out: Vec<Reaction> = rule.as_ref().map(|r| self.moved(r, Some(n))).into_iter().collect();
        out.push(Reaction::Show(AgentEvent::Review {
            run_id: self.run_id.clone(),
            item: n,
            state: "checking".into(),
            reason: None,
        }));
        out
    }

    /// The look is over. `None` means it could not be made or could not be read:
    /// the item stands, because a check that cannot be made must not stop the
    /// work it checks. Returns what to show and what the model is told.
    pub fn looked(&self, plan: &mut Plan, n: usize, look: Option<Look>) -> (Vec<Reaction>, String) {
        let rule = self.rule(Trigger::BeforeDone, Move::SecondLook);
        let signal = |what: &str| rule.as_ref().map(|r| self.signal(r, what, Some(n)));
        let show = |state: &str, reason: Option<String>| {
            Reaction::Show(AgentEvent::Review { run_id: self.run_id.clone(), item: n, state: state.into(), reason })
        };
        let mut out = Vec::new();
        let told;
        match look {
            None => {
                plan.items[n].status = PlanStatus::Done;
                plan.items[n].review = None;
                out.extend(signal("no verdict"));
                told = String::new();
            }
            Some(Look::Checked(reason)) => {
                plan.items[n].status = PlanStatus::Done;
                plan.items[n].review = Some(Review { state: ReviewState::Checked, reason: reason.clone() });
                out.push(show("checked", reason.clone()));
                out.extend(signal("checked"));
                told = format!("Second look on item {}: checked.{}", n + 1, reason.map(|r| format!(" {r}")).unwrap_or_default());
            }
            Some(Look::SentBack(reason)) => {
                let count = {
                    let mut st = self.state.lock().unwrap();
                    let c = st.sendbacks.entry(n).or_insert(0);
                    *c += 1;
                    *c
                };
                plan.items[n].review = Some(Review { state: ReviewState::SentBack, reason: Some(reason.clone()) });
                out.push(show("sent_back", Some(reason.clone())));
                out.extend(signal(&format!("sent back: {reason}")));
                out.extend(self.item_failed(n, &format!("sent back: {reason}")));
                if count > MAX_SENDBACKS {
                    // The look has said no as often as it will be listened to. The
                    // item counts, and the model must not hide why it is doubted.
                    plan.items[n].status = PlanStatus::Done;
                    told = format!(
                        "Second look on item {}: it still disagrees ({reason}). The item counts as done anyway; say \
                         plainly in your answer what is not right.",
                        n + 1
                    );
                } else {
                    plan.items[n].status = PlanStatus::Doing;
                    told = format!(
                        "Second look on item {}: sent back. {reason} The item is still in hand: fix that, run what \
                         proves it, then call it done again.",
                        n + 1
                    );
                }
            }
        }
        (out, told)
    }

    // ---- WRK-6: two ways ----

    /// An item failed: a check stopped the line while it was in hand, or the
    /// second look sent it back. The second time, when there is a way for it and
    /// the run can start helpers, the loop is asked to try two ways.
    fn item_failed(&self, n: usize, what: &str) -> Vec<Reaction> {
        let count = {
            let mut st = self.state.lock().unwrap();
            let c = st.item_fails.entry(n).or_insert(0);
            *c += 1;
            *c
        };
        if count < 2 || !self.can_branch || self.rule(Trigger::ItemFailedTwice, Move::TwoWays).is_none() {
            return Vec::new();
        }
        {
            let st = self.state.lock().unwrap();
            if st.branch_blocked || st.branched.contains(&n) || st.branch_wanted.is_some() {
                return Vec::new();
            }
        }
        let (rule, mut out) = self.due(Trigger::ItemFailedTwice, Move::TwoWays, Some(n));
        let Some(rule) = rule else { return out };
        self.state.lock().unwrap().branch_wanted = Some((n, what.to_string()));
        out.push(self.moved(&rule, Some(n)));
        out.push(self.signal(&rule, what, Some(n)));
        out
    }

    /// The item to try two ways, and what has gone wrong with it. Taken once.
    pub fn take_branch(&self) -> Option<(usize, String)> {
        let mut st = self.state.lock().unwrap();
        let wanted = st.branch_wanted.take()?;
        st.branched.insert(wanted.0);
        Some(wanted)
    }

    /// The two ways were tried and one kept: the item starts afresh, with no
    /// failures against it.
    pub fn branch_done(&self, n: usize) {
        self.state.lock().unwrap().item_fails.remove(&n);
    }

    /// Two ways could not be tried (the folder is too big, no helper could start):
    /// the run goes on with one, and does not try again.
    pub fn branch_failed(&self) {
        self.state.lock().unwrap().branch_blocked = true;
    }

    // ---- WRK-5: a fresh start ----

    /// Is my head full, and is there a way for starting fresh? `tokens` is the
    /// transcript, `turn` the loop's count. Returns whether to start fresh now,
    /// and what a "not this time" did.
    pub fn head_full(&self, tokens: usize, window: Option<usize>, turn: usize) -> (bool, Vec<Reaction>) {
        let Some(window) = window else { return (false, Vec::new()) };
        if self.rule(Trigger::HeadFull, Move::FreshStart).is_none() || tokens * HEAD_FULL.1 <= window * HEAD_FULL.0 {
            return (false, Vec::new());
        }
        {
            let st = self.state.lock().unwrap();
            if st.fresh_blocked || st.fresh_starts >= MAX_FRESH_STARTS || turn < st.fresh_pause_until {
                return (false, Vec::new());
            }
        }
        let (rule, skipped) = self.due(Trigger::HeadFull, Move::FreshStart, None);
        if rule.is_none() {
            self.state.lock().unwrap().fresh_pause_until = turn + FRESH_PAUSE;
            return (false, skipped);
        }
        (true, Vec::new())
    }

    /// The run forgot on purpose, and `note` is what it was handed.
    pub fn fresh_started(&self, note: &str) -> Vec<Reaction> {
        let item = {
            let mut st = self.state.lock().unwrap();
            st.fresh_starts += 1;
            st.current_item
        };
        let mut out = Vec::new();
        if let Some(rule) = self.rule(Trigger::HeadFull, Move::FreshStart) {
            out.push(self.moved(&rule, item));
            out.push(self.signal(&rule, "started fresh", item));
        }
        out.push(Reaction::Show(AgentEvent::FreshStart { run_id: self.run_id.clone(), note: note.to_string() }));
        out
    }

    /// A fresh start could not be made (no note, or the new start is as full as the
    /// old): the run goes on as it was, and does not try again.
    pub fn fresh_failed(&self) {
        self.state.lock().unwrap().fresh_blocked = true;
    }

    /// The model is about to answer. While the line is stopped it is asked, up to
    /// twice, to fix the check or say plainly that it could not.
    pub fn before_answer(&self) -> Option<String> {
        let mut st = self.state.lock().unwrap();
        let stopped = st.stopped.clone()?;
        if st.line_nudges >= MAX_LINE_NUDGES {
            return None;
        }
        st.line_nudges += 1;
        Some(format!(
            "You are about to finish, but `{}` is still failing. Fix it and run it again. If you cannot, say so \
             plainly in your answer: the person must not be left thinking it works.",
            stopped.check
        ))
    }

    /// The loop owes the person a question: only `ask_user` is on the menu until
    /// it is asked.
    pub fn asking(&self) -> bool {
        self.state.lock().unwrap().ask_pending.is_some()
    }

    /// `ask_user` was called, so the debt is paid.
    pub fn asked_now(&self) {
        self.state.lock().unwrap().ask_pending = None;
    }

    /// The model answered in words when it owed a question. Asked once more; then
    /// the menu comes back, because a model that will not ask is not made to.
    pub fn ask_missed(&self) -> Option<String> {
        let mut st = self.state.lock().unwrap();
        let what = st.ask_pending.clone()?;
        if st.ask_misses >= MAX_ASK_MISSES {
            st.ask_pending = None;
            return None;
        }
        st.ask_misses += 1;
        Some(format!(
            "You have hit the same problem twice ({what}) and have not asked yet. Call ask_user now with 2 or 3 \
             concrete ways to go on."
        ))
    }

    /// What the model is reminded of at the top of every request while the run
    /// works under a way that has something to say. Rendered fresh each time and
    /// taken off again, like the plan (`plan_message`), so it never goes stale.
    pub fn note(&self, plan: &Plan) -> Option<String> {
        let mut lines: Vec<String> = Vec::new();
        if self.rounds() {
            match plan.items.iter().position(|i| i.status == PlanStatus::Doing) {
                Some(n) => {
                    let item = &plan.items[n];
                    let done = item
                        .done_when
                        .as_deref()
                        .map(|d| format!(" It is done when: {d}."))
                        .unwrap_or_default();
                    lines.push(format!(
                        "You work item by item. The item in hand is {}: \"{}\".{done} Work on that and nothing else; \
                         when it is done, mark it done and the next one starts by itself. When the items build one \
                         document, it is one artifact: make it with the first part, then add each later part to it with \
                         `update_artifact` and `append`, never a new artifact per part.",
                        n + 1,
                        item.text
                    ));
                }
                None if plan.items.is_empty() => lines.push(
                    "You work item by item. Before your first tool call, write the plan with the `plan` tool, each \
                     item saying what done means for it. A plain question you can answer without tools needs no plan."
                        .to_string(),
                ),
                None => {}
            }
        }
        {
            let st = self.state.lock().unwrap();
            if let Some(s) = &st.stopped {
                lines.push(format!(
                    "Stopped: `{}` is failing. Fix that before anything else; nothing is called done until it passes.",
                    s.check
                ));
            }
            if st.criteria_phase {
                lines.push(CRITERIA_NOTE.to_string());
            } else if st.looking {
                lines.push(LOOKING_NOTE.to_string());
            }
        }
        if !plan.criteria.is_empty() && !plan.awaiting_approval {
            lines.push(
                "The person agreed what done means: it is at the top of your plan. Work to it. At the end each \
                 criterion is checked against your answer, so say plainly which are met and what shows it."
                    .to_string(),
            );
        }
        if self.has_move(Move::Parallel) {
            lines.push(
                "You work as a swarm. Hand each piece of work that does not depend on another to a helper with \
                 `delegate`, and give several tasks in one call so they run side by side. Say which plan item each task \
                 is with `item` (numbered from 1). Finish the pieces that depend on others yourself, or after."
                    .to_string(),
            );
        } else if self.has_move(Move::HandOff) {
            lines.push(
                "Hand each piece of work to the helper that is best at it with `delegate`, and say which plan item \
                 it is with `item` (numbered from 1)."
                    .to_string(),
            );
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    // ---- WRK-10: what done means, looking first, checking in ----

    /// The start of the run. `has_plan`: it carries on a plan that was already
    /// written or agreed, so there is nothing to look around for. `criteria_phase`:
    /// nothing changes until the person has agreed what done means.
    pub fn begin(&self, has_plan: bool, criteria_phase: bool) {
        // A run that cannot write a plan could never say it is clear, so it is not made to look.
        let wants_look = self.can_ask && self.can_plan && self.rule(Trigger::Unclear, Move::LookFirst).is_some();
        let mut st = self.state.lock().unwrap();
        st.criteria_phase = criteria_phase;
        st.looking = wants_look && !has_plan;
    }

    /// Is the run agreeing what done means, with the person's yes still to come?
    pub fn in_criteria_phase(&self) -> bool {
        self.state.lock().unwrap().criteria_phase
    }

    /// Does this way of working agree criteria at the start of a task?
    pub fn wants_criteria(&self) -> bool {
        self.can_ask && self.rule(Trigger::Start, Move::Criteria).is_some()
    }

    /// Is the run looking around and changing nothing?
    pub fn looking(&self) -> bool {
        self.state.lock().unwrap().looking
    }

    /// The plan was written: it was clear enough to write one.
    pub fn plan_written(&self) {
        self.state.lock().unwrap().looking = false;
    }

    /// A question was answered while the run was looking around: it is clear now.
    /// The move is only made, and only shown, when a question really settled it.
    pub fn answered(&self) -> Vec<Reaction> {
        let was = std::mem::replace(&mut self.state.lock().unwrap().looking, false);
        let Some(rule) = self.rule(Trigger::Unclear, Move::LookFirst).filter(|_| was) else { return Vec::new() };
        let item = self.state.lock().unwrap().current_item;
        vec![self.moved(&rule, item), self.signal(&rule, "asked after looking around", item)]
    }

    /// Does the final check have criteria to hold the answer to?
    pub fn criteria_look_due(&self, plan: &Plan) -> bool {
        !plan.criteria.is_empty()
            && !plan.awaiting_approval
            && self.state.lock().unwrap().criteria_looks < MAX_CRITERIA_LOOKS
    }

    /// The criteria were checked against the answer. `found` is one entry per
    /// criterion, or `None` when the check could not be made or read: then nothing
    /// is ticked and the answer stands, because a check that cannot be made must
    /// not stop the work it checks. Returns what to show, and a note for the model
    /// when something is not met and the way of working sends the answer back.
    pub fn criteria_looked(&self, plan: &mut Plan, found: Option<Vec<(bool, Option<String>)>>) -> (Vec<Reaction>, Option<String>) {
        self.state.lock().unwrap().criteria_looks += 1;
        let rule = self.rule(Trigger::BeforeAnswer, Move::SecondLook);
        let mut out = Vec::new();
        let Some(found) = found else {
            out.extend(rule.as_ref().map(|r| self.signal(r, "no verdict on the criteria", None)));
            return (out, None);
        };
        plan.tick_criteria(&found);
        let open: Vec<(String, Option<String>)> =
            plan.criteria_open().iter().map(|c| (c.text.clone(), c.why.clone())).collect();
        let Some(rule) = rule else { return (out, None) };
        if open.is_empty() {
            out.push(self.moved(&rule, None));
            out.push(self.signal(&rule, "all criteria met", None));
            return (out, None);
        }
        let (rule, skipped) = self.due(Trigger::BeforeAnswer, Move::SecondLook, None);
        out.extend(skipped);
        let Some(rule) = rule else { return (out, None) };
        let what = open.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join("; ");
        out.push(self.moved(&rule, None));
        out.push(self.signal(&rule, &format!("not met: {what}"), None));
        {
            let mut st = self.state.lock().unwrap();
            if st.criteria_sendbacks >= MAX_SENDBACKS {
                return (out, None);
            }
            st.criteria_sendbacks += 1;
        }
        let list: String = open
            .iter()
            .map(|(t, why)| match why {
                Some(w) => format!("\n- {t}: {w}"),
                None => format!("\n- {t}"),
            })
            .collect();
        let note = format!(
            "A second look at your answer found that not everything the person agreed to is met yet:{list}\n\
             Fix that now and show what proves it. If you cannot, say plainly in your answer which of these is not met \
             and why."
        );
        (out, Some(note))
    }

    /// Is a check-in due after item `n` was finished? The loop asks the person
    /// itself: the question is the loop's, and so are the options. Only between
    /// items (there has to be a next one), once per item, for a run somebody watches.
    pub fn check_in(&self, plan: &Plan, n: usize) -> (Option<CheckIn>, Vec<Reaction>) {
        if !self.can_ask || plan.items.get(n).map(|i| i.status) != Some(PlanStatus::Done) {
            return (None, Vec::new());
        }
        let Some(next) = plan.items.iter().skip(n + 1).find(|i| matches!(i.status, PlanStatus::Todo | PlanStatus::Doing)) else {
            return (None, Vec::new());
        };
        if self.rule(Trigger::AfterItem, Move::CheckIn).is_none() || !self.state.lock().unwrap().checked_in.insert(n) {
            return (None, Vec::new());
        }
        let (rule, mut out) = self.due(Trigger::AfterItem, Move::CheckIn, Some(n));
        let Some(rule) = rule else { return (None, out) };
        let item = &plan.items[n];
        let checked = matches!(&item.review, Some(Review { state: ReviewState::Checked, .. }));
        out.push(self.moved(&rule, Some(n)));
        out.push(self.signal(&rule, "after an item", Some(n)));
        let question = format!(
            "\"{}\" is done{}. Next: {}.",
            item.text,
            if checked { " and checked" } else { "" },
            next.text
        );
        (Some(CheckIn { question }), out)
    }

    /// What the person said to a check-in, as the model is told it.
    pub fn checked_in(&self, choices: &[String], text: Option<&str>) -> String {
        let text = text.map(str::trim).filter(|t| !t.is_empty());
        if choices.iter().any(|c| c == CHECK_TAKE) {
            self.state.lock().unwrap().taken_over = true;
            return "The person will take it from here. Stop now and do not start the next item. Say in a sentence or two where things stand and what is left.".to_string();
        }
        match (choices.iter().any(|c| c == CHECK_CHANGE), text) {
            (_, Some(t)) => format!(
                "The person said: {t}. Change the next step to fit that (update the plan), then carry on."
            ),
            (true, None) => "The person wants to change the next step but did not say how. Ask them with ask_user what \
                             they want, then update the plan."
                .to_string(),
            (false, None) => "The person said to continue. Go on with the next item.".to_string(),
        }
    }

    /// The person took over: the run is not offered any tool, so its next turn is
    /// the words that say where things stand.
    pub fn taken_over(&self) -> bool {
        self.state.lock().unwrap().taken_over
    }

    // ---- WRK-14: helpers ----

    /// Items were handed to helpers in one `delegate` call. `assigned` is each item
    /// and whether it went to a named agent rather than a general helper. Says what
    /// the way of working did: several at once is "in parallel", a named agent is
    /// "hand it to the one that is best at it".
    pub fn delegated(&self, assigned: &[(usize, bool)]) -> Vec<Reaction> {
        let mut out = Vec::new();
        if assigned.is_empty() {
            return out;
        }
        let first = assigned.first().map(|(n, _)| *n);
        if assigned.len() > 1 {
            if let Some(rule) = self.rule(Trigger::Start, Move::Parallel) {
                out.push(self.moved(&rule, None));
                out.push(self.signal(&rule, &format!("{} items at once", assigned.len()), first));
            }
        }
        if assigned.iter().any(|(_, named)| *named) {
            if let Some(rule) = self.rule(Trigger::Start, Move::HandOff) {
                out.push(self.moved(&rule, first));
                out.push(self.signal(&rule, "handed to an agent app", first));
            }
        }
        out
    }

    // ---- WRK-11: advice before a risky act ----

    /// Is advice due before this act (`act` says what it is in a few words)? True
    /// when the way of working has a rule for it and the person did not say "not
    /// this time". The move is shown and written down here.
    pub fn advice(&self, act: &str) -> (bool, Vec<Reaction>) {
        let item = self.state.lock().unwrap().current_item;
        let (rule, mut out) = self.due(Trigger::RiskyAction, Move::Advice, item);
        let Some(rule) = rule else { return (false, out) };
        out.push(self.moved(&rule, item));
        out.push(self.signal(&rule, act, item));
        (true, out)
    }

    // ---- WRK-13: switching how the run works ----

    /// The person asked for another preset. It applies when no item is in hand.
    pub fn request_switch(&self, slug: &str) {
        self.state.lock().unwrap().pending_switch = Some(slug.to_string());
    }

    /// A preset has been asked for, whether or not an item is in hand.
    pub fn switch_pending(&self) -> Option<String> {
        self.state.lock().unwrap().pending_switch.clone()
    }

    /// The asked-for preset is gone (deleted since): the run goes on as it was.
    pub fn drop_switch(&self) {
        self.state.lock().unwrap().pending_switch = None;
    }

    /// A requested preset that can start now: nothing is in hand.
    pub fn switch_ready(&self) -> Option<String> {
        let st = self.state.lock().unwrap();
        if st.current_item.is_some() {
            return None;
        }
        st.pending_switch.clone()
    }

    /// Work another way from here. `rules` are what the new preset holds, with the
    /// learned ones in scope. Shows the change, says it once in one plain line,
    /// and writes it down.
    pub fn switch_to(&self, slug: &str, name: &str, rules: Vec<Rule>) -> Vec<Reaction> {
        let from = {
            let mut live = self.live.write().unwrap();
            let from = live.preset.clone();
            live.preset = slug.to_string();
            live.name = name.to_string();
            live.source = "switch";
            live.rules = rules;
            from
        };
        {
            let mut st = self.state.lock().unwrap();
            st.pending_switch = None;
            // The new way starts its own count of what it tried.
            st.split_told = None;
        }
        vec![
            Reaction::Show(self.style_event()),
            Reaction::Show(AgentEvent::HarnessCommand {
                run_id: self.run_id.clone(),
                name: "style".into(),
                outcome: "done".into(),
                note: Some(format!("I'm switching to {name} from here.")),
            }),
            Reaction::Record(Signal {
                rule: STYLE_RULE.into(),
                trigger: "switch".into(),
                mv: slug.to_string(),
                what: format!("from {from}"),
                item: None,
            }),
        ]
    }

    /// Has the run grown past what a quick task is? Told once, to a run working Ad
    /// hoc that can offer the person a switch: the model offers it, the person decides.
    pub fn bigger(&self, edited: usize, can_offer: bool) -> Option<String> {
        if !self.can_ask || !can_offer || !self.is_ad_hoc() || edited < BIGGER_FILES {
            return None;
        }
        let mut st = self.state.lock().unwrap();
        if st.bigger_told || st.switch_offered {
            return None;
        }
        st.bigger_told = true;
        Some(format!(
            "This has grown: {edited} files changed so far, more than a quick task. If a different way of working \
             would help the person, offer it once with the harness tool: name switch_mode, mode mission-command, and \
             a reason that begins \"This is bigger than it looked:\". Do not switch it yourself."
        ))
    }

    /// A switch is being offered. Once per run: a second offer in one run is nagging.
    pub fn offer_switch(&self) -> bool {
        let mut st = self.state.lock().unwrap();
        if st.switch_offered {
            return false;
        }
        st.switch_offered = true;
        true
    }
}

/// `WRK-10`: what a check-in asks, for the loop to put to the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckIn {
    pub question: String,
}

pub const CHECK_CONTINUE: &str = "Continue";
pub const CHECK_CHANGE: &str = "Change the next step";
pub const CHECK_TAKE: &str = "I'll take it from here";

/// Files changed before a run working Ad hoc is told it has grown.
pub const BIGGER_FILES: usize = 9;
/// How often the criteria are checked against an answer in one run.
const MAX_CRITERIA_LOOKS: u32 = 3;

const CRITERIA_NOTE: &str = "Before anything changes, agree with the person what done means. You may read and look \
around, but change nothing yet. Write the plan with the `plan` tool: `criteria` (two to four statements, each one \
something that can be seen to be true or false when the work is over) and `items` (the steps). Then stop and say in one \
sentence that you are waiting for their yes. If it is unclear what they want, look around and ask with ask_user before \
you write the criteria. A plain question you can answer without tools needs no criteria: just answer it.";

const LOOKING_NOTE: &str = "It may be unclear what the person wants. Look around first with tools that only read, and \
change nothing. If it is still unclear, call ask_user with what you found and two or three options. If it is clear, \
write the plan.";

/// `WRK-1`: the signal as the session log keeps it.
pub const WAY_KIND: &str = "way";
/// The `rule` of a signal that says which preset a run worked under (its start, or a
/// switch). Its `move` is the preset's slug. Read back by `Db::ended_conversations_since`.
pub const STYLE_RULE: &str = "style";

/// Write a fired way of working down. Best effort, like every other write to
/// the log: a rule that fired but failed to log still fired.
pub fn record(db: &Db, conversation_id: &str, run_id: &str, signal: &Signal) {
    if let Ok(payload) = serde_json::to_value(signal) {
        let _ = db.append_session_event(conversation_id, Some(run_id), WAY_KIND, &payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan::execute_with;

    fn rules_of(preset: &str) -> Vec<Rule> {
        let dir = std::env::temp_dir().join(format!("poiesis_work_{}", uuid::Uuid::new_v4().simple()));
        let store = MemoryStore::new(&dir).unwrap();
        let rules = store.rules_for(preset, &ScopeCtx::default());
        let _ = std::fs::remove_dir_all(dir);
        rules
    }

    fn work(preset: &str, can_ask: bool) -> Work {
        Work::new("r1", preset, preset, "chat", rules_of(preset), can_ask)
    }

    fn failed_check(label: &str, why: &str) -> CheckResult {
        CheckResult { label: label.into(), passed: false, signature: why.into() }
    }

    fn passed_check(label: &str) -> CheckResult {
        CheckResult { label: label.into(), passed: true, signature: "passed".into() }
    }

    fn tells(r: &[Reaction]) -> Vec<&str> {
        r.iter().filter_map(|r| if let Reaction::Tell(t) = r { Some(t.as_str()) } else { None }).collect()
    }

    fn shows(r: &[Reaction]) -> Vec<&AgentEvent> {
        r.iter().filter_map(|r| if let Reaction::Show(e) = r { Some(e) } else { None }).collect()
    }

    fn plan_of(items: &[&str]) -> std::sync::Mutex<Plan> {
        let m = std::sync::Mutex::new(Plan::default());
        let items: Vec<serde_json::Value> =
            items.iter().map(|t| serde_json::json!({ "text": t, "done_when": format!("{t} works") })).collect();
        execute_with(&m, &serde_json::json!({ "items": items }), &Discipline::default()).unwrap();
        m
    }

    #[test]
    fn ad_hoc_has_only_the_base_rules_and_lean_adds_working_item_by_item() {
        assert!(!work(AD_HOC, true).rounds());
        assert!(work(AD_HOC, true).rule(Trigger::CheckFails, Move::StopTheLine).is_some());
        assert!(work("lean", true).rounds());
        assert!(work("lean", true).rule(Trigger::ManyChanges, Move::Split).is_some());
    }

    #[test]
    fn a_run_with_no_plan_tool_does_not_work_item_by_item_and_is_not_told_to() {
        let mut w = work("lean", true);
        w.can_plan = false;
        assert!(!w.rounds());
        assert!(!w.discipline().one_at_a_time);
        assert!(w.note(&Plan::default()).is_none(), "no instruction to write a plan it cannot write");
        // Items never start by themselves either.
        let plan = plan_of(&["one"]);
        let mut p = plan.lock().unwrap();
        w.after_plan_write(&mut p, &[], 0, true);
        assert_eq!(p.items[0].status, PlanStatus::Todo);
    }

    #[test]
    fn a_failing_check_stops_the_line_and_the_same_check_passing_starts_it_again() {
        let w = work(AD_HOC, true);
        let r = w.after_checks(vec![failed_check("cargo test", "2 failed")]);
        assert!(matches!(shows(&r)[0], AgentEvent::LineStopped { check, .. } if check == "cargo test"));
        assert!(tells(&r)[0].starts_with("Stopped: `cargo test` is failing"));
        assert!(r.iter().any(|r| matches!(r, Reaction::Record(s) if s.mv == "stop-the-line" && s.what == "cargo test")));
        assert_eq!(w.discipline().stopped_by.as_deref(), Some("cargo test"));

        // The same check failing again is the same line, not a second stop.
        let again = w.after_checks(vec![failed_check("cargo test", "1 failed")]);
        assert!(shows(&again).iter().all(|e| !matches!(e, AgentEvent::LineStopped { .. })));

        // A different check passing does not clear it.
        assert!(shows(&w.after_checks(vec![passed_check("npm run lint")])).is_empty());
        assert!(w.discipline().stopped_by.is_some());

        let cleared = w.after_checks(vec![passed_check("cargo test")]);
        assert!(matches!(shows(&cleared)[0], AgentEvent::LineCleared { .. }));
        assert_eq!(w.discipline().stopped_by, None);
    }

    #[test]
    fn with_two_checks_failing_the_line_runs_only_when_both_pass() {
        let w = work(AD_HOC, true);
        w.after_checks(vec![failed_check("cargo test", "2 failed")]);
        w.after_checks(vec![failed_check("npm test", "1 failed")]);
        assert_eq!(w.discipline().stopped_by.as_deref(), Some("npm test"));

        // The one that stopped it passes; the other is still failing, so it stays stopped.
        let r = w.after_checks(vec![passed_check("npm test")]);
        assert!(matches!(shows(&r)[0], AgentEvent::LineStopped { check, .. } if check == "cargo test"), "{r:?}");
        assert!(tells(&r)[0].contains("`cargo test` is still failing"));
        assert_eq!(w.discipline().stopped_by.as_deref(), Some("cargo test"));

        let r = w.after_checks(vec![passed_check("cargo test")]);
        assert!(matches!(shows(&r)[0], AgentEvent::LineCleared { .. }));
        assert_eq!(w.discipline().stopped_by, None);
    }

    #[test]
    fn while_the_line_is_stopped_nothing_is_called_done_and_nothing_else_is_started() {
        let w = work("lean", true);
        let plan = plan_of(&["export", "button"]);
        w.after_checks(vec![failed_check("cargo test", "2 failed")]);
        let rules = w.discipline();

        let err = execute_with(&plan, &serde_json::json!({ "update": { "index": 1, "status": "done" } }), &rules)
            .unwrap_err();
        assert!(err.starts_with("Stopped: `cargo test` is failing"), "{err}");
        assert_eq!(plan.lock().unwrap().items[0].status, PlanStatus::Todo, "and nothing moved");

        // Dropping with a reason is honest and stays possible.
        assert!(execute_with(
            &plan,
            &serde_json::json!({ "update": { "index": 2, "status": "dropped", "why": "not needed" } }),
            &rules
        )
        .is_ok());

        w.after_checks(vec![passed_check("cargo test")]);
        assert!(execute_with(
            &plan,
            &serde_json::json!({ "update": { "index": 1, "status": "done" } }),
            &w.discipline()
        )
        .is_ok());
    }

    #[test]
    fn one_at_a_time_only_the_first_open_item_can_be_started_or_finished() {
        let w = work("lean", true);
        let plan = plan_of(&["one", "two", "three"]);
        let rules = w.discipline();
        assert!(rules.one_at_a_time);

        let err = execute_with(&plan, &serde_json::json!({ "update": { "index": 2, "status": "doing" } }), &rules)
            .unwrap_err();
        assert!(err.starts_with("Item 1 comes first"), "{err}");
        let err = execute_with(&plan, &serde_json::json!({ "update": { "index": 3, "status": "done" } }), &rules)
            .unwrap_err();
        assert!(err.starts_with("Item 1 comes first"), "{err}");

        execute_with(&plan, &serde_json::json!({ "update": { "index": 1, "status": "doing" } }), &rules).unwrap();
        // Item 1 is in hand, so item 2 has to wait for it.
        assert!(execute_with(&plan, &serde_json::json!({ "update": { "index": 2, "status": "doing" } }), &rules).is_err());
        // Finishing one and starting the next in one call is the normal move.
        execute_with(
            &plan,
            &serde_json::json!({ "updates": [{ "index": 1, "status": "done" }, { "index": 2, "status": "doing" }] }),
            &rules,
        )
        .unwrap();
        let p = plan.lock().unwrap();
        assert_eq!((p.items[0].status, p.items[1].status, p.items[2].status), (PlanStatus::Done, PlanStatus::Doing, PlanStatus::Todo));
    }

    #[test]
    fn ad_hoc_plans_are_held_to_nothing_but_a_stopped_line() {
        let w = work(AD_HOC, true);
        let plan = plan_of(&["one", "two"]);
        assert!(execute_with(&plan, &serde_json::json!({ "update": { "index": 2, "status": "done" } }), &w.discipline())
            .is_ok(), "any order, as today");
        let lax = w.discipline_with(Some("cargo test".into()));
        assert_eq!(lax.unchecked, None, "Ad hoc is not held to the project's check");
    }

    /// `WRK-22`: changed files are done only once the project's check ran after them.
    /// Starting an item, or dropping one, is never held back by it.
    #[test]
    fn an_item_with_unchecked_changes_is_not_done_until_the_check_ran() {
        let w = work("lean", true);
        let plan = plan_of(&["export", "button"]);
        let rules = w.discipline_with(Some("cargo test".into()));
        assert!(execute_with(&plan, &serde_json::json!({ "update": { "index": 1, "status": "doing" } }), &rules).is_ok());
        let err = execute_with(&plan, &serde_json::json!({ "update": { "index": 1, "status": "done" } }), &rules)
            .unwrap_err();
        assert!(err.contains("`cargo test` has not run since") && err.contains("item 1"), "{err}");
        assert!(execute_with(
            &plan,
            &serde_json::json!({ "update": { "index": 1, "status": "dropped", "why": "not needed" } }),
            &rules
        )
        .is_ok());

        // The check ran after the change: nothing unchecked is passed in.
        let plan = plan_of(&["export"]);
        assert!(execute_with(&plan, &serde_json::json!({ "update": { "index": 1, "status": "done" } }), &w.discipline_with(None))
            .is_ok());
    }

    #[test]
    fn the_loop_starts_the_next_item_itself_and_says_where_each_one_began_and_ended() {
        let w = work("lean", true);
        let plan = plan_of(&["one", "two"]);
        let mut p = plan.lock().unwrap();
        let before: Vec<PlanStatus> = Vec::new();
        let events = w.after_plan_write(&mut p, &before, 0, true);
        assert_eq!(p.items[0].status, PlanStatus::Doing, "the first item is in hand without the model saying so");
        assert_eq!(p.style.as_deref(), Some("lean"));
        assert!(matches!(&events[..], [AgentEvent::ItemStarted { item: 0, done_when: Some(d), .. }] if d == "one works"));

        // The model finishes item 1; the loop starts item 2.
        let before = p.statuses();
        p.items[0].status = PlanStatus::Done;
        let events = w.after_plan_write(&mut p, &before, 0, true);
        assert_eq!(p.items[1].status, PlanStatus::Doing);
        assert!(matches!(events[0], AgentEvent::ItemEnded { item: 0, ref outcome, .. } if outcome == "done"));
        assert!(matches!(events[1], AgentEvent::ItemStarted { item: 1, .. }));
    }

    #[test]
    fn a_plan_that_waits_for_approval_does_not_start_anything() {
        let w = work("lean", true);
        let plan = plan_of(&["one"]);
        let mut p = plan.lock().unwrap();
        let events = w.after_plan_write(&mut p, &[], 0, false);
        assert_eq!(p.items[0].status, PlanStatus::Todo);
        assert!(events.is_empty());
    }

    #[test]
    fn ad_hoc_runs_draw_no_item_boundaries() {
        let w = work(AD_HOC, true);
        let plan = plan_of(&["one", "two"]);
        let mut p = plan.lock().unwrap();
        assert!(w.after_plan_write(&mut p, &[], 0, true).is_empty());
        assert_eq!(p.items[0].status, PlanStatus::Todo);
        assert_eq!(p.style, None, "Ad hoc is no style to show");
    }

    #[test]
    fn the_same_problem_twice_owes_the_person_a_question() {
        let w = work(AD_HOC, true);
        assert!(w.failed("read_file", "no such file: /a/b.txt (line 3)").is_empty(), "once is a mistake");
        let r = w.failed("read_file", "No such file: /a/b.txt (line 9)");
        assert!(tells(&r)[0].contains("Call ask_user now"));
        assert!(r.iter().any(|r| matches!(r, Reaction::Show(AgentEvent::Move { mv, .. }) if mv == "check-in")));
        assert!(r.iter().any(|r| matches!(r, Reaction::Record(s) if s.trigger == "same-problem-twice")));
        assert!(w.asking());

        // Asked about once: a third failure does not ask again.
        w.asked_now();
        assert!(!w.asking());
        assert!(w.failed("read_file", "no such file: /a/b.txt (line 1)").is_empty());
    }

    #[test]
    fn two_different_problems_are_not_the_same_problem_and_a_success_in_between_resets() {
        let w = work(AD_HOC, true);
        w.failed("read_file", "no such file: a.txt");
        assert!(w.failed("read_file", "permission denied: a.txt").is_empty());
        w.succeeded("read_file");
        assert!(w.failed("read_file", "permission denied: a.txt").is_empty(), "the count started over");
        assert!(!w.asking());
    }

    #[test]
    fn a_check_failing_the_same_way_twice_is_a_problem_to_ask_about_as_well_as_a_stopped_line() {
        let w = work(AD_HOC, true);
        w.after_checks(vec![failed_check("cargo test", "assertion failed: x == 2")]);
        let r = w.after_checks(vec![failed_check("cargo test", "assertion failed: x == 2")]);
        assert!(tells(&r)[0].contains("same problem twice"));
        assert!(w.asking());
    }

    #[test]
    fn a_run_nobody_can_ask_is_told_to_change_course_and_is_never_left_owing_a_question() {
        let w = work(AD_HOC, false);
        w.failed("run_code", "boom");
        let r = w.failed("run_code", "boom");
        assert!(tells(&r)[0].contains("Do not try the same thing a third time"));
        assert!(!w.asking());
    }

    #[test]
    fn a_model_that_will_not_ask_is_asked_once_more_and_then_let_go() {
        let w = work(AD_HOC, true);
        w.failed("x", "boom");
        w.failed("x", "boom");
        assert!(w.ask_missed().unwrap().contains("have not asked yet"));
        assert!(w.asking());
        assert!(w.ask_missed().is_none());
        assert!(!w.asking(), "the menu comes back");
    }

    #[test]
    fn an_answer_over_a_failing_check_is_asked_about_twice_and_then_allowed() {
        let w = work(AD_HOC, true);
        assert!(w.before_answer().is_none(), "nothing stopped");
        w.after_checks(vec![failed_check("cargo test", "1 failed")]);
        assert!(w.before_answer().unwrap().contains("`cargo test` is still failing"));
        assert!(w.before_answer().is_some());
        assert!(w.before_answer().is_none(), "never a gate: the answer goes out and says what it must");
    }

    #[test]
    fn many_files_in_one_item_say_split_once_for_that_item() {
        let w = work("lean", true);
        let plan = plan_of(&["one", "two"]);
        let mut p = plan.lock().unwrap();
        w.after_plan_write(&mut p, &[], 2, true); // item 1 starts with 2 files already changed
        assert!(w.after_changes(2 + MANY_FILES - 1).is_empty());
        let r = w.after_changes(2 + MANY_FILES);
        assert!(tells(&r)[0].contains("Split the rest"));
        assert!(r.iter().any(|r| matches!(r, Reaction::Show(AgentEvent::Move { mv, item: Some(0), .. }) if mv == "split")));
        assert!(w.after_changes(2 + MANY_FILES + 3).is_empty(), "told once");

        // The next item counts from where it began.
        let before = p.statuses();
        p.items[0].status = PlanStatus::Done;
        w.after_plan_write(&mut p, &before, 20, true);
        assert!(w.after_changes(21).is_empty());
        assert!(!w.after_changes(20 + MANY_FILES).is_empty());
    }

    #[test]
    fn ad_hoc_never_says_split() {
        let w = work(AD_HOC, true);
        assert!(w.after_changes(50).is_empty());
    }

    #[test]
    fn the_note_tells_the_model_the_item_in_hand_the_stopped_line_or_to_write_the_plan_first() {
        let w = work("lean", true);
        let empty = Plan::default();
        assert!(w.note(&empty).unwrap().contains("Before your first tool call, write the plan"));

        let plan = plan_of(&["export", "button"]);
        let mut p = plan.lock().unwrap();
        w.after_plan_write(&mut p, &[], 0, true);
        let note = w.note(&p).unwrap();
        assert!(note.contains("The item in hand is 1: \"export\""), "{note}");
        assert!(note.contains("done when: export works"), "{note}");

        w.after_checks(vec![failed_check("cargo test", "1 failed")]);
        assert!(w.note(&p).unwrap().contains("Stopped: `cargo test` is failing"));

        // Ad hoc has nothing to add until something stops.
        let adhoc = work(AD_HOC, true);
        assert!(adhoc.note(&p).is_none());
    }

    fn moves(rs: &[Reaction]) -> Vec<String> {
        rs.iter()
            .filter_map(|r| match r {
                Reaction::Show(AgentEvent::Move { mv, state, .. }) => Some(format!("{mv}:{state}")),
                _ => None,
            })
            .collect()
    }

    /// Write a plan the way the dispatch does, then ask what is owed a look.
    fn finish(w: &Work, plan: &std::sync::Mutex<Plan>, updates: serde_json::Value) -> (Vec<usize>, Vec<Reaction>) {
        let before = plan.lock().unwrap().statuses();
        execute_with(plan, &serde_json::json!({ "updates": updates }), &w.discipline()).unwrap();
        let mut p = plan.lock().unwrap();
        w.owe_looks(&mut p, &before)
    }

    #[test]
    fn an_item_called_done_goes_back_in_hand_until_it_has_had_its_second_look() {
        let w = work("lean", true);
        let plan = plan_of(&["one", "two"]);
        {
            let mut p = plan.lock().unwrap();
            w.after_plan_write(&mut p, &[], 0, true);
        }
        let (owed, _) = finish(&w, &plan, serde_json::json!([{ "index": 1, "status": "done" }, { "index": 2, "status": "doing" }]));
        assert_eq!(owed, vec![0]);
        let mut p = plan.lock().unwrap();
        assert_eq!(p.items[0].status, PlanStatus::Doing, "not done until the look says so");
        assert_eq!(p.items[1].status, PlanStatus::Todo, "what the same write started waits too");

        let begun = w.begin_look(&mut p, 0);
        assert_eq!(p.items[0].review.as_ref().unwrap().state, ReviewState::Checking);
        assert_eq!(moves(&begun), vec!["second-look:fired"]);

        let (_, told) = w.looked(&mut p, 0, Some(Look::Checked(Some("It works.".into()))));
        assert_eq!(p.items[0].status, PlanStatus::Done);
        assert_eq!(p.items[0].review.as_ref().unwrap().state, ReviewState::Checked);
        assert_eq!(told, "Second look on item 1: checked. It works.");
    }

    #[test]
    fn a_send_back_keeps_the_item_in_hand_with_its_reason_and_is_let_go_after_two() {
        let w = work("lean", true);
        let plan = plan_of(&["one"]);
        {
            let mut p = plan.lock().unwrap();
            w.after_plan_write(&mut p, &[], 0, true);
        }
        for round in 1..=3u32 {
            let (owed, _) = finish(&w, &plan, serde_json::json!([{ "index": 1, "status": "done" }]));
            assert_eq!(owed, vec![0], "round {round}");
            let mut p = plan.lock().unwrap();
            w.begin_look(&mut p, 0);
            let (rs, told) = w.looked(&mut p, 0, Some(Look::SentBack("no test".into())));
            let review = p.items[0].review.clone().unwrap();
            assert_eq!((review.state, review.reason.as_deref()), (ReviewState::SentBack, Some("no test")));
            assert!(rs.iter().any(|r| matches!(r, Reaction::Record(s) if s.what == "sent back: no test")));
            if round <= MAX_SENDBACKS {
                assert_eq!(p.items[0].status, PlanStatus::Doing);
                assert!(told.contains("sent back. no test") && told.contains("still in hand"), "{told}");
            } else {
                assert_eq!(p.items[0].status, PlanStatus::Done, "a look that keeps saying no is not a loop");
                assert!(told.contains("still disagrees") && told.contains("say plainly"), "{told}");
            }
        }
    }

    #[test]
    fn a_look_that_could_not_be_made_lets_the_item_stand_and_says_nothing() {
        let w = work("lean", true);
        let plan = plan_of(&["one"]);
        {
            let mut p = plan.lock().unwrap();
            w.after_plan_write(&mut p, &[], 0, true);
        }
        finish(&w, &plan, serde_json::json!([{ "index": 1, "status": "done" }]));
        let mut p = plan.lock().unwrap();
        let (_, told) = w.looked(&mut p, 0, None);
        assert_eq!(p.items[0].status, PlanStatus::Done);
        assert!(p.items[0].review.is_none());
        assert!(told.is_empty());
    }

    #[test]
    fn no_second_look_without_a_way_for_it_or_while_the_plan_waits_for_approval() {
        let adhoc = work(AD_HOC, true);
        let plan = plan_of(&["one"]);
        {
            let mut p = plan.lock().unwrap();
            let before = p.statuses();
            p.items[0].status = PlanStatus::Done;
            assert!(adhoc.owe_looks(&mut p, &before).0.is_empty());
        }
        let lean = work("lean", true);
        let mut p = plan.lock().unwrap();
        p.items[0].status = PlanStatus::Doing;
        let before = p.statuses();
        p.items[0].status = PlanStatus::Done;
        p.awaiting_approval = true;
        assert!(lean.owe_looks(&mut p, &before).0.is_empty());
    }

    #[test]
    fn not_this_time_skips_a_look_once_and_says_so_and_it_looks_again_after() {
        let w = work("lean", true);
        assert!(!w.skip_next("no-such-rule"));
        assert!(w.skip_next("lean:before-done-second-look"));
        let plan = plan_of(&["one", "two"]);
        {
            let mut p = plan.lock().unwrap();
            w.after_plan_write(&mut p, &[], 0, true);
        }
        let (owed, skipped) = finish(&w, &plan, serde_json::json!([{ "index": 1, "status": "done" }]));
        assert!(owed.is_empty(), "skipped");
        assert_eq!(moves(&skipped), vec!["second-look:skipped"]);
        assert!(skipped.iter().any(|r| matches!(r, Reaction::Record(s) if s.what == "not this time")), "the click is a signal");
        assert_eq!(plan.lock().unwrap().items[0].status, PlanStatus::Done);
        {
            let mut p = plan.lock().unwrap();
            w.after_plan_write(&mut p, &[PlanStatus::Doing, PlanStatus::Todo], 0, true);
        }
        let (owed, _) = finish(&w, &plan, serde_json::json!([{ "index": 2, "status": "done" }]));
        assert_eq!(owed, vec![1], "it was once, not for good");
    }

    #[test]
    fn the_trail_and_the_checks_are_what_the_look_is_shown_and_an_item_starts_a_new_trail() {
        let w = work("lean", true);
        let plan = plan_of(&["one", "two"]);
        {
            let mut p = plan.lock().unwrap();
            w.after_plan_write(&mut p, &[], 0, true);
        }
        w.note_step("read", "src/a.rs", true);
        w.note_step("ran", "cargo test", false);
        w.after_checks(vec![failed_check("cargo test", "assertion failed")]);
        let e = w.evidence();
        assert_eq!(e.trail, vec!["read src/a.rs", "ran cargo test (it failed)"]);
        assert_eq!(e.checks, vec!["cargo test: failing (assertion failed)"]);
        assert!(e.since_ms > 0);
        {
            let mut p = plan.lock().unwrap();
            let before = p.statuses();
            p.items[0].status = PlanStatus::Done;
            p.items[1].status = PlanStatus::Doing;
            w.after_plan_write(&mut p, &before, 0, true);
        }
        assert!(w.evidence().trail.is_empty(), "item two starts with a clean trail");
    }

    #[test]
    fn a_fresh_start_needs_a_way_for_it_a_window_and_a_head_past_two_thirds() {
        assert!(!work(AD_HOC, true).head_full(900, Some(1000), 1).0, "ad hoc has no way for it");
        let w = work("lean", true);
        assert!(!w.head_full(900, None, 1).0, "an unknown window is never full");
        assert!(!w.head_full(600, Some(1000), 1).0);
        assert!(w.head_full(700, Some(1000), 1).0);
        w.fresh_failed();
        assert!(!w.head_full(900, Some(1000), 2).0, "it does not try again once it could not");
    }

    #[test]
    fn a_run_forgets_on_purpose_at_most_three_times() {
        let w = work("lean", true);
        for _ in 0..MAX_FRESH_STARTS {
            assert!(w.head_full(900, Some(1000), 1).0);
            let rs = w.fresh_started("note");
            assert!(rs.iter().any(|r| matches!(r, Reaction::Show(AgentEvent::FreshStart { .. }))));
            assert!(rs.iter().any(|r| matches!(r, Reaction::Record(s) if s.mv == "fresh-start")));
        }
        assert!(!w.head_full(900, Some(1000), 9).0);
    }

    #[test]
    fn not_this_time_holds_a_fresh_start_off_for_a_few_turns() {
        let w = work("lean", true);
        assert!(w.skip_next("lean:head-full-fresh-start"));
        let (due, rs) = w.head_full(900, Some(1000), 3);
        assert!(!due);
        assert_eq!(moves(&rs), vec!["fresh-start:skipped"]);
        assert!(!w.head_full(900, Some(1000), 4).0, "held off, not asked about every turn");
        assert!(w.head_full(900, Some(1000), 3 + FRESH_PAUSE).0, "and then it is due again");
    }

    #[test]
    fn failures_that_read_alike_normalise_alike() {
        assert_eq!(normalize("Error at line 12, col 3"), normalize("error at LINE 7, col 45"));
        assert_ne!(normalize("no such file"), normalize("permission denied"));
    }

    #[test]
    fn a_signal_is_written_as_a_way_row_and_reads_back() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("t", None, false).unwrap();
        let s = Signal {
            rule: "stop-the-line".into(),
            trigger: "check-fails".into(),
            mv: "stop-the-line".into(),
            what: "cargo test".into(),
            item: Some(1),
        };
        record(&db, &c.id, "run1", &s);
        let rows = db.session_events(&c.id).unwrap();
        let row = rows.iter().find(|r| r.kind == WAY_KIND).expect("the row is there");
        let back: Signal = serde_json::from_str(&row.payload_json).unwrap();
        assert_eq!(back, s);
        assert!(row.payload_json.contains("\"move\""), "the wire name is `move`");
    }

    // ---- WRK-17, WRK-18: where a chat starts, and helpers and tasks ----

    fn resolved(db: &Db, mem: &MemoryStore, conv: &str, watched: bool) -> Work {
        Work::resolve(db, mem, conv, "r1", None, None, watched)
    }

    fn memory() -> (MemoryStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("poiesis_work_{}", uuid::Uuid::new_v4().simple()));
        let mem = MemoryStore::new(&dir).unwrap();
        mem.ensure_ways();
        (mem, dir)
    }

    #[test]
    fn a_chat_nobody_chose_for_starts_where_its_place_says_and_its_own_choice_wins() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let c = db.create_conversation("t", None, false).unwrap();
        assert!(resolved(&db, &mem, &c.id, true).is_ad_hoc(), "nothing set: Ad hoc");
        mem.set_default("global", Some("lean")).unwrap();
        assert_eq!(resolved(&db, &mem, &c.id, true).preset(), "lean");
        db.set_conversation_style(&c.id, Some("swarm")).unwrap();
        assert_eq!(resolved(&db, &mem, &c.id, true).preset(), "swarm", "the chat's own choice wins");
        db.set_conversation_style(&c.id, Some(AD_HOC)).unwrap();
        assert!(resolved(&db, &mem, &c.id, true).is_ad_hoc(), "Ad hoc chosen for the chat beats the default too");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_default_names_a_preset_that_is_gone_and_the_run_still_runs_ad_hoc() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let c = db.create_conversation("t", None, false).unwrap();
        let own = mem.create_preset("Mine", "", None, &[], true).unwrap();
        mem.set_default("global", Some(&own.slug)).unwrap();
        mem.delete_preset(&own.slug).unwrap();
        assert!(resolved(&db, &mem, &c.id, true).is_ad_hoc());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_helper_and_an_unattended_task_work_in_the_preset_their_conversation_has_but_cannot_ask() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        mem.set_default("global", Some("mission-command")).unwrap();
        let c = db.create_conversation("helper", None, false).unwrap();
        // No preset on its conversation and nobody watching: base rules, whatever the defaults say.
        assert!(resolved(&db, &mem, &c.id, false).is_ad_hoc());
        db.set_conversation_style(&c.id, Some("lean")).unwrap();
        let helper = resolved(&db, &mem, &c.id, false);
        assert_eq!(helper.preset(), "lean");
        assert!(helper.rule(Trigger::BeforeDone, Move::SecondLook).is_some(), "the quiet moves hold");
        assert!(!helper.wants_criteria(), "and nothing waits for a person");
        db.set_conversation_style(&c.id, Some("command-and-control")).unwrap();
        assert!(!resolved(&db, &mem, &c.id, false).wants_criteria(), "agreeing what done means needs a person");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_way_narrowed_to_a_kind_of_work_holds_only_in_that_kind() {
        let (mem, dir) = memory();
        mem.add_way(Trigger::BeforeAnswer, Move::SecondLook, crate::memory::ways::Scope::Kind("mail".into())).unwrap();
        let in_mail = mem.rules_for(AD_HOC, &ScopeCtx { kind: Some("mail"), ..Default::default() });
        let elsewhere = mem.rules_for(AD_HOC, &ScopeCtx::default());
        assert!(in_mail.iter().any(|r| r.mv == Move::SecondLook));
        assert!(elsewhere.iter().all(|r| r.mv != Move::SecondLook));
        let _ = std::fs::remove_dir_all(dir);
    }

    // ---- WRK-10 .. WRK-13 ----

    fn criteria_plan(texts: &[&str]) -> Plan {
        let mut p = Plan::default();
        p.set_criteria(texts.iter().map(|t| t.to_string()).collect());
        p
    }

    #[test]
    fn a_run_that_may_look_first_changes_nothing_until_it_has_a_plan_or_an_answer() {
        let w = work("mission-command", true);
        w.begin(false, false);
        assert!(w.looking());
        w.plan_written();
        assert!(!w.looking(), "writing a plan says it was clear enough");

        let w = work("mission-command", true);
        w.begin(false, false);
        let r = w.answered();
        assert!(!w.looking());
        assert!(matches!(shows(&r)[0], AgentEvent::Move { mv, .. } if mv == "look-first"));
        assert!(w.answered().is_empty(), "the move is made once, by the question that settled it");

        // A plan that carries on, a run nobody can ask, a run that cannot plan, or Ad hoc: nothing to look for.
        let w = work("mission-command", true);
        w.begin(true, false);
        assert!(!w.looking());
        let w = work("mission-command", false);
        w.begin(false, false);
        assert!(!w.looking());
        let mut w = work("mission-command", true);
        w.can_plan = false;
        w.begin(false, false);
        assert!(!w.looking());
        let w = work(AD_HOC, true);
        w.begin(false, false);
        assert!(!w.looking());
    }

    #[test]
    fn the_criteria_phase_is_said_to_the_model_and_the_agreed_criteria_are_reminded_after() {
        let w = work("mission-command", true);
        w.begin(false, true);
        assert!(w.in_criteria_phase() && w.wants_criteria());
        assert!(w.note(&Plan::default()).unwrap().contains("agree with the person what done means"));

        let mut agreed = criteria_plan(&["it works"]);
        w.begin(true, false);
        assert!(w.note(&agreed).unwrap().contains("The person agreed what done means"));
        agreed.awaiting_approval = true;
        assert!(w.note(&agreed).is_none(), "not yet agreed, so nothing is reminded");
        // Lean and Ad hoc agree nothing.
        assert!(!work("lean", true).wants_criteria() && !work(AD_HOC, true).wants_criteria());
    }

    #[test]
    fn criteria_are_ticked_and_an_unmet_one_is_sent_back_twice_at_most_with_its_reason() {
        let w = work("mission-command", true);
        let mut plan = criteria_plan(&["button", "excel"]);
        assert!(w.criteria_look_due(&plan));

        let (out, note) = w.criteria_looked(&mut plan, Some(vec![(true, None), (false, Some("one column".into()))]));
        let note = note.expect("the way of working sends it back");
        assert!(note.contains("- excel: one column"), "{note}");
        assert!(out.iter().any(|r| matches!(r, Reaction::Record(s) if s.what.starts_with("not met: excel"))));
        assert_eq!(plan.criteria[0].met, Some(true));

        let (_, again) = w.criteria_looked(&mut plan, Some(vec![(true, None), (false, Some("still one".into()))]));
        assert!(again.is_some());
        let (_, third) = w.criteria_looked(&mut plan, Some(vec![(true, None), (false, Some("still one".into()))]));
        assert!(third.is_none(), "sent back twice, then let go: the doubt stays on the card");
        assert!(!w.criteria_look_due(&plan), "and the check is made three times at most");

        // Everything met: ticked, and nothing is said to the model.
        let w = work("mission-command", true);
        let mut plan = criteria_plan(&["a"]);
        let (out, note) = w.criteria_looked(&mut plan, Some(vec![(true, None)]));
        assert!(note.is_none());
        assert!(out.iter().any(|r| matches!(r, Reaction::Record(s) if s.what == "all criteria met")));

        // No verdict: nothing ticked, nothing sent back.
        let w = work("mission-command", true);
        let mut plan = criteria_plan(&["a"]);
        let (_, note) = w.criteria_looked(&mut plan, None);
        assert!(note.is_none() && plan.criteria[0].met.is_none());
    }

    #[test]
    fn a_way_without_a_second_look_before_the_answer_ticks_the_boxes_and_sends_nothing_back() {
        let w = work("command-and-control", true);
        let mut plan = criteria_plan(&["a"]);
        let (_, note) = w.criteria_looked(&mut plan, Some(vec![(false, Some("missing".into()))]));
        assert!(note.is_none());
        assert_eq!(plan.criteria[0].met, Some(false));
        assert_eq!(plan.criteria[0].why.as_deref(), Some("missing"), "it stays open with its reason");
    }

    #[test]
    fn a_check_in_is_due_after_an_item_with_a_next_one_and_only_once() {
        let w = work("command-and-control", true);
        let plan = plan_of(&["export", "button"]);
        let mut p = plan.lock().unwrap();
        p.items[0].status = PlanStatus::Done;
        p.items[0].review = Some(Review { state: ReviewState::Checked, reason: None });
        let (ci, out) = w.check_in(&p, 0);
        assert_eq!(ci.unwrap().question, "\"export\" is done and checked. Next: button.");
        assert!(out.iter().any(|r| matches!(r, Reaction::Record(s) if s.mv == "check-in")));
        assert!(w.check_in(&p, 0).0.is_none(), "once per item");

        // The last item has no next one; Lean and Ad hoc do not check in; nor does a run nobody watches.
        p.items[1].status = PlanStatus::Done;
        assert!(w.check_in(&p, 1).0.is_none());
        assert!(work("lean", true).check_in(&p, 0).0.is_none());
        assert!(work("command-and-control", false).check_in(&p, 0).0.is_none());
    }

    #[test]
    fn what_the_person_says_to_a_check_in_is_told_to_the_model_and_taking_over_ends_the_work() {
        let w = work("command-and-control", true);
        assert!(w.checked_in(&["Continue".into()], None).contains("continue"));
        assert!(w.checked_in(&["Change the next step".into()], None).contains("did not say how"));
        assert!(w.checked_in(&[], Some("do the button first")).contains("do the button first"));
        assert!(!w.taken_over());
        assert!(w.checked_in(&[CHECK_TAKE.into()], None).contains("take it from here"));
        assert!(w.taken_over());
    }

    #[test]
    fn a_switch_waits_for_no_item_in_hand_and_then_replaces_the_rules_and_says_so_once() {
        let w = work("lean", true);
        let plan = plan_of(&["one"]);
        let mut p = plan.lock().unwrap();
        w.after_plan_write(&mut p, &[], 0, true);
        w.request_switch("swarm");
        assert_eq!(w.switch_pending().as_deref(), Some("swarm"));
        assert!(w.switch_ready().is_none(), "item 1 is in hand");

        let before = p.statuses();
        p.items[0].status = PlanStatus::Done;
        w.after_plan_write(&mut p, &before, 0, false);
        assert_eq!(w.switch_ready().as_deref(), Some("swarm"), "between items it can start");

        let r = w.switch_to("swarm", "Swarm", rules_of("swarm"));
        assert!(matches!(shows(&r)[0], AgentEvent::StyleSet { preset, source, .. } if preset == "swarm" && source == "switch"));
        assert!(matches!(shows(&r)[1], AgentEvent::HarnessCommand { note: Some(n), .. } if n == "I'm switching to Swarm from here."));
        assert_eq!(w.preset(), "swarm");
        assert!(!w.rounds(), "Lean's one-at-a-time is no longer held");
        assert!(w.switch_pending().is_none());
    }

    /// The loop switches as the write that ends an item comes in, before it reads the
    /// write: a switch to Ad hoc must still end the item, and the plan stops saying Lean.
    #[test]
    fn a_switch_to_ad_hoc_still_ends_the_item_in_hand() {
        let w = work("lean", true);
        let plan = plan_of(&["one", "two"]);
        let mut p = plan.lock().unwrap();
        w.after_plan_write(&mut p, &[], 0, true);
        let before = p.statuses();
        p.items[0].status = PlanStatus::Done;
        w.switch_to(AD_HOC, "Ad hoc", rules_of(AD_HOC));
        let events = w.after_plan_write(&mut p, &before, 0, true);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ItemEnded { item: 0, .. })), "{events:?}");
        assert!(!events.iter().any(|e| matches!(e, AgentEvent::ItemStarted { .. })), "Ad hoc starts nothing by itself");
        assert_eq!(p.style, None);
        let now = p.statuses();
        assert!(w.after_plan_write(&mut p, &now, 0, true).is_empty(), "and then it is quiet");
    }

    #[test]
    fn advice_is_due_before_a_risky_act_only_where_the_way_has_a_rule_and_not_this_time_skips_it() {
        let w = work("mission-command", true);
        let (due, out) = w.advice("deleting old/notes.md");
        assert!(due);
        assert!(out.iter().any(|r| matches!(r, Reaction::Record(s) if s.mv == "advice" && s.what == "deleting old/notes.md")));
        assert!(!work("lean", true).advice("deleting x").0, "Lean has no rule for it");
        assert!(!work(AD_HOC, true).advice("deleting x").0);

        let w = work("mission-command", true);
        assert!(w.skip_next("mission-command:risky-action-advice"));
        let (due, out) = w.advice("sending mail to ann");
        assert!(!due, "skipped this once");
        assert!(out.iter().any(|r| matches!(r, Reaction::Record(s) if s.what == "not this time")));
        assert!(w.advice("sending mail to ann").0, "and due again after");
    }

    #[test]
    fn helpers_handed_items_say_parallel_for_several_and_hand_off_for_a_named_agent() {
        let w = work("swarm", true);
        assert!(w.hands_out() && w.plans());
        assert!(!work("lean", true).hands_out());
        assert!(w.delegated(&[]).is_empty());
        let one = w.delegated(&[(0, false)]);
        assert!(shows(&one).is_empty(), "one general helper is neither of the two moves");
        let both = w.delegated(&[(0, false), (1, true)]);
        let mvs: Vec<&str> = shows(&both)
            .iter()
            .filter_map(|e| if let AgentEvent::Move { mv, .. } = e { Some(mv.as_str()) } else { None })
            .collect();
        assert_eq!(mvs, vec!["parallel", "hand-off"]);
        assert!(w.note(&Plan::default()).unwrap().contains("work as a swarm"));
    }

    #[test]
    fn a_run_working_ad_hoc_is_told_once_that_it_has_grown_and_may_offer_a_switch() {
        let w = work(AD_HOC, true);
        assert!(w.bigger(BIGGER_FILES - 1, true).is_none());
        assert!(w.bigger(BIGGER_FILES, false).is_none(), "nothing to offer it with");
        let said = w.bigger(BIGGER_FILES, true).unwrap();
        assert!(said.contains("mission-command") && said.contains("This is bigger than it looked"));
        assert!(w.bigger(BIGGER_FILES + 5, true).is_none(), "told once");
        assert!(work("lean", true).bigger(BIGGER_FILES, true).is_none(), "only Ad hoc is offered a switch");

        let w = work(AD_HOC, true);
        assert!(w.offer_switch());
        assert!(!w.offer_switch(), "one offer per run");
        assert!(w.bigger(BIGGER_FILES, true).is_none(), "and none after one was made");
    }
}
