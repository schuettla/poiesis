//! SCH — the quiet night shift (Part VI). A single process-wide 60-second
//! ticker drives small, named jobs while nobody is watching: `autonomy.rs` is
//! the membrane this reuses (`Auto` applies with undo, `Ask` becomes a
//! `change_proposals` row waiting in the morning, `Off` is skipped — nothing
//! here widens what a self-change class may already do), and `run_agent`'s
//! `headless` flag (SCH-3) is what makes an unattended run safe: no renders
//! (`RND-3`), and the File System skill refuses every write/edit/delete/move
//! outright rather than opening a permission prompt nobody could answer.
//!
//! Concurrency is deliberately **1** — one local GPU serialises generation
//! anyway — so a second due job just waits for the next tick. Jobs live as a
//! JSON blob in `settings`, not a new table (SCH-2): there is at most a
//! handful of them, and they change shape too easily during development to
//! want a migration for it.

use std::sync::Mutex as StdMutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::agent::run::{run_agent, AgentEventSink};
use crate::cloud::ChatEndpoint;
use crate::commands::reflect::reflect_conversation_cmd;
use crate::db::Db;
use crate::memory::MemoryStore;
use crate::permissions::PermissionManager;
use crate::runtime::proxy::CancelFlag;
use crate::runtime::{EmbedManager, RerankManager, RuntimeManager};
use crate::PoiesisError;

const JOBS_KEY: &str = "scheduler.jobs";
const DIGEST_KEY: &str = "scheduler.digest";
const TICK: std::time::Duration = std::time::Duration::from_secs(60);
const BUILT_IN_NIGHTLY_ID: &str = "nightly-reflection";
/// How many not-yet-reflected conversations one nightly pass reads back over —
/// a cap, not a target, so a heavy day doesn't turn "the quiet night shift"
/// into a long GPU-bound run.
const NIGHTLY_CONVERSATION_CAP: usize = 8;
/// A custom job's headless answer is stored as `last_result` for the Self
/// panel — long enough to be useful, short enough that a runaway answer
/// doesn't bloat the settings row.
const RESULT_CLIP: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cadence {
    Hourly,
    SixHourly,
    Daily,
    Weekly,
}

impl Cadence {
    fn period_ms(self) -> i64 {
        match self {
            Cadence::Hourly => 3_600_000,
            Cadence::SixHourly => 6 * 3_600_000,
            Cadence::Daily => 24 * 3_600_000,
            Cadence::Weekly => 7 * 24 * 3_600_000,
        }
    }
}

/// One scheduled job (SCH-2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub cadence: Cadence,
    /// Folder this job's tools may reach, if any. `None` for the built-in
    /// nightly job, which reads finished conversations, not files.
    pub scope: Option<String>,
    pub enabled: bool,
    pub next_run_at: i64,
    pub last_run_at: Option<i64>,
    pub last_result: Option<String>,
    /// True only for the seeded nightly-reflection job (SCH-5): it can be
    /// turned off but never deleted, and `run_job` special-cases it rather
    /// than running its (empty) `prompt` through the agent loop.
    #[serde(default)]
    pub built_in: bool,
    /// The conversation this task was made from, when it was made from one.
    /// Kept so the task can link back to where the idea came from.
    #[serde(default)]
    pub source_conversation_id: Option<String>,
    /// The last few runs, newest first. Each run is a real conversation in the
    /// rail: a task whose output you can't read is a black box, and a 400-char
    /// summary string was exactly that.
    #[serde(default)]
    pub runs: Vec<JobRun>,
}

/// One completed run of a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRun {
    /// The conversation this run happened in — open it to read what it did.
    pub conversation_id: String,
    pub at: i64,
    /// One line for the task list, so the history is skimmable without
    /// opening every run.
    pub summary: String,
}

/// How many runs a task remembers. Older conversations aren't deleted — they
/// stay in the rail like any other — this is just how far back the task's own
/// list reaches.
const RUNS_KEPT: usize = 10;

fn seed_nightly_job() -> Job {
    Job {
        id: BUILT_IN_NIGHTLY_ID.to_string(),
        name: "Nightly reflection".to_string(),
        prompt: String::new(),
        cadence: Cadence::Daily,
        scope: None,
        enabled: false, // SCH-5: off by default
        next_run_at: 0,
        last_run_at: None,
        last_result: None,
        built_in: true,
        source_conversation_id: None,
        runs: Vec::new(),
    }
}

