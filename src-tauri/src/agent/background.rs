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
use crate::agent::team::{Seat, Slots};
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
    /// `PLF-2`: the lead was planning first, so this child may change nothing.
    pub read_only: bool,
    /// `REM-4`: the lead's turn came from outside, so this child asks more too.
    pub remote: bool,
    pub max_steps: usize,
    /// Seconds, counted from when the child actually starts rather than from
    /// when it was queued — otherwise a full pool spends a child's whole budget
    /// on waiting.
    pub timeout_secs: u64,
    /// `CLI-4`: set when an agent app does this child's work instead of the loop.
    pub engine: Option<EngineSpawn>,
    /// `TEAM-10`: set for a team member. It waits for a place in its team's queue (at most
    /// `limits.at_once` at a time) instead of one of the pool's, and announces when it starts
    /// and ends.
    pub seat: Option<Seat>,
    /// `PRF-5`: the folder it works in and the places in it it holds while it works (empty: all
    /// of it). `None` holds nothing.
    pub claim: Option<(std::path::PathBuf, Vec<String>)>,
}

/// What an agent-app child needs that cannot be borrowed.
pub struct EngineSpawn {
    pub offered: crate::engines::rules::Offered,
    /// The lead's task, with the shape of answer it asked for.
    pub task: String,
    pub folder: std::path::PathBuf,
    pub folder_trust: crate::permissions::Trust,
    /// `XA-13`: an outside session this child continues (`/resume`).
    pub resume: Option<String>,
}

#[derive(Default)]
struct Pool {
    running: usize,
    queue: VecDeque<Spawn>,
    /// `TEAM-10`: member runs going, per team session.
    slots: Slots,
}

impl Pool {
    /// The next child that may start, if any: the first in the queue whose turn it is. A
    /// team member waits for a place in its own team (`at_once`), whatever the rest of the
    /// pool is doing; everyone else for a place in the pool. Queue order is kept within each.
    fn next(&mut self, limit: usize) -> Option<Spawn> {
        let at = self.queue.iter().position(|s| match &s.seat {
            Some(seat) => self.slots.has_room(seat),
            None => self.running < limit,
        })?;
        let spawn = self.queue.remove(at)?;
        match &spawn.seat {
            Some(seat) => self.slots.take(seat),
            None => self.running += 1,
        }
        Some(spawn)
    }

