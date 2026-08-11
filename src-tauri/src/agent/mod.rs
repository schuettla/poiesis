//! Agentic loop & built-in toolsets (PRD §7.5, §4.3). The backend owns the loop:
//! it inspects each model turn for tool calls, executes them (with permission),
//! and feeds results back until the model produces a final answer — emitting a
//! visible timeline of steps as it goes (CHT-9).

pub mod artifacts;
pub mod browser;
pub mod codeexec;
pub mod duplicates;
pub mod filesystem;
pub mod golden;
pub mod imagegen;
pub mod index;
pub mod mail;
pub mod memory_skill;
pub mod phash;
pub mod present;
pub mod recall;
pub mod retrieval;
pub mod run;
pub mod sandbox;
pub mod screen;
pub mod skillpack;
pub mod toolsets;
pub mod trash;
pub mod untrusted;
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
    /// `meta_json` is the artifact's stored metadata — `None` for every kind
    /// but media, where it is what lets the stream render the same block the
    /// composer's direct path produces: provider, dimensions, cost (`STR-1`).
    Artifact {
        id: String,
        title: String,
        kind: String,
        content: String,
        meta_json: Option<String>,
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
    /// A durable self entry was written/updated/forgotten (MEM-6 / REF-3).
    /// `collection` is "facts" | "lessons".
    MemoryWrite {
        op: String,
        name: String,
        description: String,
        collection: String,
        /// For `forget`, the trash filename that undoes it via `restore_trash`.
        /// Empty for `save` (undo = forget the new entry) and other ops.
        undo_token: String,
    },
    /// Recall search results with provenance, for the expandable timeline step.
    Recall {
        id: String,
        matches: Vec<crate::db::SearchHit>,
    },
    /// The Code Execution toolset ran a snippet (`DAT-UI-1`): the source, for the
    /// timeline step's on-demand disclosure — the same `⌄` control `Recall`
    /// uses, so the code is available without ever being dumped into the
    /// answer itself.
    Code {
        id: String,
        language: String,
        code: String,
    },
    /// One piece of outside text was marked untrusted (`TRU-1`/`TRU-2`) and fed
    /// to the model wrapped, not refused. Hangs off the step matching `id` the
    /// same way `Code`/`Recall` do — a call that wraps more than one source
    /// (e.g. several retrieved file excerpts) emits one of these per source,
    /// and the UI accumulates them (`TRU-UI-1`).
    Untrusted {
        id: String,
        /// User-facing provenance, e.g. "email from bob@x.com", "page at
        /// example.com", "file README.md".
        label: String,
        /// 0–3, `untrusted::Scan::risk`.
        risk: u8,
        flags: Vec<String>,
        /// The raw (unwrapped) text, for the step's on-demand disclosure.
        text: String,
    },
    /// The agent proposed a self-change (SOUL-2 / RCP-2); `target` as in
    /// `change_proposals`. Never applied without the user saying yes.
    Proposal {
        id: String,
        target: String,
        rationale: String,
    },
    /// A file on disk changed. The Workbench marks the row, refreshes the branch
    /// it lives in, and adds an undo affordance. `undo_token` is a `file_trash`
    /// id; empty when the operation left nothing to reverse.
    FileChanged {
        op: String,
        path: String,
        undo_token: String,
    },
    /// The Browser toolset's live session changed (`BRW-UI-1`) — the panel
    /// replaces its state wholesale rather than patching, since every field
    /// (title, domain, screenshot, trail) can change on any one action.
    Browser { state: browser::BrowserPanelState },
    /// A message actually left the machine at the `auto` rung (`MAIL-3`) —
    /// there is no undo, so this is a receipt, not a write with an undo
    /// affordance like `MemoryWrite`. Not emitted for an `email` proposal's
    /// accept, which the disappearing card already announces.
    MailSent { to: String },
    /// The run completed normally.
    Done,
    /// The run was cancelled by the user.
    Cancelled,
    /// The run ended with an error.
    Error { message: String },
}
