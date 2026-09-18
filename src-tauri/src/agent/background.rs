//! `SUB-10`..`SUB-12`: delegation that outlives the turn that asked for it.
//!
//! Foreground delegation (`SUB-5`) holds the lead's tool call open with
//! `join_all` until every child is finished. That is right when the lead needs
//! the answers to write its own, and wrong when it does not: a forty-minute
//! indexing job blocks the conversation for forty minutes, and the person who
//! asked for it sits there.
//!
//! A background child is the same `run_agent` call started from a **detached
//! task** instead. The tool returns run ids at once, the turn ends normally, and
//! the children keep working. Two things follow from that, and they are the
//! whole design:
//!
//! - **Nothing can be borrowed.** A detached task outlives the command that
//!   spawned it, so it cannot hold `ToolContext`'s references. Everything a
//!   child needs is carried in an owned `Spawn` and the rest is taken from the
//!   `AppHandle` inside the task — the same trade `media::jobs` makes, for the
//!   same reason.
//! - **Nobody is watching.** Background children are `headless: true`, so a
//!   write prompt they cannot answer is a refusal rather than a run that hangs
//!   until its clock runs out, and they may never delegate again: a fork bomb
//!   with no one in the room is the one failure mode that has no floor.
//!
//! Their events still reach the UI. `run_agent` writes to a `Channel`, and the
//! channel this module builds re-emits every event on the app bus instead of
//! down a webview invoke that has already returned. The Fleet card in the turn,
//! the Agents tab and the permission panel therefore keep working after the turn
//! is over, with no second code path on the frontend.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::agent::fleet::{Fleet, RunLimits, StopReason};
use crate::agent::run::{run_agent, AgentEventSink, RunContext};
use crate::agent::toolsets::Toolset;
use crate::agent::AgentEvent;
use crate::cloud::ChatEndpoint;
use crate::db::Db;
use crate::memory::MemoryStore;
use crate::permissions::PermissionManager;
use crate::runtime::{EmbedManager, RerankManager, RuntimeManager};

/// Set once at startup, exactly like `media::jobs`. A background child has no
/// command to borrow state from, and threading a handle through `run_agent`'s
/// twenty parameters to reach one toolset would be the worse trade.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Default children running at once. Three is what `subagents.max_parallel`
/// defaults to for the foreground, and the reason is the same: past it they
/// compete for one engine and each takes longer than all three would have.
const DEFAULT_CONCURRENT: usize = 3;
/// Hard ceiling, whatever Settings says.
const MAX_CONCURRENT: usize = 5;

/// Called once from `lib.rs` setup. Also settles children a crash or a close
/// left mid-flight: the queue is in memory, so they are not coming back, and a
/// row left saying "running" would have the Fleet card waiting on nothing.
pub fn init(app: AppHandle) {
    match app.state::<Db>().fail_interrupted_subagent_runs() {
        Ok(n) if n > 0 => eprintln!("agents: marked {n} interrupted child run(s) as stopped"),
        _ => {}
    }
    let _ = APP.set(app);
}

/// Is background delegation wired up at all? False in tests and in the `EVL`
/// harness, where `delegate {background: true}` falls back to running in the
/// foreground rather than dropping the work on the floor.
pub fn available() -> bool {
    APP.get().is_some()
}

/// Everything a background child needs that cannot be borrowed.
///
/// The run is already open in the `Fleet` and already has its `subagent_runs`
/// row before this is built: a queued child must be visible and stoppable while
/// it waits, not only once a slot frees up.
pub struct Spawn {
    pub run_id: String,
    pub child_conversation_id: String,
    pub agent: String,
    pub messages: Vec<serde_json::Value>,
    pub endpoint: ChatEndpoint,
    pub local_endpoint: Option<ChatEndpoint>,
    pub model_name: String,
    pub temperature: f32,
    /// The parent's effective toolsets: the ceiling this child may not exceed.
    pub toolsets: Vec<Toolset>,
    pub provenance: String,
    pub context_window: Option<usize>,
    pub max_steps: usize,
    /// Seconds, counted from when the child actually starts rather than from
    /// when it was queued — otherwise a full pool spends a child's whole budget
    /// on waiting.
    pub timeout_secs: u64,
}

