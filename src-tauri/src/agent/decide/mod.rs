//! `DEC`: the decision seam. A small model answers small, closed questions
//! (yes or no, pick one, rate on a scale) in tens of milliseconds, with a
//! probability for every answer. Callers ask through [`ask`] and never see
//! which engine answered.
//!
//! Two engines answer: the local one (`runtime::decideserver`) and OpenRouter's
//! (`cloud`), chosen per purpose by `policy::resolve_backend`. Every site that
//! asks keeps its old path as the fallback, so a computer with neither loses
//! nothing (`DEC-6`).
//!
//! Layout: this file holds the types and the entry point; `policy` the
//! per-purpose rules; `control` the budgets and thresholds (`CTL-2`);
//! `outcomes` and `calibration` how decisions are labelled and how far their
//! probabilities can be trusted (`CTL-3`, `CTL-4`); `seed` the labelled sets a
//! calibration runs; `local` the wire format of `llama-server`'s
//! `/v1/systemone`; `cloud` OpenRouter's `/alpha/decisions`, its catalog and
//! the probe (`DEC-5`, `DEC-5a`); `sites` the questions of the five existing callers and how
//! their answers map back; `tools` the tool menu's groups, question and held
//! state (`USE-4`); `ablation` the golden set and scoring that gate it
//! (`CTL-T-3`); `handover` the pre-match of a message against the user's
//! handover rules before the lead runs (`USE-13`); `filters` the two background
//! filters, the reflection gate and the untrusted-text flag (`USE-8`);
//! `loopctl` the three checks inside the agent loop, plan completion, steering
//! and unverified success (`USE-5`); `runif` a scheduled task's condition
//! (`USE-7`); `mailtriage` the kind, urgency and reply mark of a new message of
//! the small inbox (`USE-6`); `claims` the verdict on one claim of an answer
//! (`USE-10`); `checkable` whether a sentence of an answer can be checked at all, and
//! whether it is a fact or something the assistant says it did (`SEE-1`); `lane` the order
//! in the engine's queue, hot-path asks first (`SEE-1`); `pagecheck` what the page the browser agent just reached shows,
//! and whether a login or a captcha hands the browser to the person (`USE-11`);
//! `toolgate` how risky and how asked-for a tool call is before it runs, and
//! whether a connector tool sends (`USE-12`); `typing` what the composer says about a
//! message while it is typed, and the answers the send reuses (`SEE-3`).
//! The `classify` tool (`USE-9`) is `agent::classify`; it asks
//! `Purpose::Classify` here, and the check of an answer is `agent::verify`.

pub mod ablation;
pub mod calibration;
pub mod checkable;
pub mod claims;
pub mod cloud;
pub mod control;
pub mod filters;
pub mod handover;
pub mod lane;
pub mod local;
pub mod loopctl;
pub mod mailtriage;
pub mod outcomes;
pub mod pagecheck;
pub mod policy;
pub mod runif;
pub mod seed;
pub mod sites;
pub mod thought;
pub mod toolgate;
pub mod tools;
pub mod typing;
pub mod words;

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Instant;

use tauri::{AppHandle, Manager};

use crate::db::Db;
use crate::runtime::decideserver::{self, DecideManager};
use crate::runtime::RuntimeManager;

pub use policy::{Policy, Purpose};

/// `DEC-2`. `noul` always carries both criteria, because the cloud backend
/// requires both. A `Choice` that can be wrong in a costly way includes an
/// `unsure` option: the model is never forced into a substantive answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Question {
    Noul { instructions: String, when_true: String, when_false: String },
    /// Options are `(id, criteria)`, at most 255.
    Choice { instructions: String, options: Vec<(String, String)> },
    /// Two to ten levels, lowest first.
    Score { instructions: String, levels: Vec<String> },
}

/// An image for a question, as a `data:` URL. Only a backend that reports
/// vision takes any.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRef {
    pub data_url: String,
}

/// One request: a state, optional images, and one or more named questions.
/// A decision model reads all the questions of a request together, so a
/// caller that needs independent answers asks in separate requests.
#[derive(Debug, Clone)]
pub struct Ask<'a> {
    pub state: &'a str,
    pub images: Vec<ImageRef>,
    pub questions: Vec<(&'a str, Question)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// The probability that the statement is true.
    Noul { p_true: f32 },
    Choice { choice: String, probs: Vec<(String, f32)>, confidence: f32 },
    Score { score: f32, probs: Vec<f32>, confidence: f32 },
}

impl Answer {
    /// How sure the model is, 0 (no idea) to 1 (certain). The same measure for
    /// every kind of question and every backend.
    pub fn confidence(&self) -> f32 {
        match self {
            Answer::Noul { p_true } => confidence(&[*p_true, 1.0 - *p_true]),
            Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => *confidence,
        }
    }
}

/// `confidence` = 1 minus the entropy of the probabilities, scaled so that a
/// flat spread is 0 and a single certain answer is 1. Callers never work this
/// out themselves.
pub fn confidence(probs: &[f32]) -> f32 {
    let n = probs.len();
    if n < 2 {
        return 1.0;
    }
    let total: f32 = probs.iter().map(|p| p.max(0.0)).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let entropy: f32 = probs
        .iter()
        .map(|p| p.max(0.0) / total)
        .filter(|p| *p > 0.0)
        .map(|p| -p * p.ln())
        .sum();
    (1.0 - entropy / (n as f32).ln()).clamp(0.0, 1.0)
}

/// Which engine answered. Logged with every decision, because a different
/// model is a different calibration (`DEC-5`).
#[derive(Debug, Clone, PartialEq)]
pub enum BackendId {
    Local { model: String },
    /// `model` is the version that answered, `provider` who served it.
    OpenRouter { model: String, provider: Option<String> },
}

impl BackendId {
    pub fn label(&self) -> String {
        match self {
            BackendId::Local { model } => format!("local:{model}"),
            BackendId::OpenRouter { model, provider: Some(p) } => format!("openrouter:{model}@{p}"),
            BackendId::OpenRouter { model, provider: None } => format!("openrouter:{model}"),
        }
    }

    /// Whether the question left this computer.
    pub fn is_cloud(&self) -> bool {
        matches!(self, BackendId::OpenRouter { .. })
    }
}

/// What a backend can do right now (`DEC-8`). Only `vision` and the state
/// limit are used so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCaps {
    pub vision: bool,
    pub max_state_tokens: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// For outcome labels later (`CTL-3`).
    pub id: String,
    pub answers: BTreeMap<String, Answer>,
    pub backend: BackendId,
    /// Whether a fitted calibration backs these probabilities (`CTL-4`). When
    /// false the numbers are the model's own, and a caller may only be stricter
    /// than the plan's starting values, never looser.
    pub calibrated: bool,
    pub ms: u32,
}

