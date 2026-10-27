//! `SEE-3`: the composer reads a message while it is typed. After a pause the quick
//! decision model is asked the questions that already exist (does a stop come out of this,
//! would a handover rule take it, is the web needed and off) and at most one line comes
//! back, so the person learns before pressing Enter what the send will do.
//!
//! It is a hint and nothing more. It changes no setting, holds nothing back and sends
//! nothing: the actions on it are the person's, and each one is a per-message choice that
//! the send carries. Every ask here goes to the decision backend the person chose, local or
//! cloud ([`super::policy::Purpose::typing_policy`]), and takes no turn budget. Its answers
//! are held, not logged ([`hold`]): a draft that is never sent leaves nothing behind.
//!
//! What was answered is kept for the send. When the message goes out unchanged, the
//! handover question is not asked again; the answer given while typing is the one the turn
//! reads.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::policy::Purpose;
use super::{handover, state_hash, Answer, Ask, Verdict};
use crate::engines::rules::{Offered, ROUTE_NONE, ROUTE_UNSURE};

/// The fewest characters worth reading.
pub const MIN_CHARS: usize = 12;
/// How sure an answer has to be before a hint says anything.
pub const SURE: f32 = 0.85;
/// How long a kept answer may be reused by the send.
const KEEP_FOR: Duration = Duration::from_secs(120);

/// Whether this text is worth asking about: long enough, and not a `/` command, which is a
/// command and not a message.
pub fn askable(text: &str) -> bool {
    let t = text.trim();
    t.chars().count() >= MIN_CHARS && !t.starts_with('/')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A run is live and the text reads as a stop.
    Stop,
    /// One of the person's handover rules would take the message.
    Handover,
    /// The message needs the web and Web search is off.
    Web,
}

impl Kind {
    /// What the person can do about it, as the button says it.
    pub fn action(self) -> &'static str {
        match self {
            Kind::Stop => "Add it instead",
            Kind::Handover => "Do it yourself",
            Kind::Web => "Turn on for this message",
        }
    }
}

/// One line for the composer's bottom row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hint {
    pub kind: Kind,
    /// The line, in words.
    pub text: String,
    /// The full text for the hover, with the number (`SEE-0`).
    pub title: String,
    pub action: &'static str,
    /// The decision that said it, for the label a click writes.
    pub decision: String,
    pub p: f32,
}

fn sure_pct(p: f32) -> String {
    format!("{:.0}% sure", p * 100.0)
}

/// A live run, and a message to it that reads as a stop.
pub fn stop_hint(verdict: &Verdict) -> Option<Hint> {
    let Answer::Choice { choice, probs, .. } = verdict.get("steer")? else { return None };
    if choice != "stop" {
        return None;
    }
    let p = probs.iter().find(|(k, _)| k == choice).map(|(_, p)| *p)?;
    (p >= SURE).then(|| Hint {
        kind: Kind::Stop,
        text: "This reads as a stop".into(),
        title: format!("This reads as a stop for the run that is working ({}).", sure_pct(p)),
        action: Kind::Stop.action(),
        decision: verdict.id.clone(),
        p,
    })
}

/// A handover rule that would take the message. Reads the same answer the send does, but
/// at the hint's bar and without needing a fit behind it: a hint changes nothing.
pub fn handover_hint(offered: &[Offered], verdict: &Verdict) -> Option<Hint> {
    let Answer::Choice { choice, probs, .. } = verdict.get("route")? else { return None };
    if choice == ROUTE_NONE || choice == ROUTE_UNSURE {
        return None;
    }
    let p = probs.iter().find(|(k, _)| k == choice).map(|(_, p)| *p)?;
    if p < SURE {
        return None;
    }
    let (engine_id, label) = choice.rsplit_once(':')?;
    let o = offered.iter().find(|o| o.row.id == engine_id)?;
    let rule = handover::rule_of(o, label)?;
    let name = &o.row.display_name;
    Some(Hint {
        kind: Kind::Handover,
        text: format!("{name} may take this (your rule {label})"),
        title: format!("{name} may take this under your rule {label}: {} ({}).", rule.text, sure_pct(p)),
        action: Kind::Handover.action(),
        decision: verdict.id.clone(),
        p,
    })
}

/// The id of the one group the composer asks about.
pub const WEB_UNIT: &str = "web_search";

