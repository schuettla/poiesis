//! `USE-13`: the handover pre-match. Before the lead's first request of a user
//! turn, the decision model reads the message against the user's own rules for
//! the agent apps on offer (the same question `engines_route_preview_cmd` asks,
//! so the two share one calibration) and says which rule, if any, fits.
//!
//! What it may do is small on purpose. With a calibrated match of 0.85 or more:
//!
//! - an app in `suggest` mode gets its handover card **before the lead runs**;
//! - an app in `auto` mode, or a rule the user already said "always" to, gives
//!   the lead one line ("this looks like your rule U2") and the lead still decides.
//!
//! Anything else (uncalibrated, below the bar, `none`, `unsure`, a missed
//! deadline) changes nothing. It never starts an agent app by itself, and it never
//! refuses anything: the guards, the mode and the lead's own `delegate` call are
//! where those are decided.

use serde::Serialize;

use crate::db::engines::EngineRule;
use crate::engines::rules::{self, Offered, UseMode, ROUTE_NONE, ROUTE_UNSURE};

use super::policy::Purpose;
use super::sites::{self, SiteAsk};
use super::{Answer, Verdict};

/// The lead's `delegate` call cites a rule the pre-match scored below this: the
/// card says so. A hint, never a refusal.
pub const DOUBT_BELOW: f32 = 0.1;

/// The rule a pre-match picked.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub engine_id: String,
    /// `U2`.
    pub label: String,
    /// How sure the decision was, after calibration.
    pub p: f32,
}

/// What a pre-match said about a turn, kept on the run for the lead's `delegate`
/// call to be checked against.
#[derive(Debug, Clone, PartialEq)]
pub struct Prematch {
    pub decision_id: String,
    pub calibrated: bool,
    pub ms: u32,
    /// Every option's probability, as `<agent id>:<label>`.
    scores: Vec<(String, f32)>,
    pub picked: Option<Picked>,
    /// The user said yes to the picked rule (the early card, or "always for this").
    pub agreed: bool,
}

impl Prematch {
    /// The decision's answer as the run keeps it. `None` when it has no routing
    /// answer at all.
    pub fn of(verdict: &Verdict, offered: &[Offered]) -> Option<Prematch> {
        let Answer::Choice { probs, .. } = verdict.get("route")? else { return None };
        Some(Prematch {
            decision_id: verdict.id.clone(),
            calibrated: verdict.calibrated,
            ms: verdict.ms,
            scores: probs.clone(),
            picked: pick(offered, verdict),
            agreed: false,
        })
    }

    /// How likely the decision thought rule `label` of `engine_id` was.
    pub fn score(&self, engine_id: &str, label: &str) -> Option<f32> {
        let key = format!("{engine_id}:{label}");
        self.scores.iter().find(|(k, _)| *k == key).map(|(_, p)| *p)
    }

    /// The decision was sure this rule did **not** fit, and the lead cited it. Only
    /// a calibrated number says so: an unchecked 0.06 is not worth a line.
    pub fn doubt(&self, engine_id: &str, label: &str) -> Option<f32> {
        if !self.calibrated {
            return None;
        }
        self.score(engine_id, label).filter(|p| *p < DOUBT_BELOW)
    }

    /// This is the rule the pre-match picked, and how sure it was.
    pub fn matched(&self, engine_id: &str, label: &str) -> Option<f32> {
        self.picked.as_ref().filter(|p| p.engine_id == engine_id && p.label == label).map(|p| p.p)
    }

    /// The user already said yes to this rule in this turn.
    pub fn agreed_to(&self, engine_id: &str, label: &str) -> bool {
        self.agreed && self.matched(engine_id, label).is_some()
    }
}

/// What the pre-match has to say about a rule the lead just cited in `delegate`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Against {
    /// It was the rule the pre-match picked, and how sure it was.
    pub matched: Option<f32>,
    /// The user already said yes to it, so no second card.
    pub agreed: bool,
    /// The pre-match scored it low (calibrated only).
    pub doubt: Option<f32>,
}

pub fn against(pre: Option<&Prematch>, engine_id: &str, label: &str) -> Against {
    let Some(pre) = pre else { return Against::default() };
    Against {
        matched: pre.matched(engine_id, label),
        agreed: pre.agreed_to(engine_id, label),
        doubt: pre.doubt(engine_id, label),
    }
}

