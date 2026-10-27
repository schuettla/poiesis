//! `SEE-1`: every answer checks itself, while it streams. No click and no slow
//! model call to find the claims.
//!
//! - **Evidence** is what this turn already read ([`Pool`]): the results of this
//!   run's own tool calls, taken from the session log. The check adds no search and
//!   no fetch. Its questions go to the decision backend the person chose, so with a
//!   cloud decision model a sentence and its passages go there, like any other decision.
//!   A result the pattern scan marked as giving orders is left out (fenced off).
//! - **Sentences** are split in code (`agent::sentences`) and go to the check in
//!   order, each once the stream has moved past it.
//! - **Per sentence**, one fast ask (`ClaimCheckable`): could it be checked, and is
//!   it a fact or something the assistant says it did? A fact is held against the two
//!   best passages the turn read (similarity 0.5 or more) by the existing fast
//!   verdict (`ClaimVerdict`). A sentence no passage reaches is "nothing I read backs
//!   this" and is not asked. Something "I did" is matched in code against this run's
//!   own tool calls ([`family_of`], [`RunDid`]).
//! - **Escalation.** The slow review runs by itself only for a fact the quick verdict
//!   is sure is refuted, two an answer at most. Every other mark has **Review this**
//!   in its card (`autocheck_review_cmd`).
//! - **Nothing read.** With an empty pool no fact is asked about, so nothing is marked,
//!   and the trust line says the answer came from what I know. Only sentences that say
//!   "I did this" are still asked about, because the run's own steps are the evidence.
//!
//! The marks arrive as the app event `poiesis-autocheck`, because the check outlives
//! the run's own stream. The result is kept with the message (`verifications`, mode
//! `auto`), and a deep check replaces it.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::decide::checkable::{self, Reading};
use super::decide::claims::{self, REFUTED};
use super::decide::control::{Act, Action};
use super::decide::{self, Opts, Purpose, Verdict};
use super::sentences;
use super::verify::{best_window, parse_review, review_messages, Claim, Passage, UNCHECKED};
use crate::db::{Db, SessionEvent};

/// The two kinds of check kept with a message.
pub const AUTO: &str = "auto";
pub const DEEP: &str = "deep";

/// Slow reviews an answer starts by itself.
pub const AUTO_REVIEWS: usize = 2;
/// How alike a sentence and a passage must be, as the share of the sentence's
/// distinctive words the passage has.
pub const FLOOR: f32 = 0.5;
/// Passages a fact is judged against.
pub const PASSAGES: usize = 2;
/// A passage is cut to this many characters, as the deep check keeps them.
const CHUNK: usize = 900;
/// How much of one tool result is read, and how many passages the pool holds.
const RESULT_CHARS: usize = 40_000;
const POOL_MAX: usize = 400;
/// The finding of a sentence the assistant says it did and no step did.
pub const UNDONE: &str = "undone";
/// How a finding was reached when code matched it against the run's own steps.
pub const LEDGER: &str = "ledger";
/// The app event the marks arrive on.
pub const EVENT: &str = "poiesis-autocheck";

// ---- what the run did ----

/// One tool call of the run, with whether it came back without an error.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub id: String,
    pub name: String,
    pub args: Value,
    /// `None` until its result row is read.
    pub ok: Option<bool>,
}

/// What kind of action a sentence says the assistant took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Sent, emailed, replied, posted.
    Send,
    /// Wrote, created, saved, edited, deleted, added, fixed.
    Write,
    /// Ran, tested, executed, built, installed, committed, pushed.
    Run,
    /// Searched the web, looked something up online.
    SearchWeb,
    /// Searched your files, your mail or an earlier chat.
    Search,
    /// Read, opened or reviewed something.
    Read,
}

const SEND_VERBS: [&str; 8] = ["sent", "emailed", "mailed", "replied", "forwarded", "posted", "messaged", "delivered"];
const WRITE_VERBS: [&str; 17] = [
    "wrote", "written", "created", "saved", "edited", "updated", "modified", "changed", "renamed", "moved", "deleted",
    "removed", "added", "fixed", "generated", "scheduled", "booked",
];
const RUN_VERBS: [&str; 10] =
    ["ran", "executed", "tested", "compiled", "built", "installed", "committed", "pushed", "launched", "started"];
const SEARCH_VERBS: [&str; 5] = ["searched", "googled", "browsed", "fetched", "visited"];
const READ_VERBS: [&str; 3] = ["read", "opened", "reviewed"];
const WEB_WORDS: [&str; 6] = ["web", "online", "internet", "google", "website", "websites"];
/// Words that may sit between "I" and the verb: "I have just also created".
const AUXILIARY: [&str; 12] =
    ["have", "just", "also", "already", "then", "successfully", "now", "finally", "again", "both", "first", "had"];
