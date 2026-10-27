# The agent harness

*How Poiesis turns a message into work. Current as of 2026-10-27 (the working
tree after commit `6b1e851`, schema v42).*

This describes the code as it stands, not a plan. The plans in
[plans/](../plans/) and [plans/done/](../plans/done/) state intent and carry
per-item IDs (`HRN-4`, `CTX-3`, `SUB-5`, `COD-11`, `CLI-4`, `MCG-3`, `DEC-5`,
`USE-12`...); those IDs appear throughout the source as anchors back to the
reasoning. Where a plan and this document disagree, this document describes
what runs. Three things the plans describe are **not built**, and are marked
where they would belong: the v4 "quiet stream" redesign of the quick-decision
surfaces (§15), resuming an agent app's own session (§13), and the cognition
plan (`COGNITION_PLAN.md`).

---

## 1. Orientation

The harness is **backend-owned**. Rust drives the model, decides when a turn is
a tool call, runs it, feeds the result back, and streams a typed event for
everything it does. The frontend renders those events and, for the one
interactive entry point, still assembles the prompt it sends (§3).

```
┌─ React / TypeScript ────────────────────────────────────────────────────┐
│  lib/store.ts                                                            │
│    sendMessage → composeSystemPrompt · recallForPrompt · assembleTurns   │
│               → invoke("agent_chat_cmd", { messages, target, ... })      │
│               ◄── Channel<AgentEvent> (and the app bus for background)   │
│  TopBar (tab strip) · Conversation (timeline, plan, fleet, prompts)      │
│  Workbench (sidebar: Files/Artifacts/Agents/Browser/Changes, item tabs)  │
└──────────────────────────────────────────────────────────────────────────┘
                  │
┌─ Rust (Tauri) ──┴────────────────────────────────────────────────────────┐
│  commands/agent.rs      resolve endpoint, insert briefs, open a run      │
│  commands/scheduler.rs  unattended runs (assembles its own prompt)       │
│  agent/fleet.rs         run handles: id, cancel, steering inbox, usage   │
│  agent/run.rs           THE LOOP: prepare → assemble → request →         │
│                         classify → dispatch → record                     │
│  agent/context.rs       prompt assembly in Rust + briefs + budgeting     │
│  agent/log.rs           session log: replay, resume, fork, plans         │
│  agent/toolsets.rs      16 built-in toolsets + MCP tools, one registry   │
│  agent/*.rs             the toolsets, plus loop-owned tools (plan,       │
│                         read_result, harness, ask_user, need_tools) and  │
│                         the coding machinery                             │
│  agent/decide/          the decision seam: fast answers that only add    │
│                         caution (§15)                                    │
│  agent/connector_gate   read / change / send for every MCP + Composio    │
│  engines/               agent apps (Claude Code, Codex, ACP) as          │
│                         delegation targets (§13)                         │
│  permissions/ · autonomy.rs · memory/ · db/ (SQLite, FTS5, vectors)      │
└──────────────────────────────────────────────────────────────────────────┘
                  │ loopback HTTP (per-session token)
        llama-server · embedding · reranking · decision server · sd.cpp
        voice (sherpa-onnx, in process) · agent-app children (stdio)
                  │ HTTPS (BYOK)
        OpenAI · OpenRouter (chat + decisions) · Anthropic · MCP servers ·
        Composio Connect · IMAP/SMTP · CDP
```

---

## 2. Ways a run starts

Every run ends up in the same `run_agent` call. What differs is who builds the
opening message array and which limits apply.

| Entry point | Prompt built by | Limits | Headless |
|---|---|---|---|
| `agent_chat_cmd` (a typed message, a block action) | frontend (`store.ts`), then Rust adds the briefs | `RunLimits::top`: `agent.max_steps` (default 12, clamped 1..50), no clock | no |
| `resume_run_cmd` ("Continue where I stopped") | `log::replay` of the last logged run + `RESUME_PROMPT` | as above; the last plan is handed back | no |
| `scheduler::run_job` (Tasks) | Rust: `context::gather` + `compose_system_prompt` + briefs | 12 steps, 600 s clock | yes |
| `delegate` (foreground child) | `subagents.rs`: the task only, plus the agent type's prompt | `RunLimits::subagent`: 8 steps, 300 s, depth 0 | inherits |
| `delegate { background: true }` | `background.rs`, same as above | same | always |
| `delegate` to an **agent app** (Claude Code, Codex, OpenCode) | `engines::run::run_external`: the task fenced as data, the agent app's own system append | the app's timeout (`engines/run.rs::timeout`), at most `max_parallel` at once | follows the parent |
| `/delegate [agent] <task>` | the same path, with the rules (not the guards) skipped | as above | no |

`agent_chat_cmd` and `resume_run_cmd` both go through `execute_turn`, which
opens a handle in the `Fleet`, builds a `RunContext::top`, runs the loop and
closes the handle. A resumed run is therefore the same kind of run as a fresh
one in every way the fleet, the permission panel and the usage ledger can see.

### 2.1 One interactive turn, end to end

1. **`sendMessage`** adds the user message and an empty assistant message to
   the chat, persists both, and makes sure the engine is ready (starting
   `llama-server` for a local model).
2. Attachments become model content: images as `image_url` parts when the model
   has vision, PDFs as extracted text.
3. **`recallForPrompt`** asks the backend for memory relevant to this message
   (§8), scoped to the chat's project.
4. **`composeSystemPrompt`** layers the prompt (§3).
5. **`assembleTurns`** fits system prompt, history and the new turn into the
   model's context budget and compacts the overflow into a summary if needed.
   The optimistic message the new turn was made from is excluded from history,
   so the user's words are sent once.
6. **`agent_chat_cmd`** resolves the endpoint (`resolve_turn`: local engine,
   cloud provider with a key from the OS credential store, or a connected
   server), the model name, the local side-call endpoint and the context
   window, then inserts the **briefs** (§3.2).
7. **`run_agent`** runs the loop (§4), emitting `AgentEvent`s on the channel.
8. The frontend renders steps as the timeline, prose as streaming text, the
   plan as a card, children as a Fleet card, and side effects (blocks,
   artifacts, file changes, permission prompts, memory writes) in their own
   surfaces.
9. The assistant message is finalized in SQLite with its stop reason, steps and
   a compact `context_refs` record (which persona, facts and lessons reached the
   prompt, never the prompt text), which the "What I'm working from" panel reads.

---

## 3. Prompt assembly

### 3.1 The system prompt, in two places behind one gate

`agent/context.rs` is a byte-for-byte port of `composeSystemPrompt` in
`store.ts`. Both render `fixtures/prompt-assembly.json` and must equal
`fixtures/prompt-assembly.golden.txt` (`context_golden.rs` and a vitest of the
same name). Editing one side alone fails the build;
`UPDATE_PROMPT_GOLDEN=1 cargo test --lib context_golden` rewrites the golden and
the vitest must then agree.

Rust uses its assembly for every run that has no frontend to ask (scheduled
jobs; `gather` reads everything from the database and the memory store). The
interactive path still sends the frontend's assembly.

Blocks, in order. Slowest-changing first, so the provider prefix cache stays
warm:

| Order | Block | Source | Condition |
|---|---|---|---|
| 1 | Base prompt | persona `system_prompt`, else the `system_prompt` setting, else the default | always |
| 2 | *About you* | `PROFILE.md`, the agent's own synthesis | non-empty |
| 3 | *Standing instructions* | `SOUL.md`, user-owned | non-empty |
| 4 | *Project: name* | project instructions (`PRJ-7`), capped at 4000 chars | chat in a project with instructions |
| 5 | Memory index | `MEMORY.md` as recalled for this message | Memory toolset on |
| 6 | Skills available | name + description + when-to-use of enabled Agent Skills (1536/entry, 4000 total) | tools on |
| 7 | Block registry | blocks already live in this chat | tools on |
| 8 | Workspace surface | the current `render_ui` tree + state | tools on |
| 9 | Session state | durable per-chat state (`remember`) | non-empty |
| 10 | Tool guidance | surface, block and plan guidance; the plan sentence depends on `agent.plan_mode` | tools on |
| 11 | Memory guidance | how and when to save; sent even at zero facts | tools and Memory on |
| 12 | Tool cautions | "your `X` tool has failed often lately", from 7-day `tool_stats` | tools on |

Everything block-shaped is gated on tools being on: a model told about
`render_ui` but given no tools imitates tool-call JSON in prose.

### 3.2 Briefs

`context::insert_briefs` adds system messages after the leading system prompt
and before the first real turn. They are Rust-only, because every fact in them
comes from the disk and must match what the tools will enforce:

- **Working folder** (`filesystem::working_folder_brief`): the folder, its
  trust level and what that means.
- **Project** (`project::project_brief`, `COD-2`/`COD-3`): the project card
  (languages, branch, tasks by kind, capped at 1600 chars), a verify sentence
  when a check can run, and the project's `AGENTS.md` or `CLAUDE.md` (8000
  chars), marked as written by the project's authors.
