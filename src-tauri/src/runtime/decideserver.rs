//! The decision engine (`DEC-4`): a **fourth** `llama-server` process, dedicated
//! to a decision model. It mirrors `rerankserver.rs` (lazy start, idle stop, one
//! instance, the shared binary) and differs where a decision model differs.
//!
//! What was checked against a real Clef-flash Q4_K_M on `b11418` (2026-10-23):
//!
//! * The argument list `spawn_engine` always builds is accepted as it is,
//!   `--jinja` and `--ctx-size` included.
//! * The server forces `n_batch = n_ubatch = 512` for a decision model, and
//!   refuses a longer state with HTTP 500. `--batch-size` and `--ubatch-size`
//!   on the command line override that, so both are set to the state limit.
//! * `--mmproj` loads the vision projector next to it; `/props` then reports
//!   `modalities.vision: true`.
//! * A server running a decision model serves `/v1/systemone` only.
//! * A model that is not a decision model loads fine and answers HTTP 501.
//!
//! Not measured: the memory the 16k batch size costs on a graphics card.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use super::hardware::{classify_fit, detect_hardware, Fit, HardwareProfile};
use super::process::{spawn_engine, wait_until_healthy, EngineConfig, EngineError, RunningEngine};
use super::RuntimeManager;
use crate::db::Db;

/// Idle timeout before the engine is stopped to free memory.
pub const IDLE_STOP: Duration = Duration::from_secs(5 * 60);
/// The most state, in tokens, the engine takes: the release code's own cap.
/// The advertised 64k is unverified (`Shell facts`).
pub const MAX_STATE_TOKENS: u32 = 16_384;
/// Free graphics memory to keep beside the model (`DEC-4`).
const HEADROOM_MB: u64 = 1_024;
/// A default Stable Diffusion model, counted while a local image backend is set up.
const IMAGE_ENGINE_MB: u64 = 4_096;
/// After a failed start, how long before a hot-path ask tries again.
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60);
/// How many recent answer times the status keeps for its median.
const SAMPLES: usize = 20;

pub const PLACEMENT_KEY: &str = "decide.placement";

/// A curated decision model (`DEC-3`), pinned to exact files and sizes. Third
/// party GGUFs uploaded before the format was merged have no decision head, so
/// the catalog names files, not repositories. Copy follows the `SMP-8a` rule:
/// no *logit*, *head* or *System One*.
#[derive(Debug, Clone, Serialize)]
pub struct DecideCatalogEntry {
    pub name: String,
    pub note: String,
    pub size_label: String,
    /// Model plus projector, in MB, for the fit line.
    pub size_mb: u64,
    pub url: String,
    pub filename: String,
    pub bytes: u64,
    pub mmproj_url: String,
    pub mmproj_filename: String,
    pub mmproj_bytes: u64,
}

/// File names, byte counts and URLs checked against the Hugging Face tree API
/// on 2026-10-23, not inferred from the naming pattern.
pub fn decide_catalog() -> Vec<DecideCatalogEntry> {
    vec![
        DecideCatalogEntry {
            name: "Clef-flash".into(),
            note: "The default. Answers small yes/no and pick-one questions in a blink, next to your chat model.".into(),
            size_label: "~6.5 GB".into(),
            size_mb: (6_486_448_288u64 + 624_229_728) / 1_048_576,
            url: "https://huggingface.co/ggml-org/Clef-Flash-GGUF/resolve/main/Clef-Flash-Q4_K_M.gguf".into(),
            filename: "Clef-Flash-Q4_K_M.gguf".into(),
            bytes: 6_486_448_288,
            mmproj_url: "https://huggingface.co/ggml-org/Clef-Flash-GGUF/resolve/main/mmproj-Clef-Flash-Q8_0.gguf".into(),
            mmproj_filename: "mmproj-Clef-Flash-Q8_0.gguf".into(),
            mmproj_bytes: 624_229_728,
        },
        DecideCatalogEntry {
            name: "Clef".into(),
            note: "Larger and a little more accurate. Needs a big graphics card.".into(),
            size_label: "~19 GB".into(),
            size_mb: (19_232_219_200u64 + 629_247_456) / 1_048_576,
            url: "https://huggingface.co/ggml-org/Clef-GGUF/resolve/main/Clef-Q4_K_M.gguf".into(),
            filename: "Clef-Q4_K_M.gguf".into(),
            bytes: 19_232_219_200,
            mmproj_url: "https://huggingface.co/ggml-org/Clef-GGUF/resolve/main/mmproj-Clef-Q8_0.gguf".into(),
            mmproj_filename: "mmproj-Clef-Q8_0.gguf".into(),
            mmproj_bytes: 629_247_456,
        },
    ]
}

