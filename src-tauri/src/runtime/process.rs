//! Engine process & communication (PRD §7.4): spawn `llama-server` as a child
//! process bound to a dynamic loopback port, protected by a per-session token,
//! gated on a health check, and guaranteed to be terminated on app exit.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::Rng;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("could not find a free loopback port: {0}")]
    Port(std::io::Error),
    #[error("failed to launch llama-server: {0}")]
    Spawn(std::io::Error),
    #[error("the engine did not become ready within {0:?} — {1}")]
    Timeout(Duration, String),
    #[error("the engine stopped while starting up — {0}")]
    ExitedEarly(String),
    #[error("network error talking to engine: {0}")]
    Http(#[from] reqwest::Error),
}

/// How many of the engine's own log lines to keep. Generous enough that the
/// root-cause line still survives the ~150 lines of tensor/metadata banner
/// llama-server prints ahead of a load failure.
const LOG_TAIL_LINES: usize = 200;

/// A bounded ring of the engine's most recent log lines.
///
/// llama-server explains its own failures perfectly well — "unknown model
/// architecture: 'gemma3'" — but it says so on stderr. That used to be
/// inherited, which in a windowless GUI build means discarded, so every startup
/// failure reached the user as the bare, unactionable "engine process exited
/// during startup". Draining the pipes into this ring is also what keeps a
/// piped child from blocking once the OS pipe buffer fills.
#[derive(Debug, Clone, Default)]
pub struct LogTail(Arc<Mutex<VecDeque<String>>>);

impl LogTail {
    fn push(&self, line: String) {
        let mut buf = self.0.lock().unwrap();
        if buf.len() == LOG_TAIL_LINES {
            buf.pop_front();
        }
        buf.push_back(line);
    }

    /// The engine's own account of what went wrong, or an empty string if it
    /// said nothing. Takes the *first* line mentioning an error: llama.cpp
    /// reports failures as a cascade from the inside out, so the first line
    /// names the actual cause ("unknown model architecture") and the later ones
    /// only restate it ("failed to load model", "exiting due to model loading
    /// error").
    fn reason(&self) -> String {
        let buf = self.0.lock().unwrap();
        let is_failure = |l: &&String| {
            let l = l.to_ascii_lowercase();
            l.contains("error") || l.contains("failed")
        };
        buf.iter()
            .find(is_failure)
            .or_else(|| buf.iter().rev().find(|l| !l.trim().is_empty()))
            .map(|l| l.trim().to_string())
            .unwrap_or_default()
    }
}

/// Drain one of the child's output pipes into `tail` on a background thread.
fn drain_into(stream: Option<impl std::io::Read + Send + 'static>, tail: LogTail) {
    let Some(stream) = stream else { return };
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            // Keep `cargo tauri dev` as informative as it was before the pipes
            // were captured; the packaged build has no console to print to.
            #[cfg(debug_assertions)]
            eprintln!("[engine] {line}");
            tail.push(line);
        }
    });
}

