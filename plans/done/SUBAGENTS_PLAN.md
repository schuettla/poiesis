# Project Poiesis — Subagents Plan

**Poiesis should be able to put more than one of itself on a problem, and let
you watch and steer every one of them.**

Today one turn is one agent, one context, one timeline. Work that has several
independent branches (read six files and compare, research four vendors, fix
three failing checks) is done in sequence inside a single context that fills
up and blurs. This plan gives the agent a `delegate` tool that starts child
runs of the same loop in their own conversations, and gives the user a Fleet
surface where those children are visible, steerable, and stoppable while they
run.

> ID prefixes: **SUB** backend (Rust) - **SUB-UI** frontend - **SUB-T** tests.
>
> **Read the shared sequencing below first.** This plan is Phase 1 and Phase 4
> of a run that starts in `HARNESS_PLAN.md`. Do not start `SUB-1` before that
> plan's Phase 0 has landed: without per-run cancellation, a steering inbox and
> a budget that ends with an answer, delegation gets built on assumptions that
> are about to change.
>
> **Internal order once Phase 0 is in: SUB-1 to SUB-5 (a child can run at all)
> → SUB-UI-1/2 (you can see it) → SUB-6 to SUB-9 (you can control it) →
> SUB-UI-3 to SUB-UI-7 → SUB-10 to SUB-12 (background mode, and only after the
> session log lands).** An unwatchable background agent is the failure mode this
> plan exists to avoid.
>
> **PRES-0 (first-person copy) from `POIESIS_PLAN.md` binds every `-UI-` task.**
> §7 is the copy table. Build each UI task with its copy from the start.
>
> **Settled decisions (2026-09-03):**
> - **A subagent is a real `run_agent` call in a real child conversation.** Not
>   a second loop, not a prompt trick. The child gets a `conversations` row, a
>   transcript, artifacts, blocks, and a context manifest, exactly like any run.
> - **Foreground and parallel first, background second.** The lead's tool call
>   waits for its children. The user still sees and steers them while it waits.
> - **One tool, an array of tasks.** `delegate {tasks: [...]}` starts up to
>   three children at once. Not one call per child: the loop dispatches calls in
>   sequence, and small local models emit one call per turn.
> - **A child is never more powerful than its parent.** Toolsets, working
>   folder, and folder trust are inherited and can only be narrowed.
> - **Depth 1 by default.** A child cannot delegate. This is a fork bomb guard,
>   not a philosophy.
> - **Stop means stop everything.** The composer's Stop cancels the lead and
>   every child it started.

---

# Sequencing across both plans

This plan and `HARNESS_PLAN.md` are one programme in six phases. The same table
appears in both files. Each phase ends with something a user can see, so no
phase is infrastructure taken on faith.

| Phase | Tasks | Result |
| --- | --- | --- |
| **0** ✓ | `HRN-1`, `HRN-2`, `HRN-3`, `HRN-5`, `OBS-1`, `HRN-UI-1` | Per-run cancellation, a steering inbox, stop that keeps partial work, budgets that end with an answer, token capture. You can type to a running turn. **Built 2026-09-03 — see “Phase 0 as built” below.** |
| **1** ✓ | `SUB-1` to `SUB-9`, `SUB-UI-1` to `SUB-UI-5`, `SUB-UI-7` | Foreground parallel delegation you can watch, steer and stop. **Built 2026-09-06 — see “Phase 1 as built” in `SUBAGENTS_PLAN.md`.** |
| **2** ✓ | `HRN-4`, `HRN-7`, `HRN-8`, `OBS-2`, `OBS-3`, `HRN-UI-2` to `HRN-UI-4` | Parallel tool calls, big results kept on disk, visible cost. **Built 2026-09-06 — see “Phase 2 as built” in `HARNESS_PLAN.md`.** (`HRN-7`, the cheap model for mechanical steps, was withdrawn on 2026-09-08.) |
| **3** ✓ | `CTX-2`, `CTX-5`, `HRN-UI-5` | The session log. Fork a conversation, resume an interrupted run. **Built 2026-09-07 — see “Phase 3 as built”. `CTX-3`/`CTX-4` followed the same day — see “`CTX-3`, `CTX-4` and `HRN-6` as built”.** |
| **4** ✓ | `SUB-10` to `SUB-12` | Background delegation. **Built 2026-09-07 — see “Phase 4 as built” in `SUBAGENTS_PLAN.md`.** |
| **5** ✓ | `RPC-1` to `RPC-3`, `HRN-6` | Scripts that call tools over RPC, full loop seams. **Built 2026-09-07 — see “Phase 5 as built” and “`CTX-3`, `CTX-4` and `HRN-6` as built” in `HARNESS_PLAN.md`.** |

Two notes on why it is shaped this way:

