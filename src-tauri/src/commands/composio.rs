//! `CAP`: the Apps page's side of Composio. Save and remove the key, list and
//! connect accounts, browse the catalog, narrow a persona to some apps, count
//! what ran. Nothing here returns the key, the user id, the session id or a
//! sign-in address: the page learns that something is connected, not how.

use serde::Serialize;
use tauri::State;

use crate::agent::app_calls;
use crate::commands::connectors::{policy_view, PolicyView, ToolView};
use crate::connectors::composio::{self, Api, ToolkitInfo};
use crate::db::{Db, PersonaAppGrant};
use crate::mcp::McpTool;
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

fn key() -> Cmd<String> {
    composio::read_key().ok_or_else(|| PoiesisError::Message("Add your Composio key first.".into()))
}

/// One sign-in, as the page shows it.
#[derive(Debug, Serialize)]
pub struct AccountView {
    pub id: String,
    pub app: String,
    pub app_name: String,
    pub label: Option<String>,
    pub status: String,
}

fn account_view(a: crate::db::AppAccount) -> AccountView {
    AccountView {
        app_name: composio::app_label(&a.app),
        id: a.id,
        app: a.app,
        label: a.label,
        status: a.status,
    }
}

/// What one connected app may do without asking.
#[derive(Debug, Serialize)]
pub struct AppPolicy {
    pub app: String,
    pub policy: PolicyView,
}

#[derive(Debug, Serialize)]
pub struct ComposioStatus {
    /// A key is saved in the keychain.
    pub configured: bool,
    /// Which kind of key it is (`CAP-C1`): `connect` for `ck_`, `project` for
    /// `ak_`. `None` without a key. Never the key.
    pub mode: Option<composio::Mode>,
    pub connector_id: Option<String>,
    pub enabled: bool,
    pub tool_count: usize,
    pub checked_at: Option<i64>,
    pub last_error: Option<String>,
    pub accounts: Vec<AccountView>,
    /// One per app that has an account, for "What I may do".
    pub policies: Vec<AppPolicy>,
}

fn status_of(db: &Db) -> Cmd<ComposioStatus> {
    let configured = composio::has_key();
    let mode = composio::current_mode();
    let Some(row) = db.composio_connector().map_err(err)? else {
        return Ok(ComposioStatus {
            configured,
            mode,
            connector_id: None,
            enabled: false,
            tool_count: 0,
            checked_at: None,
            last_error: None,
            accounts: vec![],
            policies: vec![],
        });
    };
    let health = composio::health_of(&row);
    let held = db.list_app_accounts(&row.id).map_err(err)?;
    let mut apps: Vec<&str> = held.iter().map(|a| a.app.as_str()).collect();
    apps.dedup();
    let policies = apps
        .into_iter()
        .map(|app| AppPolicy { app: app.to_string(), policy: policy_view(db, &row.id, app) })
        .collect();
    let accounts = held.into_iter().map(account_view).collect();
    Ok(ComposioStatus {
        configured,
        mode,
        enabled: row.enabled,
        connector_id: Some(row.id),
        tool_count: health.tool_count,
        checked_at: health.checked_at,
        last_error: health.last_error,
        accounts,
        policies,
    })
}

/// What the Apps page needs to draw the Composio card, with no network.
#[tauri::command]
pub fn composio_status_cmd(db: State<'_, Db>) -> Cmd<ComposioStatus> {
    status_of(&db)
}

/// Check the key by making a session with it, then keep it. A key Composio
/// refuses is not stored.
#[tauri::command]
pub async fn composio_save_key_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    key: String,
) -> Cmd<ComposioStatus> {
    let key = composio::clean_key(&key);
    if key.is_empty() {
        return Err(PoiesisError::Message("Paste your Composio key first.".into()));
    }
    composio::validate_key(&db, &mgr.client, &key).await.map_err(err)?;
    composio::store_key(&key).map_err(err)?;
    composio::ensure_row(&db).map_err(err)?;
    // A key of the other kind starts the list of apps afresh.
    composio::note_key_kind(&db, composio::mode_of(&key)).map_err(err)?;
    composio::set_last_error(&db, None);
    // The key is good and saved. Whatever fails from here is shown as a note,
    // and a refresh puts it right.
    if let Err(e) = composio::refresh_tools(&db, &mgr.client).await {
        composio::set_last_error(&db, Some(&e));
    }
    let _ = composio::refresh_accounts(&db, &mgr.client).await;
    let _ = db.log_activity(None, "mcp", "Connected Composio");
    status_of(&db)
}