#[derive(Default)]
struct Pool {
    running: usize,
    queue: VecDeque<Spawn>,
}

static POOL: OnceLock<Mutex<Pool>> = OnceLock::new();

fn pool() -> &'static Mutex<Pool> {
    POOL.get_or_init(|| Mutex::new(Pool::default()))
}

fn concurrency_limit(db: &Db) -> usize {
    db.get_setting("subagents.max_background")
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_CONCURRENT)
        .clamp(1, MAX_CONCURRENT)
}

/// Queue children and start as many as the pool has room for. Returns false
/// when there is no app to run them in, which is the caller's cue to do the
/// work in the foreground instead.
pub fn submit(spawns: Vec<Spawn>) -> bool {
    if APP.get().is_none() {
        return false;
    }
    if let Ok(mut p) = pool().lock() {
        p.queue.extend(spawns);
    }
    pump();
    true
}

/// How many are waiting for a slot right now. Only `check_agents` reads this;
/// the durable status of each child is its `subagent_runs` row.
pub fn queued() -> usize {
    pool().lock().map(|p| p.queue.len()).unwrap_or(0)
}

/// Start whatever the pool has room for. Called on submit and again as each
/// child finishes, so the queue drains without a timer.
fn pump() {
    let Some(app) = APP.get() else { return };
    let limit = concurrency_limit(&app.state::<Db>());
    loop {
        let next = match pool().lock() {
            Ok(mut p) if p.running < limit => p.queue.pop_front().inspect(|_| p.running += 1),
            _ => None,
        };
        let Some(spawn) = next else { break };
        tauri::async_runtime::spawn(async move {
            run_one(spawn).await;
            if let Ok(mut p) = pool().lock() {
                p.running = p.running.saturating_sub(1);
            }
            pump();
        });
    }
}

/// A channel whose other end is the app event bus rather than a webview invoke.
///
/// `Channel<AgentEvent>` serialises the event and hands this closure the JSON,
/// which is re-parsed once so the frontend receives a real object and can run it
/// through the same handlers a live run's events go through.
fn event_channel(app: AppHandle) -> tauri::ipc::Channel<AgentEvent> {
    tauri::ipc::Channel::new(move |body: tauri::ipc::InvokeResponseBody| {
        if let tauri::ipc::InvokeResponseBody::Json(text) = body {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                let _ = app.emit("poiesis-agent-sub", value);
            }
        }
        Ok(())
    })
}