const NEGATIONS: [&str; 12] =
    ["not", "never", "no", "cannot", "unable", "couldn't", "didn't", "haven't", "wasn't", "won't", "can't", "hadn't"];
const MODALS: [&str; 8] = ["will", "can", "could", "would", "should", "might", "may", "plan"];
const FIRST_PERSON: [&str; 4] = ["i", "i've", "we", "we've"];

fn tokens(sentence: &str) -> Vec<String> {
    sentence
        .split_whitespace()
        .map(|w| {
            w.replace('\u{2019}', "'")
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

fn family_of_verb(verb: &str, web: bool) -> Option<Family> {
    if SEND_VERBS.contains(&verb) {
        Some(Family::Send)
    } else if WRITE_VERBS.contains(&verb) {
        Some(Family::Write)
    } else if RUN_VERBS.contains(&verb) {
        Some(Family::Run)
    } else if SEARCH_VERBS.contains(&verb) {
        Some(if web || matches!(verb, "browsed" | "fetched" | "visited" | "googled") { Family::SearchWeb } else { Family::Search })
    } else if READ_VERBS.contains(&verb) {
        Some(Family::Read)
    } else {
        None
    }
}

/// The actions a sentence says the assistant took, from the words of it. Only a
/// first-person sentence in the past ("I sent", "I have created and ran"), never a
/// plan ("I will send"), an offer ("I can send") or a denial ("I did not send").
/// Empty when the sentence says nothing of the kind.
pub fn families_of(sentence: &str) -> Vec<Family> {
    let t = tokens(sentence);
    let web = t.iter().any(|w| WEB_WORDS.contains(&w.as_str()));
    let Some(me) = t.iter().take(5).position(|w| FIRST_PERSON.contains(&w.as_str())) else {
        return Vec::new();
    };
    let mut out: Vec<Family> = Vec::new();
    let mut expecting = true;
    for w in &t[me + 1..] {
        let w = w.as_str();
        if NEGATIONS.contains(&w) {
            return Vec::new();
        }
        if MODALS.contains(&w) && out.is_empty() {
            return Vec::new();
        }
        if matches!(w, "and" | "then") {
            expecting = true;
            continue;
        }
        if !expecting {
            continue;
        }
        if AUXILIARY.contains(&w) {
            continue;
        }
        match family_of_verb(w, web) {
            Some(f) => {
                if !out.contains(&f) {
                    out.push(f);
                }
                expecting = false;
            }
            // Something else stood where the verb should be: "I think ...".
            None if out.is_empty() => return Vec::new(),
            None => expecting = false,
        }
    }
    out
}

/// The first family, for a sentence that is about one thing.
pub fn family_of(sentence: &str) -> Option<Family> {
    families_of(sentence).into_iter().next()
}

/// A sentence that says the change works, passes or was tested: the claim
/// `UnverifiedClaim` looks for in the answer, here in code, for the one sentence.
///
/// A red mark on a true sentence is the worse mistake, so a word alone is not enough
/// where it also describes: "works by caching" says how, not that it was checked, and
/// "passes the value" is not a test. So:
/// - **tested** and **verified** are the claim by themselves;
/// - **pass**, **passes**, **passing**, **passed** only with a test, a check or a build;
/// - **works**, **working**, **fixed**, **resolved** only as a result: with "now", after
///   "is", "are", "been" or "it's", or as "I fixed".
pub fn works_claim(sentence: &str) -> bool {
    let t = tokens(sentence);
    let has = |words: &[&str]| t.iter().any(|w| words.contains(&w.as_str()));
    if has(&["tested", "verified"]) {
        return true;
    }
    let checked = has(&["test", "tests", "check", "checks", "build", "builds", "suite", "ci"]);
    if checked && has(&["pass", "passes", "passing", "passed"]) {
        return true;
    }
    let now = has(&["now"]);
    t.iter().enumerate().any(|(i, w)| {
        matches!(w.as_str(), "works" | "working" | "fixed" | "resolved")
            && (now
                || i.checked_sub(1).is_some_and(|j| {
                    matches!(t[j].as_str(), "is" | "are" | "been" | "it's" | "i" | "i've" | "we" | "we've" | "all")
                }))
    })
}

/// What the run did, by family, from its own tool calls that came back without an error.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunDid {
    steps: Vec<(Family, String)>,
}

fn reads_only(name: &str) -> bool {
    super::toolsets::classify_mutation(name) == Some(false)
}

fn is_search(name: &str) -> bool {
    matches!(name, "search_files" | "search_folder" | "find_symbol" | "find_similar" | "search_mail" | "search_history")
}

fn is_web(name: &str) -> bool {
    matches!(name, "web_search" | "fetch_url" | "browse" | "browser_read")
}

impl RunDid {
    pub fn from_steps(steps: &[Step]) -> RunDid {
        let mut out = Vec::new();
        for s in steps.iter().filter(|s| s.ok == Some(true)) {
            let n = s.name.as_str();
            let mut add = |f: Family| out.push((f, n.to_string()));
            // A connector's tool is judged by what it does to the outside world.
            let connector = n == crate::connectors::composio::MULTI_EXECUTE || super::toolsets::classify_mutation(n).is_none();
            match n {
                "send_mail" | "reply_mail" => add(Family::Send),
                "run_command" | "run_code" | "run_task" => {
                    add(Family::Run);
                    add(Family::Write);
                }
                _ if connector => {
                    // Whether it sends cannot be told without its name's verb: be generous, so a
                    // true sentence is never marked "no step did this".
                    add(Family::Send);
                    add(Family::Write);
                    add(Family::Read);
                    add(Family::Search);
                }
                _ if super::toolsets::classify_mutation(n) == Some(true) => add(Family::Write),
                _ => {}
            }
            if is_web(n) {
                add(Family::SearchWeb);
                add(Family::Search);
            }
            if is_search(n) {
                add(Family::Search);
            }
            if reads_only(n) && !matches!(n, "classify" | "skill") {
                add(Family::Read);
            }
        }
        RunDid { steps: out }
    }

    /// The steps of this run that match a family, by tool name, once each.
    pub fn matching(&self, family: Family) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for (f, n) in &self.steps {
            if *f == family && !names.contains(n) {
                names.push(n.clone());
            }
        }
        names
    }
}

// ---- what the turn read ----

/// Which kind of source a tool's result is, for the card: `web`, `folder` or `read`.
/// `None` for a tool that reads nothing a sentence could be held against.
pub fn evidence_via(name: &str) -> Option<&'static str> {
    use super::connector_gate::{classify, CallKind, ToolHints};
    match name {
        "web_search" | "fetch_url" | "browse" | "browser_read" => Some("web"),
        "read_file" | "search_files" | "search_folder" | "find_symbol" | "find_similar" => Some("folder"),
        "read_mail" | "search_mail" | "search_history" | "read_conversation" | "read_artifact" | "read_result"
        | "search_result" | "collect_agents" | "check_agents" | "delegate" => Some("read"),
        n if n == crate::connectors::composio::MULTI_EXECUTE => Some("read"),
        n if super::toolsets::classify_mutation(n).is_some() => None,
        // A connector's own tool: what it answers is outside text, unless it sends.
        n => (classify(n, &ToolHints::default()) != CallKind::Send).then_some("read"),
    }
}

