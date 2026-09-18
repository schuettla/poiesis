//! The agent loop (PRD §7.5): drive model turns, dispatch tool calls to built-in
//! toolsets, feed results back, and emit a visible step timeline — until the model
//! produces a final answer.

use std::collections::HashMap;

use tauri::ipc::Channel;

use crate::cloud::{drive_turn, ChatEndpoint, Effort};
use crate::db::Db;
use crate::mcp::McpClient;
use crate::permissions::{PermissionManager, PermissionRequest};
use crate::runtime::proxy::{CancelFlag, Delta, ProxyError, ToolCallReq, TurnOutcome};
use crate::runtime::{EmbedManager, RerankManager, RuntimeManager};
use crate::secrets::{self, SERVICE_MCP};

use super::fleet::{Fleet, RunHandle, RunLimits, RunOutcome, StopReason};
use super::toolsets::{self, Toolset, ToolContext};
use crate::memory::MemoryStore;
use super::AgentEvent;

/// What the model is told when its last turn arrives (`HRN-5`). The point is
/// that the run ends with an answer built from what it already found, instead
/// of an error that throws all of it away.
const WRAP_UP_PROMPT: &str = "You are out of room for tool calls. Answer now with what you already have. Say plainly what you did not get to.";

/// How many times one run will ask a model to make a tool call for real after it
/// merely described one. Two is enough to cover a slip; a model that genuinely
/// cannot emit tool calls would burn the whole iteration budget otherwise.
const MAX_NARRATION_NUDGES: usize = 2;

/// What the model is told when it finishes a run that did real work and says
/// nothing at all. Deliberately short and free of blame: the model has the
/// results above it and simply has to report them.
const ANSWER_NOW_PROMPT: &str = "Your last turn was empty. The tool results above are the work you already did. Write the answer now, using them. Do not call any more tools and do not start again.";

/// What a reasoning model is told when it spent the turn thinking and never
/// wrote a reply. It is not asked to think less — only to put the answer where
/// the reader can see it.
const FINISH_THINKING_PROMPT: &str = "You finished thinking but wrote no reply. Write the answer itself now, as ordinary text.";

/// How many times one run will ask for the answer after an empty turn.
///
/// One. The point is to rescue a run whose work is sitting in the transcript
/// unreported, not to argue with a model that has nothing to say. A second ask
/// costs another whole turn to learn what the first already established.
const MAX_EMPTY_RETRIES: usize = 1;

/// `PLN-2`: how many times one run will be told that it stopped with its own
/// plan unfinished.
///
/// One, for the same reason as above. This is not a gate — the plan never
/// authorises anything, and a model that means to stop may stop. It exists
/// because a model that writes six items, does one, and then answers leaves the
/// user reading a card that says `0 of 6 done` beside a finished reply, which
/// is the lying checklist this whole feature was built to avoid. One ask turns
/// that into either the rest of the work or an honest revision of the plan.
const MAX_PLAN_NUDGES: usize = 1;

/// `PLN-2`: how many turns a run may spend purely on keeping its plan current
/// without those turns counting against its step budget.
///
/// The step cap bounds *work* — calls that cost time, money, or leave a mark
/// outside the run. A `plan` update costs none of those: it writes nothing but
/// the run's own bookkeeping. Charging for it made the feature take from the
/// thing it was meant to make legible, and the arithmetic is brutal: a six-item
/// plan marked honestly (`doing` at the start of an item, `done` at the end) is
/// twelve calls, and a model that emits each on its own turn has spent the whole
/// default budget before doing anything at all. That is exactly how a run got to
/// item four of six and stopped.
///
/// Free, but not unbounded: a model that only ever calls `plan` would otherwise
/// loop until the heat death of the universe. Six is enough for an honest pass
/// over a plan of the size this feature is for, and a model that needs more than
/// six pure-bookkeeping turns is doing something other than working.
const MAX_FREE_PLAN_STEPS: usize = 6;

/// Where an MCP-provided tool lives, so a call can be routed to its server.
#[derive(Clone)]
struct McpBinding {
    connector_id: String,
    connector_name: String,
    /// HTTP endpoint URL, or (for stdio) the server command line.
    url: String,
    transport: String,
}

/// One live MCP client per connector, reused for the whole run (LOOP-1): the
/// `initialize` handshake (and, for stdio, the child process) happens once per
/// connector instead of once per tool call. Keyed by `connector_id`. Dropping
/// the pool at run end kills stdio children (`kill_on_drop`).
type McpPool = tokio::sync::Mutex<HashMap<String, McpClient>>;

/// Which autonomy class (AUT-1) governs a tool, if any. Tools not listed here
/// don't change the agent's own self and are never gated.
fn self_change_class(tool: &str) -> Option<&'static str> {
    match tool {
        "memory" => Some("facts"),
        "propose_soul_edit" => Some("soul"),
        "propose_skill" => Some("skills"),
        _ => None,
    }
}

/// The unified tool table for one run: the OpenAI specs advertised to the model,
/// the enabled built-in toolsets, plus the routing map for MCP tools.
struct ToolRegistry {
    specs: Vec<serde_json::Value>,
    toolsets: Vec<Toolset>,
    mcp: HashMap<String, McpBinding>,
    /// Names of the Agent Skills this conversation may load (`SKL-2`/`SKL-6`).
    ///
    /// Not tools — the only tool involved is `skill` — but the system prompt
    /// lists them by name, and a small model reads that list as a menu of things
    /// to call: a local Gemma answers with `content-research-writer:outline
    /// {…}`, naming the *skill* where the tool belongs. Keeping the names here
    /// lets the content-form parser recognise that for what it is and turn it
    /// into the `skill` call the model meant.
    skills: Vec<String>,
}

impl ToolRegistry {
    /// Build from every **enabled** built-in toolset (TOOL-6), narrowed by this
    /// conversation's persona allowlist if it has one (`PER-1`/`PER-2`), plus
    /// every enabled MCP connector's cached tools (MCP-4, §7.5 unified
    /// dispatch). Built-in tools win name collisions.
    ///
    /// `SUB-5`: `ceiling` is the parent's effective toolset list for a delegated
    /// child — a child is never more powerful than the run that started it, so
    /// its own persona allowlist can only narrow this further, never widen it.
    /// `may_delegate` false withdraws the `delegate` tool entirely, which is how
    /// depth is enforced before the model can even ask.
    fn build(
        db: &Db,
        mgr: &RuntimeManager,
        conversation_id: &str,
        ceiling: Option<&[Toolset]>,
        may_delegate: bool,
    ) -> Self {
        #[derive(serde::Deserialize, Default)]
        struct CachedConfig {
            #[serde(default)]
            tools: Vec<crate::mcp::McpTool>,
        }

        let persona = db
            .get_conversation(conversation_id)
            .ok()
            .flatten()
            .and_then(|c| c.persona_id)
            .and_then(|pid| db.get_persona(&pid).ok().flatten());
        let persona_tools = persona.as_ref().and_then(|p| p.tools_json.clone());
        // The same list the system prompt advertises (`SKL-2`), narrowed the
        // same way (`SKL-6`), so the parser recognises exactly the names the
        // model was told about.
        let skills = {
            let folder = db
                .conversation_folder(conversation_id)
                .ok()
                .and_then(|(f, _)| f)
                .map(std::path::PathBuf::from);
            let packs = super::skillpack::discover(mgr.app_data_dir(), folder.as_deref());
            let allow = persona.as_ref().and_then(|p| p.skills_json.clone());
            super::skillpack::enabled_names_for_persona(db, &packs, allow.as_deref())
        };
        let mut enabled = toolsets::enabled_for_persona(db, persona_tools.as_deref());
        if let Some(ceiling) = ceiling {
            enabled.retain(|s| ceiling.contains(s));
        }
        if !may_delegate {
            enabled.retain(|s| *s != Toolset::Subagents);
        }
        let mut specs: Vec<serde_json::Value> =
            enabled.iter().flat_map(|s| s.tool_specs()).collect();
        // AUT-1: a self-change class set to "off" withdraws its tool entirely —
        // the model is never offered a capability the user has closed off.
        specs.retain(|s| {
            s.pointer("/function/name")
                .and_then(|n| n.as_str())
                .and_then(self_change_class)
                .map(|class| crate::autonomy::autonomy_gate(db, class) != crate::autonomy::Rung::Off)
                .unwrap_or(true)
        });
        // `COD-7`/`COD-8`: a project that runs nothing is not offered a tool that
        // can only refuse, and `run_command` exists only where expert mode, the
        // Settings switch and the project's own opt-in all say so. Every call is
        // checked again when it arrives; this only keeps dead tools off the menu.
        if enabled.contains(&Toolset::CodeRun) {
            let (tasks, commands) = super::coderun::advertised(db, conversation_id);
            specs.retain(|s| match s.pointer("/function/name").and_then(|n| n.as_str()) {
                Some("run_task") => tasks,
                Some("run_command") => commands,
                _ => true,
            });
        }
        // `RPC-3`: teach the loop-over-many-items shape, but only while the
        // ability is actually switched on. A model will not discover it on its
        // own, and describing a capability that is off costs a wasted step to
        // find that out.
        if super::toolrpc::is_enabled(db) {
            for spec in specs.iter_mut() {
                let is_run_code = spec
                    .pointer("/function/name")
                    .and_then(|n| n.as_str())
                    .is_some_and(|n| n == "run_code");
                if !is_run_code {
                    continue;
                }
                if let Some(d) = spec.pointer_mut("/function/description").and_then(|d| d.as_str().map(str::to_string)) {
                    spec["function"]["description"] =
                        serde_json::Value::String(d + super::toolrpc::TOOL_GUIDANCE);
                }
            }
        }
        let mut taken: std::collections::HashSet<String> = specs
            .iter()
            .filter_map(|s| s.pointer("/function/name").and_then(|n| n.as_str()))
            .map(|s| s.to_string())
            .collect();
        let mut mcp = HashMap::new();

        if let Ok(connectors) = db.list_connectors() {
            for c in connectors.into_iter().filter(|c| c.enabled) {
                let Some(url) = c.url.clone() else { continue };
                let cached: CachedConfig = c
                    .config_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_default();
                for tool in cached.tools {
                    if taken.contains(&tool.name) {
                        continue; // don't shadow a built-in or earlier connector
                    }
                    taken.insert(tool.name.clone());
                    specs.push(tool.to_openai_spec());
                    mcp.insert(
                        tool.name.clone(),
                        McpBinding {
                            connector_id: c.id.clone(),
                            connector_name: c.name.clone(),
                            url: url.clone(),
                            transport: c.transport.clone(),
                        },
                    );
                }
            }
        }

        ToolRegistry { specs, toolsets: enabled, mcp, skills }
    }

    /// The enabled built-in toolset that owns `name`, if any.
    fn builtin_for(&self, name: &str) -> Option<Toolset> {
        self.toolsets.iter().copied().find(|s| s.handles(name))
    }

    /// Every advertised tool name — used by the early-flush guard (LOOP-4) to
    /// keep buffering anything that might still turn out to be a tool call.
    fn tool_names(&self) -> Vec<String> {
        self.specs
            .iter()
            .filter_map(|s| s.pointer("/function/name").and_then(|n| n.as_str()))
            .map(str::to_string)
            .collect()
    }

    /// Every name a model might plausibly try to *invoke* — the tools, plus the
    /// skills the prompt named. Used where the question is "does this text look
    /// like an attempt at a call", as opposed to `tool_names`, which answers
    /// "is this a name we can dispatch".
    fn invocable_names(&self) -> Vec<String> {
        let mut names = self.tool_names();
        names.extend(self.skills.iter().cloned());
        names
    }

    /// The skill this token names, if any. Tolerates the `<skill>:<verb>` shape
    /// a model invents when it reads the prompt's skill list as a call menu —
    /// there is no verb to honour, since loading a skill *is* the whole act.
    fn skill_named(&self, token: &str) -> Option<&str> {
        // Only if the `skill` tool is actually on offer — a user who switched
        // the Skills toolset off must not get skills loaded by the back door.
        self.builtin_for("skill")?;
        let head = token.split(':').next().unwrap_or(token);
        self.skills.iter().find(|s| s.as_str() == head).map(String::as_str)
    }
}

/// Least tokens after which a still-unclassified buffer is judged prose (LOOP-4).
const EARLY_FLUSH_CHARS: usize = 160;

/// LOOP-4: may we start streaming this partial turn to the user live?
///
/// Deliberately dumb and biased toward buffering: a false *buffer* just restores
/// the old behavior (prose appears at end of turn), while a false *flush* leaks
/// raw tool-call JSON into the conversation — much worse. So anything that could
/// still become a tool call — a JSON/array opener, a code fence, a `<think>`
/// preamble, or any text mentioning a known tool name — keeps buffering.
fn should_flush_prose(buf: &str, tool_names: &[String]) -> bool {
    let t = buf.trim_start();
    let Some(first) = t.chars().next() else { return false };
    if matches!(first, '{' | '[' | '`' | '<') {
        return false;
    }
    if tool_names.iter().any(|n| t.contains(n.as_str())) {
        return false;
    }
    // Opening on a letter is the clearest prose signal; otherwise give the turn
    // some room to reveal itself before committing.
    first.is_alphabetic() || t.chars().count() >= EARLY_FLUSH_CHARS
}

/// `LOOP-4`'s missing half: how much of a live-streaming turn is safe to show
/// *right now*, and how much must be held back.
///
/// [`should_flush_prose`] only ever decides once, on the turn's opening chars —
/// after which `streaming_live` latches on for the rest of the turn. A model
/// that opens in prose and *then* writes a tool call therefore streamed the call
/// straight to the user and ended the run, which is exactly what a local Gemma
/// does:
///
/// prose, then a fence tagged `tool` whose one line is
/// `skill content-research-writer`.
///
/// So a fence that looks like it holds a call is withheld from the opener to the
/// end of the turn — *including* after it closes, since a closed fence is exactly
/// when we can finally tell. The end-of-turn path then either executes it (and it
/// is never shown) or emits it verbatim, so an ordinary code block is only
/// delayed, never lost. Fences that can't be calls keep streaming live.
///
/// Returns the byte offset up to which `buf` may be emitted.
fn safe_prefix(buf: &str, tool_names: &[String]) -> usize {
    let mut cut = buf.len();
    let mut at = 0;
    while let Some(rel) = buf[at..].find("```") {
        let open = at + rel;
        let after = open + 3;
        let (block, next) = match buf[after..].find("```") {
            Some(rel_close) => (&buf[after..after + rel_close], after + rel_close + 3),
            None => (&buf[after..], buf.len()),
        };
        if fence_could_be_a_call(block, tool_names) {
            cut = open;
            break;
        }
        at = next;
    }
    // A line that starts a bare JSON object is the other shape a tool call
    // arrives in mid-prose. Hold from the start of that line.
    let head = &buf[..cut];
    for (i, _) in head.match_indices('{') {
        let line_start = head[..i].rfind('\n').map(|n| n + 1).unwrap_or(0);
        if head[line_start..i].trim().is_empty() {
            return line_start;
        }
    }
    cut
}

/// Could this fenced block (its info string plus whatever body has arrived) be a
/// tool call? Kept generous on the info string and strict on the body: an
/// advertised tool name has to actually appear, so a Python or SQL block streams
/// live as it always did.
fn fence_could_be_a_call(block: &str, tool_names: &[String]) -> bool {
    let (info, body) = match block.find('\n') {
        Some(nl) => (block[..nl].trim(), &block[nl + 1..]),
        // Still mid-info-string: nothing to judge yet, so assume the worst.
        None => return true,
    };
    if matches!(info, "tool" | "tool_call" | "tool_calls" | "function" | "function_call") {
        return true;
    }
    body.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|word| tool_names.iter().any(|n| n == word))
}

/// Thin wrapper over the Tauri channel with typed emit helpers.
pub struct AgentEventSink {
    channel: Channel<AgentEvent>,
    /// `SUB-2`: the child run this sink belongs to, if any. Set, every event
    /// leaves wrapped in `AgentEvent::Sub` — one place, rather than a tag
    /// parameter on all twenty helpers below.
    sub: Option<String>,
}

impl AgentEventSink {
    pub fn new(channel: Channel<AgentEvent>) -> Self {
        Self { channel, sub: None }
    }

    /// A sink for a delegated child. Everything it sends arrives attributed to
    /// `run_id`, so a child's steps never land in the lead's own timeline.
    pub fn child(&self, run_id: &str) -> AgentEventSink {
        AgentEventSink {
            channel: self.channel.clone(),
            sub: Some(run_id.to_string()),
        }
    }

