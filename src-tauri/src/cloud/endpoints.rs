//! A user's own OpenAI-compatible model server — Ollama, LM Studio, or a
//! remote box running one of them. This is a third model source alongside
//! the integrated runtime and BYOK cloud providers, but deliberately kept
//! separate from [`crate::cloud::Provider`]: that enum is closed and
//! `&'static str`-driven (base URLs, console links, key hints baked in at
//! compile time), which fits a fixed roster of hosted providers but not a
//! user-named, user-addressed, possibly-keyless server. Endpoints are
//! persisted in the `local_endpoints` table (metadata) and the OS credential
//! store (API key, if the server even needs one).
//!
//! Discovery and streaming both reuse the existing OpenAI-compatible path —
//! `GET {base_url}/v1/models` here, and [`crate::runtime::proxy::stream_turn`]
//! (via [`crate::cloud::ChatEndpoint::OpenAi`]) for the turn itself. No new
//! wire protocol is introduced.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cloud::ChatEndpoint;
use crate::db::LocalEndpointRow;
use crate::secrets::{self, SERVICE_ENDPOINT};

/// Endpoint status for the Settings surface. `key_set` comes from the
/// keyring, not the DB — there is no such column.
#[derive(Debug, Serialize)]
pub struct EndpointInfo {
    pub id: String,
    pub label: String,
    pub base_url: String,
    pub ctx_size: i64,
    pub enabled: bool,
    pub key_set: bool,
}

impl EndpointInfo {
    pub fn from_row(row: &LocalEndpointRow) -> EndpointInfo {
        EndpointInfo {
            id: row.id.clone(),
            label: row.label.clone(),
            base_url: row.base_url.clone(),
            ctx_size: row.ctx_size,
            enabled: row.enabled,
            key_set: has_key(&row.id),
        }
    }
}

/// A model offered by a connected endpoint. Deliberately not a `CloudModel` —
/// a `CloudModel` lands in the picker's "Cloud · your key" group; this one
/// gets its own "Your own servers" group, because it never leaves the
/// machine the way a hosted provider does.
#[derive(Debug, Clone, Serialize)]
pub struct EndpointModel {
    /// `endpoint:<endpoint_id>:<model>`. Opaque — nothing splits this string;
    /// `endpoint_id` and `model` travel separately in `ChatTarget`.
    pub id: String,
    pub endpoint_id: String,
    pub endpoint_label: String,
    pub name: String,
    pub model: String,
    pub vision: bool,
    pub tools: bool,
    pub ctx_size: i64,
}

/// Result of a connectivity check, shown inline in Settings.
#[derive(Debug, Serialize)]
pub struct EndpointProbe {
    pub ok: bool,
    /// How many models the server is serving. `ok` with zero is a real state,
    /// not a contradiction — LM Studio answers `/v1/models` as soon as its
    /// server is running, before any model is loaded into it.
    pub model_count: usize,
    pub error: Option<String>,
    /// Set when the given URL failed but a `127.0.0.1` rewrite of the same
    /// port succeeded (the Windows `localhost` → `::1` gotcha, see [`probe`]).
    /// The caller should store this instead of what the user typed.
    pub resolved_base_url: Option<String>,
}

// ---- key management (keyring-backed; most endpoints need no key at all) ----

pub fn set_key(id: &str, key: &str) -> Result<(), secrets::SecretError> {
    secrets::set_secret(SERVICE_ENDPOINT, id, key)
}

pub fn clear_key(id: &str) -> Result<(), secrets::SecretError> {
    secrets::delete_secret(SERVICE_ENDPOINT, id)
}

pub fn get_key(id: &str) -> Option<String> {
    secrets::get_secret(SERVICE_ENDPOINT, id).ok().flatten()
}

pub fn has_key(id: &str) -> bool {
    secrets::has_secret(SERVICE_ENDPOINT, id)
}

// ---- base URL normalization ----

