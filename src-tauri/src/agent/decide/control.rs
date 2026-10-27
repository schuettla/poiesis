//! `CTL-2`: what happens to a decision after it is made. Three jobs:
//!
//! - **Budgets.** A user turn may ask 20 decisions and escalate 3, and may not
//!   ask the same purpose twice about the same state. Background purposes share
//!   200 an hour. A limit that is hit takes the row's below-threshold default and
//!   says so in the log, so a loop of decisions cannot run away.
//! - **Thresholds.** [`evaluate`] says whether an answer reaches its row's bar,
//!   and a closed deterministic gate beats every bar (decision 13).
//! - **The record.** [`settle`] writes what was done with the decision.
//!
//! A delegated child asks with its lead's conversation id, so what it decides
//! counts against the lead's turn, not a fresh one.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

use super::policy::{Escalation, Purpose};
use super::{Answer, Verdict};
use crate::db::{now_ms, Db};

/// Decisions one user turn may ask.
pub const TURN_DECISIONS: u32 = 20;
/// Decisions one user turn may escalate.
pub const TURN_ESCALATIONS: u32 = 3;
/// Background decisions an hour, all purposes together.
pub const BACKGROUND_PER_HOUR: usize = 200;
/// The share of those that `UntrustedFlag` may use. Its intake is every connector
/// result, agent-app report and recall, so left alone it would crowd out the
/// reflection and memory purposes (`USE-8`).
pub const UNTRUSTED_PER_HOUR: usize = BACKGROUND_PER_HOUR / 2;
/// The share that `MailTriage` may use: one decision a message, and a first sync
/// brings up to 200 (`USE-6a`). Left alone it would use the whole hour.
pub const MAIL_PER_HOUR: usize = 60;
/// Items the `classify` tool may sort in one user turn. A sorted list is one
/// decision an item, asked for by the user, so it is exempt from the 20 a turn and
/// from the repeat rule (the tool asks about one distinct item once). Two full
/// lists of `agent::classify::MAX_ITEMS` fit, and a loop of calls does not.
pub const CLASSIFY_PER_TURN: u32 = 1_000;
/// Claims one check of an answer may judge (`USE-10`). Asked for by the user, so it
/// is exempt from the 20 a turn, as `classify` is.
pub const VERIFY_CLAIMS: u32 = 25;
/// Sentences the automatic check of one answer may ask about (`SEE-1`): the same 40
/// as `agent::sentences::MAX_SENTENCES`. Exempt from the 20 a turn, for the same reason.
pub const CHECK_SENTENCES: u32 = 40;
/// Claims one check of an answer may hand to a slow review (`USE-10`). The plan's 3
/// escalations a turn are for a chat turn; a check has its own four.
pub const VERIFY_REVIEWS: u32 = 4;
const HOUR_MS: i64 = 3_600_000;
/// Turns remembered at once. A finished turn is never told to us, so the oldest
/// is let go instead of growing without end.
const TURNS_KEPT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Decisions,
    Escalations,
    /// The same purpose about the same state, in the same turn.
    Repeat,
    Background,
    /// `UntrustedFlag` has used its half of the background budget.
    Share,
    /// `MailTriage` has used its share of the background budget.
    MailShare,
    /// `Classify` has sorted its items for this turn.
    Classify,
    /// A check of an answer has judged as many claims as it may.
    Claims,
    /// The automatic check of an answer has asked about as many sentences as it may.
    Sentences,
    /// A check of an answer has sent as many claims to review as it may.
    Reviews,
}

impl Limit {
    pub fn code(self) -> &'static str {
        match self {
            Limit::Decisions => "decisions",
            Limit::Escalations => "escalations",
            Limit::Repeat => "repeat",
            Limit::Background => "background",
            Limit::Share => "share",
            Limit::MailShare => "mail-share",
            Limit::Classify => "classify",
            Limit::Claims => "claims",
            Limit::Sentences => "sentences",
            Limit::Reviews => "reviews",
        }
    }
}

impl std::fmt::Display for Limit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Limit::Decisions => "this turn has used its decisions",
            Limit::Escalations => "this turn has used its escalations",
            Limit::Repeat => "the same question about the same state was already asked this turn",
            Limit::Background => "background decisions have used this hour's budget",
            Limit::Share => "the checks of outside text have used their half of this hour's budget",
            Limit::MailShare => "mail triage has used its share of this hour's budget",
            Limit::Classify => "this turn has sorted as many items as it may",
            Limit::Claims => "this check has judged as many claims as it may",
            Limit::Sentences => "this check has asked about as many sentences as it may",
            Limit::Reviews => "this check has sent as many claims to review as it may",
        })
    }
}

