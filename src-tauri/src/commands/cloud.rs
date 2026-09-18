//! BYOK cloud-provider commands (Phase 7, CLD-2/3/4): list providers, manage
//! keys (in the OS credential store), and discover each provider's models.
//! `PRV-2/3`: the list also carries media-only backends, and a key is checked
//! against the provider before it is saved.

use tauri::State;

use crate::cloud::{self, CloudModel, Provider, ProviderInfo};
use crate::media::{self, Registry};
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn parse_provider(id: &str) -> Cmd<Provider> {
    Provider::from_id(id).ok_or_else(|| PoiesisError::Message(format!("Unknown provider '{id}'.")))
}

fn clean_key(key: &str) -> Cmd<&str> {
    let key = key.trim();
    if key.is_empty() {
        return Err(PoiesisError::Message("The key can't be empty.".into()));
    }
    Ok(key)
}

/// Every Providers card (`PRV-2`): chat providers and media-only backends,
/// with whether a key is stored and what it unlocks.
#[tauri::command]
pub fn list_providers_cmd() -> Cmd<Vec<ProviderInfo>> {
    Ok(Registry::new().provider_cards())
}

/// Store a provider API key in the OS credential store (never SQLite),
/// without checking it. The Providers page uses `verify_provider_key_cmd`.
#[tauri::command]
pub fn set_provider_key_cmd(provider: String, key: String) -> Cmd<()> {
    let key = clean_key(&key)?;
    save_key(&provider, key)
}

fn save_key(id: &str, key: &str) -> Cmd<()> {
    let out = if let Some(p) = Provider::from_id(id) {
        cloud::set_key(p, key)
    } else if Registry::new().media_keyed(id).is_some() {
        media::set_media_key(id, key)
    } else {
        return Err(PoiesisError::Message(format!("Unknown provider '{id}'.")));
    };
    cloud::record_outcome(id, None);
    out.map_err(|e| PoiesisError::Message(e.to_string()))
}

/// `PRV-3`: make one cheap authenticated call with the key and save it only
/// if that works. The error is the sentence the card shows.
#[tauri::command]
pub async fn verify_provider_key_cmd(
    mgr: State<'_, RuntimeManager>,
    provider: String,
    key: String,
) -> Cmd<()> {
    let key = clean_key(&key)?;
    let checked = if let Some(p) = Provider::from_id(&provider) {
        cloud::verify_key(&mgr.client, p, key).await
    } else {
        let registry = Registry::new();
        let backend = registry
            .media_keyed(&provider)
            .ok_or_else(|| PoiesisError::Message(format!("Unknown provider '{provider}'.")))?;
        backend.verify_key(&mgr.client, key).await
    };
    checked.map_err(PoiesisError::Message)?;
    save_key(&provider, key)
}

/// Remove a stored provider key.
#[tauri::command]
pub fn clear_provider_key_cmd(provider: String) -> Cmd<()> {
    let out = if Registry::new().media_keyed(&provider).is_some() {
        media::clear_media_key(&provider)
    } else {
        cloud::clear_key(parse_provider(&provider)?)
    };
    cloud::record_outcome(&provider, None);
    out.map_err(|e| PoiesisError::Message(e.to_string()))
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
