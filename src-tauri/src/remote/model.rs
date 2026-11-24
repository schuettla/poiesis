//! REM-2: the model a remote turn runs on, found in Rust.
//!
//! The window builds `TurnModel` and `ChatTarget` from the id stored in `default_model.chat`
//! (`lib/modelPrefs.ts`, `turnModelFor` and `targetFor` in `store.ts`). A phone has no window, so
//! this does the same from the same setting. It never picks a model the person did not pick.

use crate::agent::tier;
use crate::agent::turn::TurnModel;
use crate::cloud::{self, endpoints, CloudModel, Provider};
use crate::commands::agent::ChatTarget;
use crate::db::{Db, ModelEntry};
use crate::runtime::RuntimeManager;

/// The setting the window's "default chat model" is stored in.
const DEFAULT_CHAT_KEY: &str = "default_model.chat";

/// What `run_turn` needs to know about where a turn runs.
#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub model: TurnModel,
    pub target: ChatTarget,
    /// For a model from the library: the file the engine must be running. `None` for a hosted
    /// model or a connected server.
    pub local_path: Option<String>,
}

/// The model lists the id is looked up in. Only the list the id belongs to is filled in.
#[derive(Default)]
pub struct Catalog {
    pub local: Vec<ModelEntry>,
    pub endpoint: Vec<endpoints::EndpointModel>,
    pub cloud: Vec<CloudModel>,
}

/// Match an id to a model, exactly as the window's rules do. Pure, so it is tested per provenance.
pub fn resolve(id: &str, catalog: &Catalog) -> Option<ResolvedModel> {
    if id.starts_with("endpoint:") {
        let m = catalog.endpoint.iter().find(|m| m.id == id)?;
        return Some(ResolvedModel {
            model: TurnModel {
                name: m.name.clone(),
                provenance: "endpoint".into(),
                vision: m.vision,
                ctx_size: Some(m.ctx_size),
            },
            target: ChatTarget {
                provenance: Some("endpoint".into()),
                provider: Some(m.endpoint_id.clone()),
                model: Some(m.model.clone()),
            },
            local_path: None,
        });
    }
    if id.starts_with("cloud:") {
        let m = catalog
            .cloud
            .iter()
            .find(|m| tier::cloud_model_id(&m.provider, &m.model) == id)?;
        return Some(ResolvedModel {
            model: TurnModel {
                name: m.name.clone(),
                provenance: "cloud".into(),
                vision: m.vision,
                ctx_size: None,
            },
            target: ChatTarget {
                provenance: Some("cloud".into()),
                provider: Some(m.provider.clone()),
                model: Some(m.model.clone()),
            },
            local_path: None,
        });
    }
    // A chat model from the library. Embedding and rerank models are not chat models.
    let e = catalog.local.iter().find(|e| e.id == id && e.role == "chat")?;
    Some(ResolvedModel {
        model: TurnModel {
            name: e.name.clone(),
            provenance: "local".into(),
            vision: e.vision,
            ctx_size: None,
        },
        target: ChatTarget { provenance: Some("local".into()), provider: None, model: None },
        local_path: Some(e.path.clone()),
    })
}

/// The sentence for a person who has not picked a default chat model.
pub const NO_DEFAULT: &str = "No default chat model is set. Pick one in Models on the desktop.";