pub fn catalog_entry_for(filename: &str) -> Option<DecideCatalogEntry> {
    decide_catalog().into_iter().find(|e| e.filename == filename)
}

pub fn models_dir(mgr: &RuntimeManager) -> PathBuf {
    mgr.models_dir().join("decide")
}

// ---- placement ----

/// The user's choice in Runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementSetting {
    Auto,
    Gpu,
    Cpu,
}

impl PlacementSetting {
    pub fn parse(s: Option<&str>) -> Self {
        match s {
            Some("gpu") => Self::Gpu,
            Some("cpu") => Self::Cpu,
            _ => Self::Auto,
        }
    }
}

/// Where it will run, as far as this machine can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fits {
    /// On the graphics card, with room beside what else is loaded.
    Gpu,
    /// On the processor: background purposes only.
    Cpu,
    WontFit,
}

/// What is already using memory, in MB.
#[derive(Debug, Clone, Copy)]
pub struct Loads {
    pub vram_mb: u64,
    pub ram_mb: u64,
    pub model_mb: u64,
    pub chat_mb: u64,
    pub image_mb: u64,
}

/// Decide placement (`DEC-4`). `Auto` takes the graphics card only when the
/// model fits beside the chat model and the image engine with a gigabyte to
/// spare; otherwise the processor, if the model fits in memory at all.
pub fn plan_fit(setting: PlacementSetting, l: &Loads) -> Fits {
    let ram_fit = |l: &Loads| {
        let hw = HardwareProfile {
            cpu: super::hardware::CpuInfo { brand: String::new(), physical_cores: 0, avx2: false, avx512: false },
            ram_mb: l.ram_mb,
            gpus: Vec::new(),
        };
        classify_fit(l.model_mb, &hw) != Fit::WontFit
    };
    match setting {
        PlacementSetting::Gpu if l.vram_mb > 0 => Fits::Gpu,
        PlacementSetting::Auto if l.vram_mb >= l.model_mb + l.chat_mb + l.image_mb + HEADROOM_MB => Fits::Gpu,
        _ if ram_fit(l) => Fits::Cpu,
        _ => Fits::WontFit,
    }
}

static HARDWARE: OnceLock<HardwareProfile> = OnceLock::new();

/// The hardware survey, taken once: it does not change while the app runs, and
/// it shells out to the driver tools.
pub async fn hardware() -> HardwareProfile {
    if let Some(h) = HARDWARE.get() {
        return h.clone();
    }
    let h = tauri::async_runtime::spawn_blocking(detect_hardware).await.unwrap_or_else(|_| HardwareProfile {
        cpu: super::hardware::CpuInfo { brand: String::new(), physical_cores: 0, avx2: false, avx512: false },
        ram_mb: 0,
        gpus: Vec::new(),
    });
    let _ = HARDWARE.set(h.clone());
    h
}

fn file_mb(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len() / 1_048_576).unwrap_or(0)
}

/// The loads that are already there, and how the model would be placed.
pub async fn loads_for(mgr: &RuntimeManager, db: &Db, model_mb: u64) -> Loads {
    let hw = hardware().await;
    let chat_mb = mgr
        .status()
        .await
        .model_path
        .map(|p| file_mb(Path::new(&p)))
        .unwrap_or(0);
    let local_image = crate::media::resolve_backend(&crate::media::Registry::new(), db, crate::media::Modality::Image)
        .map(|b| matches!(b.descriptor().credential, crate::media::Credential::Local))
        .unwrap_or(false);
    Loads {
        vram_mb: hw.vram_mb(),
        ram_mb: hw.ram_mb,
        model_mb,
        chat_mb,
        image_mb: if local_image { IMAGE_ENGINE_MB } else { 0 },
    }
}

// ---- what to start ----

