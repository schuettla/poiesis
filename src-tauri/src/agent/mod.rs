//! Agentic loop & built-in skills (PRD §7.5, §4.3). The backend owns the loop:
//! it inspects each model turn for tool calls, executes them (with permission),
//! and feeds results back until the model produces a final answer — emitting a
//! visible timeline of steps as it goes (CHT-9).

pub mod artifacts;
pub mod codeexec;
pub mod filesystem;
pub mod imagegen;
pub mod present;
pub mod run;
pub mod sandbox;
pub mod skills;
pub mod websearch;

use serde::Serialize;

use crate::permissions::PermissionRequest;

/// Events streamed to the UI during an agent run. The frontend renders Step*
/// events as the timeline and Token events as the prose conclusion.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// A tool step began — render a running timeline row.
    StepStart { id: String, verb: String, target: String },
    /// A tool step finished — settle the row and show its result.
    StepDone { id: String, result: Option<String> },
    /// A tool step failed.
    StepError { id: String, error: String },
    /// A chunk of the final assistant prose.
    Token { text: String },
    /// The assistant emitted an artifact to render in the Canvas panel (CHT-6).
    Artifact {
        id: String,
        title: String,
        kind: String,
        content: String,
    },
    /// The assistant emitted a typed, interactive workspace block to render inline
    /// in the assistant turn (Generative UI). `message_id` anchors it to the
    /// assistant message it belongs to; may be null if that row wasn't persisted.
    Block {
        id: String,
        message_id: Option<String>,
        kind: String,
        title: String,
        data: serde_json::Value,
    },
    /// An in-place update of an existing block (e.g. mark plan steps done, refresh
    /// a progress meter). The renderer patches the block matching `id`.
    BlockUpdate {
        id: String,
        title: String,
        data: serde_json::Value,
    },
    /// The per-conversation durable session state changed (Phase C `remember`).
    StateUpdate { state: serde_json::Value },
    /// The agent needs a capability — show the consent side panel (§5.4.4).
    Permission { request: PermissionRequest },
    /// The run completed normally.
    Done,
    /// The run was cancelled by the user.
    Cancelled,
    /// The run ended with an error.
    Error { message: String },
}