    /// The single exit. Every helper goes through here so tagging is not
    /// something a new helper can forget to do.
    fn send(&self, event: AgentEvent) {
        let _ = match &self.sub {
            Some(run_id) => self.channel.send(AgentEvent::Sub {
                run_id: run_id.clone(),
                event: Box::new(event),
            }),
            None => self.channel.send(event),
        };
    }
    /// Send any event verbatim — for variants without a dedicated helper.
    pub fn emit(&self, event: AgentEvent) {
        self.send(event);
    }
    pub fn token(&self, text: &str) {
        self.send(AgentEvent::Token { text: text.to_string() });
    }
    /// `HRN-UI-3`: the model is thinking. Not prose, and never appended to the
    /// message — the UI shows it as its own thing, or not at all.
    pub fn thinking(&self, run_id: &str, text: &str) {
        self.send(AgentEvent::Thinking {
            run_id: run_id.to_string(),
            text: text.to_string(),
        });
    }
    pub fn step_start(&self, id: &str, verb: &str, target: &str) {
        self.send(AgentEvent::StepStart {
            id: id.to_string(),
            verb: verb.to_string(),
            target: target.to_string(),
            parent: None,
        });
    }
    /// `RPC-1`: a step a script asked for, hanging under the `run_code` step it
    /// was made from rather than sitting beside it as if the model had chosen it.
    pub fn nested_step_start(&self, id: &str, verb: &str, target: &str, parent: &str) {
        self.send(AgentEvent::StepStart {
            id: id.to_string(),
            verb: verb.to_string(),
            target: target.to_string(),
            parent: Some(parent.to_string()),
        });
    }
    pub fn step_done(&self, id: &str, result: Option<String>) {
        self.send(AgentEvent::StepDone { id: id.to_string(), result });
    }
    pub fn step_error(&self, id: &str, error: &str) {
        self.send(AgentEvent::StepError {
            id: id.to_string(),
            error: error.to_string(),
        });
    }
    pub fn file_changed(&self, op: &str, path: &str, undo_token: Option<&str>) {
        self.send(AgentEvent::FileChanged {
            op: op.to_string(),
            path: path.to_string(),
            undo_token: undo_token.unwrap_or_default().to_string(),
        });
    }
    pub fn send_permission(&self, request: PermissionRequest) {
        self.send(AgentEvent::Permission { request });
    }
    /// `BRW-UI-1`: the Browser panel replaces its state wholesale on every
    /// action — see `AgentEvent::Browser`.
    pub fn browser(&self, state: super::browser::BrowserPanelState) {
        self.send(AgentEvent::Browser { state });
    }
    pub fn artifact(&self, id: &str, title: &str, kind: &str, content: &str) {
        self.send(AgentEvent::Artifact {
            id: id.to_string(),
            title: title.to_string(),
            kind: kind.to_string(),
            content: content.to_string(),
            meta_json: None,
        });
    }
    /// A whole artifact row, metadata included. Media uses this so the stream
    /// can render it as a media block rather than a bare chip.
    pub fn artifact_row(&self, artifact: &crate::db::Artifact) {
        self.send(AgentEvent::Artifact {
            id: artifact.id.clone(),
            title: artifact.title.clone(),
            kind: artifact.kind.clone(),
            content: artifact.content.clone(),
            meta_json: artifact.meta_json.clone(),
        });
    }
    pub fn block(&self, id: &str, message_id: Option<&str>, kind: &str, title: &str, data: &serde_json::Value) {
        self.send(AgentEvent::Block {
            id: id.to_string(),
            message_id: message_id.map(str::to_string),
            kind: kind.to_string(),
            title: title.to_string(),
            data: data.clone(),
        });
    }
    pub fn block_update(&self, id: &str, title: &str, data: &serde_json::Value) {
        self.send(AgentEvent::BlockUpdate {
            id: id.to_string(),
            title: title.to_string(),
            data: data.clone(),
        });
    }
    pub fn state_update(&self, state: &serde_json::Value) {
        self.send(AgentEvent::StateUpdate { state: state.clone() });
    }
    /// `HRN-1`: hand the UI this run's id, so it has something to steer and stop.
    fn run_started(&self, run_id: &str, max_steps: usize, context_window: Option<usize>) {
        self.send(AgentEvent::RunStarted {
            run_id: run_id.to_string(),
            max_steps,
            context_window,
        });
    }
    fn run_progress(
        &self,
        run_id: &str,
        step: usize,
        max_steps: usize,
        ms: u64,
        context_tokens: usize,
    ) {
        self.send(AgentEvent::RunProgress {
            run_id: run_id.to_string(),
            step,
            max_steps,
            ms,
            context_tokens,
        });
    }
    #[allow(clippy::too_many_arguments)]
    fn run_ended(
        &self,
        run_id: &str,
        stop_reason: StopReason,
        steps: usize,
        ms: u64,
        usage: Option<crate::runtime::proxy::Usage>,
        plan: Option<super::plan::Plan>,
    ) {
        self.send(AgentEvent::RunEnded {
            run_id: run_id.to_string(),
            stop_reason: stop_reason.as_str().to_string(),
            steps,
            ms,
            usage,
            plan,
        });
    }
    /// `PLN-UI-1`: the plan changed. Sent from the `plan` tool's own dispatch,
    /// so the card and the model are updated by the same call.
    fn plan(&self, run_id: &str, plan: &super::plan::Plan) {
        self.send(AgentEvent::Plan {
            run_id: run_id.to_string(),
            plan: plan.clone(),
        });
    }
    /// `HRN-2`: a queued instruction just became part of the transcript.
    fn steered(&self, run_id: &str, text: &str) {
        self.send(AgentEvent::Steered {
            run_id: run_id.to_string(),
            text: text.to_string(),
        });
    }
    fn done(&self) {
        self.send(AgentEvent::Done);
    }
    fn cancelled(&self) {
        self.send(AgentEvent::Cancelled);
    }
    fn error(&self, message: &str) {
        self.send(AgentEvent::Error { message: message.to_string() });
    }
}

/// Build the OpenAI `assistant` message echoing the model's tool-call request, so
/// the next turn has the calls in context.
fn assistant_tool_call_message(calls: &[ToolCallReq]) -> serde_json::Value {
    let tool_calls: Vec<serde_json::Value> = calls
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "type": "function",
                "function": { "name": c.name, "arguments": c.arguments }
            })
        })
        .collect();
    serde_json::json!({ "role": "assistant", "content": null, "tool_calls": tool_calls })
}

fn tool_result_message(call_id: &str, content: &str) -> serde_json::Value {
    serde_json::json!({ "role": "tool", "tool_call_id": call_id, "content": content })
}

/// Run the agent loop to completion, streaming events to `sink`. Returns the
/// final assistant prose so the caller can persist it.
///
/// `tools_enabled` gates the built-in toolsets. When false (the default for plain
/// chat) no `tools` are advertised, so the model answers directly and prose is
/// streamed live. When true, the File System toolset is offered and the loop
/// dispatches tool calls — buffering each turn so a tool call that the engine
/// streams as plain content JSON (see [`parse_text_tool_calls`]) is executed
/// rather than leaked to the user as raw text.
///
/// Thin wrapper over [`run_agent_inner`]: every exit from the loop below is a
/// `return`, so backfilling `OUT-1`'s `skill_runs.tool_failures` (which needs
/// every `tool_stats` row this run produced, including the last one) has to
/// happen after the loop is truly done — one place, not one per early return.
/// Reshape an incoming transcript into something a strict chat template can
/// render, without dropping anything the user said.
///
/// Gemma 3's template — like Anthropic's API, and unlike most OpenAI-compatible
/// servers — `raise_exception`s unless user and assistant turns strictly
/// alternate. A turn that fails leaves the user's message standing alone with no
/// reply beside it, so three failed attempts in a row leave three consecutive
/// user messages. The transcript is then unrenderable *from the fourth attempt
/// onward*: the moment the underlying fault is fixed, the conversation is
/// already broken and every retry 400s. Folding repeats together keeps the
/// history expressible and costs nothing — the text is all still there.
fn normalize_transcript(messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    /// A message that is just a role and some text — safe to fold. Anything
    /// carrying `tool_calls` is part of the loop's own bookkeeping and is left
    /// exactly as the loop wrote it.
    fn plain_text(msg: &serde_json::Value, role: &str) -> bool {
        msg.get("role").and_then(|r| r.as_str()) == Some(role)
            && msg.get("tool_calls").is_none()
            && msg.get("content").map(|c| c.is_string()).unwrap_or(false)
    }

    let mut out: Vec<serde_json::Value> = Vec::with_capacity(messages.len());
    let mut seen_user = false;
    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("").to_string();
        match role.as_str() {
            "user" => seen_user = true,
            // An assistant turn before the user has said anything cannot be
            // expressed by these templates at all, and a blank one is what a
            // failed turn leaves behind — neither carries meaning to lose.
            "assistant" if !seen_user => continue,
            "assistant"
                if plain_text(&msg, "assistant")
                    && msg["content"].as_str().unwrap_or("").trim().is_empty() =>
            {
                continue
            }
            _ => {}
        }

        // Fold a repeated turn into the one it repeats. `system` is exempt here
        // only because it is not a *turn* — a mid-conversation one is in fact
        // unrenderable by these same templates, which is
        // [`flatten_to_alternating`]'s problem to solve rather than this
        // function's: the loop appends those itself, long after this has run.
        if matches!(role.as_str(), "user" | "assistant") {
            if let Some(prev) = out.last_mut() {
                if plain_text(prev, &role) && plain_text(&msg, &role) {
                    let addition = msg["content"].as_str().unwrap_or("");
                    let merged = format!("{}\n\n{}", prev["content"].as_str().unwrap_or(""), addition);
                    prev["content"] = serde_json::Value::String(merged);
                    continue;
                }
            }
        }
        out.push(msg);
    }
    out
}

/// Does this error mean the engine's chat template could not render what we
/// sent it? llama-server reports it as a 400 quoting the template's own
/// `raise_exception`, so the match is on the text — there is no code for it.
fn is_template_error(err: &ProxyError) -> bool {
    let m = err.provider_message().to_ascii_lowercase();
    m.contains("roles must alternate")
        || m.contains("jinja")
        || m.contains("generate parser for this template")
        || m.contains("chat template")
}

/// Rewrite a transcript into the strictly alternating user/assistant form a
/// rigid chat template can render.
///
/// [`normalize_transcript`] handles the history the *caller* hands in. This
/// handles the messages the loop itself appends, which is a harder problem:
/// Gemma 3's template raises unless every even-indexed message is `user` and
/// every odd one `assistant`, so an assistant turn carrying `tool_calls`, the
/// `tool` messages answering it, and `GRM-3`'s mid-conversation `system` nudge
/// are all unrenderable by it. The result is that a tool loop against such a
/// model dies on its *second* request no matter what the first achieved —
/// llama-server answers 400 with the template's own exception text.
///
/// So the same history is re-sent with each message rewritten as plain user or
/// assistant text. The model still sees what it called and what came back; it
/// just reads it as prose instead of as protocol. Nothing is dropped except
/// messages that were empty to begin with.
fn flatten_to_alternating(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    /// The text of a message, whether its content is a plain string or an
    /// OpenAI content-part array (vision turns, `CHT-5`).
    fn text_of(msg: &serde_json::Value) -> String {
        match msg.get("content") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }

    let mut out: Vec<serde_json::Value> = Vec::with_capacity(messages.len());
    // call id → tool name, so a result can say what produced it.
    let mut call_names: HashMap<String, String> = HashMap::new();
    let mut rest = messages;

    // A leading system message is the one position every template accepts one
    // in — Gemma's folds it into the first user turn — so it stays as it is.
    if let Some(first) = messages.first() {
        if first.get("role").and_then(|r| r.as_str()) == Some("system") {
            out.push(first.clone());
            rest = &messages[1..];
        }
    }

    let mut seen_user = false;
    for msg in rest {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if let Some(calls) = msg.get("tool_calls").and_then(|c| c.as_array()) {
            for call in calls {
                if let (Some(id), Some(name)) = (
                    call.get("id").and_then(|i| i.as_str()),
                    call.pointer("/function/name").and_then(|n| n.as_str()),
                ) {
                    call_names.insert(id.to_string(), name.to_string());
                }
            }
        }

        let (as_role, text) = match role {
            "assistant" => {
                let mut text = text_of(msg);
                if let Some(calls) = msg.get("tool_calls").and_then(|c| c.as_array()) {
                    for call in calls {
                        let name = call
                            .pointer("/function/name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("a tool");
                        let args = call
                            .pointer("/function/arguments")
                            .and_then(|a| a.as_str())
                            .unwrap_or("{}");
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(&format!("[I called {name} with {args}]"));
                    }
                }
                ("assistant", text)
            }
            "tool" => {
                let name = msg
                    .get("tool_call_id")
                    .and_then(|i| i.as_str())
                    .and_then(|id| call_names.get(id))
                    .map(String::as_str)
                    .unwrap_or("the tool");
                ("user", format!("[{name} returned]\n{}", text_of(msg)))
            }
            // `user`, a mid-conversation `system` nudge, anything else: it is
            // all text the model did not write itself, so it reaches it as user.
            _ => ("user", text_of(msg)),
        };

        if text.trim().is_empty() {
            continue;
        }
        // These templates cannot open on an assistant turn at all.
        if as_role == "assistant" && !seen_user {
            continue;
        }
        seen_user |= as_role == "user";

        match out.last_mut() {
            Some(prev) if prev["role"] == as_role => {
                let merged = format!("{}\n\n{}", prev["content"].as_str().unwrap_or(""), text);
                prev["content"] = serde_json::Value::String(merged);
            }
            _ => out.push(serde_json::json!({ "role": as_role, "content": text })),
        }
    }
    out
}

/// The reasoning effort this run should ask for, from `models.reasoning_effort`.
///
/// Unset means [`Effort::default`], which is *low* rather than the provider's
/// own default. That is a deliberate opinion: every provider defaults to its
/// maximum, which in a chat app buys latency and cost you did not ask for, and
/// on a free tier is the setting most likely to end in a model that thinks
/// itself into a corner and never answers. Someone who wants more can say so.
fn reasoning_effort(db: &Db) -> Effort {
    db.get_setting("models.reasoning_effort")
        .ok()
        .flatten()
        .map(|v| Effort::parse(&v))
        .unwrap_or_default()
}

/// [`drive_turn`], plus a one-shot adaptation for engines whose chat template
/// refuses the loop's own message shapes (see [`flatten_to_alternating`]).
///
/// The verdict is sticky for the rest of the run: a template that couldn't
/// render this transcript won't render the next one either, and paying a failed
/// request per turn to rediscover that would be visible as a stutter.
#[allow(clippy::too_many_arguments)]
async fn drive_turn_adapting<F>(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    temperature: f32,
    effort: Effort,
    cancel: &CancelFlag,
    strict_template: &mut bool,
    on_token: &mut F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(Delta),
{
    if *strict_template {
        let flat = flatten_to_alternating(messages);
        return drive_turn(client, endpoint, &flat, tools, temperature, effort, cancel, on_token).await;
    }
    match drive_turn(client, endpoint, messages, tools, temperature, effort, cancel, &mut *on_token).await {
        Err(e) if is_template_error(&e) => {
            eprintln!("drive_turn: engine template refused the transcript ({e}); flattening and retrying");
            *strict_template = true;
            let flat = flatten_to_alternating(messages);
            drive_turn(client, endpoint, &flat, tools, temperature, effort, cancel, on_token).await
        }
        other => other,
    }
}

/// `OBS-3`: roughly how many tokens a transcript will cost to send.
///
/// Four characters to the token, the same rule the frontend's `estimateTokens`
/// already uses, so the meter does not disagree with itself depending on which
/// side of the app drew it. It is an estimate and the UI says so: the exact
/// number belongs to the provider's tokenizer, which we do not have.
fn estimate_tokens(messages: &[serde_json::Value]) -> usize {
    let chars: usize = messages
        .iter()
        .map(|m| match m.get("content") {
            Some(serde_json::Value::String(s)) => s.len(),
            // A multimodal content array: count the text parts, and take an
            // image as a flat 1000 tokens rather than pretending it is free.
            Some(serde_json::Value::Array(parts)) => parts
                .iter()
                .map(|p| match p.get("type").and_then(|t| t.as_str()) {
                    Some("text") => p.get("text").and_then(|t| t.as_str()).unwrap_or("").len(),
                    _ => 4000,
                })
                .sum(),
            other => other.map(|v| v.to_string().len()).unwrap_or(0),
        })
        .sum();
    chars.div_ceil(4)
}

/// Where a run sits in the fleet: its registry entry, its budget, and — for a
/// delegated child — the ceiling it may never exceed.
///
/// One struct rather than four parameters because every one of them travels
/// together, and a child run has to be handed all four at once.
pub struct RunContext<'a> {
    /// `HRN-1`: this run's registry entry. Owns the cancel flag and the steering
    /// inbox, so a child run started below this one is stoppable on its own.
    pub run: &'a RunHandle,
    pub limits: &'a RunLimits,
    /// The live run registry, so a tool call can start a child run that the user
    /// can then watch, steer and stop by id. `None` for a caller with no fleet
    /// (the `EVL` harness), in which case delegation reports itself unavailable
    /// rather than panicking.
    pub fleet: Option<&'a Fleet>,
    /// `SUB-5`: the toolsets the parent itself had. `None` for a top-level run,
    /// which is bounded only by Settings and its persona.
    pub ceiling: Option<&'a [Toolset]>,
    /// `OBS-2`: `local` | `cloud` | `endpoint`. Only the caller knows this —
    /// the endpoint itself cannot tell a BYOK provider from the user's own
    /// server — and the usage row is worthless without it, since it is what
    /// separates a bill from a free run on this machine.
    pub provenance: &'a str,
    /// `OBS-3`: how many tokens the model this run talks to can hold, when that
    /// is knowable. `None` leaves the meter showing a step count and a clock
    /// and no percentage, rather than a made-up one.
    pub context_window: Option<usize>,
    /// `PLN-T4`: the plan a resumed run picks back up. A resume that started
    /// with an empty plan would show the work vanishing at the moment it was
    /// continued — and the model, told nothing, would write a second one.
    /// `None` for every run that is not a resume.
    pub plan: Option<super::plan::Plan>,
}

impl<'a> RunContext<'a> {
    /// A top-level turn: no ceiling, no parent, no plan yet.
    pub fn top(
        run: &'a RunHandle,
        limits: &'a RunLimits,
        fleet: Option<&'a Fleet>,
        provenance: &'a str,
        context_window: Option<usize>,
    ) -> Self {
        Self { run, limits, fleet, ceiling: None, provenance, context_window, plan: None }
    }

    /// The same turn, continuing a run that already had a plan (`PLN-T4`).
    pub fn resuming(mut self, plan: Option<super::plan::Plan>) -> Self {
        self.plan = plan;
        self
    }
}

/// What a `delegate` call needs to start a child run of this same loop
/// (`SUB-5`). Built once per run and handed to the toolset through
/// `ToolContext`, because every piece of it belongs to the loop rather than to
/// the tool call: the endpoint the turn is running on, the fleet, this run's own
/// handle, and the toolsets a child may not exceed.
pub struct DelegationContext<'a> {
    pub fleet: &'a Fleet,
    /// The same endpoint the parent turn runs on. A child is the same agent on
    /// the same model, not a cheaper stand-in.
    pub endpoint: &'a ChatEndpoint,
    pub model_name: &'a str,
    pub temperature: f32,
    /// The parent's effective toolsets: the child's ceiling.
    pub toolsets: Vec<Toolset>,
    pub parent_run: &'a RunHandle,
    /// How much deeper delegation may go below the parent.
    pub max_depth: usize,
    /// `OBS-2`/`OBS-3`: a child runs on the parent's endpoint, so it inherits
    /// both of these unchanged.
    pub provenance: &'a str,
    pub context_window: Option<usize>,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    local_endpoint: Option<&ChatEndpoint>,
    db: &Db,
    mgr: &RuntimeManager,
    embed_mgr: &EmbedManager,
    rerank_mgr: &RerankManager,
    perms: &PermissionManager,
    memory: &MemoryStore,
    browser_pool: Option<&super::browser::BrowserPool>,
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    data_dir: &std::path::Path,
    model_name: &str,
    messages: Vec<serde_json::Value>,
    temperature: f32,
    tools_enabled: bool,
    headless: bool,
    rc: &RunContext<'_>,
    sink: &AgentEventSink,
) -> RunOutcome {
    let outcome = run_agent_inner(
        client,
        endpoint,
        local_endpoint,
        db,
        mgr,
        embed_mgr,
        rerank_mgr,
        perms,
        memory,
        browser_pool,
        conversation_id,
        assistant_message_id,
        data_dir,
        model_name,
        normalize_transcript(messages),
        temperature,
        tools_enabled,
        headless,
        rc,
        sink,
    )
    .await;
    let _ = db.backfill_skill_run_failures(conversation_id);
    outcome
}

