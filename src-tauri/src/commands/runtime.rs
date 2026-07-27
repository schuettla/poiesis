//! Runtime subsystem commands (Phase 1): hardware survey, runtime provisioning,
//! engine lifecycle, and streamed chat completions.

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::State;

use crate::db::Db;
use crate::runtime::download::{
    download_with_resume, find_server_binary, resolve_asset, unpack_zip, DownloadProgress,
};
use crate::runtime::hardware::{detect_hardware, HardwareProfile};
use crate::runtime::manifest::{select_runtime, Backend, RuntimeSelection, PINNED_BUILD_TAG};
use crate::runtime::process::{EngineConfig, EngineStatus};
use crate::runtime::proxy::{stream_completion, StreamEvent};
use crate::runtime::RuntimeManager;
use crate::NexusError;

type Cmd<T> = Result<T, NexusError>;

fn err<E: std::fmt::Display>(e: E) -> NexusError {
    NexusError::Message(e.to_string())
}

/// Settings key holding the user's manual backend override (§7.3.3 step 5).
const BACKEND_OVERRIDE_KEY: &str = "runtime_backend_override";

/// Read a valid backend override from settings, if one is set and still
/// applicable to this machine's recommended selection.
fn backend_override(db: &Db, selection: &RuntimeSelection) -> Option<Backend> {
    db.get_setting(BACKEND_OVERRIDE_KEY)
        .ok()
        .flatten()
        .and_then(|s| Backend::from_kebab(&s))
        .filter(|b| *b == selection.backend || selection.alternates.contains(b))
}

/// The recommended selection for `profile` plus the backend actually in effect
/// (the override when valid, otherwise the recommendation).
fn active_selection(profile: &HardwareProfile, db: &Db) -> (RuntimeSelection, Backend) {
    let selection = select_runtime(profile);
    let active = backend_override(db, &selection).unwrap_or(selection.backend);
    (selection, active)
}

/// Provision a specific backend's `llama-server` into a per-backend directory
/// (`runtimes/<build>/<backend>`), idempotently. Downloads + unpacks the engine
/// (and the CUDA DLL package when applicable). Returns the server binary path.
async fn provision_backend(
    mgr: &RuntimeManager,
    backend: Backend,
    build_tag: &str,
    on_progress: &Channel<DownloadProgress>,
) -> Cmd<PathBuf> {
    let target_dir = mgr.runtimes_dir().join(build_tag).join(backend.kebab());
    if let Some(bin) = find_server_binary(&target_dir) {
        return Ok(bin);
    }

    let asset = resolve_asset(&mgr.client, build_tag, &backend.asset_keywords())
        .await
        .map_err(err)?;
    let archive_path = target_dir.join(&asset.name);
    download_with_resume(
        &mgr.client,
        &asset.url,
        &archive_path,
        "Getting the engine ready",
        |p| {
            let _ = on_progress.send(p);
        },
    )
    .await
    .map_err(err)?;
    unpack_zip(&archive_path, &target_dir).map_err(err)?;

    // NVIDIA backends also need the separate CUDA runtime DLL package.
    if let Some(cudart_kw) = backend.cudart_keywords() {
        if let Ok(cudart) = resolve_asset(&mgr.client, build_tag, &cudart_kw).await {
            let cudart_archive = target_dir.join(&cudart.name);
            download_with_resume(
                &mgr.client,
                &cudart.url,
                &cudart_archive,
                "Getting GPU support files",
                |p| {
                    let _ = on_progress.send(p);
                },
            )
            .await
            .map_err(err)?;
            unpack_zip(&cudart_archive, &target_dir).map_err(err)?;
        }
    }

    find_server_binary(&target_dir)
        .ok_or_else(|| NexusError::Message("engine binary not found after extraction".into()))
}

/// Provision whichever backend is in effect for this machine (override-aware).
async fn provision_active(
    mgr: &RuntimeManager,
    db: &Db,
    on_progress: &Channel<DownloadProgress>,
) -> Cmd<PathBuf> {
    let profile = tauri::async_runtime::spawn_blocking(detect_hardware)
        .await
        .map_err(err)?;
    let (selection, backend) = active_selection(&profile, db);
    provision_backend(mgr, backend, &selection.build_tag, on_progress).await
}

