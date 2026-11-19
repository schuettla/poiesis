//! `WRK-19`: the organization evolves, by proposal.
//!
//! What already happens is the evidence, and nothing is asked of a model:
//!
//! - **"Not this time"** on the same move, three times in 30 days. Said about a way
//!   I learned, I offer to retire it. Said about a preset's move, I offer to take it
//!   out of the preset, or, when every one of them was in one kind of work, to branch
//!   off a preset that works without it there (`IV.6`).
//! - **File changes put back** in three conversations of one kind of work: a second
//!   look before an item counts as done, for that kind.
//! - **A preset that does clearly better** than working ad hoc in one kind of work:
//!   offered as where that kind starts.
//! - **A way being tried out** that has had its tasks or its days: one line on whether
//!   it helped, and Keep or Retire.
//!
//! Each becomes a row in `change_proposals` with the target `way`, answered in the
//! Self view's How I work tab. A subject that was ever proposed is not proposed again,
//! so "No" stays no. A locked preset is skipped entirely, and the base rules are never
//! touched. Nothing here writes a file by itself: [`apply`] does, once someone said yes
//! (or the autonomy class says it may), and hands back how to put it back ([`Undo`]).
//!
//! `TEAM-23`: teams evolve the same way (`TEAMS_PLAN` Part VII), from what the team's own
//! finished tasks and chats show ([`TeamFacts`]): a member that got no work, a reviewer that
//! sent most of one member's items back, the rounds limit hit again and again, a member the
//! person tried to speak to without a line, a way of working the person keeps choosing. Those
//! proposals have the target `team` and are answered in Settings, Personas & teams, on the
//! team they are about. A locked team is skipped entirely; the person's own teams start locked.

use serde::{Deserialize, Serialize};

use crate::agent::episode::DONE;
use crate::agent::work::Signal;
use crate::db::{now_ms, Db};
use crate::memory::ways::{sentence, Line, LineKind, Move, PresetRule, Scope, ScopeCtx, Status, Team, Trigger, AD_HOC};
use crate::memory::MemoryStore;

/// The `change_proposals.target` for these.
pub const TARGET: &str = "way";
/// `TEAM-23`: the target of a proposal about a team.
pub const TEAM_TARGET: &str = "team";
/// The `session_events.kind` of what a team's chats show evolution (`TEAM-23`).
pub const TEAM_EVENT: &str = "team";
/// `TEAM-23`: the person tried to speak to a member they have no line to.
pub const ASKED_WITHOUT_LINE: &str = "asked-without-line";
/// `TEAM-23`: the person chose a way of working in a chat a team leads.
pub const STYLE_CHOSEN: &str = "style-chosen";
/// `TEAM-23`: finished tasks that are read for one team's signals.
const TASKS_READ: usize = 10;
/// `TEAM-23`: tasks in a row without work, before a member is offered to be taken out.
const IDLE_TASKS: usize = 5;
/// `TEAM-23`: the rounds the coordinator is offered to decide after.
const SHORT_ROUNDS: u32 = 2;
/// The autonomy class.
pub const CLASS: &str = "ways";
/// Times it has to happen before something is proposed.
pub const NEEDED: usize = 3;
const DAY_MS: i64 = 86_400_000;
/// How far back signals count.
pub const WINDOW_MS: i64 = 30 * DAY_MS;
/// How far back conversations count when a preset is judged, and a trial is compared.
const JUDGE_MS: i64 = 90 * DAY_MS;
/// Finished conversations under each side before a preset is judged against ad hoc.
const JUDGED: usize = 5;
/// The conversations before a trial that it is compared with.
const BEFORE: usize = 5;
const RECENT_KEY: &str = "ways.recent";
/// How long what evolution did lately stays in the tab.
const RECENT_MS: i64 = 30 * DAY_MS;

/// What a proposal would change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Change {
    /// A new way, narrowed to `scope`, with the conversations that led to it.
    AddWay {
        when: Trigger,
        #[serde(rename = "move")]
        mv: Move,
        scope: String,
        #[serde(default)]
        evidence: Vec<String>,
    },
    RetireWay {
        slug: String,
    },
    /// Take one move out of a preset.
    PresetRule {
        preset: String,
        when: Trigger,
        #[serde(rename = "move")]
        mv: Move,
    },
    Default {
        scope: String,
        preset: String,
    },
    /// A preset that works as `parent` does, without one move, for one kind of work.
    Offspring {
        parent: String,
        name: String,
        kind: String,
        when: Trigger,
        #[serde(rename = "move")]
        mv: Move,
    },
    /// A way being tried out has had its span: `better`, `same`, `worse` or `unknown`.
    TrialEnd {
        slug: String,
        verdict: String,
        /// The days ran out before the tasks came.
        early: bool,
    },
    /// `TEAM-23`: a member that got no work leaves the team, with the lines that named it.
    TeamTakeOut { team: String, role: String },
    /// `TEAM-23`: `from` may ask `to` before handing work in.
    TeamAsks { team: String, from: String, to: String },
    /// `TEAM-23`: the coordinator decides after this many rounds between two members.
    TeamRounds { team: String, rounds: u32 },
    /// `TEAM-23`: the person may talk with this member directly.
    TeamTalks { team: String, role: String },
    /// `TEAM-23`: the way of working the team has unless the person says otherwise.
    TeamStyle { team: String, style: String },
}

impl Change {
    /// The `change_proposals.target` this is stored under.
    pub fn target(&self) -> &'static str {
        match self {
            Change::TeamTakeOut { .. }
            | Change::TeamAsks { .. }
            | Change::TeamRounds { .. }
            | Change::TeamTalks { .. }
            | Change::TeamStyle { .. } => TEAM_TARGET,
            _ => TARGET,
        }
    }
}

/// Is this a target evolution answers (`way` and `team`)?
pub fn is_target(target: &str) -> bool {
    target == TARGET || target == TEAM_TARGET
}

/// A change worth proposing, with the words to propose it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// What it is about. A subject is only ever proposed once.
    pub subject: String,
    pub change: Change,
    /// "I'd like to work differently:"
    pub lead: String,
    /// What it would be, in a sentence.
    pub quote: String,
    /// "Why: ..."
    pub why: String,
}

/// One finished conversation, as far as evolution needs it.
#[derive(Debug, Clone)]
pub struct Ended {
    pub id: String,
    pub kind: Option<String>,
    pub project: Option<String>,
    pub done: bool,
    /// The way of working the person chose for it, if they did.
    pub style: Option<String>,
    pub at: i64,
}

/// A "not this time" or other signal, with the conversation it happened in.
#[derive(Debug, Clone)]
pub struct Sig {
    pub conversation: String,
    pub signal: Signal,
}

/// `TEAM-23`: one item that went to a reviewer, and how the reviewing went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Looked {
    pub member: String,
    pub reviewer: String,
    /// How many times the reviewer looked at it before it was kept or left to the coordinator.
    pub reviews: u32,
    /// It was sent back at least once.
    pub sent_back: bool,
    /// The last look sent it back (it was not kept).
    pub last_sent_back: bool,
}

/// `TEAM-23`: what one finished task of a team showed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskFacts {
    /// When its last run ended.
    pub at: i64,
    /// The roles that had a run in it (a reviewer's look is a run).
    pub ran: Vec<String>,
    pub looked: Vec<Looked>,
}

/// `TEAM-23`: everything evolution reads about one team.
#[derive(Debug, Clone, Default)]
pub struct TeamFacts {
    pub slug: String,
    /// Finished tasks, newest first.
    pub tasks: Vec<TaskFacts>,
    /// When each role's place began: the oldest version it has been in without a gap.
    pub since: Vec<(String, i64)>,
    /// Chats the team led where the person chose a way of working, the latest choice in each:
    /// `(conversation, style)`. Recorded when the person chooses (`team::note_style_chosen`).
    pub styles: Vec<(String, String)>,
    /// Chats where the person tried to speak to a member they have no line to: `(conversation, role)`.
    pub asked: Vec<(String, String)>,
}

/// Everything evolution reads, so the rules can be tested without a database.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub signals: Vec<Sig>,
    /// Conversations where file changes were put back.
    pub undone: Vec<String>,
    pub ended: Vec<Ended>,
    /// `TEAM-23`: one entry for each team that is not locked.
    pub teams: Vec<TeamFacts>,
}