/// Everything needed to start the engine, worked out from the installed model.
#[derive(Debug, Clone)]
pub struct Local {
    pub server_binary: PathBuf,
    pub model_path: PathBuf,
    pub model_name: String,
    /// The vision projector, when its file sits beside the model.
    pub mmproj: Option<PathBuf>,
    pub gpu: bool,
}

/// The default decision model, the shared engine binary and the placement; or
/// `None` when any of them is missing (then every site keeps its old path).
pub async fn resolve_local(mgr: &RuntimeManager, db: &Db) -> Option<Local> {
    let model = db.default_model_by_role("decide").ok()??;
    let model_path = PathBuf::from(&model.path);
    if !model_path.exists() {
        return None;
    }
    let server_binary = crate::commands::embedgen::engine_binary_path(mgr, db).await.ok().flatten()?;
    let mmproj = model_path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(catalog_entry_for)
        .map(|e| model_path.with_file_name(e.mmproj_filename))
        .filter(|p| p.exists());
    let model_mb = file_mb(&model_path) + mmproj.as_deref().map(file_mb).unwrap_or(0);
    let setting = PlacementSetting::parse(db.get_setting(PLACEMENT_KEY).ok().flatten().as_deref());
    let gpu = match plan_fit(setting, &loads_for(mgr, db, model_mb).await) {
        Fits::Gpu => true,
        Fits::Cpu => false,
        Fits::WontFit => return None,
    };
    Some(Local { server_binary, model_path, model_name: model.name, mmproj, gpu })
}

/// The arguments beyond the common ones. The batch sizes are what let a long
/// state through; without them the server refuses anything over 512 tokens.
pub fn extra_args(mmproj: Option<&Path>) -> Vec<String> {
    let mut args = vec![
        "--batch-size".to_string(),
        MAX_STATE_TOKENS.to_string(),
        "--ubatch-size".to_string(),
        MAX_STATE_TOKENS.to_string(),
    ];
    if let Some(p) = mmproj {
        args.push("--mmproj".into());
        args.push(p.to_string_lossy().to_string());
    }
    args
}

/// A running engine, as the seam needs to see it.
#[derive(Debug, Clone)]
pub struct Live {
    pub base_url: String,
    pub token: String,
    pub gpu: bool,
    pub vision: bool,
    pub max_state_tokens: u32,
}

#[derive(Debug, Clone)]
struct Started {
    gpu: bool,
    vision: bool,
}

/// What the screen shows (`DEC-UI-2`).
#[derive(Debug, Clone, Serialize)]
pub struct DecideStatus {
    /// `idle` | `starting` | `ready` | `failed`
    pub state: &'static str,
    pub model_installed: bool,
    pub model_name: Option<String>,
    pub gpu: Option<bool>,
    pub vision: Option<bool>,
    pub median_ms: Option<u32>,
    pub error: Option<String>,
}

pub struct DecideManager {
    engine: Mutex<Option<RunningEngine>>,
    started: StdMutex<Option<Started>>,
    last_used: StdMutex<Instant>,
    running: AtomicBool,
    starting: AtomicBool,
    samples: StdMutex<VecDeque<u32>>,
    last_error: StdMutex<Option<(String, Instant)>>,
}

