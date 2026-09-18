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
pub mod pricing;

use serde::{Deserialize, Serialize};

use crate::runtime::proxy::{stream_turn, CancelFlag, Delta, ProxyError, TurnOutcome};
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

/// One card on the Providers page (`PRV-2`). Built from the backend so the
/// frontend never hard-codes a provider: chat providers from [`Provider::ALL`],
/// media-only ones from every `Credential::Media` backend.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    /// "cloud" (a chat provider, whose key may also unlock media) or "media".
    pub kind: String,
    pub key_set: bool,
    pub key_hint: String,
    pub console_url: String,
    /// What connecting it gives you: any of "chat", "image", "video".
    pub unlocks: Vec<String>,
    /// The last real request's auth/billing failure, if it had one (`PRV-4`).
    pub last_error: Option<String>,
}

// ---- last request status (`PRV-4`) ----

fn last_errors() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    static ERRORS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
        std::sync::OnceLock::new();
    ERRORS.get_or_init(Default::default)
}

/// Remember how the last request to a provider went. Only failures the user
/// can fix from the Providers page are kept (a bad key, no credit); anything
/// else clears the flag, because a timeout says nothing about the account.
pub fn record_outcome(provider_id: &str, error: Option<String>) {
    if let Ok(mut map) = last_errors().lock() {
        match error {
            Some(e) => {
                map.insert(provider_id.to_string(), e);
            }
            None => {
                map.remove(provider_id);
            }
        }
    }
}

pub fn last_error(provider_id: &str) -> Option<String> {
    last_errors().lock().ok()?.get(provider_id).cloned()
}

/// The account-level meaning of an HTTP status, in words, or `None` when the
/// status says nothing about the account.
fn account_problem(provider: Provider, status: u16) -> Option<String> {
    match status {
        401 | 403 => Some(format!("{} rejected the key. Replace it to keep using its models.", provider.name())),
        402 => Some(format!("Out of credits. Top up at {} to keep using its models.", provider.name())),
        _ => None,
    }
}

/// Which provider a turn endpoint talks to, if it is one of ours.
fn provider_of(endpoint: &ChatEndpoint) -> Option<Provider> {
    match endpoint {
        ChatEndpoint::Anthropic { .. } => Some(Provider::Anthropic),
        ChatEndpoint::OpenAi { base_url, model: Some(_), .. } => {
            Provider::ALL.into_iter().find(|p| base_url.starts_with(p.base_url()))
        }
        ChatEndpoint::OpenAi { .. } => None,
    }
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
    /// USD per million tokens, when known (`MOD-5`). OpenRouter's catalog
    /// says; elsewhere it's [`pricing`]'s table. `None` is unknown, never free.
    pub prompt_per_mtok: Option<f64>,
    pub output_per_mtok: Option<f64>,
}

impl CloudModel {
    /// Fill in list prices from the table where the catalog gave none.
    fn priced(mut self) -> Self {
        if self.prompt_per_mtok.is_none() {
            if let Some(f) = pricing::facts_for(&self.model) {
                self.prompt_per_mtok = Some(f.prompt_per_mtok);
                self.output_per_mtok = Some(f.output_per_mtok);
            }
        }
        self
    }
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

/// The chat providers as cards. `media_unlocks` adds what each key also
/// unlocks through a media backend (`Credential::Cloud`); see
/// `media::provider_cards`, which is the one caller that knows.
pub fn provider_infos(media_unlocks: impl Fn(Provider) -> Vec<String>) -> Vec<ProviderInfo> {
    Provider::ALL
        .into_iter()
        .map(|p| {
            let mut unlocks = vec!["chat".to_string()];
            unlocks.extend(media_unlocks(p));
            ProviderInfo {
                id: p.id().to_string(),
                name: p.name().to_string(),
                kind: "cloud".to_string(),
                key_set: has_key(p),
                key_hint: p.key_hint().to_string(),
                console_url: p.console_url().to_string(),
                unlocks,
                last_error: last_error(p.id()),
            }
        })
        .collect()
}

// ---- key verification (`PRV-3`) ----

/// Make one cheap authenticated call with `key` and say, in words, why it
/// failed if it did. Nothing is stored here; the caller saves on `Ok`.
pub async fn verify_key(client: &reqwest::Client, provider: Provider, key: &str) -> Result<(), String> {
    let req = match provider {
        // OpenRouter's catalog is public, so listing models proves nothing
        // about the key. `/key` describes the key itself and 401s without one.
        Provider::OpenRouter => client.get("https://openrouter.ai/api/v1/key").bearer_auth(key),
        Provider::OpenAi => client.get(format!("{}/v1/models", provider.base_url())).bearer_auth(key),
        Provider::Anthropic => client
            .get(format!("{}/v1/models?limit=1", provider.base_url()))
            .header("x-api-key", key)
            .header("anthropic-version", anthropic::API_VERSION),
    };
    let resp = req
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| format!("Couldn't reach {}. Are you online?", provider.name()))?;
    verify_status(provider, resp.status().as_u16())
}

