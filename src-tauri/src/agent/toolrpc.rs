//! `RPC-1`: a snippet in the sandbox can call the run's own tools.
//!
//! The saving this exists for is context, not time. A job that is a fixed
//! sequence over forty items costs forty model turns today, each one carrying
//! the whole transcript with it. As a script it costs one turn and forty tool
//! calls, and the model never sees the forty intermediate results at all.
//!
//! The shape is deliberately the smallest one that keeps every promise the app
//! already makes:
//!
//! - **One loopback port, no tokens by default.** The listener is opened at
//!   startup like [`super::preview`]'s, but the token map is empty until a
//!   `run_code` call arms one. A request with an unknown token is refused
//!   before anything else happens, so an open port with nothing armed can do
//!   nothing at all.
//! - **A token lives exactly as long as one `run_code` call.** [`Ticket`]
//!   revokes on drop, so every exit path — success, error, timeout, a dropped
//!   future when the user pressed Stop — revokes it without remembering to.
//! - **The call goes back to the run that armed it.** The socket owns no
//!   ability of its own: it hands the call to the tool loop over a channel and
//!   waits for the answer. That is what makes permissions, folder trust,
//!   untrusted marking, the activity log and the headless refusal apply
//!   unchanged — it is the same `dispatch` the model's own calls go through,
//!   on the same run's state, not a second copy of it.
//!
//! Off by default (`tools.script_rpc`). The sandbox does not block outbound
//! network on Windows yet (see [`super::sandbox`]), and a script that can both
//! reach the network and call tools is a wider thing than one that can only do
//! the first.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use crate::db::Db;

/// Settings key for the switch in Settings → Tools. Absent means off.
pub const SETTING_KEY: &str = "tools.script_rpc";

/// Is a script allowed to call tools at all? Default **off**: this is the one
/// place in the app where code the model wrote reaches the tool layer without a
/// model turn in between, and that deserves a deliberate yes.
pub fn is_enabled(db: &Db) -> bool {
    matches!(
        db.get_setting(SETTING_KEY).ok().flatten().as_deref(),
        Some("true") | Some("1")
    )
}

/// Longest head we will read. A tool call is one line and two headers.
const MAX_HEAD: usize = 8 * 1024;
/// Longest body. Arguments are a small JSON object; a script wanting to hand
/// over a megabyte of content should write a file and pass the path.
const MAX_BODY: usize = 256 * 1024;
/// Longest a script may wait on one call before the socket gives up.
///
/// Generous on purpose: a tool call can sit on a permission panel while the
/// user decides. The real brake is the sandbox's own wall clock, which kills
/// the script — and with it this wait — long before this fires.
const CALL_TIMEOUT: Duration = Duration::from_secs(300);

/// One tool call a script asked for, on its way to the run that armed the token.
pub struct Call {
    /// The `run_code` step this came from, so the timeline can nest it there
    /// rather than showing it as a step the model asked for.
    pub parent: String,
    pub name: String,
    pub args: serde_json::Value,
    pub reply: oneshot::Sender<Result<String, String>>,
}

/// Where a served call is posted. Held by the tool loop for the length of one
/// batch of calls; cloned into the token map while a `run_code` call runs.
pub type Gate = mpsc::UnboundedSender<Call>;

/// `MCPS-1`: one `tools/call` an agent app made on the MCP endpoint, on its way
/// to the run that handed it the token.
pub struct McpCall {
    pub tool: String,
    pub args: serde_json::Value,
    /// Set when the app closes the connection before it has its answer, which is
    /// what Claude Code does when the user stops it mid-question. The run drops
    /// the question it put on the panel.
    pub gone: crate::runtime::proxy::CancelFlag,
    /// The text of the answer: for `approve`, the JSON verdict.
    pub reply: oneshot::Sender<String>,
}

/// Where an MCP call is posted. Held by the engine run that armed the token.
pub type McpGate = mpsc::UnboundedSender<McpCall>;

/// What a token opens. A script token cannot reach the MCP endpoint and the
/// other way round: the MCP token goes to an agent app, which must never be
/// able to call the run's own tools through the script path.
enum Target {
    Script(Gate),
    /// The kit, and the specs of any further tools this token was armed with
    /// (`CAP-14`: the connected-apps meta-tools). The specs are the whole list: a
    /// name on it is callable, nothing else is.
    Mcp(McpGate, McpKit, std::sync::Arc<Vec<serde_json::Value>>),
}

/// What an agent app's token opens on the MCP endpoint (`MCPS-1`, `MCPS-2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpKit {
    /// Claude Code's permission-prompt tool.
    pub approve: bool,
    /// `recall`, `present`, `ask_user` and `remember`: only when the user has
    /// switched on *Let it use my memory* for this agent.
    pub memory: bool,
}

struct Live {
    target: Target,
    parent: String,
}

/// The two paths a token can be used on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Tool,
    Mcp,
}

static BASE: OnceLock<String> = OnceLock::new();
static TOKENS: OnceLock<Mutex<HashMap<String, Live>>> = OnceLock::new();

fn tokens() -> &'static Mutex<HashMap<String, Live>> {
    TOKENS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `http://127.0.0.1:<port>`, or `None` if the loopback bind failed at startup.