impl Verdict {
    pub fn get(&self, id: &str) -> Option<&Answer> {
        self.answers.get(id)
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DecideError {
    /// No decision engine is set up, or this purpose may not use the one there is.
    #[error("no decision engine is available")]
    Unavailable,
    /// The engine was not ready in time, or answered too slowly. The caller
    /// carries on as if nothing had been asked.
    #[error("the decision engine did not answer in time")]
    Deadline,
    #[error("the decision engine cannot read images")]
    NoVision,
    /// The local engine runs on the processor, which a person waiting is not made to wait
    /// for (`DEC-4`). A cloud decision model or a graphics card answers instead.
    #[error("the decision engine runs on the processor")]
    Processor,
    /// A turn or hourly limit was hit (`CTL-2`). The caller takes its default.
    #[error("not asked: {0}")]
    Budget(control::Limit),
    #[error("the decision engine failed: {0}")]
    Failed(String),
}

/// Who is asking, for the log. Neither is needed to get an answer.
#[derive(Debug, Clone, Copy, Default)]
pub struct Opts<'a> {
    pub conversation_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
}

/// What the automatic check of an answer counts under, after its conversation's id.
const AUTO_CHECK_KEY: &str = "#auto-check";

impl Opts<'_> {
    /// The key a decision is counted under (`CTL-2`): the conversation. The automatic check
    /// of an answer (`SEE-1`) carries its run and counts apart, so a deep check started in
    /// the same chat while it runs never resets its counts, nor it the deep check's.
    pub(crate) fn count_key(&self, purpose: Purpose) -> Option<String> {
        let conversation = self.conversation_id?;
        Some(match (purpose.is_check(), self.run_id) {
            (true, Some(_)) => format!("{conversation}{AUTO_CHECK_KEY}"),
            _ => conversation.to_string(),
        })
    }
}

/// Set once at startup, as `media::jobs` and `agent::background` do. A site
/// deep inside a tool call has no command to borrow state from, and threading
/// a handle through every caller would be the worse trade.
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: AppHandle) {
    let _ = APP.set(app);
}

/// The app handle, for a check that tells the screen after the run's own stream is over.
pub(crate) fn app() -> Option<&'static AppHandle> {
    APP.get()
}

/// The database, for a background check that outlives the call that started it
/// and so holds no borrow of its own. `None` before start-up and in a test.
pub(crate) fn db() -> Option<tauri::State<'static, Db>> {
    APP.get().map(|app| app.state::<Db>())
}

/// How many characters of state one token is assumed to cost. Deliberately
/// low: running over the limit is a failed request, running under it only
/// costs a little context.
const CHARS_PER_TOKEN: usize = 3;
/// Room left for the instructions and criteria around the state.
const PROMPT_RESERVE_TOKENS: u32 = 1_500;

/// Cut `state` to `max_tokens`, from the middle, leaving a marker, because both
/// ends of a long text usually matter (what was asked, and how it ended).
/// Returns the text and whether it was cut.
pub fn truncate_middle(state: &str, max_tokens: u32) -> (String, bool) {
    let budget = (max_tokens.saturating_sub(PROMPT_RESERVE_TOKENS).max(256) as usize) * CHARS_PER_TOKEN;
    let chars: Vec<char> = state.chars().collect();
    if chars.len() <= budget {
        return (state.to_string(), false);
    }
    let left_out = chars.len() - budget;
    let head = budget / 2;
    let tail = budget - head;
    let mut out: String = chars[..head].iter().collect();
    out.push_str(&format!("\n[... {left_out} characters left out ...]\n"));
    out.extend(chars[chars.len() - tail..].iter());
    (out, true)
}