/// The web is needed and Web search is off.
pub fn web_hint(verdict: &Verdict) -> Option<Hint> {
    let Answer::Noul { p_true } = verdict.get(WEB_UNIT)? else { return None };
    (*p_true >= SURE).then(|| Hint {
        kind: Kind::Web,
        text: "Needs the web, which is off".into(),
        title: format!("This looks like it needs the web, and Web search is off ({}).", sure_pct(*p_true)),
        action: Kind::Web.action(),
        decision: verdict.id.clone(),
        p: *p_true,
    })
}

/// The label a click on a hint writes for the decision that said it. Taking the action on
/// a stop or a handover means the reading was not what the person meant; taking the web
/// action means it was. A hint that is put away is not wanted, so it is not right either.
pub fn label(kind: Kind, taken: bool) -> &'static str {
    match (kind, taken) {
        (Kind::Web, true) => "right",
        _ => "wrong",
    }
}

/// `decision_outcomes.source` for a click on a hint.
pub const SOURCE: &str = "typing-hint";

// ---- which hint, of the three ----

/// What the composer can ask, so the order can be tried without an engine. Each is one ask
/// of the quick decision model, and `None` is no hint for any reason (not sure, no answer,
/// nothing to ask about).
pub trait Reader {
    /// A message to the working run reads as a stop.
    fn stop(&self) -> impl std::future::Future<Output = Option<Hint>> + Send;
    /// One of the person's handover rules would take a new message.
    fn handover(&self) -> impl std::future::Future<Output = Option<Hint>> + Send;
    /// A new message needs the web and Web search is off.
    fn web(&self) -> impl std::future::Future<Output = Option<Hint>> + Send;
}

/// The one hint there is, or none. A message to a working run is only ever a stop or not;
/// the handover and the tools belong to a new turn. For a new turn the handover comes before
/// the web, and an ask that is not needed is not made.
pub async fn first<R: Reader>(reader: &R, run_live: bool) -> Option<Hint> {
    if run_live {
        return reader.stop().await;
    }
    if let Some(hint) = reader.handover().await {
        return Some(hint);
    }
    reader.web().await
}

// ---- what the send reuses ----

struct Kept {
    purpose: Purpose,
    state: String,
    questions: Vec<String>,
    verdict: Verdict,
    at: Instant,
}

fn kept() -> &'static Mutex<HashMap<String, Vec<Kept>>> {
    static KEPT: OnceLock<Mutex<HashMap<String, Vec<Kept>>>> = OnceLock::new();
    KEPT.get_or_init(Default::default)
}

fn fingerprint(ask: &Ask<'_>) -> (String, Vec<String>) {
    (state_hash(ask.state), ask.questions.iter().map(|(id, _)| (*id).to_string()).collect())
}

/// Keep an answer given while typing, for the send of this conversation. One per purpose.
pub fn remember(conversation_id: &str, purpose: Purpose, ask: &Ask<'_>, verdict: &Verdict) {
    let (state, questions) = fingerprint(ask);
    let mut all = kept().lock().unwrap();
    // A conversation that was never sent from would otherwise keep its answers for good.
    all.retain(|_, v| v.iter().any(|k| k.at.elapsed() < KEEP_FOR));
    let slot = all.entry(conversation_id.to_string()).or_default();
    slot.retain(|k| k.purpose != purpose);
    slot.push(Kept { purpose, state, questions, verdict: verdict.clone(), at: Instant::now() });
}

/// The answer given while typing, when the send asks the same question about the same text.
/// Taken once: a second send of the same words is a new turn and asks again.
pub fn reuse(conversation_id: &str, purpose: Purpose, ask: &Ask<'_>) -> Option<Verdict> {
    let (state, questions) = fingerprint(ask);
    let mut all = kept().lock().unwrap();
    let slot = all.get_mut(conversation_id)?;
    let at = slot.iter().position(|k| {
        k.purpose == purpose && k.state == state && k.questions == questions && k.at.elapsed() < KEEP_FOR
    })?;
    Some(slot.remove(at).verdict)
}

/// Forget what a conversation kept, when its text changed past what was asked.
pub fn forget(conversation_id: &str) {
    kept().lock().unwrap().remove(conversation_id);
}

// ---- what reaches the log ----

/// The log's words for a decision about a message that was typed and not sent: the
/// draft's own words are never kept.
pub const TYPED_PREVIEW: &str = "A message being typed (its words are not kept)";
/// How many unwritten answers are held at most. Each pause of typing makes one or more.
const HELD_MAX: usize = 64;

struct Unwritten {
    record: crate::agent::log::DecisionRecord,
    model: String,
    cost_usd: Option<f64>,
    at: Instant,
}