- **`SUB-0` and `SUB-6` from this plan are `HRN-1` and `HRN-2` in the other
  one.** Build them once, in `agent/fleet.rs`, during Phase 0. The task
  descriptions below are kept for context, not as separate work.
- **`HRN-6` (extracting loop seams) is deliberately last.** It is a
  behaviour-preserving refactor of a 1768-line function with no user-visible
  result. Delegation only needs two small seams (the tagged sink and the inbox
  drain). Doing the full extraction after this plan ships means extracting the
  seams that turned out to matter, not the ones we guessed at.

---

# Part I — State of the art, as of September 2026

Four systems worth copying from, and what each one settles.

**Claude Code / the Task tool (Anthropic).** The lead agent delegates through a
tool; each subagent runs in its own context window and returns only its result,
so the lead's context does not grow with task complexity. Agent types are
markdown files with frontmatter naming the model, the allowed tools, and the
system prompt, plus a `description` the lead reads to decide when to delegate.
The registry-by-description idea is the load-bearing part: the lead picks an
agent the same way it picks a tool.

**Anthropic's research system.** Orchestrator plus workers: the lead plans,
starts three to five specialised children in parallel, and synthesises. It beat
single-agent Opus by a wide margin on breadth-first research and costs roughly
fifteen times the tokens of a plain chat. Two lessons bind this plan: children
need explicit objectives, output formats and boundaries or they duplicate each
other, and the harness needs effort-scaling rules so a small question does not
start a fleet.

**Hermes Agent (Nous Research), async subagents.** Six tools:
`delegate_task_async` returns a task id at once, `list_tasks`, `check_task` for
non-blocking status and recent output, `steer_task` to inject a new instruction
into a running task, `cancel_task`, and `collect_task`, the single intentional
blocking wait. The steering primitive is the piece most harnesses lack, and it
is what makes a long child run recoverable instead of restartable.

**pi-subagents (Pi), FleetView.** The control surface: a persistent widget
above the editor showing every active agent with spinner, tool count, context
use and elapsed time; a navigable list where Enter opens a live conversation
overlay of a running child; steering messages that interrupt after the current
tool call; a background pool with a concurrency limit of ten and automatic
queueing; graceful "wrap up" warnings at max turns so a capped run still
produces a usable partial result.

**DeepSeek Harness.** The cleanest interface definition: a subagent seam with
multiple providers registered by name, one-shot runs (disposable, foreground,
single result) separated from continuable children (durable sessions with at
most one live activation), start-time capability checks, a result type of
`{output, structured, diagnostic, stopReason}` with `stopReason` an extensible
union (completed, aborted, error, max-tokens, refusal), and a strict authority
rule: a parent may message its own child, a child may message its parent, and
siblings may not talk at all.

**What this plan takes:** one-shot foreground children with a typed result and
a `stop_reason` (DeepSeek), parallel fan-out with explicit per-child objectives
and output formats (Anthropic), steering that lands after the current tool call
plus stop-with-partial-result (Hermes), and a live fleet surface with per-child
transcripts (Pi). **What it rejects for now:** sibling communication, durable
continuable children, and unbounded background pools.

---

# Part II — What the codebase already gives us

Reviewed 2026-09-03 against `master`.

