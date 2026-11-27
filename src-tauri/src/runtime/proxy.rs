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
    /// The provider answered, and said why it refused. `reqwest`'s own error for
    /// a non-2xx status carries only the status line — "HTTP status client error
    /// (404 Not Found)" — and drops the body, which for every OpenAI-compatible
    /// provider is the only place the actual reason lives ("No endpoints found
    /// that support tool use", "insufficient credits", …). Losing it makes a
    /// failed run undiagnosable from inside the app, so this variant carries it.
    #[error("{message}")]
    Api {
        status: u16,
        /// The provider's own message, already unwrapped from its JSON envelope.
        message: String,
    },
    /// The connection stayed open and the provider stopped sending. There is no
    /// status and no body to report — the request never failed, it simply never
    /// finished — so this cannot be an `Api` error, and calling it a network
    /// error would point the user at their own connection.
    #[error("{0}")]
    Stalled(String),
}

impl ProxyError {
    /// The provider's HTTP status, when the failure was an answered request.
    pub fn status(&self) -> Option<u16> {
        match self {
            ProxyError::Api { status, .. } => Some(*status),
            ProxyError::Http(e) => e.status().map(|s| s.as_u16()),
            ProxyError::Stalled(_) => None,
        }
    }

    /// The provider's message, for callers matching on what it said (e.g. the
    /// tool-use retry in `cloud::drive_turn`). Empty for transport errors.
    pub fn provider_message(&self) -> &str {
        match self {
            ProxyError::Api { message, .. } => message,
            ProxyError::Http(_) | ProxyError::Stalled(_) => "",
        }
    }
}

/// Turn a non-2xx response into a [`ProxyError::Api`] carrying what the provider
/// actually said. Consumes the response, so callers check the status first.
pub async fn api_error(resp: reqwest::Response) -> ProxyError {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    ProxyError::Api {
        status: status.as_u16(),
        message: format!("{} — {}", status, provider_message(&body)),
    }
}

/// Dig the human-readable reason out of an error body. OpenAI, OpenRouter and
/// Anthropic all nest it differently (`error.message`, `error`, `message`), and
/// a provider having a bad day may return no JSON at all — so fall back to the
/// raw body rather than swallowing it.
fn provider_message(body: &str) -> String {
    const CAP: usize = 400;
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "the provider gave no reason".to_string();
    }
    let text = serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .and_then(|v| {
            for path in ["/error/message", "/error", "/message", "/detail"] {
                if let Some(s) = v.pointer(path).and_then(|m| m.as_str()) {
                    if !s.trim().is_empty() {
                        return Some(s.trim().to_string());
                    }
                }
            }
            None
        })
        .unwrap_or_else(|| trimmed.to_string());
    if text.chars().count() > CAP {
        let cut: String = text.chars().take(CAP).collect();
        format!("{cut}…")
    } else {
        text
    }
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

/// How often `until_cancelled` looks at the flag while it waits.
const CANCEL_POLL_MS: u64 = 80;

