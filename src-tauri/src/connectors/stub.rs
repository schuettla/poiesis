//! Test-only stand-in for Composio Connect's MCP endpoint (`CAP-C-T`).
//!
//! It speaks just enough Streamable HTTP JSON-RPC for the client to handshake,
//! list tools and call them, answers each call from a closure the test gives it,
//! and records what it was sent: the calls, and the header the key travelled in.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// What the stub says to one request.
pub enum Reply {
    /// The text of a tool result.
    Text(String),
    /// An HTTP status instead of a JSON-RPC answer (`401`, `500`...).
    Http(u16),
    /// The stub's ordinary answer to this method.
    Default,
}

pub struct ConnectStub {
    /// The address to hand to `tests_pretend::connect_url`.
    pub url: String,
    /// `(tool, arguments)` of every `tools/call`.
    pub calls: Arc<Mutex<Vec<(String, Value)>>>,
    /// The value of `x-consumer-api-key` on every request that had one.
    pub keys: Arc<Mutex<Vec<String>>>,
    /// Every request path, so a test can say no REST call was made.
    pub paths: Arc<Mutex<Vec<String>>>,
    /// The tools `tools/list` names.
    pub tools: Arc<Mutex<Vec<String>>>,
    /// When set, every request is answered with this HTTP status.
    pub refuse: Arc<Mutex<Option<u16>>>,
}

pub const ALL_TOOLS: [&str; 7] = [
    "COMPOSIO_SEARCH_TOOLS",
    "COMPOSIO_GET_TOOL_SCHEMAS",
    "COMPOSIO_MULTI_EXECUTE_TOOL",
    "COMPOSIO_MANAGE_CONNECTIONS",
    "COMPOSIO_WAIT_FOR_CONNECTIONS",
    "COMPOSIO_REMOTE_WORKBENCH",
    "COMPOSIO_REMOTE_BASH_TOOL",
];

/// Start the stub. `answer(tool, arguments)` is asked for every `tools/call`;
/// everything else (the handshake, the tool list) the stub answers itself, unless
/// `refuse` is set, which makes every request fail with that status.
pub async fn connect_stub(answer: impl Fn(&str, &Value) -> Reply + Send + Sync + 'static) -> ConnectStub {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let keys = Arc::new(Mutex::new(Vec::new()));
    let paths = Arc::new(Mutex::new(Vec::new()));
    let tools = Arc::new(Mutex::new(ALL_TOOLS.iter().map(|t| t.to_string()).collect::<Vec<_>>()));
    let refuse = Arc::new(Mutex::new(None::<u16>));
    let answer = Arc::new(answer);
    let (c2, k2, p2, t2, r2) = (calls.clone(), keys.clone(), paths.clone(), tools.clone(), refuse.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let (calls, keys, paths, tools, answer, refuse) =
                (c2.clone(), k2.clone(), p2.clone(), t2.clone(), answer.clone(), r2.clone());
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                let (head_end, len, head) = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).to_string();
                        let len = head
                            .to_lowercase()
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        break (p + 4, len, head);
                    }
                };
                while buf.len() < head_end + len {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                if let Some(path) = head.lines().next().and_then(|l| l.split_whitespace().nth(1)) {
                    paths.lock().unwrap().push(path.to_string());
                }
                if let Some(key) = head.lines().find_map(|l| {
                    l.to_lowercase()
                        .starts_with("x-consumer-api-key:")
                        .then(|| l[l.find(':').unwrap() + 1..].trim().to_string())
                }) {
                    keys.lock().unwrap().push(key);
                }
                let req: Value = serde_json::from_slice(&buf[head_end..head_end + len]).unwrap_or_default();
                let id = req["id"].clone();
                let method = req["method"].as_str().unwrap_or("").to_string();
                let (tool, args) = if method == "tools/call" {
                    let tool = req["params"]["name"].as_str().unwrap_or("").to_string();
                    let args = req["params"]["arguments"].clone();
                    calls.lock().unwrap().push((tool.clone(), args.clone()));
                    (tool, args)
                } else {
                    (String::new(), Value::Null)
                };
                let reply = match *refuse.lock().unwrap() {
                    Some(code) => Reply::Http(code),
                    None if method == "tools/call" => answer(&tool, &args),
                    None => Reply::Default,
                };
                let (status, body) = match (reply, method.as_str()) {
                    (Reply::Http(code), _) => (format!("{code} Stub"), Value::Null),
                    (Reply::Text(text), "tools/call") => (
                        "200 OK".to_string(),
                        json!({ "jsonrpc": "2.0", "id": id, "result": { "content": [{ "type": "text", "text": text }] } }),
                    ),
                    (_, "initialize") => (
                        "200 OK".to_string(),
                        json!({ "jsonrpc": "2.0", "id": id, "result": { "protocolVersion": "2025-06-18", "capabilities": {} } }),
                    ),
                    (_, "tools/list") => {
                        let list: Vec<Value> = tools
                            .lock()
                            .unwrap()
                            .iter()
                            .map(|n| json!({ "name": n, "description": format!("{n} docs"), "inputSchema": { "type": "object" } }))
                            .collect();
                        ("200 OK".to_string(), json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": list } }))
                    }
                    _ => ("202 Accepted".to_string(), Value::Null),
                };
                let text = if body.is_null() { String::new() } else { body.to_string() };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
                let _ = sock.write_all(response.as_bytes()).await;
            });
        }
    });
    ConnectStub { url: format!("http://{addr}/mcp"), calls, keys, paths, tools, refuse }
}

/// A `COMPOSIO_MANAGE_CONNECTIONS` answer in the shape Composio's docs show for
/// the envelope; the fields inside each result are a guess until a recording
/// exists (`CAP-C9`), which is why the parser is lenient about them.
pub fn connections_answer(entries: Value) -> String {
    json!({ "successful": true, "data": { "message": "ok", "results": entries } }).to_string()
}