/// Survey the machine (MKT-4 input, §7.3.3). Cheap; called on first run and on
/// demand from Settings.
#[tauri::command]
pub async fn detect_hardware_cmd() -> Cmd<HardwareProfile> {
    // Detection shells out to system tools; run it off the async reactor.
    tauri::async_runtime::spawn_blocking(detect_hardware)
        .await
        .map_err(err)
}

/// Recommend the backend for this machine (§7.3.2), with a plain-language reason.
#[tauri::command]
pub async fn recommend_runtime_cmd() -> Cmd<RuntimeSelection> {
    let profile = tauri::async_runtime::spawn_blocking(detect_hardware)
        .await
        .map_err(err)?;
    Ok(select_runtime(&profile))
}

/// Current engine status (used for readiness gating in the UI).
#[tauri::command]
pub async fn runtime_status_cmd(mgr: State<'_, RuntimeManager>) -> Cmd<EngineStatus> {
    Ok(mgr.status().await)
}

/// Ensure the in-effect `llama-server` runtime is provisioned; returns the path
/// to the server binary. Streams download progress to the UI (§5.4.1).
#[tauri::command]
pub async fn ensure_runtime_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<String> {
    let bin = provision_active(&mgr, &db, &on_progress).await?;
    Ok(bin.to_string_lossy().to_string())
}

/// Load a model into the engine and block until ready (§7.4). Spawns the engine
/// (provisioning the runtime first if necessary).
#[tauri::command]
pub async fn load_model_cmd(
    app: tauri::AppHandle,
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    model_path: String,
    ctx_size: Option<u32>,
    n_gpu_layers: Option<u32>,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<EngineStatus> {
    let server_binary = provision_active(&mgr, &db, &on_progress).await?;
    let config = EngineConfig {
        server_binary,
        model_path: PathBuf::from(model_path),
        ctx_size: ctx_size.unwrap_or(4096),
        n_gpu_layers: n_gpu_layers.unwrap_or(999), // offload all by default; engine clamps
    };
    let status = mgr.load_model(config).await.map_err(err)?;
    // From here the engine keeps itself alive (HEAL-1).
    crate::runtime::watchdog::spawn(app, mgr.generation());
    Ok(status)
}

/// One selectable backend in the engine view, with its install + recommend state.
#[derive(serde::Serialize)]
pub struct BackendOption {
    backend: Backend,
    label: String,
    recommended: bool,
    installed: bool,
}

/// Everything the Engine view needs: hardware, the recommended + active backend,
/// install state, the running engine, and the selectable backend options.
#[derive(serde::Serialize)]
pub struct RuntimeOverview {
    hardware: HardwareProfile,
    recommended: RuntimeSelection,
    active_backend: Backend,
    override_backend: Option<Backend>,
    installed: bool,
    install_path: Option<String>,
    engine: EngineStatus,
    options: Vec<BackendOption>,
}

/// Snapshot of runtime + engine state for the Engine view.
#[tauri::command]
pub async fn runtime_overview_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
) -> Cmd<RuntimeOverview> {
    let profile = tauri::async_runtime::spawn_blocking(detect_hardware)
        .await
        .map_err(err)?;
    let selection = select_runtime(&profile);
    let override_backend = backend_override(&db, &selection);
    let active = override_backend.unwrap_or(selection.backend);

    let installed_dir = |b: Backend| mgr.runtimes_dir().join(&selection.build_tag).join(b.kebab());
    let active_bin = find_server_binary(&installed_dir(active));

    // The recommendation first, then its alternates (de-duplicated).
    let mut order = vec![selection.backend];
    for a in &selection.alternates {
        if !order.contains(a) {
            order.push(*a);
        }
    }
    let options = order
        .iter()
        .map(|b| BackendOption {
            backend: *b,
            label: b.label().to_string(),
            recommended: *b == selection.backend,
            installed: find_server_binary(&installed_dir(*b)).is_some(),
        })
        .collect();

    Ok(RuntimeOverview {
        hardware: profile,
        recommended: selection,
        active_backend: active,
        override_backend,
        installed: active_bin.is_some(),
        install_path: active_bin.map(|p| p.to_string_lossy().to_string()),
        engine: mgr.status().await,
        options,
    })
}

