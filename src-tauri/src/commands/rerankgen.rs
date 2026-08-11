//! Reranking engine setup + model library (`RRK`): the re-read-engine twin of
//! `commands/embedgen.rs`. Reuses the same shared `llama-server` binary the
//! chat and recall engines already download — only the model and launch
//! flags differ.

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::State;

use crate::db::{Db, ModelEntry, NewModelEntry};
use crate::runtime::download::{download_with_resume, DownloadProgress};
use crate::runtime::rerankserver::{rerank_catalog, RerankCatalogEntry, RerankManager};
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

use super::embedgen::engine_binary_path;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// Settings key backing `RRK-UI-2`'s on/off toggle and `SMP-3`'s single
/// Good/Sharper control — both read and write the same flag, so switching
/// modes never leaves the two views disagreeing about whether reranking runs.
pub const RERANK_ENABLED_KEY: &str = "rerank.enabled";

fn rerank_models_dir(mgr: &RuntimeManager) -> PathBuf {
    mgr.models_dir().join("rerank")
}

/// Whether reranking is ready, and where its pieces live. Same shape as
/// `EmbedSetupStatus`, plus `enabled` — reranking installs without turning
/// itself on (`RRK`'s "optional, default off").
#[derive(serde::Serialize)]
pub struct RerankSetupStatus {
    pub engine_installed: bool,
    pub model_installed: bool,
    pub model_name: Option<String>,
    pub model_path: Option<String>,
    pub running: bool,
    pub enabled: bool,
}

async fn status_snapshot(mgr: &RuntimeManager, rerank_mgr: &RerankManager, db: &Db) -> Cmd<RerankSetupStatus> {
    let engine_path = engine_binary_path(mgr, db).await?;
    let model = db.default_model_by_role("rerank").map_err(err)?;
    let running = rerank_mgr.status().await.running;
    let enabled = db.get_setting(RERANK_ENABLED_KEY).ok().flatten().as_deref() == Some("true");
    Ok(RerankSetupStatus {
        engine_installed: engine_path.is_some(),
        model_installed: model
            .as_ref()
            .map(|m| PathBuf::from(&m.path).exists())
            .unwrap_or(false),
        model_name: model.as_ref().map(|m| m.name.clone()),
        model_path: model.as_ref().map(|m| m.path.clone()),
        running,
        enabled,
    })
}

#[tauri::command]
pub async fn rerank_setup_status_cmd(
    mgr: State<'_, RuntimeManager>,
    rerank_mgr: State<'_, RerankManager>,
    db: State<'_, Db>,
) -> Cmd<RerankSetupStatus> {
    status_snapshot(&mgr, &rerank_mgr, &db).await
}

/// Install the reranking engine: ensure the shared llama-server binary is
/// present (a no-op if it's already there), then download the default
/// reranking model if none is installed. Mirrors
/// `embedgen::install_embed_engine_cmd`'s one-click shape, but never turns
/// itself on — a separate, explicit `set_rerank_enabled_cmd` does that
/// (`RRK`'s "optional, default off").
#[tauri::command]
pub async fn install_rerank_engine_cmd(
    mgr: State<'_, RuntimeManager>,
    rerank_mgr: State<'_, RerankManager>,
    db: State<'_, Db>,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<RerankSetupStatus> {
    crate::commands::runtime::provision_active(&mgr, &db, &on_progress).await?;

    if db.default_model_by_role("rerank").map_err(err)?.is_none() {
        let entry = rerank_catalog().into_iter().next().expect("rerank catalog is never empty");
        let dest = rerank_models_dir(&mgr).join(&entry.filename);
        if !dest.exists() {
            download_with_resume(&mgr.client, &entry.url, &dest, "Downloading the re-read model", |p| {
                let _ = on_progress.send(p);
            })
            .await
            .map_err(err)?;
        }
        let size_bytes = std::fs::metadata(&dest).map(|m| m.len() as i64).ok();
        db.add_model_with_role(
            &NewModelEntry {
                name: entry.name.clone(),
                path: dest.to_string_lossy().to_string(),
                quant: Some("F16".into()),
                size_bytes,
                vision: false,
            },
            "rerank",
        )
        .map_err(err)?;
    }
    let _ = db.log_activity(None, "rerank", "Installed the re-read engine");
    status_snapshot(&mgr, &rerank_mgr, &db).await
}

/// Stop the engine (if running), turn it back off, and remove every installed
/// reranking model — same undo shape as `embedgen::remove_embed_engine_cmd`.
/// Leaves the shared llama-server binary in place.
#[tauri::command]
pub async fn remove_rerank_engine_cmd(
    mgr: State<'_, RuntimeManager>,
    rerank_mgr: State<'_, RerankManager>,
    db: State<'_, Db>,
) -> Cmd<RerankSetupStatus> {
    rerank_mgr.stop().await;
    let mut failed = Vec::new();
    for model in db.list_models_by_role("rerank").map_err(err)? {
        if let Err(e) = std::fs::remove_file(&model.path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                failed.push(model.path.clone());
            }
        }
        db.delete_model(&model.id).map_err(err)?;
    }
    db.set_setting(RERANK_ENABLED_KEY, "false").map_err(err)?;
    let _ = db.log_activity(None, "rerank", "Removed the re-read model");
    if !failed.is_empty() {
        return Err(PoiesisError::Message(format!(
            "Removed the re-read model, but couldn't delete {} — you can delete it by hand.",
            failed.join(", ")
        )));
    }
    status_snapshot(&mgr, &rerank_mgr, &db).await
}