/// Parameters for launching the engine for a specific model (CHT-7 overrides).
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub server_binary: PathBuf,
    pub model_path: PathBuf,
    pub ctx_size: u32,
    /// Number of layers to offload to the GPU (`-ngl`). 0 = CPU only.
    pub n_gpu_layers: u32,
    /// Extra CLI flags appended after the common ones (e.g. the embedding
    /// engine's `--embeddings --pooling mean`). Empty for a plain chat engine.
    pub extra_args: Vec<String>,
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
    /// The engine's recent log output, so a startup failure can be reported in
    /// the engine's own words rather than as "it exited".
    log: LogTail,
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

    /// Why the engine is not serving, phrased for the user. Prefers what the
    /// engine itself said; falls back to its exit code when it said nothing.
    fn failure_reason(&mut self) -> String {
        let reason = self.log.reason();
        if !reason.is_empty() {
            return reason;
        }
        match self.child.try_wait() {
            Ok(Some(status)) => match status.code() {
                Some(code) => format!("it exited with code {code} without saying why"),
                None => "it was terminated without saying why".to_string(),
            },
            _ => "it is still running but never answered a health check".to_string(),
        }
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
    cmd.args(&config.extra_args);

    // On Windows, put the child in its own process group so a stray Ctrl-C in a
    // dev console doesn't also signal it; we manage its lifetime explicitly.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    // Capture both streams: llama-server reports load failures on stderr, and
    // an inherited stderr in a windowless build goes nowhere. Both pipes are
    // drained below — an undrained pipe would wedge the child once it fills.
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(EngineError::Spawn)?;

    let log = LogTail::default();
    drain_into(child.stdout.take(), log.clone());
    drain_into(child.stderr.take(), log.clone());

    // Bind to the kill-on-close job so an ungraceful parent death can't orphan it.
    super::jobobject::assign(&child);

    Ok(RunningEngine {
        child,
        port,
        token,
        model_path: config.model_path.clone(),
        ctx_size: config.ctx_size,
        log,
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
            return Err(EngineError::ExitedEarly(engine.failure_reason()));
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
            return Err(EngineError::Timeout(timeout, engine.failure_reason()));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact cascade llama-server printed when the pinned `b4585` engine was
    /// handed a Gemma 3 GGUF: five lines about the failure, only the first of
    /// which says anything the user can act on.
    const GEMMA3_ON_AN_OLD_ENGINE: &[&str] = &[
        "print_info: file size   = 2.31 GiB (5.12 BPW)",
        "llama_model_load: error loading model: error loading model architecture: unknown model architecture: 'gemma3'",
        "llama_model_load_from_file_impl: failed to load model",
        "common_init_from_params: failed to load model 'gemma-3-4b-it-Q4_K_M.gguf'",
        "srv    load_model: failed to load model",
        "main: exiting due to model loading error",
    ];

    fn tail_of(lines: &[&str]) -> LogTail {
        let tail = LogTail::default();
        for line in lines {
            tail.push((*line).to_string());
        }
        tail
    }

    #[test]
    fn reports_the_root_cause_not_the_last_line_of_the_cascade() {
        // "exiting due to model loading error" is true and useless; the naming
        // of the unsupported architecture is the whole point.
        assert!(tail_of(GEMMA3_ON_AN_OLD_ENGINE)
            .reason()
            .contains("unknown model architecture: 'gemma3'"));
    }

    #[test]
    fn falls_back_to_the_last_line_when_nothing_looks_like_an_error() {
        let tail = tail_of(&["loading model", "  warming up  ", ""]);
        assert_eq!(tail.reason(), "warming up");
    }

    #[test]
    fn a_silent_engine_yields_no_reason_rather_than_a_bogus_one() {
        assert_eq!(LogTail::default().reason(), "");
    }

    /// The ring is bounded, so a chatty engine cannot grow it without limit —
    /// but it must be deep enough to still hold the failure after llama-server's
    /// long metadata banner.
    #[test]
    fn the_ring_stays_bounded_and_keeps_the_newest_lines() {
        let tail = LogTail::default();
        for i in 0..LOG_TAIL_LINES * 3 {
            tail.push(format!("line {i}"));
        }
        let buf = tail.0.lock().unwrap();
        assert_eq!(buf.len(), LOG_TAIL_LINES);
        assert_eq!(buf.back().unwrap(), &format!("line {}", LOG_TAIL_LINES * 3 - 1));
    }

    /// End-to-end proof that a real engine's stderr reaches the user, rather
    /// than the bare "it exited" this replaced. Needs a real `llama-server.exe`
    /// and a GGUF, so it is opt-in:
    ///
    /// ```text
    /// POIESIS_TEST_SERVER_BIN=…/llama-server.exe \
    /// POIESIS_TEST_MODEL=…/model.gguf \
    ///   cargo test -- --ignored startup_failure
    /// ```
    #[tokio::test]
    #[ignore = "needs a local llama-server binary and model"]
    async fn startup_failure_is_reported_in_the_engines_own_words() {
        let (Ok(bin), Ok(model)) = (
            std::env::var("POIESIS_TEST_SERVER_BIN"),
            std::env::var("POIESIS_TEST_MODEL"),
        ) else {
            panic!("set POIESIS_TEST_SERVER_BIN and POIESIS_TEST_MODEL");
        };
        let mut engine = spawn_engine(&EngineConfig {
            server_binary: bin.into(),
            model_path: model.into(),
            ctx_size: 4096,
            n_gpu_layers: 999,
            extra_args: Vec::new(),
        })
        .expect("spawn");

        let client = reqwest::Client::new();
        let err = wait_until_healthy(&client, &mut engine, Duration::from_secs(90))
            .await
            .expect_err("this engine cannot load this model");
        let msg = err.to_string();
        assert!(
            msg.len() > "the engine stopped while starting up — ".len() + 10,
            "error carried no explanation: {msg}"
        );
        println!("reported: {msg}");
    }

    /// The other half of capturing the engine's pipes: an engine that *works*
    /// must still come up and stay up. A piped child whose output nobody reads
    /// wedges the moment the OS pipe buffer fills, which for llama-server is a
    /// few hundred lines into loading — so this guards the drain threads.
    /// Opt-in, with the same environment variables as the failure test above.
    #[tokio::test]
    #[ignore = "needs a local llama-server binary and model"]
    async fn a_working_engine_comes_up_with_its_pipes_captured() {
        let (Ok(bin), Ok(model)) = (
            std::env::var("POIESIS_TEST_SERVER_BIN"),
            std::env::var("POIESIS_TEST_MODEL"),
        ) else {
            panic!("set POIESIS_TEST_SERVER_BIN and POIESIS_TEST_MODEL");
        };
        let mut engine = spawn_engine(&EngineConfig {
            server_binary: bin.into(),
            model_path: model.into(),
            ctx_size: 4096,
            n_gpu_layers: 999,
            extra_args: Vec::new(),
        })
        .expect("spawn");

        let client = reqwest::Client::new();
        wait_until_healthy(&client, &mut engine, Duration::from_secs(180))
            .await
            .expect("engine should become healthy");

        // Serve a real completion: readiness is necessary, not sufficient.
        let resp = client
            .post(format!("{}/v1/chat/completions", engine.base_url()))
            .bearer_auth(&engine.token)
            .json(&serde_json::json!({
                "messages": [{ "role": "user", "content": "Say OK." }],
                "max_tokens": 16,
                "stream": false,
            }))
            .send()
            .await
            .expect("completion request");
        assert!(resp.status().is_success(), "completion failed: {:?}", resp.status());
        assert!(engine.still_running(), "engine died while serving");
    }
}