/// Set (or clear, when `backend` is `None`) the manual backend override
/// (§7.3.3 step 5). Takes effect on the next engine start.
#[tauri::command]
pub async fn set_backend_override_cmd(db: State<'_, Db>, backend: Option<String>) -> Cmd<()> {
    match backend {
        Some(s) => {
            let b = Backend::from_kebab(&s)
                .ok_or_else(|| NexusError::Message(format!("Unknown backend '{s}'.")))?;
            db.set_setting(BACKEND_OVERRIDE_KEY, b.kebab()).map_err(err)
        }
        None => db.set_setting(BACKEND_OVERRIDE_KEY, "").map_err(err),
    }
}

/// Start the engine on the default (or first available) library model, from the
/// Engine view. Provisions the runtime first if needed.
#[tauri::command]
pub async fn start_engine_cmd(
    app: tauri::AppHandle,
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<EngineStatus> {
    let model = db
        .default_model()
        .map_err(err)?
        .or_else(|| db.list_models().ok().and_then(|m| m.into_iter().next()))
        .ok_or_else(|| {
            NexusError::Message("No model in your library yet. Download one from Models first.".into())
        })?;
    let server_binary = provision_active(&mgr, &db, &on_progress).await?;
    let config = EngineConfig {
        server_binary,
        model_path: PathBuf::from(model.path),
        ctx_size: 4096,
        n_gpu_layers: 999,
    };
    let status = mgr.load_model(config).await.map_err(err)?;
    crate::runtime::watchdog::spawn(app, mgr.generation());
    Ok(status)
}

/// Information about an available upstream engine update (§7.3, D-4: pinned
/// build, surfaced as an *available* update rather than auto-applied).
#[derive(serde::Serialize)]
pub struct UpdateInfo {
    current: String,
    latest: String,
    update_available: bool,
}

/// Check whether a newer upstream llama.cpp build exists than our pinned one.
#[tauri::command]
pub async fn check_runtime_update_cmd(mgr: State<'_, RuntimeManager>) -> Cmd<UpdateInfo> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    let release: Release = mgr
        .client
        .get("https://api.github.com/repos/ggml-org/llama.cpp/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;
    Ok(UpdateInfo {
        update_available: release.tag_name != PINNED_BUILD_TAG,
        current: PINNED_BUILD_TAG.to_string(),
        latest: release.tag_name,
    })
}

/// Stop the running engine and release VRAM.
#[tauri::command]
pub async fn stop_engine_cmd(mgr: State<'_, RuntimeManager>) -> Cmd<()> {
    mgr.stop().await;
    Ok(())
}

/// Context window of the loaded local engine, or `null` when none is loaded
/// (CTX-1). The frontend budgets turns against this so llama.cpp never has to
/// truncate from the front — which would eat the system prompt.
#[tauri::command]
pub async fn get_context_budget_cmd(mgr: State<'_, RuntimeManager>) -> Cmd<Option<u32>> {
    Ok(mgr.engine_ctx_size().await)
}

/// A single chat message in the OpenAI-compatible shape.
#[derive(serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Stream a completion from the loaded local engine to the UI via `on_event`
/// (CHT-2). Cancellable via `stop_chat_cmd`.
#[tauri::command]
pub async fn chat_cmd(
    mgr: State<'_, RuntimeManager>,
    messages: Vec<ChatMessage>,
    temperature: Option<f32>,
    on_event: Channel<StreamEvent>,
) -> Cmd<()> {
    let Some((base_url, token)) = mgr.engine_endpoint().await else {
        return Err(NexusError::Message(
            "No model is loaded yet. Pick a model to get started.".into(),
        ));
    };

    let body = serde_json::json!({
        "messages": messages
            .iter()
            .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
            .collect::<Vec<_>>(),
        "temperature": temperature.unwrap_or(0.7),
        "stream": true,
    });

    let cancel = mgr.new_cancel();
    stream_completion(&mgr.client, &base_url, Some(&token), body, cancel, |evt| {
        let _ = on_event.send(evt);
    })
    .await
    .map_err(err)
}

/// Trip the active turn's cancellation flag (Stop control).
#[tauri::command]
pub fn stop_chat_cmd(mgr: State<'_, RuntimeManager>) {
    mgr.cancel_active();
}