/// The words of one `<untrusted ...>` envelope in a result: where it came from, how
/// risky the scan said it was, and what it holds.
struct Envelope<'a> {
    source: &'a str,
    risk: u8,
    body: &'a str,
}

fn attr<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let from = head.find(&key)? + key.len();
    let to = head[from..].find('"')?;
    Some(&head[from..from + to])
}

fn envelopes(text: &str) -> Vec<Envelope<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("<untrusted id=\"") {
        let after = &rest[open..];
        let Some(head_end) = after.find(">\n") else { break };
        let head = &after[..head_end];
        let body_from = head_end + 2;
        let id = attr(head, "id").unwrap_or("");
        let close = format!("\n</untrusted:{id}>");
        let Some(body_end) = after[body_from..].find(&close) else { break };
        out.push(Envelope {
            source: attr(head, "source").unwrap_or(""),
            risk: attr(head, "risk").and_then(|r| r.parse().ok()).unwrap_or(0),
            body: &after[body_from..body_from + body_end],
        });
        rest = &after[body_from + body_end + close.len()..];
    }
    out
}

/// A tool result cut into passages of about [`CHUNK`] characters at paragraph breaks.
fn chunks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut taken = 0usize;
    for para in text.split("\n\n") {
        let flat: String = para.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.is_empty() {
            continue;
        }
        taken += flat.len();
        if taken > RESULT_CHARS {
            break;
        }
        // A paragraph longer than a passage is cut at a word.
        let mut piece = flat.as_str();
        while piece.chars().count() > CHUNK {
            let cut = piece.char_indices().nth(CHUNK).map_or(piece.len(), |(i, _)| i);
            let at = piece[..cut].rfind(' ').unwrap_or(cut);
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(piece[..at].trim().to_string());
            piece = piece[at..].trim_start();
        }
        if cur.chars().count() + piece.chars().count() + 1 > CHUNK && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(piece);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The name a source is shown under: a file's name, a page's host, or the label the
/// tool gave its own result.
fn source_label(name: &str, args: &Value, label: &str) -> String {
    let label = label.trim().trim_start_matches("web result:").trim();
    if !label.is_empty() {
        return label.to_string();
    }
    let arg = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("");
    match name {
        "read_file" | "search_files" | "search_folder" | "find_symbol" => {
            let path = arg("path");
            let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
            if file.is_empty() { "a file".into() } else { file.to_string() }
        }
        "fetch_url" | "browse" => {
            let url = arg("url");
            url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url).trim_start_matches("www.").to_string()
        }
        "web_search" => "web search".into(),
        "read_mail" | "search_mail" => "mail".into(),
        "delegate" | "collect_agents" | "check_agents" => "an agent's report".into(),
        other => other.to_lowercase().replace('_', " "),
    }
}

