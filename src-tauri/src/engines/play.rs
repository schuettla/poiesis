//! What every driver does with the events it understood: show them on the Fleet
//! card, keep them for the child's transcript, and give open questions a moment
//! to withdraw. Claude Code and the ACP agents share it.

use futures_util::stream::{FuturesUnordered, StreamExt};

use crate::agent::fleet::{Steer, SteerSource};

use super::driver::{Job, StepRecord};

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
                settle(job, &id, "error", Some(format!("— {error}")));
                job.sink.step_error(&id, &error);
            }
            Ev::Session(id) => (job.session_seen)(&id),
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
        SteerSource::User => steer.text.clone(),
        SteerSource::Lead => format!("New instruction: {}", steer.text),
    }
}
