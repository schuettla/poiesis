//! Anthropic Messages API adapter (PRD §7.6). Translates the OpenAI-compatible
//! message/tool shape the agent loop builds into Anthropic's format, streams the
//! response, and maps it back to a [`TurnOutcome`] — so cloud Anthropic models get
//! the same tool-calling agent loop as local models (CLD-5).

use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::runtime::proxy::{CancelFlag, ProxyError, ToolCallReq, TurnOutcome};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 4096;

/// Stream one Anthropic turn. `messages`/`tools` are OpenAI-shaped.
#[allow(clippy::too_many_arguments)]
pub async fn stream_turn<F>(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    messages: &[Value],
    tools: &[Value],
    temperature: f32,
    cancel: &CancelFlag,
    mut on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(&str),
{
    let (system, anth_messages) = translate_messages(messages);
    let mut body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "messages": anth_messages,
        "temperature": temperature,
        "stream": true,
    });
    if let Some(system) = system {
        body["system"] = json!(system);
    }
    let anth_tools = translate_tools(tools);
    if !anth_tools.is_empty() {
        body["tools"] = json!(anth_tools);
    }

    let resp = client
        .post(ENDPOINT)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?
        .error_for_status()?;

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut content = String::new();
    let mut blocks: Vec<Block> = Vec::new();

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
                continue; // ignore "event:" lines; the JSON carries its own type
            };
            let payload = payload.trim();
            if payload.is_empty() {
                continue;
            }
            if let Ok(evt) = serde_json::from_str::<Value>(payload) {
                handle_event(&evt, &mut blocks, &mut content, &mut on_token);
            }
        }
    }

    let calls: Vec<ToolCallReq> = blocks
        .into_iter()
        .filter(|b| b.kind == "tool_use")
        .map(|b| ToolCallReq {
            id: b.id,
            name: b.name,
            arguments: if b.input_json.trim().is_empty() {
                "{}".to_string()
            } else {
                b.input_json
            },
        })
        .collect();

    if calls.is_empty() {
        Ok(TurnOutcome::Final { content })
    } else {
        Ok(TurnOutcome::ToolCalls(calls))
    }
}

#[derive(Default)]
struct Block {
    kind: String,
    id: String,
    name: String,
    input_json: String,
}

/// Apply one Anthropic stream event to the accumulators.
fn handle_event<F: FnMut(&str)>(
    evt: &Value,
    blocks: &mut Vec<Block>,
    content: &mut String,
    on_token: &mut F,
) {
    match evt.get("type").and_then(|t| t.as_str()) {
        Some("content_block_start") => {
            let idx = evt.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            while blocks.len() <= idx {
                blocks.push(Block::default());
            }
            if let Some(cb) = evt.get("content_block") {
                let kind = cb.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                blocks[idx].kind = kind.to_string();
                if kind == "tool_use" {
                    blocks[idx].id = cb.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    blocks[idx].name =
                        cb.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                }
            }
        }
        Some("content_block_delta") => {
            let idx = evt.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let Some(delta) = evt.get("delta") else { return };
            match delta.get("type").and_then(|t| t.as_str()) {
                Some("text_delta") => {
                    if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                        content.push_str(text);
                        on_token(text);
                    }
                }
                Some("input_json_delta") => {
                    if let (Some(b), Some(partial)) =
                        (blocks.get_mut(idx), delta.get("partial_json").and_then(|p| p.as_str()))
                    {
                        b.input_json.push_str(partial);
                    }
                }
                _ => {}
            }
        }
        _ => {} // message_start/stop, content_block_stop, ping, message_delta
    }
}

/// Translate OpenAI tool specs to Anthropic tool defs.
fn translate_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let f = t.get("function")?;
            let name = f.get("name")?.as_str()?;
            Some(json!({
                "name": name,
                "description": f.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                "input_schema": f.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object"})),
            }))
        })
        .collect()
}

/// Translate the OpenAI-shaped history into (system prompt, Anthropic messages).
/// System turns are hoisted out; consecutive `role:"tool"` results are merged
/// into a single `user` message of `tool_result` blocks (Anthropic's shape).
fn translate_messages(messages: &[Value]) -> (Option<String>, Vec<Value>) {
    let mut system_parts: Vec<String> = Vec::new();
    let mut out: Vec<Value> = Vec::new();
    let mut pending_results: Vec<Value> = Vec::new();

    let flush = |out: &mut Vec<Value>, pending: &mut Vec<Value>| {
        if !pending.is_empty() {
            out.push(json!({ "role": "user", "content": std::mem::take(pending) }));
        }
    };

    for m in messages {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let content = m.get("content").unwrap_or(&Value::Null);
        match role {
            "system" => {
                if let Some(text) = content_as_text(content) {
                    if !text.is_empty() {
                        system_parts.push(text);
                    }
                }
            }
            "tool" => {
                let id = m.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
                let text = content_as_text(content).unwrap_or_default();
                pending_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": text,
                }));
            }
            "assistant" => {
                flush(&mut out, &mut pending_results);
                let mut content_blocks: Vec<Value> = Vec::new();
                if let Some(text) = content.as_str() {
                    if !text.is_empty() {
                        content_blocks.push(json!({ "type": "text", "text": text }));
                    }
                }
                if let Some(tcs) = m.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tcs {
                        let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let func = tc.get("function");
                        let name = func
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let args_str = func
                            .and_then(|f| f.get("arguments"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}");
                        let input: Value = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
                        content_blocks.push(json!({
                            "type": "tool_use", "id": id, "name": name, "input": input,
                        }));
                    }
                }
                if !content_blocks.is_empty() {
                    out.push(json!({ "role": "assistant", "content": content_blocks }));
                }
            }
            _ => {
                // user
                flush(&mut out, &mut pending_results);
                out.push(json!({ "role": "user", "content": translate_user_content(content) }));
            }
        }
    }
    flush(&mut out, &mut pending_results);

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    (system, out)
}

/// Translate user content (plain string or OpenAI content parts) to Anthropic.
fn translate_user_content(content: &Value) -> Value {
    if let Some(s) = content.as_str() {
        return json!(s);
    }
    let Some(parts) = content.as_array() else {
        return json!("");
    };
    let blocks: Vec<Value> = parts
        .iter()
        .filter_map(|p| match p.get("type").and_then(|t| t.as_str()) {
            Some("text") => Some(json!({
                "type": "text",
                "text": p.get("text").and_then(|t| t.as_str()).unwrap_or(""),
            })),
            Some("image_url") => {
                let url = p.get("image_url").and_then(|u| u.get("url")).and_then(|u| u.as_str())?;
                if let Some((media_type, data)) = parse_data_uri(url) {
                    Some(json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": media_type, "data": data },
                    }))
                } else {
                    Some(json!({
                        "type": "image",
                        "source": { "type": "url", "url": url },
                    }))
                }
            }
            _ => None,
        })
        .collect();
    json!(blocks)
}

/// Flatten content that may be a string or an array of OpenAI parts to text.
fn content_as_text(content: &Value) -> Option<String> {
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(parts) = content.as_array() {
        let text: String = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("");
        return Some(text);
    }
    None
}

/// Split a `data:` URI into (media_type, base64) for Anthropic image blocks.
fn parse_data_uri(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let media_type = meta.split(';').next().unwrap_or("image/png").to_string();
    Some((media_type, data.to_string()))
}