- **Artifacts** (`artifacts::artifacts_brief`): the ids of this chat's
  artifacts, so `read_artifact`/`update_artifact` are reachable.

Folder and project briefs need the File System toolset; the artifacts brief
needs Artifacts.

### 3.3 Context budget and compaction

`assembleTurns` (and `context::budget_turns` in Rust) resolves a budget (the
engine's real context size locally, a per-model table for cloud), keeps the most
recent turns verbatim, and when the rest overflows summarizes the oldest prefix
once via `compact_conversation_cmd`. The summary rides in the system prompt;
every compaction is kept in the session log with the stretch it stands for.
Nothing is deleted. If summarizing fails, the oldest turns are dropped rather
than blocking the send.

### 3.4 Transcript normalization

`run_agent` passes the array through `normalize_transcript`, which merges
back-to-back plain-text messages of the same role. If an engine's chat template
rejects the loop's own message shapes, `drive_turn_adapting` retries once with
`flatten_to_alternating` and keeps that strict mode for the rest of the run.

---

## 4. The loop

[`agent/run.rs`](../src-tauri/src/agent/run.rs). `run_agent` normalizes the
transcript, runs `run_agent_inner`, and always backfills skill-run failure
counts afterwards.

### 4.1 State

- **`TurnCtx`** holds everything that does not change while the run lasts: the
  client, endpoints, database, managers, sink, the `RunContext`, the tool
  registry, the MCP pool, skill read roots, the result store, the plan, the
  ledger and the run start time. It is a struct of borrows on the loop's own
  stack frame, so nothing can dispatch a tool call on a run that has ended.
- **`RunState`** holds what changes turn to turn: the messages, the final text,
  the log watermark, retry and nudge counters, the free plan steps, the strict
  template flag, the wrap-up reason and the iteration count.
- **`RunContext`** says where the run sits: its `RunHandle`, `RunLimits`, the
  `Fleet`, the parent's toolsets as a ceiling (for children), provenance
  (`local`/`cloud`/`endpoint`), the context window and a plan to resume.

### 4.2 The phases

```
sink.run_started; build ToolRegistry; SessionLog.prompt(messages)
loop:
  prepare_turn   flush log · cancelled? → Aborted
                 drain steering inbox into user messages
                 out of time / steps? → push WRAP_UP_PROMPT, mark wrap_up
                 bump steps · RunProgress
  wrap_up set?   wrap_up_turn: one tool-free turn, then stop with that reason
  assemble       registry specs (+ read_result/search_result once something
                 was kept, + plan when plan mode offers it, + harness /
                 ask_user) · narrow_menu may hold tool groups back and offer
                 need_tools (§15, Fast & slow thinking only)
  request        show plan → drive_turn → hide plan · stream or hold back
                 prose (feed_check hands the growing answer to the
                 automatic check) · charge usage
  classify       → Stop(reason) | Calls(calls) | Again
  dispatch_batch tool-call gate (§15) · connector gate (§14) · run calls
                 (concurrent reads, ordered rest) · record results
end: log answer + stop · record run_usage · RunEnded · RunOutcome
```

Every exit goes through one `end` closure, so the stop reason, the log `stop`
row, the usage row and `RunEnded` are written on every path, including errors.

**Stop reasons** (`fleet::StopReason`): `completed`, `aborted`, `timeout`,
`max_steps`, `error`. Anything but `completed` is partial, and the UI says so
under the text ("I stopped at my step limit. This is what I had.").

### 4.3 Steering

`steer_run_cmd` (a user) and `steer_subagent_cmd` (the UI redirecting a child)
push a `Steer` into the run's inbox. It is drained only in `prepare_turn`: never
mid-tool-call, where the transcript is not valid, and never mid-stream. A user's
steer becomes a plain user message; a lead's is prefixed `New instruction:`.
Each is also logged as a `steer` row and announced as a `Steered` event. If the
run finished between keystroke and send, the command returns `false` and the
frontend sends an ordinary message.

### 4.4 Budgets and the wrap-up turn

When the next turn would exceed `max_iterations` (or the deadline has passed),
the loop appends `WRAP_UP_PROMPT` and runs one last turn with no tools. Its
answer is kept and the run stops with `max_steps` or `timeout`, so work is never
thrown away at a limit. If the run had a plan, `plan::wrap_up_message` tells the
model which items it never reached. Turns that only update the plan are free,
up to `MAX_FREE_PLAN_STEPS` (6).

The provider stream has three clocks of its own (`runtime/proxy.rs`): 180 s for
a first response, 120 s of silence mid-stream, and 120 s of thinking that never
produces an answer.

### 4.5 Streaming and holding back

With tools off, prose streams straight through. With tools on, the turn is
buffered until `should_flush_prose` judges it prose (starts on a letter, or 160
chars with no JSON opener, fence, `<think>` or tool name). After that it streams
live, but `safe_prefix` still holds back a fence or a line-initial `{` until it
resolves, so a turn that opens in prose and then writes a tool call does not
leak it. Thinking deltas go out as `Thinking` events and never join the answer.

### 4.6 Classifying a finished turn

In order, for a `Final` turn with tools on:

1. **Text-form tool call** (`parse_text_tool_calls`): `{"name", "parameters"}`
   and variants, fenced or after a preamble, or an invocation line like
   `skill-name:verb {...}`. The name must resolve in the registry (or be a
   skill the prompt listed, which becomes a `skill` call).
2. **Narrated call** (`narrates_a_tool_call`): the model described a call it did
   not make. Up to 2 nudges to make it for real.
3. The unshown text is emitted as the answer.
4. **Empty answer rescue**: after tool work (or a turn of only thinking), one
   system note asks for the answer (`ANSWER_NOW_PROMPT`). A second empty reply
   ends the run; the UI then shows "I did the steps above but ended without
   writing an answer".
5. **Unfinished plan** (`PLN-2`): one note if the plan still has open items.
   With Fast & slow thinking on, a decision first asks whether the answer just
   written finishes an open item (`PlanDone`, `USE-5`), so the card does not
   wait on the model remembering to tick it.
6. **Unverified change** (`COD-12`): one note if the run edited project files
   and never ran a check afterwards (§7.4). With a decision model, an answer
   that *claims* the change works is what raises it (`UnverifiedClaim`).
7. Otherwise `Done` and `completed`.

`ToolCalls` go to dispatch; `Cancelled` stops as `aborted`; an error stops as
`error` with an `Error` event.

### 4.7 The registry

`ToolRegistry::build`, once per run:

1. Every enabled built-in toolset (`toolset.<id>.enabled`, else its default),
   intersected with the chat persona's allowlist, then with the parent's
   toolsets for a child. A persona or parent can narrow, never widen.
2. A toolset that cannot work yet is dropped (`Toolset::ready`, `LRN-5`): Mail
   with no account, Image generation with no engine or cloud backend, `decide`
   with Fast & slow thinking off or no decision model. Its switch is not touched;
   Settings → Tools shows the reason (`ToolsetInfo.needs`).
3. `delegate` is removed when there is no depth left. Its spec is built from the
   run's agent types (`subagents::tool_specs(types)`): `tasks[].agent` is an enum
   of their names, an engine-backed type brings its rule block, and `because` is
   required when the agent is an agent app (§13).
4. Tools whose autonomy class is `Off` are removed (`memory`, `propose_soul_edit`,
   `propose_skill`, send-kind connector tools under `app_send`).
5. `run_task` and `run_command` are removed when `coderun::advertised` says the
   project cannot run them.
6. `run_code`'s description gains the scripting guidance when `tools.script_rpc`
   is on.
7. Enabled MCP connectors' cached tools are appended. Built-ins win name
   collisions, then earlier connectors. A Composio connector contributes only an
   allowlist of its meta-tools (§14).
8. The `harness` and `ask_user` tools (`agent/commands.rs`, `own_tools`) join a
   run that has tools on, `harness` only with the verbs the autonomy ladder
   leaves on (a child or an unattended run keeps only `compact`), `ask_user`
   only where someone is watching.

`narrow_menu` runs after all of this and never touches the registry: it filters
only the specs sent for one request (§15), so dispatch and the text-form parser
still know every tool.

### 4.8 Dispatch

`dispatch_batch` echoes the assistant tool-call message, then:

- **Partitions** the calls (`HRN-4`). Calls from File System reads, Web search,
  Recall and Folder reading run concurrently (`join_all`, announced as one
  `StepsParallel` band); everything else, including any MCP tool or unknown
  name, runs in order. A single concurrent call just runs in order.
- **Checks Stop between ordered calls.** A call that never ran still gets a
  result ("the user stopped this run") so the transcript stays valid.
- **Serves script tool calls** (`RPC-1`) while a `run_code` in the batch is
  running: they arrive on a channel and go through the same `dispatch`, shown
  as nested steps. A script may not start another script.
- **Records in call order**, whatever order results finished in: a
  content-free `tool_stats` row; a `tool_fixes` row when a tool that failed
  earlier this run now succeeds (`FIX-1`); the step line; and the result
  message. A result over **8 KB** is kept on disk (`results.rs`): the model gets
  a 2 KB preview and a handle for `read_result`/`search_result`, the user gets
  the whole text in a `KeptResult` event with "Save to the working folder".
- A failed **built-in** call gets one guided retry note per call id (`GRM-3`).

`dispatch` routes one call: loop-owned tools first (`read_result`,
`search_result`, `plan`), then the owning toolset with a fresh `ToolContext`,
then MCP through a per-run client pool (handshake once, stdio children killed
when the run ends).

### 4.9 The session log

[`agent/log.rs`](../src-tauri/src/agent/log.rs), table `session_events`. The
rule: anything the model saw must be reconstructible from these rows.

- The opening array is stored whole as one `prompt` row before the first
  request; every message appended after it becomes its own row (`user`,
  `assistant`, `tool_call`, `tool_result`, `system_note`). `steer`, `stop`,
  `plan` and `summary` rows describe the run rather than what it sent.
- **`replay(run_id)`** rebuilds the exact message array, skipping the
  descriptive rows. Resume uses it; `resume_run_cmd` also hands back
  `last_plan` so the plan continues instead of vanishing.
- **Fork** (`fork_conversation_cmd`, "Try again from here") copies the chat up
  to just before an assistant turn, keeping persona, model, folder, trust and
  summary, and returns the user turn to send again.
- A run with no `stop` row was killed with the app, which is what "Continue
  where I stopped" picks up.

`RunObserver` is the seam: the log implements it; `()` is the observer for a run
nobody records.

### 4.10 Cost

Each turn's reported usage is added to the `RunHandle` (atomics). On every exit
the loop writes a `run_usage` row (model, provenance, tokens, turns), even with
zero tokens reported, so a provider that reports nothing still shows the run.
Settings → Usage reads it; unknown prices are shown as a floor.

---

## 5. Toolsets

[`agent/toolsets.rs`](../src-tauri/src/agent/toolsets.rs). A `Toolset` is a
**tool group**: it advertises OpenAI specs, claims names, describes a call for
the timeline, and executes it. It is not an Agent Skill (§6).

| Toolset (`id`) | Tools | Default | Notes |
|---|---|---|---|
| File System (`filesystem`) | `read_file` `list_directory` `search_files` `find_symbol` `changes` `write_file` `edit_file` `create_dir` `move_file` `delete_file` | on | Real disk, scope + trust + undo (§9.1); read-before-edit (§7.2) |
| Artifacts (`artifacts`) | `create_artifact` `read_artifact` `update_artifact` `check_preview` | on | Served on a loopback origin (`preview.rs`); `check_preview` runs HTML in headless Chrome, or a code artifact in the sandbox |
| Workspace UI (`present`) | `render_ui` `remember` | on | A composable surface for chats in Workspace mode (`render_ui` is offered only there); the old `present` block tool is gone, blocks other tools leave (a build's diagnostics) are still drawn |
| Recall (`recall`) | `search_history` `read_conversation` | on | FTS over past chats |
| Memory (`memory`) | `memory` `propose_soul_edit` | on | Autonomy-gated |
| Folder reading (`indexing`) | `search_folder` `find_similar` | on | Retrieval over the index; duplicate grouping |
| Skills (`skills`) | `skill` `propose_skill` | on | §6 |
| Delegation (`subagents`) | `delegate` `check_agents` `collect_agents` | on | §5.3 |
| Image generation (`image_gen`) | `generate_image` | off | The chat-tool path; the picker path is §10 |
| Web search (`web_search`) | `web_search` `fetch_url` | off | Leaves the device |
| Code execution (`code_exec`) | `run_code` | off | Snippet sandbox; scripts may call tools when `tools.script_rpc` is on |
| Run project tasks (`code_run`) | `run_task` `run_command` | off | §7.3 |
| Mail (`mail`) | `list_mail` `read_mail` `search_mail` `send_mail` `reply_mail` | off | Direct IMAP/SMTP |
| Browser (`browser`) | `browse` `browser_read` `browser_click` `browser_type` `browser_press` `browser_scroll` `browser_screenshot` | off | Installed Chrome/Edge over CDP |
| Screen & apps (`system`) | `screenshot` `open_app` | off | Not GUI automation |
| Sort many items (`decide`) | `classify` | on, but `ready` only with Fast & slow thinking on and a decision model (local file or a chosen cloud one) | §15 |

Sensitive (flagged in Settings): Web search, Code execution, Run project tasks,
Mail, Browser, Screen & apps.

Loop-owned tools, not toolsets: `plan` (when `agent.plan_mode` is not `never`),
`read_result` and `search_result` (once a result was kept), `harness` and
`ask_user` (§5.5), and `need_tools` (only while a tool group is held back,
§15). Connected apps and MCP servers are not toolsets either: they join the
registry from `connectors` and `mcp` (§14).

### 5.1 `ToolContext`

One per tool call. Plumbing: HTTP client, DB, runtime/embedding/reranking
managers, permission manager, sink, conversation and message ids, call id,
app-data dir, memory store, browser pool. Policy:

- **`local_endpoint`**: the local engine for a toolset's own side call (for
  example classifying a fact's scope). Never the turn's endpoint, so work the
  user did not ask for stays off their cloud bill.
- **`headless`**: nobody is watching. Toolsets skip renders; every change on
  disk is refused before it happens; the browser refuses outright.
- **`rendered`** and **`step_note`**: one render per call, and an override for
  the timeline's result line.
- **`extra_read_roots`** and **`loaded_skills`**: run-wide skill state.
- **`delegation`**: what `delegate` needs to start a child of this run.
- **`rpc`**: the gate a `run_code` call arms for script tool calls.
- **`ledger`**, **`cancel`**, **`run_started_at`**: the coding state (§7).

### 5.2 Renders

`render_block` lets a toolset persist and stream a typed block directly. Skipped
when headless, one per call, 64 KB cap; a skipped render is logged, never a
tool failure.

### 5.3 Delegation

[`agent/subagents.rs`](../src-tauri/src/agent/subagents.rs),
[`background.rs`](../src-tauri/src/agent/background.rs).

- A child is a real `run_agent` call in its **own conversation**, opened in the
  fleet with the parent's run id and depth + 1, so it can be watched, steered
  and stopped. It gets its task and nothing else, not the parent's transcript.
- **Narrowing only.** Toolsets are the parent's intersected with the agent
  type's allowlist; folder and trust are the parent's; depth never resets.
  Agent types are `general` plus personas marked spawnable.
- **Limits.** Up to 5 children per `delegate` call (Settings default 3), 6 per
  assistant turn; each child 8 steps and 300 s. A child's report reaches the
  lead clipped to 4000 chars.
- **One owner of the reply.** A child's text is a tool result for the lead and
  an openable transcript for the user, never the answer.
- **Foreground** holds the call open until every child finishes. **Background**
  starts children from detached tasks and returns their run ids at once;
  children are headless, may not delegate, run at most 3 at a time by default
  (5 hard cap), and re-emit their events on the app bus so the Fleet card and
  Agents view keep working. `check_agents` and `collect_agents` read them back.
  Interrupted background runs are marked stopped at startup.
- Events from a child travel inside `Sub { run_id, event }`, bracketed by
  `SubSpawned` and `SubEnded`. `stop_run_cmd` calls `Fleet::cancel_tree`, which
  stops a run and everything below it, never a sibling tree.
- A child's permission prompt names who is asking and queues behind others.

A delegation target can also be an **agent app**: the `delegate` branch in
`subagents.rs` (`prepare_engine`) starts a child process instead of a
`run_agent` call, with the same fleet handle, conversation, Fleet card, Stop
and report. §13 covers it.

### 5.4 Plans

[`agent/plan.rs`](../src-tauri/src/agent/plan.rs). The `plan` tool writes or
updates a short list (items with `todo`/`doing`/`done`/`dropped` and a reason
for dropping). Three rules: the plan shown is the plan the model works against
(rendered into the request each turn with `show_plan`, taken back out with
`hide_plan`, so there is only ever one copy); a dropped item stays visible with
its reason; the plan never gates a tool call. Each write emits `Plan`, records a
`plan` row, and sets the step note to the current item. `agent.plan_mode` is
`always`, `auto` (default) or `never`.

### 5.5 Commands, and what the model may do to its own session

[`shared/commands.json`](../shared/commands.json) is the one registry of `/`
commands (about 45: name, aliases, group, argument shape, who may run it, what
it needs). The frontend reads it for the menu (`lib/commands.ts`); the backend
reads the same file for what the agent may call. Most commands bind to store
actions that already existed; a few are new capabilities:

- **`harness`** (`agent/commands.rs`): `compact` (clear old tool output the
  model no longer needs verbatim), `suggest` (offer one of the user's slash
  commands, at most once per command per chat), `switch_mode` (ask the user to
  turn a mode on; `workspace` is offered only when the experiment is on) and
  `schedule` (propose a task). Each is gated by an autonomy class: a verb whose
  class is `Off` is not in the spec, and the user decides every proposal
  (`HarnessProposal`, `Suggestion`).
- **`ask_user`**: a real fork with 2-4 options; the run waits on the answer
  (`Question` / `QuestionGone`). Not for permissions, which the panel handles.
- **Plan first** (`/plan`): a run that reads and changes nothing until the user
  approves its plan.
- **Rewind** (`agent/rewind.rs`): a branch plus an optional file undo. The
  original conversation is never edited; it gets one `rewound` row, the
  strongest negative signal there is, which reflection reads.
- **Goal** (`/goal`): the frontend loop keeps re-sending until
  `agent/goal.rs`'s judge says the goal holds. The judge is tool-free and
  deliberately hard to satisfy (prose, broken JSON or "met" with nothing to point
  at is "not yet"). With a decision model the quick answer must agree before
  "met", so it can only make the goal stricter. The loop stops on evidence, not
  step count (`LRN-1`): two continuations in a row in which no tool call
  finished without an error, or the same error in three different rounds,
  ends it as `stalled` with the reason as a note in the chat.
- **Checkup** (`agent/checkup.rs`): "is anything wrong with me", one first-person
  line per area, each probed on its own with a 4 s timeout.
- **Resume refuses to redo a send** (`LRN-2`): `log::outward_done` looks at the
  run's rows first; if a mail was sent, a `git push` ran or a connector send
  completed, "Continue where I stopped" does not start and says why.

---

## 6. Agent Skills

[`agent/skillpack.rs`](../src-tauri/src/agent/skillpack.rs). Folders with a
`SKILL.md` in the open [agentskills.io](https://agentskills.io) format.

- **Two-stage disclosure.** The prompt lists name, description and when-to-use;
  the `skill` tool returns the full body when the model decides it is relevant.
  A second load of the same skill in a run returns a pointer, not the body.
- **Own directories only**: `~/.poiesis/skills/` and `<folder>/.poiesis/skills/`.
  Other agents' folders are not scanned; importing is an explicit copy.
- **Bundled files** become readable for the rest of the run (`extra_read_roots`).
- **Installing is gated.** `propose_skill` raises a proposal under the `skills`
  autonomy class.
- Personas can narrow the skill list; the registry keeps the names so a model
  that "calls" a skill by name is turned into a `skill` call.

---

## 7. Working on code

The coding machinery sits on the project entity (`projects` table: name,
optional root, trust, `exec_policy`, `card_json`, `allow_json`, instructions).
A chat reaches its folder and trust through `Db::conversation_folder`, which
answers with the project's values when the chat has one.

### 7.1 The project card

[`agent/project.rs`](../src-tauri/src/agent/project.rs). Detected at the root
and one level down, cached in `projects.card_json`, cleared when the root
changes, refreshed on demand ("Re-detect"):

- manifests and package managers: `package.json` (npm, pnpm, yarn or bun, by
  lockfile), `Cargo.toml`, `pyproject.toml`, `go.mod`, `Makefile`, `*.sln`;
- **tasks** with an argv, a working directory and a kind (`build`, `check`,
  `test`, `lint`, `other`), classified by name; long-running scripts (`dev`,
  `start`, `serve`, `watch`, `preview`) are left out;
- languages by file count, git work tree and branch (read from `HEAD`),
  `AGENTS.md`/`CLAUDE.md`, README.

`check_task()` picks the task that verifies a change: a check, else a test,
else a build.

### 7.2 Read before edit

[`agent/ledger.rs`](../src-tauri/src/agent/ledger.rs). `read_file` records
path, mtime and size; `edit_file` and `write_file` on an existing file refuse
with an actionable message if it was never read this run or changed since.
`read_file` returns numbered lines; `edit_file` accepts a snippet pasted with
the numbers still on (`strip_line_numbers`). The ledger also counts edits inside
the project and remembers whether a check ran after the last one.

### 7.3 Running tasks

[`agent/coderun.rs`](../src-tauri/src/agent/coderun.rs).

**The gate** (`gate`) returns `Refuse`, `Ask` or `Run`:

| Condition | Result |
|---|---|
| no project folder, or folder read-only | refuse |
| policy `off` (project's own, or `code_run.default_policy` when the project is `inherit`) | refuse |
| headless | run only if the project's **own** policy is `allow`; otherwise refuse |
| policy `allow` | run |
| policy `ask`, task in the project's allowlist | run |
| policy `ask` | ask |

**`run_task(task, args?)`** must name a task from the card. **`run_command
(command, args)`** exists only with expert mode, the `code_run.run_command`
setting and the project's opt-in; it always asks, and `command_refusal` rejects
shells, shell metacharacters, eval flags, `git commit`/`push` and global
installs. Remembered commands are keyed `command argv[0]`, so `git status` never
grants `git push`.

The prompt is `PermissionRequest::execution`: capability `task` or `command`,
argv, project, time budget, and an "always allow" choice that maps to `Forever`
and writes to `projects.allow_json`.

**Execution** (`run_streamed` over `sandbox::run_streaming`) uses
`Profile::task()`: project folder as working directory, 300 s timeout, 4 GB
memory cap, 512 processes, the inherited environment minus secret-looking
variables (`is_secret_name`), `CI=1` and `NO_COLOR`, `.cmd` shims resolved via
`PATHEXT`, and a Job Object that ends the whole tree on Stop. It emits
`TaskStarted`, throttled `TaskOutput` lines (120 ms) and `TaskEnded`; the model
gets a 64 KB tail-biased buffer rendered through the diagnostics parser.

This is **not** network or filesystem isolation, and the sandbox module doc and
Settings say so.

### 7.4 Diagnostics, changes and the reminder

- **Diagnostics** ([`diagnostics.rs`](../src-tauri/src/agent/diagnostics.rs))
  parse rustc/cargo, tsc (both shapes), msbuild, eslint, pytest, Python
  tracebacks, go and Node's test runner (TAP) into
  `{file, line, col, severity, message, code}`, read summary counts, and fall
  back to the last 30 lines. The outcome reads `passed`, `2 failed`,
  `12 errors` or `failed (exit code N)`.
- **Changes** ([`diff.rs`](../src-tauri/src/agent/diff.rs),
  [`changes.rs`](../src-tauri/src/agent/changes.rs)) build a change set from the
  `file_trash` undo snapshots against the current disk: per file a status,
  `+N/−M` and hunks with old and new line numbers. Never from `git diff`, which
  would show the user's uncommitted work as the agent's. The set covers
  everything since the chat's last "Keep all" (`changes.kept_at.<conv>`). The
  `changes` tool renders it for the model; `undo_changes_cmd` restores one file
  or all.
- **The reminder** (`COD-12`): if a run edited project files, a check task
  exists and no check ran after the edit, `classify` adds one note naming the
  task. If tasks cannot run, the note tells the model to say plainly that the
  change is unverified.

### 7.5 Code navigation

[`agent/symbols.rs`](../src-tauri/src/agent/symbols.rs). tree-sitter
(pinned `0.25.10`) with each grammar's own `tags.scm` for Rust, TypeScript/TSX,
JavaScript, Python and Go; JSON top-level keys and Markdown headings by hand. No
language servers.

- **`find_symbol(name, path?)`** walks the folder (same walker and caps as the
  index), parses on demand with a per-file cache keyed by mtime and size, and
  lists definitions first, then same-name-different-case, then whole-word uses.
  Files with no grammar are searched as text.
- **Chunking** (`code_chunks`): the index splits code files on top-level symbol
  boundaries, each piece taking the comments and imports before it, packed up to
  2400 bytes; only a symbol over 4800 bytes is cut, labelled "part i of n".

### 7.6 Code artifacts

`artifacts::run_code` runs a Python or Node artifact in a scratch directory
through the same streaming sandbox (30 s) and returns output plus diagnostics;
the Canvas shows a Run button, and `check_preview` covers code artifacts when
Code execution is on.

---

## 8. The durable self

[`memory/mod.rs`](../src-tauri/src/memory/mod.rs). Plain markdown under the
app-data directory:

```
memory/
├─ MEMORY.md      generated index, never hand- or model-edited
├─ SOUL.md        standing instructions; user-edited, agent only proposes
├─ PROFILE.md     the agent's synthesis of how the user likes to work
├─ facts/         durable facts (frontmatter may carry a project id)
├─ lessons/       reflection output
├─ .trash/        forgotten entries (recoverable)
├─ .quarantine/   unparseable files set aside (recoverable)
└─ .snapshots/    pre-consolidation copies
```

The model calls narrow verbs; this module owns layout and index. Forgetting is
a move to `.trash/`; every write emits `MemoryWrite` with an undo token. Startup
rebuilds the FTS index from disk, quarantines unreadable files, prunes trash and
`tool_fixes`, and sweeps expired facts.

- **Recall** embeds the message and searches the vector store, removing what
  surfaced from the wholesale index so nothing appears twice. **Project
  fencing** (`PRJ-8`): an entry tagged with a different project is not
  eligible; untagged memory is shared. Without an embedding engine recall falls
  back to keyword search and says so.
- **Reflection** (`commands/reflect.rs`) draws at most one lesson when a chat
  ends; `reflected_at` is stamped first, output must parse as JSON, a critic
  call checks the draft, and a failed lesson becomes a proposal.
- **Golden set** ([`agent/golden.rs`](../src-tauri/src/agent/golden.rs)): fixed
  behavioural contracts checked around every self-change, without dispatching
  tools. A change that makes the agent worse is reverted and announced.
  `tests/eval.rs` is the sibling that does dispatch, against fixtures, by hand.

---

## 9. Consent, scope and trust

### 9.1 File access

No filesystem sandbox: the agent works on real files.

1. **Scope.** Paths are canonicalized before any check and must land inside the
   working folder, a persisted grant, a dialog grant or a run's skill roots. A
   relative path with no folder is refused rather than resolved against the
   app's own directory.
2. **Trust.** Read-only / ask-first / full, on the project (or the chat when it
   has none). `permissions::gate(trust, impact)` decides silent, prompt or
   refuse. Reads never prompt; deletes and moves ask at every level.
3. **Undo.** Anything that changes bytes is snapshotted to `file_trash` first;
   `FileChanged` carries the undo token; the Changes view and Recent changes
   restore it; every operation lands in the activity log.

A `PermissionRequest` blocks the loop on a oneshot the UI answers with
`Deny`, `Once`, `Chat` or `Forever`. The same panel carries every other consent
(a browser domain, a screenshot, an app launch, a task or command).

### 9.2 The autonomy ladder

[`autonomy.rs`](../src-tauri/src/autonomy.rs). Every write to the durable self
asks one gate: `Auto` (do it, say so, offer undo), `Ask` (proposal) or `Off`
(the tool is not advertised).

| Class | Default | |
|---|---|---|
| `facts` | auto | memory saves |
| `lessons` | auto | reflection output |
| `profile` | auto | derived |
| `consolidate` | ask | tidy-up |
| `soul` | ask | identity |
| `skills` | ask | identity |
| `email_send` | ask | leaves the machine |
| `screen` | ask | a screenshot can contain anything |

An unknown class resolves to `ask`. The gate never fails open.

### 9.3 Untrusted content

[`agent/untrusted.rs`](../src-tauri/src/agent/untrusted.rs): one
canonicalize, scan and wrap primitive for web results, fetched pages, retrieved
file excerpts, mail bodies and skill content. It blocks nothing: text is wrapped
as data, an `Untrusted` event puts a "from outside" marker on the step, and risk
≥ 2 is logged. The one place a score blocks: risky text cannot become a durable
fact or lesson.

### 9.4 Sandboxes

[`agent/sandbox.rs`](../src-tauri/src/agent/sandbox.rs). Each run gets its own
Win32 Job Object with kill-on-close, a memory cap and a process cap, plus a
wall-clock timeout. Two profiles:

- **Snippets** (`run_code`): scrubbed minimal environment, scratch directory. A
  read-only folder is never handed to a snippet; files a snippet changed are
  named in the activity log.
- **Tasks** (`run_task`/`run_command`): §7.3.

Neither blocks outbound network on Windows (that needs an AppContainer
profile), and neither confines the filesystem beyond the working directory.

Script tool calls (`toolrpc.rs`) use one loopback port with no tokens until a
`run_code` call arms one; the token is revoked when the call ends, and every
call goes back through the arming run's own `dispatch`.

---

## 10. Media

[`media/mod.rs`](../src-tauri/src/media/mod.rs). One request/response pair, a
`MediaBackend` trait and a registry: local `stable-diffusion.cpp`, OpenRouter
(image and video), OpenAI. A backend is available when it has a credential and
`is_ready`.

Creation is not a mode: media models are a group in the model picker, and a bare
"draw..." against a chat model gets an intent chip (`mediaIntent.ts`).
Generation is a background job (`media/jobs.rs`) with a `media_jobs` row carrying
the message id, delivered on the app bus so the result lands in the right turn
after a reload.

---

## 11. Perception

- **Embedding / reranking engines**: extra `llama-server` instances, lazily
  started and idle-stopped. Reranking is optional (Settings → Recall).
- **Vector store**: one table for memory recall and folder retrieval, vectors
  pre-normalized.
- **Indexing** (`index.rs`): user-initiated, background, same ignore rules and
  binary sniff as the file tools, caps of 500 files and depth 6. Prose chunks in
  overlapping 1200-char windows; code chunks on symbols (§7.5).
- **Retrieval** (`search_folder`): dot product plus a keyword bonus, MMR with a
  per-file cap, a floor, one rephrased re-query, then a sufficiency check, so a
  weak hit is reported as weak.
- **Duplicates** (`phash.rs`, `duplicates.rs`): dHash for images, centroid cosine
  for documents. Grouping only.

---

## 12. Unattended runs

[`commands/scheduler.rs`](../src-tauri/src/commands/scheduler.rs). A 60 s
ticker drives jobs stored as JSON in `settings`; one runs at a time. Each run
gets a new conversation (with the job's folder, if any), a prompt assembled in
Rust (§3.1) plus briefs, a fleet handle, 12 steps and a 600 s clock, and
`headless: true`. File changes are refused, renders skipped, the browser
refused, and project tasks run only where the project's own policy is `allow`.
Autonomy applies as usual (`Ask` leaves a proposal). Tasks run only while the
app is open. One test asserts that an unattended run refuses rather than
blocks, under a timeout.

---

## 13. Agent apps (external engines)

[`engines/`](../src-tauri/src/engines/). Claude Code, Codex and OpenCode (any
ACP agent) are **delegation targets, not chat engines**. The loop, prompt
assembly, memory, planning and folder trust stay Poiesis's; an agent app is a
kind of agent type, so everything that makes a delegated child legible (its own
conversation, the Fleet card, Stop, the report to the lead) is reused.

```
lead run_agent → delegate { tasks: [{ agent: "Claude Code", task, because }] }
  └─ subagents::delegate
       ├─ not an engine type → run_agent (§5.3)
       └─ engine type → prepare_engine (guards, rules, handover card)
            → engines::run::run_external
                 ├─ driver spawns the process (scrubbed env, Job Object)
                 ├─ stdout → driver.parse → AgentEvent on the child's sink
                 ├─ approval → permission panel → Decision → driver.answer
                 ├─ steer → driver.steer · Stop → interrupt, then kill
                 └─ ExternalOutcome { text, session, usage, paid_by } → lead
```

- **Catalog and detection.** [`shared/engines.json`](../shared/engines.json)
  names the agents and where to look (install paths, PATH read fresh from the
  registry, the Claude Code that ships inside the VS Code extension). A row per
  agent lives in `engines`; `probe` runs `--version` and the sign-in check;
  `PaidBy` (`plan`, `api_key`, `own`, `free`) is what the user sees as the cost.
- **Three drivers behind one trait** (`driver.rs`): `claude` (stream-JSON over
  stdio; approvals through an MCP tool Poiesis hosts, `--permission-prompt-tool`),
  `codex` (`app-server` JSON-RPC; approvals in-band) and `acp` (the Agent Client
  Protocol; approvals in-band). Each declares `Capabilities` (resume, mid-turn
  steer, interrupt, mounted tools, approval style); a missing ability is shown
  as missing, never faked. Recordings of real sessions are fixtures
  (`fixtures/engines/`) and a fake agent per protocol drives the end-to-end
  tests.
- **Environment.** `engines/process.rs::withheld_from_children` removes
  `POIESIS_*`, every provider key and `COMPOSIO_API_KEY` from the child's
  environment (`LRN-3`; stdio MCP servers get the same treatment). The child
  runs under a Job Object that ends the tree on Stop.
- **What the engine is told.** The task is fenced as data (`fenced_task`,
  `defang`); a short system append says it is a delegate, headless or not, and
  which Poiesis tools it has. A child's words reach the lead in a frame that
  `escape_frame` defangs (`LRN-4`).
- **Permissions: one panel.** An engine's request becomes a
  `PermissionRequest` naming who is asking. Per folder trust, `effective_trust`
  decides whether Codex runs with `on-request` + `workspace-write` (it asks
  nothing, its sandbox keeps it in the folder) or `untrusted` (it asks about every
  step); on Windows an `auto` run first asks `windowsSandbox/readiness` and falls
  back to asking when it is anything but `ready`. Relative paths are read from
  the agent's folder (`approve::outside`). A run in a background `/delegate`
  reaches the panel too; an unattended one gets "left for you".
- **When Poiesis hands work over (`RTE`).** Per agent: a **mode** (`on_request`,
  `suggest` default, `auto`), **rules** (`engine_rules`: plain "use it for" /
  "don't use it for" lines, 140 chars, each with a stable label `U1`, `A1`, a hit
  and an override count), and **guards** (`guards_json`, checked in Rust: code
  projects only, minimum trust, a cap per chat, projects where it never runs).
  The lead sees the rule block in `delegate`'s spec and must cite one in
  `because`; a missing, unknown or *avoid* citation is refused with the reason.
  In `suggest` mode a **handover card** (`capability: "handover"`) asks first:
  *Hand it over*, *Do it yourself* (refused; the rule's override count rises) or
  *Always for this* (sets `engine_rules.auto` for that one rule). A headless
  parent treats `suggest` as `on_request`. An explicit `/delegate <agent>` skips
  mode and rules, never guards or `/external off` (`conversations.engines_off`).
  Rules learn only by proposal (`engines/learn.rs`, target `engine-rule`): three
  overrides in 30 days, or three `/delegate`s for work the lead kept, raises a
  proposal the user accepts or declines. **Try a request** previews the route
  (`engines_route_preview_cmd`, `RTE-7`), which with a decision model asks it
  first and falls back to one chat call.
- **They can use Poiesis's tools** (`MCPS`). The loopback port also serves
  Streamable-HTTP MCP at `/<token>/mcp` (`agent/toolrpc.rs`). A per-run token
  carries the exact specs it may call and works only while Poiesis runs the job.
  `engines/tools.rs` offers four memory tools when the agent's *Let it use my
  memory* switch is on, and Composio's four meta-tools when *Let it use my
  connected apps* is on (`engines.app_tools`; separate, both off). An app call
  from an agent runs `run.rs::relay_composio`, the same code the loop uses, so
  persona grants, the read/change/send gate, the daily cap, redaction and usage
  all apply (§14). `COMPOSIO_MANAGE_CONNECTIONS` is answered by Poiesis itself:
  an agent cannot show a sign-in card.
- **Cost and trail.** A `run_usage` row marks the agent app; Usage has an
  *Agent apps* section. `engine_signals` records what the user did about
  a handover (`override`, `asked`, `checked`), which feeds both the rule
  proposals and the outcome labels in §15.2.

**Not wired:** resuming an agent app's own session. `Request.resume` is `None`
in both the foreground and the background; the resume branches in the three
drivers run only in tests (`Capabilities.resume`, `.interrupt` and `.approvals`
are set and not yet read). Real Codex mounts the tools but its model skips them
about half the time; Claude Code and OpenCode do not.

---

## 14. Connected apps and the connector gate

[`agent/connector_gate.rs`](../src-tauri/src/agent/connector_gate.rs),
[`agent/app_calls.rs`](../src-tauri/src/agent/app_calls.rs),
[`connectors/composio.rs`](../src-tauri/src/connectors/composio.rs). Every MCP
connector (one the user added, or Composio) passes the same two checks before
and after a call.

**Output is outside text** (`MCG-1`). `call_mcp_tool` wraps every result and
every error with `mark_untrusted`, so the "from outside" marker, the risk scan
and the activity row come with it. For Composio, `redact_links` replaces any
`composio.dev` sign-in address with "[sign-in link shown to the person]" first,
on every output and error.

**Three kinds of call** (`MCG-2`, `classify`). Precedence: a *send* by name
(send, reply, forward, post, publish, tweet, invite, share, refund, charge,
pay, transfer; or create/add/new with a message, comment, email, invoice,
payment noun; `draft` is never a send) beats a *read* by the server's
`readOnlyHint`, which beats a *read* by a leading verb (get, list, fetch,
search, read, find, check, count...), and anything else is a *change*.
`destructiveHint` never downgrades. A per-tool override (`tool_overrides`) is
kept with the app's policy; loosening it asks first.

**The gate** (`MCG-3`, `admit`), before the call:

| Kind | Default | Policy (`connector_policy`, per connector and per Composio app) |
|---|---|---|
| Read | runs | `read_policy`: `run` or `ask` |
| Change | asks every time | *Allow once / Always for this app's changes / No* |
| Send | asks every time | *Send once / Up to N a day for this app / No*; cap default 25 (`app_send_log`, counted per **target**) |

It asks through the same `PermissionRequest` panel (capability `app`), in first
person, naming the app and account, with the arguments pretty-printed (capped at
4 KB) as the `diff`. `app_send` is an autonomy class (`ask`); `Off` withdraws
send-kind tools from the registry. **Unattended runs never hang:** reads run, a
change runs only where the policy is already "always", a send only where the
policy is "allow" and the cap holds; otherwise the reply is "I need you to
approve this... I've left it for you" and the call waits in the inbox
(`render_app_proposal` / `run_approved`). Plan-first treats anything but a read
as a change.

**Composio** (`CAP`). The key kind picks the mode and is never stored apart from
the key (`mode_of`): a `ck_` **Connect** key talks to `connect.composio.dev/mcp`
with `x-consumer-api-key`; an `ak_` developer key keeps the Project mode (session,
link, `connected_accounts` REST). Either way:

- The model is offered an **allowlist** of four meta-tools (search, get schemas,
  multi-execute, manage connections). The workbench and bash tools and any new
  upstream meta-tool stay hidden. The description names the connected apps and
  adds the rule about mail ("use the mail tools; use connected apps for email
  only if asked for that account").
- `targets()` reads a `MULTI_EXECUTE` batch entry by entry. The call's kind is
  the **strictest of its targets**; a batch across two apps asks once and names
  both; an unreadable batch is refused with no call. Grants are checked first: a
  persona with app grants may use only those (`persona_app_grants`; no rows means
  every connected app).
- **Connecting from the chat.** `COMPOSIO_MANAGE_CONNECTIONS` for an unconnected
  app is read by `connect_request` (both the `{name, action}` and plain-slug
  shapes); only `add` can lead to a **connect card** (a `connect` block). The
  link is kept in memory for ten minutes and never written to the transcript.
  Polling asks `list` about the waiting app; `add` is never used to check, since
  it mints a new link each time. In Connect mode there is no account list, so the
  mirror (`app_accounts`, one row per app or per account) is filled from answers,
  from calls that succeed, and from **Check again**.
- **Several accounts** per app (up to five working); with two or more, a target
  must name one, and `app_calls::plan` stops it otherwise and tells the model
  the names to choose from.
- **Errors the model can act on** (`scope_hint`): a failed call that mentions
  insufficient scopes gets one note saying reconnecting will not add them.
- **Usage**: every target that ran is a `source = "composio"` row, with no cost.
  `app_call_log` also feeds the outcome labels in §15.

---

## 15. Quick decisions (the decision engine)

[`agent/decide/`](../src-tauri/src/agent/decide/). A small model answers small,
closed questions (yes or no, pick one, rate on a scale), each answer carrying a
probability, in tens of milliseconds. Callers ask through one seam and never see
which engine answered. Everything that changes behaviour is an **experiment**
(§15.5); the five original sites are the exception, a pure swap behind parity
tests.

### 15.1 The seam and the two engines

`decide::ask` takes a `Purpose` and typed `Question`s and returns `Answer`s
(`confidence` is one minus the normalised entropy, computed here for every
answer). There are two backends:

- **Local**: a `llama-server` started by `runtime/decideserver.rs` with the
  decision model (Clef-flash 9B, or the larger Clef when the hardware allows)
  and its projector, on `/v1/systemone`. One instance, lazy start, idle stop
  after five minutes, placement Automatic / Graphics card / Processor
  (`plan_fit` counts the loaded chat model and a local image backend). The
  engine is pre-warmed when a chat turn starts. A **hot** purpose never waits for
  a cold engine or runs on the processor: it returns `Deadline` and the caller
  keeps its old path. A background purpose waits.
- **Cloud**: OpenRouter's `/api/alpha/decisions` (`cloud.rs`) with the key
  already in Providers, the pinned `canonical_slug`, and
  `provider: { data_collection: "deny", zdr: true }` on every request. The
  catalog is read live (`/models?output_modalities=decisions` plus the zero-
  retention list, cached a day); a model with no zero-retention provider cannot
  be picked; a probe checks a fixed three-question request and, for a model
  that lists images, that it can read a picture. Errors are sorted into faults
  (401/403 stops the cloud an hour, 402 ten minutes, 429/5xx falls back to this
  computer and stops it after three in five minutes). Cost is kept per decision
  (`decisions.cost_usd`).

`policy::resolve_backend`: a purpose's own override, then `decide.backend`,
then local. **A background purpose ignores the global choice** (mail triage, the
reflection gate, the outside-text flag, `AppKind` on an unattended run): it
stays on this computer unless the user opts that one in by name. With neither
engine the seam returns `Unavailable` and every caller keeps its old behaviour.
Text typed in the composer follows the backend choice, is held in memory only
(`typing::hold`), and is written only when a hint is acted on or a send reuses
it.

### 15.2 The controller

A table, not a formula (`policy.rs`): each of the 19 `Purpose`s has a row with a
mode (Reflex, Assisted, Verify, Gate), the measure it reads (`PFalse`, a named
option, the top `Level(n)`, the winner), a deadline (300 ms hot locally, 800 ms
over the network), whether it is hot, whether it names an experiment (a purpose
whose experiment is off is not asked), and its bars before and after
calibration. `control.rs` holds budgets (twenty asks a turn, four reviews, 40
sentences per check, with separate counts for the turn and for a check).
**Uncalibrated means advisory:** a decision may only make things stricter or
cheaper, never looser, until `calibration.rs` has fitted the purpose to the
user's own outcomes. Calibrate now (`seed.rs`, labelled sets in
`tests/eval/decide/`) and a weekly refit do that.

**Outcomes beat rationalisations** (`outcomes.rs`). A decision is labelled by
what the user did, read first from tables that already existed
(`engine_signals`, `engine_rules`, `app_call_log`, connector overrides, goal
status) and written to `decision_outcomes` only where nothing records the
action: a held tool group that had to come back is `wrong`; one the run never
missed is `weak_right`; a *No* to a question a rating raised is `right`.

### 15.3 Where it runs in the loop (Fast & slow thinking)

| Hook | What a decision does | Never |
|---|---|---|
| **Tool menu** (`narrow_menu`, `ToolSelect`, `USE-4`) | Per unit (an optional toolset, Connected apps, one MCP connector, Other agents) one yes-or-no question over the message and last reply; a confident *no* holds the unit out of the specs for this request only. `need_tools` brings a held unit back; a direct call to a held tool does too. | A child, a resumed run, a scheduled task, a run with tools off; File access, Recall, Memory, Skills, Workspace UI; `/delegate` keeps Other agents; a sign-in card in progress keeps Connected apps |
| **Handover pre-match** (`prematch_turn`, `HandoverRoute`, `USE-13`) | Matches the message to the user's enabled "use it for" rules before the lead runs, as a note | Overrule a guard |
| **Steering** (`SteerClass`, `USE-5`) | Reads a message sent mid-run as stop / redirect / addition / question | Hold anything but a *calibrated* stop at 0.9 or more (it asks the user) |
| **Plan completion** (`PlanDone`) and **unverified claim** (`UnverifiedClaim`) | Whether the answer finishes an open item; whether it claims a change works when no check ran | Add more than one note per run |
| **Tool-call gate** (`ToolRisk`, `AppKind`, `toolgate.rs`, `USE-12`) | Rates a call (harmless / small / serious / irreversible) and whether it is what the user asked for. Not asked-for or irreversible at 0.8 or more adds a question of its own; a milder doubt becomes a line on the question the tool raises anyway. A read the request did not call for is asked about; a connector tool the name calls harmless can be raised to a send (one answer per tool per day). Agent-app approvals get an in-scope line against the task handed over. | Open what a deterministic gate closes; add a question to an app call or agent app (only a line); an unattended run gets a refusal with the reason, never a hang |
| **Skip a low-risk prompt** (`relax-prompts`) | One question only: writing or editing one file inside the attached folder, and only with a *calibrated* verdict (harmless and small at 0.95, in scope at 0.95) | Deletions, moves, paths outside the folder, sending, running, app calls, agent apps, handovers |
| **Classify** (`agent::classify`, `USE-9`) | The `classify` tool: one closed question about up to 500 items, an answer and a confidence per row, a count per answer for the lead | Summarise (it reads 4000 characters per item) |
| **Page check** (`pagecheck`, `USE-11`) | What the page the browser agent just reached shows; a login or captcha hands the browser to the person | |
| **Answers check themselves** (`autocheck.rs`, `sentences.rs`, `verify.rs`, `SEE-1`) | The answer's text goes to a worker as it grows (`feed_check`); each final sentence is split (`sentences::split`, 40 cap), judged checkable (`ClaimCheckable`: fact or "I did this"), then matched against **this run's own tool results** (untrusted-risky ones left out) by distinctive-word similarity, and a verdict asked only when a passage is found. "I sent / ran / created" is matched against the run's calls that returned without an error. | Search or fetch (the deep check, **Check more deeply**, is the one that does) |
| **Background filters** (`filters.rs`, `USE-8`) | Reflection gate (is there anything to learn from this conversation), outside-text flag | Block anything; the one blocking score is still the memory write |
| **Task condition, mail triage** (`runif`, `mailtriage`, `USE-7`, `USE-6`) | Whether a scheduled task's "only run if" holds; kind, urgency and reply mark of a new mail in the small inbox (`mail_seen`, `mail_sync`) | Leave this computer unless opted in by name |

A hot ask holds a guard from the moment it is made; a check ask (`lane.rs`)
takes the check lock and sends only when no hot ask is in flight, so a person
waiting is never behind a background check. The five original sites
(`RET-4` sufficiency, memory scope, the reflection critic, the goal check, the
rule preview) each ask first and keep their old call as the fallback.

### 15.4 Visibility

Every decision is logged: a `decisions` row (purpose, backend, ms, calibrated,
answers with probabilities, tokens, a 200-character preview, a state hash, the
action taken) and, when a conversation is known, a `decision` session-log row.
Activity shows them in words, folded by the hour where one request asks dozens.
In the chat, v3 puts what a decision did where it acted: a held-tools chip in the
run bar (`ToolsHeld` / `ToolsBack`), a one-line thought strip above the first
text and a receipt after it (`Thought`), the composer hint while typing, and
marks and a trust line on sentences that are checked (the `poiesis-autocheck`
app event, because the check outlives the run). A reason line sits on every kind
of permission prompt. **Not built:** v4, which removes most of that from the
stream and keeps one line when a statement looks wrong; the data and backend stay
as described here.

### 15.5 Experiments

[`experiments.rs`](../src-tauri/src/experiments.rs) is the registry, mirrored to
`lib/experiments.ts` through one command so the two cannot drift. State is
`experiment.<id>` in settings; **both sides gate** (the backend before it
advertises a tool or asks a purpose, the frontend before it shows an entry
point). Each experiment names what it requires (`DecisionModel`, `Vision`, a
provider, another experiment), what it records, and its graduation line.

| Id | What it turns on |
|---|---|
| `fast-slow` | the hooks in §15.3 except mail triage and `relax-prompts`; Verify was folded into it in v3 |
| `mail-triage` | the small inbox and its background fetch (needs a *local* decision model and a Mail account) |
| `relax-prompts` | the skip path above (needs `fast-slow` and a calibrated backend) |
| `workspace` | Workspace mode (migrated on if any conversation already used it) |

Each card shows a 30-day report built from local data (`experiments_report.rs`)
and **Reset its data**, which holds the loop it resets (a mail reset waits for a
running look at the inbox). The user's own switches from other plans (an agent
app's mode, *Let it use my memory*, connector policies) are ordinary settings,
not experiments.

---

## 16. Voice

`runtime/voice*.rs`, `lib/voice/`, `components/Voice*`. Voice is outside the loop:
one in-process engine (`sherpa-onnx`) does voice-activity detection (Silero),
recognition (Parakeet, or Moonshine on small machines) and synthesis (Kokoro,
Piper), on the CPU, with no cloud model. A turn-taking state machine in the
frontend decides when the user has stopped, lets them cut in, and stops playback.
What it hears becomes an ordinary user turn through `agent_chat_cmd`, marked as
spoken (the mark survives a fork); the reply is queued for playback as it
streams (`playbackQueue.ts`). Models are fetched on demand from Settings → Runtime → Voice.

---

## 17. The event protocol

`AgentEvent` ([`agent/mod.rs`](../src-tauri/src/agent/mod.rs)), tagged by
`type`, is the backend→UI contract. `AgentEventSink` wraps the channel;
`sink.child(run_id)` wraps a child's events in `Sub`.

| Event | Rendered as |
|---|---|
| `RunStarted` / `RunProgress` / `RunEnded` | run id for steering, the run meter (step, clock, context estimate), the stop reason |
| `StepStart` / `StepDone` / `StepError` | the timeline (a step can be nested under a script step) |
| `StepsParallel` | one "N things at once" band |
| `Token` | streaming prose |
| `Thinking` | the folded thinking line |
| `Steered` | a mid-run message marked as sent while working |
| `Plan` | the plan card |
| `KeptResult` | a big result behind a disclosure, with save |
| `TaskStarted` / `TaskOutput` / `TaskEnded` | the task step: live last line, outcome, diagnostics, tail |
| `Recall` / `Code` / `Untrusted` | disclosures on a step |
| `UntrustedFlag` | the quick decision model thinks outside text reads like an instruction (`USE-8`); it arrives after the text went to the model, so it only marks the step |
| `Block` / `BlockUpdate` / `StateUpdate` | typed blocks and session state (including the `connect` card for a connected app) |
| `Artifact` | the Canvas; moves the sidebar to Artifacts |
| `FileChanged` | tree mark, change set refresh; the first in a run moves the sidebar to Changes |
| `Permission` / `PermissionGone` | the consent panel (file, domain, screen, app, task, command, engine, handover, risk); a withdrawn question comes off it (a stop, an agent hanging up) |
| `MemoryWrite` / `Proposal` | toast with undo / a card to accept or decline (self-changes, and engine-rule edits) |
| `HarnessCommand` / `HarnessProposal` / `Suggestion` | what the agent did to its own session, a mode or schedule it asks the user to allow (the run does not wait), one `/` command it offers |
| `Question` / `QuestionGone` | a fork the run is waiting on (`ask_user`), or one an agent app put to the person; the card comes off when it is no longer wanted |
| `ToolsHeld` / `ToolsBack` | the run-bar chip for tool groups a quick decision left off the menu, and their return (by the model or by the user) |
| `Thought` | the first-thought strip and its receipt (`SEE-2`) |
| `Browser` | the live browser panel (replaced wholesale) |
| `MailSent` | a receipt |
| `SubSpawned` / `Sub` / `SubEnded` | the Fleet card and the Agents view, for ordinary children and agent apps alike |
| `Done` / `Cancelled` / `Error` | terminal states |

Some things outlive a run, so they travel on the **app bus** instead of the run's
channel: background agents re-emit their events there, media jobs deliver their
result to the right turn after a reload, and the automatic answer check sends
`poiesis-autocheck` (`progress`, `claim`, `done`).

**The trust rule on the frontend.** An event may move the right sidebar
(`setDockView`) but never open or focus a tab: a tab can take the chat the user
is typing in off the screen. Only a user action opens a file, artifact, run or
diff tab.

---

## 18. Signals the harness keeps about itself

All local.

| Table | Written by | Read by |
|---|---|---|
| `session_events` | the session log | resume, fork, rewind, plan restore, compaction history |
| `run_usage` | every run exit | Settings → Usage |
| `subagent_runs` | delegation | Fleet card, Agents view |
| `tool_stats` | every dispatched call (content-free) | reliability captions, tool cautions |
| `tool_fixes` | fail-then-fix pairs | reflection (pruned at 30 days) |
| `skill_runs` | per run, backfilled with failures | skill outcome reporting |
| `activity_log` | file ops, tasks, MCP calls, untrusted intake, memory events, decisions of note | the Activity list |
| `change_proposals` | anything gated to `ask` (including `engine-rule` edits) | proposal cards |
| `file_trash` | every destructive file op | undo, Recent changes, the Changes view |
| `engines`, `engine_rules`, `engine_signals` | Settings → External agents, `delegate`, the handover card | rule hit and override counts, rule proposals, outcome labels |
| `connector_policy`, `app_send_log`, `app_call_log` | the connector gate and every connector call | Settings → Apps, the daily cap, Usage, outcome labels |
| `app_accounts`, `persona_app_grants` | sign-ins, the persona editor | the connect card, the Composio description, the gate's grant check |
| `decisions` | every decision, with its cost when the cloud answered | Activity, Usage, calibration |
| `decision_outcomes` | where nothing else records what the user did | calibration (`CTL-4`) |
| `verifications` | the answer check (`auto` or `deep`, one row a message) | the trust line and marks on a reload |
| `mail_seen` | the small inbox's background fetch | the inbox and its triage marks |

---

## 19. Extending it

**A new toolset:** add a `Toolset` variant (and to `ALL`), a module with
`tool_specs` / `handles` / `describe` / `execute`, and the match arms in
`toolsets.rs`. Pick `default_enabled`; mark it `sensitive` if it leaves the
device or runs code; decide `is_serial` (concurrent only if it never writes).
Route outside text through `mark_untrusted`; render with `render_block`. A tool
that belongs to the run rather than the machine (like `plan`) is dispatched in
`TurnCtx::dispatch` instead.

**A new entry point for runs:** open a handle with `Fleet::open`, build a
`RunContext`, assemble the prompt with `context::gather` +
`compose_system_prompt` + `insert_briefs`, call `run_agent`, and always
`Fleet::close`.

**A new event:** add the variant to `AgentEvent` and its type to `api.ts`;
handle it in the store's stream switch and in `applySubEvent` if children can
send it. If it should move the sidebar, call `setDockView` on a transition only.

**A new diagnostics format:** a parser function in `diagnostics.rs` called from
`parse`, plus a test built from real output.

**A new grammar:** add the crate (checking its rust-version against ours), a
`Lang` variant, its `language()` and `tags_source()`, and a test in
`symbols.rs`.

**A new media backend:** one file under `media/backends/` implementing
`MediaBackend`, one line in `Registry::new()`.

**A new skill:** a `SKILL.md` folder. No code.

**A new self-change class:** add it to `AUTONOMY_DEFAULTS`, call
`autonomy_gate` at the write site, and map its tool in `self_change_class`.

**A change to the system prompt:** change `context.rs` and `store.ts` together
and regenerate the golden (§3.1).

**A new agent app:** an entry in `shared/engines.json`; if it speaks ACP, that is
all the driver it needs (`acp.rs`), otherwise a driver implementing the trait in
`engines/driver.rs`. Record a real session as a fixture first
(`fixtures/engines/`), then write the parser from it, with a fake agent for the
end-to-end test. Declare its `Capabilities` honestly. Do not add a Cargo or npm
dependency to talk to one.

**A new `/` command:** an entry in `shared/commands.json` (both sides read it) and
a binding in `lib/commands.ts`. If the agent may use it too, give it a verb on
`harness` and an autonomy class. There is no other composer syntax and none should
be added.

**A new question for the decision model (a new `Purpose`):** a variant and a row
in `decide/policy.rs` (mode, measure, hot or background, deadline, bars, the
experiment it sits behind), the question builder and the mapping from its answer
back to the caller, a seed set in `tests/eval/decide/`, and a fallback that keeps
today's behaviour when `ask` says no. Decide before writing it whether it can
only make things stricter. A purpose that runs without the user asking is a
background purpose and stays on this computer.

**A new experiment:** an entry in `experiments.rs` (title, summary, what changes,
what it records, requirements, graduation line) and a report in
`experiments_report.rs`. Gate it on both sides.

**A new connector gate rule:** `connector_gate::classify` for what a tool does,
`app_calls::plan` for how a Composio batch is read, and a test built from a
recorded reply (`fixtures/composio/`).

---

## 20. Known limits

- **The interactive prompt is still assembled on the frontend.** Rust has the
  byte-identical port and uses it for scheduled runs; `agent_chat_cmd` has not
  switched over yet.
- **`run_agent` takes 20 arguments.** `TurnCtx` bundles them inside the loop,
  but the public signature has not followed.
- **Sandboxes do not block outbound network** or confine the filesystem on
  Windows.
- **The empty-answer rescue and the guided retry are one shot each**, and the
  retry applies only to built-in tools.
- **MCP tool lists come from the connector's cached config**, so a server that
  changes its tools is not noticed until refreshed.
- **Mail opens a fresh IMAP session per call** rather than pooling per run.
- **`find_symbol` and the index share their caps** (500 files, depth 6), so a
  large repository is only partly covered.
- **The Changes view only knows edits that went through the file tools.** A
  file changed by a task or a script is not in it.
- **The agent can't see media it generated** (the multimodal plan's `SEE-1`, not
  built; unrelated to the dual-process plan's `SEE-*` items).
- **Quick decisions are barely exercised on a real model.** The engine, the
  OpenRouter backend, the policy table, calibration and every use were built
  and tested against recordings, fakes and mock servers; the engine ran against a
  real Clef-flash in step 2 and had one short hand use afterwards. Every
  threshold in `policy.rs` (0.8, 0.6, 0.85, 0.95, the 0.5 similarity floor) is a
  starting value, the seed sets were written by Claude and are unreviewed, no
  request has reached OpenRouter, and the checklist run (step 20 of the plan)
  has not been done. On a graphics card too small for the model,
  hot purposes are skipped while someone is watching and only unattended runs
  reach them (on the processor).
- **The quick-decision surfaces are louder than intended.** The thought strip,
  receipt, trust line and claim marks of v3 are built; the v4 plan to remove most
  of them is not.
- **Agent apps:** resume of an app's own session is unwired (§13); Codex skips
  Poiesis's tools about half the time; what a real Codex answers to
  `windowsSandbox/readiness` when the sandbox is not set up is unrecorded (any
  answer but `ready` is treated as not ready); a `process` test sets
  process-wide environment variables and can make others flaky.
- **Connected apps:** Composio's own refusal for an unnamed account is
  unrecorded, and a successful `add` and a `list` with two accounts have not been
  recorded from a real `ck_` key. The in-chat connect card has not been tried for
  real. A send that went out but was cut off before its result row was written
  looks unfinished, so Resume would offer it again, and a send with an innocent
  tool name is classified as a change.
- **A connector has no field for an environment variable**, so a stdio MCP server
  that needs a token the user exported inherits it; only `POIESIS_*` and the
  model providers' keys are withheld.
- **There is no mail inbox outside the `mail-triage` experiment.** Mail is still
  fetched only when a tool asks; the small inbox and its background fetch exist
  only with the experiment on, a local decision model and a Mail account.