/// `HRN-6`: something that watches a run go past without being able to steer it.
///
/// Every method has a do-nothing default, so an observer implements only the
/// moments it cares about and a new moment added here breaks nobody. The session
/// log (`CTX-2`) is the first implementation; telemetry (`OBS-2`) is the obvious
/// second. `()` is the observer for a run nobody is recording.
///
/// This is deliberately *not* `AgentEventSink`. The sink is how a run talks to
/// the user, and it is reached from inside every toolset through `ToolContext`.
/// An observer is how a run talks to whatever is writing it down, and it is
/// called only at the loop's own phase boundaries. Folding the two together
/// would have made every toolset generic over something none of them use.
/// `Send + Sync` because a run's future is spawned onto the runtime (a scheduled
/// job, a background subagent) and the observer is held across every await in it.
pub trait RunObserver: Send + Sync {
    /// The opening transcript, before the first request is made.
    fn prompt(&self, _messages: &[serde_json::Value]) {}
    /// One message reached the model's view of the conversation.
    fn appended(&self, _message: &serde_json::Value) {}
    /// Someone spoke to the run while it was already going.
    fn steered(&self, _text: &str) {}
    /// The run is over, and why.
    fn stopped(&self, _reason: &str, _steps: usize) {}
}

/// A run nobody is recording.
impl RunObserver for () {}

impl RunObserver for super::log::SessionLog<'_> {
    fn prompt(&self, messages: &[serde_json::Value]) {
        super::log::SessionLog::prompt(self, messages);
    }
    fn appended(&self, message: &serde_json::Value) {
        super::log::SessionLog::appended(self, message);
    }
    fn steered(&self, text: &str) {
        super::log::SessionLog::steered(self, text);
    }
    fn stopped(&self, reason: &str, steps: usize) {
        super::log::SessionLog::stopped(self, reason, steps);
    }
}

/// `HRN-6`: everything a turn needs that does not change while the run lasts.
///
/// This bundle is the point of the refactor. `dispatch` used to take twenty-two
/// separate borrows and `dispatch_calls` twenty-five, which meant every new
/// capability was another parameter threaded through three signatures — and it
/// made the call sites unreadable enough that a wrong argument would have looked
/// like all the others.
///
/// It is a struct of *borrows*, living on `run_agent_inner`'s own stack frame.
/// That is load-bearing, not incidental: because nothing can hold a `TurnCtx`
/// beyond the run, a detached task can never dispatch a tool call on a run that
/// has ended. `RPC-1`'s socket owns no `TurnCtx` for exactly this reason.
struct TurnCtx<'a> {
    client: &'a reqwest::Client,
    endpoint: &'a ChatEndpoint,
    /// The local engine, if one is loaded. Separate from `endpoint` so a
    /// toolset's own side call stays on this machine even when the turn itself
    /// is running against a cloud provider.
    local_endpoint: Option<&'a ChatEndpoint>,
    db: &'a Db,
    mgr: &'a RuntimeManager,
    embed_mgr: &'a EmbedManager,
    rerank_mgr: &'a RerankManager,
    perms: &'a PermissionManager,
    memory: &'a MemoryStore,
    browser_pool: Option<&'a super::browser::BrowserPool>,
    sink: &'a AgentEventSink,
    conversation_id: &'a str,
    assistant_message_id: Option<&'a str>,
    data_dir: &'a std::path::Path,
    model_name: &'a str,
    temperature: f32,
    /// How hard a reasoning model should think, resolved once per run like
    /// `cheap`. Providers default this to their maximum; the app used to send
    /// nothing at all and inherit that.
    effort: Effort,
    tools_enabled: bool,
    headless: bool,
    cancel: CancelFlag,
    rc: &'a RunContext<'a>,
    registry: ToolRegistry,
    /// `SUB-5`: what a `delegate` call would need to start a child of this run.
    delegation: Option<DelegationContext<'a>>,
    /// `LOOP-1`: MCP sessions reused across this run; dropped (and stdio
    /// children killed) when the run returns.
    mcp_pool: McpPool,
    /// `SKL-3`: directories a skill activated this run has made readable, shared
    /// across every tool call so a skill loaded early stays reachable.
    extra_read_roots: std::sync::Mutex<Vec<std::path::PathBuf>>,
    /// `SKL-2`: skills already loaded this run, so a second request for one
    /// returns a pointer instead of its whole body again.
    loaded_skills: std::sync::Mutex<Vec<String>>,
    /// `HRN-8`: where this run keeps a tool output too big to paste.
    results: super::results::ResultStore,
    /// `PLN-2`: the plan this run is working to.
    ///
    /// It lives here rather than on `RunState` — where the plan reads as
    /// per-turn state and where `PLN-2` places it — because the `plan` tool is
    /// dispatched from `dispatch`, which holds `&self` and never sees
    /// `RunState`: a batch of concurrent calls all borrow this context at once.
    /// A `Mutex` here is the same shape `extra_read_roots` and `loaded_skills`
    /// already use for run-wide state a tool call can change.
    plan: std::sync::Mutex<super::plan::Plan>,
    /// `PLN-3`: whether the plan tool is offered and what the prompt says about
    /// it. Resolved once per run, like `cheap` and `effort`.
    plan_mode: super::plan::PlanMode,
    /// What a turn might be trying to invoke, as opposed to what we can
    /// dispatch. Drives the hold-back and the narration check.
    tool_names: Vec<String>,
    invocable: Vec<String>,
    /// `COD-4`/`COD-12`: what this run read, changed and checked.
    ledger: super::ledger::Ledger,
    /// When the run began, in epoch milliseconds (`COD-11`'s "this run").
    started_at_ms: i64,
}

/// `HRN-6`: everything that changes from one turn to the next.
///
/// Split from `TurnCtx` so the phases can take `&self` and `&mut RunState`: the
/// borrow checker then states, rather than the reader having to trust, that a
/// phase cannot quietly reconfigure the run underneath the next one.
struct RunState {
    /// The transcript, as the model sees it.
    messages: Vec<serde_json::Value>,
    /// How much of `messages` has reached the observer (`CTX-2`'s watermark).
    logged: usize,
    /// The answer so far, accumulated across turns.
    final_text: String,
    /// `GRM-3`: call ids already nudged, so a failed built-in gets exactly one
    /// guided retry rather than an unbounded correction loop.
    retried: std::collections::HashSet<String>,
    /// `FIX-1`: the last failed call per tool name this run.
    last_failure: FixTracker,
    /// How many narrated-tool-call nudges this run has spent. A model that
    /// cannot emit a real tool call will not learn on the fifth ask either, and
    /// every nudge costs the user a turn.
    narration_nudges: usize,
    /// How many times this run has been asked to answer after an empty turn.
    empty_retries: usize,
    /// `PLN-2`: how many times this run has been told its plan is unfinished.
    plan_nudges: usize,
    /// `PLN-2`: turns this run spent only on plan bookkeeping, which do not
    /// count against the step budget. Capped at `MAX_FREE_PLAN_STEPS`.
    free_steps: usize,
    /// Set once this engine's chat template has proved it cannot render the
    /// loop's own message shapes.
    strict_template: bool,
    /// `HRN-5`: set once this run's budget is spent. The turn it is set on runs
    /// with no tools and is the last one, so the run always ends by saying
    /// something rather than by discarding what it learned.
    wrap_up: Option<StopReason>,
    iteration: usize,
}

/// What the `classify` phase decided this turn amounts to.
impl RunState {
    /// Whether this run has a tool result in its transcript.
    ///
    /// Derived rather than counted at each push site, for the same reason the
    /// session log uses one watermark: a new place that appends a tool result
    /// is covered by construction instead of by remembering to bump a field.
    fn did_tool_work(&self) -> bool {
        self.messages.iter().any(|m| m.get("tool_call_id").is_some())
    }

    /// Steps this run has spent on work, which is what the budget is for.
    ///
    /// Turns that only kept the plan current are not in it — see
    /// `MAX_FREE_PLAN_STEPS`. Derived rather than tracked as a second counter so
    /// the two numbers cannot disagree: `iteration` stays the honest count of
    /// times round the loop, which is what the timeline and `RunEnded` report.
    fn charged(&self) -> usize {
        self.iteration.saturating_sub(self.free_steps)
    }

    /// A turn that produced no answer but plainly had something to say: ask
    /// once for it. Returns whether the loop should go round again.
    ///
    /// Two situations reach here, and each gets the instruction that fits it:
    ///
    /// - **The run did tool work.** Everything it found is sitting in the
    ///   transcript, and stopping now throws all of it away behind an empty
    ///   turn — the same failure `HRN-5` removed at the step cap, arriving by a
    ///   different door.
    /// - **The model streamed reasoning and no answer.** A reasoning model that
    ///   spends its whole turn thinking and never writes a reply. Nothing is
    ///   lost from the transcript, but the user gets a blank turn after a long
    ///   wait, which reads as the app having broken.
    ///
    /// Two guards. Prose already written is an answer, however short. And one
    /// ask is the limit: a model with nothing to say will not have more on the
    /// second try, and each attempt costs the user a whole turn.
    fn rescue_empty_answer(&mut self, thought_but_said_nothing: bool) -> bool {
        if !self.final_text.trim().is_empty() || self.empty_retries >= MAX_EMPTY_RETRIES {
            return false;
        }
        // Never tell a model to use results that do not exist.
        let prompt = if self.did_tool_work() {
            ANSWER_NOW_PROMPT
        } else if thought_but_said_nothing {
            FINISH_THINKING_PROMPT
        } else {
            return false;
        };
        self.empty_retries += 1;
        self.messages.push(serde_json::json!({ "role": "system", "content": prompt }));
        true
    }

    /// `PLN-2`: the model wrote an answer while its own plan still has work in
    /// it. Ask once. Returns whether the loop should go round again.
    ///
    /// This is the failure the plan document calls *a plan that lies*: the model
    /// writes six items, does one, and then answers — and the card sits above
    /// the reply reading `0 of 6 done`. Either outcome of this ask fixes that.
    /// It finishes the work, or it says which items it is dropping and why, and
    /// the plan becomes true again.
    ///
    /// Deliberately not a gate. The run is never blocked on the plan, the model
    /// is told outright that stopping is a legitimate answer, and one ask is the
    /// limit — a model that means to stop will say so, and arguing with it costs
    /// the user a whole turn.
    fn nudge_unfinished_plan(&mut self, plan: &super::plan::Plan, answer: &str) -> bool {
        if self.plan_nudges >= MAX_PLAN_NUDGES {
            return false;
        }
        let left = plan.unreached();
        if left.is_empty() {
            return false;
        }
        let list = left.iter().map(|i| format!("- {}", i.text)).collect::<Vec<_>>().join("\n");
        self.plan_nudges += 1;
        // The model has to see its own reply to know what it is being asked to
        // continue from, exactly as with the narration nudge.
        if !answer.trim().is_empty() {
            self.messages.push(serde_json::json!({ "role": "assistant", "content": answer }));
        }
        self.messages.push(serde_json::json!({
            "role": "system",
            "content": format!(
                "You are about to finish, but your own plan still has these items open:\n{list}\n\
                 Carry on with them now. If you genuinely do not need one any more, drop it with \
                 plan(update:{{index, status:\"dropped\", why}}) so the plan says what happened — \
                 the user is looking at it. If everything is really done, mark the items done and \
                 then give your answer.",
            ),
        }));
        true
    }
}

impl RunState {
    /// Show the model its own answer and then the note, so it knows what it is
    /// being asked to continue from — the same shape as the plan nudge.
    fn push_nudge(&mut self, answer: &str, note: String) {
        if !answer.trim().is_empty() {
            self.messages.push(serde_json::json!({ "role": "assistant", "content": answer }));
        }
        self.messages.push(serde_json::json!({ "role": "system", "content": note }));
    }
}

/// `COD-12`: the note itself, as a pure function of what the run knows.
///
/// When the check can run, the model is asked to run it. When it cannot —
/// policy off, read-only, the toolset off, an unattended run — it is asked to
/// say so, which is the honest ending: an answer that implies it was verified
/// when nothing was run is the failure this exists to prevent.
fn unverified_message(files: usize, check: &str, can_run: bool) -> String {
    let changed = format!("You changed {files} file{} in this project", if files == 1 { "" } else { "s" });
    if can_run {
        format!(
            "{changed} and have not run `{check}` since. Run it with run_task and read the result before \
             you say the change works. If there is a real reason not to, say so plainly instead."
        )
    } else {
        format!(
            "{changed}, and `{check}` cannot be run here. Say plainly, in one sentence, that you have not \
             verified the change by running it."
        )
    }
}

impl TurnCtx<'_> {
    fn unverified_note(&self) -> Option<String> {
        if self.ledger.edited_count() == 0 {
            return None;
        }
        let (_, card) = super::project::for_conversation(self.db, self.conversation_id)?;
        let check = card.check_task()?.name.clone();
        let files = self.ledger.take_unverified()?;
        let can_run = super::coderun::can_verify(self.db, self.conversation_id, self.headless);
        Some(unverified_message(files, &check, can_run))
    }
}

enum Next {
    /// The run is over, for this reason.
    Stop(StopReason),
    /// Run these tool calls, then go round again.
    Calls(Vec<ToolCallReq>),
    /// Something was appended to the transcript; go round again.
    Again,
}

