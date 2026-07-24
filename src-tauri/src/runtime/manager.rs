//! Holds live runtime state (the running engine, the active cancellation flag)
//! and the app-data layout. Orchestration (download → spawn → health → stream)
//! is driven by the command layer, which calls into these helpers.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::Mutex;

use super::process::{spawn_engine, wait_until_healthy, EngineConfig, EngineError, EngineStatus, RunningEngine};
use super::proxy::CancelFlag;

pub struct RuntimeManager {
    pub client: reqwest::Client,
    base_dir: PathBuf,
    engine: Mutex<Option<RunningEngine>>,
    cancel: std::sync::Mutex<Option<CancelFlag>>,
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
        EngineStatus::from_engine(guard.as_ref())
    }

    /// Spawn the engine for a model and block until it is ready (readiness
    /// gating, §7.4). Replaces any currently-running engine.
    pub async fn load_model(&self, config: EngineConfig) -> Result<EngineStatus, EngineError> {
        // Tear down any previous engine first so VRAM is released.
        self.stop().await;

        let mut engine = spawn_engine(&config)?;
        wait_until_healthy(&self.client, &mut engine, Duration::from_secs(120)).await?;
        let status = EngineStatus::from_engine(Some(&engine));
        *self.engine.lock().await = Some(engine);
        Ok(status)
    }

    /// Stop and reap the running engine, if any.
    pub async fn stop(&self) {
        let mut guard = self.engine.lock().await;
        // Drop terminates the child (RunningEngine::drop).
        *guard = None;
    }

    /// Connection details for the running engine, if ready.
    pub async fn engine_endpoint(&self) -> Option<(String, String)> {
        let guard = self.engine.lock().await;
        guard.as_ref().map(|e| (e.base_url(), e.token.clone()))
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