/// Everything the turn has read so far, and every call it has made. Built from the run's
/// own session log rows and grown as the log grows ([`Pool::add`]).
#[derive(Default)]
pub struct Pool {
    /// The last log sequence number read.
    pub seen: i64,
    steps: Vec<Step>,
    passages: Vec<Passage>,
    texts: HashSet<String>,
    sources: HashSet<String>,
    calls: HashMap<String, usize>,
}

impl Pool {
    pub fn passages(&self) -> &[Passage] {
        &self.passages
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// How many different sources the passages came from.
    pub fn sources(&self) -> usize {
        self.sources.len()
    }

    /// Take in rows of this run's log that were not read yet, oldest first.
    pub fn add(&mut self, rows: &[SessionEvent]) {
        let from = self.seen;
        for row in rows.iter().filter(|r| r.seq > from) {
            self.seen = self.seen.max(row.seq);
            let Ok(v) = serde_json::from_str::<Value>(&row.payload_json) else { continue };
            match row.kind.as_str() {
                "tool_call" => {
                    for c in v["tool_calls"].as_array().into_iter().flatten() {
                        let args = c["function"]["arguments"].as_str().unwrap_or("{}");
                        let id = c["id"].as_str().unwrap_or_default().to_string();
                        self.calls.insert(id.clone(), self.steps.len());
                        self.steps.push(Step {
                            id,
                            name: c["function"]["name"].as_str().unwrap_or_default().to_string(),
                            args: serde_json::from_str(args).unwrap_or_default(),
                            ok: None,
                        });
                    }
                }
                "tool_result" => {
                    let id = v["tool_call_id"].as_str().unwrap_or_default();
                    let Some(&at) = self.calls.get(id) else { continue };
                    let content = v["content"].as_str().unwrap_or("");
                    let ok = !content.trim_start().starts_with("Error");
                    self.steps[at].ok = Some(ok);
                    if ok {
                        let step = self.steps[at].clone();
                        self.read(&step, content, row.created_at);
                    }
                }
                _ => {}
            }
        }
    }

    fn read(&mut self, step: &Step, content: &str, at: i64) {
        let Some(via) = evidence_via(&step.name) else { return };
        let fenced = envelopes(content);
        let parts: Vec<(String, &str)> = if fenced.is_empty() {
            vec![(source_label(&step.name, &step.args, ""), content)]
        } else {
            fenced
                .iter()
                // A result the scan marked as giving orders is not evidence.
                .filter(|e| e.risk < 2)
                .map(|e| (source_label(&step.name, &step.args, e.source), e.body))
                .collect()
        };
        for (source, body) in parts {
            let url = step.args.get("url").and_then(|u| u.as_str()).and_then(super::verify::public_url);
            for text in chunks(body) {
                if self.passages.len() >= POOL_MAX {
                    return;
                }
                if super::untrusted::scan(&text).risk >= 2 || !self.texts.insert(text.clone()) {
                    continue;
                }
                self.sources.insert(source.clone());
                self.passages.push(Passage {
                    text,
                    source: source.clone(),
                    url: url.clone(),
                    date: None,
                    retrieved_at: at,
                    via: via.into(),
                });
            }
        }
    }
}

// ---- holding a sentence against it ----

/// A sentence's distinctive words: four letters or more, or holding a digit.
fn terms(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in s.split_whitespace() {
        let t = t.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if (t.chars().count() >= 4 || t.chars().any(|c| c.is_ascii_digit())) && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// The share of a sentence's distinctive words that a text has.
pub fn similarity(sentence: &str, text: &str) -> f32 {
    let wanted = terms(sentence);
    if wanted.is_empty() {
        return 0.0;
    }
    let lower = text.to_lowercase();
    wanted.iter().filter(|t| lower.contains(t.as_str())).count() as f32 / wanted.len() as f32
}

/// The best [`PASSAGES`] passages for a sentence, each at least [`FLOOR`] like it.
pub fn pick(sentence: &str, pool: &[Passage]) -> Vec<Passage> {
    let mut scored: Vec<(f32, usize, String)> = pool
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let window = best_window(&p.text, sentence, CHUNK);
            (similarity(sentence, &window), i, window)
        })
        .filter(|(s, ..)| *s >= FLOOR)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(PASSAGES)
        .map(|(_, i, window)| Passage { text: window, ..pool[i].clone() })
        .collect()
}

/// A sentence that reads like a fact: it has a number, or a capitalised word that does
/// not start it. Only for deciding whether the "answered from what I know" line is worth
/// showing; nothing is ever marked on it.
pub fn looks_factual(sentence: &str) -> bool {
    let words: Vec<&str> = sentence.split_whitespace().collect();
    words.iter().any(|w| w.chars().any(|c| c.is_ascii_digit()))
        || words.iter().skip(1).any(|w| {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric());
            w.chars().next().is_some_and(|c| c.is_uppercase()) && w != "I"
        })
}

