//! Anthropic Messages API adapter (PRD §7.6). Translates the OpenAI-compatible
//! message/tool shape the agent loop builds into Anthropic's format, streams the
//! response, and maps it back to a [`TurnOutcome`] — so cloud Anthropic models get
//! the same tool-calling agent loop as local models (CLD-5).

use serde_json::{json, Value};

use crate::runtime::proxy::{
    next_chunk, send_turn, CancelFlag, Clocks, Delta, ProxyError, ToolCallReq, TurnOutcome, Usage, Waited,
};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
pub(crate) const API_VERSION: &str = "2023-06-01";
/// The most one answer may write. 4096 cut a report written into a document
/// off partway, and the cut-off tool input then failed. 32,000 is the lowest
/// "max output" of any Claude model still served (Opus 4 and 4.1); the current
/// ones allow 64K to 128K. Only what is written is billed, not this ceiling.
const MAX_TOKENS: u32 = 32_000;

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
    on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(Delta),
{
    let turn = Turn { url: ENDPOINT, api_key, model, clocks: Clocks::TURN };
    stream_turn_to(client, turn, messages, tools, temperature, cancel, on_token).await
}

/// Where a turn goes and how long it may wait: apart so a test can aim it at a
/// local server with short clocks.
struct Turn<'a> {
    url: &'a str,
    api_key: &'a str,
    model: &'a str,
    clocks: Clocks,
}

