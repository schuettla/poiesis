//! One-click local image-generation setup (9F): download the hardware-matched
//! `stable-diffusion.cpp` engine + a default model, then configure + enable the
//! toolset — reusing the Phase-1 runtime download machinery. Mirrors how the
//! llama.cpp engine is provisioned, so a consumer never touches a file path.

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::State;

use crate::agent::imagegen::{BINARY_KEY, MODEL_KEY};
use crate::agent::toolsets::Toolset;
use crate::db::Db;
use crate::media;
use crate::media::imagecatalog::{self, image_catalog, BundleManifest, ImageCatalogEntry};
use crate::runtime::download::{
    download_with_resume, resolve_asset_from, unpack_zip, DownloadProgress,
};
use crate::runtime::hardware::{classify_fit, detect_hardware, Fit};
use crate::runtime::imageengine::{
    find_sd_binary, sd_asset_keywords, sd_cudart_keywords, DEFAULT_MODEL_NAME, DEFAULT_MODEL_URL,
    SD_CUDART_PREFIX, SD_ENGINE_PREFIX,
    SD_PINNED_TAG, SD_REPO,
};
use crate::runtime::manifest::select_runtime;
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// Whether local image generation is ready, and where its pieces live.
#[derive(serde::Serialize)]
pub struct ImageSetupStatus {
    pub engine_installed: bool,
    pub engine_path: Option<String>,
    pub model_installed: bool,
    pub model_path: Option<String>,
    pub toolset_enabled: bool,
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
        toolset_enabled: Toolset::ImageGen.is_enabled(&db),
        engine_path,
        model_path,
    }
}

