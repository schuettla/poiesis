//! Streaming proxy (PRD §7.4): forwards a chat-completion request to the engine
//! and relays the SSE token stream back to the caller, with user-initiated
//! cancellation. The engine speaks an OpenAI-compatible API, so this same parser
//! also serves OpenAI/OpenRouter cloud providers later (§7.6).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
}

/// One event in a streamed assistant turn.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StreamEvent {
    /// A chunk of assistant prose.
    Token { text: String },
    /// The model requested one or more tool calls (handled by the agent loop).
    ToolCall { raw: String },
    /// Stream finished normally.
    Done,
    /// Stream ended due to an error.
    Error { message: String },
    /// Stream cancelled by the user (Stop control, CHT-2).
    Cancelled,
}

/// A shareable cancellation flag handed to the composer's Stop control.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A tool call requested by the model (native tool calling, TOOL-2).
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallReq {
    pub id: String,
    pub name: String,
    /// Raw JSON arguments string as emitted by the model.
    pub arguments: String,
}

/// How a single model turn ended.
#[derive(Debug, Clone)]
pub enum TurnOutcome {
    /// The model produced a final answer (already streamed via the token sink).
    Final { content: String },
    /// The model asked to call one or more tools before answering.
    ToolCalls(Vec<ToolCallReq>),
    /// The user cancelled mid-turn.
    Cancelled,
}

/// Accumulator for streamed tool-call deltas, keyed by their `index`.
#[derive(Default)]
struct ToolCallAccum {
    id: String,
    name: String,
    arguments: String,
}

/// Merge a streamed `tool_calls` delta array into the accumulator map.
fn accumulate_tool_calls(delta: &serde_json::Value, acc: &mut Vec<ToolCallAccum>) {
    let Some(calls) = delta.as_array() else { return };
    for call in calls {
        let idx = call.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        while acc.len() <= idx {
            acc.push(ToolCallAccum::default());
        }
        let slot = &mut acc[idx];
        if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
            if !id.is_empty() {
                slot.id = id.to_string();
            }
        }
        if let Some(func) = call.get("function") {
            if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                if !name.is_empty() {
                    slot.name = name.to_string();
                }
            }
            if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                slot.arguments.push_str(args);
            }
        }
    }
}

/// Stream one model turn: relay prose tokens through `on_token`, accumulate any
/// native tool calls, and report how the turn ended. Used by the agent loop.
pub async fn stream_turn<F>(
    client: &reqwest::Client,
    base_url: &str,
    token: Option<&str>,
    body: serde_json::Value,
    cancel: &CancelFlag,
    mut on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(&str),
{
    let url = format!("{base_url}/v1/chat/completions");
    let mut req = client.post(&url).json(&body);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await?.error_for_status()?;

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut content = String::new();
    let mut tool_acc: Vec<ToolCallAccum> = Vec::new();

    while let Some(chunk) = stream.next().await {
        if cancel.is_cancelled() {
            return Ok(TurnOutcome::Cancelled);
        }
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(nl) = buf.find('\n') {
            let line = buf[..nl].trim().to_string();
            buf.drain(..=nl);
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload == "[DONE]" {
                buf.clear();
                break;
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(delta) = json.pointer("/choices/0/delta") {
                    if let Some(tc) = delta.get("tool_calls") {
                        accumulate_tool_calls(tc, &mut tool_acc);
                    }
                    if let Some(text) = delta.get("content").and_then(|c| c.as_str()) {
                        if !text.is_empty() {
                            content.push_str(text);
                            on_token(text);
                        }
                    }
                }
            }
        }
    }

    if !tool_acc.is_empty() && tool_acc.iter().any(|t| !t.name.is_empty()) {
        let calls = tool_acc
            .into_iter()
            .filter(|t| !t.name.is_empty())
            .map(|t| ToolCallReq {
                id: if t.id.is_empty() {
                    uuid_like()
                } else {
                    t.id
                },
                name: t.name,
                arguments: if t.arguments.is_empty() {
                    "{}".to_string()
                } else {
                    t.arguments
                },
            })
            .collect();
        Ok(TurnOutcome::ToolCalls(calls))
    } else {
        Ok(TurnOutcome::Final { content })
    }
}

/// Cheap unique-ish id for tool calls a model didn't id itself.
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("call_{n:x}")
}

/// Parse the `content` delta out of an OpenAI-style streaming chunk.
fn extract_delta(json: &serde_json::Value) -> Option<String> {
    json.get("choices")?
        .get(0)?
        .get("delta")?
        .get("content")?
        .as_str()
        .map(|s| s.to_string())
}

/// Whether a chunk carries tool-call deltas (native tool calling, TOOL-2).
fn extract_tool_call(json: &serde_json::Value) -> Option<String> {
    let tc = json.get("choices")?.get(0)?.get("delta")?.get("tool_calls")?;
    if tc.is_null() {
        None
    } else {
        Some(tc.to_string())
    }
}

/// POST a chat-completion request to `base_url` (with `stream: true` already set
/// by the caller) and drive `on_event` for each parsed event. Honors `cancel`.
pub async fn stream_completion<F>(
    client: &reqwest::Client,
    base_url: &str,
    token: Option<&str>,
    body: serde_json::Value,
    cancel: CancelFlag,
    mut on_event: F,
) -> Result<(), ProxyError>
where
    F: FnMut(StreamEvent),
{
    let url = format!("{base_url}/v1/chat/completions");
    let mut req = client.post(&url).json(&body);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            on_event(StreamEvent::Error {
                message: e.to_string(),
            });
            return Err(e.into());
        }
    };

    if let Err(e) = resp.error_for_status_ref() {
        on_event(StreamEvent::Error {
            message: format!("engine returned an error: {e}"),
        });
        return Err(e.into());
    }

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();

    while let Some(chunk) = stream.next().await {
        if cancel.is_cancelled() {
            on_event(StreamEvent::Cancelled);
            return Ok(());
        }
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                on_event(StreamEvent::Error {
                    message: e.to_string(),
                });
                return Err(e.into());
            }
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));

        // SSE frames are separated by newlines; each data line is `data: …`.
        while let Some(nl) = buf.find('\n') {
            let line = buf[..nl].trim().to_string();
            buf.drain(..=nl);
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload == "[DONE]" {
                on_event(StreamEvent::Done);
                return Ok(());
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(tc) = extract_tool_call(&json) {
                    on_event(StreamEvent::ToolCall { raw: tc });
                }
                if let Some(text) = extract_delta(&json) {
                    if !text.is_empty() {
                        on_event(StreamEvent::Token { text });
                    }
                }
            }
        }
    }

    on_event(StreamEvent::Done);
    Ok(())
}
