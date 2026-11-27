//! `DBG-2`: everything recorded about one chat, gathered for the person who is looking
//! for a fault or checking how a run went.
//!
//! Reads only. Nothing here goes to a model, and nothing leaves the computer unless the
//! person copies or saves it. Long texts are cut in the report; one log row can be read
//! whole with [`event`]. Embedded files (`data:` URLs) are always left out.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

use crate::db::decisions::DecisionRow;
use crate::db::inspect::{RunUsageRow, SkillRunRow, ToolCount};
use crate::db::{ActivityEntry, Conversation, Db, DbError, Post, Project, SessionEvent, SubagentRun, ToolFix};

/// How much of a tool's arguments or result a run lists.
const CLIP_CALL: usize = 600;
/// How much of a row or a message the lists show.
const CLIP_LINE: usize = 200;

/// What only the app itself knows about where it runs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Env {
    pub app_version: String,
    pub os: String,
    pub data_dir: String,
}

#[derive(Debug, Serialize)]
pub struct SessionReport {
    pub generated_at: i64,
    pub schema_version: i64,
    #[serde(flatten)]
    pub env: Env,
    pub conversation: Conversation,
    /// The session id of the agent app this chat runs on, when it has one.
    pub engine_session: Option<String>,
    pub project: Option<Project>,
    /// The folder the chat works in, whichever of chat or project sets it.
    pub folder: Option<String>,
    pub folder_trust: String,
    pub totals: Totals,
    pub runs: Vec<RunReport>,
    pub messages: Vec<MessageReport>,
    pub events: Vec<EventReport>,
    pub helpers: Vec<SubagentRun>,
    pub posts: Vec<Post>,
    pub decisions: Vec<DecisionRow>,
    pub tool_counts: Vec<ToolCount>,
    pub tool_fixes: Vec<ToolFix>,
    pub skills: Vec<SkillRunRow>,
    pub activity: Vec<ActivityEntry>,
    /// The reports of the chats below this one. Only in a full report.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SessionReport>,
}