#[allow(clippy::too_many_arguments)]
async fn run_agent_inner(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    // The local engine, if one is loaded — see `ToolContext::local_endpoint`.
    // Separate from `endpoint` so a toolset's own side call stays on this machine
    // even when the turn itself is running against a cloud provider.
    local_endpoint: Option<&ChatEndpoint>,
    db: &Db,
    mgr: &RuntimeManager,
    embed_mgr: &EmbedManager,
    rerank_mgr: &RerankManager,
    perms: &PermissionManager,
    memory: &MemoryStore,
    // `BRW-1`: `None` for callers with no live pool — the scheduler's
    // headless runs (which the Browser toolset refuses outright) and the
    // `EVL` harness, which never dispatches a real tool call.
    browser_pool: Option<&super::browser::BrowserPool>,
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    data_dir: &std::path::Path,
    model_name: &str,
    messages: Vec<serde_json::Value>,
    temperature: f32,
    tools_enabled: bool,
    // SCH-3: true for an unattended scheduled-job run — no one is watching, so
    // toolsets must skip renders (RND-3) and the File System toolset refuses any
    // write/delete/move outright rather than opening a permission prompt that
    // could never be answered.
    headless: bool,
    rc: &RunContext<'_>,
    sink: &AgentEventSink,
) -> RunOutcome {
    let (run, limits) = (rc.run, rc.limits);
    sink.run_started(&run.id, limits.max_iterations, rc.context_window);

    // Unified tool table: built-in toolsets + enabled MCP connectors (§7.5),
    // narrowed to the parent's own toolsets when this run is a delegated child.
    let registry = ToolRegistry::build(
        db,
        mgr,
        conversation_id,
        rc.ceiling,
        // `SUB-5`: no room left below this run means no `delegate` tool at all.
        rc.fleet.is_some() && limits.max_depth > 0,
    );
    let mut tool_names = registry.tool_names();
    let mut invocable = registry.invocable_names();
    // `HRN-8`: named here rather than only once a result exists — holding back
    // a fence that mentions `read_result` costs nothing, and letting one leak
    // onto the screen as text is exactly what `safe_prefix` is for.
    for name in ["read_result", "search_result"] {
        tool_names.push(name.to_string());
        invocable.push(name.to_string());
    }
    let plan_mode = super::plan::PlanMode::current(db);
    if plan_mode.offers_tool() {
        tool_names.push("plan".to_string());
        invocable.push("plan".to_string());
    }

    let cx = TurnCtx {
        client,
        endpoint,
        local_endpoint,
        db,
        mgr,
        embed_mgr,
        rerank_mgr,
        perms,
        memory,
        browser_pool,
        sink,
        conversation_id,
        assistant_message_id,
        data_dir,
        model_name,
        temperature,
        effort: reasoning_effort(db),
        tools_enabled,
        headless,
        cancel: run.cancel.clone(),
        rc,
        // Built once here because the pieces belong to the run, not to the call.
        delegation: rc.fleet.map(|fleet| DelegationContext {
            fleet,
            endpoint,
            model_name,
            temperature,
            toolsets: registry.toolsets.clone(),
            parent_run: run,
            max_depth: limits.max_depth,
            provenance: rc.provenance,
            context_window: rc.context_window,
        }),
        registry,
        mcp_pool: Default::default(),
        extra_read_roots: Default::default(),
        loaded_skills: Default::default(),
        results: super::results::ResultStore::new(data_dir, conversation_id, &run.id),
        // `PLN-T4`: a resumed run continues its plan rather than starting blank.
        plan: std::sync::Mutex::new(rc.plan.clone().unwrap_or_default()),
        plan_mode,
        tool_names,
        invocable,
        ledger: Default::default(),
        started_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
            .saturating_sub(run.elapsed_ms() as i64),
    };

    // `CTX-2`: the model's view of this conversation, written as the run goes.
    // The opening array is stored first so a resume has something to replay even
    // if the app dies during the very first request.
    let log = super::log::SessionLog::new(db, conversation_id, &run.id);
    let obs: &dyn RunObserver = &log;
    obs.prompt(&messages);

    let mut st = RunState {
        logged: messages.len(),
        messages,
        final_text: String::new(),
        retried: Default::default(),
        last_failure: FixTracker::default(),
        narration_nudges: 0,
        empty_retries: 0,
        plan_nudges: 0,
        free_steps: 0,
        strict_template: false,
        wrap_up: None,
        iteration: 0,
    };

    // `HRN-3`: every exit announces why it stopped, so a stump is never shown
    // as if it were a finished answer.
    let end = |text: String, reason: StopReason| -> RunOutcome {
        // `CTX-2`: the answer and the reason, on every exit path. A run with no
        // `stop` row was killed with the app — which is precisely the run
        // "Continue where I stopped" exists to pick back up.
        if !text.is_empty() {
            obs.appended(&serde_json::json!({ "role": "assistant", "content": &text }));
        }
        obs.stopped(reason.as_str(), run.steps());
        // `OBS-2`: bill the run before announcing it, on every exit path —
        // including the ones that failed, because a failed run still spent
        // whatever it spent.
        //
        // Recorded even when the provider reported no usage at all. It used to
        // be skipped, which meant a provider that does not report usage — many
        // free tiers, and any OpenAI-compatible server that ignores
        // `include_usage` — left the Usage panel showing nothing after a day of
        // work, as though the runs had never happened. Zero tokens is a
        // measurement we do not have; it is not evidence that a run did not
        // occur, and the panel can say which of the two it is looking at.
        let usage = run.usage();
        let _ = db.record_run_usage(
            &run.id,
            conversation_id,
            model_name,
            rc.provenance,
            usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
            usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
            run.steps(),
        );
        // `PLN-5`: what the run meant to do, as it stood when it stopped. A run
        // that ran out of budget can now say which items it never reached
        // instead of only that it stopped.
        let plan = cx.plan.lock().unwrap();
        let final_plan = (!plan.is_empty()).then(|| plan.clone());
        drop(plan);
        sink.run_ended(&run.id, reason, run.steps(), run.elapsed_ms(), run.usage(), final_plan);
        RunOutcome { text, stop_reason: reason, steps: run.steps(), usage: run.usage() }
    };

    // `PLN-T4`: a resumed run shows the plan it is continuing from its first
    // moment, rather than only once the model next touches it.
    {
        let plan = cx.plan.lock().unwrap();
        if !plan.is_empty() {
            sink.plan(&run.id, &plan);
        }
    }

    // `HRN-6`: one turn is six named phases. Everything that used to sit inline
    // here now has a name, a signature, and a place for the next feature to go.
    loop {
        if let Some(reason) = cx.prepare_turn(&mut st, obs) {
            return end(std::mem::take(&mut st.final_text), reason);
        }

        // `HRN-5`: the wrap-up turn always ends the run, so it is its own phase
        // rather than a flag threaded through the ordinary one.
        if let Some(reason) = st.wrap_up {
            let ended = cx.wrap_up_turn(&mut st, reason).await;
            return end(std::mem::take(&mut st.final_text), ended);
        }

        let mut scratch = Vec::new();
        let tools = cx.assemble(&mut scratch);
        let (outcome, held) = cx.request(&mut st, tools).await;

        match cx.classify(&mut st, outcome, held) {
            Next::Stop(reason) => return end(std::mem::take(&mut st.final_text), reason),
            Next::Calls(calls) => cx.dispatch_batch(&mut st, &calls).await,
            // The loop continues — the model sees whatever was appended next
            // turn. (A budget-spent turn never reaches here: the wrap-up phase
            // above always returns.)
            Next::Again => {}
        }
    }
}

/// `HRN-2`/`HRN-5`: everything a turn's preamble does to the transcript, and
/// nothing else.
///
/// Split out of `prepare_turn` so the two decisions that quietly ruin a run when
/// they are wrong can be checked without a model, a database or a socket:
///
/// - **where a mid-run instruction lands.** Draining here is the only correct
///   point. Mid-tool-call the transcript is not in a valid state — an assistant
///   turn is waiting for results it has not been given — and mid-stream a new
///   message would interleave with tokens already on screen.
/// - **whether this is the last turn.** It has to be decided *before* the turn
///   runs, so the model is told and gets one tool-free turn to answer, rather
///   than being cut off with everything it learned thrown away.
///
/// Returns the drained instructions in arrival order, so the caller can announce
/// them to whoever is watching.
fn open_turn(
    run: &RunHandle,
    limits: &RunLimits,
    tools_enabled: bool,
    st: &mut RunState,
    obs: &dyn RunObserver,
) -> Vec<String> {
    let drained = run.drain_steers();
    let texts: Vec<String> = drained.iter().map(|s| s.text.clone()).collect();
    for steer in drained {
        // Recorded as an event of its own as well as as the message it becomes:
        // the two carry the same words but not the same fact, and only the log
        // can say later that this arrived mid-run.
        obs.steered(&steer.text);
        st.messages.push(steer.into_message());
    }

    if st.wrap_up.is_none() && tools_enabled {
        let spent = if limits.out_of_time() {
            Some(StopReason::Timeout)
        } else if st.charged() + 1 >= limits.max_iterations {
            Some(StopReason::MaxSteps)
        } else {
            None
        };
        if let Some(reason) = spent {
            st.messages.push(serde_json::json!({ "role": "system", "content": WRAP_UP_PROMPT }));
            st.wrap_up = Some(reason);
        }
    }
    texts
}

/// `PLN-2`: put the plan in front of the model for exactly one request.
///
/// Returns whether anything was added, which is what `hide_plan` takes back off
/// again. The pair is deliberately not an append: a plan line left in the
/// transcript is a snapshot, and by turn six the transcript would hold four of
/// them disagreeing about which item is being worked on. The model must see one
/// plan, and it must be the current one.
fn show_plan(messages: &mut Vec<serde_json::Value>, plan: &super::plan::Plan) -> bool {
    match super::plan::plan_message(plan) {
        Some(line) => {
            messages.push(line);
            true
        }
        None => false,
    }
}

/// The other half of `show_plan`. Nothing to take back when nothing was added —
/// which is the case for every run that never wrote a plan, i.e. most of them.
fn hide_plan(messages: &mut Vec<serde_json::Value>, pushed: bool) {
    if pushed {
        messages.pop();
    }
}

impl TurnCtx<'_> {
    /// **prepare_turn** — drain the inbox, apply the limits, and decide whether
    /// this is the last turn. `Some` means the run is over before it started.
    fn prepare_turn(&self, st: &mut RunState, obs: &dyn RunObserver) -> Option<StopReason> {
        let (run, limits) = (self.rc.run, self.rc.limits);

        // `CTX-2`: everything the last iteration appended — tool calls, their
        // results, a nudge — reaches the log before this one can end the run.
        flush_log(obs, &st.messages, &mut st.logged);

        if self.cancel.is_cancelled() {
            self.sink.cancelled();
            return Some(StopReason::Aborted);
        }

        // Everything this preamble does to the transcript happens in one place,
        // where it can be tested without a model. What is left here is the part
        // that talks to the outside world.
        for text in open_turn(run, limits, self.tools_enabled, st, obs) {
            self.sink.steered(&run.id, &text);
        }

        // A steer and the wrap-up prompt are both things the model is about to
        // see, so they belong in the log before the request, not after it.
        flush_log(obs, &st.messages, &mut st.logged);

        st.iteration += 1;
        run.bump_steps();
        self.sink.run_progress(
            &run.id,
            st.iteration,
            limits.max_iterations,
            run.elapsed_ms(),
            estimate_tokens(&st.messages),
        );
        None
    }

    /// **assemble** — the tools this turn is allowed to use.
    ///
    /// `HRN-8`: `read_result`/`search_result` appear only once this run has
    /// actually kept something. A tool that can only fail is worse than no tool:
    /// the model tries it, gets an error, and spends a step learning what it
    /// could have been told by its absence.
    ///
    /// `scratch` belongs to the caller so the common case — the registry's own
    /// specs, unchanged — costs no copy.
    ///
    /// `PLN-1`: `plan` is offered for the whole run whenever the setting allows
    /// it — unlike `read_result` it is useful before anything has happened, and
    /// a plan written after the work is a report, not a plan.
    fn assemble<'t>(&'t self, scratch: &'t mut Vec<serde_json::Value>) -> &'t [serde_json::Value] {
        if !self.tools_enabled {
            return &[];
        }
        let plans = self.plan_mode.offers_tool();
        if self.results.has_any() || plans {
            *scratch = self
                .registry
                .specs
                .iter()
                .cloned()
                .chain(self.results.has_any().then(super::results::tool_specs).unwrap_or_default())
                .chain(plans.then(super::plan::tool_specs).unwrap_or_default())
                .collect();
            return scratch;
        }
        &self.registry.specs
    }

    /// **request** — drive one turn against the model.
    ///
    /// In plain-chat mode prose streams live. In tools mode the turn is buffered
    /// so a content-form tool call can be intercepted before display — until the
    /// buffer clearly reads as prose, at which point what we have is flushed and
    /// the rest streams live (`LOOP-4`). Even then a fence or a line-initial `{`
    /// is held back until it resolves (`safe_prefix`), so a turn that opens in
    /// prose and then writes a tool call does not leak it.
    ///
    /// The second half of the return value is what the user has *not* been shown
    /// yet, which `classify` may still swallow if it turns out to be a call.
    async fn request(
        &self,
        st: &mut RunState,
        tools: &[serde_json::Value],
    ) -> (Result<TurnOutcome, ProxyError>, Held) {
        let mut turn_buf = String::new();
        let mut streaming_live = false;
        // How much of `turn_buf` the user has already been shown.
        let mut emitted = 0usize;
        let (tools_enabled, sink) = (self.tools_enabled, self.sink);
        let (tool_names, invocable) = (&self.tool_names, &self.invocable);
        let final_text = &mut st.final_text;
        let run_id = self.rc.run.id.as_str();
        let mut on_token = |delta: Delta| {
            // Thinking never joins `turn_buf`: everything below is about
            // deciding how much of the *answer* is safe to show, and reasoning
            // is not part of the answer. It goes straight out as its own event
            // so the run reads as working instead of hung.
            let Delta::Answer(t) = delta else {
                if let Delta::Thinking(t) = delta {
                    sink.thinking(run_id, t);
                }
                return;
            };
            turn_buf.push_str(t);
            if !tools_enabled {
                final_text.push_str(t);
                sink.token(t);
                return;
            }
            if !streaming_live && should_flush_prose(&turn_buf, tool_names) {
                streaming_live = true;
            }
            if !streaming_live {
                return;
            }
            let cut = safe_prefix(&turn_buf, invocable);
            if cut > emitted {
                let chunk = &turn_buf[emitted..cut];
                final_text.push_str(chunk);
                sink.token(chunk);
                emitted = cut;
            }
        };
        // `PLN-2`: the plan goes in for this request and comes straight back
        // out. Appending it to the transcript for good would leave a trail of
        // copies contradicting each other — turn three's "doing" sitting under
        // turn six's "done" — which is the lying checklist with extra steps.
        // Rendered last, so what the model is working to is the freshest thing
        // it read before it acts.
        let planned_pushed = show_plan(&mut st.messages, &self.plan.lock().unwrap());
        let outcome = drive_turn_adapting(
            self.client,
            self.endpoint,
            &st.messages,
            tools,
            self.temperature,
            self.effort,
            &self.cancel,
            &mut st.strict_template,
            &mut on_token,
        )
        .await;
        hide_plan(&mut st.messages, planned_pushed);

        // `OBS-1`: charge this turn to the run before branching on how it ended,
        // so a turn that goes on to fail still counts what it spent.
        if let Ok(turn) = &outcome {
            if let Some(usage) = turn.usage() {
                self.rc.run.add_usage(usage);
            }
        }
        (outcome, Held { buf: turn_buf, streaming_live, emitted })
    }

    /// **classify** — what this turn amounts to: a final answer, tool calls,
    /// narration that has to be nudged into a real call, or a stop.
    fn classify(
        &self,
        st: &mut RunState,
        outcome: Result<TurnOutcome, ProxyError>,
        held: Held,
    ) -> Next {
        match outcome {
            Ok(TurnOutcome::Final { content, reasoning, .. }) => {
                let text = if content.is_empty() { held.buf } else { content };

                // A reasoning model that spent the whole turn thinking and
                // emitted no answer. It is not a silent model and it is not a
                // hang: the tokens went into a field that is not the reply. Say
                // so in the log rather than leaving the turn looking dead.
                if text.trim().is_empty() && !reasoning.trim().is_empty() {
                    eprintln!(
                        "run_agent: the model streamed {} characters of thinking and no answer",
                        reasoning.chars().count()
                    );
                }

                if self.tools_enabled {
                    // The part of the turn the user has already seen. Anything
                    // past it was held back by `safe_prefix` and is still ours
                    // to swallow if it turns out to be a tool call.
                    let unshown = if held.streaming_live {
                        text.get(held.emitted..).unwrap_or("")
                    } else {
                        &text[..]
                    };

                    // Fallback: the engine may have written a tool call as plain
                    // content instead of structured tool_calls — the only shape
                    // some local models produce at all. Run this on *every*
                    // finished turn, not just fully-buffered ones: a model that
                    // opens in prose and then calls a tool used to end the run
                    // here with the call left on screen as text.
                    if let Some(calls) = parse_text_tool_calls(unshown, &self.registry) {
                        return Next::Calls(calls);
                    }
                    // Not parseable, but it reads as a tool call the model only
                    // *described* — a shape we cannot execute. One nudge, then
                    // on with the loop; ending the run here is what left the
                    // user staring at "I'll use the X skill" and nothing
                    // happening.
                    if st.narration_nudges < MAX_NARRATION_NUDGES
                        && narrates_a_tool_call(unshown, &self.invocable)
                    {
                        st.narration_nudges += 1;
                        // The whole turn, not just the withheld tail: the model
                        // has to see its own narration to know what it is being
                        // asked to redo.
                        if !text.trim().is_empty() {
                            st.messages
                                .push(serde_json::json!({ "role": "assistant", "content": &text }));
                        }
                        st.messages.push(serde_json::json!({
                            "role": "system",
                            "content": "You described a tool call instead of making one. Call the tool for real now — emit it as a tool call, not as text or a code block.",
                        }));
                        return Next::Again;
                    }
                    // A genuine answer: emit whatever is still unshown.
                    if !unshown.is_empty() {
                        st.final_text.push_str(unshown);
                        self.sink.token(unshown);
                    }

                    // An empty answer that had something behind it — tool
                    // results, or a whole turn of thinking — is rescued rather
                    // than shown as a blank.
                    if st.rescue_empty_answer(!reasoning.trim().is_empty()) {
                        return Next::Again;
                    }

                    // `PLN-2`: an answer that arrives with the plan half done.
                    // Asked once, after the prose has been shown — the user
                    // keeps what was written either way, and what follows
                    // continues it rather than replacing it.
                    if st.nudge_unfinished_plan(&self.plan.lock().unwrap(), &text) {
                        return Next::Again;
                    }

                    // `COD-12`: code changed in a project with a check, and the
                    // check never ran after it. Asked once; never a gate.
                    if let Some(note) = self.unverified_note() {
                        st.push_nudge(&text, note);
                        return Next::Again;
                    }
                } else if st.final_text.is_empty() {
                    st.final_text = text;
                }
                self.sink.done();
                Next::Stop(StopReason::Completed)
            }
            Ok(TurnOutcome::Cancelled) => {
                self.sink.cancelled();
                Next::Stop(StopReason::Aborted)
            }
            Ok(TurnOutcome::ToolCalls { calls, .. }) => Next::Calls(calls),
            Err(e) => {
                self.sink.error(&format!("The model run failed: {e}"));
                Next::Stop(StopReason::Error)
            }
        }
    }

    /// `HRN-5`: the last turn of a run whose budget is spent.
    ///
    /// It has no tools, so there is nothing to hold back and nothing to
    /// intercept, and it always ends the run. It runs on the model the rest of
    /// the run used: this is the last thing the user reads, and handing it to a
    /// smaller model to save one turn's tokens was never worth the trade.
    async fn wrap_up_turn(&self, st: &mut RunState, reason: StopReason) -> StopReason {
        // `PLN-5`: a run whose budget ran out mid-plan says which items it never
        // reached, in the answer itself. Left in the transcript rather than
        // popped like `plan_message`: this is the last turn, and the run ends
        // with it.
        if let Some(line) = super::plan::wrap_up_message(&self.plan.lock().unwrap()) {
            st.messages.push(line);
        }
        // A lock rather than a cell: this future has to stay Send.
        let buf = std::sync::Mutex::new(String::new());
        let mut push = |delta: Delta| match delta {
            Delta::Answer(t) => {
                buf.lock().unwrap().push_str(t);
                self.sink.token(t);
            }
            // The wrap-up is where a run out of budget writes its answer, so a
            // silent think here is the worst place to look hung.
            Delta::Thinking(t) => self.sink.thinking(&self.rc.run.id, t),
        };
        let outcome = drive_turn_adapting(
            self.client,
            self.endpoint,
            &st.messages,
            &[],
            self.temperature,
            self.effort,
            &self.cancel,
            &mut st.strict_template,
            &mut push,
        )
        .await;
        let streamed = buf.into_inner().unwrap_or_default();
        if let Ok(turn) = &outcome {
            if let Some(usage) = turn.usage() {
                self.rc.run.add_usage(usage);
            }
        }
        let answer = match outcome {
            Ok(TurnOutcome::Final { content, .. }) if !content.is_empty() => content,
            // A model that calls tools on a turn that offered none has nowhere
            // left to go. Take whatever prose came with it.
            Ok(TurnOutcome::Final { .. }) | Ok(TurnOutcome::ToolCalls { .. }) => String::new(),
            Ok(TurnOutcome::Cancelled) => {
                self.sink.cancelled();
                return StopReason::Aborted;
            }
            Err(e) => {
                self.sink.error(&format!("The model run failed: {e}"));
                return StopReason::Error;
            }
        };
        let answer = if answer.is_empty() { streamed.clone() } else { answer };
        // Streaming already showed it; only a turn that streamed nothing still
        // has to emit.
        if streamed.is_empty() && !answer.is_empty() {
            self.sink.token(&answer);
        }
        st.final_text.push_str(&answer);
        self.sink.done();
        // The answer is real, but it is what the run had in hand rather than
        // what it set out to say — so it keeps the reason its budget ended for.
        reason
    }
}

