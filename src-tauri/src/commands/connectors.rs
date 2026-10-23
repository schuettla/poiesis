//! MCP connector commands (Phase 6, MCP-1/3/4): add, list, enable/disable, test,
//! and remove remote MCP servers. Auth tokens live in the OS credential store
//! (never SQLite). Discovered tools are cached in the connector's `config_json`.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::agent::connector_gate;
use crate::connectors::composio;
use crate::db::{Connector, ConnectorPolicy, Db};
use crate::mcp::{McpClient, McpPrompt, McpTool};
use crate::runtime::RuntimeManager;
use crate::secrets::{self, SERVICE_MCP};
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// Cached discovery result stored in `connectors.config_json`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CachedConfig {
    pub tools: Vec<McpTool>,
    /// What the server offers as prompts, which Poiesis shows as commands. A
    /// connector cached before this existed has none until it is tested again.
    #[serde(default)]
    pub prompts: Vec<McpPrompt>,
    pub checked_at: Option<i64>,
}

/// A connector as shown in the Apps dashboard.
#[derive(Debug, Serialize)]
pub struct ConnectorView {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub transport: String,
    pub enabled: bool,
    pub has_auth: bool,
    pub tools: Vec<ToolView>,
    pub created_at: i64,
    /// `MCG-UI-2`: what its tools may do without asking.
    pub policy: PolicyView,
}

/// A tool plus what the gate holds it to (`MCG-UI-2`), so the chip can show
/// R / C / S and the person can see when a guess was corrected.
#[derive(Debug, Serialize)]
pub struct ToolView {
    #[serde(flatten)]
    pub tool: McpTool,
    /// What the gate holds it to now: the person's override, else `auto_kind`.
    pub kind: String,
    /// What the name and the server's hint say, before any override.
    pub auto_kind: String,
    pub overridden: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyView {
    pub read_policy: String,
    pub change_policy: String,
    pub send_policy: String,
    pub send_cap: i64,
    /// Sends that ran today, against `send_cap`.
    pub sent_today: i64,
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn cached_tools(c: &Connector) -> Vec<McpTool> {
    c.config_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<CachedConfig>(s).ok())
        .map(|cc| cc.tools)
        .unwrap_or_default()
}

/// What one connector (or one app of Composio) may do without asking, and how
/// many sends it has used today.
pub fn policy_view(db: &Db, connector_id: &str, app: &str) -> PolicyView {
    let policy = db
        .get_connector_policy(connector_id, app)
        .unwrap_or_else(|_| ConnectorPolicy::defaults(connector_id, app));
    let sent_today = db
        .app_send_count(connector_id, app, &connector_gate::send_day())
        .unwrap_or(0);
    PolicyView {
        read_policy: policy.read_policy,
        change_policy: policy.change_policy,
        send_policy: policy.send_policy,
        send_cap: policy.send_cap,
        sent_today,
    }
}

fn to_view(db: &Db, c: Connector) -> ConnectorView {
    let policy = db
        .get_connector_policy(&c.id, "")
        .unwrap_or_else(|_| ConnectorPolicy::defaults(&c.id, ""));
    let overrides = connector_gate::overrides_of(&policy);
    let tools = cached_tools(&c)
        .into_iter()
        .map(|tool| {
            let auto = connector_gate::classify(&tool.name, &connector_gate::hints_of(&tool));
            let kind = overrides.get(&tool.name).copied().unwrap_or(auto);
            ToolView {
                tool,
                kind: kind.as_str().to_string(),
                auto_kind: auto.as_str().to_string(),
                overridden: kind != auto,
            }
        })
        .collect();
    let has_auth = secrets::has_secret(SERVICE_MCP, &c.id);
    ConnectorView {
        policy: policy_view(db, &c.id, ""),
        id: c.id,
        name: c.name,
        url: c.url,
        transport: c.transport,
        enabled: c.enabled,
        has_auth,
        tools,
        created_at: c.created_at,
    }
}

/// Add an MCP connector: probe it live, cache its tools, persist it, and stash
/// any auth token in the OS credential store (MCP-1, MCP-3, §5.4.3).
#[tauri::command]
pub async fn add_connector_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    name: String,
    url: String,
    token: Option<String>,
    transport: Option<String>,
) -> Cmd<ConnectorView> {
    let token = token.filter(|t| !t.trim().is_empty());
    // "stdio": `url` carries the server command line (MCP-2). Default HTTP.
    let transport = transport.filter(|t| t == "stdio").unwrap_or_else(|| "http".into());

    // Probe before saving so the user gets immediate, actionable feedback.
    let mut client = if transport == "stdio" {
        McpClient::new_stdio(url.clone())
    } else {
        McpClient::new(mgr.client.clone(), url.clone(), token.clone())
    };
    let tools = client.discover().await.map_err(err)?;
    let prompts = client.list_prompts().await;

    let config = CachedConfig {
        tools,
        prompts,
        checked_at: Some(now_ms()),
    };
    let config_json = serde_json::to_string(&config).map_err(err)?;
    let connector = db
        .add_connector(&name, &url, &transport, Some(&config_json))
        .map_err(err)?;

    // stdio servers run locally with no bearer token; only HTTP stores one.
    if transport != "stdio" {
        if let Some(token) = token {
            secrets::set_secret(SERVICE_MCP, &connector.id, &token).map_err(err)?;
        }
    }
    let _ = db.log_activity(None, "mcp", &format!("Connected {name}"));

    Ok(to_view(&db, connector))
}

