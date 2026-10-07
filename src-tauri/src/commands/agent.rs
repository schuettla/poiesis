//! Agent loop + permission commands (Phase 4).

use tauri::ipc::Channel;
use tauri::State;

use crate::agent::artifacts::ConsoleEntry;
use crate::agent::browser::BrowserPool;
use crate::agent::fleet::{Fleet, RunLimits, Steer};
use crate::agent::run::{run_agent, AgentEventSink, RunContext, RunOptions};
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
    run_options: Option<RunOptions>,
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
        temperature.unwrap_or(0.7), tools_enabled.unwrap_or(false), None,
        run_options.unwrap_or_default(), on_event,
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
    // `REG-5`: what the user decided about this turn only.
    options: RunOptions,
    on_event: Channel<AgentEvent>,
) {
    // `PLF-4`: a Go ahead starts from the plan the user approved, with nothing
    // restricted. Settled here, once, so a stray chip can never loop approval.
    let (plan, options) = options.settle_approval(plan);
    let cancel = mgr.new_cancel();
    let run = fleet.open(conversation_id, cancel, None, 0);
    let limits = RunLimits::top(db).with_max_steps(options.max_steps);
    let provenance = target.provenance.as_deref().unwrap_or("local");
    let rc = RunContext::top(&run, &limits, Some(fleet), provenance, setup.context_window)
        .resuming(plan)
        .with_options(options);
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
    run_options: Option<RunOptions>,
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
        run_options.unwrap_or_default(),
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
    // `UCM-2`: `/fork` keeps the boundary message; "Try again" does not.
    inclusive: Option<bool>,
) -> Result<ForkedConversation, PoiesisError> {
    let (conversation, resend) = db
        .fork_conversation_at(&conversation_id, &message_id, inclusive.unwrap_or(false))
        .map_err(|e| PoiesisError::Message(e.to_string()))?;
    Ok(ForkedConversation { conversation, resend })
}

/// `BTW-1`: the one side question that may be in flight. A new one cancels the
/// last, so the card on screen is always the one being answered.
#[derive(Default)]
pub struct SideQuestion(std::sync::Mutex<Option<crate::runtime::proxy::CancelFlag>>);

/// What a side question sends: the conversation as the model has it (the summary
/// included, and as much recent history as fits), then the question. No tools,
/// and nothing about it is written back anywhere.
fn side_messages(
    system: &str,
    summary: Option<&str>,
    history: &[crate::db::Message],
    question: &str,
    window: Option<usize>,
) -> Vec<serde_json::Value> {
    let system = format!(
        "{}\n\nThe user is asking a quick question on the side. Answer it briefly and on its own, from what you already know in this conversation. Do not continue the main task.",
        crate::agent::context::with_summary(system, summary.unwrap_or(""))
    );
    let prior: Vec<serde_json::Value> = history
        .iter()
        .filter(|m| (m.role == "user" || m.role == "assistant") && !m.content.trim().is_empty())
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();
    let current = serde_json::json!({ "role": "user", "content": question });
    crate::agent::context::budget_turns(
        &system,
        &prior,
        &current,
        window.unwrap_or(8192),
        crate::agent::context::KEEP_RECENT,
    )
    .turns
}

/// `BTW-1`: ask something on the side. The answer streams back as `Token`s and
/// nothing is persisted, logged or traced: it never touches a live run, and it
/// is not in the conversation unless the user keeps it. Always tool-free.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn side_question_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    db: State<'_, Db>,
    memory: State<'_, MemoryStore>,
    side: State<'_, SideQuestion>,
    conversation_id: String,
    question: String,
    target: Option<ChatTarget>,
    on_event: Channel<AgentEvent>,
) -> Result<(), PoiesisError> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err(PoiesisError::Message("Ask me something to answer.".into()));
    }
    let target = target.unwrap_or_default();
    let setup = match resolve_turn(&mgr, &db, &target).await {
        Ok(setup) => setup,
        Err(message) => {
            let _ = on_event.send(AgentEvent::Error { message });
            return Ok(());
        }
    };

    let cancel = crate::runtime::proxy::CancelFlag::new();
    if let Ok(mut slot) = side.0.lock() {
        if let Some(previous) = slot.replace(cancel.clone()) {
            previous.cancel();
        }
    }

    let system = crate::agent::context::compose_system_prompt(
        &crate::agent::context::gather(
            &db,
            &memory,
            &mgr,
            &embed_mgr,
            crate::agent::context::GatherOpts {
                conversation_id: &conversation_id,
                tools_enabled: false,
                model_name: &setup.model_name,
                query: &question,
            },
        )
        .await,
    );
    let summary = db.get_conversation(&conversation_id).ok().flatten().and_then(|c| c.summary);
    let history = db.list_messages_window(&conversation_id, 24).unwrap_or_default();
    let messages = side_messages(&system, summary.as_deref(), &history, &question, setup.context_window);

    let result = cloud::drive_turn(
        &mgr.client,
        &setup.endpoint,
        &messages,
        &[],
        0.7,
        cloud::Effort::Low,
        &cancel,
        |delta| {
            if let crate::runtime::proxy::Delta::Answer(text) = delta {
                let _ = on_event.send(AgentEvent::Token { text: text.to_string() });
            }
        },
    )
    .await;

    // A newer question cancels this one when it takes the slot, so an uncancelled
    // flag means the slot is still ours to clear.
    if !cancel.is_cancelled() {
        if let Ok(mut slot) = side.0.lock() {
            slot.take();
        }
    }
    let _ = match result {
        _ if cancel.is_cancelled() => on_event.send(AgentEvent::Cancelled),
        Ok(_) => on_event.send(AgentEvent::Done),
        Err(e) => on_event.send(AgentEvent::Error { message: e.provider_message().to_string() }),
    };
    Ok(())
}

