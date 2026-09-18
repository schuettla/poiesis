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

struct Live {
    gate: Gate,
    parent: String,
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
    let live = Live { gate, parent: parent.to_string() };
    tokens().lock().ok()?.insert(token.clone(), live);
    Some(Ticket { token })
}

// ---- the socket ----

/// `/<token>/tool` → the token. Anything else is not one of ours.
fn route(target: &str) -> Option<&str> {
    let path = target.split(['?', '#']).next()?;
    let mut parts = path.trim_start_matches('/').split('/');
    let token = parts.next()?;
    if parts.next()? != "tool" || parts.next().is_some() || token.is_empty() {
        return None;
    }
    Some(token)
}

/// `(token, body length)` from a request head. `None` means the request was not
/// a tool call at all — wrong method, wrong path, or a length we can't read.
fn parse_head(head: &str) -> Option<(&str, usize)> {
    let mut lines = head.lines();
    let mut parts = lines.next()?.split_whitespace();
    if parts.next()? != "POST" {
        return None;
    }
    let token = route(parts.next()?)?;
    let mut len = 0usize;
    for line in lines {
        let Some((key, value)) = line.split_once(':') else { continue };
        if key.eq_ignore_ascii_case("content-length") {
            len = value.trim().parse().ok()?;
        }
    }
    Some((token, len))
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
            Some(live) => live
                .gate
                .send(Call { parent: live.parent.clone(), name, args, reply: tx })
                .is_ok(),
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
    let Some((token, len)) = parsed.map(|(t, l)| (t.to_string(), l)) else {
        let _ = stream.write_all(&refused("404 Not Found", "not a tool call")).await;
        return;
    };
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

    let out = match parse_body(&body) {
        Ok((name, args)) => forward(&token, name, args).await,
        Err(e) => refused("400 Bad Request", &e),
    };
    let _ = stream.write_all(&out).await;
    let _ = stream.flush().await;
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
        assert_eq!(parse_head(&head("POST /tok/tool HTTP/1.1")), Some(("tok", 2)));
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
        assert_eq!(route("/tok/tool"), Some("tok"));
        assert_eq!(route("/tok/tool?x=1"), Some("tok"));
    }

    #[test]
    fn a_missing_content_length_reads_as_an_empty_body() {
        assert_eq!(parse_head("POST /tok/tool HTTP/1.1\r\nHost: x\r\n\r\n"), Some(("tok", 0)));
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
        let _ = BASE.set("http://127.0.0.1:1".to_string());
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
