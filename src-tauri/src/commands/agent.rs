//! Agent loop + permission commands (Phase 4).

use tauri::ipc::Channel;
use tauri::State;

use crate::agent::run::{run_agent, AgentEventSink};
use crate::agent::skills::{self, Skill, SkillInfo};
use crate::agent::AgentEvent;
use crate::cloud::{self, ChatEndpoint, Provider};
use crate::db::Db;
use crate::permissions::{Decision, PermissionManager};
use crate::runtime::RuntimeManager;
use crate::NexusError;

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
    db: State<'_, Db>,
    perms: State<'_, PermissionManager>,
    conversation_id: String,
    assistant_message_id: Option<String>,
    messages: Vec<TurnMessage>,
    temperature: Option<f32>,
    tools_enabled: Option<bool>,
    target: Option<ChatTarget>,
    on_event: Channel<AgentEvent>,
) -> Result<(), NexusError> {
    // Resolve where this turn runs: the local engine, or a cloud provider (CLD-3).
    let target = target.unwrap_or_default();
    let is_cloud = target.provenance.as_deref() == Some("cloud");

    let endpoint = if is_cloud {
        match build_cloud_endpoint(&target) {
            Ok(ep) => ep,
            Err(message) => {
                let _ = on_event.send(AgentEvent::Error { message });
                return Ok(());
            }
        }
    } else {
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
    };

    let msgs: Vec<serde_json::Value> = messages
        .into_iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    let cancel = mgr.new_cancel();
    let sink = AgentEventSink::new(on_event);
    let images_dir = mgr.generated_images_dir();
    run_agent(
        &mgr.client,
        &endpoint,
        &db,
        &perms,
        &conversation_id,
        assistant_message_id.as_deref(),
        &images_dir,
        msgs,
        temperature.unwrap_or(0.7),
        tools_enabled.unwrap_or(false),
        cancel,
        &sink,
    )
    .await;
    Ok(())
}

/// Build the cloud endpoint for a target, fetching the provider key from the OS
/// credential store. Returns a user-facing message on failure.
fn build_cloud_endpoint(target: &ChatTarget) -> Result<ChatEndpoint, String> {
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

/// List the built-in skills with their current enabled state (TOOL-6), for the
/// Settings surface.
#[tauri::command]
pub fn list_skills_cmd(db: State<'_, Db>) -> Vec<SkillInfo> {
    skills::all_info(&db)
}

/// Enable or disable a built-in skill (TOOL-6).
#[tauri::command]
pub fn set_skill_enabled_cmd(db: State<'_, Db>, id: String, enabled: bool) -> Result<(), NexusError> {
    let skill = Skill::from_id(&id)
        .ok_or_else(|| NexusError::Message(format!("Unknown skill '{id}'.")))?;
    skill.set_enabled(&db, enabled);
    Ok(())
}
