//! `CTL-4`: the labelled sets a calibration is fitted on, and the run that
//! measures them.
//!
//! `tests/eval/decide/<purpose>.jsonl` holds 40 hand-labelled rows per purpose.
//! They are compiled in, so "Calibrate now" works in a shipped app. A run asks
//! the engine every row, keeps the model's own probabilities next to what was
//! true, and fits one temperature from them plus the outcome labels
//! (`outcomes`). The run bypasses the controller and the log: it measures the
//! model, it is not a decision anybody acts on.

use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::calibration::{self, Fit, Obs, Truth};
use super::policy::{Purpose, ALL};
use super::sites::{self, SiteAsk};
use super::{checkable, claims, filters, loopctl, mailtriage, pagecheck, runif, toolgate, tools};
use super::{outcomes, Answer, DecideError};
use crate::agent::classify;
use crate::db::Db;
use crate::runtime::decideserver::{self, DecideManager};
use crate::runtime::RuntimeManager;

/// One request may take this long while calibrating. A computer with no card
/// takes about 12 s a row, so the hot-path deadlines would fail every row.
const ROW_DEADLINE: Duration = Duration::from_secs(180);
/// A weekly refit needs this many new labels.
pub const REFIT_LABELS: u32 = 50;
const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

fn file_for(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::RetSufficiency => include_str!("../../../tests/eval/decide/ret-sufficiency.jsonl"),
        Purpose::MemScope => include_str!("../../../tests/eval/decide/mem-scope.jsonl"),
        Purpose::ReflectCritic => include_str!("../../../tests/eval/decide/reflect-critic.jsonl"),
        Purpose::GoalMet => include_str!("../../../tests/eval/decide/goal-met.jsonl"),
        Purpose::HandoverRoute => include_str!("../../../tests/eval/decide/handover-route.jsonl"),
        Purpose::ToolSelect => include_str!("../../../tests/eval/decide/tool-select.jsonl"),
        Purpose::ReflectGate => include_str!("../../../tests/eval/decide/reflect-gate.jsonl"),
        Purpose::UntrustedFlag => include_str!("../../../tests/eval/decide/untrusted-flag.jsonl"),
        Purpose::Classify => include_str!("../../../tests/eval/decide/classify.jsonl"),
        Purpose::PlanDone => include_str!("../../../tests/eval/decide/plan-done.jsonl"),
        Purpose::SteerClass => include_str!("../../../tests/eval/decide/steer-class.jsonl"),
        Purpose::UnverifiedClaim => include_str!("../../../tests/eval/decide/unverified-claim.jsonl"),
        Purpose::RunIf => include_str!("../../../tests/eval/decide/run-if.jsonl"),
        Purpose::MailTriage => include_str!("../../../tests/eval/decide/mail-triage.jsonl"),
        Purpose::ClaimVerdict => include_str!("../../../tests/eval/decide/claim-verdict.jsonl"),
        Purpose::ClaimCheckable => include_str!("../../../tests/eval/decide/claim-checkable.jsonl"),
        Purpose::BrowserCheck => include_str!("../../../tests/eval/decide/browser-check.jsonl"),
        Purpose::ToolRisk => include_str!("../../../tests/eval/decide/tool-risk.jsonl"),
        Purpose::AppKind => include_str!("../../../tests/eval/decide/app-kind.jsonl"),
    }
}

/// The rules the routing seed asks about, the way `route_options` words them.
const ROUTE_RULES: [(&str, &str); 3] = [
    ("claude-code:U1", "Hand it to Claude Code: long changes that touch many files in a project"),
    ("claude-code:U2", "Hand it to Claude Code: refactors and writing tests"),
    ("codex:U1", "Hand it to Codex: quick one-off scripts and small fixes"),
];
const ROUTE_AVOID: &str = "Do not hand it to Claude Code for: questions about my files, mail or calendar";

