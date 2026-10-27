//! Codex, driven through `codex app-server` (`CLI-11`).
//!
//! Newline-delimited JSON-RPC on stdio, **without** a `"jsonrpc"` field.
//! Recorded against 0.163.0 (`fixtures/engines/codex-*`). The calls, in order:
//! `initialize`, the `initialized` notification, `thread/start` (or
//! `thread/resume`), `turn/start`, then events until `turn/completed`.
//!
//! Rules this driver keeps, all from the recordings:
//! - `approvalsReviewer` is always `"user"`. The other values let Codex answer
//!   its own approval requests without asking us.
//! - The folder's trust picks Codex's own sandbox and approval policy
//!   ([`policy_for`]). In an `auto` folder Codex runs inside its sandbox and asks
//!   only to leave it, so those steps are counted, not asked about.
//! - An approval is answered `{decision: "accept" | "decline" | "cancel"}`.
//!   Never `acceptForSession` or an amendment: those write a rule into Codex's
//!   own files. A decline lets the model carry on, unlike an ACP agent.
//! - A file approval carries no path and no diff: they come from the `fileChange`
//!   item that started just before, joined on its id.
//! - A server request we do not know gets error `-32601`. This includes
//!   `account/chatgptAuthTokens/refresh`, so we never hand out a token.
//! - A message typed while it works goes in as `turn/steer`.
//! - Stop is `turn/interrupt` (a pending question is answered `cancel`), then the
//!   tree is killed.
//! - A watchdog: two minutes with no message, no question open and no Stop
//!   means a wedged `app-server` (it accepts input and never answers).
//! - `windowsSandbox/setupStart` is never called. On Windows an `auto` run asks
//!   `windowsSandbox/readiness` first; if the sandbox is not ready it runs with
//!   the ask-first policy instead, because nothing else would keep its commands
//!   in the folder.
//! - Poiesis's own tools (`MCPS-2`) are mounted with a `config` override on
//!   `thread/start` and `thread/resume`: `mcp_servers.poiesis.url` is the loopback
//!   address. Codex asks before each MCP tool call with
//!   `mcpServer/elicitation/request`; for our server that is answered `accept`
//!   without bothering the user, for any other server it gets `-32601` as before.
//!   `tool_timeout_sec` is set above the longest a question may wait (its default
//!   is 60 s). Recorded against 0.163.0.

use std::collections::{HashMap, HashSet};
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
use super::driver::{
    Approvals, Capabilities, DriverError, EngineDriver, ExternalOutcome, ExternalTurn, Job, Launch, ModelChoice, codex_stall,
};
use super::play::{clip, flush, keep_tail, play, steer_text, Asks, Ev};
use super::process::{self, Wait, Wire};
use super::tools;
use super::{EngineKind, PaidBy};

/// How long Codex waits for one of our tools: a bit more than a question waits
/// for a person (`approve::ASK_TIMEOUT`, ten minutes).
const TOOL_TIMEOUT_SECS: u64 = 660;

/// The server name Codex shows our tools under.
const SERVER: &str = "poiesis";

/// After `turn/interrupt`, how long Codex has to say the turn ended.
const CANCEL_GRACE: Duration = Duration::from_secs(3);

pub struct CodexDriver;

/// `(approvalPolicy, sandbox)` for a folder's trust (`CLI-6`). Never the sandbox
/// that is off (`danger-full-access`) and never the policy that never asks.
///
/// Recorded: `untrusted` asks for a file change and for every command that is
/// not plainly a read; `on-request` in `workspace-write` asked for nothing at
/// all (everything ran inside the sandbox); `on-request` in `read-only` asked for
/// a file change and let commands run, unable to write.
pub fn policy_for(trust: Trust) -> (&'static str, &'static str) {
    match trust {
        Trust::ReadOnly => ("on-request", "read-only"),
        Trust::Confirm => ("untrusted", "workspace-write"),
        Trust::Auto => ("on-request", "workspace-write"),
    }
}

#[async_trait]
impl EngineDriver for CodexDriver {
    fn kind(&self) -> EngineKind {
        EngineKind::Codex
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { resume: true, steer_mid_turn: true, interrupt: true, mcp_tools: true, approvals: Approvals::InBand }
    }