fn unwritten() -> &'static Mutex<Vec<Unwritten>> {
    static HELD: OnceLock<Mutex<Vec<Unwritten>>> = OnceLock::new();
    HELD.get_or_init(Default::default)
}

/// Hold an answer given while typing instead of writing it: most pauses give no hint and
/// lead nowhere, and a draft that is never sent must leave nothing behind.
pub(crate) fn hold(record: crate::agent::log::DecisionRecord, model: &str, cost_usd: Option<f64>) {
    let mut held = unwritten().lock().unwrap();
    held.retain(|u| u.at.elapsed() < KEEP_FOR);
    if held.len() >= HELD_MAX {
        held.remove(0);
    }
    held.push(Unwritten { record, model: model.to_string(), cost_usd, at: Instant::now() });
}

fn take_held(decision: &str) -> Option<Unwritten> {
    let mut held = unwritten().lock().unwrap();
    let at = held.iter().position(|u| u.record.id == decision)?;
    Some(held.remove(at))
}

/// The person took or put away a hint: its decision is written now, so the label has a row,
/// without the words of the draft and with no conversation (no turn was made of it).
pub fn log_shown(db: &crate::db::Db, decision: &str) {
    let Some(mut u) = take_held(decision) else { return };
    u.record.state_preview = TYPED_PREVIEW.to_string();
    super::write_record(db, super::Opts::default(), &u.record, &u.model, u.cost_usd);
}

