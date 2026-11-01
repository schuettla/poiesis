# Project Poiesis - Harness Rework Plan (the turn moves to Rust)

**The agent decides in Rust. The window only shows it.**

A normal chat turn is still put together in the React store: `sendMessage` in
`src/lib/store.ts` (about 270 lines) builds the model's copy of the message,
recalls memory, composes the system prompt, fits history to the window and
compacts. Then `streamAssistantTurn` (about 760 lines) folds the event stream
into the saved timeline and finalizes the row. The Rust half of the prompt
already exists (`agent/context.rs`), and the CTX-4 gate keeps the two copies
byte-identical. Every prompt change is made twice.

This plan finishes that port. After it:

- one Rust entry point starts, runs and saves a turn, for a typed message, a
  resumed run and the eval harness alike;
- the TypeScript prompt code and the CTX-4 gate are gone;
- the saved timeline is built in Rust, so a run that never had a window
  (scheduled jobs, delegated children) can save one later with the same code;
- evals run the path users run.

It replaces `plans/TUI_PLAN.md` (2026-10-27). The terminal UI was dropped on
2026-11-01; nothing here prepares for it.

> ID prefix: **HRW**, with **-T** for tests. IDs keep their numbers from the
> TUI plan; HRW-4 (engine loading in Rust) is dropped, see Decisions.

---

## Part 0 - What exists, and what this plan extends

Every row is extended, not rebuilt.

| Existing part | Where | Role in this plan |
|---|---|---|
| `run_agent` | `src-tauri/src/agent/run.rs` | The loop. Unchanged. |
| `execute_turn`, `resolve_turn`, `TurnSetup`, `ChatTarget`, `RunOptions` | `src-tauri/src/commands/agent.rs` | `run_turn` (HRW-1) calls `resolve_turn` + `execute_turn`. |
| `agent_chat_cmd` | `commands/agent.rs:111` | Takes pre-built turns from TS. Removed in HRW-6. |
| `resume_run_cmd` | `commands/agent.rs:254` | Gains row creation and finalize (HRW-1). |
| `gather`, `compose_system_prompt`, `budget_turns`, `with_summary`, `insert_briefs`, `KEEP_RECENT` | `src-tauri/src/agent/context.rs` | The Rust prompt half. HRW-2 uses them for every turn. |
| Scheduler's headless turn | `run_custom_job`, `commands/scheduler.rs:680` | The template for `run_turn`: gather, compose, run. |
| CTX-4 gate | `agent/context_golden.rs`, `src/lib/prompt-assembly.test.ts`, `fixtures/prompt-assembly*` | Proves the switch is safe (HRW-T-4), then retires (HRW-6). |
| `AgentEvent` + `AgentEventSink` | `agent/mod.rs`, `agent/run.rs` | Gains `TurnSaved`, `TurnFinalized`, `Compacted`. The sink feeds the fold (HRW-5). |
| `append_message`, `finalize_message` | `db` + `commands/conversations.rs` | Called by `run_turn` instead of by the frontend. |
| `compact_conversation_cmd` | `commands/conversations.rs` | Its body is called by `run_turn` on overflow. |
| `parseSteps`, `parseUnspoken`, `stepsJsonFor` | `src/lib/api.ts:3935-3965` | The saved shape. Rust writes it; the GUI keeps parsing it. |

Facts this plan rests on (checked 2026-11-01):

- `sendMessage` is `store.ts:3813-4082`; `streamAssistantTurn` is
  `store.ts:6273-7087`. Its only call into a pre-built turn is
  `api.agentChat` at `store.ts:6342`.
- The events that change the saved steps are exactly: `steps_parallel`,
  `step_start`, `task_started/output/ended`, `kept_result`, `step_done`,
  `step_error`, `recall`, `fired`, `code`, `untrusted`, `untrusted_flag`
  (`store.ts:6660-6968`). `fired` also adds lesson names to `context_json`
  (`store.ts:7040`). `run_ended` gives the stop reason (saved only when not
  `completed`) and the plan (saved only with items). `plan` updates the plan.
- Model input built in TS today (`store.ts:3981-4067`): pending workspace
  actions, `+` refs, the "Do it the same way" note (`lastTimeNote`,
  `src/lib/lastTime.ts`), PDF text, image parts or the "can't see images"
  sentence, persona and temperature, recall (query = `sendOpts.action ?? text`),
  system prompt, history budget and compaction.
- `resolveBudget` (`store.ts:5927`) differs from Rust's `context_window`: TS
  uses `CLOUD_CTX` per provider (default 32,000), the endpoint's own
  `ctxSize` (default 8,192) and the engine's budget; Rust uses the pricing
  table and the engine. `KEEP_RECENT_WORKSPACE` exists only in TS.