**The loop is already re-entrant in shape.** `run_agent` in
[run.rs:632](../src-tauri/src/agent/run.rs#L632) is a free async function over
borrowed state (`&Db`, `&RuntimeManager`, `&PermissionManager`, `&MemoryStore`,
an endpoint, a `conversation_id`, a `CancelFlag`, an `AgentEventSink`). It has
two callers already: the interactive command
[commands/agent.rs:143](../src-tauri/src/commands/agent.rs#L143) and the
scheduler's unattended job
[commands/scheduler.rs:446](../src-tauri/src/commands/scheduler.rs#L446).
Calling it again from inside a tool is a third caller, not a new subsystem.

**The scheduler already proves the child-conversation pattern.**
`run_custom_job` creates a fresh conversation per run, copies the parent scope
with `set_conversation_folder`, runs headless with a no-op channel, and returns
the final text. A subagent is that, plus a live sink and a control handle.

**Personas are already agent types.** The `personas` table has
`system_prompt`, `model_id`, `params_json`, `tools_json` (a toolset allowlist)
and `skills_json`. `ToolRegistry::build` already narrows a run's toolsets by the
conversation's persona and intersects with the global toggles
(`enabled_for_persona`, [toolsets.rs:510](../src-tauri/src/agent/toolsets.rs#L510)),
and a persona cannot re-enable a toolset the user switched off. Two fields are
missing: a `description` for the lead to choose by, and a flag marking a persona
delegatable.

**Headless mode is already the safety valve.** `ctx.headless` (SCH-3) makes the
File System toolset refuse writes rather than open a prompt no one can answer,
makes the Browser toolset refuse outright, and skips renders. Background
children reuse it unchanged.

**The event stream is a single typed enum.** `AgentEvent`
([mod.rs:38](../src-tauri/src/agent/mod.rs#L38)) already carries steps,
tokens, artifacts, blocks, permissions, untrusted marks, file changes and
browser state, and every emit goes through `AgentEventSink`
([run.rs:281](../src-tauri/src/agent/run.rs#L281)). Wrapping a child's events is
therefore a change at one place, not thirty.

**Four things block a child run today:**

1. **Cancellation is global, not per run.** `RuntimeManager::new_cancel`
   ([manager.rs:251](../src-tauri/src/runtime/manager.rs#L251)) overwrites a
   single `Option<CancelFlag>` slot. A child calling it would silently steal the
   parent's Stop button. Needs a per-run flag plus a registry.
2. **There is no way to reach a run from outside it.** Nothing maps a run id to
   its cancel flag or to an inbox, so nothing can steer or stop one child.
3. **The loop has no depth, no per-run step budget, and no wall clock.**
   `MAX_ITERATIONS` is a file constant (12) and there is no timeout at all.
4. **The sink is bound to one Tauri `Channel`.** Child events would arrive
   indistinguishable from the parent's own steps.

**Not available, so do not design around it:** there is no token accounting
anywhere in the proxy or cloud layer. Fleet meters show steps and elapsed time,
not tokens or cost. Adding usage capture is out of scope here.

---

# Part III — The shape

```
lead run (conversation A, message M)
  └─ tool call: delegate { tasks: [ {agent, task}, {agent, task} ] }
       ├─ child run 1  → conversation A1  (own transcript, own artifacts)
       └─ child run 2  → conversation A2
         both stream events, tagged with their run id, into A's event channel
         both are listed in `subagent_runs`, steerable and stoppable by run id
       ← the tool returns a report per child; the lead writes the answer
```

Three invariants that make it safe:

- **Narrowing only.** Child toolsets = parent's effective toolsets ∩ child
  persona's allowlist. Child folder = parent's folder. Child trust = parent's
  trust, never higher. Depth increments, never resets.
- **One owner of the reply.** The lead synthesises. A child's text never
  reaches the user as the answer; it reaches the lead as a tool result and the
  user as an openable transcript.
- **Everything a child does is attributable.** Every step, artifact, file
  change and permission prompt from a child is labelled with the child.

---

# Part IV — Backend tasks

### SUB-0 - Per-run cancellation and a run registry

**This is `HRN-1` in `HARNESS_PLAN.md` and belongs to Phase 0. Kept here for
context; do not build it twice.**

New `src-tauri/src/agent/fleet.rs`, held as Tauri state.

```rust
pub struct RunHandle {
    pub id: String,                    // "run_<uuid simple>"
    pub cancel: CancelFlag,
    pub inbox: Mutex<Vec<String>>,     // SUB-6 steering messages, FIFO
    pub parent: Option<String>,        // parent run id
    pub depth: usize,
    pub agent: String,                 // persona name, or "general"
    pub conversation_id: String,
    pub started_at: i64,
}

pub struct Fleet { runs: Mutex<HashMap<String, Arc<RunHandle>>> }
impl Fleet {
    pub fn open(&self, ..) -> Arc<RunHandle>;
    pub fn get(&self, id: &str) -> Option<Arc<RunHandle>>;
    pub fn close(&self, id: &str);
    pub fn children_of(&self, id: &str) -> Vec<Arc<RunHandle>>;
    pub fn cancel_tree(&self, id: &str);   // the run and every descendant
}
```

`RuntimeManager::new_cancel`/`cancel_active` stay for the top-level turn, but
`cancel_active` also calls `Fleet::cancel_tree` for the active run id. Stop must
kill children; the current single-slot flag is why it cannot today.

### SUB-1 - Thread run identity through the loop

`run_agent` gains three parameters: `run: &Arc<RunHandle>` (replacing the bare
`cancel: CancelFlag`, which it now reads from the handle), and a `limits:
RunLimits { max_iterations, deadline: Option<Instant>, max_depth }`.

`MAX_ITERATIONS` becomes `RunLimits::default()` with the current value 12 for
top-level runs, and 8 for children (setting `subagents.max_steps`).

Inside `run_agent_inner`, at the top of the iteration loop, after the existing
cancel check: if `limits.deadline` has passed, push one system message
("You are out of time. Reply now with what you have."), allow exactly one more
turn, then end with `stop_reason = "timeout"`. This is the graceful wrap-up that
turns a capped run into a usable partial result instead of an abort.

### SUB-2 - Child-tagged events

Two new `AgentEvent` variants plus a wrapper:

```rust
SubSpawned { run_id: String, conversation_id: String, agent: String, task: String, index: usize },
Sub       { run_id: String, event: Box<AgentEvent> },
SubEnded  { run_id: String, status: String, stop_reason: String, summary: String, steps: usize, ms: u64 },
```

`AgentEventSink` gains `sub: Option<String>` (the run id) and a private
`fn send(&self, ev: AgentEvent)` that wraps in `AgentEvent::Sub` when tagged.
Every existing helper is rewritten to call `self.send(..)` instead of
`self.channel.send(..)`. `AgentEventSink::child(&self, run_id) -> AgentEventSink`
clones the channel (verify `tauri::ipc::Channel` is `Clone` in the pinned Tauri
2 release; if not, hold it in an `Arc`).

`Sub` deliberately wraps rather than flattens, so permissions, untrusted marks,
artifacts and browser state from a child all reach the UI already attributed,
with no per-variant work.

### SUB-3 - Agent types on top of personas

Schema v23 (`SCHEMA_VERSION` 22 → 23, `db/schema.sql` plus a `if current < 23`
block using `add_column`):

- `personas.description TEXT` - one line, "when to use this agent". This is what
  the lead reads. Empty means not offered.
- `personas.spawnable INTEGER NOT NULL DEFAULT 0` - may be delegated to.
- `conversations.parent_conversation_id TEXT` - set on child conversations. The
  Rail filters these out of the top-level list.
- New table `subagent_runs` (created by SCHEMA, no ALTER needed):

```sql
CREATE TABLE IF NOT EXISTS subagent_runs (
  id                     TEXT PRIMARY KEY,   -- the run id
  parent_conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  parent_message_id      TEXT,
  child_conversation_id  TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  agent                  TEXT NOT NULL,
  task                   TEXT NOT NULL,
  status                 TEXT NOT NULL,      -- running|done|stopped|error
  stop_reason            TEXT,               -- completed|aborted|timeout|max_steps|error
  result                 TEXT,
  steps                  INTEGER NOT NULL DEFAULT 0,
  started_at             INTEGER NOT NULL,
  ended_at               INTEGER
);
CREATE INDEX IF NOT EXISTS idx_subagent_runs_parent ON subagent_runs(parent_conversation_id);
```

Db methods: `create_subagent_run`, `finish_subagent_run`, `list_subagent_runs(conversation_id)`,
`get_subagent_run(id)`.

A built-in default agent type exists in code, not in the table: `general`, no
extra system prompt, inheriting the parent's toolsets. It is what
`delegate {task: "..."}` uses when no `agent` is named, so delegation works
before the user has made a single persona.

### SUB-4 - The Subagents toolset

New `src-tauri/src/agent/subagents.rs`, new `Toolset::Subagents` variant
(id `subagents`, label "Delegation", `sensitive: false`, `default_enabled:
true`). Registered in `Toolset::ALL`, `tool_specs`, `handles`, `describe`,
`execute`, and the `ALL` array length bumped to 14.

One tool:

```jsonc
{
  "name": "delegate",
  "description": "Run one or more independent sub-tasks as separate agents, in parallel, each with its own fresh context. Use when a task splits into parts that do not depend on each other, or when a part would fill your context with material you only need the conclusion of. Do not use for a single short step you can do yourself.",
  "parameters": {
    "tasks": [{
      "agent": "string, optional - an agent type by name; omit for a general agent",
      "task": "string, required - the complete objective. The agent sees only this, not our conversation.",
      "output": "string, optional - the shape of the answer you want back, e.g. 'a bullet per file with its purpose'"
    }]
  }
}
```

Argument leniency, because local models are the hard case: accept `tasks` as an
array, a single object, or a bare string; accept `prompt`/`instruction` as
aliases for `task`. Reject with a usable message when `task` is empty.

`describe` returns `("delegated", "3 agents")` or `("delegated", "<agent> · <task clipped to 48>")`.

Toolset description for Settings: "Let me split a job across several agents
working at the same time, each with its own fresh context. I stay in charge and
write the final answer."

### SUB-5 - Executing a delegation

In `subagents::execute`:

1. **Guard.** If `ctx.depth >= max_depth` return an error result telling the
   model to do the work itself. (Also: `ToolRegistry::build` omits the
   `delegate` spec entirely at max depth, so this is belt and braces.)
2. **Cap.** Clip `tasks` to `subagents.max_parallel` (default 3, max 5), and to a
   per-turn total of 6 children. Say so in the returned text when clipped.
3. For each task: resolve the agent type (persona by name, else `general`);
   create a child conversation titled `<agent> · <task clipped>` with
   `parent_conversation_id` set; copy the parent's folder and trust; write the
   `subagent_runs` row; `Fleet::open` a handle at `depth + 1`; emit
   `SubSpawned`.
4. Compose the child's messages: one system message = the agent type's
   `system_prompt` (if any) + the subagent contract (below) + the parent's
   folder brief; one user message = the task text plus `output` when given.
   **The child never receives the parent's transcript.** That is the whole point.
5. Run all children concurrently with `futures::future::join_all` over
   `run_agent(..)` calls, each with `sink.child(run_id)`, the child's own
   `RunHandle`, `RunLimits { max_iterations: 8, deadline: now + 5 min }`, and a
   `ToolRegistry` narrowed to the parent's effective toolsets minus `Subagents`
   (at max depth).
6. On each child's return: `finish_subagent_run`, emit `SubEnded`, `Fleet::close`.
7. Return one text block to the lead:

```
Agent 1 (researcher) - completed in 41s, 6 steps
<the child's final text, clipped to 4000 chars>

Agent 2 (general) - stopped early (timeout), 8 steps
<partial text>
```

Plus a trailing line naming any artifacts or changed files the children produced,
so the lead can reference them. Set `set_step_note` to
`"— 2 agents, 47s"` so the timeline row says something true.

**The subagent contract** (appended to every child system prompt, fixed text):

> You are a sub-agent working for a lead agent. You were given one task and you
> cannot see the conversation it came from. Do the task and finish. Your final
> message is the whole result the lead receives, so put everything that matters
> in it and do not ask questions, because nobody will answer. If you cannot
> finish, say what you found and what is missing.

### SUB-6 - Steering a running child

**The drain mechanism is `HRN-2` in `HARNESS_PLAN.md` and belongs to Phase 0.
What stays here is only the child-facing half: the Steer control on a Fleet row
(`SUB-UI-1`).**

`RunHandle::inbox` is drained at the top of each loop iteration in
`run_agent_inner`, right after the cancel check. Each queued message is pushed
as `{"role": "user", "content": "New instruction from the lead: ..."}`. This
lands after the current tool call finishes, which is the Hermes semantics and
the only point where the transcript is in a valid state.

`Fleet::steer(run_id, text)` pushes to the inbox. Exposed as
`steer_run_cmd(run_id, text)`.

Because the inbox lives on every `RunHandle`, the same mechanism steers the
top-level run. Wiring that to the composer is SUB-UI-6.

### SUB-7 - Stopping a child, keeping what it has

`stop_run_cmd(run_id)` calls `Fleet::cancel_tree(run_id)`. The child's loop
already returns `final_text` on cancel, so the partial answer survives: record
it with `status = "stopped"`, `stop_reason = "aborted"`, and hand it to the lead
labelled as stopped by the user. The lead must be told, or it will report a
half-finished branch as a finding.

### SUB-8 - Permissions and headless rules for children

A foreground child runs with `headless: false` and may prompt. The prompt
reaches the UI as `Sub { run_id, Permission { .. } }` and the consent panel says
which agent is asking (SUB-UI-4). A child spawned by an already-headless run
(the scheduler) stays headless.

The `PermissionManager` is shared and keyed by request id, so no change is
needed there. Do check that two children prompting at the same time both
resolve; if the panel is single-slot, queue them (SUB-UI-4).

### SUB-9 - Commands and read-back

New commands in `src-tauri/src/commands/subagents.rs`, registered in `lib.rs`:

- `list_subagent_runs_cmd(conversation_id) -> Vec<SubagentRun>`
- `get_subagent_run_cmd(id) -> Option<SubagentRun>`
- `steer_run_cmd(run_id, text)`
- `stop_run_cmd(run_id)`
- `list_agent_types_cmd() -> Vec<AgentType>` (spawnable personas plus `general`)

Reading a child's transcript needs no new command: it is a conversation, so the
existing `list_messages` / artifacts / blocks commands serve the Fleet viewer.

### SUB-10 to SUB-12 - Background mode (second pass, after the UI lands)

- **SUB-10.** `delegate {background: true}` returns run ids immediately. Runs are
  spawned with `tauri::async_runtime::spawn` and an `AppHandle`, taking state
  inside the task the way `scheduler::execute_job` does, because a detached task
  cannot borrow the command's `State`. Background children are `headless: true`:
  no one is watching, so a write prompt they cannot answer must be a refusal.
- **SUB-11.** Two more tools for the lead: `check_agents {}` (run id, agent,
  status, last step, elapsed) and `collect_agents {run_ids, wait: bool}` (block
  until done, return reports). Deliberately no `steer_agent` tool: steering is a
  user control first, and a lead that steers its own children is a second-order
  feature.
- **SUB-12.** A background pool with `max_concurrent` (default 3), queueing the
  rest, and a completion notification into the parent conversation when a
  background child finishes after the parent turn already ended.

---

# Part V — Frontend tasks

The point of this half: delegation is only worth having if you can watch it. A
progress bar is not enough. You must be able to open a running child, read what
it is doing, tell it something, and stop it.

### SUB-UI-1 - Fleet card in the turn

New `src/components/Conversation/FleetCard.tsx`, rendered by `AgentRun` when
`message.subRunIds` is non-empty, above the prose and below the timeline.

One row per child:

```
● researcher      reading 4 of 12 files                     0:41   Open  Steer  Stop
● general         searching the web for "tauri updater"     0:12   Open  Steer  Stop
✓ reviewer        done · 6 steps · 1:04                     Report ⌄
```

- The dot is the agent's colour (hash the agent name to one of six tokens from
  the existing palette).
- The middle column is the child's **current step line**, built from the last
  `Sub { StepStart }` for that run, so the row is never static while work is
  happening. Falls back to the last streamed token clipped to one line.
- `Open` swaps the Workbench to the Agents tab focused on this run (SUB-UI-2).
- `Steer` opens a one-line input in place; Enter calls `steer_run_cmd` and the
  row shows "sent, it will pick this up after the current step".
- `Stop` calls `stop_run_cmd` and the row settles to "stopped, kept what it had".
- A finished row collapses to one line with a `⌄` disclosure holding the report
  text, using the same disclosure the Recall and Code steps already use.

Store work in `src/lib/store.ts`: a `subRuns: Record<string, SubRun>` map, new
cases in the `streamAssistantTurn` event switch for `sub_spawned`, `sub`
(dispatching the inner event into that run's own step/text accumulators) and
`sub_ended`, and `subRunIds` pushed onto the optimistic assistant message the
same way `artifactIds` and `fileChangeIds` already are. Types go in
`src/lib/types.ts` next to `AgentStep`.

### SUB-UI-2 - Agents tab in the Workbench

Add `"agents"` to the `Tab` union in `Workbench.tsx`. The tab appears when the
conversation has any `subagent_runs` row, labelled `Agents` with a count, and
`live` when any run is running (the existing `wb-tab-live` dot).

Extend `useFollowTheAgent`: a new child spawning is a "come look" transition and
switches the panel to this tab, the same way browsing switches to Browser.

The panel is a master-detail: the run list on the left, and on the right the
selected child's live transcript, rendered with the existing `Timeline` and
`RunText` components in read-only mode, plus its artifacts and changed files.
This is the FleetView equivalent and it is what makes a child feel like a real
agent instead of a spinner.

### SUB-UI-3 - Fleet pill in the composer

While any child is running, show a pill beside the Stop button: `2 agents
working`. Clicking it opens the Agents tab. The Stop button's tooltip becomes
"Stop me and the 2 agents I started", because SUB-0 makes that true and the user
must know it before pressing.

### SUB-UI-4 - Attributed permission prompts

The consent panel gains a line naming the asking agent when the request arrived
inside a `Sub` event: "the researcher agent wants to read this file". Queue
concurrent requests rather than replacing the visible one.

### SUB-UI-5 - Agent types in the Personas editor

`PersonaEditor` gains two fields:

- **"When should I use this agent?"** (the `description`). Help text: "I read
  this when deciding who to hand a job to, so say what it is good at."
- **"I can hand work to this agent"** (the `spawnable` toggle). Off by default.

The existing toolset allowlist already in the editor becomes meaningful here:
it is the child's ceiling, and the editor says so under the list ("An agent I
delegate to never gets more than I have myself").

### SUB-UI-6 - Steering the main run

The composer accepts input while a turn is running and, instead of queueing it
for the next turn, sends it to the active run's inbox via `steer_run_cmd`. The
message appears in the transcript immediately with a "sent mid-run" mark. This
falls out of SUB-6 for free and is the single most-felt improvement in the
plan, so do not skip it as a leftover.

### SUB-UI-7 - Settings

Under Settings → Tools, the Delegation toggle gets its caps inline: how many
agents at once (1 to 5, default 3), how many steps each may take (default 8),
how long each may run (default 5 minutes), and whether an agent may delegate
further (off). Copy is in §7.

Under Settings → Rail behaviour: child conversations do not appear in the Rail.
Say it once, where the Rail is described, so a user who spots a child in the
Library is not confused about where it came from.

---

# Part VI — Tests

- **SUB-T1** (`fleet.rs`): `cancel_tree` cancels a two-level tree and leaves a
  sibling tree alone.
- **SUB-T2** (`fleet.rs`): steering a run that has already closed is a no-op, not
  a panic.
- **SUB-T3** (`subagents.rs`): argument leniency - a bare string, a single
  object and an array all produce the same task list; an empty task errors.
- **SUB-T4** (`subagents.rs`): the cap clips five tasks to three and says so.
- **SUB-T5** (`subagents.rs`): at `depth == max_depth` the `delegate` spec is
  absent from the registry and a direct call is refused.
- **SUB-T6** (`subagents.rs`): the child's toolset set is the intersection, and a
  child persona allowing `browser` gets nothing when the parent lacks it.
- **SUB-T7** (`run.rs`): a deadline that has passed produces one wrap-up turn and
  a `timeout` stop reason, not a bare abort.
  Built 2026-09-07 as `a_run_that_is_out_of_time_also_gets_its_closing_turn`,
  against `open_turn` — the transcript-only half of `prepare_turn`, split out so
  the preamble can be tested with no engine and no database. `SUB-T1`/`SUB-T2`
  landed with Phase 0 under prose names in `fleet.rs`
  (`cancel_tree_takes_children_and_leaves_strangers_alone`,
  `closing_a_run_makes_it_unreachable`).
- **SUB-T8** (`db`): schema v23 applies on top of a v22 database and
  `list_subagent_runs` returns rows in start order.
- **SUB-T9** (frontend, vitest): the store folds `sub_spawned` / `sub` /
  `sub_ended` into a `SubRun` with steps and a final report, and a `Sub`-wrapped
  step never lands in the parent message's own timeline.

---

# Part VII — Copy (first person, PRES-0)

| Where | Text |
| --- | --- |
| Toolset blurb | I can split a job across several agents working at the same time, each with its own fresh context. I stay in charge and write the final answer. |
| Timeline step, running | delegating to 3 agents |
| Timeline step, done | delegated · 3 agents, 47s |
| Fleet card header | Agents I started |
| Row, running | still working |
| Row, stopped by user | stopped, I kept what it had |
| Row, timed out | ran out of time, this is what it had |
| Steer confirmation | Sent. It will pick this up after the step it is on. |
| Stop tooltip (composer) | Stop me and the 2 agents I started |
| Empty Agents tab | I have not handed any work out in this conversation yet. |
| Permission prompt prefix | The researcher agent I started wants to |
| Settings, caps | How many agents I may run at once |
| Settings, depth | Let an agent I started hand work out again (off is safer) |
| Child conversation note | I ran this as a sub-task for another conversation. |

---

# Phase 4 as built (2026-09-07)

`SUB-10` to `SUB-12` landed. Foreground delegation is unchanged; background is a
second path through the same child setup, and the split happens after every
child already has its conversation, its fleet handle, its row and its card row.

**Backend**

- `agent/background.rs` — the pool. A `OnceLock<AppHandle>` set from `lib.rs`
  setup (the `media::jobs` arrangement, for the same reason: a detached task
  cannot borrow a command's state), an owned `Spawn` per child, a queue with
  `subagents.max_background` (default 3, hard ceiling 5), and `pump()` called on
  submit and again as each child ends, so the queue drains with no timer.
- `delegate` gained `background`. Read loosely, like every other argument here:
  `true`, `"true"`, `"yes"`, `1` and an `async` alias all mean the same thing.
- `check_agents` and `collect_agents` — the read-back pair. `check_agents` never
  waits and reads the durable rows, consulting the fleet only for a live run's
  step count. `collect_agents` waits by default, gives up after ten minutes or
  the moment the parent is cancelled, and says which children it left behind.
- `Db::set_subagent_status` (`queued` → `running`) and
  `Db::fail_interrupted_subagent_runs`, called at startup.

**Frontend**

- `api.stillWorking(status)` — one predicate, used by the Fleet card, the Agents
  tab, the Workbench tab dot, the composer pill and `loadSubRuns`. Queued and
  working are one thing to every surface: neither is a result.
- A `poiesis-agent-sub` listener in `listenForSelfEvents`. Background children
  emit through a `Channel` whose other end is the app bus, wrapped as
  `Sub { run_id, .. }` exactly like a live child's, so `applySubEvent` and the
  permission panel work unchanged after the turn has ended.
- `applySubEvent` gained `run_started`, which is the only signal a queued child
  got its slot, and a completion toast for a child that finishes alone.

**Where it differs from the task text**

1. **Background children never delegate again** (`max_depth: 0`), whatever
   `subagents.allow_nested` says. A fork bomb with nobody in the room has no
   floor; nested delegation stays a thing you can watch.
2. **A headless parent cannot start one.** A scheduled job is accountable for
   its own run and ends when it ends. Asked for background there, `delegate`
   waits instead and says so in the result rather than refusing the work.
3. **The deadline starts when the child starts**, not when it was queued —
   otherwise a full pool spends a child's whole budget on waiting.
4. **The completion notice is a toast, not a message.** The plan says "a
   completion notification into the parent conversation". Writing a message
   there would put words in the model's mouth that it never said; the Fleet card
   in the original turn already updates itself, and the toast points at it.
5. **`SUB-11`'s `check_agents` reads rows, not the pool.** It therefore answers
   the same way after a restart as during the turn that started the children.
6. **A restart settles orphaned children** rather than the UI guessing. The
   queue is in memory, so `fail_interrupted_subagent_runs` at startup is what
   lets every reader simply believe an unfinished row.

**One thing this phase had to fix that was not in it.** The `cargo test` binary
would not start on Windows (`0xC0000139`): it imports `TaskDialogIndirect`,
which only resolves against the side-by-side comctl32 v6, and a test executable
carries no manifest asking for it. The app binary never had the problem —
Tauri embeds its manifest, which already declares that dependency. `build.rs`
now declares it for every target and opts the app binary back out, because a
second manifest there is a hard link error (CVT1100).

---

# Phase 1 as built (2026-09-06)

Everything in Phase 1 landed. What follows is what was built and every place it
deviates from the task descriptions above — those stay as written, so the
difference is readable.

**Backend**

- `agent/subagents.rs` — the `delegate` tool. `parse_tasks` accepts an array, a
  single object, a bare string, and `prompt`/`instruction`/`objective` aliases;
  only an empty task is refused. `agent_types` returns the built-in `general`
  plus every persona marked `spawnable`. Children run together under
  `futures_util::future::join_all`, each in its own conversation with the
  parent's folder and trust copied over, and the lead gets one report per child
  naming any artifacts it made.
- `run.rs` grew two small structs rather than more parameters:
  - **`RunContext`** replaces the `run` + `limits` pair and adds `fleet` and
    `ceiling`. `run_agent`'s arity is unchanged.
  - **`DelegationContext`** carries what a nested `run_agent` needs (endpoint,
    model name, temperature, the parent's toolsets, the parent's handle, the
    remaining depth) and reaches the toolset through `ToolContext::delegation`.
    `None` there — the `EVL` harness — makes `delegate` report itself
    unavailable rather than panic.
- `ToolRegistry::build` gained `ceiling` and `may_delegate`. The ceiling is
  intersected *after* the persona allowlist, so a child persona that allows the
  Browser gets nothing when the parent lacked it (`SUB-T6`). `may_delegate`
  false withdraws the `delegate` spec entirely, so depth is enforced before the
  model can ask (`SUB-T5`).
- `AgentEventSink` now has one private `send`, and every helper goes through it.
  `sink.child(run_id)` returns a sink whose events leave wrapped in
  `AgentEvent::Sub`, which is why a child's permission prompt, artifact and
  browser state all arrive attributed with no per-variant work.
- Schema **v23**: `personas.description`, `personas.spawnable`,
  `conversations.parent_conversation_id`, and the `subagent_runs` table.
- Commands: `list_subagent_runs_cmd`, `get_subagent_run_cmd`, `stop_run_cmd`,
  `steer_subagent_cmd`, `list_agent_types_cmd`.

**Frontend**

- `FleetCard` inside the turn (Open / Steer / Stop per row, a live step line, a
  `⌄` report on a finished row), the **Agents** tab in the Workbench with the
  child's own timeline, prose and artifacts, the composer's `N agents working`
  pill and the honest Stop tooltip, agent attribution on the consent panel, the
  two new persona fields, and delegation's caps under its Settings toggle.
- The Rail filters child conversations out; Library still shows them.

**Deviations, all deliberate**

- **`describe` returns `("delegated", "3 agents")`** as `SUB-4` specifies. The
  copy table's "delegating to 3 agents" would need the timeline to render a
  present tense it does not have for any other toolset.
- **The Fleet dot uses four palette tokens, not six.** The design system has
  four accents; six would have meant inventing two colours for this feature
  alone.
- **A child's artifacts, file changes and browser state are not folded into the
  parent's message.** They are read from the child's own conversation in the
  Agents tab. Folding them in would have attributed a child's work to the lead.
- **The per-turn cap is counted from `subagent_runs` rows sharing this turn's
  `parent_message_id`**, rather than from run-level state, so it survives
  several `delegate` calls in one turn without new plumbing.
- **`RunLimits::subagent()` is unused**: the caps come from Settings
  (`subagents.max_parallel`, `.max_steps`, `.timeout_secs`, `.allow_nested`),
  clamped in `subagents::capped`.
- **`SUB-UI-6` was already delivered in Phase 0** (`HRN-UI-1`).

**Tests**: `SUB-T3`/`T4` and agent-type selection in `subagents.rs`, `SUB-T5`/
`T6` in `run.rs`, `SUB-T8` in `db`, `SUB-T9` in `store.subruns.test.ts`.
`SUB-T1`/`T2` landed with Phase 0 in `fleet.rs`. 387 Rust tests, 78 frontend.

---

# Part VIII — Risks and parked items

**Cost.** Parallel children multiply token spend, and the codebase measures none
of it. The caps in SUB-UI-7 are the only brake. If token accounting lands later,
the Fleet card should show spend per child; that is the first thing to add.

**Small local models.** A 4B model will delegate badly: vague tasks, or a
delegation for something it should just do. The tool description's "do not use
for a single short step" line and the three-agent cap are the mitigation. If it
misfires in practice, gate the tool by model size rather than by making the
prompt longer.

**Parked deliberately:** siblings talking to each other, durable continuable
children that survive an app restart, a shared task board, git-worktree
isolation for children that write code, and importing `.claude/agents/*.md`
agent definitions the way `skillpack.rs` imports skills. The last one is the
most likely next step, and `IMPORTABLE_AGENTS` in `skillpack.rs` is the pattern
to copy.