/// List configured connectors with their cached tools (MCP-3).
#[tauri::command]
pub fn list_connectors_cmd(db: State<'_, Db>) -> Cmd<Vec<ConnectorView>> {
    Ok(db
        .list_connectors()
        .map_err(err)?
        .into_iter()
        .map(|c| to_view(&db, c))
        .collect())
}

/// `MCG-UI-2`: what a connector's tools may do without asking. The cap is held
/// to 1-1000 by the database. The tool overrides are left as they were. `app` is
/// a Composio app's slug when the policy is one app's; empty for the connector.
#[tauri::command]
pub fn set_connector_policy_cmd(
    db: State<'_, Db>,
    id: String,
    app: Option<String>,
    read_policy: String,
    change_policy: String,
    send_policy: String,
    send_cap: i64,
) -> Cmd<()> {
    let app = app.unwrap_or_default();
    let mut policy = db.get_connector_policy(&id, &app).map_err(err)?;
    policy.read_policy = read_policy;
    policy.change_policy = change_policy;
    policy.send_policy = send_policy;
    policy.send_cap = send_cap;
    db.save_connector_policy(&policy).map_err(err)
}

/// `MCG-7`: treat one tool as read, change or send, or (`kind` empty) go back to
/// what its name says. `app` is a Composio app's slug when the tool is one of
/// that app's; its override is kept with that app's policy.
#[tauri::command]
pub fn set_tool_kind_cmd(
    db: State<'_, Db>,
    id: String,
    tool: String,
    kind: Option<String>,
    app: Option<String>,
) -> Cmd<()> {
    set_tool_kind(&db, &id, app.as_deref().unwrap_or(""), &tool, kind.as_deref())
}

pub fn set_tool_kind(db: &Db, id: &str, app: &str, tool: &str, kind: Option<&str>) -> Cmd<()> {
    if !app.is_empty() && !composio::valid_slug(app) {
        return Err(PoiesisError::Message("I don't know that app.".into()));
    }
    let tool = tool.trim().to_string();
    if tool.is_empty() || tool.len() > 200 {
        return Err(PoiesisError::Message("Name the tool.".into()));
    }
    let mut policy = db.get_connector_policy(id, app).map_err(err)?;
    let mut overrides: std::collections::BTreeMap<String, String> = policy
        .tool_overrides
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    match kind.filter(|k| !k.is_empty()) {
        Some(k) => {
            let parsed = connector_gate::CallKind::parse(k)
                .ok_or_else(|| PoiesisError::Message("A tool is read, change or send.".into()))?;
            overrides.insert(tool, parsed.as_str().to_string());
        }
        None => {
            overrides.remove(&tool);
        }
    }
    policy.tool_overrides = if overrides.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&overrides).map_err(err)?)
    };
    db.save_connector_policy(&policy).map_err(err)
}

