# The agent harness

*How Poiesis turns a message into work. Current as of 2026-08-11 (commit
`0c1aa36`).*

This is a description of the code as it stands, not a plan. The plans in
[plans/](../plans/) state intent and carry per-task IDs (`LOOP-4`, `SKL-2`,
`TRU-2`…); those IDs appear throughout the source as anchors back to the
reasoning. Where a plan and this document disagree, this document is the one
describing what runs.

---

## 1. Orientation

The harness is **backend-owned**. The Rust side drives the model, decides when a
turn is a tool call, executes it, feeds the result back, and streams a typed
event for everything it does. The frontend does exactly two things the backend
doesn't: it **assembles the prompt** and it **renders the events**.

```
┌─ React / TypeScript ──────────────────────────────────────────────┐
│  store.ts                                                          │
│    composeSystemPrompt()  ── layered system prompt                 │
│    recallForPrompt()      ── memory that surfaced for this message │
│    assembleTurns()        ── context budget + compaction           │
│                 │ invoke("agent_chat_cmd", { messages, target, … })│
│                 │ ◄── Channel<AgentEvent> ───────────────────────  │
│  Conversation / Workbench / Surface  ── render the event stream     │
└────────────────────────────────────────────────────────────────────┘
                  │
┌─ Rust (Tauri) ───┴─────────────────────────────────────────────────┐
│  commands/agent.rs   resolve endpoint (local|cloud), model name,    │
│                      working-folder brief, cancel flag              │
│  agent/run.rs        THE LOOP — drive_turn → dispatch → repeat       │
│  agent/toolsets.rs   registry: 13 built-in toolsets + MCP tools      │
│  agent/*.rs          the toolsets themselves                        │
│  permissions/        consent gate (blocking, user-answered)         │
│  autonomy.rs         what it may change about itself without asking │
│  memory/             the durable self, as markdown on disk          │
│  db/                 SQLite: history, FTS5, vectors, blocks, stats   │
└─────────────────────────────────────────────────────────────────────┘
                  │ loopback HTTP (per-session token)
          llama-server · embedding server · reranking server · sd.cpp
                  │ HTTPS (BYOK)
          OpenAI · OpenRouter · Anthropic · MCP servers · IMAP/SMTP · CDP
```

---

## 2. One turn, end to end

1. **`sendMessage` (store.ts)** resolves the model, gates on engine readiness
   (starting `llama-server` on demand for a local model), and turns attachments
   into model-ready content — images as `image_url` parts when the model has
   vision, PDFs as extracted text.
2. **`recallForPrompt`** asks the backend for memory relevant to *this* message
   (§7). It returns a trimmed index plus the entries that surfaced by meaning.
3. **`composeSystemPrompt`** layers the prompt (§3).
4. **`assembleTurns`** fits system prompt + history + the new turn into the
   model's context budget, compacting the overflow into a summary if needed.
