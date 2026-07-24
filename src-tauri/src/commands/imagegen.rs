//! One-click local image-generation setup (9F): download the hardware-matched
//! `stable-diffusion.cpp` engine + a default model, then configure + enable the
//! skill — reusing the Phase-1 runtime download machinery. Mirrors how the
//! llama.cpp engine is provisioned, so a consumer never touches a file path.

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::State;

use crate::agent::imagegen::{BINARY_KEY, MODEL_KEY};
use crate::agent::skills::Skill;
use crate::db::Db;
use crate::runtime::download::{
    download_with_resume, resolve_asset_from, unpack_zip, DownloadProgress,
};
use crate::runtime::hardware::detect_hardware;
use crate::runtime::imageengine::{
    find_sd_binary, sd_asset_keywords, sd_cudart_keywords, DEFAULT_MODEL_NAME, DEFAULT_MODEL_URL,
    SD_PINNED_TAG, SD_REPO,
};
use crate::runtime::manifest::select_runtime;
use crate::runtime::RuntimeManager;
use crate::NexusError;

type Cmd<T> = Result<T, NexusError>;

fn err<E: std::fmt::Display>(e: E) -> NexusError {
    NexusError::Message(e.to_string())
}

/// Whether local image generation is ready, and where its pieces live.
#[derive(serde::Serialize)]
pub struct ImageSetupStatus {
    pub engine_installed: bool,
    pub engine_path: Option<String>,
    pub model_installed: bool,
    pub model_path: Option<String>,
    pub skill_enabled: bool,
}

fn setting_path(db: &Db, key: &str) -> Option<String> {
    db.get_setting(key)
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        // A stored binary/model path whose file no longer exists (deleted or a
        // wiped partial download) is treated as unset, so the next install or
        // download re-claims the default instead of pointing at a dead path.
        .filter(|s| std::path::Path::new(s).exists())
}

/// Report current setup state for the Settings UI.
#[tauri::command]
pub fn image_setup_status_cmd(db: State<'_, Db>) -> ImageSetupStatus {
    let engine_path = setting_path(&db, BINARY_KEY);
    let model_path = setting_path(&db, MODEL_KEY);
    ImageSetupStatus {
        engine_installed: engine_path.as_deref().map(|p| PathBuf::from(p).exists()).unwrap_or(false),
        model_installed: model_path.as_deref().map(|p| PathBuf::from(p).exists()).unwrap_or(false),
        skill_enabled: Skill::ImageGen.is_enabled(&db),
        engine_path,
        model_path,
    }
}