/// The routing question for a live turn, or `None` when there is nothing to ask:
/// no message, or no rule that says to use any agent on offer.
pub fn ask_for(request: &str, previous: &str, offered: &[Offered]) -> Option<SiteAsk> {
    if request.trim().is_empty() {
        return None;
    }
    let options = rules::route_options(offered);
    if options.is_empty() {
        return None;
    }
    Some(sites::route_after(request, previous, options, &rules::route_avoid_lines(offered)))
}

/// The rule a decision picked, when it is calibrated, names a rule the user has
/// switched on that says to use an agent on offer, and reaches the live bar.
pub fn pick(offered: &[Offered], verdict: &Verdict) -> Option<Picked> {
    if !verdict.calibrated {
        return None;
    }
    let Some(answer @ Answer::Choice { choice, probs, .. }) = verdict.get("route") else { return None };
    if choice == ROUTE_NONE || choice == ROUTE_UNSURE {
        return None;
    }
    if !Purpose::HandoverRoute.live_policy().act_at.reaches(answer, true) {
        return None;
    }
    let (engine_id, label) = choice.rsplit_once(':')?;
    let o = offered.iter().find(|o| o.row.id == engine_id)?;
    rule_of(o, label)?;
    let p = probs.iter().find(|(k, _)| k == choice).map(|(_, p)| *p)?;
    Some(Picked { engine_id: engine_id.into(), label: label.into(), p })
}

/// The enabled "use it for" rule of this label.
pub fn rule_of<'a>(o: &'a Offered, label: &str) -> Option<&'a EngineRule> {
    rules::labels(&o.rules).into_iter().find(|(l, r)| l == label && r.kind == "use" && r.enabled).map(|(_, r)| r)
}

/// What a picked rule leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Ask the user now, before the lead runs.
    Card,
    /// Tell the lead, which still decides.
    Hint,
}

pub fn step(o: &Offered, rule: &EngineRule) -> Step {
    if o.mode == UseMode::Suggest && !rule.auto {
        Step::Card
    } else {
        Step::Hint
    }
}

/// The lines the lead reads. Fixed words around the user's own rule label, so no
/// model text reaches the lead's prompt.
pub fn line_agreed(agent: &str, label: &str) -> String {
    format!(
        "The user agreed to hand this to {agent} under their rule {label}. Do it now: call delegate with agent \"{agent}\" and because \"{label}\". Write the task so it can be done without seeing this conversation."
    )
}

pub fn line_looks_like(agent: &str, label: &str, p: f32) -> String {
    format!("This looks like the user's rule {label} for {agent} ({p:.2}). You still decide.")
}

pub fn line_declined(agent: &str, label: &str) -> String {
    format!("The user was asked whether to hand this to {agent} under their rule {label} and said to do it yourself. Do it yourself.")
}

/// What the lead reads after the person took **Do it yourself** on the composer's hint
/// (`SEE-3`), before any rule was matched. No rule or agent is named, because none was asked.
pub const LINE_YOURSELF: &str = "The user said to do this one yourself. Do not hand it to another agent app.";

/// The card's second line, when the lead cites a rule the pre-match scored low.
/// Words only (`SEE-4`); Activity adds the number.
pub fn line_doubt(label: &str) -> String {
    format!("I'm not sure this matches {label}.")
}

/// How the early card was answered, for the label it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Label {
    Right,
    Wrong,
}

impl Label {
    pub fn name(self) -> &'static str {
        match self {
            Label::Right => "right",
            Label::Wrong => "wrong",
        }
    }
}

/// `decision_outcomes.source` for a card the pre-match raised.
pub const CARD_SOURCE: &str = "handover-card";

// ---- CTL-T-3: the handover slice ----

/// A request labelled with the rule a person would pick, or `none`.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub id: String,
    pub request: String,
    /// The assistant's last reply, for a request that only makes sense after it.
    pub previous: String,
    /// `claude-code:U2`, or `none`.
    pub expect: String,
}

const GOLDEN: &str = include_str!("../../../tests/eval/decide/handover-prematch.jsonl");