/// The first `n` characters of `s`, for the log's state preview.
fn preview(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// What the log keeps of a state. A mail's text is read for the check and not
/// kept (`USE-6a`), so only its header lines are; every other purpose keeps the
/// start of its state.
fn log_preview(purpose: Purpose, state: &str, n: usize) -> String {
    match purpose {
        Purpose::MailTriage => preview(mailtriage::header_of(state), n),
        _ => preview(state, n),
    }
}

/// A short stable fingerprint of the state, so two decisions on the same state
/// can be told apart from two on different ones without storing the state.
pub(crate) fn state_hash(state: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(state.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Ask a question of the decision engine (`DEC-5`). `Err` always means "carry on
/// the way you did before there was a decision engine"; no caller treats it as
/// a failure of its own.
pub async fn ask(ask: &Ask<'_>, purpose: Purpose, opts: Opts<'_>) -> Result<Verdict, DecideError> {
    let app = APP.get().ok_or(DecideError::Unavailable)?;
    ask_with(app, ask, purpose, opts).await
}

/// [`ask`] for a streaming turn: the purpose's live row (`USE-13`). The same
/// question and the same fit, with the deadline and experiment of a turn that has
/// a person waiting.
pub async fn ask_live(ask: &Ask<'_>, purpose: Purpose, opts: Opts<'_>) -> Result<Verdict, DecideError> {
    let app = APP.get().ok_or(DecideError::Unavailable)?;
    ask_under(app, ask, purpose, opts, purpose.live_policy()).await
}

/// [`ask`] for a message still being typed (`SEE-3`): the purpose's typing row, on the
/// backend the person chose. Asked with no conversation, so it uses none of the turn's
/// decisions; what it answers is kept by [`typing`] for the send to reuse, and is not
/// logged unless the hint is acted on or the message is sent.
pub async fn ask_typing(ask: &Ask<'_>, purpose: Purpose, opts: Opts<'_>) -> Result<Verdict, DecideError> {
    let app = APP.get().ok_or(DecideError::Unavailable)?;
    ask_under(app, ask, purpose, opts, purpose.typing_policy()).await
}

/// [`ask`] for a gate purpose (`USE-12`): the row an unattended run reads when
/// nobody is there to wait, and the ordinary one when somebody is.
pub async fn ask_gate(ask: &Ask<'_>, purpose: Purpose, opts: Opts<'_>, headless: bool) -> Result<Verdict, DecideError> {
    let app = APP.get().ok_or(DecideError::Unavailable)?;
    let policy = if headless { purpose.headless_policy() } else { purpose.policy() };
    ask_under(app, ask, purpose, opts, policy).await
}

/// [`ask`] against a given app, which is what the tests use.
async fn ask_with<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    ask: &Ask<'_>,
    purpose: Purpose,
    opts: Opts<'_>,
) -> Result<Verdict, DecideError> {
    ask_under(app, ask, purpose, opts, purpose.policy()).await
}

async fn ask_under<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    ask: &Ask<'_>,
    purpose: Purpose,
    opts: Opts<'_>,
    policy: policy::Policy,
) -> Result<Verdict, DecideError> {
    let db = app.state::<Db>();
    let mgr = app.state::<RuntimeManager>();
    let decide = app.state::<DecideManager>();

    // `SEE-1`: a hot-path ask is in the lane from here to its answer, so no check ask
    // goes out ahead of it.
    let _hot = policy.needs_fast.then(lane::hot);

    // A purpose behind an experiment is not asked while the experiment is off
    // (`EXP-1`: both sides gate).
    if policy.experiment.is_some_and(|id| !crate::experiments::is_on(&db, id)) {
        return Err(DecideError::Unavailable);
    }

    local::check_ask(ask).map_err(DecideError::Failed)?;
    let hash = state_hash(ask.state);

    // The cloud, when this purpose goes there. A cloud that cannot be used for
    // this call (no key, a stop in force, busy) is asked of this computer instead,
    // and never the other way round (`DEC-5`).
    let mut admitted = false;
    let kind = policy::resolve_backend(
        &policy,
        db.get_setting(cloud::BACKEND_KEY).ok().flatten().as_deref(),
        db.get_setting(&policy::override_key(purpose)).ok().flatten().as_deref(),
    );
    if kind == policy::BackendKind::Cloud {
        match cloud::attempt(app, ask, purpose, opts, &policy, &hash).await {
            cloud::Attempt::Done(verdict) => return Ok(verdict),
            cloud::Attempt::Stop(e) => return Err(e),
            cloud::Attempt::Local { admitted: a } => admitted = a,
        }
    }

    let started = Instant::now();
    let local = decideserver::resolve_local(&mgr, &db).await.ok_or(DecideError::Unavailable)?;
    // A running engine is where it was started, which may not be where a fresh
    // start would put it now.
    if policy.needs_fast && !decide.running_gpu().unwrap_or(local.gpu) {
        // On the processor only background purposes run (`DEC-4`).
        return Err(DecideError::Processor);
    }
    if !ask.images.is_empty() && local.mmproj.is_none() {
        return Err(DecideError::NoVision);
    }

    // The hot path never waits for a cold engine: it starts one for next time
    // and lets the caller carry on (decision 6). A background purpose waits.
    if policy.needs_fast && !decide.is_warm() {
        decideserver::warm(app.clone());
        return Err(DecideError::Deadline);
    }

    // `SEE-1`: a check ask waits its turn behind the earlier check asks, and behind any
    // hot-path ask. Held until the answer is back.
    let _turn = match purpose.is_check() {
        true => Some(lane::check_turn().await),
        false => None,
    };

    // Budgets (`CTL-2`): counted here, once the engine is known to be there, so a
    // skipped or unavailable ask never uses one up. A call the cloud already
    // counted is not counted again.
    let model_key = local
        .model_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| local.model_name.clone());
    if !admitted {
        if let Err(limit) = control::controller().admit(
            opts.count_key(purpose).as_deref(),
            purpose,
            &hash,
            policy.background,
            control::now(),
        ) {
            let backend = BackendId::Local { model: local.model_name.clone() };
            log_budget_skip(&db, opts, purpose, &backend.label(), &model_key, ask.state, &hash, limit);
            return Err(DecideError::Budget(limit));
        }
    }

    let live = decide
        .ensure_started(&mgr.client, &local)
        .await
        .map_err(|e| DecideError::Failed(e.to_string()))?;

    // Only a background purpose gives way to a streaming turn. A tool the turn
    // itself called (`Classify`) is that turn: waiting for it to end would wait
    // for the answer to the very question being asked.
    if policy.background {
        decideserver::wait_for_quiet_gpu(app, live.gpu).await;
    }

    let cap = policy.state_cap_tokens.min(live.max_state_tokens);
    let (state, cut) = truncate_middle(ask.state, cap);
    if cut {
        let _ = db.log_activity(
            opts.conversation_id,
            "decide",
            &format!("{}: the state was cut to fit ({} characters)", purpose.name(), ask.state.chars().count()),
        );
    }
    let body = local::build_body(ask, &state);
    let reply = local::post(&mgr.client, &live.base_url, &live.token, &body, policy.deadline).await?;
    let (answers, input_tokens) = local::parse_response(ask, &reply).map_err(DecideError::Failed)?;

    let ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
    decide.record_ms(ms);
    let mut verdict = Verdict {
        id: uuid::Uuid::new_v4().to_string(),
        answers,
        backend: BackendId::Local { model: local.model_name.clone() },
        calibrated: false,
        ms,
    };
    record_verdict(&db, opts, purpose, &mut verdict, &model_key, &hash, input_tokens, None, ask.state, policy.typing);
    Ok(verdict)
}

/// A decision skipped by a budget is still a row, so Activity can show it
/// (`CTL-2`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn log_budget_skip(
    db: &Db,
    opts: Opts<'_>,
    purpose: Purpose,
    backend: &str,
    model_key: &str,
    state: &str,
    hash: &str,
    limit: control::Limit,
) {
    let _ = db.add_decision(&crate::db::decisions::NewDecision {
        id: &uuid::Uuid::new_v4().to_string(),
        conversation_id: opts.conversation_id,
        run_id: opts.run_id,
        purpose: purpose.name(),
        backend,
        model: model_key,
        ms: 0,
        calibrated: false,
        answers: &serde_json::json!({}),
        input_tokens: 0,
        state_preview: &log_preview(purpose, state, 200),
        state_hash: hash,
        action: Some(&control::Action::Budget(limit).code()),
    });
}

/// Calibrate an answer and write it to the log (`DEC-7`). `cost_usd` is what the
/// cloud charged, when it said. An ask about a message still being typed (`typed`)
/// is not written: [`typing`] holds it until the hint it gave is acted on or the
/// message is sent, so a draft that is never sent leaves nothing in the log.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_verdict(
    db: &Db,
    opts: Opts<'_>,
    purpose: Purpose,
    verdict: &mut Verdict,
    model_key: &str,
    hash: &str,
    input_tokens: u32,
    cost_usd: Option<f64>,
    state: &str,
    typed: bool,
) {
    // The log keeps the model's own numbers, so a later fit starts from them and
    // never from numbers already rescaled by an earlier fit.
    let raw = summarise(verdict);
    calibrate(db, model_key, purpose, verdict);
    let record = super::log::DecisionRecord {
        id: verdict.id.clone(),
        purpose: purpose.name().to_string(),
        backend: verdict.backend.label(),
        ms: verdict.ms,
        calibrated: verdict.calibrated,
        answers: raw,
        input_tokens,
        state_preview: log_preview(purpose, state, 200),
        state_hash: hash.to_string(),
        action: None,
    };
    if typed {
        typing::hold(record, model_key, cost_usd);
        return;
    }
    write_record(db, opts, &record, model_key, cost_usd);
}