/// The send reused an answer given while typing: it is this turn's decision now, so it is
/// written with the turn, where its receipt and its labels find it. The words were sent.
pub fn log_sent(db: &crate::db::Db, decision: &str, conversation_id: &str, run_id: Option<&str>) {
    let Some(u) = take_held(decision) else { return };
    let opts = super::Opts { conversation_id: Some(conversation_id), run_id };
    super::write_record(db, opts, &u.record, &u.model, u.cost_usd);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::decide::sites::{self, SiteAsk};
    use crate::agent::decide::BackendId;
    use crate::db::engines::{EngineRow, EngineRule};
    use crate::engines::rules::{self, UseMode};
    use std::collections::BTreeMap;

    fn verdict(id: &str, answers: Vec<(&str, Answer)>) -> Verdict {
        Verdict {
            id: id.into(),
            answers: answers.into_iter().map(|(k, a)| (k.to_string(), a)).collect::<BTreeMap<_, _>>(),
            backend: BackendId::Local { model: "t".into() },
            calibrated: false,
            ms: 30,
        }
    }

    fn choice(pick: &str, p: f32, others: &[&str]) -> Answer {
        let rest = (1.0 - p) / others.len().max(1) as f32;
        let mut probs = vec![(pick.to_string(), p)];
        probs.extend(others.iter().map(|o| (o.to_string(), rest)));
        let confidence = crate::agent::decide::confidence(&probs.iter().map(|(_, p)| *p).collect::<Vec<_>>());
        Answer::Choice { choice: pick.into(), probs, confidence }
    }

    fn offered() -> Vec<Offered> {
        let mut row = EngineRow::new("claude-code", "claude-code", "Claude Code");
        row.enabled = true;
        let rules_ = rules::default_rules(&row.kind)
            .into_iter()
            .enumerate()
            .map(|(i, (kind, text))| EngineRule {
                id: format!("r{i}"),
                engine_id: row.id.clone(),
                kind: kind.into(),
                text: text.into(),
                enabled: true,
                auto: false,
                position: i as i64,
                source: "default".into(),
                hits: 0,
                overrides: 0,
            })
            .collect();
        vec![Offered { guards: rules::Guards::resolve("{}", &row.kind), mode: UseMode::Suggest, rules: rules_, row }]
    }

    #[test]
    fn only_a_message_of_twelve_characters_that_is_not_a_command_is_read() {
        assert!(askable("fix the failing tests"));
        assert!(askable("   twelve chars!   "));
        assert!(!askable("fix it"));
        assert!(!askable("           x"), "spaces do not count");
        assert!(!askable("/delegate claude-code fix the tests"), "a command is not a message");
        assert!(!askable(""));
        // Characters, not bytes.
        assert!(askable("ääääääääääää"));
        assert!(!askable("äääääääääää"));
    }

    #[test]
    fn a_stop_shows_only_when_it_is_the_winner_and_sure() {
        let sure = verdict("d1", vec![("steer", choice("stop", 0.93, &["redirect", "add", "question"]))]);
        let hint = stop_hint(&sure).unwrap();
        assert_eq!((hint.kind, hint.action, hint.decision.as_str()), (Kind::Stop, "Add it instead", "d1"));
        assert_eq!(hint.text, "This reads as a stop");
        assert!(!hint.text.contains('9'), "no number on the line");
        assert!(hint.title.contains("93% sure"), "the number is on the hover: {}", hint.title);

        let weak = verdict("d2", vec![("steer", choice("stop", 0.80, &["redirect", "add", "question"]))]);
        assert!(stop_hint(&weak).is_none(), "below the bar");
        let other = verdict("d3", vec![("steer", choice("add", 0.97, &["stop", "redirect", "question"]))]);
        assert!(stop_hint(&other).is_none(), "a sure addition is not a stop");
        assert!(stop_hint(&verdict("d4", vec![])).is_none());
    }

    #[test]
    fn a_handover_names_the_agent_and_the_rule_and_needs_no_fit() {
        let o = offered();
        let v = verdict("d1", vec![("route", choice("claude-code:U2", 0.91, &["claude-code:U1", "none", "unsure"]))]);
        let hint = handover_hint(&o, &v).unwrap();
        assert_eq!((hint.kind, hint.action), (Kind::Handover, "Do it yourself"));
        assert_eq!(hint.text, "Claude Code may take this (your rule U2)");
        assert!(hint.title.contains("91% sure") && hint.title.contains("U2"), "{}", hint.title);
        assert!(!v.calibrated, "a hint does not wait for a fit; the send still does");

        for (pick, p) in [("none", 0.97), ("unsure", 0.97), ("claude-code:U2", 0.80), ("claude-code:U9", 0.97), ("codex:U1", 0.97)] {
            let v = verdict("x", vec![("route", choice(pick, p, &["claude-code:U1", "none"]))]);
            assert!(handover_hint(&o, &v).is_none(), "{pick} at {p}");
        }
    }

    #[test]
    fn a_rule_the_person_switched_off_is_not_offered_as_a_hint() {
        let mut o = offered();
        let at = o[0].rules.iter().position(|r| r.kind == "use").unwrap();
        o[0].rules[at].enabled = false;
        let label = rules::labels(&o[0].rules).into_iter().find(|(_, r)| r.id == o[0].rules[at].id).map(|(l, _)| l).unwrap();
        let v = verdict("d", vec![("route", choice(&format!("claude-code:{label}"), 0.97, &["none"]))]);
        assert!(handover_hint(&o, &v).is_none());
    }

    #[test]
    fn the_web_hint_needs_a_sure_yes_for_the_one_group_asked_about() {
        assert_eq!(WEB_UNIT, "web_search");
        assert_eq!(crate::agent::toolsets::Toolset::WebSearch.id(), WEB_UNIT, "the unit the tool menu names");
        let v = verdict("d1", vec![(WEB_UNIT, Answer::Noul { p_true: 0.9 })]);
        let hint = web_hint(&v).unwrap();
        assert_eq!((hint.kind, hint.text.as_str(), hint.action), (Kind::Web, "Needs the web, which is off", "Turn on for this message"));
        assert!(hint.title.contains("90% sure"));
        assert!(web_hint(&verdict("d2", vec![(WEB_UNIT, Answer::Noul { p_true: 0.84 })])).is_none());
        assert!(web_hint(&verdict("d3", vec![(WEB_UNIT, Answer::Noul { p_true: 0.05 })])).is_none(), "a sure no is not a hint");
        assert!(web_hint(&verdict("d4", vec![])).is_none());
    }

    #[test]
    fn the_label_follows_what_the_person_did() {
        assert_eq!(label(Kind::Web, true), "right", "they did need the web");
        assert_eq!(label(Kind::Web, false), "wrong");
        assert_eq!(label(Kind::Stop, true), "wrong", "they meant to add");
        assert_eq!(label(Kind::Handover, true), "wrong");
        assert_eq!(label(Kind::Handover, false), "wrong");
    }

    fn ask_of(site: &SiteAsk) -> Ask<'_> {
        site.as_ask()
    }

    #[test]
    fn the_send_reuses_the_answer_for_the_same_text_once() {
        let site = sites::scope("units", "use metric", "Always give distances in kilometres.");
        let v = verdict("d1", vec![("route", choice("none", 0.9, &["a"]))]);
        remember("conv-reuse", Purpose::HandoverRoute, &ask_of(&site), &v);
        let got = reuse("conv-reuse", Purpose::HandoverRoute, &ask_of(&site)).unwrap();
        assert_eq!(got.id, "d1");
        assert!(reuse("conv-reuse", Purpose::HandoverRoute, &ask_of(&site)).is_none(), "taken once");
    }

    #[test]
    fn a_different_text_question_purpose_or_chat_gets_nothing() {
        let site = sites::scope("units", "use metric", "Always give distances in kilometres.");
        let other = sites::scope("units", "use metric", "Always give distances in miles.");
        let v = verdict("d1", vec![]);
        remember("conv-a", Purpose::HandoverRoute, &ask_of(&site), &v);
        assert!(reuse("conv-a", Purpose::HandoverRoute, &ask_of(&other)).is_none(), "the text changed");
        assert!(reuse("conv-a", Purpose::ToolSelect, &ask_of(&site)).is_none(), "another purpose");
        assert!(reuse("conv-b", Purpose::HandoverRoute, &ask_of(&site)).is_none(), "another chat");
        assert!(reuse("conv-a", Purpose::HandoverRoute, &ask_of(&site)).is_some(), "the right one is still there");
    }

    #[test]
    fn a_newer_answer_replaces_the_older_one_of_its_purpose() {
        let site = sites::scope("units", "use metric", "Always give distances in kilometres.");
        remember("conv-new", Purpose::HandoverRoute, &ask_of(&site), &verdict("old", vec![]));
        remember("conv-new", Purpose::HandoverRoute, &ask_of(&site), &verdict("new", vec![]));
        assert_eq!(reuse("conv-new", Purpose::HandoverRoute, &ask_of(&site)).unwrap().id, "new");
        forget("conv-new");
        assert!(reuse("conv-new", Purpose::HandoverRoute, &ask_of(&site)).is_none());
    }

    /// A reader that answers as told and counts what it was asked.
    struct Told {
        stop: Option<Hint>,
        handover: Option<Hint>,
        web: Option<Hint>,
        asked: Mutex<Vec<&'static str>>,
    }

    fn hint(kind: Kind) -> Hint {
        Hint { kind, text: format!("{kind:?}"), title: String::new(), action: kind.action(), decision: "d".into(), p: 0.9 }
    }

    impl Told {
        fn new(stop: bool, handover: bool, web: bool) -> Told {
            Told {
                stop: stop.then(|| hint(Kind::Stop)),
                handover: handover.then(|| hint(Kind::Handover)),
                web: web.then(|| hint(Kind::Web)),
                asked: Mutex::new(Vec::new()),
            }
        }
        fn asked(&self) -> Vec<&'static str> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl Reader for Told {
        async fn stop(&self) -> Option<Hint> {
            self.asked.lock().unwrap().push("stop");
            self.stop.clone()
        }
        async fn handover(&self) -> Option<Hint> {
            self.asked.lock().unwrap().push("handover");
            self.handover.clone()
        }
        async fn web(&self) -> Option<Hint> {
            self.asked.lock().unwrap().push("web");
            self.web.clone()
        }
    }

    /// `SEE-T-7`: priority. The handover comes before the web, and an ask that is not needed
    /// is not made.
    #[tokio::test]
    async fn the_handover_comes_before_the_web_and_stops_the_asking() {
        let both = Told::new(false, true, true);
        assert_eq!(first(&both, false).await.map(|h| h.kind), Some(Kind::Handover));
        assert_eq!(both.asked(), vec!["handover"], "the web was not asked about");

        let only_web = Told::new(false, false, true);
        assert_eq!(first(&only_web, false).await.map(|h| h.kind), Some(Kind::Web));
        assert_eq!(only_web.asked(), vec!["handover", "web"]);

        let nothing = Told::new(false, false, false);
        assert!(first(&nothing, false).await.is_none());
        assert_eq!(nothing.asked(), vec!["handover", "web"]);
    }

    /// A message to a working run is a stop or it is not: nothing else is read for it.
    #[tokio::test]
    async fn a_message_to_a_working_run_is_only_read_as_a_stop() {
        let everything = Told::new(true, true, true);
        assert_eq!(first(&everything, true).await.map(|h| h.kind), Some(Kind::Stop));
        assert_eq!(everything.asked(), vec!["stop"]);

        let not_a_stop = Told::new(false, true, true);
        assert!(first(&not_a_stop, true).await.is_none(), "a handover is for a new turn");
        assert_eq!(not_a_stop.asked(), vec!["stop"]);
    }

    #[test]
    fn every_kind_has_words_for_its_button() {
        for kind in [Kind::Stop, Kind::Handover, Kind::Web] {
            assert!(!kind.action().is_empty());
        }
    }
}
