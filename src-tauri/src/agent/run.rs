//! The agent loop (PRD §7.5): drive model turns, dispatch tool calls to built-in
//! skills, feed results back, and emit a visible step timeline — until the model
//! produces a final answer.

use std::collections::HashMap;

use tauri::ipc::Channel;

use crate::cloud::{drive_turn, ChatEndpoint};
use crate::db::Db;
use crate::mcp::McpClient;
use crate::permissions::{PermissionManager, PermissionRequest};
use crate::runtime::proxy::{CancelFlag, ToolCallReq, TurnOutcome};
use crate::secrets::{self, SERVICE_MCP};

use super::skills::{self, Skill, SkillContext};
use crate::memory::MemoryStore;
use super::AgentEvent;

/// Cap on tool-call iterations to bound runaway loops. Blocks add present/update
/// calls that consume iterations, so this is a little higher than a plain loop.
const MAX_ITERATIONS: usize = 12;

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
        "propose_recipe" => Some("recipes"),
        _ => None,
    }
}

/// The unified tool table for one run: the OpenAI specs advertised to the model,
/// the enabled built-in skills, plus the routing map for MCP tools.
struct ToolRegistry {
    specs: Vec<serde_json::Value>,
    skills: Vec<Skill>,
    mcp: HashMap<String, McpBinding>,
}

impl ToolRegistry {
    /// Build from every **enabled** built-in skill (TOOL-6) plus every enabled
    /// MCP connector's cached tools (MCP-4, §7.5 unified dispatch). Built-in
    /// tools win name collisions.
    fn build(db: &Db) -> Self {
        #[derive(serde::Deserialize, Default)]
        struct CachedConfig {
            #[serde(default)]
            tools: Vec<crate::mcp::McpTool>,
        }

        let enabled = skills::enabled(db);
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

        ToolRegistry { specs, skills: enabled, mcp }
    }