/// Write one decision to the log: its row, its cost, and the conversation's session log.
pub(crate) fn write_record(
    db: &Db,
    opts: Opts<'_>,
    record: &super::log::DecisionRecord,
    model_key: &str,
    cost_usd: Option<f64>,
) {
    let _ = db.add_decision(&crate::db::decisions::NewDecision {
        id: &record.id,
        conversation_id: opts.conversation_id,
        run_id: opts.run_id,
        purpose: &record.purpose,
        backend: &record.backend,
        model: model_key,
        ms: record.ms,
        calibrated: record.calibrated,
        answers: &record.answers,
        input_tokens: record.input_tokens,
        state_preview: &record.state_preview,
        state_hash: &record.state_hash,
        action: None,
    });
    if let Some(cost) = cost_usd {
        let _ = db.set_decision_cost(&record.id, cost);
    }
    if let Some(conversation_id) = opts.conversation_id {
        super::log::record_decision(db, conversation_id, opts.run_id, record);
    }
}

/// Rescale a verdict's probabilities by the purpose's fit, when a good one
/// exists for this model (`CTL-4`). Without one the numbers stay the model's own
/// and `calibrated` stays false, so a caller can only be stricter.
fn calibrate(db: &Db, model_key: &str, purpose: Purpose, verdict: &mut Verdict) {
    let Some(fit) = calibration::load(db, model_key, purpose.name()).filter(|f| f.is_calibrated()) else { return };
    verdict.answers = verdict.answers.iter().map(|(k, a)| (k.clone(), calibration::temper_answer(a, fit.t))).collect();
    verdict.calibrated = true;
}

/// The answers as the log keeps them: the winner and its probability, small
/// enough to read in a row.
fn summarise(v: &Verdict) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (id, a) in &v.answers {
        let row = match a {
            Answer::Noul { p_true } => serde_json::json!({ "p": p_true }),
            Answer::Choice { choice, probs, confidence } => serde_json::json!({
                "choice": choice,
                "probs": probs.iter().map(|(k, p)| serde_json::json!([k, p])).collect::<Vec<_>>(),
                "confidence": confidence,
            }),
            Answer::Score { score, probs, confidence } => {
                serde_json::json!({ "score": score, "probs": probs, "confidence": confidence })
            }
        };
        out.insert(id.clone(), row);
    }
    serde_json::Value::Object(out)
}

/// Say what was done with an answer: acted on, or held back (`CTL-2`). The site
/// that asked calls this once it has read the answer, so the log can show it
/// next to the probabilities. Best effort, like the log itself.
pub fn settle(verdict: &Verdict, purpose: Purpose, act: control::Act, opts: Opts<'_>) -> control::Action {
    let key = opts.count_key(purpose);
    match APP.get() {
        Some(app) => control::settle(&app.state::<Db>(), verdict, purpose, act, key.as_deref()),
        None => control::after(purpose, act, key.as_deref()),
    }
}

/// `SEE-2`: say, in words, what a decision changed, for the receipt under the run.
/// Best effort like the log: a decision that was never logged has nowhere to put it.
pub fn note(verdict: &Verdict, text: &str) {
    if let Some(app) = APP.get() {
        let _ = app.state::<Db>().set_decision_note(&verdict.id, text);
    }
}

/// A new user turn begins in this conversation: its decision counters start
/// again (`CTL-2`).
pub fn begin_turn(conversation_id: &str) {
    control::controller().begin_turn(conversation_id);
    toolgate::forget_turn(conversation_id);
}

/// A check of an answer begins in this conversation: its claim and review counts
/// start again (`USE-10`). The chat turn's own counts are left alone.
pub fn begin_check(conversation_id: &str) {
    control::controller().begin_check(conversation_id);
}

/// The automatic check of an answer begins (`SEE-1`): its own counts start again, apart from
/// a deep check of the same chat ([`Opts::count_key`]).
pub fn begin_auto_check(conversation_id: &str) {
    control::controller().begin_check(&format!("{conversation_id}{AUTO_CHECK_KEY}"));
}

