//! The improvement step (`IMP-1` to `IMP-5`, `COGNITION_PLAN` III.9).
//!
//! Once a day, for at most two problems that keep coming back: draft up to three
//! fixes of different kinds, put each and the unchanged setup through the
//! rehearsal runner on up to three replayed conversations, and keep the one that
//! does better than the setup I already have, or none. What is kept waits for the
//! person's yes on the Lessons tab. The rest stay as rows nobody is shown.
//!
//! Every fix is an ordinary `change_proposals` row (`add_improvement`): the same
//! store, the same card, the same accept path as any other proposal. Nothing here
//! applies a change by itself.
//!
//! What this build drafts by rules and not by a model: the three candidates (their
//! words come from the lesson, the failed call and the fixed call), and the rules of
//! thumb. A model's drafting can replace the first later; the probe and the compose
//! step would not change.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::golden::{self, Attempt, Rehearsal};
use crate::agent::log::{CommandTrace, COMMAND_KIND};
use crate::autonomy::{autonomy_gate, Rung};
use crate::db::{ChangeProposal, Db, ToolFix};
use crate::memory::{Fact, MemoryStore};

const DAY_MS: i64 = 86_400_000;
/// How far back a problem is looked for.
const WINDOW_DAYS: i64 = 90;
/// A problem that was dealt with, kept as is, or is waiting is not taken up again for this long.
const RECENT_DAYS: i64 = 14;
/// Replayed conversations per problem, and problems per pass.
const MAX_MOMENTS: usize = 3;
const MAX_PROBLEMS: usize = 2;
const MAX_CANDIDATES: usize = 3;
/// A fix is judged on how the problem went after this long.
const JUDGE_AFTER_DAYS: i64 = 14;
/// Rules of thumb are rewritten once a month, and there are never more than five.
const RULES_EVERY_DAYS: i64 = 30;
const MAX_RULES: usize = 5;
/// Outcomes needed in a form before a rule may say anything about it.
const RULE_MIN: usize = 3;
/// Agreeing times in a row before an area is trusted.
const TRUST_STREAK: usize = 5;

pub const RULES_KEY: &str = "improve.rules";
const RULES_AT_KEY: &str = "improve.rules_at";
const RULES_DELETED_KEY: &str = "improve.rules_deleted";

/// The forms a fix takes, in the order they are drafted when nothing has been learned.
const FORMS: [&str; 4] = ["reminder", "check", "instruction", "earlier"];

// ---- IMP-1: the problem queue ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// The person had to say it twice. First in the queue.
    ToldTwice,
    /// A lesson I learned again.
    Lesson,
    /// A kind of task that keeps failing or being given up.
    TaskKind,
}

/// A call that failed and was fixed in one conversation: a moment to replay.
#[derive(Debug, Clone)]
pub struct Moment {
    pub conversation_id: String,
    pub fix: ToolFix,
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub source: Source,
    /// The problem in words. Also how a problem is told from one already handled.
    pub cue: String,
    pub tool: Option<String>,
    pub lesson: Option<Fact>,
    pub moments: Vec<Moment>,
    /// What the corrections or the failures said, for a problem with no lesson behind it.
    pub note: String,
}

/// The same words in any case and spacing are one cue.
pub fn cue_key(cue: &str) -> String {
    cue.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ").to_lowercase()
}

fn tool_of(cue: Option<&str>) -> Option<String> {
    cue?.trim().strip_prefix("tool:").map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// Up to `MAX_MOMENTS` conversations in which `tool` failed and was fixed, newest first.
fn moments_for(db: &Db, tool: &str) -> Vec<Moment> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for row in db.tool_fix_log().unwrap_or_default().into_iter().rev() {
        if row.tool_name != tool || !seen.insert(row.conversation_id.clone()) {
            continue;
        }
        if let Some(fix) = db.tool_fixes_in(&row.conversation_id).unwrap_or_default().into_iter().find(|f| f.tool_name == tool) {
            out.push(Moment { conversation_id: row.conversation_id, fix });
        }
        if out.len() == MAX_MOMENTS {
            break;
        }
    }
    out
}

/// Everything that qualifies, told-twice first, one per cue. Problems already handled
/// are not removed here (`handled` does that).
pub fn problems(db: &Db, mem: &MemoryStore, now: i64) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::new();

    // Told twice: the same correction at least twice in the window.
    let mut told: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for row in db.session_events_of_kind_between(COMMAND_KIND, now - WINDOW_DAYS * DAY_MS, now + 1).unwrap_or_default() {
        let Ok(t) = serde_json::from_str::<CommandTrace>(&row.payload_json) else { continue };
        let (key, line) = if t.name == "forget" && t.by == "user" {
            let subject = t.note.as_deref().unwrap_or(&t.args).trim().to_string();
            (format!("forget {}", cue_key(&subject)), format!("The user has asked me more than once to drop \"{subject}\". Do not bring it up or save it again."))
        } else if t.name == "suggest" && t.outcome == "declined" {
            let subject = t.args.trim().to_string();
            (format!("suggest {}", cue_key(&subject)), format!("The user has declined my suggestion \"{subject}\" more than once. Do not suggest it again."))
        } else {
            continue;
        };
        let e = told.entry(key).or_insert((0, line));
        e.0 += 1;
    }
    for (_, (n, line)) in told {
        if n >= 2 {
            out.push(Problem {
                source: Source::ToldTwice,
                cue: format!("you had to tell me twice: {}", line.split('"').nth(1).unwrap_or("this")),
                tool: None,
                lesson: None,
                moments: Vec::new(),
                note: line,
            });
        }
    }

    // A lesson learned again.
    for lesson in mem.list_lessons() {
        if lesson.recurrence.unwrap_or(1) < 2 {
            continue;
        }
        let tool = tool_of(lesson.cue.as_deref());
        let moments = tool.as_deref().map(|t| moments_for(db, t)).unwrap_or_default();
        out.push(Problem {
            source: Source::Lesson,
            cue: lesson.description.trim().to_string(),
            tool,
            note: lesson.body.clone(),
            moments,
            lesson: Some(lesson),
        });
    }

    // A kind of task that keeps going wrong.
    let mut kinds: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in db.episodes_since(now - WINDOW_DAYS * DAY_MS).unwrap_or_default() {
        if matches!(e.outcome.as_deref(), Some("failed" | "abandoned")) {
            if let Some(kind) = e.task_kind.filter(|k| !k.trim().is_empty() && k != crate::agent::episode::UNKNOWN) {
                kinds.entry(kind).or_default().push(e.went_wrong.unwrap_or_default());
            }
        }
    }
    for (kind, wrong) in kinds {
        if wrong.len() >= 2 {
            let lines: Vec<String> = wrong.into_iter().map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).take(2).collect();
            out.push(Problem {
                source: Source::TaskKind,
                cue: format!("{kind} tasks keep going wrong"),
                tool: None,
                lesson: None,
                moments: Vec::new(),
                note: if lines.is_empty() { format!("{kind} tasks have gone wrong more than once. Take extra care with them.") } else { format!("When doing {kind} tasks, watch for: {}", lines.join("; ")) },
            });
        }
    }

    out.sort_by_key(|p| p.source);
    let mut seen = HashSet::new();
    out.retain(|p| seen.insert(cue_key(&p.cue)));
    out
}