impl Facts {
    pub fn gather(db: &Db, now: i64) -> Facts {
        let signals = db
            .way_signals_since(now - WINDOW_MS)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(conversation, _, json)| Some(Sig { conversation, signal: serde_json::from_str(&json).ok()? }))
            .collect();
        let ended = db
            .ended_conversations_since(now - JUDGE_MS)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, kind, project, outcome, style, at)| Ended {
                id,
                kind: kind.filter(|k| k != "unknown"),
                project,
                done: outcome == DONE,
                style,
                at,
            })
            .collect();
        Facts { signals, undone: db.undone_conversations_since(now - WINDOW_MS).unwrap_or_default(), ended, teams: Vec::new() }
    }

    /// `TEAM-23`: what each team that is not locked has shown lately. A locked team is not read at all.
    pub fn gather_teams(db: &Db, mem: &MemoryStore, now: i64) -> Vec<TeamFacts> {
        mem.teams()
            .into_iter()
            .map(|r| r.team)
            .filter(|t| !t.locked)
            .map(|team| {
                // What the person did in the team's chats lately, oldest first.
                let events: Vec<(String, serde_json::Value)> = db
                    .team_events_since(now - WINDOW_MS)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(conversation, _, json)| {
                        let v: serde_json::Value = serde_json::from_str(&json).ok()?;
                        (v["team"] == team.slug.as_str()).then_some((conversation, v))
                    })
                    .collect();
                let field = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
                // The way of working each chat was last given by the person (not by a scheduled task).
                let mut styles: Vec<(String, String)> = Vec::new();
                for (conversation, v) in events.iter().filter(|(_, v)| v["what"] == STYLE_CHOSEN) {
                    styles.retain(|(c, _)| c != conversation);
                    styles.push((conversation.clone(), field(v, "style")));
                }
                styles.retain(|(_, s)| !s.is_empty());
                let asked = events
                    .iter()
                    .filter(|(_, v)| v["what"] == ASKED_WITHOUT_LINE)
                    .map(|(conversation, v)| (conversation.clone(), field(v, "member")))
                    .filter(|(_, role)| !role.is_empty())
                    .collect();
                TeamFacts {
                    tasks: crate::agent::team::task_facts(db, &team, TASKS_READ),
                    since: member_since(mem, &team),
                    slug: team.slug,
                    styles,
                    asked,
                }
            })
            .collect()
    }

    fn kind_of(&self, conversation: &str) -> Option<&str> {
        self.ended.iter().find(|e| e.id == conversation).and_then(|e| e.kind.as_deref())
    }
}

/// A kind of work in the person's words, short enough for a name.
fn kind_label(kind: &str) -> &str {
    match kind {
        "report" => "reports",
        other => other,
    }
}

fn distinct(mut v: Vec<&str>) -> Vec<String> {
    v.sort_unstable();
    v.dedup();
    v.into_iter().map(str::to_string).collect()
}

/// "For mail: " before a way that is narrowed, nothing before one that is not.
fn reach(scope: &Scope) -> String {
    match scope {
        Scope::Everywhere => String::new(),
        Scope::Kind(k) => format!("For {}: ", kind_label(k)),
        Scope::Project(_) => "For one project: ".into(),
        Scope::Persona(_) => "For one persona: ".into(),
    }
}

/// Every proposal the facts support, minus the subjects already proposed (`known`).
pub fn candidates(facts: &Facts, mem: &MemoryStore, known: &dyn Fn(&str) -> bool) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut push = |c: Candidate| {
        if !known(&c.subject) && !out.iter().any(|o| o.subject == c.subject) {
            out.push(c);
        }
    };
    skipped(facts, mem, &mut push);
    undone(facts, mem, &mut push);
    better_preset(facts, mem, &mut push);
    for tf in &facts.teams {
        // A team that is gone or locked is skipped entirely, whatever the facts say.
        if let Some(team) = mem.team(&tf.slug).filter(|t| !t.locked) {
            idle_member(tf, &team, &mut push);
            sent_back_most(tf, &team, &mut push);
            rounds_hit(tf, &team, now_ms(), &mut push);
            unreachable_member(tf, &team, &mut push);
            chosen_style(tf, &team, mem, &mut push);
        }
    }
    out
}

// ---- teams (`TEAM-23`, `TEAMS_PLAN` Part VII) ----

/// When each current role's place began, from the team's history: the oldest version that has it
/// with no version in between that lacks it.
pub fn member_since(mem: &MemoryStore, team: &Team) -> Vec<(String, i64)> {
    let history = mem.team_history(&team.slug); // newest first
    team.members
        .iter()
        .map(|m| {
            let mut since = 0;
            for entry in &history {
                if !entry.team.members.iter().any(|x| x.role == m.role) {
                    break;
                }
                since = entry.at;
            }
            (m.role.clone(), since)
        })
        .collect()
}

fn lead_for(team: &Team) -> String {
    format!("I'd like to change {}:", team.name)
}

/// A member that had no run in the team's last [`IDLE_TASKS`] tasks, in all of which it was a member
/// already. Not one the person talks with, and never the last member.
fn idle_member(tf: &TeamFacts, team: &Team, push: &mut dyn FnMut(Candidate)) {
    let last: Vec<&TaskFacts> = tf.tasks.iter().take(IDLE_TASKS).collect();
    if last.len() < IDLE_TASKS || team.members.len() < 2 {
        return;
    }
    for m in &team.members {
        let since = tf.since.iter().find(|(r, _)| *r == m.role).map_or(0, |(_, at)| *at);
        let talked_with = team.lines.iter().any(|l| l.kind == LineKind::Talks && l.to == m.role);
        if talked_with || last.iter().any(|t| t.at < since || t.ran.contains(&m.role)) {
            continue;
        }
        push(Candidate {
            subject: format!("team:{}:out:{}", team.slug, m.role),
            change: Change::TeamTakeOut { team: team.slug.clone(), role: m.role.clone() },
            lead: lead_for(team),
            quote: format!("Take {} out of the team.", m.role),
            why: format!("{} got no work in its last {IDLE_TASKS} tasks.", m.role),
        });
    }
}

/// A reviewer that sent back more than half of one member's items, over the last [`IDLE_TASKS`] tasks.
fn sent_back_most(tf: &TeamFacts, team: &Team, push: &mut dyn FnMut(Candidate)) {
    let looked: Vec<&Looked> = tf.tasks.iter().take(IDLE_TASKS).flat_map(|t| &t.looked).collect();
    for m in &team.members {
        let mine: Vec<&&Looked> = looked.iter().filter(|l| l.member == m.role).collect();
        let sent = mine.iter().filter(|l| l.sent_back).count();
        let Some(reviewer) = mine.first().map(|l| l.reviewer.clone()) else { continue };
        let still_there = team.members.iter().any(|x| x.role == reviewer);
        let drawn = team.lines.contains(&Line::new(LineKind::Asks, &m.role, &reviewer));
        if mine.len() < NEEDED || sent * 2 <= mine.len() || !still_there || drawn {
            continue;
        }
        push(Candidate {
            subject: format!("team:{}:asks:{}-{reviewer}", team.slug, m.role),
            change: Change::TeamAsks { team: team.slug.clone(), from: m.role.clone(), to: reviewer.clone() },
            lead: lead_for(team),
            quote: format!("{} may ask {reviewer} before handing work in.", m.role),
            why: format!("{reviewer} sent back {sent} of {} items from {} lately.", mine.len(), m.role),
        });
    }
}

/// The rounds limit hit in [`NEEDED`] tasks this month, where the coordinator could decide sooner.
fn rounds_hit(tf: &TeamFacts, team: &Team, now: i64, push: &mut dyn FnMut(Candidate)) {
    if team.limits.rounds <= SHORT_ROUNDS {
        return;
    }
    let hit = |l: &Looked| l.last_sent_back && l.reviews >= team.limits.rounds;
    let tasks: Vec<&TaskFacts> =
        tf.tasks.iter().filter(|t| t.at >= now - WINDOW_MS).filter(|t| t.looked.iter().any(hit)).collect();
    if tasks.len() < NEEDED {
        return;
    }
    // The two who disagreed most often, for the words.
    let mut pairs: Vec<(&str, &str)> = tasks.iter().flat_map(|t| t.looked.iter().filter(|l| hit(l))).map(|l| (l.member.as_str(), l.reviewer.as_str())).collect();
    pairs.sort_unstable();
    let top = pairs.iter().max_by_key(|p| pairs.iter().filter(|q| q == p).count()).copied();
    let who = top.map_or("two members".to_string(), |(a, b)| format!("{a} and {b}"));
    push(Candidate {
        subject: format!("team:{}:rounds", team.slug),
        change: Change::TeamRounds { team: team.slug.clone(), rounds: SHORT_ROUNDS },
        lead: lead_for(team),
        quote: format!("The coordinator decides after {SHORT_ROUNDS} rounds, not {}.", team.limits.rounds),
        why: format!("{who} did not agree within {} rounds in {} tasks this month.", team.limits.rounds, tasks.len()),
    });
}

/// A member the person tried to speak to, in [`NEEDED`] different chats, with no line to it.
fn unreachable_member(tf: &TeamFacts, team: &Team, push: &mut dyn FnMut(Candidate)) {
    for m in team.members.iter().filter(|m| m.persona.starts_with("persona:")) {
        let chats = distinct(tf.asked.iter().filter(|(_, r)| *r == m.role).map(|(c, _)| c.as_str()).collect());
        let drawn = team.lines.contains(&Line::new(LineKind::Talks, "you", &m.role));
        if chats.len() < NEEDED || drawn {
            continue;
        }
        push(Candidate {
            subject: format!("team:{}:talks:{}", team.slug, m.role),
            change: Change::TeamTalks { team: team.slug.clone(), role: m.role.clone() },
            lead: lead_for(team),
            quote: format!("You can talk with {} directly.", m.role),
            why: format!("you tried to speak to {} in {} different chats this month.", m.role, chats.len()),
        });
    }
}