fn verify_status(provider: Provider, status: u16) -> Result<(), String> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(format!("That key was rejected ({status}). Check you copied all of it.")),
        402 => Err(format!("The key works, but the account is out of credits. Top up at {}.", provider.name())),
        429 => Err(format!("{} is rate-limiting this key. Try again in a minute.", provider.name())),
        _ => Err(format!("{} answered with an error ({status}). Try again later.", provider.name())),
    }
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
        Provider::Anthropic => Ok(discover_anthropic(client).await.unwrap_or_else(|_| curated_anthropic())),
    }
}

#[derive(Deserialize)]
struct AnthropicModelList {
    data: Vec<AnthropicModel>,
}
#[derive(Deserialize)]
struct AnthropicModel {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
}

/// `PRV-7`: Anthropic's own list, so the Models page never shows a stale
/// catalog. The curated list below is only the offline fallback.
async fn discover_anthropic(client: &reqwest::Client) -> Result<Vec<CloudModel>, String> {
    let key = get_key(Provider::Anthropic).ok_or("No API key set for this provider.")?;
    let list: AnthropicModelList = client
        .get(format!("{}/v1/models?limit=1000", Provider::Anthropic.base_url()))
        .header("x-api-key", &key)
        .header("anthropic-version", anthropic::API_VERSION)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(anthropic_models(list))
}

fn anthropic_models(list: AnthropicModelList) -> Vec<CloudModel> {
    list.data
        .into_iter()
        .map(|m| CloudModel {
            id: format!("anthropic:{}", m.id),
            name: m.display_name.unwrap_or_else(|| m.id.clone()),
            provider: "anthropic".to_string(),
            // Every Claude model since 3 takes images except 3.5 Haiku.
            vision: !m.id.contains("3-5-haiku"),
            model: m.id,
            tools: true,
            prompt_per_mtok: None,
            output_per_mtok: None,
        }
        .priced())
        .collect()
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
                prompt_per_mtok: None,
                output_per_mtok: None,
            }
            .priced()
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
    /// USD per token, as decimal strings.
    #[serde(default)]
    pricing: Option<OrPricing>,
}
#[derive(Deserialize)]
struct OrPricing {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
}

