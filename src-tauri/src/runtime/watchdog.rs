//! The engine watchdog (HEAL-1): the app keeping its own runtime alive.
//!
//! llama-server can die — OOM, a driver reset, a stray `taskkill` — and until
//! now that left the app looking broken until the user noticed and restarted it
//! by hand. The watchdog notices instead, and puts it back.
//!
//! Two limits keep self-repair from becoming self-harm. It restarts only after
//! **three consecutive** failed health checks, so one slow response during a
//! long generation is not a crash. And it gives up after **three restarts in a
//! rolling hour**: an engine that keeps dying has a real problem, and looping on
//! it would burn VRAM and hide the fault.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager};

use super::RuntimeManager;

/// How often the engine is asked whether it is alive.
pub const POLL: Duration = Duration::from_secs(30);
/// Consecutive failed polls before we conclude the engine is gone.
pub const FAILURES_BEFORE_RESTART: u8 = 3;
/// Restarts allowed inside `LIMIT_WINDOW` before giving up.
pub const MAX_RESTARTS: usize = 3;
pub const LIMIT_WINDOW: Duration = Duration::from_secs(3600);

/// Wait before the nth restart of the hour: quick the first time (a transient
/// crash), then slower, so a genuinely broken engine isn't hammered.
pub fn backoff(prior_restarts: usize) -> Duration {
    match prior_restarts {
        0 => Duration::from_secs(2),
        1 => Duration::from_secs(10),
        _ => Duration::from_secs(30),
    }
}

/// May we restart again? Drops restarts that have aged out of the window, then
/// answers against what's left. Pure, so the limit is testable without a GPU.
pub fn may_restart(history: &mut VecDeque<Instant>, now: Instant) -> bool {
    while let Some(front) = history.front() {
        if now.duration_since(*front) > LIMIT_WINDOW {
            history.pop_front();
        } else {
            break;
        }
    }
    history.len() < MAX_RESTARTS
}

/// Watch the engine that is running now. Exits by itself once that engine is
/// superseded — a user-initiated stop or a new model bumps the generation, so
/// there is never more than one live watchdog.
pub fn spawn(app: tauri::AppHandle, generation: u64) {
    tauri::async_runtime::spawn(async move {
        let mut consecutive_failures = 0u8;
        loop {
            tokio::time::sleep(POLL).await;

            let Some(mgr) = app.try_state::<RuntimeManager>() else { return };
            if mgr.generation() != generation {
                return; // this engine is no longer the one in play
            }
            if mgr.engine_is_healthy().await {
                consecutive_failures = 0;
                continue;
            }
            consecutive_failures += 1;
            if consecutive_failures < FAILURES_BEFORE_RESTART {
                continue;
            }
            consecutive_failures = 0;

            match mgr.heal().await {
                HealResult::Restarted { attempt } => {
                    if let Some(db) = app.try_state::<crate::db::Db>() {
                        let _ = db.log_activity(None, "heal", "engine restarted (self-heal)");
                    }
                    let _ = app.emit(
                        "poiesis-healed",
                        serde_json::json!({ "attempt": attempt, "ok": true }),
                    );
                }
                HealResult::Failed { attempt } => {
                    if let Some(db) = app.try_state::<crate::db::Db>() {
                        let _ = db.log_activity(None, "heal", "engine restart failed");
                    }
                    let _ = app.emit(
                        "poiesis-healed",
                        serde_json::json!({ "attempt": attempt, "ok": false }),
                    );
                }
                // The user took over mid-repair. They don't need a toast about
                // an engine they just stopped, and the generation check at the
                // top of the loop will retire this watchdog on the next tick.
                HealResult::Superseded => return,
                HealResult::GaveUp => {
                    if let Some(db) = app.try_state::<crate::db::Db>() {
                        let _ = db.log_activity(None, "heal", "gave up restarting the engine");
                    }
                    let _ = app.emit(
                        "poiesis-healed",
                        serde_json::json!({ "attempt": MAX_RESTARTS, "ok": false }),
                    );
                    return; // the limit is the end of this watchdog's job
                }
            }
        }
    });
}

/// What one healing attempt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealResult {
    Restarted { attempt: u32 },
    Failed { attempt: u32 },
    /// The rolling-hour limit is spent — stop trying and say so.
    GaveUp,
    /// The user stopped or replaced the engine while we were restoring it.
    /// Their choice wins: nothing was put back, and nothing is said about it.
    Superseded,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rolling_hour_limits_restarts_without_ending_them_forever() {
        let now = Instant::now();
        let mut history = VecDeque::new();

        for i in 0..MAX_RESTARTS {
            assert!(may_restart(&mut history, now), "restart {i} should be allowed");
            history.push_back(now);
        }
        assert!(!may_restart(&mut history, now), "the 4th restart in an hour is refused");

        // An hour later the old restarts have aged out and healing resumes.
        let later = now + LIMIT_WINDOW + Duration::from_secs(1);
        assert!(may_restart(&mut history, later));
        assert!(history.is_empty(), "aged-out restarts are forgotten, not just ignored");
    }

    #[test]
    fn backoff_grows_then_settles() {
        assert_eq!(backoff(0), Duration::from_secs(2));
        assert_eq!(backoff(1), Duration::from_secs(10));
        assert_eq!(backoff(2), Duration::from_secs(30));
        assert_eq!(backoff(9), Duration::from_secs(30));
    }
}