/// Download + install the image engine and a default model, then enable the
/// toolset. Streams progress to the UI like the llama.cpp engine download (§5.4.1).
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
            let asset = resolve_asset_from(&mgr.client, SD_REPO, SD_PINNED_TAG, SD_ENGINE_PREFIX, &sd_asset_keywords(backend))
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
                if let Ok(cudart) = resolve_asset_from(&mgr.client, SD_REPO, SD_PINNED_TAG, SD_CUDART_PREFIX, &cudart_kw).await {
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
                .ok_or_else(|| PoiesisError::Message("Image engine binary not found after extraction.".into()))?
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
        .map_err(|e| PoiesisError::Message(format!(
            "Couldn't download the default image model: {e}. You can pick a model file manually below."
        )))?;
    }
    db.set_setting(MODEL_KEY, &model_path.to_string_lossy()).map_err(err)?;

    // 4) Turn the toolset on so it's usable immediately.
    Toolset::ImageGen.set_enabled(&db, true);
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
            let asset = resolve_asset_from(&mgr.client, SD_REPO, SD_PINNED_TAG, SD_ENGINE_PREFIX, &sd_asset_keywords(backend))
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
                if let Ok(cudart) = resolve_asset_from(&mgr.client, SD_REPO, SD_PINNED_TAG, SD_CUDART_PREFIX, &cudart_kw).await {
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
                .ok_or_else(|| PoiesisError::Message("Image engine binary not found after extraction.".into()))?
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

fn diffusion_dir(mgr: &RuntimeManager) -> PathBuf {
    mgr.models_dir().join("diffusion")
}

/// A catalog entry paired with how it will run on *this* machine — the same
/// verdict the language marketplace shows, from the same classifier.
#[derive(serde::Serialize)]
pub struct ImageCatalogItem {
    #[serde(flatten)]
    entry: ImageCatalogEntry,
    fit: Fit,
    /// Plain-language memory requirement, e.g. "needs ~3.6 GB VRAM".
    vram_label: String,
}

const MB: u64 = 1024 * 1024;

#[tauri::command]
pub async fn image_catalog_cmd() -> Cmd<Vec<ImageCatalogItem>> {
    let hw = tauri::async_runtime::spawn_blocking(detect_hardware)
        .await
        .map_err(err)?;
    Ok(image_catalog()
        .into_iter()
        .map(|entry| {
            let vram_mb = entry.vram_bytes() / MB;
            // The transformer is what has to fit on the card…
            let mut fit = classify_fit(vram_mb, &hw);
            // …but the encoders still have to fit in RAM, so a machine that
            // can't hold them can't run the model however big its GPU is.
            let host_mb = entry.host_bytes() / MB;
            if hw.ram_mb < host_mb + 2048 {
                fit = Fit::WontFit;
            }
            let vram_label = format!("needs ~{:.1} GB VRAM", vram_mb as f64 / 1024.0);
            ImageCatalogItem { entry, fit, vram_label }
        })
        .collect())
}

/// Sum the bytes of a bundle directory's files.
fn dir_size(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// List diffusion models present on disk, marking the active default. A
/// multi-file model is one directory with a manifest, and is reported as a
/// single entry — its parts are not models in their own right and listing them
/// separately would offer the user a VAE to generate with.
#[tauri::command]
pub fn list_image_models_cmd(mgr: State<'_, RuntimeManager>, db: State<'_, Db>) -> Vec<ImageModel> {
    let default = setting_path(&db, MODEL_KEY);
    let dir = diffusion_dir(&mgr);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let path = e.path();
            let path_str = path.to_string_lossy().to_string();
            let is_default = default.as_deref() == Some(path_str.as_str());

            if let Some(manifest) = imagecatalog::read_manifest(&path) {
                out.push(ImageModel {
                    name: manifest.name,
                    size_bytes: dir_size(&path),
                    is_default,
                    path: path_str,
                });
                continue;
            }

            let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
            if !matches!(ext.as_str(), "safetensors" | "gguf" | "ckpt") {
                continue;
            }
            out.push(ImageModel {
                name: path.file_name().and_then(|n| n.to_str()).unwrap_or("model").to_string(),
                size_bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
                is_default,
                path: path_str,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Download a catalog entry — one file or a whole bundle — reporting a single
/// continuous progress bar across all of its parts. A bundle lands in its own
/// directory alongside a manifest naming which file plays which role; the
/// manifest is written last, so an interrupted download is never mistaken for
/// a usable model.
#[tauri::command]
pub async fn download_image_catalog_model_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    id: String,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<()> {
    let entry = image_catalog()
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| PoiesisError::Message(format!("Unknown image model \"{id}\".")))?;

    let bundle = entry.arch.is_bundle();
    let dest_dir = if bundle { diffusion_dir(&mgr).join(&entry.id) } else { diffusion_dir(&mgr) };
    std::fs::create_dir_all(&dest_dir).map_err(err)?;

    let total = entry.total_bytes;
    let mut done: u64 = 0;
    let mut files = std::collections::BTreeMap::new();

    for (i, comp) in entry.components.iter().enumerate() {
        let dest = dest_dir.join(&comp.filename);
        files.insert(comp.role.clone(), comp.filename.clone());
        if dest.exists() {
            done += comp.size_bytes;
            continue;
        }
        let label = if entry.components.len() > 1 {
            format!("Downloading {} — part {} of {}", entry.name, i + 1, entry.components.len())
        } else {
            format!("Downloading {}", entry.name)
        };
        // A sidecar `.part` keeps an interrupted transfer resumable and stops
        // a half-written file from ever looking like a finished one.
        let part = dest.with_file_name(format!("{}.part", comp.filename));
        let base = done;
        download_with_resume(&mgr.client, &comp.url, &part, &label, |p| {
            let _ = on_progress.send(DownloadProgress {
                received: base + p.received,
                total: Some(total),
                label: p.label,
            });
        })
        .await
        .map_err(|e| {
            PoiesisError::Message(format!("Couldn't download {} ({}): {e}", entry.name, comp.filename))
        })?;
        std::fs::rename(&part, &dest).map_err(err)?;
        done += comp.size_bytes;
    }

    let model_path = if bundle {
        let manifest = BundleManifest { name: entry.name.clone(), profile: entry.profile.clone(), files };
        std::fs::write(
            dest_dir.join(imagecatalog::MANIFEST_NAME),
            serde_json::to_string_pretty(&manifest).map_err(err)?,
        )
        .map_err(err)?;
        dest_dir
    } else {
        dest_dir.join(&entry.components[0].filename)
    };

    // First model becomes the default.
    if setting_path(&db, MODEL_KEY).is_none() {
        db.set_setting(MODEL_KEY, &model_path.to_string_lossy()).map_err(err)?;
    }
    let _ = db.log_activity(None, "image", &format!("Downloaded image model {}", entry.name));
    Ok(())
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

/// Directly generate an image (the primary consumer path — not routed through
/// the chat model). Submits a background job and returns it (`JOB-1`); the
/// artifact arrives on the `poiesis-media-job` event. The artifact itself is
/// the data-loss fix (`ART-2`) — every image made through the composer used
/// to exist only as a message attachment, absent from Library and unsaveable.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn generate_image_cmd(
    db: State<'_, Db>,
    conversation_id: Option<String>,
    message_id: Option<String>,
    prompt: String,
    model_path: Option<String>,
    negative: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    steps: Option<i64>,
    seed: Option<i64>,
) -> Cmd<crate::db::MediaJob> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(PoiesisError::Message("Enter a prompt describing the image.".into()));
    }
    // `model_path` picks a specific local model; the local backend still reads
    // the *engine* binary from settings, so this only overrides which model
    // file it loads if one was already configured.
    if let Some(model_path) = model_path.filter(|s| !s.trim().is_empty()) {
        db.set_setting(MODEL_KEY, &model_path).map_err(err)?;
    }

    let req = media::MediaRequest {
        prompt: prompt.to_string(),
        negative: negative.filter(|n| !n.trim().is_empty()),
        width,
        height,
        steps,
        seed,
        ..Default::default()
    };

    media::jobs::submit(
        &db,
        media::jobs::SubmitArgs {
            conversation_id,
            message_id,
            modality: media::Modality::Image,
            // No declared model: this path is the local engine by way of the
            // precedence chain, which `model_path` above has already pointed
            // at the right checkpoint.
            model_id: None,
            request: req,
            parent_artifact_id: None,
        },
    )
    .map_err(PoiesisError::Message)
}

#[tauri::command]
pub fn set_default_image_model_cmd(db: State<'_, Db>, path: String) -> Cmd<()> {
    db.set_setting(MODEL_KEY, &path).map_err(err)
}

/// The first diffusion checkpoint still on disk, if any — used to re-home the
/// default when the current one is deleted. Bundle directories count, since
/// they are models too.
fn first_diffusion_model(mgr: &RuntimeManager) -> Option<String> {
    let mut found: Vec<String> = std::fs::read_dir(diffusion_dir(mgr))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            if imagecatalog::read_manifest(p).is_some() {
                return true;
            }
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
            matches!(ext.as_str(), "safetensors" | "gguf" | "ckpt")
        })
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    found.sort();
    found.into_iter().next()
}

#[tauri::command]
pub fn delete_image_model_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    path: String,
) -> Cmd<()> {
    // A multi-file model is a directory; removing only the file it names would
    // leave tens of gigabytes of orphaned parts behind.
    if std::path::Path::new(&path).is_dir() {
        let _ = std::fs::remove_dir_all(&path);
    }
    let _ = std::fs::remove_file(&path);
    // Compare against the *raw* setting, not `setting_path`: that helper hides
    // paths whose file is missing, and this one has just been deleted — so the
    // stored default could never match here and was left naming a deleted
    // checkpoint, which hid every remaining model from the picker too.
    let current = db.get_setting(MODEL_KEY).ok().flatten().unwrap_or_default();
    if current == path {
        // Promote whatever is still on disk, mirroring how the language-model
        // library re-homes its default in `Db::delete_model`.
        let next = first_diffusion_model(&mgr).unwrap_or_default();
        db.set_setting(MODEL_KEY, &next).map_err(err)?;
    }
    Ok(())
}