    /// A child that `next` handed out has ended.
    fn ended(&mut self, seat: Option<&Seat>) {
        match seat {
            Some(seat) => self.slots.free(seat),
            None => self.running = self.running.saturating_sub(1),
        }
    }
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
        let next = pool().lock().ok().and_then(|mut p| p.next(limit));
        let Some(spawn) = next else { break };
        let seat = spawn.seat.clone();
        tauri::async_runtime::spawn(async move {
            run_one(spawn).await;
            if let Ok(mut p) = pool().lock() {
                p.ended(seat.as_ref());
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
    // `TEAM-12`: a member runs under its task's allowance. When that ran out while it queued,
    // that is why it never started.
    let allowance = spawn.seat.as_ref().and_then(|s| crate::agent::team::allowance_of(&s.session));
    if run.cancel.is_cancelled() {
        let why = if allowance.as_ref().is_some_and(|a| a.spent_out().is_some()) { StopReason::Budget } else { StopReason::Aborted };
        let _ = db.finish_subagent_run(
            &spawn.run_id,
            "stopped",
            why.as_str(),
            "It was stopped before it started.",
            0,
        );
        fleet.close(&spawn.run_id);
        announce_end(app, &db, &spawn.run_id);
        return;
    }

    // `PRF-5`: not in a place another helper is changing. Held until the child ends.
    let _claim = match &spawn.claim {
        Some((at, paths)) => Some(
            crate::agent::subagents::scopes()
                .claim(&spawn.run_id, &crate::agent::subagents::leads_of(&fleet, &run), &spawn.agent, at, paths, &run.cancel, |because| {
                    let _ = db.set_subagent_queued_because(&spawn.run_id, because);
                    let _ = db.set_subagent_status(&spawn.run_id, if because.is_some() { "queued" } else { "running" });
                    announce_waiting(app, &spawn.run_id, because);
                })
                .await,
        ),
        None => None,
    };
    let _ = db.set_subagent_status(&spawn.run_id, "running");

    let mgr = app.state::<RuntimeManager>();
    let embed_mgr = app.state::<EmbedManager>();
    let rerank_mgr = app.state::<RerankManager>();
    let perms = app.state::<PermissionManager>();
    let memory = app.state::<MemoryStore>();
    let data_dir = mgr.generated_media_dir();

    let channel = event_channel(app.clone());
    let parent_sink = AgentEventSink::new(channel);
    if let Some(seat) = &spawn.seat {
        parent_sink.emit(AgentEvent::MemberStarted { run_id: spawn.run_id.clone(), member: seat.role.clone(), item: seat.item });
    }
    // Wrapped as `Sub { run_id, .. }` exactly like a foreground child's, so the
    // frontend needs no idea whether this run has a webview attached.
    let sink = parent_sink.child(&spawn.run_id);

    let limits = RunLimits {
        max_iterations: spawn.max_steps,
        deadline: Some(Instant::now() + Duration::from_secs(spawn.timeout_secs)),
        // Never. See the module doc.
        max_depth: 0,
        plan_ceiling: None,
        // `TEAM-12`: a member runs under its task's allowance, so what it costs counts and
        // the limit stops it.
        allowance: allowance.clone(),
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
        options: Default::default(),
        read_only: spawn.read_only,
        remote: spawn.remote,
        // Only a Full run can hand work out (`TIER-5`), so what it hands out is Full.
        tier: crate::agent::tier::ModelTier::Full,
    };

    let outcome = if let Some(e) = &spawn.engine {
        // An agent app, not the loop. Not headless: the user is in the app (a
        // headless parent never gets here), so what it asks reaches the panel,
        // attributed to it, instead of being refused.
        crate::engines::run::run_external(crate::engines::run::Request {
            db: &db,
            perms: &perms,
            sink: &sink,
            run: &run,
            offered: &e.offered,
            child_conversation_id: &spawn.child_conversation_id,
            task: &e.task,
            folder: &e.folder,
            folder_trust: e.folder_trust,
            headless: false,
            remote: spawn.remote,
            resume: e.resume.clone(),
        })
        .await
    } else {
        run_agent(
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
        .await
    };

    let text = outcome.text.trim();
    let text = if text.is_empty() { "(it produced no text)" } else { text };
    let status = match outcome.stop_reason {
        StopReason::Completed => "done",
        StopReason::Aborted | StopReason::Budget => "stopped",
        StopReason::Error | StopReason::PlanLimit => "error",
        _ => "done",
    };
    let _ = db.finish_subagent_run(
        &spawn.run_id,
        status,
        outcome.stop_reason.as_str(),
        text,
        outcome.steps,
    );
    // `TEAM-15`: what a member found goes to the coordinator as a post, on the app bus like the rest.
    crate::agent::team::record_end(&db, &fleet, &parent_sink, &spawn.run_id);
    // The lead's own turn is usually over by now, so this goes on the app bus.
    parent_sink.emit(AgentEvent::SubEnded {
        run_id: spawn.run_id.clone(),
        status: status.to_string(),
        stop_reason: outcome.stop_reason.as_str().to_string(),
        summary: text.to_string(),
        steps: outcome.steps,
        ms: run.elapsed_ms(),
        evidence: db.get_subagent_run(&spawn.run_id).ok().flatten().and_then(|r| r.evidence),
    });
    fleet.close(&spawn.run_id);
    if let Some(seat) = &spawn.seat {
        parent_sink.emit(AgentEvent::MemberEnded { run_id: spawn.run_id.clone(), member: seat.role.clone(), item: seat.item });
    }
    let _ = db.log_activity(
        Some(&spawn.child_conversation_id),
        "subagent",
        &format!("the {} agent finished in the background", spawn.agent),
    );
}

/// Tell the screen why a child waits (`PRF-5`), on the app bus: no turn is listening.
fn announce_waiting(app: &AppHandle, run_id: &str, because: Option<&str>) {
    let event = AgentEvent::SubWaiting { run_id: run_id.to_string(), because: because.map(String::from) };
    if let Ok(value) = serde_json::to_value(&event) {
        let _ = app.emit("poiesis-agent-sub", value);
    }
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
        evidence: row.evidence,
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

    fn spawn(run_id: &str, seat: Option<Seat>) -> Spawn {
        Spawn {
            run_id: run_id.into(),
            child_conversation_id: "c".into(),
            agent: "a".into(),
            messages: Vec::new(),
            endpoint: ChatEndpoint::OpenAi { base_url: String::new(), api_key: None, model: None },
            local_endpoint: None,
            model_name: String::new(),
            temperature: 0.0,
            toolsets: Vec::new(),
            provenance: String::new(),
            context_window: None,
            read_only: false,
            remote: false,
            max_steps: 1,
            timeout_secs: 1,
            engine: None,
            seat,
            claim: None,
        }
    }

    fn seat(session: &str, role: &str, at_once: usize) -> Seat {
        Seat { session: session.into(), at_once, role: role.into(), item: None }
    }

    /// `TEAM-10`: a team starts at most `at_once` members, the rest wait in order and start as
    /// one ends. Other helpers keep to the pool's own limit and are not held up by the team.
    #[test]
    fn a_team_runs_at_most_at_once_members_and_the_rest_wait_in_order() {
        let mut pool = Pool::default();
        for (id, role) in [("r1", "api"), ("r2", "ui"), ("r3", "docs"), ("r4", "qa")] {
            pool.queue.push_back(spawn(id, Some(seat("s1", role, 2))));
        }
        pool.queue.push_back(spawn("h1", None));
        let started: Vec<String> = std::iter::from_fn(|| pool.next(1)).map(|s| s.run_id).collect();
        // Two members (the first two), and the plain helper, which has its own place in the pool.
        assert_eq!(started, vec!["r1", "r2", "h1"]);
        assert_eq!(pool.queue.iter().map(|s| s.run_id.as_str()).collect::<Vec<_>>(), vec!["r3", "r4"]);

        pool.ended(Some(&seat("s1", "api", 2)));
        assert_eq!(pool.next(1).map(|s| s.run_id), Some("r3".to_string()), "the next in line, not r4");
        assert!(pool.next(1).is_none(), "two are going again");
    }

    /// With no app there is nothing to spawn into, and saying so is what lets
    /// `delegate` fall back to the foreground instead of losing the tasks.
    #[test]
    fn submitting_without_an_app_refuses_rather_than_dropping_the_work() {
        assert!(!available());
        assert!(!submit(Vec::new()));
    }
}