/// OpenRouter's per-token price string as USD per million tokens. A negative
/// price is its marker for "varies" (routers), which is unknown here.
fn per_mtok(raw: Option<&String>) -> Option<f64> {
    let v: f64 = raw?.trim().parse().ok()?;
    (v >= 0.0).then_some(v * 1_000_000.0)
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
            let (prompt_per_mtok, output_per_mtok) = match &m.pricing {
                Some(p) => (per_mtok(p.prompt.as_ref()), per_mtok(p.completion.as_ref())),
                None => (None, None),
            };
            CloudModel {
                name: m.name.unwrap_or_else(|| m.id.clone()),
                id: format!("openrouter:{}", m.id),
                provider: "openrouter".to_string(),
                model: m.id,
                vision,
                tools,
                prompt_per_mtok,
                output_per_mtok,
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
            prompt_per_mtok: None,
            output_per_mtok: None,
        }
        .priced())
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
#[allow(clippy::too_many_arguments)]
pub async fn drive_turn<F>(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    temperature: f32,
    effort: Effort,
    cancel: &CancelFlag,
    mut on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(Delta),
{
    let result = drive_turn_inner(client, endpoint, messages, tools, temperature, effort, cancel, &mut on_token).await;
    // `PRV-4`: the Providers card reads its state from the last real request.
    if let Some(provider) = provider_of(endpoint) {
        match &result {
            Ok(_) => record_outcome(provider.id(), None),
            Err(e) => {
                if let Some(problem) = e.status().and_then(|s| account_problem(provider, s)) {
                    record_outcome(provider.id(), Some(problem));
                }
            }
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn drive_turn_inner<F>(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    temperature: f32,
    effort: Effort,
    cancel: &CancelFlag,
    mut on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(Delta),
{
    let eff = Some(effort);
    let first = drive_once(client, endpoint, messages, tools, temperature, eff, cancel, &mut on_token).await;
    match first {
        Err(e) if !tools.is_empty() && is_tool_support_error(&e) => {
            eprintln!("drive_turn: provider refused tools ({e}); retrying this turn without them");
            drive_once(client, endpoint, messages, &[], temperature, eff, cancel, &mut on_token).await
        }
        // A compatible server that has never heard of the reasoning parameter.
        // Dropping it costs the setting for this turn, which is a far better
        // trade than losing the turn.
        Err(e) if is_reasoning_param_error(&e) => {
            eprintln!("drive_turn: provider refused the reasoning parameter ({e}); retrying without it");
            drive_once(client, endpoint, messages, tools, temperature, None, cancel, &mut on_token).await
        }
        other => other,
    }
}

/// How hard a reasoning model should think before answering.
///
/// Providers default this to their maximum, which for a chat app is the wrong
/// default in both directions: it is the slowest and the most expensive, and on
/// a free tier it is the setting most likely to end in a model that thinks for
/// ten minutes and never answers. Nothing in the app used to set it at all.
///
/// `Provider` sends nothing and takes whatever the provider does — kept because
/// it is the only honest option for a server we know nothing about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Effort {
    Off,
    #[default]
    Low,
    Medium,
    High,
    Provider,
}

impl Effort {
    /// Parse a stored setting. Anything unrecognised is the default rather than
    /// an error: a bad value in the database must not cost you a turn.
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" => Effort::Off,
            "medium" => Effort::Medium,
            "high" => Effort::High,
            "provider" | "default" => Effort::Provider,
            _ => Effort::Low,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Off => "off",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Provider => "provider",
        }
    }
}

/// Where the reasoning control goes in an OpenAI-shaped request body.
///
/// There are two spellings and they are not interchangeable. OpenRouter takes a
/// unified `reasoning` object (`{ effort }`, or `{ enabled: false }` to turn it
/// off); OpenAI's own API takes a flat `reasoning_effort` string and rejects
/// unknown top-level fields outright. Sending the wrong one is a 400, so this
/// picks by host rather than sending both and hoping.
///
/// Returns `None` when nothing should be sent: no model named (the integrated
/// engine, which we must not hand unknown fields), or the user asked for the
/// provider's own default.
fn reasoning_field(base_url: &str, effort: Effort) -> Option<(&'static str, serde_json::Value)> {
    if effort == Effort::Provider {
        return None;
    }
    if base_url.contains("openrouter.ai") {
        // `enabled: false` is the documented way to turn reasoning off; an
        // `effort` of "off" is not a value the API takes.
        return Some(match effort {
            Effort::Off => ("reasoning", serde_json::json!({ "enabled": false })),
            other => ("reasoning", serde_json::json!({ "effort": other.as_str() })),
        });
    }
    // Everything else OpenAI-shaped. There is no "off" here — the flat
    // parameter has no disable value — so the nearest truthful thing is the
    // lowest setting the API accepts.
    Some((
        "reasoning_effort",
        serde_json::json!(match effort {
            Effort::Off | Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High | Effort::Provider => "high",
        }),
    ))
}

/// Did the provider reject the request because of the reasoning parameter?
///
/// Same shape as `is_tool_support_error` and for the same reason: an
/// OpenAI-compatible server that has never heard of `reasoning_effort` should
/// cost us one retry, not the whole turn.
fn is_reasoning_param_error(e: &ProxyError) -> bool {
    let msg = e.provider_message().to_ascii_lowercase();
    (msg.contains("reasoning_effort") || msg.contains("reasoning"))
        && (msg.contains("unknown")
            || msg.contains("unsupported")
            || msg.contains("not support")
            || msg.contains("unrecognized")
            || msg.contains("extra fields")
            || msg.contains("additional properties"))
}

/// One attempt at a turn — the body-building and adapter routing, without the
/// tools-refused retry above.
#[allow(clippy::too_many_arguments)]
async fn drive_once<F>(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    temperature: f32,
    effort: Option<Effort>,
    cancel: &CancelFlag,
    on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(Delta),
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
                // `OBS-2`: a plain OpenAI-compatible stream reports its usage
                // only when asked. Asked here and not for the integrated engine
                // (`model: None`), which already reports on its final chunk and
                // is the one server we cannot afford to hand an unknown field.
                body["stream_options"] = serde_json::json!({ "include_usage": true });
                // Same caution: only where a model is named, i.e. never for the
                // integrated engine. `None` here is the retry saying the last
                // attempt was rejected over exactly this field.
                if let Some((key, value)) = effort.and_then(|e| reasoning_field(base_url, e)) {
                    body[key] = value;
                }
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

    const OPENROUTER: &str = "https://openrouter.ai/api";
    const OPENAI: &str = "https://api.openai.com";

    /// The two spellings are not interchangeable and sending the wrong one is a
    /// 400, so the choice is made by host rather than by sending both.
    #[test]
    fn each_provider_gets_the_reasoning_parameter_it_actually_takes() {
        assert_eq!(
            reasoning_field(OPENROUTER, Effort::Low),
            Some(("reasoning", serde_json::json!({ "effort": "low" })))
        );
        assert_eq!(
            reasoning_field(OPENAI, Effort::Low),
            Some(("reasoning_effort", serde_json::json!("low")))
        );
    }

    /// OpenRouter turns reasoning off with `enabled: false`; "off" is not a
    /// value its `effort` field takes, and sending it would be a 400.
    #[test]
    fn off_is_spelled_the_way_each_api_spells_it() {
        assert_eq!(
            reasoning_field(OPENROUTER, Effort::Off),
            Some(("reasoning", serde_json::json!({ "enabled": false })))
        );
        // The flat parameter has no disable value, so the nearest truthful
        // thing is the lowest it accepts — never an invented "off".
        assert_eq!(
            reasoning_field(OPENAI, Effort::Off),
            Some(("reasoning_effort", serde_json::json!("low")))
        );
    }

    /// Asking for the provider's own behaviour must send nothing at all — an
    /// explicit "high" is a different request from saying nothing.
    #[test]
    fn the_provider_default_sends_no_field() {
        assert_eq!(reasoning_field(OPENROUTER, Effort::Provider), None);
        assert_eq!(reasoning_field(OPENAI, Effort::Provider), None);
    }

    /// Unset means low, not the provider's maximum. This is the opinion: every
    /// provider defaults to its hardest setting, which costs latency and money
    /// nobody asked for, and on a free tier is what makes a model think itself
    /// into a corner and never answer.
    #[test]
    fn the_default_is_low_and_nonsense_does_not_cost_a_turn() {
        assert_eq!(Effort::default(), Effort::Low);
        assert_eq!(Effort::parse("HIGH"), Effort::High);
        assert_eq!(Effort::parse(" medium "), Effort::Medium);
        assert_eq!(Effort::parse("none"), Effort::Off);
        assert_eq!(Effort::parse("default"), Effort::Provider);
        assert_eq!(Effort::parse("banana"), Effort::Low, "a bad setting falls back, never errors");
    }

    /// A compatible server that has never heard of the parameter should cost
    /// one retry, not the turn.
    #[test]
    fn a_provider_that_refuses_the_parameter_is_recognised() {
        assert!(is_reasoning_param_error(&api(400, "Unknown parameter: 'reasoning_effort'.")));
        assert!(is_reasoning_param_error(&api(
            400,
            "additional properties are not allowed ('reasoning' was unexpected)"
        )));
        // Not every mention of the word is this failure. A model refusing to
        // reason about something must not silently drop the setting.
        assert!(!is_reasoning_param_error(&api(500, "reasoning engine overloaded")));
        assert!(!is_reasoning_param_error(&api(429, "rate limited")));
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

    /// `PRV-3`: a key is saved only on a 2xx, and every failure says what to do.
    #[test]
    fn key_verification_only_passes_on_success() {
        assert!(verify_status(Provider::OpenAi, 200).is_ok());
        let rejected = verify_status(Provider::OpenAi, 401).unwrap_err();
        assert!(rejected.contains("rejected (401)"), "{rejected}");
        assert!(verify_status(Provider::OpenRouter, 402).unwrap_err().contains("out of credits"));
        assert!(verify_status(Provider::Anthropic, 500).is_err());
    }

    /// `PRV-4`: only account problems mark a card; a timeout says nothing.
    #[test]
    fn only_account_failures_mark_a_provider() {
        assert!(account_problem(Provider::OpenRouter, 402).unwrap().contains("Out of credits"));
        assert!(account_problem(Provider::OpenAi, 401).is_some());
        assert!(account_problem(Provider::OpenAi, 429).is_none());
        assert!(account_problem(Provider::OpenAi, 500).is_none());
    }

    /// The integrated runtime and own servers are never mistaken for a provider.
    #[test]
    fn a_turn_endpoint_names_its_provider() {
        let anth = ChatEndpoint::Anthropic { api_key: "k".into(), model: "m".into() };
        assert_eq!(provider_of(&anth), Some(Provider::Anthropic));
        let or = ChatEndpoint::OpenAi {
            base_url: Provider::OpenRouter.base_url().into(),
            api_key: Some("k".into()),
            model: Some("x".into()),
        };
        assert_eq!(provider_of(&or), Some(Provider::OpenRouter));
        let local = ChatEndpoint::OpenAi { base_url: "http://127.0.0.1:8080".into(), api_key: None, model: None };
        assert_eq!(provider_of(&local), None);
    }

    /// `PRV-7`: Anthropic's own list, read as the picker needs it.
    /// `MOD-5`: OpenRouter's per-token strings become per-million figures;
    /// its "varies" marker is unknown, not free.
    #[test]
    fn reads_openrouter_prices_per_million() {
        assert_eq!(per_mtok(Some(&"0.000003".to_string())).map(|v| (v * 100.0).round() / 100.0), Some(3.0));
        assert_eq!(per_mtok(Some(&"0".to_string())), Some(0.0));
        assert_eq!(per_mtok(Some(&"-1".to_string())), None);
        assert_eq!(per_mtok(None), None);
    }

    #[test]
    fn reads_anthropics_model_list() {
        let list: AnthropicModelList = serde_json::from_str(
            r#"{"data":[
                {"id":"claude-sonnet-4-5","display_name":"Claude Sonnet 4.5","type":"model"},
                {"id":"claude-3-5-haiku-20241022","display_name":"Claude Haiku 3.5","type":"model"}
            ],"has_more":false}"#,
        )
        .unwrap();
        let models = anthropic_models(list);
        assert_eq!(models[0].id, "anthropic:claude-sonnet-4-5");
        assert_eq!(models[0].name, "Claude Sonnet 4.5");
        assert!(models[0].vision);
        assert!(!models[1].vision);
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