/// What `request` streamed and how much of it the user has already seen.
struct Held {
    buf: String,
    streaming_live: bool,
    emitted: usize,
}

/// `FIX-1`'s bookkeeping: the last failed call per tool name, for one run.
///
/// The rule this type exists to hold is narrower than "remember failures", and
/// each narrowing is deliberate:
///
/// - **Same tool.** A different tool succeeding says nothing about the one that
///   failed, so the pair is keyed by tool name.
/// - **Same run.** The tracker lives on the stack of a single run; a correction
///   the user made in a later conversation isn't the model correcting itself.
/// - **Only on success.** A tool that fails and never succeeds teaches nothing
///   except that it's broken, which is `HEAL-2`'s job, not a lesson's.
/// - **At most one pair per failure.** `succeeded` takes the entry rather than
///   reading it, so a tool that fails once and then succeeds five times yields
///   one row, not five.
#[derive(Default)]
struct FixTracker(HashMap<String, (String, String)>);

impl FixTracker {
    /// Remember a failed call, replacing any earlier unpaired failure of the
    /// same tool — the most recent wrong approach is the one the correction
    /// actually corrected.
    fn failed(&mut self, tool: &str, args: &str, error: &str) {
        self.0.insert(tool.to_string(), (args.to_string(), error.to_string()));
    }

    /// The `(failed_args, error)` this success corrects, if any. Clears it.
    fn succeeded(&mut self, tool: &str) -> Option<(String, String)> {
        self.0.remove(tool)
    }
}

/// `CTX-2`: write everything appended to the transcript since the last flush.
///
/// A watermark rather than a call at each push site, because there are a dozen
/// push sites across two functions and every future one would have to remember
/// to log itself. This way a message that reaches the model reaches the log by
/// construction, and the only thing a new push site has to get right is the
/// message.
fn flush_log(obs: &dyn RunObserver, messages: &[serde_json::Value], logged: &mut usize) {
    for message in messages.iter().skip(*logged) {
        obs.appended(message);
    }
    *logged = messages.len();
}

/// `HRN-4`: split one turn's calls into the ones that may run together and the
/// ones that must wait their turn.
///
/// Anything we cannot place — a skill, an MCP tool, a name no toolset claims —
/// is serial. Guessing wrong about a write costs correctness; guessing wrong
/// about a read only costs time.
fn partition_calls(registry: &ToolRegistry, calls: &[ToolCallReq]) -> (Vec<usize>, Vec<usize>) {
    let mut concurrent = Vec::new();
    let mut ordered = Vec::new();
    for (i, call) in calls.iter().enumerate() {
        match registry.builtin_for(&call.name) {
            Some(ts) if !ts.is_serial(&call.name) => concurrent.push(i),
            _ => ordered.push(i),
        }
    }
    // One call on its own is not a batch. Running it through the concurrent
    // path would announce "1 thing at once", which is just a step.
    if concurrent.len() < 2 {
        ordered.append(&mut concurrent);
        ordered.sort_unstable();
    }
    (concurrent, ordered)
}

/// `PLN-2`: did this turn do nothing but keep the plan current?
///
/// A batch that mixes `plan` with real work is work — the model updating its
/// plan in the same turn as the search it describes is exactly what it should
/// do, and that turn is charged for the search, as it should be. Only a turn
/// that is *purely* bookkeeping goes free.
fn is_bookkeeping(calls: &[ToolCallReq]) -> bool {
    !calls.is_empty() && calls.iter().all(|c| super::plan::handles(&c.name))
}

/// Execute a batch of tool calls: echo them into the message history, run each
/// through its toolset or MCP server (emitting timeline steps), and append results.
#[allow(clippy::too_many_arguments)]
impl TurnCtx<'_> {
/// **dispatch** and **record** — run a turn's tool calls, then write down what
/// each one did: the transcript the model sees next turn, the reliability stat,
/// the fail-then-fix pair, and the step line the user reads.
async fn dispatch_batch(&self, st: &mut RunState, calls: &[ToolCallReq]) {
    // `PLN-2`: a turn that only kept the plan current is not work, and must not
    // be charged as if it were. Decided here, on what the turn actually did,
    // rather than guessed anywhere else.
    if is_bookkeeping(calls) && st.free_steps < MAX_FREE_PLAN_STEPS {
        st.free_steps += 1;
    }
    let (db, sink, registry) = (self.db, self.sink, &self.registry);
    let (conversation_id, model_name, cancel) =
        (self.conversation_id, self.model_name, &self.cancel);
    let results = &self.results;
    st.messages.push(assistant_tool_call_message(calls));

    let parsed: Vec<serde_json::Value> = calls
        .iter()
        .map(|c| serde_json::from_str(&c.arguments).unwrap_or_else(|_| serde_json::json!({})))
        .collect();
    let (concurrent, ordered) = partition_calls(registry, calls);

    type CallResult = Result<(String, Option<String>), String>;
    // `RPC-1`: a `run_code` call in this batch may make tool calls of its own
    // from inside the sandbox. They arrive here, on this channel, and are run
    // through the very same `dispatch` the model's calls go through — which is
    // what makes permissions, folder trust, untrusted marking and the headless
    // refusal apply to them without a second implementation of any of it.
    //
    // The channel exists only when something in this batch could use it, so a
    // batch with no `run_code` in it behaves exactly as it did before.
    let (gate, mut inbox) = tokio::sync::mpsc::unbounded_channel::<super::toolrpc::Call>();
    let rpc = (calls.iter().any(|c| super::codeexec::handles(&c.name))
        && super::toolrpc::is_enabled(db))
    .then_some(&gate);

    // The batch owns its own results and hands them back when it is done, so
    // nothing below borrows anything the future is still holding.
    let run_tools = async {
        let mut outcomes: Vec<Option<CallResult>> = (0..calls.len()).map(|_| None).collect();
        // `HRN-4`: the reads first, all at once. Their steps are announced before
        // the first await, so the timeline shows three rows filling together
        // instead of one row moving three times.
        if !concurrent.is_empty() && !cancel.is_cancelled() {
            let ids: Vec<String> = concurrent.iter().map(|&i| calls[i].id.clone()).collect();
            sink.emit(AgentEvent::StepsParallel { ids });
            for &i in &concurrent {
                let (verb, target) = describe(&calls[i].name, &parsed[i], registry);
                sink.step_start(&calls[i].id, &verb, &target);
            }
            let done = futures_util::future::join_all(concurrent.iter().map(|&i| {
                let call = &calls[i];
                let args = &parsed[i];
                async move {
                    (i, self.dispatch(rpc, &call.id, &call.name, args).await)
                }
            }))
            .await;
            for (i, result) in done {
                outcomes[i] = Some(result);
            }
        }

        for &i in &ordered {
            let call = &calls[i];
            // `CHT-2b`: a model can ask for several tools in one turn, and the loop
            // only looked at the flag between *turns*. Pressing Stop during a batch
            // used to sit through every remaining call before anything noticed.
            if cancel.is_cancelled() {
                break;
            }
            let (verb, target) = describe(&call.name, &parsed[i], registry);
            sink.step_start(&call.id, &verb, &target);
            outcomes[i] = Some(self.dispatch(rpc, &call.id, &call.name, &parsed[i]).await);
        }
        outcomes
    };

    // Run the batch, answering the sandbox's calls as they come in. One at a
    // time: a script blocks on each call anyway, so nothing is gained by
    // overlapping them, and serving them in order keeps the timeline in the
    // order the script actually did the work.
    tokio::pin!(run_tools);
    let mut outcomes = loop {
        tokio::select! {
            done = &mut run_tools => break done,
            Some(call) = inbox.recv(), if rpc.is_some() => {
                let id = format!("rpc_{}", uuid::Uuid::new_v4().simple());
                let result = match script_refusal(registry, &call.name) {
                    Some(why) => Err(why),
                    None => {
                        let (verb, target) = describe(&call.name, &call.args, registry);
                        sink.nested_step_start(&id, &verb, &target, &call.parent);
                        // No gate passed down: a script may not start a script.
                        let out = self.dispatch(None, &id, &call.name, &call.args).await;
                        db.add_tool_stat(model_name, &call.name, conversation_id, out.is_ok());
                        match &out {
                            Ok((output, note)) => {
                                sink.step_done(&id, note.clone().or_else(|| summarize(output)))
                            }
                            Err(e) => sink.step_error(&id, e),
                        }
                        out.map(|(output, _)| output)
                    }
                };
                let _ = call.reply.send(result);
            }
        }
    };

    // Results land in the order the model asked for them, whatever order they
    // finished in, so the transcript is the same every time.
    for (i, call) in calls.iter().enumerate() {
        let Some(result) = outcomes[i].take() else {
            // Never ran: stopped before its turn came. The transcript still has
            // to make sense to the model, so the call gets a result saying why.
            sink.step_error(&call.id, "Stopped.");
            st.messages.push(tool_result_message(&call.id, "Error: the user stopped this run."));
            continue;
        };
        // GRM-4/LOOP-5: record every dispatched call's outcome (content-free).
        db.add_tool_stat(model_name, &call.name, conversation_id, result.is_ok());
        match result {
            Ok((output, note)) => {
                // FIX-1: this tool just succeeded — if its last call in this
                // run had failed, that pair is exactly "wrong approach, then
                // right approach". Nothing is written if it never failed.
                if let Some((failed_args, error)) = st.last_failure.succeeded(&call.name) {
                    db.add_tool_fix(conversation_id, &call.name, &failed_args, &error, &call.arguments);
                }
                // `HRN-8`: too big to paste means kept on disk, with a handle
                // for the model and the whole thing for the user.
                match results.keep(&call.id, &output) {
                    Some((reference, shown)) => {
                        sink.emit(AgentEvent::KeptResult {
                            id: call.id.clone(),
                            reference,
                            bytes: output.len(),
                            text: output.clone(),
                        });
                        sink.step_done(
                            &call.id,
                            Some(format!("\u{2014} {}, kept", super::results::human_size(output.len()))),
                        );
                        st.messages.push(tool_result_message(&call.id, &shown));
                    }
                    None => {
                        sink.step_done(&call.id, note.or_else(|| summarize(&output)));
                        st.messages.push(tool_result_message(&call.id, &output));
                    }
                }
            }
            Err(e) => {
                st.last_failure.failed(&call.name, &call.arguments, &e);
                sink.step_error(&call.id, &e);
                st.messages.push(tool_result_message(&call.id, &format!("Error: {e}")));
                // GRM-3: give a failed *built-in* call one guided retry. MCP
                // errors take the LOOP-UI-2 path, not this nudge.
                if registry.builtin_for(&call.name).is_some() && st.retried.insert(call.id.clone()) {
                    st.messages.push(serde_json::json!({
                        "role": "system",
                        "content": format!(
                            "Fix the previous tool call: {e}. Reply with ONLY the corrected tool call."
                        ),
                    }));
                }
            }
        }
    }
}
}

/// Fallback tool-call parser (TOOL-2): some chat templates and older llama.cpp
/// builds stream a tool call as plain assistant *content* JSON (e.g. Llama 3.x's
/// `{"name": "...", "parameters": {...}}`) instead of structured `tool_calls`
/// deltas. Recognize that shape — but only when the `name` is a real built-in
/// tool, so a model that legitimately answers with JSON isn't misread.
fn parse_text_tool_calls(content: &str, registry: &ToolRegistry) -> Option<Vec<ToolCallReq>> {
    let body = strip_code_fence(strip_think(content.trim()));
    // Whole-string parse first: an array of calls or a single call object.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        let calls: Vec<ToolCallReq> = match &value {
            serde_json::Value::Array(items) => {
                items.iter().filter_map(|v| one_text_call(v, registry)).collect()
            }
            serde_json::Value::Object(_) => one_text_call(&value, registry).into_iter().collect(),
            _ => Vec::new(),
        };
        if !calls.is_empty() {
            return Some(calls);
        }
    }
    // Salvage: a call object embedded in surrounding prose (a reasoning
    // preamble, a trailing sentence). Parse the first complete JSON value
    // starting at the first '{'; the registry-name guard still applies, so a
    // genuine answer that merely contains JSON isn't misread.
    if let Some(start) = body.find('{') {
        let mut stream =
            serde_json::Deserializer::from_str(&body[start..]).into_iter::<serde_json::Value>();
        if let Some(Ok(value)) = stream.next() {
            let calls: Vec<ToolCallReq> = one_text_call(&value, registry).into_iter().collect();
            if !calls.is_empty() {
                return Some(calls);
            }
        }
    }
    // Last resort, and the shape a small local model actually produces: no JSON
    // at all, just the tool's name and its argument on one line —
    // `skill content-research-writer`, sometimes inside a ```tool fence.
    //
    // Only attempted where the text is *nothing but* the call: a fenced block,
    // or a single line. Loose in a paragraph this would misread an ordinary
    // sentence that happens to open on a tool's name ("web_search works.") as
    // an invocation, and calling a tool the user didn't ask for is worse than
    // missing one.
    let fenced = content.trim_start().starts_with("```");
    if fenced || body.lines().filter(|l| !l.trim().is_empty()).count() == 1 {
        return invocation_line_call(body, registry).map(|c| vec![c]);
    }
    None
}

/// Parse `<tool> <argument>` / `<tool>(<json>)` / `<tool> {json}` as a call, when
/// `<tool>` is advertised by the registry. A bare scalar argument is bound to the
/// tool's single required property — the only case where the mapping is
/// unambiguous, and the one that covers `skill <name>`.
fn invocation_line_call(body: &str, registry: &ToolRegistry) -> Option<ToolCallReq> {
    for line in body.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '>', '#']).trim();
        // Models often wrap the whole thing in backticks or `call:` chatter.
        let line = line.trim_matches('`').trim();
        let line = line.strip_prefix("call:").or_else(|| line.strip_prefix("tool:")).unwrap_or(line).trim();
        if line.is_empty() {
            continue;
        }
        let (head, rest) = match line.find(['(', ' ', '\t', '{']) {
            Some(i) => (&line[..i], line[i..].trim()),
            None => (line, ""),
        };
        // A skill named where a tool belongs. Loading it is the whole act, so
        // whatever the model wrote after the name is discarded — including the
        // invented `:verb`, which no skill has.
        if let Some(skill) = registry.skill_named(head.trim_end_matches(':')) {
            return Some(ToolCallReq {
                id: format!("call_{}", uuid::Uuid::new_v4().simple()),
                name: "skill".to_string(),
                arguments: serde_json::json!({ "name": skill }).to_string(),
            });
        }
        let name = head.trim_end_matches(':');
        if registry.builtin_for(name).is_none() && !registry.mcp.contains_key(name) {
            continue;
        }
        let rest = rest.trim_start_matches('(').trim_end_matches([')', '.', ';']).trim();
        if rest.is_empty() {
            continue; // a tool named in passing, not invoked
        }
        // `{...}` / `key=value`-free JSON is taken as the argument object.
        let arguments = if rest.starts_with('{') {
            match serde_json::from_str::<serde_json::Value>(rest) {
                Ok(v) if v.is_object() => v.to_string(),
                _ => continue,
            }
        } else {
            let key = sole_required_property(registry, name)?;
            let value = rest.trim_matches(['"', '\'']).trim();
            if value.is_empty() || value.contains(char::is_whitespace) {
                continue; // a sentence about the tool, not an argument
            }
            serde_json::json!({ key: value }).to_string()
        };
        return Some(ToolCallReq {
            id: format!("call_{}", uuid::Uuid::new_v4().simple()),
            name: name.to_string(),
            arguments,
        });
    }
    None
}

/// The name of `tool`'s one required property, when it has exactly one. `None`
/// otherwise — with two or more, a bare argument can't be placed without
/// guessing, and a wrong guess is worse than not calling the tool.
fn sole_required_property(registry: &ToolRegistry, tool: &str) -> Option<String> {
    let spec = registry
        .specs
        .iter()
        .find(|s| s.pointer("/function/name").and_then(|n| n.as_str()) == Some(tool))?;
    let required = spec.pointer("/function/parameters/required")?.as_array()?;
    match required.as_slice() {
        [only] => only.as_str().map(str::to_string),
        _ => None,
    }
}

/// Does this finished turn read as a tool call the model *described* rather than
/// made? Used only after every parser has already failed, to decide between one
/// corrective nudge and accepting the prose as the answer. Requires an advertised
/// tool name inside a code fence or backticks — prose that merely mentions a tool
/// ("I could search the web") must not trigger it.
fn narrates_a_tool_call(text: &str, tool_names: &[String]) -> bool {
    let mut marked = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            marked.push_str(line);
            marked.push('\n');
        } else {
            // Inline `code` spans on an otherwise prose line.
            let mut parts = line.split('`');
            let _ = parts.next();
            while let Some(code) = parts.next() {
                marked.push_str(code);
                marked.push('\n');
                let _ = parts.next();
            }
        }
    }
    // Hyphens count as word characters here: skill names are kebab-case, and
    // splitting on `-` would shatter `content-research-writer` into three words
    // that match nothing.
    tool_names.iter().any(|n| {
        marked
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
            .any(|word| word == n)
    })
}

/// Drop a `<think>…</think>` reasoning preamble some models stream as content.
fn strip_think(s: &str) -> &str {
    let t = s.trim_start();
    if let Some(rest) = t.strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            return rest[end + "</think>".len()..].trim_start();
        }
    }
    s
}