/// One labelled row, ready to ask: the question, and which option is true.
pub struct SeedRow {
    pub id: String,
    pub site: SiteAsk,
    /// Index into the question's options (a yes-or-no question has `true` first).
    pub truth: usize,
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

fn option_ids(site: &SiteAsk) -> Vec<String> {
    match &site.question {
        super::Question::Choice { options, .. } => options.iter().map(|(id, _)| id.clone()).collect(),
        super::Question::Noul { .. } => vec!["yes".into(), "no".into()],
        super::Question::Score { levels, .. } => levels.clone(),
    }
}

fn row(purpose: Purpose, v: &Value) -> Option<SeedRow> {
    let site = match purpose {
        Purpose::RetSufficiency => sites::sufficiency(text(v, "query")?, text(v, "excerpts")?),
        Purpose::MemScope => sites::scope(text(v, "name")?, text(v, "description")?, text(v, "body")?),
        Purpose::ReflectCritic => sites::critic(text(v, "transcript")?, text(v, "name")?, text(v, "body")?),
        Purpose::GoalMet => sites::goal(crate::agent::goal::decision_state(
            text(v, "objective")?,
            text(v, "until")?,
            text(v, "answer")?,
            text(v, "changes")?,
        )),
        Purpose::HandoverRoute => {
            let mut options: Vec<(String, String)> =
                ROUTE_RULES.iter().map(|(id, t)| ((*id).to_string(), (*t).to_string())).collect();
            options.push((crate::engines::rules::ROUTE_NONE.into(), crate::engines::rules::ROUTE_NONE_TEXT.into()));
            options.push((crate::engines::rules::ROUTE_UNSURE.into(), crate::engines::rules::ROUTE_UNSURE_TEXT.into()));
            sites::route(text(v, "request")?, options, &[ROUTE_AVOID.to_string()])
        }
        Purpose::ToolSelect => {
            // One group a row, so the model's answer for it is measured without
            // the others in the prompt (`ablation` asks them all together).
            let unit = super::ablation::unit(text(v, "unit")?)?;
            let id = super::ablation::CATALOGUE.iter().find(|(i, ..)| *i == unit.id)?.0;
            SiteAsk {
                state: tools::ask_for(text(v, "request")?, "", std::slice::from_ref(&unit))?.state,
                id,
                question: tools::question(&unit),
            }
        }
        Purpose::ReflectGate => filters::reflect_gate_ask(text(v, "transcript")?),
        Purpose::UntrustedFlag => filters::intake_ask(text(v, "text")?),
        // A row names its own question, so the set covers pick-one questions of
        // two to three answers and yes or no, the way the tool is called.
        Purpose::Classify => {
            let options = match v.get("options") {
                None => None,
                Some(list) => Some(
                    list.as_array()?
                        .iter()
                        .map(|o| Some((text(o, "id")?.to_string(), text(o, "criteria")?.to_string())))
                        .collect::<Option<Vec<_>>>()?,
                ),
            };
            classify::seed_ask(text(v, "question")?, options, text(v, "item")?)
        }
        Purpose::PlanDone => loopctl::plan_done_ask(text(v, "item")?, text(v, "answer")?),
        Purpose::SteerClass => loopctl::steer_ask(text(v, "task")?, text(v, "message")?),
        Purpose::UnverifiedClaim => loopctl::claim_ask(text(v, "answer")?),
        Purpose::RunIf => runif::condition_ask(text(v, "condition")?, text(v, "input")?),
        // The kind alone, over the default kinds: the live check adds the reply and
        // urgency questions to the same request.
        Purpose::MailTriage => mailtriage::seed_ask(text(v, "from")?, text(v, "subject")?, text(v, "body")?),
        // A claim and the passages found for it, as a live check shows them.
        Purpose::ClaimVerdict => {
            let passages = v
                .get("evidence")?
                .as_array()?
                .iter()
                .map(|p| Some((text(p, "source")?.to_string(), text(p, "text")?.to_string())))
                .collect::<Option<Vec<_>>>()?;
            claims::claim_ask(text(v, "claim")?, &claims::render_evidence(&passages))
        }
        // One sentence and the question the row names: whether it can be checked, or
        // whether it is a fact or something the assistant did. The live request asks both.
        Purpose::ClaimCheckable => {
            let state = checkable::state(text(v, "sentence")?);
            match text(v, "ask")? {
                "checkable" => SiteAsk { state, id: checkable::CHECKABLE_ID, question: checkable::checkable_question() },
                "kind" => SiteAsk { state, id: checkable::KIND_ID, question: checkable::kind_question() },
                _ => return None,
            }
        }
        // The page as text alone: the live check adds the picture. A seed run has no
        // screenshots, so this measures how the model reads the words of a page.
        Purpose::BrowserCheck => SiteAsk {
            state: pagecheck::ask_state(text(v, "host")?, text(v, "path")?, text(v, "title")?, text(v, "text")?),
            id: pagecheck::PAGE_ID,
            question: pagecheck::question(),
        },
        // One question a row, which the row names: the rating or whether the call was
        // asked for. The live check asks both in one request.
        Purpose::ToolRisk => {
            let state = toolgate::own_state(text(v, "request")?, text(v, "tool")?, v.get("args")?);
            match text(v, "ask")? {
                "risk" => SiteAsk { state, id: toolgate::RISK_ID, question: toolgate::risk_question() },
                "scope" => SiteAsk { state, id: toolgate::SCOPE_ID, question: toolgate::scope_question() },
                _ => return None,
            }
        }
        // The name and what the tool's maker says it does, as the live check shows them.
        Purpose::AppKind => SiteAsk {
            state: toolgate::kind_state(text(v, "app")?, text(v, "tool")?, text(v, "description")?),
            id: toolgate::KIND_ID,
            question: toolgate::kind_question(),
        },
    };
    let expect = text(v, "expect")?;
    let truth = option_ids(&site).iter().position(|id| id == expect)?;
    Some(SeedRow { id: text(v, "id")?.to_string(), site, truth })
}

/// Every row of one purpose's seed set. A row that does not parse is dropped,
/// and the test below fails if any was.
pub fn rows(purpose: Purpose) -> Vec<SeedRow> {
    file_for(purpose)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| row(purpose, &v))
        .collect()
}