5. **`agent_chat_cmd`** resolves the endpoint — the loopback engine, or a cloud
   provider whose key comes from the OS credential store — and inserts the
   working-folder brief as a system message (only when tools are on and the File
   System toolset is enabled, so it can never describe a scope the file tools
   won't actually enforce).
6. **`run_agent`** runs the loop (§4), emitting `AgentEvent`s on a Tauri
   `Channel`.
7. The frontend renders steps as an **agent-run timeline**, prose as streaming
   text, and side effects (blocks, artifacts, files, permissions, memory writes)
   in their own surfaces.
8. On completion the assistant message is finalized in SQLite along with a
   compact `context_refs` record — *which* persona, facts and lessons reached
   the prompt, never the prompt text — which is what the "What I'm working from"
   panel reads back.

---

## 3. Prompt assembly

`composeSystemPrompt` ([src/lib/store.ts:3678](../src/lib/store.ts#L3678))
concatenates blocks in a fixed order. The order is deliberate: slowest-changing
first, so the provider-side prefix cache stays warm across turns.

| Order | Block | Source | Condition |
|---|---|---|---|
| 1 | Base prompt | persona's `system_prompt`, else the global default | always |
| 2 | *About you, as I understand it* | `PROFILE.md` — the agent's own synthesis | non-empty |
| 3 | *Standing instructions* | `SOUL.md` — user-owned; outranks the persona on conflict | non-empty |
| 4 | *Your notes about the user* | `MEMORY.md` index, minus anything already injected | non-empty |
| 5 | *Skills available* | name + description of enabled Agent Skills | tools on |
| 6 | Workspace block registry | blocks already live in this conversation | tools on |
| 7 | Workspace surface | the current `render_ui` tree + bound state | tools on |
| 8 | Session state | durable per-conversation state (`remember` tool) | non-empty |
| 9 | Tool guidance | surface / block / plan-first guidance | tools on |
| 10 | Tool cautions | "your `X` tool has failed often lately" | tools on |

Two things worth calling out:

- **Everything block-shaped is gated on `toolsEnabled`.** A model told about
  `render_ui` but given no tools to call imitates tool-call JSON as prose, and
  it leaks raw into the transcript.
- **Tool cautions are self-repair with no storage.** `toolCautions` reads
  7-day `tool_stats`, takes the worst two tools with ≥8 calls and <40% success,
  and says so in the prompt. It changes nothing but this one turn.

**Context homeostasis.** `assembleTurns` resolves a budget (the engine's real
context size locally; a per-provider table for cloud), keeps the most recent N
turns verbatim, and when the rest overflows summarizes the oldest prefix once
via `compactConversation`. The summary rides in the system prompt and the
boundary message id is recorded. Nothing is deleted — this only decides what
gets *sent*. If summarizing fails, the oldest turns are simply dropped rather
than blocking the send.

---

## 4. The loop

[`agent/run.rs`](../src-tauri/src/agent/run.rs). `run_agent` is a thin wrapper
that guarantees post-run bookkeeping (`backfill_skill_run_failures`) happens on
every exit path; `run_agent_inner` is the loop.

```
build ToolRegistry (once per run)
loop, at most MAX_ITERATIONS = 12:
    cancelled? → emit Cancelled, return
    drive_turn(endpoint, messages, tools, temperature, on_token)
      ├ Final{content}   → maybe parse as a text-form tool call; else answer, Done
      ├ ToolCalls(calls) → dispatch_calls, continue
      ├ Cancelled        → emit Cancelled, return
      └ Err(e)           → emit Error, return
emit Error("Reached the limit of tool steps for one turn.")
```

### 4.1 The tool registry

`ToolRegistry::build` assembles one flat tool table per run:

1. Every **enabled built-in toolset** (`toolset.<id>.enabled` in settings, else
   the toolset's default), intersected with the conversation persona's
   allowlist. A persona can *narrow* its reach, never widen it past the global
   toggles.
2. Minus any tool whose **autonomy class is `Off`** — a capability the user
   closed off is never advertised, not merely refused on call.
3. Plus every **enabled MCP connector's** cached tools, converted to OpenAI
   specs. Built-ins win name collisions; earlier connectors win over later ones.

### 4.2 Streaming vs. buffering (`LOOP-4`)

With tools off, prose streams straight through. With tools on, the turn is
buffered — because some engines emit a tool call as plain assistant *content*
JSON, and flushing that leaks raw JSON into the conversation.

`should_flush_prose` decides when a partial buffer is safely prose. It is
deliberately dumb and biased toward buffering: a JSON/array opener, a code
fence, a `<think>` preamble, or *any known tool name anywhere in the buffer*
keeps buffering. Starting on a letter is the clearest prose signal; anything
else waits for 160 characters. A wrong "buffer" only restores the old
end-of-turn behaviour; a wrong "flush" is visible garbage.

### 4.3 Text-form tool calls (`TOOL-2`)

`parse_text_tool_calls` recognizes `{"name": …, "parameters": …}` (and the
`function`/`arguments` spellings) in assistant content — whole-string first,
then a salvage pass that parses the first complete JSON value after the first
`{`, for models that wrap the call in a reasoning preamble. The guard that keeps
this from misreading a genuine JSON answer: **the name must resolve to a tool
in the registry.**

### 4.4 Dispatch

`dispatch_calls` echoes the assistant's tool-call message into history, then per
call: emit `StepStart`, route through `dispatch`, record a content-free
`tool_stats` row, emit `StepDone`/`StepError`, append a `role: "tool"` result
message.

Two learning behaviours sit here:

- **Guided retry (`GRM-3`).** A failed *built-in* call gets exactly one system
  nudge ("Fix the previous tool call: …") keyed by call id. MCP failures don't
  get it.
- **Fail→fix mining (`FIX-1`).** `FixTracker` remembers the last failed call per
  tool name for this run; if the *same tool* later succeeds, one `tool_fixes`
  row records the pair. Same tool, same run, only on success, at most one row
  per failure — each narrowing is deliberate and each is pinned by a test.

### 4.5 Routing

`dispatch` looks the name up in the registry. Built-ins get a fresh
`ToolContext` and run `Toolset::execute`. MCP tools go through `call_mcp_tool`,
which reuses one live client per connector for the whole run (`LOOP-1`) — the
`initialize` handshake and, for stdio, the child process, happen once, and
dropping the pool at run end kills stdio children.

---

## 5. Toolsets

[`agent/toolsets.rs`](../src-tauri/src/agent/toolsets.rs). A `Toolset` is a
**tool group**: it advertises OpenAI specs, claims the names it handles,
describes a call for the timeline, and executes it. Adding a capability means
one enum variant and one backing module; four exhaustive `match` arms keep it
honest.

> Naming: these were called "skills" through Phase 9. The word now belongs to
> **Agent Skills** (§6), which are a different thing entirely — prompt-level
> capability packs. Settings keys were migrated `skill.*` → `toolset.*` by the
> v9 schema migration.

| Toolset | Tools | Default | Notes |
|---|---|---|---|
| File system | `read_file` `write_file` `edit_file` `list_directory` `search_files` `create_dir` `delete_file` `move_file` | on | Real disk, no sandbox — see §8.1 |
| Artifacts | `create_artifact` | on | HTML/SVG/markdown/code into the Workbench |
| Workspace UI | `render_ui` `present` `remember` | on | Generative UI — typed blocks + a composable surface |
| Recall | `search_history` `read_conversation` | on | FTS over this device's own past |
| Memory | `memory` `propose_soul_edit` | on | The durable self; autonomy-gated |
| Folder reading | `search_folder` `find_similar` | on | Retrieval over what the indexer read; duplicate grouping |
| Skills | `skill` `propose_skill` | on | Agent Skills, §6 |
| Image generation | `generate_image` | off | Chat-tool path; the picker path is §9 |
| Web search | `web_search` `fetch_url` | off | Leaves the device |
| Code execution | `run_code` | off | Job-Object-confined subprocess |
| Mail | `list_mail` `read_mail` `search_mail` `send_mail` `reply_mail` | off | Direct IMAP/SMTP, no relay |
| Browser | `browse` `browser_read` `browser_click` `browser_type` `browser_press` `browser_scroll` `browser_screenshot` | off | Drives installed Chrome/Edge over CDP |
| Screen & apps | `screenshot` `open_app` | off | Deliberately not GUI automation |

### 5.1 `ToolContext`

One context is constructed per tool call and carries everything a toolset might
need: the HTTP client, the DB, the runtime/embedding/reranking managers, the
permission manager, the event sink, conversation and message ids, the app-data
dir, the memory store, the browser pool. Three fields encode policy rather than
plumbing:

- **`local_endpoint`** — the local engine specifically, for a toolset's own
  small side call (e.g. the Memory toolset classifying a fact's scope). Never
  this turn's endpoint: work the user didn't ask for must not land on their
  cloud bill or leave the machine. `None` means the toolset does without.
- **`headless`** — an unattended scheduled run. Toolsets skip renders and the
  File System toolset refuses every write/delete/move outright rather than
  raising a prompt nobody can answer.
- **`rendered` + `step_note`** — one render per tool call, enforced here rather
  than trusted to each toolset; and an override for the timeline's result line
  when the generic "— N lines" summary would mislead (a weak retrieval has to
  reach the *user* as "I'm not sure these answer this").

### 5.2 Tool-emitted renders (`RND`)

`render_block` lets any toolset persist and stream a typed block directly,
instead of returning text for the model to describe. Guard rails: skip when
headless, one per tool call (claimed with an atomic swap before the work), and
a 64 KB payload cap — a render that large is a tool dumping file contents, not
something worth showing. A skipped render is logged, never surfaced as a tool
failure.

---

## 6. Agent Skills

[`agent/skillpack.rs`](../src-tauri/src/agent/skillpack.rs). Skills are folders
containing a `SKILL.md` with frontmatter — the open
[agentskills.io](https://agentskills.io) format, not a proprietary one, so a
folder written for another agent works here unchanged.

**Two-stage disclosure.** Stage 1 is the prompt: name + description +
`when_to_use` for every enabled skill, capped at 1536 chars per entry and 4000
for the block (the standard's own numbers, so a skill isn't truncated
differently here than elsewhere). Stage 2 is the `skill` tool, which returns the
full body only when the model decides it's relevant.

**Discovery is Poiesis's own directories only** — `~/.poiesis/skills/` and
`<folder>/.poiesis/skills/`. Other agents' config folders are deliberately not
scanned: silently reading them would mean instructions the user never pointed at
us start steering the model. Importing one is a copy, and that copy is an
explicit act.

**Bundled resources (`SKL-3`).** Loading a skill pushes its folder onto the
run's shared `extra_read_roots`, so a `references/` or `assets/` file is
readable by `read_file`/`search_files` for the rest of the run without a prompt.
Shared across the run, not per call — a skill activated early must stay
reachable later.

**Installing is the gate.** `propose_skill` never writes; it raises a proposal
under the `skills` autonomy class, which defaults to `ask`.

---

## 7. The durable self

[`memory/mod.rs`](../src-tauri/src/memory/mod.rs). Plain markdown under the
app-data directory, owned by the user:

```
memory/
├─ MEMORY.md      generated index — never hand-edited, never model-edited
├─ SOUL.md        standing instructions; user-edited, agent only proposes
├─ PROFILE.md     the agent's synthesis of how the user likes to be worked with
├─ facts/         durable facts about the user
├─ lessons/       reflection output
├─ .trash/        forgotten entries (recoverable)
├─ .quarantine/   unparseable files set aside (recoverable)
└─ .snapshots/    pre-consolidation copies
```

The model never rewrites a file wholesale — it calls narrow verbs and this
module owns the layout and the index. Nothing is destroyed: "forgetting" is a
move to `.trash/`, and every write emits a `MemoryWrite` event carrying an
`undo_token`.

At startup `lib.rs` rebuilds the FTS index from disk (so hand-edits made in
Notepad are searchable), quarantines anything unreadable and says so in the
activity log, migrates pre-Skills recipes, prunes trash and `tool_fixes`, and
sweeps expired short-lived facts.

**Recall.** `recall_for` embeds the message and searches the vector store,
returning entries by meaning rather than word overlap, and *removing* them from
the wholesale index so nothing appears twice. With no embedding engine installed
it falls back to keyword search and says so. A failed call falls back to the
last cached wholesale index rather than dropping memory from the turn.

**Reflection.** When a conversation ends, `commands/reflect.rs` draws at most
one lesson from it. `reflected_at` is stamped *before* the model runs, so a hung
turn can't retry-loop; output is parsed strictly as JSON or discarded; saving
obeys `autonomy_gate("lessons")`; and a second local call criticizes the draft
against the same transcript first. A lesson that fails the critic is demoted to
a proposal regardless of the configured rung, and the verdict is logged so drift
in reflection quality is visible later.

**Golden set (`GLD`).** [`agent/golden.rs`](../src-tauri/src/agent/golden.rs)
holds a small fixed set of behavioural contracts ("ask me to remember something
and I call `memory`"; "a page telling me to ignore my instructions doesn't
work"), checked automatically around every self-change. It never dispatches a
tool call, which is what makes it safe to run unattended; the sibling `EVL`
harness (`tests/eval.rs`) does dispatch for real, against fixtures, by hand.
A self-change that makes the agent worse is reverted and announced.

---

## 8. Consent, scope and trust

### 8.1 File access

There is no filesystem sandbox — the agent reads and writes the user's real
files. Three things carry the safety:

1. **Scope.** Every path is canonicalised (collapsing `..`, resolving symlinks)
   *before* any check and must land inside the working folder, a persisted
   grant, a dialog-granted path, or a run's `extra_read_roots`.
2. **Trust.** The conversation carries a level — read-only / ask-first / full —
   and `permissions::gate(trust, impact)` decides silent / prompt / refuse.
   Reads never prompt; read-only refuses every change; deletes and moves ask at
   *every* level.
3. **Undo.** Anything that destroys bytes is snapshotted to `file_trash` first
   and restorable from the Workbench. Every operation emits `FileChanged` with
   an undo token and lands in the visible activity log.

A `PermissionRequest` blocks the loop on a oneshot the UI answers
(Deny / Once / Chat / Forever). The same panel and the same `Decision` channel
carry non-filesystem consent — a browser domain, a screenshot, launching an app
— rather than a parallel mechanism.

### 8.2 The autonomy ladder

[`autonomy.rs`](../src-tauri/src/autonomy.rs). Every site that writes to the
durable self consults one gate returning `Auto` (do it, say so, offer undo),
`Ask` (write a proposal), or `Off` (withdraw the capability — the tool is not
even advertised).

| Class | Default | |
|---|---|---|
| `facts` | auto | memory saves, undoable |
| `lessons` | auto | reflection output, undoable |
| `profile` | auto | derived and regenerable |
| `consolidate` | ask | tidy-up |
| `soul` | ask | identity |
| `skills` | ask | identity |
| `email_send` | ask | leaves the machine |
| `screen` | ask | a screenshot can contain anything |

An unknown class resolves to `ask`. The gate never fails open.

### 8.3 Untrusted content (`TRU`)

[`agent/untrusted.rs`](../src-tauri/src/agent/untrusted.rs) is one
canonicalize + scan + wrap primitive shared by every intake site that can carry
attacker-supplied prose: web results, fetched pages, retrieved file chunks, mail
bodies, skill content.

**It blocks nothing.** A heuristic score is never precise enough to gate on —
refusing content that merely *resembles* an injection would drop legitimate
mail, a support article quoting a phishing email, or a page about prompt
injection itself. So `mark_untrusted` wraps the text as data, tells the user
where it came from (an `Untrusted` event → a `◇ from outside` chip on the step),
and logs risk ≥ 2 to the activity log.

The one place a score *does* block: `MemoryStore` refuses to let scanned-risky
text become a durable fact or lesson. Durable self-state is where a poisoned
string would re-enter every future prompt rather than just this one.

### 8.4 Code execution

[`agent/sandbox.rs`](../src-tauri/src/agent/sandbox.rs). Each run gets its own
Win32 Job Object (separate from the engine's) with kill-on-close, a memory cap
and an active-process limit, plus a wall-clock timeout, a scrubbed environment
and a throwaway scratch directory. `kill_on_drop` and the job both terminate the
tree if the future is dropped.

Honest limit: this confines CPU, memory and lifetime and isolates the
filesystem, but does **not** yet block outbound network on Windows (that needs
an AppContainer profile). A read-only folder is never handed to a snippet at
all, and any file a snippet changed is named in the activity log afterwards.

---

## 9. Media

[`media/mod.rs`](../src-tauri/src/media/mod.rs). One provider-agnostic
request/response pair, a `MediaBackend` trait, and a registry — adding a
provider is one file plus one line. Three backends today: local
(`stable-diffusion.cpp`), OpenRouter (image + video), OpenAI. Availability needs
two predicates: a credential *and* `is_ready(&Db)`, because the local backend is
"always credentialed" but useless without a binary and checkpoint on disk.

Creation is **not modal**. Media models appear as a category in the ordinary
model picker; choosing one retargets the composer. For a bare message against a
chat model, `mediaIntent.ts` infers intent from a leading imperative verb
(`draw…`, `zeichne…`) and offers a chip — a declaration through the picker
always wins over inference.

Generation runs as a **background job** (`media/jobs.rs`): a `media_jobs` row
plus a worker, the caller gets a job id immediately, and the result is delivered
on an app-level event rather than the agent-run channel — by the time a video
finishes, the run that asked for it has usually ended. The row carries
`message_id`, so the result lands in the right turn even after a reload.

---

## 10. Perception

- **`EMB`** — a second `llama-server` instance running a small embedding model,
  lazily started and idle-stopped. Same binary as chat; only model and flags
  differ.
- **`VEC`** — one vector table serving both memory recall and folder retrieval.
  Vectors are stored pre-normalised, so similarity is a plain dot product.
- **`IDX`** — folder indexing is user-initiated ("Read it" in the Workbench) and
  runs on a background task; it must never block a chat turn. It reuses
  `filesystem`'s ignore rules and binary sniff so a folder looks the same to the
  indexer as to `list_directory`.
- **`RET`** — `search_folder` scores by dot product plus an additive keyword
  bonus, diversifies with MMR under a per-file cap, and applies a floor below
  which nothing is returned. A weak top hit triggers one rephrased re-query, and
  if it's still weak, a one-shot sufficiency check — so a guess never reaches
  the model dressed as a confident answer.
- **`RRK`** — an optional third engine that re-reads the top candidates more
  carefully. Default off.
- **`PHS`** — 64-bit dHash for images, whole-file centroid cosine for documents.
  Grouping only; nothing here deletes anything.

Deferred, specified but not built: vision captioning on index, OCR, and field
extraction (`PERCEPTION_PLAN.md` Part IV).

---

## 11. Unattended runs

[`commands/scheduler.rs`](../src-tauri/src/commands/scheduler.rs). A single
process-wide 60-second ticker drives named jobs. Concurrency is deliberately 1 —
one local GPU serialises generation anyway — so a second due job waits for the
next tick. Jobs live as a JSON blob in `settings`, not a table.

Unattended runs call `run_agent` directly with `headless: true`, which is the
whole safety story: no renders, and the File System toolset refuses every
write/edit/delete/move outright rather than opening a prompt nobody could
answer. The same autonomy membrane applies — `Auto` applies with undo, `Ask`
leaves a proposal for the morning, `Off` skips. Each run happens in its own
readable conversation. Tasks only run while the app is open; there is no
background service.

One test asserts that an unattended run refuses rather than blocks. Its failure
mode is hanging, so it asserts under a timeout.

---

## 12. The event protocol

`AgentEvent` ([`agent/mod.rs`](../src-tauri/src/agent/mod.rs)) is the whole
backend→UI contract, serialized tagged by `type`.

| Event | Rendered as |
|---|---|
| `StepStart` / `StepDone` / `StepError` | the agent-run timeline |
| `Token` | streaming prose |
| `Recall` / `Code` / `Untrusted` | expandable disclosure hanging off a step |
| `Block` / `BlockUpdate` | typed interactive blocks inline in the turn |
| `Artifact` | Workbench canvas (media carries `meta_json` so the stream renders the same block the direct path does) |
| `StateUpdate` | durable session state |
| `Permission` | the consent side panel |
| `MemoryWrite` | a toast with Undo |
| `Proposal` | a card the user accepts or declines |
| `FileChanged` | Workbench row mark + refresh + undo |
| `Browser` | the live browser panel (replaced wholesale, not patched) |
| `MailSent` | a receipt — there is no undo for a sent message |
| `Done` / `Cancelled` / `Error` | run terminal states |

---

## 13. Signals the harness keeps about itself

All content-free unless noted, all local:

| Table | Written by | Read by |
|---|---|---|
| `tool_stats` | every dispatched call | Settings reliability captions; `toolCautions` in the prompt; critic-verdict drift |
| `tool_fixes` | `FixTracker` on fail→fix | reflection (pruned at 30 days — it holds arguments and error text) |
| `skill_runs` | per run, backfilled with failure counts | skill outcome reporting |
| `activity_log` | file ops, MCP calls, untrusted intake, memory events | the visible Activity list |
| `change_proposals` | anything gated to `ask` | the proposal cards |

---

## 14. Extending it

**A new toolset:** add a variant to `Toolset`, a module exposing
`tool_specs` / `handles` / `describe` / `execute`, and arms in the four `match`
blocks. Pick a default (`default_enabled`) and mark it `sensitive` if it leaves
the device or runs code. If it takes in outside text, route that text through
`mark_untrusted`. If it can render, use `render_block` rather than emitting
blocks directly.

**A new media backend:** one file under `media/backends/` implementing
`MediaBackend` (with a `BackendDescriptor` and, if it needs local state,
`is_ready`), plus one line in `Registry::new()`.

**A new skill:** a `SKILL.md` folder in `~/.poiesis/skills/`. No code.

**A new self-change class:** add it to `AUTONOMY_DEFAULTS` and call
`autonomy_gate` at the write site. If a tool embodies the class, map it in
`self_change_class` so `Off` withdraws the tool rather than only refusing it.

---

## 15. Known limits

- **Iteration cap is 12** and it's a hard error, not a graceful wind-down. Block
  render/update calls consume iterations.
- **`run_agent` takes 20 arguments.** Every one is a real dependency, but the
  signature is at the point where a context struct would be an improvement.
- **The code sandbox doesn't block outbound network on Windows.**
- **The retry nudge is one shot per call id** and applies only to built-ins.
- **MCP tool lists are read from the connector's cached `config_json`**, so a
  server that changes its tools mid-session isn't noticed until the connector is
  refreshed.
- **Mail opens a fresh IMAP session per call** rather than pooling per run the
  way MCP does.
- **The agent can't see its own generated media** (`SEE-1`, not built).