// ---- the claims it leaves ----

fn blank(index: usize, text: &str, kind: &str) -> Claim {
    Claim {
        id: format!("s{}", index + 1),
        text: text.to_string(),
        quote: text.to_string(),
        source: None,
        importance: 2,
        kind: kind.into(),
        finding: UNCHECKED.into(),
        p: None,
        how: "none".into(),
        settled: false,
        fast: None,
        slow: None,
        reason: None,
        passages: Vec::new(),
        decision: None,
        mark: None,
    }
}

fn step_passage(names: &[String], what: &str) -> Passage {
    Passage {
        text: format!("{what}: {}", names.join(", ")),
        source: "this run's steps".into(),
        url: None,
        date: None,
        retrieved_at: crate::db::now_ms(),
        via: "run".into(),
    }
}

/// What the quick checks need, so the order, the limits and the marks can be tried
/// without an engine.
pub trait Judge {
    /// The `ClaimCheckable` ask. `None` when the decision model gave no answer. The second
    /// value is the decision's id, for the receipt's words.
    fn checkable(&self, sentence: &str) -> impl Future<Output = Option<(Reading, String)>> + Send;
    /// The `ClaimVerdict` ask against the passages.
    fn verdict(&self, sentence: &str, passages: &[Passage]) -> impl Future<Output = Option<Verdict>> + Send;
    /// Say what was done with a verdict. An escalation counts against the check's reviews.
    fn settle(&self, verdict: &Verdict, act: Act) -> Action;
    /// The receipt's words for a decision that changed something.
    fn note(&self, decision: &str, text: &str);
    /// The slow review's reply: the claim and the evidence, never the quick verdict.
    fn review(&self, sentence: &str, passages: &[Passage]) -> impl Future<Output = Option<String>> + Send;
}

/// What a check counted, for the trust line and the receipt.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// Final sentences the check took.
    pub sentences: u32,
    /// Sentences it asked the decision model about.
    pub asked: u32,
    /// Of those, the ones that got an answer.
    pub answered: u32,
    /// Sentences past the 40 it takes.
    pub over: u32,
    /// Passages this turn read, and the sources they came from.
    pub pool: u32,
    pub sources: u32,
    /// With nothing read: sentences that read like a fact and were left unchecked.
    pub factual: u32,
}

fn undone(index: usize, text: &str, reason: &str, steps: Vec<Passage>, decision: Option<String>, p: Option<f32>) -> Claim {
    let mut c = blank(index, text, "action");
    c.finding = UNDONE.into();
    c.how = LEDGER.into();
    c.settled = true;
    c.reason = Some(reason.into());
    c.passages = steps;
    c.decision = decision;
    c.p = p;
    c
}

fn done(index: usize, text: &str, steps: Vec<Passage>, decision: Option<String>, p: Option<f32>) -> Claim {
    let mut c = blank(index, text, "action");
    c.finding = claims::SUPPORTED.into();
    c.how = LEDGER.into();
    c.settled = true;
    c.reason = Some("A step of this run did it.".into());
    c.passages = steps;
    c.decision = decision;
    c.p = p;
    c
}

