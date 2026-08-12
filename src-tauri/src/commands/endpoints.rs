//! Commands for a user's own OpenAI-compatible model server (Ollama, LM
//! Studio, or a remote box) — a third model source alongside the integrated
//! runtime and BYOK cloud providers (`commands::cloud`). Mirrors that
//! module's shape closely; see `cloud/endpoints.rs` for why this is a
//! separate concept from `cloud::Provider` rather than a new variant of it.

use futures_util::future::join_all;
use tauri::State;

use crate::cloud::endpoints::{self, EndpointInfo, EndpointModel, EndpointProbe};
use crate::db::Db;
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

/// Connected endpoints and whether a key is stored for each.
#[tauri::command]
pub fn list_endpoints_cmd(db: State<'_, Db>) -> Cmd<Vec<EndpointInfo>> {
    let rows = db.list_local_endpoints().map_err(|e| PoiesisError::Message(e.to_string()))?;
    Ok(rows.iter().map(EndpointInfo::from_row).collect())
}

/// Add a new connected server. `base_url` is normalized (scheme defaulted,
/// trailing slash / `/v1` stripped) before it's stored.
#[tauri::command]
pub fn add_endpoint_cmd(
    db: State<'_, Db>,
    label: String,
    base_url: String,
    api_key: Option<String>,
    ctx_size: Option<i64>,
) -> Cmd<EndpointInfo> {
    let label = label.trim();
    if label.is_empty() {
        return Err(PoiesisError::Message("Give it a name.".into()));
    }
    let base_url =
        endpoints::normalize_base_url(&base_url).map_err(PoiesisError::Message)?;
    // Adding the same server twice is an easy accident (two clicks on the
    // Ollama preset) and every model it serves would then appear twice in the
    // picker under two names. `model_library` needed a migration to clean up
    // exactly this class of duplicate; cheaper to refuse it at the door.
    let existing = db.list_local_endpoints().map_err(|e| PoiesisError::Message(e.to_string()))?;
    if let Some(dupe) = existing.iter().find(|e| e.base_url == base_url) {
        return Err(PoiesisError::Message(format!(
            "That address is already connected as “{}”.",
            dupe.label
        )));
    }
    let row = db
        .insert_local_endpoint(label, &base_url, ctx_size.unwrap_or(8192))
        .map_err(|e| PoiesisError::Message(e.to_string()))?;
    if let Some(key) = api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        endpoints::set_key(&row.id, key).map_err(|e| PoiesisError::Message(e.to_string()))?;
    }
    Ok(EndpointInfo::from_row(&row))
}

/// Update a connected server's label, address, or context window. Passing
/// `api_key` replaces the stored key; omit it to leave the existing key (or
/// lack of one) untouched.
#[tauri::command]
pub fn update_endpoint_cmd(
    db: State<'_, Db>,
    id: String,
    label: String,
    base_url: String,
    ctx_size: i64,
    api_key: Option<String>,
) -> Cmd<()> {
    let label = label.trim();
    if label.is_empty() {
        return Err(PoiesisError::Message("Give it a name.".into()));
    }
    let base_url =
        endpoints::normalize_base_url(&base_url).map_err(PoiesisError::Message)?;
    // Same duplicate rule as `add_endpoint_cmd`, minus this row itself — an
    // edit (or the `localhost`→`127.0.0.1` rewrite a re-test applies) must not
    // be able to collide two entries onto one address either.
    let existing = db.list_local_endpoints().map_err(|e| PoiesisError::Message(e.to_string()))?;
    if let Some(dupe) = existing.iter().find(|e| e.base_url == base_url && e.id != id) {
        return Err(PoiesisError::Message(format!(
            "That address is already connected as “{}”.",
            dupe.label
        )));
    }
    db.update_local_endpoint(&id, label, &base_url, ctx_size)
        .map_err(|e| PoiesisError::Message(e.to_string()))?;
    if let Some(key) = api_key.as_deref().map(str::trim) {
        if key.is_empty() {
            endpoints::clear_key(&id).map_err(|e| PoiesisError::Message(e.to_string()))?;
        } else {
            endpoints::set_key(&id, key).map_err(|e| PoiesisError::Message(e.to_string()))?;
        }
    }
    Ok(())
}

/// Turn a connected server on/off without forgetting it (`Off` in the row
/// still keeps the address and any key, just stops it appearing in the
/// picker).
#[tauri::command]
pub fn set_endpoint_enabled_cmd(db: State<'_, Db>, id: String, enabled: bool) -> Cmd<()> {
    db.set_local_endpoint_enabled(&id, enabled)
        .map_err(|e| PoiesisError::Message(e.to_string()))
}

/// Forget a connected server and its stored key.
#[tauri::command]
pub fn delete_endpoint_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    db.delete_local_endpoint(&id).map_err(|e| PoiesisError::Message(e.to_string()))?;
    // The row is the source of truth for whether the endpoint exists, and it
    // is already gone. A credential-store hiccup here leaves an orphaned
    // secret nothing can reach — not worth reporting the removal as failed.
    let _ = endpoints::clear_key(&id);
    Ok(())
}

/// Check that a server is reachable. Takes a raw address rather than only an
/// endpoint id so the Settings form can verify while the user is still
/// filling it in.
///
/// `endpoint_id` names an already-saved endpoint whose stored key should be
/// used. Without it, re-testing a saved endpoint that needs a key would probe
/// unauthenticated and report a 401 for a server that actually works — the
/// key never leaves the credential store, so the frontend can't pass it back.
/// An explicit `api_key` still wins, so the add form can test a key the user
/// has typed but not yet saved.
#[tauri::command]
pub async fn test_endpoint_cmd(
    mgr: State<'_, RuntimeManager>,
    base_url: String,
    api_key: Option<String>,
    endpoint_id: Option<String>,
) -> Cmd<EndpointProbe> {
    let base_url = endpoints::normalize_base_url(&base_url).map_err(PoiesisError::Message)?;
    let typed = api_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_string);
    let key = match typed {
        Some(k) => Some(k),
        None => endpoint_id.as_deref().and_then(endpoints::get_key),
    };
    Ok(endpoints::probe(&mgr.client, &base_url, key.as_deref()).await)
}

/// Discover models across every enabled endpoint. Best-effort, like
/// `list_cloud_models_cmd`: an endpoint that's unreachable right now is
/// skipped, not fatal — a sleeping Ollama box shouldn't block the picker.
/// Endpoints are probed concurrently so N unreachable servers cost one
/// timeout, not N.
#[tauri::command]
pub async fn list_endpoint_models_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
) -> Cmd<Vec<EndpointModel>> {
    let rows = db.list_local_endpoints().map_err(|e| PoiesisError::Message(e.to_string()))?;
    let client = &mgr.client;
    let results = join_all(
        rows.into_iter()
            .filter(|ep| ep.enabled)
            .map(|ep| async move { endpoints::discover(client, &ep).await }),
    )
    .await;
    Ok(results.into_iter().filter_map(Result::ok).flatten().collect())
}