/// Was this problem filed lately, or is a fix for it still waiting?
pub fn handled(rows: &[ChangeProposal], cue: &str, now: i64) -> bool {
    let key = cue_key(cue);
    rows.iter().any(|r| {
        r.problem_cue.as_deref().map(cue_key).as_deref() == Some(key.as_str())
            && (r.status == "pending" || r.created_at >= now - RECENT_DAYS * DAY_MS)
    })
}

// ---- IMP-2: candidates ----

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub form: String,
    pub target: String,
    pub slug: Option<String>,
    /// The row's `proposed_text`.
    pub text: String,
    /// What the rehearsal puts in the prompt: the words as the model would meet them.
    pub hint: String,
    pub description: String,
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('\u{2026}');
    t
}

/// A fix's name in the person's words.
pub fn form_label(form: &str) -> &'static str {
    match form {
        "check" => "a check on the tool",
        "reminder" => "a written reminder",
        "instruction" => "a line in the project's instructions",
        "earlier" => "what worked before",
        "merged" => "two of these together",
        _ => "the setup as it is",
    }
}

/// Up to three fixes of different kinds. `rows` are the earlier improvements, read
/// first: what has held for this tool is drafted first, and "what already worked" is
/// taken from there.
pub fn draft(problem: &Problem, project_instructions: Option<(String, Option<String>)>, rows: &[ChangeProposal]) -> Vec<Candidate> {
    let moment = problem.moments.first();
    let tool = problem.tool.as_deref();
    let mut all: Vec<Candidate> = Vec::new();

    let reminder_body = match (&problem.lesson, moment) {
        (Some(l), Some(m)) => format!("{} Last time {} failed with \"{}\" when called like {}.", l.body.trim(), m.fix.tool_name, clip(&m.fix.error, 120), clip(&m.fix.failed_args, 160)),
        (Some(l), None) => l.body.trim().to_string(),
        _ => problem.note.clone(),
    };
    let reminder_slug = problem
        .lesson
        .as_ref()
        .map(|l| format!("{}-again", l.name))
        .or_else(|| crate::memory::slugify(&problem.cue).ok());
    all.push(Candidate {
        form: "reminder".into(),
        target: "lesson".into(),
        slug: reminder_slug,
        text: reminder_body.clone(),
        hint: reminder_body,
        description: clip(&problem.cue, 120),
    });

    if let (Some(lesson), Some(tool), Some(m)) = (&problem.lesson, tool, moment) {
        let check = crate::agent::habit::check_for(&m.fix.error);
        let look = check.map(|c| format!(" I look at {} after it runs.", crate::agent::habit::label(c))).unwrap_or_default();
        let note = format!("What worked with {tool}: {}.{look}", clip(&m.fix.fixed_args, 200));
        all.push(Candidate {
            form: "check".into(),
            target: "habit".into(),
            slug: Some(lesson.name.clone()),
            text: json!({ "on": tool, "check": check, "note": note }).to_string(),
            hint: format!("{}\n{note}", lesson.body.trim()),
            description: lesson.description.clone(),
        });
    }

    if let (Some((project, current)), Some(tool), Some(m)) = (project_instructions, tool, moment) {
        let line = format!("When you use {tool}, call it like {}, not like {}.", clip(&m.fix.fixed_args, 160), clip(&m.fix.failed_args, 160));
        let text = match current.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(c) => format!("{c}\n- {line}"),
            None => format!("- {line}"),
        };
        all.push(Candidate { form: "instruction".into(), target: "project_instructions".into(), slug: Some(project), text, hint: line, description: clip(&problem.cue, 120) });
    }

    // What already worked: an earlier fix for the same problem that held. Read as data;
    // the words are not trusted beyond being offered again for a yes.
    let key = cue_key(&problem.cue);
    if let Some(r) = rows.iter().find(|r| {
        r.status == "applied" && r.live_outcome.as_deref() == Some("held") && r.problem_cue.as_deref().map(cue_key).as_deref() == Some(key.as_str())
    }) {
        let hint = r.rehearsal_json.as_deref().and_then(|j| serde_json::from_str::<Value>(j).ok()).and_then(|v| v["hint"].as_str().map(str::to_string));
        let form = r.form.clone().unwrap_or_default();
        if let Some(hint) = hint {
            if !all.iter().any(|c| c.form == form) {
                all.push(Candidate { form: "earlier".into(), target: r.target.clone(), slug: r.slug.clone(), text: r.proposed_text.clone(), hint, description: r.description.clone().unwrap_or_default() });
            }
        }
    }

    let order = form_order(rows, tool);
    all.sort_by_key(|c| order.iter().position(|f| *f == c.form).unwrap_or(order.len()));
    all.truncate(MAX_CANDIDATES);
    all
}

// ---- IMP-3: the probe ----

/// How each fix and the unchanged setup did, in right calls out of `tries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    pub tries: usize,
    pub base_right: usize,
    pub right: Vec<usize>,
}

/// Every moment, every sample, the unchanged setup and each fix. Nothing is run:
/// `ask` returns the first tool call a model would make, and that is all.
pub async fn probe<F, Fut>(moments: &[Rehearsal], hints: &[String], ask: &mut F) -> Probed
where
    F: FnMut(Vec<Value>, f32) -> Fut,
    Fut: std::future::Future<Output = Option<Attempt>>,
{
    let mut out = Probed { tries: 0, base_right: 0, right: vec![0; hints.len()] };
    for m in moments {
        for temp in golden::REHEARSAL_TEMPS {
            out.tries += 1;
            if ask(m.messages.clone(), temp).await.is_some_and(|a| golden::is_right(m, &a)) {
                out.base_right += 1;
            }
            for (i, hint) in hints.iter().enumerate() {
                if ask(golden::with_candidate(&m.messages, hint), temp).await.is_some_and(|a| golden::is_right(m, &a)) {
                    out.right[i] += 1;
                }
            }
        }
    }
    out
}

// ---- IMP-4: compose ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Nothing did better than the setup as it is.
    Base,
    One(usize),
    /// The two best, both written words, tried together.
    Pair(usize, usize),
}