fn load_jobs(db: &Db) -> Vec<Job> {
    let mut jobs: Vec<Job> = db
        .get_setting(JOBS_KEY)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if !jobs.iter().any(|j| j.id == BUILT_IN_NIGHTLY_ID) {
        jobs.push(seed_nightly_job());
        let _ = save_jobs(db, &jobs);
    }
    jobs
}

fn save_jobs(db: &Db, jobs: &[Job]) -> Result<(), String> {
    let json = serde_json::to_string(jobs).map_err(|e| e.to_string())?;
    db.set_setting(JOBS_KEY, &json).map_err(|e| e.to_string())
}

/// The most recent nightly first-person summary (SCH-UI-1), and whether the
/// Self panel has shown it yet (SCH-UI-2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Digest {
    pub text: String,
    pub created_at: i64,
    pub unread: bool,
}

fn load_digest(db: &Db) -> Option<Digest> {
    db.get_setting(DIGEST_KEY).ok().flatten().and_then(|s| serde_json::from_str(&s).ok())
}

fn save_digest(db: &Db, text: &str) {
    let digest = Digest { text: text.to_string(), created_at: crate::db::now_ms(), unread: true };
    if let Ok(json) = serde_json::to_string(&digest) {
        let _ = db.set_setting(DIGEST_KEY, &json);
    }
}

/// What the Rail shows while a job runs (SCH-UI-4): enough to say what's
/// happening, and a flag the user can raise to end it.
#[derive(Debug, Clone, Serialize)]
pub struct RunningJob {
    pub job_id: String,
    pub job_name: String,
    pub started_at: i64,
    #[serde(skip)]
    cancel: CancelFlag,
}

/// Concurrency 1 (SCH-1): one local GPU serialises generation anyway, so at
/// most one job runs at a time. A second due job simply waits for a later
/// tick rather than racing the first for the engine.
#[derive(Default)]
pub struct SchedulerState {
    running: StdMutex<Option<RunningJob>>,
}

impl SchedulerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim the one run slot for `job`, or `None` if something is already in it.
    fn try_start(&self, job: &Job) -> Option<CancelFlag> {
        let mut guard = self.running.lock().unwrap();
        if guard.is_some() {
            return None;
        }
        let cancel = CancelFlag::new();
        *guard = Some(RunningJob {
            job_id: job.id.clone(),
            job_name: job.name.clone(),
            started_at: crate::db::now_ms(),
            cancel: cancel.clone(),
        });
        Some(cancel)
    }

    fn finish(&self) {
        *self.running.lock().unwrap() = None;
    }

    pub fn status(&self) -> Option<RunningJob> {
        self.running.lock().unwrap().clone()
    }

    /// Cancel the run in progress, if any (SCH-UI-4's Stop). Returns whether
    /// there was one to cancel.
    pub fn stop(&self) -> bool {
        match self.running.lock().unwrap().as_ref() {
            Some(r) => {
                r.cancel.cancel();
                true
            }
            None => false,
        }
    }
}

/// Run one job to completion and persist its new schedule + result. Shared by
/// the ticker and the manual "Run now" command (SCH-UI-3), so both go through
/// the same concurrency-1 guard.
async fn execute_job(app: &AppHandle, job_id: &str) -> Result<String, String> {
    let db = app.state::<Db>();
    let sched = app.state::<SchedulerState>();

    let Some(mut job) = load_jobs(&db).into_iter().find(|j| j.id == job_id) else {
        return Err("That job no longer exists.".to_string());
    };

    let Some(cancel) = sched.try_start(&job) else {
        return Err("Another scheduled job is already running.".to_string());
    };
    // SCH-UI-4: the Rail has no other way to learn a job just started — there
    // is no open channel for a run nobody invoked from the UI.
    let _ = app.emit(
        "poiesis-job-started",
        serde_json::json!({ "job_id": job.id, "job_name": job.name }),
    );

    let built_in = job.built_in;
    let outcome = if built_in {
        RunOutcome::bare(run_nightly_reflection_digest(app, cancel).await)
    } else {
        run_custom_job(app, &mut job, cancel).await
    };
    let result = outcome.summary;

    sched.finish();

    // Re-read rather than writing back the snapshot loaded before the run: a
    // run takes minutes, and anything the user created, edited, disabled or
    // deleted in that window would otherwise be silently undone the moment the
    // job finished. Only this job's own bookkeeping is written, onto whatever
    // the jobs list looks like *now* — including a cadence changed mid-run,
    // which is what the next run should be measured against.
    let db = app.state::<Db>();
    let mut jobs = load_jobs(&db);
    if let Some(current) = jobs.iter_mut().find(|j| j.id == job_id) {
        let now = crate::db::now_ms();
        current.last_run_at = Some(now);
        current.next_run_at = now + current.cadence.period_ms();
        current.last_result = Some(result.clone());
        if let Some(conversation_id) = outcome.conversation_id.clone() {
            current.runs.insert(
                0,
                JobRun { conversation_id, at: now, summary: result.clone() },
            );
            current.runs.truncate(RUNS_KEPT);
        }
        let _ = save_jobs(&db, &jobs);
    }
    let _ = app.emit(
        "poiesis-job-finished",
        serde_json::json!({ "job_id": job_id, "result": result, "built_in": built_in }),
    );
    Ok(result)
}

