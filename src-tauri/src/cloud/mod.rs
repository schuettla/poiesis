//! Bring-your-own-key cloud providers (PRD §7.6, CLD-*). Poiesis stays local-first;
//! cloud is opt-in and gated on a key the user supplies, stored in the OS
//! credential store (never SQLite). Two API shapes are supported:
//!
//! * **OpenAI-compatible** (OpenAI, OpenRouter, and the local llama-server) — reuses
//!   the streaming proxy in [`crate::runtime::proxy`].
//! * **Anthropic Messages API** — a dedicated adapter ([`anthropic`]).
//!
//! Routing (local vs cloud, and which adapter) is decided per turn by
//! [`ChatEndpoint`] + [`drive_turn`], so the agent loop is provider-agnostic.

pub mod anthropic;
pub mod endpoints;

use serde::{Deserialize, Serialize};

use crate::runtime::proxy::{stream_turn, CancelFlag, ProxyError, TurnOutcome};
use crate::secrets::{self, SERVICE_CLOUD};

/// A supported cloud provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    OpenAi,
    OpenRouter,
    Anthropic,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::OpenAi, Provider::OpenRouter, Provider::Anthropic];

    pub fn id(&self) -> &'static str {
        match self {
            Provider::OpenAi => "openai",
            Provider::OpenRouter => "openrouter",
            Provider::Anthropic => "anthropic",
        }
    }

    pub fn from_id(s: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.id() == s)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Provider::OpenAi => "OpenAI",
            Provider::OpenRouter => "OpenRouter",
            Provider::Anthropic => "Anthropic",
        }
    }

    /// Base URL (without the `/v1/...` path the adapters append).
    pub fn base_url(&self) -> &'static str {
        match self {
            Provider::OpenAi => "https://api.openai.com",
            Provider::OpenRouter => "https://openrouter.ai/api",
            Provider::Anthropic => "https://api.anthropic.com",
        }
    }

    pub fn uses_anthropic_api(&self) -> bool {
        matches!(self, Provider::Anthropic)
    }

    /// A short hint shown by the key-entry field (§5.4.5).
    pub fn key_hint(&self) -> &'static str {
        match self {
            Provider::OpenAi => "Starts with “sk-…”",
            Provider::OpenRouter => "Starts with “sk-or-…”",
            Provider::Anthropic => "Starts with “sk-ant-…”",
        }
    }

    /// Where to get a key.
    pub fn console_url(&self) -> &'static str {
        match self {
            Provider::OpenAi => "https://platform.openai.com/api-keys",
            Provider::OpenRouter => "https://openrouter.ai/keys",
            Provider::Anthropic => "https://console.anthropic.com/settings/keys",
        }
    }
}

/// Provider status for the Settings key-entry UI.
#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub key_set: bool,
    pub key_hint: String,
    pub console_url: String,
}

/// A cloud model offered in the unified picker (CLD-3).
#[derive(Debug, Clone, Serialize)]
pub struct CloudModel {
    /// `<provider>:<model>`, e.g. "openrouter:anthropic/claude-3.5-sonnet".
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub vision: bool,
    /// Whether the model can be given a `tools` array at all. OpenRouter answers
    /// a tool-carrying request to a model with no tool-capable endpoint with a
    /// bare `404 No endpoints found that support tool use` — so a picker that
    /// doesn't know this offers models that fail the moment the agent loop
    /// needs a tool. Reported per model by `/v1/models`; assumed true where a
    /// provider doesn't say (OpenAI's chat models and Anthropic's all do).
    pub tools: bool,
}

// ---- key management (keyring-backed) ----

pub fn set_key(provider: Provider, key: &str) -> Result<(), secrets::SecretError> {
    let out = secrets::set_secret(SERVICE_CLOUD, provider.id(), key);
    // A media backend's availability is decided by whether its key is present,
    // so the cached catalog is wrong the moment one is added or removed.
    crate::media::invalidate_model_cache();
    out
}

pub fn clear_key(provider: Provider) -> Result<(), secrets::SecretError> {
    let out = secrets::delete_secret(SERVICE_CLOUD, provider.id());
    crate::media::invalidate_model_cache();
    out
}

pub fn get_key(provider: Provider) -> Option<String> {
    secrets::get_secret(SERVICE_CLOUD, provider.id()).ok().flatten()
}

pub fn has_key(provider: Provider) -> bool {
    secrets::has_secret(SERVICE_CLOUD, provider.id())
}

pub fn provider_infos() -> Vec<ProviderInfo> {
    Provider::ALL
        .into_iter()
        .map(|p| ProviderInfo {
            id: p.id().to_string(),
            name: p.name().to_string(),
            key_set: has_key(p),
            key_hint: p.key_hint().to_string(),
            console_url: p.console_url().to_string(),
        })
        .collect()
}