/// Adopt the fix that beat the unchanged setup by most (ties go to the one drafted
/// first), try two written ones together when the best was not perfect, or keep as is.
pub fn pick(probed: &Probed, cands: &[Candidate]) -> Pick {
    let mut beating: Vec<usize> = (0..cands.len()).filter(|&i| probed.right[i] > probed.base_right).collect();
    beating.sort_by(|&a, &b| probed.right[b].cmp(&probed.right[a]).then(a.cmp(&b)));
    let Some(&best) = beating.first() else { return Pick::Base };
    if probed.right[best] < probed.tries {
        if let Some(&second) = beating.get(1) {
            if cands[best].target == "lesson" && cands[second].target == "lesson" {
                return Pick::Pair(best, second);
            }
        }
    }
    Pick::One(best)
}

fn replays_line(right: usize, tries: usize) -> String {
    format!("right in {right} of {tries} replays")
}

/// File the outcome of one problem. Returns `(proposed, kept_base)`.
#[allow(clippy::too_many_arguments)]
async fn compose_and_file<F, Fut>(
    db: &Db,
    problem: &Problem,
    cands: &[Candidate],
    replays: &[Rehearsal],
    probed: &Probed,
    ask: &mut F,
) -> (usize, usize)
where
    F: FnMut(Vec<Value>, f32) -> Fut,
    Fut: std::future::Future<Output = Option<Attempt>>,
{
    let group = uuid::Uuid::new_v4().simple().to_string();
    let conversations: Vec<&str> = problem.moments.iter().map(|m| m.conversation_id.as_str()).collect();
    let (tries, base) = (probed.tries, probed.base_right);
    let meta = |cand_hint: &str, right: usize, evidence: &str| {
        json!({
            "rehearsed": true,
            "tool": problem.tool,
            "hint": cand_hint,
            "replays": replays.len(),
            "tries": tries,
            "right": right,
            "base_right": base,
            "verdict": golden::verdict_of(base, right, tries),
            "conversations": conversations,
            "evidence": evidence,
        })
        .to_string()
    };
    let rationale = format!("{} came up again. I tried {} fixes on the chats where it happened.", clip(&problem.cue, 100), cands.len());
    let file = |c: &Candidate, status: &str, right: usize, word: &str, label: &str, text: &str| {
        let evidence = format!("{word} -> {label} -> {} (without it: {base} of {tries})", replays_line(right, tries));
        let _ = db.add_improvement(&c.target, c.slug.as_deref(), text, &rationale, Some(&c.description), status, &problem.cue, &c.form, &group, &meta(&c.hint, right, &evidence));
    };

    match pick(probed, cands) {
        Pick::Base => {
            let summary = cands.iter().enumerate().map(|(i, c)| format!("{}: {}", form_label(&c.form), replays_line(probed.right[i], tries))).collect::<Vec<_>>().join("; ");
            let evidence = format!("kept as is -> nothing beat the setup I have (right in {base} of {tries} replays) -> {summary}");
            let rj = json!({ "rehearsed": true, "tool": problem.tool, "replays": replays.len(), "tries": tries, "base_right": base, "evidence": evidence, "conversations": conversations }).to_string();
            let _ = db.add_improvement("improve", None, "", &format!("{} came up again, and none of my fixes did better than what I already do.", clip(&problem.cue, 100)), None, "kept_base", &problem.cue, "base", &group, &rj);
            for (i, c) in cands.iter().enumerate() {
                file(c, "dropped", probed.right[i], "dropped", form_label(&c.form), &c.text);
            }
            (0, 1)
        }
        Pick::One(best) => {
            for (i, c) in cands.iter().enumerate() {
                if i == best {
                    file(c, "pending", probed.right[i], "kept", form_label(&c.form), &c.text);
                } else {
                    file(c, "dropped", probed.right[i], "dropped", form_label(&c.form), &c.text);
                }
            }
            (1, 0)
        }
        Pick::Pair(a, b) => {
            // Two written reminders, tried once more as one.
            let joined = format!("{}\n{}", cands[a].hint, cands[b].hint);
            let again = probe(replays, std::slice::from_ref(&joined), ask).await;
            let merged_right = again.right[0];
            if merged_right > probed.right[a] {
                let merged = Candidate {
                    form: "merged".into(),
                    target: "lesson".into(),
                    slug: cands[a].slug.clone(),
                    text: format!("{}\n{}", cands[a].text, cands[b].text),
                    hint: joined,
                    description: cands[a].description.clone(),
                };
                file(&merged, "pending", merged_right, "merged", &format!("{} and {}", form_label(&cands[a].form), form_label(&cands[b].form)), &merged.text);
                for (i, c) in cands.iter().enumerate() {
                    file(c, "dropped", probed.right[i], "dropped", form_label(&c.form), &c.text);
                }
            } else {
                for (i, c) in cands.iter().enumerate() {
                    if i == a {
                        file(c, "pending", probed.right[i], "kept", form_label(&c.form), &c.text);
                    } else {
                        file(c, "dropped", probed.right[i], "dropped", form_label(&c.form), &c.text);
                    }
                }
            }
            (1, 0)
        }
    }
}

/// What one pass of the step did, for the digest. It claims nothing else.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    pub problems: usize,
    /// Fixes waiting for a yes.
    pub proposed: usize,
    /// Problems where nothing beat what I already do.
    pub kept_base: usize,
    pub stopped: bool,
}

/// The step, with the model behind `ask`. `stop` is the person's input ending a pass
/// that started by itself; it is looked at before each problem.
pub async fn improve_with<F, Fut>(db: &Db, mem: &MemoryStore, now: i64, stop: &(dyn Fn() -> bool + Sync), ask: &mut F) -> Outcome
where
    F: FnMut(Vec<Value>, f32) -> Fut,
    Fut: std::future::Future<Output = Option<Attempt>>,
{
    let mut out = Outcome::default();
    if autonomy_gate(db, "lessons") == Rung::Off {
        return out;
    }
    fill_live_outcomes(db, now);
    refresh_rules(db, now, false);
    let rows = db.list_improvements().unwrap_or_default();
    let queue: Vec<Problem> = problems(db, mem, now).into_iter().filter(|p| !handled(&rows, &p.cue, now)).take(MAX_PROBLEMS).collect();
    for problem in queue {
        if stop() {
            out.stopped = true;
            break;
        }
        out.problems += 1;
        let rows = db.list_improvements().unwrap_or_default();
        let project = problem.moments.first().and_then(|m| db.conversation_project(&m.conversation_id).ok().flatten()).map(|p| (p.id, p.instructions));
        let cands = draft(&problem, project, &rows);
        if cands.is_empty() {
            continue;
        }
        let replays: Vec<Rehearsal> = problem.moments.iter().filter_map(|m| golden::build_rehearsal(db, &m.conversation_id, &m.fix)).collect();
        if replays.is_empty() {
            // Nothing to replay: the first fix is offered as it is, and says it was not tried.
            let c = &cands[0];
            let rj = json!({ "rehearsed": false, "tool": problem.tool, "hint": c.hint, "evidence": format!("kept -> {} -> not tried: I have no conversation to replay for this", form_label(&c.form)) }).to_string();
            let group = uuid::Uuid::new_v4().simple().to_string();
            let rationale = format!("{} came up again. I have no chat to try a fix on, so this one is untried.", clip(&problem.cue, 100));
            if db.add_improvement(&c.target, c.slug.as_deref(), &c.text, &rationale, Some(&c.description), "pending", &problem.cue, &c.form, &group, &rj).is_ok() {
                out.proposed += 1;
            }
            continue;
        }
        let hints: Vec<String> = cands.iter().map(|c| c.hint.clone()).collect();
        let probed = probe(&replays, &hints, ask).await;
        let (proposed, kept) = compose_and_file(db, &problem, &cands, &replays, &probed, ask).await;
        out.proposed += proposed;
        out.kept_base += kept;
    }
    out
}

