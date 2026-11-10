//! Agent loop + permission commands (Phase 4).

use tauri::ipc::Channel;
use tauri::State;

use crate::agent::artifacts::ConsoleEntry;
use crate::agent::browser::BrowserPool;
use crate::agent::fleet::{Fleet, RunLimits, RunOutcome, Steer};
use crate::agent::tier::{self, ModelTier};
use crate::commands::files::DialogGrants;
use crate::agent::run::{run_agent, AgentEventSink, RunContext, RunOptions};
use crate::agent::toolsets::{self, Toolset, ToolsetInfo};
use crate::agent::AgentEvent;
use crate::cloud::{self, endpoints, ChatEndpoint, Provider};
use crate::db::Db;
use crate::memory::MemoryStore;
use crate::permissions::{Decision, PermissionManager};
use crate::runtime::{EmbedManager, RerankManager, RuntimeManager};
use crate::PoiesisError;

/// Which model a turn should run against (CLD-3 routing). Defaults to the local
/// engine when absent.
#[derive(serde::Deserialize, Default, Clone, Debug)]
pub struct ChatTarget {
    pub provenance: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// Everything a turn needs to know about *where* it runs, resolved once. Shared
/// by the ordinary turn and by resume, so a resumed run cannot quietly land on
/// a different model than the run it is continuing.
pub(crate) struct TurnSetup {
    pub(crate) endpoint: ChatEndpoint,
    local_endpoint: Option<ChatEndpoint>,
    pub(crate) model_name: String,
    pub(crate) context_window: Option<usize>,
    /// `TIER-3`: what this model can carry. Decided once here, so a typed turn, a
    /// resume and a scheduled run cannot disagree about it.
    pub(crate) tier: ModelTier,
}

/// The long-lived handles a turn runs with, borrowed once. `execute_turn` and
/// the Rust turn entry points (`agent::turn`) take this instead of nine
/// separate parameters each.
pub struct TurnEnv<'a> {
    pub mgr: &'a RuntimeManager,
    pub embed_mgr: &'a EmbedManager,
    pub rerank_mgr: &'a RerankManager,
    pub db: &'a Db,
    pub perms: &'a PermissionManager,
    pub memory: &'a MemoryStore,
    pub browser_pool: &'a BrowserPool,
    pub fleet: &'a Fleet,
    pub grants: &'a DialogGrants,
}

/// Resolve where this turn runs: the local engine, a cloud provider (CLD-3), or
/// a user's own connected server. `Err` carries a message meant for the user.
pub(crate) async fn resolve_turn(
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
            // Not in the price table: what the provider's catalog said, or what the
            // person told us when they added their own server. The meter and a
            // fresh start (`WRK-5`) both need to know how much a head holds.
            .or_else(|| declared_window(db, target))
    } else {
        mgr.engine_ctx_size().await.map(|n| n as usize)
    };

    let tier = tier_of(mgr, db, target).await;
    Ok(TurnSetup { endpoint, local_endpoint, model_name, context_window, tier })
}

/// The window a cloud model's catalog entry or the person's own server declares.
fn declared_window(db: &Db, target: &ChatTarget) -> Option<usize> {
    let provider = target.provider.as_deref().unwrap_or_default();
    match target.provenance.as_deref() {
        Some("cloud") => cloud::tier_facts(provider, target.model.as_deref().unwrap_or_default()).and_then(|f| f.window),
        Some("endpoint") => db
            .get_local_endpoint(provider)
            .ok()
            .flatten()
            .and_then(|ep| usize::try_from(ep.ctx_size).ok())
            .filter(|n| *n > 0),
        _ => None,
    }
}

