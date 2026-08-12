//! Agent loop + permission commands (Phase 4).

use tauri::ipc::Channel;
use tauri::State;

use crate::agent::browser::BrowserPool;
use crate::agent::run::{run_agent, AgentEventSink};
use crate::agent::toolsets::{self, Toolset, ToolsetInfo};
use crate::agent::AgentEvent;
use crate::cloud::{self, endpoints, ChatEndpoint, Provider};
use crate::db::Db;
use crate::memory::MemoryStore;
use crate::permissions::{Decision, PermissionManager};
use crate::runtime::{EmbedManager, RerankManager, RuntimeManager};
use crate::PoiesisError;

/// A turn message whose `content` is either a plain string or an OpenAI-style
/// content-part array (text + image_url) for multimodal/vision input (CHT-5).
#[derive(serde::Deserialize)]
pub struct TurnMessage {
    pub role: String,
    pub content: serde_json::Value,
}

/// Which model a turn should run against (CLD-3 routing). Defaults to the local
/// engine when absent.
#[derive(serde::Deserialize, Default)]
pub struct ChatTarget {
    pub provenance: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// Run an agentic turn against the loaded local engine, streaming a visible step
/// timeline + prose to `on_event` (CHT-9). Tools may pause for consent (§5.4.4).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn agent_chat_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    rerank_mgr: State<'_, RerankManager>,
    db: State<'_, Db>,
    perms: State<'_, PermissionManager>,
    memory: State<'_, MemoryStore>,
    browser_pool: State<'_, BrowserPool>,
    conversation_id: String,
    assistant_message_id: Option<String>,
    messages: Vec<TurnMessage>,
    temperature: Option<f32>,
    tools_enabled: Option<bool>,
    target: Option<ChatTarget>,
    on_event: Channel<AgentEvent>,
) -> Result<(), PoiesisError> {
    // Resolve where this turn runs: the local engine, a cloud provider (CLD-3),
    // or a user's own connected server.
    let target = target.unwrap_or_default();
    let is_remote = matches!(target.provenance.as_deref(), Some("cloud") | Some("endpoint"));

    let endpoint = match build_remote_endpoint(&db, &target) {
        Ok(Some(ep)) => ep,
        Ok(None) => {
            let Some((base_url, token)) = mgr.engine_endpoint().await else {
                let _ = on_event.send(AgentEvent::Error {
                    message: "No model is loaded yet. Pick a model to get started.".into(),
                });
                return Ok(());
            };
            ChatEndpoint::OpenAi {
                base_url,
                api_key: Some(token),
                model: None,
            }
        }
        Err(message) => {
            let _ = on_event.send(AgentEvent::Error { message });
            return Ok(());
        }
    };

    let mut msgs: Vec<serde_json::Value> = messages
        .into_iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    // The working folder is described here rather than in the frontend's prompt
    // so it can never drift from what the file tools will actually enforce. Only
    // worth saying when the model has tools to act on it.
    if tools_enabled.unwrap_or(false) && Toolset::FileSystem.is_enabled(&db) {
        if let Some(brief) = crate::agent::filesystem::working_folder_brief(&db, &conversation_id) {
            let at = msgs
                .iter()
                .position(|m| m["role"] != "system")
                .unwrap_or(msgs.len());
            msgs.insert(at, serde_json::json!({ "role": "system", "content": brief }));
        }
    }

    // Key for per-model tool reliability stats (GRM-4): the cloud/endpoint
    // model id, or the running local model's file stem.
    let model_name = if is_remote {
        target.model.clone().unwrap_or_else(|| "cloud".to_string())
    } else {
        mgr.engine_model_name()
            .await
            .unwrap_or_else(|| "local".to_string())
    };

    // A toolset's own side call (SCP-1's scope classification) runs here,
    // whatever the turn itself is running on: work the user didn't ask for
    // must not land on their cloud bill or leave the machine. The integrated
    // engine is preferred when it's loaded; a turn already running against
    // the user's own connected server satisfies the same rule, so that's the
    // fallback rather than `None`. Only a bare cloud turn with nothing loaded
    // locally leaves this `None` — the toolset then does without.
    let local_endpoint = match mgr.engine_endpoint().await {
        Some((base_url, token)) => Some(ChatEndpoint::OpenAi {
            base_url,
            api_key: Some(token),
            model: None,
        }),
        None if target.provenance.as_deref() == Some("endpoint") => Some(endpoint.clone()),
        None => None,
    };

    let cancel = mgr.new_cancel();
    let sink = AgentEventSink::new(on_event);
    let images_dir = mgr.generated_media_dir();
    run_agent(
        &mgr.client,
        &endpoint,
        local_endpoint.as_ref(),
        &db,
        &mgr,
        &embed_mgr,
        &rerank_mgr,
        &perms,
        &memory,
        Some(&browser_pool),
        &conversation_id,
        assistant_message_id.as_deref(),
        &images_dir,
        &model_name,
        msgs,
        temperature.unwrap_or(0.7),
        tools_enabled.unwrap_or(false),
        // Every caller of this command is an interactive turn — headless runs
        // go through `scheduler::run_job` instead, which calls `run_agent`
        // directly with `headless: true`.
        false,
        cancel,
        &sink,
    )
    .await;
    Ok(())
}

