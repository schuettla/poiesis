//! Agents that speak the Agent Client Protocol (`CLI-8`).
//!
//! JSON-RPC 2.0 lines on stdio. The calls: `initialize`, `session/new` (or
//! `session/load` to continue one), `session/prompt`; the end of a turn is the
//! **reply** to `session/prompt`. Recorded against OpenCode 2.1.0
//! (`fixtures/engines/opencode-*`).
//!
//! Rules this driver keeps, all from the recordings:
//! - It never calls `authenticate`. The only method OpenCode offers is a login
//!   in a terminal, and calling it can open a browser or hang. A missing sign-in
//!   shows up as an error from `session/new`.
//! - The agent is made to **ask** before it edits or runs anything: the catalog
//!   sets its own permission setting in the child's environment, because
//!   OpenCode never asks otherwise. The folder's trust picks how much it asks.
//! - A permission request is answered by the option's **kind**, never by its
//!   name or position: `allow_once` or `reject_once`. `allow_always` is never
//!   chosen, so nothing is stored in the agent's own files.
//! - A reject ends the turn (`stopReason: cancelled`). That is "declined", not
//!   an error.
//! - Updates are only read after our own prompt was sent: `session/load` replays
//!   old history without marking it.
//! - The agent cannot take a message mid-turn. What the user types meanwhile is
//!   sent as the next prompt of the same session, once the turn ends.
//! - Unlike Codex there is no stall watchdog: a free model can be quiet for a
//!   long time.
//! - Poiesis's own tools (`MCPS-2`) are mounted in `session/new` as
//!   `mcpServers: [{type: "http", name: "poiesis", url, headers: []}]`, and only when
//!   the agent's `initialize` says `mcpCapabilities.http`. Recorded against OpenCode
//!   2.1.0: it connects at once, lists the tools, runs a call without any permission
//!   request, and shows the call as its own `execute` tool whose `rawInput.code` reads
//!   `tools.poiesis.recall(...)`.
//! - Plan mode is not used. Recorded: it writes into a folder under the user's
//!   home, outside the working folder. Read-only is our own check instead: every
//!   edit and command is refused.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::{json, Value};

use crate::agent::toolrpc::{self, McpCall};
use crate::agent::AgentEvent;
use crate::permissions::Trust;
use crate::runtime::proxy::{CancelFlag, Usage};

use super::approve::{self, Port, Verdict};
use super::driver::{Approvals, Capabilities, DriverError, EngineDriver, ExternalOutcome, ExternalTurn, Job, Launch, ModelChoice};
use super::play::{clip, flush, keep_tail, play, steer_text, Asks, Ev};
use super::process::{self, Wait, Wire};
use super::tools;
use super::{EngineKind, PaidBy};

/// After `session/cancel`, how long the agent has to say it stopped.
const CANCEL_GRACE: Duration = Duration::from_secs(3);

pub struct AcpDriver;

#[async_trait]
impl EngineDriver for AcpDriver {
    fn kind(&self) -> EngineKind {
        EngineKind::Acp
    }

    /// The baseline. What an agent really supports is in its `initialize`
    /// reply (`loadSession`, `mcpCapabilities.http`); the run reads it there
    /// instead of hard-coding it per agent. It cannot take a message mid-turn.
    fn capabilities(&self) -> Capabilities {
        Capabilities { resume: false, steer_mid_turn: false, interrupt: true, mcp_tools: true, approvals: Approvals::InBand }
    }

    /// The models the agent offers, from the `configOptions` of a new session.
    async fn list_models(&self, launch: &Launch) -> Result<Vec<ModelChoice>, DriverError> {
        let dir = std::env::temp_dir().join(format!("poiesis-models-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).map_err(|e| DriverError::Start(format!("I couldn't make a scratch folder: {e}")))?;
        let result = models_in(launch, &dir).await;
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    async fn run_turn(&self, turn: ExternalTurn<'_>) -> Result<ExternalOutcome, DriverError> {
        let resume = turn.job.as_ref().and_then(|j| j.resume.clone());
        run(&turn, resume).await
    }
}

async fn models_in(launch: &Launch, dir: &Path) -> Result<Vec<ModelChoice>, DriverError> {
    let mut args = launch.base_args.clone();
    args.extend(launch.extra_args.iter().cloned());
    let cmd = process::command(&launch.program, &args, &launch.env).map_err(DriverError::Start)?;
    let mut wire = Wire::start(cmd, Some(dir)).map_err(DriverError::Start)?;
    let deadline = Instant::now() + Duration::from_secs(45);
    let never = CancelFlag::new();
    let result = async {
        wire.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init_params()}))
            .await
            .map_err(DriverError::Start)?;
        let mut asked = false;
        loop {
            match wire.next_until(&never, deadline).await {
                Wait::Message(m) => {
                    if let Some(err) = m.get("error") {
                        return Err(rpc_error(err, true));
                    }
                    match (m.get("id").and_then(|i| i.as_i64()), m.get("result")) {
                        (Some(1), Some(_)) if !asked => {
                            asked = true;
                            wire.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": dir, "mcpServers": []}}))
                                .await
                                .map_err(DriverError::Start)?;
                        }
                        (Some(2), Some(result)) => return Ok(model_options(result)),
                        _ => {}
                    }
                }
                Wait::Closed => return Err(DriverError::Exited(String::new())),
                Wait::TimedOut => return Err(DriverError::TimedOut),
                Wait::Cancelled => return Err(DriverError::Cancelled),
            }
        }
    }
    .await;
    wire.finish().await;
    result.map(|(options, current)| {
        options
            .into_iter()
            .map(|(id, name)| {
                let is_default = Some(&id) == current.as_ref();
                ModelChoice { id, name, is_default }
            })
            .collect()
    })
}

fn init_params() -> Value {
    json!({
        "protocolVersion": 1,
        // The agent uses its own file tools inside the folder.
        "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}},
    })
}