/// The labelled requests. A line that does not parse is dropped, and the test
/// below fails if any was.
pub fn cases() -> Vec<Case> {
    GOLDEN
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| {
            let text = |k: &str| v.get(k).and_then(|t| t.as_str()).map(str::to_string);
            Some(Case { id: text("id")?, request: text("request")?, previous: text("previous").unwrap_or_default(), expect: text("expect")? })
        })
        .collect()
}

/// The agent the slice offers: Claude Code with the rules a fresh install writes
/// (`rules::default_rules`), in `suggest` mode. One agent, so that "the rule a
/// person would pick" has one answer.
pub fn default_agent() -> Offered {
    let row = crate::db::engines::EngineRow::new("claude-code", "claude-code", "Claude Code");
    let rules = rules::default_rules(&row.kind)
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
    Offered { guards: rules::Guards::resolve("{}", &row.kind), mode: UseMode::Suggest, rules, row }
}

/// What the slice found. A card is raised when the decision names a rule at the
/// bar; it is right when that is the rule a person would pick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slice {
    pub total: usize,
    /// Requests where a person would pick a rule.
    pub wanted: usize,
    pub cards: usize,
    pub correct: usize,
}

impl Slice {
    /// Of the cards raised, the share that were right. `None` when none were.
    pub fn precision(&self) -> Option<f64> {
        (self.cards > 0).then(|| self.correct as f64 / self.cards as f64)
    }

    /// Of the requests that wanted a rule, the share that got the right card.
    pub fn recall(&self) -> Option<f64> {
        (self.wanted > 0).then(|| self.correct as f64 / self.wanted as f64)
    }
}

/// Score a model's answers at `bar` (the top probability a card needs). `answer`
/// gives the routing answer for a case; one that is not a choice raises no card.
pub fn score(cases: &[Case], answer: &dyn Fn(&Case) -> Option<Answer>, bar: f32) -> Slice {
    let mut out = Slice { total: cases.len(), ..Default::default() };
    for case in cases {
        if case.expect != ROUTE_NONE {
            out.wanted += 1;
        }
        let Some(Answer::Choice { choice, probs, .. }) = answer(case) else { continue };
        let top = probs.iter().map(|(_, p)| *p).fold(0.0, f32::max);
        if choice == ROUTE_NONE || choice == ROUTE_UNSURE || top < bar {
            continue;
        }
        out.cards += 1;
        if choice == case.expect {
            out.correct += 1;
        }
    }
    out
}

/// The most a wrong early card may cost the person: one click, so nearly every
/// card has to be right.
pub const SHIP_PRECISION: f64 = 0.95;

/// The ship rule for `fast-slow`'s handover feature: precision at the bar must be
/// at least 0.95. A model that raises no card has nothing to be wrong about, so it
/// passes, and its recall says how little it did.
pub fn ship(slice: &Slice) -> Result<(), String> {
    match slice.precision() {
        Some(p) if p < SHIP_PRECISION => Err(format!(
            "{} of {} early cards were right ({:.0}%); at least {:.0}% is needed",
            slice.correct,
            slice.cards,
            p * 100.0,
            SHIP_PRECISION * 100.0
        )),
        _ => Ok(()),
    }
}