#[derive(Debug, Default, Serialize)]
pub struct Totals {
    pub user_messages: u32,
    pub assistant_messages: u32,
    pub runs: usize,
    pub events: usize,
    pub tool_calls: usize,
    pub tool_errors: usize,
    pub prompt_tokens: u64,
    pub output_tokens: u64,
    /// At list prices, over the runs a price is known for. `None` when none is.
    pub cost_usd: Option<f64>,
    /// Tokens of the chats below this one, not counted above.
    pub helper_tokens: u64,
    pub artifacts: u32,
    pub files_changed: u32,
    pub compactions: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct RunReport {
    pub run_id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    /// Why it stopped. `None` while it runs, or when it was cut off with the app.
    pub stop_reason: Option<String>,
    /// Still working now. Filled in by the command, which holds the live runs.
    pub live: bool,
    pub steps: Option<u64>,
    pub model: Option<String>,
    pub provenance: Option<String>,
    pub cost_usd: Option<f64>,
    pub usage: Vec<RunUsageRow>,
    /// The `setup` row. `None` for a run made before it was recorded.
    pub setup: Option<Value>,
    /// The row holding what the model was first sent.
    pub prompt_seq: Option<i64>,
    pub prompt_messages: usize,
    pub prompt_chars: usize,
    pub tool_calls: Vec<ToolCallReport>,
    pub steers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolCallReport {
    pub id: String,
    pub name: String,
    pub args: String,
    /// `None` when no result was logged.
    pub ok: Option<bool>,
    pub result: Option<String>,
    pub call_seq: i64,
    pub result_seq: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct MessageReport {
    pub id: String,
    pub role: String,
    pub created_at: i64,
    pub model_name: Option<String>,
    pub model_provenance: Option<String>,
    pub stop_reason: Option<String>,
    pub spoken: bool,
    pub author: Option<String>,
    pub addressed: Option<String>,
    pub attachments: usize,
    pub chars: usize,
    pub preview: String,
    /// What of memory and instructions reached this answer.
    pub context: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct EventReport {
    pub seq: i64,
    pub at: i64,
    pub kind: String,
    pub run_id: Option<String>,
    pub bytes: usize,
    pub preview: String,
    /// The whole row. Only in a full report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

/// The report for one conversation. `full` adds every log row and message whole, and
/// the reports of the chats below. `None` when there is no such conversation.
pub fn report(db: &Db, env: &Env, conversation_id: &str, full: bool) -> Result<Option<SessionReport>, DbError> {
    build(db, env, conversation_id, full, &mut HashSet::new())
}

/// One log row as readable JSON, or `None` when there is no such row.
pub fn event(db: &Db, conversation_id: &str, seq: i64) -> Result<Option<String>, DbError> {
    Ok(db.session_event(conversation_id, seq)?.map(|row| {
        let value = payload(&row);
        serde_json::to_string_pretty(&value).unwrap_or(row.payload_json)
    }))
}

fn build(
    db: &Db,
    env: &Env,
    id: &str,
    full: bool,
    seen: &mut HashSet<String>,
) -> Result<Option<SessionReport>, DbError> {
    if !seen.insert(id.to_string()) {
        return Ok(None);
    }
    let Some(conversation) = db.get_conversation(id)? else { return Ok(None) };
    let rows = db.session_events(id)?;
    let run_of = db.session_event_runs(id)?;
    let usage = db.usage_rows_in(id)?;
    let card = db.session_card(id)?;
    let (folder, folder_trust) = db.conversation_folder(id)?;

    let runs = runs(&rows, &run_of, &usage);
    let events: Vec<EventReport> = rows
        .iter()
        .map(|row| {
            let value = payload(row);
            EventReport {
                seq: row.seq,
                at: row.created_at,
                kind: row.kind.clone(),
                run_id: run_of.get(&row.seq).cloned(),
                bytes: row.payload_json.len(),
                preview: clip(&line(&row.kind, &value), CLIP_LINE),
                payload: full.then_some(value),
            }
        })
        .collect();

    let mut messages = Vec::new();
    for m in db.list_messages(id)? {
        let context = db.message_context_json(&m.id)?.and_then(|s| serde_json::from_str(&s).ok());
        let parsed = |s: &Option<String>| -> Option<Value> {
            let mut v: Value = serde_json::from_str(s.as_deref()?).ok()?;
            elide(&mut v);
            Some(v)
        };
        messages.push(MessageReport {
            steps: full.then(|| parsed(&m.steps_json)).flatten(),
            plan: full.then(|| parsed(&m.plan_json)).flatten(),
            chars: m.content.chars().count(),
            preview: clip(&m.content, CLIP_LINE),
            content: full.then_some(m.content),
            attachments: m.attachments.len(),
            context,
            id: m.id,
            role: m.role,
            created_at: m.created_at,
            model_name: m.model_name,
            model_provenance: m.model_provenance,
            stop_reason: m.stop_reason,
            spoken: m.spoken,
            author: m.author,
            addressed: m.addressed,
        });
    }

    let mut helper_tokens = 0;
    let mut children = Vec::new();
    for child in db.child_conversation_ids(id)? {
        helper_tokens += db.usage_rows_in(&child)?.iter().map(|u| u.prompt_tokens + u.output_tokens).sum::<u64>();
        if full {
            children.extend(build(db, env, &child, full, seen)?);
        }
    }

    let calls = runs.iter().flat_map(|r| &r.tool_calls);
    let costs: Vec<f64> = runs.iter().filter_map(|r| r.cost_usd).collect();
    let totals = Totals {
        user_messages: card.user_messages,
        assistant_messages: card.assistant_messages,
        runs: runs.len(),
        events: events.len(),
        tool_calls: calls.clone().count(),
        tool_errors: calls.filter(|c| c.ok == Some(false)).count(),
        prompt_tokens: usage.iter().map(|u| u.prompt_tokens).sum(),
        output_tokens: usage.iter().map(|u| u.output_tokens).sum(),
        cost_usd: (!costs.is_empty()).then(|| costs.iter().sum()),
        helper_tokens,
        artifacts: card.artifact_count,
        files_changed: card.files_changed,
        compactions: rows.iter().filter(|r| r.kind == super::log::COMPACTION_KIND).count(),
    };

    Ok(Some(SessionReport {
        generated_at: crate::db::now_ms(),
        schema_version: crate::db::SCHEMA_VERSION,
        env: env.clone(),
        engine_session: db.conversation_engine_session(id),
        project: match &conversation.project_id {
            Some(p) => db.get_project(p)?,
            None => None,
        },
        folder,
        folder_trust,
        totals,
        runs,
        messages,
        events,
        helpers: db.list_subagent_runs(id)?,
        posts: db.list_posts(id)?,
        decisions: db.decisions_in(id)?,
        tool_counts: db.tool_counts_in(id)?,
        tool_fixes: db.tool_fixes_in(id)?,
        skills: db.skill_runs_in(id)?,
        activity: db.activity_in(id)?,
        children,
        conversation,
    }))
}

/// The runs of a conversation, oldest first: what the log says each did, and what each
/// reported spending. A job with usage and no log (an agent app's) is listed too.
fn runs(rows: &[SessionEvent], run_of: &HashMap<i64, String>, usage: &[RunUsageRow]) -> Vec<RunReport> {
    let mut runs: Vec<RunReport> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut slot = |runs: &mut Vec<RunReport>, id: &str, started: i64| -> usize {
        *at.entry(id.to_string()).or_insert_with(|| {
            runs.push(RunReport { run_id: id.to_string(), started_at: started, ..RunReport::default() });
            runs.len() - 1
        })
    };

    for row in rows {
        let Some(id) = run_of.get(&row.seq) else { continue };
        let i = slot(&mut runs, id, row.created_at);
        let run = &mut runs[i];
        let v = payload(row);
        match row.kind.as_str() {
            "prompt" => {
                let sent = v.as_array().map(Vec::as_slice).unwrap_or_default();
                run.prompt_seq = Some(row.seq);
                run.prompt_messages = sent.len();
                run.prompt_chars = row.payload_json.len();
            }
            super::log::SETUP_KIND => run.setup = Some(v),
            "tool_call" => {
                for c in v["tool_calls"].as_array().into_iter().flatten() {
                    run.tool_calls.push(ToolCallReport {
                        id: text(&c["id"]),
                        name: text(&c["function"]["name"]),
                        args: clip(c["function"]["arguments"].as_str().unwrap_or_default(), CLIP_CALL),
                        ok: None,
                        result: None,
                        call_seq: row.seq,
                        result_seq: None,
                    });
                }
            }
            "tool_result" => {
                let call = text(&v["tool_call_id"]);
                let said = content(&v);
                // The last open call with this id: some models reuse ids across turns.
                if let Some(c) = run.tool_calls.iter_mut().rev().find(|c| c.id == call && c.result_seq.is_none()) {
                    c.ok = Some(!failed(&said));
                    c.result = Some(clip(&said, CLIP_CALL));
                    c.result_seq = Some(row.seq);
                }
            }
            "steer" => run.steers.push(text(&v["text"])),
            "stop" => {
                run.stop_reason = v["reason"].as_str().map(str::to_string);
                run.steps = v["steps"].as_u64();
                run.ended_at = Some(row.created_at);
            }
            _ => {}
        }
    }

    for u in usage {
        let started = u.created_at - u.duration_ms.unwrap_or(0);
        let i = slot(&mut runs, &u.run_id, started);
        runs[i].usage.push(u.clone());
    }

    for run in &mut runs {
        let setup = |key: &str| run.setup.as_ref().and_then(|s| s[key].as_str()).map(str::to_string);
        let billed = run.usage.last();
        run.model = billed.map(|u| u.model_name.clone()).or_else(|| setup("model"));
        run.provenance = billed.map(|u| u.provenance.clone()).or_else(|| setup("provenance"));
        run.steps = run.steps.or_else(|| billed.map(|u| u.turns));
        let costs: Vec<f64> = run
            .usage
            .iter()
            // A local run costs nothing to ask for, and an agent app's price is its own.
            .filter(|u| u.provenance != "local" && u.provenance != crate::db::AGENT_APP)
            .filter(|u| u.prompt_tokens + u.output_tokens > 0)
            .filter_map(|u| crate::cloud::pricing::cost_usd(&u.model_name, u.prompt_tokens, u.output_tokens))
            .collect();
        run.cost_usd = (!costs.is_empty()).then(|| costs.iter().sum());
    }
    runs.sort_by_key(|r| r.started_at);
    runs
}

/// A row's payload, embedded files left out. A row that will not parse is shown as text.
fn payload(row: &SessionEvent) -> Value {
    let mut v = serde_json::from_str(&row.payload_json).unwrap_or_else(|_| Value::String(row.payload_json.clone()));
    elide(&mut v);
    v
}

/// Replace every `data:` URL by its size: an image is megabytes of letters nobody reads.
fn elide(v: &mut Value) {
    match v {
        Value::String(s) if s.starts_with("data:") && s.len() > CLIP_LINE => {
            let kind = s.split([';', ',']).next().unwrap_or("data:").to_string();
            *s = format!("[{kind}: {} characters left out]", s.len());
        }
        Value::Array(items) => items.iter_mut().for_each(elide),
        Value::Object(map) => map.values_mut().for_each(elide),
        _ => {}
    }
}

/// One line that says what a row holds.
fn line(kind: &str, v: &Value) -> String {
    match kind {
        "prompt" => format!("{} messages sent to the model", v.as_array().map(Vec::len).unwrap_or(0)),
        "tool_call" => v["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| format!("{}({})", text(&c["function"]["name"]), text(&c["function"]["arguments"])))
            .collect::<Vec<_>>()
            .join(", "),
        "stop" => format!("{} after {} steps", text(&v["reason"]), v["steps"].as_u64().unwrap_or(0)),
        "steer" | super::log::COMPACTION_KIND => text(&v["text"]),
        super::log::SETUP_KIND => format!(
            "{}, {} tools, up to {} steps",
            text(&v["model"]),
            v["tools"].as_array().map(Vec::len).unwrap_or(0),
            v["max_steps"].as_u64().unwrap_or(0)
        ),
        _ => {
            let said = content(v);
            if said.is_empty() {
                v.to_string()
            } else {
                said
            }
        }
    }
}

/// A message's text, whether it is a string or parts.
fn content(v: &Value) -> String {
    match &v["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    }
}

/// The same reading of a result the log's own readers use, plus a script that did not
/// end well.
fn failed(result: &str) -> bool {
    result.trim_start().starts_with("Error")
        || result.contains("(exited with code")
        || result.starts_with("The code ran longer than")
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &s[..end]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chat() -> (Db, String) {
        let db = Db::open_in_memory().expect("in-memory db");
        let id = db.create_conversation("A chat", None, false).unwrap().id;
        (db, id)
    }

    fn write(db: &Db, conv: &str, run: &str, kind: &str, v: Value) -> i64 {
        db.append_session_event(conv, Some(run), kind, &v).unwrap()
    }

    #[test]
    fn a_run_says_how_it_was_set_up_what_it_called_and_how_it_ended() {
        let (db, conv) = chat();
        write(&db, &conv, "r1", "prompt", json!([{ "role": "system", "content": "s" }, { "role": "user", "content": "u" }]));
        write(&db, &conv, "r1", "setup", json!({ "model": "m", "tools": ["read_file"], "max_steps": 12 }));
        write(&db, &conv, "r1", "tool_call", json!({ "role": "assistant", "tool_calls": [
            { "id": "a", "function": { "name": "read_file", "arguments": "{}" } },
            { "id": "b", "function": { "name": "read_file", "arguments": "{}" } },
            { "id": "c", "function": { "name": "write_file", "arguments": "{}" } },
        ] }));
        write(&db, &conv, "r1", "tool_result", json!({ "role": "tool", "tool_call_id": "a", "content": "text" }));
        write(&db, &conv, "r1", "tool_result", json!({ "role": "tool", "tool_call_id": "b", "content": "Error: no such file" }));
        write(&db, &conv, "r1", "steer", json!({ "text": "stop soon" }));
        write(&db, &conv, "r1", "stop", json!({ "reason": "completed", "steps": 2 }));
        db.record_run_usage("r1", &conv, "m", "local", 100, 20, 2).unwrap();

        let r = report(&db, &Env::default(), &conv, false).unwrap().unwrap();
        let run = &r.runs[0];
        assert_eq!(r.runs.len(), 1);
        assert_eq!(run.stop_reason.as_deref(), Some("completed"));
        assert_eq!(run.steps, Some(2));
        assert_eq!(run.prompt_messages, 2);
        assert_eq!(run.setup.as_ref().unwrap()["max_steps"], 12);
        assert_eq!(run.tool_calls.iter().map(|c| c.ok).collect::<Vec<_>>(), [Some(true), Some(false), None]);
        assert_eq!(run.steers, ["stop soon"]);
        assert_eq!((r.totals.tool_calls, r.totals.tool_errors), (3, 1));
        assert_eq!((r.totals.prompt_tokens, r.totals.output_tokens, r.totals.cost_usd), (100, 20, None));
        assert!(r.events.iter().all(|e| e.payload.is_none()));
    }

    #[test]
    fn a_full_report_holds_the_helpers_and_never_the_files() {
        let (db, conv) = chat();
        let child = db.create_conversation("A helper", None, false).unwrap().id;
        db.set_conversation_parent(&child, &conv).unwrap();
        db.create_subagent_run("c1", &conv, None, &child, "general", "look").unwrap();
        let image = format!("data:image/png;base64,{}", "A".repeat(5000));
        let seq = write(&db, &conv, "r1", "prompt", json!([{ "role": "user", "content": [
            { "type": "image_url", "image_url": { "url": image } }
        ] }]));
        write(&db, &child, "c1", "stop", json!({ "reason": "error", "steps": 1 }));

        let r = report(&db, &Env::default(), &conv, true).unwrap().unwrap();
        let sent = serde_json::to_string(&r).unwrap();
        assert!(!sent.contains("AAAAAAAAAA"));
        assert!(sent.contains("characters left out"));
        assert_eq!(r.children.len(), 1);
        assert_eq!(r.children[0].runs[0].stop_reason.as_deref(), Some("error"));
        assert!(!event(&db, &conv, seq).unwrap().unwrap().contains("AAAAAAAAAA"));
        assert_eq!(event(&db, &conv, 999).unwrap(), None);
    }

    #[test]
    fn a_run_with_no_stop_row_has_no_reason() {
        let (db, conv) = chat();
        write(&db, &conv, "r1", "prompt", json!([]));

        let r = report(&db, &Env::default(), &conv, false).unwrap().unwrap();
        assert_eq!(r.runs[0].stop_reason, None);
        assert_eq!(r.runs[0].ended_at, None);
        assert!(report(&db, &Env::default(), "nobody", false).unwrap().is_none());
    }
}