/// The `mcpServers` of a new session: ours, when there is an address and the agent
/// can reach a server over HTTP; otherwise none.
fn mcp_servers(url: Option<&str>, http: bool) -> Value {
    match url {
        Some(url) if http => json!([{"type": "http", "name": "poiesis", "url": url, "headers": []}]),
        _ => json!([]),
    }
}

/// `(value, name)` of every model the agent offers, and the one in use.
fn model_options(session: &Value) -> (Vec<(String, String)>, Option<String>) {
    let Some(opt) = session
        .get("configOptions")
        .and_then(|c| c.as_array())
        .and_then(|c| c.iter().find(|o| o.get("id").and_then(|i| i.as_str()) == Some("model")))
    else {
        return (Vec::new(), None);
    };
    let options = opt
        .get("options")
        .and_then(|o| o.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|o| {
                    let value = o.get("value").and_then(|v| v.as_str())?.to_string();
                    let name = o.get("name").and_then(|v| v.as_str()).unwrap_or(&value).to_string();
                    Some((value, name))
                })
                .collect()
        })
        .unwrap_or_default();
    (options, opt.get("currentValue").and_then(|v| v.as_str()).map(String::from))
}

/// What a reply from the agent answers.
enum Pending {
    Init,
    /// `session/new`, or `session/load` of the session we hold.
    Session { resuming: Option<String> },
    /// `session/set_config_option` for the model.
    Model(String),
    Prompt,
}

/// What an answered permission request leaves for the loop to send.
struct AskReply {
    id: Value,
    result: Value,
    /// The step it refused, in words.
    refused: Option<String>,
}

fn setting(env: &mut Vec<(String, String)>, key: &str, value: &str) {
    env.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    env.push((key.to_string(), value.to_string()));
}

