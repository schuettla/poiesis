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
}

/// Thin wrapper over the Tauri channel with typed emit helpers.
pub struct AgentEventSink {
    channel: Channel<AgentEvent>,
}

impl AgentEventSink {
    pub fn new(channel: Channel<AgentEvent>) -> Self {
        Self { channel }
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
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    data_dir: &std::path::Path,
    mut messages: Vec<serde_json::Value>,
    temperature: f32,
    tools_enabled: bool,
    cancel: CancelFlag,
    sink: &AgentEventSink,
) -> String {
    // Unified tool table: built-in skills + enabled MCP connectors (§7.5).
    let registry = ToolRegistry::build(db);
    let no_tools: Vec<serde_json::Value> = Vec::new();
    let mut final_text = String::new();

    for _ in 0..MAX_ITERATIONS {
        if cancel.is_cancelled() {
            sink.cancelled();
            return final_text;
        }

        let tools_for_turn = if tools_enabled { &registry.specs } else { &no_tools };

        // In plain-chat mode we stream prose live. In tools mode we buffer the
        // turn so a content-form tool call can be intercepted before display.
        let mut turn_buf = String::new();
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
                }
            },
        )
        .await;

        match outcome {
            Ok(TurnOutcome::Final { content }) => {
                let text = if content.is_empty() { turn_buf } else { content };

                // Fallback: the engine may have streamed a tool call as content
                // JSON instead of structured tool_calls. Execute it if so.
                if tools_enabled {
                    if let Some(calls) = parse_text_tool_calls(&text, &registry) {
                        dispatch_calls(client, db, perms, sink, conversation_id, assistant_message_id, data_dir, &registry, &mut messages, &calls)
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
                dispatch_calls(client, db, perms, sink, conversation_id, assistant_message_id, data_dir, &registry, &mut messages, &calls)
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
    sink: &AgentEventSink,
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    data_dir: &std::path::Path,
    registry: &ToolRegistry,
    messages: &mut Vec<serde_json::Value>,
    calls: &[ToolCallReq],
) {
    messages.push(assistant_tool_call_message(calls));
    for call in calls {
        let args: serde_json::Value =
            serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));
        let (verb, target) = describe(&call.name, &args, registry);
        sink.step_start(&call.id, &verb, &target);

        match dispatch(client, db, perms, sink, conversation_id, assistant_message_id, data_dir, registry, &call.name, &args).await {
            Ok(output) => {
                sink.step_done(&call.id, summarize(&output));
                messages.push(tool_result_message(&call.id, &output));
            }
            Err(e) => {
                sink.step_error(&call.id, &e);
                messages.push(tool_result_message(&call.id, &format!("Error: {e}")));
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
    sink: &AgentEventSink,
    conversation_id: &str,
    assistant_message_id: Option<&str>,
    data_dir: &std::path::Path,
    registry: &ToolRegistry,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    if let Some(skill) = registry.builtin_for(name) {
        let ctx = SkillContext { client, db, perms, sink, conversation_id, assistant_message_id, data_dir };
        skill.execute(&ctx, name, args).await
    } else if let Some(binding) = registry.mcp.get(name) {
        call_mcp_tool(client, db, conversation_id, binding, name, args).await
    } else {
        Err(format!("No skill or connector provides the tool '{name}'."))
    }
}

/// Invoke a tool on a remote MCP server (MCP-4): connect, call, and log the act
/// in the visible activity log.
async fn call_mcp_tool(
    client: &reqwest::Client,
    db: &Db,
    conversation_id: &str,
    binding: &McpBinding,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let mut mcp = if binding.transport == "stdio" {
        McpClient::new_stdio(binding.url.clone())
    } else {
        let token = secrets::get_secret(SERVICE_MCP, &binding.connector_id)
            .ok()
            .flatten();
        McpClient::new(client.clone(), binding.url.clone(), token)
    };
    mcp.initialize().await.map_err(|e| e.to_string())?;
    let output = mcp.call_tool(name, args.clone()).await.map_err(|e| e.to_string())?;
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
