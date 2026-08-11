//! Embedding engine setup + model library (Perception, EMB): the recall-engine
//! twin of `commands/imagegen.rs`. Reuses the same `llama-server` binary the
//! chat engine downloads — only the model and launch flags differ — so
//! installing is just "get a small embedding model", not a second engine
//! download.

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::State;

use crate::db::{Db, ModelEntry, NewModelEntry};
use crate::runtime::download::{download_with_resume, find_server_binary, DownloadProgress};
use crate::runtime::embedserver::{embed_catalog, EmbedCatalogEntry, EmbedManager};
use crate::runtime::hardware::detect_hardware;
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

fn embed_models_dir(mgr: &RuntimeManager) -> PathBuf {
    mgr.models_dir().join("embed")
}

/// The shared llama-server binary path for this machine's active backend, if
/// already provisioned — never triggers a download (that only happens via
/// `install_embed_engine_cmd`, same as the chat engine's own install flow).
pub(crate) async fn engine_binary_path(mgr: &RuntimeManager, db: &Db) -> Cmd<Option<PathBuf>> {
    let profile = tauri::async_runtime::spawn_blocking(detect_hardware)
        .await
        .map_err(err)?;
    let (selection, backend) = crate::commands::runtime::active_selection(&profile, db);
    let dir = mgr.runtimes_dir().join(&selection.build_tag).join(backend.kebab());
    Ok(find_server_binary(&dir))
}

/// Whether local semantic recall is ready, and where its pieces live.
#[derive(serde::Serialize)]
pub struct EmbedSetupStatus {
    pub engine_installed: bool,
    pub model_installed: bool,
    pub model_name: Option<String>,
    pub model_path: Option<String>,
    /// True once the engine has actually spun up (lazy-started, EMB-2) — a
    /// separate fact from "installed", since it idles down after 5 minutes.
    pub running: bool,
}

async fn status_snapshot(mgr: &RuntimeManager, embed_mgr: &EmbedManager, db: &Db) -> Cmd<EmbedSetupStatus> {
    let engine_path = engine_binary_path(mgr, db).await?;
    let model = db.default_model_by_role("embed").map_err(err)?;
    let running = embed_mgr.status().await.running;
    Ok(EmbedSetupStatus {
        engine_installed: engine_path.is_some(),
        model_installed: model
            .as_ref()
            .map(|m| PathBuf::from(&m.path).exists())
            .unwrap_or(false),
        model_name: model.as_ref().map(|m| m.name.clone()),
        model_path: model.as_ref().map(|m| m.path.clone()),
        running,
    })
}

#[tauri::command]
pub async fn embed_setup_status_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    db: State<'_, Db>,
) -> Cmd<EmbedSetupStatus> {
    status_snapshot(&mgr, &embed_mgr, &db).await
}

/// Install the recall engine: ensure the shared llama-server binary is
/// present (reusing the chat engine's provisioning — a no-op if it's already
/// there), then download the default embedding model if none is installed.
/// Mirrors `setup_image_generation_cmd`'s one-click shape.
#[tauri::command]
pub async fn install_embed_engine_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    db: State<'_, Db>,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<EmbedSetupStatus> {
    crate::commands::runtime::provision_active(&mgr, &db, &on_progress).await?;

    if db.default_model_by_role("embed").map_err(err)?.is_none() {
        let entry = embed_catalog().into_iter().next().expect("embed catalog is never empty");
        let dest = embed_models_dir(&mgr).join(&entry.filename);
        if !dest.exists() {
            download_with_resume(&mgr.client, &entry.url, &dest, "Downloading the recall model", |p| {
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
            "embed",
        )
        .map_err(err)?;
    }
    let _ = db.log_activity(None, "embed", "Installed the recall engine");
    status_snapshot(&mgr, &embed_mgr, &db).await
}

/// Stop the engine (if running) and remove **every** installed recall model,
/// so the "install my recall engine" affordance has a real undo. Removing only
/// the default would leave a second model on disk with nothing pointing at it.
/// The vectors go too: they mean nothing without the model that produced them
/// (VEC-4). Leaves the shared llama-server binary in place — the chat engine
/// may still need it.
#[tauri::command]
pub async fn remove_embed_engine_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    db: State<'_, Db>,
) -> Cmd<EmbedSetupStatus> {
    // Stop first: on Windows the running engine holds the .gguf open.
    embed_mgr.stop().await;
    let mut failed = Vec::new();
    for model in db.list_models_by_role("embed").map_err(err)? {
        if let Err(e) = std::fs::remove_file(&model.path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                failed.push(model.path.clone());
            }
        }
        db.delete_model(&model.id).map_err(err)?;
    }
    db.invalidate_all_vectors().map_err(err)?;
    let _ = db.log_activity(None, "embed", "Removed the recall model");
    if !failed.is_empty() {
        // The rows are gone either way; say so rather than leaving files behind
        // silently (HEAL-3's rule: set aside, then tell the user).
        return Err(PoiesisError::Message(format!(
            "Removed the recall model, but couldn't delete {} — you can delete it by hand.",
            failed.join(", ")
        )));
    }
    status_snapshot(&mgr, &embed_mgr, &db).await
}