/// A way of working the person chose in [`NEEDED`] different chats the team led, which is not the team's own.
fn chosen_style(tf: &TeamFacts, team: &Team, mem: &MemoryStore, push: &mut dyn FnMut(Candidate)) {
    let mut slugs: Vec<&str> = tf.styles.iter().map(|(_, s)| s.as_str()).filter(|s| *s != team.style).collect();
    slugs.sort_unstable();
    slugs.dedup();
    let best = slugs
        .into_iter()
        .filter_map(|s| {
            let chats = distinct(tf.styles.iter().filter(|(_, x)| x == s).map(|(c, _)| c.as_str()).collect());
            let preset = mem.preset(s)?;
            (chats.len() >= NEEDED).then_some((chats.len(), preset))
        })
        .max_by_key(|(n, _)| *n);
    if let Some((n, preset)) = best {
        push(Candidate {
            subject: format!("team:{}:style:{}", team.slug, preset.slug),
            change: Change::TeamStyle { team: team.slug.clone(), style: preset.slug.clone() },
            lead: lead_for(team),
            quote: format!("{} works {} unless you say otherwise.", team.name, preset.name),
            why: format!("you chose {} in {n} different chats this month.", preset.name),
        });
    }
}

fn skipped(facts: &Facts, mem: &MemoryStore, push: &mut dyn FnMut(Candidate)) {
    let mut rules: Vec<&str> = facts.signals.iter().filter(|s| s.signal.what == "not this time").map(|s| s.signal.rule.as_str()).collect();
    rules.sort_unstable();
    rules.dedup();
    for rule_id in rules {
        let skips: Vec<&Sig> =
            facts.signals.iter().filter(|s| s.signal.what == "not this time" && s.signal.rule == rule_id).collect();
        let convs = distinct(skips.iter().map(|s| s.conversation.as_str()).collect());
        if convs.len() < NEEDED {
            continue;
        }
        let (Some(when), Some(mv)) = (Trigger::parse(&skips[0].signal.trigger), Move::parse(&skips[0].signal.mv)) else { continue };
        let why = format!("you said \u{201c}not this time\u{201d} to this in {} different chats this month.", convs.len());
        if let Some((slug, _)) = rule_id.split_once(':') {
            let Some(preset) = mem.preset(slug) else { continue };
            // Locked: evolution skips it completely. Ad hoc has nothing to take out.
            if preset.locked || preset.slug == AD_HOC || !preset.rules.contains(&PresetRule { when, mv }) {
                continue;
            }
            let kinds: Vec<Option<&str>> = convs.iter().map(|c| facts.kind_of(c)).collect();
            let one_kind = kinds.first().copied().flatten().filter(|k| kinds.iter().all(|x| *x == Some(*k)));
            let taken = |name: &str| mem.presets().iter().any(|p| p.name.to_lowercase() == name.to_lowercase());
            if let Some(kind) = one_kind {
                let name = format!("{} for {}", preset.name, kind_label(kind));
                if !taken(&name) {
                    push(Candidate {
                        subject: format!("offspring:{}:{kind}:{}-{}", preset.slug, when.slug(), mv.slug()),
                        change: Change::Offspring { parent: preset.slug.clone(), name: name.clone(), kind: kind.to_string(), when, mv },
                        lead: format!("I work differently on {} with {}.", kind_label(kind), preset.name),
                        quote: format!("Shall I keep that as its own style, \u{201c}{name}\u{201d}? {} stays as it is.", preset.name),
                        why: format!("every time, it was {}: {why}", kind_label(kind)),
                    });
                    continue;
                }
            }
            push(Candidate {
                subject: format!("skip:{}:{}-{}", preset.slug, when.slug(), mv.slug()),
                change: Change::PresetRule { preset: preset.slug.clone(), when, mv },
                lead: format!("I'd like to change how {} works:", preset.name),
                quote: format!("No longer: {}", sentence(when, mv)),
                why,
            });
        } else if let Some(way) = mem.way(rule_id) {
            if matches!(way.status, Status::Learned | Status::Trying) {
                push(Candidate {
                    subject: format!("retire:{}", way.slug),
                    change: Change::RetireWay { slug: way.slug.clone() },
                    lead: "I'd like to stop doing this:".into(),
                    quote: format!("{}{}", reach(&way.scope), way.sentence()),
                    why,
                });
            }
        }
    }
}

fn undone(facts: &Facts, mem: &MemoryStore, push: &mut dyn FnMut(Candidate)) {
    let mut kinds: Vec<&str> = facts.undone.iter().filter_map(|c| facts.kind_of(c)).collect();
    kinds.sort_unstable();
    kinds.dedup();
    for kind in kinds {
        let convs: Vec<String> = distinct(facts.undone.iter().filter(|c| facts.kind_of(c) == Some(kind)).map(String::as_str).collect());
        if convs.len() < NEEDED {
            continue;
        }
        let ctx = ScopeCtx { kind: Some(kind), ..Default::default() };
        if mem.rules_for(AD_HOC, &ctx).iter().any(|r| r.when == Trigger::BeforeDone && r.mv == Move::SecondLook) {
            continue;
        }
        let scope = Scope::Kind(kind.to_string());
        push(Candidate {
            subject: format!("add:before-done-second-look:{}", scope.as_text()),
            change: Change::AddWay { when: Trigger::BeforeDone, mv: Move::SecondLook, scope: scope.as_text(), evidence: convs.clone() },
            lead: "I'd like to work differently:".into(),
            quote: format!("{}{}", reach(&scope), sentence(Trigger::BeforeDone, Move::SecondLook)),
            why: format!("you put my changes back in {} different {} chats.", convs.len(), kind_label(kind)),
        });
    }
}

/// Of finished conversations, the share that were done, as a whole number of percent.
fn done_rate(of: &[&Ended]) -> usize {
    if of.is_empty() {
        0
    } else {
        of.iter().filter(|e| e.done).count() * 100 / of.len()
    }
}

fn better_preset(facts: &Facts, mem: &MemoryStore, push: &mut dyn FnMut(Candidate)) {
    let mut kinds: Vec<&str> = facts.ended.iter().filter(|e| e.at >= now_ms() - 60 * DAY_MS).filter_map(|e| e.kind.as_deref()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    let defaults = mem.defaults();
    for kind in kinds {
        let scope = format!("kind:{kind}");
        if defaults.iter().any(|(s, _)| *s == scope) {
            continue;
        }
        let here: Vec<&Ended> =
            facts.ended.iter().filter(|e| e.at >= now_ms() - 60 * DAY_MS && e.kind.as_deref() == Some(kind)).collect();
        let plain: Vec<&Ended> = here.iter().copied().filter(|e| e.style.is_none() || e.style.as_deref() == Some(AD_HOC)).collect();
        if plain.len() < JUDGED || done_rate(&plain) > 50 {
            continue;
        }
        let mut slugs: Vec<&str> = here.iter().filter_map(|e| e.style.as_deref()).filter(|s| *s != AD_HOC).collect();
        slugs.sort_unstable();
        slugs.dedup();
        for slug in slugs {
            let with: Vec<&Ended> = here.iter().copied().filter(|e| e.style.as_deref() == Some(slug)).collect();
            let Some(preset) = mem.preset(slug) else { continue };
            if with.len() < JUDGED || done_rate(&with) < 80 {
                continue;
            }
            push(Candidate {
                subject: format!("default:{scope}:{slug}"),
                change: Change::Default { scope: scope.clone(), preset: slug.to_string() },
                lead: "I'd like to start differently:".into(),
                quote: format!("For {}, chats start with {}.", kind_label(kind), preset.name),
                why: format!("{} has gone better than working ad hoc on {}.", preset.name, kind_label(kind)),
            });
        }
    }
}

// ---- trying out ----

/// How a trial stands: the tasks of its kind that ended since it began, and how they
/// went against the ones before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialStanding {
    pub tasks: usize,
    pub verdict: &'static str,
}

fn in_scope(scope: &Scope, e: &Ended) -> bool {
    match scope {
        Scope::Kind(k) => e.kind.as_deref() == Some(k.as_str()),
        Scope::Project(p) => e.project.as_deref() == Some(p.as_str()),
        _ => true,
    }
}

pub fn standing(facts: &Facts, scope: &Scope, from: i64) -> TrialStanding {
    let mine: Vec<&Ended> = facts.ended.iter().filter(|e| e.at >= from && in_scope(scope, e)).collect();
    let mut before: Vec<&Ended> = facts.ended.iter().filter(|e| e.at < from && in_scope(scope, e)).collect();
    before.sort_by_key(|e| std::cmp::Reverse(e.at));
    before.truncate(BEFORE);
    let verdict = if before.len() < 2 || mine.is_empty() {
        "unknown"
    } else {
        let (a, b) = (done_rate(&mine) as i64, done_rate(&before) as i64);
        if a >= b + 20 {
            "better"
        } else if a + 20 <= b {
            "worse"
        } else {
            "same"
        }
    };
    TrialStanding { tasks: mine.len(), verdict }
}