/// SCH-5: the built-in job. Reads back over the day's not-yet-reflected
/// conversations (`CRT` still gates what each pass actually writes), then
/// composes a short first-person digest. Reflection reads conversations and
/// writes lessons/proposals — it never touches a file, so the exit criterion
/// ("an activity log showing the run touched no files") holds by construction.
///
/// `cancel` is checked between conversations (SCH-UI-4). Reflection itself is
/// one indivisible model call, so Stop finishes the conversation in flight and
/// then ends — it can't abandon one half-read — and the digest says how far it
/// actually got rather than implying the whole night's work happened.
async fn run_nightly_reflection_digest(app: &AppHandle, cancel: CancelFlag) -> String {
    // TTL-2: let short-lived facts go as part of the same quiet pass that
    // already runs overnight, not only at app startup.
    {
        let db = app.state::<Db>();
        let mem = app.state::<MemoryStore>();
        let swept = mem.sweep_expired(&db);
        if !swept.is_empty() {
            let _ = db.log_activity(None, "memory", &format!("let {} expired notes go", swept.len()));
            let _ = app.emit("poiesis-expiry-swept", serde_json::json!({ "count": swept.len() }));
        }
    }

    let due: Vec<String> = app
        .state::<Db>()
        .list_conversations()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.reflected_at.is_none())
        .take(NIGHTLY_CONVERSATION_CAP)
        .map(|c| c.id)
        .collect();

    if due.is_empty() {
        let text = "Last night I looked back over the day but didn't find anything new to reflect on.";
        save_digest(&app.state::<Db>(), text);
        return "0 conversations".to_string();
    }

    let mut learned = 0usize;
    let mut proposed = 0usize;
    let mut read = 0usize;
    for conversation_id in &due {
        if cancel.is_cancelled() {
            break;
        }
        // `State` isn't `Copy`, so each call gets its own fresh lookup —
        // cheap (it's just indexing into the app's managed-state map).
        let outcome = reflect_conversation_cmd(
            app.state::<RuntimeManager>(),
            app.state::<Db>(),
            app.state::<MemoryStore>(),
            app.clone(),
            conversation_id.clone(),
            None,
        )
        .await;
        read += 1;
        if let Ok(r) = outcome {
            learned += r.saved.len();
            proposed += r.proposed.len();
        }
    }

    let n = read;
    let plural = if n == 1 { "" } else { "s" };
    let digest = compose_digest(read, due.len(), learned, proposed);
    let db = app.state::<Db>();
    save_digest(&db, &digest);
    let _ = db.log_activity(
        None,
        "scheduler",
        &format!("nightly reflection: read {n} conversation{plural}, learned {learned}, {proposed} pending"),
    );

    format!("{n} conversation{plural}, {learned} learned, {proposed} pending")
}

/// The digest sentence, kept apart from the run that produces it so the wording
/// can be tested without a model behind it. `read < total` means Stop ended the
/// pass early: saying "I read back over the day" then would claim work that
/// didn't happen, which is the one thing a digest must never do.
fn compose_digest(read: usize, total: usize, learned: usize, proposed: usize) -> String {
    if read == 0 {
        return if total == 0 {
            "Last night I looked back over the day but didn't find anything new to reflect on."
                .to_string()
        } else {
            "You stopped me last night before I'd read anything back.".to_string()
        };
    }
    let plural = if read == 1 { "" } else { "s" };
    let lead = if read < total {
        format!("Last night I read back over {read} conversation{plural} before you stopped me")
    } else {
        format!("Last night I read back over {read} conversation{plural}")
    };
    let learn_clause = match learned {
        0 => "didn't find anything new to learn".to_string(),
        1 => "learned one thing".to_string(),
        k => format!("learned {k} things"),
    };
    match proposed {
        0 => format!("{lead}. I {learn_clause}."),
        1 => format!("{lead}. I {learn_clause}, and there's one change I'd like to make."),
        k => format!("{lead}. I {learn_clause}, and there are {k} changes I'd like to make."),
    }
}

