//! `MET-1`: what I know about how well I do, by kind of work (`COGNITION_PLAN`
//! III.7).
//!
//! The evidence is the outcome and task kind the encoding pass already wrote on
//! each finished conversation (`agent::episode`). Nothing is asked of a model.
//! A kind is judged only after [`MIN_JUDGED`] conversations in the last 60
//! days, so one bad afternoon does not make a weakness.
//!
//! - **Weak**: 40% or more of the conversations failed or were abandoned.
//! - **Strong**: 80% or more were done, and 10% or fewer failed or were abandoned.
//! - **Steady**: judged, and neither.
//!
//! What a weak kind changes is small: a turn in it gets a line that asks me to
//! check my work and to ask one short question early (`commands::memory`), and
//! the automatic answer check starts more slow reviews by itself
//! (`autocheck::Check::careful`). A strong kind changes nothing.

use serde::{Deserialize, Serialize};

use super::decide::calibration;
use super::episode::{self, ABANDONED, DONE, FAILED, PARTIAL};
use crate::db::Db;

const DAY_MS: i64 = 86_400_000;
/// How far back conversations count.
pub const WINDOW_MS: i64 = 60 * DAY_MS;
/// Conversations of one kind (with a known outcome) before it is judged.
pub const MIN_JUDGED: i64 = 5;
/// The portrait is rebuilt at most this often.
const PORTRAIT_MS: i64 = 7 * DAY_MS;
const PORTRAIT_KEY: &str = "self.portrait";

/// How I do at one kind of work.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct KindStat {
    pub kind: String,
    pub done: i64,
    pub partial: i64,
    pub failed: i64,
    pub abandoned: i64,
    /// `unknown | weak | steady | strong`.
    pub strength: &'static str,
}

impl KindStat {
    pub fn judged(&self) -> i64 {
        self.done + self.partial + self.failed + self.abandoned
    }

    fn bad(&self) -> i64 {
        self.failed + self.abandoned
    }

    fn rate(&mut self) {
        let n = self.judged();
        self.strength = if n < MIN_JUDGED {
            "unknown"
        } else if self.bad() * 10 >= n * 4 {
            "weak"
        } else if self.done * 10 >= n * 8 && self.bad() * 10 <= n {
            "strong"
        } else {
            "steady"
        };
    }
}

/// Every kind with any conversation in the window, by name.
pub fn kinds(db: &Db, now: i64) -> Vec<KindStat> {
    let mut out: Vec<KindStat> = Vec::new();
    for (kind, outcome, n) in db.outcomes_by_kind(now - WINDOW_MS).unwrap_or_default() {
        let at = match out.iter().position(|k| k.kind == kind) {
            Some(i) => i,
            None => {
                out.push(KindStat { kind, ..Default::default() });
                out.len() - 1
            }
        };
        let stat = &mut out[at];
        match outcome.as_str() {
            DONE => stat.done += n,
            PARTIAL => stat.partial += n,
            FAILED => stat.failed += n,
            ABANDONED => stat.abandoned += n,
            _ => {}
        }
    }
    out.iter_mut().for_each(KindStat::rate);
    out.sort_by(|a, b| a.kind.cmp(&b.kind));
    out
}

/// The kind of work a conversation is in so far, by the same rules the encoding
/// pass uses at the end. `None` while it cannot be told.
///
/// Only what the kind rules read (tools, artifacts, folder): three small reads,
/// not the whole encoding pass, because this runs on every turn once a kind is weak.
pub fn kind_now(db: &Db, conversation_id: &str) -> Option<&'static str> {
    let signals = episode::Signals {
        tools: db.tools_used_in(conversation_id).unwrap_or_default(),
        artifacts: db.list_artifacts(conversation_id).unwrap_or_default().into_iter().map(|a| a.kind).collect(),
        folder: db.conversation_folder(conversation_id).ok().and_then(|(f, _)| f).is_some(),
        ..Default::default()
    };
    let kind = episode::task_kind(&signals);
    (kind != episode::UNKNOWN).then_some(kind)
}

/// The weak kind this conversation is in, if it is in one. Cheap when nothing is
/// weak: one grouped read and no look at the conversation.
pub fn weak_kind_of(db: &Db, conversation_id: &str, now: i64) -> Option<String> {
    let weak: Vec<String> = kinds(db, now).into_iter().filter(|k| k.strength == "weak").map(|k| k.kind).collect();
    if weak.is_empty() {
        return None;
    }
    let kind = kind_now(db, conversation_id)?;
    weak.into_iter().find(|k| k == kind)
}

