//! `CTL-3`: what happened after a decision, as labels calibration can use.
//!
//! Two kinds, kept apart on purpose (decision 14):
//!
//! - **Read from what already exists.** A user who said no to a handover has
//!   already left that in `engine_signals`. Calibration joins it to the decision
//!   when it runs. Nothing is copied into `decision_outcomes`, so the two cannot
//!   drift.
//! - **Written new**, for the things nothing else records. [`record_goal_event`]
//!   is the first: the goal's end state lives on the screen, and this is the
//!   hook the goal loop (`LRN-1`) left for this.
//!
//! Labels are data for calibration. They are never read as instructions.

use super::calibration::{self, Obs, Truth, WEAK_WEIGHT};
use super::policy::Purpose;
use crate::db::decisions::DecisionRow;
use crate::db::{now_ms, Db};

/// How far after a decision a signal still counts as the user's answer to it.
const JOIN_WINDOW_MS: i64 = 6 * 60 * 60 * 1000;
/// "At once" for a new goal after one judged met: ten minutes.
const NEW_GOAL_WINDOW_MS: i64 = 10 * 60 * 1000;
/// A goal round judged "not yet" counts as a confident one at or below this.
const CONFIDENT_NOT_YET: f32 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Right,
    Wrong,
    /// Inferred from what the user did next, not said.
    WeakRight,
    WeakWrong,
    /// The user gave the right answer.
    Corrected,
}

impl Kind {
    pub fn code(self) -> &'static str {
        match self {
            Kind::Right => "right",
            Kind::Wrong => "wrong",
            Kind::WeakRight => "weak_right",
            Kind::WeakWrong => "weak_wrong",
            Kind::Corrected => "corrected",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "right" => Kind::Right,
            "wrong" => Kind::Wrong,
            "weak_right" => Kind::WeakRight,
            "weak_wrong" => Kind::WeakWrong,
            "corrected" => Kind::Corrected,
            _ => return None,
        })
    }

    pub fn weight(self) -> f32 {
        match self {
            Kind::WeakRight | Kind::WeakWrong => WEAK_WEIGHT,
            _ => 1.0,
        }
    }
}

/// One label on one decision, wherever it was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub kind: Kind,
    pub value: Option<String>,
    pub source: String,
}

/// A decision's labels: those written for it, and those read from the tables
/// that record what the user did.
pub fn labels_for(db: &Db, row: &DecisionRow) -> Vec<Label> {
    let mut out: Vec<Label> = row
        .outcomes
        .iter()
        .filter_map(|o| Some(Label { kind: Kind::parse(&o.label)?, value: o.value.clone(), source: o.source.clone() }))
        .collect();
    if row.purpose == Purpose::HandoverRoute.name() {
        out.extend(handover_labels(db, row));
    }
    out
}

/// The request a routing decision was about, back out of its logged preview.
fn request_of(row: &DecisionRow) -> String {
    squash(row.state_preview.trim_start_matches("The user's request:"))
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Two clipped copies of one request: one is the start of the other. A short
/// stub is not evidence of anything, so it has to be a real stretch of text.
fn same_request(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim_end_matches('…'), b.trim_end_matches('…'));
    let n = a.chars().count().min(b.chars().count());
    n >= 12 && a.chars().take(n).eq(b.chars().take(n))
}