/// Whether a deterministic gate allows the thing the decision would do. The
/// connector gate, agent-app approvals, `Toolset::ready`, persona grants and
/// `/external off` all end up as one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Open,
    Closed,
}

/// The outcome of holding an answer against its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Sure enough: act on it.
    Go,
    /// Not sure enough: the row's below-threshold default, or its escalation.
    Below,
    /// A gate is closed, whatever the answer says.
    Blocked,
}

/// What was done, as the log keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Acted,
    /// Below the bar, and the row's default was taken.
    Default,
    Escalated(Escalation),
    Budget(Limit),
    Blocked,
}

impl Action {
    /// Stable text for `decisions.action`. The screen turns it into words.
    pub fn code(self) -> String {
        match self {
            Action::Acted => "acted".into(),
            Action::Default => "default".into(),
            Action::Escalated(e) => format!("escalated:{}", escalation_code(e)),
            Action::Budget(l) => format!("budget:{}", l.code()),
            Action::Blocked => "blocked".into(),
        }
    }
}

fn escalation_code(e: Escalation) -> &'static str {
    match e {
        Escalation::None => "none",
        Escalation::Evidence => "evidence",
        Escalation::Slow => "slow",
        Escalation::User => "user",
    }
}

/// `CTL-T-1`. A closed gate comes first: no answer opens what it closes. Then the
/// row's bar, on the number the row names, uncalibrated or calibrated.
pub fn evaluate(purpose: Purpose, answer: &Answer, calibrated: bool, gate: Gate) -> Act {
    if gate == Gate::Closed {
        return Act::Blocked;
    }
    if purpose.policy().act_at.reaches(answer, calibrated) {
        Act::Go
    } else {
        Act::Below
    }
}

#[derive(Default)]
struct Turn {
    decisions: u32,
    escalations: u32,
    /// Items sorted by the `classify` tool, which has its own count.
    classified: u32,
    seen: HashSet<(Purpose, String)>,
}

/// Claims judged and claims reviewed by one check of an answer (`USE-10`). Kept
/// apart from the chat turn, so a message sent during a check does not start its
/// counts again, and a check does not start the turn's.
#[derive(Default)]
struct Check {
    verified: u32,
    reviewed: u32,
    /// Sentences asked about by the automatic check (`SEE-1`).
    sentences: u32,
}

#[derive(Default)]
struct Inner {
    turns: HashMap<String, Turn>,
    order: VecDeque<String>,
    /// One per conversation: the check running there, or the last one.
    checks: HashMap<String, Check>,
    /// When each background decision was admitted, oldest first.
    background: VecDeque<i64>,
    /// The same, for the `UntrustedFlag` share only (each is also in `background`).
    untrusted: VecDeque<i64>,
    /// The same, for the `MailTriage` share only.
    mail: VecDeque<i64>,
}

/// The counters. One per app, reached through [`controller`].
#[derive(Default)]
pub struct Controller {
    inner: Mutex<Inner>,
}

impl Controller {
    /// A new user turn starts in this conversation: its counters begin again.
    pub fn begin_turn(&self, key: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.order.retain(|k| k != key);
        inner.order.push_back(key.to_string());
        inner.turns.insert(key.to_string(), Turn::default());
        while inner.order.len() > TURNS_KEPT {
            if let Some(old) = inner.order.pop_front() {
                inner.turns.remove(&old);
            }
        }
    }

    /// A check of an answer starts in this conversation: its counts begin again.
    pub fn begin_check(&self, key: &str) {
        let mut inner = self.inner.lock().unwrap();
        if inner.checks.len() >= TURNS_KEPT && !inner.checks.contains_key(key) {
            inner.checks.clear();
        }
        inner.checks.insert(key.to_string(), Check::default());
    }