/// What one run produced: the line the task list shows, and the conversation
/// it happened in (`None` for the built-in job, which writes a digest instead).
struct RunOutcome {
    summary: String,
    conversation_id: Option<String>,
}

impl RunOutcome {
    /// A run that never reached a conversation — no engine, no room to work.
    fn bare(summary: impl Into<String>) -> Self {
        Self { summary: summary.into(), conversation_id: None }
    }
}

/// A user-defined task (SCH-2): one headless turn of the real agent loop in a
/// **fresh conversation per run**, so every run is something you can open and
/// read in the rail rather than a summary string you have to trust.
/// `headless: true` is what makes this safe to leave unattended (SCH-3) — see
/// the module doc.
async fn run_custom_job(app: &AppHandle, job: &mut Job, cancel: CancelFlag) -> RunOutcome {
    let db = app.state::<Db>();
    let mgr = app.state::<RuntimeManager>();
    let embed_mgr = app.state::<EmbedManager>();
    let rerank_mgr = app.state::<RerankManager>();
    let perms = app.state::<PermissionManager>();
    let memory = app.state::<MemoryStore>();

    let Some((base_url, token)) = mgr.engine_endpoint().await else {
        return RunOutcome::bare("No model is loaded, so this run was skipped.");
    };
    let endpoint = ChatEndpoint::OpenAi { base_url, api_key: Some(token), model: None };

    // A new conversation each run, numbered so a week of runs doesn't read as
    // one title repeated down the rail.
    let title = format!("{} · run {}", job.name, job.runs.len() + 1);
    let conversation_id = match db.create_conversation(&title, None, false) {
        Ok(conv) => conv.id,
        Err(e) => return RunOutcome::bare(format!("Couldn't set up a run for this task: {e}")),
    };
    if let Some(scope) = &job.scope {
        let _ = db.set_conversation_folder(&conversation_id, Some(scope));
    }

    // No live webview is listening — the run's answer comes back as the
    // return value, not a stream, so an empty handler is enough.
    let sink = AgentEventSink::new(tauri::ipc::Channel::new(|_| Ok(())));
    let messages = vec![serde_json::json!({ "role": "user", "content": &job.prompt })];
    let images_dir = mgr.generated_media_dir();
    let model_name = mgr.engine_model_name().await.unwrap_or_else(|| "local".to_string());

    let text = run_agent(
        &mgr.client,
        &endpoint,
        Some(&endpoint),
        &db,
        &mgr,
        &embed_mgr,
        &rerank_mgr,
        &perms,
        &memory,
        // Headless: the Browser toolset refuses outright before ever
        // touching a pool (`browser::execute`'s `ctx.headless` check).
        None,
        &conversation_id,
        None,
        &images_dir,
        &model_name,
        messages,
        0.4,
        true,
        true,
        cancel,
        &sink,
    )
    .await;

    let _ = db.log_activity(
        Some(&conversation_id),
        "scheduler",
        &format!("ran task \u{201c}{}\u{201d}", job.name),
    );

    // The summary is only the list line now — the full answer lives in the
    // conversation, so clipping it here loses nothing.
    let summary = if text.trim().is_empty() {
        "Finished with no output.".to_string()
    } else {
        text.chars().take(RESULT_CLIP).collect()
    };
    RunOutcome { summary, conversation_id: Some(conversation_id) }
}

/// Background 60-second ticker (SCH-1) — the process-wide clock every job's
/// cadence is measured against. No cron dependency: a job simply becomes due
/// when `next_run_at` is in the past, and this loop is the only thing that
/// ever checks.
pub fn spawn_ticker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            let Some(db) = app.try_state::<Db>() else { return };
            let now = crate::db::now_ms();
            let due = load_jobs(&db)
                .into_iter()
                .find(|j| j.enabled && j.next_run_at <= now)
                .map(|j| j.id);
            drop(db);
            if let Some(job_id) = due {
                let _ = execute_job(&app, &job_id).await;
            }
        }
    });
}

