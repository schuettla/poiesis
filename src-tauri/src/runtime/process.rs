//! Engine process & communication (PRD §7.4): spawn `llama-server` as a child
//! process bound to a dynamic loopback port, protected by a per-session token,
//! gated on a health check, and guaranteed to be terminated on app exit.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

use rand::Rng;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("could not find a free loopback port: {0}")]
    Port(std::io::Error),
    #[error("failed to launch llama-server: {0}")]
    Spawn(std::io::Error),
    #[error("engine did not become ready within {0:?}")]
    Timeout(Duration),
    #[error("engine process exited during startup")]
    ExitedEarly,
    #[error("network error talking to engine: {0}")]
    Http(#[from] reqwest::Error),
}

/// Parameters for launching the engine for a specific model (CHT-7 overrides).
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub server_binary: PathBuf,
    pub model_path: PathBuf,
    pub ctx_size: u32,
    /// Number of layers to offload to the GPU (`-ngl`). 0 = CPU only.
    pub n_gpu_layers: u32,
}

/// A live engine instance. Dropping it kills the child process (lifecycle
/// safety, §7.4) so no orphan holds VRAM.
#[derive(Debug)]
pub struct RunningEngine {
    child: Child,
    pub port: u16,
    pub token: String,
    pub model_path: PathBuf,
    /// Context window this engine was launched with (`--ctx-size`). Drives the
    /// turn budgeter (CTX-1) so requests never overflow and get front-truncated.
    pub ctx_size: u32,
}

/// Snapshot of engine state for the UI / readiness gating.
#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub model_path: Option<String>,
    pub ctx_size: Option<u32>,
    /// Whether the running engine enforces structured tool calls natively
    /// (GRM-1/2). True whenever an engine is up: we always launch with `--jinja`
    /// (see `spawn_engine`), which turns on llama.cpp's lazy-grammar tool-call
    /// enforcement. GRM-3's validate-and-retry is the universal fallback for any
    /// model that slips a call out as content JSON regardless.
    pub structured_tool_output: bool,
    /// How many times the watchdog put this engine back on its feet since the
    /// app started (HEAL-1). 0 for a session that never needed healing.
    pub restarts_session: u32,
    /// True once self-repair hit its rolling-hour limit and stopped trying.
    pub self_heal_gave_up: bool,
}

impl RunningEngine {
    /// The loopback base URL for the running engine.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn still_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for RunningEngine {
    fn drop(&mut self) {
        // Best-effort terminate; the child is bound to loopback only, so a brief
        // window before the OS reaps it is harmless.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Pick a free loopback TCP port by binding to port 0 and reading it back. The
/// listener is dropped immediately; there is an inherent (small) race before the
/// engine binds it, accepted per §7.4 ("dynamic port chosen at launch").
fn free_loopback_port() -> Result<u16, EngineError> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(EngineError::Port)?;
    let port = listener.local_addr().map_err(EngineError::Port)?.port();
    Ok(port)
}

fn random_token() -> String {
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| {
            let n: u8 = rng.gen_range(0..16);
            std::char::from_digit(n as u32, 16).unwrap()
        })
        .collect()
}

/// Launch the engine. The returned [`RunningEngine`] is not yet ready — callers
/// must `wait_until_healthy` before sending requests (readiness gating, §7.4).
pub fn spawn_engine(config: &EngineConfig) -> Result<RunningEngine, EngineError> {
    let port = free_loopback_port()?;
    let token = random_token();

    let mut cmd = Command::new(&config.server_binary);
    cmd.arg("--model")
        .arg(&config.model_path)
        .arg("--host")
        .arg("127.0.0.1") // loopback only (§7.4 loopback security)
        .arg("--port")
        .arg(port.to_string())
        .arg("--api-key")
        .arg(&token) // per-session auth token
        .arg("--ctx-size")
        .arg(config.ctx_size.to_string())
        .arg("--n-gpu-layers")
        .arg(config.n_gpu_layers.to_string())
        // Use the model's embedded Jinja chat template. Required for native
        // tool-calling (TOOL-2): llama-server rejects requests carrying a
        // `tools` array with HTTP 500 ("tools param requires --jinja flag")
        // unless this is enabled.
        .arg("--jinja");

    // On Windows, put the child in its own process group so a stray Ctrl-C in a
    // dev console doesn't also signal it; we manage its lifetime explicitly.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let child = cmd.spawn().map_err(EngineError::Spawn)?;

    // Bind to the kill-on-close job so an ungraceful parent death can't orphan it.
    super::jobobject::assign(&child);

    Ok(RunningEngine {
        child,
        port,
        token,
        model_path: config.model_path.clone(),
        ctx_size: config.ctx_size,
    })
}

/// Poll the engine's `/health` endpoint until it reports ready or the deadline
/// passes. llama-server returns 503 while the model loads and 200 once ready.
pub async fn wait_until_healthy(
    client: &reqwest::Client,
    engine: &mut RunningEngine,
    timeout: Duration,
) -> Result<(), EngineError> {
    let url = format!("{}/health", engine.base_url());
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !engine.still_running() {
            return Err(EngineError::ExitedEarly);
        }
        if let Ok(resp) = client
            .get(&url)
            .bearer_auth(&engine.token)
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            if resp.status().is_success() {
                return Ok(());
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(EngineError::Timeout(timeout));
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

impl EngineStatus {
    pub fn from_engine(engine: Option<&RunningEngine>) -> Self {
        match engine {
            Some(e) => EngineStatus {
                running: true,
                port: Some(e.port),
                model_path: Some(e.model_path.to_string_lossy().to_string()),
                ctx_size: Some(e.ctx_size),
                structured_tool_output: true,
                restarts_session: 0,
                self_heal_gave_up: false,
            },
            None => EngineStatus {
                running: false,
                port: None,
                model_path: None,
                ctx_size: None,
                structured_tool_output: false,
                restarts_session: 0,
                self_heal_gave_up: false,
            },
        }
    }
}