/// How long one stream may go silent before the turn gives up on it.
///
/// This is a gap *between chunks*, not a limit on the turn: a model that is
/// thinking hard still emits reasoning deltas, and one that is answering emits
/// content, so a live stream refreshes this clock constantly. Two minutes of
/// nothing at all means the provider has stopped, and waiting longer only makes
/// the app look like the thing that broke. Generous on purpose — the failure it
/// replaces (waiting forever) is worse than giving up a little late.
const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// How long the wait for response *headers* may last before the turn gives up.
///
/// `STREAM_IDLE_TIMEOUT` only guards the gaps between chunks, which means it
/// never starts: a provider that accepts the connection and then never answers
/// leaves the run parked on step 1 with a running clock and no way out but Stop
/// — exactly the hang it was meant to fix. This covers the other half. It is
/// longer than the idle limit because a free tier can legitimately queue a
/// request for a while before the first byte, and unlike a mid-stream stall
/// there is no evidence yet that anything is wrong.
///
/// In `stream_turn` it runs to the first bytes of the body, not only to the
/// headers: OpenRouter sends its headers at once and then waits in the body.
const RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// How long a provider may keep a turn open with keepalives only (OpenRouter's
/// `: OPENROUTER PROCESSING`, Anthropic's `ping`) and no real output.
///
/// A keepalive says the provider is still working, so it holds off the idle
/// clock: a provider may keep a tool call back until it is whole. But on a
/// free OpenRouter model every silence past a minute ended in nothing (an
/// empty answer, an error, or the user pressing Stop after four minutes), while
/// the same model streamed a whole report in 35 seconds. So the cap is short,
/// and a turn cut by it is asked again (`agent::run`, `STALL_RETRIES`).
const QUIET_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// How long a model may think without producing a single word of answer.
///
/// Neither clock above catches a model that is *busy* going nowhere: reasoning
/// deltas keep arriving, so the idle timeout is refreshed by the very thing
/// that has gone wrong. Observed in the wild at ten minutes and a hundred
/// thousand characters of thinking with no answer — a loop, not deep thought.
///
/// Hitting this does not fail the turn: the stream is cut and whatever the turn
/// has is returned, which for an all-thinking turn is empty and lands in
/// `RunState::rescue_empty_answer` — the model is asked once, plainly, to write
/// the answer. That is a better outcome than either waiting or erroring.
const THINKING_BUDGET: std::time::Duration = std::time::Duration::from_secs(120);

/// The message a caller sees when either clock fires. Both failures look the
/// same from the outside — nothing arrived — so they read the same too.
fn stalled(secs: u64, what: &str) -> ProxyError {
    ProxyError::Stalled(format!(
        "The model {what} for {secs} seconds, so I gave up waiting. This is usually the \
         provider rather than your request — try again, or use a different model."
    ))
}