/// The default chat model, ready for `run_turn`. The error is one plain line, fit to show in a chat.
///
/// A model from the library has to be running already: a phone message does not start an engine.
pub async fn default_turn_model(db: &Db, mgr: &RuntimeManager) -> Result<ResolvedModel, String> {
    let id = db
        .get_setting(DEFAULT_CHAT_KEY)
        .ok()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| NO_DEFAULT.to_string())?;
    let mut catalog = Catalog::default();
    if id.starts_with("endpoint:") {
        let rows = db.list_local_endpoints().map_err(|e| e.to_string())?;
        for ep in rows.into_iter().filter(|ep| ep.enabled) {
            if id.starts_with(&tier::endpoint_model_id(&ep.id, "")) {
                catalog.endpoint = endpoints::discover(&mgr.client, &ep).await.unwrap_or_default();
            }
        }
    } else if let Some(rest) = id.strip_prefix("cloud:") {
        let provider = rest.split(':').next().and_then(Provider::from_id);
        if let Some(provider) = provider.filter(|p| cloud::has_key(*p)) {
            catalog.cloud = cloud::discover_models(&mgr.client, provider).await.unwrap_or_default();
        }
    } else {
        catalog.local = db.list_models().map_err(|e| e.to_string())?;
    }
    let resolved = resolve(&id, &catalog).ok_or_else(|| {
        let label = db
            .get_setting("models.labels")
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str::<std::collections::HashMap<String, String>>(&raw).ok())
            .and_then(|m| m.get(&id).cloned())
            .unwrap_or_else(|| "Your default model".into());
        format!("{label} is not available right now. Check Models on the desktop.")
    })?;
    if let Some(path) = &resolved.local_path {
        let running = mgr.engine_model_path().await.map(|p| p.to_string_lossy().into_owned());
        if running.as_deref() != Some(path.as_str()) {
            return Err(format!(
                "{} is not running on the desktop. Start it there first.",
                resolved.model.name
            ));
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, role: &str, vision: bool) -> ModelEntry {
        ModelEntry {
            id: id.into(),
            name: format!("name of {id}"),
            path: format!("C:/models/{id}.gguf"),
            quant: None,
            size_bytes: None,
            vision,
            role: role.into(),
            is_default: false,
            added_at: 0,
        }
    }

    #[test]
    fn a_local_model_carries_its_file_and_vision() {
        let c = Catalog { local: vec![entry("m1", "chat", true)], ..Catalog::default() };
        let r = resolve("m1", &c).expect("found");
        assert_eq!(r.model.provenance, "local");
        assert_eq!(r.model.name, "name of m1");
        assert!(r.model.vision);
        assert_eq!(r.model.ctx_size, None);
        assert_eq!(r.target.provenance.as_deref(), Some("local"));
        assert_eq!(r.local_path.as_deref(), Some("C:/models/m1.gguf"));
    }

    #[test]
    fn an_embedding_model_is_never_a_chat_model() {
        let c = Catalog { local: vec![entry("e1", "embed", false)], ..Catalog::default() };
        assert!(resolve("e1", &c).is_none());
    }

    #[test]
    fn an_endpoint_model_keeps_the_servers_own_window() {
        let c = Catalog {
            endpoint: vec![endpoints::EndpointModel {
                id: "endpoint:ep1:qwen".into(),
                endpoint_id: "ep1".into(),
                endpoint_label: "Ollama".into(),
                name: "qwen".into(),
                model: "qwen".into(),
                vision: false,
                tools: true,
                ctx_size: 16384,
            }],
            ..Catalog::default()
        };
        let r = resolve("endpoint:ep1:qwen", &c).expect("found");
        assert_eq!(r.model.provenance, "endpoint");
        assert_eq!(r.model.ctx_size, Some(16384));
        assert_eq!(r.target.provenance.as_deref(), Some("endpoint"));
        assert_eq!(r.target.provider.as_deref(), Some("ep1"));
        assert_eq!(r.target.model.as_deref(), Some("qwen"));
        assert!(r.local_path.is_none());
    }

    #[test]
    fn a_cloud_model_is_routed_by_provider_and_model() {
        let c = Catalog {
            cloud: vec![CloudModel {
                id: "openrouter:acme/big".into(),
                name: "Big".into(),
                provider: "openrouter".into(),
                model: "acme/big".into(),
                vision: true,
                tools: true,
                prompt_per_mtok: None,
                output_per_mtok: None,
                window: None,
            }],
            ..Catalog::default()
        };
        let r = resolve("cloud:openrouter:acme/big", &c).expect("found");
        assert_eq!(r.model.provenance, "cloud");
        assert!(r.model.vision);
        assert_eq!(r.target.provider.as_deref(), Some("openrouter"));
        assert_eq!(r.target.model.as_deref(), Some("acme/big"));
    }

    #[test]
    fn an_id_nobody_serves_is_not_found() {
        assert!(resolve("cloud:openai:gpt", &Catalog::default()).is_none());
        assert!(resolve("endpoint:x:y", &Catalog::default()).is_none());
        assert!(resolve("gone", &Catalog::default()).is_none());
    }

    #[tokio::test]
    async fn with_no_default_set_the_answer_is_one_plain_line() {
        let db = Db::open_in_memory().unwrap();
        let mgr = RuntimeManager::new(std::env::temp_dir());
        let err = default_turn_model(&db, &mgr).await.unwrap_err();
        assert_eq!(err, NO_DEFAULT);
        db.set_setting(DEFAULT_CHAT_KEY, "  ").unwrap();
        assert_eq!(default_turn_model(&db, &mgr).await.unwrap_err(), NO_DEFAULT);
    }

    #[tokio::test]
    async fn a_default_that_is_gone_or_not_running_is_said_so() {
        let db = Db::open_in_memory().unwrap();
        let mgr = RuntimeManager::new(std::env::temp_dir());
        let added = db
            .add_model(&crate::db::NewModelEntry {
                name: "Small".into(),
                path: "C:/models/small.gguf".into(),
                quant: None,
                size_bytes: None,
                vision: false,
            })
            .unwrap();
        db.set_setting(DEFAULT_CHAT_KEY, &added.id).unwrap();
        let err = default_turn_model(&db, &mgr).await.unwrap_err();
        assert_eq!(err, "Small is not running on the desktop. Start it there first.");
        db.set_setting(DEFAULT_CHAT_KEY, "missing").unwrap();
        let err = default_turn_model(&db, &mgr).await.unwrap_err();
        assert_eq!(err, "Your default model is not available right now. Check Models on the desktop.");
    }
}