/// Convert a single `{name, parameters|arguments}` object into a [`ToolCallReq`]
/// if `name` is a known tool (built-in or from an enabled MCP connector). Some
/// chat templates (Llama 3.x) name the tool under `function` instead of `name`.
fn one_text_call(v: &serde_json::Value, registry: &ToolRegistry) -> Option<ToolCallReq> {
    let name = v
        .get("name")
        .or_else(|| v.get("function"))
        .and_then(|n| n.as_str())?;
    // A skill named where the tool belongs, in JSON form this time.
    if let Some(skill) = registry.skill_named(name) {
        return Some(ToolCallReq {
            id: format!("call_{}", uuid::Uuid::new_v4().simple()),
            name: "skill".to_string(),
            arguments: serde_json::json!({ "name": skill }).to_string(),
        });
    }
    if registry.builtin_for(name).is_none() && !registry.mcp.contains_key(name) {
        return None;
    }
    let args_val = v.get("parameters").or_else(|| v.get("arguments"));
    let arguments = match args_val {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "{}".to_string(),
    };
    Some(ToolCallReq {
        id: format!("call_{}", uuid::Uuid::new_v4().simple()),
        name: name.to_string(),
        arguments,
    })
}

/// Strip a leading code fence (and its trailing ```), if present. The info
/// string is dropped whatever it says: models label these `json`, `tool`,
/// `tool_call`, `function`, or nothing at all, and none of that is content.
fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    let Some(rest) = s.strip_prefix("```") else {
        return s;
    };
    // Everything up to the first newline is the fence's info string.
    let rest = match rest.find('\n') {
        Some(nl) if !rest[..nl].contains('`') => &rest[nl + 1..],
        _ => rest,
    };
    match rest.rfind("```") {
        Some(end) => rest[..end].trim(),
        None => rest.trim(),
    }
}

/// Route a tool call to the toolset or MCP server that handles it (§7.5).
#[allow(clippy::too_many_arguments)]
impl TurnCtx<'_> {
async fn dispatch(
    &self,
    rpc: Option<&super::toolrpc::Gate>,
    call_id: &str,
    name: &str,
    args: &serde_json::Value,
) -> Result<(String, Option<String>), String> {
    let registry = &self.registry;
    let results = &self.results;
    // `HRN-8`: reading back a result this run kept belongs to no toolset — it
    // is the loop's own bookkeeping, and it is only reachable at all once
    // something has been kept.
    if super::results::handles(name) {
        return super::results::execute(results, name, args).map(|out| (out, None));
    }
    // `PLN-1`: the plan belongs to the run, not to a toolset — see `plan.rs` on
    // why it is dispatched here beside `read_result` rather than through the
    // registry. The three things a plan write has to do happen together: the
    // model gets the new list back, the user's card is updated, and the log has
    // a row that a resume or a fork can read.
    if super::plan::handles(name) && self.plan_mode.offers_tool() {
        let output = super::plan::execute(&self.plan, args)?;
        let plan = self.plan.lock().unwrap();
        self.sink.plan(&self.rc.run.id, &plan);
        super::log::record_plan(self.db, self.conversation_id, &self.rc.run.id, &plan);
        let note = match plan.current() {
            Some(item) => format!("\u{2014} {}", crate::media::ellipsize(&item.text, 48)),
            None => "\u{2014} every step done".to_string(),
        };
        return Ok((output, Some(note)));
    }
    if let Some(toolset) = registry.builtin_for(name) {
        let ctx = ToolContext {
            client: self.client,
            local_endpoint: self.local_endpoint,
            db: self.db,
            mgr: self.mgr,
            embed_mgr: self.embed_mgr,
            rerank_mgr: self.rerank_mgr,
            perms: self.perms,
            sink: self.sink,
            conversation_id: self.conversation_id,
            assistant_message_id: self.assistant_message_id,
            data_dir: self.data_dir,
            call_id,
            memory: self.memory,
            headless: self.headless,
            // One context per tool call, so this is RND-3's one-render budget.
            rendered: std::sync::atomic::AtomicBool::new(false),
            step_note: std::sync::Mutex::new(None),
            extra_read_roots: &self.extra_read_roots,
            loaded_skills: &self.loaded_skills,
            browser_pool: self.browser_pool,
            delegation: self.delegation.as_ref(),
            rpc,
            ledger: &self.ledger,
            cancel: Some(&self.cancel),
            run_started_at: self.started_at_ms,
        };
        let output = toolset.execute(&ctx, name, args).await?;
        // A toolset that said what its step line should read (RET-UI-2) wins over
        // the generic summary; everything else still gets `summarize`.
        Ok((output, ctx.step_note.into_inner().unwrap_or(None)))
    } else if let Some(binding) = registry.mcp.get(name) {
        call_mcp_tool(self.client, self.db, self.conversation_id, &self.mcp_pool, binding, name, args)
            .await
            .map(|out| (out, None))
    } else {
        Err(format!("No toolset or connector provides the tool '{name}'."))
    }
}
}

/// Invoke a tool on a remote MCP server (MCP-4): reuse this run's live client for
/// the connector (LOOP-1), calling `initialize` only the first time it is seen,
/// then log the act in the visible activity log.
async fn call_mcp_tool(
    client: &reqwest::Client,
    db: &Db,
    conversation_id: &str,
    mcp_pool: &McpPool,
    binding: &McpBinding,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let mut pool = mcp_pool.lock().await;
    if !pool.contains_key(&binding.connector_id) {
        let mut mcp = if binding.transport == "stdio" {
            McpClient::new_stdio(binding.url.clone())
        } else {
            let token = secrets::get_secret(SERVICE_MCP, &binding.connector_id)
                .ok()
                .flatten();
            McpClient::new(client.clone(), binding.url.clone(), token)
        };
        // Handshake once per connector per run. If it fails, leave the slot
        // empty so a later call can retry the connection.
        mcp.initialize().await.map_err(|e| e.to_string())?;
        pool.insert(binding.connector_id.clone(), mcp);
    }
    let mcp = pool
        .get_mut(&binding.connector_id)
        .expect("just inserted or already present");
    let output = mcp.call_tool(name, args.clone()).await.map_err(|e| e.to_string())?;
    drop(pool);
    let _ = db.log_activity(
        Some(conversation_id),
        "mcp",
        &format!("{}: {name}", binding.connector_name),
    );
    Ok(output)
}

/// `RPC-1`: is there a reason a *script* may not make this call, over and above
/// the ones that apply to the model's own calls?
///
/// Exactly one: the sandbox itself. Everything else a script asks for goes
/// through the same gates the model's calls do, and a run's tool list is
/// already the only list either of them can reach. But a snippet that can start
/// another snippet is a fork bomb with a permission prompt on the outside of
/// it, and no legitimate use is lost by refusing — the script is already
/// running code.
fn script_refusal(registry: &ToolRegistry, name: &str) -> Option<String> {
    (registry.builtin_for(name) == Some(Toolset::CodeExec))
        .then(|| "A script can't start another script. Do the work in this one.".to_string())
}

/// (verb, target) for a tool call, dispatched to the owning toolset or connector.
fn describe(name: &str, args: &serde_json::Value, registry: &ToolRegistry) -> (String, String) {
    if super::results::handles(name) {
        super::results::describe(name, args)
    } else if super::plan::handles(name) {
        super::plan::describe(name, args)
    } else if let Some(toolset) = registry.builtin_for(name) {
        toolset.describe(name, args)
    } else if let Some(binding) = registry.mcp.get(name) {
        ("used".to_string(), format!("{} · {name}", binding.connector_name))
    } else {
        (name.to_string(), String::new())
    }
}

