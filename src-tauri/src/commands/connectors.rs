//! MCP connector commands (Phase 6, MCP-1/3/4): add, list, enable/disable, test,
//! and remove remote MCP servers. Auth tokens live in the OS credential store
//! (never SQLite). Discovered tools are cached in the connector's `config_json`.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{Connector, Db};
use crate::mcp::{McpClient, McpTool};
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
    pub tools: Vec<McpTool>,
    pub created_at: i64,
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

fn to_view(c: Connector) -> ConnectorView {
    let tools = cached_tools(&c);
    let has_auth = secrets::has_secret(SERVICE_MCP, &c.id);
    ConnectorView {
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

    let config = CachedConfig {
        tools,
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

    Ok(to_view(connector))
}

/// List configured connectors with their cached tools (MCP-3).
#[tauri::command]
pub fn list_connectors_cmd(db: State<'_, Db>) -> Cmd<Vec<ConnectorView>> {
    Ok(db.list_connectors().map_err(err)?.into_iter().map(to_view).collect())
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
    let connector = db
        .get_connector(&id)
        .map_err(err)?
        .ok_or_else(|| PoiesisError::Message("Connector not found.".into()))?;
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
        McpClient::new(mgr.client.clone(), url, token)
    };
    match client.discover().await {
        Ok(tools) => {
            let count = tools.len();
            let config = CachedConfig {
                tools,
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