// ---- Tauri commands ----

#[tauri::command]
pub fn list_scheduler_jobs_cmd(db: State<'_, Db>) -> Vec<Job> {
    load_jobs(&db)
}

/// What a job-editing form submits (SCH-UI-3): everything but the id and the
/// bookkeeping fields the backend owns.
#[derive(Debug, Deserialize)]
pub struct JobInput {
    pub name: String,
    pub prompt: String,
    pub cadence: Cadence,
    pub scope: Option<String>,
    pub enabled: bool,
    /// Set when the task was made out of an open chat ("Schedule this"), so it
    /// can link back to where the idea came from.
    #[serde(default)]
    pub source_conversation_id: Option<String>,
}

#[tauri::command]
pub fn create_scheduler_job_cmd(db: State<'_, Db>, input: JobInput) -> Result<Job, PoiesisError> {
    let mut jobs = load_jobs(&db);
    let now = crate::db::now_ms();
    // Enabling schedules the first run for the very next tick rather than a
    // full cadence period out — "enable the job, leave the app open
    // overnight" only works if enabling doesn't make the user wait a day.
    let next_run_at = if input.enabled { now } else { now + input.cadence.period_ms() };
    let job = Job {
        id: format!("job_{}", uuid::Uuid::new_v4()),
        name: input.name,
        prompt: input.prompt,
        cadence: input.cadence,
        scope: input.scope,
        enabled: input.enabled,
        next_run_at,
        last_run_at: None,
        last_result: None,
        built_in: false,
        source_conversation_id: input.source_conversation_id,
        runs: Vec::new(),
    };
    jobs.push(job.clone());
    save_jobs(&db, &jobs).map_err(PoiesisError::Message)?;
    Ok(job)
}

#[tauri::command]
pub fn update_scheduler_job_cmd(
    db: State<'_, Db>,
    id: String,
    input: JobInput,
) -> Result<Job, PoiesisError> {
    let mut jobs = load_jobs(&db);
    let Some(existing) = jobs.iter_mut().find(|j| j.id == id) else {
        return Err(PoiesisError::Message("That job no longer exists.".to_string()));
    };
    let was_enabled = existing.enabled;
    existing.name = input.name;
    existing.prompt = input.prompt;
    existing.cadence = input.cadence;
    existing.scope = input.scope;
    existing.enabled = input.enabled;
    if input.enabled && !was_enabled {
        existing.next_run_at = crate::db::now_ms();
    }
    let updated = existing.clone();
    save_jobs(&db, &jobs).map_err(PoiesisError::Message)?;
    Ok(updated)
}

#[tauri::command]
pub fn delete_scheduler_job_cmd(db: State<'_, Db>, id: String) -> Result<(), PoiesisError> {
    let jobs = load_jobs(&db);
    if jobs.iter().any(|j| j.id == id && j.built_in) {
        return Err(PoiesisError::Message(
            "The nightly job can be turned off, but not deleted.".to_string(),
        ));
    }
    let kept: Vec<Job> = jobs.into_iter().filter(|j| j.id != id).collect();
    save_jobs(&db, &kept).map_err(PoiesisError::Message)
}

#[tauri::command]
pub async fn run_scheduler_job_now_cmd(app: AppHandle, id: String) -> Result<String, PoiesisError> {
    execute_job(&app, &id).await.map_err(PoiesisError::Message)
}

#[tauri::command]
pub fn scheduler_status_cmd(sched: State<'_, SchedulerState>) -> Option<RunningJob> {
    sched.status()
}

#[tauri::command]
pub fn stop_scheduler_job_cmd(sched: State<'_, SchedulerState>) -> bool {
    sched.stop()
}

#[tauri::command]
pub fn get_scheduler_digest_cmd(db: State<'_, Db>) -> Option<Digest> {
    load_digest(&db)
}