/// `BTW-UI-1`: Dismiss. Stops the answer that is streaming, if any.
#[tauri::command]
pub fn cancel_side_question_cmd(side: State<'_, SideQuestion>) {
    if let Ok(mut slot) = side.0.lock() {
        if let Some(cancel) = slot.take() {
            cancel.cancel();
        }
    }
}

/// `GOL-2`: has the goal been reached? One tool-free completion over the goal,
/// my last answer and what I changed. It judges and nothing else: it does not
/// start a turn, and the answer is only ever a hint to the loop in the store.
#[tauri::command]
pub async fn goal_check_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    conversation_id: String,
    objective: String,
    until: String,
    target: Option<ChatTarget>,
) -> Result<crate::agent::goal::GoalCheck, PoiesisError> {
    use crate::agent::goal;
    let target = target.unwrap_or_default();
    let setup = resolve_turn(&mgr, &db, &target).await.map_err(PoiesisError::Message)?;
    let last_answer = db
        .list_messages_window(&conversation_id, 8)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .find(|m| m.role == "assistant" && !m.content.trim().is_empty())
        .map(|m| m.content)
        .unwrap_or_default();
    let since = crate::agent::changes::view_since(&db, &conversation_id);
    let changes = goal::changes_line(&crate::agent::changes::change_set(&db, &conversation_id, since));
    let messages = goal::check_messages(&objective, &until, &last_answer, &changes);

    let outcome = cloud::drive_turn(
        &mgr.client,
        &setup.endpoint,
        &messages,
        &[],
        0.0,
        cloud::Effort::Off,
        &crate::runtime::proxy::CancelFlag::new(),
        |_| {},
    )
    .await
    .map_err(|e| PoiesisError::Message(e.provider_message().to_string()))?;
    match outcome {
        crate::runtime::proxy::TurnOutcome::Final { content, .. } => Ok(goal::parse_check(&content)),
        // A judge that asked for a tool instead of answering has not answered.
        _ => Ok(goal::parse_check("")),
    }
}

/// `RWD-1`: go back to before one of the user's turns. A branch plus, when
/// asked, a file undo; the original conversation is never edited.
#[tauri::command]
pub fn rewind_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    message_id: String,
    undo_files: bool,
) -> Result<crate::agent::rewind::Rewound, PoiesisError> {
    crate::agent::rewind::rewind(&db, &conversation_id, &message_id, undo_files)
        .map_err(PoiesisError::Message)
}

/// `RWD-2`: how many files a rewind to before this turn would put back.
#[tauri::command]
pub fn changes_since_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    message_id: String,
) -> Result<usize, PoiesisError> {
    crate::agent::rewind::files_since(&db, &conversation_id, &message_id)
        .map_err(PoiesisError::Message)
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

/// `AGC-3`: the user's answer to a question a run is waiting on. `false` when
/// the run is gone or the question was already answered, which the caller turns
/// into an ordinary message rather than losing the words.
#[tauri::command]
pub fn answer_question_cmd(
    fleet: State<'_, Fleet>,
    run_id: String,
    question_id: String,
    answer: crate::agent::fleet::Answer,
) -> Result<bool, PoiesisError> {
    Ok(fleet.get(&run_id).is_some_and(|run| run.answer(&question_id, answer)))
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

/// `REG-4`: a command the user ran that changed something. Written between runs,
/// so it carries no run id, and never reaches the model (`replay` skips it).
#[tauri::command]
pub fn record_command_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    trace: crate::agent::log::CommandTrace,
) -> Result<(), PoiesisError> {
    crate::agent::log::record_command(&db, &conversation_id, None, &trace);
    Ok(())
}

/// `REG-4`: the commands a conversation has traced, oldest first, so a reload
/// shows the same notes the session did.
#[tauri::command]
pub fn conversation_commands_cmd(
    db: State<'_, Db>,
    conversation_id: String,
) -> Result<Vec<crate::agent::log::CommandRecord>, PoiesisError> {
    Ok(crate::agent::log::commands(&db, &conversation_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Message;

    fn m(role: &str, text: &str) -> Message {
        Message {
            id: format!("m-{text}"),
            conversation_id: "c".into(),
            role: role.into(),
            content: text.into(),
            model_name: None,
            model_provenance: None,
            steps_json: None,
            stop_reason: None,
            plan_json: None,
            spoken: false,
            created_at: 0,
            attachments: Vec::new(),
        }
    }

    /// `BTW-1`: the conversation as the model has it, then the question last; the
    /// summary rides in the system prompt; empty turns and the loop's own
    /// bookkeeping roles never go in.
    #[test]
    fn a_side_question_carries_the_conversation_and_ends_with_the_question() {
        let history = vec![m("user", "plan the trip"), m("assistant", "ok, Lisbon?"), m("assistant", "  "), m("tool", "x")];
        let out = side_messages("SYSTEM", Some("they like trains"), &history, "what was the city?", Some(8000));
        assert_eq!(out.first().unwrap()["role"], "system");
        let system = out[0]["content"].as_str().unwrap();
        assert!(system.starts_with("SYSTEM"));
        assert!(system.contains("they like trains"), "the summary is in what the model reads");
        assert!(system.contains("on the side"), "it is told this is a side question");
        let last = out.last().unwrap();
        assert_eq!((last["role"].as_str(), last["content"].as_str()), (Some("user"), Some("what was the city?")));
        let roles: Vec<&str> = out.iter().map(|t| t["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"], "no blank turn, no tool message");
    }
}
