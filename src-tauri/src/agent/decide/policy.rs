//! `CTL-1`: the policy table. One row per purpose, in code, so it can be
//! reviewed and diffed, and the UI shows it read-only (`DEC-UI-2`).
//!
//! Only the purposes that have a site are here: the five of `DEC-6`,
//! `ToolSelect` (`USE-4`), the two background filters of `USE-8`
//! (`ReflectGate`, `UntrustedFlag`), the `classify` tool's `Classify`
//! (`USE-9`), the four of `USE-5` and `USE-7` (`PlanDone`, `SteerClass`,
//! `UnverifiedClaim`, `RunIf`), the `MailTriage` of `USE-6`, the `ClaimVerdict` of
//! `USE-10` and the `ClaimCheckable` of `SEE-1`, the `BrowserCheck` of `USE-11` and the two gate purposes of `USE-12`
//! (`ToolRisk`, `AppKind`). The plan's other
//! rows arrive with the step that builds their site, because a row without a site
//! would be a threshold nothing reads. Their values are in the plan's table.
//!
//! The thresholds are starting values (`CTL-4` tunes them). **A threshold never
//! opens what a deterministic gate closes** (decision 13): see
//! [`super::control::act`], which takes the gate's answer first.

use std::time::Duration;

use super::Answer;

/// Why a decision is being asked. One value per row of the policy table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// `RET-4`: do the closest excerpts answer the question?
    RetSufficiency,
    /// `SCP-1`: does a saved fact apply always, or only on its subject?
    MemScope,
    /// `CRT-1`: does a drafted lesson hold up against its conversation?
    ReflectCritic,
    /// `GOL-2`: does the goal now hold?
    GoalMet,
    /// `RTE-7`: which agent app, if any, would the rules hand this to?
    HandoverRoute,
    /// `USE-4`: does this message need a tool unit (the web, mail, connected
    /// apps, other agents)? Asked once a turn, one question a unit.
    ToolSelect,
    /// `USE-8`: did anything go wrong in this finished conversation that a
    /// reflection pass could learn from?
    ReflectGate,
    /// `USE-8`: does a piece of outside text try to instruct an AI assistant?
    UntrustedFlag,
    /// `USE-9`: which of the answers the assistant listed fits one item? Asked
    /// once an item by the `classify` tool, for as many items as it was given.
    Classify,
    /// `USE-5`: does the answer a run just wrote finish one plan item that is
    /// still open? Asked once an open item, when the run is about to end.
    PlanDone,
    /// `USE-5`: is a message typed at a running agent a stop, a redirect, an
    /// addition or a question?
    SteerClass,
    /// `USE-5`: does the answer claim the change works or was tested? Asked only
    /// where files changed and no check ran.
    UnverifiedClaim,
    /// `USE-7`: does a scheduled task's own condition hold, so it should run now?
    RunIf,
    /// `USE-6`: what kind of mail is this, how urgent, and does it want a reply?
    /// Asked once a new message of the small inbox, in the background.
    MailTriage,
    /// `USE-10`: does the evidence found for one claim of an answer support it,
    /// refute it, fall short or disagree? Asked once a claim, when the user asks
    /// for an answer to be checked.
    ClaimVerdict,
    /// `SEE-1`: could this sentence of an answer be checked, and is it a fact or
    /// something the assistant says it did? Asked once a sentence, for every
    /// answer, while it streams.
    ClaimCheckable,
    /// `USE-11`: what does the page the browser agent just reached show: ordinary
    /// content, a login, a captcha or an error? Asked once an action, with a
    /// picture of the page.
    BrowserCheck,
    /// `USE-12`: how bad would one tool call be if it were a mistake, and is it what
    /// the user asked for? Asked before an own tool runs (both questions) and
    /// before an app call or an agent app's approval is shown (the second only).
    /// It can only add caution.
    ToolRisk,
    /// `USE-12`: does a connector tool whose name reads as a read or a change really
    /// send something? Asked once a tool a day. It can only make a gate stricter.
    AppKind,
}

pub const ALL: [Purpose; 19] = [
    Purpose::RetSufficiency,
    Purpose::MemScope,
    Purpose::ReflectCritic,
    Purpose::GoalMet,
    Purpose::HandoverRoute,
    Purpose::ToolSelect,
    Purpose::ReflectGate,
    Purpose::UntrustedFlag,
    Purpose::Classify,
    Purpose::PlanDone,
    Purpose::SteerClass,
    Purpose::UnverifiedClaim,
    Purpose::RunIf,
    Purpose::MailTriage,
    Purpose::ClaimVerdict,
    Purpose::ClaimCheckable,
    Purpose::BrowserCheck,
    Purpose::ToolRisk,
    Purpose::AppKind,
];

/// How much a decision is trusted to do on its own (DPAR section 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// The fast answer is the action.
    Reflex,
    /// The fast answer informs a path that something slower can still change.
    Assisted,
    /// The fast answer is checked against evidence before it counts.
    Verify,
    /// The fast answer may only make a deterministic gate stricter.
    Gate,
}

/// What a wrong decision costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Impact {
    Cheap,
    Visible,
    Costly,
    Irreversible,
}

/// What happens when a decision does not reach its threshold (`CTL-2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Escalation {
    /// Take the row's default.
    None,
    /// Fetch something new, then ask once more.
    Evidence,
    /// Hand it to the slower language-model pass.
    Slow,
    /// Show it to the user.
    User,
}

/// Whether a purpose needs a backend that reads images (`DEC-8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NeedsVision {
    No,
    /// An experimental backend will do.
    Any,
    Yes,
}