/// `HandoverRoute`, from `engine_signals`:
/// - the user said no to a handover that cited a rule this decision chose
///   (`override`): the choice was wrong;
/// - the user named an agent with `/delegate` on a request the decision called
///   "no rule fits" (`asked`): that was wrong too.
///
/// The decision's own answer says which it was, so the same signal cannot label
/// a decision that chose something else.
fn handover_labels(db: &Db, row: &DecisionRow) -> Vec<Label> {
    let Some(chosen) = row.answers.as_object().and_then(|a| a.values().next()).and_then(|a| a.get("choice")).and_then(|c| c.as_str())
    else {
        return Vec::new();
    };
    let request = request_of(row);
    let until = row.at + JOIN_WINDOW_MS;
    let after = row.at - 1;
    let mut out = Vec::new();
    let matching = |signals: Vec<(i64, String)>| signals.into_iter().any(|(at, task)| at <= until && same_request(&task, &request));
    if chosen == crate::engines::rules::ROUTE_NONE {
        let Ok(engines) = db.list_engines() else { return out };
        let asked = engines
            .iter()
            .any(|e| matching(db.engine_signals_since(&e.id, None, "asked", after).unwrap_or_default()));
        if asked {
            out.push(Label { kind: Kind::Wrong, value: None, source: "engine-asked".into() });
        }
    } else if let Some((engine_id, _)) = chosen.rsplit_once(':') {
        if matching(db.engine_signals_since(engine_id, None, "override", after).unwrap_or_default()) {
            out.push(Label { kind: Kind::Wrong, value: None, source: "engine-override".into() });
        }
    }
    out
}

/// A labelled decision as a calibration observation. The strongest label wins:
/// a stated answer over an inferred one, a correction over a bare verdict.
pub fn observation(row: &DecisionRow, labels: &[Label]) -> Option<Obs> {
    let (names, probs) = calibration::probs_of(&row.answers)?;
    let winner = probs.iter().enumerate().fold(0, |best, (i, p)| if *p > probs[best] { i } else { best });
    let best = labels
        .iter()
        .max_by(|a, b| a.kind.weight().total_cmp(&b.kind.weight()).then((a.kind == Kind::Corrected).cmp(&(b.kind == Kind::Corrected))))?;
    let truth = match best.kind {
        Kind::Right | Kind::WeakRight => Truth::Class(winner),
        Kind::Wrong | Kind::WeakWrong => Truth::NotClass(winner),
        Kind::Corrected => best
            .value
            .as_ref()
            .and_then(|v| names.iter().position(|n| n == v))
            .map_or(Truth::NotClass(winner), Truth::Class),
    };
    Some(Obs { probs, truth, weight: best.kind.weight() })
}

/// `ToolSelect` asks one yes-or-no question per unit, so one decision carries
/// several answers and each label names its unit in `value`. A label on a unit
/// that was held back says what became true: `wrong` and `weak_wrong` mean it
/// was needed, `right` and `weak_right` mean it was not.
fn unit_observations(row: &DecisionRow, labels: &[Label]) -> Vec<Obs> {
    labels
        .iter()
        .filter_map(|label| {
            let unit = label.value.as_deref()?;
            let p = row.answers.get(unit)?.get("p")?.as_f64()? as f32;
            let needed = matches!(label.kind, Kind::Wrong | Kind::WeakWrong | Kind::Corrected);
            Some(Obs {
                probs: vec![p, 1.0 - p],
                truth: Truth::Class(if needed { 0 } else { 1 }),
                weight: label.kind.weight(),
            })
        })
        .collect()
}

/// Every labelled decision of one (model, purpose), ready to fit.
pub fn observations(db: &Db, purpose: &str, model: &str) -> Vec<Obs> {
    db.decisions_for(purpose, model)
        .unwrap_or_default()
        .iter()
        .flat_map(|row| {
            let labels = labels_for(db, row);
            if labels.is_empty() {
                return Vec::new();
            }
            if row.purpose == Purpose::ToolSelect.name() {
                return unit_observations(row, &labels);
            }
            observation(row, &labels).into_iter().collect()
        })
        .collect()
}

/// What happened to a goal that a decision had judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalEvent {
    /// The goal loop stopped because it was stuck (`LRN-1`).
    Stalled,
    /// The goal ran out of rounds.
    Exhausted,
    /// The user set a new goal.
    NewGoal,
}

impl GoalEvent {
    pub fn parse(s: &str) -> Option<GoalEvent> {
        Some(match s {
            "stalled" => GoalEvent::Stalled,
            "exhausted" => GoalEvent::Exhausted,
            "new-goal" => GoalEvent::NewGoal,
            _ => return None,
        })
    }

    fn source(self) -> &'static str {
        match self {
            GoalEvent::Stalled => "goal-stalled",
            GoalEvent::Exhausted => "goal-exhausted",
            GoalEvent::NewGoal => "goal-replaced",
        }
    }
}