async fn run(turn: &ExternalTurn<'_>, resume: Option<String>) -> Result<ExternalOutcome, DriverError> {
    let launch = turn.launch;
    let job = turn.job.as_ref();
    let mut env = launch.env.clone();
    // In a folder the user let agents edit freely, edits go through without a
    // question, as Claude Code's own `acceptEdits` does. Commands still ask.
    if job.is_some_and(|j| j.trust == Trust::Auto) {
        for (k, v) in &launch.auto_env {
            setting(&mut env, k, v);
        }
    }
    // MCPS-2: the loopback address our tools live at, only when the user switched
    // them on for this agent. The ticket is revoked when this function ends.
    let (mcp_tx, mut mcp_rx) = tokio::sync::mpsc::unbounded_channel::<McpCall>();
    let mut _ticket = None;
    let mut mcp_url: Option<String> = None;
    if let Some(j) = job.filter(|j| j.tools.is_some()) {
        let kit = toolrpc::McpKit { approve: false, memory: j.tools.is_some_and(|t| t.memory) };
        let ticket = toolrpc::arm_mcp_with(mcp_tx, &j.run.id, kit, j.tools.map(|t| t.app_specs()).unwrap_or_default())
            .ok_or_else(|| DriverError::Start("My local helper port isn't running, so I can't give it my tools.".into()))?;
        mcp_url = Some(toolrpc::mcp_url(&ticket).ok_or_else(|| DriverError::Start("I have no local address for the helper.".into()))?);
        _ticket = Some(ticket);
    }
    let mut mcp_open = mcp_url.is_some();
    let mcp = mcp_url.as_deref();

    let mut args = launch.base_args.clone();
    args.extend(launch.extra_args.iter().cloned());
    let cmd = process::command(&launch.program, &args, &env).map_err(DriverError::Start)?;
    let mut wire = Wire::start(cmd, Some(&turn.cwd)).map_err(DriverError::Start)?;
    let deadline = Instant::now() + turn.timeout;

    let mut conn = Conn { next_id: 1, pending: HashMap::new() };
    if let Err(e) = conn.call(&mut wire, "initialize", init_params(), Pending::Init).await {
        wire.finish().await;
        return Err(DriverError::Start(e));
    }

    let mut run = Run::new(&turn.cwd);
    let never = CancelFlag::new();
    let mut cancelled_at: Option<Instant> = None;
    let mut asks: Asks<'_, AskReply> = FuturesUnordered::new();
    let mut calls: Asks<'_> = FuturesUnordered::new();
    let mut prompting = false;

    let failure: Option<DriverError> = loop {
        let (flag, until) = match cancelled_at {
            Some(t) => (&never, t + CANCEL_GRACE),
            None => (&turn.cancel, deadline),
        };
        tokio::select! {
            biased;
            call = mcp_rx.recv(), if mcp_open => match call {
                Some(call) => {
                    if let Some(j) = job {
                        calls.push(Box::pin(tools::serve(j.tools, call, &turn.cancel)));
                    }
                }
                None => mcp_open = false,
            },
            Some(()) = calls.next(), if !calls.is_empty() => {}
            Some(reply) = asks.next(), if !asks.is_empty() => {
                if let Some(r) = reply.refused {
                    run.rejected.push(r);
                }
                let _ = wire.send(&json!({"jsonrpc": "2.0", "id": reply.id, "result": reply.result})).await;
            }
            w = wire.next_tick(flag, until) => match w {
                None => {}
                Some(Wait::Closed) => break Some(DriverError::Exited(String::new())),
                Some(Wait::TimedOut) => break Some(if cancelled_at.is_some() { DriverError::Cancelled } else { DriverError::TimedOut }),
                Some(Wait::Cancelled) => {
                    cancelled_at = Some(Instant::now());
                    match &run.session {
                        Some(s) if prompting => {
                            let _ = wire.send(&json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": s}})).await;
                        }
                        _ => break Some(DriverError::Cancelled),
                    }
                }
                Some(Wait::Message(m)) => {
                    let method = m.get("method").and_then(|x| x.as_str()).map(String::from);
                    let id = m.get("id").cloned();
                    match (method, id) {
                        // The agent asks us something.
                        (Some(method), Some(id)) => {
                            let params = m.get("params").cloned().unwrap_or(Value::Null);
                            if method == "session/request_permission" {
                                match job {
                                    Some(j) => asks.push(Box::pin(ask(j.port, id, params, turn.cwd.clone(), &turn.cancel))),
                                    None => {
                                        // Try it: nothing is allowed.
                                        let reply = refuse(id, &params);
                                        run.rejected.push(reply.refused.clone().unwrap_or_default());
                                        let _ = wire.send(&json!({"jsonrpc": "2.0", "id": reply.id, "result": reply.result})).await;
                                    }
                                }
                            } else {
                                let _ = wire
                                    .send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}}))
                                    .await;
                            }
                        }
                        (Some(method), None) => {
                            let events = run.notification(&method, m.get("params").unwrap_or(&Value::Null), prompting);
                            if let Some(j) = job {
                                play(j, events);
                            }
                        }
                        (None, Some(id)) => {
                            let Some(entry) = id.as_i64().and_then(|i| conn.pending.remove(&i)) else { continue };
                            if let Some(err) = m.get("error") {
                                match entry {
                                    // The session we were asked to continue is gone:
                                    // start a new one, once, and say so.
                                    Pending::Session { resuming: Some(_) } => {
                                        run.resume_failed = true;
                                        if let Err(e) = conn.call(&mut wire, "session/new", json!({"cwd": turn.cwd, "mcpServers": mcp_servers(mcp, run.mcp_http)}), Pending::Session { resuming: None }).await {
                                            break Some(DriverError::Start(e));
                                        }
                                    }
                                    _ => break Some(rpc_error(err, !prompting)),
                                }
                                continue;
                            }
                            let result = m.get("result").cloned().unwrap_or(Value::Null);
                            match entry {
                                Pending::Init => {
                                    run.learn(&result);
                                    let (method, params, what) = match resume.clone().filter(|_| run.load_session) {
                                        Some(sid) => (
                                            "session/load",
                                            json!({"sessionId": sid, "cwd": turn.cwd, "mcpServers": mcp_servers(mcp, run.mcp_http)}),
                                            Pending::Session { resuming: Some(sid) },
                                        ),
                                        None => {
                                            // Asked to continue, and this agent cannot.
                                            if resume.is_some() {
                                                run.resume_failed = true;
                                            }
                                            ("session/new", json!({"cwd": turn.cwd, "mcpServers": mcp_servers(mcp, run.mcp_http)}), Pending::Session { resuming: None })
                                        }
                                    };
                                    if let Err(e) = conn.call(&mut wire, method, params, what).await {
                                        break Some(DriverError::Start(e));
                                    }
                                }
                                Pending::Session { resuming } => {
                                    let Some(session) = resuming.or_else(|| result.get("sessionId").and_then(|s| s.as_str()).map(String::from)) else {
                                        break Some(DriverError::Protocol("session/new gave no session id".into()));
                                    };
                                    run.set_session(&session, &result);
                                    if let Some(j) = job {
                                        (j.session_seen)(&session);
                                    }
                                    // The model the user chose for this agent, if it is
                                    // not the one the session started with.
                                    let want = launch.model.clone().filter(|m| Some(m) != run.model.as_ref());
                                    let step = match want {
                                        Some(model) => {
                                            if !run.model_options.iter().any(|(v, _)| *v == model) {
                                                break Some(DriverError::Failed(format!("It doesn't offer the model {model}. Pick another under External agents.")));
                                            }
                                            conn.call(
                                                &mut wire,
                                                "session/set_config_option",
                                                json!({"sessionId": session, "configId": "model", "value": model}),
                                                Pending::Model(model),
                                            )
                                            .await
                                        }
                                        None => {
                                            prompting = true;
                                            conn.call(&mut wire, "session/prompt", prompt_params(&session, turn, job, None), Pending::Prompt).await
                                        }
                                    };
                                    if let Err(e) = step {
                                        break Some(DriverError::Start(e));
                                    }
                                }
                                Pending::Model(model) => {
                                    run.model = Some(model);
                                    let session = run.session.clone().unwrap_or_default();
                                    prompting = true;
                                    if let Err(e) = conn.call(&mut wire, "session/prompt", prompt_params(&session, turn, job, None), Pending::Prompt).await {
                                        break Some(DriverError::Start(e));
                                    }
                                }
                                Pending::Prompt => {
                                    run.finish(&result);
                                    // Words the user typed while it worked go in as the
                                    // next prompt of the same session.
                                    let again = match (job, cancelled_at, run.stop_reason.as_deref()) {
                                        (Some(j), None, Some(r)) if r != "cancelled" => steers_of(j),
                                        _ => None,
                                    };
                                    match again {
                                        Some(text) => {
                                            if let Some(j) = job {
                                                j.sink.emit(AgentEvent::Steered { run_id: j.run.id.clone(), text: text.clone() });
                                            }
                                            run.next_turn();
                                            let session = run.session.clone().unwrap_or_default();
                                            if let Err(e) = conn.call(&mut wire, "session/prompt", prompt_params(&session, turn, job, Some(&text)), Pending::Prompt).await {
                                                break Some(DriverError::Start(e));
                                            }
                                        }
                                        None => break None,
                                    }
                                }
                            }
                        }
                        (None, None) => run.unknown += 1,
                    }
                }
            }
        }
    };

    // Let a question that was on the panel withdraw itself, and tell the agent we
    // will not answer it.
    if failure.is_some() || cancelled_at.is_some() {
        let mut leftover = std::mem::take(&mut asks);
        flush(&mut leftover).await;
    }
    drop(asks);
    // A tool of ours that was waiting for the user hears Stop and ends too.
    flush(&mut calls).await;
    drop(calls);

    if let Some(j) = job {
        play(j, run.close_open_steps());
    }
    let mut out = run.into_outcome(launch.model.as_deref());
    out.unknown_events += wire.unknown + wire.dropped_lines();
    out.stderr_tail = wire.tail.text();
    keep_tail(job, &out.stderr_tail);
    let code = wire.finish().await;

    if let Some(err) = failure {
        return Err(match err {
            DriverError::Exited(_) => DriverError::Exited(match (code, out.stderr_tail.lines().last()) {
                (Some(c), Some(t)) => format!(" (code {c}): {t}"),
                (Some(c), None) => format!(" (code {c})."),
                _ => ".".into(),
            }),
            other => other,
        });
    }
    if cancelled_at.is_some() || (out.stop_reason == "cancelled" && out.denials.is_empty()) {
        return Err(DriverError::Cancelled);
    }
    Ok(out)
}

