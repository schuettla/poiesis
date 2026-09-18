//! Agentic loop & built-in toolsets (PRD §7.5, §4.3). The backend owns the loop:
//! it inspects each model turn for tool calls, executes them (with permission),
//! and feeds results back until the model produces a final answer — emitting a
//! visible timeline of steps as it goes (CHT-9).

pub mod artifacts;
pub mod background;
pub mod browser;
pub mod changes;
pub mod codeexec;
pub mod coderun;
pub mod context;
mod context_golden;
pub mod diagnostics;
pub mod diff;
pub mod duplicates;
pub mod filesystem;
pub mod fleet;
pub mod golden;
pub mod imagegen;
pub mod index;
pub mod ledger;
pub mod log;
pub mod mail;
pub mod memory_skill;
pub mod phash;
pub mod plan;
pub mod present;
pub mod preview;
pub mod project;
pub mod recall;
pub mod results;
pub mod retrieval;
pub mod run;
pub mod sandbox;
pub mod screen;
pub mod skillpack;
pub mod subagents;
pub mod symbols;
pub mod toolsets;
pub mod toolrpc;
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
    StepStart {
        id: String,
        verb: String,
        target: String,
        /// `RPC-1`: the step this one happened *inside*, when it was not the
        /// model that asked for it. A script running in the sandbox can make
        /// tool calls of its own, and forty of them arriving as ordinary
        /// timeline rows would read as forty things the model decided to do.
        /// Nested, they read as what they are: the work of the one step above.
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
    },
    /// A tool step finished — settle the row and show its result.
    StepDone { id: String, result: Option<String> },
    /// A tool step failed.
    StepError { id: String, error: String },
    /// A chunk of the final assistant prose.
    Token { text: String },
    /// A chunk of the model's *thinking*, which is not the answer (`HRN-UI-3`).
    ///
    /// Deliberately a separate event from `Token`, and it never reaches the
    /// message body. A reasoning model can spend minutes here before its first
    /// word of prose, and with nothing on this channel the run looked hung —
    /// the app's own timer ticking beside an empty screen.
    Thinking { run_id: String, text: String },
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
    /// `COD-UI-2`: a project task started. Hangs off the step matching `id`,
    /// like `Code`. `kind` is the task's `TaskKind`; a free-form command is
    /// `other`.
    TaskStarted {
        id: String,
        task: String,
        argv: Vec<String>,
        cwd: String,
        kind: project::TaskKind,
        timeout_secs: u64,
    },
    /// The latest line a running task printed, so a long build shows what it
    /// is doing. Throttled: not every line, just the newest one now and then.
    TaskOutput { id: String, line: String },
    /// A task finished, however it finished. The diagnostics come first in
    /// the step's disclosure, the raw tail below them (`COD-10`).
    TaskEnded {
        id: String,
        outcome: String,
        exit_code: Option<i32>,
        timed_out: bool,
        cancelled: bool,
        duration_ms: u64,
        diagnostics: Vec<diagnostics::Diagnostic>,
        tail: String,
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
    /// `HRN-4`/`HRN-UI-2`: these steps are running at the same time, in this
    /// order. Emitted before their `StepStart`s, so the timeline can group them
    /// as one "3 things at once" band rather than stacking them one by one and
    /// making concurrent work look sequential.
    StepsParallel { ids: Vec<String> },
    /// `HRN-8`: this step's output was too big to paste into the transcript, so
    /// it was kept on disk and the model got a preview plus a handle. The user
    /// gets the whole thing, because the point is that they can read what the
    /// model chose not to.
    KeptResult {
        /// The tool call id, so this hangs off that step like `Code` does.
        id: String,
        /// The handle the model was given, e.g. `res_7f3a`.
        reference: String,
        bytes: usize,
        text: String,
    },
    /// `SUB-2`: a delegated child run started. `index` is its position in the
    /// `delegate` call, so the Fleet card can order rows the way the lead asked
    /// for them rather than the way they happen to finish.
    SubSpawned {
        run_id: String,
        conversation_id: String,
        agent: String,
        task: String,
        index: usize,
    },
    /// One event from a child run, tagged with whose it is. Deliberately wraps
    /// rather than flattens: a permission prompt, an artifact or a browser
    /// state from a child reaches the UI already attributed, with no per-variant
    /// work and no chance of a child's step landing in the lead's own timeline.
    Sub { run_id: String, event: Box<AgentEvent> },
    /// A child finished. `status` is `done` | `stopped` | `error` (what the row
    /// says), `stop_reason` is why the loop ended (what the lead is told).
    SubEnded {
        run_id: String,
        status: String,
        stop_reason: String,
        summary: String,
        steps: usize,
        ms: u64,
    },
    /// The loop has a run id (`HRN-1`). Emitted once, before the first model
    /// call, so the UI can steer this run (`steer_run_cmd`) and size its meter.
    RunStarted {
        run_id: String,
        max_steps: usize,
        /// `OBS-3`: how much the model can hold. `None` when the provider does
        /// not say, in which case the meter shows no percentage rather than a
        /// made-up one.
        context_window: Option<usize>,
    },
    /// One iteration of the loop began (`HRN-UI-3`'s meter). `step` is
    /// 1-based; `ms` is elapsed wall clock for the whole run so far.
    RunProgress {
        run_id: String,
        step: usize,
        max_steps: usize,
        ms: u64,
        /// `OBS-3`: an estimate of what this turn is about to send, against
        /// `context_window`. An estimate, and named as one — the exact number
        /// is the provider's tokenizer's business.
        context_tokens: usize,
    },
    /// The loop finished, however it finished. Always emitted, immediately
    /// before the matching `Done`/`Cancelled`/`Error`, so the UI can tell a
    /// finished answer from what a run had in hand when its budget ran out.
    RunEnded {
        run_id: String,
        /// `completed` | `aborted` | `timeout` | `max_steps` | `error`.
        stop_reason: String,
        steps: usize,
        ms: u64,
        /// `OBS-1`: what the run cost, when the provider said. Null means
        /// unknown, which is not the same as free.
        usage: Option<crate::runtime::proxy::Usage>,
        /// `PLN-5`: the plan as it stood when the run ended. A run that stopped
        /// at its step cap can then say which items it never reached, which is
        /// the honest version of "I stopped at my step limit". `None` for a run
        /// that never wrote one.
        plan: Option<plan::Plan>,
    },
    /// `PLN-1`/`PLN-UI-1`: the run wrote or revised its plan. Sent on every
    /// change, carrying the whole plan rather than a patch — a plan is a handful
    /// of short strings, and a card that rebuilds itself from the current state
    /// cannot drift out of step with the model's copy the way a patched one can.
    Plan {
        run_id: String,
        plan: plan::Plan,
    },
    /// A mid-run instruction was picked up and is now part of the transcript
    /// (`HRN-2`). The UI settles its optimistic "sent mid-run" mark on this.
    Steered { run_id: String, text: String },
    /// The run completed normally.
    Done,
    /// The run was cancelled by the user.
    Cancelled,
    /// The run ended with an error.
    Error { message: String },
}
