//! `TEAM-10`: a session a team leads.
//!
//! The coordinator runs in the session like any lead (its persona is the chat's persona, its
//! style the team's `style`). What changes is who it may hand work to and how the work comes
//! back, and all of it rides on the delegation that exists (`subagents.rs`, `background.rs`):
//!
//! - **The roster.** `delegate` offers only the members the coordinator has a `hands-over`
//!   line to. A hand-over to anyone else is refused in the org loop's own words.
//! - **Members are helpers with a name.** Each has a chat of its own for the task
//!   (`subagent_runs.member`): the first hand-over opens it, a later one to the same member in
//!   the same task continues it. They run in the background, under the team's `member_style`,
//!   with their persona's instructions and memory (the chat's `persona_id`) and the member
//!   contract below in place of the helper one.
//! - **At most `limits.at_once` at a time.** The rest wait in order ([`Slots`], used by the
//!   background pool).
//!
//! A task is the session's current plan. A session without a plan reuses member chats until the
//! coordinator's run ends (Part XI of the plan, to be checked against real use).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use crate::agent::evolve::{Looked, TaskFacts};
use crate::agent::fleet::{Allowance, Spent};
use crate::agent::leader::Leader;
use crate::agent::plan::Plan;
use crate::db::{Conversation, Db, NewPost, Persona, Post, SubagentRun};
use crate::memory::ways::{Folder, LineKind, Member, Team};
use crate::memory::MemoryStore;

pub const COORDINATOR: &str = "coordinator";

/// The team that leads this chat, when one does. A helper's or a member's chat never has a
/// leader of its own (Part I.1), so only the session itself is a team run.
pub fn led_by(mem: &MemoryStore, conv: &Conversation) -> Option<Team> {
    if conv.parent_conversation_id.is_some() {
        return None;
    }
    match Leader::parse(conv.leader.as_deref()?)? {
        Leader::Team(slug) => mem.team(&slug),
        _ => None,
    }
}

pub fn of_session(db: &Db, mem: &MemoryStore, conv_id: &str) -> Option<Team> {
    led_by(mem, &db.get_conversation(conv_id).ok().flatten()?)
}

fn norm(s: &str) -> String {
    s.trim().to_ascii_lowercase().replace(['_', ' '], "-")
}

/// The members the coordinator may hand work to: the ones a `hands-over` line from the
/// coordinator reaches, in the order the team lists them.
pub fn roster(team: &Team) -> Vec<&Member> {
    team.members
        .iter()
        .filter(|m| team.lines.iter().any(|l| l.kind == LineKind::HandsOver && l.from == COORDINATOR && l.to == m.role))
        .collect()
}

fn names(members: &[&Member]) -> String {
    members.iter().map(|m| m.role.as_str()).collect::<Vec<_>>().join(", ")
}

/// Who a hand-over is for.
#[derive(Debug, Clone)]
pub struct Assigned {
    pub member: Member,
    /// `None` for a plain helper (and for a persona that no longer exists).
    pub persona: Option<Persona>,
}

/// Check a hand-over against the lines and find who gets it. `Err` is the refusal the
/// coordinator reads (Part VIII).
pub fn assign(db: &Db, team: &Team, want: Option<&str>) -> Result<Assigned, String> {
    let roster = roster(team);
    if roster.is_empty() {
        return Err(format!(
            "No line goes from the coordinator to anyone in {}, so there is no one to hand work to. Do the work yourself.",
            team.name
        ));
    }
    let Some(want) = want.map(str::trim).filter(|w| !w.is_empty()) else {
        return Err(format!("Say who gets this task. You can hand work to: {}.", names(&roster)));
    };
    let Some(member) = team.members.iter().find(|m| norm(&m.role) == norm(want)) else {
        return Err(format!("Nobody called {want} is in this team. You can hand work to: {}.", names(&roster)));
    };
    if !roster.iter().any(|m| m.role == member.role) {
        return Err(format!(
            "There is no line from {COORDINATOR} to {} in this team. You can hand work to: {}.",
            member.role,
            names(&roster)
        ));
    }
    Ok(assigned(db, member))
}

fn assigned(db: &Db, member: &Member) -> Assigned {
    let persona = member.persona.strip_prefix("persona:").and_then(|id| db.get_persona(id).ok().flatten());
    Assigned { member: member.clone(), persona }
}

/// A member by role, with no line needed: the hand-overs the loop makes itself (a review, a
/// send-back) go to someone the coordinator may have no line to.
pub fn member_named(db: &Db, team: &Team, role: &str) -> Option<Assigned> {
    team.members.iter().find(|m| m.role == role).map(|m| assigned(db, m))
}

fn who(target: &str) -> String {
    if target == COORDINATOR { "the coordinator".to_string() } else { target.to_string() }
}

/// What a member is told in place of the helper contract: its role, the team's job, whom it
/// may ask, and that its final message is the result.
pub fn contract(team: &Team, member: &Member) -> String {
    let mut out = format!("You are {}, a member of the team \"{}\".", member.role, team.name);
    let job = team.line.trim().trim_end_matches('.');
    if !job.is_empty() {
        out.push_str(&format!(" The team's job: {job}."));
    }
    let part = member.job.trim().trim_end_matches('.');
    if !part.is_empty() {
        out.push_str(&format!(" Your part: {part}."));
    }
    out.push_str(
        " The coordinator handed you one task and you cannot see the conversation it came from. Do the task and finish. Your final message is the whole result the coordinator receives, so put everything that matters in it.",
    );
    // `TEAM-15`: the post tool is how a member reaches anyone else, along the lines the team draws.
    let asks: Vec<String> =
        team.lines.iter().filter(|l| l.kind == LineKind::Asks && l.from == member.role).map(|l| who(&l.to)).collect();
    out.push_str(" To tell the coordinator something while you work, or to ask for an answer you cannot go on without, use the post tool.");
    if !asks.is_empty() {
        out.push_str(&format!(" You may also ask {} with it.", asks.join(", ")));
    }
    out.push_str(" Set wait to true only for a question you must have answered. An answer can be slow, so if none comes, say in your result what you assumed. Nobody else is told anything you do not post.");
    out.push_str(" If you cannot finish, say what you found and what is missing. Anything that would need a person to approve is refused here, so plan around that.");
    // `TEAM-13`: where the files are.
    match team.folder {
        Folder::Copies => out.push_str(
            " You work in your own copy of the project folder. When your item is done the coordinator brings your changes into the main folder, so do not write in the main folder yourself.",
        ),
        Folder::Owners if member.owns.is_empty() => out.push_str(
            " You work in the project folder. A place that another member owns is refused to you.",
        ),
        Folder::Owners => out.push_str(&format!(
            " You work in the project folder, and only in your own places: {}. A change anywhere else is refused, so ask the coordinator when your task needs one.",
            member.owns.join(", ")
        )),
    }
    // `TEAM-11`: how a review works, for the one who is looked at and the one who looks.
    if let Some(reviewer) = reviewer_of(team, &member.role) {
        out.push_str(&format!(
            " {} looks at your work when you finish. If it is sent back you get the reason as a new task in this chat: fix that and finish again with the whole result.",
            reviewer.role
        ));
    }
    if team.lines.iter().any(|l| l.kind == LineKind::Reviews && l.from == member.role) {
        out.push_str(&format!(
            " A task that begins \"{REVIEW_ITEM}\" or \"{REVIEW_WORK}\" is a teammate's work for you to look at. You may read files and run checks, but change nothing. End with the two lines VERDICT: CHECKED or SENT BACK, and REASON: one sentence (required when you send it back)."
        ));
    }
    out
}

/// How members work: the team's one style for them, Lean when it names none (the helper default).
pub fn member_style(team: &Team) -> &str {
    Some(team.member_style.trim()).filter(|s| !s.is_empty()).unwrap_or("lean")
}

/// The tool table of a team's coordinator: `delegate` names the members it may hand work to and
/// says how the work comes back, and `collect_agents` can wait for the first one. Any other run
/// keeps the tools it has.
pub fn specialise(specs: &mut Vec<serde_json::Value>, db: &Db, mem: &MemoryStore, run: &crate::agent::fleet::RunHandle, conv_id: &str) {
    // `TEAM-15`: the coordinator and the members can post, by the lines the team draws.
    if let Some(post) = post_spec_for(db, mem, run) {
        specs.push(post);
    }
    if run.parent.is_some() {
        return;
    }
    let Some(team) = of_session(db, mem, conv_id) else { return };
    let roster = roster(&team);
    for spec in specs.iter_mut() {
        match spec.pointer("/function/name").and_then(|n| n.as_str()) {
            Some("delegate") => *spec = delegate_spec(db, &team, &roster),
            Some("collect_agents") => {
                spec["function"]["description"] = serde_json::Value::String(
                    "Read the reports of the members you handed work to. By default it waits until they have all finished. With until \"any\" it returns as soon as one member has finished, so you can act on it while the others work. A member whose report you already read is not returned again by until \"any\". Posts the members sent you arrive with the reports, and a question from a member ends the wait at once: answer it with the post tool (reply_to its id), because the member is holding on for it.".into(),
                );
                spec["function"]["parameters"]["properties"]["until"] = serde_json::json!({
                    "type": "string",
                    "enum": ["all", "any"],
                    "description": "all (default): wait for every member. any: return on the first member that finishes."
                });
            }
            _ => {}
        }
    }
}

fn delegate_spec(db: &Db, team: &Team, roster: &[&Member]) -> serde_json::Value {
    let mut spec = crate::agent::subagents::tool_specs()[0].clone();
    let mut text = format!(
        "Hand work to the members of {}. A member works in the background in a chat of its own: a second task for the same member continues where it left off, and the member sees only the task you write, not our conversation. Use collect_agents to read what a member found, with until \"any\" to act on the first that finishes.",
        team.name
    );
    if roster.is_empty() {
        text.push_str("\n\nNo line goes from you to any member, so there is no one to hand work to.");
    } else {
        text.push_str("\n\nMembers you may hand work to:");
        for m in roster {
            let persona = m.persona.strip_prefix("persona:").and_then(|id| db.get_persona(id).ok().flatten()).map(|p| p.name);
            let mut line = format!("\n- {}", m.role);
            if let Some(p) = persona {
                line.push_str(&format!(" ({p})"));
            }
            if !m.job.trim().is_empty() {
                line.push_str(&format!(": {}", m.job.trim().trim_end_matches('.')));
            }
            text.push_str(&line);
        }
    }
    spec["function"]["description"] = serde_json::Value::String(text);
    // Hand-overs in a team always run in the background, so there is nothing to choose.
    if let Some(props) = spec.pointer_mut("/function/parameters/properties").and_then(|p| p.as_object_mut()) {
        props.remove("background");
    }
    if let Some(item) = spec.pointer_mut("/function/parameters/properties/tasks/items") {
        item["properties"]["agent"] = serde_json::json!({
            "type": "string",
            "enum": roster.iter().map(|m| m.role.clone()).collect::<Vec<_>>(),
            "description": "The member who gets this task, by role."
        });
        item["properties"]["item"] = serde_json::json!({
            "type": "integer",
            "description": "Which item of your plan this task is, numbered from 1."
        });
        item["required"] = serde_json::json!(["agent", "task"]);
    }
    spec
}

/// What the coordinator reads on its own transcript, every turn of a session its team leads: the
/// work is the members', and how to give it to them. The `delegate` description alone left a model
/// doing every part itself. `can_hand_over` is whether this run has `delegate`; `planning` is a run
/// that only writes the plan (planning first, or agreeing what done means), which has none by design.
/// `None` when no line from the coordinator reaches a member: then the work is the coordinator's.
pub fn coordinator_note(db: &Db, team: &Team, can_hand_over: bool, planning: bool) -> Option<String> {
    let roster = roster(team);
    if roster.is_empty() {
        return None;
    }
    let mut out = format!("You coordinate the team \"{}\"", team.name);
    let line = team.line.trim().trim_end_matches('.');
    if !line.is_empty() {
        out.push_str(&format!(" ({line})"));
    }
    out.push_str(". Its members do the work, not you. Members you hand work to:");
    for m in &roster {
        let persona = m.persona.strip_prefix("persona:").and_then(|id| db.get_persona(id).ok().flatten()).map(|p| p.name);
        out.push_str(&format!("\n- {}", m.role));
        if let Some(p) = persona {
            out.push_str(&format!(" ({p})"));
        }
        if !m.job.trim().is_empty() {
            out.push_str(&format!(": {}", m.job.trim().trim_end_matches('.')));
        }
    }
    // A reviewer is handed its look by the loop, so the coordinator never hands it the review.
    for reviewer in team.members.iter().filter(|m| !roster.iter().any(|r| r.role == m.role)) {
        let looks_at: Vec<&str> = team
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Reviews && l.from == reviewer.role)
            .map(|l| l.to.as_str())
            .collect();
        if !looks_at.is_empty() {
            out.push_str(&format!(
                "\n{} looks at the work of {} when they finish. That happens on its own, so you do not hand it the review.",
                reviewer.role,
                looks_at.join(" and ")
            ));
        }
    }
    if planning {
        out.push_str("\nWrite the plan so that each item is one member's part, and name the member in the item. The members start once the person agrees to the plan.");
    } else if can_hand_over {
        out.push_str("\nYou have no tools to search, read or write yourself: those are the members' tools. Hand each part to the member whose job it is with delegate, and give the number of the plan item it is when there is a plan. Read what they found with collect_agents, then write the answer from their reports.");
    } else {
        out.push_str("\nYou have no delegate tool in this chat, so the members cannot take part. Do the work yourself, and say at the start of your answer that the team's members did not work on it because handing work over is switched off.");
    }
    Some(out)
}

// ---- one chat per member per task ----

/// Where this task started: runs of a member from here on are the same task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    /// The coordinator's turn that is handing work over now. Whatever it handed out before is
    /// the same task, whether or not there is a plan.
    pub turn: Option<String>,
    /// With a plan still open from earlier turns: when its first turn began.
    pub since: Option<i64>,
    /// `TEAM-12`: that first turn's message, which names the task the same way on every turn of it.
    pub first: Option<String>,
}

/// The task the session is in now (Part XI): the plan that is still open, back to the turn that
/// wrote it. Without one, only this turn counts.
pub fn boundary(db: &Db, session: &str, turn: Option<&str>) -> Boundary {
    let mut since = None;
    let mut first = None;
    for m in db.list_messages(session).unwrap_or_default().iter().rev().filter(|m| m.role == "assistant") {
        if Some(m.id.as_str()) == turn {
            continue;
        }
        let open = m
            .plan_json
            .as_deref()
            .and_then(|j| serde_json::from_str::<Plan>(j).ok())
            .is_some_and(|p| !p.open_items().is_empty());
        if !open {
            break;
        }
        since = Some(m.created_at);
        first = Some(m.id.clone());
    }
    Boundary { turn: turn.map(str::to_string), since, first }
}

/// The member's latest run in this task, whose chat a new hand-over continues.
pub fn earlier_run(db: &Db, session: &str, role: &str, b: &Boundary) -> Option<SubagentRun> {
    db.list_subagent_runs(session)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .find(|r| r.member.as_deref() == Some(role))
        .filter(|r| {
            (b.turn.is_some() && r.parent_message_id == b.turn) || b.since.is_some_and(|s| r.started_at >= s)
        })
}