    /// `model/list` after the handshake. A fresh `app-server` that is stopped
    /// again straight away; no thread is started.
    async fn list_models(&self, launch: &Launch) -> Result<Vec<ModelChoice>, DriverError> {
        let cmd = process::command(&launch.program, &launch.base_args, &launch.env).map_err(DriverError::Start)?;
        let mut wire = Wire::start(cmd, None).map_err(DriverError::Start)?;
        let never = CancelFlag::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        let _ = wire.send(&json!({"id": 1, "method": "initialize", "params": init_params()})).await;
        let mut out = Err(DriverError::Protocol("no answer to model/list".into()));
        loop {
            match wire.next_until(&never, deadline).await {
                Wait::Message(m) => {
                    let id = m.get("id").and_then(|i| i.as_i64());
                    if m.get("method").is_some() {
                        // Nothing here needs a server request answered.
                        if let Some(rid) = m.get("id") {
                            let _ = wire.send(&json!({"id": rid, "error": {"code": -32601, "message": "method not found"}})).await;
                        }
                    } else if id == Some(1) {
                        let _ = wire.send(&json!({"method": "initialized"})).await;
                        let _ = wire.send(&json!({"id": 2, "method": "model/list", "params": {}})).await;
                    } else if id == Some(2) {
                        if let Some(err) = m.get("error") {
                            out = Err(rpc_error(err));
                        } else {
                            out = Ok(m
                                .pointer("/result/data")
                                .and_then(|d| d.as_array())
                                .map(|d| {
                                    d.iter()
                                        .filter_map(|x| {
                                            let id = x.get("id")?.as_str()?.to_string();
                                            let name = x.get("displayName").and_then(|n| n.as_str()).unwrap_or(&id).to_string();
                                            Some(ModelChoice { id, name, is_default: x.get("isDefault").and_then(|b| b.as_bool()).unwrap_or(false) })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default());
                        }
                        break;
                    }
                }
                Wait::Closed => {
                    out = Err(DriverError::Exited(String::new()));
                    break;
                }
                Wait::TimedOut => {
                    out = Err(DriverError::TimedOut);
                    break;
                }
                Wait::Cancelled => break,
            }
        }
        wire.finish().await;
        out
    }

    async fn run_turn(&self, turn: ExternalTurn<'_>) -> Result<ExternalOutcome, DriverError> {
        run(&turn).await
    }
}

fn init_params() -> Value {
    json!({"clientInfo": {"name": "poiesis", "title": null, "version": env!("CARGO_PKG_VERSION")}, "capabilities": {"experimentalApi": false}})
}

fn text_item(text: &str) -> Value {
    json!({"type": "text", "text": text, "text_elements": []})
}

/// What a reply from the app answers.
enum Pending {
    Init,
    /// `windowsSandbox/readiness`, asked before an `auto` thread on Windows.
    Readiness,
    /// `thread/resume` of the thread we hold.
    Resume(String),
    Start,
    Turn,
    Steer,
    Interrupt,
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
        let mut msg = json!({"id": id, "method": method});
        // A method that takes nothing is sent with no `params`, as recorded.
        if !params.is_null() {
            msg["params"] = params;
        }
        wire.send(&msg).await
    }
}

/// Does this trust lean on Codex's own sandbox to keep commands in the folder?
/// Only `auto` does: there Codex runs commands without asking (`policy_for`).
fn needs_sandbox(trust: Option<Trust>) -> bool {
    cfg!(windows) && trust == Some(Trust::Auto)
}

/// What a `windowsSandbox/readiness` result means. Anything but `ready` is
/// "not ready" (an error reply is too, in the loop).
fn sandbox_ready(result: &Value) -> bool {
    result.get("status").and_then(|s| s.as_str()) == Some("ready")
}

/// Start the thread, or continue the one we were given.
async fn open_thread(
    conn: &mut Conn,
    wire: &mut Wire,
    turn: &ExternalTurn<'_>,
    trust: Option<Trust>,
    mcp: Option<&str>,
    resume: Option<String>,
) -> Result<(), String> {
    match resume {
        Some(thread) => {
            // `excludeTurns`: do not load the whole history.
            let mut p = thread_params(turn, trust, mcp);
            p["threadId"] = json!(thread);
            p["excludeTurns"] = json!(true);
            conn.call(wire, "thread/resume", p, Pending::Resume(thread)).await
        }
        None => conn.call(wire, "thread/start", thread_params(turn, trust, mcp), Pending::Start).await,
    }
}

/// An answered approval, for the loop to send.
struct AskReply {
    id: Value,
    result: Value,
    /// The step it refused, in words.
    refused: Option<String>,
}

fn thread_params(turn: &ExternalTurn<'_>, trust: Option<Trust>, mcp_url: Option<&str>) -> Value {
    // Try it has no job: nothing may be written and everything is asked.
    let (approval, sandbox) = trust.map_or(("untrusted", "read-only"), policy_for);
    let mut p = json!({
        "cwd": turn.cwd,
        "approvalPolicy": approval,
        "sandbox": sandbox,
        "approvalsReviewer": "user",
    });
    if let Some(model) = &turn.launch.model {
        p["model"] = json!(model);
    }
    if let Some(url) = mcp_url {
        p["config"] = json!({"mcp_servers": {SERVER: {"url": url, "tool_timeout_sec": TOOL_TIMEOUT_SECS}}});
    }
    p
}

/// Codex asks before it runs an MCP tool. Ours were switched on by the user and
/// each only suggests or shows, so that question is answered `accept` here. Any
/// other server's tool is not ours to allow: `None` leaves it to the old path.
fn elicitation_reply(params: &Value, ours_mounted: bool) -> Option<Value> {
    let ours = ours_mounted
        && params.get("serverName").and_then(|s| s.as_str()) == Some(SERVER)
        && params.pointer("/_meta/codex_approval_kind").and_then(|k| k.as_str()) == Some("mcp_tool_call");
    ours.then(|| json!({"action": "accept", "content": null}))
}

async fn run(turn: &ExternalTurn<'_>) -> Result<ExternalOutcome, DriverError> {
    let launch = turn.launch;
    let job = turn.job.as_ref();
    let mut trust = job.map(|j| j.trust);
    let resume = job.and_then(|j| j.resume.clone());

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
    let cmd = process::command(&launch.program, &args, &launch.env).map_err(DriverError::Start)?;
    let mut wire = Wire::start(cmd, Some(&turn.cwd)).map_err(DriverError::Start)?;
    let overall = Instant::now() + turn.timeout;

    let mut conn = Conn { next_id: 1, pending: HashMap::new() };
    if let Err(e) = conn.call(&mut wire, "initialize", init_params(), Pending::Init).await {
        wire.finish().await;
        return Err(DriverError::Start(e));
    }

    let mut run = Run::new(&turn.cwd);
    let never = CancelFlag::new();
    let mut cancelled_at: Option<Instant> = None;
    let mut last_message = Instant::now();
    let mut asks: Asks<'_, AskReply> = FuturesUnordered::new();
    let mut calls: Asks<'_> = FuturesUnordered::new();

    let failure: Option<DriverError> = loop {
        // A question waiting on a person is not a stall, and neither is one of our
        // tools that is waiting for the same person.
        if !asks.is_empty() || !calls.is_empty() {
            last_message = Instant::now();
        }
        let (flag, until) = match cancelled_at {
            Some(t) => (&never, t + CANCEL_GRACE),
            None => (&turn.cancel, overall.min(last_message + codex_stall())),
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
                let _ = wire.send(&json!({"id": reply.id, "result": reply.result})).await;
            }
            w = wire.next_tick(flag, until) => match w {
                None => {
                    // Pass on what the user or the lead typed while it works.
                    if let (Some(j), Some(thread), Some(turn_id), None) = (job, run.thread_id.clone(), run.turn_id.clone(), cancelled_at) {
                        for steer in j.run.drain_steers() {
                            let sent = conn
                                .call(
                                    &mut wire,
                                    "turn/steer",
                                    json!({"threadId": thread, "expectedTurnId": turn_id, "input": [text_item(&steer_text(&steer))]}),
                                    Pending::Steer,
                                )
                                .await;
                            if sent.is_ok() {
                                j.sink.emit(AgentEvent::Steered { run_id: j.run.id.clone(), text: steer.text });
                            }
                        }
                    }
                }
                Some(Wait::Closed) => break Some(DriverError::Exited(String::new())),
                Some(Wait::TimedOut) => {
                    if cancelled_at.is_some() {
                        break Some(DriverError::Cancelled);
                    }
                    if Instant::now() >= overall {
                        break Some(DriverError::TimedOut);
                    }
                    // Quiet for too long: ask it to stop, then give up on it.
                    if let (Some(t), Some(u)) = (&run.thread_id, &run.turn_id) {
                        let _ = conn.call(&mut wire, "turn/interrupt", json!({"threadId": t, "turnId": u}), Pending::Interrupt).await;
                    }
                    break Some(DriverError::Stalled);
                }
                Some(Wait::Cancelled) => {
                    cancelled_at = Some(Instant::now());
                    match (&run.thread_id, &run.turn_id) {
                        (Some(t), Some(u)) => {
                            let _ = conn.call(&mut wire, "turn/interrupt", json!({"threadId": t, "turnId": u}), Pending::Interrupt).await;
                        }
                        _ => break Some(DriverError::Cancelled),
                    }
                }
                Some(Wait::Message(m)) => {
                    last_message = Instant::now();
                    let method = m.get("method").and_then(|x| x.as_str()).map(String::from);
                    let id = m.get("id").cloned();
                    match (method, id) {
                        // The app asks us something.
                        (Some(method), Some(id)) => {
                            let params = m.get("params").cloned().unwrap_or(Value::Null);
                            if method == "mcpServer/elicitation/request" {
                                if let Some(result) = elicitation_reply(&params, mcp.is_some()) {
                                    let _ = wire.send(&json!({"id": id, "result": result})).await;
                                    continue;
                                }
                            }
                            match run.ask_for(&method, &params) {
                                Some(ask) => match job {
                                    Some(j) => asks.push(Box::pin(decide(j.port, id, ask, &turn.cancel))),
                                    None => {
                                        // Try it: nothing is allowed.
                                        let reply = refuse(id, ask);
                                        run.rejected.push(reply.refused.clone().unwrap_or_default());
                                        let _ = wire.send(&json!({"id": reply.id, "result": reply.result})).await;
                                    }
                                },
                                None => {
                                    // A request this version does not know is refused, and counted.
                                    run.unknown += 1;
                                    let _ = wire.send(&json!({"id": id, "error": {"code": -32601, "message": "method not found"}})).await;
                                }
                            }
                        }
                        (Some(method), None) => {
                            let (events, done) = run.notification(&method, m.get("params").unwrap_or(&Value::Null));
                            if let Some(j) = job {
                                play(j, events);
                            }
                            if done {
                                break None;
                            }
                        }
                        (None, Some(id)) => {
                            let Some(entry) = id.as_i64().and_then(|i| conn.pending.remove(&i)) else { continue };
                            if let Some(err) = m.get("error") {
                                match entry {
                                    // The thread we were asked to continue is gone:
                                    // start a new one, once, and say so.
                                    Pending::Resume(_) if thread_is_gone(err) => {
                                        run.resume_failed = true;
                                        if let Err(e) = open_thread(&mut conn, &mut wire, turn, trust, mcp, None).await {
                                            break Some(DriverError::Start(e));
                                        }
                                    }
                                    // A steer that arrived as the turn ended, or an
                                    // interrupt of one that already had: nothing to do.
                                    Pending::Steer | Pending::Interrupt => {}
                                    // A Codex that cannot say is treated as not ready.
                                    Pending::Readiness => {
                                        trust = Some(Trust::Confirm);
                                        run.sandbox_off = true;
                                        if let Err(e) = open_thread(&mut conn, &mut wire, turn, trust, mcp, resume.clone()).await {
                                            break Some(DriverError::Start(e));
                                        }
                                    }
                                    _ => break Some(rpc_error(err)),
                                }
                                continue;
                            }
                            let result = m.get("result").cloned().unwrap_or(Value::Null);
                            match entry {
                                Pending::Init => {
                                    let _ = wire.send(&json!({"method": "initialized"})).await;
                                    // In an `auto` folder only Codex's sandbox keeps its
                                    // commands in the folder. Ask whether it is set up
                                    // first (read-only; `setupStart` is never called).
                                    let step = if needs_sandbox(trust) {
                                        conn.call(&mut wire, "windowsSandbox/readiness", Value::Null, Pending::Readiness).await
                                    } else {
                                        open_thread(&mut conn, &mut wire, turn, trust, mcp, resume.clone()).await
                                    };
                                    if let Err(e) = step {
                                        break Some(DriverError::Start(e));
                                    }
                                }
                                Pending::Readiness => {
                                    // Not set up: every step is asked about, as in
                                    // an ask-first folder, and the report says why.
                                    if !sandbox_ready(&result) {
                                        trust = Some(Trust::Confirm);
                                        run.sandbox_off = true;
                                    }
                                    if let Err(e) = open_thread(&mut conn, &mut wire, turn, trust, mcp, resume.clone()).await {
                                        break Some(DriverError::Start(e));
                                    }
                                }
                                Pending::Resume(asked) => {
                                    let thread = result.pointer("/thread/id").and_then(|t| t.as_str()).map(String::from).unwrap_or(asked);
                                    if let Err(e) = begin_turn(&mut conn, &mut wire, &mut run, turn, job, &thread, &result).await {
                                        break Some(e);
                                    }
                                }
                                Pending::Start => {
                                    let Some(thread) = result.pointer("/thread/id").and_then(|t| t.as_str()).map(String::from) else {
                                        break Some(DriverError::Protocol("thread/start gave no thread id".into()));
                                    };
                                    if let Err(e) = begin_turn(&mut conn, &mut wire, &mut run, turn, job, &thread, &result).await {
                                        break Some(e);
                                    }
                                }
                                Pending::Turn => {
                                    if let Some(id) = result.pointer("/turn/id").and_then(|t| t.as_str()) {
                                        run.turn_id = Some(id.to_string());
                                    }
                                }
                                Pending::Steer | Pending::Interrupt => {}
                            }
                        }
                        (None, None) => run.unknown += 1,
                    }
                }
            }
        }
    };

    // Let a question that was on the panel withdraw itself; Codex hears `cancel`.
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
    if cancelled_at.is_some() {
        return Err(DriverError::Cancelled);
    }
    match out.stop_reason.as_str() {
        "completed" => Ok(out),
        "interrupted" => Err(DriverError::Cancelled),
        _ => Err(DriverError::Failed(if out.text.is_empty() { "The turn failed without saying why.".into() } else { out.text })),
    }
}

/// The thread is known: say so, and send the task.
async fn begin_turn(
    conn: &mut Conn,
    wire: &mut Wire,
    run: &mut Run,
    turn: &ExternalTurn<'_>,
    job: Option<&Job<'_>>,
    thread: &str,
    started: &Value,
) -> Result<(), DriverError> {
    run.thread_id = Some(thread.to_string());
    run.model = started.pointer("/thread/model").and_then(|m| m.as_str()).map(String::from);
    if let Some(j) = job {
        (j.session_seen)(thread);
    }
    // The fixed words and the fenced task are two parts of the same message.
    let mut input = Vec::new();
    if let Some(j) = job {
        input.push(text_item(&j.system_append));
    }
    input.push(text_item(&turn.task));
    conn.call(wire, "turn/start", json!({"threadId": thread, "input": input}), Pending::Turn)
        .await
        .map_err(DriverError::Start)
}

/// One approval, put to the user (or refused) and answered `accept`, `decline`
/// or `cancel`.
async fn decide(port: &Port<'_>, id: Value, ask: Ask, stop: &CancelFlag) -> AskReply {
    let gone = CancelFlag::new();
    let verdict = port.decide(&ask.tool, &ask.input, &gone, stop).await;
    // Stop: the question is withdrawn and Codex is told so.
    if stop.is_cancelled() {
        return AskReply { id, result: json!({"decision": "cancel"}), refused: None };
    }
    match verdict {
        Verdict::Allow if ask.can_accept => AskReply { id, result: json!({"decision": "accept"}), refused: None },
        // No plain "accept" on offer: say no rather than pick another kind.
        Verdict::Allow => AskReply { id, result: json!({"decision": "cancel"}), refused: Some(ask.title) },
        Verdict::Deny(_) => AskReply { id, result: json!({"decision": "decline"}), refused: Some(ask.title) },
    }
}

/// Try it, and any run with nobody to ask: every approval is declined.
fn refuse(id: Value, ask: Ask) -> AskReply {
    AskReply { id, result: json!({"decision": "decline"}), refused: Some(ask.title) }
}

/// What an approval request is asking, in the words the panel already uses.
struct Ask {
    tool: String,
    input: Value,
    /// For the report's list of what was refused.
    title: String,
    /// `accept` is among the answers Codex offered.
    can_accept: bool,
}

fn rpc_error(err: &Value) -> DriverError {
    let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("The app returned an error.").to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("log in") || lower.contains("login") || lower.contains("sign in") || lower.contains("unauthorized") {
        DriverError::NeedsSignIn(msg)
    } else {
        DriverError::Failed(msg)
    }
}

/// `thread/resume` of a thread that no longer exists answers
/// `no rollout found for thread id ...`.
fn thread_is_gone(err: &Value) -> bool {
    let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("").to_ascii_lowercase();
    msg.contains("no rollout found") || msg.contains("not found")
}

/// The command as a person would type it. Codex reports the whole line with its
/// shell wrapper (`"...powershell.exe" -Command 'mkdir x'`) and, next to it, what
/// it understood the command to be.
fn inner_command(command: &str, actions: &Value) -> String {
    let actions = actions.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    if let [one] = actions {
        if let Some(c) = one.get("command").and_then(|c| c.as_str()).map(str::trim).filter(|c| !c.is_empty()) {
            return c.to_string();
        }
    }
    command.trim().to_string()
}

/// One file in a `fileChange` item.
#[derive(Debug, Clone)]
struct Change {
    path: String,
    /// `add` | `update` | `delete`; anything else reads as an update.
    kind: String,
    diff: String,
}

fn changes_of(item: &Value) -> Vec<Change> {
    item.get("changes")
        .and_then(|c| c.as_array())
        .map(|list| {
            list.iter()
                .map(|c| Change {
                    path: c.get("path").and_then(|p| p.as_str()).unwrap_or("").to_string(),
                    kind: c.pointer("/kind/type").and_then(|k| k.as_str()).unwrap_or("update").to_string(),
                    diff: c.get("diff").and_then(|d| d.as_str()).unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One step in a `fileChange`, as the timeline words it: `(verb, target)`.
fn change_step(changes: &[Change], cwd: &Path) -> (String, String) {
    let Some(first) = changes.first() else { return ("edited".into(), String::new()) };
    let verb = match first.kind.as_str() {
        "add" => "wrote",
        "delete" => "deleted",
        _ => "edited",
    };
    let mut target = approve::shown_path(&first.path, cwd);
    if changes.len() > 1 {
        target.push_str(&format!(" and {} more", changes.len() - 1));
    }
    (verb.into(), target)
}

/// What the panel is shown for a file approval.
fn file_ask(changes: &[Change], grant_root: Option<&str>, cwd: &Path) -> (String, Value, String) {
    let shown = |c: &Change| approve::shown_path(&c.path, cwd);
    if let ([one], None) = (changes, grant_root) {
        match one.kind.as_str() {
            "add" => return ("Write".into(), json!({"file_path": one.path, "content": one.diff}), format!("create {}", shown(one))),
            "delete" => return ("Delete".into(), json!({"file_path": one.path}), format!("delete {}", shown(one))),
            _ => {}
        }
    }
    let mut diff = String::new();
    if let Some(root) = grant_root {
        diff.push_str(&format!("It asks to write anywhere under {root}.\n\n"));
    }
    for c in changes {
        if changes.len() > 1 {
            diff.push_str(&format!("--- {}\n", shown(c)));
        }
        diff.push_str(&c.diff);
        if !diff.ends_with('\n') {
            diff.push('\n');
        }
    }
    let first = changes.first().map(|c| c.path.clone()).unwrap_or_default();
    let title = match changes {
        [] => "edit a file".to_string(),
        [one] => format!("edit {}", shown(one)),
        [one, rest @ ..] => format!("edit {} and {} more", shown(one), rest.len()),
    };
    ("Patch".into(), json!({"file_path": first, "diff": diff.trim_end()}), title)
}

/// An `mcpToolCall` item for one of our tools.
fn is_ours(item: &Value) -> bool {
    item.get("server").and_then(|s| s.as_str()) == Some(SERVER)
}

/// The timeline words for one of our tools, the same as for Claude Code.
fn our_step(item: &Value, cwd: &Path) -> (String, String) {
    let tool = item.get("tool").and_then(|t| t.as_str()).unwrap_or("");
    approve::describe_tool(&format!("mcp__{SERVER}__{tool}"), item.get("arguments").unwrap_or(&Value::Null), cwd)
}

/// What a run has seen so far.
struct Run {
    cwd: PathBuf,
    thread_id: Option<String>,
    turn_id: Option<String>,
    model: Option<String>,
    resume_failed: bool,
    /// The sandbox was not set up, so this `auto` run asked about every step.
    sandbox_off: bool,
    final_text: Option<String>,
    last_text: Option<String>,
    /// Everything streamed, in case no message ever completed.
    deltas: String,
    /// The message the last delta belonged to, and the ones that streamed at all.
    last_text_item: Option<String>,
    streamed: HashSet<String>,
    usage: Option<Usage>,
    paid_by: Option<PaidBy>,
    status: Option<String>,
    error: Option<String>,
    unknown: u32,
    /// File changes seen so far, by item id: the approval has no path of its own.
    changes: HashMap<String, Vec<Change>>,
    /// Steps shown, and steps ended.
    started: HashSet<String>,
    ended: HashSet<String>,
    /// Items it asked about.
    asked: HashSet<String>,
    /// Steps that ran without a question to us.
    self_allowed: u32,
    /// Approvals that were refused: what they were.
    rejected: Vec<String>,
}

/// Method families we know exist. A message outside them is counted.
const KNOWN_PREFIXES: &[&str] = &[
    "thread/",
    "turn/",
    "item/",
    "account/",
    "mcpServer/",
    "remoteControl/",
    "serverRequest/",
    "windowsSandbox/",
    "model/",
    "deprecationNotice",
];

/// Item types we show or deliberately ignore. Another is counted.
const KNOWN_ITEMS: &[&str] = &["userMessage", "reasoning", "agentMessage", "commandExecution", "fileChange"];

impl Run {
    fn new(cwd: &Path) -> Run {
        Run {
            cwd: cwd.to_path_buf(),
            thread_id: None,
            turn_id: None,
            model: None,
            resume_failed: false,
            sandbox_off: false,
            final_text: None,
            last_text: None,
            deltas: String::new(),
            last_text_item: None,
            streamed: HashSet::new(),
            usage: None,
            paid_by: None,
            status: None,
            error: None,
            unknown: 0,
            changes: HashMap::new(),
            started: HashSet::new(),
            ended: HashSet::new(),
            asked: HashSet::new(),
            self_allowed: 0,
            rejected: Vec::new(),
        }
    }

    /// An approval request from the app, or `None` for a request we do not
    /// answer (which gets error `-32601`).
    fn ask_for(&mut self, method: &str, params: &Value) -> Option<Ask> {
        let item = params.get("itemId").and_then(|i| i.as_str()).unwrap_or("").to_string();
        let can_accept = params
            .get("availableDecisions")
            .and_then(|d| d.as_array())
            .is_none_or(|d| d.iter().any(|x| x.as_str() == Some("accept")));
        let (tool, input, title) = match method {
            "item/commandExecution/requestApproval" => {
                let command = inner_command(
                    params.get("command").and_then(|c| c.as_str()).unwrap_or(""),
                    params.get("commandActions").unwrap_or(&Value::Null),
                );
                ("Bash".to_string(), json!({"command": command}), command)
            }
            "item/fileChange/requestApproval" => {
                let changes = self.changes.get(&item).cloned().unwrap_or_default();
                file_ask(&changes, params.get("grantRoot").and_then(|g| g.as_str()), &self.cwd)
            }
            // Everything else, `account/chatgptAuthTokens/refresh` included.
            _ => return None,
        };
        self.asked.insert(item);
        Some(Ask { tool, input, title, can_accept })
    }

    /// Fold one notification in. `true` = the turn is over.
    fn notification(&mut self, method: &str, params: &Value) -> (Vec<Ev>, bool) {
        let mut ev = Vec::new();
        match method {
            "account/updated" => {
                self.paid_by = match params.get("authMode").and_then(|a| a.as_str()) {
                    Some("chatgpt") => Some(PaidBy::Plan),
                    Some("apikey") | Some("api_key") | Some("apiKey") => Some(PaidBy::ApiKey),
                    _ => self.paid_by,
                };
            }
            "thread/tokenUsage/updated" => {
                if let Some(t) = params.pointer("/tokenUsage/total") {
                    let n = |k: &str| t.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                    self.usage = Some(Usage { prompt_tokens: n("inputTokens"), output_tokens: n("outputTokens") });
                }
            }
            "turn/started" => {
                if let Some(id) = params.pointer("/turn/id").and_then(|t| t.as_str()) {
                    self.turn_id = Some(id.to_string());
                }
            }
            "item/agentMessage/delta" => {
                if let Some(d) = params.get("delta").and_then(|d| d.as_str()).filter(|d| !d.is_empty()) {
                    let item = params.get("itemId").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    let fresh = self.last_text_item.as_deref() != Some(item.as_str());
                    self.last_text_item = Some(item.clone());
                    self.streamed.insert(item);
                    self.deltas.push_str(d);
                    ev.push(Ev::Delta { text: d.to_string(), fresh });
                }
            }
            "item/started" => {
                if let Some(item) = params.get("item") {
                    ev.extend(self.item_started(item));
                }
            }
            "item/completed" => {
                if let Some(item) = params.get("item") {
                    ev.extend(self.item_completed(item));
                }
            }
            "turn/completed" => {
                if let Some(turn) = params.get("turn") {
                    if let Some(items) = turn.get("items").and_then(|i| i.as_array()) {
                        for item in items {
                            self.take_agent_message(item);
                        }
                    }
                    self.status = turn.get("status").and_then(|s| s.as_str()).map(String::from);
                    self.error = turn.get("error").and_then(|e| {
                        e.as_str().map(String::from).or_else(|| e.get("message").and_then(|m| m.as_str()).map(String::from))
                    });
                }
                return (ev, true);
            }
            m if !KNOWN_PREFIXES.iter().any(|p| m.starts_with(p)) => self.unknown += 1,
            _ => {}
        }
        (ev, false)
    }

    fn item_started(&mut self, item: &Value) -> Vec<Ev> {
        let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
        match item.get("type").and_then(|t| t.as_str()) {
            Some("commandExecution") => {
                let command = inner_command(
                    item.get("command").and_then(|c| c.as_str()).unwrap_or(""),
                    item.get("commandActions").unwrap_or(&Value::Null),
                );
                let (verb, target) = approve::describe_tool("Bash", &json!({"command": command}), &self.cwd);
                self.start(&id, verb, target)
            }
            Some("fileChange") => {
                let changes = changes_of(item);
                let (verb, target) = change_step(&changes, &self.cwd);
                self.changes.insert(id.clone(), changes);
                self.start(&id, verb, target)
            }
            Some("mcpToolCall") if is_ours(item) => {
                let (verb, target) = our_step(item, &self.cwd);
                self.start(&id, verb, target)
            }
            Some(other) if !KNOWN_ITEMS.contains(&other) => {
                self.unknown += 1;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn start(&mut self, id: &str, verb: String, target: String) -> Vec<Ev> {
        if !self.started.insert(id.to_string()) {
            return Vec::new();
        }
        // The words after a step begin a new paragraph.
        self.last_text_item = None;
        vec![Ev::StepStart { id: id.to_string(), verb, target }]
    }

    fn item_completed(&mut self, item: &Value) -> Vec<Ev> {
        let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
        let ty = item.get("type").and_then(|t| t.as_str());
        match ty {
            Some("agentMessage") => {
                self.take_agent_message(item);
                // A message that never streamed is shown whole.
                let text = item.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if !text.trim().is_empty() && !self.streamed.contains(&id) {
                    let fresh = self.last_text_item.as_deref() != Some(id.as_str());
                    self.last_text_item = Some(id);
                    return vec![Ev::Delta { text: text.to_string(), fresh }];
                }
                Vec::new()
            }
            Some("commandExecution") | Some("fileChange") => {
                let mut ev = Vec::new();
                if !self.started.contains(&id) {
                    // It ended without our seeing it begin.
                    if ty == Some("fileChange") {
                        let changes = changes_of(item);
                        let (verb, target) = change_step(&changes, &self.cwd);
                        self.changes.insert(id.clone(), changes);
                        ev.extend(self.start(&id, verb, target));
                    } else {
                        let command = inner_command(
                            item.get("command").and_then(|c| c.as_str()).unwrap_or(""),
                            item.get("commandActions").unwrap_or(&Value::Null),
                        );
                        let (verb, target) = approve::describe_tool("Bash", &json!({"command": command}), &self.cwd);
                        ev.extend(self.start(&id, verb, target));
                    }
                }
                if !self.ended.insert(id.clone()) {
                    return ev;
                }
                let status = item.get("status").and_then(|s| s.as_str()).unwrap_or("completed");
                let exit = item.get("exitCode").and_then(|c| c.as_i64());
                let output = clip(item.get("aggregatedOutput").and_then(|o| o.as_str()).unwrap_or(""), 2000);
                // Ran, and nobody on our side was asked.
                if status != "declined" && !self.asked.contains(&id) {
                    self.self_allowed += 1;
                }
                ev.push(match status {
                    "declined" => Ev::StepError { id, error: "declined".into() },
                    "failed" => Ev::StepError { id, error: if output.is_empty() { "it failed".into() } else { output } },
                    _ if exit.is_some_and(|c| c != 0) => Ev::StepError {
                        id,
                        error: if output.is_empty() { format!("it ended with code {}", exit.unwrap_or(0)) } else { output },
                    },
                    _ => Ev::StepDone { id, result: (!output.is_empty()).then_some(output) },
                });
                ev
            }
            // One of our tools (`MCPS-2`). Not counted as self-allowed: we answered it.
            Some("mcpToolCall") if is_ours(item) => {
                let mut ev = Vec::new();
                if !self.started.contains(&id) {
                    let (verb, target) = our_step(item, &self.cwd);
                    ev.extend(self.start(&id, verb, target));
                }
                if !self.ended.insert(id.clone()) {
                    return ev;
                }
                let failed = item.get("status").and_then(|s| s.as_str()) == Some("failed");
                let text = if failed {
                    clip(item.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or(""), 2000)
                } else {
                    let parts: Vec<&str> = item
                        .pointer("/result/content")
                        .and_then(|c| c.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                        .collect();
                    clip(&parts.join("\n"), 2000)
                };
                ev.push(if failed {
                    Ev::StepError { id, error: if text.is_empty() { "it failed".into() } else { text } }
                } else {
                    Ev::StepDone { id, result: (!text.is_empty()).then_some(text) }
                });
                ev
            }
            _ => Vec::new(),
        }
    }

    fn take_agent_message(&mut self, item: &Value) {
        if item.get("type").and_then(|t| t.as_str()) != Some("agentMessage") {
            return;
        }
        let Some(text) = item.get("text").and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()) else { return };
        if item.get("phase").and_then(|p| p.as_str()) == Some("final_answer") {
            self.final_text = Some(text.to_string());
        }
        self.last_text = Some(text.to_string());
    }

    /// A step that began and never ended when the turn did: shown as stopped.
    fn close_open_steps(&mut self) -> Vec<Ev> {
        let mut open: Vec<String> = self.started.iter().filter(|id| !self.ended.contains(*id)).cloned().collect();
        open.sort();
        open.into_iter()
            .map(|id| {
                self.ended.insert(id.clone());
                Ev::StepError { id, error: "it stopped before this finished".into() }
            })
            .collect()
    }

    fn into_outcome(self, chosen_model: Option<&str>) -> ExternalOutcome {
        let status = self.status.clone().unwrap_or_else(|| "unknown".into());
        let text = if status == "completed" {
            self.final_text.or(self.last_text).unwrap_or(self.deltas)
        } else {
            self.error.unwrap_or_default()
        };
        ExternalOutcome {
            text,
            session: self.thread_id,
            model: self.model.or_else(|| chosen_model.map(String::from)),
            usage: self.usage,
            notional_cost_usd: None,
            paid_by: self.paid_by,
            stop_reason: status,
            denials: self.rejected,
            unknown_events: self.unknown,
            stderr_tail: String::new(),
            self_allowed: self.self_allowed,
            resume_failed: self.resume_failed,
            sandbox_off: self.sandbox_off,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CWD: &str = "C:\\proj";

    /// Replay what the real app-server sent (`CLI-T-1`): the same notifications,
    /// and its approval requests answered the way `allow` says. Returns the run,
    /// every event that would reach the screen, and every answer given.
    fn replay(file: &str, allow: bool) -> (Run, Vec<Ev>, Vec<Value>, bool) {
        let file = &file.replace("<cwd>", "C:\\\\proj");
        let mut run = Run::new(Path::new(CWD));
        let mut events = Vec::new();
        let mut answers = Vec::new();
        let mut done = false;
        for line in file.lines().filter(|l| !l.trim().is_empty()) {
            let row: Value = serde_json::from_str(line).unwrap();
            if row["dir"] != "out" {
                continue;
            }
            let m = &row["msg"];
            match (m.get("method").and_then(|x| x.as_str()), m.get("id")) {
                (Some(method), Some(_)) => {
                    if let Some(ask) = run.ask_for(method, m.get("params").unwrap_or(&Value::Null)) {
                        let reply = if allow { json!({"decision": "accept"}) } else { json!({"decision": "decline"}) };
                        if !allow {
                            run.rejected.push(ask.title.clone());
                        }
                        answers.push(json!({"tool": ask.tool, "input": ask.input, "title": ask.title, "answer": reply}));
                    }
                }
                (Some(method), None) => {
                    let (ev, over) = run.notification(method, m.get("params").unwrap_or(&Value::Null));
                    events.extend(ev);
                    if over {
                        done = true;
                        break;
                    }
                }
                _ => {}
            }
        }
        (run, events, answers, done)
    }

    fn steps(events: &[Ev]) -> Vec<(String, String)> {
        events
            .iter()
            .filter_map(|e| match e {
                Ev::StepStart { verb, target, .. } => Some((verb.clone(), target.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_recorded_session_ends_with_a_final_report_usage_and_three_questions() {
        let (run, events, answers, done) = replay(include_str!("../../../fixtures/engines/codex-0.163.0.jsonl"), false);
        assert!(done);
        assert_eq!(answers.len(), 3, "a file add and two commands asked");
        assert_eq!(run.rejected.len(), 3);
        let out = run.into_outcome(None);
        assert_eq!(out.stop_reason, "completed");
        assert!(out.text.contains("hello.txt"), "{}", out.text);
        assert_eq!(out.paid_by, Some(PaidBy::Plan));
        assert!(out.usage.unwrap().prompt_tokens > 0);
        assert!(out.notional_cost_usd.is_none(), "Codex reports tokens, never a price");
        assert_eq!(out.unknown_events, 0);
        assert_eq!(out.self_allowed, 0, "every step was asked about");
        assert_eq!(
            steps(&events),
            vec![
                ("wrote".to_string(), "hello.txt".to_string()),
                ("ran".to_string(), "mkdir allowed-dir".to_string()),
                ("ran".to_string(), "mkdir denied-dir".to_string()),
            ],
            "a command shows as typed, not with its shell wrapper"
        );
    }

    /// The panel is shown the real thing, not the wrapper around it: the command as
    /// typed, and the new file with its words (the approval itself carries neither
    /// path nor diff).
    #[test]
    fn the_questions_name_the_command_and_the_file() {
        let (_, _, answers, _) = replay(include_str!("../../../fixtures/engines/codex-0.163.0.jsonl"), true);
        assert_eq!(answers[0]["tool"], "Write");
        assert!(answers[0]["input"]["file_path"].as_str().unwrap().ends_with("hello.txt"), "{}", answers[0]);
        assert_eq!(answers[0]["input"]["content"], "hi\n");
        assert_eq!(answers[0]["title"], "create hello.txt");
        assert_eq!(answers[1]["tool"], "Bash");
        assert_eq!(answers[1]["input"], json!({"command": "mkdir allowed-dir"}));
        assert_eq!(answers[2]["title"], "mkdir denied-dir");
    }

    /// `on-request` in a writable sandbox asked for nothing; every step is counted
    /// as one its own settings allowed (`CLI-6`'s footer).
    #[test]
    fn on_request_in_a_writable_sandbox_asks_nothing_and_the_steps_are_counted() {
        let (run, events, answers, done) = replay(include_str!("../../../fixtures/engines/codex-0.163.0-on-request.jsonl"), true);
        assert!(done);
        assert!(answers.is_empty());
        let out = run.into_outcome(None);
        assert_eq!(out.self_allowed, 3);
        assert_eq!(
            steps(&events),
            vec![
                ("wrote".to_string(), "a.txt".to_string()),
                ("ran".to_string(), "mkdir inside-dir".to_string()),
                ("ran".to_string(), "Get-Date".to_string()),
            ]
        );
        let ended = events.iter().filter(|e| matches!(e, Ev::StepDone { .. } | Ev::StepError { .. })).count();
        assert_eq!(ended, 3);
    }

    /// `on-request` in a read-only sandbox asked about the file and let commands
    /// run unable to write. The one that could not write ends as an error step.
    #[test]
    fn read_only_asks_about_files_and_a_command_that_cannot_write_fails_visibly() {
        let (run, events, answers, done) = replay(include_str!("../../../fixtures/engines/codex-0.163.0-read-only.jsonl"), false);
        assert!(done);
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0]["tool"], "Write");
        let errors: Vec<_> = events.iter().filter(|e| matches!(e, Ev::StepError { .. })).collect();
        assert!(!errors.is_empty(), "the mkdir that the sandbox refused is an error step");
        let out = run.into_outcome(None);
        assert_eq!(out.self_allowed, 2, "a listing and a mkdir that ran in the sandbox without a question");
    }

    #[test]
    fn an_interrupted_turn_is_not_a_success() {
        let (run, _, _, done) = replay(include_str!("../../../fixtures/engines/codex-0.163.0-interrupt.jsonl"), false);
        assert!(done);
        assert_eq!(run.into_outcome(None).stop_reason, "interrupted");
    }

    #[test]
    fn a_steered_turn_is_one_turn_with_both_answers_in_the_report() {
        let (run, events, _, done) = replay(include_str!("../../../fixtures/engines/codex-0.163.0-steer.jsonl"), true);
        assert!(done);
        let out = run.into_outcome(None);
        assert_eq!(out.stop_reason, "completed");
        assert!(!out.text.is_empty());
        assert_eq!(out.unknown_events, 0);
        assert!(steps(&events).len() >= 2, "{:?}", steps(&events));
    }

    #[test]
    fn only_the_two_known_approvals_are_asked_and_the_rest_get_an_error() {
        let mut run = Run::new(Path::new(CWD));
        for m in ["item/commandExecution/requestApproval", "item/fileChange/requestApproval"] {
            assert!(run.ask_for(m, &json!({"itemId": "x", "command": "ls"})).is_some(), "{m}");
        }
        // A token refresh is refused outright, as is anything unknown.
        assert!(run.ask_for("account/chatgptAuthTokens/refresh", &json!({})).is_none());
        assert!(run.ask_for("item/tool/requestUserInput", &json!({})).is_none());
        assert!(run.ask_for("item/permissions/requestApproval", &json!({})).is_none());
        assert!(run.ask_for("applyPatchApproval", &json!({})).is_none());
    }

    #[test]
    fn only_a_ready_answer_counts_as_a_ready_sandbox() {
        assert!(sandbox_ready(&json!({"status": "ready"})), "as recorded on 0.163.0");
        assert!(!sandbox_ready(&json!({"status": "notConfigured"})));
        assert!(!sandbox_ready(&json!({})));
        // Only an `auto` folder leans on the sandbox, and only Windows is asked.
        assert_eq!(needs_sandbox(Some(Trust::Auto)), cfg!(windows));
        assert!(!needs_sandbox(Some(Trust::Confirm)) && !needs_sandbox(Some(Trust::ReadOnly)) && !needs_sandbox(None));
    }

    /// Trust becomes Codex's own sandbox and policy; none of them is the one that
    /// turns the sandbox off or the one that never asks.
    #[test]
    fn the_folders_trust_picks_the_sandbox_and_the_policy() {
        assert_eq!(policy_for(Trust::ReadOnly), ("on-request", "read-only"));
        assert_eq!(policy_for(Trust::Confirm), ("untrusted", "workspace-write"));
        assert_eq!(policy_for(Trust::Auto), ("on-request", "workspace-write"));
        for t in [Trust::ReadOnly, Trust::Confirm, Trust::Auto] {
            let (policy, sandbox) = policy_for(t);
            assert_ne!(policy, "never");
            assert_ne!(sandbox, "danger-full-access");
        }
    }

    #[test]
    fn a_question_without_a_plain_accept_cannot_be_allowed() {
        let mut run = Run::new(Path::new(CWD));
        let ask = run.ask_for("item/commandExecution/requestApproval", &json!({"itemId": "a", "command": "ls", "availableDecisions": ["cancel"]})).unwrap();
        assert!(!ask.can_accept);
        let ask = run.ask_for("item/commandExecution/requestApproval", &json!({"itemId": "b", "command": "ls", "availableDecisions": ["accept", "cancel"]})).unwrap();
        assert!(ask.can_accept);
    }

    #[test]
    fn a_request_for_a_whole_folder_says_so_on_the_panel() {
        let mut run = Run::new(Path::new(CWD));
        run.notification(
            "item/started",
            &json!({"item": {"type": "fileChange", "id": "f1", "status": "inProgress", "changes": [{"path": "C:\\proj\\a.rs", "kind": {"type": "update"}, "diff": "@@ -1 +1 @@\n-a\n+b\n"}]}}),
        );
        let ask = run.ask_for("item/fileChange/requestApproval", &json!({"itemId": "f1", "grantRoot": "C:\\elsewhere"})).unwrap();
        assert_eq!(ask.tool, "Patch");
        assert!(ask.input["diff"].as_str().unwrap().starts_with("It asks to write anywhere under C:\\elsewhere."), "{}", ask.input);
    }

    #[test]
    fn a_message_that_never_streamed_is_shown_whole_and_new_kinds_of_item_are_counted() {
        let mut run = Run::new(Path::new(CWD));
        let (ev, _) = run.notification("item/completed", &json!({"item": {"type": "agentMessage", "id": "m1", "text": "all done", "phase": "final_answer"}}));
        assert_eq!(ev, vec![Ev::Delta { text: "all done".into(), fresh: true }]);
        run.notification("item/started", &json!({"item": {"type": "somethingNew", "id": "n1"}}));
        run.notification("something/else", &json!({}));
        assert_eq!(run.unknown, 2);
    }

    #[test]
    fn a_step_that_never_ended_is_closed_as_stopped() {
        let mut run = Run::new(Path::new(CWD));
        run.notification("item/started", &json!({"item": {"type": "commandExecution", "id": "c1", "command": "sleep 99", "commandActions": []}}));
        let closed = run.close_open_steps();
        assert!(matches!(&closed[..], [Ev::StepError { id, .. }] if id == "c1"));
        assert!(run.close_open_steps().is_empty());
    }

    /// `MCPS-2`, from a real 0.163.0 session: the server goes in `config`, with a
    /// timeout longer than a question waits; no `config` without the switch.
    #[test]
    fn our_server_is_mounted_in_the_thread_config_only_when_asked_for() {
        let launch = Launch { program: "codex".into(), base_args: vec![], extra_args: vec![], env: vec![], auto_env: vec![], model: None };
        let turn = ExternalTurn { launch: &launch, task: String::new(), cwd: PathBuf::from(CWD), tool_free: false, cancel: CancelFlag::new(), timeout: Duration::from_secs(5), job: None };
        let with = thread_params(&turn, Some(Trust::Confirm), Some("http://127.0.0.1:9/tok/mcp"));
        assert_eq!(with["config"]["mcp_servers"]["poiesis"]["url"], "http://127.0.0.1:9/tok/mcp");
        assert_eq!(with["config"]["mcp_servers"]["poiesis"]["tool_timeout_sec"], TOOL_TIMEOUT_SECS);
        assert!(TOOL_TIMEOUT_SECS > super::approve::ASK_TIMEOUT.as_secs(), "a tool may not time out before the person has had their time");
        assert_eq!(with["approvalsReviewer"], "user", "the rest of the policy is unchanged");
        assert!(thread_params(&turn, Some(Trust::Confirm), None).get("config").is_none());
    }

    /// The question Codex asks before an MCP tool call, as recorded: ours is
    /// allowed quietly, another server's is left to the old path.
    #[test]
    fn only_our_servers_tool_calls_are_allowed_without_asking() {
        let ask = |server: &str, kind: &str| {
            json!({"threadId": "t", "turnId": "u", "serverName": server, "mode": "form", "_meta": {"codex_approval_kind": kind},
                   "message": "Allow the poiesis MCP server to run tool \"recall\"?", "requestedSchema": {"type": "object", "properties": {}}})
        };
        assert_eq!(elicitation_reply(&ask("poiesis", "mcp_tool_call"), true), Some(json!({"action": "accept", "content": null})));
        assert_eq!(elicitation_reply(&ask("poiesis", "mcp_tool_call"), false), None, "not mounted: not ours to allow");
        assert_eq!(elicitation_reply(&ask("github", "mcp_tool_call"), true), None);
        assert_eq!(elicitation_reply(&ask("poiesis", "something_else"), true), None);
    }

    #[test]
    fn a_call_to_our_tool_is_a_step_in_our_words_and_another_servers_call_is_counted() {
        let mut run = Run::new(Path::new(CWD));
        let call = |id: &str, server: &str, status: &str, extra: Value| {
            let mut item = json!({"type": "mcpToolCall", "id": id, "server": server, "tool": "recall", "status": status, "arguments": {"query": "pnpm"}});
            for (k, v) in extra.as_object().cloned().unwrap_or_default() {
                item[k] = v;
            }
            json!({"item": item})
        };
        let (ev, _) = run.notification("item/started", &call("m1", "poiesis", "inProgress", json!({})));
        assert!(matches!(&ev[..], [Ev::StepStart { verb, target, .. }] if verb == "recalled" && target == "pnpm"), "{ev:?}");
        let done = json!({"result": {"content": [{"type": "text", "text": "Found: we agreed on pnpm."}]}});
        let (ev, _) = run.notification("item/completed", &call("m1", "poiesis", "completed", done));
        assert!(matches!(&ev[..], [Ev::StepDone { result: Some(r), .. }] if r == "Found: we agreed on pnpm."), "{ev:?}");
        assert_eq!(run.self_allowed, 0, "we answered it, Codex did not let it through by itself");

        let (ev, _) = run.notification("item/completed", &call("m2", "poiesis", "failed", json!({"error": {"message": "timed out awaiting tools/call after 3s"}})));
        assert!(matches!(&ev[..], [Ev::StepStart { .. }, Ev::StepError { error, .. }] if error.contains("timed out")), "{ev:?}");

        // Somebody else's server is not ours to describe.
        let (ev, _) = run.notification("item/started", &call("m3", "github", "inProgress", json!({})));
        assert!(ev.is_empty());
        assert_eq!(run.unknown, 1);
    }

    #[test]
    fn a_missing_thread_is_a_readable_error() {
        let file = include_str!("../../../fixtures/engines/codex-0.163.0-resume-missing.jsonl");
        let err = file
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find_map(|r| r["msg"].get("error").cloned())
            .expect("the recording has an error reply");
        assert!(thread_is_gone(&err));
        assert!(matches!(rpc_error(&err), DriverError::Failed(m) if m.contains("no rollout found")));
    }

    #[test]
    fn a_sign_in_error_is_reported_as_needing_sign_in() {
        let e = json!({"code": -32000, "message": "Not logged in. Run codex login."});
        assert!(matches!(rpc_error(&e), DriverError::NeedsSignIn(_)));
        assert!(!thread_is_gone(&e));
    }
}