/// Which number of an answer a threshold is compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    /// The probability that a yes-or-no statement is true.
    PTrue,
    /// The probability of the winning option.
    TopProb,
    /// The probability that a yes-or-no statement is false. For a purpose whose
    /// action is to take something away when the answer is a confident no.
    PFalse,
    /// One minus the normalised entropy (`super::confidence`). Strict: for three
    /// options 0.6 needs a winner near 0.93.
    Confidence,
    /// The probability of one named option of a pick-one question, whether or
    /// not it is the winner. For a purpose that acts on one answer only
    /// (`SteerClass` acts on `stop`).
    Option(&'static str),
    /// The probability of one level of a rating, counted from 0 (the lowest). For a
    /// purpose that acts on the top of a scale only (`ToolRisk` acts on
    /// "irreversible").
    Level(usize),
}

/// The bar a decision has to reach before it is acted on. Two values, because a
/// probability nobody has checked is worth less than one that was fitted
/// (`CTL-4`): the uncalibrated bar is never lower than the calibrated one, so
/// having no calibration can only make a decision harder to act on.
#[derive(Debug, Clone, Copy)]
pub struct Threshold {
    pub measure: Measure,
    pub calibrated: f32,
    pub uncalibrated: f32,
}

impl Threshold {
    pub fn at(&self, calibrated: bool) -> f32 {
        if calibrated {
            self.calibrated
        } else {
            self.uncalibrated
        }
    }

    /// Whether the answer is sure enough on this row's measure.
    pub fn reaches(&self, answer: &Answer, calibrated: bool) -> bool {
        let value = match (self.measure, answer) {
            (Measure::PTrue, Answer::Noul { p_true }) => *p_true,
            (Measure::PFalse, Answer::Noul { p_true }) => 1.0 - *p_true,
            (Measure::TopProb, Answer::Choice { probs, .. }) => {
                probs.iter().map(|(_, p)| *p).fold(0.0, f32::max)
            }
            (Measure::TopProb, Answer::Score { probs, .. }) => probs.iter().copied().fold(0.0, f32::max),
            (Measure::TopProb, Answer::Noul { p_true }) => p_true.max(1.0 - p_true),
            (Measure::Option(id), Answer::Choice { probs, .. }) => {
                probs.iter().find(|(k, _)| k == id).map_or(0.0, |(_, p)| *p)
            }
            (Measure::Option(_), _) => 0.0,
            (Measure::Level(i), Answer::Score { probs, .. }) => probs.get(i).copied().unwrap_or(0.0),
            (Measure::Level(_), _) => 0.0,
            (_, a) => a.confidence(),
        };
        value >= self.at(calibrated)
    }
}

/// One row of the table.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub purpose: Purpose,
    pub mode: Mode,
    pub impact: Impact,
    pub act_at: Threshold,
    pub escalate: Escalation,
    /// Runs without the user asking for it in that moment, so it stays on this
    /// computer even when the global backend is the cloud (decision 12).
    pub background: bool,
    /// On the hot path: a person or a streaming turn is waiting. Needs a warm
    /// engine on the graphics card, and is skipped rather than waited for
    /// (decisions 5 and 6).
    pub needs_fast: bool,
    pub needs_vision: NeedsVision,
    /// How long one request may take once the engine is up.
    pub deadline: Duration,
    /// The most state, in tokens, this purpose sends. The seam also applies
    /// the model's own limit and takes the smaller.
    pub state_cap_tokens: u32,
    /// The experiment that has to be on for this purpose to run (`EXP-2`).
    pub experiment: Option<&'static str>,
    /// Asked while the person is still typing (`SEE-3`). It follows the backend choice like
    /// any purpose the person is there for; what it answers is held, not logged, until the
    /// hint is acted on or the message is sent (`typing::hold`).
    pub typing: bool,
}

/// What a request may take over a network, at the least: a hot purpose's local
/// deadline (300 ms) is for an engine on this computer.
const CLOUD_MIN_DEADLINE: Duration = Duration::from_millis(800);

impl Policy {
    /// How long a request to the cloud may take: the row's own deadline, but never
    /// under 800 ms (`USE-4`, `USE-13`). A miss is the same as a local one.
    pub fn cloud_deadline(&self) -> Duration {
        self.deadline.max(CLOUD_MIN_DEADLINE)
    }
}