/// Requests sent and not yet answered.
struct Conn {
    next_id: i64,
    pending: HashMap<i64, Pending>,
}

impl Conn {
    async fn call(&mut self, wire: &mut Wire, method: &str, params: Value, what: Pending) -> Result<(), String> {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, what);
        wire.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).await
    }
}

/// What the user (or the lead) typed while the agent worked, as one message.
fn steers_of(job: &Job<'_>) -> Option<String> {
    let all = job.run.drain_steers();
    if all.is_empty() {
        return None;
    }
    Some(all.iter().map(steer_text).collect::<Vec<_>>().join("\n\n"))
}

/// The prompt: a job's fixed words and the fenced task the first time, then just
/// what was typed.
fn prompt_params(session: &str, turn: &ExternalTurn<'_>, job: Option<&Job<'_>>, follow_up: Option<&str>) -> Value {
    let mut blocks = Vec::new();
    match follow_up {
        Some(text) => blocks.push(json!({"type": "text", "text": text})),
        None => {
            if let Some(j) = job {
                blocks.push(json!({"type": "text", "text": j.system_append}));
            }
            blocks.push(json!({"type": "text", "text": turn.task}));
        }
    }
    json!({"sessionId": session, "prompt": blocks})
}

/// One permission request, put to the user (or refused) and answered by the
/// option's kind.
async fn ask(port: &Port<'_>, id: Value, params: Value, cwd: PathBuf, stop: &CancelFlag) -> AskReply {
    let call = params.get("toolCall").cloned().unwrap_or(Value::Null);
    let (tool, input, title) = tool_of(&call, &cwd);
    let gone = CancelFlag::new();
    let verdict = port.decide(&tool, &input, &gone, stop).await;
    let options = options_of(&params);
    if stop.is_cancelled() {
        return AskReply { id, result: json!({"outcome": {"outcome": "cancelled"}}), refused: None };
    }
    match verdict {
        Verdict::Allow => match options.once {
            Some(o) => AskReply { id, result: selected(&o), refused: None },
            // No plain "allow" offered: say no rather than pick another kind.
            None => AskReply { id, result: json!({"outcome": {"outcome": "cancelled"}}), refused: Some(title) },
        },
        Verdict::Deny(_) => AskReply {
            id,
            result: match options.reject {
                Some(o) => selected(&o),
                None => json!({"outcome": {"outcome": "cancelled"}}),
            },
            refused: Some(title),
        },
    }
}

/// Try it, and any run with nobody to ask: every request is refused.
fn refuse(id: Value, params: &Value) -> AskReply {
    let title = params
        .pointer("/toolCall/title")
        .and_then(|t| t.as_str())
        .or_else(|| params.pointer("/toolCall/kind").and_then(|t| t.as_str()))
        .unwrap_or("a step")
        .to_string();
    let result = match options_of(params).reject {
        Some(o) => selected(&o),
        None => json!({"outcome": {"outcome": "cancelled"}}),
    };
    AskReply { id, result, refused: Some(title) }
}

fn selected(option_id: &str) -> Value {
    json!({"outcome": {"outcome": "selected", "optionId": option_id}})
}

struct Options {
    once: Option<String>,
    reject: Option<String>,
}

/// By kind. The order and the names of the options are never trusted, and
/// `allow_always` is never picked.
fn options_of(params: &Value) -> Options {
    let find = |kind: &str| {
        params
            .get("options")
            .and_then(|o| o.as_array())
            .and_then(|o| o.iter().find(|o| o.get("kind").and_then(|k| k.as_str()) == Some(kind)))
            .and_then(|o| o.get("optionId").and_then(|i| i.as_str()))
            .map(String::from)
    };
    Options { once: find("allow_once"), reject: find("reject_once") }
}

fn rpc_error(err: &Value, before_prompt: bool) -> DriverError {
    let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("The agent returned an error.").to_string();
    let lower = msg.to_ascii_lowercase();
    let auth = lower.contains("auth") || lower.contains("log in") || lower.contains("login") || lower.contains("sign in");
    if auth && before_prompt {
        DriverError::NeedsSignIn(msg)
    } else {
        DriverError::Failed(msg)
    }
}