    /// Count one decision about to be asked, or say which limit stops it.
    /// `key` is the turn it belongs to (the lead's conversation id); with none,
    /// only the background budget applies. `now` is epoch ms.
    pub fn admit(
        &self,
        key: Option<&str>,
        purpose: Purpose,
        state_hash: &str,
        background: bool,
        now: i64,
    ) -> Result<(), Limit> {
        let mut inner = self.inner.lock().unwrap();
        if background {
            while inner.background.front().is_some_and(|t| now - t >= HOUR_MS) {
                inner.background.pop_front();
            }
            while inner.untrusted.front().is_some_and(|t| now - t >= HOUR_MS) {
                inner.untrusted.pop_front();
            }
            while inner.mail.front().is_some_and(|t| now - t >= HOUR_MS) {
                inner.mail.pop_front();
            }
            let flag = purpose == Purpose::UntrustedFlag;
            let mail = purpose == Purpose::MailTriage;
            // A share is checked first, so a refused check leaves the shared
            // budget alone for the purposes behind it.
            if flag && inner.untrusted.len() >= UNTRUSTED_PER_HOUR {
                return Err(Limit::Share);
            }
            if mail && inner.mail.len() >= MAIL_PER_HOUR {
                return Err(Limit::MailShare);
            }
            if inner.background.len() >= BACKGROUND_PER_HOUR {
                return Err(Limit::Background);
            }
            inner.background.push_back(now);
            if flag {
                inner.untrusted.push_back(now);
            }
            if mail {
                inner.mail.push_back(now);
            }
            return Ok(());
        }
        let Some(key) = key else { return Ok(()) };
        if purpose == Purpose::ClaimCheckable {
            let check = inner.checks.entry(key.to_string()).or_default();
            if check.sentences >= CHECK_SENTENCES {
                return Err(Limit::Sentences);
            }
            check.sentences += 1;
            return Ok(());
        }
        if purpose == Purpose::ClaimVerdict {
            let check = inner.checks.entry(key.to_string()).or_default();
            if check.verified >= VERIFY_CLAIMS {
                return Err(Limit::Claims);
            }
            check.verified += 1;
            return Ok(());
        }
        if !inner.turns.contains_key(key) {
            inner.order.push_back(key.to_string());
            while inner.order.len() > TURNS_KEPT {
                if let Some(old) = inner.order.pop_front() {
                    inner.turns.remove(&old);
                }
            }
        }
        let turn = inner.turns.entry(key.to_string()).or_default();
        if purpose == Purpose::Classify {
            if turn.classified >= CLASSIFY_PER_TURN {
                return Err(Limit::Classify);
            }
            turn.classified += 1;
            return Ok(());
        }
        if turn.decisions >= TURN_DECISIONS {
            return Err(Limit::Decisions);
        }
        // The same purpose about the same state gets the same answer, so asking
        // again is a loop, not new information.
        if !turn.seen.insert((purpose, state_hash.to_string())) {
            return Err(Limit::Repeat);
        }
        turn.decisions += 1;
        Ok(())
    }

    /// Count one escalation, or say that the turn has used them.
    pub fn escalate(&self, key: Option<&str>) -> Result<(), Limit> {
        let Some(key) = key else { return Ok(()) };
        let mut inner = self.inner.lock().unwrap();
        let turn = inner.turns.entry(key.to_string()).or_default();
        if turn.escalations >= TURN_ESCALATIONS {
            return Err(Limit::Escalations);
        }
        turn.escalations += 1;
        Ok(())
    }

    /// Count one claim sent to a slow review, or say that the check has sent its
    /// four (`USE-10`).
    pub fn review(&self, key: Option<&str>) -> Result<(), Limit> {
        let Some(key) = key else { return Ok(()) };
        let mut inner = self.inner.lock().unwrap();
        let check = inner.checks.entry(key.to_string()).or_default();
        if check.reviewed >= VERIFY_REVIEWS {
            return Err(Limit::Reviews);
        }
        check.reviewed += 1;
        Ok(())
    }
}

static CONTROLLER: OnceLock<Controller> = OnceLock::new();

pub fn controller() -> &'static Controller {
    CONTROLLER.get_or_init(Controller::default)
}

/// What to do about a decision that was held against its row: the escalation
/// the row names, if the turn can still afford one, else the default.
pub fn after(purpose: Purpose, act: Act, key: Option<&str>) -> Action {
    after_with(controller(), purpose, act, key)
}