impl DecideManager {
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(None),
            started: StdMutex::new(None),
            last_used: StdMutex::new(Instant::now()),
            running: AtomicBool::new(false),
            starting: AtomicBool::new(false),
            samples: StdMutex::new(VecDeque::new()),
            last_error: StdMutex::new(None),
        }
    }

    /// Is an engine up right now? The hot path asks this and never waits.
    pub fn is_warm(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Where the running engine actually is, which may differ from where a new
    /// start would put it now.
    pub fn running_gpu(&self) -> Option<bool> {
        if !self.is_warm() {
            return None;
        }
        self.started.lock().ok()?.as_ref().map(|s| s.gpu)
    }

    pub fn record_ms(&self, ms: u32) {
        if let Ok(mut s) = self.samples.lock() {
            if s.len() == SAMPLES {
                s.pop_front();
            }
            s.push_back(ms);
        }
        self.touch();
    }

    fn median_ms(&self) -> Option<u32> {
        let s = self.samples.lock().ok()?;
        if s.is_empty() {
            return None;
        }
        let mut v: Vec<u32> = s.iter().copied().collect();
        v.sort_unstable();
        Some(v[v.len() / 2])
    }

    /// Start the engine for `local` if it is not already up on this model.
    /// A running engine is reused even if a fresh placement would differ: the
    /// next start places again, which keeps it from thrashing.
    pub async fn ensure_started(&self, client: &reqwest::Client, local: &Local) -> Result<Live, EngineError> {
        self.touch();
        let mut guard = self.engine.lock().await;
        if let Some(e) = guard.as_mut() {
            if e.still_running() && e.model_path == local.model_path {
                let s = self.started.lock().ok().and_then(|s| s.clone()).unwrap_or(Started {
                    gpu: local.gpu,
                    vision: local.mmproj.is_some(),
                });
                return Ok(Live {
                    base_url: e.base_url(),
                    token: e.token.clone(),
                    gpu: s.gpu,
                    vision: s.vision,
                    max_state_tokens: MAX_STATE_TOKENS,
                });
            }
            *guard = None;
            self.running.store(false, Ordering::Relaxed);
        }
        self.starting.store(true, Ordering::Relaxed);
        let result = self.start_locked(client, local).await;
        self.starting.store(false, Ordering::Relaxed);
        match result {
            Ok(engine) => {
                let live = Live {
                    base_url: engine.base_url(),
                    token: engine.token.clone(),
                    gpu: local.gpu,
                    vision: local.mmproj.is_some(),
                    max_state_tokens: MAX_STATE_TOKENS,
                };
                if let Ok(mut s) = self.started.lock() {
                    *s = Some(Started { gpu: local.gpu, vision: live.vision });
                }
                if let Ok(mut e) = self.last_error.lock() {
                    *e = None;
                }
                *guard = Some(engine);
                self.running.store(true, Ordering::Relaxed);
                Ok(live)
            }
            Err(e) => {
                if let Ok(mut slot) = self.last_error.lock() {
                    *slot = Some((e.to_string(), Instant::now()));
                }
                Err(e)
            }
        }
    }

    async fn start_locked(&self, client: &reqwest::Client, local: &Local) -> Result<RunningEngine, EngineError> {
        let config = EngineConfig {
            server_binary: local.server_binary.clone(),
            model_path: local.model_path.clone(),
            ctx_size: MAX_STATE_TOKENS,
            n_gpu_layers: if local.gpu { 999 } else { 0 },
            extra_args: extra_args(local.mmproj.as_deref()),
        };
        let mut engine = spawn_engine(&config)?;
        wait_until_healthy(client, &mut engine, Duration::from_secs(120)).await?;
        Ok(engine)
    }

    pub async fn stop(&self) {
        *self.engine.lock().await = None;
        self.running.store(false, Ordering::Relaxed);
    }

    fn touch(&self) {
        if let Ok(mut last) = self.last_used.lock() {
            *last = Instant::now();
        }
    }

    fn idle_for(&self) -> Duration {
        self.last_used.lock().map(|l| l.elapsed()).unwrap_or(Duration::ZERO)
    }

    /// A failed start in the last minute, which keeps hot asks from trying again.
    fn recently_failed(&self) -> bool {
        self.last_error
            .lock()
            .ok()
            .and_then(|e| e.as_ref().map(|(_, at)| at.elapsed() < RETRY_AFTER_FAILURE))
            .unwrap_or(false)
    }

    pub fn status(&self, model_name: Option<String>) -> DecideStatus {
        let error = self.last_error.lock().ok().and_then(|e| e.as_ref().map(|(m, _)| m.clone()));
        let started = self.started.lock().ok().and_then(|s| s.clone());
        let ready = self.is_warm();
        let state = if ready {
            "ready"
        } else if self.starting.load(Ordering::Relaxed) {
            "starting"
        } else if error.is_some() {
            "failed"
        } else {
            "idle"
        };
        DecideStatus {
            state,
            model_installed: model_name.is_some(),
            model_name,
            gpu: started.as_ref().filter(|_| ready).map(|s| s.gpu),
            vision: started.as_ref().filter(|_| ready).map(|s| s.vision),
            median_ms: self.median_ms(),
            error: if ready { None } else { error },
        }
    }
}