/// The state of the check as it goes.
#[derive(Default)]
pub struct Check {
    pub claims: Vec<Claim>,
    pub summary: Summary,
    reviews: usize,
}

impl Check {
    /// Check one final sentence. Returns the claim it left, if it left one.
    pub async fn sentence<J: Judge>(
        &mut self,
        judge: &J,
        index: usize,
        text: &str,
        pool: &Pool,
        unchecked_edits: bool,
    ) -> Option<Claim> {
        self.summary.sentences += 1;
        let did = RunDid::from_steps(pool.steps());
        let families = families_of(text);
        let works = unchecked_edits && works_claim(text);
        let nothing_read = pool.passages().is_empty();
        // With nothing read only what the run's own steps can settle is asked.
        if nothing_read && families.is_empty() && !works {
            if looks_factual(text) {
                self.summary.factual += 1;
            }
            return None;
        }
        self.summary.asked += 1;
        let (reading, decision) = judge.checkable(text).await?;
        self.summary.answered += 1;
        let claim = match reading {
            Reading::Skip => None,
            Reading::Did { p } => self.did(index, text, &families, &did, works, decision.clone(), p),
            Reading::Fact { p } => {
                if works {
                    Some(undone(index, text, "Files changed and no check ran afterwards.", Vec::new(), Some(decision.clone()), Some(p)))
                } else if nothing_read {
                    None
                } else {
                    self.fact(judge, index, text, pool).await
                }
            }
        };
        if let Some(c) = &claim {
            if c.finding == UNDONE {
                judge.note(&decision, &format!("Marked a sentence: {}", c.reason.as_deref().unwrap_or("no step did this")));
            }
        }
        if let Some(c) = &claim {
            self.claims.push(c.clone());
        }
        claim
    }

    fn did(
        &self,
        index: usize,
        text: &str,
        families: &[Family],
        did: &RunDid,
        works: bool,
        decision: String,
        p: f32,
    ) -> Option<Claim> {
        // The model says it is something done, and the words do not name an action the
        // run's steps could have taken: nothing to match, so nothing to mark.
        if families.is_empty() {
            return works.then(|| undone(index, text, "Files changed and no check ran afterwards.", Vec::new(), Some(decision), Some(p)));
        }
        let mut matched: Vec<String> = Vec::new();
        let mut missing = false;
        for f in families {
            let names = did.matching(*f);
            if names.is_empty() {
                missing = true;
            }
            for n in names {
                if !matched.contains(&n) {
                    matched.push(n);
                }
            }
        }
        if missing {
            Some(undone(index, text, "No step in this run did this.", Vec::new(), Some(decision), Some(p)))
        } else if works {
            Some(undone(index, text, "Files changed and no check ran afterwards.", Vec::new(), Some(decision), Some(p)))
        } else {
            Some(done(index, text, vec![step_passage(&matched, "Step")], Some(decision), Some(p)))
        }
    }

    async fn fact<J: Judge>(&mut self, judge: &J, index: usize, text: &str, pool: &Pool) -> Option<Claim> {
        let mut c = blank(index, text, "fact");
        let passages = pick(text, pool.passages());
        if passages.is_empty() {
            // Nothing I read is close to it: said, and not asked.
            c.finding = claims::INSUFFICIENT.into();
            c.reason = Some("Nothing I read for this answer backs it.".into());
            c.settled = true;
            return Some(c);
        }
        c.passages = passages;
        let verdict = judge.verdict(text, &c.passages).await;
        let Some(v) = verdict else {
            c.reason = Some("The decision model gave no verdict for it.".into());
            return Some(c);
        };
        let Some(fast) = claims::fast_from(&v) else { return Some(c) };
        c.finding = fast.finding.clone();
        c.fast = Some(fast.finding.clone());
        c.p = Some(fast.p);
        c.how = "fast".into();
        c.decision = Some(v.id.clone());
        if claims::settles(&v, &fast, c.importance) {
            judge.settle(&v, Act::Go);
            c.settled = true;
            return Some(c);
        }
        // Only a refutation the model is sure of starts the slow review by itself, two an answer.
        let sure_refuted = fast.finding == REFUTED && fast.p >= claims::SURE;
        if sure_refuted {
            judge.note(&v.id, &format!("Marked a sentence as contradicted by {}", c.passages[0].source));
        }
        if sure_refuted && self.reviews < AUTO_REVIEWS && matches!(judge.settle(&v, Act::Below), Action::Escalated(_)) {
            self.reviews += 1;
            let reply = judge.review(text, &c.passages).await;
            match reply.as_deref().and_then(parse_review) {
                Some((finding, reason)) => {
                    c.slow = Some(finding.clone());
                    c.finding = finding;
                    c.how = "reviewed".into();
                    c.settled = true;
                    if !reason.is_empty() {
                        c.reason = Some(reason);
                    }
                }
                None => c.reason = Some("The review gave no usable answer.".into()),
            }
        }
        Some(c)
    }
}