/// Forget the key. The connector row and the sign-ins stay, so saving the key
/// again finds them.
#[tauri::command]
pub fn composio_remove_key_cmd(db: State<'_, Db>) -> Cmd<ComposioStatus> {
    composio::remove_key().map_err(err)?;
    let _ = db.log_activity(None, "mcp", "Removed the Composio key");
    status_of(&db)
}

/// Ask Composio again for the tools and the accounts.
#[tauri::command]
pub async fn composio_refresh_cmd(db: State<'_, Db>, mgr: State<'_, RuntimeManager>) -> Cmd<ComposioStatus> {
    key()?;
    match composio::refresh_tools(&db, &mgr.client).await {
        Ok(_) => composio::set_last_error(&db, None),
        Err(e) => composio::set_last_error(&db, Some(&e)),
    }
    let _ = composio::refresh_accounts(&db, &mgr.client).await;
    status_of(&db)
}

/// The signed-in accounts. `refresh` asks Composio first; the connect card polls
/// with it while it waits for a sign-in to finish. `apps` narrows the question to
/// those apps in Connect mode, where Composio has no account list to read.
#[tauri::command]
pub async fn composio_accounts_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    refresh: bool,
    apps: Option<Vec<String>>,
) -> Cmd<Vec<AccountView>> {
    if refresh && composio::has_key() {
        composio::check_accounts(&db, &mgr.client, apps.as_deref(), true).await.map_err(err)?;
    }
    Ok(status_of(&db)?.accounts)
}

/// Open a sign-in address in the person's browser. It is opened here and never
/// sent to the page.
fn open_sign_in(app_handle: &tauri::AppHandle, url: String) -> Cmd<()> {
    use tauri_plugin_opener::OpenerExt;
    if !composio::safe_to_open(&url) {
        return Err(PoiesisError::Message(composio::ApiError::Untrusted.plain()));
    }
    app_handle
        .opener()
        .open_url(url, None::<&str>)
        .map_err(|_| PoiesisError::Message("I couldn't open your browser. Try Connect again.".into()))
}

/// Start a sign-in and open it in the person's browser. An app that turns out to
/// be connected already opens nothing.
#[tauri::command]
pub async fn composio_connect_cmd(
    app_handle: tauri::AppHandle,
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    app: String,
    label: Option<String>,
) -> Cmd<AccountView> {
    key()?;
    let started = composio::connect_any(&db, &mgr.client, app.trim(), label.as_deref()).await.map_err(err)?;
    if let Some(url) = started.redirect_url {
        open_sign_in(&app_handle, url)?;
        let _ = db.log_activity(None, "mcp", &format!("Started a {} sign-in", composio::app_label(&started.account.app)));
    }
    Ok(account_view(started.account))
}

/// Make a sign-in that stopped working again, and open it.
#[tauri::command]
pub async fn composio_reconnect_cmd(
    app_handle: tauri::AppHandle,
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    account_id: String,
) -> Cmd<AccountView> {
    key()?;
    let started = composio::reconnect_any(&db, &mgr.client, &account_id).await.map_err(err)?;
    if let Some(url) = started.redirect_url {
        open_sign_in(&app_handle, url)?;
    }
    Ok(account_view(started.account))
}

/// The person gave up on a sign-in that was open.
#[tauri::command]
pub async fn composio_cancel_cmd(db: State<'_, Db>, mgr: State<'_, RuntimeManager>, account_id: String) -> Cmd<()> {
    key()?;
    composio::cancel_any(&db, &mgr.client, &account_id).await.map_err(err)
}

/// Disconnect: revoke one sign-in at the app (Project), or remove every account
/// Composio holds for the app (Connect), then stop listing it here.
#[tauri::command]
pub async fn composio_disconnect_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    account_id: String,
) -> Cmd<Vec<AccountView>> {
    key()?;
    composio::disconnect_any(&db, &mgr.client, &account_id).await.map_err(err)?;
    Ok(status_of(&db)?.accounts)
}

/// `CAP-C6`: give one account a new name, at Composio and here (Connect keys).
#[tauri::command]
pub async fn composio_rename_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    account_id: String,
    label: String,
) -> Cmd<Vec<AccountView>> {
    key()?;
    composio::rename_any(&db, &mgr.client, &account_id, &label).await.map_err(err)?;
    Ok(status_of(&db)?.accounts)
}