/// One line for the eval to print.
pub fn report(slice: &Slice, bar: f32) -> String {
    let pct = |v: Option<f64>| v.map_or("n/a".to_string(), |v| format!("{:.0}%", v * 100.0));
    format!(
        "handover slice at {bar:.2}: requests={} wanted a rule={} cards={} right={} precision={} recall={}",
        slice.total,
        slice.wanted,
        slice.cards,
        slice.correct,
        pct(slice.precision()),
        pct(slice.recall())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::decide::{BackendId, Answer};
    use crate::db::engines::EngineRow;
    use crate::db::Db;
    use std::collections::BTreeMap;

    fn offered(db: &Db, mode: &str) -> Vec<Offered> {
        let mut row = EngineRow::new("claude-code", "claude-code", "Claude Code");
        row.found_path = Some("C:\\x\\claude.exe".into());
        row.enabled = true;
        row.signed_in = Some(true);
        row.use_mode = mode.into();
        db.insert_engine_if_absent(&row).unwrap();
        db.save_engine_settings(&row).unwrap();
        rules::candidates(db)
    }

    fn verdict(choice: &str, probs: &[(&str, f32)], calibrated: bool) -> Verdict {
        let p: Vec<(String, f32)> = probs.iter().map(|(k, p)| ((*k).to_string(), *p)).collect();
        let confidence = crate::agent::decide::confidence(&p.iter().map(|(_, p)| *p).collect::<Vec<_>>());
        Verdict {
            id: "d1".into(),
            answers: BTreeMap::from([("route".to_string(), Answer::Choice { choice: choice.into(), probs: p, confidence })]),
            backend: BackendId::Local { model: "test".into() },
            calibrated,
            ms: 41,
        }
    }

    fn sure(choice: &str, p: f32, calibrated: bool) -> Verdict {
        let rest = (1.0 - p) / 5.0;
        verdict(
            choice,
            &[
                ("claude-code:U1", if choice == "claude-code:U1" { p } else { rest }),
                ("claude-code:U2", if choice == "claude-code:U2" { p } else { rest }),
                ("claude-code:U3", rest),
                ("claude-code:U4", rest),
                ("none", if choice == "none" { p } else { rest }),
                ("unsure", if choice == "unsure" { p } else { rest }),
            ],
            calibrated,
        )
    }

    #[test]
    fn the_live_row_has_its_own_deadline_bar_and_experiment() {
        let live = Purpose::HandoverRoute.live_policy();
        let preview = Purpose::HandoverRoute.policy();
        assert_eq!(live.experiment, Some("fast-slow"));
        assert_eq!(preview.experiment, None, "the preview is a swap, not an experiment");
        assert!(live.deadline < preview.deadline);
        assert_eq!(live.deadline.as_millis(), 300);
        assert!(live.act_at.uncalibrated >= live.act_at.calibrated);
        // The other purposes read the same row in both.
        assert_eq!(Purpose::GoalMet.live_policy().deadline, Purpose::GoalMet.policy().deadline);
    }

    /// `USE-T-4`: only a calibrated match at the bar picks anything.
    #[test]
    fn a_calibrated_match_at_the_bar_is_picked_and_nothing_else_is() {
        let db = Db::open_in_memory().unwrap();
        let o = offered(&db, "suggest");
        let hit = pick(&o, &sure("claude-code:U2", 0.91, true)).unwrap();
        assert_eq!((hit.engine_id.as_str(), hit.label.as_str()), ("claude-code", "U2"));
        assert!((hit.p - 0.91).abs() < 1e-6);

        assert!(pick(&o, &sure("claude-code:U2", 0.91, false)).is_none(), "uncalibrated changes nothing");
        assert!(pick(&o, &sure("claude-code:U2", 0.80, true)).is_none(), "below the bar");
        assert!(pick(&o, &sure("none", 0.97, true)).is_none());
        assert!(pick(&o, &sure("unsure", 0.97, true)).is_none());
        assert!(pick(&o, &sure("claude-code:U9", 0.97, true)).is_none(), "a rule that does not exist");
        assert!(pick(&o, &sure("codex:U1", 0.97, true)).is_none(), "an agent that is not on offer");
    }

    #[test]
    fn a_switched_off_or_avoid_rule_is_never_picked() {
        let db = Db::open_in_memory().unwrap();
        let mut o = offered(&db, "suggest");
        o[0].rules[1].enabled = false;
        assert!(pick(&o, &sure("claude-code:U2", 0.97, true)).is_none());
        // `A1` is an avoid rule: it is not a use rule, so it is never an option to pick.
        assert!(pick(&o, &sure("claude-code:A1", 0.97, true)).is_none());
    }

    /// `USE-T-4`: the mode decides between a card and a line for the lead.
    #[test]
    fn suggest_mode_asks_and_auto_mode_or_a_standing_yes_only_tells_the_lead() {
        let db = Db::open_in_memory().unwrap();
        let o = offered(&db, "suggest");
        let rule = rule_of(&o[0], "U2").unwrap().clone();
        assert_eq!(step(&o[0], &rule), Step::Card);
        let standing = EngineRule { auto: true, ..rule.clone() };
        assert_eq!(step(&o[0], &standing), Step::Hint);

        let auto = offered_auto(&db);
        assert_eq!(step(&auto[0], &rule), Step::Hint);
    }

    fn offered_auto(db: &Db) -> Vec<Offered> {
        let mut row = db.get_engine("claude-code").unwrap().unwrap();
        row.use_mode = "auto".into();
        db.save_engine_settings(&row).unwrap();
        rules::candidates(db)
    }

    #[test]
    fn no_use_rule_or_no_message_means_nothing_to_ask() {
        let db = Db::open_in_memory().unwrap();
        let o = offered(&db, "suggest");
        assert!(ask_for("fix the failing tests", "", &o).is_some());
        assert!(ask_for("   ", "", &o).is_none());
        assert!(ask_for("fix the tests", "", &[]).is_none());
    }

    #[test]
    fn the_question_carries_the_last_reply_after_the_request() {
        let db = Db::open_in_memory().unwrap();
        let o = offered(&db, "suggest");
        let ask = ask_for("do that in the other module too", "I renamed the config options in billing.", &o).unwrap();
        assert!(ask.state.starts_with("The user's request:\ndo that in the other module too"), "{}", ask.state);
        assert!(ask.state.contains("The assistant's last reply:\nI renamed the config options"));
        assert!(crate::agent::decide::local::check_ask(&ask.as_ask()).is_ok());
    }

    /// `USE-T-4`: the check on the lead needs a calibrated, low score.
    #[test]
    fn the_lead_citing_a_rule_the_decision_scored_low_is_doubted_only_when_calibrated() {
        let db = Db::open_in_memory().unwrap();
        let o = offered(&db, "suggest");
        let v = verdict(
            "claude-code:U1",
            &[("claude-code:U1", 0.9), ("claude-code:U2", 0.06), ("claude-code:U3", 0.02), ("none", 0.01), ("unsure", 0.01)],
            true,
        );
        let pm = Prematch::of(&v, &o).unwrap();
        assert_eq!(pm.doubt("claude-code", "U2"), Some(0.06));
        assert_eq!(pm.doubt("claude-code", "U1"), None, "the rule it picked is not doubted");
        assert_eq!(pm.doubt("claude-code", "U7"), None, "a rule it never scored says nothing");

        let unchecked = Prematch::of(&verdict("claude-code:U1", &[("claude-code:U1", 0.9), ("claude-code:U2", 0.06)], false), &o).unwrap();
        assert_eq!(unchecked.doubt("claude-code", "U2"), None);
        assert_eq!(line_doubt("U2"), "I'm not sure this matches U2.");
    }

    #[test]
    fn a_yes_counts_only_for_the_rule_that_was_picked() {
        let db = Db::open_in_memory().unwrap();
        let o = offered(&db, "suggest");
        let mut pm = Prematch::of(&sure("claude-code:U2", 0.93, true), &o).unwrap();
        assert!(!pm.agreed_to("claude-code", "U2"));
        pm.agreed = true;
        assert!(pm.agreed_to("claude-code", "U2"));
        assert!(!pm.agreed_to("claude-code", "U1"), "a different rule still asks");
        assert_eq!(pm.matched("claude-code", "U2"), Some(0.93));
    }

    // ---- CTL-T-3: the handover slice ----

    #[test]
    fn the_golden_requests_all_parse_and_name_a_rule_that_exists_or_none() {
        let all = cases();
        assert_eq!(all.len(), GOLDEN.lines().filter(|l| !l.trim().is_empty()).count(), "a line did not parse");
        assert!(all.len() >= 60, "the plan asks for 60 or more, got {}", all.len());
        let agent = default_agent();
        let options: Vec<String> = rules::route_options(&[agent.clone()]).into_iter().map(|(id, _)| id).collect();
        let mut seen = std::collections::HashSet::new();
        for c in &all {
            assert!(seen.insert(c.id.clone()), "{} is there twice", c.id);
            assert!(options.contains(&c.expect), "{} expects {}, which is not an option", c.id, c.expect);
            let ask = ask_for(&c.request, &c.previous, &[agent.clone()]).unwrap();
            assert!(crate::agent::decide::local::check_ask(&ask.as_ask()).is_ok(), "{}", c.id);
        }
        // Every rule is asked about, and so is "none".
        for want in ["claude-code:U1", "claude-code:U2", "claude-code:U3", "claude-code:U4", "none"] {
            assert!(all.iter().filter(|c| c.expect == want).count() >= 6, "too few for {want}");
        }
        assert!(all.iter().any(|c| !c.previous.is_empty()), "some requests only make sense after the last reply");
    }

    #[test]
    fn the_slice_offers_the_rules_a_fresh_install_has() {
        let agent = default_agent();
        assert_eq!(rules::route_options(&[agent.clone()]).len(), 4 + 2, "four use rules, none and unsure");
        assert_eq!(rules::route_avoid_lines(&[agent]).len(), 3);
    }

    fn answering(choice: &str, p: f32) -> Answer {
        let rest = (1.0 - p) / 2.0;
        let probs = vec![(choice.to_string(), p), ("a".to_string(), rest), ("b".to_string(), rest)];
        let confidence = crate::agent::decide::confidence(&probs.iter().map(|(_, p)| *p).collect::<Vec<_>>());
        Answer::Choice { choice: choice.into(), probs, confidence }
    }

    #[test]
    fn a_model_that_always_picks_what_a_person_would_gets_every_card_right() {
        let all = cases();
        let s = score(&all, &|c| Some(answering(&c.expect, 0.97)), 0.85);
        assert_eq!(s.cards, s.wanted, "no card for a request that wants none");
        assert_eq!((s.precision(), s.recall()), (Some(1.0), Some(1.0)));
        assert_eq!(s.total, all.len());
        assert!(ship(&s).is_ok());
    }

    #[test]
    fn a_model_that_is_unsure_or_below_the_bar_raises_no_card_and_passes_with_no_recall() {
        let all = cases();
        let unsure = score(&all, &|_| Some(answering("unsure", 0.99)), 0.85);
        assert_eq!((unsure.cards, unsure.precision(), unsure.recall()), (0, None, Some(0.0)));
        assert!(ship(&unsure).is_ok());
        let wavering = score(&all, &|c| Some(answering(&c.expect, 0.80)), 0.85);
        assert_eq!(wavering.cards, 0, "the bar is the top probability");
        assert_eq!(score(&all, &|_| None, 0.85).cards, 0, "no answer, no card");
    }

    #[test]
    fn a_model_that_names_the_wrong_rule_confidently_fails_the_ship_rule() {
        let all = cases();
        let s = score(&all, &|_| Some(answering("claude-code:U1", 0.97)), 0.85);
        assert_eq!(s.cards, all.len(), "a wrong rule is a card even for a request that wants none");
        assert!(s.precision().unwrap() < 0.5);
        let why = ship(&s).unwrap_err();
        assert!(why.contains("early cards were right") && why.contains("95%"), "{why}");
        assert!(report(&s, 0.85).contains("precision="));
    }

    #[test]
    fn the_ship_rule_is_ninety_five_percent_of_the_cards_raised() {
        let at = |correct| Slice { total: 67, wanted: 36, cards: 20, correct };
        assert!(ship(&at(19)).is_ok());
        assert!(ship(&at(18)).is_err());
    }

    /// `SEE-3`: what the lead reads after **Do it yourself** on the composer's hint names no
    /// model text, no agent and no rule.
    #[test]
    fn the_line_after_do_it_yourself_is_fixed_words() {
        assert!(LINE_YOURSELF.starts_with("The user said to do this one yourself."));
        assert!(LINE_YOURSELF.ends_with("another agent app."));
        assert!(!LINE_YOURSELF.contains('{') && !LINE_YOURSELF.contains("U2"));
    }

    #[test]
    fn the_lines_the_lead_reads_are_fixed_words_around_the_rule() {
        assert_eq!(
            line_looks_like("Claude Code", "U2", 0.913),
            "This looks like the user's rule U2 for Claude Code (0.91). You still decide."
        );
        assert!(line_agreed("Claude Code", "U2").contains("agent \"Claude Code\" and because \"U2\""));
        assert!(line_declined("Claude Code", "U2").ends_with("Do it yourself."));
    }
}