// ---- the check as the run feeds it ----

/// What the screen is told, over the app event.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Event {
    Progress { conversation_id: String, message_id: String, run_id: String, done: usize, total: usize },
    Claim { conversation_id: String, message_id: String, run_id: String, claim: Box<Claim> },
    /// The check is over. `check` is `None` when nothing was kept (the decision model never answered).
    Done { conversation_id: String, message_id: String, run_id: String, check: Option<crate::commands::verify::CheckView> },
}

enum Job {
    Sentence { index: usize, text: String },
    End { over: usize },
}

/// Where the run hands the answer's text as it grows.
pub struct Stream {
    tx: Option<tokio::sync::mpsc::UnboundedSender<Job>>,
    taken: usize,
    unchecked: Arc<AtomicBool>,
}

/// What a run needs to start a check.
pub struct Start {
    pub conversation_id: String,
    pub run_id: String,
    pub message_id: String,
    pub client: reqwest::Client,
    pub endpoint: crate::cloud::ChatEndpoint,
    pub cancel: crate::runtime::proxy::CancelFlag,
}

impl Stream {
    /// Whether this run's answers are checked: Fast & slow thinking is on.
    pub fn wanted(db: &Db) -> bool {
        crate::experiments::is_on(db, "fast-slow")
    }

    pub fn start(start: Start) -> Stream {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let unchecked = Arc::new(AtomicBool::new(false));
        tokio::spawn(live::work(start, rx, unchecked.clone()));
        Stream { tx: Some(tx), taken: 0, unchecked }
    }

    /// The answer so far, and whether files changed with no check after (`COD-12`).
    pub fn feed(&mut self, text: &str, unchecked_edits: bool) {
        self.unchecked.store(unchecked_edits, Ordering::Relaxed);
        let Some(tx) = &self.tx else { return };
        let (all, _) = sentences::finals(text, false);
        for (i, s) in all.iter().enumerate().skip(self.taken) {
            let _ = tx.send(Job::Sentence { index: i, text: s.text.clone() });
        }
        self.taken = self.taken.max(all.len());
    }

    /// The answer is over: the sentences that were not final yet are, and the check ends
    /// when they are done.
    pub fn finish(&mut self, text: &str, unchecked_edits: bool) {
        self.unchecked.store(unchecked_edits, Ordering::Relaxed);
        let Some(tx) = self.tx.take() else { return };
        let (all, over) = sentences::finals(text, true);
        for (i, s) in all.iter().enumerate().skip(self.taken) {
            let _ = tx.send(Job::Sentence { index: i, text: s.text.clone() });
        }
        let _ = tx.send(Job::End { over });
    }

    /// The run ended without an answer to check (it was stopped, or failed).
    pub fn abandon(&mut self) {
        self.tx = None;
    }
}

mod live {
    use super::*;

    /// The quick checks, asked of the real engine.
    pub struct Asking {
        pub conversation_id: String,
        pub run_id: String,
        pub client: reqwest::Client,
        pub endpoint: crate::cloud::ChatEndpoint,
        pub cancel: crate::runtime::proxy::CancelFlag,
    }

