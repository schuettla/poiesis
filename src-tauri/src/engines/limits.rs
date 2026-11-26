//! What an agent app says about the person's plan, and what a plan limit looks
//! like when a job runs into it (`XA-9`, `XA-11`).
//!
//! Claude Code sends a `rate_limit_event` and Codex an `account/rateLimits/updated`
//! notification while they work. Both are kept, per agent, in the settings table
//! (`engine_limits:<id>`), so Usage and Settings can say how much of the plan is
//! used and when the window resets. A job that ends on a limit is not a crash:
//! the agent works again after a known time, so it is recorded as reached and the
//! agent is not offered more work until then.
//!
//! The shapes parsed here are our own recordings (`claude-2.1.301.jsonl`,
//! `codex-0.163.0.jsonl`). Nothing else is guessed.

use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::{now_ms, Db};

/// One window of the plan: how much of it is used, and when it resets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// Ready to show: "5 hours", "week", "month".
    pub name: String,
    /// 0 to 1.
    pub used: f64,
    pub resets_at_ms: Option<i64>,
}

/// What an agent last said about the plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanUsage {
    /// When it was seen.
    pub at_ms: i64,
    pub windows: Vec<Window>,
    /// The agent said the limit is hit, or a job ended on it.
    pub reached: bool,
    /// When the limit lifts, when the agent said.
    pub resets_at_ms: Option<i64>,
}

/// A record of "reached" with no reset time stops counting after this long: the
/// agent has not said when it is back, and a stale guess must not shut it out.
const UNKNOWN_RESET_HOLDS_MS: i64 = 5 * 60 * 60 * 1000;
/// Counts as a full window.
const FULL: f64 = 0.999;

impl PlanUsage {
    /// Is the agent shut out right now? Only a reached record whose reset time is
    /// still ahead, or (no time given) is younger than five hours.
    pub fn holds(&self, now: i64) -> bool {
        self.reached
            && match self.resets_at_ms {
                Some(t) => t > now,
                None => now - self.at_ms < UNKNOWN_RESET_HOLDS_MS,
            }
    }

    /// The same, with `reached` read as of `now`, for the screens.
    pub fn view(&self, now: i64) -> PlanUsage {
        PlanUsage { reached: self.holds(now), ..self.clone() }
    }

    /// The most used window, 0 to 1.
    pub fn highest(&self) -> Option<f64> {
        self.windows.iter().map(|w| w.used).fold(None, |m, u| Some(m.map_or(u, |m: f64| m.max(u))))
    }
}

fn ms_of(seconds: &Value) -> Option<i64> {
    seconds.as_i64().or_else(|| seconds.as_f64().map(|f| f as i64)).map(|s| s * 1000)
}

/// Claude Code's own names for its windows, as words.
fn claude_window_name(key: &str) -> String {
    match key {
        "five_hour" => "5 hours".into(),
        "seven_day" => "week".into(),
        other => other.replace('_', " "),
    }
}

/// A window from the minutes it lasts: 300 is "5 hours", 10080 "week", 43200 "month".
fn codex_window_name(minutes: i64) -> String {
    match minutes {
        300 => "5 hours".into(),
        10080 => "week".into(),
        43200 => "month".into(),
        m => {
            let hours = ((m as f64) / 60.0).round().max(1.0) as i64;
            if hours == 1 { "1 hour".into() } else { format!("{hours} hours") }
        }
    }
}

/// `rate_limit_event` from Claude Code. `None` when `rate_limit_info` is not an
/// object (the oh-my-hermes recordings have the string `"<removed>"`).
///
/// Only `status: "allowed"` has been recorded. Any other value is read as "reached"
/// but has never been seen.
pub fn from_claude(event: &Value) -> Option<PlanUsage> {
    let info = event.get("rate_limit_info").filter(|i| i.is_object())?;
    let windows: Vec<Window> = info
        .get("unifiedWindows")
        .and_then(|w| w.as_object())
        .into_iter()
        .flatten()
        .filter_map(|(key, w)| {
            Some(Window {
                name: claude_window_name(key),
                used: w.get("utilization").and_then(|u| u.as_f64())?.clamp(0.0, 1.0),
                resets_at_ms: w.get("resetsAt").and_then(ms_of),
            })
        })
        .collect();
    let status = info.get("status").and_then(|s| s.as_str());
    if status.is_none() && windows.is_empty() {
        return None;
    }
    // The top-level `resetsAt` belongs to `rateLimitType`, the window that matters now.
    let resets_at_ms = info
        .get("resetsAt")
        .and_then(ms_of)
        .or_else(|| windows.iter().filter_map(|w| w.resets_at_ms).min());
    Some(PlanUsage { at_ms: now_ms(), windows, reached: status.is_some_and(|s| s != "allowed"), resets_at_ms })
}