    /// The enabled built-in skill that owns `name`, if any.
    fn builtin_for(&self, name: &str) -> Option<Skill> {
        self.skills.iter().copied().find(|s| s.handles(name))
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
    pub fn send_permission(&self, request: PermissionRequest) {
        let _ = self.channel.send(AgentEvent::Permission { request });
    }
    pub fn artifact(&self, id: &str, title: &str, kind: &str, content: &str) {
        let _ = self.channel.send(AgentEvent::Artifact {
            id: id.to_string(),
            title: title.to_string(),
            kind: kind.to_string(),
            content: content.to_string(),
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
/// `tools_enabled` gates the built-in skills. When false (the default for plain
/// chat) no `tools` are advertised, so the model answers directly and prose is
/// streamed live. When true, the File System skill is offered and the loop
/// dispatches tool calls — buffering each turn so a tool call that the engine
/// streams as plain content JSON (see [`parse_text_tool_calls`]) is executed
/// rather than leaked to the user as raw text.
#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    db: &Db,
    perms: &PermissionManager,
    memory: &MemoryStore,
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    data_dir: &std::path::Path,
    model_name: &str,
    mut messages: Vec<serde_json::Value>,
    temperature: f32,
    tools_enabled: bool,
    cancel: CancelFlag,
    sink: &AgentEventSink,
) -> String {
    // Unified tool table: built-in skills + enabled MCP connectors (§7.5).
    let registry = ToolRegistry::build(db);
    let tool_names = registry.tool_names();
    let no_tools: Vec<serde_json::Value> = Vec::new();
    let mut final_text = String::new();
    // GRM-3: call ids we've already nudged, so a failed built-in gets exactly
    // one guided retry — not an unbounded correction loop.
    let mut retried: std::collections::HashSet<String> = std::collections::HashSet::new();
    // LOOP-1: MCP sessions reused across this run; dropped (and stdio children
    // killed) when the run returns.
    let mcp_pool: McpPool = Default::default();

    for _ in 0..MAX_ITERATIONS {
        if cancel.is_cancelled() {
            sink.cancelled();
            return final_text;
        }

        let tools_for_turn = if tools_enabled { &registry.specs } else { &no_tools };

        // In plain-chat mode we stream prose live. In tools mode we buffer the
        // turn so a content-form tool call can be intercepted before display —
        // until the buffer clearly reads as prose, at which point we flush what
        // we have and stream the rest live (LOOP-4).
        let mut turn_buf = String::new();
        let mut streaming_live = false;
        let outcome = drive_turn(
            client,
            endpoint,
            &messages,
            tools_for_turn,
            temperature,
            &cancel,
            |t| {
                turn_buf.push_str(t);
                if !tools_enabled {
                    final_text.push_str(t);
                    sink.token(t);
                } else if streaming_live {
                    final_text.push_str(t);
                    sink.token(t);
                } else if should_flush_prose(&turn_buf, &tool_names) {
                    streaming_live = true;
                    final_text.push_str(&turn_buf);
                    sink.token(&turn_buf);
                }
            },
        )
        .await;

        match outcome {
            Ok(TurnOutcome::Final { content }) => {
                // Already streamed live (LOOP-4): `final_text` holds the whole
                // turn, so don't re-emit it and don't re-parse it as a tool call.
                if tools_enabled && streaming_live {
                    sink.done();
                    return final_text;
                }

                let text = if content.is_empty() { turn_buf } else { content };

                // Fallback: the engine may have streamed a tool call as content
                // JSON instead of structured tool_calls. Execute it if so.
                if tools_enabled {
                    if let Some(calls) = parse_text_tool_calls(&text, &registry) {
                        dispatch_calls(client, db, perms, memory, sink, conversation_id, assistant_message_id, data_dir, model_name, &registry, &mcp_pool, &mut messages, &mut retried, &calls)
                            .await;
                        continue;
                    }
                    // A genuine answer: emit it now (it was buffered).
                    if !text.is_empty() {
                        final_text.push_str(&text);
                        sink.token(&text);
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
                dispatch_calls(client, db, perms, memory, sink, conversation_id, assistant_message_id, data_dir, model_name, &registry, &mcp_pool, &mut messages, &mut retried, &calls)
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

/// Execute a batch of tool calls: echo them into the message history, run each
/// through its skill or MCP server (emitting timeline steps), and append results.
#[allow(clippy::too_many_arguments)]
async fn dispatch_calls(
    client: &reqwest::Client,
    db: &Db,
    perms: &PermissionManager,
    memory: &MemoryStore,
    sink: &AgentEventSink,
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    data_dir: &std::path::Path,
    model_name: &str,
    registry: &ToolRegistry,
    mcp_pool: &McpPool,
    messages: &mut Vec<serde_json::Value>,
    retried: &mut std::collections::HashSet<String>,
    calls: &[ToolCallReq],
) {
    messages.push(assistant_tool_call_message(calls));
    for call in calls {
        let args: serde_json::Value =
            serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));
        let (verb, target) = describe(&call.name, &args, registry);
        sink.step_start(&call.id, &verb, &target);

        let result = dispatch(client, db, perms, memory, sink, conversation_id, assistant_message_id, data_dir, registry, mcp_pool, &call.id, &call.name, &args).await;
        // GRM-4/LOOP-5: record every dispatched call's outcome (content-free).
        db.add_tool_stat(model_name, &call.name, conversation_id, result.is_ok());
        match result {
            Ok(output) => {
                sink.step_done(&call.id, summarize(&output));
                messages.push(tool_result_message(&call.id, &output));
            }
            Err(e) => {
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
    let start = body.find('{')?;
    let mut stream =
        serde_json::Deserializer::from_str(&body[start..]).into_iter::<serde_json::Value>();
    let value = stream.next()?.ok()?;
    let calls: Vec<ToolCallReq> = one_text_call(&value, registry).into_iter().collect();
    if calls.is_empty() {
        None
    } else {
        Some(calls)
    }
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

/// Strip a leading ```json / ``` code fence (and trailing ```), if present.
fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    let Some(rest) = s.strip_prefix("```") else {
        return s;
    };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let rest = rest.trim_start_matches(['\n', '\r']);
    match rest.rfind("```") {
        Some(end) => rest[..end].trim(),
        None => rest.trim(),
    }
}

/// Route a tool call to the skill or MCP server that handles it (§7.5).
#[allow(clippy::too_many_arguments)]
async fn dispatch(
    client: &reqwest::Client,
    db: &Db,
    perms: &PermissionManager,
    memory: &MemoryStore,
    sink: &AgentEventSink,
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    data_dir: &std::path::Path,
    registry: &ToolRegistry,
    mcp_pool: &McpPool,
    call_id: &str,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    if let Some(skill) = registry.builtin_for(name) {
        let ctx = SkillContext { client, db, perms, sink, conversation_id, assistant_message_id, data_dir, call_id, memory };
        skill.execute(&ctx, name, args).await
    } else if let Some(binding) = registry.mcp.get(name) {
        call_mcp_tool(client, db, conversation_id, mcp_pool, binding, name, args).await
    } else {
        Err(format!("No skill or connector provides the tool '{name}'."))
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

/// (verb, target) for a tool call, dispatched to the owning skill or connector.
fn describe(name: &str, args: &serde_json::Value, registry: &ToolRegistry) -> (String, String) {
    if let Some(skill) = registry.builtin_for(name) {
        skill.describe(name, args)
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
    } else if trimmed.len() > 48 {
        Some(format!("— {}…", &trimmed[..48]))
    } else {
        Some(format!("— {trimmed}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec!["render_ui".to_string(), "web_search".to_string()]
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