/// Trim a tool's output into a short timeline result note.
fn summarize(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lines = trimmed.lines().count();
    if lines > 1 {
        Some(format!("— {lines} lines"))
    } else {
        // Char-safe, never a byte index (`FIX-1`): a tool's one-line result
        // carries user text — the image tool's is literally the prompt — so a
        // byte cut lands inside a multi-byte character sooner or later and
        // panics the run. `ellipsize` already appends the ellipsis, and
        // returns the string untouched when it's short enough.
        Some(format!("— {}", crate::media::ellipsize(trimmed, 48)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec!["render_ui".to_string(), "web_search".to_string()]
    }

    /// A registry built the way one run builds it. `RuntimeManager::new` only
    /// stores paths, so this needs no engine and touches no process.
    fn registry_for(
        db: &Db,
        conversation_id: &str,
        ceiling: Option<&[Toolset]>,
        may_delegate: bool,
    ) -> ToolRegistry {
        let mgr = RuntimeManager::new(std::env::temp_dir().join("poiesis-test"));
        ToolRegistry::build(db, &mgr, conversation_id, ceiling, may_delegate)
    }

    fn call(id: &str, name: &str) -> ToolCallReq {
        ToolCallReq { id: id.into(), name: name.into(), arguments: "{}".into() }
    }

    /// `HRN-6`: an observer that writes down what it was told, so the watermark
    /// can be checked without a database.
    #[derive(Default)]
    struct Recorder(std::sync::Mutex<Vec<String>>);

    impl RunObserver for Recorder {
        fn appended(&self, message: &serde_json::Value) {
            self.0.lock().unwrap().push(message["content"].as_str().unwrap_or("?").to_string());
        }
    }

    /// `CTX-2`'s watermark: a message reaches the observer exactly once, however
    /// many times the loop flushes. Double-writing would put a tool result into
    /// a resumed run twice, which reads to the model as the tool having been
    /// called twice.
    #[test]
    fn every_message_reaches_the_observer_once_and_only_once() {
        let rec = Recorder::default();
        let obs: &dyn RunObserver = &rec;
        let mut messages = vec![serde_json::json!({ "role": "user", "content": "one" })];
        let mut logged = 0usize;

        flush_log(obs, &messages, &mut logged);
        // Nothing new: the second flush must be a no-op, not a repeat.
        flush_log(obs, &messages, &mut logged);
        messages.push(serde_json::json!({ "role": "assistant", "content": "two" }));
        flush_log(obs, &messages, &mut logged);

        assert_eq!(*rec.0.lock().unwrap(), vec!["one", "two"]);
        assert_eq!(logged, 2);
    }

    /// The whole point of the default methods: an observer that cares about one
    /// moment does not have to write the other three, and a run nobody is
    /// recording still runs.
    #[test]
    fn an_observer_that_wants_nothing_has_to_write_nothing() {
        let obs: &dyn RunObserver = &();
        obs.prompt(&[]);
        obs.appended(&serde_json::json!({}));
        obs.steered("keep going");
        obs.stopped("completed", 3);

        // A recorder implements only `appended`; the rest stay no-ops.
        let rec = Recorder::default();
        RunObserver::steered(&rec, "ignored");
        assert!(rec.0.lock().unwrap().is_empty());
    }

    /// `COD-12-T`: the note asks for the check when it can run, and asks for
    /// honesty when it cannot.
    #[test]
    fn the_unverified_note_asks_for_the_check_or_for_honesty() {
        let runnable = unverified_message(2, "cargo check", true);
        assert!(runnable.starts_with("You changed 2 files in this project and have not run `cargo check`"), "{runnable}");
        assert!(runnable.contains("run_task"));
        let blocked = unverified_message(1, "npm run test", false);
        assert!(blocked.contains("1 file in this project"));
        assert!(blocked.contains("have not verified"));
        assert!(!blocked.contains("run_task"), "never tell a model to use a tool it cannot");
    }

    /// A run state at the top of a fresh turn, with `logged` honest about how
    /// much of `messages` has already been written down.
    fn state(messages: Vec<serde_json::Value>) -> RunState {
        RunState {
            logged: messages.len(),
            messages,
            final_text: String::new(),
            retried: Default::default(),
            last_failure: FixTracker::default(),
            narration_nudges: 0,
            empty_retries: 0,
            plan_nudges: 0,
            free_steps: 0,
            strict_template: false,
            wrap_up: None,
            iteration: 0,
        }
    }

    fn user(text: &str) -> serde_json::Value {
        serde_json::json!({ "role": "user", "content": text })
    }

    fn planned(items: &[&str]) -> super::super::plan::Plan {
        let held = std::sync::Mutex::new(super::super::plan::Plan::default());
        super::super::plan::execute(&held, &serde_json::json!({ "items": items })).unwrap();
        held.into_inner().unwrap()
    }

    /// `PLN-T2`: the plan reaches the model on every turn, and a run without one
    /// adds nothing at all.
    ///
    /// The second half is the one worth having. Empty scaffolding — "your plan:
    /// (none)" — is an invitation to write a plan, and most requests should not
    /// have one; a five-item plan for "what is 2+2" is noise, and the cheapest
    /// way to stop it is to never mention plans to a run that has not made one.
    #[test]
    fn the_plan_reaches_the_model_each_turn_and_an_empty_one_adds_nothing() {
        let mut messages = vec![user("build the thing")];

        assert!(!show_plan(&mut messages, &super::super::plan::Plan::default()));
        assert_eq!(messages.len(), 1, "no plan means no scaffolding");

        let plan = planned(&["read the spec", "write the file"]);
        assert!(show_plan(&mut messages, &plan));
        assert_eq!(messages.len(), 2);
        assert_eq!(role(&messages[1]), "system");
        assert!(content(&messages[1]).contains("1. read the spec"));

        // …and it comes straight back off, so the next turn renders the plan as
        // it is *then* rather than stacking a second, contradicting copy.
        hide_plan(&mut messages, true);
        assert_eq!(messages, vec![user("build the thing")]);
    }

    /// The plan is rendered fresh each turn, not once: a turn that ticks an item
    /// off must be followed by a turn that sees it ticked off. A transcript that
    /// keeps saying "doing" after the work is done is the lying checklist this
    /// whole feature exists to avoid.
    #[test]
    fn the_rendered_plan_is_the_current_one_not_the_one_it_started_as() {
        let held = std::sync::Mutex::new(planned(&["read the spec", "write the file"]));

        let mut first = vec![user("go")];
        show_plan(&mut first, &held.lock().unwrap());
        assert!(content(&first[1]).contains("read the spec — to do"));

        super::super::plan::execute(
            &held,
            &serde_json::json!({ "update": { "index": 1, "status": "done" } }),
        )
        .unwrap();

        let mut second = vec![user("go")];
        show_plan(&mut second, &held.lock().unwrap());
        assert!(content(&second[1]).contains("read the spec — done"), "{}", content(&second[1]));
    }

    /// `PLN-2`: a model that writes six items, does one, and then answers is
    /// asked once to finish or to say why not.
    ///
    /// This is the bug the first real run of the feature hit: the card sat
    /// above a finished reply reading `0 of 6 done`, which is precisely the
    /// lying checklist the whole design exists to prevent. The ask is not a
    /// gate — one is the limit, and dropping an item with a reason is an
    /// accepted answer.
    #[test]
    fn answering_with_the_plan_half_done_gets_one_ask_and_only_one() {
        let plan = std::sync::Mutex::new(planned(&["research", "map", "draft"]));
        super::super::plan::execute(
            &plan,
            &serde_json::json!({ "update": { "index": 1, "status": "done" } }),
        )
        .unwrap();
        let mut st = state(vec![user("how do I adapt marketing to AI?")]);

        assert!(st.nudge_unfinished_plan(&plan.lock().unwrap(), "Here is a great question."));
        // Its own answer first, so it knows what it is continuing from, then
        // the ask — naming the items that are actually open.
        assert_eq!(role(&st.messages[1]), "assistant");
        let asked = content(&st.messages[2]);
        assert_eq!(role(&st.messages[2]), "system");
        assert!(asked.contains("- map") && asked.contains("- draft"), "{asked}");
        assert!(!asked.contains("- research"), "finished work is not open: {asked}");
        assert!(asked.contains("dropped"), "dropping an item is an accepted answer: {asked}");

        // Once. A model that means to stop will stop, and a second ask spends
        // another whole turn learning what the first already established.
        assert!(!st.nudge_unfinished_plan(&plan.lock().unwrap(), "Still done."));
    }

    /// A run that finished its plan — or never wrote one — is not asked
    /// anything. The nudge exists for a plan that is visibly untrue, not as a
    /// toll on every turn.
    #[test]
    fn a_finished_plan_and_no_plan_at_all_are_both_left_alone() {
        let mut st = state(vec![user("go")]);
        assert!(!st.nudge_unfinished_plan(&super::super::plan::Plan::default(), "Done."));

        let plan = std::sync::Mutex::new(planned(&["one", "two"]));
        for n in 1..=2 {
            super::super::plan::execute(
                &plan,
                &serde_json::json!({ "update": { "index": n, "status": "done" } }),
            )
            .unwrap();
        }
        assert!(!st.nudge_unfinished_plan(&plan.lock().unwrap(), "Done."));

        // A dropped item counts as reached: it was decided about, not skipped.
        let dropped = std::sync::Mutex::new(planned(&["one"]));
        super::super::plan::execute(
            &dropped,
            &serde_json::json!({ "update": { "index": 1, "status": "dropped", "why": "not needed" } }),
        )
        .unwrap();
        assert!(!st.nudge_unfinished_plan(&dropped.lock().unwrap(), "Done."));
        assert_eq!(st.messages.len(), 1, "nothing was added to the transcript");
    }

    /// `PLN-2`: keeping the plan honest must not cost the work its budget.
    ///
    /// The run this pins came back with `3 of 6 done` and "I stopped at my step
    /// limit": four of its twelve steps had gone on `plan` calls that did
    /// nothing but tick boxes. A feature meant to show you the work was eating
    /// the work. A pure-bookkeeping turn is therefore free — bounded, so a model
    /// that only ever plans still terminates — while a turn that plans *and*
    /// works is charged, because it did work.
    #[test]
    fn keeping_the_plan_current_does_not_spend_the_step_budget() {
        let plan_call = |id: &str| ToolCallReq {
            id: id.into(),
            name: "plan".into(),
            arguments: r#"{"update":{"index":1,"status":"done"}}"#.into(),
        };
        assert!(is_bookkeeping(&[plan_call("a"), plan_call("b")]));
        // Mixed with real work, the turn is work and pays for itself.
        assert!(!is_bookkeeping(&[plan_call("a"), call("b", "web_search")]));
        // An empty batch is not a free turn either.
        assert!(!is_bookkeeping(&[]));

        let mut st = state(vec![user("build it")]);
        st.iteration = 10;
        assert_eq!(st.charged(), 10, "with nothing free, every turn counts");
        st.free_steps = 4;
        assert_eq!(st.charged(), 6, "the four bookkeeping turns are not work");

        // The allowance is bounded: a model that does nothing but plan cannot
        // buy itself an unbounded run.
        st.free_steps = MAX_FREE_PLAN_STEPS;
        st.iteration = MAX_FREE_PLAN_STEPS;
        assert_eq!(st.charged(), 0);
        st.iteration += 1;
        assert_eq!(st.charged(), 1, "past the allowance, turns are charged again");
    }

    /// `PLN-T5`: a run that stopped at its budget says which items it never
    /// reached. The wrap-up turn is where the answer gets written, so that is
    /// where it has to be told — "I stopped at my step limit" is honest; "these
    /// three are done, these two are not" is useful.
    #[test]
    fn a_run_out_of_budget_is_told_what_it_never_reached() {
        let held = std::sync::Mutex::new(planned(&["one", "two", "three"]));
        super::super::plan::execute(
            &held,
            &serde_json::json!({ "update": { "index": 1, "status": "done" } }),
        )
        .unwrap();

        let line = super::super::plan::wrap_up_message(&held.lock().unwrap())
            .expect("something is outstanding");
        let said = content(&line);
        assert!(said.contains("- two") && said.contains("- three"), "{said}");
        assert!(!said.contains("- one"), "finished work is not outstanding: {said}");

        // A plan that was finished says nothing — a run that did everything it
        // meant to has no shortfall to confess.
        for n in 1..=3 {
            super::super::plan::execute(
                &held,
                &serde_json::json!({ "update": { "index": n, "status": "done" } }),
            )
            .unwrap();
        }
        assert!(super::super::plan::wrap_up_message(&held.lock().unwrap()).is_none());
    }

    /// An observer that writes down the steers it was told about, so the order
    /// of the two records can be compared.
    #[derive(Default)]
    struct Steers(std::sync::Mutex<Vec<String>>);

    impl RunObserver for Steers {
        fn steered(&self, text: &str) {
            self.0.lock().unwrap().push(text.to_string());
        }
    }

    fn role(m: &serde_json::Value) -> &str {
        m["role"].as_str().unwrap_or("?")
    }

    fn content(m: &serde_json::Value) -> &str {
        m["content"].as_str().unwrap_or("")
    }

    /// `HRN-T2`: something typed while the run is working becomes a plain user
    /// message at the top of the next turn, in the order it was said, and is
    /// taken from the inbox exactly once.
    ///
    /// The place matters more than it looks. The transcript the model sees has
    /// to stay a valid conversation: a user message dropped between an assistant
    /// turn that asked for tools and the results it is waiting for gives the
    /// model a shape no provider promises to accept, and some refuse outright.
    /// Draining at the top is what keeps that from happening.
    #[test]
    fn a_mid_run_instruction_lands_as_a_user_message_at_the_top_of_the_next_turn() {
        let fleet = Fleet::new();
        let run = fleet.open("conv", CancelFlag::default(), None, 0);
        let limits = RunLimits::default();
        let obs = Steers::default();

        // The transcript is mid-work: the model has already spoken once.
        let mut st = state(vec![
            user("summarise the report"),
            serde_json::json!({ "role": "assistant", "content": "reading it now" }),
        ]);

        run.steer(super::super::fleet::Steer::user("only the last quarter"));
        run.steer(super::super::fleet::Steer::user("and keep it short"));

        let announced = open_turn(&run, &limits, true, &mut st, &obs);

        assert_eq!(announced, vec!["only the last quarter", "and keep it short"]);
        assert_eq!(*obs.0.lock().unwrap(), announced, "the log hears the same words, in the same order");

        // Appended, in order, as ordinary user turns — not merged into one, not
        // rewritten, and not put anywhere but the end.
        assert_eq!(st.messages.len(), 4);
        assert_eq!(role(&st.messages[2]), "user");
        assert_eq!(content(&st.messages[2]), "only the last quarter");
        assert_eq!(role(&st.messages[3]), "user");
        assert_eq!(content(&st.messages[3]), "and keep it short");

        // The inbox is now empty: a second turn must not replay what was already
        // said, which to the model reads as the person repeating themselves.
        let again = open_turn(&run, &limits, true, &mut st, &obs);
        assert!(again.is_empty());
        assert_eq!(st.messages.len(), 4);
    }

    /// A parent redirecting a child is marked as a redirection; a person's own
    /// words are not. To the model, what the user typed a moment ago and what
    /// they typed mid-run are the same kind of thing, and dressing one up as an
    /// announcement makes it read like a system instruction.
    #[test]
    fn a_lead_redirecting_a_child_is_labelled_but_a_person_is_not() {
        let fleet = Fleet::new();
        let run = fleet.open("conv", CancelFlag::default(), None, 0);
        let mut st = state(vec![user("go")]);

        run.steer(super::super::fleet::Steer::user("stop at page two"));
        run.steer(super::super::fleet::Steer {
            text: "look at the appendix instead".into(),
            from: super::super::fleet::SteerSource::Lead,
        });
        open_turn(&run, &RunLimits::default(), true, &mut st, &());

        assert_eq!(content(&st.messages[1]), "stop at page two");
        assert_eq!(content(&st.messages[2]), "New instruction: look at the appendix instead");
    }

    /// `HRN-T4`: running out of steps sets up one tool-free closing turn instead
    /// of aborting.
    ///
    /// This is the difference between a run that ends with an answer and a run
    /// that ends with an error after doing all the work. The last iteration is
    /// spent asking the model to say what it found, so the person gets the
    /// partial result rather than nothing.
    #[test]
    fn running_out_of_steps_buys_one_closing_turn_rather_than_an_abort() {
        let fleet = Fleet::new();
        let run = fleet.open("conv", CancelFlag::default(), None, 0);
        let limits = RunLimits { max_iterations: 3, ..RunLimits::default() };
        let mut st = state(vec![user("find it")]);

        // Two turns with room to spare: nothing is added and no tools are taken
        // away.
        for _ in 0..2 {
            open_turn(&run, &limits, true, &mut st, &());
            assert!(st.wrap_up.is_none());
            assert_eq!(st.messages.len(), 1);
            st.iteration += 1;
        }

        // The third would be the last, so it is the closing one.
        open_turn(&run, &limits, true, &mut st, &());
        assert_eq!(st.wrap_up, Some(StopReason::MaxSteps));
        assert_eq!(st.messages.len(), 2);
        assert_eq!(role(&st.messages[1]), "system");
        assert_eq!(content(&st.messages[1]), WRAP_UP_PROMPT);

        // And exactly one. Asking twice would spend the closing turn on reading
        // the same instruction again.
        st.iteration += 1;
        open_turn(&run, &limits, true, &mut st, &());
        assert_eq!(st.messages.len(), 2);
    }

    /// `SUB-T7`: a deadline that has passed does the same thing as a spent step
    /// budget, and says so as a timeout. A delegated child has a clock precisely
    /// because nobody is watching it, so it is the run most likely to reach this.
    #[test]
    fn a_run_that_is_out_of_time_also_gets_its_closing_turn() {
        let fleet = Fleet::new();
        let run = fleet.open("conv", CancelFlag::default(), None, 0);
        let limits = RunLimits {
            max_iterations: 50,
            deadline: Some(std::time::Instant::now() - std::time::Duration::from_secs(1)),
            max_depth: 0,
        };
        let mut st = state(vec![user("find it")]);

        open_turn(&run, &limits, true, &mut st, &());

        assert_eq!(st.wrap_up, Some(StopReason::Timeout));
        assert_eq!(content(&st.messages[1]), WRAP_UP_PROMPT);
    }

    /// A turn with no tools has no budget to spend, so it is never told to wrap
    /// up. A plain chat reply would otherwise be handed a closing instruction it
    /// was never working towards.
    #[test]
    fn a_run_without_tools_is_never_told_to_wrap_up() {
        let fleet = Fleet::new();
        let run = fleet.open("conv", CancelFlag::default(), None, 0);
        let limits = RunLimits { max_iterations: 1, ..RunLimits::default() };
        let mut st = state(vec![user("hello")]);

        open_turn(&run, &limits, false, &mut st, &());

        assert!(st.wrap_up.is_none());
        assert_eq!(st.messages.len(), 1);
    }

    fn tool_round() -> Vec<serde_json::Value> {
        let calls = vec![call("c1", "delegate")];
        vec![
            user("compare three frameworks, one agent each"),
            assistant_tool_call_message(&calls),
            tool_result_message("c1", "Agent 1 (general) — done in 41s, 6 steps\nAxum routes with…"),
        ]
    }

    /// The failure this exists for: three agents come back with their reports,
    /// the model then produces an empty turn, and the run ends showing nothing.
    /// Everything the agents found is in the transcript, so it is asked for.
    #[test]
    fn a_run_that_did_work_and_then_said_nothing_is_asked_for_the_answer() {
        let mut st = state(tool_round());

        assert!(st.rescue_empty_answer(false), "the run goes round again rather than ending empty");
        assert_eq!(st.messages.len(), 4);
        assert_eq!(role(&st.messages[3]), "system");
        assert_eq!(content(&st.messages[3]), ANSWER_NOW_PROMPT);
        // The results it is being told to use are still above it.
        assert!(st.did_tool_work());
    }

    /// A reasoning model that spent the whole turn thinking and wrote no reply.
    /// Nothing is lost from the transcript, but the user waited minutes for a
    /// blank turn, which reads as the app having broken rather than the model
    /// having put its output somewhere the reader cannot see.
    ///
    /// This is also where a runaway think lands. `THINKING_BUDGET` in
    /// `proxy.rs` cuts a stream that has thought for two minutes without a word
    /// of answer (seen at ten minutes and 100k characters), which produces
    /// exactly this shape — empty content, plenty of reasoning — so the model
    /// gets asked once for the answer instead of being left to loop.
    #[test]
    fn a_turn_that_only_thought_is_asked_to_write_the_answer_down() {
        let mut st = state(vec![user("create a playable pacman game")]);

        assert!(st.rescue_empty_answer(true));
        assert_eq!(content(&st.messages[1]), FINISH_THINKING_PROMPT);
    }

    /// The two instructions are not interchangeable. A run with tool results is
    /// told to use them; a run with none must never be, because there are none
    /// to use and the model would go looking for something that is not there.
    #[test]
    fn a_thinking_run_that_also_used_tools_is_pointed_at_the_results() {
        let mut st = state(tool_round());
        assert!(st.rescue_empty_answer(true));
        assert_eq!(content(&st.messages[3]), ANSWER_NOW_PROMPT);
    }

    /// Asked once, never twice. A model with nothing to say will not have more
    /// on the second try, and each attempt costs the user a whole turn.
    #[test]
    fn the_answer_is_asked_for_once_and_not_again() {
        let mut st = state(tool_round());
        assert!(st.rescue_empty_answer(false));
        assert!(!st.rescue_empty_answer(false), "the second empty turn ends the run");
        assert_eq!(st.messages.len(), 4, "no second instruction was appended");
    }

    /// A model that did nothing and thought nothing is simply a model that said
    /// nothing. There is no work to rescue, and the chat already states that
    /// outright — spending a turn asking again would only make the wait longer.
    #[test]
    fn a_run_with_nothing_behind_it_is_left_to_end() {
        let mut st = state(vec![user("hello")]);
        assert!(!st.rescue_empty_answer(false));
        assert_eq!(st.messages.len(), 1);
    }

    /// Prose already written is an answer, however short. Asking again would
    /// append a second answer to the first.
    #[test]
    fn a_run_that_answered_is_never_asked_again() {
        let mut st = state(tool_round());
        st.final_text = "Axum uses a Router; Actix and Rocket use macros.".into();
        assert!(!st.rescue_empty_answer(false));
    }

    /// Whitespace is not an answer. A model that emits a newline and stops
    /// leaves exactly as blank a turn as one that emits nothing.
    #[test]
    fn whitespace_does_not_count_as_having_answered() {
        let mut st = state(tool_round());
        st.final_text = "\n  \n".into();
        assert!(st.rescue_empty_answer(false));
    }

    /// A conversation with the sandbox switched on, which is the only state in
    /// which `RPC` means anything.
    fn db_with_sandbox() -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        Toolset::CodeExec.set_enabled(&db, true);
        let conv = db.create_conversation("Scripts", None, false).unwrap();
        (db, conv.id)
    }

    /// `RPC-T1`: a script reaches the run's own tools and no others, and the
    /// one thing held back is the sandbox itself — a snippet that can start a
    /// snippet is a fork bomb behind a single permission prompt.
    #[test]
    fn a_script_may_use_the_runs_tools_but_never_start_another_script() {
        let (db, conv) = db_with_sandbox();
        let registry = registry_for(&db, &conv, None, false);
        assert!(script_refusal(&registry, "run_code").is_some());
        assert!(script_refusal(&registry, "read_file").is_none());
        // A tool this run does not have needs no special refusal here: dispatch
        // already says so, and in its own words.
        assert!(script_refusal(&registry, "nosuchtool").is_none());
    }

    /// `RPC-T1` again, from the other side: a tool the *user* switched off is
    /// not in the registry at all, so a script cannot reach it either.
    #[test]
    fn a_script_cannot_reach_a_toolset_the_user_switched_off() {
        let (db, conv) = db_with_sandbox();
        Toolset::WebSearch.set_enabled(&db, false);
        let registry = registry_for(&db, &conv, None, false);
        assert!(registry.builtin_for("web_search").is_none());
    }

    /// `RPC-3`: the loop-over-many-items shape is taught only while the ability
    /// is on. Describing a capability that is switched off costs a wasted step
    /// to find that out.
    #[test]
    fn the_loop_shape_is_taught_only_while_scripts_may_call_tools() {
        let (db, conv) = db_with_sandbox();
        let description = |registry: &ToolRegistry| -> String {
            registry
                .specs
                .iter()
                .find(|s| s.pointer("/function/name").and_then(|n| n.as_str()) == Some("run_code"))
                .and_then(|s| s.pointer("/function/description"))
                .and_then(|d| d.as_str())
                .unwrap_or_default()
                .to_string()
        };

        let off = description(&registry_for(&db, &conv, None, false));
        assert!(!off.is_empty(), "the sandbox is on, so run_code is advertised");
        assert!(!off.contains("from poiesis import tool"));

        db.set_setting(crate::agent::toolrpc::SETTING_KEY, "true").unwrap();
        let on = description(&registry_for(&db, &conv, None, false));
        assert!(on.starts_with(&off), "the guidance is added, never a replacement");
        assert!(on.contains("from poiesis import tool"));
    }

    /// `HRN-T3`: the partition is the whole of `HRN-4`'s correctness. Reads go
    /// together; anything that writes, drives a live session, or that we cannot
    /// place at all waits its turn.
    #[test]
    fn reads_run_together_and_writes_wait_their_turn() {
        let db = Db::open_in_memory().unwrap();
        Toolset::WebSearch.set_enabled(&db, true);
        Toolset::Browser.set_enabled(&db, true);
        let conv = db.create_conversation("c", None, false).unwrap();
        let registry = registry_for(&db, &conv.id, None, true);

        let calls = vec![
            call("a", "web_search"),
            call("b", "write_file"),
            call("c", "read_file"),
            call("d", "browse"),
            call("e", "search_history"),
            call("f", "some_mcp_tool_we_cannot_see_inside"),
        ];
        let (concurrent, ordered) = partition_calls(&registry, &calls);
        assert_eq!(concurrent, vec![0, 2, 4], "reads");
        assert_eq!(ordered, vec![1, 3, 5], "a write, a live session, and an unknown");
    }

    /// One eligible call is not a batch: running it down the parallel path
    /// would announce "1 thing at once", which is just a step.
    #[test]
    fn a_lone_read_is_not_a_parallel_group() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("c", None, false).unwrap();
        let registry = registry_for(&db, &conv.id, None, true);
        let calls = vec![call("a", "read_file"), call("b", "write_file")];
        let (concurrent, ordered) = partition_calls(&registry, &calls);
        assert!(concurrent.is_empty());
        assert_eq!(ordered, vec![0, 1], "and still in the order the model asked");
    }

    /// `SUB-T5`: at max depth the `delegate` tool is not merely refused, it is
    /// never offered — a model cannot be tempted by a tool it never sees.
    #[test]
    fn a_run_with_no_room_below_it_is_never_shown_the_delegate_tool() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("c", None, false).unwrap();

        let offered = registry_for(&db, &conv.id, None, true);
        assert!(offered.tool_names().iter().any(|n| n == "delegate"));

        let child = registry_for(&db, &conv.id, None, false);
        assert!(!child.tool_names().iter().any(|n| n == "delegate"));
        assert!(!child.toolsets.contains(&Toolset::Subagents));
    }

    /// `SUB-T6`: a child is never more powerful than its parent. A persona that
    /// allows the Browser gets nothing when the parent didn't have it.
    #[test]
    fn a_child_persona_cannot_reach_past_its_parents_toolsets() {
        let db = Db::open_in_memory().unwrap();
        Toolset::Browser.set_enabled(&db, true);
        let persona = db
            .create_persona(&crate::db::NewPersona {
                name: "browsing agent".into(),
                system_prompt: "You browse.".into(),
                model_id: None,
                params_json: None,
                tools_json: Some(r#"["browser","filesystem"]"#.into()),
                skills_json: None,
                description: None,
                spawnable: true,
            })
            .unwrap();
        let conv = db.create_conversation("child", None, false).unwrap();
        db.set_conversation_persona(&conv.id, Some(&persona.id), None).unwrap();

        // On its own the persona reaches both.
        let unbounded = registry_for(&db, &conv.id, None, false);
        assert!(unbounded.toolsets.contains(&Toolset::Browser));
        assert!(unbounded.toolsets.contains(&Toolset::FileSystem));

        // Under a parent that only had file access, the Browser is gone.
        let ceiling = [Toolset::FileSystem];
        let bounded = registry_for(&db, &conv.id, Some(&ceiling), false);
        assert_eq!(bounded.toolsets, vec![Toolset::FileSystem]);
    }

    fn tool_names() -> Vec<String> {
        registry().tool_names()
    }

    /// A registry with the two toolsets these tests exercise, built without a
    /// database so the parsing rules can be tested on their own.
    fn registry() -> ToolRegistry {
        let toolsets = vec![Toolset::Skills, Toolset::WebSearch];
        ToolRegistry {
            specs: toolsets.iter().flat_map(|s| s.tool_specs()).collect(),
            toolsets,
            mcp: HashMap::new(),
            skills: vec!["content-research-writer".to_string()],
        }
    }

    /// What a local Gemma actually writes once it has read the prompt's
    /// "Skills available" list: the *skill's* name where the tool belongs, with
    /// an invented `:verb` and invented arguments. The skill exists and loading
    /// it is read-only, so the meant call is unambiguous.
    #[test]
    fn a_skill_named_as_though_it_were_a_tool_becomes_a_skill_call() {
        let line = r#"content-research-writer:outline {topic: "How will AI impact the job market?"}"#;
        let calls = parse_text_tool_calls(&format!("```tool\n{line}\n```"), &registry()).expect("parses");
        assert_eq!(calls[0].name, "skill");
        assert_eq!(calls[0].arguments, r#"{"name":"content-research-writer"}"#);
    }

    /// The same confusion in JSON form.
    #[test]
    fn a_skill_named_in_json_becomes_a_skill_call() {
        let calls = parse_text_tool_calls(
            r#"{"name": "content-research-writer", "parameters": {"topic": "x"}}"#,
            &registry(),
        )
        .expect("parses");
        assert_eq!(calls[0].name, "skill");
        assert_eq!(calls[0].arguments, r#"{"name":"content-research-writer"}"#);
    }

    /// A skill name is only invocable while the `skill` tool is on offer —
    /// switching the Skills toolset off must close the back door too.
    #[test]
    fn skills_are_not_invocable_when_the_toolset_is_off() {
        let toolsets = vec![Toolset::WebSearch];
        let without = ToolRegistry {
            specs: toolsets.iter().flat_map(|s| s.tool_specs()).collect(),
            toolsets,
            mcp: HashMap::new(),
            skills: vec!["content-research-writer".to_string()],
        };
        assert!(parse_text_tool_calls("content-research-writer:outline {}", &without).is_none());
    }

    /// A skill name is kebab-case, so the narration check has to keep hyphens
    /// as word characters — otherwise the nudge never fires for exactly the
    /// turns that need it.
    #[test]
    fn narration_matches_a_kebab_case_skill_name() {
        let invocable = registry().invocable_names();
        assert!(narrates_a_tool_call("```tool\nsome-unknown-thing content-research-writer\n```", &invocable));
        assert!(!narrates_a_tool_call("I'll write some content research for you.", &invocable));
    }

    /// The local-Gemma transcript this shipped for. The model opened in prose,
    /// so `streaming_live` latched on; the fenced call that followed used to be
    /// streamed to the user as text and end the run with nothing done.
    #[test]
    fn holds_back_a_fence_that_opens_after_prose() {
        let turn = "Okay. Let's explore this topic.\n\nFirst, I'll use the tool.\n\n```tool\nskill content-research-writer\n```";
        let cut = safe_prefix(turn, &tool_names());
        let (shown, held) = turn.split_at(cut);
        assert!(shown.ends_with("I'll use the tool.\n\n"), "prose is still shown live: {shown:?}");
        assert!(held.starts_with("```tool"), "the call is withheld: {held:?}");

        let calls = parse_text_tool_calls(held, &registry()).expect("the held tail parses as a call");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "skill");
        assert_eq!(calls[0].arguments, r#"{"name":"content-research-writer"}"#);
    }

    /// An ordinary code block names no tool, so it must keep streaming live —
    /// the hold-back is not "any fence", or every answer containing code would
    /// stall until the turn ended.
    #[test]
    fn releases_a_plain_code_fence_and_holds_a_tool_one() {
        let closed = "Here is code:\n\n```py\nprint(1)\n```\n\nThat's it.";
        assert_eq!(safe_prefix(closed, &tool_names()), closed.len(), "a plain code block is shown");

        let then_open = format!("{closed}\n\n```json\n{{\"name\": \"web_search\"");
        let cut = safe_prefix(&then_open, &tool_names());
        assert!(then_open[cut..].starts_with("```json"));
    }

    /// A fence stays held after it closes — a closed fence is precisely when we
    /// can finally tell what it was, so releasing it on the closing ``` would
    /// put the call on screen a moment before we could catch it.
    #[test]
    fn keeps_holding_a_tool_fence_after_it_closes() {
        let turn = "Here goes.\n\n```tool\nskill x\n```\n";
        let cut = safe_prefix(turn, &tool_names());
        assert_eq!(&turn[..cut], "Here goes.\n\n");
    }

    /// The other shape a call arrives in mid-prose: a bare object on its own
    /// line. Held from the start of that line, not from the brace.
    #[test]
    fn holds_back_a_line_initial_json_object() {
        let turn = "I'll search for that.\n{\"name\": \"web_search\", \"parameters\": {\"query\": \"x\"}}";
        let cut = safe_prefix(turn, &tool_names());
        assert_eq!(&turn[..cut], "I'll search for that.\n");

        let calls = parse_text_tool_calls(&turn[cut..], &registry()).expect("parses");
        assert_eq!(calls[0].name, "web_search");
    }

    /// Prose that merely contains a brace mid-sentence is not a tool call and
    /// must keep streaming — the hold-back only fires on a line-initial one.
    #[test]
    fn ordinary_prose_streams_whole() {
        let turn = "Use the {placeholder} syntax to interpolate.";
        assert_eq!(safe_prefix(turn, &tool_names()), turn.len());
    }

    /// `skill <name>` with no JSON anywhere is what a 4B local model produces.
    /// The bare argument binds to the tool's single required property.
    #[test]
    fn parses_a_bare_invocation_line() {
        let calls = parse_text_tool_calls("skill content-research-writer", &registry()).expect("parses");
        assert_eq!(calls[0].name, "skill");
        assert_eq!(calls[0].arguments, r#"{"name":"content-research-writer"}"#);

        let fenced = parse_text_tool_calls("```\nskill(content-research-writer)\n```", &registry()).expect("parses");
        assert_eq!(fenced[0].arguments, r#"{"name":"content-research-writer"}"#);
    }

    /// The guard that keeps the bare-line parser from eating prose: a sentence
    /// mentioning a tool is not an invocation, and neither is a name alone.
    #[test]
    fn a_sentence_about_a_tool_is_not_a_call() {
        let r = registry();
        assert!(parse_text_tool_calls("I could use skill to read the instructions first.", &r).is_none());
        assert!(parse_text_tool_calls("skill", &r).is_none());
        assert!(parse_text_tool_calls("Skills are useful here.", &r).is_none());
        // The case that forced the "fenced, or nothing but the call" guard: a
        // paragraph whose line happens to open on a tool's name.
        let paragraph = "Here is how it works.\nweb_search works.\nThat is all.";
        assert!(parse_text_tool_calls(paragraph, &r).is_none());
    }

    /// The nudge fires only when an advertised tool name appears as *code* —
    /// otherwise every answer that discusses the agent's own tools would loop.
    #[test]
    fn narration_is_detected_only_inside_code_markup() {
        assert!(narrates_a_tool_call("I'll use `web_search` now.", &names()));
        assert!(narrates_a_tool_call("Doing it:\n```tool\nweb_search(...)\n```", &names()));
        assert!(!narrates_a_tool_call("I will search the web for you.", &names()));
        assert!(!narrates_a_tool_call("You can enable web_search in Settings.", &names()));
    }

    /// Every message role the loop appends, in the order a real tool round
    /// produces them. Gemma 3's template raises on all three of the shapes
    /// after the first user turn, which is why the run died on request two.
    #[test]
    fn flattens_a_tool_round_into_alternating_turns() {
        let calls = vec![ToolCallReq {
            id: "call_1".into(),
            name: "skill".into(),
            arguments: r#"{"name":"content-research-writer"}"#.into(),
        }];
        let out = flatten_to_alternating(&[
            msg("system", "You are Poiesis."),
            msg("user", "how will ai influence the job market?"),
            assistant_tool_call_message(&calls),
            tool_result_message("call_1", "Skill loaded: research first, then outline."),
            msg("system", "Fix the previous tool call: bad path."),
        ]);

        assert_eq!(roles(&out), ["system", "user", "assistant", "user"]);
        // The call and its result survive as text — flattening must not cost
        // the model the memory of what it did.
        assert!(out[2]["content"].as_str().unwrap().contains("I called skill with"));
        let last = out[3]["content"].as_str().unwrap();
        assert!(last.contains("[skill returned]"), "the result names its tool: {last}");
        assert!(last.contains("research first"));
        // The GRM-3 nudge folded into the same user turn rather than becoming
        // a third consecutive message.
        assert!(last.contains("Fix the previous tool call"));
    }

    /// The alternation the template actually checks: user at even indices,
    /// assistant at odd, counting from after a leading system message.
    #[test]
    fn flattened_output_strictly_alternates() {
        let calls = vec![ToolCallReq { id: "c1".into(), name: "web_search".into(), arguments: "{}".into() }];
        let out = flatten_to_alternating(&[
            msg("system", "s"),
            msg("user", "a"),
            assistant_tool_call_message(&calls),
            tool_result_message("c1", "r1"),
            assistant_tool_call_message(&calls),
            tool_result_message("c1", "r2"),
        ]);
        let body = &out[1..];
        for (i, m) in body.iter().enumerate() {
            let want = if i % 2 == 0 { "user" } else { "assistant" };
            assert_eq!(m["role"], want, "index {i} broke alternation: {out:#?}");
        }
    }

    /// An already-alternating transcript must come through unchanged in
    /// substance — the retry re-sends the same conversation, not a lesser one.
    #[test]
    fn flattening_a_plain_conversation_keeps_every_word() {
        let out = flatten_to_alternating(&[
            msg("system", "s"),
            msg("user", "question"),
            msg("assistant", "answer"),
        ]);
        assert_eq!(roles(&out), ["system", "user", "assistant"]);
        assert_eq!(out[1]["content"], "question");
        assert_eq!(out[2]["content"], "answer");
    }

    /// llama-server's 400 quotes the template's own exception; that text is the
    /// only signal there is that flattening is what's needed.
    #[test]
    fn recognises_the_template_rejection() {
        let err = |m: &str| ProxyError::Api { status: 400, message: m.to_string() };
        assert!(is_template_error(&err(
            "400 — Unable to generate parser for this template. Automatic parser generation failed: Error: Jinja Exception: Conversation roles must alternate user/assistant/user/assistant/..."
        )));
        assert!(!is_template_error(&err("400 — context window exceeded")));
        assert!(!is_template_error(&err("404 — No endpoints found that support tool use.")));
    }

    #[test]
    fn strips_any_fence_info_string() {
        assert_eq!(strip_code_fence("```tool\nskill x\n```"), "skill x");
        assert_eq!(strip_code_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fence("```\nplain\n```"), "plain");
        assert_eq!(strip_code_fence("no fence here"), "no fence here");
    }

    fn msg(role: &str, content: &str) -> serde_json::Value {
        serde_json::json!({ "role": role, "content": content })
    }

    fn roles(msgs: &[serde_json::Value]) -> Vec<&str> {
        msgs.iter().map(|m| m["role"].as_str().unwrap()).collect()
    }

    /// The transcript this shipped for: the engine failed three times, so three
    /// user messages piled up with no replies between them. Every later attempt
    /// then died on the template rather than on the original fault.
    #[test]
    fn folds_the_user_turns_a_failing_engine_left_unanswered() {
        let out = normalize_transcript(vec![
            msg("system", "You are Poiesis."),
            msg("user", "what is the impact of ai on digital work tools?"),
            msg("user", "try again"),
            msg("user", "again"),
        ]);
        assert_eq!(roles(&out), ["system", "user"]);
        // Nothing the user typed may be lost in the folding.
        let text = out[1]["content"].as_str().unwrap();
        for said in ["impact of ai", "try again", "again"] {
            assert!(text.contains(said), "dropped {said:?} from: {text}");
        }
    }

    #[test]
    fn leaves_an_already_alternating_transcript_untouched() {
        let original = vec![
            msg("system", "s"),
            msg("user", "a"),
            msg("assistant", "b"),
            msg("user", "c"),
        ];
        assert_eq!(normalize_transcript(original.clone()), original);
    }

    /// A system message mid-conversation is fine — llama-server hoists those
    /// out of the alternation — so it must not be treated as a turn, nor block
    /// the fold of the user turns it sits between.
    #[test]
    fn system_messages_are_left_where_they_are() {
        let out = normalize_transcript(vec![
            msg("user", "a"),
            msg("system", "note"),
            msg("user", "b"),
        ]);
        assert_eq!(roles(&out), ["user", "system", "user"]);
    }

    #[test]
    fn drops_the_empty_assistant_turns_a_failure_leaves_behind() {
        let out = normalize_transcript(vec![
            msg("user", "a"),
            msg("assistant", "   "),
            msg("user", "b"),
        ]);
        assert_eq!(roles(&out), ["user"]);
        assert_eq!(out[0]["content"], "a\n\nb");
    }

    #[test]
    fn drops_an_assistant_turn_that_precedes_any_user_turn() {
        let out = normalize_transcript(vec![
            msg("system", "s"),
            msg("assistant", "unprompted hello"),
            msg("user", "a"),
        ]);
        assert_eq!(roles(&out), ["system", "user"]);
    }

    /// The loop's own bookkeeping — an assistant message whose payload is
    /// `tool_calls`, and the `tool` results answering it — is a valid sequence
    /// that only looks like a repeat. Folding it would destroy the call ids.
    #[test]
    fn never_folds_the_loops_own_tool_call_bookkeeping() {
        let calls = vec![ToolCallReq {
            id: "call_1".into(),
            name: "web_search".into(),
            arguments: "{}".into(),
        }];
        let original = vec![
            msg("user", "a"),
            assistant_tool_call_message(&calls),
            tool_result_message("call_1", "result"),
            assistant_tool_call_message(&calls),
        ];
        assert_eq!(normalize_transcript(original.clone()), original);
    }

    /// `FIX-1`, the site the plan's three didn't cover: every tool result is
    /// summarised here, and the image tool's result carries the user's prompt.
    /// A byte cut at 48 landed inside a multi-byte character and panicked the
    /// whole run — for this user, on an ordinary German prompt.
    #[test]
    fn summarize_cuts_on_char_boundaries_not_bytes() {
        let output = "Generated an image for \"Zeichne eine Straße bei Nacht\" and opened it.";
        let note = summarize(output).expect("a one-line result summarises");
        assert!(note.starts_with("— Generated an image"));
        assert!(note.ends_with('…'), "a cut result says so: {note}");
    }

    #[test]
    fn summarize_leaves_a_short_result_whole() {
        assert_eq!(summarize("wrote 3 files").as_deref(), Some("— wrote 3 files"));
        assert_eq!(summarize("   ").as_deref(), None);
        assert_eq!(summarize("one\ntwo").as_deref(), Some("— 2 lines"));
    }

    /// `FIX-1`: a pair is written only when the *same* tool that failed later
    /// succeeds in the *same* run.
    #[test]
    fn a_fix_pair_needs_the_same_tool_to_fail_then_succeed() {
        let mut t = FixTracker::default();
        t.failed("read_file", r#"{"path":"/etc/hosts"}"#, "path outside the working folder");

        // A *different* tool succeeding is not a correction of `read_file`.
        assert!(t.succeeded("web_search").is_none(), "another tool's success proves nothing");

        let (args, err) = t.succeeded("read_file").expect("the same tool succeeding is the fix");
        assert_eq!(args, r#"{"path":"/etc/hosts"}"#);
        assert_eq!(err, "path outside the working folder");
    }

    /// A run of failures teaches nothing except that the tool is broken, which
    /// is `HEAL-2`'s job — no `tool_fixes` row may come out of it.
    #[test]
    fn all_fail_and_never_succeed_records_nothing() {
        let mut t = FixTracker::default();
        t.failed("run_code", "{}", "timed out");
        t.failed("run_code", "{}", "timed out again");
        // The run ends here: nothing ever asked for a pair, so nothing is written.
        assert_eq!(t.0.len(), 1, "the pending failure is held, not recorded");
    }

    #[test]
    fn one_failure_yields_at_most_one_pair() {
        let mut t = FixTracker::default();
        t.failed("read_file", "bad", "nope");
        assert!(t.succeeded("read_file").is_some());
        assert!(
            t.succeeded("read_file").is_none(),
            "later successes must not each re-record the same mistake"
        );
    }

    #[test]
    fn the_most_recent_wrong_approach_is_the_one_paired() {
        let mut t = FixTracker::default();
        t.failed("read_file", "first", "e1");
        t.failed("read_file", "second", "e2");
        let (args, err) = t.succeeded("read_file").unwrap();
        assert_eq!(args, "second");
        assert_eq!(err, "e2");
    }

    #[test]
    fn early_flush_streams_plain_prose() {
        assert!(should_flush_prose("Here's what I found", &names()));
        assert!(should_flush_prose("I", &names()));
    }

    #[test]
    fn early_flush_holds_anything_that_could_be_a_tool_call() {
        // JSON / array openers, fences and reasoning preambles keep buffering,
        // even part-way through a long emission.
        assert!(!should_flush_prose("{\"name\": \"render", &names()));
        assert!(!should_flush_prose("[{\"name\"", &names()));
        assert!(!should_flush_prose("```json", &names()));
        assert!(!should_flush_prose("<think>let me", &names()));
        assert!(!should_flush_prose(&format!("{{{}", "x".repeat(400)), &names()));
        // A known tool name anywhere in the buffer is enough to keep waiting.
        assert!(!should_flush_prose("I will call web_search now", &names()));
        assert!(!should_flush_prose("", &names()));
    }

    #[test]
    fn early_flush_waits_out_an_ambiguous_opener() {
        // Starts on a digit — could be prose or could be anything, so hold until
        // there is enough text to be confident.
        assert!(!should_flush_prose("1. first", &names()));
        assert!(should_flush_prose(&format!("1. {}", "word ".repeat(60)), &names()));
    }
}
