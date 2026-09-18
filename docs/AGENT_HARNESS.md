# The agent harness

*How Poiesis turns a message into work. Current as of 2026-09-14 (the working
tree after commit `03cadad`, schema v29).*

This describes the code as it stands, not a plan. The plans in
[plans/](../plans/) state intent and carry per-item IDs (`HRN-4`, `CTX-3`,
`SUB-5`, `COD-11`...); those IDs appear throughout the source as anchors back
to the reasoning. Where a plan and this document disagree, this document
describes what runs.

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
│  agent/toolsets.rs      15 built-in toolsets + MCP tools, one registry   │
│  agent/*.rs             the toolsets, plus loop-owned tools (plan,       │
│                         read_result) and the coding machinery            │
│  permissions/ · autonomy.rs · memory/ · db/ (SQLite, FTS5, vectors)      │
└──────────────────────────────────────────────────────────────────────────┘
                  │ loopback HTTP (per-session token)
        llama-server · embedding server · reranking server · sd.cpp
                  │ HTTPS (BYOK)
        OpenAI · OpenRouter · Anthropic · MCP servers · IMAP/SMTP · CDP
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
                 was kept, + plan when plan mode offers it)
  request        show plan → drive_turn → hide plan · stream or hold back
                 prose · charge usage
  classify       → Stop(reason) | Calls(calls) | Again
  dispatch_batch run calls (concurrent reads, ordered rest) · record results
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
6. **Unverified change** (`COD-12`): one note if the run edited project files
   and never ran a check afterwards (§7.4).
7. Otherwise `Done` and `completed`.

`ToolCalls` go to dispatch; `Cancelled` stops as `aborted`; an error stops as
`error` with an `Error` event.

### 4.7 The registry

`ToolRegistry::build`, once per run:

1. Every enabled built-in toolset (`toolset.<id>.enabled`, else its default),
   intersected with the chat persona's allowlist, then with the parent's
   toolsets for a child. A persona or parent can narrow, never widen.
2. `delegate` is removed when there is no depth left.
3. Tools whose autonomy class is `Off` are removed (`memory`, `propose_soul_edit`,
   `propose_skill`).
4. `run_task` and `run_command` are removed when `coderun::advertised` says the
   project cannot run them.
5. `run_code`'s description gains the scripting guidance when `tools.script_rpc`
   is on.
6. Enabled MCP connectors' cached tools are appended. Built-ins win name
   collisions, then earlier connectors.

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
| Workspace UI (`present`) | `render_ui` `present` `remember` | on | Typed blocks (incl. `diagnostics`) and a composable surface |
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

Sensitive (flagged in Settings): Web search, Code execution, Run project tasks,
Mail, Browser, Screen & apps.

Loop-owned tools, not toolsets: `plan` (when `agent.plan_mode` is not `never`),
`read_result` and `search_result` (once a result was kept).

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

### 5.4 Plans

[`agent/plan.rs`](../src-tauri/src/agent/plan.rs). The `plan` tool writes or
updates a short list (items with `todo`/`doing`/`done`/`dropped` and a reason
for dropping). Three rules: the plan shown is the plan the model works against
(rendered into the request each turn with `show_plan`, taken back out with
`hide_plan`, so there is only ever one copy); a dropped item stays visible with
its reason; the plan never gates a tool call. Each write emits `Plan`, records a
`plan` row, and sets the step note to the current item. `agent.plan_mode` is
`always`, `auto` (default) or `never`.

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

## 13. The event protocol

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
| `Block` / `BlockUpdate` / `StateUpdate` | typed blocks and session state |
| `Artifact` | the Canvas; moves the sidebar to Artifacts |
| `FileChanged` | tree mark, change set refresh; the first in a run moves the sidebar to Changes |
| `Permission` | the consent panel (file, domain, screen, app, task, command) |
| `MemoryWrite` / `Proposal` | toast with undo / a card to accept or decline |
| `Browser` | the live browser panel (replaced wholesale) |
| `MailSent` | a receipt |
| `SubSpawned` / `Sub` / `SubEnded` | the Fleet card and the Agents view |
| `Done` / `Cancelled` / `Error` | terminal states |

**The trust rule on the frontend.** An event may move the right sidebar
(`setDockView`) but never open or focus a tab: a tab can take the chat the user
is typing in off the screen. Only a user action opens a file, artifact, run or
diff tab.

---

## 14. Signals the harness keeps about itself

All local.

| Table | Written by | Read by |
|---|---|---|
| `session_events` | the session log | resume, fork, plan restore, compaction history |
| `run_usage` | every run exit | Settings → Usage |
| `subagent_runs` | delegation | Fleet card, Agents view |
| `tool_stats` | every dispatched call (content-free) | reliability captions, tool cautions |
| `tool_fixes` | fail-then-fix pairs | reflection (pruned at 30 days) |
| `skill_runs` | per run, backfilled with failures | skill outcome reporting |
| `activity_log` | file ops, tasks, MCP calls, untrusted intake, memory events | the Activity list |
| `change_proposals` | anything gated to `ask` | proposal cards |
| `file_trash` | every destructive file op | undo, Recent changes, the Changes view |

---

## 15. Extending it

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

---

## 16. Known limits

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
- **The agent can't see media it generated** (`SEE-1`, not built).