/// What the agent's tool call is, in the words the permission panel and the
/// timeline already use (`approve::describe_tool`): `(tool, input, title)`.
/// Works on a `toolCall` of a permission request and on one built up from
/// `tool_call` and `tool_call_update`.
pub fn tool_of(call: &Value, cwd: &Path) -> (String, Value, String) {
    let kind = call.get("kind").and_then(|k| k.as_str()).unwrap_or("other");
    let title = call.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
    let raw = call.get("rawInput").cloned().unwrap_or(Value::Null);
    let text = |key: &str| raw.get(key).and_then(|v| v.as_str());
    let absolute = |p: &str| {
        let path = Path::new(p);
        if path.is_absolute() {
            p.to_string()
        } else {
            cwd.join(path).to_string_lossy().to_string()
        }
    };
    let diff = call.get("content").and_then(|c| c.as_array()).and_then(|c| c.iter().find(|b| b.get("type").and_then(|t| t.as_str()) == Some("diff")));
    let path = || {
        diff.and_then(|d| d.get("path").and_then(|p| p.as_str()))
            .or_else(|| call.pointer("/locations/0/path").and_then(|p| p.as_str()))
            .or_else(|| text("path"))
            .or_else(|| text("filePath"))
            .map(absolute)
            .unwrap_or_default()
    };
    // One of our tools (`MCPS-2`) reaches the timeline as OpenCode's own `execute`,
    // with the call written as code: `tools.poiesis.recall({...})`.
    if let Some(name) = text("code").and_then(our_tool_in) {
        return (format!("mcp__poiesis__{name}"), json!({}), title);
    }
    match kind {
        "execute" => ("Bash".into(), json!({"command": text("command").unwrap_or(&title)}), title.clone()),
        "edit" => {
            let new = diff
                .and_then(|d| d.get("newText").and_then(|t| t.as_str()))
                .or_else(|| text("content"))
                .or_else(|| text("newString"))
                .unwrap_or("");
            let old = diff.and_then(|d| d.get("oldText").and_then(|t| t.as_str())).or_else(|| text("oldString"));
            match old {
                Some(old) if !old.is_empty() => ("Edit".into(), json!({"file_path": path(), "old_string": old, "new_string": new}), title.clone()),
                _ => ("Write".into(), json!({"file_path": path(), "content": new}), title.clone()),
            }
        }
        "delete" => ("Delete".into(), json!({"file_path": path()}), title.clone()),
        "move" => ("Move".into(), json!({"file_path": path()}), title.clone()),
        "read" => ("Read".into(), json!({"file_path": path()}), title.clone()),
        "search" => ("Grep".into(), json!({"pattern": text("pattern").or_else(|| text("query")).unwrap_or(&title)}), title.clone()),
        "fetch" => ("WebFetch".into(), json!({"url": text("url").unwrap_or(&title)}), title.clone()),
        _ => {
            let name = call.get("name").and_then(|n| n.as_str()).filter(|n| !n.is_empty()).unwrap_or(kind);
            (name.to_string(), raw, title.clone())
        }
    }
}

/// The tool of ours that a piece of code calls, if it calls one.
fn our_tool_in(code: &str) -> Option<&'static str> {
    let rest = &code[code.find("tools.poiesis.")? + "tools.poiesis.".len()..];
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
    tools::short_name(&rest[..end])
}

/// One tool call as it has been told to us so far.
struct Tool {
    call: Value,
    started: bool,
    ended: bool,
}

/// What a run has seen so far.
struct Run {
    cwd: PathBuf,
    load_session: bool,
    /// The agent can reach an MCP server over HTTP.
    mcp_http: bool,
    session: Option<String>,
    resume_failed: bool,
    model: Option<String>,
    model_options: Vec<(String, String)>,
    /// What it has said since its last step, and the earlier pieces of this turn.
    segment: String,
    segments: Vec<String>,
    /// The closing words of each prompt, in order.
    turns: Vec<String>,
    last_was_text: bool,
    tools: HashMap<String, Tool>,
    usage: Option<Usage>,
    cost: Option<f64>,
    stop_reason: Option<String>,
    unknown: u32,
    /// Permission requests that were refused: their titles.
    rejected: Vec<String>,
}

impl Run {
    fn new(cwd: &Path) -> Run {
        Run {
            cwd: cwd.to_path_buf(),
            load_session: false,
            mcp_http: false,
            session: None,
            resume_failed: false,
            model: None,
            model_options: Vec::new(),
            segment: String::new(),
            segments: Vec::new(),
            turns: Vec::new(),
            last_was_text: false,
            tools: HashMap::new(),
            usage: None,
            cost: None,
            stop_reason: None,
            unknown: 0,
            rejected: Vec::new(),
        }
    }

    /// What the agent says it can do (read at run time, never assumed).
    fn learn(&mut self, init: &Value) {
        self.load_session = init.pointer("/agentCapabilities/loadSession").and_then(|v| v.as_bool()).unwrap_or(false);
        self.mcp_http = init.pointer("/agentCapabilities/mcpCapabilities/http").and_then(|v| v.as_bool()).unwrap_or(false);
    }

    fn set_session(&mut self, session: &str, result: &Value) {
        self.session = Some(session.to_string());
        let (options, current) = model_options(result);
        if !options.is_empty() {
            self.model_options = options;
        }
        if current.is_some() {
            self.model = current;
        }
    }

    /// A new prompt of the same session: close the words of the last one.
    fn next_turn(&mut self) {
        let closing = self.closing_words();
        if !closing.is_empty() {
            self.turns.push(closing);
        }
        self.segment.clear();
        self.segments.clear();
        self.last_was_text = false;
        self.stop_reason = None;
    }

    /// What this prompt ended on: the words after its last step, or, if it ended
    /// on a step, the words before it.
    fn closing_words(&self) -> String {
        let last = self.segment.trim();
        if !last.is_empty() {
            return last.to_string();
        }
        self.segments.iter().rev().map(|s| s.trim()).find(|s| !s.is_empty()).unwrap_or("").to_string()
    }