async fn run_one(spawn: Spawn) {
    let Some(app) = APP.get() else { return };
    let db = app.state::<Db>();
    let fleet = app.state::<Fleet>();

    // The handle is gone when Stop already closed this run while it queued.
    let Some(run) = fleet.get(&spawn.run_id) else {
        let _ = db.finish_subagent_run(
            &spawn.run_id,
            "stopped",
            StopReason::Aborted.as_str(),
            "It was stopped before it started.",
            0,
        );
        announce_end(app, &db, &spawn.run_id);
        return;
    };
    if run.cancel.is_cancelled() {
        let _ = db.finish_subagent_run(
            &spawn.run_id,
            "stopped",
            StopReason::Aborted.as_str(),
            "It was stopped before it started.",
            0,
        );
        fleet.close(&spawn.run_id);
        announce_end(app, &db, &spawn.run_id);
        return;
    }

    let _ = db.set_subagent_status(&spawn.run_id, "running");

    let mgr = app.state::<RuntimeManager>();
    let embed_mgr = app.state::<EmbedManager>();
    let rerank_mgr = app.state::<RerankManager>();
    let perms = app.state::<PermissionManager>();
    let memory = app.state::<MemoryStore>();
    let data_dir = mgr.generated_media_dir();

    let channel = event_channel(app.clone());
    let parent_sink = AgentEventSink::new(channel);
    // Wrapped as `Sub { run_id, .. }` exactly like a foreground child's, so the
    // frontend needs no idea whether this run has a webview attached.
    let sink = parent_sink.child(&spawn.run_id);

    let limits = RunLimits {
        max_iterations: spawn.max_steps,
        deadline: Some(Instant::now() + Duration::from_secs(spawn.timeout_secs)),
        // Never. See the module doc.
        max_depth: 0,
    };
    let rc = RunContext {
        run: &run,
        limits: &limits,
        fleet: Some(&fleet),
        ceiling: Some(&spawn.toolsets),
        provenance: &spawn.provenance,
        context_window: spawn.context_window,
        // `PLN`: one plan per run. A child writes its own or none; it never
        // inherits its parent's.
        plan: None,
    };

    let outcome = run_agent(
        &mgr.client,
        &spawn.endpoint,
        spawn.local_endpoint.as_ref(),
        &db,
        &mgr,
        &embed_mgr,
        &rerank_mgr,
        &perms,
        &memory,
        // Headless, so the Browser toolset refuses before ever touching a pool.
        None,
        &spawn.child_conversation_id,
        None,
        &data_dir,
        &spawn.model_name,
        spawn.messages.clone(),
        spawn.temperature,
        true,
        true,
        &rc,
        &sink,
    )
    .await;

    let text = outcome.text.trim();
    let text = if text.is_empty() { "(it produced no text)" } else { text };
    let status = match outcome.stop_reason {
        StopReason::Completed => "done",
        StopReason::Aborted => "stopped",
        StopReason::Error => "error",
        _ => "done",
    };
    let _ = db.finish_subagent_run(
        &spawn.run_id,
        status,
        outcome.stop_reason.as_str(),
        text,
        outcome.steps,
    );
    // The lead's own turn is usually over by now, so this goes on the app bus.
    parent_sink.emit(AgentEvent::SubEnded {
        run_id: spawn.run_id.clone(),
        status: status.to_string(),
        stop_reason: outcome.stop_reason.as_str().to_string(),
        summary: text.to_string(),
        steps: outcome.steps,
        ms: run.elapsed_ms(),
    });
    fleet.close(&spawn.run_id);
    let _ = db.log_activity(
        Some(&spawn.child_conversation_id),
        "subagent",
        &format!("the {} agent finished in the background", spawn.agent),
    );
}

/// Report a child that ended before it ever ran. There is no sink for it —
/// nothing was ever started — so the event is built from the row.
fn announce_end(app: &AppHandle, db: &Db, run_id: &str) {
    let Ok(Some(row)) = db.get_subagent_run(run_id) else { return };
    let event = AgentEvent::SubEnded {
        run_id: row.id,
        status: row.status,
        stop_reason: row.stop_reason.unwrap_or_else(|| StopReason::Aborted.as_str().to_string()),
        summary: row.result.unwrap_or_default(),
        steps: row.steps,
        ms: 0,
    };
    if let Ok(value) = serde_json::to_value(&event) {
        let _ = app.emit("poiesis-agent-sub", value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pool's size is a clamp on a setting, never a rejection — and
    /// Settings can lower it but not raise it past the ceiling.
    #[test]
    fn the_pool_size_cannot_be_raised_past_the_ceiling() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(concurrency_limit(&db), DEFAULT_CONCURRENT);
        db.set_setting("subagents.max_background", "1").unwrap();
        assert_eq!(concurrency_limit(&db), 1);
        db.set_setting("subagents.max_background", "99").unwrap();
        assert_eq!(concurrency_limit(&db), MAX_CONCURRENT);
        db.set_setting("subagents.max_background", "0").unwrap();
        assert_eq!(concurrency_limit(&db), 1);
        db.set_setting("subagents.max_background", "nonsense").unwrap();
        assert_eq!(concurrency_limit(&db), DEFAULT_CONCURRENT);
    }

    /// With no app there is nothing to spawn into, and saying so is what lets
    /// `delegate` fall back to the foreground instead of losing the tasks.
    #[test]
    fn submitting_without_an_app_refuses_rather_than_dropping_the_work() {
        assert!(!available());
        assert!(!submit(Vec::new()));
    }
}