/// Download + install the image engine and a default model, then enable the
/// skill. Streams progress to the UI like the llama.cpp engine download (§5.4.1).
#[tauri::command]
pub async fn setup_image_generation_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<ImageSetupStatus> {
    // 1) Pick the backend for this machine (reuse the engine's selector).
    let profile = tauri::async_runtime::spawn_blocking(detect_hardware)
        .await
        .map_err(err)?;
    let backend = select_runtime(&profile).backend;

    // 2) Provision the sd.cpp binary into runtimes/sd/<tag>/<backend>/.
    let sd_dir = mgr.runtimes_dir().join("sd").join(SD_PINNED_TAG).join(backend.kebab());
    let binary = match find_sd_binary(&sd_dir) {
        Some(bin) => bin,
        None => {
            let asset = resolve_asset_from(&mgr.client, SD_REPO, SD_PINNED_TAG, &sd_asset_keywords(backend))
                .await
                .map_err(err)?;
            let archive = sd_dir.join(&asset.name);
            download_with_resume(&mgr.client, &asset.url, &archive, "Getting the image engine ready", |p| {
                let _ = on_progress.send(p);
            })
            .await
            .map_err(err)?;
            unpack_zip(&archive, &sd_dir).map_err(err)?;

            // NVIDIA also needs the CUDA runtime DLL package.
            if let Some(cudart_kw) = sd_cudart_keywords(backend) {
                if let Ok(cudart) = resolve_asset_from(&mgr.client, SD_REPO, SD_PINNED_TAG, &cudart_kw).await {
                    let cudart_archive = sd_dir.join(&cudart.name);
                    download_with_resume(&mgr.client, &cudart.url, &cudart_archive, "Getting GPU support files", |p| {
                        let _ = on_progress.send(p);
                    })
                    .await
                    .map_err(err)?;
                    unpack_zip(&cudart_archive, &sd_dir).map_err(err)?;
                }
            }

            find_sd_binary(&sd_dir)
                .ok_or_else(|| NexusError::Message("Image engine binary not found after extraction.".into()))?
        }
    };
    db.set_setting(BINARY_KEY, &binary.to_string_lossy()).map_err(err)?;

    // 3) Provision a default model into models/diffusion/.
    let model_path = mgr.models_dir().join("diffusion").join(DEFAULT_MODEL_NAME);
    if !model_path.exists() {
        download_with_resume(
            &mgr.client,
            DEFAULT_MODEL_URL,
            &model_path,
            "Downloading the image model (about 4 GB, one time)",
            |p| {
                let _ = on_progress.send(p);
            },
        )
        .await
        .map_err(|e| NexusError::Message(format!(
            "Couldn't download the default image model: {e}. You can pick a model file manually below."
        )))?;
    }
    db.set_setting(MODEL_KEY, &model_path.to_string_lossy()).map_err(err)?;

    // 4) Turn the skill on so it's usable immediately.
    Skill::ImageGen.set_enabled(&db, true);
    let _ = db.log_activity(None, "image", "Set up local image generation");

    Ok(image_setup_status_cmd(db))
}

/// Install only the image engine (hardware-matched sd.cpp binary), without a
/// model. Used by the Models → Image tab so the engine and models are managed
/// separately.
#[tauri::command]
pub async fn install_image_engine_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<ImageSetupStatus> {
    let profile = tauri::async_runtime::spawn_blocking(detect_hardware)
        .await
        .map_err(err)?;
    let backend = select_runtime(&profile).backend;
    let sd_dir = mgr.runtimes_dir().join("sd").join(SD_PINNED_TAG).join(backend.kebab());

    let binary = match find_sd_binary(&sd_dir) {
        Some(bin) => bin,
        None => {
            let asset = resolve_asset_from(&mgr.client, SD_REPO, SD_PINNED_TAG, &sd_asset_keywords(backend))
                .await
                .map_err(err)?;
            let archive = sd_dir.join(&asset.name);
            download_with_resume(&mgr.client, &asset.url, &archive, "Getting the image engine ready", |p| {
                let _ = on_progress.send(p);
            })
            .await
            .map_err(err)?;
            unpack_zip(&archive, &sd_dir).map_err(err)?;
            if let Some(cudart_kw) = sd_cudart_keywords(backend) {
                if let Ok(cudart) = resolve_asset_from(&mgr.client, SD_REPO, SD_PINNED_TAG, &cudart_kw).await {
                    let cudart_archive = sd_dir.join(&cudart.name);
                    download_with_resume(&mgr.client, &cudart.url, &cudart_archive, "Getting GPU support files", |p| {
                        let _ = on_progress.send(p);
                    })
                    .await
                    .map_err(err)?;
                    unpack_zip(&cudart_archive, &sd_dir).map_err(err)?;
                }
            }
            find_sd_binary(&sd_dir)
                .ok_or_else(|| NexusError::Message("Image engine binary not found after extraction.".into()))?
        }
    };
    db.set_setting(BINARY_KEY, &binary.to_string_lossy()).map_err(err)?;
    Ok(image_setup_status_cmd(db))
}

/// A diffusion model on disk.
#[derive(serde::Serialize)]
pub struct ImageModel {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub is_default: bool,
}

/// A curated, downloadable diffusion model suggestion.
#[derive(serde::Serialize)]
pub struct ImageCatalogEntry {
    pub name: String,
    pub note: String,
    pub size_label: String,
    pub url: String,
    pub filename: String,
}

fn diffusion_dir(mgr: &RuntimeManager) -> PathBuf {
    mgr.models_dir().join("diffusion")
}

