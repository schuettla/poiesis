//! Agent loop + permission commands (Phase 4).

use tauri::ipc::Channel;
use tauri::State;

use crate::agent::artifacts::ConsoleEntry;
use crate::agent::browser::BrowserPool;
use crate::agent::fleet::{Fleet, RunLimits, Steer};
use crate::agent::run::{run_agent, AgentEventSink, RunContext};
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
#[derive(serde::Deserialize, Default, Clone)]
pub struct ChatTarget {
    pub provenance: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// Everything a turn needs to know about *where* it runs, resolved once. Shared
/// by the ordinary turn and by resume, so a resumed run cannot quietly land on
/// a different model than the run it is continuing.
struct TurnSetup {
    endpoint: ChatEndpoint,
    local_endpoint: Option<ChatEndpoint>,
    model_name: String,
    context_window: Option<usize>,
}

/// Resolve where this turn runs: the local engine, a cloud provider (CLD-3), or
/// a user's own connected server. `Err` carries a message meant for the user.
async fn resolve_turn(
    mgr: &RuntimeManager,
    db: &Db,
    target: &ChatTarget,
) -> Result<TurnSetup, String> {
    let is_remote = matches!(target.provenance.as_deref(), Some("cloud") | Some("endpoint"));
    let endpoint = match build_remote_endpoint(db, target)? {
        Some(ep) => ep,
        None => {
            let (base_url, token) = mgr
                .engine_endpoint()
                .await
                .ok_or("No model is loaded yet. Pick a model to get started.")?;
            ChatEndpoint::OpenAi { base_url, api_key: Some(token), model: None }
        }
    };

    // Key for per-model tool reliability stats (GRM-4): the cloud/endpoint
    // model id, or the running local model's file stem.
    let model_name = if is_remote {
        target.model.clone().unwrap_or_else(|| "cloud".to_string())
    } else {
        mgr.engine_model_name().await.unwrap_or_else(|| "local".to_string())
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

    // `OBS-3`: how much this model can hold, when that is knowable — the loaded
    // engine says so directly, a cloud model only if it is in the small price
    // table. Unknown leaves the meter without a percentage rather than with a
    // made-up one.
    let context_window = if is_remote {
        target
            .model
            .as_deref()
            .and_then(crate::cloud::pricing::facts_for)
            .map(|f| f.context_window)
    } else {
        mgr.engine_ctx_size().await.map(|n| n as usize)
    };

    Ok(TurnSetup { endpoint, local_endpoint, model_name, context_window })
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
    fleet: State<'_, Fleet>,
    conversation_id: String,
    assistant_message_id: Option<String>,
    messages: Vec<TurnMessage>,
    temperature: Option<f32>,
    tools_enabled: Option<bool>,
    target: Option<ChatTarget>,
    on_event: Channel<AgentEvent>,
) -> Result<(), PoiesisError> {
    let target = target.unwrap_or_default();
    let setup = match resolve_turn(&mgr, &db, &target).await {
        Ok(setup) => setup,
        Err(message) => {
            let _ = on_event.send(AgentEvent::Error { message });
            return Ok(());
        }
    };

    let mut msgs: Vec<serde_json::Value> = messages
        .into_iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    // `CTX-3`: the folder and Canvas briefs live in `agent::context` now, so
    // every way a run can start gets them, not just this one.
    crate::agent::context::insert_briefs(
        &db,
        &conversation_id,
        tools_enabled.unwrap_or(false),
        false,
        &mut msgs,
    );

    execute_turn(
        &mgr, &embed_mgr, &rerank_mgr, &db, &perms, &memory, &browser_pool, &fleet, &setup,
        &target, &conversation_id, assistant_message_id.as_deref(), msgs,
        temperature.unwrap_or(0.7), tools_enabled.unwrap_or(false), None, on_event,
    )
    .await;
    Ok(())
}

/// Start a run and see it through: register it with the fleet so Stop and
/// steering can address it by id, run the loop, then close it.
///
/// Both the ordinary turn and `resume_run_cmd` end here, which is the point —
/// a resumed run must be the same kind of run in every way that the fleet, the
/// permission prompts and the usage ledger can see. The only difference between
/// them is the message array, and that difference is settled before this call.
#[allow(clippy::too_many_arguments)]
async fn execute_turn(
    mgr: &RuntimeManager,
    embed_mgr: &EmbedManager,
    rerank_mgr: &RerankManager,
    db: &Db,
    perms: &PermissionManager,
    memory: &MemoryStore,
    browser_pool: &BrowserPool,
    fleet: &Fleet,
    setup: &TurnSetup,
    target: &ChatTarget,
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    msgs: Vec<serde_json::Value>,
    temperature: f32,
    tools_enabled: bool,
    // `PLN-T4`: the plan a resumed run is continuing. `None` for a fresh turn,
    // which starts with no plan and writes one if it wants one.
    plan: Option<crate::agent::plan::Plan>,
    on_event: Channel<AgentEvent>,
) {
    let cancel = mgr.new_cancel();
    let run = fleet.open(conversation_id, cancel, None, 0);
    let limits = RunLimits::top(db);
    let provenance = target.provenance.as_deref().unwrap_or("local");
    let rc = RunContext::top(&run, &limits, Some(fleet), provenance, setup.context_window)
        .resuming(plan);
    let sink = AgentEventSink::new(on_event);
    let images_dir = mgr.generated_media_dir();
    run_agent(
        &mgr.client,
        &setup.endpoint,
        setup.local_endpoint.as_ref(),
        db,
        mgr,
        embed_mgr,
        rerank_mgr,
        perms,
        memory,
        Some(browser_pool),
        conversation_id,
        assistant_message_id,
        &images_dir,
        &setup.model_name,
        msgs,
        temperature,
        tools_enabled,
        // Every caller of this command is an interactive turn — headless runs
        // go through `scheduler::run_job` instead, which calls `run_agent`
        // directly with `headless: true`.
        false,
        &rc,
        &sink,
    )
    .await;
    fleet.close(&run.id);
}

/// `HRN-UI-5`: pick an interrupted run back up with its work intact.
///
/// The transcript comes out of the session log (`CTX-2`) rather than being
/// rebuilt from the visible messages, and that is the whole point: the visible
/// messages have the prose, but the log has the tool results — the part that
/// cost minutes and money and that starting over would spend again.
///
/// `Ok(false)` means there was nothing to resume. That is not an error: a run
/// that finished cleanly, or one from before this log existed, simply has
/// nothing to continue, and the caller turns it into a quiet absence of the
/// button rather than a failure.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn resume_run_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    rerank_mgr: State<'_, RerankManager>,
    db: State<'_, Db>,
    perms: State<'_, PermissionManager>,
    memory: State<'_, MemoryStore>,
    browser_pool: State<'_, BrowserPool>,
    fleet: State<'_, Fleet>,
    conversation_id: String,
    assistant_message_id: Option<String>,
    temperature: Option<f32>,
    tools_enabled: Option<bool>,
    target: Option<ChatTarget>,
    on_event: Channel<AgentEvent>,
) -> Result<bool, PoiesisError> {
    let Some((run_id, _reason)) = db
        .last_logged_run(&conversation_id)
        .map_err(|e| PoiesisError::Message(e.to_string()))?
    else {
        return Ok(false);
    };
    let mut msgs = crate::agent::log::replay(&db, &run_id);
    if msgs.is_empty() {
        return Ok(false);
    }
    msgs.push(serde_json::json!({
        "role": "system",
        "content": crate::agent::log::RESUME_PROMPT,
    }));

    let target = target.unwrap_or_default();
    let setup = match resolve_turn(&mgr, &db, &target).await {
        Ok(setup) => setup,
        Err(message) => {
            let _ = on_event.send(AgentEvent::Error { message });
            return Ok(false);
        }
    };
    execute_turn(
        &mgr, &embed_mgr, &rerank_mgr, &db, &perms, &memory, &browser_pool, &fleet, &setup,
        &target, &conversation_id, assistant_message_id.as_deref(), msgs,
        temperature.unwrap_or(0.7), tools_enabled.unwrap_or(false),
        // `PLN-T4`: the plan is not in the replayed messages — a `plan` row is
        // state about the run, not a turn in it — so it is handed over
        // separately. Without this a resumed run would show its plan vanish at
        // the moment it was picked back up, and write a second one.
        crate::agent::log::last_plan(&db, &conversation_id),
        on_event,
    )
    .await;
    Ok(true)
}

/// `HRN-UI-5`: "Try again from here". Branches the conversation just before one
/// assistant turn and hands back the user turn that prompted it, for the caller
/// to send into the fork.
#[tauri::command]
pub fn fork_conversation_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    message_id: String,
) -> Result<ForkedConversation, PoiesisError> {
    let (conversation, resend) = db
        .fork_conversation(&conversation_id, &message_id)
        .map_err(|e| PoiesisError::Message(e.to_string()))?;
    Ok(ForkedConversation { conversation, resend })
}

