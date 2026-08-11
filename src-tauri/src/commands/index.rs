//! Folder indexing commands (Perception, IDX-UI): build/cancel/forget an
//! index, and read its status for the Workbench header and Settings.
//!
//! An index root is always the conversation's attached working folder —
//! `FolderHeader.tsx` is the one surface that offers "Read it", so there is
//! no separate path parameter to build/cancel: the folder is re-resolved from
//! the conversation on every call (`IDX-1`'s "re-check on every build, not
//! just at start"), never trusted from a stale caller-supplied path.

use tauri::ipc::Channel;
use tauri::State;

use crate::agent::index::{self, IndexManager, IndexProgress, SkippedFile};
use crate::agent::toolsets::Toolset;
use crate::db::Db;
use crate::permissions::canonicalize_lenient;
use crate::runtime::{EmbedManager, RuntimeManager};
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// One indexed root, shaped for the frontend (`IDX-UI-1`/`IDX-UI-4`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexRootView {
    pub path: String,
    /// idle | building | stale.
    pub state: String,
    pub file_count: i64,
    pub chunk_count: i64,
    pub skipped: Vec<SkippedFile>,
    pub size_bytes: i64,
    pub updated_at: i64,
    /// `IDX-UI-3`'s "N files changed" — only computed (a stat-only walk) when
    /// `state == "stale"`; `None` otherwise, since it costs a directory walk.
    pub changed_count: Option<i64>,
}

fn view_for(db: &Db, path: &str) -> Cmd<Option<IndexRootView>> {
    let Some(row) = db.get_index_root(path).map_err(err)? else {
        return Ok(None);
    };
    let skipped: Vec<SkippedFile> = row
        .skipped
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default();
    let size_bytes = db.scope_size_bytes("file", path).map_err(err)?;
    let changed_count = if row.state == "stale" {
        Some(index::count_changed(db, std::path::Path::new(path), path) as i64)
    } else {
        None
    };
    Ok(Some(IndexRootView {
        path: row.path,
        state: row.state,
        file_count: row.file_count,
        chunk_count: row.chunk_count,
        skipped,
        size_bytes,
        updated_at: row.updated_at,
        changed_count,
    }))
}

/// The conversation's attached folder, canonicalised — the only path IDX ever
/// acts on. `None` (never an error) when nothing is attached, so callers can
/// treat "no folder" as an ordinary, expected state.
fn attached_folder(db: &Db, conversation_id: &str) -> Cmd<Option<String>> {
    let (folder, _trust) = db.conversation_folder(conversation_id).map_err(err)?;
    Ok(folder.map(|f| canonicalize_lenient(std::path::Path::new(&f)).to_string_lossy().to_string()))
}

/// `IDX-UI-1`'s state for the currently attached folder, or `None` if either
/// no folder is attached or it has never been built ("I haven't read this
/// folder yet").
#[tauri::command]
pub fn index_status_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<Option<IndexRootView>> {
    let Some(path) = attached_folder(&db, &conversation_id)? else {
        return Ok(None);
    };
    view_for(&db, &path)
}

/// `SMP-4d`: a folder whose reading the user stopped is not read again on the
/// next attach — it goes back to offering `Read it`. Remembered per folder,
/// not per conversation, because the decision is about the folder.
fn stopped_key(path: &str) -> String {
    format!("index.stopped.{path}")
}

fn was_stopped(db: &Db, path: &str) -> bool {
    matches!(db.get_setting(&stopped_key(path)).ok().flatten().as_deref(), Some("true"))
}

/// `SMP-4a`: should attaching this folder start reading it straight away?
/// Only for a folder that has never been read and whose reading was never
/// stopped — a built, building or stale root is `IDX-UI-1`/`IDX-UI-3`'s
/// business, and a stale one is never rebuilt without the user's word.
#[tauri::command]
pub fn should_auto_index_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<bool> {
    if !Toolset::Indexing.is_enabled(&db) {
        return Ok(false);
    }
    let Some(path) = attached_folder(&db, &conversation_id)? else {
        return Ok(false);
    };
    if was_stopped(&db, &path) {
        return Ok(false);
    }
    Ok(db.get_index_root(&path).map_err(err)?.is_none())
}