/// `TIER-3`: the tier a turn on this target runs at, from the facts of its
/// provenance. The user's own choice for the model beats the rule.
///
/// - local: tools if the model's chat template has a place for them, and the
///   window is the one the running engine was started with;
/// - cloud: what the provider's catalog said when the picker last read it, else
///   the price table's window, else unknown;
/// - endpoint: the window the user gave the server when they added it.
pub(crate) async fn tier_of(mgr: &RuntimeManager, db: &Db, target: &ChatTarget) -> ModelTier {
    let provider = target.provider.as_deref().unwrap_or_default();
    let model = target.model.as_deref().unwrap_or_default();
    let (id, tools, window) = match target.provenance.as_deref() {
        Some("cloud") => {
            let facts = cloud::tier_facts(provider, model);
            (tier::cloud_model_id(provider, model), facts.is_none_or(|f| f.tools), facts.and_then(|f| f.window))
        }
        Some("endpoint") => {
            let window = db
                .get_local_endpoint(provider)
                .ok()
                .flatten()
                .and_then(|ep| usize::try_from(ep.ctx_size).ok())
                .filter(|n| *n > 0);
            (tier::endpoint_model_id(provider, model), true, window)
        }
        _ => {
            let window = mgr.engine_ctx_size().await.map(|n| n as usize);
            let path = mgr.engine_model_path().await;
            let id = path.as_ref().and_then(|p| db.find_model_by_path(&p.to_string_lossy()).ok().flatten()).map(|e| e.id);
            let tools = crate::runtime::gguf::takes_tools(path.and_then(|p| crate::runtime::gguf::read_header(&p)).as_ref());
            (id.unwrap_or_default(), tools, window)
        }
    };
    tier::tier_for(tools, window, tier::tier_override(db, &id))
}

/// Start a run and see it through: register it with the fleet so Stop and
/// steering can address it by id, run the loop, then close it.
///
/// Both the ordinary turn and `resume_run_cmd` end here, which is the point —
/// a resumed run must be the same kind of run in every way that the fleet, the
/// permission prompts and the usage ledger can see. The only difference between
/// them is the message array, and that difference is settled before this call.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_turn(
    env: &TurnEnv<'_>,
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
    sink: &AgentEventSink,
) -> RunOutcome {
    // `PLF-4`: a Go ahead starts from the plan the user approved, with nothing
    // restricted. Settled here, once, so a stray chip can never loop approval.
    let (plan, options) = options.settle_approval(plan);
    let cancel = env.mgr.new_cancel();
    let run = env.fleet.open(conversation_id, cancel, None, 0);
    let limits = RunLimits::top(env.db).with_max_steps(options.max_steps);
    let provenance = target.provenance.as_deref().unwrap_or("local");
    let rc = RunContext::top(&run, &limits, Some(env.fleet), provenance, setup.context_window)
        .resuming(plan)
        .with_options(options)
        .with_tier(setup.tier);
    let images_dir = env.mgr.generated_media_dir();
    let outcome = run_agent(
        &env.mgr.client,
        &setup.endpoint,
        setup.local_endpoint.as_ref(),
        env.db,
        env.mgr,
        env.embed_mgr,
        env.rerank_mgr,
        env.perms,
        env.memory,
        Some(env.browser_pool),
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
        sink,
    )
    .await;
    env.fleet.close(&run.id);
    outcome
}

/// `HRN-UI-5`: pick an interrupted run back up with its work intact. The work is
/// `agent::turn::resume_turn`; this hands it the window's choices.
///
/// `Ok(false)` means there was nothing to resume. That is not an error: a run
/// that finished cleanly, or one from before the session log existed, has nothing
/// to continue. The answer row says so itself.
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
    grants: State<'_, DialogGrants>,
    conversation_id: String,
    model: crate::agent::turn::TurnModel,
    tools_enabled: Option<bool>,
    target: Option<ChatTarget>,
    run_options: Option<RunOptions>,
    on_event: Channel<AgentEvent>,
) -> Result<bool, PoiesisError> {
    let env = TurnEnv {
        mgr: &mgr,
        embed_mgr: &embed_mgr,
        rerank_mgr: &rerank_mgr,
        db: &db,
        perms: &perms,
        memory: &memory,
        browser_pool: &browser_pool,
        fleet: &fleet,
        grants: &grants,
    };
    Ok(crate::agent::turn::resume_turn(
        &env,
        &conversation_id,
        &model,
        target.unwrap_or_default(),
        tools_enabled.unwrap_or(false),
        run_options.unwrap_or_default(),
        on_event,
    )
    .await)
}

