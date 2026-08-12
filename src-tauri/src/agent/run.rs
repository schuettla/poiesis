//! The agent loop (PRD §7.5): drive model turns, dispatch tool calls to built-in
//! toolsets, feed results back, and emit a visible step timeline — until the model
//! produces a final answer.

use std::collections::HashMap;

use tauri::ipc::Channel;

use crate::cloud::{drive_turn, ChatEndpoint};
use crate::db::Db;
use crate::mcp::McpClient;
use crate::permissions::{PermissionManager, PermissionRequest};
use crate::runtime::proxy::{CancelFlag, ProxyError, ToolCallReq, TurnOutcome};
use crate::runtime::{EmbedManager, RerankManager, RuntimeManager};
use crate::secrets::{self, SERVICE_MCP};

use super::toolsets::{self, Toolset, ToolContext};
use crate::memory::MemoryStore;
use super::AgentEvent;

/// Cap on tool-call iterations to bound runaway loops. Blocks add present/update
/// calls that consume iterations, so this is a little higher than a plain loop.
const MAX_ITERATIONS: usize = 12;

/// How many times one run will ask a model to make a tool call for real after it
/// merely described one. Two is enough to cover a slip; a model that genuinely
/// cannot emit tool calls would burn the whole iteration budget otherwise.
const MAX_NARRATION_NUDGES: usize = 2;

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
    fn build(db: &Db, mgr: &RuntimeManager, conversation_id: &str) -> Self {
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
        let enabled = toolsets::enabled_for_persona(db, persona_tools.as_deref());
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
}