#[derive(Debug, Serialize)]
pub struct CatalogView {
    pub apps: Vec<ToolkitInfo>,
    /// Composio has more apps than it sent; search to reach the rest.
    pub partial: bool,
    /// `composio` when the list came from Composio, `bundled` when it is the
    /// list that ships with the app.
    pub source: &'static str,
    /// Why the bundled list is shown, when it is not just for lack of a key.
    pub note: Option<String>,
    /// How many apps there are in all, before the search.
    pub total: usize,
}

fn matches(app: &ToolkitInfo, q: &str) -> bool {
    q.is_empty()
        || app.slug.contains(q)
        || app.name.to_lowercase().contains(q)
        || app.description.to_lowercase().contains(q)
        || app.categories.iter().any(|c| c.to_lowercase().contains(q))
}

/// Logos ship with the app: the webview loads no remote image. An app Composio
/// lists that the bundle has no mark for shows a monogram.
fn with_local_logos(mut apps: Vec<ToolkitInfo>) -> Vec<ToolkitInfo> {
    for a in &mut apps {
        a.logo = composio::bundled().iter().find(|b| b.slug == a.slug).and_then(|b| b.logo.clone());
    }
    apps
}

/// `CAP-8`: the apps to browse. Composio's own list when a developer key works,
/// else the list that ships with the app. Either way read-only.
#[tauri::command]
pub async fn composio_catalog_cmd(mgr: State<'_, RuntimeManager>, query: Option<String>) -> Cmd<CatalogView> {
    let q = query.unwrap_or_default().trim().to_lowercase();
    let all = composio::bundled();
    let bundled = |note: Option<String>| CatalogView {
        apps: all.iter().filter(|a| matches(a, &q)).cloned().collect(),
        partial: false,
        source: "bundled",
        note,
        total: all.len(),
    };
    let Some(key) = composio::read_key() else { return Ok(bundled(None)) };
    // `CAP-C6`: a Connect key cannot list toolkits, so the bundled list is the list.
    if composio::mode_of(&key) == composio::Mode::Connect {
        return Ok(bundled(None));
    }
    let api = Api::new(&mgr.client, &key);
    match composio::catalog(&api).await {
        Ok(live) => {
            let partial = live.len() >= composio::CATALOG_PAGE;
            let total = if partial { all.len() } else { live.len() };
            let mut apps: Vec<ToolkitInfo> = live.into_iter().filter(|a| matches(a, &q)).collect();
            // A long catalog is cut off: ask Composio for what the person typed.
            if partial && !q.is_empty() {
                if let Ok(found) = api.toolkits(Some(&q)).await {
                    if !found.is_empty() {
                        apps = found;
                    }
                }
            }
            Ok(CatalogView { apps: with_local_logos(apps), partial, source: "composio", note: None, total })
        }
        Err(e) => Ok(bundled(Some(e))),
    }
}

/// `CAP-10`: the apps a persona may use. No rows means no narrowing.
#[tauri::command]
pub fn get_persona_app_grants_cmd(db: State<'_, Db>, persona_id: String) -> Cmd<Vec<PersonaAppGrant>> {
    db.persona_app_grants(&persona_id).map_err(err)
}

/// Replace a persona's grants. Each line names the Composio connector, an app
/// (or `''` for the explicit "none"), and `*` or a JSON list of tool slugs.
#[tauri::command]
pub fn set_persona_app_grants_cmd(db: State<'_, Db>, persona_id: String, grants: Vec<PersonaAppGrant>) -> Cmd<()> {
    let row = db
        .composio_connector()
        .map_err(err)?
        .ok_or_else(|| PoiesisError::Message("Add your Composio key first.".into()))?;
    for g in &grants {
        if g.connector_id != row.id {
            return Err(PoiesisError::Message("That isn't the Composio connector.".into()));
        }
        if !g.app.is_empty() && !composio::valid_slug(&g.app) {
            return Err(PoiesisError::Message("I don't know that app.".into()));
        }
        let tools_ok = g.tools == "*"
            || serde_json::from_str::<Vec<String>>(&g.tools).is_ok_and(|l| l.iter().all(|t| !t.trim().is_empty()));
        if !tools_ok {
            return Err(PoiesisError::Message("Tools are * or a list of tool names.".into()));
        }
        // The empty app is the "no apps" line, and it carries no tools.
        if g.app.is_empty() && g.tools != "[]" {
            return Err(PoiesisError::Message("The no-apps line carries no tools.".into()));
        }
    }
    db.set_persona_app_grants(&persona_id, &grants).map_err(err)
}