- TS recall runs whenever the memory toggle is on (`store.ts:5870-5879`); Rust
  `gather` recalls only when memory **and** tools are on (`context.rs:547`).
  Settle this in HRW-2 (see there).
- No Rust code writes `steps_json`. Scheduled runs and delegated children never
  save a timeline.
- `toolsEnabled` lives only in Zustand (default `true`, not saved). Surface
  state is saved after a pause (`saveSurfaceState`), so the DB can lag the
  screen.
- Every other way a turn starts (plan "Go ahead", goal turns, skill commands,
  mail summary, fork resend, block and surface actions) goes through
  `sendMessage`. Resume (`resumeLastRun`, `store.ts:3683`) finishes through
  `streamAssistantTurn`.
- SEE-1 autocheck starts inside `run_agent`, keyed by
  `assistant_message_id`.

---

## HRW-0 The rule

Anything that changes **what the model sees or does** is decided in Rust.
Anything that only changes **what the user sees** stays in the window. The
window passes the user's input and choices; it never builds model input.

Done means the TypeScript copy is **deleted**, not switched off.

So these stay in TypeScript: choosing and loading the engine (with its progress
indicator), the media-model reroute (PIK-2), the conversation title
(`deriveTitle`), the skill command record (`recordCommand`, now with the user
row id from `TurnSaved`), the goal loop, after-turn hooks, and everything the
bubble shows while it streams.

---

## HRW-1 `run_turn`: one entry point for a user turn

New module `src-tauri/src/agent/turn.rs`, plus `run_turn_cmd` in
`commands/agent.rs`.

```rust
pub struct TurnInput {
    pub conversation_id: String,
    pub text: String,                     // what the user typed; saved on the user row
    pub action: Option<String>,           // a block or surface action; the recall query
    pub attachments: Vec<TurnAttachment>, // kind image | pdf | file, name, path, data_uri?
    pub refs: Vec<TurnRef>,               // CMP-3: conversation | artifact, id, label
    pub pending_actions: Vec<String>,     // W2 workspace interactions, already worded
    pub last_time: Option<LastTimeTaken>, // EPI-UI-1: at, conversation id, digest, skill
    pub spoken: bool,                     // VTN-1
    pub model: TurnModel,                 // name, provenance, vision, ctx_size (endpoint)
    pub target: ChatTarget,
    pub options: RunOptions,              // REG-5 per-turn choices (unchanged type)
    pub tools_enabled: bool,
}
```

`run_turn` does, in order:

1. Save the user row (attachments, `spoken`) and an empty assistant row (model
   name and provenance, `spoken`).
2. Emit `AgentEvent::TurnSaved { user_message_id, assistant_message_id }`
   before any slow step, so the bubble takes the real ids at once.
3. `resolve_turn`. If it fails (no engine loaded, missing key), write its
   message to the assistant row, emit `Error`, stop.
4. Build the model's copy of the user message. Port the strings from
   `store.ts:3988-4034` word for word: pending actions prefix, refs, the
   last-time note (port `lastTimeNote`; the date is model input, so a fixed
   English "12 Oct" form, not the webview locale), PDF text (first 20,000
   chars, the scanned and unreadable wordings), image parts when
   `model.vision`, otherwise the "can't see images" sentence.
5. HRW-2 (prompt and history), HRW-3 (recall step and context record).
6. `execute_turn` (unchanged), with the HRW-5 fold attached to the sink.
7. HRW-5 finalize.

A run that fails is saved as `That didn't work: <message>`, as today
(`store.ts:6974-6978`). `tests/copy_lint.rs` covers any new string.

**Resume moves too.** `resume_run_cmd` creates the assistant row, emits
`TurnSaved`, attaches the fold and finalizes through HRW-5. The "I have no
record of that run to pick up" text (`store.ts:6993`) is written by Rust when
there is nothing to resume. After this, only Rust finishes a turn.

## HRW-2 Prompt, history and compaction in Rust

- System prompt: `compose_system_prompt(&gather(...))` with `query` =
  `action` or `text`. Persona, temperature (conversation override, then
  persona, then default) and `spoken` are resolved here.
- History: the conversation's saved messages after `summary_upto_message_id`,
  skipping empty ones and the current user row, then `with_summary` +
  `budget_turns` with `KEEP_RECENT` or `KEEP_RECENT_WORKSPACE` (port the
  second constant).
- Budget: port `resolveBudget` as it is (`CLOUD_CTX` table, endpoint
  `ctx_size` from `TurnModel`, engine budget, the same defaults). Leave the
  run's own `context_window` (the meter, `OBS-3`) as it is.
- Overflow with auto-compact on: run the body of `compact_conversation_cmd`
  with the boundary message, rebudget, and emit
  `AgentEvent::Compacted { conversation_id, summary, summary_upto_message_id }`
  so the window updates its copy. A failed compaction never blocks the send.
