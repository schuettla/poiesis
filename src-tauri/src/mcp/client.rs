//! Minimal MCP client supporting two transports:
//!   * **Streamable HTTP** (MCP-1): a single endpoint accepting JSON-RPC over
//!     POST, replying with `application/json` or an SSE frame.
//!   * **stdio** (MCP-2): a locally-spawned server process speaking newline-
//!     delimited JSON-RPC over stdin/stdout.
//!
//! The JSON-RPC framing (`initialize`, `tools/list`, `tools/call`) is shared; a
//! `Transport` enum abstracts how one request/notification travels.

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout, Command};

use super::McpTool;

/// Protocol revision we advertise during `initialize`.
const PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("MCP server error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("unexpected MCP response: {0}")]
    Protocol(String),
}

/// A connection to one MCP server. Stateless across calls except for an optional
/// negotiated session id (HTTP) or the live child process (stdio); each
/// high-level op performs its own handshake.
pub struct McpClient {
    transport: Transport,
    next_id: u64,
}

impl McpClient {
    /// Streamable HTTP transport (MCP-1).
    pub fn new(http: reqwest::Client, url: impl Into<String>, token: Option<String>) -> Self {
        Self {
            transport: Transport::Http(HttpTransport {
                http,
                url: url.into(),
                token,
                session_id: None,
            }),
            next_id: 1,
        }
    }

    /// stdio transport (MCP-2): `command` is a command line (`program args…`) for
    /// a local MCP server spoken to over its stdin/stdout.
    pub fn new_stdio(command: impl Into<String>) -> Self {
        Self {
            transport: Transport::Stdio(StdioTransport::new(command.into())),
            next_id: 1,
        }
    }

    /// Handshake: `initialize` then the `notifications/initialized` ack.
    pub async fn initialize(&mut self) -> Result<(), McpError> {
        let params = serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "Project Nexus",
                "version": env!("CARGO_PKG_VERSION"),
            }
        });
        let _ = self.request("initialize", params).await?;
        self.notify("notifications/initialized").await?;
        Ok(())
    }

    /// List the server's tools (`tools/list`).
    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let result = self.request("tools/list", serde_json::json!({})).await?;
        #[derive(Deserialize)]
        struct ToolsList {
            #[serde(default)]
            tools: Vec<McpTool>,
        }
        let parsed: ToolsList = serde_json::from_value(result)
            .map_err(|e| McpError::Protocol(format!("bad tools/list result: {e}")))?;
        Ok(parsed.tools)
    }

    /// Invoke a tool (`tools/call`); returns the flattened text content.
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        let params = serde_json::json!({ "name": name, "arguments": arguments });
        let result = self.request("tools/call", params).await?;
        Ok(flatten_content(&result))
    }

    /// Connect + list tools in one shot (used for "test"/refresh).
    pub async fn discover(&mut self) -> Result<Vec<McpTool>, McpError> {
        self.initialize().await?;
        self.list_tools().await
    }

    // ---- JSON-RPC framing (transport-agnostic) ----

    /// Send a JSON-RPC request and return its `result`, surfacing RPC errors.
    async fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let message = self.transport.send_request(&body, id).await?;

        if let Some(err) = message.get("error") {
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error")
                .to_string();
            return Err(McpError::Rpc { code, message: msg });
        }
        Ok(message.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    /// Fire-and-forget JSON-RPC notification (no id, no result expected).
    async fn notify(&mut self, method: &str) -> Result<(), McpError> {
        let body = serde_json::json!({ "jsonrpc": "2.0", "method": method });
        self.transport.send_notification(&body).await
    }
}

/// How a single JSON-RPC message travels to the server and back.
enum Transport {
    Http(HttpTransport),
    Stdio(StdioTransport),
}

impl Transport {
    async fn send_request(
        &mut self,
        body: &serde_json::Value,
        id: u64,
    ) -> Result<serde_json::Value, McpError> {
        match self {
            Transport::Http(h) => h.send_request(body, id).await,
            Transport::Stdio(s) => s.send_request(body, id).await,
        }
    }

    async fn send_notification(&mut self, body: &serde_json::Value) -> Result<(), McpError> {
        match self {
            Transport::Http(h) => h.send_notification(body).await,
            Transport::Stdio(s) => s.send_notification(body).await,
        }
    }
}

/// Streamable HTTP transport state.
struct HttpTransport {
    http: reqwest::Client,
    url: String,
    token: Option<String>,
    session_id: Option<String>,
}