/// `MCG-7` for Composio: the tools of one app I can name (the ones that ran, and
/// the ones given a kind from the approval panel), each with what the gate holds
/// it to, so a wrong guess can be corrected from the Apps page.
#[tauri::command]
pub fn composio_app_tools_cmd(db: State<'_, Db>, app: String) -> Cmd<Vec<ToolView>> {
    if !composio::valid_slug(&app) {
        return Err(PoiesisError::Message("I don't know that app.".into()));
    }
    let Some(row) = db.composio_connector().map_err(err)? else { return Ok(vec![]) };
    Ok(app_calls::known_tools(&db, &row.id, &app)
        .into_iter()
        .map(|t| ToolView {
            tool: McpTool {
                description: match t.calls {
                    0 => "Not used yet; you set this from an approval.".to_string(),
                    1 => "Used once.".to_string(),
                    n => format!("Used {n} times."),
                },
                name: t.slug,
                input_schema: serde_json::Value::Null,
                annotations: None,
            },
            kind: t.kind.as_str().to_string(),
            auto_kind: t.auto.as_str().to_string(),
            overridden: t.kind != t.auto,
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct UsageRow {
    pub app: String,
    pub app_name: String,
    pub calls: i64,
}

/// `CAP-12`: calls that ran per app in the last `days` days.
#[tauri::command]
pub fn composio_usage_cmd(db: State<'_, Db>, days: i64) -> Cmd<Vec<UsageRow>> {
    let days = days.clamp(1, 365);
    let since = crate::db::now_ms() - days * 86_400_000;
    Ok(db
        .app_calls_since(since)
        .map_err(err)?
        .into_iter()
        .map(|(app, calls)| UsageRow { app_name: composio::app_label(&app), app, calls })
        .collect())
}

/// One call for the recorder to make and keep.
#[derive(Debug, serde::Deserialize)]
pub struct RecordStep {
    /// The file name, without extension: `manage_connections_gmail`.
    pub name: String,
    pub tool: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

/// `CAP-C9`: record what Composio Connect really answers, as fixtures to test the
/// parsers against. Debug builds only, and only with a Connect key saved. It
/// lists the tools and makes the calls it is given; each answer is scrubbed of
/// the key, sign-in addresses, ids and email addresses before it is written to
/// `fixtures/composio/connect/`. The key is read here and goes nowhere but the
/// request. Run it from the webview console, then read the files before they are
/// committed.
#[tauri::command]
pub async fn composio_record_cmd(mgr: State<'_, RuntimeManager>, steps: Vec<RecordStep>) -> Cmd<Vec<String>> {
    record(&mgr.client, steps).await
}

#[cfg(not(debug_assertions))]
async fn record(_http: &reqwest::Client, _steps: Vec<RecordStep>) -> Cmd<Vec<String>> {
    Err(PoiesisError::Message("Recording is only for debug builds.".into()))
}

#[cfg(debug_assertions)]
async fn record(http: &reqwest::Client, steps: Vec<RecordStep>) -> Cmd<Vec<String>> {
    let key = key()?;
    if composio::mode_of(&key) != composio::Mode::Connect {
        return Err(PoiesisError::Message("Recording needs a Connect key (ck_).".into()));
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("fixtures").join("composio").join("connect");
    std::fs::create_dir_all(&dir).map_err(err)?;
    let keep = |name: &str, body: &serde_json::Value| -> Cmd<String> {
        let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
        let text = composio::scrub_for_fixture(&serde_json::to_string_pretty(body).map_err(err)?, &key);
        std::fs::write(dir.join(format!("{safe}.json")), text).map_err(err)?;
        Ok(safe)
    };
    let mut client = composio::connect_client(http, &key);
    let tools = client.discover().await.map_err(|e| PoiesisError::Message(composio::connect_error(&e)))?;
    let mut written = vec![keep("tools_list", &serde_json::to_value(&tools).map_err(err)?)?];
    for step in steps {
        let answer = match client.call_tool(&step.tool, step.arguments.clone()).await {
            Ok(text) => serde_json::json!({ "text": text }),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        };
        let body = serde_json::json!({ "tool": step.tool, "arguments": step.arguments, "answer": answer });
        written.push(keep(&step.name, &body)?);
    }
    Ok(written)
}