/// The step against the loaded or chosen model: the same tool table the golden set uses,
/// nothing dispatched.
pub async fn improve(mgr: &crate::runtime::RuntimeManager, db: &Db, mem: &MemoryStore, endpoint: Option<(String, String)>, stop: &(dyn Fn() -> bool + Sync)) -> Outcome {
    let Some((base_url, token)) = endpoint else { return Outcome::default() };
    let endpoint = crate::cloud::ChatEndpoint::OpenAi { base_url, api_key: Some(token), model: None };
    let specs: Vec<Value> = crate::agent::toolsets::enabled_for_persona(db, None).into_iter().flat_map(|t| t.tool_specs()).collect();
    let client = &mgr.client;
    let mut ask = |messages: Vec<Value>, temp: f32| {
        let (endpoint, specs) = (&endpoint, &specs);
        async move {
            match crate::cloud::drive_turn(client, endpoint, &messages, specs, temp, crate::cloud::Effort::Off, &crate::runtime::proxy::CancelFlag::new(), |_| {}).await {
                Ok(crate::runtime::proxy::TurnOutcome::ToolCalls { calls, .. }) => calls.into_iter().next().map(|c| Attempt { tool: c.name, args: c.arguments }),
                _ => None,
            }
        }
    };
    improve_with(db, mem, crate::db::now_ms(), stop, &mut ask).await
}

// ---- IMP-5: the improver learns ----

fn tool_of_row(r: &ChangeProposal) -> Option<String> {
    serde_json::from_str::<Value>(r.rehearsal_json.as_deref()?).ok()?["tool"].as_str().map(str::to_string)
}

/// When an improvement was applied, as `resolve_change_proposal` stamped it.
fn applied_at(r: &ChangeProposal) -> Option<i64> {
    serde_json::from_str::<Value>(r.rehearsal_json.as_deref()?).ok()?["applied_at"].as_i64()
}

/// How an applied fix held up: `held` when the tool did not need fixing again in a
/// later conversation, `failed` when it did. Read from the fix log, not from a model.
pub fn fill_live_outcomes(db: &Db, now: i64) -> usize {
    let fixes = db.tool_fix_log().unwrap_or_default();
    let mut filled = 0;
    for r in db.list_improvements().unwrap_or_default() {
        // From when it took effect: failures while it waited to be accepted are not its own.
        let since = applied_at(&r).unwrap_or(r.created_at);
        if r.status != "applied" || r.live_outcome.is_some() || since > now - JUDGE_AFTER_DAYS * DAY_MS {
            continue;
        }
        let Some(tool) = tool_of_row(&r) else { continue };
        let again = fixes.iter().any(|f| f.tool_name == tool && f.created_at > since);
        if db.set_live_outcome(&r.id, if again { "failed" } else { "held" }).is_ok() {
            filled += 1;
        }
    }
    filled
}

/// `(held, total)` per form, over applied fixes whose outcome is known. `tool` narrows
/// to one area when it has any.
pub fn form_stats(rows: &[ChangeProposal], tool: Option<&str>) -> BTreeMap<String, (usize, usize)> {
    let tally = |only: Option<&str>| {
        let mut m: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for r in rows.iter().filter(|r| r.status == "applied") {
            let (Some(form), Some(outcome)) = (r.form.as_deref(), r.live_outcome.as_deref()) else { continue };
            if only.is_some() && tool_of_row(r).as_deref() != only {
                continue;
            }
            let e = m.entry(form.to_string()).or_default();
            e.1 += 1;
            if outcome == "held" {
                e.0 += 1;
            }
        }
        m
    };
    let area = tool.map(|t| tally(Some(t))).unwrap_or_default();
    if area.is_empty() { tally(None) } else { area }
}

/// The forms in the order to draft them: what has held most often first, then the
/// default order.
pub fn form_order(rows: &[ChangeProposal], tool: Option<&str>) -> Vec<&'static str> {
    let stats = form_stats(rows, tool);
    let mut order = FORMS.to_vec();
    let score = |f: &str| stats.get(f).map(|&(h, n)| (h as f32 + 1.0) / (n as f32 + 2.0)).unwrap_or(0.0);
    // Only forms with a record move; the rest keep their place.
    if stats.is_empty() {
        return order;
    }
    order.sort_by(|a, b| {
        let (sa, sb) = (stats.contains_key(*a), stats.contains_key(*b));
        match (sa, sb) {
            (true, true) => score(b).total_cmp(&score(a)),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => std::cmp::Ordering::Equal,
        }
    });
    order
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// `<form that works>><form that does not>`, for deleting it.
    pub key: String,
    pub text: String,
    pub count: usize,
    pub of: usize,
}

fn plural(form: &str) -> &'static str {
    match form {
        "check" => "Checks",
        "reminder" => "Written reminders",
        "instruction" => "Project instructions",
        _ => "Copying an earlier fix",
    }
}

fn plural_lower(form: &str) -> &'static str {
    match form {
        "check" => "checks",
        "reminder" => "written reminders",
        "instruction" => "project instructions",
        _ => "an earlier fix",
    }
}

/// At most five sentences about which form holds up and which does not, from the rows.
pub fn derive_rules(rows: &[ChangeProposal]) -> Vec<Rule> {
    let stats = form_stats(rows, None);
    let mut out = Vec::new();
    for (a, &(ha, na)) in &stats {
        for (b, &(hb, nb)) in &stats {
            if a == b || na < RULE_MIN || nb < RULE_MIN || (ha as f32) / (na as f32) < 0.8 || (hb as f32) / (nb as f32) > 0.4 {
                continue;
            }
            out.push(Rule {
                key: format!("{a}>{b}"),
                text: format!("{} rarely fix how I call a tool. I use {} for that now. ({ha} of {na} times)", plural(b), plural_lower(a)),
                count: ha,
                of: na,
            });
        }
    }
    out.sort_by(|x, y| y.of.cmp(&x.of).then(x.key.cmp(&y.key)));
    out.truncate(MAX_RULES);
    out
}