impl Purpose {
    /// The stable name used in settings keys and in the log.
    pub fn name(self) -> &'static str {
        match self {
            Purpose::RetSufficiency => "ret-sufficiency",
            Purpose::MemScope => "mem-scope",
            Purpose::ReflectCritic => "reflect-critic",
            Purpose::GoalMet => "goal-met",
            Purpose::HandoverRoute => "handover-route",
            Purpose::ToolSelect => "tool-select",
            Purpose::ReflectGate => "reflect-gate",
            Purpose::UntrustedFlag => "untrusted-flag",
            Purpose::Classify => "classify",
            Purpose::PlanDone => "plan-done",
            Purpose::SteerClass => "steer-class",
            Purpose::UnverifiedClaim => "unverified-claim",
            Purpose::RunIf => "run-if",
            Purpose::MailTriage => "mail-triage",
            Purpose::ClaimVerdict => "claim-verdict",
            Purpose::ClaimCheckable => "claim-checkable",
            Purpose::BrowserCheck => "browser-check",
            Purpose::ToolRisk => "tool-risk",
            Purpose::AppKind => "app-kind",
        }
    }

    /// The words the screen uses for this purpose.
    pub fn label(self) -> &'static str {
        match self {
            Purpose::RetSufficiency => "Do the found passages answer the question?",
            Purpose::MemScope => "Does a saved note apply always, or only on its subject?",
            Purpose::ReflectCritic => "Does a drafted lesson hold up?",
            Purpose::GoalMet => "Is the goal reached?",
            Purpose::HandoverRoute => "Would this be handed to another app?",
            Purpose::ToolSelect => "Does this message need a group of tools?",
            Purpose::ReflectGate => "Is there anything to learn from this chat?",
            Purpose::UntrustedFlag => "Does this outside text try to give me orders?",
            Purpose::Classify => "Which answer fits each item in a list?",
            Purpose::PlanDone => "Does my answer finish a step of my plan?",
            Purpose::SteerClass => "Is a message sent mid-run a stop, a redirect, an addition or a question?",
            Purpose::UnverifiedClaim => "Does my answer claim the change works?",
            Purpose::RunIf => "Does a task's own condition hold right now?",
            Purpose::MailTriage => "What kind of mail is this, how urgent is it, and does it need a reply?",
            Purpose::ClaimVerdict => "Does the evidence support this claim of my answer?",
            Purpose::ClaimCheckable => "Is this sentence something that could be checked?",
            Purpose::BrowserCheck => "Is the page I just opened a login, a captcha or an error?",
            Purpose::ToolRisk => "Is this what you asked for, and could it not be undone?",
            Purpose::AppKind => "Does this app tool send something, whatever its name says?",
        }
    }

    /// What happens below the bar, in the words the screen uses.
    pub fn below(self) -> &'static str {
        match self {
            Purpose::RetSufficiency => "Counts as not answering.",
            Purpose::MemScope => "Stores nothing; the backfill asks again later.",
            Purpose::ReflectCritic => "Asks the language model to review it.",
            Purpose::GoalMet => "Counts as not yet.",
            Purpose::HandoverRoute => "Asks the language model, as before.",
            Purpose::ToolSelect => "Keeps the tools on the menu.",
            Purpose::ReflectGate => "Runs the full reflection pass, as before.",
            Purpose::UntrustedFlag => "Adds no flag. The pattern scan still marks the text.",
            Purpose::Classify => "Marks the row low confidence, so you and I look at it.",
            Purpose::PlanDone => "Leaves the step open. Before a fit, a likely finish is only called probably done.",
            Purpose::SteerClass => "Queues the message as it always did. Only a fitted stop asks you first.",
            Purpose::UnverifiedClaim => "Asks me to run the check or say I did not, as before.",
            Purpose::RunIf => "Skips the run and writes the reason in the task's history.",
            Purpose::MailTriage => "Shows nothing for that part of the mail. Mail is never moved or changed.",
            Purpose::ClaimVerdict => "Hands the claim to the language model for a slower review, up to four a check.",
            Purpose::ClaimCheckable => "Leaves the sentence unmarked. Nothing is ever marked on a guess.",
            Purpose::BrowserCheck => "Stops the browser and asks you to take over, up to three times a turn. No picture reader means no check.",
            Purpose::ToolRisk => "Adds nothing. A call it flags gets a question before it runs, or a line on the question that was coming anyway.",
            Purpose::AppKind => "Keeps the kind the name gives. A tool it takes for a send is asked about like one.",
        }
    }

    /// A question of the check that runs on an answer. These wait behind every
    /// hot-path ask in the engine's queue (`decide::lane`).
    pub fn is_check(self) -> bool {
        matches!(self, Purpose::ClaimCheckable | Purpose::ClaimVerdict)
    }

    /// Asked once for each of many items, so the Activity screen folds them into
    /// one row an hour like the background ones, instead of five hundred rows. The
    /// sentence check asks about every sentence of every answer.
    pub fn is_bulk(self) -> bool {
        matches!(self, Purpose::Classify | Purpose::ClaimCheckable)
    }

    pub fn policy(self) -> Policy {
        let row = |mode, impact, measure, at: f32, escalate, background, needs_fast, deadline_s, state_cap_tokens| Policy {
            purpose: self,
            mode,
            impact,
            // The two bars start equal: nothing is calibrated yet, and the
            // plan's starting values do not distinguish them for these rows.
            act_at: Threshold { measure, calibrated: at, uncalibrated: at },
            escalate,
            background,
            needs_fast,
            needs_vision: NeedsVision::No,
            deadline: Duration::from_secs(deadline_s),
            state_cap_tokens,
            experiment: None,
            typing: false,
        };
        use Escalation as E;
        use Impact::{Cheap, Costly, Irreversible, Visible};
        use Measure::{Confidence, PFalse, PTrue, TopProb};
        use Mode::{Assisted, Gate, Reflex, Verify};
        match self {
            Purpose::RetSufficiency => row(Assisted, Cheap, PTrue, 0.5, E::None, false, true, 5, 4_000),
            Purpose::MemScope => row(Reflex, Cheap, Confidence, 0.6, E::None, true, false, 30, 2_000),
            Purpose::ReflectCritic => row(Assisted, Visible, TopProb, 0.5, E::Slow, true, false, 30, 12_000),
            Purpose::GoalMet => row(Assisted, Visible, PTrue, 0.8, E::None, false, true, 8, 6_000),
            Purpose::HandoverRoute => row(Assisted, Cheap, Confidence, 0.6, E::Slow, false, true, 8, 4_000),
            // Acts by taking a tool unit away, so the bar is how sure the model is
            // that it is NOT needed: 0.85 once calibrated, 0.95 before. A wrong
            // keep costs a few tokens; a wrong drop costs a step.
            Purpose::ToolSelect => Policy {
                act_at: Threshold { measure: PFalse, calibrated: 0.85, uncalibrated: 0.95 },
                deadline: Duration::from_millis(300),
                experiment: Some("fast-slow"),
                ..row(Assisted, Visible, PFalse, 0.85, E::None, false, true, 1, 2_000)
            },
            // Acts by skipping the drafting pass, so like the tool menu the bar is
            // how sure the model is that nothing went wrong: p(something to learn)
            // of 0.3 or less once calibrated, 0.15 or less before. A wrong skip
            // loses one lesson; a wrong run costs a model call.
            Purpose::ReflectGate => Policy {
                act_at: Threshold { measure: PFalse, calibrated: 0.7, uncalibrated: 0.85 },
                experiment: Some("fast-slow"),
                ..row(Reflex, Cheap, PFalse, 0.7, E::None, true, false, 30, 6_000)
            },
            // Acts by adding a flag, which never blocks anything, so one bar for
            // both. The plan's value is 0.7.
            Purpose::UntrustedFlag => Policy {
                experiment: Some("fast-slow"),
                ..row(Reflex, Cheap, PTrue, 0.7, E::None, true, false, 30, 6_000)
            },
            // The user asked for the list to be sorted and is waiting on the tool,
            // so it is neither background nor hot: it runs on the processor too,
            // item by item, and never waits for a quiet card. Nothing is taken
            // away below the bar, a row is marked, so a miss costs a glance. The
            // bar is the winner's own probability, the number the table shows.
            Purpose::Classify => Policy {
                act_at: Threshold { measure: TopProb, calibrated: 0.6, uncalibrated: 0.7 },
                needs_vision: NeedsVision::Any,
                experiment: Some("fast-slow"),
                ..row(Assisted, Cheap, TopProb, 0.6, E::None, false, false, 30, 6_000)
            },
            // Acts by ticking a plan item off, which the model sees and the user can
            // undo. The plan's "never auto" before a fit is held in the site, which
            // acts only on a calibrated answer: the 1.0 here is a statement, not a
            // number a probability is meant to reach. Asked when a run is about to
            // end, so a person is waiting and a cold engine is skipped.
            Purpose::PlanDone => Policy {
                act_at: Threshold { measure: PTrue, calibrated: 0.8, uncalibrated: 1.0 },
                experiment: Some("fast-slow"),
                ..row(Assisted, Visible, PTrue, 0.8, E::None, false, true, 5, 6_000)
            },
            // Acts only on `stop`: the probability of that one option, 0.9 once
            // calibrated and never before (the site checks the fit). A wrong stop
            // prompt costs the user one click, a wrong silent stop would cost the run.
            Purpose::SteerClass => Policy {
                act_at: Threshold { measure: Measure::Option("stop"), calibrated: 0.9, uncalibrated: 1.0 },
                deadline: Duration::from_millis(500),
                experiment: Some("fast-slow"),
                ..row(Reflex, Visible, Measure::Option("stop"), 0.9, E::None, false, true, 1, 2_000)
            },
            // Acts by skipping a nudge that asks the run to check its work, so like
            // the tool menu the bar is how sure the model is that the answer claims
            // nothing: p(claims) of 0.15 or less once calibrated, 0.05 or less before.
            Purpose::UnverifiedClaim => Policy {
                act_at: Threshold { measure: PFalse, calibrated: 0.85, uncalibrated: 0.95 },
                experiment: Some("fast-slow"),
                ..row(Assisted, Visible, PFalse, 0.85, E::None, false, true, 3, 6_000)
            },
            // The task runs at 0.5 or more and is skipped below, so a task is never
            // skipped by a model that was not asked: no engine, no budget or a failed
            // read of its input all run it. A scheduled task has nobody waiting.
            Purpose::RunIf => Policy {
                experiment: Some("fast-slow"),
                ..row(Reflex, Visible, PTrue, 0.5, E::None, true, false, 30, 6_000)
            },
            // Shown, never acted on: a tag, a bar and a reply mark on a row of the
            // inbox. Below 0.6 that part shows nothing. The bar is the winner's own
            // probability (a reply mark needs 0.6 for "yes"), one value for both fits
            // because a wrong tag costs a glance and is corrected with a click. It
            // reads mail, so it stays on this computer (decision 12), behind its own
            // experiment and not `fast-slow`.
            Purpose::MailTriage => Policy {
                experiment: Some("mail-triage"),
                ..row(Reflex, Visible, TopProb, 0.6, E::None, true, false, 30, 4_000)
            },
            // Settled at 0.85 on the winner's own probability. Below it, and for
            // every refuted claim, the language model reviews the evidence without
            // seeing this verdict (`USE-10`, blind review). The user clicked Verify
            // and watches a progress row, so it is neither background nor hot: it
            // runs on the processor too, claim by claim, and never waits for a quiet
            // card. A wrong verdict is an underline the user can answer with a click.
            Purpose::ClaimVerdict => Policy {
                experiment: Some("fast-slow"),
                ..row(Verify, Visible, TopProb, 0.85, E::Slow, false, false, 60, 6_000)
            },
            // Asked once a sentence of every answer, while it streams, so it is the
            // cheapest ask the check has. The bar is how sure the model is that the
            // sentence can be checked at all: below it the sentence is left alone and
            // nothing is marked, which is the safe side of a doubt. Not hot (a turn is
            // not waiting on it) and not background (the person is reading the answer
            // it marks), so it runs on the processor too, and a hot-path ask goes
            // before it in the engine's queue (`decide::lane`).
            Purpose::ClaimCheckable => Policy {
                experiment: Some("fast-slow"),
                ..row(Verify, Visible, PTrue, 0.85, E::None, false, false, 30, 2_000)
            },
            // The agent goes on at 0.8 on `fine` and is otherwise stopped for the
            // person to look, so a wrong stop costs one click and a wrong go is what
            // browsing was before this check. A turn is waiting on the picture, so a
            // cold engine or a processor is skipped, not waited for. Any picture
            // reader will do: a page that cannot be read is stopped, not guessed.
            Purpose::BrowserCheck => Policy {
                needs_vision: NeedsVision::Any,
                experiment: Some("fast-slow"),
                ..row(Verify, Costly, Measure::Option("fine"), 0.8, E::User, false, true, 4, 2_000)
            },
            // Acts only by adding a question, so one bar for both fits: the rating's
            // top level ("irreversible") at 0.8 or more. The in-scope question has its
            // own bars in `toolgate`. A person is waiting on the call, so a cold engine
            // or a processor is skipped and the call runs as it did before.
            Purpose::ToolRisk => Policy {
                experiment: Some("fast-slow"),
                ..row(Gate, Irreversible, Measure::Level(3), 0.8, E::None, false, true, 1, 3_000)
            },
            // Acts only by making a gate stricter: `send` at 0.6 or more and never the
            // other way round. Asked once a tool a day, so a processor waits its turn
            // the same as the rest.
            Purpose::AppKind => Policy {
                experiment: Some("fast-slow"),
                ..row(Gate, Irreversible, Measure::Option("send"), 0.6, E::None, false, true, 1, 2_000)
            },
        }
    }
}

