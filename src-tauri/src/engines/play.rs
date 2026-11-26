//! What every driver does with the events it understood: show them on the Fleet
//! card, keep them for the child's transcript, and give open questions a moment
//! to withdraw. Claude Code and the ACP agents share it.

use std::path::Path;

use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::Value;

use crate::agent::diff;
use crate::agent::fleet::{Steer, SteerSource};

use super::driver::{EditSeen, Job, StepRecord};

/// What an agent's event means for the screen.
#[derive(Debug, Clone, PartialEq)]
pub enum Ev {
    /// A whole block of words (Claude Code).
    Text(String),
    /// A piece of words as they stream in (ACP). `fresh` starts a new paragraph:
    /// it is the first piece since a step.
    Delta { text: String, fresh: bool },
    Thinking(String),
    StepStart { id: String, verb: String, target: String },
    StepDone { id: String, result: Option<String> },
    StepError { id: String, error: String },
    Session(String),
    /// A file edit, as the lines it added and removed (`PRF-1`). Comes right after
    /// the `StepStart` of the call that made it.
    Edit(EditSeen),
}

/// The added and removed lines of a change from `old` to `new`.
fn lines_changed(old: &str, new: &str) -> (Vec<String>, Vec<String>) {
    let (mut added, mut removed) = (Vec::new(), Vec::new());
    for h in diff::hunks(old, new) {
        for l in h.lines {
            match l.kind {
                diff::LineKind::Added => added.push(l.text),
                diff::LineKind::Removed => removed.push(l.text),
                diff::LineKind::Context => {}
            }
        }
    }
    (added, removed)
}

/// A path as the checks name it: inside the working folder it is relative, and
/// the parts are always split by `/`.
pub fn edit_path(path: &str, cwd: &Path) -> String {
    super::approve::shown_path(path, cwd).replace('\\', "/")
}

/// The edit a tool call makes, in the words of Claude Code's tools (the ACP driver
/// turns its own calls into these with `tool_of`): `Edit` (`old_string`,
/// `new_string`), `MultiEdit` (`edits` of the same) and `Write` (`content`, no old
/// text). Any other tool makes none.
pub fn edit_events(id: &str, tool: &str, input: &Value, cwd: &Path) -> Vec<Ev> {
    let Some(path) = input.get("file_path").and_then(|p| p.as_str()).filter(|p| !p.is_empty()) else { return Vec::new() };
    let text = |v: &Value, k: &str| v.get(k).and_then(|t| t.as_str()).unwrap_or("").to_string();
    let (added, removed) = match tool {
        "Edit" => lines_changed(&text(input, "old_string"), &text(input, "new_string")),
        "MultiEdit" => {
            let (mut added, mut removed) = (Vec::new(), Vec::new());
            for e in input.get("edits").and_then(|e| e.as_array()).into_iter().flatten() {
                let (a, r) = lines_changed(&text(e, "old_string"), &text(e, "new_string"));
                added.extend(a);
                removed.extend(r);
            }
            (added, removed)
        }
        "Write" => lines_changed("", &text(input, "content")),
        _ => return Vec::new(),
    };
    vec![Ev::Edit(EditSeen { id: id.to_string(), path: edit_path(path, cwd), added, removed })]
}

/// Send what a parsed event means to the child's sink, and keep it in the trace.
pub fn play(job: &Job<'_>, events: Vec<Ev>) {
    for ev in events {
        match ev {
            Ev::Text(t) => {
                let mut said = job.trace.said.lock().unwrap();
                if !said.is_empty() {
                    said.push_str("\n\n");
                }
                said.push_str(t.trim());
                drop(said);
                job.sink.token(&t);
            }
            Ev::Delta { text, fresh } => {
                let mut said = job.trace.said.lock().unwrap();
                let lead = fresh && !said.is_empty();
                if lead {
                    said.push_str("\n\n");
                }
                said.push_str(&text);
                drop(said);
                job.sink.token(&if lead { format!("\n\n{text}") } else { text });
            }
            Ev::Thinking(t) => job.sink.thinking(&job.run.id, &t),
            Ev::StepStart { id, verb, target } => {
                job.run.bump_steps();
                job.trace.steps.lock().unwrap().push(StepRecord {
                    id: id.clone(),
                    verb: verb.clone(),
                    target: target.clone(),
                    status: "running",
                    result: None,
                });
                job.sink.step_start(&id, &verb, &target);
            }
            Ev::StepDone { id, result } => {
                settle(job, &id, "done", result.clone());
                job.sink.step_done(&id, result);
            }
            Ev::StepError { id, error } => {
                // A refused or failed edit changed nothing.
                job.trace.edits.lock().unwrap().retain(|e| e.id != id);
                settle(job, &id, "error", Some(format!("— {error}")));
                job.sink.step_error(&id, &error);
            }
            Ev::Session(id) => (job.session_seen)(&id),
            Ev::Edit(edit) => {
                let mut edits = job.trace.edits.lock().unwrap();
                // A call can be reported more than once: it counts once.
                if !edits.iter().any(|e| e.id == edit.id && e.path == edit.path) {
                    edits.push(edit);
                }
            }
        }
    }
}

fn settle(job: &Job<'_>, id: &str, status: &'static str, result: Option<String>) {
    if let Some(step) = job.trace.steps.lock().unwrap().iter_mut().find(|s| s.id == id) {
        step.status = status;
        step.result = result;
    }
}

pub fn keep_tail(job: Option<&Job<'_>>, tail: &str) {
    if let Some(j) = job {
        *j.trace.stderr.lock().unwrap() = tail.to_string();
    }
}

/// Keep what the agent said about the plan, so a failed turn still has it.
pub fn keep_plan(job: Option<&Job<'_>>, usage: Option<&super::limits::PlanUsage>) {
    if let (Some(j), Some(u)) = (job, usage) {
        *j.trace.plan_usage.lock().unwrap() = Some(u.clone());
    }
}

pub fn clip(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        t.chars().take(max).collect::<String>() + "…"
    }
}

/// The questions an agent has open right now.
pub type Asks<'a, T = ()> = FuturesUnordered<std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>>;

/// Give questions that are still open a moment to withdraw from the panel.
pub async fn flush<T>(asks: &mut Asks<'_, T>) {
    let _ = tokio::time::timeout(std::time::Duration::from_millis(800), async { while asks.next().await.is_some() {} }).await;
}

/// What a message typed while the agent works says to it. The lead's words are
/// marked, so the agent can tell an instruction from the user's own message.
pub fn steer_text(steer: &Steer) -> String {
    match steer.from {
        SteerSource::User | SteerSource::Team => steer.text.clone(),
        SteerSource::Lead => format!("New instruction: {}", steer.text),
    }
}
