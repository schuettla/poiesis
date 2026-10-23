//! Claude Code, driven headless over stream-json (`CLI-7`).
//!
//! `claude -p --input-format stream-json --output-format stream-json`: the task
//! goes in on stdin as one `user` line, and events come back one JSON object per
//! line until a `result`. Recorded against 2.1.301 (`fixtures/engines/claude-*`).
//!
//! Two shapes of turn:
//!
//! - **Try it** (`job` is `None`): no prompt tool, so Claude itself refuses
//!   anything that needs permission, and `--tools ""` takes the tools away.
//! - **A delegated job**: the permission mode follows the folder's trust, and
//!   every question Claude has goes to Poiesis's `approve` tool on the loopback
//!   MCP endpoint (`MCPS-1`), which puts it on the permission panel. The task
//!   goes over stdin, never in argv; the fixed instructions and the MCP address
//!   (which carries a token) go in files in a private scratch folder that is
//!   deleted when the turn ends.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use async_trait::async_trait;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::{json, Value};

use crate::agent::toolrpc::{self, McpCall};
use crate::agent::AgentEvent;
use crate::permissions::Trust;
use crate::runtime::proxy::{CancelFlag, Usage};

use super::approve;
use super::driver::{
    Approvals, Capabilities, DriverError, EngineDriver, ExternalOutcome, ExternalTurn, Job, Launch, ModelChoice,
};
use super::play::{clip, flush, keep_tail, play, steer_text, Asks, Ev};
use super::tools;
use super::process::{self, Wait, Wire};
use super::{EngineKind, PaidBy};

pub struct ClaudeDriver;

#[async_trait]
impl EngineDriver for ClaudeDriver {
    fn kind(&self) -> EngineKind {
        EngineKind::ClaudeCode
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { resume: true, steer_mid_turn: true, interrupt: true, mcp_tools: true, approvals: Approvals::PromptTool }
    }

    /// Claude Code takes an alias for the latest model of a family; its help
    /// names `sonnet` and `opus`. It cannot list models, so these are the two
    /// it documents.
    async fn list_models(&self, _launch: &Launch) -> Result<Vec<ModelChoice>, DriverError> {
        Ok(vec![
            ModelChoice { id: "sonnet".into(), name: "Sonnet (latest)".into(), is_default: false },
            ModelChoice { id: "opus".into(), name: "Opus (latest)".into(), is_default: false },
        ])
    }

    async fn run_turn(&self, turn: ExternalTurn<'_>) -> Result<ExternalOutcome, DriverError> {
        let resume = turn.job.as_ref().and_then(|j| j.resume.clone());
        match attempt(&turn, resume.as_deref()).await {
            // The session it was asked to continue is gone: start again, once,
            // and say so. Never a second silent retry (`TRM-4`).
            Err(DriverError::Failed(m)) if resume.is_some() && m.contains("No conversation found") => {
                let mut out = attempt(&turn, None).await?;
                out.resume_failed = true;
                Ok(out)
            }
            other => other,
        }
    }
}

/// A private folder for the files one turn needs. Gone when the turn is.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, DriverError> {
        let dir = std::env::temp_dir().join(format!("poiesis-engine-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).map_err(|e| DriverError::Start(format!("I couldn't make a scratch folder: {e}")))?;
        Ok(Scratch(dir))
    }

    fn write(&self, name: &str, text: &str) -> Result<PathBuf, DriverError> {
        let path = self.0.join(name);
        std::fs::write(&path, text).map_err(|e| DriverError::Start(format!("I couldn't write {name}: {e}")))?;
        Ok(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How a folder's trust becomes Claude's own permission mode (`CLI-6`).
/// Never `bypassPermissions`.
pub fn permission_mode(trust: Trust) -> &'static str {
    match trust {
        Trust::ReadOnly => "plan",
        Trust::Confirm => "default",
        Trust::Auto => "acceptEdits",
    }
}

fn user_line(text: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}})
}