/// What a continued chat starts from: everything its last run saw and said, then the new task.
/// `None` when that run left no log, in which case the chat goes on with a fresh start.
pub fn continued(db: &Db, earlier: &SubagentRun, task: &str) -> Option<Vec<serde_json::Value>> {
    let mut messages = crate::agent::log::replay(db, &earlier.id);
    if messages.is_empty() {
        return None;
    }
    let ended_on_text = messages.last().is_some_and(|m| m["role"] == "assistant" && m.get("tool_calls").is_none());
    if !ended_on_text {
        let said = earlier.result.as_deref().unwrap_or_default().trim();
        messages.push(serde_json::json!({ "role": "assistant", "content": if said.is_empty() { "(it produced no text)" } else { said } }));
    }
    messages.push(serde_json::json!({ "role": "user", "content": task }));
    Some(messages)
}

// ---- reading what members found ----

/// Member runs whose report the coordinator has read, so `until: "any"` waits for the next one.
fn read() -> &'static Mutex<HashSet<String>> {
    static READ: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    READ.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn mark_read(run_ids: impl IntoIterator<Item = String>) {
    if let Ok(mut set) = read().lock() {
        set.extend(run_ids);
    }
}

pub fn is_read(run_id: &str) -> bool {
    read().lock().map(|s| s.contains(run_id)).unwrap_or(false)
}

/// Does a collect with `until` mean "the first one"?
pub fn wants_any(args: &serde_json::Value) -> bool {
    args.get("until").and_then(|v| v.as_str()).is_some_and(|u| u.trim().eq_ignore_ascii_case("any"))
}

// ---- TEAM-12: what a task may spend, and the question before it starts ----

/// One allowance per team task, found by the session the team leads: the coordinator's turn
/// makes it, and the members of that task (which are not the coordinator, and may run on after
/// its turn) find it by the session on their seat.
fn allowances() -> &'static Mutex<HashMap<String, (String, Arc<Allowance>)>> {
    static ALLOWANCES: OnceLock<Mutex<HashMap<String, (String, Arc<Allowance>)>>> = OnceLock::new();
    ALLOWANCES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn allowance_of(session: &str) -> Option<Arc<Allowance>> {
    allowances().lock().ok()?.get(session).map(|(_, a)| a.clone())
}

/// The allowance for the task this coordinator turn belongs to, made when the turn starts: the
/// team's `limits.budget` and `limits.minutes`. The turns of one task (a plan still open from
/// earlier turns) share it. A task that already hit its limit gets a fresh one on the person's
/// next message, because sending it is the person saying to go on.
pub fn start_task(db: &Db, mem: &MemoryStore, session: &str, turn: Option<&str>) -> Option<Arc<Allowance>> {
    let team = of_session(db, mem, session)?;
    let b = boundary(db, session, turn);
    let key = b.first.or(b.turn).unwrap_or_default();
    let mut all = allowances().lock().ok()?;
    if let Some((k, a)) = all.get(session) {
        if *k == key && a.spent_out().is_none() {
            return Some(a.clone());
        }
    }
    let a = Arc::new(Allowance::new(team.limits.budget, team.limits.minutes));
    all.insert(session.to_string(), (key, a.clone()));
    Some(a)
}

/// What the coordinator reads when a hand-over comes after the limit was reached.
pub fn spent(session: &str) -> Option<String> {
    let what = allowance_of(session)?.spent_out()?;
    Some(format!("{} No more work is handed out in this task. Report what is done and what is not.", spent_words(what)))
}

fn spent_words(what: Spent) -> &'static str {
    match what {
        Spent::Money => "The team has spent the most it may spend on this task.",
        Spent::Time => "The team has used the most time it may take on this task.",
    }
}

/// The closing turn of a coordinator whose task ran out: the members are stopped already.
pub fn spent_prompt(what: Spent) -> String {
    format!(
        "Stop working now. {} Every member was stopped. Answer with what you have. Say which items are done and which are not, in a few sentences, so the person can decide how to go on.",
        spent_words(what)
    )
}

// The question card (Part II.8).
pub const COST_START: &str = "Start";
pub const COST_NOT_NOW: &str = "Not now";
pub const COST_DONT_ASK: &str = "Don't ask for this team";
/// The one line a turn says when the person chose Not now.
pub const NOT_STARTED: &str = "Not started.";

fn cost_key(session: &str) -> String {
    format!("team.cost.{session}")
}

/// Was the person asked, and said Start, for this team in this session? A hand-over to a team
/// (`leader::set_leader`) forgets it, so the card comes again.
pub fn cost_asked(db: &Db, session: &str, slug: &str) -> bool {
    db.get_setting(&cost_key(session)).ok().flatten().as_deref() == Some(slug)
}

pub fn forget_cost_answer(db: &Db, session: &str) {
    let _ = db.delete_setting(&cost_key(session));
}

/// The person said Start. With the box ticked the team stops asking: `ask_cost: no`, saved as
/// a version of the team like any change the person makes.
pub fn cost_answered(db: &Db, mem: &MemoryStore, session: &str, team: &Team, stop_asking: bool) {
    let _ = db.set_setting(&cost_key(session), &team.slug);
    if stop_asking {
        if let Some(mut t) = mem.team(&team.slug) {
            t.limits.ask_cost = false;
            let _ = mem.save_team(t, "you", "stopped asking before a task");
        }
    }
}

/// A task as big as the team usually makes, for the estimate before there are past tasks to
/// read it from: tokens in and out, for each run of it (the coordinator and every member once).
const TYPICAL_PROMPT_TOKENS: u64 = 60_000;
const TYPICAL_OUTPUT_TOKENS: u64 = 8_000;
/// A rough figure is a half to one and a half times the typical one.
const ROUGH_LOW: f64 = 0.5;
const ROUGH_HIGH: f64 = 1.5;
/// Past tasks needed before the estimate is theirs, and how many of the latest it reads.
pub const PAST_TASKS_NEEDED: usize = 3;
const PAST_TASKS_READ: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub low: f64,
    pub high: f64,
    /// From prices and a typical size, not from tasks the team really did.
    pub rough: bool,
}

/// What one run costs, in USD, the way Settings → Usage counts it: a local model is free, and a
/// model with no price in the table is not guessed at (`None`).
fn price_of(model: &str, provenance: &str, prompt: u64, output: u64) -> Option<f64> {
    match provenance {
        "local" | "agent-app" => Some(0.0),
        _ => crate::cloud::pricing::cost_usd(model, prompt, output),
    }
}

/// What the team's last finished tasks cost, newest first (at most the five that are read). A
/// task is one coordinator turn that handed work to members and has ended; its cost is the
/// coordinator's run and the runs of those members, from the usage rows.
pub fn past_costs(db: &Db, slug: &str) -> Vec<f64> {
    let mut tasks: Vec<(i64, f64)> = Vec::new();
    for session in db.conversations_led_by(&format!("team:{slug}")).unwrap_or_default() {
        let rows: Vec<SubagentRun> =
            db.list_subagent_runs(&session).unwrap_or_default().into_iter().filter(|r| r.member.is_some()).collect();
        let messages = db.list_messages(&session).unwrap_or_default();
        let own = db.run_usage_of(&session).unwrap_or_default();
        let mut seen = HashSet::new();
        for turn in rows.iter().filter_map(|r| r.parent_message_id.as_deref()).filter(|t| seen.insert(*t)) {
            let mine: Vec<&SubagentRun> = rows.iter().filter(|r| r.parent_message_id.as_deref() == Some(turn)).collect();
            if mine.iter().any(|r| r.ended_at.is_none()) {
                continue;
            }
            let Some(at) = messages.iter().position(|m| m.id == turn) else { continue };
            let from = messages[at].created_at;
            let to = messages.get(at + 1).map_or(i64::MAX, |m| m.created_at);
            let coordinator: Vec<_> = own.iter().filter(|u| u.5 >= from && u.5 < to).collect();
            // The coordinator's run writes its row when it ends: no row, not finished.
            if coordinator.is_empty() {
                continue;
            }
            let mut cost: f64 = coordinator.iter().map(|u| price_of(&u.1, &u.2, u.3, u.4).unwrap_or(0.0)).sum();
            for run in &mine {
                for u in db.run_usage_of(&run.child_conversation_id).unwrap_or_default().iter().filter(|u| u.0 == run.id) {
                    cost += price_of(&u.1, &u.2, u.3, u.4).unwrap_or(0.0);
                }
            }
            tasks.push((mine.iter().filter_map(|r| r.ended_at).max().unwrap_or(from), cost));
        }
    }
    tasks.sort_by_key(|t| std::cmp::Reverse(t.0));
    tasks.into_iter().take(PAST_TASKS_READ).map(|t| t.1).collect()
}

/// What a task like this costs: from the team's own past tasks when there are three, else from
/// the price of the model each run uses and a typical size. `None` when the model has no price.
pub fn estimate(past: &[f64], runs: usize, model: &str, provenance: &str) -> Option<Estimate> {
    if past.len() >= PAST_TASKS_NEEDED {
        let low = past.iter().cloned().fold(f64::INFINITY, f64::min);
        let high = past.iter().cloned().fold(0.0, f64::max);
        return Some(Estimate { low, high, rough: false });
    }
    let one = price_of(model, provenance, TYPICAL_PROMPT_TOKENS, TYPICAL_OUTPUT_TOKENS)?;
    let typical = one * runs as f64;
    Some(Estimate { low: typical * ROUGH_LOW, high: typical * ROUGH_HIGH, rough: true })
}

/// The Usage view's money: dollars, because that is what the prices are in.
fn money(usd: f64) -> String {
    if usd < 0.01 {
        "under $0.01".to_string()
    } else {
        format!("${usd:.2}")
    }
}

pub struct CostCard {
    pub title: String,
    pub text: String,
}

/// The card before a team's task starts (Part II.8): who works on what model, what a task like
/// this costs, and where it stops. `model` is what the members run on, which is the model the
/// chat runs on (a persona's own model is not used yet).
pub fn cost_card(db: &Db, team: &Team, model: &str, provenance: &str) -> CostCard {
    let on = if provenance == "local" { "local".to_string() } else { model.rsplit('/').next().unwrap_or(model).to_string() };
    let coordinator = team
        .coordinator_persona()
        .and_then(|id| db.get_persona(id).ok().flatten())
        .map_or_else(|| "Poiesis".to_string(), |p| p.name);
    let mut who = vec![coordinator];
    let mut personas = true;
    for m in &team.members {
        match assigned(db, m).persona {
            Some(p) => who.push(p.name),
            None => {
                personas = false;
                who.push(m.role.clone());
            }
        }
    }
    let count = who.len();
    let noun = match (personas, count) {
        (true, 1) => "persona",
        (true, _) => "personas",
        (false, 1) => "member",
        (false, _) => "members",
    };
    let roster = who.iter().map(|n| format!("{n} ({on})")).collect::<Vec<_>>().join(", ");
    let cost = match estimate(&past_costs(db, &team.slug), count, model, provenance) {
        None => "I can't tell yet what a task like this costs.".to_string(),
        Some(e) if e.high < 0.005 => "A task like this costs nothing: the models run on this computer.".to_string(),
        Some(e) => {
            let range = if e.high - e.low < 0.005 { money(e.high) } else { format!("{} to {}", money(e.low), money(e.high)) };
            if e.rough {
                format!("A task like this costs roughly {range}.")
            } else {
                format!("A task like this has cost about {range}.")
            }
        }
    };
    let stop = if team.limits.budget > 0.0 {
        format!("It stops at ${:.2} or after {} minutes.", team.limits.budget, team.limits.minutes)
    } else {
        format!("It stops after {} minutes.", team.limits.minutes)
    };
    CostCard { title: format!("{} is about to start", team.name), text: format!("{count} {noun}: {roster}.\n\n{cost}\n\n{stop}") }
}

// ---- the queue ----

/// A member run's place in its team's queue and what the board draws for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    /// The session the team leads.
    pub session: String,
    /// `limits.at_once` of the team.
    pub at_once: usize,
    pub role: String,
    pub item: Option<usize>,
}

/// How many member runs each team's session has going, against its `at_once`.
#[derive(Debug, Default)]
pub struct Slots {
    running: HashMap<String, usize>,
}

impl Slots {
    pub fn has_room(&self, seat: &Seat) -> bool {
        self.running.get(&seat.session).copied().unwrap_or(0) < seat.at_once.max(1)
    }

    pub fn take(&mut self, seat: &Seat) {
        *self.running.entry(seat.session.clone()).or_default() += 1;
    }

    pub fn free(&mut self, seat: &Seat) {
        if let Some(n) = self.running.get_mut(&seat.session) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.running.remove(&seat.session);
            }
        }
    }
}

// ---- TEAM-11: a second look from the member on a `reviews` line ----
//
// The loop, not the model, makes it. When a member that has a reviewer ends, the next time the
// coordinator reads its report (`collect_agents`, or at once for a foreground hand-over) the
// reviewer is handed the item as a task of its own, in its own chat for the task. Its answer is
// read with the forgiving verdict parser of the second look (`aside::parse_look`; a reply nobody
// can read lets the item stand). Sent back, the member gets the reason as a new task in its own
// chat. Everything is read back from the run rows, so nothing else is kept: a review is a run of
// the reviewer whose task begins `Review item N by api:`, a send-back is a run of the member whose
// task begins `Sent back by`.

/// How the loop's own hand-overs begin. The coordinator's turn cap does not count them.
const REVIEW_ITEM: &str = "Review item ";
const REVIEW_WORK: &str = "Review the work of ";
const SENT_BACK: &str = "Sent back by ";
/// `TEAM-15`: a post that wakes a member who is not working begins its new task like this.
const MESSAGE: &str = "Message from ";

/// Was this task written by the loop, not by the coordinator?
pub fn made_by_loop(task: &str) -> bool {
    [REVIEW_ITEM, REVIEW_WORK, SENT_BACK, MESSAGE].iter().any(|p| task.starts_with(p))
}

/// `TEAM-13`: whose work a review task is about, read from how the loop wrote it
/// ("Review item 3 by api: ...", "Review the work of api: ...").
pub fn reviewed(task: &str) -> Option<&str> {
    let rest = match task.strip_prefix(REVIEW_ITEM) {
        Some(r) => r.split_once(" by ")?.1,
        None => task.strip_prefix(REVIEW_WORK)?,
    };
    Some(rest.split(':').next()?.trim()).filter(|r| !r.is_empty())
}

/// The member that looks at `member`'s work: the first `reviews` line onto it. The coordinator
/// is not a member; its second look stays the side call.
pub fn reviewer_of<'t>(team: &'t Team, member: &str) -> Option<&'t Member> {
    team.lines
        .iter()
        .filter(|l| l.kind == LineKind::Reviews && l.to == member)
        .find_map(|l| team.members.iter().find(|m| m.role == l.from))
}

fn review_head(member: &str, item: Option<usize>) -> String {
    match item {
        Some(n) => format!("{REVIEW_ITEM}{n} by {member}:"),
        None => format!("{REVIEW_WORK}{member}:"),
    }
}

