//! Holds live runtime state (the running engine, the active cancellation flag)
//! and the app-data layout. Orchestration (download → spawn → health → stream)
//! is driven by the command layer, which calls into these helpers.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use super::process::{spawn_engine, wait_until_healthy, EngineConfig, EngineError, EngineStatus, RunningEngine};
use super::proxy::CancelFlag;
use super::watchdog::{self, HealResult};

pub struct RuntimeManager {
    pub client: reqwest::Client,
    base_dir: PathBuf,
    engine: Mutex<Option<RunningEngine>>,
    cancel: std::sync::Mutex<Option<CancelFlag>>,
    /// How the running engine was launched, so the watchdog can put back the
    /// *same* engine rather than guessing at a configuration (HEAL-1).
    last_config: std::sync::Mutex<Option<EngineConfig>>,
    /// Bumped whenever the engine the user asked for changes (load or stop).
    /// A watchdog exits as soon as its generation is stale, so exactly one is
    /// ever live.
    generation: AtomicU64,
    /// When self-repair restarted the engine, for the rolling-hour limit.
    restarts: std::sync::Mutex<VecDeque<Instant>>,
    restarts_session: AtomicU32,
    /// Set once the limit is spent — the UI says so instead of pretending.
    gave_up: AtomicBool,
}