/// Curated diffusion models. Every entry is a single, self-contained checkpoint
/// (UNet + VAE + text encoders in one file) that the engine loads with `-m`, and
/// every URL is an un-gated Hugging Face `resolve` link — no HF token needed, so
/// downloads work on any user's machine. Others can still be added by URL (like
/// the LLM library). Multi-file families (Flux, SD 3.5) are intentionally absent:
/// they ship as 3–4 separate files and their official repos are license-gated,
/// which the tokenless downloader can't fetch.
fn image_catalog() -> Vec<ImageCatalogEntry> {
    vec![
        ImageCatalogEntry {
            name: "Stable Diffusion 1.5".into(),
            note: "Fast and light. Runs on almost anything — great default.".into(),
            size_label: "~4 GB".into(),
            url: DEFAULT_MODEL_URL.into(),
            filename: DEFAULT_MODEL_NAME.into(),
        },
        ImageCatalogEntry {
            name: "SD-Turbo".into(),
            note: "Single-step 512px generation — near-instant. Best at 1–4 steps.".into(),
            size_label: "~4.9 GB".into(),
            url: "https://huggingface.co/stabilityai/sd-turbo/resolve/main/sd_turbo.safetensors".into(),
            filename: "sd_turbo.safetensors".into(),
        },
        ImageCatalogEntry {
            name: "SDXL-Turbo".into(),
            note: "SDXL quality in 1–4 steps. Fast and sharp; the go-to for most users.".into(),
            size_label: "~6.5 GB".into(),
            url: "https://huggingface.co/stabilityai/sdxl-turbo/resolve/main/sd_xl_turbo_1.0_fp16.safetensors".into(),
            filename: "sd_xl_turbo_1.0_fp16.safetensors".into(),
        },
        ImageCatalogEntry {
            name: "Stable Diffusion XL (base 1.0)".into(),
            note: "The full SDXL base — top all-round quality. Best at 25–40 steps.".into(),
            size_label: "~6.9 GB".into(),
            url: "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/resolve/main/sd_xl_base_1.0.safetensors".into(),
            filename: "sd_xl_base_1.0.safetensors".into(),
        },
        ImageCatalogEntry {
            name: "DreamShaper XL (Turbo v2)".into(),
            note: "Versatile SDXL finetune — art + photoreal, fast at ~6 steps.".into(),
            size_label: "~6.5 GB".into(),
            url: "https://huggingface.co/Lykon/dreamshaper-xl-v2-turbo/resolve/main/DreamShaperXL_Turbo_v2_1.safetensors".into(),
            filename: "DreamShaperXL_Turbo_v2_1.safetensors".into(),
        },
        ImageCatalogEntry {
            name: "Juggernaut XL (v9)".into(),
            note: "State-of-the-art SDXL photorealism. Best at 30–40 steps.".into(),
            size_label: "~6.6 GB".into(),
            url: "https://huggingface.co/RunDiffusion/Juggernaut-XL-v9/resolve/main/Juggernaut-XL_v9_RunDiffusionPhoto_v2.safetensors".into(),
            filename: "Juggernaut-XL_v9_RunDiffusionPhoto_v2.safetensors".into(),
        },
        ImageCatalogEntry {
            name: "RealVisXL (V4.0)".into(),
            note: "Photorealistic portraits & scenes. Best at 25–35 steps.".into(),
            size_label: "~6.5 GB".into(),
            url: "https://huggingface.co/SG161222/RealVisXL_V4.0/resolve/main/RealVisXL_V4.0.safetensors".into(),
            filename: "RealVisXL_V4.0.safetensors".into(),
        },
        ImageCatalogEntry {
            name: "Playground v2.5".into(),
            note: "High-aesthetic 1024px generations — vivid color and contrast.".into(),
            size_label: "~6.5 GB".into(),
            url: "https://huggingface.co/playgroundai/playground-v2.5-1024px-aesthetic/resolve/main/playground-v2.5-1024px-aesthetic.fp16.safetensors".into(),
            filename: "playground-v2.5-1024px-aesthetic.fp16.safetensors".into(),
        },
    ]
}

