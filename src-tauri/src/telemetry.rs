//! Opt-in, content-free telemetry plumbing (§6.3, NFR privacy).
//!
//! Disabled by default. When the user opts in, Nexus records only **aggregate
//! event counts** (e.g. how many chats were started) — never message content,
//! file paths, prompts, model names, or any personal data. Counts are kept
//! locally in the settings table; there is no network transmission in v1, so
//! "telemetry" here is strictly a local, inspectable tally that the plumbing is
//! ready to forward if the user ever opts into sharing.

use crate::db::Db;

pub const ENABLED_KEY: &str = "telemetry_enabled";
const COUNTS_KEY: &str = "telemetry_counts";

/// Whether the user has opted in.
pub fn is_enabled(db: &Db) -> bool {
    db.get_setting(ENABLED_KEY)
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false)
}

/// Record one content-free event. No-ops unless the user opted in. `event` must
/// be a fixed identifier (e.g. "app_open", "chat_started") — never user content.
pub fn record(db: &Db, event: &str) {
    if !is_enabled(db) {
        return;
    }
    let mut counts: serde_json::Map<String, serde_json::Value> = db
        .get_setting(COUNTS_KEY)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let next = counts.get(event).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
    counts.insert(event.to_string(), serde_json::json!(next));

    if let Ok(json) = serde_json::to_string(&counts) {
        let _ = db.set_setting(COUNTS_KEY, &json);
    }
}