/// An answer as probabilities in the question's option order, which is the form
/// the fit works on.
fn probs_in_order(answer: &Answer) -> Vec<f32> {
    match answer {
        Answer::Noul { p_true } => vec![*p_true, 1.0 - *p_true],
        Answer::Choice { probs, .. } => probs.iter().map(|(_, p)| *p).collect(),
        Answer::Score { probs, .. } => probs.clone(),
    }
}

/// The key a model's fits are stored under: its file stem.
pub fn model_key(db: &Db) -> Option<String> {
    let model = db.default_model_by_role("decide").ok()??;
    std::path::Path::new(&model.path).file_stem().map(|s| s.to_string_lossy().to_string())
}

/// Ask the engine every row of one purpose. `progress(done, total)` is called
/// after each row. Returns the observations (the model's raw probabilities and
/// the true option). A row the engine fails is skipped; a wholly failed run is
/// an error.
pub async fn measure<R: tauri::Runtime>(
    app: &AppHandle<R>,
    purpose: Purpose,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<Obs>, DecideError> {
    let db = app.state::<Db>();
    let mgr = app.state::<RuntimeManager>();
    let decide = app.state::<DecideManager>();
    let local = decideserver::resolve_local(&mgr, &db).await.ok_or(DecideError::Unavailable)?;
    let live = decide.ensure_started(&mgr.client, &local).await.map_err(|e| DecideError::Failed(e.to_string()))?;
    let cap = purpose.policy().state_cap_tokens.min(live.max_state_tokens);

    let seed = rows(purpose);
    let total = seed.len();
    let mut obs = Vec::new();
    let mut last_error = None;
    for (i, r) in seed.iter().enumerate() {
        let ask = r.site.as_ask();
        let (state, _) = super::truncate_middle(ask.state, cap);
        let body = super::local::build_body(&ask, &state);
        let got = super::local::post(&mgr.client, &live.base_url, &live.token, &body, ROW_DEADLINE).await;
        match got.and_then(|reply| super::local::parse_response(&ask, &reply).map_err(DecideError::Failed)) {
            Ok((answers, _)) => {
                if let Some(answer) = answers.get(r.site.id) {
                    obs.push(Obs { probs: probs_in_order(answer), truth: Truth::Class(r.truth), weight: 1.0 });
                }
            }
            Err(e) => last_error = Some(e),
        }
        progress(i + 1, total);
    }
    if obs.is_empty() {
        return Err(last_error.unwrap_or(DecideError::Failed("the engine answered no rows".into())));
    }
    Ok(obs)
}

/// What a fit says, for the screen.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Report {
    pub purpose: &'static str,
    pub label: &'static str,
    pub fit: Option<Fit>,
    pub calibrated: bool,
    /// The error before the fit, to say "was 0.21, now 0.05".
    pub ece_before: Option<f32>,
    /// Labels written since the fit, which a refit would use.
    pub new_labels: u32,
}