    fn notification(&mut self, method: &str, params: &Value, prompting: bool) -> Vec<Ev> {
        let mut ev = Vec::new();
        if method != "session/update" {
            self.unknown += 1;
            return ev;
        }
        // Before our prompt: history replayed by `session/load`, not this turn.
        if !prompting {
            return ev;
        }
        let Some(update) = params.get("update") else { return ev };
        match update.get("sessionUpdate").and_then(|s| s.as_str()) {
            Some("agent_message_chunk") => {
                if let Some(t) = update.pointer("/content/text").and_then(|t| t.as_str()).filter(|t| !t.is_empty()) {
                    let fresh = !self.last_was_text;
                    self.last_was_text = true;
                    self.segment.push_str(t);
                    ev.push(Ev::Delta { text: t.to_string(), fresh });
                }
            }
            Some("agent_thought_chunk") => {
                if let Some(t) = update.pointer("/content/text").and_then(|t| t.as_str()).filter(|t| !t.is_empty()) {
                    ev.push(Ev::Thinking(t.to_string()));
                }
            }
            Some("tool_call") => {
                let id = update.get("toolCallId").and_then(|i| i.as_str()).unwrap_or("").to_string();
                self.break_text();
                let call = strip(update);
                self.tools.insert(id.clone(), Tool { call, started: false, ended: false });
                // A tool call usually arrives `pending` with next to nothing in it
                // and fills in with its first `in_progress` update: the label is
                // taken from there. One that arrives already under way has all it
                // will say.
                if update.get("status").and_then(|s| s.as_str()).is_some_and(|s| s != "pending") {
                    ev.extend(self.start(&id));
                }
            }
            Some("tool_call_update") => {
                let id = update.get("toolCallId").and_then(|i| i.as_str()).unwrap_or("").to_string();
                let entry = self.tools.entry(id.clone()).or_insert_with(|| Tool { call: json!({}), started: false, ended: false });
                if let (Some(into), Some(from)) = (entry.call.as_object_mut(), strip(update).as_object()) {
                    for (k, v) in from {
                        // The result's `content` must not replace the diff the
                        // step was labelled from.
                        if k == "content" && entry.started {
                            continue;
                        }
                        into.insert(k.clone(), v.clone());
                    }
                }
                let status = update.get("status").and_then(|s| s.as_str()).unwrap_or("");
                let ready = status == "completed" || status == "failed" || has_input(&self.tools[&id].call);
                if ready && !self.tools[&id].started && matches!(status, "in_progress" | "completed" | "failed") {
                    self.break_text();
                    ev.extend(self.start(&id));
                }
                let tool = self.tools.get_mut(&id);
                if let Some(tool) = tool {
                    if tool.started && !tool.ended && (status == "completed" || status == "failed") {
                        tool.ended = true;
                        let result = clip(&result_text(update.get("content")), 2000);
                        ev.push(if status == "failed" {
                            Ev::StepError { id: id.clone(), error: if result.is_empty() { "it failed".into() } else { result } }
                        } else {
                            Ev::StepDone { id: id.clone(), result: (!result.is_empty()).then_some(result) }
                        });
                    }
                }
            }
            Some("usage_update") => {
                self.cost = update.pointer("/cost/amount").and_then(|c| c.as_f64()).or(self.cost);
            }
            Some("available_commands_update") | Some("user_message_chunk") | Some("plan") | Some("current_mode_update")
            | Some("config_option_update") | Some("session_info_update") => {}
            _ => self.unknown += 1,
        }
        ev
    }

    /// A step begins: the text before it is finished.
    fn break_text(&mut self) {
        if !self.segment.trim().is_empty() {
            self.segments.push(std::mem::take(&mut self.segment));
        }
        self.last_was_text = false;
    }

    fn start(&mut self, id: &str) -> Vec<Ev> {
        let Some(tool) = self.tools.get_mut(id) else { return Vec::new() };
        if tool.started {
            return Vec::new();
        }
        tool.started = true;
        let (name, input, _) = tool_of(&tool.call, &self.cwd);
        let (verb, target) = approve::describe_tool(&name, &input, &self.cwd);
        vec![Ev::StepStart { id: id.to_string(), verb, target }]
    }

    /// A step that began and never ended when the turn did: shown as stopped.
    fn close_open_steps(&mut self) -> Vec<Ev> {
        let mut open: Vec<String> = self.tools.iter().filter(|(_, t)| t.started && !t.ended).map(|(id, _)| id.clone()).collect();
        open.sort();
        open.into_iter()
            .map(|id| {
                if let Some(t) = self.tools.get_mut(&id) {
                    t.ended = true;
                }
                Ev::StepError { id, error: "it stopped before this finished".into() }
            })
            .collect()
    }

    fn finish(&mut self, result: &Value) {
        self.stop_reason = result.get("stopReason").and_then(|s| s.as_str()).map(String::from);
        if let Some(u) = result.get("usage") {
            let n = |k: &str| u.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
            let add = Usage { prompt_tokens: n("inputTokens") + n("cachedReadTokens"), output_tokens: n("outputTokens") };
            self.usage = Some(match self.usage {
                Some(old) => Usage { prompt_tokens: old.prompt_tokens + add.prompt_tokens, output_tokens: old.output_tokens + add.output_tokens },
                None => add,
            });
        }
    }

    fn into_outcome(mut self, chosen_model: Option<&str>) -> ExternalOutcome {
        let closing = self.closing_words();
        if !closing.is_empty() {
            self.turns.push(closing);
        }
        let declined = self.stop_reason.as_deref() == Some("cancelled") && !self.rejected.is_empty();
        let model = self.model.clone().or_else(|| chosen_model.map(String::from));
        // A model the agent calls free is free; anything else is the user's own
        // provider account (there is no plan of ours behind an ACP agent).
        let free = model.as_deref().is_some_and(|m| m.ends_with("-free"));
        ExternalOutcome {
            text: self.turns.join("\n\n"),
            session: self.session,
            usage: self.usage,
            notional_cost_usd: self.cost.filter(|c| *c > 0.0),
            paid_by: Some(if free { PaidBy::Free } else { PaidBy::Own }),
            stop_reason: if declined { "declined".into() } else { self.stop_reason.unwrap_or_default() },
            denials: self.rejected,
            unknown_events: self.unknown,
            model,
            stderr_tail: String::new(),
            resume_failed: self.resume_failed,
            ..Default::default()
        }
    }
}

/// An update without the fields that only name the update.
fn strip(update: &Value) -> Value {
    let mut v = update.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("sessionUpdate");
        o.remove("toolCallId");
    }
    v
}

fn has_input(call: &Value) -> bool {
    call.get("rawInput").and_then(|r| r.as_object()).is_some_and(|o| !o.is_empty())
}

