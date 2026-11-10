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

use serde::{Deserialize, Serialize};

use crate::agent::episode::DONE;
use crate::agent::work::Signal;
use crate::db::{now_ms, Db};
use crate::memory::ways::{sentence, Move, PresetRule, Scope, ScopeCtx, Status, Trigger, AD_HOC};
use crate::memory::MemoryStore;

/// The `change_proposals.target` for these.
pub const TARGET: &str = "way";
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

/// Everything evolution reads, so the rules can be tested without a database.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub signals: Vec<Sig>,
    /// Conversations where file changes were put back.
    pub undone: Vec<String>,
    pub ended: Vec<Ended>,
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
        Facts { signals, undone: db.undone_conversations_since(now - WINDOW_MS).unwrap_or_default(), ended }
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
    out
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "undo", rename_all = "snake_case")]
pub enum Undo {
    RemoveWay { slug: String },
    SetStatus { slug: String, status: String },
    PresetRule { preset: String, when: Trigger, #[serde(rename = "move")] mv: Move, on: bool },
    Default { scope: String, before: Option<String> },
    Offspring { slug: String, scope: String, before: Option<String> },
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
        Undo::Nothing => Ok(()),
    }
}

// ---- proposals ----

/// Write a candidate down as a proposal waiting for a yes. Returns its id.
pub fn raise(db: &Db, c: &Candidate) -> Result<String, String> {
    let payload = serde_json::to_string(&c.change).map_err(|e| e.to_string())?;
    let text = serde_json::json!({ "lead": c.lead, "quote": c.quote, "change": serde_json::from_str::<serde_json::Value>(&payload).unwrap_or_default() });
    db.add_change_proposal(TARGET, Some(&c.subject), &text.to_string(), &c.why, Some(&c.quote))
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    let facts = Facts::gather(db, now);
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