pub fn base_url() -> Option<&'static str> {
    BASE.get().map(|s| s.as_str())
}

/// A live token, revoked when this is dropped.
///
/// The whole point of the type: `run_code` has half a dozen ways to end, and
/// the one that matters most (the future dropped because the user pressed Stop)
/// runs no code of its own. `Drop` reaches all of them.
pub struct Ticket {
    token: String,
}

impl Ticket {
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if let Ok(mut map) = tokens().lock() {
            map.remove(&self.token);
        }
    }
}

/// Mint a token for one `run_code` call. `None` when the server never bound, in
/// which case the snippet simply runs without the ability and is told so.
pub fn arm(gate: Gate, parent: &str) -> Option<Ticket> {
    base_url()?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let live = Live { target: Target::Script(gate), parent: parent.to_string() };
    tokens().lock().ok()?.insert(token.clone(), live);
    Some(Ticket { token })
}

/// `MCPS-1`: mint a token for one agent-app run. Armed by `run_external`,
/// independent of `tools.script_rpc`: that switch is about code the model wrote,
/// while this endpoint is reachable only by a process Poiesis started and handed
/// the token to. The token dies with the run.
pub fn arm_mcp(gate: McpGate, run_id: &str, kit: McpKit) -> Option<Ticket> {
    arm_mcp_with(gate, run_id, kit, Vec::new())
}

/// [`arm_mcp`] for a run that also offers `apps`: MCP tool specs served beside
/// the kit's own (`CAP-14`). Poiesis answers each call itself, so the Composio key
/// and session never reach the agent app.
pub fn arm_mcp_with(gate: McpGate, run_id: &str, kit: McpKit, apps: Vec<serde_json::Value>) -> Option<Ticket> {
    base_url()?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let live = Live { target: Target::Mcp(gate, kit, std::sync::Arc::new(apps)), parent: run_id.to_string() };
    tokens().lock().ok()?.insert(token.clone(), live);
    Some(Ticket { token })
}

/// The URL an agent app is told to connect to for a ticket from [`arm_mcp`].
pub fn mcp_url(ticket: &Ticket) -> Option<String> {
    Some(format!("{}/{}/mcp", base_url()?, ticket.token()))
}

// ---- the socket ----

/// `/<token>/tool` or `/<token>/mcp` → the token and which one. Anything else is
/// not one of ours.
fn route(target: &str) -> Option<(&str, Endpoint)> {
    let path = target.split(['?', '#']).next()?;
    let mut parts = path.trim_start_matches('/').split('/');
    let token = parts.next()?;
    let endpoint = match parts.next()? {
        "tool" => Endpoint::Tool,
        "mcp" => Endpoint::Mcp,
        _ => return None,
    };
    if parts.next().is_some() || token.is_empty() {
        return None;
    }
    Some((token, endpoint))
}

/// What a request head asked for.
#[derive(Debug, PartialEq, Eq)]
enum Head<'a> {
    /// A POST to one of our paths, with the body length it announced.
    Call { token: &'a str, endpoint: Endpoint, len: usize },
    /// A GET or DELETE on the MCP path. Answered 405, which tells an MCP client
    /// this server offers no event stream, so it does not wait for one.
    NotAllowed,
}

/// Read a request head. `None` means the request was not ours at all: wrong
/// method, wrong path, or a length we can't read.
fn parse_head(head: &str) -> Option<Head<'_>> {
    let mut lines = head.lines();
    let mut parts = lines.next()?.split_whitespace();
    let method = parts.next()?;
    let (token, endpoint) = route(parts.next()?)?;
    if method != "POST" {
        return (endpoint == Endpoint::Mcp && matches!(method, "GET" | "DELETE")).then_some(Head::NotAllowed);
    }
    let mut len = 0usize;
    for line in lines {
        let Some((key, value)) = line.split_once(':') else { continue };
        if key.eq_ignore_ascii_case("content-length") {
            len = value.trim().parse().ok()?;
        }
    }
    Some(Head::Call { token, endpoint, len })
}

/// The two fields a call carries. Arguments default to `{}` so `tool("x")` with
/// no arguments is a call, not an error.
fn parse_body(body: &str) -> Result<(String, serde_json::Value), String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("the request body was not JSON: {e}"))?;
    let name = value
        .get("name")
        .and_then(|n| n.as_str())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| "no tool name was given".to_string())?
        .to_string();
    let args = value
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Ok((name, args))
}

/// Every answer is JSON with an `ok` flag, so a client library can raise on a
/// refusal without parsing prose or reading the status line.
fn reply(status: &str, body: &serde_json::Value) -> Vec<u8> {
    let text = body.to_string();
    let head = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n",
        text.len()
    );
    let mut out = head.into_bytes();
    out.extend_from_slice(text.as_bytes());
    out
}

fn refused(status: &str, why: &str) -> Vec<u8> {
    reply(status, &serde_json::json!({ "ok": false, "error": why }))
}

/// End of the request head, as an index into `buf`.
fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|at| at + 4)
}