/// A kind in the words the person uses for it.
pub fn words(kind: &str) -> &str {
    match kind {
        "mail" => "mail",
        "code" => "code and file work",
        "report" => "reports",
        other => other,
    }
}

fn list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// One or two sentences, words and no numbers. Empty while nothing is judged.
pub fn portrait(stats: &[KindStat]) -> String {
    let of = |strength: &str| stats.iter().filter(|k| k.strength == strength).map(|k| words(&k.kind)).collect::<Vec<_>>();
    let (strong, weak) = (of("strong"), of("weak"));
    let mut out = Vec::new();
    if !strong.is_empty() {
        out.push(format!("I'm reliable with {}.", list(&strong)));
    }
    if !weak.is_empty() {
        out.push(format!("I often get {} wrong on the first try, so I check that work before I go on.", list(&weak)));
    }
    out.join(" ")
}

#[derive(Serialize, Deserialize)]
struct Kept {
    built_at: i64,
    text: String,
}

/// The portrait, rebuilt once a week so the sentence does not change under the
/// person's eyes. Only a sentence is kept: a week with nothing to say is asked
/// again next time.
pub fn portrait_now(db: &Db, now: i64) -> String {
    let kept: Option<Kept> =
        db.get_setting(PORTRAIT_KEY).ok().flatten().and_then(|raw| serde_json::from_str(&raw).ok());
    if let Some(k) = kept.filter(|k| now - k.built_at < PORTRAIT_MS && !k.text.is_empty()) {
        return k.text;
    }
    let text = portrait(&kinds(db, now));
    if !text.is_empty() {
        if let Ok(raw) = serde_json::to_string(&Kept { built_at: now, text: text.clone() }) {
            let _ = db.set_setting(PORTRAIT_KEY, &raw);
        }
    }
    text
}

/// How well a decision purpose's stated confidence matches what happened, for
/// the model in play (`CTL-4`'s fit). Only purposes that have a fit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Confidence {
    pub purpose: &'static str,
    /// Expected calibration error: 0 is perfect.
    pub ece: f32,
    pub labels: u32,
    pub calibrated: bool,
}