/// Best-effort embedding for any caller (`SEM`, later `RET`): resolves the
/// installed recall model and the shared engine binary, embeds `texts`, and
/// returns the model name + dimension the embedding actually happened under.
/// Returns `None` on anything that isn't ready — no model, no engine binary,
/// or a failed request — so every caller degrades to keyword behaviour
/// instead of erroring (EMB-5). Never called with an empty `texts`.
pub(crate) async fn embed_texts_or_none(
    mgr: &RuntimeManager,
    embed_mgr: &EmbedManager,
    db: &Db,
    texts: &[String],
) -> Option<(Vec<Vec<f32>>, String, i64)> {
    if texts.is_empty() {
        return None;
    }
    let model = db.default_model_by_role("embed").ok()??;
    let model_path = PathBuf::from(&model.path);
    if !model_path.exists() {
        return None;
    }
    let server_binary = engine_binary_path(mgr, db).await.ok()??;
    let vectors = embed_mgr
        .embed_texts(&mgr.client, server_binary, model_path, texts)
        .await
        .ok()?;
    let dim = vectors.first()?.len() as i64;
    Some((vectors, model.name, dim))
}

#[tauri::command]
pub fn embed_catalog_cmd() -> Vec<EmbedCatalogEntry> {
    embed_catalog()
}

#[tauri::command]
pub fn list_embed_models_cmd(db: State<'_, Db>) -> Cmd<Vec<ModelEntry>> {
    db.list_models_by_role("embed").map_err(err)
}

/// Download a specific embedding model (catalog entry or custom URL).
#[tauri::command]
pub async fn download_embed_model_cmd(
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
        .unwrap_or("embed-model.gguf");
    let dest = embed_models_dir(&mgr).join(safe);
    if !dest.exists() {
        download_with_resume(&mgr.client, &url, &dest, "Downloading the recall model", |p| {
            let _ = on_progress.send(p);
        })
        .await
        .map_err(err)?;
    }
    let size_bytes = std::fs::metadata(&dest).map(|m| m.len() as i64).ok();
    db.add_model_with_role(
        &NewModelEntry { name, path: dest.to_string_lossy().to_string(), quant: Some("F16".into()), size_bytes, vision: false },
        "embed",
    )
    .map_err(err)
}

/// Switch which model does the recalling. Everything already embedded was
/// embedded in the *old* model's space, and the two are not comparable, so
/// changing it discards every vector and marks indexed folders stale (VEC-4).
/// Memory comes back on its own (SEM-2); folders wait for the user (IDX-UI-3).
#[tauri::command]
pub async fn set_default_embed_model_cmd(
    embed_mgr: State<'_, EmbedManager>,
    db: State<'_, Db>,
    id: String,
) -> Cmd<()> {
    let previous = db.default_model_by_role("embed").map_err(err)?.map(|m| m.id);
    if previous.as_deref() == Some(id.as_str()) {
        return Ok(());
    }
    db.set_default_model(&id).map_err(err)?;
    // The running engine holds the old model — the next request starts the new one.
    embed_mgr.stop().await;
    let dropped = db.invalidate_all_vectors().map_err(err)?;
    if previous.is_some() && dropped > 0 {
        let _ = db.log_activity(
            None,
            "embed",
            "Switched recall model — I'll need to learn what I'd read again",
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_embed_model_cmd(
    embed_mgr: State<'_, EmbedManager>,
    db: State<'_, Db>,
    id: String,
) -> Cmd<()> {
    let was_default = db
        .default_model_by_role("embed")
        .map_err(err)?
        .map(|m| m.id == id)
        .unwrap_or(false);
    if was_default {
        embed_mgr.stop().await;
    }
    if let Some(path) = db.delete_model(&id).map_err(err)? {
        let _ = std::fs::remove_file(PathBuf::from(path));
    }
    // Deleting the default promotes another model — a different space, so the
    // stored vectors are no more valid than after an explicit switch.
    if was_default {
        db.invalidate_all_vectors().map_err(err)?;
    }
    Ok(())
}