impl RuntimeManager {
    pub fn new(base_dir: PathBuf) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ProjectNexus/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            base_dir,
            engine: Mutex::new(None),
            cancel: std::sync::Mutex::new(None),
            last_config: std::sync::Mutex::new(None),
            generation: AtomicU64::new(0),
            restarts: std::sync::Mutex::new(VecDeque::new()),
            restarts_session: AtomicU32::new(0),
            gave_up: AtomicBool::new(false),
        }
    }

    pub fn runtimes_dir(&self) -> PathBuf {
        self.base_dir.join("runtimes")
    }
    pub fn models_dir(&self) -> PathBuf {
        self.base_dir.join("models")
    }
    /// Where the Image Generation skill writes generated PNGs (CHT / 9F). Kept
    /// under app-data so artifacts survive across restarts.
    pub fn generated_images_dir(&self) -> PathBuf {
        self.base_dir.join("generated-images")
    }

    pub async fn status(&self) -> EngineStatus {
        let guard = self.engine.lock().await;
        let mut status = EngineStatus::from_engine(guard.as_ref());
        status.restarts_session = self.restarts_session();
        status.self_heal_gave_up = self.gave_up.load(Ordering::Relaxed);
        status
    }

    /// Spawn the engine for a model and block until it is ready (readiness
    /// gating, §7.4). Replaces any currently-running engine.
    pub async fn load_model(&self, config: EngineConfig) -> Result<EngineStatus, EngineError> {
        // Tear down any previous engine first so VRAM is released.
        self.stop().await;

        let mut engine = spawn_engine(&config)?;
        wait_until_healthy(&self.client, &mut engine, Duration::from_secs(120)).await?;
        let mut status = EngineStatus::from_engine(Some(&engine));
        *self.engine.lock().await = Some(engine);
        // A deliberate load is a fresh start: remember how, and forgive the
        // history of whatever went wrong with the last engine.
        *self.last_config.lock().unwrap() = Some(config);
        self.restarts.lock().unwrap().clear();
        self.gave_up.store(false, Ordering::Relaxed);
        status.restarts_session = self.restarts_session();
        Ok(status)
    }

    /// Stop and reap the running engine, if any. Bumping the generation also
    /// retires the watchdog — a user-requested stop must not be "healed".
    pub async fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let mut guard = self.engine.lock().await;
        // Drop terminates the child (RunningEngine::drop).
        *guard = None;
    }

    // ---- self-repair (HEAL-1) ----

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn restarts_session(&self) -> u32 {
        self.restarts_session.load(Ordering::Relaxed)
    }

    /// Is the engine both alive as a process and answering `/health`? Used only
    /// by the watchdog; a `false` here is one failed poll, not a verdict.
    pub async fn engine_is_healthy(&self) -> bool {
        let mut guard = self.engine.lock().await;
        let Some(engine) = guard.as_mut() else {
            return true; // no engine wanted, nothing to heal
        };
        if !engine.still_running() {
            return false;
        }
        let url = format!("{}/health", engine.base_url());
        let token = engine.token.clone();
        drop(guard); // don't hold the engine lock across the network call
        matches!(
            self.client
                .get(&url)
                .bearer_auth(token)
                .timeout(Duration::from_secs(5))
                .send()
                .await,
            Ok(resp) if resp.status().is_success()
        )
    }

    /// Put the engine back, honouring the rolling-hour limit. Deliberately does
    /// **not** bump the generation: this is the same engine the user asked for,
    /// restored, so the watchdog carries on watching it.
    pub async fn heal(&self) -> HealResult {
        // Healing takes up to ~150s (backoff, then the readiness wait), and the
        // user can stop the engine at any point in that window. Every step that
        // could put an engine back checks that the one being restored is still
        // the one they want — otherwise a stop would be silently undone.
        let generation = self.generation();
        let attempt = {
            let mut history = self.restarts.lock().unwrap();
            if !watchdog::may_restart(&mut history, Instant::now()) {
                self.gave_up.store(true, Ordering::Relaxed);
                return HealResult::GaveUp;
            }
            history.push_back(Instant::now());
            history.len()
        };
        let Some(config) = self.last_config.lock().unwrap().clone() else {
            return HealResult::GaveUp;
        };

        tokio::time::sleep(watchdog::backoff(attempt - 1)).await;
        if self.generation() != generation {
            return HealResult::Superseded;
        }

        // Reap the remnant before respawning, so a half-dead child can't hold
        // the model's VRAM while its replacement tries to load.
        *self.engine.lock().await = None;
        let session = self.restarts_session.fetch_add(1, Ordering::Relaxed) + 1;

        let Ok(mut engine) = spawn_engine(&config) else {
            return HealResult::Failed { attempt: session };
        };
        if wait_until_healthy(&self.client, &mut engine, Duration::from_secs(120))
            .await
            .is_err()
        {
            return HealResult::Failed { attempt: session };
        }

        // Last check, holding the lock: from here to the assignment nothing can
        // interleave, so a stop either lands before this (and we drop the fresh
        // engine) or after it (and stops the engine we just installed).
        let mut guard = self.engine.lock().await;
        if self.generation() != generation {
            return HealResult::Superseded; // `engine` drops here, killing the child
        }
        *guard = Some(engine);
        HealResult::Restarted { attempt: session }
    }

    /// Connection details for the running engine, if ready.
    pub async fn engine_endpoint(&self) -> Option<(String, String)> {
        let guard = self.engine.lock().await;
        guard.as_ref().map(|e| (e.base_url(), e.token.clone()))
    }

    /// Context window of the running local engine, if any (CTX-1).
    pub async fn engine_ctx_size(&self) -> Option<u32> {
        self.engine.lock().await.as_ref().map(|e| e.ctx_size)
    }

    /// A short label for the running local model — its file stem (GRM-4), used
    /// to key per-model tool reliability stats.
    pub async fn engine_model_name(&self) -> Option<String> {
        self.engine.lock().await.as_ref().and_then(|e| {
            e.model_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
    }

    /// Install a fresh cancellation flag for a new turn and return it.
    pub fn new_cancel(&self) -> CancelFlag {
        let flag = CancelFlag::new();
        *self.cancel.lock().unwrap() = Some(flag.clone());
        flag
    }

    /// Trip the active cancellation flag (Stop control, CHT-2).
    pub fn cancel_active(&self) {
        if let Some(flag) = self.cancel.lock().unwrap().as_ref() {
            flag.cancel();
        }
    }
}

/// Best-effort path to a previously-extracted runtime's server binary.
// Used by the "update runtime" flow (Settings) in a later phase.
#[allow(dead_code)]
pub fn existing_server_binary(runtimes_dir: &Path, build_tag: &str) -> Option<PathBuf> {
    let dir = runtimes_dir.join(build_tag);
    if dir.exists() {
        super::download::find_server_binary(&dir)
    } else {
        None
    }
}