pub fn load_rules(db: &Db) -> Vec<Rule> {
    db.get_setting(RULES_KEY).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn deleted_rules(db: &Db) -> BTreeMap<String, usize> {
    db.get_setting(RULES_DELETED_KEY).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Rewrite the rules of thumb: once a month, or at once with `force`. A rule the person
/// deleted comes back only when there are more rows behind it than when it was deleted.
pub fn refresh_rules(db: &Db, now: i64, force: bool) -> Vec<Rule> {
    let last: i64 = db.get_setting(RULES_AT_KEY).ok().flatten().and_then(|s| s.parse().ok()).unwrap_or(0);
    if !force && last > 0 && now - last < RULES_EVERY_DAYS * DAY_MS {
        return load_rules(db);
    }
    let deleted = deleted_rules(db);
    let rules: Vec<Rule> = derive_rules(&db.list_improvements().unwrap_or_default())
        .into_iter()
        .filter(|r| deleted.get(&r.key).is_none_or(|&of| r.of > of))
        .collect();
    let _ = db.set_setting(RULES_KEY, &serde_json::to_string(&rules).unwrap_or_default());
    let _ = db.set_setting(RULES_AT_KEY, &now.to_string());
    rules
}

/// The person deleted a rule: it stays gone until new rows say it again.
pub fn delete_rule(db: &Db, key: &str) {
    let rules = load_rules(db);
    if let Some(r) = rules.iter().find(|r| r.key == key) {
        let mut deleted = deleted_rules(db);
        deleted.insert(key.to_string(), r.of);
        let _ = db.set_setting(RULES_DELETED_KEY, &serde_json::to_string(&deleted).unwrap_or_default());
    }
    let kept: Vec<&Rule> = rules.iter().filter(|r| r.key != key).collect();
    let _ = db.set_setting(RULES_KEY, &serde_json::to_string(&kept).unwrap_or_default());
}

/// Whether rehearsal and what happened later agreed, for one area (a tool).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Agreement {
    pub area: String,
    pub agreed: usize,
    pub total: usize,
    /// Agreeing times in a row, newest back.
    pub streak: usize,
    /// Five in a row: the area may ship without asking when its rung is Auto. A
    /// disagreement in the latest row sends it back to Ask.
    pub trusted: bool,
}

pub fn agreement(rows: &[ChangeProposal]) -> Vec<Agreement> {
    let mut by_area: BTreeMap<String, Vec<bool>> = BTreeMap::new();
    let mut ordered: Vec<&ChangeProposal> = rows.iter().filter(|r| r.status == "applied" && r.live_outcome.is_some()).collect();
    ordered.sort_by_key(|r| r.created_at);
    for r in ordered {
        let Some(area) = tool_of_row(r) else { continue };
        let Some(v) = r.rehearsal_json.as_deref().and_then(|j| serde_json::from_str::<Value>(j).ok()) else { continue };
        if v["rehearsed"].as_bool() != Some(true) {
            continue;
        }
        // A fix is kept only when rehearsal said it helped, so "held" is the agreeing answer.
        let predicted_ok = v["verdict"].as_str() == Some("helped");
        let held = r.live_outcome.as_deref() == Some("held");
        by_area.entry(area).or_default().push(predicted_ok == held);
    }
    by_area
        .into_iter()
        .map(|(area, hits)| {
            let streak = hits.iter().rev().take_while(|&&h| h).count();
            Agreement { area, agreed: hits.iter().filter(|&&h| h).count(), total: hits.len(), streak, trusted: streak >= TRUST_STREAK }
        })
        .collect()
}

/// What the Lessons tab shows under "How I improve".
#[derive(Debug, Clone, Serialize)]
pub struct ImproveSummary {
    pub rules: Vec<Rule>,
    pub areas: Vec<Agreement>,
    /// Fixes that held, of those with an answer.
    pub hold_rate: Option<f32>,
    /// Rehearsal and what happened later, agreeing, of those with an answer.
    pub agreement_rate: Option<f32>,
    pub tried: usize,
}

pub fn summary(db: &Db) -> ImproveSummary {
    let rows = db.list_improvements().unwrap_or_default();
    let known: Vec<&ChangeProposal> = rows.iter().filter(|r| r.status == "applied" && r.live_outcome.is_some()).collect();
    let held = known.iter().filter(|r| r.live_outcome.as_deref() == Some("held")).count();
    let areas = agreement(&rows);
    let (agreed, total) = areas.iter().fold((0, 0), |(a, t), x| (a + x.agreed, t + x.total));
    ImproveSummary {
        rules: load_rules(db),
        areas,
        hold_rate: (!known.is_empty()).then(|| held as f32 / known.len() as f32),
        agreement_rate: (total > 0).then(|| agreed as f32 / total as f32),
        tried: rows.iter().map(|r| r.group_id.clone()).collect::<HashSet<_>>().len(),
    }
}

// ---- commands ----

type Cmd<T> = Result<T, crate::PoiesisError>;

/// The fixes that competed for one problem, whatever became of them, for the card.
#[tauri::command]
pub fn improvement_group_cmd(db: tauri::State<'_, Db>, group_id: String) -> Cmd<Vec<ChangeProposal>> {
    let mut rows: Vec<ChangeProposal> = db
        .list_improvements()
        .map_err(|e| crate::PoiesisError::Message(e.to_string()))?
        .into_iter()
        .filter(|r| r.group_id.as_deref() == Some(group_id.as_str()))
        .collect();
    rows.sort_by_key(|r| r.created_at);
    Ok(rows)
}

#[tauri::command]
pub fn improve_summary_cmd(db: tauri::State<'_, Db>) -> ImproveSummary {
    summary(&db)
}

#[tauri::command]
pub fn delete_improve_rule_cmd(db: tauri::State<'_, Db>, key: String) {
    delete_rule(&db, &key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const NAIVE: &str = r#"{"path":"data.csv"}"#;
    const SKILLED: &str = r#"{"path":"data.csv","has_header":true}"#;

    struct World {
        db: Db,
        mem: MemoryStore,
        conv: String,
        _dir: tempfile::TempDir,
    }

    /// A planted problem: a lesson learned twice about `read_table`, and one
    /// conversation where the call failed and was fixed.
    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let mem = MemoryStore::new(dir.path()).unwrap();
        let conv = db.create_conversation("Fail then fix", None, false).unwrap().id;
        let log = |kind: &str, v: Value| {
            db.append_session_event(&conv, Some("run1"), kind, &v).unwrap();
        };
        log("prompt", json!([{ "role": "system", "content": "You are Poiesis." }, { "role": "user", "content": "Summarise data.csv" }]));
        let call = |id: &str, args: &str| json!({ "role": "assistant", "tool_calls": [{ "id": id, "type": "function", "function": { "name": "read_table", "arguments": args } }] });
        log("tool_call", call("c1", NAIVE));
        log("tool_result", json!({ "role": "tool", "tool_call_id": "c1", "content": "Error: missing header row" }));
        log("tool_call", call("c2", SKILLED));
        log("tool_result", json!({ "role": "tool", "tool_call_id": "c2", "content": "12 rows" }));
        db.add_tool_fix(&conv, "read_table", NAIVE, "missing header row", SKILLED);
        mem.save_lesson(
            &db,
            &Fact {
                name: "check-header".into(),
                description: "check the header row of a table".into(),
                kind: "lesson".into(),
                body: "Look at the first row of a table.".into(),
                cue: Some("tool:read_table".into()),
                recurrence: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        World { db, mem, conv, _dir: dir }
    }

    /// A stand-in model. `fixes_with` is the word that, when it is in the prompt, makes
    /// the call right.
    fn model<'a>(fixes_with: &'static str, asked: &'a Cell<usize>) -> impl FnMut(Vec<Value>, f32) -> std::future::Ready<Option<Attempt>> + 'a {
        move |messages, _| {
            asked.set(asked.get() + 1);
            let prompt = messages[0]["content"].as_str().unwrap_or_default().to_string();
            let right = !fixes_with.is_empty() && prompt.contains(fixes_with);
            std::future::ready(Some(Attempt { tool: "read_table".into(), args: if right { SKILLED.into() } else { NAIVE.into() } }))
        }
    }

    fn rows(w: &World) -> Vec<ChangeProposal> {
        w.db.list_improvements().unwrap()
    }

    #[test]
    fn a_lesson_learned_again_is_a_problem_with_a_moment_to_replay() {
        let w = world();
        let ps = problems(&w.db, &w.mem, crate::db::now_ms());
        assert_eq!(ps.len(), 1);
        assert_eq!((ps[0].source, ps[0].tool.as_deref()), (Source::Lesson, Some("read_table")));
        assert_eq!(ps[0].moments.len(), 1);
        assert_eq!(ps[0].moments[0].conversation_id, w.conv);
    }

    /// Chats that fit no kind are not a kind of task to fix, and the word
    /// "unknown" is never shown to the person.
    #[test]
    fn unknown_tasks_that_go_wrong_are_not_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let mem = MemoryStore::new(dir.path()).unwrap();
        let ended = |kind: &str| {
            let id = db.create_conversation("A chat", None, false).unwrap().id;
            db.save_episode_rules(&id, "abandoned", kind, "{}").unwrap();
            db.save_episode_draft(&id, "goal", "digest", "", "it went wrong", "[]").unwrap();
        };
        ended("unknown");
        ended("unknown");
        assert!(problems(&db, &mem, crate::db::now_ms()).iter().all(|p| p.source != Source::TaskKind));
        ended("mail");
        ended("mail");
        let ps = problems(&db, &mem, crate::db::now_ms());
        let kinds: Vec<&str> = ps.iter().filter(|p| p.source == Source::TaskKind).map(|p| p.cue.as_str()).collect();
        assert_eq!(kinds, ["mail tasks keep going wrong"]);
    }

    #[test]
    fn told_twice_comes_first_and_the_same_cue_is_one_problem() {
        let w = world();
        let trace = |name: &str, args: &str, outcome: &str| CommandTrace { name: name.into(), args: args.into(), by: "user".into(), outcome: outcome.into(), note: None, message_id: None };
        for _ in 0..2 {
            crate::agent::log::record_command(&w.db, &w.conv, None, &trace("suggest", "Use pandas", "declined"));
        }
        let ps = problems(&w.db, &w.mem, crate::db::now_ms());
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].source, Source::ToldTwice);
        assert!(ps[0].note.contains("Use pandas"));
        assert_eq!(ps[1].source, Source::Lesson);
        assert_eq!(cue_key("Check  the HEADER, row"), cue_key("check the header row"));
    }

    #[test]
    fn candidates_are_of_different_forms_and_a_check_names_what_it_looks_at() {
        let w = world();
        let p = problems(&w.db, &w.mem, 0).remove(0);
        let c = draft(&p, None, &[]);
        let forms: Vec<&str> = c.iter().map(|c| c.form.as_str()).collect();
        assert_eq!(forms, vec!["reminder", "check"], "no project, no instruction line; nothing earlier");
        let check = c.iter().find(|c| c.form == "check").unwrap();
        assert_eq!((check.target.as_str(), check.slug.as_deref()), ("habit", Some("check-header")));
        let v: Value = serde_json::from_str(&check.text).unwrap();
        assert_eq!((v["on"].as_str(), v["check"].as_str()), (Some("read_table"), Some("header_row")));
        assert!(check.hint.contains("has_header"), "the check carries what worked");
        let with_project = draft(&p, Some(("proj1".into(), Some("Be brief.".into()))), &[]);
        let ins = with_project.iter().find(|c| c.form == "instruction").unwrap();
        assert!(ins.text.starts_with("Be brief.\n- When you use read_table"));
    }

    /// `IMP-4` accept: only the check fixes the replay, so the check is proposed and
    /// the other fix is dropped with its result.
    #[tokio::test]
    async fn a_problem_where_only_the_check_fixes_the_replay_proposes_the_check_and_drops_the_other() {
        let w = world();
        let asked = Cell::new(0);
        let mut ask = model("What worked with read_table", &asked);
        let out = improve_with(&w.db, &w.mem, crate::db::now_ms(), &|| false, &mut ask).await;
        assert_eq!((out.problems, out.proposed, out.kept_base), (1, 1, 0));

        let all = rows(&w);
        assert_eq!(all.len(), 2);
        let check = all.iter().find(|r| r.form.as_deref() == Some("check")).unwrap();
        let reminder = all.iter().find(|r| r.form.as_deref() == Some("reminder")).unwrap();
        assert_eq!(check.status, "pending");
        assert_eq!(reminder.status, "dropped");
        assert_eq!(check.group_id, reminder.group_id, "rivals share a group");
        assert_eq!(check.problem_cue.as_deref(), Some("check the header row of a table"));
        let kept: Value = serde_json::from_str(check.rehearsal_json.as_deref().unwrap()).unwrap();
        assert_eq!((kept["right"].as_u64(), kept["base_right"].as_u64(), kept["tries"].as_u64()), (Some(2), Some(0), Some(2)));
        assert_eq!(kept["verdict"], "helped");
        assert!(kept["evidence"].as_str().unwrap().starts_with("kept -> a check on the tool -> right in 2 of 2 replays"));
        let lost: Value = serde_json::from_str(reminder.rehearsal_json.as_deref().unwrap()).unwrap();
        assert_eq!(lost["right"], 0);
        assert!(lost["evidence"].as_str().unwrap().starts_with("dropped -> a written reminder -> right in 0 of 2 replays"));
        // Nothing was dispatched, and the rest of the week's work is still ahead.
        assert_eq!(w.db.tool_results_between(&w.conv, "read_table", 0, None).unwrap(), (0, 0));
        assert_eq!(asked.get(), 2 * 3, "one replay, two samples, the setup and two fixes");
        // Only the winner shows as waiting.
        let pending = w.db.list_change_proposals().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].target, "habit");
    }

    /// `IMP-4` accept: nothing beats the unchanged setup, so nothing is proposed and
    /// one `kept_base` row says so.
    #[tokio::test]
    async fn a_problem_where_nothing_beats_the_setup_proposes_nothing_and_says_so_once() {
        let w = world();
        let asked = Cell::new(0);
        let mut ask = model("", &asked);
        let out = improve_with(&w.db, &w.mem, crate::db::now_ms(), &|| false, &mut ask).await;
        assert_eq!((out.proposed, out.kept_base), (0, 1));
        assert!(w.db.list_change_proposals().unwrap().is_empty(), "nothing waits for a yes");
        let all = rows(&w);
        assert_eq!(all.iter().filter(|r| r.status == "kept_base").count(), 1);
        assert_eq!(all.iter().filter(|r| r.status == "dropped").count(), 2);
        let base = all.iter().find(|r| r.status == "kept_base").unwrap();
        assert!(serde_json::from_str::<Value>(base.rehearsal_json.as_deref().unwrap()).unwrap()["evidence"].as_str().unwrap().starts_with("kept as is -> nothing beat the setup I have"));

        // It is not tried again tomorrow.
        let again = improve_with(&w.db, &w.mem, crate::db::now_ms() + DAY_MS, &|| false, &mut model("", &asked)).await;
        assert_eq!(again.problems, 0);
        // A fortnight on, it may be.
        let later = improve_with(&w.db, &w.mem, crate::db::now_ms() + 15 * DAY_MS, &|| false, &mut model("", &asked)).await;
        assert_eq!(later.problems, 1);
    }

    #[tokio::test]
    async fn a_fix_that_is_already_right_without_it_is_no_better_than_the_setup() {
        let w = world();
        let always = |_: Vec<Value>, _: f32| std::future::ready(Some(Attempt { tool: "read_table".into(), args: SKILLED.into() }));
        let mut ask = always;
        let out = improve_with(&w.db, &w.mem, crate::db::now_ms(), &|| false, &mut ask).await;
        assert_eq!((out.proposed, out.kept_base), (0, 1));
    }

    #[tokio::test]
    async fn a_problem_with_nothing_to_replay_is_offered_untried_and_says_so() {
        let w = world();
        w.db.exec_for_tests("DELETE FROM session_events", []);
        let asked = Cell::new(0);
        let out = improve_with(&w.db, &w.mem, crate::db::now_ms(), &|| false, &mut model("x", &asked)).await;
        assert_eq!((out.proposed, out.kept_base), (1, 0));
        assert_eq!(asked.get(), 0, "no model was asked");
        let r = &rows(&w)[0];
        assert!(serde_json::from_str::<Value>(r.rehearsal_json.as_deref().unwrap()).unwrap()["evidence"].as_str().unwrap().contains("not tried"));
    }

    #[tokio::test]
    async fn the_step_stops_when_the_person_does_something_and_does_nothing_when_lessons_are_off() {
        let w = world();
        let asked = Cell::new(0);
        let out = improve_with(&w.db, &w.mem, crate::db::now_ms(), &|| true, &mut model("x", &asked)).await;
        assert_eq!((out.problems, out.stopped, asked.get()), (0, true, 0));
        w.db.set_setting("autonomy.lessons", "off").unwrap();
        let off = improve_with(&w.db, &w.mem, crate::db::now_ms(), &|| false, &mut model("x", &asked)).await;
        assert_eq!(off, Outcome::default());
    }

    fn cand(form: &str, target: &str) -> Candidate {
        Candidate { form: form.into(), target: target.into(), slug: None, text: String::new(), hint: String::new(), description: String::new() }
    }

    #[test]
    fn the_best_that_beats_the_setup_is_adopted_and_two_written_ones_may_be_joined() {
        let c = [cand("reminder", "lesson"), cand("check", "habit"), cand("earlier", "lesson")];
        let p = |base, right: [usize; 3]| Probed { tries: 4, base_right: base, right: right.to_vec() };
        assert_eq!(pick(&p(2, [1, 2, 2]), &c), Pick::Base, "equal is not better");
        assert_eq!(pick(&p(1, [2, 4, 3]), &c), Pick::One(1), "a perfect score needs no help");
        assert_eq!(pick(&p(1, [3, 2, 3]), &c), Pick::Pair(0, 2), "two written ones, the best not perfect");
        assert_eq!(pick(&p(1, [2, 3, 2]), &c), Pick::One(1), "a check is not joined to words");
        assert_eq!(pick(&p(0, [3, 0, 3]), &c), Pick::Pair(0, 2), "ties go to the one drafted first");
    }

    #[tokio::test]
    async fn two_written_fixes_are_kept_as_one_only_if_together_they_do_better() {
        let w = world();
        let p = problems(&w.db, &w.mem, 0).remove(0);
        let replays: Vec<Rehearsal> = p.moments.iter().filter_map(|m| golden::build_rehearsal(&w.db, &m.conversation_id, &m.fix)).collect();
        let a = Candidate { form: "reminder".into(), target: "lesson".into(), slug: Some("a".into()), text: "A.".into(), hint: "WORD-A".into(), description: "d".into() };
        let b = Candidate { form: "earlier".into(), target: "lesson".into(), slug: Some("b".into()), text: "B.".into(), hint: "WORD-B".into(), description: "d".into() };
        // One sample right with either alone; both together are right twice.
        let probed = Probed { tries: 2, base_right: 0, right: vec![1, 1] };
        let asked = Cell::new(0);
        let mut ask = |m: Vec<Value>, t: f32| {
            let both = m[0]["content"].as_str().unwrap().contains("WORD-A") && m[0]["content"].as_str().unwrap().contains("WORD-B");
            asked.set(asked.get() + 1);
            let _ = t;
            std::future::ready(Some(Attempt { tool: "read_table".into(), args: if both { SKILLED.into() } else { NAIVE.into() } }))
        };
        let (proposed, _) = compose_and_file(&w.db, &p, &[a, b], &replays, &probed, &mut ask).await;
        assert_eq!(proposed, 1);
        let all = rows(&w);
        let merged = all.iter().find(|r| r.status == "pending").unwrap();
        assert_eq!(merged.form.as_deref(), Some("merged"));
        assert_eq!(merged.proposed_text, "A.\nB.");
        assert_eq!(all.iter().filter(|r| r.status == "dropped").count(), 2, "both parts are dropped");
        assert!(serde_json::from_str::<Value>(merged.rehearsal_json.as_deref().unwrap()).unwrap()["evidence"].as_str().unwrap().starts_with("merged -> a written reminder and what worked before"));
    }

    // ---- IMP-5 ----

    /// A finished fix, as the learning step reads it.
    fn applied(db: &Db, form: &str, tool: &str, outcome: &str, verdict: &str, at: i64) {
        let rj = json!({ "rehearsed": true, "tool": tool, "hint": "h", "verdict": verdict }).to_string();
        let r = db.add_improvement("lesson", Some("x"), "t", "why", None, "applied", "cue", form, "g", &rj).unwrap();
        db.set_live_outcome(&r.id, outcome).unwrap();
        db.exec_for_tests("UPDATE change_proposals SET created_at = ?2 WHERE id = ?1", rusqlite::params![r.id, at]);
    }

    /// `IMP-5` accept: five times a check held and a written reminder did not, for
    /// `read_file`. The next `read_file` problem drafts the check first and the rules say so.
    #[test]
    fn what_keeps_winning_is_drafted_first_and_written_down_as_a_rule() {
        let w = world();
        let p = Problem { tool: Some("read_file".into()), ..problems(&w.db, &w.mem, 0).remove(0) };
        let before: Vec<String> = draft(&p, None, &[]).into_iter().map(|c| c.form).collect();
        assert_eq!(before[0], "reminder", "nothing learned yet: the default order");

        for i in 0..5 {
            applied(&w.db, "check", "read_file", "held", "helped", 1000 + i);
            applied(&w.db, "reminder", "read_file", "failed", "helped", 2000 + i);
        }
        let all = rows(&w);
        let after: Vec<String> = draft(&p, None, &all).into_iter().map(|c| c.form).collect();
        assert_eq!(after[0], "check");

        let rules = refresh_rules(&w.db, 10_000, false);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].text, "Written reminders rarely fix how I call a tool. I use checks for that now. (5 of 5 times)");
        assert_eq!(load_rules(&w.db), rules, "kept in the settings");
    }

    #[test]
    fn rules_are_written_monthly_at_most_five_and_a_deleted_one_stays_gone_until_there_is_more() {
        let w = world();
        for i in 0..3 {
            applied(&w.db, "check", "read_file", "held", "helped", 1000 + i);
            applied(&w.db, "reminder", "read_file", "failed", "helped", 2000 + i);
        }
        let now = 10 * DAY_MS;
        assert_eq!(refresh_rules(&w.db, now, false).len(), 1);
        // New rows inside the month change nothing.
        applied(&w.db, "check", "read_file", "held", "helped", 3000);
        assert_eq!(load_rules(&w.db)[0].of, 3);
        assert_eq!(refresh_rules(&w.db, now + DAY_MS, false)[0].of, 3);
        assert_eq!(refresh_rules(&w.db, now + 31 * DAY_MS, false)[0].of, 4, "a month on");

        delete_rule(&w.db, "check>reminder");
        assert!(load_rules(&w.db).is_empty());
        assert!(refresh_rules(&w.db, now + 62 * DAY_MS, false).is_empty(), "the same rows do not bring it back");
        applied(&w.db, "check", "read_file", "held", "helped", 4000);
        assert_eq!(refresh_rules(&w.db, now + 93 * DAY_MS, false).len(), 1, "more rows may");

        for form in ["instruction", "earlier"] {
            for i in 0..3 {
                applied(&w.db, form, "read_file", "failed", "helped", 5000 + i);
            }
        }
        assert!(derive_rules(&rows(&w)).len() <= MAX_RULES);
    }

    #[test]
    fn an_applied_fix_is_judged_by_whether_the_tool_needed_fixing_again() {
        let w = world();
        let rj = json!({ "rehearsed": true, "tool": "read_table", "verdict": "helped" }).to_string();
        let r = w.db.add_improvement("habit", Some("check-header"), "{}", "why", None, "applied", "cue", "check", "g", &rj).unwrap();
        w.db.exec_for_tests("UPDATE change_proposals SET created_at = 0 WHERE id = ?1", rusqlite::params![r.id]);
        // The fix in `world` is after created_at 0: the tool needed fixing again.
        assert_eq!(fill_live_outcomes(&w.db, 20 * DAY_MS), 1);
        assert_eq!(rows(&w).iter().find(|x| x.id == r.id).unwrap().live_outcome.as_deref(), Some("failed"));
        // Too soon to say, or not applied: left alone.
        let young = w.db.add_improvement("habit", Some("x"), "{}", "why", None, "applied", "cue2", "check", "g2", &rj).unwrap();
        assert_eq!(fill_live_outcomes(&w.db, young.created_at + DAY_MS), 0);
    }

    /// A failure while the fix still waited to be accepted is not held against it.
    #[test]
    fn a_fix_is_judged_from_when_it_was_applied_not_when_it_was_drafted() {
        let w = world();
        let rj = json!({ "rehearsed": true, "tool": "read_table", "verdict": "helped" }).to_string();
        let r = w.db.add_improvement("habit", Some("check-header"), "{}", "why", None, "pending", "cue", "check", "g", &rj).unwrap();
        w.db.exec_for_tests("UPDATE change_proposals SET created_at = 0 WHERE id = ?1", rusqlite::params![r.id]);
        // The fix in `world` happened while it was pending; it is accepted now.
        w.db.resolve_change_proposal(&r.id, "applied").unwrap();
        let row = rows(&w).into_iter().find(|x| x.id == r.id).unwrap();
        let at = applied_at(&row).expect("the time it was applied is kept");
        assert_eq!(fill_live_outcomes(&w.db, at + DAY_MS), 0, "too soon after it took effect");
        assert_eq!(fill_live_outcomes(&w.db, at + 20 * DAY_MS), 1);
        assert_eq!(rows(&w).iter().find(|x| x.id == r.id).unwrap().live_outcome.as_deref(), Some("held"));
    }

    #[test]
    fn five_agreements_in_a_row_trust_an_area_and_one_disagreement_sends_it_back_to_ask() {
        let w = world();
        for i in 0..5 {
            applied(&w.db, "check", "read_file", "held", "helped", 1000 + i);
        }
        let a = agreement(&rows(&w));
        assert_eq!((a[0].area.as_str(), a[0].agreed, a[0].total, a[0].streak, a[0].trusted), ("read_file", 5, 5, 5, true));
        applied(&w.db, "check", "read_file", "failed", "helped", 9000);
        let a = agreement(&rows(&w));
        assert_eq!((a[0].agreed, a[0].total, a[0].streak, a[0].trusted), (5, 6, 0, false));
        let s = summary(&w.db);
        assert!((s.hold_rate.unwrap() - 5.0 / 6.0).abs() < 1e-6);
        assert!((s.agreement_rate.unwrap() - 5.0 / 6.0).abs() < 1e-6);
    }
}