fn mark_digest_read(db: &Db) -> Result<(), String> {
    if let Some(mut digest) = load_digest(db) {
        digest.unread = false;
        let json = serde_json::to_string(&digest).map_err(|e| e.to_string())?;
        db.set_setting(DIGEST_KEY, &json).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn mark_digest_read_cmd(db: State<'_, Db>) -> Result<(), PoiesisError> {
    mark_digest_read(&db).map_err(PoiesisError::Message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_periods_are_in_milliseconds() {
        assert_eq!(Cadence::Hourly.period_ms(), 3_600_000);
        assert_eq!(Cadence::SixHourly.period_ms(), 6 * 3_600_000);
        assert_eq!(Cadence::Daily.period_ms(), 24 * 3_600_000);
        assert_eq!(Cadence::Weekly.period_ms(), 7 * 24 * 3_600_000);
    }

    #[test]
    fn the_nightly_job_is_seeded_once_and_off_by_default() {
        let db = Db::open_in_memory().unwrap();
        let jobs = load_jobs(&db);
        assert_eq!(jobs.iter().filter(|j| j.id == BUILT_IN_NIGHTLY_ID).count(), 1);
        let nightly = jobs.iter().find(|j| j.id == BUILT_IN_NIGHTLY_ID).unwrap();
        assert!(!nightly.enabled, "SCH-5: off by default");
        assert!(nightly.built_in);

        // Seeding is idempotent — a second load doesn't duplicate it, and a
        // job added in between survives.
        let mut again = load_jobs(&db);
        again.push(Job {
            id: "custom".into(),
            name: "Custom".into(),
            prompt: "Summarize the week".into(),
            cadence: Cadence::Weekly,
            scope: None,
            enabled: true,
            next_run_at: 0,
            last_run_at: None,
            last_result: None,
            built_in: false,
            source_conversation_id: None,
            runs: Vec::new(),
        });
        save_jobs(&db, &again).unwrap();
        let reloaded = load_jobs(&db);
        assert_eq!(reloaded.iter().filter(|j| j.id == BUILT_IN_NIGHTLY_ID).count(), 1);
        assert!(reloaded.iter().any(|j| j.id == "custom"));
    }

    #[test]
    fn jobs_round_trip_through_settings() {
        let db = Db::open_in_memory().unwrap();
        let mut jobs = load_jobs(&db);
        jobs[0].enabled = true;
        jobs[0].last_result = Some("3 conversations, 1 learned, 1 pending".into());
        save_jobs(&db, &jobs).unwrap();

        let reloaded = load_jobs(&db);
        let nightly = reloaded.iter().find(|j| j.id == BUILT_IN_NIGHTLY_ID).unwrap();
        assert!(nightly.enabled);
        assert_eq!(nightly.last_result.as_deref(), Some("3 conversations, 1 learned, 1 pending"));
    }

    #[test]
    fn only_one_job_runs_at_a_time() {
        let sched = SchedulerState::new();
        let job = seed_nightly_job();
        let cancel = sched.try_start(&job).expect("first claim succeeds");
        assert!(sched.try_start(&job).is_none(), "a second claim while one is running must fail");
        assert!(sched.status().is_some());

        assert!(sched.stop(), "stop cancels the in-progress run");
        assert!(cancel.is_cancelled());

        sched.finish();
        assert!(sched.status().is_none());
        assert!(sched.try_start(&job).is_some(), "the slot is free again after finish");
    }

    #[test]
    fn a_digest_never_claims_more_reading_than_happened() {
        // The whole pass, the ordinary case.
        let full = compose_digest(3, 3, 1, 0);
        assert_eq!(full, "Last night I read back over 3 conversations. I learned one thing.");
        assert!(!full.contains("stopped"));

        // Stopped partway: the count is what was actually read, and it says so
        // rather than implying the night's work finished.
        let partial = compose_digest(2, 8, 0, 2);
        assert!(partial.starts_with("Last night I read back over 2 conversations before you stopped me"));
        assert!(partial.contains("there are 2 changes I'd like to make"));

        // Stopped before anything was read — "read back over 0 conversations"
        // would be a sentence about nothing.
        assert_eq!(
            compose_digest(0, 8, 0, 0),
            "You stopped me last night before I'd read anything back."
        );

        // Nothing was due in the first place: not the same event as being stopped.
        assert!(compose_digest(0, 0, 0, 0).contains("didn't find anything new to reflect on"));
    }

    #[test]
    fn a_digest_starts_unread_and_can_be_marked_read() {
        let db = Db::open_in_memory().unwrap();
        assert!(load_digest(&db).is_none());

        save_digest(&db, "Last night I read back over 2 conversations. I learned one thing.");
        let digest = load_digest(&db).unwrap();
        assert!(digest.unread);
        assert!(digest.text.starts_with("Last night"));

        mark_digest_read(&db).unwrap();
        assert!(!load_digest(&db).unwrap().unread);
    }
}
