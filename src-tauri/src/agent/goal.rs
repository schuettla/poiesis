//! `GOL`: the check behind `/goal`.
//!
//! A goal is the user's: they set it, and only they can stop or change it. This
//! file holds the one thing the backend does for it, which is to judge whether
//! the goal now holds. It is one tool-free completion over the goal, what I last
//! said, and what I changed. It never starts a turn, and nothing here is offered
//! to the agent as a tool.
//!
//! The judge is deliberately hard to satisfy. A model that answers in prose, in
//! broken JSON, or says "met" with nothing to point at has not shown the goal
//! holds, so the answer is "not yet" and the loop goes on (and is stopped by its
//! round limit, not by a hopeful guess).

use serde::{Deserialize, Serialize};

use super::changes::ChangeSet;

/// How much of my last answer the judge reads. A goal is judged on how the work
/// ended, and the end of a long answer is where that is said.
const ANSWER_CHARS: usize = 4000;
/// How many changed files the judge is told about by name.
const FILES_NAMED: usize = 12;

/// What the check found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalCheck {
    pub met: bool,
    /// What shows it, in a line. Empty unless `met`.
    pub evidence: String,
    /// What is left to do, in a line. Empty when `met`.
    pub next: String,
    /// How likely the quick decision judged the goal to hold (`DEC-6`). Absent
    /// when the check came from the prose answer alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p: Option<f32>,
}

impl GoalCheck {
    /// The answer for anything the judge could not settle.
    fn not_yet(next: &str) -> Self {
        Self { met: false, evidence: String::new(), next: next.to_string(), p: None }
    }
}

/// How likely the decision model judged the goal to hold, and whether that
/// number is backed by a fitted calibration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    pub p: f32,
    pub calibrated: bool,
}

/// A goal counts as met by the decision only at the bar of its policy row
/// (`CTL-1`, 0.8). The judge is meant to be hard to satisfy, so the bar is high.
fn met_at(calibrated: bool) -> f32 {
    crate::agent::decide::Purpose::GoalMet.policy().act_at.at(calibrated)
}

/// The state the decision reads: the same facts the prose judge is given.
pub fn decision_state(objective: &str, until: &str, last_answer: &str, changes: &str) -> String {
    format!(
        "Goal: {objective}\nDone when: {until}\nFiles the assistant changed: {changes}\n\
         The assistant's last answer:\n{}",
        tail(last_answer.trim(), ANSWER_CHARS)
    )
}

/// Put the quick decision and the prose check together (`DEC-6`).
///
/// Until the decision is calibrated, "met" needs **both**: the decision at
/// the policy bar or above and the prose check agreeing. So the decision can only
/// make the check stricter (decision 3), never looser. Once calibrated the
/// decision decides alone, and the prose check only supplies the line.
pub fn combine(decision: Option<Decision>, prose: GoalCheck) -> GoalCheck {
    const WHAT_IS_LEFT: &str = "Keep going, and say plainly what you did and what it showed.";
    let Some(d) = decision else { return prose };
    let sure = d.p >= met_at(d.calibrated);
    let met = if d.calibrated { sure } else { sure && prose.met };
    if met {
        let evidence = if prose.evidence.trim().is_empty() {
            "The check judged the goal met from the last answer.".to_string()
        } else {
            prose.evidence
        };
        return GoalCheck { met: true, evidence, next: String::new(), p: Some(d.p) };
    }
    let next = if prose.next.trim().is_empty() { WHAT_IS_LEFT.to_string() } else { prose.next };
    GoalCheck { met: false, evidence: String::new(), next, p: Some(d.p) }
}

/// The changed files as one line the judge can read, or `none`.
pub fn changes_line(set: &ChangeSet) -> String {
    if set.files.is_empty() {
        return "none".to_string();
    }
    let mut names: Vec<&str> = set.files.iter().take(FILES_NAMED).map(|f| f.display.as_str()).collect();
    let more = set.files.len().saturating_sub(FILES_NAMED);
    if more > 0 {
        names.push("and more");
    }
    format!("{} file(s): {}", set.files.len(), names.join(", "))
}

/// The tail of `text`, so the end of a long answer is what survives the cut.
fn tail(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    format!("…{}", chars[chars.len() - max..].iter().collect::<String>())
}

/// The two messages the check sends.
pub fn check_messages(objective: &str, until: &str, last_answer: &str, changes: &str) -> Vec<serde_json::Value> {
    let prompt = format!(
        "The user gave an assistant this goal and a condition that says when it is done. \
         Decide, from what the assistant says below, whether the condition now holds.\n\
         Goal: {objective}\n\
         Done when: {until}\n\
         Files the assistant changed: {changes}\n\
         The assistant's last answer:\n{answer}\n\n\
         Answer met:true only if the answer itself shows the condition holds (for example it \
         reports a command's real result). A plan, a promise or \"I think so\" is met:false. \
         When met is true, evidence is one line quoting or naming what shows it. When met is \
         false, next is one line saying what is still left to do.\n\
         JSON schema: {{\"met\":true|false,\"evidence\":\"...\",\"next\":\"...\"}}",
        answer = tail(last_answer.trim(), ANSWER_CHARS),
    );
    vec![
        serde_json::json!({
            "role": "system",
            "content": "You check whether a stated goal has been reached. You are strict about evidence and fair about wording. Output ONLY JSON, no preamble.",
        }),
        serde_json::json!({ "role": "user", "content": prompt }),
    ]
}

/// Strip a ```json fence if the model wrapped its answer in one.
fn strip_fence(s: &str) -> &str {
    let s = s.trim();
    let Some(rest) = s.strip_prefix("```") else { return s };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.trim_start_matches('\n').trim_end_matches('`').trim()
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    met: bool,
    #[serde(default)]
    evidence: String,
    #[serde(default)]
    next: String,
}

