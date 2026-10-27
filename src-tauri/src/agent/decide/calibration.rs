//! `CTL-4`: is a probability worth what it says? A model that says 0.9 should be
//! right nine times in ten. Neither Clef nor Jev is assumed to be: one
//! temperature per (model file, purpose) is fitted to labelled decisions, and a
//! purpose counts as **calibrated** only when the fit is good (`ece <= 0.08`)
//! and rests on enough of them (`n >= 40`).
//!
//! The fit rescales probabilities (`p^(1/t)`, renormalised), which never changes
//! which option wins. Only how sure the winner looks changes, and so whether it
//! reaches its row's bar.

use super::Answer;
use crate::db::Db;

/// A fit counts as calibrated with at least this many labelled decisions.
pub const MIN_N: u32 = 40;
/// ...and an expected calibration error no larger than this.
pub const MAX_ECE: f32 = 0.08;
/// A label inferred from behaviour, not stated, counts for less.
pub const WEAK_WEIGHT: f32 = 0.3;
const BINS: usize = 10;

/// What a labelled decision says about the truth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Truth {
    /// This option was the right one.
    Class(usize),
    /// This option was wrong; which one was right is not known.
    NotClass(usize),
}

/// One labelled decision: the probabilities it gave, what was true, how much to
/// trust the label.
#[derive(Debug, Clone, PartialEq)]
pub struct Obs {
    pub probs: Vec<f32>,
    pub truth: Truth,
    pub weight: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fit {
    pub t: f32,
    pub ece: f32,
    pub n: u32,
    pub fitted_at: i64,
}

impl Fit {
    pub fn is_calibrated(&self) -> bool {
        self.n >= MIN_N && self.ece <= MAX_ECE
    }
}

/// `p^(1/t)`, renormalised. `t > 1` flattens an overconfident model, `t < 1`
/// sharpens an underconfident one, `t = 1` changes nothing.
pub fn temper(probs: &[f32], t: f32) -> Vec<f32> {
    let t = t.max(0.05);
    let raised: Vec<f32> = probs.iter().map(|p| p.max(1e-9).powf(1.0 / t)).collect();
    let total: f32 = raised.iter().sum();
    if total <= 0.0 {
        return probs.to_vec();
    }
    raised.iter().map(|p| p / total).collect()
}

fn top(probs: &[f32]) -> usize {
    probs.iter().enumerate().fold(0, |best, (i, p)| if *p > probs[best] { i } else { best })
}

/// Weighted negative log-likelihood of the labels under temperature `t`.
pub fn nll(obs: &[Obs], t: f32) -> f32 {
    let mut total = 0.0;
    let mut weight = 0.0;
    for o in obs {
        let q = temper(&o.probs, t);
        let p = match o.truth {
            Truth::Class(i) => q.get(i).copied().unwrap_or(0.0),
            Truth::NotClass(i) => 1.0 - q.get(i).copied().unwrap_or(0.0),
        };
        total += -o.weight * p.max(1e-6).ln();
        weight += o.weight;
    }
    if weight <= 0.0 {
        0.0
    } else {
        total / weight
    }
}

/// The temperature that minimises [`nll`]: a coarse search over a log scale, then
/// a finer one around the best point. Cheap, and it has no local minima to fall
/// into, because the loss is smooth in `1/t` for these likelihoods.
pub fn fit_temperature(obs: &[Obs]) -> f32 {
    let (lo, hi) = (0.1f32.ln(), 10.0f32.ln());
    let search = |lo: f32, hi: f32, steps: usize| {
        let mut best = (f32::MAX, 1.0f32);
        for i in 0..=steps {
            let t = (lo + (hi - lo) * i as f32 / steps as f32).exp();
            let l = nll(obs, t);
            if l < best.0 {
                best = (l, t);
            }
        }
        best.1
    };
    let coarse = search(lo, hi, 80);
    let width = (hi - lo) / 80.0;
    search(coarse.ln() - width, coarse.ln() + width, 40)
}

/// Expected calibration error: in ten bins of the winner's probability, how far
/// the stated sureness is from how often the winner was right, averaged by weight.
pub fn ece(obs: &[Obs], t: f32) -> f32 {
    let mut conf = [0.0f32; BINS];
    let mut hit = [0.0f32; BINS];
    let mut weight = [0.0f32; BINS];
    for o in obs {
        let q = temper(&o.probs, t);
        let winner = top(&q);
        let p = q[winner];
        let bin = ((p * BINS as f32) as usize).min(BINS - 1);
        let correct = matches!(o.truth, Truth::Class(i) if i == winner);
        conf[bin] += o.weight * p;
        hit[bin] += o.weight * correct as u8 as f32;
        weight[bin] += o.weight;
    }
    let all: f32 = weight.iter().sum();
    if all <= 0.0 {
        return 0.0;
    }
    (0..BINS)
        .filter(|b| weight[*b] > 0.0)
        .map(|b| (weight[b] / all) * (conf[b] / weight[b] - hit[b] / weight[b]).abs())
        .sum()
}

/// Fit one (model, purpose) from labelled decisions. `None` when there are none.
pub fn fit(obs: &[Obs], now: i64) -> Option<Fit> {
    if obs.is_empty() {
        return None;
    }
    let t = fit_temperature(obs);
    Some(Fit { t, ece: ece(obs, t), n: obs.len() as u32, fitted_at: now })
}

/// The ECE if nothing were fitted, for the screen to say "was 0.21, now 0.05".
pub fn ece_before(obs: &[Obs]) -> f32 {
    ece(obs, 1.0)
}

/// The same answer with its probabilities rescaled. The winner is unchanged;
/// the confidence is worked out again from the new spread. A score's value is
/// left alone: it is a reading of the scale, not a probability.
pub fn temper_answer(answer: &Answer, t: f32) -> Answer {
    match answer {
        Answer::Noul { p_true } => {
            let q = temper(&[*p_true, 1.0 - *p_true], t);
            Answer::Noul { p_true: q[0] }
        }
        Answer::Choice { choice, probs, .. } => {
            let q = temper(&probs.iter().map(|(_, p)| *p).collect::<Vec<_>>(), t);
            let confidence = super::confidence(&q);
            Answer::Choice {
                choice: choice.clone(),
                probs: probs.iter().zip(q).map(|((k, _), p)| (k.clone(), p)).collect(),
                confidence,
            }
        }
        Answer::Score { score, probs, .. } => {
            let q = temper(probs, t);
            let confidence = super::confidence(&q);
            Answer::Score { score: *score, probs: q, confidence }
        }
    }
}

/// The probabilities of the first answer of a decision, and the option names
/// (`true`, `false` for a yes-or-no question), as the log keeps them.
pub fn probs_of(answers: &serde_json::Value) -> Option<(Vec<String>, Vec<f32>)> {
    let first = answers.as_object()?.values().next()?;
    if let Some(p) = first.get("p").and_then(|p| p.as_f64()) {
        let p = p as f32;
        return Some((vec!["true".into(), "false".into()], vec![p, 1.0 - p]));
    }
    if let Some(list) = first.get("probs").and_then(|p| p.as_array()) {
        let mut names = Vec::new();
        let mut ps = Vec::new();
        for item in list {
            if let Some(pair) = item.as_array() {
                names.push(pair.first()?.as_str()?.to_string());
                ps.push(pair.get(1)?.as_f64()? as f32);
            } else if let Some(p) = item.as_f64() {
                names.push(names.len().to_string());
                ps.push(p as f32);
            }
        }
        if !ps.is_empty() {
            return Some((names, ps));
        }
    }
    None
}

/// The settings key a fit is kept under (`CTL-4`).
pub fn key(model: &str, purpose: &str) -> String {
    format!("decide.calibration.{model}.{purpose}")
}

pub fn load(db: &Db, model: &str, purpose: &str) -> Option<Fit> {
    let raw = db.get_setting(&key(model, purpose)).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

pub fn save(db: &Db, model: &str, purpose: &str, fit: &Fit) {
    if let Ok(raw) = serde_json::to_string(fit) {
        let _ = db.set_setting(&key(model, purpose), &raw);
    }
}

/// The seed observations of the last run are kept, so a refit from new labels
/// does not have to run the model on the seed sets again.
fn seed_key(model: &str, purpose: &str) -> String {
    format!("decide.seed.{model}.{purpose}")
}

pub fn load_seed(db: &Db, model: &str, purpose: &str) -> Vec<Obs> {
    #[derive(serde::Deserialize)]
    struct Stored(Vec<f32>, usize);
    let Some(raw) = db.get_setting(&seed_key(model, purpose)).ok().flatten() else { return Vec::new() };
    serde_json::from_str::<Vec<Stored>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .map(|Stored(probs, class)| Obs { probs, truth: Truth::Class(class), weight: 1.0 })
        .collect()
}

pub fn save_seed(db: &Db, model: &str, purpose: &str, obs: &[Obs]) {
    let rows: Vec<(Vec<f32>, usize)> = obs
        .iter()
        .filter_map(|o| match o.truth {
            Truth::Class(i) => Some((o.probs.clone(), i)),
            Truth::NotClass(_) => None,
        })
        .collect();
    if let Ok(raw) = serde_json::to_string(&rows) {
        let _ = db.set_setting(&seed_key(model, purpose), &raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(p: f32, truth_is_yes: bool) -> Obs {
        Obs { probs: vec![p, 1.0 - p], truth: Truth::Class(if truth_is_yes { 0 } else { 1 }), weight: 1.0 }
    }

    /// 100 answers at 0.95 of which 70 are right: overconfident, so the fit
    /// flattens it (`t > 1`) and the error drops.
    fn overconfident() -> Vec<Obs> {
        (0..100).map(|i| binary(0.95, i < 70)).collect()
    }

    #[test]
    fn temper_keeps_the_winner_and_the_sum() {
        let q = temper(&[0.7, 0.2, 0.1], 2.0);
        assert!((q.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert_eq!(top(&q), 0);
        assert!(q[0] < 0.7, "t > 1 flattens");
        let sharp = temper(&[0.7, 0.2, 0.1], 0.5);
        assert!(sharp[0] > 0.7, "t < 1 sharpens");
        assert_eq!(temper(&[0.6, 0.4], 1.0).iter().map(|p| (p * 100.0).round()).collect::<Vec<_>>(), vec![60.0, 40.0]);
    }

    /// `CTL-T-2`: ECE on a set whose answer is known.
    #[test]
    fn ece_is_the_gap_between_how_sure_and_how_often_right() {
        // Every answer says 0.9; 90 in 100 are right: no gap.
        let calibrated: Vec<Obs> = (0..100).map(|i| binary(0.9, i < 90)).collect();
        assert!(ece(&calibrated, 1.0) < 0.02, "{}", ece(&calibrated, 1.0));
        // Every answer says 0.9; 60 in 100 are right: a gap of 0.3.
        let over: Vec<Obs> = (0..100).map(|i| binary(0.9, i < 60)).collect();
        assert!((ece(&over, 1.0) - 0.3).abs() < 0.02, "{}", ece(&over, 1.0));
    }

    #[test]
    fn the_fit_flattens_an_overconfident_model_and_lowers_its_error() {
        let obs = overconfident();
        let f = fit(&obs, 7).unwrap();
        assert!(f.t > 1.5, "t = {}", f.t);
        assert!(f.ece < ece_before(&obs), "{} then {}", ece_before(&obs), f.ece);
        assert!(f.ece < 0.08, "{}", f.ece);
        assert_eq!((f.n, f.fitted_at), (100, 7));
    }

    #[test]
    fn the_fit_sharpens_an_underconfident_model() {
        // Says 0.6 and is right 95 times in 100.
        let obs: Vec<Obs> = (0..100).map(|i| binary(0.6, i < 95)).collect();
        assert!(fit(&obs, 0).unwrap().t < 0.7);
    }

    #[test]
    fn a_calibrated_model_keeps_a_temperature_near_one() {
        let obs: Vec<Obs> = (0..200).map(|i| binary(0.8, i < 160)).collect();
        let f = fit(&obs, 0).unwrap();
        assert!((f.t - 1.0).abs() < 0.25, "t = {}", f.t);
    }

    /// `CTL-T-2`: weak labels count for less. The same labels, mostly weak and
    /// wrong, pull the fit less than when they are firm.
    #[test]
    fn weak_labels_pull_the_fit_less_than_firm_ones() {
        let mut firm = overconfident();
        let mut weak = overconfident();
        for o in weak.iter_mut().filter(|o| matches!(o.truth, Truth::Class(1))) {
            o.weight = WEAK_WEIGHT;
        }
        let firm_t = fit_temperature(&firm);
        let weak_t = fit_temperature(&weak);
        assert!(weak_t < firm_t, "fewer firm misses, so less flattening: {weak_t} vs {firm_t}");
        firm.clear();
    }

    /// A label that says only "this winner was wrong" counts against the winner.
    #[test]
    fn a_wrong_label_without_the_right_answer_counts_against_the_winner() {
        let wrong = Obs { probs: vec![0.8, 0.1, 0.1], truth: Truth::NotClass(0), weight: 1.0 };
        let right = Obs { probs: vec![0.8, 0.1, 0.1], truth: Truth::Class(0), weight: 1.0 };
        assert!(nll(&[wrong.clone()], 1.0) > nll(&[right], 1.0));
        assert!((ece(&[wrong], 1.0) - 0.8).abs() < 1e-3, "stated 0.8, right 0 times");
    }

    #[test]
    fn calibrated_needs_a_good_fit_and_enough_decisions() {
        let good = Fit { t: 1.2, ece: 0.05, n: 40, fitted_at: 0 };
        assert!(good.is_calibrated());
        assert!(!Fit { n: 39, ..good }.is_calibrated(), "too few");
        assert!(!Fit { ece: 0.09, ..good }.is_calibrated(), "too far off");
        assert!(Fit { ece: 0.08, ..good }.is_calibrated(), "the bound is inclusive");
    }

    #[test]
    fn no_labels_no_fit() {
        assert!(fit(&[], 0).is_none());
        assert_eq!(ece(&[], 1.0), 0.0);
    }

    #[test]
    fn a_tempered_answer_keeps_its_winner_and_changes_how_sure_it_looks() {
        let probs = vec![("a".to_string(), 0.9), ("b".to_string(), 0.1)];
        let confidence = crate::agent::decide::confidence(&[0.9, 0.1]);
        let answer = Answer::Choice { choice: "a".into(), probs, confidence };
        let Answer::Choice { choice, confidence: after, probs } = temper_answer(&answer, 3.0) else { panic!() };
        assert_eq!(choice, "a");
        assert!(after < confidence);
        assert!(probs[0].1 < 0.9 && probs[0].1 > probs[1].1);
        let Answer::Noul { p_true } = temper_answer(&Answer::Noul { p_true: 0.95 }, 2.0) else { panic!() };
        assert!(p_true < 0.95 && p_true > 0.5);
    }

    #[test]
    fn probabilities_are_read_back_from_the_log() {
        let yes_no = serde_json::json!({ "enough": { "p": 0.8 } });
        let (names, p) = probs_of(&yes_no).unwrap();
        assert_eq!(names, vec!["true", "false"]);
        assert!((p[0] - 0.8).abs() < 1e-6 && (p[1] - 0.2).abs() < 1e-6);
        let pick = serde_json::json!({ "scope": { "choice": "global", "probs": [["global", 0.6], ["topical", 0.3], ["unsure", 0.1]] } });
        let (names, p) = probs_of(&pick).unwrap();
        assert_eq!(names, vec!["global", "topical", "unsure"]);
        assert_eq!(p.len(), 3);
        assert!(probs_of(&serde_json::json!({})).is_none());
    }

    #[test]
    fn a_fit_and_the_seed_run_are_kept_in_settings() {
        let db = Db::open_in_memory().unwrap();
        assert!(load(&db, "Clef-Flash-Q4_K_M", "goal-met").is_none());
        let f = Fit { t: 1.4, ece: 0.04, n: 50, fitted_at: 9 };
        save(&db, "Clef-Flash-Q4_K_M", "goal-met", &f);
        assert_eq!(load(&db, "Clef-Flash-Q4_K_M", "goal-met"), Some(f));
        assert!(load(&db, "Clef-Flash-Q4_K_M", "mem-scope").is_none(), "per purpose");
        assert!(load(&db, "Another-Model", "goal-met").is_none(), "per model: a version bump starts uncalibrated");

        let seed = vec![binary(0.9, true), binary(0.7, false)];
        save_seed(&db, "m", "p", &seed);
        assert_eq!(load_seed(&db, "m", "p"), seed);
    }
}
