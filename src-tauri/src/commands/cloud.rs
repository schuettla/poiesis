//! BYOK cloud-provider commands (Phase 7, CLD-2/3/4): list providers, manage
//! keys (in the OS credential store), and discover each provider's models.

use tauri::State;

use crate::cloud::{self, CloudModel, Provider, ProviderInfo};
use crate::runtime::RuntimeManager;
use crate::NexusError;

type Cmd<T> = Result<T, NexusError>;

fn parse_provider(id: &str) -> Cmd<Provider> {
    Provider::from_id(id).ok_or_else(|| NexusError::Message(format!("Unknown provider '{id}'.")))
}

/// Providers and whether a key is stored for each (CLD-2, §5.4.5).
#[tauri::command]
pub fn list_providers_cmd() -> Cmd<Vec<ProviderInfo>> {
    Ok(cloud::provider_infos())
}

/// Store a provider API key in the OS credential store (never SQLite).
#[tauri::command]
pub fn set_provider_key_cmd(provider: String, key: String) -> Cmd<()> {
    let provider = parse_provider(&provider)?;
    let key = key.trim();
    if key.is_empty() {
        return Err(NexusError::Message("The key can't be empty.".into()));
    }
    cloud::set_key(provider, key).map_err(|e| NexusError::Message(e.to_string()))
}

/// Remove a stored provider key.
#[tauri::command]
pub fn clear_provider_key_cmd(provider: String) -> Cmd<()> {
    let provider = parse_provider(&provider)?;
    cloud::clear_key(provider).map_err(|e| NexusError::Message(e.to_string()))
}

/// Discover models across every provider that has a key (CLD-3, CLD-4).
/// Best-effort: a provider that fails discovery is skipped, not fatal.
#[tauri::command]
pub async fn list_cloud_models_cmd(mgr: State<'_, RuntimeManager>) -> Cmd<Vec<CloudModel>> {
    let mut out = Vec::new();
    for provider in Provider::ALL {
        if !cloud::has_key(provider) {
            continue;
        }
        if let Ok(models) = cloud::discover_models(&mgr.client, provider).await {
            out.extend(models);
        }
    }
    Ok(out)
}