/// Fit one (model, purpose) from the saved seed run and the outcome labels, and
/// keep the result. `None` when there is nothing to fit on.
pub fn refit(db: &Db, model: &str, purpose: Purpose, now: i64) -> Option<(Fit, f32)> {
    let mut obs = calibration::load_seed(db, model, purpose.name());
    obs.extend(outcomes::observations(db, purpose.name(), model));
    let fit = calibration::fit(&obs, now)?;
    let before = calibration::ece_before(&obs);
    calibration::save(db, model, purpose.name(), &fit);
    Some((fit, before))
}

pub fn report(db: &Db, model: &str, purpose: Purpose) -> Report {
    let fit = calibration::load(db, model, purpose.name());
    let new_labels = fit.map_or(0, |f| db.outcomes_since(Some(purpose.name()), Some(model), f.fitted_at).unwrap_or(0));
    Report {
        purpose: purpose.name(),
        label: purpose.label(),
        fit,
        calibrated: fit.is_some_and(|f| f.is_calibrated()),
        ece_before: None,
        new_labels,
    }
}

pub fn reports(db: &Db) -> Vec<Report> {
    let Some(model) = model_key(db) else { return Vec::new() };
    ALL.iter().map(|p| report(db, &model, *p)).collect()
}

/// "Calibrate now": run every seed set through the engine and fit each purpose.
/// `progress(purpose, done, total)` after each row.
pub async fn calibrate_now<R: tauri::Runtime>(
    app: &AppHandle<R>,
    mut progress: impl FnMut(Purpose, usize, usize),
) -> Result<Vec<Report>, DecideError> {
    let db = app.state::<Db>();
    let model = model_key(&db).ok_or(DecideError::Unavailable)?;
    let mut out = Vec::new();
    for purpose in ALL {
        let obs = measure(app, purpose, |done, total| progress(purpose, done, total)).await?;
        calibration::save_seed(&db, &model, purpose.name(), &obs);
        let mut r = report(&db, &model, purpose);
        if let Some((fit, before)) = refit(&db, &model, purpose, crate::db::now_ms()) {
            r.fit = Some(fit);
            r.calibrated = fit.is_calibrated();
            r.ece_before = Some(before);
            r.new_labels = 0;
        }
        out.push(r);
    }
    Ok(out)
}

/// Whether a weekly refit is due for one purpose: a fit exists, it is a week
/// old, and enough labels have come in since.
pub fn refit_due(db: &Db, model: &str, purpose: Purpose, now: i64) -> bool {
    let Some(fit) = calibration::load(db, model, purpose.name()) else { return false };
    now - fit.fitted_at >= WEEK_MS
        && db.outcomes_since(Some(purpose.name()), Some(model), fit.fitted_at).unwrap_or(0) >= REFIT_LABELS
}

