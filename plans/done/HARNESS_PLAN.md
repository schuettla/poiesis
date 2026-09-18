# Project Poiesis — Harness Plan

**The loop should be as good as the membrane around it.**

Poiesis is ahead of most agent harnesses on consent, undo, provenance of
outside text, and making work legible to a person. It is behind them on
runtime mechanics: a durable session log, a loop with seams, concurrency,
steering, budgets that end gracefully, per-step model choice, and code as a way
to avoid model turns. This plan closes that half.

> ID prefixes: **HRN** loop and runtime - **CTX** session log and context
> ownership - **RPC** code as orchestration - **OBS** cost and telemetry -
> **HRN-UI** frontend - **-T** tests.
>
> **Read the shared sequencing below first.** The tracks here are not built in
> one run. They interleave with `SUBAGENTS_PLAN.md`: this plan's Phase 0 is the
> prerequisite for delegation, and delegation ships before the rest of this
> plan does.
>
> **Track order when read on its own: A → B → F → C → D → E.** A and B are
> small and unblock everything. F (cost accounting) comes before C because
> every budget claim in this plan is unmeasurable without it. C is the large one
> and should not start until A and B have shipped and settled.
>
> **PRES-0 (first-person copy) binds every `-UI-` task.** §Copy is the table.
>
> **Settled decisions (2026-09-03):**
> - **No rewrite of `run.rs`.** Seams and extraction, not a new loop. The
>   local-model tolerance in there (`parse_text_tool_calls`,
>   `flatten_to_alternating`, `safe_prefix`) is hard-won and stays exactly as
>   it is.
> - **The backend becomes the owner of the prompt.** The frontend keeps owning
>   what the *user* sees. It stops owning what the *model* sees.
> - **Every cap ends with an answer, never with an error.** A run that hits a
>   limit gets one wrap-up turn. "Reached the limit of tool steps" with the work
>   thrown away is a bug, not a guard rail.
> - **Nothing here changes the permission membrane.** Every new path
>   (RPC from the sandbox, per-step model routing, replay) goes through the same
>   gates, or it does not ship.

---

# Sequencing across both plans

This plan and `SUBAGENTS_PLAN.md` are one programme in six phases. The same
table appears in both files. Each phase ends with something a user can see, so
no phase is infrastructure taken on faith.

| Phase | Tasks | Result |
| --- | --- | --- |
| **0** ✓ | `HRN-1`, `HRN-2`, `HRN-3`, `HRN-5`, `OBS-1`, `HRN-UI-1` | Per-run cancellation, a steering inbox, stop that keeps partial work, budgets that end with an answer, token capture. You can type to a running turn. **Built 2026-09-03 — see “Phase 0 as built” below.** |
| **1** ✓ | `SUB-1` to `SUB-9`, `SUB-UI-1` to `SUB-UI-5`, `SUB-UI-7` | Foreground parallel delegation you can watch, steer and stop. **Built 2026-09-06 — see “Phase 1 as built” in `SUBAGENTS_PLAN.md`.** |
| **2** ✓ | `HRN-4`, `HRN-7`, `HRN-8`, `OBS-2`, `OBS-3`, `HRN-UI-2` to `HRN-UI-4` | Parallel tool calls, big results kept on disk, visible cost. **Built 2026-09-06 — see “Phase 2 as built” in `HARNESS_PLAN.md`.** (`HRN-7`, the cheap model for mechanical steps, was withdrawn on 2026-09-08.) |
| **3** ✓ | `CTX-2`, `CTX-5`, `HRN-UI-5` | The session log. Fork a conversation, resume an interrupted run. **Built 2026-09-07 — see “Phase 3 as built”. `CTX-3`/`CTX-4` followed the same day — see “`CTX-3`, `CTX-4` and `HRN-6` as built”.** |
| **4** ✓ | `SUB-10` to `SUB-12` | Background delegation. **Built 2026-09-07 — see “Phase 4 as built” in `SUBAGENTS_PLAN.md`.** |
| **5** ✓ | `RPC-1` to `RPC-3`, `HRN-6` | Scripts that call tools over RPC, full loop seams. **Built 2026-09-07 — see “Phase 5 as built” and “`CTX-3`, `CTX-4` and `HRN-6` as built” in `HARNESS_PLAN.md`.** |

## Phase 0 as built (2026-09-03)

What landed, and where it differs from the task text above.

- `src-tauri/src/agent/fleet.rs` is new: `RunHandle`, `Steer`/`SteerSource`,
  `StopReason`, `RunOutcome`, `RunLimits`, `Fleet`. Registered as Tauri state in
  `lib.rs`. `RuntimeManager::new_cancel`/`cancel_active` are untouched and still
  serve the plain-chat stream, which registers no run; `stop_chat_cmd` now also
  calls `Fleet::cancel_all`, which is what Stop means once a turn is a tree.
- `run_agent` takes `&RunHandle` and `&RunLimits` in place of `cancel:
  CancelFlag`, and returns `RunOutcome` instead of `String`. Both callers
  (`agent_chat_cmd`, `scheduler::run_custom_job`) open and close a run around it.
- The loop is a `loop`, not `for 0..MAX_ITERATIONS`. It drains the steering
  inbox at the top of each iteration, and sets `wrap_up` *before* running the
  turn that will be the last one — that turn shadows `tools_enabled` to false,
  so the wrap-up behaves exactly like plain-chat mode and cannot start another
  call. `MAX_ITERATIONS` is gone; `WRAP_UP_PROMPT` replaced it.
- New events: `RunStarted`, `RunProgress`, `RunEnded` (carries `stop_reason`,
  `steps`, `ms`, `usage`) and `Steered`. `Done`/`Cancelled`/`Error` are
  unchanged and still fire, so nothing downstream had to be rewritten.
- `OBS-1` landed as **parse-only**. `TurnOutcome::Final`/`ToolCalls` now carry
  `Option<Usage>`, summed onto the `RunHandle` and reported in `RunEnded`. The
  request shape is unchanged: a plain OpenAI-compatible cloud stream only sends
  `usage` when asked via `stream_options`, and adding that risks every cloud
  turn for a number nothing shows yet. So today usage is real for the local
  engine and for Anthropic, and `null` (not zero) elsewhere. `OBS-2` adds
  `stream_options` alongside the price table that needs it.
- `HRN-3`'s persistence used schema **v22** (`messages.stop_reason`), which
  pushed the two later plans' reservations to v23 and v24.
- Frontend: `store.activeRun`, `steerActiveRun`, `api.steerRun`,
  `Message.midRun` and `Message.stopReason`; the composer sends to the run
  instead of blocking on `busy`, and shows `↑` rather than `■` while there is
  text to send; `RunMeter` and `StoppedNote` in `AgentRun`.

Still open from Phase 0's spirit: nothing shows token counts yet (that is
`HRN-UI-3`'s cloud half, gated on `OBS-2`).

Why delegation interrupts this plan after Phase 0 rather than waiting for it to
finish:

- **Phase 0 is exactly what delegation needs and nothing more.** Per-run
  cancellation, a steering inbox, partial results on stop, and a budget that
  ends with an answer. Those four are the difference between a fleet you can
  control and a fleet you can only start.
- **The rest of this plan is not a blocker.** `SUB-5` runs its children with
  `join_all` inside one tool call, so it does not need `HRN-4`. Foreground
  children do not need the session log. Both would benefit, neither is required.
- **Background delegation does need Phase 3.** `SUB-10` to `SUB-12` are placed
  after `CTX` for that reason, not as a preference.
- **`HRN-6` is last on purpose.** It is a behaviour-preserving refactor of a
  1768-line function with no user-visible result. Delegation needs two small
  seams only (the tagged sink, the inbox drain). Extracting the rest afterwards
  means extracting the seams that turned out to matter.

