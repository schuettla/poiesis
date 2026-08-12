//! Marketplace + library commands (Phase 3, MKT-1..6).

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::State;

use crate::db::{Db, ModelEntry, NewModelEntry};
use crate::marketplace::{
    catalog::{estimate_speed, CatalogModel},
    github, huggingface, recommended_catalog,
};
use crate::runtime::download::{download_with_resume, DownloadProgress};
use crate::runtime::hardware::{classify_fit, detect_hardware, Fit};
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// A catalog model paired with its fit verdict + speed estimate (MKT-2, MKT-4).
#[derive(serde::Serialize)]
pub struct CatalogEntry {
    #[serde(flatten)]
    model: CatalogModel,
    fit: Fit,
    speed: String,
}

async fn with_fit(models: Vec<CatalogModel>) -> Cmd<Vec<CatalogEntry>> {
    let hw = tauri::async_runtime::spawn_blocking(detect_hardware)
        .await
        .map_err(err)?;
    Ok(models
        .into_iter()
        .map(|m| {
            let fit = classify_fit(m.size_mb, &hw);
            let speed = estimate_speed(m.size_mb, fit);
            CatalogEntry { model: m, fit, speed }
        })
        .collect())
}

/// Curated "recommended" models with fit badges (D-5, §5.4.2).
#[tauri::command]
pub async fn recommended_catalog_cmd() -> Cmd<Vec<CatalogEntry>> {
    with_fit(recommended_catalog()).await
}

/// Search Hugging Face for GGUF models (MKT-1).
#[tauri::command]
pub async fn search_huggingface_cmd(
    mgr: State<'_, RuntimeManager>,
    query: String,
) -> Cmd<Vec<huggingface::HfModelSummary>> {
    huggingface::search_models(&mgr.client, &query, 20)
        .await
        .map_err(err)
}

/// List a Hugging Face repo's GGUF files with sizes + fit (MKT-2, MKT-6).
#[tauri::command]
pub async fn list_repo_files_cmd(
    mgr: State<'_, RuntimeManager>,
    repo: String,
) -> Cmd<Vec<CatalogEntry>> {
    let models = huggingface::list_gguf_files(&mgr.client, &repo)
        .await
        .map_err(err)?;
    with_fit(models).await
}

/// List GGUF assets from a GitHub repo's releases with fit (MKT-1, both sources).
#[tauri::command]
pub async fn list_github_models_cmd(
    mgr: State<'_, RuntimeManager>,
    owner_repo: String,
) -> Cmd<Vec<CatalogEntry>> {
    let models = github::list_release_models(&mgr.client, &owner_repo)
        .await
        .map_err(err)?;
    with_fit(models).await
}

fn filename_from_url(url: &str, fallback: &str) -> String {
    url.split('?')
        .next()
        .and_then(|u| u.rsplit('/').next())
        .filter(|n| n.to_ascii_lowercase().ends_with(".gguf"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{fallback}.gguf"))
}

/// Download a GGUF (from the catalog or a user-supplied URL) into the local
/// library with resume support + progress, then register it (MKT-3, MKT-5, MKT-6).
#[tauri::command]
pub async fn download_model_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    url: String,
    name: String,
    quant: Option<String>,
    vision: Option<bool>,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<ModelEntry> {
    let filename = filename_from_url(&url, &name.replace(' ', "-"));
    let dest = mgr.models_dir().join(&filename);
    let dest_str = dest.to_string_lossy().to_string();

    // Serialize concurrent requests for the same destination (e.g. the model
    // view is left mid-download, the user comes back and clicks "Download"
    // again) so two streams never write to the same file at once.
    let _guard = mgr.lock_download(&dest).await;

    // Already registered by an earlier call — the common case for a repeat
    // click once the first request finished in the background. Registering it
    // again would add a duplicate row for the same file.
    let existing = db.find_model_by_path(&dest_str).map_err(err)?;
    if let Some(entry) = &existing {
        let on_disk = std::fs::metadata(&dest).map(|m| m.len() as i64).ok();
        match (on_disk, entry.size_bytes) {
            // Registered and the bytes match what was recorded — nothing to do.
            (Some(a), Some(b)) if a == b => return Ok(entry.clone()),
            // The file is there but its size was never recorded, so there is
            // nothing to contradict it. Deleting on a mere unknown would throw
            // away a good download.
            (Some(_), None) => return Ok(entry.clone()),
            // Recorded as a different size: half-written, or corrupted by the
            // very concurrent-download bug this guard exists to prevent.
            // Resuming would append onto bytes we can't vouch for.
            (Some(_), Some(_)) => {
                let _ = std::fs::remove_file(&dest);
            }
            // Gone from disk entirely — just fetch it again.
            (None, _) => {}
        }
    }

    download_with_resume(&mgr.client, &url, &dest, "Getting your model ready", |p| {
        let _ = on_progress.send(p);
    })
    .await
    .map_err(err)?;

    let size_bytes = std::fs::metadata(&dest).map(|m| m.len() as i64).ok();
    if let Some(entry) = existing {
        // Repairing a row that already names this file — update it in place
        // rather than adding a second one beside it.
        db.set_model_size(&entry.id, size_bytes).map_err(err)?;
        return Ok(ModelEntry { size_bytes, ..entry });
    }
    db.add_model(&NewModelEntry {
        name,
        path: dest_str,
        quant,
        size_bytes,
        vision: vision.unwrap_or(false),
    })
    .map_err(err)
}

/// Add a model already on disk by its file path (MKT-6 local import).
#[tauri::command]
pub fn add_local_model_cmd(
    db: State<'_, Db>,
    path: String,
    name: String,
    quant: Option<String>,
    vision: Option<bool>,
) -> Cmd<ModelEntry> {
    let size_bytes = std::fs::metadata(&path).map(|m| m.len() as i64).ok();
    db.add_model(&NewModelEntry {
        name,
        path,
        quant,
        size_bytes,
        vision: vision.unwrap_or(false),
    })
    .map_err(err)
}

#[tauri::command]
pub fn list_models_cmd(db: State<'_, Db>) -> Cmd<Vec<ModelEntry>> {
    db.list_models().map_err(err)
}

/// Remove a model from the library and delete its file to reclaim space (MKT-5).
#[tauri::command]
pub fn delete_model_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    if let Some(path) = db.delete_model(&id).map_err(err)? {
        let _ = std::fs::remove_file(PathBuf::from(path));
    }
    Ok(())
}

#[tauri::command]
pub fn set_default_model_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    db.set_default_model(&id).map_err(err)
}
