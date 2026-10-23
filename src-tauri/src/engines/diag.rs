//! What an agent app did lately, kept in memory (`CLI-10`).
//!
//! The last 20 runs per agent: how many events this version of Poiesis did not
//! recognise, the last thing it wrote on stderr, the last error. A rising count
//! of unrecognised events is the early warning that an update changed the
//! protocol. Nothing here is written to disk and nothing leaves the machine.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

const KEEP: usize = 20;
/// Lines of stderr kept per run.
const STDERR_LINES: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub struct RunDiag {
    /// Epoch milliseconds.
    pub at: i64,
    pub seconds: f64,
    pub ok: bool,
    pub unknown_events: u32,
    pub error: Option<String>,
    /// The last lines the app wrote on stderr.
    pub stderr_tail: String,
    pub self_allowed: u32,
}

static LOG: OnceLock<Mutex<HashMap<String, VecDeque<RunDiag>>>> = OnceLock::new();

fn log() -> &'static Mutex<HashMap<String, VecDeque<RunDiag>>> {
    LOG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The last `STDERR_LINES` lines of a tail.
pub fn last_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(STDERR_LINES)..].join("\n")
}

pub fn record(engine_id: &str, run: RunDiag) {
    if let Ok(mut map) = log().lock() {
        let q = map.entry(engine_id.to_string()).or_default();
        q.push_front(run);
        q.truncate(KEEP);
    }
}

/// Newest first.
pub fn recent(engine_id: &str) -> Vec<RunDiag> {
    log().lock().ok().and_then(|m| m.get(engine_id).map(|q| q.iter().cloned().collect())).unwrap_or_default()
}

/// What Settings shows for one agent.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub runs: usize,
    pub failed: usize,
    /// Unrecognised events over the kept runs.
    pub unknown_events: u32,
    pub last_error: Option<String>,
    pub last_stderr: String,
}

pub fn summary(engine_id: &str) -> Summary {
    let runs = recent(engine_id);
    Summary {
        runs: runs.len(),
        failed: runs.iter().filter(|r| !r.ok).count(),
        unknown_events: runs.iter().map(|r| r.unknown_events).sum(),
        last_error: runs.iter().find_map(|r| r.error.clone()),
        last_stderr: runs.iter().find(|r| !r.stderr_tail.is_empty()).map(|r| r.stderr_tail.clone()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ok: bool, unknown: u32, error: Option<&str>, stderr: &str) -> RunDiag {
        RunDiag { at: 0, seconds: 1.0, ok, unknown_events: unknown, error: error.map(String::from), stderr_tail: stderr.into(), self_allowed: 0 }
    }

    #[test]
    fn only_the_last_twenty_runs_are_kept_newest_first() {
        for i in 0..25 {
            record("diag-test-a", run(true, i, None, ""));
        }
        let kept = recent("diag-test-a");
        assert_eq!(kept.len(), 20);
        assert_eq!(kept[0].unknown_events, 24, "newest first");
        assert_eq!(kept[19].unknown_events, 5);
    }

    #[test]
    fn the_summary_adds_up_the_runs_and_names_the_last_trouble() {
        record("diag-test-b", run(false, 2, Some("It crashed"), "boom"));
        record("diag-test-b", run(true, 1, None, ""));
        let s = summary("diag-test-b");
        assert_eq!((s.runs, s.failed, s.unknown_events), (2, 1, 3));
        assert_eq!(s.last_error.as_deref(), Some("It crashed"));
        assert_eq!(s.last_stderr, "boom");
        assert_eq!(summary("diag-test-never").runs, 0);
    }

    #[test]
    fn a_long_stderr_is_cut_to_its_last_twenty_lines() {
        let text = (1..=50).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let cut = last_lines(&text);
        assert_eq!(cut.lines().count(), 20);
        assert!(cut.starts_with("line 31") && cut.ends_with("line 50"));
    }
}