/// The readable text of a tool update's `content`.
fn result_text(content: Option<&Value>) -> String {
    content
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.pointer("/content/text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replay what the real agent sent (`CLI-T-1`): the same updates, the same
    /// requests (answered as this driver would with nobody to ask), the same ends.
    fn replay(file: &str) -> (Run, Vec<Ev>) {
        // The recordings have the folder scrubbed to `<cwd>`; give it a real one.
        let file = &file.replace("<cwd>", "C:\\\\proj");
        let mut run = Run::new(Path::new("C:\\proj"));
        let mut events = Vec::new();
        let mut prompting = false;
        for line in file.lines().filter(|l| !l.trim().is_empty()) {
            let row: Value = serde_json::from_str(line).unwrap();
            let m = &row["msg"];
            if row["dir"] == "in" && m["method"] == "session/prompt" {
                prompting = true;
            }
            if row["dir"] != "out" {
                continue;
            }
            match (m.get("method").and_then(|x| x.as_str()), m.get("id")) {
                (Some("session/request_permission"), Some(id)) => {
                    if let Some(t) = refuse(id.clone(), &m["params"]).refused {
                        run.rejected.push(t);
                    }
                }
                (Some(method), None) => events.extend(run.notification(method, m.get("params").unwrap_or(&Value::Null), prompting)),
                (None, Some(_)) if m.pointer("/result/stopReason").is_some() => run.finish(&m["result"]),
                (None, Some(_)) if m.pointer("/result/sessionId").is_some() => {
                    run.set_session(m["result"]["sessionId"].as_str().unwrap(), &m["result"]);
                }
                (None, Some(_)) if m.pointer("/result/agentCapabilities").is_some() => run.learn(&m["result"]),
                _ => {}
            }
        }
        (run, events)
    }

    #[test]
    fn a_rejected_step_ends_the_turn_as_declined_not_as_an_error() {
        let (run, _) = replay(include_str!("../../../fixtures/engines/opencode-2.1.0.jsonl"));
        assert_eq!(run.rejected.len(), 3, "all three requests were refused by this replay");
        let out = run.into_outcome(None);
        assert_eq!(out.stop_reason, "declined");
        assert_eq!(out.denials.len(), 3);
        assert_eq!(out.paid_by, Some(PaidBy::Own), "a model id that does not say -free is the user's own account");
        assert!(out.usage.is_some());
    }

    #[test]
    fn history_replayed_by_a_load_is_not_part_of_the_answer() {
        let (run, _) = replay(include_str!("../../../fixtures/engines/opencode-2.1.0-load.jsonl"));
        let out = run.into_outcome(None);
        assert_eq!(out.stop_reason, "end_turn");
        // Only the answer to our question: not the replayed first prompt.
        assert_eq!(
            out.text,
            "I created one file, `hello.txt` (containing \"hi\"), and one directory, `allowed-dir`; `mkdir denied-dir` was declined."
        );
    }

    /// `CLI-T-1`: the screen events of the recording, in order. A step is
    /// labelled from the first update that has its input, and ends once.
    #[test]
    fn the_recording_plays_as_steps_with_the_timelines_words() {
        let (_, events) = replay(include_str!("../../../fixtures/engines/opencode-2.1.0.jsonl"));
        let starts: Vec<(&str, &str)> = events
            .iter()
            .filter_map(|e| match e {
                Ev::StepStart { verb, target, .. } => Some((verb.as_str(), target.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(starts.len(), 3, "{starts:?}");
        assert_eq!(starts[0], ("wrote", "hello.txt"), "{starts:?}");
        assert_eq!(starts[1], ("ran", "mkdir allowed-dir"));
        assert_eq!(starts[2], ("ran", "mkdir denied-dir"));
        let ended = events.iter().filter(|e| matches!(e, Ev::StepDone { .. } | Ev::StepError { .. })).count();
        assert_eq!(ended, 3, "each step ends exactly once even though it was updated twice");
        assert!(events.iter().any(|e| matches!(e, Ev::StepError { error, .. } if error.contains("declined"))), "{events:?}");
        assert!(events.iter().any(|e| matches!(e, Ev::Thinking(_))));
    }

    #[test]
    fn a_model_the_agent_calls_free_is_called_free() {
        let mut run = Run::new(Path::new("C:\\proj"));
        run.model = Some("opencode/beam-free".into());
        assert_eq!(run.into_outcome(None).paid_by, Some(PaidBy::Free));
    }

    #[test]
    fn the_second_recording_ends_normally_with_its_words() {
        let (run, events) = replay(include_str!("../../../fixtures/engines/opencode-2.1.0-set-model.jsonl"));
        assert!(events.iter().any(|e| matches!(e, Ev::Delta { text, .. } if text == "ready")));
        let out = run.into_outcome(None);
        assert_eq!(out.text, "ready");
        assert_eq!(out.stop_reason, "end_turn");
        assert_eq!(out.model.as_deref(), Some("opencode/big-pickle"));
    }

    #[test]
    fn the_models_come_from_the_new_sessions_config_options() {
        let line = include_str!("../../../fixtures/engines/opencode-2.1.0-set-model.jsonl")
            .lines()
            .find(|l| l.contains("\"sessionId\"") && l.contains("configOptions") && l.contains("\"id\":2"))
            .expect("the session/new reply");
        let row: Value = serde_json::from_str(line).unwrap();
        let (options, current) = model_options(&row["msg"]["result"]);
        assert!(options.len() > 3, "many models are offered");
        assert_eq!(current.as_deref(), Some("opencode/big-pickle"));
        assert!(options.iter().any(|(v, _)| v == "opencode/big-pickle"));
    }

    #[test]
    fn a_reject_is_picked_by_kind_whatever_the_order_or_the_names() {
        let p = json!({"toolCall": {"title": "x"}, "options": [
            {"optionId": "always", "kind": "allow_always", "name": "Reject"},
            {"optionId": "no", "kind": "reject_once", "name": "Allow once"},
            {"optionId": "once", "kind": "allow_once", "name": "Reject"},
        ]});
        assert_eq!(refuse(json!(1), &p).result["outcome"]["optionId"], "no");
        let o = options_of(&p);
        assert_eq!((o.once.as_deref(), o.reject.as_deref()), (Some("once"), Some("no")), "never the always option");
        // With no reject option, the request is cancelled. An allow is never chosen.
        let p = json!({"options": [{"optionId": "once", "kind": "allow_once", "name": "Allow"}]});
        assert_eq!(refuse(json!(1), &p).result["outcome"]["outcome"], "cancelled");
    }

    #[test]
    fn what_the_agent_asks_to_do_is_described_in_the_panels_words() {
        let cwd = Path::new("C:\\proj");
        // A new file.
        let call = json!({"kind": "edit", "title": "hello.txt", "locations": [{"path": "C:\\proj\\hello.txt"}],
            "rawInput": {"path": "hello.txt", "content": "hi"},
            "content": [{"type": "diff", "path": "C:\\proj\\hello.txt", "oldText": null, "newText": "hi"}]});
        let (tool, input, _) = tool_of(&call, cwd);
        assert_eq!(tool, "Write");
        assert_eq!(input["file_path"], "C:\\proj\\hello.txt");
        assert_eq!(input["content"], "hi");
        // A change to an existing one.
        let call = json!({"kind": "edit", "title": "a.rs", "content": [{"type": "diff", "path": "src\\a.rs", "oldText": "a", "newText": "b"}]});
        let (tool, input, _) = tool_of(&call, cwd);
        assert_eq!((tool.as_str(), input["old_string"].as_str(), input["new_string"].as_str()), ("Edit", Some("a"), Some("b")));
        assert_eq!(input["file_path"], "C:\\proj\\src\\a.rs", "a relative path is joined to the folder");
        // A command.
        let (tool, input, _) = tool_of(&json!({"kind": "execute", "title": "mkdir x", "rawInput": {"command": "mkdir x"}}), cwd);
        assert_eq!((tool.as_str(), input["command"].as_str()), ("Bash", Some("mkdir x")));
        // Anything else is asked about under its own name.
        let (tool, _, _) = tool_of(&json!({"kind": "other", "name": "mcp_thing", "title": "t"}), cwd);
        assert_eq!(tool, "mcp_thing");
    }

    #[test]
    fn a_sign_in_problem_while_starting_is_reported_as_one() {
        let e = json!({"code": -32000, "message": "Authentication required"});
        assert!(matches!(rpc_error(&e, true), DriverError::NeedsSignIn(_)));
        assert!(matches!(rpc_error(&e, false), DriverError::Failed(_)));
    }

    /// Words said between steps are kept apart, and the report is what it said
    /// last, not its running commentary.
    #[test]
    fn the_report_is_the_closing_words_not_the_commentary() {
        let mut run = Run::new(Path::new("C:\\proj"));
        let say = |run: &mut Run, t: &str| run.notification("session/update", &json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": t}}}), true);
        say(&mut run, "Let me look");
        say(&mut run, " first.");
        run.notification("session/update", &json!({"update": {"sessionUpdate": "tool_call", "toolCallId": "t1", "kind": "read", "title": "read", "rawInput": {"path": "a"}}}), true);
        let ev = say(&mut run, "Done.");
        assert_eq!(ev, vec![Ev::Delta { text: "Done.".into(), fresh: true }], "the first words after a step start a new paragraph");
        run.finish(&json!({"stopReason": "end_turn"}));
        assert_eq!(run.into_outcome(None).text, "Done.");
    }

    /// `MCPS-2`, as recorded from OpenCode 2.1.0: our server is named only when
    /// there is an address and the agent says it can reach an HTTP server.
    #[test]
    fn our_server_is_named_in_the_new_session_only_when_it_can_be_reached() {
        let url = "http://127.0.0.1:9/tok/mcp";
        assert_eq!(mcp_servers(Some(url), true), json!([{"type": "http", "name": "poiesis", "url": url, "headers": []}]));
        assert_eq!(mcp_servers(Some(url), false), json!([]), "the agent cannot reach it");
        assert_eq!(mcp_servers(None, true), json!([]), "the switch is off");
        let mut run = Run::new(Path::new("C:\\proj"));
        run.learn(&json!({"agentCapabilities": {"loadSession": true, "mcpCapabilities": {"http": true, "sse": false}}}));
        assert!(run.mcp_http && run.load_session);
        run.learn(&json!({"agentCapabilities": {}}));
        assert!(!run.mcp_http);
    }

    /// OpenCode runs our tool as its own `execute`, with the call as code. It reads
    /// as our tool in the timeline; other code stays what it was.
    #[test]
    fn a_call_to_our_tool_written_as_code_is_described_in_our_words() {
        let cwd = Path::new("C:\\proj");
        let call = |code: &str| json!({"kind": "other", "name": "execute", "title": "execute", "rawInput": {"code": code}});
        let (tool, input, _) = tool_of(&call("const r = await tools.poiesis.recall({ query: \"pnpm\" });\nreturn r;"), cwd);
        assert_eq!(tool, "mcp__poiesis__recall");
        assert_eq!(approve::describe_tool(&tool, &input, cwd).0, "recalled");
        assert_eq!(tool_of(&call("await tools.poiesis.ask_user({question: 'x'})"), cwd).0, "mcp__poiesis__ask_user");
        // Not ours: a wrong name, another server, or something else entirely.
        assert_eq!(tool_of(&call("await tools.poiesis.delete_everything()"), cwd).0, "execute");
        assert_eq!(tool_of(&call("await tools.github.recall({})"), cwd).0, "execute");
        assert_eq!(tool_of(&call("return 1 + 1;"), cwd).0, "execute");
    }

    #[test]
    fn a_step_that_never_ended_is_shown_as_stopped() {
        let mut run = Run::new(Path::new("C:\\proj"));
        let ev = run.notification("session/update", &json!({"update": {"sessionUpdate": "tool_call", "toolCallId": "t1", "status": "in_progress", "kind": "execute", "title": "sleep", "rawInput": {"command": "sleep 99"}}}), true);
        assert!(matches!(ev.first(), Some(Ev::StepStart { .. })));
        let closed = run.close_open_steps();
        assert!(matches!(closed.as_slice(), [Ev::StepError { .. }]));
        assert!(run.close_open_steps().is_empty(), "only once");
    }

    #[test]
    fn follow_up_words_are_joined_and_marked() {
        use crate::agent::fleet::Steer;
        assert_eq!(steer_text(&Steer::user("also add a test")), "also add a test");
    }
}