async fn stream_turn_to<F>(
    client: &reqwest::Client,
    turn: Turn<'_>,
    messages: &[Value],
    tools: &[Value],
    temperature: f32,
    cancel: &CancelFlag,
    mut on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(Delta),
{
    let Turn { url, api_key, model, clocks } = turn;
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

    let req = client
        .post(url)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&body);
    let asked = std::time::Instant::now();
    // Same clocks and the same Stop as the OpenAI-shaped turns. Before, this had
    // neither: a quiet model waited forever, and Stop was read only when the
    // next chunk came.
    let Some(resp) = send_turn(req, cancel, clocks).await? else {
        return Ok(TurnOutcome::Cancelled);
    };

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut content = String::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut usage: Option<Usage> = None;
    // When real output last came, and when any bytes last came. A `ping` keeps
    // the turn alive, but only up to `Clocks::quiet`: it comes while nothing is
    // being written. With `eager_input_streaming` on every tool, a long tool
    // input arrives in pieces, so a healthy turn keeps refreshing `heard`.
    let mut heard: Option<std::time::Instant> = None;
    let mut alive: Option<std::time::Instant> = None;

    loop {
        let chunk = match next_chunk(&mut stream, cancel, clocks, asked, heard, alive).await? {
            Waited::Chunk(c) => c?,
            Waited::End => break,
            Waited::Cancelled => return Ok(TurnOutcome::Cancelled),
        };
        alive = Some(std::time::Instant::now());
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
                match evt.get("type").and_then(|t| t.as_str()) {
                    Some("ping") => continue,
                    // A failure after the headers were sent (`overloaded_error`
                    // is the common one). Read as data, it was an empty answer.
                    Some("error") => {
                        let kind = evt.pointer("/error/type").and_then(|t| t.as_str()).unwrap_or("");
                        let said = evt.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or(kind);
                        return Err(ProxyError::Api {
                            // What the same failure would be without streaming.
                            status: if kind == "overloaded_error" { 529 } else { 500 },
                            message: format!("The provider stopped the answer with an error: {said}"),
                        });
                    }
                    _ => heard = Some(std::time::Instant::now()),
                }
                // `OBS-1`: input tokens arrive on `message_start`, output
                // tokens on `message_delta`. Both are merged into one figure.
                for pointer in ["/message/usage", "/usage"] {
                    if let Some(u) = evt.pointer(pointer) {
                        let entry = usage.get_or_insert_with(Usage::default);
                        if let Some(n) = u.get("input_tokens").and_then(|v| v.as_u64()) {
                            entry.prompt_tokens = n;
                        }
                        if let Some(n) = u.get("output_tokens").and_then(|v| v.as_u64()) {
                            entry.output_tokens = n;
                        }
                    }
                }
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
        // Anthropic's extended thinking arrives as its own content block, which
        // this parser does not collect, so there is nothing to carry here.
        Ok(TurnOutcome::Final { content, usage, reasoning: String::new() })
    } else {
        Ok(TurnOutcome::ToolCalls { calls, usage })
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
fn handle_event<F: FnMut(Delta)>(
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
                        on_token(Delta::Answer(text));
                    }
                }
                // Extended thinking. Relayed so a long think reads as work in
                // progress; still not collected into `content`, which is the
                // answer and only the answer.
                Some("thinking_delta") => {
                    if let Some(text) = delta.get("thinking").and_then(|t| t.as_str()) {
                        on_token(Delta::Thinking(text));
                    }
                }
                Some("input_json_delta") => {
                    if let (Some(b), Some(partial)) =
                        (blocks.get_mut(idx), delta.get("partial_json").and_then(|p| p.as_str()))
                    {
                        b.input_json.push_str(partial);
                        on_token(Delta::Writing { tool: &b.name, args: &b.input_json });
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
                // Without it the API holds each input value back until it is
                // whole, so a report written into a tool arrives all at once after
                // minutes of pings: no progress to show, and only the quiet cap
                // ends a stall. Stable on every model, no beta header.
                "eager_input_streaming": true,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Thinking and answer arrive on the same stream and must never merge.
    /// `content` is what becomes the assistant message; if a `thinking_delta`
    /// ever lands in it, the app shows the model's private reasoning as its
    /// reply — which is exactly what stripping `<think>` blocks exists to stop.
    #[test]
    fn thinking_is_relayed_but_never_becomes_the_answer() {
        let mut blocks: Vec<Block> = Vec::new();
        let mut content = String::new();
        let mut seen: Vec<(bool, String)> = Vec::new();
        let mut on_token = |d: Delta| match d {
            Delta::Answer(t) => seen.push((false, t.to_string())),
            Delta::Thinking(t) => seen.push((true, t.to_string())),
            Delta::Writing { .. } => {}
        };

        for evt in [
            json!({ "type": "content_block_delta", "index": 0,
                    "delta": { "type": "thinking_delta", "thinking": "let me work this out" } }),
            json!({ "type": "content_block_delta", "index": 0,
                    "delta": { "type": "text_delta", "text": "42" } }),
        ] {
            handle_event(&evt, &mut blocks, &mut content, &mut on_token);
        }

        assert_eq!(content, "42", "only the answer belongs in the message");
        assert_eq!(
            seen,
            vec![(true, "let me work this out".into()), (false, "42".into())],
            "both were relayed, each labelled as what it is"
        );
    }

    #[test]
    fn every_tool_streams_its_input() {
        let tools = [json!({ "type": "function", "function": { "name": "create_artifact", "parameters": {} } })];
        assert_eq!(translate_tools(&tools)[0]["eager_input_streaming"], true);
    }

    // ---- the stream, against a local server ----

    use crate::runtime::proxy::clock_tests::{serve, SHORT};
    use std::time::Duration;

    async fn turn(base: &str, writes: &mut Vec<(String, usize)>) -> Result<TurnOutcome, ProxyError> {
        let url = format!("{base}/v1/messages");
        let turn = Turn { url: &url, api_key: "k", model: "m", clocks: SHORT };
        let on_token = |d: Delta| {
            if let Delta::Writing { tool, args } = d {
                writes.push((tool.to_string(), args.len()));
            }
        };
        let client = reqwest::Client::new();
        let cancel = CancelFlag::new();
        tokio::time::timeout(Duration::from_secs(5), stream_turn_to(&client, turn, &[], &[], 0.0, &cancel, on_token))
            .await
            .expect("the turn ended by itself")
    }

    const PING: &str = "event: ping\ndata: {\"type\": \"ping\"}\n\n";

    #[tokio::test]
    async fn pings_alone_end_the_turn_at_the_quiet_cap() {
        let began = std::time::Instant::now();
        let out = turn(&serve(vec![], Some(PING)).await, &mut Vec::new()).await;
        assert!(matches!(out, Err(ProxyError::Stalled(ref m)) if m.contains("keepalive")), "{out:?}");
        assert!(began.elapsed() >= SHORT.quiet, "held until the cap, not the first-word clock");
    }

    #[tokio::test]
    async fn an_error_event_is_an_error_not_an_empty_answer() {
        let failed = "event: error\ndata: {\"type\": \"error\", \"error\": {\"type\": \"overloaded_error\", \"message\": \"Overloaded\"}}\n\n";
        match turn(&serve(vec![(0, failed)], None).await, &mut Vec::new()).await {
            Err(e) => {
                assert_eq!(e.status(), Some(529));
                assert!(e.to_string().contains("Overloaded"), "{e}");
            }
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_tool_input_written_slowly_is_shown_and_not_cut() {
        let start = "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"create_artifact\"}}\n\n";
        let piece = "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"aaaa\"}}\n\n";
        // Pieces 150 ms apart, a ping between each: longer than both clocks in total.
        let parts = [(0, start)]
            .into_iter()
            .chain([(75, PING), (75, piece)].into_iter().cycle().take(10))
            .collect();
        let mut writes = Vec::new();
        let out = turn(&serve(parts, None).await, &mut writes).await;
        assert!(matches!(out, Ok(TurnOutcome::ToolCalls { ref calls, .. }) if calls[0].arguments.len() == 20), "{out:?}");
        assert_eq!(writes.last(), Some(&("create_artifact".to_string(), 20)));
        assert_eq!(writes.len(), 5);
    }
}