impl AgentEventSink {
    pub fn new(channel: Channel<AgentEvent>) -> Self {
        Self { channel }
    }
    /// Send any event verbatim — for variants without a dedicated helper.
    pub fn emit(&self, event: AgentEvent) {
        let _ = self.channel.send(event);
    }
    pub fn token(&self, text: &str) {
        let _ = self.channel.send(AgentEvent::Token { text: text.to_string() });
    }
    pub fn step_start(&self, id: &str, verb: &str, target: &str) {
        let _ = self.channel.send(AgentEvent::StepStart {
            id: id.to_string(),
            verb: verb.to_string(),
            target: target.to_string(),
        });
    }
    pub fn step_done(&self, id: &str, result: Option<String>) {
        let _ = self.channel.send(AgentEvent::StepDone { id: id.to_string(), result });
    }
    pub fn step_error(&self, id: &str, error: &str) {
        let _ = self.channel.send(AgentEvent::StepError {
            id: id.to_string(),
            error: error.to_string(),
        });
    }
    pub fn file_changed(&self, op: &str, path: &str, undo_token: Option<&str>) {
        let _ = self.channel.send(AgentEvent::FileChanged {
            op: op.to_string(),
            path: path.to_string(),
            undo_token: undo_token.unwrap_or_default().to_string(),
        });
    }
    pub fn send_permission(&self, request: PermissionRequest) {
        let _ = self.channel.send(AgentEvent::Permission { request });
    }
    /// `BRW-UI-1`: the Browser panel replaces its state wholesale on every
    /// action — see `AgentEvent::Browser`.
    pub fn browser(&self, state: super::browser::BrowserPanelState) {
        let _ = self.channel.send(AgentEvent::Browser { state });
    }
    pub fn artifact(&self, id: &str, title: &str, kind: &str, content: &str) {
        let _ = self.channel.send(AgentEvent::Artifact {
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
        let _ = self.channel.send(AgentEvent::Artifact {
            id: artifact.id.clone(),
            title: artifact.title.clone(),
            kind: artifact.kind.clone(),
            content: artifact.content.clone(),
            meta_json: artifact.meta_json.clone(),
        });
    }
    pub fn block(&self, id: &str, message_id: Option<&str>, kind: &str, title: &str, data: &serde_json::Value) {
        let _ = self.channel.send(AgentEvent::Block {
            id: id.to_string(),
            message_id: message_id.map(str::to_string),
            kind: kind.to_string(),
            title: title.to_string(),
            data: data.clone(),
        });
    }
    pub fn block_update(&self, id: &str, title: &str, data: &serde_json::Value) {
        let _ = self.channel.send(AgentEvent::BlockUpdate {
            id: id.to_string(),
            title: title.to_string(),
            data: data.clone(),
        });
    }
    pub fn state_update(&self, state: &serde_json::Value) {
        let _ = self.channel.send(AgentEvent::StateUpdate { state: state.clone() });
    }
    fn done(&self) {
        let _ = self.channel.send(AgentEvent::Done);
    }
    fn cancelled(&self) {
        let _ = self.channel.send(AgentEvent::Cancelled);
    }
    fn error(&self, message: &str) {
        let _ = self.channel.send(AgentEvent::Error { message: message.to_string() });
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
    cancel: &CancelFlag,
    strict_template: &mut bool,
    on_token: &mut F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(&str),
{
    if *strict_template {
        let flat = flatten_to_alternating(messages);
        return drive_turn(client, endpoint, &flat, tools, temperature, cancel, on_token).await;
    }
    match drive_turn(client, endpoint, messages, tools, temperature, cancel, &mut *on_token).await {
        Err(e) if is_template_error(&e) => {
            eprintln!("drive_turn: engine template refused the transcript ({e}); flattening and retrying");
            *strict_template = true;
            let flat = flatten_to_alternating(messages);
            drive_turn(client, endpoint, &flat, tools, temperature, cancel, on_token).await
        }
        other => other,
    }
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
    cancel: CancelFlag,
    sink: &AgentEventSink,
) -> String {
    let text = run_agent_inner(
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
        cancel,
        sink,
    )
    .await;
    let _ = db.backfill_skill_run_failures(conversation_id);
    text
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
    mut messages: Vec<serde_json::Value>,
    temperature: f32,
    tools_enabled: bool,
    // SCH-3: true for an unattended scheduled-job run — no one is watching, so
    // toolsets must skip renders (RND-3) and the File System toolset refuses any
    // write/delete/move outright rather than opening a permission prompt that
    // could never be answered.
    headless: bool,
    cancel: CancelFlag,
    sink: &AgentEventSink,
) -> String {
    // Unified tool table: built-in toolsets + enabled MCP connectors (§7.5).
    let registry = ToolRegistry::build(db, mgr, conversation_id);
    let tool_names = registry.tool_names();
    // Tools *and* skills: what a turn might be trying to invoke, as opposed to
    // what we can dispatch. Drives the hold-back and the narration check.
    let invocable = registry.invocable_names();
    let no_tools: Vec<serde_json::Value> = Vec::new();
    let mut final_text = String::new();
    // GRM-3: call ids we've already nudged, so a failed built-in gets exactly
    // one guided retry — not an unbounded correction loop.
    let mut retried: std::collections::HashSet<String> = std::collections::HashSet::new();
    // FIX-1: the last failed call per tool name this run, so a later success
    // of the *same* tool writes exactly one `tool_fixes` row and nothing is
    // written if the tool never succeeds.
    let mut last_failure = FixTracker::default();
    // LOOP-1: MCP sessions reused across this run; dropped (and stdio children
    // killed) when the run returns.
    let mcp_pool: McpPool = Default::default();
    // `SKL-3`: directories a skill activated this run has made readable,
    // shared across every tool call in the run (not per-call) so a skill
    // loaded early stays reachable for the rest of it.
    let extra_read_roots: std::sync::Mutex<Vec<std::path::PathBuf>> = Default::default();
    // `SKL-2`: skills already loaded this run, so a second request for one
    // returns a pointer instead of its whole body again.
    let loaded_skills: std::sync::Mutex<Vec<String>> = Default::default();
    // How many narrated-tool-call nudges this run has spent. A model that can't
    // emit a real tool call won't learn on the fifth ask either, and every nudge
    // costs the user a turn — so after `MAX_NARRATION_NUDGES` we take the prose
    // as the answer and let the run end.
    let mut narration_nudges = 0usize;
    // Set once this engine's chat template has proved it can't render the
    // loop's own message shapes (`flatten_to_alternating`).
    let mut strict_template = false;

    for _ in 0..MAX_ITERATIONS {
        if cancel.is_cancelled() {
            sink.cancelled();
            return final_text;
        }

        let tools_for_turn = if tools_enabled { &registry.specs } else { &no_tools };

        // In plain-chat mode we stream prose live. In tools mode we buffer the
        // turn so a content-form tool call can be intercepted before display —
        // until the buffer clearly reads as prose, at which point we flush what
        // we have and stream the rest live (LOOP-4). Even then, a fence or a
        // line-initial `{` is held back until it resolves (`safe_prefix`), so a
        // turn that opens in prose and then writes a tool call doesn't leak it.
        let mut turn_buf = String::new();
        let mut streaming_live = false;
        // How much of `turn_buf` the user has already been shown.
        let mut emitted = 0usize;
        let mut on_token = |t: &str| {
            turn_buf.push_str(t);
            if !tools_enabled {
                final_text.push_str(t);
                sink.token(t);
                return;
            }
            if !streaming_live && should_flush_prose(&turn_buf, &tool_names) {
                streaming_live = true;
            }
            if !streaming_live {
                return;
            }
            let cut = safe_prefix(&turn_buf, &invocable);
            if cut > emitted {
                let chunk = &turn_buf[emitted..cut];
                final_text.push_str(chunk);
                sink.token(chunk);
                emitted = cut;
            }
        };
        let outcome = drive_turn_adapting(
            client,
            endpoint,
            &messages,
            tools_for_turn,
            temperature,
            &cancel,
            &mut strict_template,
            &mut on_token,
        )
        .await;

        match outcome {
            Ok(TurnOutcome::Final { content }) => {
                let text = if content.is_empty() { turn_buf } else { content };

                if tools_enabled {
                    // The part of the turn the user has already seen. Anything
                    // past it was held back by `safe_prefix` and is still ours
                    // to swallow if it turns out to be a tool call.
                    let held = if streaming_live { text.get(emitted..).unwrap_or("") } else { &text[..] };

                    // Fallback: the engine may have written a tool call as plain
                    // content instead of structured tool_calls — the only shape
                    // some local models produce at all. Run this on *every*
                    // finished turn, not just fully-buffered ones: a model that
                    // opens in prose and then calls a tool used to end the run
                    // here with the call left on screen as text.
                    if let Some(calls) = parse_text_tool_calls(held, &registry) {
                        dispatch_calls(client, local_endpoint, db, mgr, embed_mgr, rerank_mgr, perms, memory, browser_pool, sink, conversation_id, assistant_message_id, data_dir, model_name, headless, &registry, &mcp_pool, &extra_read_roots, &loaded_skills, &mut messages, &mut retried, &mut last_failure, &calls)
                            .await;
                        continue;
                    }
                    // Not parseable, but it reads as a tool call the model only
                    // *described* — a shape we can't execute. One nudge, then on
                    // with the loop; ending the run here is what left the user
                    // staring at "I'll use the X skill" and nothing happening.
                    if narration_nudges < MAX_NARRATION_NUDGES && narrates_a_tool_call(held, &invocable) {
                        narration_nudges += 1;
                        // The whole turn, not just the withheld tail: the model
                        // has to see its own narration to know what it is being
                        // asked to redo.
                        if !text.trim().is_empty() {
                            messages.push(serde_json::json!({ "role": "assistant", "content": &text }));
                        }
                        messages.push(serde_json::json!({
                            "role": "system",
                            "content": "You described a tool call instead of making one. Call the tool for real now — emit it as a tool call, not as text or a code block.",
                        }));
                        continue;
                    }
                    // A genuine answer: emit whatever is still unshown.
                    if !held.is_empty() {
                        final_text.push_str(held);
                        sink.token(held);
                    }
                } else if final_text.is_empty() {
                    final_text = text;
                }
                sink.done();
                return final_text;
            }
            Ok(TurnOutcome::Cancelled) => {
                sink.cancelled();
                return final_text;
            }
            Ok(TurnOutcome::ToolCalls(calls)) => {
                dispatch_calls(client, local_endpoint, db, mgr, embed_mgr, rerank_mgr, perms, memory, browser_pool, sink, conversation_id, assistant_message_id, data_dir, model_name, headless, &registry, &mcp_pool, &extra_read_roots, &loaded_skills, &mut messages, &mut retried, &mut last_failure, &calls)
                    .await;
                // Loop continues — the model sees the tool results next turn.
            }
            Err(e) => {
                sink.error(&format!("The model run failed: {e}"));
                return final_text;
            }
        }
    }

    sink.error("Reached the limit of tool steps for one turn.");
    final_text
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

/// Execute a batch of tool calls: echo them into the message history, run each
/// through its toolset or MCP server (emitting timeline steps), and append results.
#[allow(clippy::too_many_arguments)]
async fn dispatch_calls(
    client: &reqwest::Client,
    local_endpoint: Option<&ChatEndpoint>,
    db: &Db,
    mgr: &RuntimeManager,
    embed_mgr: &EmbedManager,
    rerank_mgr: &RerankManager,
    perms: &PermissionManager,
    memory: &MemoryStore,
    browser_pool: Option<&super::browser::BrowserPool>,
    sink: &AgentEventSink,
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    data_dir: &std::path::Path,
    model_name: &str,
    headless: bool,
    registry: &ToolRegistry,
    mcp_pool: &McpPool,
    extra_read_roots: &std::sync::Mutex<Vec<std::path::PathBuf>>,
    loaded_skills: &std::sync::Mutex<Vec<String>>,
    messages: &mut Vec<serde_json::Value>,
    retried: &mut std::collections::HashSet<String>,
    last_failure: &mut FixTracker,
    calls: &[ToolCallReq],
) {
    messages.push(assistant_tool_call_message(calls));
    for call in calls {
        let args: serde_json::Value =
            serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));
        let (verb, target) = describe(&call.name, &args, registry);
        sink.step_start(&call.id, &verb, &target);

        let result = dispatch(client, local_endpoint, db, mgr, embed_mgr, rerank_mgr, perms, memory, browser_pool, sink, conversation_id, assistant_message_id, data_dir, headless, registry, mcp_pool, extra_read_roots, loaded_skills, &call.id, &call.name, &args).await;
        // GRM-4/LOOP-5: record every dispatched call's outcome (content-free).
        db.add_tool_stat(model_name, &call.name, conversation_id, result.is_ok());
        match result {
            Ok((output, note)) => {
                // FIX-1: this tool just succeeded — if its last call in this
                // run had failed, that pair is exactly "wrong approach, then
                // right approach". Nothing is written if it never failed.
                if let Some((failed_args, error)) = last_failure.succeeded(&call.name) {
                    db.add_tool_fix(conversation_id, &call.name, &failed_args, &error, &call.arguments);
                }
                sink.step_done(&call.id, note.or_else(|| summarize(&output)));
                messages.push(tool_result_message(&call.id, &output));
            }
            Err(e) => {
                last_failure.failed(&call.name, &call.arguments, &e);
                sink.step_error(&call.id, &e);
                messages.push(tool_result_message(&call.id, &format!("Error: {e}")));
                // GRM-3: give a failed *built-in* call one guided retry. MCP
                // errors take the LOOP-UI-2 path, not this nudge.
                if registry.builtin_for(&call.name).is_some() && retried.insert(call.id.clone()) {
                    messages.push(serde_json::json!({
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
async fn dispatch(
    client: &reqwest::Client,
    local_endpoint: Option<&ChatEndpoint>,
    db: &Db,
    mgr: &RuntimeManager,
    embed_mgr: &EmbedManager,
    rerank_mgr: &RerankManager,
    perms: &PermissionManager,
    memory: &MemoryStore,
    browser_pool: Option<&super::browser::BrowserPool>,
    sink: &AgentEventSink,
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    data_dir: &std::path::Path,
    headless: bool,
    registry: &ToolRegistry,
    mcp_pool: &McpPool,
    extra_read_roots: &std::sync::Mutex<Vec<std::path::PathBuf>>,
    loaded_skills: &std::sync::Mutex<Vec<String>>,
    call_id: &str,
    name: &str,
    args: &serde_json::Value,
) -> Result<(String, Option<String>), String> {
    if let Some(toolset) = registry.builtin_for(name) {
        let ctx = ToolContext {
            client,
            local_endpoint,
            db,
            mgr,
            embed_mgr,
            rerank_mgr,
            perms,
            sink,
            conversation_id,
            assistant_message_id,
            data_dir,
            call_id,
            memory,
            headless,
            // One context per tool call, so this is RND-3's one-render budget.
            rendered: std::sync::atomic::AtomicBool::new(false),
            step_note: std::sync::Mutex::new(None),
            extra_read_roots,
            loaded_skills,
            browser_pool,
        };
        let output = toolset.execute(&ctx, name, args).await?;
        // A toolset that said what its step line should read (RET-UI-2) wins over
        // the generic summary; everything else still gets `summarize`.
        Ok((output, ctx.step_note.into_inner().unwrap_or(None)))
    } else if let Some(binding) = registry.mcp.get(name) {
        call_mcp_tool(client, db, conversation_id, mcp_pool, binding, name, args)
            .await
            .map(|out| (out, None))
    } else {
        Err(format!("No toolset or connector provides the tool '{name}'."))
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

/// (verb, target) for a tool call, dispatched to the owning toolset or connector.
fn describe(name: &str, args: &serde_json::Value, registry: &ToolRegistry) -> (String, String) {
    if let Some(toolset) = registry.builtin_for(name) {
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