/// Result of re-probing a connector.
#[derive(Debug, Serialize)]
pub struct ConnectorStatus {
    pub ok: bool,
    pub tool_count: usize,
    pub error: Option<String>,
}

/// Re-connect to a connector, refresh its cached tools, and report status (MCP-3).
#[tauri::command]
pub async fn test_connector_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    id: String,
) -> Cmd<ConnectorStatus> {
    probe_connector(&db, &mgr.client, &id).await
}

/// The probe behind `test_connector_cmd`, also what `/checkup` asks (`CHK-1`).
pub async fn probe_connector(db: &Db, http: &reqwest::Client, id: &str) -> Cmd<ConnectorStatus> {
    let id = id.to_string();
    let connector = db
        .get_connector(&id)
        .map_err(err)?
        .ok_or_else(|| PoiesisError::Message("Connector not found.".into()))?;
    // Composio has no stored address: its session is made, or found, on demand.
    if connector.transport == composio::TRANSPORT {
        if !composio::has_key() {
            return Ok(ConnectorStatus {
                ok: false,
                tool_count: 0,
                error: Some("No Composio key is saved.".into()),
            });
        }
        return Ok(match composio::refresh_tools(db, http).await {
            Ok(tool_count) => {
                composio::set_last_error(db, None);
                // So the checkup can tell which sign-ins have expired.
                let _ = composio::refresh_accounts(db, http).await;
                ConnectorStatus { ok: true, tool_count, error: None }
            }
            Err(e) => {
                composio::set_last_error(db, Some(&e));
                ConnectorStatus { ok: false, tool_count: 0, error: Some(e) }
            }
        });
    }
    let Some(url) = connector.url.clone() else {
        return Ok(ConnectorStatus {
            ok: false,
            tool_count: 0,
            error: Some("This connector has no URL.".into()),
        });
    };
    let mut client = if connector.transport == "stdio" {
        McpClient::new_stdio(url)
    } else {
        let token = secrets::get_secret(SERVICE_MCP, &id).ok().flatten();
        McpClient::new(http.clone(), url, token)
    };
    match client.discover().await {
        Ok(tools) => {
            let count = tools.len();
            let prompts = client.list_prompts().await;
            let config = CachedConfig {
                tools,
                prompts,
                checked_at: Some(now_ms()),
            };
            if let Ok(json) = serde_json::to_string(&config) {
                let _ = db.set_connector_config(&id, &json);
            }
            Ok(ConnectorStatus {
                ok: true,
                tool_count: count,
                error: None,
            })
        }
        Err(e) => Ok(ConnectorStatus {
            ok: false,
            tool_count: 0,
            error: Some(e.to_string()),
        }),
    }
}

/// One prompt a connected server offers, for the command menu.
#[derive(Debug, Serialize)]
pub struct PromptView {
    pub connector_id: String,
    pub connector_name: String,
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub arguments: Vec<crate::mcp::McpPromptArgument>,
}

/// The prompts of every enabled connector, as last cached.
fn prompts_of(connectors: Vec<Connector>) -> Vec<PromptView> {
    let mut out = Vec::new();
    for c in connectors.into_iter().filter(|c| c.enabled) {
        let prompts = c
            .config_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<CachedConfig>(s).ok())
            .map(|cc| cc.prompts)
            .unwrap_or_default();
        for p in prompts {
            out.push(PromptView {
                connector_id: c.id.clone(),
                connector_name: c.name.clone(),
                name: p.name,
                title: p.title,
                description: p.description,
                arguments: p.arguments,
            });
        }
    }
    out
}

/// `UCM`: the prompts that connected servers offer, to list beside the skills.
/// Read from what each connector last said, so the menu never waits on a network.
#[tauri::command]
pub fn list_mcp_prompts_cmd(db: State<'_, Db>) -> Cmd<Vec<PromptView>> {
    Ok(prompts_of(db.list_connectors().map_err(err)?))
}