async fn attempt(turn: &ExternalTurn<'_>, resume: Option<&str>) -> Result<ExternalOutcome, DriverError> {
    let launch = turn.launch;
    let job = turn.job.as_ref();
    let scratch = Scratch::new()?;

    let mode = job.map_or("default", |j| permission_mode(j.trust));
    let mut args: Vec<String> = ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--permission-mode", mode]
        .map(String::from)
        .to_vec();

    // MCPS-1: the loopback address the approve tool lives at. The ticket is
    // revoked when this function ends, however it ends.
    let (mcp_tx, mut mcp_rx) = tokio::sync::mpsc::unbounded_channel::<McpCall>();
    let mut _ticket = None;
    let mut mcp_open = false;
    if let Some(j) = job {
        let kit = toolrpc::McpKit { approve: true, memory: j.tools.is_some_and(|t| t.memory) };
        let ticket = toolrpc::arm_mcp_with(mcp_tx, &j.run.id, kit, j.tools.map(|t| t.app_specs()).unwrap_or_default())
            .ok_or_else(|| DriverError::Start("My local helper port isn't running, so I can't ask you about what it wants to do.".into()))?;
        let url = toolrpc::mcp_url(&ticket).ok_or_else(|| DriverError::Start("I have no local address for the helper.".into()))?;
        let config = scratch.write("mcp.json", &json!({"mcpServers": {"poiesis": {"type": "http", "url": url}}}).to_string())?;
        let system = scratch.write("append.txt", &j.system_append)?;
        args.extend([
            "--strict-mcp-config".to_string(),
            "--mcp-config".to_string(),
            config.to_string_lossy().to_string(),
            "--permission-prompt-tool".to_string(),
            "mcp__poiesis__approve".to_string(),
            "--append-system-prompt-file".to_string(),
            system.to_string_lossy().to_string(),
        ]);
        _ticket = Some(ticket);
        mcp_open = true;
    } else {
        // Only MCP servers we name. None are named, so the user's own servers
        // do not load and slow the start.
        args.push("--strict-mcp-config".into());
        if turn.tool_free {
            args.extend(["--tools".to_string(), String::new()]);
        }
    }
    if let Some(id) = resume {
        args.extend(["--resume".to_string(), id.to_string()]);
    }
    if let Some(m) = &launch.model {
        args.extend(["--model".to_string(), m.clone()]);
    }
    args.extend(launch.extra_args.iter().cloned());

    let cmd = process::command(&launch.program, &args, &launch.env).map_err(DriverError::Start)?;
    let mut wire = Wire::start(cmd, Some(&turn.cwd)).map_err(DriverError::Start)?;
    let deadline = Instant::now() + turn.timeout;

    if let Err(e) = wire.send(&user_line(&turn.task)).await {
        let tail = wire.tail.text();
        wire.finish().await;
        return Err(DriverError::Start(if tail.is_empty() { e } else { format!("{e}\n{tail}") }));
    }

    let mut out = ExternalOutcome::default();
    let mut live = Live::new(&turn.cwd);
    let mut result: Option<Value> = None;
    let mut cancelled = false;
    let mut asks: Asks<'_> = FuturesUnordered::new();

    loop {
        tokio::select! {
            biased;
            call = mcp_rx.recv(), if mcp_open => match call {
                Some(call) => {
                    if let Some(j) = job {
                        asks.push(Box::pin(answer(j, call, &turn.cancel)));
                    }
                }
                None => mcp_open = false,
            },
            Some(()) = asks.next(), if !asks.is_empty() => {}
            w = wire.next_tick(&turn.cancel, deadline) => match w {
                None => {
                    if let Some(j) = job {
                        forward_steers(&mut wire, j).await;
                    }
                }
                Some(Wait::Message(m)) => {
                    let (done, events) = handle_event(&m, &mut out, &mut live);
                    if let Some(j) = job {
                        play(j, events);
                    }
                    if done {
                        result = Some(m);
                        break;
                    }
                }
                Some(Wait::Closed) => break,
                Some(Wait::Cancelled) => {
                    cancelled = true;
                    // Ask it to stop. It answers within about 50 ms, rejects the
                    // question it was waiting on and closes that connection.
                    let _ = wire
                        .send(&json!({"type": "control_request", "request_id": "poiesis-interrupt", "request": {"subtype": "interrupt"}}))
                        .await;
                    break;
                }
                Some(Wait::TimedOut) => {
                    out.stderr_tail = wire.tail.text();
                    keep_tail(job, &out.stderr_tail);
                    flush(&mut asks).await;
                    wire.finish().await;
                    return Err(DriverError::TimedOut);
                }
            }
        }
    }
    // Let a question that was on the panel withdraw itself before we go.
    flush(&mut asks).await;
    drop(asks);

    out.unknown_events += wire.unknown + wire.dropped_lines();
    out.stderr_tail = wire.tail.text();
    keep_tail(job, &out.stderr_tail);
    out.self_allowed = live.self_allowed;
    let code = wire.finish().await;

    if cancelled {
        return Err(DriverError::Cancelled);
    }
    let Some(result) = result else {
        let tail = out.stderr_tail.lines().last().unwrap_or("").to_string();
        return Err(DriverError::Exited(match (code, tail.is_empty()) {
            (Some(c), true) => format!(" (code {c})."),
            (Some(c), false) => format!(" (code {c}): {tail}"),
            (None, _) => ".".into(),
        }));
    };
    finish(result, out)
}