pub fn verdict_words(verdict: &str) -> &'static str {
    match verdict {
        "better" => "It went better than before.",
        "worse" => "It didn't go as well as before.",
        "same" => "It went about as well as before.",
        _ => "I can't tell yet how it compared.",
    }
}

/// The ways being tried out whose tasks have come, or whose days have run out.
pub fn trials_due(facts: &Facts, mem: &MemoryStore, now: i64, known: &dyn Fn(&str) -> bool) -> Vec<Candidate> {
    let mut out = Vec::new();
    for way in mem.ways().into_iter().filter(|w| w.status == Status::Trying) {
        let Some(trial) = way.trial else { continue };
        let s = standing(facts, &way.scope, trial.from);
        let enough = s.tasks >= trial.tasks as usize;
        let late = now >= trial.from + trial.days as i64 * DAY_MS;
        if !enough && !(late && !trial.asked) {
            continue;
        }
        // Two cards a trial can raise: the days ran out (`late`, asked once), and its
        // tasks came (`end`). "Keep trying" answers the first and waits for the second;
        // while the first is still unanswered, the second waits for it.
        let subject = |stage: &str| format!("trial:{}:{}:{stage}", way.slug, trial.from);
        let stage = if enough { "end" } else { "late" };
        if known(&subject(stage)) || (enough && !trial.asked && known(&subject("late"))) {
            continue;
        }
        let subject = subject(stage);
        out.push(Candidate {
            subject,
            change: Change::TrialEnd { slug: way.slug.clone(), verdict: s.verdict.to_string(), early: !enough },
            lead: if enough { "I've tried something for a while:".into() } else { "I've been trying something for a month:".into() },
            quote: format!("{}{}", reach(&way.scope), way.sentence()),
            why: if enough {
                verdict_words(s.verdict).to_string()
            } else {
                format!("Only {} {} of that kind came up. {}", s.tasks, if s.tasks == 1 { "task" } else { "tasks" }, verdict_words(s.verdict))
            },
        });
    }
    out
}

// ---- applying ----

/// How to put a change back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "undo", rename_all = "snake_case")]
pub enum Undo {
    RemoveWay { slug: String },
    SetStatus { slug: String, status: String },
    PresetRule { preset: String, when: Trigger, #[serde(rename = "move")] mv: Move, on: bool },
    Default { scope: String, before: Option<String> },
    Offspring { slug: String, scope: String, before: Option<String> },
    /// `TEAM-23`: the team as it was, put back as a new version, unless it changed since
    /// (`version` is the one this change made).
    Team { slug: String, before: Box<Team>, version: u32 },
    Nothing,
}

/// What a change did, in the first person, and how to take it back.
#[derive(Debug, Clone)]
pub struct Applied {
    pub said: String,
    pub undo: Undo,
}

fn scope_of(s: &str) -> Scope {
    if s == "everywhere" {
        Scope::Everywhere
    } else {
        Scope::parse(s)
    }
}

/// Make the change. `answer` is what the person said: `try` or `always` for a new way,
/// anything else (`yes`) for the rest; for a trial that ended, `keep`, `retire` or
/// `keep-trying`. `why` goes into a preset's history.
pub fn apply(mem: &MemoryStore, change: &Change, answer: &str, why: &str) -> Result<Applied, String> {
    match change {
        Change::AddWay { when, mv, scope, evidence } => {
            let status = if answer == "always" { Status::Learned } else { Status::Trying };
            let sc = scope_of(scope);
            let way = mem.add_way_from(*when, *mv, sc.clone(), status, "evolution", evidence.clone())?;
            let said = format!("started {} {}", if status == Status::Trying { "trying" } else { "doing" }, scope_words(&sc, *mv));
            Ok(Applied { said, undo: Undo::RemoveWay { slug: way.slug } })
        }
        Change::RetireWay { slug } => {
            let way = mem.way(slug).ok_or("I can't find that rule.")?;
            mem.update_way(slug, crate::memory::ways::WayEdit { status: Some(Status::Retired), ..Default::default() })?;
            Ok(Applied {
                said: format!("stopped {}", scope_words(&way.scope, way.mv)),
                undo: Undo::SetStatus { slug: slug.clone(), status: way.status.slug().into() },
            })
        }
        Change::PresetRule { preset, when, mv } => {
            let p = mem.preset(preset).ok_or("I can't find that style.")?;
            if p.locked {
                return Err(format!("{} is locked, so I leave it alone.", p.name));
            }
            mem.set_preset_rule_by(preset, PresetRule { when: *when, mv: *mv }, false, "evolution", why)?;
            Ok(Applied {
                said: format!("changed how {} works", p.name),
                undo: Undo::PresetRule { preset: preset.clone(), when: *when, mv: *mv, on: true },
            })
        }
        Change::Default { scope, preset } => {
            let before = mem.defaults().into_iter().find(|(s, _)| s == scope).map(|(_, p)| p);
            mem.set_default(scope, Some(preset))?;
            let name = mem.preset(preset).map(|p| p.name).unwrap_or_default();
            let place = scope.strip_prefix("kind:").map(kind_label).unwrap_or(scope);
            Ok(Applied { said: format!("started {place} chats with {name}"), undo: Undo::Default { scope: scope.clone(), before } })
        }
        Change::Offspring { parent, name, kind, when, mv } => {
            let from = mem.preset(parent).ok_or("I can't find the preset it branches from.")?;
            if from.locked {
                return Err(format!("{} is locked, so I leave it alone.", from.name));
            }
            let rules: Vec<PresetRule> = from.rules.iter().filter(|r| !(r.when == *when && r.mv == *mv)).copied().collect();
            let line = format!("{} on {}, without one move", from.line, kind_label(kind));
            let kid = mem.create_offspring(parent, name, &line, rules, why)?;
            let scope = format!("kind:{kind}");
            let before = mem.defaults().into_iter().find(|(s, _)| *s == scope).map(|(_, p)| p);
            mem.set_default(&scope, Some(&kid.slug))?;
            Ok(Applied {
                said: format!("started {} as its own style", kid.name),
                undo: Undo::Offspring { slug: kid.slug, scope, before },
            })
        }
        Change::TeamTakeOut { team, role } => edit_team(mem, team, why, |t| {
            if !t.members.iter().any(|m| m.role == *role) {
                return Err(format!("{role} is no longer in {}.", t.name));
            }
            t.members.retain(|m| m.role != *role);
            t.lines.retain(|l| l.from != *role && l.to != *role);
            Ok(format!("took {role} out of {}", t.name))
        }),
        Change::TeamAsks { team, from, to } => edit_team(mem, team, why, |t| {
            if ![from, to].iter().all(|r| t.members.iter().any(|m| m.role == **r)) {
                return Err(format!("{from} and {to} are no longer both in {}.", t.name));
            }
            add_line(t, Line::new(LineKind::Asks, from, to))?;
            Ok(format!("let {from} ask {to} in {}", t.name))
        }),
        Change::TeamRounds { team, rounds } => edit_team(mem, team, why, |t| {
            t.limits.rounds = (*rounds).max(1);
            Ok(format!("made {} decide after {} rounds", t.name, t.limits.rounds))
        }),
        Change::TeamTalks { team, role } => edit_team(mem, team, why, |t| {
            if !t.members.iter().any(|m| m.role == *role) {
                return Err(format!("{role} is no longer in {}.", t.name));
            }
            add_line(t, Line::new(LineKind::Talks, "you", role))?;
            Ok(format!("let you talk with {role} in {}", t.name))
        }),
        Change::TeamStyle { team, style } => edit_team(mem, team, why, |t| {
            let preset = mem.preset(style).ok_or("I can't find that style.")?;
            t.style = preset.slug.clone();
            Ok(format!("made {} work {} unless you say otherwise", t.name, preset.name))
        }),
        Change::TrialEnd { slug, .. } => {
            let way = mem.way(slug).ok_or("I can't find that rule.")?;
            let edit = |s| crate::memory::ways::WayEdit { status: Some(s), ..Default::default() };
            match answer {
                "keep-trying" => {
                    mem.extend_trial(slug)?;
                    Ok(Applied { said: format!("kept trying {}", scope_words(&way.scope, way.mv)), undo: Undo::Nothing })
                }
                "retire" => {
                    mem.update_way(slug, edit(Status::Retired))?;
                    Ok(Applied {
                        said: format!("stopped {}", scope_words(&way.scope, way.mv)),
                        undo: Undo::SetStatus { slug: slug.clone(), status: "trying".into() },
                    })
                }
                _ => {
                    mem.update_way(slug, edit(Status::Learned))?;
                    Ok(Applied {
                        said: format!("kept {}", scope_words(&way.scope, way.mv)),
                        undo: Undo::SetStatus { slug: slug.clone(), status: "trying".into() },
                    })
                }
            }
        }
    }
}

/// `TEAM-23`: change a team that is not locked, as a new version made by evolution.
fn edit_team(mem: &MemoryStore, slug: &str, why: &str, change: impl FnOnce(&mut Team) -> Result<String, String>) -> Result<Applied, String> {
    let before = mem.team(slug).ok_or("I can't find that team.")?;
    if before.locked {
        return Err(format!("{} is locked, so I leave it alone.", before.name));
    }
    let mut after = before.clone();
    let said = change(&mut after)?;
    if after == before {
        return Err(format!("{} is already like that.", before.name));
    }
    let done = mem.save_team(after, "evolution", why)?;
    Ok(Applied { said, undo: Undo::Team { slug: slug.to_string(), before: Box::new(before), version: done.version } })
}

/// A line the team does not have yet. Saving would drop a line it has already without saying so,
/// and the change would be reported as made.
fn add_line(t: &mut Team, line: Line) -> Result<(), String> {
    if t.lines.contains(&line) {
        return Err(format!("{} is already like that.", t.name));
    }
    t.lines.push(line);
    Ok(())
}

/// "a second look for mail", as a clause.
fn scope_words(scope: &Scope, mv: Move) -> String {
    let where_ = match scope {
        Scope::Everywhere => String::new(),
        Scope::Kind(k) => format!(" for {}", kind_label(k)),
        Scope::Project(_) => " in one project".into(),
        Scope::Persona(_) => " for one persona".into(),
    };
    let what = match mv {
        Move::SecondLook => "a second look",
        Move::Split => "splitting the work up",
        Move::OneAtATime => "finishing one thing before the next",
        Move::CheckIn => "checking in with you",
        Move::Advice => "asking for advice first",
        Move::Criteria => "agreeing with you what done means",
        Move::LookFirst => "looking around first",
        Move::FreshStart => "starting fresh with a handover note",
        Move::TwoWays => "trying two ways",
        Move::HandOff => "handing work to a helper",
        Move::Parallel => "working on several items at once",
        Move::StopTheLine => "fixing things before anything else",
    };
    format!("{what}{where_}")
}

/// Put a change back.
pub fn undo(mem: &MemoryStore, u: &Undo) -> Result<(), String> {
    match u {
        Undo::RemoveWay { slug } => mem
            .update_way(slug, crate::memory::ways::WayEdit { status: Some(Status::Retired), ..Default::default() })
            .map(|_| ()),
        Undo::SetStatus { slug, status } => {
            let s = Status::parse(status).ok_or("I don't know that state.")?;
            mem.update_way(slug, crate::memory::ways::WayEdit { status: Some(s), ..Default::default() }).map(|_| ())
        }
        Undo::PresetRule { preset, when, mv, on } => mem
            .set_preset_rule_by(preset, PresetRule { when: *when, mv: *mv }, *on, "you", "put it back")
            .map(|_| ()),
        Undo::Default { scope, before } => mem.set_default(scope, before.as_deref()),
        Undo::Offspring { slug, scope, before } => {
            mem.set_default(scope, before.as_deref())?;
            mem.delete_preset(slug).map(|_| ())
        }
        Undo::Team { slug, before, version } => {
            let now = mem.team(slug).ok_or("I can't find that team.")?;
            if now.version != *version {
                return Err(format!("{} has changed since, so I leave it as it is.", now.name));
            }
            mem.save_team(*before.clone(), "you", "put it back").map(|_| ())
        }
        Undo::Nothing => Ok(()),
    }
}

// ---- proposals ----

/// Write a candidate down as a proposal waiting for a yes. Returns its id.
pub fn raise(db: &Db, c: &Candidate) -> Result<String, String> {
    let payload = serde_json::to_string(&c.change).map_err(|e| e.to_string())?;
    let text = serde_json::json!({ "lead": c.lead, "quote": c.quote, "change": serde_json::from_str::<serde_json::Value>(&payload).unwrap_or_default() });
    db.add_change_proposal(c.change.target(), Some(&c.subject), &text.to_string(), &c.why, Some(&c.quote))
        .map(|p| p.id)
        .map_err(|e| e.to_string())
}

/// What a way proposal's `proposed_text` holds.
#[derive(Debug, Clone, Deserialize)]
pub struct Stored {
    pub lead: String,
    pub quote: String,
    pub change: Change,
}

pub fn read(text: &str) -> Option<Stored> {
    serde_json::from_str(text).ok()
}

// ---- what was done lately ----

/// One thing evolution did, kept for a while so the tab can say so, and, when it was
/// done on its own, so it can be undone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub id: String,
    pub at: i64,
    /// "started trying a second look for mail".
    pub said: String,
    /// Done without asking: it can be undone from the tab.
    pub auto: bool,
    pub undo: Undo,
}