pub fn confidence(db: &Db, model: &str) -> Vec<Confidence> {
    super::decide::policy::ALL
        .iter()
        .filter_map(|p| {
            let fit = calibration::load(db, model, p.name())?;
            Some(Confidence { purpose: p.name(), ece: fit.ece, labels: fit.n, calibrated: fit.is_calibrated() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` conversations of one kind and outcome, encoded just now.
    fn chats(db: &Db, kind: &str, outcome: &str, n: usize) {
        for _ in 0..n {
            let id = db.create_conversation("A chat", None, false).unwrap().id;
            db.save_episode_rules(&id, outcome, kind, "{}").unwrap();
        }
    }

    fn stat<'a>(all: &'a [KindStat], kind: &str) -> &'a KindStat {
        all.iter().find(|k| k.kind == kind).unwrap()
    }

    #[test]
    fn a_kind_is_not_judged_before_five_conversations() {
        let db = Db::open_in_memory().unwrap();
        chats(&db, "mail", FAILED, 4);
        let all = kinds(&db, crate::db::now_ms());
        assert_eq!(stat(&all, "mail").judged(), 4);
        assert_eq!(stat(&all, "mail").strength, "unknown");
        assert_eq!(portrait(&all), "");
    }

    #[test]
    fn many_failures_or_abandoned_chats_make_a_kind_weak() {
        let db = Db::open_in_memory().unwrap();
        chats(&db, "report", DONE, 3);
        chats(&db, "report", FAILED, 1);
        chats(&db, "report", ABANDONED, 1);
        let all = kinds(&db, crate::db::now_ms());
        assert_eq!(stat(&all, "report").strength, "weak", "2 of 5 is 40%");
    }

    #[test]
    fn a_kind_that_mostly_works_is_strong_and_a_mixed_one_is_steady() {
        let db = Db::open_in_memory().unwrap();
        chats(&db, "mail", DONE, 9);
        chats(&db, "mail", FAILED, 1);
        chats(&db, "code", DONE, 5);
        chats(&db, "code", PARTIAL, 2);
        chats(&db, "code", FAILED, 1);
        let all = kinds(&db, crate::db::now_ms());
        assert_eq!(stat(&all, "mail").strength, "strong", "9 of 10 done, 1 of 10 bad");
        assert_eq!(stat(&all, "code").strength, "steady", "5 of 8 done is under 80%, 1 of 8 bad is over 10%");
    }

    #[test]
    fn unknown_kinds_and_unknown_outcomes_do_not_count() {
        let db = Db::open_in_memory().unwrap();
        chats(&db, "unknown", FAILED, 6);
        chats(&db, "mail", "unknown", 6);
        let all = kinds(&db, crate::db::now_ms());
        assert!(all.iter().all(|k| k.kind != "unknown"));
        assert_eq!(stat(&all, "mail").judged(), 0);
    }

    #[test]
    fn old_conversations_fall_out_of_the_window() {
        let db = Db::open_in_memory().unwrap();
        chats(&db, "mail", FAILED, 6);
        assert_eq!(stat(&kinds(&db, crate::db::now_ms()), "mail").judged(), 6);
        assert!(kinds(&db, crate::db::now_ms() + 2 * WINDOW_MS).is_empty());
    }

    #[test]
    fn the_portrait_says_what_is_strong_and_what_is_weak_in_words() {
        let db = Db::open_in_memory().unwrap();
        chats(&db, "mail", DONE, 6);
        chats(&db, "report", FAILED, 3);
        chats(&db, "report", DONE, 2);
        let text = portrait(&kinds(&db, crate::db::now_ms()));
        assert_eq!(
            text,
            "I'm reliable with mail. I often get reports wrong on the first try, so I check that work before I go on."
        );
        assert!(!text.chars().any(|c| c.is_ascii_digit()), "words, not numbers");
    }

    #[test]
    fn the_portrait_is_kept_for_a_week() {
        let db = Db::open_in_memory().unwrap();
        let now = crate::db::now_ms();
        chats(&db, "mail", DONE, 6);
        let first = portrait_now(&db, now);
        assert!(first.contains("mail"));
        chats(&db, "code", FAILED, 6);
        assert_eq!(portrait_now(&db, now + DAY_MS), first, "the same sentence inside a week");
        assert!(portrait_now(&db, now + 8 * DAY_MS).contains("code"), "rebuilt after a week");
    }

    #[test]
    fn only_a_weak_kind_the_chat_is_in_makes_me_careful() {
        let db = Db::open_in_memory().unwrap();
        let now = crate::db::now_ms();
        let chat = db.create_conversation("Now", None, false).unwrap().id;
        db.set_conversation_folder(&chat, Some("/tmp/x")).unwrap();
        assert_eq!(weak_kind_of(&db, &chat, now), None, "nothing is weak yet");
        chats(&db, "mail", FAILED, 6);
        assert_eq!(weak_kind_of(&db, &chat, now), None, "this chat is code work, not mail");
        chats(&db, "code", FAILED, 6);
        assert_eq!(weak_kind_of(&db, &chat, now).as_deref(), Some("code"));
    }

    /// The light read gives the same kind as the full encoding rules.
    #[test]
    fn the_kind_now_matches_the_encoding_rules() {
        let db = Db::open_in_memory().unwrap();
        let mail = db.create_conversation("Mail", None, false).unwrap().id;
        db.add_tool_stat("m", "list_mail", &mail, true);
        let plain = db.create_conversation("Chat", None, false).unwrap().id;
        for chat in [&mail, &plain] {
            let full = episode::task_kind(&episode::gather(&db, chat));
            assert_eq!(kind_now(&db, chat).unwrap_or(episode::UNKNOWN), full);
        }
        assert_eq!(kind_now(&db, &mail), Some("mail"));
        assert_eq!(kind_now(&db, &plain), None);
    }

    #[test]
    fn the_confidence_rows_are_the_fits_that_exist() {
        let db = Db::open_in_memory().unwrap();
        assert!(confidence(&db, "m").is_empty());
        calibration::save(&db, "m", "goal-met", &calibration::Fit { t: 1.0, ece: 0.05, n: 60, fitted_at: 0 });
        let rows = confidence(&db, "m");
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].purpose, rows[0].calibrated), ("goal-met", true));
    }
}