fn after_with(controller: &Controller, purpose: Purpose, act: Act, key: Option<&str>) -> Action {
    match act {
        Act::Go => Action::Acted,
        Act::Blocked => Action::Blocked,
        Act::Below if purpose == Purpose::ClaimVerdict => match controller.review(key) {
            Ok(()) => Action::Escalated(Escalation::Slow),
            Err(limit) => Action::Budget(limit),
        },
        Act::Below => match purpose.policy().escalate {
            Escalation::None => Action::Default,
            e => match controller.escalate(key) {
                Ok(()) => Action::Escalated(e),
                Err(limit) => Action::Budget(limit),
            },
        },
    }
}

/// Record what was done with a decision, and say it. Best effort: a missing row
/// (no database, a decision that was never logged) is not a problem for the
/// site that asked.
pub fn settle(db: &Db, verdict: &Verdict, purpose: Purpose, act: Act, key: Option<&str>) -> Action {
    let action = after(purpose, act, key);
    let _ = db.set_decision_action(&verdict.id, &action.code());
    action
}

/// Now, for the callers that have no clock of their own.
pub fn now() -> i64 {
    now_ms()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noul(p: f32) -> Answer {
        Answer::Noul { p_true: p }
    }

    // ---- CTL-T-1: policy ----

    /// A closed gate beats the bar, however sure the answer is.
    #[test]
    fn a_closed_gate_beats_every_threshold() {
        for purpose in super::super::policy::ALL {
            let sure = Answer::Noul { p_true: 1.0 };
            assert_eq!(evaluate(purpose, &sure, true, Gate::Closed), Act::Blocked, "{}", purpose.name());
            assert_eq!(evaluate(purpose, &sure, false, Gate::Closed), Act::Blocked, "{}", purpose.name());
        }
    }

    #[test]
    fn an_open_gate_leaves_the_decision_to_the_bar() {
        assert_eq!(evaluate(Purpose::GoalMet, &noul(0.9), false, Gate::Open), Act::Go);
        assert_eq!(evaluate(Purpose::GoalMet, &noul(0.7), false, Gate::Open), Act::Below);
    }

    // ---- CTL-2: budgets ----

    #[test]
    fn a_turn_may_ask_twenty_decisions_and_no_more() {
        let c = Controller::default();
        for i in 0..TURN_DECISIONS {
            assert!(c.admit(Some("conv"), Purpose::GoalMet, &format!("s{i}"), false, 0).is_ok(), "{i}");
        }
        assert_eq!(c.admit(Some("conv"), Purpose::GoalMet, "one more", false, 0), Err(Limit::Decisions));
        // Another conversation has its own turn.
        assert!(c.admit(Some("other"), Purpose::GoalMet, "s", false, 0).is_ok());
    }

    /// An unchanged state hash blocks a re-ask; a changed one does not.
    #[test]
    fn the_same_question_about_the_same_state_is_not_asked_twice() {
        let c = Controller::default();
        assert!(c.admit(Some("conv"), Purpose::RetSufficiency, "h1", false, 0).is_ok());
        assert_eq!(c.admit(Some("conv"), Purpose::RetSufficiency, "h1", false, 0), Err(Limit::Repeat));
        assert!(c.admit(Some("conv"), Purpose::RetSufficiency, "h2", false, 0).is_ok(), "new evidence");
        assert!(c.admit(Some("conv"), Purpose::GoalMet, "h1", false, 0).is_ok(), "another purpose");
    }

    /// A new user turn starts the counters again.
    #[test]
    fn a_new_turn_starts_the_counters_again() {
        let c = Controller::default();
        assert!(c.admit(Some("conv"), Purpose::GoalMet, "h", false, 0).is_ok());
        assert_eq!(c.admit(Some("conv"), Purpose::GoalMet, "h", false, 0), Err(Limit::Repeat));
        c.begin_turn("conv");
        assert!(c.admit(Some("conv"), Purpose::GoalMet, "h", false, 0).is_ok());
    }

    #[test]
    fn a_turn_may_escalate_three_times() {
        let c = Controller::default();
        for _ in 0..TURN_ESCALATIONS {
            assert!(c.escalate(Some("conv")).is_ok());
        }
        assert_eq!(c.escalate(Some("conv")), Err(Limit::Escalations));
        assert!(c.escalate(None).is_ok(), "no turn, no turn budget");
    }

    /// `CTL-T-2`: a delegated child asks with its lead's key, so its decisions
    /// come out of the lead's turn.
    #[test]
    fn a_childs_decisions_count_against_its_leads_turn() {
        let c = Controller::default();
        for i in 0..TURN_DECISIONS - 1 {
            c.admit(Some("lead"), Purpose::GoalMet, &format!("lead{i}"), false, 0).unwrap();
        }
        assert!(c.admit(Some("lead"), Purpose::GoalMet, "child", false, 0).is_ok());
        assert_eq!(c.admit(Some("lead"), Purpose::GoalMet, "child 2", false, 0), Err(Limit::Decisions));
    }

    #[test]
    fn background_decisions_share_two_hundred_an_hour() {
        let c = Controller::default();
        for i in 0..BACKGROUND_PER_HOUR as i64 {
            assert!(c.admit(None, Purpose::MemScope, "h", true, i).is_ok(), "{i}");
        }
        assert_eq!(c.admit(None, Purpose::ReflectCritic, "h", true, 1_000), Err(Limit::Background));
        // An hour after the first, room opens up again.
        assert!(c.admit(None, Purpose::MemScope, "h", true, HOUR_MS + 1).is_ok());
    }

    /// `USE-8`: the checks of outside text get half of the hour, so a burst of
    /// connector results cannot use up what reflection and memory need.
    #[test]
    fn the_untrusted_flag_may_use_half_of_the_background_hour() {
        let c = Controller::default();
        for i in 0..UNTRUSTED_PER_HOUR as i64 {
            assert!(c.admit(None, Purpose::UntrustedFlag, "h", true, i).is_ok(), "{i}");
        }
        assert_eq!(c.admit(None, Purpose::UntrustedFlag, "h", true, 1_000), Err(Limit::Share));
        // The other half is still there for everyone else, and a refused flag
        // check did not take any of it.
        for i in 0..(BACKGROUND_PER_HOUR - UNTRUSTED_PER_HOUR) as i64 {
            assert!(c.admit(None, Purpose::MemScope, "h", true, 2_000 + i).is_ok(), "{i}");
        }
        assert_eq!(c.admit(None, Purpose::ReflectGate, "h", true, 9_000), Err(Limit::Background));
        // An hour on, both open again.
        assert!(c.admit(None, Purpose::UntrustedFlag, "h", true, HOUR_MS + 1).is_ok());
    }

    /// `USE-6`: mail triage gets its own share, and a refused check leaves the rest
    /// of the hour to reflection, memory and the flag.
    #[test]
    fn mail_triage_may_use_its_share_of_the_background_hour_and_no_more() {
        let c = Controller::default();
        for i in 0..MAIL_PER_HOUR as i64 {
            assert!(c.admit(None, Purpose::MailTriage, "h", true, i).is_ok(), "{i}");
        }
        assert_eq!(c.admit(None, Purpose::MailTriage, "h", true, 1_000), Err(Limit::MailShare));
        // What is left of the hour is still there for everyone else.
        for i in 0..(BACKGROUND_PER_HOUR - MAIL_PER_HOUR) as i64 {
            assert!(c.admit(None, Purpose::MemScope, "h", true, 2_000 + i).is_ok(), "{i}");
        }
        assert_eq!(c.admit(None, Purpose::MemScope, "h", true, 9_000), Err(Limit::Background));
        // An hour on, mail is let in again.
        assert!(c.admit(None, Purpose::MailTriage, "h", true, HOUR_MS + 1).is_ok());
    }

    /// The other purposes can use the whole hour when no flag check has.
    #[test]
    fn the_other_background_purposes_are_not_held_to_the_flags_share() {
        let c = Controller::default();
        for i in 0..BACKGROUND_PER_HOUR as i64 {
            assert!(c.admit(None, Purpose::ReflectGate, "h", true, i).is_ok(), "{i}");
        }
        assert_eq!(c.admit(None, Purpose::UntrustedFlag, "h", true, 1_000), Err(Limit::Background));
    }

    /// `USE-9`: a sorted list is not twenty decisions. It has its own count, asks
    /// the same state as often as the tool says, and leaves the turn's twenty alone.
    #[test]
    fn sorting_a_list_has_its_own_count_and_leaves_the_turns_decisions_alone() {
        let c = Controller::default();
        for i in 0..CLASSIFY_PER_TURN {
            assert!(c.admit(Some("conv"), Purpose::Classify, "same", false, 0).is_ok(), "{i}");
        }
        assert_eq!(c.admit(Some("conv"), Purpose::Classify, "same", false, 0), Err(Limit::Classify));
        assert_eq!(Limit::Classify.code(), "classify");
        // The turn's own twenty were never touched, and another turn has its own count.
        for i in 0..TURN_DECISIONS {
            assert!(c.admit(Some("conv"), Purpose::GoalMet, &format!("s{i}"), false, 0).is_ok(), "{i}");
        }
        assert!(c.admit(Some("other"), Purpose::Classify, "same", false, 0).is_ok());
        c.begin_turn("conv");
        assert!(c.admit(Some("conv"), Purpose::Classify, "same", false, 0).is_ok(), "a new turn starts again");
    }

    /// `USE-10`: a check of an answer judges up to 25 claims and sends up to 4 to a
    /// slow review. Neither count is the turn's 20 decisions and 3 escalations, and
    /// the fifth review is refused by the controller, so the log says so.
    #[test]
    fn a_check_of_an_answer_has_its_own_claim_and_review_counts() {
        let c = Controller::default();
        for i in 0..VERIFY_CLAIMS {
            assert!(c.admit(Some("conv"), Purpose::ClaimVerdict, &format!("s{i}"), false, 0).is_ok(), "{i}");
        }
        assert_eq!(c.admit(Some("conv"), Purpose::ClaimVerdict, "last", false, 0), Err(Limit::Claims));
        assert_eq!(Limit::Claims.code(), "claims");
        // The turn's twenty are untouched by the 25 claims.
        for i in 0..TURN_DECISIONS {
            assert!(c.admit(Some("conv"), Purpose::GoalMet, &format!("g{i}"), false, 0).is_ok(), "{i}");
        }
        for _ in 0..VERIFY_REVIEWS {
            assert_eq!(
                after_with(&c, Purpose::ClaimVerdict, Act::Below, Some("conv")),
                Action::Escalated(Escalation::Slow)
            );
        }
        let fifth = after_with(&c, Purpose::ClaimVerdict, Act::Below, Some("conv"));
        assert_eq!(fifth, Action::Budget(Limit::Reviews));
        assert_eq!(fifth.code(), "budget:reviews");
        // A claim that is settled never uses a review, and the turn's own three escalations are separate.
        assert_eq!(after_with(&c, Purpose::ClaimVerdict, Act::Go, Some("conv")), Action::Acted);
        assert_eq!(
            after_with(&c, Purpose::HandoverRoute, Act::Below, Some("conv")),
            Action::Escalated(Escalation::Slow)
        );
        // A message sent while the check runs starts the chat turn again, not the check.
        c.begin_turn("conv");
        assert_eq!(c.admit(Some("conv"), Purpose::ClaimVerdict, "s0", false, 0), Err(Limit::Claims));
        assert_eq!(after_with(&c, Purpose::ClaimVerdict, Act::Below, Some("conv")), Action::Budget(Limit::Reviews));
        // The next check begins again.
        c.begin_check("conv");
        assert!(c.admit(Some("conv"), Purpose::ClaimVerdict, "s0", false, 0).is_ok());
        assert_eq!(after_with(&c, Purpose::ClaimVerdict, Act::Below, Some("conv")), Action::Escalated(Escalation::Slow));
    }

    /// And a check starting does not give the chat turn its decisions back.
    #[test]
    fn a_check_starting_leaves_the_turns_counts_alone() {
        let c = Controller::default();
        for i in 0..TURN_DECISIONS {
            c.admit(Some("conv"), Purpose::GoalMet, &format!("s{i}"), false, 0).unwrap();
        }
        c.begin_check("conv");
        assert_eq!(c.admit(Some("conv"), Purpose::GoalMet, "one more", false, 0), Err(Limit::Decisions));
    }

    /// `SEE-1`: the automatic check asks about up to 40 sentences an answer, outside the
    /// turn's twenty, and the same sentence twice is not a loop there.
    #[test]
    fn the_automatic_check_may_ask_about_forty_sentences_outside_the_turns_twenty() {
        let c = Controller::default();
        for _ in 0..CHECK_SENTENCES {
            assert!(c.admit(Some("conv"), Purpose::ClaimCheckable, "same sentence", false, 0).is_ok());
        }
        assert_eq!(c.admit(Some("conv"), Purpose::ClaimCheckable, "one more", false, 0), Err(Limit::Sentences));
        assert_eq!(Limit::Sentences.code(), "sentences");
        assert!(c.admit(Some("conv"), Purpose::GoalMet, "g", false, 0).is_ok(), "the turn's decisions are untouched");
        // The next answer's check begins again.
        c.begin_check("conv");
        assert!(c.admit(Some("conv"), Purpose::ClaimCheckable, "s", false, 0).is_ok());
        assert_eq!(CHECK_SENTENCES as usize, crate::agent::sentences::MAX_SENTENCES);
    }

    /// Background decisions are not turn decisions: they never touch a turn's count.
    #[test]
    fn a_background_decision_does_not_use_a_turns_budget() {
        let c = Controller::default();
        for _ in 0..TURN_DECISIONS + 5 {
            assert!(c.admit(Some("conv"), Purpose::MemScope, "h", true, 0).is_ok());
        }
        assert!(c.admit(Some("conv"), Purpose::GoalMet, "h", false, 0).is_ok());
    }

    #[test]
    fn only_the_newest_turns_are_remembered() {
        let c = Controller::default();
        for i in 0..TURNS_KEPT + 10 {
            c.begin_turn(&format!("c{i}"));
        }
        assert!(c.inner.lock().unwrap().turns.len() <= TURNS_KEPT);
    }

    // ---- what is done with a decision that is below its bar ----

    #[test]
    fn below_the_bar_takes_the_default_or_the_rows_escalation() {
        let c = Controller::default();
        assert_eq!(after_with(&c, Purpose::GoalMet, Act::Go, None), Action::Acted);
        assert_eq!(after_with(&c, Purpose::MemScope, Act::Below, None), Action::Default, "store unclassified");
        assert_eq!(
            after_with(&c, Purpose::HandoverRoute, Act::Below, Some("k")),
            Action::Escalated(Escalation::Slow)
        );
        assert_eq!(after_with(&c, Purpose::ReflectCritic, Act::Blocked, None), Action::Blocked);
    }

    /// `CTL-T-1`: the budget hit falls back to the default and says why.
    #[test]
    fn once_the_escalations_are_used_the_default_is_taken_and_logged_as_budget() {
        let c = Controller::default();
        for _ in 0..TURN_ESCALATIONS {
            assert_eq!(
                after_with(&c, Purpose::HandoverRoute, Act::Below, Some("k")),
                Action::Escalated(Escalation::Slow)
            );
        }
        let hit = after_with(&c, Purpose::HandoverRoute, Act::Below, Some("k"));
        assert_eq!(hit, Action::Budget(Limit::Escalations));
        assert_eq!(hit.code(), "budget:escalations");
    }

    #[test]
    fn actions_have_stable_codes() {
        assert_eq!(Action::Acted.code(), "acted");
        assert_eq!(Action::Default.code(), "default");
        assert_eq!(Action::Escalated(Escalation::Evidence).code(), "escalated:evidence");
        assert_eq!(Action::Budget(Limit::Repeat).code(), "budget:repeat");
        assert_eq!(Action::Budget(Limit::Share).code(), "budget:share");
        assert_eq!(Action::Budget(Limit::MailShare).code(), "budget:mail-share");
        assert_eq!(Action::Blocked.code(), "blocked");
    }

    #[test]
    fn settle_writes_the_action_on_the_logged_decision() {
        use crate::agent::decide::{BackendId, Verdict};
        let db = Db::open_in_memory().unwrap();
        let answers = serde_json::json!({ "scope": { "confidence": 0.2 } });
        db.add_decision(&crate::db::decisions::NewDecision {
            id: "d1",
            conversation_id: None,
            run_id: None,
            purpose: "mem-scope",
            backend: "local:x",
            model: "x",
            ms: 1,
            calibrated: false,
            answers: &answers,
            input_tokens: 1,
            state_preview: "",
            state_hash: "h",
            action: None,
        })
        .unwrap();
        let verdict = Verdict {
            id: "d1".into(),
            answers: Default::default(),
            backend: BackendId::Local { model: "x".into() },
            calibrated: false,
            ms: 1,
        };
        let action = settle(&db, &verdict, Purpose::MemScope, Act::Below, None);
        assert_eq!(action, Action::Default);
        assert_eq!(db.decisions_recent(1, None).unwrap()[0].action.as_deref(), Some("default"));
    }
}