/// `RRK-UI-2`'s toggle (and `SMP-3`'s `Sharper` choice): turn reranking on or
/// off without touching what's installed. Refuses to enable with nothing
/// installed — `RRK-UI-3`'s "explain, don't silently no-op".
#[tauri::command]
pub async fn set_rerank_enabled_cmd(mgr: State<'_, RuntimeManager>, db: State<'_, Db>, enabled: bool) -> Cmd<()> {
    if enabled && db.default_model_by_role("rerank").map_err(err)?.is_none() {
        return Err(PoiesisError::Message(
            "Install the re-read model first — there's nothing to re-read with yet.".into(),
        ));
    }
    let _ = &mgr; // kept for signature symmetry with the other engine setters
    db.set_setting(RERANK_ENABLED_KEY, if enabled { "true" } else { "false" }).map_err(err)
}

/// Best-effort rerank for `retrieval.rs` (`RRK-4`/`5`): resolves the installed
/// reranking model and the shared engine binary, scores `documents` against
/// `query`, and returns one score per document in the same order. `None` on
/// anything that isn't ready or fails — the caller falls back to the
/// embedding-only ranking untouched (`RRK-5`: reranking must never be able to
/// fail a search).
pub(crate) async fn rerank_or_none(
    mgr: &RuntimeManager,
    rerank_mgr: &RerankManager,
    db: &Db,
    query: &str,
    documents: &[String],
) -> Option<Vec<f32>> {
    if documents.is_empty() {
        return None;
    }
    if db.get_setting(RERANK_ENABLED_KEY).ok().flatten().as_deref() != Some("true") {
        return None;
    }
    let model = db.default_model_by_role("rerank").ok()??;
    let model_path = PathBuf::from(&model.path);
    if !model_path.exists() {
        return None;
    }
    let Some(server_binary) = engine_binary_path(mgr, db).await.ok().flatten() else {
        return None;
    };
    match rerank_mgr
        .rerank_documents(&mgr.client, server_binary, model_path, query, documents)
        .await
    {
        Ok(scores) => Some(scores),
        Err(e) => {
            // Only a genuine failure logs (RRK-5) — turned off or not yet
            // installed is the default, expected state and would otherwise
            // spam the activity log on every single search.
            let _ = db.log_activity(None, "rerank", &format!("re-read pass failed, used plain ranking: {e}"));
            None
        }
    }
}

#[tauri::command]
pub fn rerank_catalog_cmd() -> Vec<RerankCatalogEntry> {
    rerank_catalog()
}

#[tauri::command]
pub fn list_rerank_models_cmd(db: State<'_, Db>) -> Cmd<Vec<ModelEntry>> {
    db.list_models_by_role("rerank").map_err(err)
}

/// Download a specific reranking model (catalog entry or custom URL).
#[tauri::command]
pub async fn download_rerank_model_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    url: String,
    name: String,
    filename: String,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<ModelEntry> {
    let safe = filename
        .split(['/', '\\'])
        .last()
        .filter(|s| !s.is_empty())
        .unwrap_or("rerank-model.gguf");
    let dest = rerank_models_dir(&mgr).join(safe);
    if !dest.exists() {
        download_with_resume(&mgr.client, &url, &dest, "Downloading the re-read model", |p| {
            let _ = on_progress.send(p);
        })
        .await
        .map_err(err)?;
    }
    let size_bytes = std::fs::metadata(&dest).map(|m| m.len() as i64).ok();
    db.add_model_with_role(
        &NewModelEntry {
            name,
            path: dest.to_string_lossy().to_string(),
            quant: Some("F16".into()),
            size_bytes,
            vision: false,
        },
        "rerank",
    )
    .map_err(err)
}

/// Switch which model does the re-reading. Unlike the embedder, nothing
/// stored elsewhere depends on which reranker produced it — a cross-encoder
/// score is never persisted, only used for one query's ordering — so
/// switching is just a default-model change, no invalidation needed.
#[tauri::command]
pub async fn set_default_rerank_model_cmd(rerank_mgr: State<'_, RerankManager>, db: State<'_, Db>, id: String) -> Cmd<()> {
    let previous = db.default_model_by_role("rerank").map_err(err)?.map(|m| m.id);
    if previous.as_deref() == Some(id.as_str()) {
        return Ok(());
    }
    db.set_default_model(&id).map_err(err)?;
    rerank_mgr.stop().await;
    Ok(())
}

#[tauri::command]
pub async fn delete_rerank_model_cmd(rerank_mgr: State<'_, RerankManager>, db: State<'_, Db>, id: String) -> Cmd<()> {
    let was_default = db
        .default_model_by_role("rerank")
        .map_err(err)?
        .map(|m| m.id == id)
        .unwrap_or(false);
    if was_default {
        rerank_mgr.stop().await;
    }
    if let Some(path) = db.delete_model(&id).map_err(err)? {
        let _ = std::fs::remove_file(PathBuf::from(path));
    }
    if was_default && db.default_model_by_role("rerank").map_err(err)?.is_none() {
        db.set_setting(RERANK_ENABLED_KEY, "false").map_err(err)?;
    }
    Ok(())
}