pub fn recent(db: &Db, now: i64) -> Vec<Recent> {
    let mut list: Vec<Recent> = db
        .get_setting(RECENT_KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    list.retain(|r| now - r.at < RECENT_MS);
    list.sort_by_key(|r| std::cmp::Reverse(r.at));
    list.truncate(20);
    list
}

pub fn note(db: &Db, applied: &Applied, auto: bool, now: i64) {
    let mut list = recent(db, now);
    list.insert(0, Recent { id: uuid::Uuid::new_v4().simple().to_string(), at: now, said: applied.said.clone(), auto, undo: applied.undo.clone() });
    if let Ok(json) = serde_json::to_string(&list) {
        let _ = db.set_setting(RECENT_KEY, &json);
    }
}

/// Take an entry out of the list, handing it back.
pub fn take(db: &Db, id: &str, now: i64) -> Option<Recent> {
    let mut list = recent(db, now);
    let at = list.iter().position(|r| r.id == id)?;
    let found = list.remove(at);
    if let Ok(json) = serde_json::to_string(&list) {
        let _ = db.set_setting(RECENT_KEY, &json);
    }
    Some(found)
}

/// Look at what happened, and raise what is due. Returns the candidates, for a run
/// that applies them by itself.
pub fn due(db: &Db, mem: &MemoryStore, now: i64) -> Vec<Candidate> {
    let mut facts = Facts::gather(db, now);
    facts.teams = Facts::gather_teams(db, mem, now);
    let known = |s: &str| db.way_proposal_exists(s);
    let mut out = trials_due(&facts, mem, now, &known);
    out.extend(candidates(&facts, mem, &known));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (MemoryStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("poiesis_evolve_{}", uuid::Uuid::new_v4().simple()));
        let s = MemoryStore::new(&dir).unwrap();
        s.ensure_ways();
        (s, dir)
    }

    fn skip(conv: &str, rule: &str, when: &str, mv: &str) -> Sig {
        Sig {
            conversation: conv.into(),
            signal: Signal { rule: rule.into(), trigger: when.into(), mv: mv.into(), what: "not this time".into(), item: None },
        }
    }

    fn ended(id: &str, kind: &str, done: bool, style: Option<&str>, at: i64) -> Ended {
        Ended { id: id.into(), kind: Some(kind.into()), project: None, done, style: style.map(Into::into), at }
    }

    fn none(_: &str) -> bool {
        false
    }

    #[test]
    fn three_skips_in_different_kinds_offer_to_take_the_move_out_of_the_preset() {
        let (s, dir) = store();
        let rule = "lean:many-changes-split";
        let facts = Facts {
            signals: vec![skip("a", rule, "many-changes", "split"), skip("b", rule, "many-changes", "split"), skip("c", rule, "many-changes", "split")],
            ended: vec![ended("a", "mail", true, None, 1), ended("b", "code", true, None, 2), ended("c", "mail", true, None, 3)],
            ..Default::default()
        };
        let got = candidates(&facts, &s, &none);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].change, Change::PresetRule { preset: "lean".into(), when: Trigger::ManyChanges, mv: Move::Split });
        assert!(got[0].lead.contains("Lean") && got[0].why.contains("3 different chats"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn skips_all_in_one_kind_offer_an_offspring_for_that_kind() {
        let (s, dir) = store();
        let rule = "lean:many-changes-split";
        let facts = Facts {
            signals: ["a", "b", "c"].iter().map(|c| skip(c, rule, "many-changes", "split")).collect(),
            ended: ["a", "b", "c"].iter().enumerate().map(|(i, c)| ended(c, "code", true, None, i as i64)).collect(),
            ..Default::default()
        };
        let got = candidates(&facts, &s, &none);
        assert_eq!(got.len(), 1);
        assert!(matches!(&got[0].change, Change::Offspring { parent, name, kind, .. } if parent == "lean" && name == "Lean for code" && kind == "code"));
        // Where that name is taken already, the plain change is offered instead.
        s.create_preset("Lean for code", "", None, &[], true).unwrap();
        assert!(matches!(candidates(&facts, &s, &none)[0].change, Change::PresetRule { .. }));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_locked_preset_is_skipped_entirely_and_two_skips_are_not_enough() {
        let (s, dir) = store();
        let rule = "lean:many-changes-split";
        let three = |ids: &[&str]| Facts {
            signals: ids.iter().map(|c| skip(c, rule, "many-changes", "split")).collect(),
            ended: ids.iter().enumerate().map(|(i, c)| ended(c, "code", true, None, i as i64)).collect(),
            ..Default::default()
        };
        assert!(candidates(&three(&["a", "b"]), &s, &none).is_empty());
        // The same chat three times is one chat.
        assert!(candidates(&three(&["a", "a", "a"]), &s, &none).is_empty());
        s.edit_preset("lean", crate::memory::ways::PresetEdit { locked: Some(true), ..Default::default() }).unwrap();
        assert!(candidates(&three(&["a", "b", "c"]), &s, &none).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_way_skipped_three_times_is_offered_for_retiring_but_a_base_rule_never() {
        let (s, dir) = store();
        let w = s.add_way(Trigger::AfterItem, Move::CheckIn, Scope::Everywhere).unwrap();
        let mk = |rule: &str, when: &str, mv: &str| Facts {
            signals: ["a", "b", "c"].iter().map(|c| skip(c, rule, when, mv)).collect(),
            ..Default::default()
        };
        let got = candidates(&mk(&w.slug, "after-item", "check-in"), &s, &none);
        assert_eq!(got[0].change, Change::RetireWay { slug: w.slug.clone() });
        assert!(candidates(&mk("stop-the-line", "check-fails", "stop-the-line"), &s, &none).is_empty());
        assert!(candidates(&mk(&w.slug, "after-item", "check-in"), &s, &|sub| sub == format!("retire:{}", w.slug)).is_empty(), "No stays no");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn changes_put_back_in_three_chats_of_one_kind_offer_a_second_look_for_that_kind() {
        let (s, dir) = store();
        let facts = Facts {
            undone: ["a", "b", "c", "d"].iter().map(|c| c.to_string()).collect(),
            ended: vec![ended("a", "code", true, None, 1), ended("b", "code", true, None, 2), ended("c", "code", true, None, 3), ended("d", "mail", true, None, 4)],
            ..Default::default()
        };
        let got = candidates(&facts, &s, &none);
        assert_eq!(got.len(), 1, "mail has only one");
        assert!(matches!(&got[0].change, Change::AddWay { when: Trigger::BeforeDone, mv: Move::SecondLook, scope, evidence } if scope == "kind:code" && evidence.len() == 3));
        assert!(got[0].quote.starts_with("For code: "));
        // Where a second look already happens, there is nothing to offer.
        s.add_way(Trigger::BeforeDone, Move::SecondLook, Scope::Kind("code".into())).unwrap();
        assert!(candidates(&facts, &s, &none).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_preset_that_clearly_does_better_is_offered_as_where_a_kind_starts() {
        let (s, dir) = store();
        let now = now_ms();
        let mut ended_all = Vec::new();
        for i in 0..5 {
            ended_all.push(ended(&format!("p{i}"), "code", i == 0, None, now - i));
            ended_all.push(ended(&format!("l{i}"), "code", i < 4, Some("lean"), now - i));
        }
        let facts = Facts { ended: ended_all.clone(), ..Default::default() };
        let got = candidates(&facts, &s, &none);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].change, Change::Default { scope: "kind:code".into(), preset: "lean".into() });
        // Not when it is only a little better, or when a place already says.
        let facts = Facts { ended: ended_all.iter().cloned().map(|mut e| { e.done = true; e }).collect(), ..Default::default() };
        assert!(candidates(&facts, &s, &none).is_empty(), "ad hoc did fine too");
        s.set_default("kind:code", Some("swarm")).unwrap();
        let facts = Facts { ended: ended_all, ..Default::default() };
        assert!(candidates(&facts, &s, &none).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_trial_ends_when_its_tasks_have_come_and_says_whether_it_helped() {
        let (s, dir) = store();
        let w = s.add_way_from(Trigger::BeforeDone, Move::SecondLook, Scope::Kind("code".into()), Status::Trying, "evolution", vec![]).unwrap();
        let from = w.trial.unwrap().from;
        let mut e = vec![ended("o1", "code", false, None, from - 10), ended("o2", "code", false, None, from - 9), ended("o3", "code", true, None, from - 8)];
        for i in 0..4 {
            e.push(ended(&format!("n{i}"), "code", true, None, from + i));
        }
        let facts = Facts { ended: e.clone(), ..Default::default() };
        assert!(trials_due(&facts, &s, from + 1000, &none).is_empty(), "four tasks are not five");
        e.push(ended("n4", "code", true, None, from + 5));
        e.push(ended("mail", "mail", false, None, from + 6));
        let facts = Facts { ended: e, ..Default::default() };
        let due = trials_due(&facts, &s, from + 1000, &none);
        assert_eq!(due.len(), 1);
        assert!(matches!(&due[0].change, Change::TrialEnd { verdict, early: false, .. } if verdict == "better"));
        assert_eq!(due[0].why, "It went better than before.");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_trial_that_ran_out_of_days_asks_once() {
        let (s, dir) = store();
        let w = s.add_way_from(Trigger::BeforeDone, Move::SecondLook, Scope::Everywhere, Status::Trying, "evolution", vec![]).unwrap();
        let from = w.trial.unwrap().from;
        let facts = Facts::default();
        assert!(trials_due(&facts, &s, from + 29 * DAY_MS, &none).is_empty());
        let late = from + 31 * DAY_MS;
        let due = trials_due(&facts, &s, late, &none);
        assert!(matches!(&due[0].change, Change::TrialEnd { early: true, verdict, .. } if verdict == "unknown"));
        // Keep trying waits for the tasks, and does not ask about the days a second time.
        let asked = due[0].subject.clone();
        let was_asked = |subject: &str| subject == asked;
        apply(&s, &due[0].change, "keep-trying", "").unwrap();
        assert!(trials_due(&facts, &s, late + 40 * DAY_MS, &was_asked).is_empty());
        // Its tasks come after all: the trial ends with its own card.
        let facts = Facts { ended: (0..5).map(|i| ended(&format!("n{i}"), "code", true, None, from + i)).collect(), ..Default::default() };
        let due = trials_due(&facts, &s, late + 40 * DAY_MS, &was_asked);
        assert_eq!(due.len(), 1);
        assert!(matches!(&due[0].change, Change::TrialEnd { early: false, .. }));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn every_change_can_be_put_back() {
        let (s, dir) = store();
        // A new way, tried, then undone.
        let add = Change::AddWay { when: Trigger::BeforeDone, mv: Move::SecondLook, scope: "kind:mail".into(), evidence: vec!["c1".into()] };
        let done = apply(&s, &add, "try", "").unwrap();
        assert_eq!(done.said, "started trying a second look for mail");
        let way = s.ways().into_iter().find(|w| w.origin == "evolution").unwrap();
        assert_eq!((way.status, way.evidence.clone()), (Status::Trying, vec!["c1".to_string()]));
        assert!(way.trial.is_some());
        undo(&s, &done.undo).unwrap();
        assert_eq!(s.way(&way.slug).unwrap().status, Status::Retired);

        // A move out of a preset, and back in; the history says who.
        let rule = Change::PresetRule { preset: "lean".into(), when: Trigger::ManyChanges, mv: Move::Split };
        let done = apply(&s, &rule, "yes", "you skipped it three times").unwrap();
        assert!(!s.preset("lean").unwrap().rules.contains(&PresetRule { when: Trigger::ManyChanges, mv: Move::Split }));
        let h = s.preset_history("lean");
        assert_eq!((h[0].by.as_str(), h[0].why.as_str()), ("evolution", "you skipped it three times"));
        undo(&s, &done.undo).unwrap();
        assert!(s.preset("lean").unwrap().rules.contains(&PresetRule { when: Trigger::ManyChanges, mv: Move::Split }));

        // A default, and an offspring that takes the place and gives it back.
        let d = Change::Default { scope: "kind:code".into(), preset: "swarm".into() };
        let done = apply(&s, &d, "yes", "").unwrap();
        assert_eq!(s.defaults(), vec![("kind:code".to_string(), "swarm".to_string())]);
        undo(&s, &done.undo).unwrap();
        assert!(s.defaults().is_empty());
        let off = Change::Offspring { parent: "lean".into(), name: "Lean for code".into(), kind: "code".into(), when: Trigger::ManyChanges, mv: Move::Split };
        let done = apply(&s, &off, "yes", "").unwrap();
        let kid = s.preset("lean-for-code").unwrap();
        assert_eq!(kid.rules.len(), 3, "without the one move");
        assert_eq!(s.defaults(), vec![("kind:code".to_string(), "lean-for-code".to_string())]);
        undo(&s, &done.undo).unwrap();
        assert!(s.preset("lean-for-code").is_none() && s.defaults().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_locked_preset_is_not_changed_even_if_a_proposal_was_waiting() {
        let (s, dir) = store();
        let rule = Change::PresetRule { preset: "lean".into(), when: Trigger::ManyChanges, mv: Move::Split };
        s.edit_preset("lean", crate::memory::ways::PresetEdit { locked: Some(true), ..Default::default() }).unwrap();
        assert!(apply(&s, &rule, "yes", "").is_err());
        assert_eq!(s.preset("lean").unwrap().version, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_proposal_reads_back_from_what_was_stored() {
        let c = Candidate {
            subject: "retire:x".into(),
            change: Change::RetireWay { slug: "x".into() },
            lead: "I'd like to stop doing this:".into(),
            quote: "q".into(),
            why: "w".into(),
        };
        let db = Db::open_in_memory().unwrap();
        raise(&db, &c).unwrap();
        let p = db.list_change_proposals().unwrap().remove(0);
        assert_eq!((p.target.as_str(), p.slug.as_deref()), (TARGET, Some("retire:x")));
        assert!(db.way_proposal_exists("retire:x") && !db.way_proposal_exists("retire:y"));
        assert_eq!(read(&p.proposed_text).unwrap().change, c.change);
    }

    // ---- TEAM-23: teams evolve ----

    /// A team of four that evolution may change: api, ui, a docs helper, and a verifier that reviews api.
    fn shop(s: &MemoryStore) -> Team {
        use crate::memory::ways::{shape_lines, Member, Shape};
        let member = |role: &str, who: &str| Member { role: role.into(), persona: who.into(), job: String::new(), owns: vec![] };
        let mut t = Team::blank();
        t.name = "Shop team".into();
        t.members = vec![member("api", "persona:a1"), member("ui", "persona:b2"), member("docs", "helper"), member("verifier", "persona:c3")];
        t.lines = shape_lines(Shape::Hub, &t.members);
        t.lines.push(Line::new(LineKind::Reviews, "verifier", "api"));
        let made = s.save_team(t, "you", "").unwrap();
        assert!(made.locked, "a team the person made starts locked");
        s.save_team(Team { locked: false, ..made }, "you", "").unwrap()
    }

    fn task(at: i64, ran: &[&str], looked: Vec<Looked>) -> TaskFacts {
        TaskFacts { at, ran: ran.iter().map(|r| r.to_string()).collect(), looked }
    }

    fn look(member: &str, reviews: u32, sent_back: bool, last: bool) -> Looked {
        Looked { member: member.into(), reviewer: "verifier".into(), reviews, sent_back, last_sent_back: last }
    }

    fn about(tf: TeamFacts) -> Facts {
        Facts { teams: vec![tf], ..Default::default() }
    }

    fn five_tasks(ran: &[&str]) -> Vec<TaskFacts> {
        (0..5).map(|i| task(100 - i, ran, vec![])).collect()
    }

    #[test]
    fn a_member_with_no_work_in_five_tasks_is_offered_to_be_taken_out() {
        let (s, dir) = store();
        let team = shop(&s);
        let busy = ["api", "ui", "verifier"];
        let tf = |tasks| TeamFacts { slug: team.slug.clone(), tasks, ..Default::default() };
        let got = candidates(&about(tf(five_tasks(&busy))), &s, &none);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].change, Change::TeamTakeOut { team: "shop-team".into(), role: "docs".into() });
        assert_eq!((got[0].change.target(), got[0].lead.as_str()), ("team", "I'd like to change Shop team:"));
        assert_eq!(got[0].why, "docs got no work in its last 5 tasks.");
        // Four tasks are not five, and a member that did work is left alone.
        assert!(candidates(&about(tf(five_tasks(&busy)[..4].to_vec())), &s, &none).is_empty());
        assert!(candidates(&about(tf(five_tasks(&["api", "ui", "verifier", "docs"]))), &s, &none).is_empty());
        // A member that joined after the oldest of the five has not had the five tasks.
        let late = TeamFacts { since: vec![("docs".into(), 99)], ..tf(five_tasks(&busy)) };
        assert!(candidates(&about(late), &s, &none).is_empty());
        // No stays no.
        assert!(candidates(&about(tf(five_tasks(&busy))), &s, &|sub| sub == "team:shop-team:out:docs").is_empty());
        // The person talks with it: it is not idle, it is spoken to.
        let mut talks = s.team("shop-team").unwrap();
        talks.lines.push(Line::new(LineKind::Talks, "you", "docs"));
        s.save_team(talks, "you", "").unwrap();
        assert!(candidates(&about(tf(five_tasks(&busy))), &s, &none).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_locked_team_is_skipped_whatever_the_facts_say() {
        let (s, dir) = store();
        let team = shop(&s);
        let facts = about(TeamFacts { slug: team.slug.clone(), tasks: five_tasks(&["api"]), ..Default::default() });
        assert!(!candidates(&facts, &s, &none).is_empty());
        s.save_team(Team { locked: true, ..s.team("shop-team").unwrap() }, "you", "").unwrap();
        assert!(candidates(&facts, &s, &none).is_empty());
        // A card that was waiting when it was locked is not carried out either.
        let change = Change::TeamTakeOut { team: "shop-team".into(), role: "docs".into() };
        assert!(apply(&s, &change, "yes", "").unwrap_err().contains("locked"));
        assert!(s.team("shop-team").unwrap().members.iter().any(|m| m.role == "docs"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_reviewer_that_sends_most_of_a_members_items_back_offers_an_asks_line() {
        let (s, dir) = store();
        let team = shop(&s);
        let run = |sent: &[bool]| {
            let tasks = sent.iter().enumerate().map(|(i, b)| task(100 - i as i64, &["api", "ui", "verifier", "docs"], vec![look("api", 1, *b, *b)])).collect();
            about(TeamFacts { slug: team.slug.clone(), tasks, ..Default::default() })
        };
        let got = candidates(&run(&[true, true, true, false]), &s, &none);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].change, Change::TeamAsks { team: "shop-team".into(), from: "api".into(), to: "verifier".into() });
        assert_eq!(got[0].why, "verifier sent back 3 of 4 items from api lately.");
        assert!(candidates(&run(&[true, true, false, false]), &s, &none).is_empty(), "half is not most");
        assert!(candidates(&run(&[true, true]), &s, &none).is_empty(), "two items are too few");
        let mut asks = s.team("shop-team").unwrap();
        asks.lines.push(Line::new(LineKind::Asks, "api", "verifier"));
        s.save_team(asks, "you", "").unwrap();
        assert!(candidates(&run(&[true, true, true, false]), &s, &none).is_empty(), "the line is drawn");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_rounds_limit_hit_in_three_tasks_offers_the_coordinator_deciding_after_two() {
        let (s, dir) = store();
        let team = shop(&s); // three rounds
        let now = now_ms();
        let stuck = || vec![look("api", 3, true, true)];
        let run = |tasks: Vec<TaskFacts>| about(TeamFacts { slug: team.slug.clone(), tasks, ..Default::default() });
        // An item sent back every time also shows the reviewer's habit; this is about the limit.
        let candidates = |f: &Facts, s: &MemoryStore, k: &dyn Fn(&str) -> bool| -> Vec<Candidate> {
            super::candidates(f, s, k).into_iter().filter(|c| matches!(c.change, Change::TeamRounds { .. })).collect()
        };
        let three: Vec<TaskFacts> = (0..3).map(|i| task(now - i, &["api", "ui", "verifier", "docs"], stuck())).collect();
        let got = candidates(&run(three.clone()), &s, &none);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].change, Change::TeamRounds { team: "shop-team".into(), rounds: 2 });
        assert_eq!(got[0].why, "api and verifier did not agree within 3 rounds in 3 tasks this month.");
        assert_eq!(got[0].quote, "The coordinator decides after 2 rounds, not 3.");
        assert!(candidates(&run(three[..2].to_vec()), &s, &none).is_empty(), "two tasks are not three");
        let old: Vec<TaskFacts> = (0..3).map(|i| task(now - 40 * DAY_MS - i, &["api", "ui", "verifier", "docs"], stuck())).collect();
        assert!(candidates(&run(old), &s, &none).is_empty(), "not this month");
        // A kept item that took three looks did not hit the limit.
        let kept: Vec<TaskFacts> = (0..3).map(|i| task(now - i, &["api", "ui", "verifier", "docs"], vec![look("api", 3, true, false)])).collect();
        assert!(candidates(&run(kept), &s, &none).is_empty());
        let mut two = s.team("shop-team").unwrap();
        two.limits.rounds = 2;
        s.save_team(two, "you", "").unwrap();
        assert!(candidates(&run(three), &s, &none).is_empty(), "already two");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_member_the_person_tried_to_speak_to_in_three_chats_offers_a_talks_line() {
        let (s, dir) = store();
        let team = shop(&s);
        let asked = |who: &[(&str, &str)]| {
            about(TeamFacts { slug: team.slug.clone(), asked: who.iter().map(|(c, r)| (c.to_string(), r.to_string())).collect(), ..Default::default() })
        };
        let got = candidates(&asked(&[("a", "ui"), ("b", "ui"), ("c", "ui")]), &s, &none);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].change, Change::TeamTalks { team: "shop-team".into(), role: "ui".into() });
        assert_eq!(got[0].why, "you tried to speak to ui in 3 different chats this month.");
        assert!(candidates(&asked(&[("a", "ui"), ("a", "ui"), ("a", "ui")]), &s, &none).is_empty(), "one chat is one chat");
        assert!(candidates(&asked(&[("a", "docs"), ("b", "docs"), ("c", "docs")]), &s, &none).is_empty(), "a helper has no voice");
        let mut talks = s.team("shop-team").unwrap();
        talks.lines.push(Line::new(LineKind::Talks, "you", "ui"));
        s.save_team(talks, "you", "").unwrap();
        assert!(candidates(&asked(&[("a", "ui"), ("b", "ui"), ("c", "ui")]), &s, &none).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_way_of_working_the_person_keeps_choosing_offers_to_become_the_teams_own() {
        let (s, dir) = store();
        let team = shop(&s);
        let chose = |styles: &[(&str, &str)]| {
            about(TeamFacts { slug: team.slug.clone(), styles: styles.iter().map(|(c, st)| (c.to_string(), st.to_string())).collect(), ..Default::default() })
        };
        let got = candidates(&chose(&[("a", "mission-command"), ("b", "mission-command"), ("c", "mission-command")]), &s, &none);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].change, Change::TeamStyle { team: "shop-team".into(), style: "mission-command".into() });
        assert_eq!(got[0].quote, "Shop team works Mission Command unless you say otherwise.");
        assert!(candidates(&chose(&[("a", "mission-command"), ("b", "mission-command")]), &s, &none).is_empty());
        assert!(candidates(&chose(&[("a", "lean"), ("b", "mission-command"), ("c", "swarm")]), &s, &none).is_empty(), "three different ones");
        assert!(candidates(&chose(&[("a", "nonesuch"), ("b", "nonesuch"), ("c", "nonesuch")]), &s, &none).is_empty(), "no such style");
        let mut own = s.team("shop-team").unwrap();
        own.style = "mission-command".into();
        s.save_team(own, "you", "").unwrap();
        assert!(candidates(&chose(&[("a", "mission-command"), ("b", "mission-command"), ("c", "mission-command")]), &s, &none).is_empty(), "it is already so");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_change_to_a_team_is_a_version_by_evolution_and_can_be_put_back_until_the_team_changes() {
        let (s, dir) = store();
        let team = shop(&s);
        let v = team.version;
        let out = Change::TeamTakeOut { team: "shop-team".into(), role: "docs".into() };
        let done = apply(&s, &out, "yes", "docs got no work").unwrap();
        assert_eq!(done.said, "took docs out of Shop team");
        let now = s.team("shop-team").unwrap();
        assert_eq!(now.version, v + 1);
        assert!(!now.members.iter().any(|m| m.role == "docs") && !now.lines.iter().any(|l| l.from == "docs" || l.to == "docs"), "its lines go with it");
        let h = s.team_history("shop-team");
        assert_eq!((h[0].by.as_str(), h[0].why.as_str()), ("evolution", "docs got no work"));
        undo(&s, &done.undo).unwrap();
        let back = s.team("shop-team").unwrap();
        assert!(back.members.iter().any(|m| m.role == "docs") && back.lines.contains(&Line::new(LineKind::HandsOver, "coordinator", "docs")));
        assert_eq!(back.version, v + 2, "putting it back is a new version");

        // Each of the others, once; the team being edited since makes an undo step aside.
        let asks = apply(&s, &Change::TeamAsks { team: "shop-team".into(), from: "api".into(), to: "verifier".into() }, "yes", "").unwrap();
        assert!(s.team("shop-team").unwrap().lines.contains(&Line::new(LineKind::Asks, "api", "verifier")));
        assert!(apply(&s, &Change::TeamAsks { team: "shop-team".into(), from: "api".into(), to: "verifier".into() }, "yes", "").unwrap_err().contains("already"));
        apply(&s, &Change::TeamRounds { team: "shop-team".into(), rounds: 2 }, "yes", "").unwrap();
        assert_eq!(s.team("shop-team").unwrap().limits.rounds, 2);
        assert!(undo(&s, &asks.undo).unwrap_err().contains("changed since"));
        apply(&s, &Change::TeamTalks { team: "shop-team".into(), role: "ui".into() }, "yes", "").unwrap();
        assert!(s.team("shop-team").unwrap().lines.contains(&Line::new(LineKind::Talks, "you", "ui")));
        apply(&s, &Change::TeamStyle { team: "shop-team".into(), style: "swarm".into() }, "yes", "").unwrap();
        assert_eq!(s.team("shop-team").unwrap().style, "swarm");
        // A member that left, or a style that is gone, is said.
        assert!(apply(&s, &Change::TeamTakeOut { team: "shop-team".into(), role: "nobody".into() }, "yes", "").is_err());
        assert!(apply(&s, &Change::TeamStyle { team: "shop-team".into(), style: "nonesuch".into() }, "yes", "").is_err());
        assert!(apply(&s, &Change::TeamRounds { team: "gone".into(), rounds: 2 }, "yes", "").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_team_proposal_is_stored_under_its_own_target_and_never_asked_twice() {
        let (s, dir) = store();
        let team = shop(&s);
        let facts = about(TeamFacts { slug: team.slug.clone(), tasks: five_tasks(&["api", "ui", "verifier"]), ..Default::default() });
        let c = candidates(&facts, &s, &none).remove(0);
        let db = Db::open_in_memory().unwrap();
        raise(&db, &c).unwrap();
        let p = db.list_change_proposals().unwrap().remove(0);
        assert_eq!((p.target.as_str(), p.slug.as_deref()), ("team", Some("team:shop-team:out:docs")));
        assert!(is_target(&p.target) && is_target("way") && !is_target("soul"));
        assert!(db.way_proposal_exists("team:shop-team:out:docs"), "answered or not, it is not proposed again");
        assert_eq!(read(&p.proposed_text).unwrap().change, c.change);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_daily_pass_reads_a_teams_own_chats_and_never_a_locked_teams() {
        use crate::agent::leader::{set_leader, Leader};
        let (s, dir) = store();
        let team = shop(&s);
        let db = Db::open_in_memory().unwrap();
        for _ in 0..3 {
            let chat = db.create_conversation("c", None, false).unwrap().id;
            set_leader(&db, &s, &chat, Some(&Leader::Team(team.slug.clone()))).unwrap();
            // The person first chose Lean here, then Swarm: the latest choice counts.
            assert!(crate::agent::team::note_style_chosen(&db, &s, &chat, "lean"));
            assert!(crate::agent::team::note_style_chosen(&db, &s, &chat, "swarm"));
            assert!(crate::agent::team::note_asked_without_line(&db, &s, &chat, "ui"));
        }
        // A chat nobody's team led, with a style, is nobody's signal.
        let plain = db.create_conversation("p", None, false).unwrap().id;
        assert!(!crate::agent::team::note_style_chosen(&db, &s, &plain, "mission-command"));
        // A scheduled task's runs carry its style, set by the task, not chosen by the person each time.
        for _ in 0..3 {
            let run = db.create_conversation("run", None, false).unwrap().id;
            set_leader(&db, &s, &run, Some(&Leader::Team(team.slug.clone()))).unwrap();
            db.set_conversation_style(&run, Some("mission-command")).unwrap();
        }

        let changes: Vec<Change> = due(&db, &s, now_ms()).into_iter().map(|c| c.change).collect();
        assert!(changes.contains(&Change::TeamStyle { team: "shop-team".into(), style: "swarm".into() }), "{changes:?}");
        assert!(changes.contains(&Change::TeamTalks { team: "shop-team".into(), role: "ui".into() }), "{changes:?}");
        assert!(!changes.iter().any(|c| matches!(c, Change::TeamStyle { style, .. } if style != "swarm")), "{changes:?}");
        // Each is raised once, and a locked team is not even read.
        for c in due(&db, &s, now_ms()) {
            raise(&db, &c).unwrap();
        }
        assert!(due(&db, &s, now_ms()).is_empty(), "No stays no, and so does Yes");
        s.save_team(Team { locked: true, ..s.team("shop-team").unwrap() }, "you", "").unwrap();
        assert!(Facts::gather_teams(&db, &s, now_ms()).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_members_place_began_with_the_oldest_version_it_has_been_in_without_a_gap() {
        let (s, dir) = store();
        let team = shop(&s);
        let since = |t: &Team, role: &str| member_since(&s, t).into_iter().find(|(r, _)| r == role).unwrap().1;
        assert!(since(&team, "api") > 0 && since(&team, "docs") > 0);
        let mut changed = s.team("shop-team").unwrap();
        changed.members.retain(|m| m.role != "docs");
        changed.lines.retain(|l| l.from != "docs" && l.to != "docs");
        let changed = s.save_team(changed, "you", "").unwrap();
        assert!(member_since(&s, &changed).iter().all(|(r, _)| r != "docs"));
        assert!(since(&changed, "api") <= since(&team, "api"), "api was in every version");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_was_done_lately_is_kept_for_a_while_and_taken_when_undone() {
        let db = Db::open_in_memory().unwrap();
        let now = now_ms();
        note(&db, &Applied { said: "started x".into(), undo: Undo::Nothing }, true, now);
        note(&db, &Applied { said: "old".into(), undo: Undo::Nothing }, false, now - 40 * DAY_MS);
        let list = recent(&db, now);
        assert_eq!(list.len(), 1, "a month later it is no longer lately");
        assert!(take(&db, &list[0].id, now).is_some());
        assert!(recent(&db, now).is_empty());
    }
}