/// The `params` of `account/rateLimits/updated` from Codex. `None` when there is
/// no window and nothing reached.
///
/// `rateLimitReachedType` has been `null` in every recording: that it is not null
/// when the limit is hit is read from its name, and has never been seen.
pub fn from_codex(params: &Value) -> Option<PlanUsage> {
    let limits = params.get("rateLimits").filter(|l| l.is_object())?;
    let windows: Vec<Window> = ["primary", "secondary"]
        .iter()
        .filter_map(|k| limits.get(*k).filter(|w| w.is_object()))
        .filter_map(|w| {
            Some(Window {
                name: codex_window_name(w.get("windowDurationMins").and_then(|m| m.as_i64())?),
                used: (w.get("usedPercent").and_then(|u| u.as_f64())? / 100.0).clamp(0.0, 1.0),
                resets_at_ms: w.get("resetsAt").and_then(ms_of),
            })
        })
        .collect();
    let reached = limits.get("rateLimitReachedType").is_some_and(|r| !r.is_null());
    if windows.is_empty() && !reached {
        return None;
    }
    // When the limit is hit, back when every full window has reset; else the next reset.
    let full: Vec<i64> = windows.iter().filter(|w| w.used >= FULL).filter_map(|w| w.resets_at_ms).collect();
    let resets_at_ms = if reached && !full.is_empty() {
        full.into_iter().max()
    } else {
        windows.iter().filter_map(|w| w.resets_at_ms).min()
    };
    Some(PlanUsage { at_ms: now_ms(), windows, reached, resets_at_ms })
}

fn key(engine_id: &str) -> String {
    format!("engine_limits:{engine_id}")
}

pub fn save(db: &Db, engine_id: &str, usage: &PlanUsage) {
    if let Ok(json) = serde_json::to_string(usage) {
        let _ = db.set_setting(&key(engine_id), &json);
    }
}