/// Hand one parsed call to the run that armed the token and wait for its
/// answer. Split out from the socket so the refusal paths are testable without
/// one.
async fn forward(token: &str, name: String, args: serde_json::Value) -> Vec<u8> {
    let (tx, rx) = oneshot::channel();
    // The lock is released before the await: a tool call can take minutes, and
    // holding the token map across it would stall every other script.
    let posted = match tokens().lock() {
        Ok(map) => match map.get(token) {
            Some(Live { target: Target::Script(gate), parent }) => gate
                .send(Call { parent: parent.clone(), name, args, reply: tx })
                .is_ok(),
            // An agent app's token opens the MCP endpoint and nothing else.
            Some(_) => return refused("403 Forbidden", "this token is not for tool calls"),
            None => return refused("403 Forbidden", "this token is not live any more"),
        },
        Err(_) => false,
    };
    if !posted {
        return refused("410 Gone", "the run that could answer this has ended");
    }
    match tokio::time::timeout(CALL_TIMEOUT, rx).await {
        Ok(Ok(Ok(output))) => reply("200 OK", &serde_json::json!({ "ok": true, "result": output })),
        Ok(Ok(Err(e))) => reply("200 OK", &serde_json::json!({ "ok": false, "error": e })),
        Ok(Err(_)) => refused("410 Gone", "the run that could answer this has ended"),
        Err(_) => refused("504 Gateway Timeout", "the tool call took too long"),
    }
}

/// Serve one connection: read the head, read the body, answer, close.
async fn serve(mut stream: tokio::net::TcpStream) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let at = loop {
        match stream.read(&mut chunk).await {
            Ok(0) => return,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(at) = head_end(&buf) {
                    break at;
                }
                if buf.len() > MAX_HEAD {
                    return;
                }
            }
            Err(_) => return,
        }
    };

    let parsed = std::str::from_utf8(&buf[..at]).ok().and_then(parse_head);
    let (token, endpoint, len) = match parsed {
        Some(Head::Call { token, endpoint, len }) => (token.to_string(), endpoint, len),
        Some(Head::NotAllowed) => {
            let _ = stream.write_all(&refused("405 Method Not Allowed", "this server answers POST only")).await;
            return;
        }
        None => {
            let _ = stream.write_all(&refused("404 Not Found", "not a tool call")).await;
            return;
        }
    };
    // `MCPS-1`: a token that is not live, or belongs to the other path, gets a
    // 404 before a byte of the body is read or parsed.
    if endpoint == Endpoint::Mcp && !mcp_token_live(&token) {
        let _ = stream.write_all(&refused("404 Not Found", "not found")).await;
        return;
    }
    if len > MAX_BODY {
        let _ = stream
            .write_all(&refused("413 Payload Too Large", "the arguments were too big to send"))
            .await;
        return;
    }

    while buf.len() < at + len {
        match stream.read(&mut chunk).await {
            Ok(0) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => return,
        }
    }
    // Exactly the announced length: anything the client sent past it is not
    // part of this call and is not ours to interpret.
    let body = String::from_utf8_lossy(&buf[at..at + len]).into_owned();

    if endpoint == Endpoint::Mcp {
        serve_mcp(&mut stream, &token, &body).await;
        return;
    }
    let out = match parse_body(&body) {
        Ok((name, args)) => forward(&token, name, args).await,
        Err(e) => refused("400 Bad Request", &e),
    };
    let _ = stream.write_all(&out).await;
    let _ = stream.flush().await;
}

// ---- `MCPS-1`: the MCP endpoint ----

/// The longest an agent app's question may sit unanswered before the endpoint
/// gives up on it. The run itself decides sooner (a person has a few minutes);
/// this only keeps a lost run from holding a connection open forever.
const MCP_CALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

fn mcp_token_live(token: &str) -> bool {
    tokens().lock().is_ok_and(|m| matches!(m.get(token), Some(Live { target: Target::Mcp(..), .. })))
}

/// A JSON-RPC answer, as an HTTP response.
fn rpc_result(id: &serde_json::Value, result: serde_json::Value) -> Vec<u8> {
    reply("200 OK", &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn rpc_error(id: &serde_json::Value, code: i64, message: &str) -> Vec<u8> {
    reply("200 OK", &serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }))
}

/// `202 Accepted`, no body: the answer to a notification.
fn accepted() -> Vec<u8> {
    b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n".to_vec()
}

/// What `tools/list` says for this token: `approve` (Claude Code's
/// permission-prompt tool, which it calls before it uses a tool that needs
/// permission) and, when the user allowed it, the four memory tools.
fn mcp_tools(kit: McpKit, apps: &[serde_json::Value]) -> serde_json::Value {
    let mut tools: Vec<serde_json::Value> = Vec::new();
    if kit.approve {
        tools.push(serde_json::json!({
            "name": "approve",
            "description": "Ask the user to approve one tool call.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tool_name": { "type": "string" },
                    "input": { "type": "object" },
                    "tool_use_id": { "type": "string" }
                },
                "required": ["tool_name", "input"]
            }
        }));
    }
    if kit.memory {
        if let Some(list) = crate::engines::tools::specs().as_array() {
            tools.extend(list.iter().cloned());
        }
    }
    tools.extend(apps.iter().cloned());
    serde_json::json!({ "tools": tools })
}