/// `CTL-3`: label the goal check's last decision in a conversation.
/// - Stalled or exhausted after a round the decision judged "not yet" with high
///   confidence: weak right (the goal really was not met).
/// - A new goal at once after one the decision judged met: weak wrong (the user
///   did not think it was done).
///
/// Returns the label written, if any.
pub fn record_goal_event(db: &Db, conversation_id: &str, event: GoalEvent) -> Option<Kind> {
    record_goal_event_at(db, conversation_id, event, now_ms())
}

fn record_goal_event_at(db: &Db, conversation_id: &str, event: GoalEvent, now: i64) -> Option<Kind> {
    let row = db.latest_decision(conversation_id, Purpose::GoalMet.name()).ok().flatten()?;
    let (_, probs) = calibration::probs_of(&row.answers)?;
    let p_met = *probs.first()?;
    let kind = match event {
        GoalEvent::Stalled | GoalEvent::Exhausted if p_met <= CONFIDENT_NOT_YET => Kind::WeakRight,
        GoalEvent::NewGoal
            if p_met >= Purpose::GoalMet.policy().act_at.at(row.calibrated) && now - row.at <= NEW_GOAL_WINDOW_MS =>
        {
            Kind::WeakWrong
        }
        _ => return None,
    };
    let written = db.add_outcome(&row.id, kind.code(), None, event.source()).ok()?;
    written.then_some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::decisions::NewDecision;

    fn add(db: &Db, id: &str, purpose: &str, conv: Option<&str>, answers: serde_json::Value, preview: &str) {
        db.add_decision(&NewDecision {
            id,
            conversation_id: conv,
            run_id: None,
            purpose,
            backend: "local:Clef-flash",
            model: "Clef-Flash-Q4_K_M",
            ms: 10,
            calibrated: false,
            answers: &answers,
            input_tokens: 10,
            state_preview: preview,
            state_hash: id,
            action: None,
        })
        .unwrap();
    }

    fn route_answer(choice: &str) -> serde_json::Value {
        serde_json::json!({ "route": { "choice": choice, "probs": [[choice, 0.9], ["other", 0.1]], "confidence": 0.5 } })
    }

    fn row(db: &Db, id: &str) -> DecisionRow {
        db.decisions_recent(50, None).unwrap().into_iter().find(|r| r.id == id).unwrap()
    }

    // ---- CTL-T-2: read from what already exists ----

    /// The user said no to a handover that cited a rule the decision chose: that
    /// joins as `wrong`, and nothing is copied into `decision_outcomes`.
    #[test]
    fn an_override_after_a_matched_rule_joins_as_wrong_without_being_copied() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "d1", "handover-route", None, route_answer("claude-code:U2"), "The user's request:\nrefactor the billing module and add tests");
        db.add_engine_signal("claude-code", Some("r1"), "override", "refactor the billing module and add tests").unwrap();
        let r = row(&db, "d1");
        let labels = labels_for(&db, &r);
        assert_eq!(labels, vec![Label { kind: Kind::Wrong, value: None, source: "engine-override".into() }]);
        assert!(r.outcomes.is_empty(), "read, not copied");
        assert_eq!(db.outcomes_since(None, None, 0).unwrap(), 0);
    }

    #[test]
    fn a_signal_about_another_agent_or_another_request_labels_nothing() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "d1", "handover-route", None, route_answer("claude-code:U2"), "The user's request:\nrefactor the billing module and add tests");
        db.add_engine_signal("codex", Some("r1"), "override", "refactor the billing module and add tests").unwrap();
        db.add_engine_signal("claude-code", Some("r1"), "override", "write a poem about the sea in spring").unwrap();
        assert!(labels_for(&db, &row(&db, "d1")).is_empty());
    }

    #[test]
    fn an_older_signal_does_not_label_a_newer_decision() {
        let db = Db::open_in_memory().unwrap();
        db.add_engine_signal_at("claude-code", Some("r1"), "override", "refactor the billing module and add tests", 1);
        add(&db, "d1", "handover-route", None, route_answer("claude-code:U2"), "The user's request:\nrefactor the billing module and add tests");
        assert!(labels_for(&db, &row(&db, "d1")).is_empty());
    }

    /// `/delegate <agent>` on a request the decision said no rule fits: wrong.
    #[test]
    fn naming_an_agent_after_a_confident_none_joins_as_wrong() {
        let db = Db::open_in_memory().unwrap();
        let mut agent = crate::db::engines::EngineRow::new("codex", "codex", "Codex");
        agent.found_path = Some("C:\\x\\codex.exe".into());
        db.insert_engine_if_absent(&agent).unwrap();
        add(&db, "d1", "handover-route", None, route_answer("none"), "The user's request:\nport the parser to rust please");
        db.add_engine_signal("codex", None, "asked", "port the parser to rust please").unwrap();
        let labels = labels_for(&db, &row(&db, "d1"));
        assert_eq!(labels.len(), 1);
        assert_eq!((labels[0].kind, labels[0].source.as_str()), (Kind::Wrong, "engine-asked"));
        // The same signal does not label a decision that chose a rule.
        add(&db, "d2", "handover-route", None, route_answer("codex:U1"), "The user's request:\nport the parser to rust please");
        assert!(labels_for(&db, &row(&db, "d2")).is_empty());
    }

    #[test]
    fn a_clipped_task_still_matches_and_a_stub_does_not() {
        assert!(same_request("refactor the billing module and…", "refactor the billing module and add tests"));
        assert!(!same_request("fix", "fix the thing in the other place"));
    }

    // ---- CTL-T-2: written new ----

    fn goal_answer(p: f32) -> serde_json::Value {
        serde_json::json!({ "met": { "p": p } })
    }

    #[test]
    fn a_goal_that_stalls_after_a_confident_not_yet_is_a_weak_right() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("t", None, false).unwrap().id;
        add(&db, "d1", "goal-met", Some(&conv), goal_answer(0.05), "Goal: x");
        assert_eq!(record_goal_event(&db, &conv, GoalEvent::Stalled), Some(Kind::WeakRight));
        assert_eq!(row(&db, "d1").outcomes[0].source, "goal-stalled");
        // The same event twice counts once.
        assert_eq!(record_goal_event(&db, &conv, GoalEvent::Stalled), None);
    }

    #[test]
    fn a_goal_that_stalls_after_an_unsure_round_is_not_a_label() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("t", None, false).unwrap().id;
        add(&db, "d1", "goal-met", Some(&conv), goal_answer(0.5), "Goal: x");
        assert_eq!(record_goal_event(&db, &conv, GoalEvent::Exhausted), None);
        assert!(row(&db, "d1").outcomes.is_empty());
    }

    #[test]
    fn a_new_goal_right_after_a_met_one_is_a_weak_wrong() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("t", None, false).unwrap().id;
        add(&db, "d1", "goal-met", Some(&conv), goal_answer(0.93), "Goal: x");
        assert_eq!(record_goal_event(&db, &conv, GoalEvent::NewGoal), Some(Kind::WeakWrong));
    }

    #[test]
    fn a_new_goal_long_after_is_just_a_new_goal() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("t", None, false).unwrap().id;
        add(&db, "d1", "goal-met", Some(&conv), goal_answer(0.93), "Goal: x");
        let later = now_ms() + NEW_GOAL_WINDOW_MS + 60_000;
        assert_eq!(record_goal_event_at(&db, &conv, GoalEvent::NewGoal, later), None);
        // And a new goal after a "not yet" says nothing about the check.
        add(&db, "d2", "goal-met", Some(&conv), goal_answer(0.1), "Goal: x");
        assert_eq!(record_goal_event(&db, &conv, GoalEvent::NewGoal), None);
    }

    #[test]
    fn with_no_decision_in_the_conversation_there_is_nothing_to_label() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("t", None, false).unwrap().id;
        assert_eq!(record_goal_event(&db, &conv, GoalEvent::Stalled), None);
    }

    // ---- observations for the fit ----

    #[test]
    fn a_right_label_says_the_winner_was_right_and_a_wrong_one_that_it_was_not() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "d1", "mem-scope", None, serde_json::json!({ "scope": { "choice": "global", "probs": [["global", 0.7], ["topical", 0.2], ["unsure", 0.1]] } }), "");
        let r = row(&db, "d1");
        let lab = |kind, value: Option<&str>| Label { kind, value: value.map(String::from), source: "t".into() };
        assert_eq!(observation(&r, &[lab(Kind::Right, None)]).unwrap().truth, Truth::Class(0));
        assert_eq!(observation(&r, &[lab(Kind::Wrong, None)]).unwrap().truth, Truth::NotClass(0));
        let corrected = observation(&r, &[lab(Kind::Corrected, Some("topical"))]).unwrap();
        assert_eq!(corrected.truth, Truth::Class(1));
        assert_eq!(corrected.weight, 1.0);
        let weak = observation(&r, &[lab(Kind::WeakRight, None)]).unwrap();
        assert_eq!(weak.weight, WEAK_WEIGHT);
        // A correction to something that was never an option still says "not the winner".
        assert_eq!(observation(&r, &[lab(Kind::Corrected, Some("nonsense"))]).unwrap().truth, Truth::NotClass(0));
        assert!(observation(&r, &[]).is_none());
    }

    #[test]
    fn a_firm_label_outranks_a_weak_one_on_the_same_decision() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "d1", "goal-met", None, goal_answer(0.9), "");
        let r = row(&db, "d1");
        let weak = Label { kind: Kind::WeakWrong, value: None, source: "a".into() };
        let firm = Label { kind: Kind::Right, value: None, source: "b".into() };
        let o = observation(&r, &[weak, firm]).unwrap();
        assert_eq!((o.truth, o.weight), (Truth::Class(0), 1.0));
    }

    #[test]
    fn observations_gather_stored_and_joined_labels_for_one_model() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "d1", "goal-met", None, goal_answer(0.9), "");
        db.add_outcome("d1", "right", None, "gold").unwrap();
        add(&db, "d2", "goal-met", None, goal_answer(0.9), "");
        assert_eq!(observations(&db, "goal-met", "Clef-Flash-Q4_K_M").len(), 1, "d2 has no label");
        assert!(observations(&db, "goal-met", "Another").is_empty());
    }

    // ---- ToolSelect: one label per held unit ----

    fn tool_answer() -> serde_json::Value {
        serde_json::json!({ "web_search": { "p": 0.03 }, "apps": { "p": 0.04 }, "mail": { "p": 0.9 } })
    }

    #[test]
    fn a_unit_that_was_needed_after_all_says_yes_and_one_never_missed_says_no() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "d1", "tool-select", None, tool_answer(), "The user's message:
hi");
        db.add_outcome("d1", "wrong", Some("web_search"), "need-tools:web_search").unwrap();
        db.add_outcome("d1", "weak_right", Some("apps"), "kept-out:apps").unwrap();
        let got = observations(&db, "tool-select", "Clef-Flash-Q4_K_M");
        assert_eq!(got.len(), 2, "one observation per labelled unit");
        let needed = got.iter().find(|o| o.truth == Truth::Class(0)).unwrap();
        assert_eq!((needed.probs[0], needed.weight), (0.03, 1.0), "a miss is a firm label");
        let not_needed = got.iter().find(|o| o.truth == Truth::Class(1)).unwrap();
        assert_eq!((not_needed.probs[0], not_needed.weight), (0.04, WEAK_WEIGHT), "no miss is a weak one");
    }

    #[test]
    fn a_tool_select_label_without_a_known_unit_is_not_an_observation() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "d1", "tool-select", None, tool_answer(), "");
        db.add_outcome("d1", "wrong", None, "x").unwrap();
        db.add_outcome("d1", "wrong", Some("nonsense"), "y").unwrap();
        assert!(observations(&db, "tool-select", "Clef-Flash-Q4_K_M").is_empty());
    }
}