// ---- model discovery (CLD-4) ----

/// Discover the chat models a provider offers for the stored key.
pub async fn discover_models(
    client: &reqwest::Client,
    provider: Provider,
) -> Result<Vec<CloudModel>, String> {
    match provider {
        Provider::OpenRouter => discover_openrouter(client).await,
        Provider::OpenAi => discover_openai(client, provider).await,
        Provider::Anthropic => Ok(curated_anthropic()),
    }
}

#[derive(Deserialize)]
struct OpenAiModelList {
    data: Vec<OpenAiModel>,
}
#[derive(Deserialize)]
struct OpenAiModel {
    id: String,
}

async fn discover_openai(
    client: &reqwest::Client,
    provider: Provider,
) -> Result<Vec<CloudModel>, String> {
    let key = get_key(provider).ok_or("No API key set for this provider.")?;
    let list: OpenAiModelList = client
        .get(format!("{}/v1/models", provider.base_url()))
        .bearer_auth(&key)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut models: Vec<CloudModel> = list
        .data
        .into_iter()
        .filter(|m| m.id.starts_with("gpt-") || m.id.starts_with("o1") || m.id.starts_with("o3"))
        .map(|m| {
            let vision = m.id.contains("gpt-4o") || m.id.contains("gpt-4.1") || m.id.contains("o1");
            CloudModel {
                id: format!("{}:{}", provider.id(), m.id),
                name: m.id.clone(),
                provider: provider.id().to_string(),
                model: m.id,
                vision,
                // OpenAI's `/v1/models` reports no capabilities; every model
                // this filter keeps (gpt-*/o1/o3) accepts `tools`.
                tools: true,
            }
        })
        .collect();
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

#[derive(Deserialize)]
struct OrModelList {
    data: Vec<OrModel>,
}
#[derive(Deserialize)]
struct OrModel {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    architecture: Option<OrArch>,
    /// Which request parameters the model's endpoints accept. `"tools"` here is
    /// the difference between a working agent run and a bare `404 No endpoints
    /// found that support tool use` on the first turn that needs a tool.
    #[serde(default)]
    supported_parameters: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct OrArch {
    #[serde(default)]
    input_modalities: Vec<String>,
}

async fn discover_openrouter(client: &reqwest::Client) -> Result<Vec<CloudModel>, String> {
    // OpenRouter's catalog is public; the key is only needed at inference time.
    let list: OrModelList = client
        .get("https://openrouter.ai/api/v1/models")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut models: Vec<CloudModel> = list
        .data
        .into_iter()
        .map(|m| {
            let vision = m
                .architecture
                .as_ref()
                .map(|a| a.input_modalities.iter().any(|s| s == "image"))
                .unwrap_or(false);
            // Absent (an older catalog shape, or a model OpenRouter hasn't
            // classified) is read as capable: withdrawing tools from a model
            // that has them is a worse failure than the 404 this avoids, and
            // `drive_turn`'s retry still catches the latter.
            let tools = m
                .supported_parameters
                .as_ref()
                .map(|p| p.iter().any(|s| s == "tools"))
                .unwrap_or(true);
            CloudModel {
                name: m.name.unwrap_or_else(|| m.id.clone()),
                id: format!("openrouter:{}", m.id),
                provider: "openrouter".to_string(),
                model: m.id,
                vision,
                tools,
            }
        })
        .collect();
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

/// Anthropic doesn't need a discovery round-trip for the common set; curate the
/// current flagship models (CLD-4).
fn curated_anthropic() -> Vec<CloudModel> {
    let entries = [
        ("claude-3-5-sonnet-latest", "Claude 3.5 Sonnet", true),
        ("claude-3-5-haiku-latest", "Claude 3.5 Haiku", false),
        ("claude-3-opus-latest", "Claude 3 Opus", true),
    ];
    entries
        .into_iter()
        .map(|(model, name, vision)| CloudModel {
            id: format!("anthropic:{model}"),
            name: name.to_string(),
            provider: "anthropic".to_string(),
            model: model.to_string(),
            vision,
            tools: true,
        })
        .collect()
}

// ---- per-turn routing ----

/// Where a single agent turn should be sent.
#[derive(Debug, Clone)]
pub enum ChatEndpoint {
    /// Local llama-server, or any OpenAI-compatible cloud (OpenAI, OpenRouter).
    OpenAi {
        base_url: String,
        api_key: Option<String>,
        /// Required for cloud; `None` for local (engine uses the loaded model).
        model: Option<String>,
    },
    /// Anthropic Messages API.
    Anthropic { api_key: String, model: String },
}

/// Does this provider error mean "I can't take a `tools` array for this model"?
///
/// OpenRouter answers such a request with a bare `404` whose body carries the
/// only explanation there is — historically discarded by `error_for_status()`,
/// which is why this failure read as an unexplained "404 Not Found". Which
/// endpoints back a model changes between requests (sharply so for `:free`
/// variants), so this can strike mid-run after several tool calls have already
/// succeeded — it is not something a capability check at pick time can fully
/// rule out.
fn is_tool_support_error(err: &ProxyError) -> bool {
    let msg = err.provider_message().to_ascii_lowercase();
    matches!(err.status(), Some(404) | Some(400))
        && msg.contains("tool")
        && (msg.contains("no endpoints") || msg.contains("not support") || msg.contains("unsupported"))
}

/// Stream one turn to whichever endpoint is selected, returning how it ended.
/// `messages`/`tools` are in the OpenAI-compatible shape the agent loop builds;
/// the Anthropic adapter translates them internally.
///
/// When the provider refuses the `tools` array outright, the turn is retried
/// once without it rather than failing the run: the model still sees the tool
/// names in its system prompt, and the agent loop's text-tool-call fallback
/// (`run::parse_text_tool_calls`) can execute a call the model writes as
/// content. A degraded turn beats a dead one.
pub async fn drive_turn<F>(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    temperature: f32,
    cancel: &CancelFlag,
    mut on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(&str),
{
    let first = drive_once(client, endpoint, messages, tools, temperature, cancel, &mut on_token).await;
    match first {
        Err(e) if !tools.is_empty() && is_tool_support_error(&e) => {
            eprintln!("drive_turn: provider refused tools ({e}); retrying this turn without them");
            drive_once(client, endpoint, messages, &[], temperature, cancel, &mut on_token).await
        }
        other => other,
    }
}

/// One attempt at a turn — the body-building and adapter routing, without the
/// tools-refused retry above.
async fn drive_once<F>(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    temperature: f32,
    cancel: &CancelFlag,
    on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(&str),
{
    match endpoint {
        ChatEndpoint::OpenAi {
            base_url,
            api_key,
            model,
        } => {
            let mut body = serde_json::json!({
                "messages": messages,
                "temperature": temperature,
                "stream": true,
            });
            if !tools.is_empty() {
                body["tools"] = serde_json::Value::Array(tools.to_vec());
            }
            if let Some(model) = model {
                body["model"] = serde_json::Value::String(model.clone());
            }
            stream_turn(client, base_url, api_key.as_deref(), body, cancel, on_token).await
        }
        ChatEndpoint::Anthropic { api_key, model } => {
            anthropic::stream_turn(client, api_key, model, messages, tools, temperature, cancel, on_token)
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(status: u16, message: &str) -> ProxyError {
        ProxyError::Api { status, message: message.to_string() }
    }

    /// The exact failure this shipped for: OpenRouter's 404 body, once it is
    /// no longer thrown away, is recognisable as "drop the tools and retry".
    #[test]
    fn recognises_openrouters_tool_use_refusal() {
        assert!(is_tool_support_error(&api(404, "404 Not Found — No endpoints found that support tool use.")));
        assert!(is_tool_support_error(&api(400, "400 — This model does not support tools")));
    }

    /// Everything else must fail loudly rather than being silently retried
    /// with a weaker request — a missing key or an exhausted balance is the
    /// user's to see, and retrying it just doubles the wait.
    #[test]
    fn leaves_unrelated_failures_alone() {
        assert!(!is_tool_support_error(&api(404, "404 Not Found — No endpoints found for nvidia/nemotron-3-ultra:free")));
        assert!(!is_tool_support_error(&api(401, "401 — invalid api key")));
        assert!(!is_tool_support_error(&api(402, "402 — insufficient credits")));
        assert!(!is_tool_support_error(&api(429, "429 — rate limited, no tools involved")));
    }

    /// `supported_parameters` is what tells the picker a model can't be given
    /// tools at all — the field the catalog parser used to drop on the floor.
    #[test]
    fn reads_tool_capability_from_the_openrouter_catalog() {
        let list: OrModelList = serde_json::from_str(
            r#"{"data":[
                {"id":"a/with-tools","supported_parameters":["temperature","tools"]},
                {"id":"b/no-tools","supported_parameters":["temperature"]},
                {"id":"c/unclassified"}
            ]}"#,
        )
        .unwrap();
        let caps: Vec<bool> = list
            .data
            .iter()
            .map(|m| {
                m.supported_parameters
                    .as_ref()
                    .map(|p| p.iter().any(|s| s == "tools"))
                    .unwrap_or(true)
            })
            .collect();
        assert_eq!(caps, [true, false, true], "an unclassified model is assumed capable");
    }
}