/// Build (first time) or rebuild the attached folder's index, streaming
/// `IDX-7` progress as it goes. Runs to completion within this call — the
/// caller stays responsive because Tauri commands are already async, the same
/// way a model download is (`start_engine_cmd`, `install_embed_engine_cmd`).
#[tauri::command]
pub async fn build_index_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    index_mgr: State<'_, IndexManager>,
    db: State<'_, Db>,
    conversation_id: String,
    on_progress: Channel<IndexProgress>,
) -> Cmd<IndexRootView> {
    if !Toolset::Indexing.is_enabled(&db) {
        return Err(PoiesisError::Message(
            "Folder reading is off — turn it on in Settings → Tools first.".into(),
        ));
    }
    let Some(path) = attached_folder(&db, &conversation_id)? else {
        return Err(PoiesisError::Message("No folder is attached to this conversation.".into()));
    };
    if !std::path::Path::new(&path).is_dir() {
        return Err(PoiesisError::Message("That folder no longer exists.".into()));
    }
    // Asking for it explicitly undoes an earlier stop (SMP-4d).
    let _ = db.set_setting(&stopped_key(&path), "false");

    // Whether this is a genuine first build (⇒ "never built" on failure) or a
    // rebuild of something that already succeeded once (⇒ keep the old result
    // on failure rather than lose it).
    let had_prior = db
        .get_index_root(&path)
        .map_err(err)?
        .map(|r| r.file_count > 0)
        .unwrap_or(false);

    db.set_index_root_building(&path).map_err(err)?;
    let cancel = index_mgr.start(&path);
    let root = std::path::PathBuf::from(&path);
    let outcome = index::build_index(&mgr.client, &mgr, &embed_mgr, &db, &root, &cancel, |p| {
        let _ = on_progress.send(p);
    })
    .await;
    index_mgr.finish(&path);

    match outcome {
        Ok(built) => {
            let skipped_json = if built.skipped.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&built.skipped).map_err(err)?)
            };
            db.set_index_root_result(
                &path,
                &built.model,
                built.dim,
                built.file_count,
                built.chunk_count,
                skipped_json.as_deref(),
                "idle",
            )
            .map_err(err)?;
            let _ = db.log_activity(
                Some(&conversation_id),
                "index",
                &format!("read {} files in {path}", built.file_count),
            );
        }
        Err(message) => {
            // Revert `building` rather than leave it stuck: back to "never
            // built" for a first attempt, or back to the last good result for
            // a rebuild. A user-cancelled build isn't a failure — say so
            // quietly and stop, no error banner.
            if had_prior {
                db.set_index_root_state(&path, "idle").map_err(err)?;
            } else {
                db.forget_index_root(&path).map_err(err)?;
            }
            if message != "cancelled" {
                return Err(PoiesisError::Message(message));
            }
        }
    }

    Ok(view_for(&db, &path)?.unwrap_or(IndexRootView {
        path,
        state: "idle".into(),
        file_count: 0,
        chunk_count: 0,
        skipped: Vec::new(),
        size_bytes: 0,
        updated_at: 0,
        changed_count: None,
    }))
}

/// Stop a running build (`IDX-UI-1`'s "Stop"). `Ok(false)` if nothing was
/// building — not an error, just a no-op the button can ignore.
#[tauri::command]
pub fn cancel_index_cmd(index_mgr: State<'_, IndexManager>, db: State<'_, Db>, conversation_id: String) -> Cmd<bool> {
    let Some(path) = attached_folder(&db, &conversation_id)? else {
        return Ok(false);
    };
    // SMP-4d: remember the stop, so re-attaching this folder later offers
    // `Read it` rather than quietly starting over.
    let _ = db.set_setting(&stopped_key(&path), "true");
    Ok(index_mgr.cancel(&path))
}

/// `IDX-UI-4`'s "Forget this folder": drop its row and every vector it
/// produced. Takes an explicit path, since Settings lists every indexed root
/// across conversations, not just the currently attached one.
#[tauri::command]
pub fn forget_index_cmd(db: State<'_, Db>, path: String) -> Cmd<()> {
    db.forget_index_root(&path).map_err(err)
}

/// Every indexed root, for the Settings → Tools → Folder reading list.
#[tauri::command]
pub fn list_index_roots_cmd(db: State<'_, Db>) -> Cmd<Vec<IndexRootView>> {
    db.list_index_roots()
        .map_err(err)?
        .into_iter()
        .map(|r| {
            view_for(&db, &r.path).map(|v| {
                v.unwrap_or(IndexRootView {
                    path: r.path,
                    state: r.state,
                    file_count: r.file_count,
                    chunk_count: r.chunk_count,
                    skipped: Vec::new(),
                    size_bytes: 0,
                    updated_at: r.updated_at,
                    changed_count: None,
                })
            })
        })
        .collect()
}