pub fn load(db: &Db, engine_id: &str) -> Option<PlanUsage> {
    let raw = db.get_setting(&key(engine_id)).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

/// The phrases of an agent's own error text that mean a plan or provider limit.
/// From oh-my-hermes, each seen in a real failure. No shorter ones: a bare "429"
/// is a line number and a bare "quota" a disk quota.
const LIMIT_PHRASES: &[&str] = &[
    "rate limit",
    "usage limit",
    "quota exceed",
    "quota exhaust",
    "api quota",
    "status 429",
    "error 429",
    "http 429",
    "429 too many",
    "insufficient credit",
    "out of credits",
    "limit reached",
    "session limit",
    "hit your limit",
];

/// Which phrase of a limit the text holds, if any. Only for a job that failed:
/// a report that talks about rate limiting is not one.
pub fn limit_words(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    LIMIT_PHRASES.iter().copied().find(|p| lower.contains(p))
}

/// A failed job: was it the plan? `seen` is what the agent said about the plan
/// during the job. Yes when it said "reached", or its error words (or the last of
/// its stderr) are a limit's. Returns what to record, with `reached` set.
pub fn plan_limit_of(db: &Db, engine_id: &str, seen: Option<&PlanUsage>, error: &str, stderr: &str) -> Option<PlanUsage> {
    let by_record = seen.is_some_and(|u| u.reached);
    if !by_record && limit_words(error).is_none() && limit_words(stderr).is_none() {
        return None;
    }
    let mut usage = match seen.cloned().or_else(|| load(db, engine_id)) {
        Some(u) => u,
        None => PlanUsage { at_ms: now_ms(), windows: Vec::new(), reached: true, resets_at_ms: None },
    };
    let now = now_ms();
    // The words say it and the record did not, or the record's time has passed (an
    // earlier limit): its reset time is not about this one.
    if !usage.reached || usage.resets_at_ms.is_some_and(|t| t <= now) {
        usage.resets_at_ms = None;
    }
    usage.reached = true;
    usage.at_ms = now;
    Some(usage)
}

/// When, for a person: `14:30` today, `Tue 14:30` another day.
pub fn when(ms: i64) -> String {
    when_in(ms, now_ms(), &Local)
}

fn when_in<Tz: TimeZone>(ms: i64, now: i64, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(at) = DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(tz)) else { return String::new() };
    let today = DateTime::from_timestamp_millis(now).map(|d| d.with_timezone(tz).date_naive());
    if today == Some(at.date_naive()) {
        at.format("%H:%M").to_string()
    } else {
        at.format("%a %H:%M").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    fn recorded(file: &str, find: &str) -> Value {
        let line = file.lines().find(|l| l.contains(find)).expect("the recording has the line");
        let row: Value = serde_json::from_str(line).unwrap();
        row["msg"].clone()
    }

    #[test]
    fn claude_says_two_windows_and_when_the_five_hours_reset() {
        let m = recorded(include_str!("../../../fixtures/engines/claude-2.1.301.jsonl"), "rate_limit_event");
        let u = from_claude(&m).unwrap();
        assert_eq!(u.windows.len(), 2);
        let five = u.windows.iter().find(|w| w.name == "5 hours").unwrap();
        let week = u.windows.iter().find(|w| w.name == "week").unwrap();
        assert!((five.used - 0.11).abs() < 1e-9 && (week.used - 0.59).abs() < 1e-9);
        assert_eq!(five.resets_at_ms, Some(1_792_499_400_000), "seconds become milliseconds");
        assert!(!u.reached, "status was allowed");
        assert_eq!(u.resets_at_ms, Some(1_792_499_400_000));
    }

    #[test]
    fn codex_says_one_window_named_by_its_length() {
        let m = recorded(include_str!("../../../fixtures/engines/codex-0.163.0.jsonl"), "account/rateLimits/updated");
        let u = from_codex(&m["params"]).unwrap();
        assert_eq!(u.windows.len(), 1);
        assert_eq!(u.windows[0].name, "month");
        assert!((u.windows[0].used - 0.06).abs() < 1e-9);
        assert!(!u.reached);
        assert_eq!(u.resets_at_ms, Some(1_795_077_814_000));
        assert_eq!(codex_window_name(300), "5 hours");
        assert_eq!(codex_window_name(10080), "week");
        assert_eq!(codex_window_name(180), "3 hours");
        assert_eq!(codex_window_name(30), "1 hour");
    }

    #[test]
    fn what_is_not_an_object_is_ignored() {
        assert!(from_claude(&json!({"type": "rate_limit_event", "rate_limit_info": "<removed>"})).is_none());
        assert!(from_claude(&json!({"type": "rate_limit_event"})).is_none());
        assert!(from_codex(&json!({"rateLimits": "<removed>"})).is_none());
        assert!(from_codex(&json!({"rateLimits": {"primary": null, "secondary": null, "rateLimitReachedType": null}})).is_none());
    }

    #[test]
    fn a_status_other_than_allowed_and_a_reached_type_both_mean_reached() {
        let c = from_claude(&json!({"rate_limit_info": {"status": "rejected", "resetsAt": 1000}})).unwrap();
        assert!(c.reached && c.resets_at_ms == Some(1_000_000) && c.windows.is_empty());
        let x = from_codex(&json!({"rateLimits": {
            "primary": {"usedPercent": 100, "windowDurationMins": 300, "resetsAt": 500},
            "secondary": {"usedPercent": 100, "windowDurationMins": 10080, "resetsAt": 900},
            "rateLimitReachedType": "primary"}}))
        .unwrap();
        assert!(x.reached);
        assert_eq!(x.resets_at_ms, Some(900_000), "back when every full window is back");
    }

    #[test]
    fn what_was_saved_is_what_is_loaded() {
        let db = Db::open_in_memory().unwrap();
        assert!(load(&db, "claude-code").is_none());
        let m = recorded(include_str!("../../../fixtures/engines/claude-2.1.301.jsonl"), "rate_limit_event");
        let u = from_claude(&m).unwrap();
        save(&db, "claude-code", &u);
        assert_eq!(load(&db, "claude-code"), Some(u));
        assert!(load(&db, "codex").is_none(), "each agent has its own");
    }

    #[test]
    fn limit_words_find_a_limit_and_not_a_number() {
        assert_eq!(limit_words("You've hit your session limit · resets 6:10pm"), Some("session limit"));
        assert_eq!(limit_words("Claude AI usage limit reached"), Some("usage limit"));
        assert_eq!(limit_words("Error: 429 Too Many Requests"), Some("429 too many"));
        assert_eq!(limit_words("HTTP 429 from the provider"), Some("http 429"));
        assert_eq!(limit_words("error at line 429"), None);
        assert_eq!(limit_words("disk quota"), None);
        assert_eq!(limit_words("The build failed: 429 tests"), None);
        assert_eq!(limit_words(""), None);
    }

    fn open_record(at_ms: i64, resets_at_ms: Option<i64>, reached: bool) -> PlanUsage {
        PlanUsage { at_ms, windows: Vec::new(), reached, resets_at_ms }
    }

    #[test]
    fn a_reached_record_holds_until_the_reset_or_five_hours() {
        let now = 10_000_000_000;
        assert!(open_record(now - 1000, Some(now + 60_000), true).holds(now));
        assert!(!open_record(now - 1000, Some(now - 1), true).holds(now), "the time has passed");
        assert!(open_record(now - 3_600_000, None, true).holds(now), "no time given, one hour old");
        assert!(!open_record(now - 6 * 3_600_000, None, true).holds(now), "no time given, six hours old");
        assert!(!open_record(now, Some(now + 60_000), false).holds(now), "not reached");
        assert!(!open_record(now - 1000, Some(now - 1), true).view(now).reached);
    }

    #[test]
    fn a_failed_job_is_a_plan_limit_by_its_words_or_by_the_record() {
        let db = Db::open_in_memory().unwrap();
        // By words alone: no windows, no time.
        let u = plan_limit_of(&db, "claude-code", None, "Claude AI usage limit reached", "").unwrap();
        assert!(u.reached && u.resets_at_ms.is_none());
        // By the last of stderr.
        assert!(plan_limit_of(&db, "codex", None, "It stopped by itself (code 1)", "status 429 from the server").is_some());
        // Not a limit.
        assert!(plan_limit_of(&db, "codex", None, "The build failed", "error at line 429").is_none());
        // By the record: its time is kept.
        let seen = open_record(1, Some(4_000_000_000_000), true);
        let u = plan_limit_of(&db, "codex", Some(&seen), "It stopped", "").unwrap();
        assert_eq!(u.resets_at_ms, Some(4_000_000_000_000));
        // Words over a record that said "allowed": its reset time is not about this.
        let allowed = open_record(1, Some(4_000_000_000_000), false);
        let u = plan_limit_of(&db, "codex", Some(&allowed), "rate limit", "").unwrap();
        assert!(u.reached && u.resets_at_ms.is_none());
        // A new limit over an earlier one whose time has passed: that time would let the
        // agent back at once, so it is dropped and the new record holds.
        save(&db, "claude-code", &open_record(1, Some(2), true));
        let u = plan_limit_of(&db, "claude-code", None, "usage limit reached", "").unwrap();
        assert!(u.resets_at_ms.is_none() && u.holds(now_ms()), "{u:?}");
    }

    #[test]
    fn a_time_is_the_clock_today_and_the_day_after_that() {
        let day = 24 * 3_600_000;
        let noon = 20_000 * day + 12 * 3_600_000; // some day at 12:00 UTC
        assert_eq!(when_in(noon + 2 * 3_600_000 + 30 * 60_000, noon, &Utc), "14:30");
        let later = when_in(noon + 2 * day, noon, &Utc);
        assert!(later.ends_with("12:00") && later.len() > 5, "{later}");
    }
}