fn kit_of(token: &str) -> Option<(McpKit, std::sync::Arc<Vec<serde_json::Value>>)> {
    match tokens().lock().ok()?.get(token) {
        Some(Live { target: Target::Mcp(_, kit, apps), .. }) => Some((*kit, apps.clone())),
        _ => None,
    }
}

/// Is this tool one the token may call?
fn kit_allows(kit: McpKit, apps: &[serde_json::Value], tool: &str) -> bool {
    (tool == "approve" && kit.approve)
        || (kit.memory && crate::engines::tools::NAMES.contains(&tool))
        || apps.iter().any(|t| t.get("name").and_then(|n| n.as_str()) == Some(tool))
}

/// The protocol version to answer with. Claude Code first asks with a newer one
/// we do not know and falls back; whatever it offers is echoed, so the two never
/// disagree. A value that is not a short plain string is replaced.
fn echo_version(params: &serde_json::Value) -> String {
    params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty() && v.len() <= 32 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or("2025-06-18")
        .to_string()
}

/// Answer one JSON-RPC message from an agent app. Replies as `application/json`
/// only; one-shot calls need no event stream.
async fn serve_mcp(stream: &mut tokio::net::TcpStream, token: &str, body: &str) {
    let Ok(msg) = serde_json::from_str::<serde_json::Value>(body) else {
        let _ = stream.write_all(&refused("400 Bad Request", "the body was not JSON")).await;
        return;
    };
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);

    // A message with no id is a notification (`notifications/initialized`,
    // `notifications/cancelled`): nothing to answer.
    let Some(id) = id else {
        let _ = stream.write_all(&accepted()).await;
        return;
    };

    let out = match method {
        "initialize" => rpc_result(
            &id,
            serde_json::json!({
                "protocolVersion": echo_version(&params),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "poiesis", "version": env!("CARGO_PKG_VERSION") }
            }),
        ),
        "ping" => rpc_result(&id, serde_json::json!({})),
        "tools/list" => {
            let (kit, apps) = kit_of(token).unwrap_or((McpKit { approve: false, memory: false }, Default::default()));
            rpc_result(&id, mcp_tools(kit, &apps))
        }
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if !kit_of(token).is_some_and(|(kit, apps)| kit_allows(kit, &apps, name)) {
                rpc_error(&id, -32602, "there is no such tool")
            } else {
                let args = params.get("arguments").cloned().unwrap_or_else(|| serde_json::json!({}));
                match forward_mcp(stream, token, name, args).await {
                    // The app closed the connection: nobody is left to read an answer.
                    Forwarded::Gone => return,
                    Forwarded::Text(text) => rpc_result(&id, serde_json::json!({ "content": [{ "type": "text", "text": text }] })),
                    Forwarded::Refused(why) => rpc_error(&id, -32000, why),
                }
            }
        }
        // `server/discover` (a newer protocol's first question) and anything else:
        // "method not found" makes the client fall back to `initialize`.
        _ => rpc_error(&id, -32601, "method not found"),
    };
    let _ = stream.write_all(&out).await;
    let _ = stream.flush().await;
}

enum Forwarded {
    Text(String),
    Refused(&'static str),
    Gone,
}

/// Hand a `tools/call` to the run that armed the token and wait for its answer
/// while watching the socket: if the app hangs up first, the call is marked gone
/// so the run withdraws what it put in front of the user.
async fn forward_mcp(stream: &mut tokio::net::TcpStream, token: &str, tool: &str, args: serde_json::Value) -> Forwarded {
    let (tx, mut rx) = oneshot::channel();
    let gone = crate::runtime::proxy::CancelFlag::new();
    let posted = match tokens().lock() {
        Ok(map) => match map.get(token) {
            Some(Live { target: Target::Mcp(gate, _, _), .. }) => {
                gate.send(McpCall { tool: tool.to_string(), args, gone: gone.clone(), reply: tx }).is_ok()
            }
            _ => return Forwarded::Refused("this token is not live any more"),
        },
        Err(_) => false,
    };
    if !posted {
        return Forwarded::Refused("the run that could answer this has ended");
    }
    let deadline = tokio::time::sleep(MCP_CALL_TIMEOUT);
    tokio::pin!(deadline);
    let mut scratch = [0u8; 256];
    loop {
        tokio::select! {
            answer = &mut rx => {
                return match answer {
                    Ok(text) => Forwarded::Text(text),
                    Err(_) => Forwarded::Refused("the run that could answer this has ended"),
                };
            }
            // A client that sent exactly one request has nothing more to say, so
            // any end of the stream means it hung up.
            read = stream.read(&mut scratch) => {
                if matches!(read, Ok(0) | Err(_)) {
                    gone.cancel();
                    return Forwarded::Gone;
                }
            }
            _ = &mut deadline => {
                gone.cancel();
                return Forwarded::Refused("nobody answered in time");
            }
        }
    }
}

/// Start the tool RPC server. Bound at startup so the port is known, but it can
/// answer nothing until a `run_code` call arms a token. A bind failure is
/// survivable: [`arm`] then returns `None` and snippets run without the ability.
pub fn start() {
    let Ok(listener) = tauri::async_runtime::block_on(async { TcpListener::bind(("127.0.0.1", 0)).await })
    else {
        eprintln!("tool rpc: could not bind loopback; scripts will not be able to call tools");
        return;
    };
    let Ok(addr) = listener.local_addr() else { return };
    let _ = BASE.set(format!("http://127.0.0.1:{}", addr.port()));
    tauri::async_runtime::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            tauri::async_runtime::spawn(serve(stream));
        }
    });
}