/// Users paste whatever their server's docs show them — with or without a
/// scheme, with a trailing slash, sometimes with `/v1` already on it (LM
/// Studio's own UI displays the URL that way). Normalize down to a bare
/// `scheme://host[:port]` so every call site can append `/v1/...` itself.
pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let mut s = raw.trim().to_string();
    if s.is_empty() {
        return Err("The address can't be empty.".into());
    }
    if !s.contains("://") {
        s = format!("http://{s}");
    }
    let parsed = reqwest::Url::parse(&s).map_err(|_| "That doesn't look like a valid address.".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("The address must start with http:// or https://.".into());
    }
    let mut out = s.trim_end_matches('/').to_string();
    if let Some(stripped) = out.strip_suffix("/v1") {
        out = stripped.to_string();
    }
    Ok(out)
}

// ---- connectivity + discovery ----

#[derive(Deserialize)]
struct OpenAiModelList {
    data: Vec<OpenAiModel>,
}
#[derive(Deserialize)]
struct OpenAiModel {
    id: String,
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

/// Substrings that suggest a model accepts image input. `/v1/models` on these
/// servers reports no capabilities at all, so this is a heuristic: a false
/// negative just hides the image affordance, a false positive is caught by
/// the server's own error on the first image sent.
const VISION_HINTS: [&str; 6] = ["llava", "vision", "-vl", "vl-", "minicpm-v", "moondream"];

fn guess_vision(model_id: &str) -> bool {
    let lower = model_id.to_lowercase();
    VISION_HINTS.iter().any(|h| lower.contains(h)) || lower.contains("gemma3") || lower.contains("gemma-3")
}

async fn fetch_models(
    client: &reqwest::Client,
    base_url: &str,
    key: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut req = client
        .get(format!("{base_url}/v1/models"))
        .timeout(PROBE_TIMEOUT);
    if let Some(k) = key {
        req = req.bearer_auth(k);
    }
    let list: OpenAiModelList = req
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(list.data.into_iter().map(|m| m.id).collect())
}

/// Check that a server is reachable and OpenAI-compatible. Never returns
/// `Err` — a failure is reported inline via `EndpointProbe.error` so the
/// Settings form can show it next to the field the user just filled in.
///
/// On Windows, `localhost` can resolve to `::1` while Ollama/LM Studio only
/// bind `127.0.0.1`, producing a bare connection-refused for a URL that looks
/// correct. If the given URL uses the `localhost` host and fails, retry once
/// against `127.0.0.1` on the same port; a `resolved_base_url` on success
/// tells the caller to persist the address that actually works.
pub async fn probe(client: &reqwest::Client, base_url: &str, key: Option<&str>) -> EndpointProbe {
    match fetch_models(client, base_url, key).await {
        Ok(models) => EndpointProbe {
            ok: true,
            model_count: models.len(),
            error: None,
            resolved_base_url: None,
        },
        Err(first_err) => {
            if let Some(rewritten) = rewrite_localhost(base_url) {
                if let Ok(models) = fetch_models(client, &rewritten, key).await {
                    return EndpointProbe {
                        ok: true,
                        model_count: models.len(),
                        error: None,
                        resolved_base_url: Some(rewritten),
                    };
                }
            }
            EndpointProbe {
                ok: false,
                model_count: 0,
                error: Some(first_err),
                resolved_base_url: None,
            }
        }
    }
}

fn rewrite_localhost(base_url: &str) -> Option<String> {
    let url = reqwest::Url::parse(base_url).ok()?;
    if url.host_str()? != "localhost" {
        return None;
    }
    let mut rewritten = url.clone();
    rewritten.set_host(Some("127.0.0.1")).ok()?;
    Some(rewritten.as_str().trim_end_matches('/').to_string())
}

/// List the chat models an endpoint offers, unfiltered — unlike
/// `cloud::discover_openai`'s `gpt-*`/`o1*`/`o3*` filter, which would drop
/// every Ollama or LM Studio model id.
pub async fn discover(client: &reqwest::Client, ep: &LocalEndpointRow) -> Result<Vec<EndpointModel>, String> {
    let key = get_key(&ep.id);
    let ids = fetch_models(client, &ep.base_url, key.as_deref()).await?;
    let mut models: Vec<EndpointModel> = ids
        .into_iter()
        .map(|id| EndpointModel {
            id: format!("endpoint:{}:{}", ep.id, id),
            endpoint_id: ep.id.clone(),
            endpoint_label: ep.label.clone(),
            name: id.clone(),
            vision: guess_vision(&id),
            // Many small local models can't take a `tools` array at all, but
            // reporting `false` here would withdraw tools from them
            // permanently. `cloud::drive_turn` already retries once without
            // `tools` on refusal, and the agent loop falls back to parsing
            // tool calls a model emits as prose JSON — both nets are already
            // in place, so the more useful default is to offer tools and let
            // those nets catch the model that can't use them.
            tools: true,
            ctx_size: ep.ctx_size,
            model: id,
        })
        .collect();
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

/// Build the turn endpoint for a model served by this connection.
pub fn chat_endpoint(ep: &LocalEndpointRow, model: String) -> ChatEndpoint {
    ChatEndpoint::OpenAi {
        base_url: ep.base_url.clone(),
        api_key: get_key(&ep.id),
        model: Some(model),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_what_users_actually_paste() {
        // Bare host:port — what someone reads off the Ollama tray icon.
        assert_eq!(normalize_base_url("localhost:11434").unwrap(), "http://localhost:11434");
        // Trailing slash, and the `/v1` LM Studio's own UI displays.
        assert_eq!(normalize_base_url("http://localhost:1234/v1").unwrap(), "http://localhost:1234");
        assert_eq!(normalize_base_url("http://localhost:1234/v1/").unwrap(), "http://localhost:1234");
        assert_eq!(normalize_base_url("http://localhost:11434/").unwrap(), "http://localhost:11434");
        // Surrounding whitespace from a copy-paste.
        assert_eq!(normalize_base_url("  http://box.lan:8080  ").unwrap(), "http://box.lan:8080");
        // A server behind a path prefix keeps the prefix — `/v1` is appended
        // to whatever survives, so stripping the whole path would break it.
        assert_eq!(normalize_base_url("https://host/openai/v1").unwrap(), "https://host/openai");
        assert!(normalize_base_url("https://host:8443").is_ok());
    }

    #[test]
    fn rejects_what_cannot_be_called() {
        assert!(normalize_base_url("").is_err());
        assert!(normalize_base_url("   ").is_err());
        // A non-HTTP scheme would fail later, deep in reqwest, with a much
        // worse message than the one the form can show now.
        assert!(normalize_base_url("ftp://host").is_err());
        assert!(normalize_base_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn rewrites_localhost_to_loopback_preserving_port() {
        // The Windows `localhost`→`::1` case: same port, IPv4 literal.
        assert_eq!(
            rewrite_localhost("http://localhost:11434").as_deref(),
            Some("http://127.0.0.1:11434")
        );
        // Anything not named `localhost` is left alone — there is nothing to
        // second-guess about an address the user gave explicitly.
        assert_eq!(rewrite_localhost("http://127.0.0.1:11434"), None);
        assert_eq!(rewrite_localhost("http://box.lan:1234"), None);
    }

    #[test]
    fn vision_guess_tracks_model_naming() {
        assert!(guess_vision("llava:13b"));
        assert!(guess_vision("qwen2.5-vl-7b"));
        assert!(guess_vision("llama3.2-vision:11b"));
        assert!(guess_vision("moondream"));
        assert!(!guess_vision("llama3.2:3b"));
        assert!(!guess_vision("qwen2.5-coder:7b"));
    }
}