/// One call on the endpoint. `approve` is put to the user (or refused) and
/// answered with the JSON Claude Code reads; one of Poiesis's own tools
/// (`MCPS-2`) is answered with its text.
async fn answer(job: &Job<'_>, call: McpCall, stop: &CancelFlag) {
    if call.tool != "approve" {
        tools::serve(job.tools, call, stop).await;
        return;
    }
    let tool = call.args.get("tool_name").and_then(|t| t.as_str()).unwrap_or("").to_string();
    let input = call.args.get("input").cloned().unwrap_or_else(|| json!({}));
    let verdict = if tool.is_empty() {
        approve::Verdict::Deny("Not allowed: the request did not say what it was for.".into())
    } else if job.tools.is_some() && tools::short_name(&tool).is_some() {
        // Claude asks before it uses any MCP tool. The user switched these on and
        // each one only suggests or shows, so the question would be noise.
        approve::Verdict::Allow
    } else {
        job.port.decide(&tool, &input, &call.gone, stop).await
    };
    let _ = call.reply.send(verdict.to_claude_json(&input));
}

/// Pass on what the user or the lead typed while it works. Claude folds a second
/// user message into the turn it is on (finding 12).
async fn forward_steers(wire: &mut Wire, job: &Job<'_>) {
    for steer in job.run.drain_steers() {
        if wire.send(&user_line(&steer_text(&steer))).await.is_ok() {
            job.sink.emit(AgentEvent::Steered { run_id: job.run.id.clone(), text: steer.text });
        }
    }
}

/// Per-turn reading state.
pub struct Live {
    cwd: PathBuf,
    pub self_allowed: u32,
    /// `tool_use` id → tool name, so a finished step can be told apart.
    tools: HashMap<String, String>,
}

impl Live {
    pub fn new(cwd: &Path) -> Self {
        Live { cwd: cwd.to_path_buf(), self_allowed: 0, tools: HashMap::new() }
    }
}

/// The readable text of a `tool_result`'s content (a string, or text blocks).
fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Fold one event into the outcome and say what it means for the screen.
/// `true` = this was the final `result`.
pub fn handle_event(m: &Value, out: &mut ExternalOutcome, live: &mut Live) -> (bool, Vec<Ev>) {
    let mut ev = Vec::new();
    match m.get("type").and_then(|t| t.as_str()) {
        Some("system") => {
            // The user's own SessionStart hooks send `hook_started` and
            // `hook_response` here too. Only `init` carries what we need.
            if m.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                out.session = m.get("session_id").and_then(|s| s.as_str()).map(String::from);
                out.model = m.get("model").and_then(|s| s.as_str()).map(String::from);
                out.paid_by = Some(match m.get("apiKeySource").and_then(|s| s.as_str()) {
                    Some("none") | None => PaidBy::Plan,
                    Some(_) => PaidBy::ApiKey,
                });
                if let Some(id) = &out.session {
                    ev.push(Ev::Session(id.clone()));
                }
            }
        }
        Some("assistant") => {
            for block in m.pointer("/message/content").and_then(|c| c.as_array()).into_iter().flatten() {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()) {
                            ev.push(Ev::Text(t.to_string()));
                        }
                    }
                    Some("thinking") => {
                        if let Some(t) = block.get("thinking").and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()) {
                            ev.push(Ev::Thinking(t.to_string()));
                        }
                    }
                    Some("tool_use") => {
                        let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                        let (verb, target) = approve::describe_tool(name, block.get("input").unwrap_or(&Value::Null), &live.cwd);
                        live.tools.insert(id.clone(), name.to_string());
                        ev.push(Ev::StepStart { id, verb, target });
                    }
                    _ => {}
                }
            }
        }
        Some("user") => {
            // `tool_result_meta` says who allowed each tool. `config` means
            // Claude's own settings let it through and we were never asked.
            for meta in m.get("tool_result_meta").and_then(|a| a.as_array()).into_iter().flatten() {
                let accepted = meta.pointer("/permission_decision/decision").and_then(|d| d.as_str()) == Some("accept");
                if accepted && meta.pointer("/permission_decision/source").and_then(|d| d.as_str()) == Some("config") {
                    live.self_allowed += 1;
                }
            }
            for block in m.pointer("/message/content").and_then(|c| c.as_array()).into_iter().flatten() {
                if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                    continue;
                }
                let id = block.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                if live.tools.remove(&id).is_none() {
                    continue;
                }
                let text = clip(&result_text(block.get("content").unwrap_or(&Value::Null)), 2000);
                if block.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false) {
                    ev.push(Ev::StepError { id, error: if text.is_empty() { "it failed".into() } else { text } });
                } else {
                    ev.push(Ev::StepDone { id, result: (!text.is_empty()).then_some(text) });
                }
            }
        }
        Some("rate_limit_event") | Some("stream_event") | Some("control_response") => {}
        Some("result") => return (true, ev),
        _ => out.unknown_events += 1,
    }
    (false, ev)
}