/// For tests that need a live endpoint (the agent-app end-to-end tests): a real
/// listener on its own thread and runtime, started once per test process. The
/// unit tests here use it too, so none of them leaves a dead address in `BASE`.
#[cfg(test)]
pub fn serve_for_tests() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let _ = BASE.set(format!("http://127.0.0.1:{port}"));
                tx.send(()).unwrap();
                loop {
                    let Ok((stream, _)) = listener.accept().await else { continue };
                    tokio::spawn(serve(stream));
                }
            });
        });
        rx.recv().unwrap();
    });
}

// ---- `RPC-2`: what the snippet imports ----

/// The Python side. Standard library only, no install, and it fails loudly
/// rather than returning `None` — a script that silently skipped every tool
/// call would produce a confident, empty answer.
pub const PY_CLIENT: &str = r#"
"""Call the agent's own tools from inside the sandbox."""
import json as _json
import os as _os
import urllib.error as _urlerror
import urllib.request as _urlrequest

_URL = _os.environ.get("POIESIS_TOOL_URL")
_TOKEN = _os.environ.get("POIESIS_RUN_TOKEN")


class ToolError(RuntimeError):
    """A tool refused the call, or could not be reached."""


def available():
    """True when this snippet may call tools."""
    return bool(_URL and _TOKEN)


def tool(name, **arguments):
    """Call one of the agent's tools and return its output as text.

    Raises ToolError if the tool refused. Permissions still apply: a call that
    needs the user's say-so waits here until they answer it.
    """
    if not available():
        raise ToolError(
            "This snippet can't call tools. Turn on 'Let a script call my tools' "
            "in Settings > Tools, or do the work with a tool call of your own."
        )
    payload = _json.dumps({"name": name, "arguments": arguments}).encode("utf-8")
    request = _urlrequest.Request(
        "%s/%s/tool" % (_URL, _TOKEN),
        data=payload,
        headers={"Content-Type": "application/json"},
    )
    try:
        with _urlrequest.urlopen(request) as response:
            body = _json.loads(response.read().decode("utf-8"))
    except _urlerror.HTTPError as e:
        try:
            body = _json.loads(e.read().decode("utf-8"))
        except Exception:
            raise ToolError("the tool call failed: HTTP %s" % e.code)
    except Exception as e:
        raise ToolError("could not reach the tool server: %s" % e)
    if not body.get("ok"):
        raise ToolError(body.get("error") or "the tool call failed")
    return body.get("result", "")
"#;

/// The Node side. `fetch` is built in from Node 18, which is older than any
/// runtime that would be installed today, so this needs nothing either.
pub const JS_CLIENT: &str = r#"// Call the agent's own tools from inside the sandbox.
const URL_BASE = process.env.POIESIS_TOOL_URL;
const TOKEN = process.env.POIESIS_RUN_TOKEN;

class ToolError extends Error {}

function available() {
  return Boolean(URL_BASE && TOKEN);
}

// Returns the tool's output as text. Throws ToolError if it refused.
// Permissions still apply: a call that needs the user's say-so waits here
// until they answer it.
async function tool(name, args = {}) {
  if (!available()) {
    throw new ToolError(
      "This snippet can't call tools. Turn on 'Let a script call my tools' " +
        "in Settings > Tools, or do the work with a tool call of your own."
    );
  }
  let body;
  try {
    const response = await fetch(`${URL_BASE}/${TOKEN}/tool`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, arguments: args }),
    });
    body = await response.json();
  } catch (e) {
    throw new ToolError(`could not reach the tool server: ${e.message}`);
  }
  if (!body.ok) throw new ToolError(body.error || "the tool call failed");
  return body.result ?? "";
}

module.exports = { tool, available, ToolError };
"#;