impl Purpose {
    /// The row as a streaming turn reads it (`USE-13`). `HandoverRoute` is one
    /// question with two uses, so it keeps one calibration; but in a live turn a
    /// person is waiting, so it has a 300 ms deadline, a higher bar (a card for a
    /// match of 0.85 or more) and sits behind `fast-slow`. Every other purpose
    /// reads the same row in both.
    pub fn live_policy(self) -> Policy {
        let base = self.policy();
        match self {
            Purpose::HandoverRoute => Policy {
                impact: Impact::Visible,
                act_at: Threshold { measure: Measure::TopProb, calibrated: 0.85, uncalibrated: 0.85 },
                deadline: Duration::from_millis(300),
                experiment: Some("fast-slow"),
                ..base
            },
            _ => base,
        }
    }

    /// `SEE-3`: the row as the composer reads it while a message is being typed. The same
    /// question and the same fit, on the backend the person chose (a cloud decision model
    /// answers here too), on the card, and skipped while a local engine is still cold. Nobody
    /// waits on it, so it has a short deadline of its own, and it is not counted against the
    /// turn it may become (the caller passes no conversation).
    pub fn typing_policy(self) -> Policy {
        let base = self.policy();
        Policy {
            typing: true,
            background: false,
            needs_fast: true,
            deadline: Duration::from_millis(500),
            experiment: Some("fast-slow"),
            ..base
        }
    }