- `insert_briefs` as `agent_chat_cmd` does now.
- **The recall difference.** Today TS recalls with tools off and Rust does
  not. Keep what the user gets today (TS behaviour), unless reading
  `compose_system_prompt` shows the index is dropped with tools off anyway, in
  which case the difference is unobservable and Rust's rule stands. Write down
  which it was in the step's status line.
- Before the switch, `sendMessage` runs `flushAllSurfaceStates()`, and
  `tools_enabled` goes in `TurnInput`. While porting, check every other prompt
  input (session state, memory toggle, persona skills, project, plan mode) is
  in the DB before a send can read it; fix any that is not the same way.

**Gate (HRW-T-4):** the CTX-4 fixtures render byte-identically through
`run_turn`'s assembly before the window switches. Add a history case, a summary
case and a tools-off case if they are missing.

## HRW-3 Recall shows up as a step

`run_turn` emits the recall matches as an `AgentEvent::Recall` with a step id
before the first model call, and the fold puts it first, matching
`recallStep` (`store.ts:5913`: verb `remembered`, target the snippet or
"N things", status `done`). `run_turn` also builds the WHY-2 `context_json`
that `buildContextRefs` builds today; `gather` already fills most of it.

## HRW-5 The saved timeline is folded in Rust, and the window shows the saved one

New `agent/timeline.rs`: a one-to-one port of the step fold in
`streamAssistantTurn` to the JSON shape of `AgentStep` (`types.ts:52`). The
sink feeds it as events pass. Port exactly:

- `step_start` pushes `running` with `parallelGroup` (first id of its
  `steps_parallel` batch) and `nestedUnder` (`parent`); `step_done` sets
  `done` and `result`; `step_error` sets `error` and `"— " + error`.
- `task_*` merge as `applyTaskEvent` does; `kept_result`, `code`, `recall`
  (`matches`), `fired` (appended), `untrusted` (appended), `untrusted_flag`
  (`markReadsAsInstructions`) merge into the step with the same id.
- The HRW-3 recall step comes first.
- Output matches `stepsJsonFor`: an array, `{steps, unspoken}` when the voice
  fix supplies `unspoken`, nothing when there are no steps.
- `context_json` gets `fired` lesson names; the stop reason is kept unless
  `completed`; the plan only when it has items.

`run_turn` calls `finalize_message` with text, steps, context, stop reason and
plan, then emits `AgentEvent::TurnFinalized { message_id, steps_json }`.

**One fold, not two.** The window keeps its live fold only to draw steps while
the run works. On `turn_finalized` it replaces the bubble's `steps` with
`api.parseSteps(e.steps_json)`. From then on the screen shows exactly what a
reload would show, so a mismatch between the two folds is visible at once
instead of after a restart, and HRW-T-3 catches it in tests.

**Voice (VTN-6).** Today the TS finalize waits for speech to end and saves
only what was heard (`store.ts:7005-7014`). Rust finishes before the audio.
Fix: Rust finalizes as usual; when speech was cut off, the window calls a new
`set_heard_cmd(message_id, heard)`, which sets the content to `heard`, moves
the full text into `unspoken` in `steps_json`, and sets the stop reason to
`interrupted`.

**Recording the fixtures.** HRW-T-3 needs real event streams next to the
`steps_json` the TS fold saved for them. In debug builds only, when
`POIESIS_RECORD_TURNS` names a folder, `AgentEventSink` appends each event of a
run to `<folder>/<assistant_message_id>.events.jsonl`. After a few real turns
in the window, `scripts/timeline-fixtures.mjs` pairs each file with its row's
`steps_json` from the DB into `src-tauri/tests/fixtures/timeline/`. The
recorder and the script are deleted in HRW-6.

## HRW-6 The window switches, and the TS copy goes

- `sendMessage` (and every path through it) calls `api.runTurn(...)` after its
  UI work (optimistic bubble, PIK-2 reroute, title, engine load). `resumeLastRun`
  calls `api.resumeRun(...)`. Neither calls `appendMessage`, `finalizeMessage`,
  `extractPdfText`, `readImageDataUri`, `recallForPrompt` or `assembleTurns`
  for a turn.
- **Ids:** on `turn_saved` the bubble takes the real ids, and every later patch
  uses them. The swap block after finalize (`store.ts:7051-7069`,
  `harnessProposals`, `verifications`) is deleted. Check the `thought` strip,
  blocks (`messageId ?? assistantId`) and the 41 `patchAssistant` calls use the
  same id. `recordCommand` uses `user_message_id`.
- `compacted` updates the conversation's `summary` and
  `summaryUptoMessageId`, as `assembleTurns` does now.
- After the run the window still does its own work: `refreshToolHealth`,
  `refreshChanges`, the SEE-2 receipt, the goal loop (with `failed`,
  `stopReason`, plan approval and step evidence as today) and the voice fix.