fn finish(result: Value, mut out: ExternalOutcome) -> Result<ExternalOutcome, DriverError> {
    let is_error = result.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false)
        || result.get("subtype").and_then(|s| s.as_str()).is_some_and(|s| s != "success");
    if is_error {
        let errors: Vec<String> = result
            .get("errors")
            .and_then(|e| e.as_array())
            .map(|a| a.iter().filter_map(|e| e.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let msg = if !errors.is_empty() {
            errors.join(" ")
        } else {
            result.get("result").and_then(|r| r.as_str()).unwrap_or("It reported an error without saying why.").to_string()
        };
        let signed_out = msg.to_ascii_lowercase().contains("log in") || msg.to_ascii_lowercase().contains("login");
        return Err(if signed_out { DriverError::NeedsSignIn(msg) } else { DriverError::Failed(msg) });
    }
    out.text = result.get("result").and_then(|r| r.as_str()).unwrap_or_default().to_string();
    if out.session.is_none() {
        out.session = result.get("session_id").and_then(|s| s.as_str()).map(String::from);
    }
    out.stop_reason = result.get("stop_reason").and_then(|s| s.as_str()).unwrap_or("end_turn").to_string();
    out.notional_cost_usd = result.get("total_cost_usd").and_then(|c| c.as_f64());
    if let Some(u) = result.get("usage") {
        let n = |k: &str| u.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
        out.usage = Some(Usage {
            prompt_tokens: n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens"),
            output_tokens: n("output_tokens"),
        });
    }
    out.denials = result
        .get("permission_denials")
        .and_then(|d| d.as_array())
        .map(|a| a.iter().filter_map(|d| d.get("tool_name").and_then(|t| t.as_str()).map(String::from)).collect())
        .unwrap_or_default();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replay the recorded session (`CLI-T-1`): the same events the real CLI
    /// sent are folded into the outcome and the screen events.
    fn replay(file: &str) -> (Result<ExternalOutcome, DriverError>, Vec<Ev>) {
        let mut out = ExternalOutcome::default();
        let mut live = Live::new(Path::new("C:\\proj"));
        let mut result = None;
        let mut events = Vec::new();
        for line in file.lines().filter(|l| !l.trim().is_empty()) {
            let row: Value = serde_json::from_str(line).unwrap();
            if row["dir"] != "out" || row.get("ch").is_some() {
                continue;
            }
            let (done, ev) = handle_event(&row["msg"], &mut out, &mut live);
            events.extend(ev);
            if done {
                result = Some(row["msg"].clone());
                break;
            }
        }
        out.self_allowed = live.self_allowed;
        (finish(result.expect("the recording has a result"), out), events)
    }

    #[test]
    fn the_recorded_session_ends_with_a_report_cost_usage_and_the_refused_tool() {
        let (out, _) = replay(include_str!("../../../fixtures/engines/claude-2.1.301.jsonl"));
        let out = out.unwrap();
        assert!(out.text.contains("Step 3 was refused"), "{}", out.text);
        assert_eq!(out.paid_by, Some(PaidBy::Plan));
        assert!(out.session.is_some());
        assert!(out.notional_cost_usd.unwrap() > 0.0);
        let usage = out.usage.unwrap();
        assert!(usage.prompt_tokens > 0 && usage.output_tokens > 0);
        assert_eq!(out.denials, vec!["Bash"], "the denied mkdir");
        assert_eq!(out.stop_reason, "end_turn");
        assert_eq!(out.unknown_events, 0, "hooks, rate limits and tool events are all known");
    }

    /// `CLI-T-1`: the screen events the recording produces, in order.
    #[test]
    fn the_recording_plays_as_steps_with_the_timelines_words() {
        let (_, events) = replay(include_str!("../../../fixtures/engines/claude-2.1.301.jsonl"));
        assert!(matches!(events.first(), Some(Ev::Session(_))), "the session id comes first");
        let starts: Vec<(&str, &str)> = events
            .iter()
            .filter_map(|e| match e {
                Ev::StepStart { verb, target, .. } => Some((verb.as_str(), target.as_str())),
                _ => None,
            })
            .collect();
        assert!(starts.iter().any(|(v, t)| *v == "wrote" && t.ends_with("hello.txt")), "{starts:?}");
        assert!(starts.iter().any(|(v, t)| *v == "ran" && t.starts_with("mkdir")), "{starts:?}");
        // Every step that started also ended, and the refused one is an error.
        let started = events.iter().filter(|e| matches!(e, Ev::StepStart { .. })).count();
        let ended = events.iter().filter(|e| matches!(e, Ev::StepDone { .. } | Ev::StepError { .. })).count();
        assert_eq!(started, ended, "{events:?}");
        assert!(events.iter().any(|e| matches!(e, Ev::StepError { error, .. } if error.contains("declined"))), "{events:?}");
        assert!(events.iter().any(|e| matches!(e, Ev::Text(t) if t.contains("Step 3"))));
    }

    /// Finding 5: a tool Claude's own settings let through is counted.
    #[test]
    fn a_step_claudes_own_settings_allowed_is_counted() {
        let mut out = ExternalOutcome::default();
        let mut live = Live::new(Path::new("C:\\proj"));
        let tool_use = json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"echo first"}}]}});
        handle_event(&tool_use, &mut out, &mut live);
        let result = json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"first"}]},
            "tool_result_meta":[{"id":"t1","permission_decision":{"decision":"accept","source":"config"}}]});
        let (_, ev) = handle_event(&result, &mut out, &mut live);
        assert_eq!(live.self_allowed, 1);
        assert_eq!(ev, vec![Ev::StepDone { id: "t1".into(), result: Some("first".into()) }]);
        // One we answered ourselves is not counted.
        let ours = json!({"type":"user","message":{"content":[]},
            "tool_result_meta":[{"id":"t2","permission_decision":{"decision":"accept","source":"user_temporary"}}]});
        handle_event(&ours, &mut out, &mut live);
        assert_eq!(live.self_allowed, 1);
    }

    #[test]
    fn an_unknown_session_is_a_clear_failure_not_a_hang() {
        let (err, _) = replay(include_str!("../../../fixtures/engines/claude-2.1.301-resume-missing.jsonl"));
        match err.unwrap_err() {
            DriverError::Failed(m) => assert!(m.contains("No conversation found"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_interrupted_run_is_not_reported_as_a_success() {
        // The recording ends in `error_during_execution`: it is an error result.
        assert!(replay(include_str!("../../../fixtures/engines/claude-2.1.301-interrupt.jsonl")).0.is_err());
    }

    #[test]
    fn the_folders_trust_picks_claudes_own_mode_and_never_skips_permissions() {
        assert_eq!(permission_mode(Trust::ReadOnly), "plan");
        assert_eq!(permission_mode(Trust::Confirm), "default");
        assert_eq!(permission_mode(Trust::Auto), "acceptEdits");
        for t in [Trust::ReadOnly, Trust::Confirm, Trust::Auto] {
            assert_ne!(permission_mode(t), "bypassPermissions");
        }
    }

    #[test]
    fn a_tool_result_can_be_a_string_or_text_blocks() {
        assert_eq!(result_text(&json!("plain")), "plain");
        assert_eq!(result_text(&json!([{"type":"text","text":"a"},{"type":"image"},{"type":"text","text":"b"}])), "a\nb");
        assert_eq!(result_text(&Value::Null), "");
    }
}