/// What the reviewer is handed: the item, what the member reported, what it changed, and the
/// shape of the answer.
fn review_task(member: &str, item: Option<usize>, task: &str, report: &str, patch: &str) -> String {
    let clip = |s: &str, max: usize| {
        let s = s.trim();
        if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max).collect::<String>()) }
    };
    let patch = patch.trim();
    let patch = if patch.is_empty() || patch.starts_with("You have not changed any files") {
        "(no files were changed)".to_string()
    } else {
        clip(patch, 14_000)
    };
    format!(
        "{} {}\n\n{member}'s report:\n{}\n\nWhat {member} changed on disk:\n{patch}\n\nIs it really done? Look at what was changed and run what proves it, but change nothing yourself. Send it back only for something that is missing, wrong or unproven, and say exactly what. Reply in exactly this form and nothing else:\nVERDICT: CHECKED or SENT BACK\nREASON: one sentence (required when you send it back)",
        review_head(member, item),
        clip(task, 1_500),
        clip(report, 4_000),
    )
}

fn sent_back_task(reviewer: &str, item: Option<usize>, reason: &str) -> String {
    let what = item.map_or("your work".to_string(), |n| format!("item {n}"));
    format!("{SENT_BACK}{reviewer} on {what}: {reason}\nFix that, run what proves it, and finish again with the whole result.")
}

/// What a finished review said. A reviewer that did not finish, or whose answer cannot be read,
/// lets the item stand: a check that cannot be made must not stop the work it checks.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Said {
    Kept(Option<String>),
    SentBack(String),
}

fn said(review: &SubagentRun) -> Said {
    if review.stop_reason.as_deref() != Some("completed") {
        return Said::Kept(None);
    }
    match crate::agent::aside::parse_look(review.result.as_deref().unwrap_or_default()) {
        Some(crate::agent::aside::Look::SentBack(reason)) => Said::SentBack(reason),
        Some(crate::agent::aside::Look::Checked(reason)) => Said::Kept(reason),
        None => Said::Kept(None),
    }
}

/// Where a finished member run stands with its reviewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Nobody has looked yet.
    Owed,
    /// The reviewer kept it (or its answer could not be read).
    Kept(Option<String>),
    /// Sent back, and the member is to work on it again.
    Back(String),
    /// Sent back as often as the team allows: the coordinator decides.
    Stuck(String),
    /// Sent back and the member has worked again since, or the coordinator moved on:
    /// a later run of the member is the one that counts.
    Gone,
}

/// Is this run in the task the coordinator is working on now?
pub fn in_task(r: &SubagentRun, b: &Boundary) -> bool {
    (b.turn.is_some() && r.parent_message_id == b.turn) || b.since.is_some_and(|s| r.started_at >= s)
}

/// The standing of `rows[at]` with its reviewer. `None` when it is not a member run that is owed
/// a look (it has no reviewer, it did not finish, or its reviewer is still looking).
pub fn standing(team: &Team, rows: &[SubagentRun], at: usize) -> Option<(Standing, String)> {
    let run = &rows[at];
    let role = run.member.as_deref()?;
    if run.ended_at.is_none() || run.stop_reason.as_deref() != Some("completed") {
        return None;
    }
    let reviewer = reviewer_of(team, role)?.role.clone();
    let next = rows[at + 1..].iter().position(|x| x.member.as_deref() == Some(role)).map(|i| at + 1 + i);
    let head = review_head(role, run.item);
    let is_review = |x: &SubagentRun| x.member.as_deref() == Some(&reviewer) && x.item == run.item && x.task.starts_with(&head);
    let until = next.unwrap_or(rows.len());
    let Some(review) = rows[at + 1..until].iter().rev().find(|x| is_review(x)) else {
        return Some((if next.is_some() { Standing::Gone } else { Standing::Owed }, reviewer));
    };
    if review.ended_at.is_none() {
        return None;
    }
    let standing = match said(review) {
        Said::Kept(reason) => Standing::Kept(reason),
        Said::SentBack(_) if next.is_some() => Standing::Gone,
        Said::SentBack(reason) => {
            // The rounds of this hand-over: the reviews since its first run.
            let mut first = at;
            while first > 0 && rows[first].task.starts_with(SENT_BACK) {
                match rows[..first].iter().rposition(|x| x.member.as_deref() == Some(role)) {
                    Some(p) => first = p,
                    None => break,
                }
            }
            let rounds = rows[first..].iter().filter(|x| is_review(x)).count() as u32;
            if rounds >= team.limits.rounds.max(1) { Standing::Stuck(reason) } else { Standing::Back(reason) }
        }
    };
    Some((standing, reviewer))
}

/// What the coordinator reads under a member's report about its second look. `None` when there
/// is nothing to say.
pub fn note(team: &Team, rows: &[SubagentRun], at: usize) -> Option<String> {
    let role = rows[at].member.clone()?;
    let (standing, reviewer) = standing(team, rows, at)?;
    match standing {
        Standing::Kept(reason) => Some(format!("{reviewer} looked at it and kept it.{}", reason.map(|r| format!(" {r}")).unwrap_or_default())),
        Standing::Stuck(reason) => Some(format!(
            "{}. Decide what happens with it: accept it, hand it over again with your own instruction, or do it yourself.",
            disagreement(&role, &reviewer, rows[at].item, &reason)
        )),
        Standing::Owed | Standing::Back(_) => Some(format!("{reviewer} has not looked at it yet.")),
        Standing::Gone => None,
    }
}

/// "api and verifier did not agree on item 3: …" (Part VIII).
pub fn disagreement(member: &str, reviewer: &str, item: Option<usize>, reason: &str) -> String {
    let on = item.map_or("their work".to_string(), |n| format!("item {n}"));
    format!("{member} and {reviewer} did not agree on {on}: {}", reason.trim().trim_end_matches('.'))
}

/// A run of a member whose standing is `Gone` is left out of what the coordinator reads.
pub fn is_gone(team: &Team, rows: &[SubagentRun], at: usize) -> bool {
    matches!(standing(team, rows, at), Some((Standing::Gone, _)))
}

/// The reason a kept item was kept, for the coordinator's own second look (`work.rs`) to take
/// instead of looking again: the latest member run for `item` (from 1) in this task, when its
/// reviewer kept it.
pub fn kept(db: &Db, team: &Team, session: &str, turn: Option<&str>, item: usize) -> Option<Option<String>> {
    let rows = db.list_subagent_runs(session).ok()?;
    let b = boundary(db, session, turn);
    let at = rows.iter().rposition(|r| {
        r.item == Some(item) && in_task(r, &b) && r.member.as_deref().is_some_and(|m| reviewer_of(team, m).is_some())
    })?;
    match standing(team, &rows, at)? {
        (Standing::Kept(reason), _) => Some(reason),
        _ => None,
    }
}

// ---- what evolution reads of finished tasks (`TEAM-23`) ----

/// What the team's finished tasks showed, newest first, at most `limit`. A task is as in
/// [`past_costs`]: one coordinator turn that handed work to members, all of whose runs have ended.
pub fn task_facts(db: &Db, team: &Team, limit: usize) -> Vec<TaskFacts> {
    let mut out: Vec<TaskFacts> = Vec::new();
    for session in db.conversations_led_by(&format!("team:{}", team.slug)).unwrap_or_default() {
        let rows: Vec<SubagentRun> =
            db.list_subagent_runs(&session).unwrap_or_default().into_iter().filter(|r| r.member.is_some()).collect();
        out.extend(session_tasks(team, &rows));
    }
    out.sort_by_key(|t| std::cmp::Reverse(t.at));
    out.truncate(limit);
    out
}

/// One session's tasks, from the member runs alone (in the order they started). A review of an item is
/// a run of the reviewer whose task begins "Review item N by api:"; a send-back is a run of the member
/// that begins "Sent back by"; a hand-over the coordinator wrote starts a new chain for its item.
fn session_tasks(team: &Team, rows: &[SubagentRun]) -> Vec<TaskFacts> {
    struct Chain {
        turn: Option<String>,
        looked: Looked,
    }
    let mut chains: Vec<Chain> = Vec::new();
    let mut open: HashMap<(String, Option<usize>), usize> = HashMap::new();
    for r in rows {
        let Some(role) = r.member.as_deref() else { continue };
        if let Some(of) = reviewed(&r.task) {
            // A look at an item of `of`, counted when it has ended: one still going has said nothing.
            if let (Some(&at), true) = (open.get(&(of.to_string(), r.item)), r.ended_at.is_some()) {
                let looked = &mut chains[at].looked;
                let back = matches!(said(r), Said::SentBack(_));
                looked.reviews += 1;
                looked.sent_back |= back;
                looked.last_sent_back = back;
                looked.reviewer = role.to_string();
            }
        } else if r.task.starts_with(SENT_BACK) || r.task.starts_with(MESSAGE) {
            // The same chain goes on, and the member did work.
        } else if let Some(reviewer) = reviewer_of(team, role) {
            let looked = Looked { member: role.to_string(), reviewer: reviewer.role.clone(), reviews: 0, sent_back: false, last_sent_back: false };
            chains.push(Chain { turn: r.parent_message_id.clone(), looked });
            open.insert((role.to_string(), r.item), chains.len() - 1);
        }
    }
    let mut turns: Vec<&str> = Vec::new();
    for t in rows.iter().filter_map(|r| r.parent_message_id.as_deref()) {
        if !turns.contains(&t) {
            turns.push(t);
        }
    }
    turns
        .into_iter()
        .filter_map(|turn| {
            let mine: Vec<&SubagentRun> = rows.iter().filter(|r| r.parent_message_id.as_deref() == Some(turn)).collect();
            if mine.iter().any(|r| r.ended_at.is_none()) {
                return None;
            }
            let mut ran: Vec<String> = mine.iter().filter_map(|r| r.member.clone()).collect();
            ran.sort_unstable();
            ran.dedup();
            let looked = chains
                .iter()
                .filter(|c| c.turn.as_deref() == Some(turn) && c.looked.reviews > 0)
                .map(|c| c.looked.clone())
                .collect();
            Some(TaskFacts { at: mine.iter().filter_map(|r| r.ended_at).max()?, ran, looked })
        })
        .collect()
}

/// `TEAM-23`: the person tried to speak to `who` (a role) in a session a team leads and has no line to
/// them: a persona that plays a member, with no `you talks` line. Remembered once for each chat and
/// member, for evolution to read; it offers to draw the line after three chats. Anything else is not a
/// signal and is not remembered.
pub fn note_asked_without_line(db: &Db, mem: &MemoryStore, session: &str, who: &str) -> bool {
    let Some(team) = of_session(db, mem, session) else { return false };
    let Some(member) = team.members.iter().find(|m| norm(&m.role) == norm(who)) else { return false };
    let drawn = team.lines.iter().any(|l| l.kind == LineKind::Talks && l.from == YOU && l.to == member.role);
    if drawn || !member.persona.starts_with("persona:") {
        return false;
    }
    let said = |v: &serde_json::Value| v["team"] == team.slug.as_str() && v["member"] == member.role.as_str();
    let again = db.team_events_since(0).unwrap_or_default().into_iter().any(|(conv, _, json)| {
        conv == session && serde_json::from_str::<serde_json::Value>(&json).is_ok_and(|v| said(&v))
    });
    if again {
        return false;
    }
    let payload = serde_json::json!({ "team": team.slug, "what": crate::agent::evolve::ASKED_WITHOUT_LINE, "member": member.role });
    db.append_session_event(session, None, crate::agent::evolve::TEAM_EVENT, &payload).is_ok()
}

/// `TEAM-23`: the person chose `style` in a session a team leads. Remembered for evolution, which offers
/// to make a way of working the team's own once the person chose it in three different chats. Only
/// the person's own choice is noted (`set_conversation_style_cmd`): a scheduled task sets its style
/// on each run's chat by itself, and that is one choice, not one per run.
pub fn note_style_chosen(db: &Db, mem: &MemoryStore, session: &str, style: &str) -> bool {
    let Some(team) = of_session(db, mem, session) else { return false };
    let payload = serde_json::json!({ "team": team.slug, "what": crate::agent::evolve::STYLE_CHOSEN, "style": style });
    db.append_session_event(session, None, crate::agent::evolve::TEAM_EVENT, &payload).is_ok()
}

// ---- keeping helpers as a team (`TEAM-22`) ----

/// A role name for a helper type: lowercase letters, digits, `-` and `_`, at most 24 long, not a
/// reserved word, and not one already taken.
fn role_for(agent: &str, taken: &[String]) -> String {
    let base: String = match agent.trim().to_ascii_lowercase().as_str() {
        "general" | "" => "helper".to_string(),
        other => other
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .chars()
            .take(20)
            .collect(),
    };
    let base = if base.is_empty() || ["coordinator", "you", "all"].contains(&base.as_str()) { "helper".to_string() } else { base };
    let mut role = base.clone();
    let mut n = 2;
    while taken.contains(&role) {
        role = format!("{base}-{n}");
        n += 1;
    }
    role
}

/// The job line for a member made from a plain helper: its first task, one line, short, and with
/// nothing in it the team file would read as a separator.
fn job_from(task: &str) -> String {
    let line = task.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().replace(" \u{b7} ", " - ");
    let mut out: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        out.push('\u{2026}');
    }
    out
}

/// `TEAM-22`: "Keep these helpers as a team". A New team made from what a session did: its leader
/// coordinates, each helper type it used is a member (a persona of that name plays it; any other is
/// a plain helper whose job is the task it was first given), and the lines are the hand-overs that
/// happened. A Hub, as every new team starts. Not saved: the person edits it first. `None` when the
/// session used fewer than two helpers, or a team already leads it. Agent apps and the loop's own
/// hand-overs are not members.
pub fn keep_as_team(db: &Db, mem: &MemoryStore, session: &str) -> Option<Team> {
    let conv = db.get_conversation(session).ok().flatten()?;
    if conv.parent_conversation_id.is_some() || led_by(mem, &conv).is_some() {
        return None;
    }
    let runs: Vec<SubagentRun> = db
        .list_subagent_runs(session)
        .ok()?
        .into_iter()
        .filter(|r| r.engine_id.is_none() && r.member.is_none() && !made_by_loop(&r.task))
        .collect();
    if runs.len() < 2 {
        return None;
    }
    let personas = db.list_personas().unwrap_or_default();
    let mut members: Vec<Member> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for run in &runs {
        if seen.contains(&run.agent.as_str()) {
            continue;
        }
        seen.push(run.agent.as_str());
        let persona = personas.iter().find(|p| p.name.eq_ignore_ascii_case(run.agent.trim()));
        let taken: Vec<String> = members.iter().map(|m| m.role.clone()).collect();
        members.push(Member {
            role: role_for(&run.agent, &taken),
            persona: persona.map_or("helper".to_string(), |p| format!("persona:{}", p.id)),
            job: if persona.is_some() { String::new() } else { job_from(&run.task) },
            owns: Vec::new(),
        });
    }
    let mut team = Team::blank();
    team.coordinator = match crate::agent::leader::effective(db, mem, &conv).leader.strip_prefix("persona:") {
        Some(id) => format!("persona:{id}"),
        None => "poiesis".to_string(),
    };
    team.name = format!("{} team", conv.title.trim()).chars().take(40).collect();
    team.lines = crate::memory::ways::shape_lines(team.shape, &members);
    team.members = members;
    Some(team)
}

