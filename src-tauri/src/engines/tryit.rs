//! **Try it** (`CLI-13`): one real, tiny, tool-free turn in a fresh folder.
//!
//! It proves the whole path (start, protocol, sign-in, reading the answer)
//! before the user relies on the app in a real job. Nothing is asked of the
//! model that needs a tool, every approval request is declined, and the folder
//! is deleted afterwards.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::db::Db;
use crate::runtime::proxy::CancelFlag;

use super::driver::{driver_for, launch_for, DriverError, ExternalTurn, ModelChoice};
use super::{EngineKind, PaidBy};

const TASK: &str = "Reply with the single word ready.";
/// Generous: a first start loads the user's hooks and plugins, and a free model
/// can be slow.
const LIMIT: Duration = Duration::from_secs(120);

#[derive(Debug, Serialize)]
pub struct TryResult {
    pub ok: bool,
    pub reply: String,
    pub seconds: f64,
    pub paid_by: Option<PaidBy>,
    pub model: Option<String>,
    /// What went wrong, in words the user can act on.
    pub error: Option<String>,
    /// Set when the problem is a missing sign-in.
    pub needs_sign_in: bool,
}

pub async fn run(db: &Db, id: &str, cancel: CancelFlag) -> Result<TryResult, String> {
    let row = db.get_engine(id).map_err(|e| e.to_string())?.ok_or("I don't know that agent.")?;
    let kind = EngineKind::parse(&row.kind).ok_or("Unknown kind of agent.")?;
    let started = Instant::now();
    let failed = |error: String, needs_sign_in: bool| TryResult {
        ok: false,
        reply: String::new(),
        seconds: started.elapsed().as_secs_f64(),
        paid_by: None,
        model: None,
        error: Some(error),
        needs_sign_in,
    };
    let launch = match launch_for(&row) {
        Ok(l) => l,
        Err(e) => return Ok(failed(e.to_string(), false)),
    };

    let dir = std::env::temp_dir().join(format!("poiesis-try-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("I couldn't make a scratch folder: {e}"))?;
    let outcome = driver_for(kind)
        .run_turn(ExternalTurn { launch: &launch, task: TASK.into(), cwd: dir.clone(), tool_free: true, cancel, timeout: LIMIT, job: None })
        .await;
    let _ = std::fs::remove_dir_all(&dir);

    let seconds = started.elapsed().as_secs_f64();
    Ok(match outcome {
        Ok(o) if !o.text.trim().is_empty() => TryResult {
            ok: true,
            reply: o.text.trim().chars().take(200).collect(),
            seconds,
            paid_by: o.paid_by,
            model: o.model,
            error: None,
            needs_sign_in: false,
        },
        Ok(_) => failed("It finished but said nothing.".into(), false),
        Err(e) => failed(e.to_string(), matches!(e, DriverError::NeedsSignIn(_))),
    })
}

/// The models to offer in the picker. Empty = no picker.
pub async fn models(db: &Db, id: &str) -> Result<Vec<ModelChoice>, String> {
    let row = db.get_engine(id).map_err(|e| e.to_string())?.ok_or("I don't know that agent.")?;
    let kind = EngineKind::parse(&row.kind).ok_or("Unknown kind of agent.")?;
    let launch = launch_for(&row).map_err(|e| e.to_string())?;
    driver_for(kind).list_models(&launch).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::{self, locate::SearchRoots};

    /// Runs the real thing: finds, probes and tries whatever is installed on
    /// this computer. Each try is one tiny tool-free turn on the user's own
    /// plan. `POIESIS_TEST_CLAUDE` may point at a `claude.exe` that is not on
    /// the PATH. Run with:
    /// `cargo test --lib real_agents -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "needs the real agent apps installed and signed in"]
    async fn real_agents_can_be_found_probed_and_tried() {
        let db = Db::open_in_memory().unwrap();
        engines::detect_all(&db, 0).await.unwrap();
        if let Ok(p) = std::env::var("POIESIS_TEST_CLAUDE") {
            let mut row = engines::ensure_row(&db, "claude-code").unwrap();
            row.program = Some(p);
            db.save_engine_settings(&row).unwrap();
            engines::refresh(&db, &SearchRoots::from_system(), "claude-code", 0).await.unwrap();
        }
        for info in engines::list_infos(&db).unwrap() {
            println!("{}: found={} version={:?} tested={:?} signed_in={:?} paid_by={:?} error={:?}",
                info.id, info.found, info.version, info.tested, info.signed_in, info.paid_by, info.last_error);
            if !info.found {
                continue;
            }
            let r = run(&db, &info.id, CancelFlag::new()).await.unwrap();
            println!("  try it: ok={} reply={:?} {:.1}s paid_by={:?} model={:?} error={:?}",
                r.ok, r.reply, r.seconds, r.paid_by, r.model, r.error);
            let models = models(&db, &info.id).await;
            println!("  models: {:?}", models.map(|m| m.len()));
        }
    }
}