impl Default for DecideManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Start the engine in the background so a later ask finds it warm. Does
/// nothing without an installed model, while a start is already under way, or
/// in the minute after a failed one.
pub fn warm<R: tauri::Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let decide = app.state::<DecideManager>();
        if decide.is_warm() || decide.starting.load(Ordering::Relaxed) || decide.recently_failed() {
            return;
        }
        let mgr = app.state::<RuntimeManager>();
        let db = app.state::<Db>();
        let Some(local) = resolve_local(&mgr, &db).await else { return };
        // Only the hot path warms the engine, and the hot path needs the card.
        if !local.gpu {
            return;
        }
        if let Err(e) = decide.ensure_started(&mgr.client, &local).await {
            let _ = db.log_activity(None, "decide", &format!("the decision engine did not start: {e}"));
        }
    });
}

/// A background purpose that shares the graphics card with a streaming turn
/// waits for the turn to end rather than slowing it down (`DEC-4`). Gives up
/// waiting after a minute: a background question can afford to be slow, not to
/// be starved.
pub async fn wait_for_quiet_gpu<R: tauri::Runtime>(app: &AppHandle<R>, gpu: bool) {
    if !gpu {
        return;
    }
    let Some(fleet) = app.try_state::<crate::agent::fleet::Fleet>() else { return };
    let until = Instant::now() + Duration::from_secs(60);
    while !fleet.roots().is_empty() && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// The head check (`DEC-3`): start the engine on this model and ask the fixed
/// three-question request. A file that only generates text answers HTTP 501,
/// which comes back as "This file can't make decisions."
pub async fn head_check(decide: &DecideManager, client: &reqwest::Client, local: &Local) -> Result<(), String> {
    use crate::agent::decide::{local as wire, DecideError};
    let live = decide.ensure_started(client, local).await.map_err(|e| e.to_string())?;
    let ask = wire::head_check_ask();
    let body = wire::build_body(&ask, ask.state);
    let reply = wire::post(client, &live.base_url, &live.token, &body, Duration::from_secs(120))
        .await
        .map_err(|e| match e {
            DecideError::Failed(m) => m,
            other => other.to_string(),
        })?;
    if wire::head_check_passes(&reply) {
        Ok(())
    } else {
        Err("This file can't make decisions. Download the supported one.".into())
    }
}

/// Background idle-stop loop, identical in shape to `rerankserver::spawn_idle_stop`.
pub fn spawn_idle_stop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let Some(mgr) = app.try_state::<DecideManager>() else { return };
            let idle = mgr.idle_for() >= IDLE_STOP;
            let mut guard = mgr.engine.lock().await;
            let died = guard.as_mut().map(|e| !e.still_running()).unwrap_or(false);
            let stopping = guard.is_some() && (idle || died);
            if stopping {
                *guard = None;
                mgr.running.store(false, Ordering::Relaxed);
            }
            drop(guard);
            if stopping {
                if let Some(db) = app.try_state::<Db>() {
                    let note = if died {
                        "the decision engine exited on its own"
                    } else {
                        "stopped the decision engine (idle)"
                    };
                    let _ = db.log_activity(None, "decide", note);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loads(vram: u64, ram: u64, model: u64, chat: u64, image: u64) -> Loads {
        Loads { vram_mb: vram, ram_mb: ram, model_mb: model, chat_mb: chat, image_mb: image }
    }

    #[test]
    fn the_catalog_names_files_not_repositories_and_the_urls_agree() {
        let catalog = decide_catalog();
        assert_eq!(catalog[0].name, "Clef-flash");
        for e in &catalog {
            assert!(e.url.ends_with(&e.filename), "{} url and filename disagree", e.name);
            assert!(e.mmproj_url.ends_with(&e.mmproj_filename), "{} projector url and filename disagree", e.name);
            assert!(e.url.starts_with("https://huggingface.co/ggml-org/"));
            assert!(e.bytes > 1_000_000_000 && e.mmproj_bytes > 100_000_000);
        }
    }

    /// `SMP-8a`: the words in front of the user.
    #[test]
    fn user_facing_copy_has_no_internal_words() {
        for e in decide_catalog() {
            let text = format!("{} {}", e.name, e.note).to_lowercase();
            for banned in ["logit", "joint head", "system one", "systemone", "noul"] {
                assert!(!text.contains(banned), "{} mentions {banned}", e.name);
            }
        }
    }

    #[test]
    fn auto_takes_the_card_only_when_the_model_fits_beside_the_others() {
        // 12 GB card, 7 GB model, 3 GB chat model, nothing else: 7+3+1 = 11 fits.
        assert_eq!(plan_fit(PlacementSetting::Auto, &loads(12_000, 32_000, 7_000, 3_000, 0)), Fits::Gpu);
        // The image engine's 4 GB tips it over to the processor.
        assert_eq!(plan_fit(PlacementSetting::Auto, &loads(12_000, 32_000, 7_000, 3_000, 4_096)), Fits::Cpu);
        // No card at all.
        assert_eq!(plan_fit(PlacementSetting::Auto, &loads(0, 32_000, 7_000, 0, 0)), Fits::Cpu);
    }

    #[test]
    fn a_model_bigger_than_memory_does_not_fit() {
        assert_eq!(plan_fit(PlacementSetting::Auto, &loads(8_000, 16_000, 20_000, 0, 0)), Fits::WontFit);
        assert_eq!(plan_fit(PlacementSetting::Cpu, &loads(24_000, 8_000, 20_000, 0, 0)), Fits::WontFit);
    }

    #[test]
    fn the_overrides_are_honoured() {
        // Forced onto the card even when it would be tight.
        assert_eq!(plan_fit(PlacementSetting::Gpu, &loads(8_000, 32_000, 7_000, 3_000, 0)), Fits::Gpu);
        // Forced onto the processor even when the card has room.
        assert_eq!(plan_fit(PlacementSetting::Cpu, &loads(24_000, 32_000, 7_000, 0, 0)), Fits::Cpu);
        // "Graphics card" on a machine without one cannot be honoured.
        assert_eq!(plan_fit(PlacementSetting::Gpu, &loads(0, 32_000, 7_000, 0, 0)), Fits::Cpu);
    }

    #[test]
    fn placement_setting_parses_and_defaults_to_auto() {
        assert_eq!(PlacementSetting::parse(Some("gpu")), PlacementSetting::Gpu);
        assert_eq!(PlacementSetting::parse(Some("cpu")), PlacementSetting::Cpu);
        assert_eq!(PlacementSetting::parse(Some("whatever")), PlacementSetting::Auto);
        assert_eq!(PlacementSetting::parse(None), PlacementSetting::Auto);
    }

    /// Without these the server refuses a state over 512 tokens (HTTP 500).
    #[test]
    fn the_batch_sizes_match_the_state_limit() {
        let args = extra_args(None);
        let at = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].clone());
        assert_eq!(at("--batch-size").as_deref(), Some("16384"));
        assert_eq!(at("--ubatch-size").as_deref(), Some("16384"));
        assert!(!args.iter().any(|a| a == "--mmproj"));
        let with = extra_args(Some(Path::new("C:\\m\\mmproj.gguf")));
        assert_eq!(with.last().map(String::as_str), Some("C:\\m\\mmproj.gguf"));
        assert!(with.iter().any(|a| a == "--mmproj"));
    }

    #[test]
    fn a_fresh_manager_is_idle_and_not_warm() {
        let m = DecideManager::new();
        assert!(!m.is_warm());
        assert_eq!(m.running_gpu(), None);
        let s = m.status(None);
        assert_eq!(s.state, "idle");
        assert!(!s.model_installed);
        assert_eq!(m.status(Some("Clef-flash".into())).model_name.as_deref(), Some("Clef-flash"));
    }

    #[test]
    fn the_median_uses_recent_answers_and_stays_bounded() {
        let m = DecideManager::new();
        assert_eq!(m.median_ms(), None);
        for ms in [10, 90, 30] {
            m.record_ms(ms);
        }
        assert_eq!(m.median_ms(), Some(30));
        for _ in 0..100 {
            m.record_ms(5);
        }
        assert_eq!(m.samples.lock().unwrap().len(), SAMPLES);
        assert_eq!(m.median_ms(), Some(5));
    }

    #[test]
    fn a_failed_start_blocks_a_retry_for_a_while_and_shows_as_failed() {
        let m = DecideManager::new();
        *m.last_error.lock().unwrap() = Some(("it ran out of memory".into(), Instant::now()));
        assert!(m.recently_failed());
        let s = m.status(Some("Clef-flash".into()));
        assert_eq!(s.state, "failed");
        assert_eq!(s.error.as_deref(), Some("it ran out of memory"));
    }
}