/// `RPC-3`: appended to `run_code`'s description, and only while the ability is
/// actually on. A model will not discover this on its own, and teaching it a
/// capability that is switched off costs a wasted step to find that out.
pub const TOOL_GUIDANCE: &str = " You can also call my own tools from inside the snippet, which is the right shape when a job is the same fixed sequence over many items: one snippet that loops costs one turn, where one tool call per item costs a turn each and fills my context with results I don't need to read. In Python: `from poiesis import tool` then `text = tool(\"web_search\", query=q)`. In Node: `const { tool } = require(\"./poiesis\")` then `const text = await tool(\"web_search\", { query: q })`. It returns the tool's output as a string and raises if the tool refused. Every tool I have is reachable except run_code itself, each call still asks the user for anything that needs asking, and each one shows up in the timeline under this step. Use it for the loop, not for a single call — one call is cheaper made directly.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_post_to_the_tool_path_is_a_call() {
        let head = |line: &str| format!("{line}\r\nContent-Length: 2\r\n\r\n");
        assert_eq!(
            parse_head(&head("POST /tok/tool HTTP/1.1")),
            Some(Head::Call { token: "tok", endpoint: Endpoint::Tool, len: 2 })
        );
        assert_eq!(parse_head(&head("GET /tok/tool HTTP/1.1")), None, "reads are not calls");
        assert_eq!(parse_head(&head("POST /tok HTTP/1.1")), None);
        assert_eq!(parse_head(&head("POST /tok/tool/extra HTTP/1.1")), None);
        assert_eq!(parse_head(&head("POST / HTTP/1.1")), None);
    }

    /// The preview server shares this machine's loopback interface. A request
    /// shaped for that one must not be answered here, and vice versa.
    #[test]
    fn an_artifact_request_is_not_a_tool_call() {
        assert_eq!(route("/tok/art-1"), None);
        assert_eq!(route("/tok/tool"), Some(("tok", Endpoint::Tool)));
        assert_eq!(route("/tok/tool?x=1"), Some(("tok", Endpoint::Tool)));
        assert_eq!(route("/tok/mcp"), Some(("tok", Endpoint::Mcp)));
        assert_eq!(route("/tok/mcp/extra"), None);
    }

    #[test]
    fn a_missing_content_length_reads_as_an_empty_body() {
        assert_eq!(
            parse_head("POST /tok/tool HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some(Head::Call { token: "tok", endpoint: Endpoint::Tool, len: 0 })
        );
    }

    #[test]
    fn only_the_mcp_path_answers_a_get_and_it_says_no() {
        assert_eq!(parse_head("GET /tok/mcp HTTP/1.1\r\n\r\n"), Some(Head::NotAllowed));
        assert_eq!(parse_head("DELETE /tok/mcp HTTP/1.1\r\n\r\n"), Some(Head::NotAllowed));
        assert_eq!(parse_head("GET /tok/tool HTTP/1.1\r\n\r\n"), None);
    }

    // ---- `MCPS-1`, over a real loopback connection ----

    /// One HTTP exchange against a live `serve`: send a POST, read to the end.
    async fn exchange(path: &str, body: &str) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            serve(s).await;
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!("POST {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        c.write_all(req.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), c.read_to_end(&mut out)).await;
        String::from_utf8_lossy(&out).into_owned()
    }

    fn json_body(http: &str) -> serde_json::Value {
        serde_json::from_str(http.split("\r\n\r\n").nth(1).unwrap_or("null")).unwrap_or(serde_json::Value::Null)
    }

    fn armed() -> (Ticket, mpsc::UnboundedReceiver<McpCall>) {
        serve_for_tests();
        let (tx, rx) = mpsc::unbounded_channel();
        (arm_mcp(tx, "run-1", McpKit { approve: true, memory: false }).expect("a bound server arms"), rx)
    }

    #[tokio::test]
    async fn a_revoked_or_unknown_mcp_token_gets_a_404_before_anything_is_parsed() {
        let out = exchange("/nosuchtoken/mcp", "this is not even json").await;
        assert!(out.starts_with("HTTP/1.1 404"), "{out}");
        let (ticket, _rx) = armed();
        let path = format!("/{}/mcp", ticket.token());
        let live = exchange(&path, r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).await;
        assert!(live.starts_with("HTTP/1.1 200"), "{live}");
        drop(ticket);
        let after = exchange(&path, r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).await;
        assert!(after.starts_with("HTTP/1.1 404"), "a returned run leaves no usable token: {after}");
    }

    #[tokio::test]
    async fn the_two_kinds_of_token_do_not_open_each_others_path() {
        let (mcp, _rx) = armed();
        let (tx, _srx) = mpsc::unbounded_channel();
        let script = arm(tx, "call-1").unwrap();
        // An agent app's token cannot call the run's tools.
        let tool = exchange(&format!("/{}/tool", mcp.token()), r#"{"name":"read_file"}"#).await;
        assert!(tool.starts_with("HTTP/1.1 403") && tool.contains("not for tool calls"), "{tool}");
        // A script's token does not open the MCP endpoint.
        let other = exchange(&format!("/{}/mcp", script.token()), r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).await;
        assert!(other.starts_with("HTTP/1.1 404"), "{other}");
    }

    /// The handshake Claude Code does: a newer `server/discover` is refused so it
    /// falls back, `initialize` echoes its version, `initialized` is a 202, and
    /// `tools/list` offers `approve`.
    #[tokio::test]
    async fn the_handshake_claude_code_does_works() {
        let (ticket, _rx) = armed();
        let path = format!("/{}/mcp", ticket.token());

        let discover = exchange(&path, r#"{"jsonrpc":"2.0","id":"d","method":"server/discover","params":{}}"#).await;
        assert_eq!(json_body(&discover)["error"]["code"], -32601);

        let init = exchange(&path, r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#).await;
        let init = json_body(&init);
        assert_eq!(init["result"]["protocolVersion"], "2025-11-25", "the client's version is echoed");
        assert_eq!(init["result"]["serverInfo"]["name"], "poiesis");
        let odd = exchange(&path, r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"<script>"}}"#).await;
        assert_eq!(json_body(&odd)["result"]["protocolVersion"], "2025-06-18", "nothing odd is echoed");

        let note = exchange(&path, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).await;
        assert!(note.starts_with("HTTP/1.1 202") && note.ends_with("\r\n\r\n"), "{note}");

        let list = json_body(&exchange(&path, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).await);
        assert_eq!(list["result"]["tools"][0]["name"], "approve");
    }

    /// `MCPS-T`: an allow and a deny each come back as the JSON Claude Code
    /// reads, as one text content.
    #[tokio::test]
    async fn an_approval_goes_to_the_run_and_its_answer_comes_back_as_text() {
        let (ticket, mut rx) = armed();
        let path = format!("/{}/mcp", ticket.token());
        let run = tokio::spawn(async move {
            for verdict in [r#"{"behavior":"allow","updatedInput":{"command":"ls"}}"#, r#"{"behavior":"deny","message":"Not allowed: the user declined."}"#] {
                let call = rx.recv().await.expect("the call reaches the run that armed the token");
                assert_eq!(call.tool, "approve");
                assert_eq!(call.args["tool_name"], "Bash");
                let _ = call.reply.send(verdict.to_string());
            }
        });
        let body = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"approve","arguments":{"tool_name":"Bash","input":{"command":"ls"},"tool_use_id":"t1"}}}"#;
        let first = json_body(&exchange(&path, body).await);
        let text = first["result"]["content"][0]["text"].as_str().unwrap().to_string();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap()["behavior"], "allow");
        let second = json_body(&exchange(&path, body).await);
        let text = second["result"]["content"][0]["text"].as_str().unwrap().to_string();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap()["message"], "Not allowed: the user declined.");
        run.await.unwrap();

        // Only `approve` exists.
        let other = exchange(&path, r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"read_file","arguments":{}}}"#).await;
        assert_eq!(json_body(&other)["error"]["code"], -32602);
    }

    /// `MCPS-2`: which tools a token opens is decided when it is armed. The memory
    /// tools need the user's switch; `approve` is for agents that ask through it.
    #[tokio::test]
    async fn the_memory_tools_are_listed_and_callable_only_for_a_token_armed_with_them() {
        serve_for_tests();
        let names = |list: &serde_json::Value| -> Vec<String> {
            list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
        };
        let (tx, _plain_rx) = mpsc::unbounded_channel();
        let plain = arm_mcp(tx, "r1", McpKit { approve: true, memory: false }).unwrap();
        let (tx, mut rich_rx) = mpsc::unbounded_channel();
        let rich = arm_mcp(tx, "r2", McpKit { approve: false, memory: true }).unwrap();
        let plain_path = format!("/{}/mcp", plain.token());
        let rich_path = format!("/{}/mcp", rich.token());

        let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        assert_eq!(names(&json_body(&exchange(&plain_path, list).await)), ["approve"]);
        assert_eq!(names(&json_body(&exchange(&rich_path, list).await)), ["recall", "present", "ask_user", "remember"]);

        let call = |name: &str| format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{name}","arguments":{{"query":"x"}}}}}}"#);
        // A token without the switch cannot call them, and one with it cannot call `approve`.
        assert_eq!(json_body(&exchange(&plain_path, &call("recall")).await)["error"]["code"], -32602);
        assert_eq!(json_body(&exchange(&rich_path, &call("approve")).await)["error"]["code"], -32602);

        // With the switch the call reaches the run that armed the token.
        let run = tokio::spawn(async move {
            let c = rich_rx.recv().await.expect("the call reaches the run");
            assert_eq!((c.tool.as_str(), c.args["query"].as_str()), ("recall", Some("x")));
            let _ = c.reply.send("found it".to_string());
        });
        let answer = json_body(&exchange(&rich_path, &call("recall")).await);
        assert_eq!(answer["result"]["content"][0]["text"], "found it");
        run.await.unwrap();
    }

    /// `CAP-14`: the connected-apps tools are on a token only when it was armed with
    /// them, and a name that is not on its list is not callable.
    #[tokio::test]
    async fn a_token_armed_with_app_tools_lists_and_allows_only_those() {
        serve_for_tests();
        let spec = |name: &str| serde_json::json!({ "name": name, "description": "d", "inputSchema": { "type": "object" } });
        let (tx, mut rx) = mpsc::unbounded_channel();
        let kit = McpKit { approve: false, memory: false };
        let armed = arm_mcp_with(tx, "r1", kit, vec![spec("COMPOSIO_SEARCH_TOOLS")]).unwrap();
        let path = format!("/{}/mcp", armed.token());
        let list = json_body(&exchange(&path, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).await);
        let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["COMPOSIO_SEARCH_TOOLS"]);

        let call = |name: &str| format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{name}","arguments":{{}}}}}}"#);
        assert_eq!(json_body(&exchange(&path, &call("COMPOSIO_MULTI_EXECUTE_TOOL")).await)["error"]["code"], -32602);
        assert_eq!(json_body(&exchange(&path, &call("recall")).await)["error"]["code"], -32602);
        let run = tokio::spawn(async move {
            let c = rx.recv().await.expect("the call reaches the run");
            assert_eq!(c.tool, "COMPOSIO_SEARCH_TOOLS");
            let _ = c.reply.send("found".to_string());
        });
        let answer = json_body(&exchange(&path, &call("COMPOSIO_SEARCH_TOOLS")).await);
        assert_eq!(answer["result"]["content"][0]["text"], "found");
        run.await.unwrap();
    }

    /// Finding 11: Claude Code closes the connection when the user stops it
    /// mid-question. The run is told, so it can take the question off the panel.
    #[tokio::test]
    async fn a_hung_up_connection_marks_the_call_gone() {
        let (ticket, mut rx) = armed();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            serve(s).await;
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        let body = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"approve","arguments":{"tool_name":"Bash","input":{}}}}"#;
        let req = format!("POST /{}/mcp HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}", ticket.token(), body.len());
        c.write_all(req.as_bytes()).await.unwrap();
        let call = rx.recv().await.unwrap();
        assert!(!call.gone.is_cancelled());
        drop(c);
        for _ in 0..50 {
            if call.gone.is_cancelled() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the endpoint never noticed the connection was closed");
    }

    #[test]
    fn a_call_needs_a_name_and_may_omit_its_arguments() {
        let (name, args) = parse_body(r#"{"name":"web_search"}"#).unwrap();
        assert_eq!(name, "web_search");
        assert_eq!(args, serde_json::json!({}));

        let (name, args) = parse_body(r#"{"name":"read_file","arguments":{"path":"a.txt"}}"#).unwrap();
        assert_eq!(name, "read_file");
        assert_eq!(args["path"], "a.txt");

        assert!(parse_body(r#"{"arguments":{}}"#).is_err());
        assert!(parse_body(r#"{"name":""}"#).is_err());
        assert!(parse_body("not json").is_err());
    }

    /// `RPC-T1`: a token that was never armed, or has been revoked, gets
    /// nothing. This is the only thing standing between an open loopback port
    /// and every tool the user has granted.
    #[tokio::test]
    async fn a_token_that_was_never_armed_is_refused() {
        let out = String::from_utf8(forward("nosuchtoken", "read_file".into(), serde_json::json!({})).await).unwrap();
        assert!(out.starts_with("HTTP/1.1 403"), "got: {out}");
        assert!(out.contains("not live"));
    }

    /// The ticket is what makes "revoked when the call returns" true on every
    /// exit path, including the ones that run no code.
    #[tokio::test]
    async fn dropping_the_ticket_revokes_the_token() {
        serve_for_tests();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let ticket = arm(tx, "call-1").expect("a bound server arms");
        let token = ticket.token().to_string();

        // The run's side of the channel, answering the way the tool loop does.
        let loop_side = tokio::spawn(async move {
            let call = rx.recv().await.expect("the call reaches the run that armed it");
            assert_eq!(call.parent, "call-1", "the timeline needs the step this hangs under");
            assert_eq!(call.name, "read_file");
            let _ = call.reply.send(Ok("two lines".into()));
        });
        let answered =
            String::from_utf8(forward(&token, "read_file".into(), serde_json::json!({})).await)
                .unwrap();
        loop_side.await.unwrap();
        assert!(answered.contains(r#""ok":true"#));
        assert!(answered.contains("two lines"));

        drop(ticket);
        let after = String::from_utf8(forward(&token, "read_file".into(), serde_json::json!({})).await).unwrap();
        assert!(after.starts_with("HTTP/1.1 403"), "a returned call leaves no usable token");
    }

    /// A refusal has to arrive as data, not as prose: the client libraries
    /// raise on `ok: false` without reading the status line.
    #[test]
    fn every_answer_says_whether_it_worked() {
        let ok = String::from_utf8(reply("200 OK", &serde_json::json!({"ok": true, "result": "x"}))).unwrap();
        assert!(ok.contains("Content-Type: application/json"));
        assert!(ok.ends_with(r#"{"ok":true,"result":"x"}"#));
        let no = String::from_utf8(refused("403 Forbidden", "nope")).unwrap();
        assert!(no.starts_with("HTTP/1.1 403"));
        assert!(no.contains(r#""ok":false"#));
    }

    #[test]
    fn the_head_ends_at_the_blank_line() {
        assert_eq!(head_end(b"POST / HTTP/1.1\r\n\r\nbody"), Some(19));
        assert_eq!(head_end(b"POST / HTTP/1.1\r\n"), None);
    }

    /// Both clients must fail loudly when the ability is off. A script that
    /// silently skipped its tool calls would produce a confident empty answer,
    /// which is worse than an error the model can read and route around.
    #[test]
    fn both_client_libraries_refuse_rather_than_return_nothing() {
        for source in [PY_CLIENT, JS_CLIENT] {
            assert!(source.contains("POIESIS_TOOL_URL"));
            assert!(source.contains("POIESIS_RUN_TOKEN"));
            assert!(source.contains("ToolError"));
            assert!(source.contains("Settings > Tools"));
        }
    }
}