// ---- what the loop owes the person ----

/// Something the person may want to hear about: a member that did not finish, or two members who
/// could not agree. The coordinator's loop puts it to the person through the check-in card when the
/// way of working or the team's `report` says to (`run.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub item: Option<usize>,
    pub text: String,
}

fn problems() -> &'static Mutex<Vec<(String, Problem)>> {
    static P: OnceLock<Mutex<Vec<(String, Problem)>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(Vec::new()))
}

/// Said once per key (a run id).
fn once(key: &str) -> bool {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashSet::new())).lock().map(|mut s| s.insert(key.to_string())).unwrap_or(false)
}

pub fn raise(session: &str, key: &str, problem: Problem) {
    if once(key) {
        if let Ok(mut p) = problems().lock() {
            p.push((session.to_string(), problem));
        }
    }
}

pub fn take_problems(session: &str) -> Vec<Problem> {
    let Ok(mut all) = problems().lock() else { return Vec::new() };
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut *all).into_iter().partition(|(s, _)| s == session);
    *all = rest;
    mine.into_iter().map(|(_, p)| p).collect()
}

/// A member that did not finish says so to the coordinator in its report, and to the person when
/// the team reports on problems.
pub fn failed(session: &str, run: &SubagentRun) {
    if run.member.is_some() && run.ended_at.is_some() && run.stop_reason.as_deref() != Some("completed") {
        let who = run.member.as_deref().unwrap_or_default();
        let on = run.item.map_or(String::new(), |n| format!(" item {n}"));
        raise(session, &run.id, Problem { item: run.item, text: format!("{who} did not finish{on}.") });
    }
}

/// Make the reviews the loop owes, for every member run of this task that has a reviewer: hand
/// the item to the reviewer, read the answer, and send a member back with its reason. With
/// `background`, the member works again in the pool and this returns (the next call sees its new
/// run); without, it works again here and this goes on until nothing is owed. A team with no
/// `reviews` line does nothing.
pub fn settle<'a>(
    ctx: &'a crate::agent::toolsets::ToolContext<'a>,
    team: &'a Team,
    background: bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        use crate::agent::subagents::{run_delegation, TaskSpec};
        // `TEAM-15`: a post for a member who is not working wakes it, wherever the loop looks.
        wake(ctx, team).await;
        if !team.lines.iter().any(|l| l.kind == LineKind::Reviews) {
            return;
        }
        let Some(del) = ctx.delegation else { return };
        let db = ctx.db;
        let session = ctx.conversation_id;
        let hand = |agent: &str, task: String, item: Option<usize>| TaskSpec {
            agent: Some(agent.to_string()),
            task,
            output: None,
            because: None,
            style: None,
            item,
            by_loop: true,
        };
        loop {
            if del.parent_run.cancel.is_cancelled() {
                return;
            }
            let rows = db.list_subagent_runs(session).unwrap_or_default();
            let b = boundary(db, session, ctx.assistant_message_id);
            let Some((at, what, reviewer)) = (0..rows.len()).filter(|&i| in_task(&rows[i], &b)).find_map(|i| {
                match standing(team, &rows, i)? {
                    (Standing::Owed, r) => Some((i, None, r)),
                    (Standing::Back(reason), r) => Some((i, Some(reason), r)),
                    _ => None,
                }
            }) else {
                return;
            };
            let run = &rows[at];
            let role = run.member.clone().unwrap_or_default();
            let tell = |state: &str, reason: Option<String>| {
                if let Some(n) = run.item {
                    ctx.sink.emit(crate::agent::AgentEvent::Review {
                        run_id: del.parent_run.id.clone(),
                        item: n - 1,
                        state: state.into(),
                        reason,
                    });
                }
            };
            let sent = match what {
                None => {
                    // The task as the coordinator wrote it: the first run of this hand-over.
                    let mut first = at;
                    while rows[first].task.starts_with(SENT_BACK) {
                        match rows[..first].iter().rposition(|x| x.member.as_deref() == Some(&role)) {
                            Some(p) => first = p,
                            None => break,
                        }
                    }
                    let patch = crate::agent::changes::render_for_model(&crate::agent::changes::change_set(
                        db,
                        &run.child_conversation_id,
                        rows[first].started_at,
                    ));
                    let task = review_task(&role, run.item, &rows[first].task, run.result.as_deref().unwrap_or_default(), &patch);
                    tell("checking", None);
                    // `TEAM-15`: the item goes to the reviewer, and the verdict comes back along the same line.
                    record(db, ctx.sink, session, NewPost {
                        run_id: Some(&run.id),
                        from: &role,
                        to: &reviewer,
                        kind: KIND_REVIEW,
                        item: run.item,
                        text: &clip_text(run.result.as_deref().unwrap_or_default(), POST_CLIP),
                        ..Default::default()
                    });
                    let sent = run_delegation(ctx, vec![hand(&reviewer, task, run.item)], false, None).await.is_ok();
                    let after = db.list_subagent_runs(session).unwrap_or_default();
                    let verdict = |kind: &str, text: &str| {
                        record(db, ctx.sink, session, NewPost {
                            from: &reviewer,
                            to: &role,
                            kind,
                            item: run.item,
                            text,
                            ..Default::default()
                        });
                    };
                    match standing(team, &after, at) {
                        Some((Standing::Back(reason), _)) => {
                            verdict(KIND_SENT_BACK, &reason);
                            tell("sent_back", Some(reason))
                        }
                        Some((Standing::Stuck(reason), _)) => {
                            verdict(KIND_SENT_BACK, &reason);
                            tell("sent_back", Some(reason.clone()));
                            raise(
                                session,
                                &run.id,
                                Problem { item: run.item, text: disagreement(&role, &reviewer, run.item, &reason) },
                            );
                        }
                        Some((Standing::Kept(reason), _)) => {
                            verdict(KIND_REVIEW, &format!("Kept.{}", reason.map(|r| format!(" {r}")).unwrap_or_default()));
                            tell("checked", None)
                        }
                        _ => tell("checked", None),
                    }
                    sent
                }
                Some(reason) => {
                    let task = sent_back_task(&reviewer, run.item, &reason);
                    run_delegation(ctx, vec![hand(&role, task, run.item)], background, None).await.is_ok()
                }
            };
            // A hand-over the loop could not make (the member's persona is gone, say) would be
            // owed for ever: leave it, and let the coordinator read the report as it stands.
            if !sent || db.list_subagent_runs(session).unwrap_or_default().len() == rows.len() {
                return;
            }
        }
    })
}

// ---- TEAM-15: the board (Part I.6) ----
//
// Every hand-over, result, review and verdict is a post, written by the loop where the loop does it
// (`record`, already handed over, so nothing waits on it). A question, an answer, a note and a report to
// the person are written by the `post` tool, which is offered to the coordinator and to members of a
// team run only, and is refused where no line allows it. A post that has not been handed over waits:
// a running member gets it as a steer, the coordinator reads it when `collect_agents` returns, and a
// member who is not working is woken by the loop (`wake`) with the post as its next task, in its own
// chat for the task.

pub const YOU: &str = "you";
pub const KIND_HAND_OVER: &str = "hand-over";
pub const KIND_RESULT: &str = "result";
pub const KIND_REVIEW: &str = "review";
pub const KIND_SENT_BACK: &str = "sent-back";
pub const KIND_ASK: &str = "ask";
pub const KIND_ANSWER: &str = "answer";
pub const KIND_REPORT: &str = "report";
pub const KIND_NOTE: &str = "note";

/// What a post the loop writes keeps of a long report: the Board shows a line, the run keeps the rest.
const POST_CLIP: usize = 1_500;
/// What the `post` tool accepts.
const POST_MAX: usize = 4_000;
/// The longest a question waits for its answer, whatever the run's own limit says.
const ASK_CAP: std::time::Duration = std::time::Duration::from_secs(300);

fn clip_text(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max).collect::<String>()) }
}

/// Write a post and tell the window, as given.
fn publish(db: &Db, sink: &crate::agent::run::AgentEventSink, session: &str, p: NewPost<'_>) -> Option<Post> {
    let post = db.add_post(session, &p).ok()?;
    sink.emit(crate::agent::AgentEvent::Posted { post: post.clone() });
    Some(post)
}

/// A post the loop writes itself. It is a record, not a message: nobody waits to be handed it.
pub fn record(db: &Db, sink: &crate::agent::run::AgentEventSink, session: &str, p: NewPost<'_>) -> Option<Post> {
    publish(db, sink, session, NewPost { delivered: true, ..p })
}

/// Whom `from` may post to: the lines decide (Part I.6). A line of any kind between two members lets
/// them post to each other (a result comes back along a hand-over, a verdict along a review, an answer
/// along a question). The coordinator and the person are the two places every member may reach: the
/// coordinator along its hand-over, the person only along a line that says so.
pub fn may_post(team: &Team, from: &str, to: &str) -> Result<(), String> {
    if from == to {
        return Err("That is you. Post to someone else.".to_string());
    }
    if allowed(team, from, to) {
        return Ok(());
    }
    Err(if to == YOU {
        format!("There is no line from {from} to you in this team. Tell the coordinator instead.")
    } else if from == COORDINATOR {
        format!("There is no line from {COORDINATOR} to {to} in this team. You can post to: {}.", places(team, from).join(", "))
    } else {
        format!("There is no line from {from} to {to} in this team. Ask the coordinator instead.")
    })
}

fn allowed(team: &Team, from: &str, to: &str) -> bool {
    if from == to {
        return false;
    }
    if to == YOU {
        return from == COORDINATOR
            || team.lines.iter().any(|l| matches!(l.kind, LineKind::Reports | LineKind::Asks) && l.from == from && l.to == YOU);
    }
    team.lines.iter().any(|l| {
        matches!(l.kind, LineKind::HandsOver | LineKind::Reviews | LineKind::Asks | LineKind::Reports)
            && ((l.from == from && l.to == to) || (l.from == to && l.to == from))
    })
}

/// Everyone `from` may post to, in the order the team lists them.
pub fn places(team: &Team, from: &str) -> Vec<String> {
    let mut all = vec![COORDINATOR.to_string()];
    all.extend(team.members.iter().map(|m| m.role.clone()));
    all.push(YOU.to_string());
    all.retain(|p| allowed(team, from, p));
    all
}

/// A place the model named, spelled loosely, as the team spells it.
fn place_named(team: &Team, want: &str) -> Option<String> {
    let want = norm(want);
    if want == COORDINATOR {
        return Some(COORDINATOR.to_string());
    }
    if want == YOU {
        return Some(YOU.to_string());
    }
    team.members.iter().find(|m| norm(&m.role) == want).map(|m| m.role.clone())
}

/// Who a run is in a team: the session the team leads and its role there. The coordinator is the run of
/// the session itself; a member is a run with a row that names its role. Any other run is no one.
pub fn seat_of(db: &Db, run: &crate::agent::fleet::RunHandle) -> Option<(String, String)> {
    if run.parent.is_none() {
        return Some((run.conversation_id.clone(), COORDINATOR.to_string()));
    }
    let row = db.get_subagent_run(&run.id).ok().flatten()?;
    Some((row.parent_conversation_id, row.member?))
}

/// The `post` tool for this run, when it is the coordinator of a team's session or one of its members.
pub fn post_spec_for(db: &Db, mem: &MemoryStore, run: &crate::agent::fleet::RunHandle) -> Option<serde_json::Value> {
    let (session, role) = seat_of(db, run)?;
    let team = of_session(db, mem, &session)?;
    let places = places(&team, &role);
    if places.is_empty() {
        return None;
    }
    let mut text = if role == COORDINATOR {
        format!(
            "Send a short message to someone in {}. A member reads it while it works, or when it is woken for it. Set wait to true for a question you cannot go on without: you wait for the answer. To answer a post you were handed, set reply_to to its id. Do not use it for work: hand that over with delegate.",
            team.name
        )
    } else {
        format!(
            "Send a short message to someone in {}, for a question you need answered or a blocker the coordinator should know about. Set wait to true to hold on for the answer, and only when you cannot go on without it. To answer a post you were handed, set reply_to to its id. Do not use it for your result: your final message is the result.",
            team.name
        )
    };
    text.push_str(&format!(" You can post to: {}.", places.join(", ")));
    Some(serde_json::json!({
        "type": "function",
        "function": {
            "name": "post",
            "description": text,
            "parameters": {
                "type": "object",
                "properties": {
                    "to": { "type": "string", "enum": places, "description": "Who gets it. Left out when reply_to says whom you are answering." },
                    "text": { "type": "string", "description": "What to say, in a few sentences." },
                    "reply_to": { "type": "string", "description": "The id of the post you are answering." },
                    "wait": { "type": "boolean", "description": "Wait for the answer to this question." }
                },
                "required": ["text"]
            }
        }
    }))
}

/// Member runs the coordinator is waiting for in the foreground: nobody can answer them while they work,
/// so they do not wait for an answer.
fn foreground() -> &'static Mutex<HashSet<String>> {
    static F: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Mark a member run as one the coordinator waits for, until the guard is dropped.
pub struct InForeground(String);

pub fn in_foreground(run_id: &str) -> InForeground {
    if let Ok(mut set) = foreground().lock() {
        set.insert(run_id.to_string());
    }
    InForeground(run_id.to_string())
}

impl Drop for InForeground {
    fn drop(&mut self) {
        if let Ok(mut set) = foreground().lock() {
            set.remove(&self.0);
        }
    }
}

fn answerable(run_id: &str) -> bool {
    !foreground().lock().map(|s| s.contains(run_id)).unwrap_or(false)
}

/// How a post reads to the one who gets it.
fn post_words(post: &Post) -> String {
    let on = post.item.map_or(String::new(), |n| format!(" on item {n}"));
    let mut out = match post.kind.as_str() {
        KIND_ASK => format!("{} asks you{on} (post {}): {}", post.from_member, post.id, post.text),
        KIND_ANSWER => format!("{} answers{on} (post {}): {}", post.from_member, post.id, post.text),
        _ => format!("{} says{on} (post {}): {}", post.from_member, post.id, post.text),
    };
    if post.kind == KIND_ASK {
        out.push_str(&format!("\nAnswer it with the post tool, reply_to {}.", post.id));
    }
    out
}

/// Take what waits for the coordinator: the words to put in its report, and the posts marked as handed over.
pub fn read_posts(db: &Db, session: &str) -> Option<String> {
    let waiting = db.waiting_posts(session, COORDINATOR).unwrap_or_default();
    if waiting.is_empty() {
        return None;
    }
    let mut out = String::from("Posts for you:");
    for p in &waiting {
        out.push_str(&format!("\n- {}", post_words(p).replace('\n', "\n  ")));
        let _ = db.mark_post_delivered(&p.id);
    }
    Some(out)
}

/// Does something wait for the coordinator? An ask always ends a wait: its sender is holding on.
pub fn posts_waiting(db: &Db, session: &str, only_asks: bool) -> bool {
    db.waiting_posts(session, COORDINATOR).unwrap_or_default().iter().any(|p| !only_asks || p.kind == KIND_ASK)
}