/// Ask a connector to build one of its prompts, with the arguments filled in, and
/// return the text to send. The server does the substitution.
#[tauri::command]
pub async fn get_mcp_prompt_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    connector_id: String,
    name: String,
    arguments: std::collections::HashMap<String, String>,
) -> Cmd<String> {
    let connector = db
        .get_connector(&connector_id)
        .map_err(err)?
        .ok_or_else(|| PoiesisError::Message("That connector is gone.".into()))?;
    if !connector.enabled {
        return Err(PoiesisError::Message(format!("{} is turned off.", connector.name)));
    }
    let Some(url) = connector.url.clone() else {
        return Err(PoiesisError::Message("This connector has no URL.".into()));
    };
    let mut client = if connector.transport == "stdio" {
        McpClient::new_stdio(url)
    } else {
        let token = secrets::get_secret(SERVICE_MCP, &connector_id).ok().flatten();
        McpClient::new(mgr.client.clone(), url, token)
    };
    client.initialize().await.map_err(err)?;
    let text = client
        .get_prompt(&name, serde_json::to_value(&arguments).map_err(err)?)
        .await
        .map_err(err)?;
    if text.trim().is_empty() {
        return Err(PoiesisError::Message(format!("{} sent back an empty prompt.", connector.name)));
    }
    Ok(text)
}

/// Enable or disable a connector (its tools join/leave the agent dispatch table).
#[tauri::command]
pub fn set_connector_enabled_cmd(db: State<'_, Db>, id: String, enabled: bool) -> Cmd<()> {
    db.set_connector_enabled(&id, enabled).map_err(err)
}

/// Remove a connector and its stored token (MCP-3).
#[tauri::command]
pub fn delete_connector_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    db.delete_connector(&id).map_err(err)?;
    let _ = secrets::delete_secret(SERVICE_MCP, &id);
    Ok(())
}

/// One connector in an import/export bundle (MCP-5). Secrets are **never**
/// included — the user re-enters any token after import.
#[derive(Debug, Serialize, Deserialize)]
pub struct ConnectorExport {
    pub name: String,
    pub url: String,
    pub transport: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConnectorBundle {
    pub version: u32,
    pub connectors: Vec<ConnectorExport>,
}

/// Export all connectors as a shareable JSON bundle, minus any secrets (MCP-5).
#[tauri::command]
pub fn export_connectors_cmd(db: State<'_, Db>) -> Cmd<String> {
    let connectors = db
        .list_connectors()
        .map_err(err)?
        .into_iter()
        .filter_map(|c| {
            c.url.map(|url| ConnectorExport {
                name: c.name,
                url,
                transport: c.transport,
            })
        })
        .collect();
    let bundle = ConnectorBundle {
        version: 1,
        connectors,
    };
    serde_json::to_string_pretty(&bundle).map_err(err)
}

/// Import a connector bundle (MCP-5). Each entry is added disabled-of-secrets:
/// tools are discovered lazily via "test", and any auth token must be re-entered.
/// Returns the number imported.
#[tauri::command]
pub fn import_connectors_cmd(db: State<'_, Db>, json: String) -> Cmd<usize> {
    let bundle: ConnectorBundle = serde_json::from_str(&json)
        .map_err(|e| PoiesisError::Message(format!("That doesn't look like a valid bundle: {e}")))?;
    let existing = db.list_connectors().map_err(err)?;
    let mut added = 0;
    for entry in bundle.connectors {
        // Skip duplicates (same url already configured).
        if existing.iter().any(|c| c.url.as_deref() == Some(entry.url.as_str())) {
            continue;
        }
        let transport = if entry.transport == "stdio" { "stdio" } else { "http" };
        if db.add_connector(&entry.name, &entry.url, transport, None).is_ok() {
            added += 1;
        }
    }
    let _ = db.log_activity(None, "mcp", &format!("Imported {added} connector(s)"));
    Ok(added)
}