/// `HRW-1`: start, run and save one user turn. The window sends what the user
/// typed and chose; the rows, the prompt and the run are Rust's.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn run_turn_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    rerank_mgr: State<'_, RerankManager>,
    db: State<'_, Db>,
    perms: State<'_, PermissionManager>,
    memory: State<'_, MemoryStore>,
    browser_pool: State<'_, BrowserPool>,
    fleet: State<'_, Fleet>,
    grants: State<'_, DialogGrants>,
    input: crate::agent::turn::TurnInput,
    on_event: Channel<AgentEvent>,
) -> Result<(), PoiesisError> {
    let env = TurnEnv {
        mgr: &mgr,
        embed_mgr: &embed_mgr,
        rerank_mgr: &rerank_mgr,
        db: &db,
        perms: &perms,
        memory: &memory,
        browser_pool: &browser_pool,
        fleet: &fleet,
        grants: &grants,
    };
    crate::agent::turn::run_turn(&env, input, on_event).await;
    Ok(())
}

/// `HRW-1`: save a turn's two rows ahead of `run_turn_cmd`. The window does this
/// before it starts the engine, so the message is kept even when the start
/// fails, the app is closed during it, or the run never begins; `run_turn_cmd`
/// then gets the ids in `input.saved`.
#[tauri::command]
pub fn save_turn_cmd(
    db: State<'_, Db>,
    input: crate::agent::turn::TurnInput,
) -> Result<crate::agent::turn::SavedTurn, PoiesisError> {
    crate::agent::turn::save_turn(&db, &input).map_err(|e| PoiesisError::Message(e.to_string()))
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
                tier: setup.tier,
                model_name: &setup.model_name,
                query: &question,
                attended: true,
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

/// What became of a message sent to a running agent (`HRN-2`, `USE-5`).
#[derive(serde::Serialize)]
pub struct SteerOutcome {
    /// `queued`: the run will read it at its next step. `held`: the decision
    /// model is sure it is a stop, so it waits for the user's answer to "Stop the
    /// run?". `gone`: the run finished between the keystroke and the send.
    pub status: &'static str,
    /// What the decision model read the message as, when it was sure enough to say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<crate::agent::decide::loopctl::SteerRead>,
}

/// The request a conversation's run is working on: its latest user message,
/// clipped. The steering check reads a message against it ("stop" after "fix the
/// tests" is not the same as after "draft the mail").
pub(crate) fn current_task(db: &crate::db::Db, conversation_id: &str) -> String {
    db.list_messages_window(conversation_id, 8)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content)
        .unwrap_or_default()
}

/// `HRN-2`: say something to a run that is already working. The text lands in
/// the run's inbox and is picked up at the top of its next iteration — not
/// mid-tool-call, where the transcript is not in a valid state.
///
/// `USE-5`: first the decision model reads the message as a stop, a redirect, an
/// addition or a question. The class goes back with the answer for the screen to
/// tag; only a calibrated stop is held. `already_read` skips the read, for a
/// message the user has just answered "Keep going" to.
#[tauri::command]
pub async fn steer_run_cmd(
    fleet: State<'_, Fleet>,
    db: State<'_, crate::db::Db>,
    run_id: String,
    text: String,
    already_read: Option<bool>,
) -> Result<SteerOutcome, PoiesisError> {
    let gone = SteerOutcome { status: "gone", read: None };
    // The run may finish between the keystroke and the send. Not an error: the
    // caller turns this into an ordinary next-turn message.
    let Some(run) = fleet.get(&run_id) else { return Ok(gone) };
    let read = if already_read == Some(true) {
        None
    } else {
        let task = current_task(&db, &run.conversation_id);
        crate::agent::decide::loopctl::read_steer(&task, &text, &run.conversation_id, &run.id).await
    };
    if read.as_ref().is_some_and(|r| r.ask_stop) {
        return Ok(SteerOutcome { status: "held", read });
    }
    // The read can take a moment; the run may have ended in it.
    let Some(run) = fleet.get(&run_id) else { return Ok(gone) };
    run.steer(Steer::user(text));
    Ok(SteerOutcome { status: "queued", read })
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

/// `USE-UI-1`: the user clicked the "tools held back" chip. Those groups are on the
/// run's menu again from its next step, the same as if it had asked with
/// `need_tools`, and the decision is labelled as a miss. Returns how many came
/// back; `0` when the run is gone or nothing named was held.
#[tauri::command]
pub fn restore_tools_cmd(fleet: State<'_, Fleet>, run_id: String, units: Vec<String>) -> Result<usize, PoiesisError> {
    let Some(run) = fleet.get(&run_id) else { return Ok(0) };
    let back = run.held.lock().map(|mut held| held.restore(&units, crate::agent::decide::tools::Back::Chip));
    Ok(back.map_or(0, |b| b.len()))
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