#[tauri::command]
pub fn image_catalog_cmd() -> Vec<ImageCatalogEntry> {
    image_catalog()
}

/// List diffusion models present on disk, marking the active default.
#[tauri::command]
pub fn list_image_models_cmd(mgr: State<'_, RuntimeManager>, db: State<'_, Db>) -> Vec<ImageModel> {
    let default = setting_path(&db, MODEL_KEY);
    let dir = diffusion_dir(&mgr);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let path = e.path();
            let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
            if !matches!(ext.as_str(), "safetensors" | "gguf" | "ckpt") {
                continue;
            }
            let path_str = path.to_string_lossy().to_string();
            out.push(ImageModel {
                name: path.file_name().and_then(|n| n.to_str()).unwrap_or("model").to_string(),
                size_bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
                is_default: default.as_deref() == Some(path_str.as_str()),
                path: path_str,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Download a diffusion model (by URL) into the diffusion dir; make it the
/// default if none is set yet. Streams progress.
#[tauri::command]
pub async fn download_image_model_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    url: String,
    filename: String,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<()> {
    let safe = filename
        .split(['/', '\\'])
        .last()
        .filter(|s| !s.is_empty())
        .unwrap_or("model.safetensors");
    let dest = diffusion_dir(&mgr).join(safe);
    if !dest.exists() {
        // Download into a sidecar `.part` file so an interrupted transfer can be
        // resumed and never masquerades as a finished, loadable model. Only a
        // fully-received file is atomically promoted to its final name.
        let part = dest.with_file_name(format!("{safe}.part"));
        download_with_resume(&mgr.client, &url, &part, "Downloading image model", |p| {
            let _ = on_progress.send(p);
        })
        .await
        .map_err(err)?;
        std::fs::rename(&part, &dest).map_err(err)?;
    }
    // First model becomes the default.
    if setting_path(&db, MODEL_KEY).is_none() {
        db.set_setting(MODEL_KEY, &dest.to_string_lossy()).map_err(err)?;
    }
    let _ = db.log_activity(None, "image", &format!("Downloaded image model {safe}"));
    Ok(())
}

/// Directly generate an image (the primary consumer path — not routed through the
/// chat model). Returns the path to the produced PNG.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn generate_image_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    prompt: String,
    model_path: Option<String>,
    negative: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    steps: Option<i64>,
) -> Cmd<String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(NexusError::Message("Enter a prompt describing the image.".into()));
    }
    let binary = setting_path(&db, BINARY_KEY)
        .ok_or_else(|| NexusError::Message("Install the image engine under Engine → Image first.".into()))?;
    let model = model_path
        .filter(|s| !s.trim().is_empty())
        .or_else(|| setting_path(&db, MODEL_KEY))
        .ok_or_else(|| NexusError::Message("Get an image model under Models → Image first.".into()))?;

    let out_path = mgr
        .generated_images_dir()
        .join(format!("img-{}.png", uuid::Uuid::new_v4().simple()));

    crate::agent::imagegen::generate(
        &binary,
        &model,
        &out_path,
        prompt,
        negative.as_deref(),
        width.unwrap_or(512),
        height.unwrap_or(512),
        steps.unwrap_or(20),
    )
    .await
    .map_err(NexusError::Message)?;

    let short = if prompt.len() > 60 { &prompt[..60] } else { prompt };
    let _ = db.log_activity(None, "image", &format!("generated: {short}"));
    Ok(out_path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn set_default_image_model_cmd(db: State<'_, Db>, path: String) -> Cmd<()> {
    db.set_setting(MODEL_KEY, &path).map_err(err)
}

#[tauri::command]
pub fn delete_image_model_cmd(db: State<'_, Db>, path: String) -> Cmd<()> {
    let _ = std::fs::remove_file(&path);
    if setting_path(&db, MODEL_KEY).as_deref() == Some(path.as_str()) {
        db.set_setting(MODEL_KEY, "").map_err(err)?;
    }
    Ok(())
}