/// Resolve the wait of whoever asked `ask_id`, with `text`. `false` when nobody waits for it any more.
fn resolve_ask(db: &Db, fleet: &crate::agent::fleet::Fleet, ask_id: &str, text: &str) -> bool {
    let Some(ask) = db.get_post(ask_id).ok().flatten() else { return false };
    let Some(run) = ask.run_id.as_deref().and_then(|id| fleet.get(id)) else { return false };
    run.answer(ask_id, crate::agent::fleet::Answer { choices: Vec::new(), text: Some(text.to_string()) })
}

/// The post a woken run was woken for, read from how its task begins (`Message from ui (post p_1): ...`).
fn woken_for(task: &str) -> Option<&str> {
    let rest = task.strip_prefix(MESSAGE)?;
    let inner = rest.split_once("(post ")?.1;
    Some(inner.split_once(')')?.0)
}

fn message_task(post: &Post) -> String {
    let on = post.item.map_or(String::new(), |n| format!(" on item {n}"));
    let mut out = format!("{MESSAGE}{} (post {}){on}: {}", post.from_member, post.id, post.text);
    if post.kind == KIND_ASK {
        out.push_str(&format!(
            "\n\nThis is a question for you. Answer it with the post tool (reply_to {}), or finish with the answer as your final message.",
            post.id
        ));
    }
    out
}

// ---- TEAM-17: talking to a member directly (Part I.8) ----

/// A member of a team the person may speak to, and the persona that speaks as it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addressable {
    pub role: String,
    pub persona_id: String,
}

/// Who a message addressed to `role` goes to: a member of the team that leads this session that the
/// person has a `you talks` line to, and that is a persona that still exists (a plain helper has no
/// voice of its own). `None` otherwise, and the message goes to the coordinator as any other does.
pub fn addressable(db: &Db, mem: &MemoryStore, session: &str, role: &str) -> Option<Addressable> {
    let team = of_session(db, mem, session)?;
    let member = team.members.iter().find(|m| norm(&m.role) == norm(role))?;
    if !team.lines.iter().any(|l| l.kind == LineKind::Talks && l.from == YOU && l.to == member.role) {
        return None;
    }
    let id = member.persona.strip_prefix("persona:")?;
    db.get_persona(id).ok().flatten()?;
    Some(Addressable { role: member.role.clone(), persona_id: id.to_string() })
}

/// `TEAM-18`: a member of the team that leads this session that is a persona that still exists, for a
/// reply nobody asked for. No line from the person is needed: the lines that let it speak up were
/// checked where the choice was made (`team_voices`).
pub fn voice(db: &Db, mem: &MemoryStore, session: &str, role: &str) -> Option<Addressable> {
    let team = of_session(db, mem, session)?;
    let member = team.members.iter().find(|m| norm(&m.role) == norm(role))?;
    let id = member.persona.strip_prefix("persona:")?;
    db.get_persona(id).ok().flatten()?;
    Some(Addressable { role: member.role.clone(), persona_id: id.to_string() })
}

/// What a member who is spoken to knows of the team's work: who it is, and the latest of what passed to
/// and from it on the board (the member "sees the session, its own memory and the board").
pub fn board_note(db: &Db, team: &Team, session: &str, role: &str) -> String {
    let mut out = format!("You are {role} in the team \"{}\". The person is speaking to you directly, so answer them as yourself.", team.name);
    let posts = db.list_posts(session).unwrap_or_default();
    let mut mine: Vec<&Post> = posts.iter().rev().filter(|p| p.from_member == role || p.to_member == role).take(10).collect();
    mine.reverse();
    if !mine.is_empty() {
        out.push_str(" What has passed on the team's board that involves you, oldest first:");
        for p in mine {
            let on = p.item.map_or(String::new(), |n| format!(", item {n}"));
            out.push_str(&format!("\n- {} to {}, {}{on}: {}", p.from_member, p.to_member, p.kind, clip_text(&p.text, 300)));
        }
    }
    out
}

/// The `post` tool (Part I.6): write the post, then hand it over the way its place takes it.
pub async fn post_tool(ctx: &crate::agent::toolsets::ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let db = ctx.db;
    let del = ctx.delegation.ok_or_else(|| "Posting is not available in this run.".to_string())?;
    let run = del.parent_run;
    let (session, from) = seat_of(db, run).ok_or_else(|| "You are not part of a team, so there is no one to post to.".to_string())?;
    let team = of_session(db, ctx.memory, &session).ok_or_else(|| "No team leads this chat any more.".to_string())?;

    let text = args.get("text").and_then(|t| t.as_str()).map(str::trim).filter(|t| !t.is_empty());
    let text = clip_text(text.ok_or_else(|| "`text` is required.".to_string())?, POST_MAX);
    let wait = matches!(args.get("wait"), Some(serde_json::Value::Bool(true)));
    let reply_to = args.get("reply_to").and_then(|r| r.as_str()).map(str::trim).filter(|r| !r.is_empty());
    let answered = match reply_to {
        Some(id) => Some(
            db.get_post(id)
                .ok()
                .flatten()
                .filter(|p| p.conversation_id == session)
                .ok_or_else(|| format!("There is no post {id} in this team's work."))?,
        ),
        None => None,
    };
    let to = match (args.get("to").and_then(|t| t.as_str()).map(str::trim).filter(|t| !t.is_empty()), &answered) {
        (Some(want), _) => place_named(&team, want)
            .ok_or_else(|| format!("Nobody called {want} is in this team. You can post to: {}.", places(&team, &from).join(", ")))?,
        (None, Some(p)) => p.from_member.clone(),
        (None, None) => return Err(format!("Say who gets it. You can post to: {}.", places(&team, &from).join(", "))),
    };
    may_post(&team, &from, &to)?;
    if wait && to == YOU {
        return Err("Nobody can answer you while you work. Put the question in your final message instead.".to_string());
    }
    let item = db.get_subagent_run(&run.id).ok().flatten().and_then(|r| r.item);
    let kind = match &answered {
        Some(p) if p.kind == KIND_ASK => KIND_ANSWER,
        _ if wait => KIND_ASK,
        _ if to == YOU => KIND_REPORT,
        _ => KIND_NOTE,
    };
    let post = publish(
        db,
        ctx.sink,
        &session,
        NewPost {
            run_id: Some(&run.id),
            from: &from,
            to: &to,
            kind,
            item: answered.as_ref().and_then(|p| p.item).or(item),
            reply_to,
            text: &text,
            delivered: to == YOU,
        },
    )
    .ok_or_else(|| "I could not write that post.".to_string())?;
    // Registered before it is handed over, so an answer that comes at once finds someone waiting.
    let waiting = (wait && kind == KIND_ASK).then(|| run.ask(&post.id));

    // The person, in a run nobody watches: nobody can be asked now, so what was said waits for
    // them in the inbox and the run goes on (`TEAM-19`). A post to the person never holds on for
    // an answer (refused above), so it is a report or a blocker, said as such: the person may
    // answer it or leave it.
    if to == YOU && ctx.headless {
        let payload = serde_json::json!({ "post": post.id, "from": from, "item": post.item });
        let line = format!("{from} says: {}", post.text);
        let entry = db
            .add_waiting("waiting-ask", ctx.conversation_id, Some(&from), &payload.to_string(), &line)
            .map_err(|e| e.to_string())?;
        crate::waiting::announce(db, &entry);
        return Ok("Nobody is here, so this is left for the person. Go on with other work, and say in your result that it waits.".to_string());
    }
    // The person: a check-in, put to them with the card the loop already has (`TEAM-11`).
    if to == YOU {
        raise(&session, &post.id, Problem { item: post.item, text: format!("{from}: {}", post.text) });
        return Ok("Told the person. They are asked the next time you hand work over or collect reports.".to_string());
    }
    // An answer reaches the one waiting for it directly.
    let mut handed = false;
    if kind == KIND_ANSWER {
        if let Some(ask) = &answered {
            handed = resolve_ask(db, del.fleet, &ask.id, &post.text);
        }
    }
    let mut said_to = String::new();
    if !handed && to != COORDINATOR {
        let rows = db.list_subagent_runs(&session).unwrap_or_default();
        if let Some(live) = rows.iter().rev().find(|r| r.member.as_deref() == Some(to.as_str()) && r.ended_at.is_none()).and_then(|r| del.fleet.get(&r.id)) {
            live.steer(crate::agent::fleet::Steer { text: post_words(&post), from: crate::agent::fleet::SteerSource::Team });
            handed = true;
        }
    }
    if handed {
        let _ = db.mark_post_delivered(&post.id);
    } else if to == COORDINATOR {
        said_to = "The coordinator reads it when it next collects reports.".to_string();
    } else if from == COORDINATOR {
        // The coordinator's own call is where the loop can wake the member.
        // Boxed: `wake` holds a whole delegation, which would swell every call of the loop's dispatch.
        Box::pin(wake(ctx, &team)).await;
        said_to = format!("{to} is not working now, so it is woken with this.");
    } else {
        said_to = format!("{to} is not working now. The coordinator wakes it with this when it next looks.");
    }

    let Some(waiting) = waiting else {
        return Ok(format!("Posted to {to}. {said_to}").trim().to_string());
    };
    if !answerable(&run.id) {
        run.drop_question(&post.id);
        return Ok(format!("Posted to {to}, but nobody can answer while you work, because the coordinator is waiting for you to finish. Carry on, and say in your result what you assumed."));
    }
    match crate::runtime::proxy::until_cancelled(&run.cancel, tokio::time::timeout(ASK_CAP, waiting)).await {
        Some(Ok(Ok(answer))) => Ok(format!("{to} answered: {}", answer.text.unwrap_or_default())),
        Some(Ok(Err(_))) => Ok(format!("The question to {to} was withdrawn. Carry on without an answer.")),
        Some(Err(_)) => {
            run.drop_question(&post.id);
            Ok(format!("{to} did not answer in time. Carry on, and say in your result what you assumed."))
        }
        None => {
            run.drop_question(&post.id);
            Err("The user stopped this run.".to_string())
        }
    }
}

/// Wake the members who are not working and have a post waiting for them: the post becomes the next task
/// in their chat for this task, as a hand-over the loop makes itself. Only the coordinator's context calls
/// this. A post that cannot be handed over (the task spent its limit, the member is gone) is not left to
/// wait for ever: whoever is holding on for the answer is told.
pub async fn wake(ctx: &crate::agent::toolsets::ToolContext<'_>, team: &Team) {
    use crate::agent::subagents::{run_delegation, TaskSpec};
    let Some(del) = ctx.delegation else { return };
    let (db, session) = (ctx.db, ctx.conversation_id);
    for post in db.waiting_posts_all(session).unwrap_or_default() {
        if !team.members.iter().any(|m| m.role == post.to_member) {
            continue;
        }
        let rows = db.list_subagent_runs(session).unwrap_or_default();
        if let Some(live) = rows.iter().rev().find(|r| r.member.as_deref() == Some(post.to_member.as_str()) && r.ended_at.is_none()) {
            if let Some(run) = del.fleet.get(&live.id) {
                run.steer(crate::agent::fleet::Steer { text: post_words(&post), from: crate::agent::fleet::SteerSource::Team });
                let _ = db.mark_post_delivered(&post.id);
            }
            continue;
        }
        let _ = db.mark_post_delivered(&post.id);
        let task = TaskSpec {
            agent: Some(post.to_member.clone()),
            task: message_task(&post),
            output: None,
            because: None,
            style: None,
            item: post.item,
            by_loop: true,
        };
        if let Err(why) = run_delegation(ctx, vec![task], true, None).await {
            if post.kind == KIND_ASK {
                resolve_ask(db, del.fleet, &post.id, &format!("{} could not be reached: {why}", post.to_member));
            }
        }
    }
}