/// Read the judge's answer. Anything but a well-formed `met:true` with something
/// to point at is "not yet".
pub fn parse_check(raw: &str) -> GoalCheck {
    let Ok(parsed) = serde_json::from_str::<Raw>(strip_fence(raw)) else {
        return GoalCheck::not_yet("Keep going, and say plainly what you did and what it showed.");
    };
    let evidence = parsed.evidence.trim();
    if parsed.met && !evidence.is_empty() {
        return GoalCheck { met: true, evidence: evidence.to_string(), next: String::new(), p: None };
    }
    let next = parsed.next.trim();
    GoalCheck::not_yet(if next.is_empty() {
        "Keep going, and say plainly what you did and what it showed."
    } else {
        next
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn met_needs_evidence() {
        let ok = parse_check(r#"{"met":true,"evidence":"npm test exited 0","next":""}"#);
        assert!(ok.met);
        assert_eq!(ok.evidence, "npm test exited 0");

        // "met" with nothing to point at is a hopeful guess.
        let bare = parse_check(r#"{"met":true,"evidence":"  ","next":""}"#);
        assert!(!bare.met);
    }

    #[test]
    fn not_met_says_what_is_left() {
        let c = parse_check(r#"{"met":false,"evidence":"","next":"2 tests still fail"}"#);
        assert_eq!(c, GoalCheck { met: false, evidence: String::new(), next: "2 tests still fail".into(), p: None });
    }

    fn met_prose() -> GoalCheck {
        parse_check(r#"{"met":true,"evidence":"npm test exited 0","next":""}"#)
    }

    /// `DEC-T-2`: an uncalibrated 0.9 with a prose check saying "not met" is
    /// "not yet": the decision cannot loosen the check.
    #[test]
    fn an_uncalibrated_yes_cannot_override_a_prose_no() {
        let prose = parse_check(r#"{"met":false,"evidence":"","next":"2 tests still fail"}"#);
        let c = combine(Some(Decision { p: 0.9, calibrated: false }), prose);
        assert!(!c.met);
        assert_eq!(c.next, "2 tests still fail");
        assert_eq!(c.p, Some(0.9));
    }

    /// The decision can make the check stricter: prose says met, the decision
    /// is not sure.
    #[test]
    fn a_doubtful_decision_turns_a_prose_yes_into_not_yet() {
        let c = combine(Some(Decision { p: 0.5, calibrated: false }), met_prose());
        assert!(!c.met);
        assert!(!c.next.is_empty(), "not yet always says what to do next");
        assert!(c.evidence.is_empty());
        assert_eq!(c.p, Some(0.5));
    }

    #[test]
    fn both_agreeing_is_met_and_keeps_the_evidence() {
        let c = combine(Some(Decision { p: 0.91, calibrated: false }), met_prose());
        assert!(c.met);
        assert_eq!(c.evidence, "npm test exited 0");
        assert_eq!(c.p, Some(0.91));
    }

    /// `DEC-T-2`: broken prose with a calibrated "met" is still "met".
    #[test]
    fn a_calibrated_decision_decides_alone_even_when_the_prose_is_broken() {
        let broken = parse_check("not json at all");
        let c = combine(Some(Decision { p: 0.95, calibrated: true }), broken);
        assert!(c.met);
        assert!(!c.evidence.is_empty(), "met always has a line to show");
        let c = combine(Some(Decision { p: 0.4, calibrated: true }), met_prose());
        assert!(!c.met, "a calibrated no beats a prose yes");
    }

    #[test]
    fn with_no_decision_the_prose_check_stands_untouched() {
        let c = combine(None, met_prose());
        assert_eq!(c, met_prose());
        assert_eq!(c.p, None);
    }

    /// `GoalCheck` keeps its fields; `p` is only on the wire when there is one.
    #[test]
    fn p_is_left_off_the_wire_when_absent() {
        let without = serde_json::to_value(met_prose()).unwrap();
        assert!(without.get("p").is_none());
        let with = serde_json::to_value(combine(Some(Decision { p: 0.9, calibrated: false }), met_prose())).unwrap();
        assert!(with.get("p").is_some());
    }

    #[test]
    fn the_decision_reads_the_same_facts_as_the_prose_judge() {
        let long = format!("{}THE END", "x".repeat(10_000));
        let state = decision_state("g", "u", &long, "2 file(s): a.rs, b.rs");
        assert!(state.contains("Goal: g") && state.contains("Done when: u"));
        assert!(state.contains("a.rs, b.rs"));
        assert!(state.contains("THE END"), "the end of a long answer is what is read");
    }

    #[test]
    fn a_fence_is_read_and_prose_is_not_met() {
        let fenced = parse_check("```json\n{\"met\":true,\"evidence\":\"done\"}\n```");
        assert!(fenced.met);
        // Prose that merely says the word is not a verdict.
        let prose = parse_check("Yes, it is met.");
        assert!(!prose.met);
        assert!(!prose.next.is_empty());
    }

    #[test]
    fn the_end_of_a_long_answer_is_what_the_judge_reads() {
        let long = format!("{}THE END", "x".repeat(10_000));
        let messages = check_messages("g", "u", &long, "none");
        let prompt = messages[1]["content"].as_str().unwrap();
        assert!(prompt.contains("THE END"));
        assert!(prompt.len() < 6000);
    }

    #[test]
    fn changes_are_named_and_capped() {
        let none = ChangeSet { files: vec![], added: 0, removed: 0, since: 0, this_run: false };
        assert_eq!(changes_line(&none), "none");
    }
}