`HRN-1` and `HRN-2` here are `SUB-0` and `SUB-6` in the other plan. Build them
once, in `agent/fleet.rs`, during Phase 0.

---

# Part I — The gap, stated once

What the reference harnesses do that Poiesis cannot today.

| Capability | Reference | Poiesis today |
|---|---|---|
| Steer a run in flight | Hermes `steer_task`, Pi `steer_subagent`, applied after the current tool call | Stop only. The loop reads no outside input mid-run. |
| Parallel tool calls | Standard everywhere | [`dispatch_calls`](../src-tauri/src/agent/run.rs#L903) runs a turn's calls in a `for` loop |
| More than one run at once | Pools with `maxConcurrent` and queueing | [`new_cancel`](../src-tauri/src/runtime/manager.rs#L251) overwrites one global flag |
| Graceful budget end | Pi warns "wrap up" at max turns | `MAX_ITERATIONS` 12, then an error and the work is lost |
| Durable session log | DeepSeek: the log is the context, replay and fork fall out of it | The prompt is built in [store.ts](../src/lib/store.ts) and the transcript lives on a stack frame |
| Per-task model choice | Agent frontmatter pins model and thinking level | `personas.model_id` exists, but routing is per conversation |
| Code instead of turns | Hermes: Python calling tools over RPC. Pi: `SubagentWorkflow` | The `codeexec` sandbox cannot call Poiesis tools |
| Tool output economy | Large results go to a handle, the model reads what it needs | The whole tool output string goes into the transcript |
| Token accounting | Everywhere | Nowhere in `proxy.rs` or the cloud layer |

## Phase 2 as built (2026-09-06)

`HRN-4`, `HRN-7`, `HRN-8`, `OBS-2`, `OBS-3`, `HRN-UI-2` to `HRN-UI-4`, and the
Models/Usage halves of `HRN-UI-6`. Schema is now **v24**.

### Backend

- **`HRN-4` parallel dispatch.** [`partition_calls`](../src-tauri/src/agent/run.rs)
  splits a turn's calls into a concurrent set and an ordered one;
  `dispatch_calls` runs the first through `join_all` and the second in sequence,
  then folds every result back **in the model's original call order**, so the
  transcript is the same whatever order the work finished in. A call that never
  ran because Stop was pressed still gets a tool result saying so.
- **`Toolset::is_serial(self, tool)`** takes the tool name as well as the
  toolset, which the task text did not ask for. One toolset is not one answer:
  `read_file` is safe beside anything and `write_file` is safe beside nothing,
  and both are the File System toolset. Only `WebSearch`, `Recall`, `Indexing`
  and File System *reads* are concurrent. MCP and skills are serial — we cannot
  see whether an MCP tool writes, and guessing wrong costs correctness while
  guessing right only saves time.
- **`HRN-8` results on disk.** New [`results.rs`](../src-tauri/src/agent/results.rs):
  a per-run `ResultStore` under `<data_dir>/results/<conversation>/<run>/`.
  Anything past 8 KB is written out and the model gets the first 2 KB plus a
  `res_xxxxxx` handle. `read_result` and `search_result` are dispatched by the
  loop rather than by a toolset, and are **not advertised until the run has
  actually kept something** — a tool that can only fail costs a step to learn
  what its absence would have said. Deleting a conversation deletes its kept
  results.
- **`HRN-7` cheap steps. Built, then removed on 2026-09-08 — see "HRN-7
  withdrawn" below.** What survives is the shape it forced: the wrap-up turn
  moved into its own block in `run_agent_inner`, since it always ends the run
  and a flag threaded through the main path was the wrong shape.
- **`OBS-2` cost.** `run_usage` (v24) plus `Db::record_run_usage` /
  `Db::usage_summary`, written from `run_agent_inner`'s `end` closure so every
  exit path bills, including the failures. `RunContext` gained `provenance`,
  since only the caller can tell a BYOK provider from the user's own server.
  [`cloud/pricing.rs`](../src-tauri/src/cloud/pricing.rs) is the small constant
  table the task asked for: longest-prefix match, and a miss is *unknown*, never
  free.
- **`OBS-3` context meter.** `RunContext.context_window` (the engine's
  `ctx_size`, or the price table's figure for a cloud model) rides on
  `RunStarted`; `RunProgress` carries an `estimate_tokens` of the assembled
  transcript, using the same four-chars-per-token rule as the frontend so the
  two halves of the app cannot disagree.

### Frontend

- **`HRN-UI-2`.** A `steps_parallel` event arrives before the batch's
  `step_start`s and tags them with a group. `Timeline` folds consecutive
  same-group steps into one `.step-band` headed `3 things at once`, and settles
  it back into ordinary rows when they finish. A child's timeline in the Agents
  tab bands the same way.
- **`HRN-UI-3`.** The Phase 0 run meter now reads
  `step 4 of 12 · 0:41 · context 38%`, and falls back to
  `about 12k in context` where the window is unknown rather than inventing a
  percentage.
- **`HRN-UI-4`.** A kept result hangs off its step behind the same `⌄`
  disclosure `Code` and `Recall` use, with the full text and a **Save to the
  working folder** action (`save_kept_result_cmd`, which refuses when the chat
  has no folder rather than choosing somewhere itself).
- **`HRN-UI-6`.** Settings → Models gained the one switch (`CheapSteps`),
  removed with `HRN-7` on 2026-09-08. What remains is the new
  Settings → **Usage** tab listing spend by day, by model and by chat. Local rows
  read *on your device, no cost*; an unpriced model reads *price unknown* and
  the total says it is a floor.

### Deliberate deviations

1. **`is_serial` takes the tool name**, not just the toolset (above).
2. **`HRN-7` withdrawn (2026-09-08).** Only one step in the loop ever qualified
   as mechanical — the wrap-up, where a run out of budget summarises what it
   already has. The narration nudge asks the model to emit a real tool call,
   which is exactly what a smaller model is worse at, and title generation has
   no backend path to route. So the feature was one setting, one fallback path
   and one buffered-instead-of-streamed special case, all to save one turn's
   output tokens on runs that hit their step limit — which should be rare. The
   switch, `cheap_endpoint`, `models.cheap_mechanical_steps`, `TurnCtx.cheap`
   and the `CheapSteps` panel are gone; the wrap-up runs on the run's own model
   and streams like any other turn. Per-step routing can come back if a real
   second use for it appears.
3. **`read_result`/`search_result` are the loop's, not a toolset's.** The task
   text says "owned by the toolset that produced the output"; that would mean
   the same two tools defined in five places, each with its own store. They are
   loop bookkeeping and live with the loop.
4. **Only model buckets get a price.** A day or a chat can mix models, and
   adding a known price to an unknown one would read as a total when it is a
   floor.
5. **`run_usage` has no foreign key to `conversations`.** Deleting a chat should
   not quietly erase a month's spend. The id is kept so the surviving chats can
   still be named.
6. **Usage is grouped by UTC day and labelled in local time**, which can put a
   late-evening run on the next day's line. Smaller wrong than a timezone
   database for a spend panel.

### Tests

`HRN-T3` (`run.rs`: reads concurrent, writes/live sessions/unknowns ordered;
a lone read is not a batch), `HRN-T6` (`results.rs`: the 8 KB boundary itself
stays inline, the preview carries the handle, a window returns the right slice
and says what is left, search gives line numbers, an unknown handle is refused),
`OBS-T1` (`db`: three-way grouping, a silent run records nothing rather than a
zero row, a window that starts after the rows sees none), plus four pricing
tests and, on the frontend, four `bands` tests and two more `applySubEvent`
ones. **399 Rust, 84 frontend, `tsc` and `clippy` clean.**

---

## Phase 3 as built (2026-09-07)

`CTX-2`, `CTX-5` and `HRN-UI-5` are built. `CTX-3` and `CTX-4` are not, on
purpose — see "What is deliberately still open" at the end of this section.

### Backend

- **`src-tauri/src/agent/log.rs`** is new: `SessionLog` (the writer), `replay`
  (`CTX-T2`), `kind_of` (what a transcript message is), and `RESUME_PROMPT`.
- **Schema v25** adds `session_events` exactly as `CTX-2` specifies, plus an
  index on `run_id` because replay is a per-run read. The unique index on
  `(conversation_id, seq)` is what makes "fork at this point" a range instead of
  a guess. `ON DELETE CASCADE`, so deleting a chat takes its log with it.
- **`db/mod.rs`** gains `append_session_event` (allocates `seq` inside the same
  lock as the insert), `session_events`, `session_events_for_run`,
  `last_logged_run`, `fork_session_events`, `seq_at_time`, and
  `fork_conversation`.
- **`run.rs`** writes the log through one watermark, `flush_log`, called at the
  top of every iteration and again after the steer drain. A dozen push sites
  across two functions would each have had to remember to log themselves;
  a watermark means a message that reaches the model reaches the log by
  construction. `end` writes the answer and the `stop` row on every exit path.
- **`commands/agent.rs`** is refactored rather than duplicated: `resolve_turn`
  answers "where does this run" once, and `execute_turn` runs it. `agent_chat_cmd`
  and the new `resume_run_cmd` both end there, so a resumed run is the same kind
  of run in every way the fleet, the permission prompts and the usage ledger can
  see. `fork_conversation_cmd` is the other new command.

### Frontend

- `api.resumeRun`, `api.forkConversation`, and a shared `RunOptions` so the two
  run entry points cannot drift.
- `store.forkFromMessage` and `store.resumeLastRun`. `streamAssistantTurn` gains
  one `resume` flag and a `startRun` indirection; every event is handled by the
  same code either way, which is the point — a resumed run must look like a run.
- `TurnActions` under every finished assistant turn: **Try again from here**
  always, **Continue where I stopped** only on the last turn and only when it
  ended `aborted`, `timeout` or `max_steps`.

### Deliberate deviations

1. **A `prompt` row.** The plan's kind list has no such kind, because it assumes
   `CTX-3` has already landed and the loop writes the opening turns itself.
   Assembly still lives in `store.ts`, so the loop is handed an array it did not
   build and cannot take apart without guessing. It stores that array whole, in
   one row. Replay is identical either way, and when `CTX-3` lands this row
   splits into the per-kind rows the plan describes with no reader changing.
2. **The prefix is stored once per run, not once per conversation.** The
   frontend budgets and summarizes, so the assembled prefix genuinely differs
   between runs and cannot be shared. This is the cost of deviation 1 and it
   goes away with `CTX-3`.
3. **A steer is logged twice** — once as a `steer` event and once as the user
   message it becomes. `replay` skips the `steer` row. They carry the same words
   but not the same fact, and only the log can say later that this arrived
   mid-run.
4. **Fork does not copy the user turn it is redoing**; it hands it back for the
   caller to send. Sending is what makes this a rerun rather than a copy: it
   builds a fresh prompt from the branch's own history. Copying it as well would
   show it twice the moment the caller sends.
5. **Fork keeps the working folder and its trust level.** Re-granting trust is
   not part of asking the same question again, and a fork the user has to
   re-configure is not a fork.
6. **`resume_run_cmd` returns `false` rather than an error** when there is
   nothing to continue. A run from before this log existed has nothing to
   continue, which is a fact about the run, not a failure.
7. **"Continue" is offered only on the last turn.** `last_logged_run` can
   continue exactly one run — the newest. A button on an older turn would
   quietly resume a different run than the one it sits under.

### What is deliberately still open

`CTX-3` (port `composeSystemPrompt`, `budgetTurns`, `withSummary` and the two
briefs to `agent/context.rs`) and `CTX-4` (the byte-identical equivalence gate,
then the switch) are **not** built. The reasons, in order:

- It is a port of roughly 300 lines whose every input — persona, memory context,
  skills, the live surface, session state, tool health — is frontend state that
  has no Rust reader yet. The port is the small half; wiring the inputs is the
  large half.
- `CTX-4` demands byte-identity, so it cannot be shipped incrementally. Half a
  port is a second assembly path that disagrees with the first, which is the
  exact bug `CTX-1` describes.
- Nothing in Phase 3's user-visible promise needs it. Resume replays what was
  sent; it never re-assembles. Fork re-sends through the existing path.

What stays broken until it lands: `scheduler::run_custom_job` still sends one
bare user message with no soul, no memory index, no skills list and no tool
guidance (`CTX-1`'s drift). That bug is unchanged by this phase, not caused by
it. Phase 4 (background delegation) is the phase that should carry it, since a
background run is a scheduled run in every way that matters here.

## Phase 5 as built (2026-09-07)

`RPC-1`, `RPC-2` and `RPC-3`. `HRN-6` is **not** built — see the end of this
section. No schema change: the whole feature is one setting row.

### Backend

- **`agent/toolrpc.rs` is new.** A second loopback listener beside
  [`preview.rs`](../src-tauri/src/agent/preview.rs)'s, answering exactly one
  route: `POST /<token>/tool` with `{name, arguments}`. It owns no ability of
  its own — it posts the call to the run that armed the token over an `mpsc`
  channel and waits on a `oneshot` for the answer. That is what makes
  permissions, folder trust, untrusted marking, the activity log and the
  headless refusal apply unchanged: it is the same `dispatch`, on the same run's
  state, not a second copy of any of it.
- **The token is a `Ticket` that revokes on `Drop`.** `run_code` has half a
  dozen ways to end and the one that matters most — the future dropped because
  the user pressed Stop — runs no code of its own. `Drop` reaches all of them,
  so "minted per call, revoked when the call returns" is enforced by the type
  rather than by remembering to clean up on every exit path.
- **`dispatch_calls` serves the calls, not a spawned task.** The batch future
  and the RPC inbox are raced in one `tokio::select!` loop. A detached task
  could not have done this: every one of `dispatch`'s twenty-two arguments is a
  borrow of the loop's own stack frame, which is precisely the property that
  makes the served call indistinguishable from a call the model made.
- **`ToolContext` gains `rpc: Option<&Gate>`**, and the channel is created only
  when the batch actually contains a `run_code` call *and* the setting is on. A
  batch without one behaves exactly as it did before.
- **`RPC-2`: two client libraries as `const &str`,** written into the snippet's
  own directory next to `main.py`/`main.js`. Python puts the script's folder on
  `sys.path` and Node resolves `./poiesis` against the module file, so the
  import works whichever folder the sandbox runs from — including a skill's,
  where the working directory is not the scratch directory.
- **`RPC-3`: the paragraph is appended in `ToolRegistry::build`,** not baked
  into `codeexec::tool_specs()`, so it appears only while the ability is
  actually on. `tool_specs(self)` takes no arguments and giving all fourteen
  toolsets a `&Db` to serve one conditional sentence would have been the worse
  trade.

### Frontend

- **`AgentEvent::StepStart` gains `parent: Option<String>`** (skipped when
  `None`, so nothing else on the wire changed) and `AgentStep` gains
  `nestedUnder`. A script's tool calls are drawn indented under the step that
  ran the script, hung off a hairline. Forty of them arriving as ordinary rows
  would read as forty things the agent decided to do.
- Both timeline surfaces render through the same `Timeline`, so the Fleet card
  and the Agents tab got the nesting for free.
- The screen-reader sentence says "from the script, …" outright — indentation
  carries this for a sighted reader and nothing did for anyone else.
- **Settings → Tools**: `ScriptToolsSwitch`, shown under Code execution only
  while the sandbox itself is on, since the ability has nothing to attach to
  otherwise.

### Deliberate deviations

1. **The port is bound at startup even when the setting is off.** The plan
   implies a per-run server. One listener with an empty token map can answer
   nothing, matches `preview.rs`'s arrangement, and means there is one way
   loopback is done in this codebase rather than two.
2. **The token is not injected as one env var.** `POIESIS_TOOL_URL` is the base
   and `POIESIS_RUN_TOKEN` the token, as the task text says; the client library
   joins them. A single pre-joined URL would have put the secret in a variable
   whose name does not say it is one.
3. **A snippet that may call tools gets 120 seconds, not 10.** Its wall clock
   goes on waiting for searches, reads and permission panels rather than on
   computing, and the loop-over-many-items shape the whole task exists for
   cannot finish in ten seconds. The timeout message now names the actual limit,
   because "the 10-second limit" had become a lie the model would plan around.
4. **Served calls are answered one at a time.** A script blocks on each call
   anyway, so overlapping them buys nothing, and serving them in order keeps the
   timeline in the order the script did the work. The cost is that a concurrent
   sibling read in the same batch pauses while a script's call is served.
5. **A script may not start a script, whatever the allowlist says.** The task
   text asks for "the run's own toolsets minus `CodeExec`" and that is what
   `script_refusal` does — but as a rule of its own rather than by subtracting a
   toolset, so a future toolset that can also run code is one line to add.
6. **No refusal for a tool this run does not have.** `dispatch` already says so
   in its own words, and a second message would drift from the first.

### What was still open at the time, and is now built

`HRN-6` was deferred out of this phase because its `assemble` seam is where
`CTX-3` lands, and `CTX-3` was not built yet — extracting the seam first would
have produced an empty box and the refactor would have had to be done twice. The
order recorded here was `CTX-3` → `CTX-4` → `HRN-6`, against the sequencing table
above. That is the order that was then followed; see the next section.

## `CTX-3`, `CTX-4` and `HRN-6` as built (2026-09-07)

Prompt assembly moves to Rust, an equivalence gate pins the two implementations
together, and the loop is cut into named phases. No schema change.

### `CTX-3` — `agent/context.rs`

- **The whole of `composeSystemPrompt` is ported**, block for block: the
  about-you synthesis, SOUL.md, the memory index, the skills list, the block
  registry, the surface, session state, the three guidance blocks, the
  remembering block and the tool cautions. `budgetTurns`, `withSummary`,
  `estimateTokens`, `KEEP_RECENT` and `KEEP_RECENT_WORKSPACE` come with them.
- **JSON text is passed through, never re-serialized.** Session state, the
  surface tree and a block's payload are all stored as JSON *text*. JavaScript
  keeps object keys in insertion order and `serde_json` sorts them, so
  re-serializing would reorder keys and break byte-identity for nothing. The
  module takes those fields as `String` and parses only to ask "is this object
  empty", throwing the parse away again.
- **Lengths are counted in UTF-16 code units**, because that is what
  JavaScript's `String.length` counts. Every cap in the file is a port of one
  measured that way, and a budget that disagrees with the meter the user is
  looking at is worse than a slightly wrong one.
- **`gather` reads the inputs out of the database**, split into `from_db` (needs
  nothing but a schema, so it is testable against a real one) and the half that
  needs the `MemoryStore` and an app data directory.
  `commands::memory::recall_for` was lifted out of its Tauri command so scoped
  recall is the *same* call from both sides rather than two that could drift.
- **The folder brief and the Canvas brief moved** out of `agent_chat_cmd` into
  `context::insert_briefs`, so every way a run can start gets them.

### `CTX-4` — the equivalence gate

- `fixtures/prompt-assembly.json` is the shared input;
  `fixtures/prompt-assembly.golden.txt` is the shared expected output. The Rust
  test (`agent/context_golden.rs`) and the vitest
  (`src/lib/prompt-assembly.test.ts`) each render the fixture through their own
  assembly and compare against that one file. Both matching one file is the same
  claim as the two matching each other, and it needs no running app and no IPC.
- A second test on each side asserts the fixture actually **overflows its
  budget**. A golden recorded from a window nothing overflowed would pass forever
  while proving nothing about the half of assembly that decides what to drop.
- `UPDATE_PROMPT_GOLDEN=1` rewrites the golden from the Rust side; the vitest
  must then agree without being told anything.
- **The scheduler is switched over.** `scheduler::run_custom_job` assembles
  through `context::gather` + `compose_system_prompt` + `insert_briefs`. A
  scheduled task now runs with the persona, standing instructions, memory index,
  skills list and tool guidance it has never had. That was `CTX-1`'s drift and
  it is fixed.

### `HRN-6` — lifecycle seams

- `run_agent_inner`'s loop is now six named phases on a `TurnCtx`:
  `prepare_turn`, `assemble`, `request`, `classify`, `dispatch_batch` (which
  carries `record`), plus `wrap_up_turn` for the budget-spent last turn.
  `classify` returns a `Next` enum rather than returning out of the loop from
  four different places.
- **`TurnCtx` is a struct of borrows on the loop's own stack frame.** That is
  load-bearing, not tidiness: because nothing can hold one beyond the run, a
  detached task can never dispatch a tool call on a run that has ended — the
  property `RPC-1` depends on. `dispatch` went from 22 parameters to 4 and
  `dispatch_calls` from 25 to 2.
- **`RunObserver`** is a four-method trait with do-nothing defaults (`prompt`,
  `appended`, `steered`, `stopped`). `SessionLog` is the first implementation
  and `()` is the observer for a run nobody is recording. The loop holds
  `&dyn RunObserver`, so `CTX-2`'s writer is swappable and `OBS-2`'s telemetry
  becomes an added impl rather than another edit inside the loop.
- The existing tests in `run.rs` pass **untouched**, which is the task's own
  exit criterion. Two were *added*, for the seam itself: the watermark hands
  each message over exactly once, and an observer that wants nothing has to
  write nothing.

### Deliberate deviations

1. **`TurnInput` was not added.** The task asks for `run_agent(messages:
   TurnInput)` with `Assembled` and `FromLog` variants. Both collapse to
   `Vec<Value>` before `run_agent`'s first line, and resume already replays from
   the log in `resume_run_cmd`, so the enum would have been a signature change
   across every call site buying nothing. Assembly happens at the call site.
2. **The gate is a shared fixture, not a dev-only command.** The task suggests
   exporting the frontend's result through a command and comparing in a test.
   That needs a running app and an IPC round trip to compare two pure functions.
   Two tests against one file is the same guarantee with neither.
3. **The golden is plain text with a marker line per turn, not a serialized
   array.** Comparing serialized arrays would have tested `serde_json` against
   `JSON.stringify` — two things allowed to differ — instead of the two
   assemblies.
4. **`AgentEventSink` did not become a `RunObserver`.** The task says it should.
   The sink is how a run talks to the *user* and is reached from inside every
   toolset through `ToolContext`; an observer is how a run talks to whatever is
   writing it down, and is called only at the loop's own phase boundaries.
   Folding them together would have made every toolset generic over something
   none of them use.
5. **`is_none_or` is spelled out as a `match`.** It is stable since Rust 1.82
   and this crate's stated minimum is 1.77.

### What is deliberately still open

**`agent_chat_cmd` still takes an assembled `messages` array.** The scheduler
was switched over — that is where `CTX-1`'s drift actually lived. The chat
window was not, because its `messages` array is not only prompt assembly: PDF
text extraction, image encoding, the vision-capable check, the queued
workspace-action prefix, the auto-compaction round trip and `WHY-2`'s context
refs are all folded into the same code path, and none of them is assembly.
Moving them is its own piece of work with real regression risk on the one path
the user is on constantly, and it buys nothing a user can see — the chat
window's prompt is already correct.

Two implementations kept honest by a byte-identical gate is a sound intermediate
state; two implementations with nothing pinning them together was not, and that
is what changed. When the chat path does move, it lands in `TurnCtx::assemble`,
which now exists for it.

# Track A — Run identity, steering, stop

*Phase 0. Shared with `SUBAGENTS_PLAN.md` SUB-0 and SUB-6, which are the same
code. Build once, here.*

### HRN-1 - `agent/fleet.rs`: a handle per run

```rust
pub struct RunHandle {
    pub id: String,                 // "run_<uuid simple>"
    pub cancel: CancelFlag,
    pub inbox: Mutex<Vec<Steer>>,   // FIFO, drained at the top of each iteration
    pub parent: Option<String>,
    pub depth: usize,
    pub conversation_id: String,
    pub started_at: i64,
    pub steps: AtomicUsize,         // for the live meter
}

pub struct Steer { pub text: String, pub from: SteerSource }  // User | Lead
pub struct Fleet { runs: Mutex<HashMap<String, Arc<RunHandle>>> }
```

`Fleet` is Tauri state. `open`, `get`, `close`, `children_of`, `cancel_tree`.

`RuntimeManager::new_cancel` and `cancel_active` keep working for the top-level
turn, but `cancel_active` also calls `Fleet::cancel_tree`. One global flag is
the assumption that has to go; leaving the old API in place keeps the change
small.

### HRN-2 - The loop reads its inbox

In `run_agent_inner`, at the top of the iteration loop, right after the cancel
check: drain `run.inbox` and push each entry as
`{"role": "user", "content": "<text>"}` (prefixed "New instruction: " when
`from == Lead`). This is the only correct point. Mid-tool-call the transcript is
not in a valid state, and mid-stream would interleave with tokens.

Command: `steer_run_cmd(run_id, text)`.

### HRN-3 - Stop keeps what it has *(Phase 0)*

The loop already returns `final_text` on cancel. Make that path explicit: return
a `RunOutcome { text, stop_reason, steps }` where `stop_reason` is one of
`completed | aborted | timeout | max_steps | error`, borrowed from DeepSeek's
extensible union. Callers that only want the text keep working through a
`.text` field. Persist the reason on the assistant message so a stopped turn
still reads as "stopped", not as a short answer.

---

# Track B — Loop mechanics

### HRN-4 - Parallel tool dispatch

In `dispatch_calls`, partition a turn's calls into **independent** and
**ordered**, then run the independent ones with `futures::future::join_all` and
the ordered ones in sequence, before appending all results in the original call
order so the transcript is deterministic.

A call is ordered if any of these is true:
- Its toolset declares itself serial. Add `Toolset::is_serial(self) -> bool`,
  true for `FileSystem` writes, `Browser`, `Memory`, `Present`, `System`.
  Anything that mutates shared state or drives one live session runs in order.
- It needs a permission prompt (unknowable in advance, so a serial toolset is
  the proxy for it).
- `ctx.rendered` matters: only one render per call already, but two concurrent
  calls both rendering would race the `dockOpen` switch, so renders stay serial.

Safe and worthwhile in parallel: `web_search`, `read_file`, `search_files`,
`search_folder`, `recall`, MCP reads. Those are also the ones models batch.

Emit `StepStart` for every parallel call up front, so the timeline shows three
rows filling at once rather than one row moving. That is the visible half of
this task and it is why it is worth doing early.

### HRN-5 - Budgets that end with an answer *(Phase 0)*

Replace the `MAX_ITERATIONS` constant with

```rust
pub struct RunLimits {
    pub max_iterations: usize,      // default 12, subagent 8
    pub deadline: Option<Instant>,  // wall clock, None for interactive top-level
    pub max_depth: usize,           // default 1
}
```

When `iteration == max_iterations - 1`, or the deadline has passed, do not
break out. Push one system message:

> You are out of room for tool calls. Answer now with what you already have.
> Say plainly what you did not get to.

Then run exactly one more turn with `tools: []` so the model cannot start
another call, and return with `stop_reason = max_steps | timeout`. The current
behaviour throws away everything the run learned, which is the single worst
failure mode in the loop today.

### HRN-6 - Lifecycle seams

Extract the body of `run_agent_inner`'s loop into named phases, each a private
function taking a small `TurnCtx`:

```
prepare_turn   -> drain inbox, apply limits, decide the model for this step
assemble       -> messages + tool specs for this turn (CTX-3 moves work here)
request        -> drive_turn_adapting
classify       -> final | tool calls | narration | cancelled
dispatch       -> dispatch_calls
record         -> tool stats, fixes, session log append (CTX-2)
```

Add a `RunObserver` trait with a no-op default, called at each phase boundary.
`AgentEventSink` becomes one implementation; the session log writer (CTX-2)
becomes another; telemetry (OBS-2) a third. This is the seam that stops every
future feature from being another edit inside a 1768-line function.

No behaviour change in this task. It is done when the existing tests in
`run.rs` pass untouched.

### HRN-7 - Per-step model routing (built, then withdrawn 2026-09-08)

Built in Phase 2 and removed again: only the wrap-up turn ever qualified, and
one setting plus a fallback path was too much machinery for one turn's tokens.
See "HRN-7 withdrawn" under Phase 2's deliberate deviations. The original task
text follows, for whoever revisits this.

A `ModelPlan` resolved once per run:

```rust
pub struct ModelPlan { pub main: ChatEndpoint, pub cheap: Option<ChatEndpoint> }
```

`cheap` is the local engine when one is loaded, else a persona-named small
model, else `None`. Steps that are mechanical rather than judgemental run on
`cheap` when it exists:

- the wrap-up turn from HRN-5,
- the narration nudge retry (`MAX_NARRATION_NUDGES` path),
- title generation and any other side call,
- (later) a subagent whose agent type pins a cheap model.

This is deliberately a short list. Routing the *reasoning* turns automatically
is a quality decision the user should make, not a heuristic. Settings gets one
switch: "Use my local model for small mechanical steps" (default on when an
engine is loaded).

---

# Track C — The session log

This is the big one. It is what makes replay, fork, resume, honest compaction,
and background subagents possible, and it is currently split across
`store.ts` and a stack frame.

### CTX-1 - What is true today

Prompt assembly lives in the frontend:
[`composeSystemPrompt`](../src/lib/store.ts#L3947) (persona base, about-you,
soul, memory index, skills block, block registry, surface, session state, tool
guidance, memory guidance, tool cautions), plus
[`context.ts`](../src/lib/context.ts) (`budgetTurns`, `withSummary`,
`KEEP_RECENT`) and the summarize-on-overflow path in `streamAssistantTurn`.

The backend adds two system messages of its own inside `agent_chat_cmd` (the
folder brief and the artifacts brief) and then normalises the whole thing in
`normalize_transcript`.

Two clients already disagree: `scheduler::run_custom_job` sends one bare user
message with none of the above. A scheduled task therefore runs with no soul,
no memory index, no skills list and no tool guidance. That is a real bug this
track fixes as a side effect.

### CTX-2 - `session_events`, append-only

Schema v24:

```sql
CREATE TABLE IF NOT EXISTS session_events (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  run_id          TEXT,
  seq             INTEGER NOT NULL,   -- per conversation, monotonic
  kind            TEXT NOT NULL,      -- user|assistant|tool_call|tool_result|system_note|summary|steer|stop
  payload_json    TEXT NOT NULL,
  created_at      INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_session_events_seq ON session_events(conversation_id, seq);
```

The rule, copied from DeepSeek: **anything the model saw must be reconstructible
from these rows.** The `messages` table stays exactly as it is and remains what
the UI reads; `session_events` is the model's view, not the person's. They are
different shapes on purpose, and trying to make one table serve both is what
forced the current split in the first place.

Written by a `RunObserver` (HRN-6), so no call site changes.

### CTX-3 - `agent/context.rs`: assembly moves to Rust

Port, function for function, keeping the same output text:

- `compose_system_prompt` (all the blocks from `composeSystemPrompt`)
- `budget_turns`, `with_summary` (from `context.ts`), `estimate_tokens`
- the folder brief and artifacts brief already in `agent_chat_cmd`

`run_agent` gains `messages: TurnInput` where `TurnInput` is either
`Assembled(Vec<Value>)` (today's path, kept) or `FromLog { conversation_id,
user_turn }` (the new path). Nothing switches over until CTX-4 proves they
agree.

### CTX-4 - The equivalence gate

A golden test in the spirit of [`golden.rs`](../src-tauri/src/agent/golden.rs):
for a fixture conversation with a persona, memory, skills, a surface, session
state and an overflowing history, the Rust assembly and the TypeScript assembly
must produce **byte-identical** message arrays. Export the frontend's result
through a dev-only command and compare in a test.

Ship the switch only when that test is green. Then:

- `agent_chat_cmd` takes `{conversation_id, user_turn}` and assembles in Rust.
- `store.ts` deletes its assembly path and keeps `composeSystemPrompt` only if
  something still needs it for display.
- `scheduler::run_custom_job` gets the same assembly for free, fixing CTX-1's
  drift.

### CTX-5 - What the log then buys

- **Resume.** A run interrupted by an app close can be continued: the log has
  every tool result.
- **Fork.** "Try that again from here, differently" copies events up to a `seq`
  into a new conversation. This is the feature users ask for constantly and it
  is impossible today.
- **Honest compaction.** Summaries become `kind = "summary"` rows pointing at
  the range they replace, instead of a `conversations.summary` column that
  cannot say what it covered.
- **Real "why this answer".** The existing WHY-2 manifest becomes a pointer
  into the log rather than a parallel record.

Build resume and fork as HRN-UI-5. Do not build compaction changes in this pass.

**Honest compaction, as built (2026-09-07).**

The starting claim needs one correction: `conversations.summary` was never
entirely blind — `summary_upto_message_id` next to it does say which message the
summary runs up to. What it could not do is keep more than one. A second
compaction summarizes the first summary and overwrites the column, so the
original wording is gone and the conversation holds no account of how it was
compressed.

- **Every compaction is now a `kind = "summary"` row** in `session_events`,
  written by `agent::log::record_compaction` with the text, the range it covers
  (`from_message_id` .. `upto_message_id`), how many messages that is, and
  whether it folded an earlier summary in. No `run_id`: compaction happens
  between runs, while the next request is being assembled, and pinning it to a
  run would make it look like something a run did. The schema already listed
  `summary` as a kind, so there is no migration.
- **The column stays** and assembly is untouched. It is the fast path and the
  thing that gets sent; the log is now the history behind it. Byte-identity with
  the frontend assembly therefore still holds.
- **`replay` skips `summary` rows.** A compaction row is a note about the
  conversation, not a turn in it, and replaying one would hand a resumed run a
  bookkeeping object where a message belongs. It cannot reach `replay` today
  (no `run_id`), but replay is the one reader that must not guess at a kind it
  does not know.
- **A fork now carries its summary across.** This was a real bug, not just a
  gap: `fork_conversation` copied messages with fresh ids and left `summary`
  null, so forking a long conversation silently discarded work already paid for
  — the copy resent every old turn verbatim, overflowed, and compacted again.
  The boundary is re-pointed at the copied message. A summary whose boundary
  sits at or after the cut is dropped instead, because it covers turns the fork
  does not have.
- **`CompactDivider` shows it.** Opening the divider now says how many messages
  stand behind the summary and that they are all still above unchanged; when the
  summary is a summary of a summary it says so plainly, and lists the earlier
  versions with their dates. Fetched on open, so a chat that was never compacted
  pays nothing.
- Tests: `every_compaction_is_kept_not_just_the_latest`,
  `a_conversation_that_was_never_compacted_has_no_summaries`,
  `a_summary_row_is_never_replayed_as_a_message`,
  `a_fork_keeps_the_summary_it_can_still_account_for`,
  `a_fork_drops_a_summary_that_covers_turns_it_does_not_have`.

Still open after this: **compaction only ever runs from the chat window.**
`assembleTurns` in `store.ts` is the only caller, so a scheduled job or a
background subagent that overflows its window drops its oldest turns silently
instead of summarizing them. That is the same shape of drift as `CTX-1`, and it
lands when `agent_chat_cmd`'s assembly moves into `TurnCtx::assemble`.

---

# Track D — Tool output economy

### HRN-8 - Big results get a handle, not a paste

Today a tool's whole output string goes into the transcript. One
`search_folder` over a large repo or one `read_file` on a big file can spend a
third of the window in one call.

In `dispatch_calls`, after a successful call: if the output exceeds
`RESULT_INLINE_CAP` (8 KB), write it to `<data_dir>/results/<run_id>/<call_id>.txt`
and give the model the first 2 KB plus:

```
[truncated: 41 KB total. Use read_result {ref: "res_7f3a", offset, limit} or
search_result {ref: "res_7f3a", query} to see more.]
```

Two new tools in a small `results` module owned by the toolset that produced
the output, so they are only advertised once a reference exists. Files are
deleted when the conversation is deleted, same as artifacts.

The UI half matters here: the timeline step shows `— 41 KB, kept` with the same
`⌄` disclosure `Recall` and `Code` already use, so the user can read what the
model chose not to.

---

# Track E — Code as orchestration

### RPC-1 - A tool RPC endpoint for the sandbox

The two pieces already exist. [`preview.rs`](../src-tauri/src/agent/preview.rs)
is a token-gated loopback HTTP server on `127.0.0.1`, and
[`sandbox.rs`](../src-tauri/src/agent/sandbox.rs) `Profile` already injects
`extra_env` into the confined subprocess.

Add a second loopback route, `POST /<token>/tool`, accepting
`{name, arguments}` and dispatching through the **same** `dispatch` function
the model's calls go through. The token is per run, injected as
`POIESIS_TOOL_URL` and `POIESIS_RUN_TOKEN`.

Non-negotiable properties:
- The token is minted per `run_code` call and revoked when the call returns.
- The call goes through `dispatch`, so permissions, folder trust, untrusted
  marking, activity logging and headless refusal all apply unchanged.
- Every RPC call emits its own timeline step, nested under the `run_code` step.
  A script that quietly made forty tool calls with no trace is exactly the
  thing this product is not.
- The toolset allowlist for RPC is the run's own, minus `CodeExec` itself.

### RPC-2 - A tiny client library

Inject a `poiesis` module onto the scratch path so a snippet can write:

```python
from poiesis import tool
pages = [tool("web_search", query=q) for q in queries]
tool("write_file", path="out/summary.md", content=render(pages))
```

Same for Node. Twenty lines each, no dependencies, just `POIESIS_TOOL_URL`.

### RPC-3 - Teach it in the tool description

`run_code`'s description gains one paragraph: when a job is a fixed sequence
over many items, write a script that calls the tools in a loop instead of
making one model turn per item. This is where the context saving actually comes
from, and a model will not discover it on its own.

---

# Track F — Cost and telemetry

### OBS-1 - Capture usage *(Phase 0)*

`drive_turn` in `runtime/proxy.rs` and the cloud paths already parse streaming
responses. Capture the `usage` object (OpenAI-compatible and Anthropic both
send one on the final chunk when asked) into
`TurnOutcome::Final { content, usage }` and `ToolCalls`.

New table, schema v24 alongside `session_events`:

```sql
CREATE TABLE IF NOT EXISTS run_usage (
  run_id          TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  model_name      TEXT NOT NULL,
  provenance      TEXT NOT NULL,   -- local|cloud|endpoint
  prompt_tokens   INTEGER NOT NULL,
  output_tokens   INTEGER NOT NULL,
  turns           INTEGER NOT NULL,
  created_at      INTEGER NOT NULL
);
```

Local runs record tokens with zero cost. That is still worth having: it is how
context pressure becomes visible.

### OBS-2 - Cost per run, per conversation, per day

A `RunObserver` implementation summing into `run_usage`, plus
`usage_summary_cmd(range)` for Settings. Cloud cost needs a per-model price
table; keep it a small constant map with an "unknown" fallback rather than a
fetched catalogue, and show tokens when price is unknown.

### OBS-3 - A context meter

`estimate_tokens` already exists in the frontend and moves to Rust in CTX-3.
Expose the current turn's assembled size against the model's window as part of
the run's first event, so HRN-UI-3 can show it.

---

# Part — Frontend tasks

### HRN-UI-1 - Type while it works *(Phase 0, ship this first)*

The composer accepts input during a run. Enter sends it to `steer_run_cmd`
instead of queueing it for the next turn. The message appears in the transcript
right away with a small "sent mid-run" mark, and the run's next iteration picks
it up.

Copy on the composer placeholder while running: `Tell me something while I work`.

This is the single most-felt change in this plan. It is small, it depends only
on Track A, and it should ship first.

### HRN-UI-2 - Parallel steps look parallel

`Timeline` renders concurrently running steps as a group with all their dots
pulsing, not a stack that appears one at a time. A group header reads
`3 things at once`. When they settle, they collapse back into ordinary rows in
call order.

### HRN-UI-3 - The run meter

A quiet line under the timeline while a run is live:

`step 4 of 12 · 0:41 · context 38%`

and, when cloud usage is known (OBS-2), a token count. On a run that ended at a
cap it stays visible and reads `stopped at my step limit, this is what I had`,
which is the visible half of HRN-5.

### HRN-UI-4 - Result references are openable

A step whose output was kept on disk (HRN-8) gets the `⌄` disclosure showing
the full text with a "save to the working folder" action.

### HRN-UI-5 - Fork and resume

On any assistant message, two actions in the existing message menu:
- **Try again from here** - forks the conversation at that `seq` into a new one
  (CTX-5) and reruns the last user turn.
- **Continue** - on a run that ended `aborted` or `timeout`, resumes it with the
  full tool history intact instead of starting over.

Both are impossible before CTX-2 and trivial after it. Do not attempt a partial
version earlier.

### HRN-UI-6 - Settings

- Tools: "Let a script call my tools" (RPC-1, default off, marked sensitive,
  with the line that every call it makes still asks the same permissions).
- ~~Models: "Use my local model for small mechanical steps" (HRN-7).~~ Removed
  with HRN-7 on 2026-09-08.
- A new Settings → Usage panel: tokens and cost by day, by conversation, by
  model (OBS-2). Local runs show tokens and "on your device, no cost".

---

# Tests

- **HRN-T1** `fleet.rs`: steering a closed run is a no-op; `cancel_tree` reaches
  grandchildren.
- **HRN-T2** `run.rs`: an inbox entry drained mid-run lands as a user message
  before the next request, and never inside a tool round.
- **HRN-T3** `run.rs`: parallel partition puts `web_search` and `read_file` in
  the concurrent set, `write_file` and `browse` in the serial set, and results
  are appended in original call order regardless of finish order.
- **HRN-T4** `run.rs`: hitting `max_iterations` produces one tool-free wrap-up
  turn and a `max_steps` stop reason, and the returned text is non-empty when
  the run had produced anything.
- **HRN-T5** `run.rs`: HRN-6 refactor is behaviour-preserving. The existing
  `run.rs` test module passes with no edits.
- **CTX-T1** the equivalence gate of CTX-4, byte-identical assembly.
- **CTX-T2** `session_events` replay of a fixture run reproduces the exact
  message array that was sent.
- **CTX-T3** forking at a `seq` copies events up to it and nothing after.
- **HRN-T6** `results`: an 8 KB boundary output stays inline, 8 KB + 1 is
  offloaded, and `read_result` with an offset returns the right slice.
- **RPC-T1** an RPC call with a stale or wrong token is refused; a call to a
  toolset outside the run's allowlist is refused; a write during a headless run
  is refused.
- **HRN-T2/T4 as built (2026-09-07).** Both needed the loop's preamble, which
  used to be reachable only by running a real model. `HRN-6` had already cut it
  out; a further split moved everything that touches only the transcript into
  `open_turn(run, limits, tools_enabled, st, obs)`, leaving `prepare_turn` with
  the parts that talk to the sink, the cancel flag and the log watermark. The
  tests are then plain function calls with no engine, database or socket:
  `a_mid_run_instruction_lands_as_a_user_message_at_the_top_of_the_next_turn`
  (order preserved, drained exactly once, appended at the end so a tool round is
  never split), `a_lead_redirecting_a_child_is_labelled_but_a_person_is_not`,
  `running_out_of_steps_buys_one_closing_turn_rather_than_an_abort` (and only
  one), `a_run_that_is_out_of_time_also_gets_its_closing_turn` (`SUB-T7`), and
  `a_run_without_tools_is_never_told_to_wrap_up`. What is still not covered here
  is the other half of `HRN-T4`'s sentence — that the returned text is non-empty
  — because that is `wrap_up_turn`, which does make a model call.
- **An empty answer after real work is rescued (2026-09-07, found in live use).**
  `HRN-5` made the *step cap* end with an answer, but the same loss arrived by
  another door: a model that finished a run of tool calls and then produced an
  empty turn ended the run `completed` with nothing on screen, throwing away
  everything in the transcript. Seen with three delegated agents whose reports
  came back and were never reported. `RunState::rescue_empty_answer` asks once,
  with the results still in view (`ANSWER_NOW_PROMPT`), and only when the run
  actually called a tool — a run with no results must not be told to use them.
  Tests: `a_run_that_did_work_and_then_said_nothing_is_asked_for_the_answer`,
  `the_answer_is_asked_for_once_and_not_again`,
  `a_run_that_never_used_a_tool_is_not_told_to_report_results`,
  `a_run_that_answered_is_never_asked_again`,
  `whitespace_does_not_count_as_having_answered`.
- **Thinking is a channel of its own (2026-09-07, second live pass).** The
  streaming parser read only `delta.content`, so a reasoning model that spends
  minutes in `reasoning` / `reasoning_content` showed nothing at all: the run
  meter ticking beside an empty screen, reported twice as "stuck on step 1".
  `proxy::Delta { Answer, Thinking }` replaces the bare `FnMut(&str)` callback
  through `stream_turn` → `drive_turn` → `drive_turn_adapting` → `TurnCtx`, and
  `AgentEvent::Thinking { run_id, text }` carries it to a folded `<details>`
  under the meter (`ThinkingTrace` in `AgentRun.tsx`, cleared on each new step
  and on the first word of prose). Anthropic's `thinking_delta` joins the same
  channel. **Thinking never enters `content` and never becomes the message** —
  the type is what enforces it, and
  `thinking_is_relayed_but_never_becomes_the_answer` is the test. This is the
  display half of the decision recorded above; the capture half already existed.
- **Nothing timed out the wait for the first byte (2026-09-07, same pass).**
  `STREAM_IDLE_TIMEOUT` guards the gaps *between* chunks, which means it never
  starts when no chunk ever arrives. A provider that accepted the connection and
  then went quiet held the run open forever, with Stop the only way out.
  `RESPONSE_TIMEOUT` (180s, longer than the idle limit because a free tier may
  legitimately queue) now covers `req.send()` in both `stream_turn` and
  `stream_completion`, and `stream_completion`'s chunk loop — which had no idle
  clock either — was brought in line. Both failures report through one
  `stalled()` helper, because from the outside they are the same event.
- **A model can go nowhere busily (2026-09-07, third live pass).** Neither
  timeout catches a runaway *think*: reasoning deltas keep arriving, so the idle
  clock is refreshed by the very thing that has gone wrong. Seen at ten minutes
  and a hundred thousand characters with no answer. `THINKING_BUDGET` (120s of
  thinking with no word of answer; reset by any content delta, so it measures
  thinking-with-nothing-to-show and not total thinking) cuts the stream and
  returns what the turn has. For an all-thinking turn that is empty, which lands
  in `rescue_empty_answer` — the model is asked once to write the answer, and
  `MAX_EMPTY_RETRIES` bounds it there. Cutting rather than erroring is the point:
  the run still produces something.
- **The meter says what it is doing, not what its budget is (2026-09-07).**
  "step 1 of 12" was the step budget rendered as a status line. It said nothing
  about what the run was doing, and the "of 12" read as a twelve-step plan the
  app had never made. It now shows the running tool (`verb target`), else
  `thinking` when thinking is arriving, else `working`. The budget stays where a
  limit belongs: the setting below, and `StoppedNote` if a run ever hits it.
  Tests: `AgentRun.meter.test.tsx`, five cases, the first of which asserts the
  budget is *not* rendered.
- **The Usage panel was blank for anyone whose provider does not report usage
  (2026-09-07).** `record_run_usage` returned early on zero tokens, on the
  reasoning that "no row" and "zero tokens" mean the same thing. They do not:
  plenty of providers report no usage at all (free tiers, and any
  OpenAI-compatible server that ignores `include_usage`), so those runs were
  dropped and the panel showed nothing after a day of real work. The guard is
  gone; `run.rs`'s `end` closure records unconditionally, the row reads *tokens
  not reported* rather than `0 in · 0 out`, and the panel has a real empty state
  saying what would ever appear there instead of three ledgers each saying
  "nothing yet". `.usage-panel` also had no padding — it renders bare rather
  than inside `.surface`, the same trap `.agents-panel` fell into. Test:
  `a_run_the_provider_never_priced_is_still_a_run`.
- **Reasoning effort is sent, and defaults to brief (2026-09-07).** The app set
  no reasoning parameter at all, so every provider applied its own default —
  which is always its maximum. That is the root cause of the runaway think
  above, not just a cost problem. `cloud::Effort` (`Off | Low | Medium | High |
  Provider`, default `Low`) is read once per run from `models.reasoning_effort`
  into `TurnCtx.effort`. **The two API spellings are not interchangeable and
  were checked against the docs, not guessed:** OpenRouter takes a unified
  `reasoning: { effort }` object with `{ enabled: false }` to disable, and
  OpenAI takes a flat `reasoning_effort` string and rejects unknown top-level
  fields, so `reasoning_field` picks by host. `Provider` sends nothing, which is
  the only honest option for a server we know nothing about, and nothing is ever
  sent to the integrated engine. A provider that refuses the field gets one
  retry without it (`is_reasoning_param_error`, same shape as
  `is_tool_support_error`), so the setting can never cost a turn. Internal
  mechanical calls — retrieval rephrasing, reflection, memory classification —
  pass `Effort::Off` explicitly. Five tests in `cloud/mod.rs`.
  **UI: the composer footer, beside the model picker** (`EffortPicker`), not a
  settings page — it is a property of the answer that model is about to give,
  changed in the same breath as choosing the model. Hidden for image and video
  models, where there is nothing to think about.
- **The step cap is a setting, not a constant (2026-09-07).** `12` was baked
  into `RunLimits::default()`, so "step 3 of 12" read as a law of the app;
  delegated children already had `subagents.max_steps` and the top-level run had
  nothing. `RunLimits::top(db)` reads `agent.max_steps` (default 12, clamped
  1..=50), `commands/agent.rs` uses it, and `StepLimit` in `routes/Tools.tsx`
  exposes it above the toolset list, since it governs any tool-using turn rather
  than one toolset. Test:
  `the_step_limit_is_a_setting_with_a_sane_default_and_a_ceiling`.
- **HRN-7 removed (2026-09-08).** The cheap-steps switch and everything behind
  it are gone; the wrap-up turn runs on the run's own model. Reasoning under
  Phase 2's deliberate deviations.
- **HRN-T1** is covered in `fleet.rs` under prose names:
  `closing_a_run_makes_it_unreachable` and
  `cancel_tree_takes_children_and_leaves_strangers_alone`.
- **OBS-T1** usage from a streamed response lands in `run_usage` with the right
  provenance, and a response with no usage object records zero rather than
  failing the run.

---

# Copy (first person, PRES-0)

| Where | Text |
| --- | --- |
| Composer, while running | Tell me something while I work |
| Steer sent | Got it. I will pick this up after the step I am on. |
| Parallel step group | 3 things at once |
| Run meter | searched job market · 0:41 · context 38% |
| Run meter, thinking | thinking · 2:14 · about 1k in context |
| Run meter, between steps | working · 0:08 |
| Thinking, folded | Thinking (12,480 characters so far) |
| Step-limit setting | How many steps one turn of mine may take |
| Effort picker | No thinking / Think briefly / Think / Think hard / Model's default |
| Provider went quiet | The model never started answering for 180 seconds, so I gave up waiting. |
| Ended at step cap | I stopped at my step limit. Here is what I had. |
| Ended at time cap | I ran out of time. Here is what I had. |
| Kept result | I kept the full result. Open it here. |
| Fork action | Try again from here |
| Resume action | Continue where I stopped |
| RPC setting | Let a script I write call my own tools. It still asks you for anything that needs asking. |
| Usage panel, local | On your device, no cost |

---

# Risks and parked items

**The CTX track is a real migration.** Assembly currently lives where the data
already is (the store has personas, memory, skills, surface, session state in
hand). Moving it to Rust means the backend must load all of that per turn. That
is a database read per turn, which is fine, but it is the reason this track is
sequenced after A, B and F rather than first.

**RPC widens the blast radius of the sandbox.** The sandbox does not block
outbound network on Windows yet (noted in `sandbox.rs`). A script that can call
tools and reach the network is a bigger deal than one that can only do the
latter. Default off, marked sensitive, per-call token, every call in the
timeline. If the AppContainer hardening lands, revisit the default.

**Parallel dispatch and permissions interact.** Two concurrent calls both
wanting a prompt would queue behind one panel. The serial partition avoids it
by construction today; if a read ever grows a prompt, it must move to the
serial set in the same commit.

**Parked deliberately:** a plugin system for the loop driver (the observer trait
is enough), swappable model adapters beyond what `ChatEndpoint` already covers,
distributed or multi-machine runs, and a shared task board across conversations.