/// Resolve a `ChatTarget` to the endpoint that should serve it. `Ok(None)`
/// means "use the integrated runtime" — the caller still owns that fallback,
/// since only it knows whether a missing engine is fatal for this call.
///
/// This is the one place both remote kinds (BYOK cloud, and a user's own
/// connected server) are decided, so every call site that used to hand-roll
/// the `provenance == "cloud"` branch shares this instead.
pub(crate) fn build_remote_endpoint(db: &Db, target: &ChatTarget) -> Result<Option<ChatEndpoint>, String> {
    match target.provenance.as_deref() {
        Some("cloud") => build_cloud_endpoint(target).map(Some),
        Some("endpoint") => {
            let endpoint_id = target
                .provider
                .as_deref()
                .ok_or("This model is missing its server id.")?;
            let model = target
                .model
                .clone()
                .ok_or("This model is missing its model id.")?;
            let row = db
                .get_local_endpoint(endpoint_id)
                .map_err(|e| e.to_string())?
                .ok_or("That model server is no longer connected. Add it again in Settings.")?;
            Ok(Some(endpoints::chat_endpoint(&row, model)))
        }
        _ => Ok(None),
    }
}

/// Build the cloud endpoint for a target, fetching the provider key from the OS
/// credential store. Returns a user-facing message on failure.
pub(crate) fn build_cloud_endpoint(target: &ChatTarget) -> Result<ChatEndpoint, String> {
    let provider_id = target
        .provider
        .as_deref()
        .ok_or("This cloud model is missing its provider.")?;
    let provider =
        Provider::from_id(provider_id).ok_or_else(|| format!("Unknown provider '{provider_id}'."))?;
    let model = target
        .model
        .clone()
        .ok_or("This cloud model is missing its model id.")?;
    let key = cloud::get_key(provider).ok_or_else(|| {
        format!(
            "No API key for {}. Add one in Settings to use its models.",
            provider.name()
        )
    })?;

    Ok(if provider.uses_anthropic_api() {
        ChatEndpoint::Anthropic { api_key: key, model }
    } else {
        ChatEndpoint::OpenAi {
            base_url: provider.base_url().to_string(),
            api_key: Some(key),
            model: Some(model),
        }
    })
}

/// Answer a pending permission request from the side panel (§5.4.4).
#[tauri::command]
pub fn resolve_permission_cmd(perms: State<'_, PermissionManager>, id: String, decision: Decision) {
    perms.resolve(&id, decision);
}

/// List the built-in toolsets with their current enabled state (TOOL-6, TSET-2),
/// for the Settings surface.
#[tauri::command]
pub fn list_toolsets_cmd(db: State<'_, Db>) -> Vec<ToolsetInfo> {
    toolsets::all_info(&db)
}

/// How reliably one toolset's tools have run lately (LOOP-UI-1), for the muted
/// caption under each Settings toggle.
#[derive(serde::Serialize)]
pub struct ToolsetReliability {
    pub skill_id: String,
    pub ok_percent: i64,
    pub calls: i64,
}

/// Aggregate the last 7 days of `tool_stats` per toolset. The toolset↔tool
/// mapping lives here (`Toolset::handles`), so the UI just renders what it's
/// given.
#[tauri::command]
pub fn get_tool_stats_cmd(db: State<'_, Db>) -> Vec<ToolsetReliability> {
    let Ok(rows) = db.tool_stats_since(7) else {
        return Vec::new();
    };
    Toolset::ALL
        .into_iter()
        .filter_map(|toolset| {
            let (ok, calls) = rows
                .iter()
                .filter(|r| toolset.handles(&r.tool_name))
                .fold((0, 0), |(o, c), r| (o + r.ok, c + r.total));
            if calls == 0 {
                return None; // absent when there's no data
            }
            Some(ToolsetReliability {
                skill_id: toolset.id().to_string(),
                ok_percent: (ok * 100) / calls,
                calls,
            })
        })
        .collect()
}

/// Enable or disable a built-in toolset (TOOL-6, TSET-2).
#[tauri::command]
pub fn set_toolset_enabled_cmd(db: State<'_, Db>, id: String, enabled: bool) -> Result<(), PoiesisError> {
    let toolset = Toolset::from_id(&id)
        .ok_or_else(|| PoiesisError::Message(format!("Unknown toolset '{id}'.")))?;
    toolset.set_enabled(&db, enabled);
    Ok(())
}