    impl Asking {
        fn opts(&self) -> Opts<'_> {
            Opts { conversation_id: Some(&self.conversation_id), run_id: Some(&self.run_id) }
        }
    }

    impl Judge for Asking {
        async fn checkable(&self, sentence: &str) -> Option<(Reading, String)> {
            let state = checkable::state(sentence);
            let v = decide::ask(&checkable::ask_of(&state), Purpose::ClaimCheckable, self.opts()).await.ok()?;
            let reading = checkable::read(&v);
            decide::settle(&v, Purpose::ClaimCheckable, if reading == Reading::Skip { Act::Below } else { Act::Go }, self.opts());
            Some((reading, v.id))
        }

        async fn verdict(&self, sentence: &str, passages: &[Passage]) -> Option<Verdict> {
            let shown: Vec<(String, String)> = passages.iter().map(|p| (p.source.clone(), p.text.clone())).collect();
            let site = claims::claim_ask(sentence, &claims::render_evidence(&shown));
            decide::ask(&site.as_ask(), Purpose::ClaimVerdict, self.opts()).await.ok()
        }

        fn settle(&self, verdict: &Verdict, act: Act) -> Action {
            decide::settle(verdict, Purpose::ClaimVerdict, act, self.opts())
        }

        fn note(&self, decision: &str, text: &str) {
            if let Some(db) = decide::db() {
                let _ = db.set_decision_note(decision, text);
            }
        }

        async fn review(&self, sentence: &str, passages: &[Passage]) -> Option<String> {
            let messages = review_messages(sentence, passages);
            let outcome = crate::cloud::drive_turn(
                &self.client,
                &self.endpoint,
                &messages,
                &[],
                0.0,
                crate::cloud::Effort::Off,
                &self.cancel,
                |_| {},
            )
            .await
            .ok()?;
            match outcome {
                crate::runtime::proxy::TurnOutcome::Final { content, .. } => Some(content),
                _ => None,
            }
        }
    }

    fn emit(event: &Event) {
        use tauri::Emitter;
        if let Some(app) = decide::app() {
            let _ = app.emit(EVENT, event);
        }
    }

    /// The worker: one sentence at a time, in order, until the answer is over.
    pub async fn work(
        start: Start,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<Job>,
        unchecked: Arc<AtomicBool>,
    ) {
        let Start { conversation_id, run_id, message_id, client, endpoint, cancel } = start;
        let judge = Asking {
            conversation_id: conversation_id.clone(),
            run_id: run_id.clone(),
            client,
            endpoint,
            cancel: cancel.clone(),
        };
        decide::begin_auto_check(&conversation_id);
        let mut check = Check::default();
        let mut pool = Pool::default();
        let mut total = 0usize;
        let mut finished = false;
        let mut over = 0usize;
        while let Some(job) = rx.recv().await {
            if cancel.is_cancelled() {
                break;
            }
            match job {
                Job::Sentence { index, text } => {
                    total = total.max(index + 1);
                    if let Some(db) = decide::db() {
                        if let Ok(rows) = db.session_events_for_run_after(&run_id, pool.seen) {
                            pool.add(&rows);
                        }
                    }
                    let claim = check.sentence(&judge, index, &text, &pool, unchecked.load(Ordering::Relaxed)).await;
                    if let Some(claim) = claim {
                        emit(&Event::Claim {
                            conversation_id: conversation_id.clone(),
                            message_id: message_id.clone(),
                            run_id: run_id.clone(),
                            claim: Box::new(claim),
                        });
                    }
                    emit(&Event::Progress {
                        conversation_id: conversation_id.clone(),
                        message_id: message_id.clone(),
                        run_id: run_id.clone(),
                        done: check.summary.sentences as usize,
                        total,
                    });
                }
                Job::End { over: o } => {
                    over = o;
                    finished = true;
                    break;
                }
            }
        }
        let kept = if finished && !cancel.is_cancelled() {
            check.summary.over = over as u32;
            check.summary.pool = pool.passages().len() as u32;
            check.summary.sources = pool.sources() as u32;
            keep(&conversation_id, &message_id, &check)
        } else {
            None
        };
        emit(&Event::Done { conversation_id, message_id, run_id, check: kept });
    }

    /// Keep what the check found with the message. `None` when there is nothing to show:
    /// the decision model never answered, or there was nothing to say.
    fn keep(conversation_id: &str, message_id: &str, check: &Check) -> Option<crate::commands::verify::CheckView> {
        let s = &check.summary;
        let nothing_asked = s.asked == 0 && s.factual == 0 && check.claims.is_empty();
        let model_silent = s.asked > 0 && s.answered == 0;
        if nothing_asked || model_silent {
            return None;
        }
        let db = decide::db()?;
        save(&db, conversation_id, message_id, check).ok()?;
        db.verification(message_id).ok().flatten().map(Into::into)
    }
}

/// Keep an automatic check with its message, unless a deep check was kept already:
/// the deeper one stands.
pub fn save(db: &Db, conversation_id: &str, message_id: &str, check: &Check) -> Result<bool, String> {
    if db.verification(message_id).map_err(|e| e.to_string())?.is_some_and(|c| c.mode == DEEP) {
        return Ok(false);
    }
    db.save_verification(&crate::db::verifications::StoredCheck {
        message_id: message_id.to_string(),
        conversation_id: conversation_id.to_string(),
        claims_json: serde_json::to_string(&check.claims).map_err(|e| e.to_string())?,
        note: None,
        skipped: 0,
        created_at: crate::db::now_ms(),
        mode: AUTO.into(),
        summary: Some(serde_json::to_string(&check.summary).map_err(|e| e.to_string())?),
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