/// Start the engine in the background if a chat turn is beginning and some
/// hot-path purpose could use it, so the first decision of the turn is not the
/// one that pays for the start (`DEC-4`, decision 6). Does nothing when no
/// decision model is installed.
pub fn prewarm() {
    if let Some(app) = APP.get() {
        decideserver::warm(app.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_is_one_for_a_certain_answer_and_zero_for_a_flat_one() {
        assert!((confidence(&[1.0, 0.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(confidence(&[0.25, 0.25, 0.25, 0.25]).abs() < 1e-6);
        assert!(confidence(&[0.5, 0.5]).abs() < 1e-6);
    }

    #[test]
    fn confidence_rises_as_the_spread_narrows() {
        let wide = confidence(&[0.4, 0.3, 0.3]);
        let narrow = confidence(&[0.8, 0.1, 0.1]);
        assert!(narrow > wide);
        assert!((0.0..=1.0).contains(&wide) && (0.0..=1.0).contains(&narrow));
    }

    /// The same number whether the question had two options or twenty: the
    /// scale is normalised by the most the entropy could be.
    #[test]
    fn confidence_is_scaled_by_the_number_of_options() {
        let two = confidence(&[0.9, 0.1]);
        let four = confidence(&[0.9, 0.1 / 3.0, 0.1 / 3.0, 0.1 / 3.0]);
        assert!(two > 0.0 && four > 0.0);
        assert!((two - 0.531).abs() < 0.01, "binary 0.9 is about 0.53, got {two}");
    }

    #[test]
    fn a_noul_answer_gets_its_confidence_from_its_two_sides() {
        assert!(Answer::Noul { p_true: 0.5 }.confidence().abs() < 1e-6);
        assert!(Answer::Noul { p_true: 0.99 }.confidence() > 0.9);
        assert!((Answer::Noul { p_true: 0.2 }.confidence() - Answer::Noul { p_true: 0.8 }.confidence()).abs() < 1e-6);
    }

    /// `USE-6a`: the log may hold who wrote and the subject, never the text.
    #[test]
    fn a_mail_question_is_logged_by_its_header_and_not_by_its_text() {
        let site = mailtriage::seed_ask("Ana Ruiz", "Contract draft", "The salary is 90000 and my phone is 555 0100.");
        let kept = log_preview(Purpose::MailTriage, &site.state, 200);
        assert!(kept.contains("Subject: Contract draft"), "{kept}");
        assert!(!kept.contains("salary") && !kept.contains("555"), "{kept}");
        // Another purpose keeps the start of its state, as before.
        assert_eq!(log_preview(Purpose::GoalMet, "one two three", 7), "one two");
    }

    #[test]
    fn short_state_is_left_alone() {
        let (out, cut) = truncate_middle("hello", 4_000);
        assert_eq!(out, "hello");
        assert!(!cut);
    }

    #[test]
    fn long_state_is_cut_from_the_middle_and_says_so() {
        let state = format!("START{}END", "x".repeat(100_000));
        let (out, cut) = truncate_middle(&state, 4_000);
        assert!(cut);
        assert!(out.starts_with("START"));
        assert!(out.ends_with("END"));
        assert!(out.contains("characters left out"));
        assert!(out.chars().count() < (4_000usize) * CHARS_PER_TOKEN);
    }

    #[test]
    fn a_tiny_cap_still_leaves_room_for_something() {
        let (out, cut) = truncate_middle(&"y".repeat(10_000), 10);
        assert!(cut);
        assert!(out.chars().count() >= 256);
    }

    // ---- `ask` itself, against a mock app ----

    fn mock_app() -> (tauri::App<tauri::test::MockRuntime>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        app.manage(Db::open_in_memory().unwrap());
        app.manage(RuntimeManager::new(dir.path().to_path_buf()));
        app.manage(DecideManager::new());
        (app, dir)
    }

    fn one_question() -> sites::SiteAsk {
        sites::sufficiency("what is the refund window?", "Refunds within 30 days.")
    }

    /// With no decision model installed, every purpose is unavailable, and each
    /// site then keeps its old path. Nothing is started.
    #[tokio::test]
    async fn without_a_model_every_purpose_is_unavailable() {
        let (app, _dir) = mock_app();
        let site = one_question();
        for purpose in policy::ALL {
            let got = ask_with(app.handle(), &site.as_ask(), purpose, Opts::default()).await;
            assert_eq!(got.unwrap_err(), DecideError::Unavailable, "{}", purpose.name());
        }
        assert!(!app.state::<DecideManager>().is_warm());
    }

    /// `DEC-T-5`: with the cloud chosen globally, a background purpose still
    /// resolves to this computer, and with no local engine it is unavailable,
    /// never sent anywhere else. (There is no cloud backend yet, so this also
    /// pins that choosing it does not change the answer.)
    #[tokio::test]
    async fn a_background_purpose_with_no_local_engine_is_unavailable_even_when_the_cloud_is_chosen() {
        let (app, _dir) = mock_app();
        app.state::<Db>().set_setting("decide.backend", "cloud").unwrap();
        let site = sites::scope("name", "desc", "body");
        let got = ask_with(app.handle(), &site.as_ask(), Purpose::MemScope, Opts::default()).await;
        assert_eq!(got.unwrap_err(), DecideError::Unavailable);
    }

    // ---- the cloud, against a server that answers once ----

    /// A reply for a one-question ask: the yes-or-no of the sufficiency check.
    fn cloud_reply(site: &sites::SiteAsk, p: f64) -> String {
        serde_json::json!({
            "id": "dec_1",
            "model": "cloudflare/clef-flash",
            "provider": "DeepInfra",
            "answers": { site.id: { "type": "noul", "noul": p } },
            "usage": { "input_tokens": 120, "output_tokens": 0, "cost": 0.000_004 },
        })
        .to_string()
    }

    fn cloud_db(app: &tauri::App<tauri::test::MockRuntime>, base: &str) {
        let db = app.state::<Db>();
        db.set_setting(cloud::BACKEND_KEY, "cloud").unwrap();
        db.set_setting("decide.cloud.url", base).unwrap();
        db.set_setting("decide.cloud.test_key", "sk-or-test").unwrap();
    }

    /// `DEC-5`: with the cloud chosen, a purpose someone is waiting on is answered
    /// by it, with no local engine at all, and the answer is logged with who
    /// served it and what it cost.
    #[tokio::test]
    async fn a_chosen_cloud_answers_with_no_local_engine_and_the_cost_is_kept() {
        let _turn = cloud::test_lock();
        let (app, _dir) = mock_app();
        let site = one_question();
        let (base, seen) = cloud::tests::serve_once(200, cloud_reply(&site, 0.93)).await;
        cloud_db(&app, &base);
        let got = ask_with(app.handle(), &site.as_ask(), Purpose::RetSufficiency, Opts::default()).await.expect("answered");
        assert_eq!(got.backend.label(), "openrouter:cloudflare/clef-flash@DeepInfra");
        assert!(got.backend.is_cloud());
        assert!(!got.calibrated, "a cloud answer is advisory until a fit backs it");
        assert!(matches!(got.get(site.id), Some(Answer::Noul { p_true }) if (*p_true - 0.93).abs() < 1e-6));
        assert!(seen.await.unwrap().contains("\"zdr\":true"));

        let db = app.state::<Db>();
        let rows = db.decisions_recent(5, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, "cloudflare/clef-flash", "keyed by the pinned model, not a file");
        assert_eq!(rows[0].backend, "openrouter:cloudflare/clef-flash@DeepInfra");
        let usage = db.decision_usage_since(0).unwrap();
        assert_eq!((usage[0].backend.as_str(), usage[0].count), ("openrouter", 1));
        assert!((usage[0].cost_usd - 0.000_004).abs() < 1e-12);
    }

    /// Decision 12 / `DEC-T-5`: a background purpose is not sent out by the global
    /// choice, and goes only when its own override says so.
    #[tokio::test]
    async fn only_its_own_override_sends_a_background_purpose_to_the_cloud() {
        let _turn = cloud::test_lock();
        let (app, _dir) = mock_app();
        let site = sites::scope("units", "use metric", "Always give distances in kilometres.");
        let reply = serde_json::json!({
            "model": "cloudflare/clef-flash",
            "answers": { site.id: {
                "type": "choice", "choice": "global",
                "probabilities": { "global": 0.9, "topical": 0.07, "unsure": 0.03 },
            } },
            "usage": { "input_tokens": 90 },
        })
        .to_string();
        let (base, _seen) = cloud::tests::serve_once(200, reply).await;
        cloud_db(&app, &base);
        let none = ask_with(app.handle(), &site.as_ask(), Purpose::MemScope, Opts::default()).await;
        assert_eq!(none.unwrap_err(), DecideError::Unavailable, "global cloud, background purpose, no local engine");

        app.state::<Db>().set_setting(&policy::override_key(Purpose::MemScope), "cloud").unwrap();
        let got = ask_with(app.handle(), &site.as_ask(), Purpose::MemScope, Opts::default()).await.expect("answered");
        assert!(got.backend.is_cloud());
        assert_eq!(app.state::<Db>().decision_usage_since(0).unwrap()[0].cost_usd, 0.0, "no cost said, none kept");
    }

    /// `DEC-5`: out of credits is told once, this call goes to this computer (here
    /// there is none, so it is unavailable and the site keeps its old path), and
    /// the cloud is not asked again until the stop ends.
    #[tokio::test]
    async fn out_of_credits_falls_back_tells_once_and_is_not_asked_again() {
        let _turn = cloud::test_lock();
        cloud::forget_faults();
        let (app, _dir) = mock_app();
        let site = one_question();
        let (base, _seen) = cloud::tests::serve_once(402, "{}".into()).await;
        cloud_db(&app, &base);
        let first = ask_with(app.handle(), &site.as_ask(), Purpose::RetSufficiency, Opts::default()).await;
        assert_eq!(first.unwrap_err(), DecideError::Unavailable);
        let db = app.state::<Db>();
        let status = cloud::status(&db);
        assert_eq!(status.state, "credits");
        let told = db.list_activity(10).unwrap();
        assert_eq!(told.iter().filter(|a| a.detail.contains("Out of OpenRouter credits")).count(), 1);
        assert!(crate::cloud::last_error("openrouter").unwrap().contains("Out of credits"), "the Providers card says so too");

        // The server is gone now; a second ask must not even try it.
        let again = ask_with(app.handle(), &site.as_ask(), Purpose::RetSufficiency, Opts::default()).await;
        assert_eq!(again.unwrap_err(), DecideError::Unavailable);
        assert_eq!(db.list_activity(10).unwrap().iter().filter(|a| a.detail.contains("Out of OpenRouter credits")).count(), 1);
        cloud::forget_faults();
    }

    /// A request the service refuses is a bug to see, not a reason to ask somewhere
    /// else: `Failed`, and the cloud stays usable.
    #[tokio::test]
    async fn a_refused_request_is_failed_and_does_not_stop_the_cloud() {
        let _turn = cloud::test_lock();
        cloud::forget_faults();
        let (app, _dir) = mock_app();
        let site = one_question();
        let (base, _seen) = cloud::tests::serve_once(400, "state is not valid".into()).await;
        cloud_db(&app, &base);
        let got = ask_with(app.handle(), &site.as_ask(), Purpose::RetSufficiency, Opts::default()).await;
        assert!(matches!(got, Err(DecideError::Failed(m)) if m.contains("state is not valid")));
        assert_eq!(cloud::status(&app.state::<Db>()).state, "ready");
    }

    /// A purpose someone is waiting on does not fall back to this computer when the
    /// cloud is slow: it misses its deadline (800 ms, not the local 300) and the
    /// caller carries on. A slow answer is not a reason to stop using the cloud.
    #[tokio::test]
    async fn a_hot_purpose_that_the_cloud_is_too_slow_for_misses_its_deadline() {
        let _turn = cloud::test_lock();
        cloud::forget_faults();
        let (app, _dir) = mock_app();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let _hold = tokio::spawn(async move {
            let _keep = listener.accept().await;
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        });
        cloud_db(&app, &base);
        let site = one_question();
        let hot = policy::Policy { experiment: None, ..Purpose::SteerClass.policy() };
        assert!(hot.needs_fast && hot.deadline < std::time::Duration::from_millis(800));
        let started = Instant::now();
        let got = ask_under(app.handle(), &site.as_ask(), Purpose::SteerClass, Opts::default(), hot).await;
        assert_eq!(got.unwrap_err(), DecideError::Deadline);
        let took = started.elapsed();
        assert!(took >= std::time::Duration::from_millis(700) && took < std::time::Duration::from_secs(3), "{took:?}");
        assert_eq!(cloud::status(&app.state::<Db>()).state, "ready");
    }

    /// `SEE-3`: with a cloud decision model chosen, a message being typed is read there, so a
    /// computer with no local engine still gets hints. The answer is held, not logged: only a
    /// hint the person acts on is written, and then without the words of the draft.
    #[tokio::test]
    async fn typed_words_are_read_by_the_chosen_cloud_and_kept_out_of_the_log() {
        let _turn = cloud::test_lock();
        cloud::forget_faults();
        let (app, _dir) = mock_app();
        let site = one_question();
        let (base, _seen) = cloud::tests::serve_once(200, cloud_reply(&site, 0.93)).await;
        cloud_db(&app, &base);
        let typing = policy::Policy { experiment: None, ..Purpose::RetSufficiency.typing_policy() };
        let verdict = ask_under(app.handle(), &site.as_ask(), Purpose::RetSufficiency, Opts::default(), typing)
            .await
            .expect("the cloud answers a typed message");
        let db = app.state::<Db>();
        assert!(db.decisions_recent(10, None).unwrap().is_empty(), "nothing is logged while typing");
        typing::log_shown(&db, &verdict.id);
        let rows = db.decisions_recent(10, None).unwrap();
        assert_eq!(rows.len(), 1, "an acted-on hint is written");
        assert_eq!(rows[0].state_preview, typing::TYPED_PREVIEW, "without the draft's words");
        assert!(rows[0].conversation_id.is_none());
        cloud::forget_faults();
    }

    /// `SEE-1`: the automatic check of an answer (which carries its run) and a deep check of the
    /// same chat count apart, so starting one never resets the other's claims and reviews.
    #[test]
    fn an_automatic_check_and_a_deep_check_count_apart() {
        let auto = Opts { conversation_id: Some("c"), run_id: Some("r") };
        let deep = Opts { conversation_id: Some("c"), run_id: None };
        assert_ne!(auto.count_key(Purpose::ClaimVerdict), deep.count_key(Purpose::ClaimVerdict));
        assert_eq!(auto.count_key(Purpose::ClaimCheckable).as_deref(), Some("c#auto-check"));
        assert_eq!(deep.count_key(Purpose::ClaimVerdict).as_deref(), Some("c"));
        // Every other decision of the run counts with its turn, as before.
        assert_eq!(auto.count_key(Purpose::ToolSelect).as_deref(), Some("c"));
        assert_eq!(Opts::default().count_key(Purpose::ClaimVerdict), None);

        let c = control::Controller::default();
        c.begin_check("c");
        for _ in 0..control::VERIFY_REVIEWS {
            c.review(deep.count_key(Purpose::ClaimVerdict).as_deref()).unwrap();
        }
        c.begin_check(auto.count_key(Purpose::ClaimVerdict).as_deref().unwrap());
        assert!(c.review(deep.count_key(Purpose::ClaimVerdict).as_deref()).is_err(), "the deep check's count stands");
        assert!(c.review(auto.count_key(Purpose::ClaimVerdict).as_deref()).is_ok());
    }

    /// With the cloud chosen but no key, nothing is sent anywhere and the purpose is
    /// unavailable, so the site keeps its old path.
    #[tokio::test]
    async fn a_chosen_cloud_with_no_key_sends_nothing() {
        let _turn = cloud::test_lock();
        let (app, _dir) = mock_app();
        let site = one_question();
        let (base, seen) = cloud::tests::serve_once(200, cloud_reply(&site, 0.9)).await;
        cloud_db(&app, &base);
        // Take the key away again: the choice stays, the key does not.
        app.state::<Db>().set_setting("decide.cloud.test_key", "").unwrap();
        let got = ask_with(app.handle(), &site.as_ask(), Purpose::RetSufficiency, Opts::default()).await;
        assert_eq!(got.unwrap_err(), DecideError::Unavailable);
        assert!(!seen.is_finished(), "the server was never reached");
    }

    /// A library row whose file is gone is the same as no model at all.
    #[tokio::test]
    async fn a_model_row_whose_file_is_missing_is_unavailable() {
        let (app, dir) = mock_app();
        app.state::<Db>()
            .add_model_with_role(
                &crate::db::NewModelEntry {
                    name: "Clef-flash".into(),
                    path: dir.path().join("gone.gguf").to_string_lossy().to_string(),
                    quant: None,
                    size_bytes: None,
                    vision: true,
                },
                "decide",
            )
            .unwrap();
        let site = one_question();
        let got = ask_with(app.handle(), &site.as_ask(), Purpose::RetSufficiency, Opts::default()).await;
        assert_eq!(got.unwrap_err(), DecideError::Unavailable);
    }

    /// The model is there but the shared engine binary is not: still unavailable.
    #[tokio::test]
    async fn a_model_without_the_engine_binary_is_unavailable() {
        let (app, dir) = mock_app();
        let file = dir.path().join("Clef-Flash-Q4_K_M.gguf");
        std::fs::write(&file, b"not really a model").unwrap();
        app.state::<Db>()
            .add_model_with_role(
                &crate::db::NewModelEntry {
                    name: "Clef-flash".into(),
                    path: file.to_string_lossy().to_string(),
                    quant: None,
                    size_bytes: None,
                    vision: true,
                },
                "decide",
            )
            .unwrap();
        let site = one_question();
        let got = ask_with(app.handle(), &site.as_ask(), Purpose::GoalMet, Opts::default()).await;
        assert_eq!(got.unwrap_err(), DecideError::Unavailable);
    }

    /// A question the server could never answer is refused before anything runs.
    #[tokio::test]
    async fn a_malformed_ask_is_refused_before_anything_starts() {
        let (app, _dir) = mock_app();
        let empty = Ask { state: "s", images: Vec::new(), questions: Vec::new() };
        let got = ask_with(app.handle(), &empty, Purpose::GoalMet, Opts::default()).await;
        assert!(matches!(got, Err(DecideError::Failed(m)) if m.contains("at least one question")));
    }

    /// The whole path, for real: a hot purpose is skipped while the engine is
    /// cold and starts it for next time, the next ask is answered and logged, and
    /// a background purpose waits for the start instead of skipping. Needs the
    /// engine and the files, so it is opt-in:
    ///
    /// ```text
    /// DECIDE_SERVER_DIR=C:\path\to\folder-with-llama-server.exe \
    /// DECIDE_MODEL_PATH=C:\path\to\Clef-Flash-Q4_K_M.gguf \
    ///   cargo test --lib ask_end_to_end -- --ignored --nocapture
    /// ```
    ///
    /// The model file must keep its catalog name, so its projector (when it sits
    /// beside it) is found.
    #[tokio::test]
    #[ignore = "needs a local llama-server and a Clef-flash GGUF"]
    async fn ask_end_to_end_skips_cold_then_answers_and_logs() {
        let (Ok(server_dir), Ok(model)) = (std::env::var("DECIDE_SERVER_DIR"), std::env::var("DECIDE_MODEL_PATH"))
        else {
            panic!("set DECIDE_SERVER_DIR and DECIDE_MODEL_PATH");
        };
        let (app, dir) = mock_app();
        let db = app.state::<Db>();
        let mgr = app.state::<RuntimeManager>();

        // Put the engine where `engine_binary_path` looks for it on this machine.
        let hw = decideserver::hardware().await;
        let (selection, backend) = crate::commands::runtime::active_selection(&hw, &db);
        let target = mgr.runtimes_dir().join(&selection.build_tag).join(backend.kebab());
        std::fs::create_dir_all(&target).unwrap();
        for entry in std::fs::read_dir(&server_dir).unwrap().flatten() {
            if entry.path().is_file() {
                std::fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
            }
        }
        db.add_model_with_role(
            &crate::db::NewModelEntry {
                name: "Clef-flash".into(),
                path: model.clone(),
                quant: Some("Q4_K_M".into()),
                size_bytes: None,
                vision: true,
            },
            "decide",
        )
        .unwrap();
        // Hot purposes need the card; on a machine without one this test can only
        // show the processor refusing them.
        db.set_setting(decideserver::PLACEMENT_KEY, "gpu").unwrap();
        let on_card = hw.vram_mb() > 0;
        let resolved = decideserver::resolve_local(&mgr, &db).await;
        assert!(
            resolved.is_some(),
            "nothing resolved: binary {:?}, copied to {:?} ({:?}), vram {} MB, ram {} MB, gpu list {:?}",
            crate::commands::embedgen::engine_binary_path(&mgr, &db).await.ok().flatten(),
            target,
            std::fs::read_dir(&target).map(|d| d.flatten().count()),
            hw.vram_mb(),
            hw.ram_mb,
            hw.gpus.iter().map(|g| (&g.name, g.vram_mb)).collect::<Vec<_>>(),
        );

        let conv = db.create_conversation("t", None, false).unwrap().id;
        let site = sites::sufficiency("What is the refund window?", "Refunds are accepted within 30 days.");
        let opts = Opts { conversation_id: Some(&conv), run_id: None };
        let decide = app.state::<DecideManager>();

        if on_card {
            // Cold: skipped at once, and a start is under way.
            let cold = ask_with(app.handle(), &site.as_ask(), Purpose::RetSufficiency, opts).await;
            assert_eq!(cold.unwrap_err(), DecideError::Deadline);
            for _ in 0..240 {
                if decide.is_warm() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            assert!(
                decide.is_warm(),
                "the cold ask should have started the engine: {:?}",
                decide.status(Some("Clef-flash".into()))
            );
        } else {
            let cpu = ask_with(app.handle(), &site.as_ask(), Purpose::RetSufficiency, opts).await;
            assert_eq!(cpu.unwrap_err(), DecideError::Unavailable, "no card, so no hot-path decisions");
        }

        // A background purpose waits for the start and is answered either way.
        let scope = sites::scope("units", "use metric", "Always give distances in kilometres.");
        let v = ask_with(app.handle(), &scope.as_ask(), Purpose::MemScope, opts).await.expect("answered");
        assert!(!v.calibrated);
        assert!(v.ms > 0);
        // What the model thinks of a fact is not this test's business; that the
        // answer is read by the rule (unsure or below 0.6 stays unclassified) is.
        eprintln!("scope answer: {:?}", v.answers);
        let Some(Answer::Choice { choice, confidence, probs }) = v.get("scope") else { panic!("not a choice") };
        assert_eq!(probs.len(), 3);
        let kept = sites::scope_from(&v);
        assert_eq!(
            kept.is_some(),
            choice != "unsure" && *confidence >= Purpose::MemScope.policy().act_at.at(false),
            "{:?}",
            v.answers
        );

        if on_card {
            // The engine in this test is usually the CPU build, which cannot make
            // the 5 second deadline of a hot purpose; that is the deadline doing
            // its job, so a late answer is accepted and a timely one is checked.
            match ask_with(app.handle(), &site.as_ask(), Purpose::RetSufficiency, opts).await {
                Ok(hot) => assert!(sites::sufficiency_from(&hot).unwrap().enough),
                Err(DecideError::Deadline) => eprintln!("the hot ask missed its deadline (a CPU build is slow)"),
                Err(other) => panic!("unexpected: {other:?}"),
            }
        }

        // Each answer is in the session log, with a fingerprint of its state.
        let rows = db.session_events(&conv).unwrap();
        let logged: Vec<_> = rows.iter().filter(|r| r.kind == crate::agent::log::DECISION_KIND).collect();
        assert!(!logged.is_empty());
        assert!(logged.iter().all(|r| r.payload_json.contains("state_hash")));
        // And in the decisions table, which also holds conversation-less ones.
        let stored = db.decisions_recent(20, None).unwrap();
        assert!(stored.len() >= logged.len());
        assert!(stored.iter().all(|d| d.model.starts_with("Clef-Flash")), "keyed by the file stem");
        assert!(decide.status(Some("Clef-flash".into())).median_ms.is_some());
        decide.stop().await;
        drop(dir);
    }

    #[tokio::test]
    async fn without_the_global_handle_the_public_ask_is_unavailable() {
        // `init` is never called in the library tests, so the public entry point
        // has no app to reach, which is the state of a test harness or an eval.
        let site = one_question();
        let got = ask(&site.as_ask(), Purpose::RetSufficiency, Opts::default()).await;
        assert_eq!(got.unwrap_err(), DecideError::Unavailable);
    }

    fn noul_verdict(p: f32) -> Verdict {
        Verdict {
            id: "v".into(),
            answers: BTreeMap::from([("enough".to_string(), Answer::Noul { p_true: p })]),
            backend: BackendId::Local { model: "Clef-flash".into() },
            calibrated: false,
            ms: 1,
        }
    }

    /// `CTL-4`: a good fit for this model rescales the answer and marks it
    /// calibrated; nothing else about the verdict moves.
    #[test]
    fn a_good_fit_rescales_the_answer_and_marks_it_calibrated() {
        let db = Db::open_in_memory().unwrap();
        calibration::save(&db, "Clef-Flash-Q4_K_M", "ret-sufficiency", &calibration::Fit { t: 2.0, ece: 0.03, n: 50, fitted_at: 1 });
        let mut v = noul_verdict(0.95);
        calibrate(&db, "Clef-Flash-Q4_K_M", Purpose::RetSufficiency, &mut v);
        assert!(v.calibrated);
        let Some(Answer::Noul { p_true }) = v.get("enough") else { panic!() };
        assert!(*p_true < 0.95 && *p_true > 0.5, "{p_true}");
        assert_eq!((v.id.as_str(), v.ms), ("v", 1));
    }

    /// A fit that is off, too small, for another model or another purpose
    /// changes nothing: the numbers stay the model's own.
    #[test]
    fn a_poor_or_foreign_fit_leaves_the_answer_alone() {
        let db = Db::open_in_memory().unwrap();
        let fit = |ece, n| calibration::Fit { t: 2.0, ece, n, fitted_at: 1 };
        calibration::save(&db, "Clef-Flash-Q4_K_M", "ret-sufficiency", &fit(0.2, 80));
        calibration::save(&db, "Clef-Flash-Q4_K_M", "goal-met", &fit(0.02, 10));
        calibration::save(&db, "Clef-Flash-Q4_K_M", "mem-scope", &fit(0.02, 80));
        for (model, purpose) in [
            ("Clef-Flash-Q4_K_M", Purpose::RetSufficiency),
            ("Clef-Flash-Q4_K_M", Purpose::GoalMet),
            ("Another-Model", Purpose::MemScope),
            ("Clef-Flash-Q4_K_M", Purpose::HandoverRoute),
        ] {
            let mut v = noul_verdict(0.95);
            calibrate(&db, model, purpose, &mut v);
            assert!(!v.calibrated, "{model} {}", purpose.name());
            assert_eq!(v, noul_verdict(0.95));
        }
    }

    #[test]
    fn the_state_hash_is_stable_and_differs_by_state() {
        assert_eq!(state_hash("a"), state_hash("a"));
        assert_ne!(state_hash("a"), state_hash("b"));
        assert_eq!(state_hash("a").len(), 16);
    }
}