    /// The row as an unattended run reads it (`USE-12`). Nobody waits on a decision
    /// there, so the two gate purposes may wait for the engine and run on the
    /// processor, and stay on this computer like any background purpose
    /// (decision 12). Every other purpose reads the same row.
    pub fn headless_policy(self) -> Policy {
        let base = self.policy();
        match self {
            Purpose::ToolRisk | Purpose::AppKind => Policy {
                background: true,
                needs_fast: false,
                deadline: Duration::from_secs(30),
                ..base
            },
            _ => base,
        }
    }
}

/// Where a decision is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Local,
    Cloud,
}

/// The settings key holding one purpose's own backend choice.
pub fn override_key(purpose: Purpose) -> String {
    format!("decide.backend.{}", purpose.name())
}

/// Pick the backend (`DEC-5`): the purpose's own override, then the global
/// `decide.backend`, then local. A background purpose ignores the global
/// setting: it goes to the cloud only when its own override says so.
pub fn resolve_backend(policy: &Policy, global: Option<&str>, purpose_override: Option<&str>) -> BackendKind {
    match purpose_override {
        Some("cloud") => BackendKind::Cloud,
        Some("local") => BackendKind::Local,
        _ if policy.background => BackendKind::Local,
        _ if global == Some("cloud") => BackendKind::Cloud,
        _ => BackendKind::Local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SEE-3`: the composer's row is the same question and fit, on the card, with a short
    /// deadline of its own, and it follows the backend choice; no other row is a typing row.
    #[test]
    fn the_typing_row_is_hot_short_and_follows_the_backend_choice() {
        for p in [Purpose::SteerClass, Purpose::HandoverRoute, Purpose::ToolSelect] {
            let typing = p.typing_policy();
            assert!(typing.typing && typing.needs_fast && !typing.background, "{}", p.name());
            assert_eq!(resolve_backend(&typing, Some("cloud"), None), BackendKind::Cloud, "{}", p.name());
            assert_eq!(resolve_backend(&typing, None, None), BackendKind::Local, "{}", p.name());
            assert_eq!(typing.deadline, Duration::from_millis(500), "{}", p.name());
            assert_eq!(typing.experiment, Some("fast-slow"), "{}", p.name());
            assert_eq!(typing.act_at.calibrated, p.policy().act_at.calibrated, "{}: one calibration", p.name());
            assert_eq!(typing.act_at.uncalibrated, p.policy().act_at.uncalibrated, "{}", p.name());
        }
        for p in ALL {
            assert!(!p.policy().typing && !p.live_policy().typing, "{}", p.name());
        }
    }

    #[test]
    fn every_purpose_has_a_row_and_a_unique_name() {
        let mut names: Vec<_> = ALL.iter().map(|p| p.name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), ALL.len());
        for p in ALL {
            assert_eq!(p.policy().purpose, p);
            assert!(p.policy().state_cap_tokens > 0);
        }
    }

    /// `DEC-T-5`: the global setting never moves a background purpose.
    #[test]
    fn a_background_purpose_stays_local_when_the_global_backend_is_cloud() {
        let policy = Purpose::MemScope.policy();
        assert!(policy.background);
        assert_eq!(resolve_backend(&policy, Some("cloud"), None), BackendKind::Local);
        assert_eq!(resolve_backend(&policy, Some("local"), None), BackendKind::Local);
    }

    /// `DEC-T-5`: only the purpose's own override, set by name, sends it out.
    #[test]
    fn the_purposes_own_override_sends_a_background_purpose_to_the_cloud() {
        let policy = Purpose::ReflectCritic.policy();
        assert_eq!(resolve_backend(&policy, None, Some("cloud")), BackendKind::Cloud);
        assert_eq!(resolve_backend(&policy, Some("cloud"), Some("local")), BackendKind::Local);
    }

    #[test]
    fn a_foreground_purpose_follows_the_global_backend() {
        let policy = Purpose::GoalMet.policy();
        assert!(!policy.background);
        assert_eq!(resolve_backend(&policy, Some("cloud"), None), BackendKind::Cloud);
        assert_eq!(resolve_backend(&policy, None, None), BackendKind::Local);
        assert_eq!(resolve_backend(&policy, Some("cloud"), Some("local")), BackendKind::Local);
    }

    /// `CTL-T-1`: no calibration can only make a decision harder to act on. A bar
    /// that is *lower* when nothing is fitted would let an unchecked number do
    /// more than a checked one.
    #[test]
    fn an_uncalibrated_bar_is_never_lower_than_the_calibrated_one() {
        for p in ALL {
            let at = p.policy().act_at;
            assert!(at.uncalibrated >= at.calibrated, "{}", p.name());
            assert!((0.0..=1.0).contains(&at.calibrated) && (0.0..=1.0).contains(&at.uncalibrated));
        }
    }

    #[test]
    fn a_threshold_reads_the_measure_its_row_names() {
        let at = |measure| Threshold { measure, calibrated: 0.6, uncalibrated: 0.6 };
        let noul = Answer::Noul { p_true: 0.7 };
        assert!(at(Measure::PTrue).reaches(&noul, false));
        assert!(!Threshold { measure: Measure::PTrue, calibrated: 0.8, uncalibrated: 0.8 }.reaches(&noul, false));

        // Three options with the winner at 0.6: the top probability reaches 0.6,
        // the entropy-based confidence does not, which is the strict one.
        let probs = vec![("a".to_string(), 0.6), ("b".to_string(), 0.22), ("c".to_string(), 0.18)];
        let confidence = crate::agent::decide::confidence(&[0.6, 0.22, 0.18]);
        let choice = Answer::Choice { choice: "a".into(), probs, confidence };
        assert!(at(Measure::TopProb).reaches(&choice, true));
        assert!(!at(Measure::Confidence).reaches(&choice, true));
    }

    /// `USE-4`: the tool menu acts by dropping, so the number that counts is how
    /// sure the model is that a unit is NOT needed.
    #[test]
    fn tool_select_drops_a_unit_only_on_a_confident_no() {
        let bar = Purpose::ToolSelect.policy().act_at;
        assert_eq!(bar.measure, Measure::PFalse);
        let says = |p_true| Answer::Noul { p_true };
        // p(needed) 0.03: sure enough once calibrated, and before (0.97 >= 0.95).
        assert!(bar.reaches(&says(0.03), true) && bar.reaches(&says(0.03), false));
        // p(needed) 0.10: sure enough only with a fit behind it.
        assert!(bar.reaches(&says(0.10), true));
        assert!(!bar.reaches(&says(0.10), false));
        // p(needed) 0.5 and above: never dropped.
        assert!(!bar.reaches(&says(0.5), true));
        assert!(!bar.reaches(&says(0.9), true));
    }

    #[test]
    fn tool_select_is_hot_visible_and_behind_fast_slow() {
        let row = Purpose::ToolSelect.policy();
        assert!(row.needs_fast, "a turn is waiting on it");
        assert!(!row.background);
        assert_eq!(row.experiment, Some("fast-slow"));
        assert_eq!(row.deadline, Duration::from_millis(300));
    }

    /// `USE-4`, `USE-13`: 300 ms is for an engine on this computer. Over a network
    /// the same row waits 800 ms, and a row that already waits longer keeps its own.
    #[test]
    fn a_hot_row_gets_at_least_800_ms_over_a_network() {
        assert_eq!(Purpose::ToolSelect.policy().cloud_deadline(), Duration::from_millis(800));
        assert_eq!(Purpose::HandoverRoute.live_policy().cloud_deadline(), Duration::from_millis(800));
        assert_eq!(Purpose::SteerClass.policy().cloud_deadline(), Duration::from_millis(800));
        assert_eq!(Purpose::GoalMet.policy().cloud_deadline(), Duration::from_secs(8));
        assert_eq!(Purpose::ToolSelect.policy().deadline, Duration::from_millis(300), "the local deadline is not moved");
    }

    /// `USE-8`: both filters run without anyone waiting, behind `fast-slow`, and
    /// stay on this computer.
    #[test]
    fn the_two_background_filters_are_background_and_behind_fast_slow() {
        for p in [Purpose::ReflectGate, Purpose::UntrustedFlag] {
            let row = p.policy();
            assert!(row.background && !row.needs_fast, "{}", p.name());
            assert_eq!(row.experiment, Some("fast-slow"), "{}", p.name());
            assert_eq!(resolve_backend(&row, Some("cloud"), None), BackendKind::Local, "{}", p.name());
        }
    }

    /// `USE-8`: the reflection gate skips only on a confident "nothing to learn":
    /// 0.3 or less once calibrated, 0.15 or less before.
    #[test]
    fn the_reflection_gate_skips_only_when_the_model_is_sure_nothing_went_wrong() {
        let bar = Purpose::ReflectGate.policy().act_at;
        let says = |p_true| Answer::Noul { p_true };
        assert!(bar.reaches(&says(0.30), true) && !bar.reaches(&says(0.31), true));
        assert!(bar.reaches(&says(0.15), false) && !bar.reaches(&says(0.20), false));
        // Unsure, or sure that something was corrected: reflection runs.
        assert!(!bar.reaches(&says(0.5), true) && !bar.reaches(&says(0.9), true));
    }

    /// `USE-8`: the untrusted flag is raised at 0.7 and not below.
    #[test]
    fn the_untrusted_flag_is_raised_at_seven_tenths() {
        let bar = Purpose::UntrustedFlag.policy().act_at;
        assert!(bar.reaches(&Answer::Noul { p_true: 0.7 }, false));
        assert!(!bar.reaches(&Answer::Noul { p_true: 0.69 }, true));
    }

    /// `USE-9`: a person is waiting on the tool, so `classify` is neither hot nor
    /// background; it reads the winner's own probability, and a row below the bar
    /// is marked, never dropped.
    #[test]
    fn classify_marks_a_row_below_the_winners_probability_bar() {
        let row = Purpose::Classify.policy();
        assert!(!row.needs_fast && !row.background, "waited for, on the processor too");
        assert_eq!(row.experiment, Some("fast-slow"));
        assert_eq!(row.escalate, Escalation::None);
        let choice = |top: f32| Answer::Choice {
            choice: "bug".into(),
            probs: vec![("bug".into(), top), ("feature".into(), 1.0 - top)],
            confidence: 0.0,
        };
        assert!(row.act_at.reaches(&choice(0.6), true) && !row.act_at.reaches(&choice(0.59), true));
        assert!(row.act_at.reaches(&choice(0.7), false) && !row.act_at.reaches(&choice(0.65), false));
        // A yes-or-no answer reads the likelier side, whichever it is.
        assert!(row.act_at.reaches(&Answer::Noul { p_true: 0.05 }, false));
        assert!(!row.act_at.reaches(&Answer::Noul { p_true: 0.55 }, false));
        // Its rows are folded by the hour on the Activity screen; no other purpose's are.
        assert_eq!(ALL.iter().filter(|p| p.is_bulk()).count(), 2);
        assert!(Purpose::Classify.is_bulk() && Purpose::ClaimCheckable.is_bulk());
    }

    /// `USE-5`: a stop is read off the `stop` option alone, wherever it ranks.
    #[test]
    fn steer_class_reads_the_stop_option_and_not_the_winner() {
        let row = Purpose::SteerClass.policy();
        assert!(row.needs_fast && !row.background);
        let says = |stop: f32, other: f32| Answer::Choice {
            choice: if stop > other { "stop".into() } else { "add".into() },
            probs: vec![("stop".into(), stop), ("add".into(), other), ("redirect".into(), 0.0)],
            confidence: 0.0,
        };
        assert!(row.act_at.reaches(&says(0.9, 0.1), true));
        assert!(!row.act_at.reaches(&says(0.89, 0.11), true));
        // A winner that is not `stop` never counts, however sure it is.
        assert!(!row.act_at.reaches(&says(0.05, 0.95), true));
        // An answer with no such option, and a yes-or-no one, never reach it.
        assert!(!row.act_at.reaches(&Answer::Noul { p_true: 1.0 }, true));
        assert!(!Measure::Option("stop").eq(&Measure::Option("add")));
    }

    /// `USE-5`: a plan item is ticked at 0.8 once a fit backs the number, and the
    /// task and the claim check keep their own bars.
    #[test]
    fn the_loop_control_rows_hold_the_plans_bars() {
        let at = |p: Purpose| p.policy().act_at;
        assert_eq!((at(Purpose::PlanDone).calibrated, at(Purpose::PlanDone).uncalibrated), (0.8, 1.0));
        assert_eq!((at(Purpose::SteerClass).calibrated, at(Purpose::SteerClass).uncalibrated), (0.9, 1.0));
        assert_eq!(at(Purpose::UnverifiedClaim).measure, Measure::PFalse);
        assert_eq!(at(Purpose::RunIf).calibrated, 0.5);
        for p in [Purpose::PlanDone, Purpose::SteerClass, Purpose::UnverifiedClaim, Purpose::RunIf] {
            assert_eq!(p.policy().experiment, Some("fast-slow"), "{}", p.name());
        }
        // Only the task check runs with nobody waiting, so only it is local-only.
        assert!(Purpose::RunIf.policy().background && !Purpose::RunIf.policy().needs_fast);
        for p in [Purpose::PlanDone, Purpose::SteerClass, Purpose::UnverifiedClaim] {
            assert!(p.policy().needs_fast && !p.policy().background, "{}", p.name());
        }
        assert_eq!(Purpose::SteerClass.policy().deadline, Duration::from_millis(500));
    }

    /// `SEE-1`: the automatic check follows the backend choice, so a cloud decision model
    /// checks answers on a computer with no local one; its own override still wins.
    #[test]
    fn the_automatic_check_follows_the_backend_choice() {
        for p in [Purpose::ClaimCheckable, Purpose::ClaimVerdict] {
            let row = p.policy();
            assert!(!row.background && !row.needs_fast, "{}", p.name());
            assert_eq!(resolve_backend(&row, Some("cloud"), None), BackendKind::Cloud, "{}", p.name());
            assert_eq!(resolve_backend(&row, Some("cloud"), Some("local")), BackendKind::Local, "{}", p.name());
            assert_eq!(resolve_backend(&row, None, None), BackendKind::Local, "{}", p.name());
        }
    }

    /// `SEE-1`: the sentence check is Verify mode, visible, neither hot nor background,
    /// 0.85 on "can be checked", no escalation, behind `fast-slow`.
    #[test]
    fn the_sentence_check_row_is_the_one_the_plan_names() {
        let row = Purpose::ClaimCheckable.policy();
        assert_eq!((row.mode, row.impact), (Mode::Verify, Impact::Visible));
        assert!(!row.background && !row.needs_fast);
        assert_eq!(row.escalate, Escalation::None);
        assert_eq!(row.experiment, Some("fast-slow"));
        assert_eq!((row.act_at.calibrated, row.act_at.uncalibrated), (0.85, 0.85));
        assert_eq!(row.act_at.measure, Measure::PTrue);
        assert_eq!(Purpose::ClaimCheckable.name(), "claim-checkable");
    }

    /// `USE-12`: the risk row reads the rating's top level, wherever the winner is.
    #[test]
    fn tool_risk_reads_the_top_level_of_its_rating() {
        let row = Purpose::ToolRisk.policy();
        assert_eq!((row.mode, row.impact), (Mode::Gate, Impact::Irreversible));
        assert!(row.needs_fast && !row.background, "a person waits on the call");
        assert_eq!(row.experiment, Some("fast-slow"));
        assert_eq!(row.escalate, Escalation::None, "it only ever adds caution");
        let rating = |p: [f32; 4]| Answer::Score { score: 0.0, probs: p.to_vec(), confidence: 0.0 };
        assert!(row.act_at.reaches(&rating([0.1, 0.05, 0.05, 0.8]), false));
        assert!(!row.act_at.reaches(&rating([0.1, 0.05, 0.06, 0.79]), true));
        // A rating that is mostly "serious" is not "irreversible", however sure.
        assert!(!row.act_at.reaches(&rating([0.0, 0.0, 0.95, 0.05]), true));
        // Other kinds of answer, and a missing level, never reach it.
        assert!(!row.act_at.reaches(&Answer::Noul { p_true: 1.0 }, true));
        let short = Answer::Score { score: 0.0, probs: vec![1.0, 0.0], confidence: 1.0 };
        assert!(!row.act_at.reaches(&short, true));
    }

    /// `USE-12`: a connector tool is taken for a send at 0.6 on that one option, and
    /// the two gate purposes read the unattended row when nobody is there.
    #[test]
    fn app_kind_reads_the_send_option_and_a_gate_purpose_waits_when_nobody_is() {
        let row = Purpose::AppKind.policy();
        assert_eq!(row.act_at.measure, Measure::Option("send"));
        assert_eq!((row.act_at.calibrated, row.act_at.uncalibrated), (0.6, 0.6));
        for p in [Purpose::ToolRisk, Purpose::AppKind] {
            let live = p.policy();
            let away = p.headless_policy();
            assert!(live.needs_fast && !live.background, "{}", p.name());
            assert!(away.background && !away.needs_fast, "{}", p.name());
            assert_eq!(away.deadline, Duration::from_secs(30));
            assert_eq!(resolve_backend(&away, Some("cloud"), None), BackendKind::Local, "{}: unattended stays here", p.name());
            assert_eq!(resolve_backend(&live, Some("cloud"), None), BackendKind::Cloud, "{}", p.name());
        }
        // Every other row reads the same thing unattended.
        assert!(!Purpose::GoalMet.headless_policy().background);
    }

    #[test]
    fn the_bar_in_force_follows_whether_a_fit_backs_the_numbers() {
        let t = Threshold { measure: Measure::PTrue, calibrated: 0.5, uncalibrated: 0.9 };
        assert_eq!(t.at(true), 0.5);
        assert_eq!(t.at(false), 0.9);
        assert!(t.reaches(&Answer::Noul { p_true: 0.7 }, true));
        assert!(!t.reaches(&Answer::Noul { p_true: 0.7 }, false));
    }

    /// A row that names an experiment names one that exists, so the screen can
    /// say what turning it on does (`EXP-2`).
    #[test]
    fn a_row_that_needs_an_experiment_names_a_registered_one() {
        for p in ALL {
            if let Some(id) = p.policy().experiment {
                assert!(crate::experiments::find(id).is_some(), "{} names {id}", p.name());
            }
        }
    }

    /// Background rows are never hot: they wait for the engine, hot ones skip.
    #[test]
    fn hot_and_background_do_not_overlap() {
        for p in ALL {
            let policy = p.policy();
            assert!(!(policy.background && policy.needs_fast), "{} is both", p.name());
        }
    }
}