/// Refit whatever is due. Pure arithmetic over rows already stored, so it needs
/// neither the engine nor the card and never waits behind a running chat turn.
pub fn refit_due_now(db: &Db, now: i64) -> Vec<Purpose> {
    let Some(model) = model_key(db) else { return Vec::new() };
    ALL.iter()
        .copied()
        .filter(|p| refit_due(db, &model, *p, now))
        .filter(|p| refit(db, &model, *p, now).is_some())
        .collect()
}

/// Check once ten minutes after start and every six hours after.
pub fn start_refits<R: tauri::Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(600)).await;
        loop {
            let refitted = refit_due_now(&app.state::<Db>(), crate::db::now_ms());
            if !refitted.is_empty() {
                let names: Vec<&str> = refitted.iter().map(|p| p.name()).collect();
                let _ = app.state::<Db>().log_activity(None, "decide", &format!("Refitted the decision calibration: {}", names.join(", ")));
            }
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::decisions::NewDecision;
    use std::collections::HashMap;

    /// `CTL-4`: 40 to 100 rows each, every row readable, every label one of the
    /// question's own options, and no purpose lopsided enough to fool a fit.
    #[test]
    fn every_seed_set_has_forty_readable_rows_with_real_labels() {
        for purpose in ALL {
            let lines = file_for(purpose).lines().filter(|l| !l.trim().is_empty()).count();
            let parsed = rows(purpose);
            assert_eq!(parsed.len(), lines, "{}: a row did not parse or its label is not an option", purpose.name());
            assert!((40..=100).contains(&parsed.len()), "{}: {} rows", purpose.name(), parsed.len());
            let mut ids: Vec<&str> = parsed.iter().map(|r| r.id.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), parsed.len(), "{}: duplicate ids", purpose.name());

            let mut per_class: HashMap<usize, usize> = HashMap::new();
            for r in &parsed {
                *per_class.entry(r.truth).or_default() += 1;
            }
            let biggest = per_class.values().copied().max().unwrap();
            assert!(biggest * 100 <= parsed.len() * 60, "{}: one answer is {} of {}", purpose.name(), biggest, parsed.len());
            assert!(per_class.len() >= 2);
        }
    }

    /// Every row builds a question the server accepts, and a state that fits.
    #[test]
    fn every_seed_row_builds_a_valid_ask() {
        for purpose in ALL {
            for r in rows(purpose) {
                assert!(super::super::local::check_ask(&r.site.as_ask()).is_ok(), "{} {}", purpose.name(), r.id);
            }
        }
    }

    #[test]
    fn a_yes_is_the_first_class_of_a_yes_or_no_question() {
        let rows = rows(Purpose::GoalMet);
        let yes = rows.iter().find(|r| r.id == "goal-01").unwrap();
        let no = rows.iter().find(|r| r.id == "goal-02").unwrap();
        assert_eq!((yes.truth, no.truth), (0, 1));
        // And that is how an answer's probabilities are laid out.
        let laid_out = probs_in_order(&Answer::Noul { p_true: 0.8 });
        assert!((laid_out[0] - 0.8).abs() < 1e-6 && (laid_out[1] - 0.2).abs() < 1e-6);
    }

    /// The routing seed carries the real `none` and `unsure` options.
    #[test]
    fn the_routing_seed_ends_with_none_and_unsure() {
        let r = rows(Purpose::HandoverRoute);
        let ids = option_ids(&r[0].site);
        assert_eq!(&ids[ids.len() - 2..], ["none", "unsure"]);
    }

    fn seed_db(model: &str, purpose: Purpose, calibrated_looking: bool) -> Db {
        let db = Db::open_in_memory().unwrap();
        // 60 yes-or-no answers at 0.95 of which 45 were right.
        let obs: Vec<Obs> = (0..60)
            .map(|i| Obs {
                probs: vec![0.95, 0.05],
                truth: Truth::Class(if i < 45 { 0 } else { 1 }),
                weight: 1.0,
            })
            .collect();
        calibration::save_seed(&db, model, purpose.name(), &obs);
        if calibrated_looking {
            calibration::save(&db, model, purpose.name(), &Fit { t: 1.0, ece: 0.2, n: 60, fitted_at: 0 });
        }
        db
    }

    /// `CTL-T-2`: the saved seed run and the labels fit together.
    #[test]
    fn a_refit_uses_the_saved_seed_run_and_the_labels() {
        let db = seed_db("Clef-Flash-Q4_K_M", Purpose::GoalMet, false);
        let (fit, before) = refit(&db, "Clef-Flash-Q4_K_M", Purpose::GoalMet, 5).unwrap();
        assert_eq!(fit.n, 60);
        assert!(fit.t > 1.5, "overconfident, so flattened: {}", fit.t);
        assert!(fit.ece < before);
        assert!(fit.is_calibrated(), "ece {}", fit.ece);
        assert_eq!(calibration::load(&db, "Clef-Flash-Q4_K_M", "goal-met"), Some(fit));

        // A label from behaviour joins the next fit.
        let answers = serde_json::json!({ "met": { "p": 0.9 } });
        db.add_decision(&NewDecision {
            id: "d1",
            conversation_id: None,
            run_id: None,
            purpose: "goal-met",
            backend: "local:x",
            model: "Clef-Flash-Q4_K_M",
            ms: 1,
            calibrated: false,
            answers: &answers,
            input_tokens: 1,
            state_preview: "",
            state_hash: "h",
            action: None,
        })
        .unwrap();
        db.add_outcome("d1", "weak_wrong", None, "goal-replaced").unwrap();
        assert_eq!(refit(&db, "Clef-Flash-Q4_K_M", Purpose::GoalMet, 6).unwrap().0.n, 61);
    }

    #[test]
    fn nothing_to_fit_keeps_nothing() {
        let db = Db::open_in_memory().unwrap();
        assert!(refit(&db, "m", Purpose::MemScope, 0).is_none());
        assert!(calibration::load(&db, "m", "mem-scope").is_none());
    }

    #[test]
    fn a_weekly_refit_needs_a_week_and_fifty_new_labels() {
        let db = seed_db("m", Purpose::GoalMet, true);
        let week = WEEK_MS;
        assert!(!refit_due(&db, "m", Purpose::GoalMet, week + 1), "no new labels");
        // Fifty labels written now, on one decision each.
        let answers = serde_json::json!({ "met": { "p": 0.9 } });
        for i in 0..REFIT_LABELS {
            let id = format!("d{i}");
            db.add_decision(&NewDecision {
                id: &id,
                conversation_id: None,
                run_id: None,
                purpose: "goal-met",
                backend: "local:x",
                model: "m",
                ms: 1,
                calibrated: false,
                answers: &answers,
                input_tokens: 1,
                state_preview: "",
                state_hash: &id,
                action: None,
            })
            .unwrap();
            db.add_outcome(&id, "right", None, "t").unwrap();
        }
        assert!(refit_due(&db, "m", Purpose::GoalMet, week + 1));
        assert!(!refit_due(&db, "m", Purpose::GoalMet, week - 1), "not a week yet");
        assert!(!refit_due(&db, "m", Purpose::MemScope, week + 1), "another purpose has no fit");
    }

    #[test]
    fn a_report_counts_the_labels_written_since_the_fit() {
        let db = seed_db("m", Purpose::GoalMet, true);
        let r = report(&db, "m", Purpose::GoalMet);
        assert!(!r.calibrated, "ece 0.2 is not calibrated");
        assert_eq!(r.new_labels, 0);
        assert_eq!(report(&db, "m", Purpose::MemScope).fit, None);
    }
}