impl HttpTransport {
    fn build_post(&self, body: &serde_json::Value) -> reqwest::RequestBuilder {
        let mut req = self
            .http
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_VERSION)
            .json(body);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        if let Some(sid) = &self.session_id {
            req = req.header("Mcp-Session-Id", sid);
        }
        req
    }

    async fn send_request(
        &mut self,
        body: &serde_json::Value,
        id: u64,
    ) -> Result<serde_json::Value, McpError> {
        let resp = self.build_post(body).send().await?.error_for_status()?;

        // Capture a server-assigned session id (initialize response).
        if let Some(sid) = resp.headers().get("mcp-session-id") {
            if let Ok(s) = sid.to_str() {
                self.session_id = Some(s.to_string());
            }
        }

        let is_sse = resp
            .headers()
            .get("content-type")
            .and_then(|c| c.to_str().ok())
            .map(|c| c.contains("text/event-stream"))
            .unwrap_or(false);

        let text = resp.text().await?;
        if is_sse {
            extract_sse_json(&text, id)
                .ok_or_else(|| McpError::Protocol("no JSON-RPC message in SSE stream".into()))
        } else {
            serde_json::from_str(&text)
                .map_err(|e| McpError::Protocol(format!("invalid JSON response: {e}")))
        }
    }

    async fn send_notification(&mut self, body: &serde_json::Value) -> Result<(), McpError> {
        // A 202/200 with no body is normal; ignore the response payload.
        let _ = self.build_post(body).send().await?.error_for_status()?;
        Ok(())
    }
}

/// stdio transport state: the child MCP server plus its piped I/O, spawned lazily
/// on first use and killed on drop (`kill_on_drop`).
struct StdioTransport {
    command: String,
    stdin: Option<ChildStdin>,
    reader: Option<Lines<BufReader<ChildStdout>>>,
    // Held to keep the process alive (and to kill it on drop).
    child: Option<tokio::process::Child>,
}

impl StdioTransport {
    fn new(command: String) -> Self {
        Self {
            command,
            stdin: None,
            reader: None,
            child: None,
        }
    }

    fn ensure_spawned(&mut self) -> Result<(), McpError> {
        if self.child.is_some() {
            return Ok(());
        }
        let parts = split_command(&self.command);
        let (program, args) = parts
            .split_first()
            .ok_or_else(|| McpError::Protocol("empty stdio command".into()))?;
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|e| McpError::Protocol(format!("couldn't start the MCP server: {e}")))?;
        self.stdin = child.stdin.take();
        self.reader = child.stdout.take().map(|s| BufReader::new(s).lines());
        self.child = Some(child);
        Ok(())
    }

    async fn write_line(&mut self, body: &serde_json::Value) -> Result<(), McpError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| McpError::Protocol("MCP server stdin unavailable".into()))?;
        let mut line =
            serde_json::to_string(body).map_err(|e| McpError::Protocol(e.to_string()))?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| McpError::Protocol(format!("write to MCP server failed: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::Protocol(e.to_string()))?;
        Ok(())
    }

    async fn send_request(
        &mut self,
        body: &serde_json::Value,
        id: u64,
    ) -> Result<serde_json::Value, McpError> {
        self.ensure_spawned()?;
        self.write_line(body).await?;
        let reader = self
            .reader
            .as_mut()
            .ok_or_else(|| McpError::Protocol("MCP server stdout unavailable".into()))?;
        // Read lines until the JSON-RPC reply with our id; skip server log lines
        // and unrelated notifications.
        loop {
            match reader
                .next_line()
                .await
                .map_err(|e| McpError::Protocol(format!("read from MCP server failed: {e}")))?
            {
                Some(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        if v.get("id").and_then(|x| x.as_u64()) == Some(id) {
                            return Ok(v);
                        }
                    }
                    // Non-matching JSON or a plain log line — keep reading.
                }
                None => {
                    return Err(McpError::Protocol(
                        "the MCP server closed the connection".into(),
                    ))
                }
            }
        }
    }

    async fn send_notification(&mut self, body: &serde_json::Value) -> Result<(), McpError> {
        self.ensure_spawned()?;
        self.write_line(body).await
    }
}

/// Split a command line into program + args, honoring simple double-quotes so a
/// quoted path with spaces stays one token.
fn split_command(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in command.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::split_command;

    #[test]
    fn splits_command_with_quoted_paths() {
        assert_eq!(
            split_command("npx -y @modelcontextprotocol/server-filesystem C:/Users/me/docs"),
            vec![
                "npx",
                "-y",
                "@modelcontextprotocol/server-filesystem",
                "C:/Users/me/docs"
            ]
        );
        assert_eq!(
            split_command("node \"C:/Program Files/mcp/server.js\" --flag"),
            vec!["node", "C:/Program Files/mcp/server.js", "--flag"]
        );
    }
}

/// Pull the first JSON-RPC object matching `id` out of an SSE body.
fn extract_sse_json(body: &str, id: u64) -> Option<serde_json::Value> {
    for line in body.lines() {
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
            if value.get("id").and_then(|v| v.as_u64()) == Some(id) {
                return Some(value);
            }
        }
    }
    None
}

/// Flatten an MCP `tools/call` result's content array into plain text.
fn flatten_content(result: &serde_json::Value) -> String {
    let Some(content) = result.get("content").and_then(|c| c.as_array()) else {
        // Some servers return a bare structured result; stringify it.
        return result.to_string();
    };
    let mut out = String::new();
    for part in content {
        match part.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(t);
                }
            }
            // Non-text parts (images, resources) — note their presence.
            Some(other) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("[{other} content]"));
            }
            None => {}
        }
    }
    if out.is_empty() {
        result.to_string()
    } else {
        out
    }
}