/// The new branch, and the question to ask it again.
#[derive(serde::Serialize)]
pub struct ForkedConversation {
    pub conversation: crate::db::Conversation,
    /// `None` when the fork point had no user turn before it — an empty branch
    /// rather than a rerun of nothing.
    pub resend: Option<String>,
}

/// `HRN-2`: say something to a run that is already working. The text lands in
/// the run's inbox and is picked up at the top of its next iteration — not
/// mid-tool-call, where the transcript is not in a valid state.
#[tauri::command]
pub fn steer_run_cmd(
    fleet: State<'_, Fleet>,
    run_id: String,
    text: String,
) -> Result<bool, PoiesisError> {
    let Some(run) = fleet.get(&run_id) else {
        // The run finished between the keystroke and the send. Not an error:
        // the caller turns this into an ordinary next-turn message.
        return Ok(false);
    };
    run.steer(Steer::user(text));
    Ok(true)
}

/// `HRN-UI-4`: write a kept result into the conversation's working folder.
///
/// The text comes from the caller rather than from the store on disk: the run
/// that produced it may be long gone, and the UI has held the whole thing since
/// the `kept_result` event either way. The folder is the one the user already
/// attached, so this needs no fresh consent — and it refuses outright without
/// one, rather than picking somewhere itself.
#[tauri::command]
pub fn save_kept_result_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    reference: String,
    text: String,
) -> Result<String, PoiesisError> {
    let folder = db
        .conversation_folder(&conversation_id)
        .map_err(|e| PoiesisError::Message(e.to_string()))?
        .0
        .ok_or_else(|| PoiesisError::Message("This chat has no working folder to save into.".into()))?;
    // The reference is ours, not the model's, but it still becomes a filename,
    // so nothing but its own alphabet gets through.
    let safe: String = reference
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if safe.is_empty() {
        return Err(PoiesisError::Message("That result has no name to save under.".into()));
    }
    let path = std::path::Path::new(&folder).join(format!("{safe}.txt"));
    std::fs::write(&path, text).map_err(|e| PoiesisError::Message(e.to_string()))?;
    Ok(path.to_string_lossy().into_owned())
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
                .ok_or("That model server is no longer connected. Add it again in Settings → Runtime → Your servers.")?;
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
            "No API key for {}. Connect it in Settings → Providers to use its models.",
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

/// `ART-5`: hand the console output of an artifact's live preview to the agent
/// side, so `read_artifact` can return it. The preview is a sandboxed iframe
/// with no bridge of its own — the frontend collects what it posts up and
/// forwards it here.
#[tauri::command]
pub fn record_artifact_console_cmd(artifact_id: String, entries: Vec<ConsoleEntry>) {
    crate::agent::artifacts::record_console(&artifact_id, entries);
}

/// `ART-6`: where the Canvas should point an html artifact's preview —
/// `http://127.0.0.1:<port>/<token>`, with the artifact's id appended. `None`
/// when the loopback server didn't start, in which case the Canvas falls back
/// to rendering the source inline.
#[tauri::command]
pub fn preview_base_url_cmd() -> Option<String> {
    crate::agent::preview::base_url().map(|s| s.to_string())
}

/// Drop what a preview printed — it reloaded, so those lines describe a run
/// that no longer exists.
#[tauri::command]
pub fn clear_artifact_console_cmd(artifact_id: String) {
    crate::agent::artifacts::clear_console(&artifact_id);
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