- Delete: TS `composeSystemPrompt`, `assembleTurns`, `resolveBudget`,
  `recallForPrompt`, `buildContextRefs`, `recallStep`, the TS `budgetTurns` /
  `withSummary` if nothing else uses them, `prompt-assembly.test.ts`,
  `context_golden.rs` (the Rust prompt snapshot stays as an ordinary test),
  `agent_chat_cmd`, the turn recorder and its script.
- `tests/eval.rs` calls `run_turn` instead of a hand-built `run_agent`.

## HRW-T Tests

- **HRW-T-1** Before a `store.*.test.ts` case is deleted, its behaviour has a
  Rust test. At least: `store.compose-system-prompt.test.ts`,
  `prompt-assembly.test.ts`, `context.test.ts`, the attachment, refs and
  last-time wording, and the budget defaults. Audit the 22 `store.*.test.ts`
  files and list which move in the step's status line.
- **HRW-T-2** `run_turn` end to end against a fake OpenAI endpoint (the
  pattern in `run.rs` tests): rows saved, `TurnSaved` first, PDF text and image
  parts present, overflow compacts and emits `Compacted`, row finalized with
  steps, a `resolve_turn` failure saved on the row.
- **HRW-T-3** The fold: every recorded fixture folds to its saved
  `steps_json`. At least one each of tools, parallel steps, a nested step, a
  task, an error, a recall, a fired lesson and untrusted text.
- **HRW-T-4** The CTX-4 gate passes through `run_turn`, then retires.
- **HRW-T-5** Resume through Rust: row created, `TurnSaved` sent, finalized
  with steps; nothing to resume saves the fixed text.
- **HRW-T-6** `set_heard_cmd`: content is the heard text, the full text is in
  `unspoken`, the stop reason is `interrupted`.
- **HRW-T-7** The vitest cases for proposals and verifications pass with real
  ids from `TurnSaved` and no swap.

---

## What stays in TypeScript for now

| Piece | Where | Why it waits |
|---|---|---|
| `/goal` loop (GOL) | `goal.ts`, `goalHost` | Stops when you leave the chat; goals are not saved. Moving it needs a decision and a schema bump. |
| Catch-up reflection, auto profile rebuild | after-turn code in `store.ts` | Triggered by window timing. Moving them changes when model calls happen. |
| Recall offer, first-time hints | `maybeOfferRecall`, `maybeFirstTime` | Pure UI. They stay. |

---

## Order

Each step ends green (`npx tsc --noEmit`, `npm test`, `cargo test`). Steps 1 to
3 add Rust beside the running window and change nothing a user sees. Step 4 is
the only one that changes the window.

| Step | IDs | You can feel |
|---|---|---|
| 0 Clean start | - | The open work is committed, and the waiting manual checks (dual-process step 20, voice VOC-T7, commands 3-5) are done or written down as parked. Otherwise a regression after step 4 can't be traced. |
| 1 Rust turn | HRW-1 (with resume), HRW-3, HRW-T-2, HRW-T-5 | `run_turn_cmd` exists and is tested; the window does not use it yet. |
| 2 Prompt and history | HRW-2, HRW-T-1 (prompt cases), HRW-T-4 | The CTX-4 fixtures pass through `run_turn`; the recall difference is settled. |
| 3 Fold and voice | HRW-5, HRW-T-3, HRW-T-6 | Erich runs a handful of turns with the recorder on; they fold in Rust to the same `steps_json`. |
| 4 Window switch | HRW-6, HRW-T-1 (rest), HRW-T-7 | The window behaves as before, voice, proposals, verifications, goals and resume included. TS prompt code, the gate, `agent_chat_cmd` and the recorder are gone; evals run the real path. |
| 5 Pass | - | Erich's manual pass: a long chat that compacts, a PDF and an image, a block action, "Do it the same way", a spoken turn cut off, resume after a stop, a goal, a reload of each. |

## Decisions (2026-11-01)

1. **The TUI is dropped.** Parts II and III of the TUI plan are gone, with
   `run_headless`, `create: false`, `ui.lock`, the `tui` feature and the
   source scripts. Nothing in this plan changes how the release window starts.
2. **Engine loading stays in the window (was HRW-4).** It is a visible step
   with its own progress indicator, not model input. `run_turn` reports
   `resolve_turn`'s error when no engine runs.
3. **Title, skill record and PIK-2 reroute stay in the window.** They change
   what the user sees, not what the model sees.
4. **The goal loop and after-turn hooks stay in TypeScript** for this plan.

## What this is not

- Not a new agent or a new loop. `run_agent` does not change.
- Not a UI change. The window looks and behaves as it does today.
- Not a second fold. After a turn, the window shows the saved timeline.