/// A member's run ended: what it found goes to the coordinator as a post, and a run that was woken to
/// answer a question, and did not answer it with `post`, answers with its final message.
pub fn record_end(db: &Db, fleet: &crate::agent::fleet::Fleet, sink: &crate::agent::run::AgentEventSink, run_id: &str) {
    let Some(run) = db.get_subagent_run(run_id).ok().flatten() else { return };
    let (Some(role), Some(_)) = (run.member.clone(), run.ended_at) else { return };
    let session = run.parent_conversation_id.as_str();
    // A review's words are the verdict `settle` writes.
    if reviewed(&run.task).is_some() && made_by_loop(&run.task) {
        return;
    }
    let text = clip_text(run.result.as_deref().unwrap_or_default(), POST_CLIP);
    record(db, sink, session, NewPost { run_id: Some(&run.id), from: &role, to: COORDINATOR, kind: KIND_RESULT, item: run.item, text: &text, ..Default::default() });
    if let Some(ask_id) = woken_for(&run.task) {
        let answered = db.list_posts(session).unwrap_or_default().iter().any(|p| p.reply_to.as_deref() == Some(ask_id));
        if let (false, Some(ask)) = (answered, db.get_post(ask_id).ok().flatten()) {
            let said_it = run.result.as_deref().unwrap_or_default().trim();
            if run.stop_reason.as_deref() == Some("completed") && !said_it.is_empty() {
                record(db, sink, session, NewPost { run_id: Some(&run.id), from: &role, to: &ask.from_member, kind: KIND_ANSWER, item: ask.item, reply_to: Some(ask_id), text: &clip_text(said_it, POST_MAX), ..Default::default() });
                resolve_ask(db, fleet, ask_id, said_it);
            } else {
                resolve_ask(db, fleet, ask_id, &format!("{role} did not finish, so there is no answer."));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::ways::{Limits, Line};

    fn db_and_personas() -> (Db, String, String) {
        let db = Db::open_in_memory().unwrap();
        let make = |name: &str| {
            db.create_persona(&crate::db::NewPersona {
                name: name.into(),
                system_prompt: format!("You are {name}."),
                model_id: None,
                params_json: None,
                tools_json: None,
                skills_json: None,
                description: None,
                spawnable: false,
            })
            .unwrap()
            .id
        };
        let (atlas, juno) = (make("Atlas"), make("Juno"));
        (db, atlas, juno)
    }

    fn shop(atlas: &str, juno: &str) -> Team {
        let mut t = Team::blank();
        t.slug = "shop-team".into();
        t.name = "Shop team".into();
        t.line = "Builds and checks web shop features".into();
        t.limits = Limits { at_once: 2, ..Limits::default() };
        let member = |role: &str, persona: String, job: &str| Member { role: role.into(), persona, job: job.into(), owns: vec![] };
        t.members = vec![
            member("api", format!("persona:{atlas}"), "Builds the API"),
            member("ui", format!("persona:{juno}"), "Builds the screens"),
            member("docs", "helper".into(), "Keeps the README current"),
        ];
        // The coordinator hands over to api and ui. Nothing is drawn to docs.
        t.lines = vec![
            Line::new(LineKind::HandsOver, "coordinator", "api"),
            Line::new(LineKind::HandsOver, "coordinator", "ui"),
            Line::new(LineKind::Asks, "ui", "api"),
            Line::new(LineKind::Reports, "coordinator", "you"),
            Line::new(LineKind::Talks, "you", "coordinator"),
        ];
        t
    }

    #[test]
    fn the_roster_is_the_members_a_line_reaches() {
        let (_db, a, j) = db_and_personas();
        let team = shop(&a, &j);
        assert_eq!(roster(&team).iter().map(|m| m.role.as_str()).collect::<Vec<_>>(), vec!["api", "ui"]);
    }

    #[test]
    fn a_hand_over_with_no_line_is_refused_and_names_who_may_get_work() {
        let (db, a, j) = db_and_personas();
        let team = shop(&a, &j);
        let err = assign(&db, &team, Some("docs")).unwrap_err();
        assert_eq!(err, "There is no line from coordinator to docs in this team. You can hand work to: api, ui.");
        assert!(assign(&db, &team, Some("nobody")).unwrap_err().contains("Nobody called nobody is in this team"));
        assert!(assign(&db, &team, None).unwrap_err().contains("Say who gets this task"));
        // A drawn line is accepted, spelled loosely, with the persona behind it.
        let got = assign(&db, &team, Some(" API ")).unwrap();
        assert_eq!(got.member.role, "api");
        assert_eq!(got.persona.unwrap().name, "Atlas");
        // A plain helper with a line has no persona.
        let mut team = team;
        team.lines.push(Line::new(LineKind::HandsOver, "coordinator", "docs"));
        assert!(assign(&db, &team, Some("docs")).unwrap().persona.is_none());
    }

    #[test]
    fn delegate_offers_only_the_roster_and_always_runs_in_the_background() {
        let (db, a, j) = db_and_personas();
        let team = shop(&a, &j);
        let spec = delegate_spec(&db, &team, &roster(&team));
        let item = &spec["function"]["parameters"]["properties"]["tasks"]["items"];
        assert_eq!(item["properties"]["agent"]["enum"], serde_json::json!(["api", "ui"]));
        assert!(spec["function"]["parameters"]["properties"].get("background").is_none());
        let text = spec["function"]["description"].as_str().unwrap();
        assert!(text.contains("- api (Atlas): Builds the API") && !text.contains("docs"), "{text}");
    }

    #[test]
    fn the_member_contract_says_role_job_and_whom_it_may_ask() {
        let (_db, a, j) = db_and_personas();
        let team = shop(&a, &j);
        let api = contract(&team, &team.members[0]);
        assert!(api.contains("You are api") && api.contains("Shop team") && api.contains("Builds the API"), "{api}");
        assert!(api.contains("final message is the whole result") && api.contains("use the post tool"), "{api}");
        assert!(!api.contains("You may also ask"), "{api}");
        let ui = contract(&team, &team.members[1]);
        assert!(ui.contains("You may also ask api with it"), "{ui}");
    }

    fn child_run(db: &Db, session: &str, role: &str, turn: Option<&str>, ended: bool) -> SubagentRun {
        let conv = db.create_conversation(role, None, false).unwrap();
        db.set_conversation_parent(&conv.id, session).unwrap();
        let run = uuid::Uuid::new_v4().to_string();
        let row = db.create_subagent_run(&run, session, turn, &conv.id, role, "task").unwrap();
        db.set_subagent_member(&run, role, None).unwrap();
        if ended {
            db.finish_subagent_run(&run, "done", "completed", "built it", 3).unwrap();
        }
        SubagentRun { member: Some(role.into()), ..row }
    }

    fn turn(db: &Db, session: &str, plan: Option<&str>) -> String {
        let m = db
            .append_message(
                session,
                &crate::db::NewMessage {
                    role: "assistant".into(),
                    content: String::new(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: vec![],
                },
            )
            .unwrap();
        db.finalize_message(&m.id, "ok", None, None, None, plan).unwrap();
        m.id
    }

    const OPEN: &str = r#"{"items":[{"text":"Apple Pay","status":"doing"}]}"#;
    const DONE: &str = r#"{"items":[{"text":"Apple Pay","status":"done"}]}"#;

    #[test]
    fn a_second_hand_over_to_a_member_in_the_same_turn_continues_its_chat() {
        let (db, ..) = db_and_personas();
        let session = db.create_conversation("s", None, false).unwrap().id;
        let t1 = turn(&db, &session, None);
        let first = child_run(&db, &session, "api", Some(&t1), true);
        let _ui = child_run(&db, &session, "ui", Some(&t1), true);

        let b = boundary(&db, &session, Some(&t1));
        let found = earlier_run(&db, &session, "api", &b).expect("api has a chat in this task");
        assert_eq!(found.child_conversation_id, first.child_conversation_id, "api's chat, not ui's");
        assert!(earlier_run(&db, &session, "docs", &b).is_none(), "docs never had one");
    }

    #[test]
    fn without_a_plan_the_next_turn_is_a_new_task_with_new_chats() {
        let (db, ..) = db_and_personas();
        let session = db.create_conversation("s", None, false).unwrap().id;
        let t1 = turn(&db, &session, None);
        child_run(&db, &session, "api", Some(&t1), true);
        let t2 = turn(&db, &session, None);
        let b = boundary(&db, &session, Some(&t2));
        assert!(earlier_run(&db, &session, "api", &b).is_none());
    }

    #[test]
    fn an_open_plan_keeps_a_members_chat_across_turns_until_it_is_done() {
        let (db, ..) = db_and_personas();
        let session = db.create_conversation("s", None, false).unwrap().id;
        let t1 = turn(&db, &session, Some(OPEN));
        let first = child_run(&db, &session, "api", Some(&t1), true);
        // Next turn: the plan is still open, so it is the same task.
        let t2 = turn(&db, &session, Some(OPEN));
        let b = boundary(&db, &session, Some(&t2));
        assert_eq!(earlier_run(&db, &session, "api", &b).unwrap().child_conversation_id, first.child_conversation_id);
        // The plan got finished at the end of turn 2; turn 3 starts a new task.
        let _ = turn(&db, &session, Some(DONE));
        let t3 = turn(&db, &session, None);
        let b = boundary(&db, &session, Some(&t3));
        assert!(earlier_run(&db, &session, "api", &b).is_none(), "a finished plan ends the task");
    }

    #[test]
    fn a_continued_chat_carries_what_it_saw_then_the_new_task() {
        let (db, ..) = db_and_personas();
        let session = db.create_conversation("s", None, false).unwrap().id;
        let t1 = turn(&db, &session, None);
        let first = child_run(&db, &session, "api", Some(&t1), true);
        assert!(continued(&db, &first, "Now the refund endpoint").is_none(), "no log, no continuation");
        let log = crate::agent::log::SessionLog::new(&db, &first.child_conversation_id, &first.id);
        log.prompt(&[
            serde_json::json!({ "role": "system", "content": "You are api" }),
            serde_json::json!({ "role": "user", "content": "Build the charge endpoint" }),
        ]);
        log.appended(&serde_json::json!({ "role": "assistant", "content": "Built it." }));
        let m = continued(&db, &first, "Now the refund endpoint").unwrap();
        let roles: Vec<&str> = m.iter().map(|x| x["role"].as_str().unwrap()).collect();
        assert_eq!(roles, vec!["system", "user", "assistant", "user"]);
        assert_eq!(m[3]["content"], "Now the refund endpoint");
    }

    #[test]
    fn at_once_is_respected_per_session() {
        let seat = |session: &str| Seat { session: session.into(), at_once: 2, role: "api".into(), item: None };
        let mut slots = Slots::default();
        let a = seat("s1");
        assert!(slots.has_room(&a));
        slots.take(&a);
        slots.take(&a);
        assert!(!slots.has_room(&a), "two are going, the third waits");
        assert!(slots.has_room(&seat("s2")), "another team's session has its own room");
        slots.free(&a);
        assert!(slots.has_room(&a), "one ended, one may start");
    }

    fn memory() -> (MemoryStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("poiesis_team_{}", uuid::Uuid::new_v4().simple()));
        let mem = MemoryStore::new(&dir).unwrap();
        mem.ensure_ways();
        (mem, dir)
    }

    /// The team is saved, so it has a slug; the session is led by it.
    fn led_session(db: &Db, mem: &MemoryStore, team: Team) -> (String, Team) {
        let team = mem.save_team(Team { slug: String::new(), ..team }, "you", "").unwrap();
        let session = db.create_conversation("s", None, false).unwrap().id;
        crate::agent::leader::set_leader(db, mem, &session, Some(&Leader::Team(team.slug.clone()))).unwrap();
        (session, team)
    }

    /// Members hold the Always rules whatever style the team gives them, and they work in the
    /// team's member style, with their persona's own place in the rules.
    #[test]
    fn a_member_works_the_members_style_and_the_always_rules_hold() {
        use crate::agent::work::Work;
        use crate::memory::ways::{Move, Trigger};
        let (db, a, j) = db_and_personas();
        let (mem, dir) = memory();
        let mut team = shop(&a, &j);
        team.member_style = "mission-command".into();
        assert_eq!(member_style(&team), "mission-command");
        assert_eq!(member_style(&Team { member_style: String::new(), ..team.clone() }), "lean");

        let chat = db.create_conversation("api", None, false).unwrap().id;
        db.set_conversation_persona(&chat, Some(&a), None).unwrap();
        db.set_conversation_style(&chat, Some(member_style(&team))).unwrap();
        let work = Work::resolve(&db, &mem, &chat, "r1", None, None, false);
        assert_eq!(work.preset(), "mission-command", "the team's member style");
        assert!(work.rule(Trigger::CheckFails, Move::StopTheLine).is_some(), "the Always rules hold in a member");
        assert!(work.rule(Trigger::SameProblemTwice, Move::CheckIn).is_some());
        // The same holds when the team names no style.
        db.set_conversation_style(&chat, Some("lean")).unwrap();
        assert!(Work::resolve(&db, &mem, &chat, "r2", None, None, false).rule(Trigger::CheckFails, Move::StopTheLine).is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_coordinator_works_the_teams_style_unless_the_place_says_otherwise() {
        use crate::agent::work::Work;
        let (db, a, j) = db_and_personas();
        let (mem, dir) = memory();
        mem.set_default("global", Some("lean")).unwrap();
        let mut team = shop(&a, &j);
        team.coordinator = format!("persona:{a}");
        team.style = "mission-command".into();
        let (session, _) = led_session(&db, &mem, team);
        let resolve = |id: &str| Work::resolve(&db, &mem, id, "r1", None, None, true).preset();
        assert_eq!(resolve(&session), "mission-command", "the team's style, not everywhere's");

        let shop_project = db.create_project("Shop", None).unwrap();
        mem.set_default(&format!("project:{}", shop_project.id), Some("swarm")).unwrap();
        db.set_conversation_project(&session, Some(&shop_project.id)).unwrap();
        assert_eq!(resolve(&session), "swarm", "the place the chat is in comes first");
        db.set_conversation_style(&session, Some("lean")).unwrap();
        assert_eq!(resolve(&session), "lean", "the chat's own style comes before both");

        let poiesis = db.create_conversation("p", None, false).unwrap().id;
        assert_eq!(resolve(&poiesis), "lean", "a chat no team leads is as before");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_the_session_a_team_leads_gets_the_roster_tools() {
        let (db, a, j) = db_and_personas();
        let (mem, dir) = memory();
        let (session, team) = led_session(&db, &mem, shop(&a, &j));
        let plain = crate::agent::subagents::tool_specs().as_array().unwrap().clone();

        let fleet = crate::agent::fleet::Fleet::new();
        let handle = |conv: &str, parent: Option<&str>| {
            fleet.open(conv, crate::runtime::proxy::CancelFlag::new(), parent.map(str::to_string), usize::from(parent.is_some()))
        };
        let lead = handle(&session, None);
        let mut specs = plain.clone();
        specialise(&mut specs, &db, &mem, &lead, &session);
        let agents = &specs[0]["function"]["parameters"]["properties"]["tasks"]["items"]["properties"]["agent"]["enum"];
        assert_eq!(agents, &serde_json::json!(["api", "ui"]));
        assert!(specs[2]["function"]["parameters"]["properties"]["until"]["enum"].as_array().unwrap().contains(&"any".into()));
        // `TEAM-15`: and it can post, to whoever its lines reach.
        let post = specs.iter().find(|s| s["function"]["name"] == "post").expect("the coordinator may post");
        assert_eq!(post["function"]["parameters"]["properties"]["to"]["enum"], serde_json::json!(["api", "ui", "you"]));

        // A chat no team leads keeps the tools it has.
        let mut specs = plain.clone();
        let poiesis = db.create_conversation("p", None, false).unwrap().id;
        specialise(&mut specs, &db, &mem, &handle(&poiesis, None), &poiesis);
        assert_eq!(specs, plain);
        // A member's own chat is no coordinator: no roster, and no `post` while its run has no
        // member row (a helper that is no member of the team).
        let member_chat = db.create_conversation("api", None, false).unwrap().id;
        db.set_conversation_parent(&member_chat, &session).unwrap();
        db.set_conversation_leader(&member_chat, Some(&format!("team:{}", team.slug)), None).unwrap();
        specialise(&mut specs, &db, &mem, &handle(&member_chat, Some(&lead.id)), &member_chat);
        assert_eq!(specs, plain, "a member never leads");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A team chat once ended with the coordinator doing every part itself and no member ever
    /// started: the coordinator is now told the work is the members', in each of its three cases.
    #[test]
    fn the_coordinator_is_told_the_work_is_the_members() {
        let (db, a, j) = db_and_personas();
        let mut team = shop(&a, &j);
        team.members.push(Member { role: "verifier".into(), persona: "helper".into(), job: "Checks every change".into(), owns: vec![] });
        team.lines.push(Line::new(LineKind::Reviews, "verifier", "api"));
        team.lines.push(Line::new(LineKind::Reviews, "verifier", "ui"));

        let working = coordinator_note(&db, &team, true, false).unwrap();
        assert!(working.starts_with("You coordinate the team \"Shop team\" (Builds and checks web shop features). Its members do the work, not you."), "{working}");
        let atlas = db.get_persona(&a).unwrap().unwrap().name;
        assert!(working.contains(&format!("\n- api ({atlas}): Builds the API")), "{working}");
        assert!(!working.contains("- docs"), "no line reaches docs: {working}");
        assert!(working.contains("verifier looks at the work of api and ui when they finish"), "{working}");
        assert!(working.contains("with delegate") && working.contains("collect_agents"), "{working}");

        let planning = coordinator_note(&db, &team, true, true).unwrap();
        assert!(planning.contains("each item is one member's part") && !planning.contains("with delegate"), "{planning}");

        let off = coordinator_note(&db, &team, false, false).unwrap();
        assert!(off.contains("the members cannot take part") && off.contains("say at the start of your answer"), "{off}");

        // No line from the coordinator to anyone: the work is the coordinator's, and nothing is said.
        team.lines.retain(|l| l.kind != LineKind::HandsOver);
        assert!(coordinator_note(&db, &team, true, false).is_none());
    }

    #[test]
    fn what_was_read_is_not_returned_again_by_until_any() {
        assert!(wants_any(&serde_json::json!({ "until": "any" })));
        assert!(!wants_any(&serde_json::json!({ "until": "all" })) && !wants_any(&serde_json::json!({})));
        assert!(!is_read("run-never-seen"));
        mark_read(["run-read-once".to_string()]);
        assert!(is_read("run-read-once"));
    }

    // ---- TEAM-11 ----

    /// A member run with its task and item, finished the way `done` says (`None`: still going).
    fn run_row(db: &Db, session: &str, role: &str, task: &str, item: Option<usize>, turn: &str, done: Option<(&str, &str)>) {
        let conv = db.create_conversation(role, None, false).unwrap();
        db.set_conversation_parent(&conv.id, session).unwrap();
        let run = uuid::Uuid::new_v4().to_string();
        db.create_subagent_run(&run, session, Some(turn), &conv.id, role, task).unwrap();
        db.set_subagent_member(&run, role, item).unwrap();
        if let Some((stop, result)) = done {
            db.finish_subagent_run(&run, if stop == "completed" { "done" } else { "error" }, stop, result, 2).unwrap();
        }
    }

    /// `ui` looks at `api`'s work. Two rounds are allowed.
    fn reviewed(a: &str, j: &str) -> Team {
        let mut t = shop(a, j);
        t.lines.push(Line::new(LineKind::Reviews, "ui", "api"));
        t.limits.rounds = 2;
        t
    }

    fn rows(db: &Db, session: &str) -> Vec<SubagentRun> {
        db.list_subagent_runs(session).unwrap()
    }

    #[test]
    fn a_reviews_line_names_the_reviewer_and_the_contracts_say_how_a_review_works() {
        let (_db, a, j) = db_and_personas();
        let team = reviewed(&a, &j);
        assert_eq!(reviewer_of(&team, "api").map(|m| m.role.as_str()), Some("ui"));
        assert!(reviewer_of(&team, "ui").is_none() && reviewer_of(&shop(&a, &j), "api").is_none());
        // A coordinator that reviews is not a member: its second look stays the side call.
        let mut t = shop(&a, &j);
        t.lines.push(Line::new(LineKind::Reviews, "coordinator", "api"));
        assert!(reviewer_of(&t, "api").is_none());

        let api = contract(&team, &team.members[0]);
        assert!(api.contains("ui looks at your work when you finish") && api.contains("as a new task in this chat"), "{api}");
        let ui = contract(&team, &team.members[1]);
        assert!(ui.contains("Review item") && ui.contains("VERDICT: CHECKED or SENT BACK") && ui.contains("change nothing"), "{ui}");
        assert!(!contract(&shop(&a, &j), &team.members[0]).contains("looks at your work"), "no line, no words");
    }

    #[test]
    fn what_the_loop_hands_over_is_known_by_how_it_begins() {
        assert!(made_by_loop("Review item 3 by api: x") && made_by_loop("Review the work of api: x") && made_by_loop("Sent back by ui on item 3: x"));
        assert!(!made_by_loop("Build the endpoint") && !made_by_loop("Please review the endpoint"));
        let task = review_task("api", Some(3), "Build it", "Built it.", "1 file changed, +2 -0\n+x");
        assert!(task.starts_with("Review item 3 by api: Build it\n\napi's report:\nBuilt it."), "{task}");
        assert!(task.contains("1 file changed") && task.contains("VERDICT: CHECKED or SENT BACK"));
        let none = review_task("api", None, "Build it", "Built it.", "You have not changed any files in this run.");
        assert!(none.starts_with("Review the work of api:") && none.contains("(no files were changed)"));
    }

    /// The standing of a run is read from the rows alone: owed, kept, sent back, stuck, gone.
    #[test]
    fn a_member_run_is_owed_a_look_then_kept_sent_back_or_stuck_by_what_the_rows_say() {
        let (db, a, j) = db_and_personas();
        let team = reviewed(&a, &j);
        let session = db.create_conversation("s", None, false).unwrap().id;
        let t1 = turn(&db, &session, None);
        let done = |r: &'static str| Some(("completed", r));
        let stand = |at: usize| standing(&team, &rows(&db, &session), at).map(|(s, _)| s);

        // Still working, or not reviewed by anyone: no standing at all.
        run_row(&db, &session, "api", "Build it", Some(1), &t1, None);
        assert_eq!(stand(0), None, "not finished");
        run_row(&db, &session, "docs", "Write it", None, &t1, done("Wrote it."));
        assert_eq!(stand(1), None, "docs has no reviewer");

        db.finish_subagent_run(&rows(&db, &session)[0].id, "done", "completed", "Built it.", 3).unwrap();
        assert_eq!(stand(0), Some(Standing::Owed), "api finished and ui has not looked");

        // The loop's review is a run of ui. While it works, nothing more is owed.
        run_row(&db, &session, "ui", "Review item 1 by api: Build it\n\nreport", Some(1), &t1, None);
        assert_eq!(stand(0), None);
        db.finish_subagent_run(&rows(&db, &session)[2].id, "done", "completed", "VERDICT: SENT BACK\nREASON: no test", 2).unwrap();
        assert_eq!(stand(0), Some(Standing::Back("no test".into())), "one round, one left");

        // api works again and is looked at again; the first run is out of date.
        run_row(&db, &session, "api", "Sent back by ui on item 1: no test", Some(1), &t1, done("Built it with a test."));
        assert_eq!(stand(0), Some(Standing::Gone));
        assert_eq!(stand(3), Some(Standing::Owed));
        run_row(&db, &session, "ui", "Review item 1 by api: Build it\n\nreport", Some(1), &t1, done("VERDICT: SENT BACK\nREASON: still no test"));
        assert_eq!(stand(3), Some(Standing::Stuck("still no test".into())), "two rounds: the coordinator decides");
        let all = rows(&db, &session);
        assert_eq!(
            note(&team, &all, 3).unwrap(),
            "api and ui did not agree on item 1: still no test. Decide what happens with it: accept it, hand it over again with your own instruction, or do it yourself."
        );
        assert!(is_gone(&team, &all, 0) && !is_gone(&team, &all, 3));

        // Kept: the reason is what the coordinator reads, and the coordinator's own second look has nothing to add.
        run_row(&db, &session, "api", "Build it, as decided", Some(2), &t1, done("Built."));
        run_row(&db, &session, "ui", "Review item 2 by api: Build it, as decided", Some(2), &t1, done("VERDICT: CHECKED\nREASON: it works"));
        let all = rows(&db, &session);
        assert_eq!(note(&team, &all, 5).unwrap(), "ui looked at it and kept it. it works");
        assert_eq!(kept(&db, &team, &session, Some(&t1), 2), Some(Some("it works".into())));
        assert_eq!(kept(&db, &team, &session, Some(&t1), 1), None, "item 1 was never kept");
    }

    /// `TEAM-23`: finished tasks are read from the rows: who ran, and how each item's looks went.
    #[test]
    fn finished_tasks_show_who_worked_and_how_the_reviewing_went() {
        let (db, a, j) = db_and_personas();
        let team = reviewed(&a, &j); // ui reviews api, two rounds
        let (mem, dir) = memory();
        let (session, _) = led_session(&db, &mem, team.clone());
        let t1 = turn(&db, &session, None);
        let done = |r: &'static str| Some(("completed", r));
        // Item 1 is sent back, worked again, and sent back again: the limit of two rounds.
        run_row(&db, &session, "api", "Build it", Some(1), &t1, done("Built."));
        run_row(&db, &session, "ui", "Review item 1 by api: Build it", Some(1), &t1, done("VERDICT: SENT BACK\nREASON: no test"));
        run_row(&db, &session, "api", "Sent back by ui on item 1: no test", Some(1), &t1, done("Built it with a test."));
        run_row(&db, &session, "ui", "Review item 1 by api: Build it", Some(1), &t1, done("VERDICT: SENT BACK\nREASON: still no test"));
        // Item 2 is kept at once.
        run_row(&db, &session, "api", "Build more", Some(2), &t1, done("Built."));
        run_row(&db, &session, "ui", "Review item 2 by api: Build more", Some(2), &t1, done("VERDICT: CHECKED\nREASON: fine"));
        // The next turn: docs only, which has no reviewer.
        let t2 = turn(&db, &session, None);
        run_row(&db, &session, "docs", "Write it", None, &t2, done("Wrote it."));
        // A turn whose run is still going is not a finished task.
        let t3 = turn(&db, &session, None);
        run_row(&db, &session, "api", "Later", Some(1), &t3, None);

        let tasks = task_facts(&db, &team, 10);
        assert_eq!(tasks.len(), 2, "the third is still going");
        let first = tasks.iter().find(|t| t.ran.contains(&"api".to_string())).unwrap();
        assert_eq!(first.ran, vec!["api".to_string(), "ui".to_string()]);
        let mut looked = first.looked.clone();
        looked.sort_by_key(|l| l.reviews);
        assert_eq!(
            looked,
            vec![
                Looked { member: "api".into(), reviewer: "ui".into(), reviews: 1, sent_back: false, last_sent_back: false },
                Looked { member: "api".into(), reviewer: "ui".into(), reviews: 2, sent_back: true, last_sent_back: true },
            ]
        );
        let second = tasks.iter().find(|t| t.ran == vec!["docs".to_string()]).unwrap();
        assert!(second.looked.is_empty());
        assert_eq!(task_facts(&db, &team, 1).len(), 1, "at most what was asked for");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `TEAM-23`: speaking to a member there is no line to is remembered once, for a persona only.
    #[test]
    fn speaking_to_a_member_without_a_line_is_remembered_once_for_a_persona() {
        let (db, a, j) = db_and_personas();
        let (mem, dir) = memory();
        let mut team = shop(&a, &j);
        team.lines.push(Line::new(LineKind::Talks, "you", "api")); // a line to api, none to ui
        let (session, _) = led_session(&db, &mem, team);
        assert!(note_asked_without_line(&db, &mem, &session, "ui"));
        assert!(!note_asked_without_line(&db, &mem, &session, "UI"), "once for each chat and member");
        assert!(!note_asked_without_line(&db, &mem, &session, "api"), "there is a line to api");
        assert!(!note_asked_without_line(&db, &mem, &session, "docs"), "a helper has no voice to speak to");
        assert!(!note_asked_without_line(&db, &mem, &session, "nobody"));
        let plain = db.create_conversation("p", None, false).unwrap().id;
        assert!(!note_asked_without_line(&db, &mem, &plain, "ui"), "no team leads it");
        assert_eq!(db.team_events_since(0).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A review that cannot be made or read lets the item stand, as the side call does.
    #[test]
    fn a_review_nobody_can_read_or_that_did_not_finish_lets_the_item_stand() {
        let (db, a, j) = db_and_personas();
        let team = reviewed(&a, &j);
        let session = db.create_conversation("s", None, false).unwrap().id;
        let t1 = turn(&db, &session, None);
        run_row(&db, &session, "api", "Build it", Some(1), &t1, Some(("completed", "Built it.")));
        run_row(&db, &session, "ui", "Review item 1 by api: x", Some(1), &t1, Some(("completed", "Looks fine to me.")));
        assert_eq!(standing(&team, &rows(&db, &session), 0).map(|s| s.0), Some(Standing::Kept(None)));
        run_row(&db, &session, "api", "Another", Some(2), &t1, Some(("completed", "Built.")));
        run_row(&db, &session, "ui", "Review item 2 by api: x", Some(2), &t1, Some(("error", "VERDICT: SENT BACK\nREASON: nope")));
        assert_eq!(standing(&team, &rows(&db, &session), 2).map(|s| s.0), Some(Standing::Kept(None)), "a reviewer that failed says nothing");
        // A send-back with no reason is not a verdict.
        run_row(&db, &session, "api", "Third", Some(3), &t1, Some(("completed", "Built.")));
        run_row(&db, &session, "ui", "Review item 3 by api: x", Some(3), &t1, Some(("completed", "VERDICT: SENT BACK")));
        assert_eq!(standing(&team, &rows(&db, &session), 4).map(|s| s.0), Some(Standing::Kept(None)));
    }

    #[test]
    fn a_member_that_did_not_finish_is_raised_once_for_the_person_and_nothing_else_is() {
        let (db, ..) = db_and_personas();
        let session = format!("session-{}", uuid::Uuid::new_v4());
        let real = db.create_conversation("s", None, false).unwrap().id;
        let t = turn(&db, &real, None);
        run_row(&db, &real, "api", "Build it", Some(2), &t, Some(("timeout", "Got this far.")));
        run_row(&db, &real, "ui", "Build it", None, &t, Some(("completed", "Built.")));
        for row in rows(&db, &real) {
            failed(&session, &row);
            failed(&session, &row);
        }
        assert_eq!(take_problems(&session), vec![Problem { item: Some(2), text: "api did not finish item 2.".into() }]);
        assert!(take_problems(&session).is_empty(), "taken once");
        assert!(take_problems("another-session").is_empty());
    }

    // ---- TEAM-12 ----

    /// A finished team task: a coordinator turn that handed work to `api`, with the usage rows
    /// each run wrote. `tokens` is the (prompt, output) of the coordinator and of the member.
    fn finished_task(db: &Db, session: &str, tokens: ((u64, u64), (u64, u64))) {
        let pause = || std::thread::sleep(std::time::Duration::from_millis(3));
        let t = turn(db, session, None);
        pause();
        let conv = db.create_conversation("api", None, false).unwrap();
        db.set_conversation_parent(&conv.id, session).unwrap();
        let run = uuid::Uuid::new_v4().to_string();
        db.create_subagent_run(&run, session, Some(&t), &conv.id, "api", "task").unwrap();
        db.set_subagent_member(&run, "api", None).unwrap();
        db.record_run_usage(&run, &conv.id, "claude-sonnet-4-5", "cloud", tokens.1 .0, tokens.1 .1, 3).unwrap();
        db.finish_subagent_run(&run, "done", "completed", "built it", 3).unwrap();
        pause();
        db.record_run_usage(&format!("coordinator-{run}"), session, "claude-sonnet-4-5", "cloud", tokens.0 .0, tokens.0 .1, 5).unwrap();
        pause();
    }

    /// `TEAM-12`: the estimate is the rough one from prices until the team has three finished
    /// tasks, and theirs from then on.
    #[test]
    fn the_estimate_switches_from_rough_to_past_tasks_at_three() {
        let (db, a, j) = db_and_personas();
        let (mem, dir) = memory();
        let (session, team) = led_session(&db, &mem, shop(&a, &j));
        let cost = |db: &Db| cost_card(db, &team, "claude-sonnet-4-5", "cloud").text;

        let rough = cost(&db);
        assert!(rough.contains("A task like this costs roughly $0.60 to $1.80."), "{rough}");

        // Each task: coordinator and api, $0.30 + $0.30, then $0.60 + $0.60, then $0.15 + $0.15.
        finished_task(&db, &session, ((100_000, 0), (100_000, 0)));
        finished_task(&db, &session, ((200_000, 0), (200_000, 0)));
        assert_eq!(past_costs(&db, &team.slug).len(), 2);
        assert!(cost(&db).contains("costs roughly"), "two tasks are not enough");

        finished_task(&db, &session, ((50_000, 0), (50_000, 0)));
        assert_eq!(past_costs(&db, &team.slug).len(), 3);
        let text = cost(&db);
        assert!(text.contains("A task like this has cost about $0.30 to $1.20."), "{text}");
        assert!(!text.contains("roughly"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// A task that is still going (a member without an end) is not one the team has finished.
    #[test]
    fn a_task_still_going_is_not_a_past_task() {
        let (db, a, j) = db_and_personas();
        let (mem, dir) = memory();
        let (session, team) = led_session(&db, &mem, shop(&a, &j));
        finished_task(&db, &session, ((100_000, 0), (100_000, 0)));
        let t = turn(&db, &session, None);
        run_row(&db, &session, "api", "Build it", None, &t, None);
        assert_eq!(past_costs(&db, &team.slug).len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_estimate_is_free_for_local_models_and_unknown_for_a_model_with_no_price() {
        let rough = estimate(&[], 4, "gemma-3", "local").unwrap();
        assert_eq!((rough.low, rough.high), (0.0, 0.0));
        assert!(estimate(&[], 4, "some-new-model", "cloud").is_none());
        // Past tasks need no price: they are what was spent.
        assert!(estimate(&[0.1, 0.2, 0.3], 4, "some-new-model", "cloud").is_some_and(|e| !e.rough));
    }

    #[test]
    fn the_card_says_who_works_what_it_costs_and_where_it_stops() {
        let (db, a, j) = db_and_personas();
        let team = shop(&a, &j);
        let card = cost_card(&db, &team, "anthropic/claude-sonnet-4-5", "cloud");
        assert_eq!(card.title, "Shop team is about to start");
        assert_eq!(
            card.text,
            "4 members: Poiesis (claude-sonnet-4-5), Atlas (claude-sonnet-4-5), Juno (claude-sonnet-4-5), docs (claude-sonnet-4-5).\n\nA task like this costs roughly $0.60 to $1.80.\n\nIt stops at $2.00 or after 90 minutes."
        );
        let local = cost_card(&db, &team, "gemma-3", "local").text;
        assert!(local.contains("Poiesis (local)") && local.contains("costs nothing"), "{local}");
        let mut no_cap = team.clone();
        no_cap.limits.budget = 0.0;
        assert!(cost_card(&db, &no_cap, "gemma-3", "local").text.ends_with("It stops after 90 minutes."));
        assert!(!card.text.contains('\u{2014}'), "no em dash in UI text");
    }

    /// Start remembers the answer for this session and team; a hand-over to a team asks again;
    /// the box is `ask_cost: no`, saved as a version of the team.
    #[test]
    fn start_is_remembered_until_a_hand_over_and_the_box_writes_the_team_file() {
        let (db, a, j) = db_and_personas();
        let (mem, dir) = memory();
        let (session, team) = led_session(&db, &mem, shop(&a, &j));
        assert!(!cost_asked(&db, &session, &team.slug));

        cost_answered(&db, &mem, &session, &team, false);
        assert!(cost_asked(&db, &session, &team.slug));
        assert!(mem.team(&team.slug).unwrap().limits.ask_cost, "not ticked: still asks in the next session");
        assert!(!cost_asked(&db, &session, "another-team"));

        crate::agent::leader::set_leader(&db, &mem, &session, Some(&Leader::Team(team.slug.clone()))).unwrap();
        assert!(!cost_asked(&db, &session, &team.slug), "a hand-over asks again");

        let before = mem.team(&team.slug).unwrap().version;
        cost_answered(&db, &mem, &session, &team, true);
        let after = mem.team(&team.slug).unwrap();
        assert!(!after.limits.ask_cost);
        assert_eq!(after.version, before + 1, "a version of its own, so History shows it");
        std::fs::remove_dir_all(dir).ok();
    }

    /// The turns of one task share one allowance; the next task, or the person going on after a
    /// stop, gets a fresh one.
    #[test]
    fn one_allowance_per_task_and_a_fresh_one_after_a_stop() {
        let (db, a, j) = db_and_personas();
        let (mem, dir) = memory();
        let (session, _) = led_session(&db, &mem, shop(&a, &j));
        let t1 = turn(&db, &session, Some(OPEN));
        let first = start_task(&db, &mem, &session, Some(&t1)).unwrap();
        assert!(Arc::ptr_eq(&first, &allowance_of(&session).unwrap()));

        // The plan is still open, so this turn is the same task.
        let t2 = turn(&db, &session, None);
        let same = start_task(&db, &mem, &session, Some(&t2)).unwrap();
        assert!(Arc::ptr_eq(&first, &same));

        // The limit is reached: no hand-over, and the next message starts a new allowance.
        same.charge("coordinator", 99.0);
        assert!(spent(&session).unwrap().contains("No more work is handed out"));
        let t3 = turn(&db, &session, None);
        let fresh = start_task(&db, &mem, &session, Some(&t3)).unwrap();
        assert!(!Arc::ptr_eq(&first, &fresh));
        assert!(spent(&session).is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    /// Everything but a team's own session has no allowance.
    #[test]
    fn a_session_nobody_leads_has_no_allowance() {
        let (db, ..) = db_and_personas();
        let (mem, dir) = memory();
        let plain = db.create_conversation("plain", None, false).unwrap().id;
        assert!(start_task(&db, &mem, &plain, None).is_none());
        assert!(spent(&plain).is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    // ---- TEAM-15 ----

    #[test]
    fn a_post_goes_only_along_a_line_and_the_refusal_names_the_way_round() {
        let (_db, a, j) = db_and_personas();
        let team = shop(&a, &j);
        // The result comes back along the hand-over, the answer along the question.
        assert!(may_post(&team, "api", "coordinator").is_ok());
        assert!(may_post(&team, "coordinator", "ui").is_ok());
        assert!(may_post(&team, "ui", "api").is_ok(), "ui asks api");
        assert!(may_post(&team, "api", "ui").is_ok(), "and api answers ui along the same line");
        // docs has no line at all; ui has no line to the person.
        assert_eq!(
            may_post(&team, "ui", "docs").unwrap_err(),
            "There is no line from ui to docs in this team. Ask the coordinator instead."
        );
        assert_eq!(
            may_post(&team, "ui", "you").unwrap_err(),
            "There is no line from ui to you in this team. Tell the coordinator instead."
        );
        assert!(may_post(&team, "coordinator", "you").is_ok(), "the coordinator always reports to the person");
        let to_docs = may_post(&team, "coordinator", "docs").unwrap_err();
        assert!(to_docs.starts_with("There is no line from coordinator to docs in this team. You can post to: api, ui"), "{to_docs}");
        assert!(may_post(&team, "api", "api").is_err());
    }

    #[test]
    fn places_are_who_a_line_reaches_and_are_spelled_as_the_team_spells_them() {
        let (_db, a, j) = db_and_personas();
        let team = shop(&a, &j);
        assert_eq!(places(&team, "coordinator"), vec!["api", "ui", "you"]);
        assert_eq!(places(&team, "api"), vec!["coordinator", "ui"]);
        assert_eq!(places(&team, "ui"), vec!["coordinator", "api"]);
        assert_eq!(place_named(&team, " UI ").as_deref(), Some("ui"));
        assert_eq!(place_named(&team, "Coordinator").as_deref(), Some("coordinator"));
        assert_eq!(place_named(&team, "nobody"), None);
    }

    fn make_post(db: &Db, session: &str, from: &str, to: &str, kind: &str, text: &str) -> Post {
        db.add_post(session, &NewPost { from, to, kind, text, ..Default::default() }).unwrap()
    }

    #[test]
    fn the_board_keeps_posts_in_order_and_remembers_what_was_handed_over() {
        let (db, ..) = db_and_personas();
        let session = db.create_conversation("s", None, false).unwrap().id;
        let first = make_post(&db, &session, "api", "coordinator", KIND_NOTE, "The table is locked.");
        let second = make_post(&db, &session, "ui", "api", KIND_ASK, "Which field?");
        let kept = db
            .add_post(&session, &NewPost { from: "coordinator", to: "api", kind: KIND_HAND_OVER, item: Some(2), text: "Build it", delivered: true, ..Default::default() })
            .unwrap();
        assert_eq!(db.list_posts(&session).unwrap().iter().map(|p| p.id.clone()).collect::<Vec<_>>(), vec![first.id.clone(), second.id.clone(), kept.id.clone()]);
        // A record the loop wrote waits for nobody; the others wait for whoever they are for.
        assert_eq!(db.waiting_posts(&session, "coordinator").unwrap(), vec![first.clone()]);
        assert_eq!(db.waiting_posts(&session, "api").unwrap(), vec![second.clone()], "the hand-over to api is a record");
        assert_eq!(db.waiting_posts_all(&session).unwrap().len(), 2);
        db.mark_post_delivered(&first.id).unwrap();
        assert!(db.waiting_posts(&session, "coordinator").unwrap().is_empty());
        assert!(db.get_post(&first.id).unwrap().unwrap().delivered_at.is_some());
        // They go with the session.
        db.delete_conversation(&session).unwrap();
        assert!(db.list_posts(&session).unwrap().is_empty());
    }

    #[test]
    fn the_coordinator_reads_its_posts_once_and_a_question_says_how_to_answer() {
        let (db, ..) = db_and_personas();
        let session = db.create_conversation("s", None, false).unwrap().id;
        assert!(read_posts(&db, &session).is_none());
        let note = make_post(&db, &session, "api", "coordinator", KIND_NOTE, "The table is locked.");
        let ask = make_post(&db, &session, "ui", "coordinator", KIND_ASK, "Which field is the id?");
        assert!(posts_waiting(&db, &session, false));
        assert!(posts_waiting(&db, &session, true), "a question is waiting");
        let said = read_posts(&db, &session).unwrap();
        assert!(said.starts_with("Posts for you:"), "{said}");
        assert!(said.contains(&format!("api says (post {}): The table is locked.", note.id)), "{said}");
        assert!(said.contains(&format!("ui asks you (post {}): Which field is the id?", ask.id)), "{said}");
        assert!(said.contains(&format!("Answer it with the post tool, reply_to {}.", ask.id)), "{said}");
        assert!(read_posts(&db, &session).is_none(), "read once");
        assert!(!posts_waiting(&db, &session, false));
    }

    #[test]
    fn a_woken_run_knows_which_post_woke_it() {
        let (db, ..) = db_and_personas();
        let session = db.create_conversation("s", None, false).unwrap().id;
        let ask = db
            .add_post(&session, &NewPost { from: "ui", to: "api", kind: KIND_ASK, item: Some(2), text: "Which field?", ..Default::default() })
            .unwrap();
        let task = message_task(&ask);
        assert!(task.starts_with(&format!("Message from ui (post {}) on item 2: Which field?", ask.id)), "{task}");
        assert!(task.contains("This is a question for you"), "{task}");
        assert_eq!(woken_for(&task), Some(ask.id.as_str()));
        assert!(made_by_loop(&task), "the coordinator's turn cap does not count it");
        assert_eq!(woken_for("Build the charge endpoint"), None);
        let note = make_post(&db, &session, "ui", "api", KIND_NOTE, "FYI");
        assert!(!message_task(&note).contains("This is a question"));
    }

    // ---- TEAM-22 ----

    /// A helper run of the session, as `delegate` writes it: no member, no item.
    fn helper_run(db: &Db, session: &str, agent: &str, task: &str) {
        let conv = db.create_conversation(agent, None, false).unwrap();
        db.set_conversation_parent(&conv.id, session).unwrap();
        db.create_subagent_run(&uuid::Uuid::new_v4().to_string(), session, None, &conv.id, agent, task).unwrap();
    }

    /// `TEAM-22-T`: the helpers a session used become members, a persona by its name and a plain
    /// helper with its first task as the job; the leader coordinates; the lines are the hand-overs.
    #[test]
    fn helpers_a_session_used_become_a_team_with_the_hand_overs_as_lines() {
        let (db, atlas, _juno) = db_and_personas();
        let (mem, dir) = memory();
        let session = db.create_conversation("Shop rebuild", None, false).unwrap().id;
        db.set_conversation_persona(&session, Some(&atlas), None).unwrap();
        helper_run(&db, &session, "Juno", "Check the copy on the cart page");
        helper_run(&db, &session, "general", "Find every place the old price is used\nand list the files");
        helper_run(&db, &session, "Juno", "Check the checkout page too");
        helper_run(&db, &session, "general", "Rename the helper");

        let team = keep_as_team(&db, &mem, &session).expect("two helper types were used");
        assert_eq!(team.name, "Shop rebuild team");
        assert_eq!(team.coordinator, format!("persona:{atlas}"), "the session's leader coordinates");
        let roles: Vec<(&str, &str, &str)> = team.members.iter().map(|m| (m.role.as_str(), m.persona.as_str(), m.job.as_str())).collect();
        assert_eq!(roles.len(), 2, "each helper type once: {roles:?}");
        assert_eq!(roles[0].0, "juno");
        assert!(roles[0].1.starts_with("persona:"), "a persona plays its own name");
        assert_eq!(roles[0].2, "", "a persona needs no job line");
        assert_eq!(roles[1], ("helper", "helper", "Find every place the old price is used"));
        let hands: Vec<String> = team.lines.iter().filter(|l| l.kind == LineKind::HandsOver).map(|l| l.text()).collect();
        assert_eq!(hands, ["coordinator hands-over juno", "coordinator hands-over helper"]);
        assert!(team.lines.iter().any(|l| l.kind == LineKind::Talks && l.from == "you"), "the fixed lines stay");
        assert!(team.slug.is_empty(), "it is not saved: the person edits it first");
        std::fs::remove_dir_all(dir).ok();
    }

    /// `TEAM-22-T`: not offered for one helper, for agent apps and the loop's own work, or when a
    /// team already leads the session.
    #[test]
    fn keeping_helpers_is_offered_only_when_there_is_a_team_to_make() {
        let (db, _atlas, _juno) = db_and_personas();
        let (mem, dir) = memory();
        let session = db.create_conversation("s", None, false).unwrap().id;
        helper_run(&db, &session, "general", "One thing");
        assert!(keep_as_team(&db, &mem, &session).is_none(), "one helper is no team");

        // The loop's own hand-over ("Review item 3 by ...") is not a helper the model chose.
        helper_run(&db, &session, "Juno", "Review item 3 by api: the report");
        assert!(keep_as_team(&db, &mem, &session).is_none());

        helper_run(&db, &session, "Juno", "Check the copy");
        assert!(keep_as_team(&db, &mem, &session).is_some());

        let child = db.create_conversation("c", None, false).unwrap().id;
        db.set_conversation_parent(&child, &session).unwrap();
        assert!(keep_as_team(&db, &mem, &child).is_none(), "a helper's chat is never a session");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn roles_are_valid_and_never_repeat() {
        assert_eq!(role_for("general", &[]), "helper");
        assert_eq!(role_for("Code Reviewer", &[]), "code-reviewer");
        assert_eq!(role_for("you", &[]), "helper", "a reserved word is not a role");
        assert_eq!(role_for("general", &["helper".to_string()]), "helper-2");
        assert_eq!(role_for("\u{4e2d}\u{6587}", &[]), "helper", "nothing usable left");
        assert!(role_for(&"x".repeat(60), &[]).len() <= 24);
        assert_eq!(job_from("a \u{b7} b"), "a - b");
    }
}