/// Await `fut`, but give up as soon as `cancel` trips. `None` means cancelled.
///
/// `CHT-2b`: Stop used to be read only between arriving chunks, which is the
/// one moment a stalled turn never reaches. Waiting on the response headers of
/// a slow or free-tier endpoint could sit there for a minute with the flag
/// already set and the user pressing a button that did nothing. A `CancelFlag`
/// is a bare atomic with nothing to await on, so this polls it on a short tick
/// instead of growing a notification channel — the tick costs nothing next to
/// the network wait it is racing.
///
/// Dropping `fut` on cancellation is what actually stops the work: dropping a
/// `reqwest` future closes the connection.
pub(crate) async fn until_cancelled<T>(cancel: &CancelFlag, fut: impl std::future::Future<Output = T>) -> Option<T> {
    if cancel.is_cancelled() {
        return None;
    }
    tokio::pin!(fut);
    loop {
        tokio::select! {
            out = &mut fut => return Some(out),
            _ = tokio::time::sleep(std::time::Duration::from_millis(CANCEL_POLL_MS)) => {
                if cancel.is_cancelled() {
                    return None;
                }
            }
        }
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

/// What one model turn cost (`OBS-1`).
///
/// `None` rather than zero when the provider said nothing: a turn whose cost is
/// unknown must not read as a free one. The integrated engine reports usage on
/// its final chunk, and Anthropic always does. A plain OpenAI-compatible cloud
/// stream only reports it when the request asks — `OBS-2` now sets
/// `stream_options.include_usage` on every request that names a model, which is
/// every remote one. The integrated engine (no `model` field) is deliberately
/// left alone: it already reports, and it is the one server we cannot afford to
/// hand an unknown field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    /// Read an OpenAI-shaped `usage` object. Anthropic's adapter builds one
    /// directly; its field names differ.
    pub fn from_openai(value: &serde_json::Value) -> Option<Self> {
        let obj = value.as_object()?;
        let prompt = obj.get("prompt_tokens").and_then(|v| v.as_u64());
        let output = obj.get("completion_tokens").and_then(|v| v.as_u64());
        if prompt.is_none() && output.is_none() {
            return None;
        }
        Some(Self {
            prompt_tokens: prompt.unwrap_or(0),
            output_tokens: output.unwrap_or(0),
        })
    }

    pub fn add(&mut self, other: Usage) {
        self.prompt_tokens += other.prompt_tokens;
        self.output_tokens += other.output_tokens;
    }
}

/// How a single model turn ended.
#[derive(Debug, Clone)]
pub enum TurnOutcome {
    /// The model produced a final answer (already streamed via the token sink).
    Final {
        content: String,
        usage: Option<Usage>,
        /// A reasoning model's thinking, when the provider streams it in a
        /// field of its own rather than inside `content`.
        ///
        /// Kept apart from the answer on purpose — thinking is not a reply, and
        /// the loop already strips the `<think>…</think>` form for the same
        /// reason. It is carried so a turn that produced *only* thinking can be
        /// told apart from one that produced nothing at all: those two look
        /// identical from `content` alone, and they need opposite handling.
        reasoning: String,
    },
    /// The model asked to call one or more tools before answering.
    ToolCalls {
        calls: Vec<ToolCallReq>,
        usage: Option<Usage>,
    },
    /// The user cancelled mid-turn.
    Cancelled,
}

impl TurnOutcome {
    /// What this turn cost, if the provider said.
    pub fn usage(&self) -> Option<Usage> {
        match self {
            TurnOutcome::Final { usage, .. } | TurnOutcome::ToolCalls { usage, .. } => *usage,
            TurnOutcome::Cancelled => None,
        }
    }
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
/// What a streamed chunk turned out to be.
///
/// The two are kept apart all the way up rather than merged into one string of
/// text: thinking is evidence that a turn is alive, and the answer is the
/// answer. Anything that shows one as the other is a bug, and a single `&str`
/// callback made that bug easy to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delta<'a> {
    Answer(&'a str),
    Thinking(&'a str),
    /// A tool call is being written: its name and its arguments so far.
    Writing { tool: &'a str, args: &'a str },
}

pub async fn stream_turn<F>(
    client: &reqwest::Client,
    base_url: &str,
    token: Option<&str>,
    body: serde_json::Value,
    cancel: &CancelFlag,
    on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(Delta),
{
    stream_turn_timed(client, base_url, token, body, cancel, Clocks::TURN, on_token).await
}

/// The waits of a turn, apart so a test can make them short.
#[derive(Clone, Copy)]
pub(crate) struct Clocks {
    /// From the request to the first bytes of any kind.
    pub first_word: std::time::Duration,
    /// From one arrival of bytes of any kind to the next.
    pub idle: std::time::Duration,
    /// From the last real output (or the request) while only keepalives come.
    pub quiet: std::time::Duration,
}

impl Clocks {
    pub(crate) const TURN: Clocks =
        Clocks { first_word: RESPONSE_TIMEOUT, idle: STREAM_IDLE_TIMEOUT, quiet: QUIET_TIMEOUT };
}

/// What waiting for the next chunk of a stream came to.
pub(crate) enum Waited<T> {
    Chunk(T),
    End,
    Cancelled,
}

/// The next chunk of a turn's stream, on the turn's clocks. `alive` is when any
/// bytes last came (keepalives too), `heard` when real output last came; both
/// are `None` before any, and the clocks then run from `asked`. Only the caller
/// can tell real output from a keepalive, so it keeps both.
///
/// Stop is polled while waiting: a model that has gone quiet leaves this await
/// hanging, and the next chunk may never come.
pub(crate) async fn next_chunk<S: futures_util::Stream + Unpin>(
    stream: &mut S,
    cancel: &CancelFlag,
    clocks: Clocks,
    asked: std::time::Instant,
    heard: Option<std::time::Instant>,
    alive: Option<std::time::Instant>,
) -> Result<Waited<S::Item>, ProxyError> {
    // Nothing at all on the line: the connection or the provider is gone.
    let (since, limit) = match alive {
        None => (asked, clocks.first_word),
        Some(at) => (at, clocks.idle),
    };
    let what = if heard.is_none() { "never started answering" } else { "stopped sending anything" };
    // Keepalives only: the provider says it is working, but not for ever.
    let quiet_end = heard.unwrap_or(asked) + clocks.quiet;
    let (end, limit, what) = if quiet_end < since + limit {
        (quiet_end, clocks.quiet, "sent only keepalive signals and no words")
    } else {
        (since + limit, limit, what)
    };
    let deadline = tokio::time::Instant::from_std(end);
    match until_cancelled(cancel, tokio::time::timeout_at(deadline, stream.next())).await {
        Some(Ok(Some(c))) => Ok(Waited::Chunk(c)),
        Some(Ok(None)) => Ok(Waited::End),
        // The connection is open but no output has come for a long time. Before
        // this, a provider that stalled held the run open forever and the only
        // way out was Stop, which reads as the app being broken, not the provider.
        Some(Err(_)) => Err(stalled(limit.as_secs(), what)),
        None => Ok(Waited::Cancelled),
    }
}

/// Send a request on the turn's first clock, with Stop honored while it waits.
/// `Ok(None)` is Stop.
pub(crate) async fn send_turn(
    req: reqwest::RequestBuilder,
    cancel: &CancelFlag,
    clocks: Clocks,
) -> Result<Option<reqwest::Response>, ProxyError> {
    // The wait for response headers is the longest part of a slow turn, and
    // used to be the part Stop could not interrupt (`CHT-2b`). It also used to
    // have no clock at all, which is the hang that survived the idle timeout.
    let Some(resp) = until_cancelled(cancel, tokio::time::timeout(clocks.first_word, req.send())).await else {
        return Ok(None);
    };
    let resp = match resp {
        Ok(r) => r?,
        Err(_) => return Err(stalled(clocks.first_word.as_secs(), "never started answering")),
    };
    if resp.status().is_client_error() || resp.status().is_server_error() {
        return Err(api_error(resp).await);
    }
    Ok(Some(resp))
}

async fn stream_turn_timed<F>(
    client: &reqwest::Client,
    base_url: &str,
    token: Option<&str>,
    body: serde_json::Value,
    cancel: &CancelFlag,
    clocks: Clocks,
    mut on_token: F,
) -> Result<TurnOutcome, ProxyError>
where
    F: FnMut(Delta),
{
    let url = format!("{base_url}/v1/chat/completions");
    let mut req = client.post(&url).json(&body);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let asked = std::time::Instant::now();
    let Some(resp) = send_turn(req, cancel, clocks).await? else {
        return Ok(TurnOutcome::Cancelled);
    };

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_acc: Vec<ToolCallAccum> = Vec::new();
    // `OBS-1`: servers that report usage put it on the last chunk, whose
    // `choices` array is usually empty — so it is read from the chunk itself,
    // not from a delta.
    let mut usage: Option<Usage> = None;
    // When the current unbroken run of thinking began. Reset by anything the
    // model actually says, so this measures "thinking with nothing to show for
    // it", not total thinking.
    let mut thinking_since: Option<std::time::Instant> = None;
    let mut ran_away = false;
    // When the last `data:` line came, and when any bytes last came. OpenRouter
    // sends `: OPENROUTER PROCESSING` comment lines while the provider works
    // with nothing to show, also while it holds back a whole tool call. They
    // keep the turn alive, but only up to `Clocks::quiet` (see `next_chunk`).
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
                continue;
            };
            let payload = payload.trim();
            heard = Some(std::time::Instant::now());
            if payload == "[DONE]" {
                buf.clear();
                break;
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
                // A provider that fails after the headers were sent says so in a
                // chunk with a top-level `error`, still under 200 OK. Read as
                // data, it was an empty answer: "I stopped without saying anything".
                if let Some(err) = json.get("error").filter(|e| !e.is_null()) {
                    let status = err.get("code").and_then(|c| c.as_u64()).and_then(|c| u16::try_from(c).ok());
                    return Err(ProxyError::Api {
                        status: status.unwrap_or(502),
                        message: format!("The provider stopped the answer with an error: {}", provider_message(payload)),
                    });
                }
                if let Some(u) = json.get("usage").and_then(Usage::from_openai) {
                    usage = Some(u);
                }
                if let Some(delta) = json.pointer("/choices/0/delta") {
                    if let Some(tc) = delta.get("tool_calls") {
                        accumulate_tool_calls(tc, &mut tool_acc);
                        // A tool call being written is output too: a long one (a
                        // whole report into a document) can take minutes, and the
                        // screen says what is being written (`Delta::Writing`).
                        let at = tc.as_array().and_then(|a| a.last()).and_then(|c| c.get("index")).and_then(|i| i.as_u64());
                        if let Some(slot) = tool_acc.get(at.unwrap_or(0) as usize) {
                            thinking_since = None;
                            on_token(Delta::Writing { tool: &slot.name, args: &slot.arguments });
                        }
                    }
                    if let Some(text) = delta.get("content").and_then(|c| c.as_str()) {
                        if !text.is_empty() {
                            // It said something, so it is not stuck in its own
                            // head any more. The budget starts again from here.
                            thinking_since = None;
                            content.push_str(text);
                            on_token(Delta::Answer(text));
                        }
                    }
                    // A reasoning model streams its thinking in a field of its
                    // own: OpenRouter calls it `reasoning`, DeepSeek and others
                    // `reasoning_content`. Relayed as `Thinking`, never as the
                    // answer — a turn that spends three minutes thinking and a
                    // turn that has died are indistinguishable otherwise, which
                    // is what made a live run look like a hang.
                    for field in ["reasoning", "reasoning_content"] {
                        if let Some(text) = delta.get(field).and_then(|c| c.as_str()) {
                            if !text.is_empty() {
                                reasoning.push_str(text);
                                on_token(Delta::Thinking(text));
                                let since = thinking_since.get_or_insert_with(std::time::Instant::now);
                                if since.elapsed() > THINKING_BUDGET {
                                    eprintln!(
                                        "stream_turn: the model has been thinking for {}s and {} characters without \
                                         answering; cutting the stream and asking it for the answer",
                                        since.elapsed().as_secs(),
                                        reasoning.chars().count()
                                    );
                                    ran_away = true;
                                }
                            }
                        }
                    }
                }
            }
            if ran_away {
                break;
            }
        }
        // Dropping the stream closes the connection, which is what actually
        // stops a model that would otherwise think until the provider times it
        // out. Whatever it managed to say is kept and returned below.
        if ran_away {
            break;
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
        Ok(TurnOutcome::ToolCalls { calls, usage })
    } else {
        Ok(TurnOutcome::Final { content, usage, reasoning })
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;

    /// The body OpenRouter actually returns for the failure this shipped for.
    /// Before, the user saw only "HTTP status client error (404 Not Found)".
    #[test]
    fn unwraps_openrouters_error_envelope() {
        let body = r#"{"error":{"code":404,"message":"No endpoints found that support tool use.","metadata":{"error_type":"not_found"}}}"#;
        assert_eq!(provider_message(body), "No endpoints found that support tool use.");
    }

    #[test]
    fn unwraps_the_other_shapes_providers_use() {
        assert_eq!(provider_message(r#"{"message":"insufficient credits"}"#), "insufficient credits");
        assert_eq!(provider_message(r#"{"error":"model not found"}"#), "model not found");
    }

    /// A provider having a bad day returns an HTML error page or nothing at
    /// all. Neither may swallow the only evidence there is.
    #[test]
    fn falls_back_to_the_raw_body_and_says_so_when_empty() {
        assert_eq!(provider_message("<html>502 Bad Gateway</html>"), "<html>502 Bad Gateway</html>");
        assert_eq!(provider_message("   "), "the provider gave no reason");
    }

    #[test]
    fn caps_a_runaway_body_on_char_boundaries() {
        let body = format!(r#"{{"error":{{"message":"{}"}}}}"#, "ä".repeat(900));
        let msg = provider_message(&body);
        assert!(msg.ends_with('…'));
        assert_eq!(msg.chars().count(), 401);
    }
}

#[cfg(test)]
pub(crate) mod clock_tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A server that answers one request with a stream: each part after its delay,
    /// then, if `hold` is set, that keepalive every 20 ms until the client leaves.
    pub(crate) async fn serve(parts: Vec<(u64, &'static str)>, hold: Option<&'static str>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = sock.read(&mut buf).await.unwrap();
                seen.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&seen);
                if let Some(split) = text.find("\r\n\r\n") {
                    let len = text[..split]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if n == 0 || seen.len() >= split + 4 + len {
                        break;
                    }
                }
            }
            let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
            if sock.write_all(head.as_bytes()).await.is_err() {
                return;
            }
            for (delay, part) in parts {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                if sock.write_all(part.as_bytes()).await.is_err() {
                    return;
                }
            }
            while let Some(keepalive) = hold {
                tokio::time::sleep(Duration::from_millis(20)).await;
                if sock.write_all(keepalive.as_bytes()).await.is_err() {
                    return;
                }
            }
        });
        base
    }

    /// OpenRouter's, sent while the model has not started.
    const KEEPALIVE: &str = ": OPENROUTER PROCESSING\n\n";

    /// Far apart, and far from the test gaps: Windows sleeps round up and a
    /// loaded test run adds more, so tight margins fail at random.
    pub(crate) const SHORT: Clocks =
        Clocks { first_word: Duration::from_millis(600), idle: Duration::from_millis(500), quiet: Duration::from_millis(2000) };

    async fn turn(base: &str) -> Result<TurnOutcome, ProxyError> {
        let client = reqwest::Client::new();
        let cancel = CancelFlag::new();
        tokio::time::timeout(
            Duration::from_secs(5),
            stream_turn_timed(&client, base, None, serde_json::json!({}), &cancel, SHORT, |_| {}),
        )
        .await
        .expect("the turn ended by itself")
    }

    #[tokio::test]
    async fn keepalives_alone_end_the_turn_at_the_quiet_cap() {
        let base = serve(vec![], Some(KEEPALIVE)).await;
        let began = std::time::Instant::now();
        let out = turn(&base).await;
        assert!(matches!(out, Err(ProxyError::Stalled(ref m)) if m.contains("keepalive")), "{out:?}");
        assert!(began.elapsed() >= SHORT.quiet, "held until the cap, not the first-word clock");
    }

    #[tokio::test]
    async fn keepalives_after_some_words_end_the_turn_at_the_quiet_cap() {
        let base = serve(vec![(0, "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n")], Some(KEEPALIVE)).await;
        let out = turn(&base).await;
        assert!(matches!(out, Err(ProxyError::Stalled(ref m)) if m.contains("keepalive")), "{out:?}");
    }

    #[tokio::test]
    async fn a_silent_line_after_some_words_ends_on_the_idle_clock() {
        let word = "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n";
        let out = turn(&serve(vec![(0, word), (3000, "data: [DONE]\n\n")], None).await).await;
        assert!(matches!(out, Err(ProxyError::Stalled(ref m)) if m.contains("stopped sending")), "{out:?}");
    }

    /// What cut almost every long write: the provider keeps the whole tool call
    /// back and sends only keepalives while the model writes it.
    #[tokio::test]
    async fn a_tool_call_held_back_behind_keepalives_is_not_cut() {
        let first = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n";
        let whole = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"create_artifact\",\"arguments\":\"{}\"}}]}}]}\n\n";
        // 900 ms of keepalives: past both the idle and the first-word clock, inside the cap.
        let parts = [(0, first)]
            .into_iter()
            .chain(std::iter::repeat((90, KEEPALIVE)).take(10))
            .chain([(0, whole), (0, "data: [DONE]\n\n")])
            .collect();
        let out = turn(&serve(parts, None).await).await;
        assert!(matches!(out, Ok(TurnOutcome::ToolCalls { ref calls, .. }) if calls[0].name == "create_artifact"), "{out:?}");
    }

    #[tokio::test]
    async fn a_tool_call_written_slowly_is_shown_and_not_cut() {
        let piece = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"create_artifact\",\"arguments\":\"aaaa\"}}]}}]}\n\n";
        // Five pieces 150 ms apart, keepalives between: longer than both clocks in total.
        let parts = [(75, KEEPALIVE), (75, piece)].into_iter().cycle().take(10).chain([(0, "data: [DONE]\n\n")]).collect();
        let mut writes: Vec<(String, usize)> = Vec::new();
        let client = reqwest::Client::new();
        let cancel = CancelFlag::new();
        let on_token = |d: Delta| {
            if let Delta::Writing { tool, args } = d {
                writes.push((tool.to_string(), args.len()));
            }
        };
        let base = serve(parts, None).await;
        let out = stream_turn_timed(&client, &base, None, serde_json::json!({}), &cancel, SHORT, on_token).await;
        assert!(matches!(out, Ok(TurnOutcome::ToolCalls { ref calls, .. }) if calls[0].arguments.len() == 20), "{out:?}");
        assert_eq!(writes.len(), 5);
        assert_eq!(writes.last(), Some(&("create_artifact".to_string(), 20)));
    }

    #[tokio::test]
    async fn a_slow_answer_that_keeps_coming_is_not_cut() {
        let word = "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n";
        // 24 words 100 ms apart: longer than every clock in total, never a gap past one.
        let parts = vec![(100, word); 24].into_iter().chain([(0, "data: [DONE]\n\n")]).collect();
        let out = turn(&serve(parts, None).await).await;
        assert!(matches!(out, Ok(TurnOutcome::Final { ref content, .. }) if content == &"a".repeat(24)), "{out:?}");
    }

    /// OpenRouter's shape for a failure after the headers were sent.
    #[tokio::test]
    async fn an_error_chunk_is_an_error_not_an_empty_answer() {
        let failed = "data: {\"error\":{\"code\":502,\"message\":\"Provider disconnected unexpectedly\"},\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"error\"}]}\n\n";
        let out = turn(&serve(vec![(0, failed)], None).await).await;
        match out {
            Err(e) => {
                assert_eq!(e.status(), Some(502));
                assert!(e.to_string().contains("Provider disconnected unexpectedly"), "{e}");
            }
            other => panic!("expected an error, got {other:?}"),
        }
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

    // `CHT-2b`: cancellable before the first byte, same as `stream_turn` — and
    // on the same clock, so a provider that never answers ends the turn here
    // instead of holding it open.
    let Some(sent) = until_cancelled(&cancel, tokio::time::timeout(RESPONSE_TIMEOUT, req.send())).await
    else {
        on_event(StreamEvent::Cancelled);
        return Ok(());
    };
    let sent = match sent {
        Ok(s) => s,
        Err(_) => {
            let err = stalled(RESPONSE_TIMEOUT.as_secs(), "never started answering");
            on_event(StreamEvent::Error { message: err.to_string() });
            return Err(err);
        }
    };
    let resp = match sent {
        Ok(r) => r,
        Err(e) => {
            on_event(StreamEvent::Error {
                message: e.to_string(),
            });
            return Err(e.into());
        }
    };

    if resp.status().is_client_error() || resp.status().is_server_error() {
        let err = api_error(resp).await;
        on_event(StreamEvent::Error {
            message: format!("engine returned an error: {err}"),
        });
        return Err(err);
    }

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();

    while let Some(chunk) = match until_cancelled(
        &cancel,
        tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next()),
    )
    .await
    {
        Some(Ok(c)) => c,
        Some(Err(_)) => {
            let err = stalled(STREAM_IDLE_TIMEOUT.as_secs(), "stopped sending anything");
            on_event(StreamEvent::Error { message: err.to_string() });
            return Err(err);
        }
        None => {
            on_event(StreamEvent::Cancelled);
            return Ok(());
        }
    } {
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
