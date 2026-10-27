# Poiesis Agent

A **local-first, agentic desktop LLM application** for Windows. Chat with local
models that run entirely on your machine, give the assistant real capabilities
(files, folder reading, web search, code execution, image generation, your
connected apps, the coding agents you already have installed), talk to it out
loud, and optionally bring your own cloud API keys — all in a calm, editorial
"Paper / Slate" interface.

**Poiesis Agent** takes its name from *autopoiesis* (Maturana & Varela): a
system that continuously produces and repairs the components that constitute
it. It doesn't just use its memory, instructions, and procedures — it
maintains them: it observes its own mistakes, distills lessons, repairs
degraded parts, and evolves its own way of working, with you as the boundary
that decides what may change.

## What Poiesis Agent remembers

Everything the agent knows about you lives as **plain markdown files on your
device**, under the app-data directory — readable, editable, and deletable in
any text editor, with no database in the way:

```
memory/
├─ MEMORY.md      the index injected into every conversation
├─ SOUL.md        standing instructions — yours; the agent only proposes edits
├─ PROFILE.md     the agent's own synthesis of how you like to be worked with
├─ facts/         durable facts about you, one file each
└─ lessons/       what it learned from its own mistakes
```

Procedures live next door in `skills/` as **Agent Skills** — plain `SKILL.md`
folders in an open format, so one you already use with another agent works here
unchanged (see [Agent Skills](#agent-skills-settings--skills)).

It writes to these by itself, and **every self-write is visible the moment it
happens** — a quiet toast with an Undo, not a log entry you'd have to go
looking for. What it may change without asking is set per class in
**Self → Autonomy**: *Auto with undo*, *Ask first*, or *Off*. Anything on
"Ask first" waits as a proposal you accept or decline.

- **Recall by meaning.** Memories and lessons surface when they're *relevant*,
  not when they happen to share words with your question — a locally-run
  embedding model does the matching. Without it the agent falls back to
  keyword search and says so rather than pretending.
- **Reflection.** When a conversation ends, it reads back over it and draws a
  lesson if there's one worth keeping. A critic gate checks the lesson before
  it's allowed in; what fails is demoted to a proposal, not silently dropped.
- **Self-repair.** A memory file it can't parse is quarantined rather than
  deleted, and surfaced in **Self → Health** so you can fix or discard it. Tools
  that have been failing lately are noted to itself, so it routes around the
  damage.
- **Self-check.** Every change it makes to itself is checked against a small set
  of fixed behavioural contracts — *does it still call `memory` when asked to
  remember something; does a web page telling it to ignore its instructions
  still fail to work.* A change that makes it worse is put back, and it tells
  you it did.

---

## Tech

- **Shell:** Tauri v2 (Rust backend + system WebView)
- **Frontend:** React 18 + TypeScript + Vite + Zustand
- **Local engine:** externally-managed, prebuilt `llama-server` (llama.cpp) over loopback HTTP
- **Embedding + reranking engines:** prebuilt, CPU-only, lazily started and idle-stopped
- **Decision engine:** a small "decision model" (Clef-flash, 9B) on the same `llama-server` binary, or OpenRouter's decision models in the cloud — answers yes/no/pick-one questions in tens of milliseconds
- **Image engine:** prebuilt `stable-diffusion.cpp` (downloaded on demand)
- **Voice:** `sherpa-onnx` in-process — speech recognition and synthesis on your CPU, no cloud model
- **Agent apps:** Claude Code, Codex and OpenCode, driven headless over their own stdio protocols (stream-JSON, `app-server`, ACP)
- **Connected apps:** Composio Connect (one hosted MCP endpoint, your own `ck_` key) beside any MCP server you add yourself
- **Storage:** embedded SQLite (FTS5 full-text search + a vector store), plus markdown for the agent's memory
- **Secrets:** OS credential store (Windows Credential Manager) — never in files or the database

**How the agent actually works** — the loop, the toolsets, prompt assembly,
consent, agent apps, connected apps, the decision engine and the event
protocol — is written up in [docs/AGENT_HARNESS.md](docs/AGENT_HARNESS.md).

Plans and specs live in [plans/](plans/). In the folder:
`CLI_ENGINES_PLAN.md` (built: Claude Code, Codex and OpenCode as delegation
targets, with a visible rule set for when Poiesis hands work over),
`CONNECTED_APPS_PLAN.md` (built: a gate on every MCP connector, then Composio
Connect), `DUAL_PROCESS_PLAN.md` (built through step 19, step 20 is a real-model
run: the decision engine, the controller, Experiments, and the "make it felt"
v3; v4 "the stream goes quiet" is written, not built), `COGNITION_PLAN.md` (not
built: memory and self-learning on the human-memory model), plus `JUDGE_PLAN.md`
and `dual_process_agent_runtime_concept.md`, which the dual-process plan
supersedes and builds on. Finished plans move to [plans/done/](plans/done/):
`IMPLEMENTATION_PLAN.md` (architecture, phases 0–8), `FILESYSTEM_PLAN.md`
(working folders, trust, undo), `POIESIS_PLAN.md` (the durable self and its
maintenance loop), `PERCEPTION_PLAN.md` (recall, retrieval, tasks),
`CAPABILITIES_PLAN.md` (Agent Skills, mail, browser, untrusted content, the
self-checking harness), `MULTIMODAL_PLAN.md` (media backends and in-stream
creation), `HARNESS_PLAN.md` and `SUBAGENTS_PLAN.md` (one six-phase
programme), `SHELL_PLAN.md` (one tab strip, a sidebar that navigates itself),
`PROJECTS_PLAN.md` (projects as groups of sessions), `PLANNING_PLAN.md` (a
visible plan a run works through), `CODING_PLAN.md` (running project tasks,
reviewing the agent's patch, code navigation), `MODELS_PLAN.md` (Models,
Providers, Runtime), `COMMANDS_PLAN.md` (the `/` menu), `VOICE_PLAN.md`,
`AUTOUPDATE_PLAN.md`, `TASKS.md` (checklist) and `Project_Nexus_PRD.md`
(product spec).

---

## Download

**[Download the latest Windows installer](https://github.com/schuettla/poiesis/releases/latest)**
from the Releases page (the `-setup.exe` file). Installed copies check for
newer releases themselves and always ask before installing one.

The installer isn't code-signed yet, so Windows may show "Windows protected
your PC". Choose **More info → Run anyway**.

To build it yourself instead, see Quick start below.

## Quick start

```sh
npm install
npm run tauri dev      # builds the Rust backend + frontend, opens the app
```

The first launch compiles the Rust backend (a few minutes) and opens the desktop
window. From there:

1. **Models** → download a local model (it detects your GPU and shows which models
   "Run great / slowly / Won't fit"), or add one by Hugging Face repo / GitHub / URL.
2. Start chatting. Toggle the **⚒ tools** button in the composer to let the
   assistant act.
3. **Attach a folder** in the right-hand sidebar and the agent reads it, so you
   can ask questions phrased in your own words rather than the file's. The
   folder joins a **project**, so the next chat you start there already has it.
4. **To create images or video:** just ask for one in the conversation — "draw a
   fox in the snow". Pick where it's made from the same model picker you pick a
   chat model from (see [Making images and video](#making-images-and-video)).
5. Type **/** (or press the `/` button beside the `+`) to *do* things rather
   than say them: stop, branch, rewind, set a goal, hand a job to another
   agent, connect an app. See [The `/` menu](#the--menu).
6. Everything else lives behind the **cog** at the bottom of the left rail:
   General, **Models** (what you can use), **Providers** (where it comes from),
   **Runtime** (what runs on this PC), **External agents**, Tools, **Skills**,
   **Apps**, **Self** (memory, lessons, health, autonomy), **Tasks** (scheduled
   work), Mail, Activity, Usage, **Experiments**, Working dir and About.

By default the app runs in **Simple** mode, which keeps the machinery out of
the way. **Settings → General → "Show me everything"** reveals the engine
controls, per-tool toggles and other expert surfaces.

### Prerequisites

| Requirement | Notes |
|---|---|
| **Rust** (via [rustup](https://rustup.rs)) | Backend language |
| **MSVC C++ Build Tools** | The Rust linker on Windows (Visual Studio Build Tools → "Desktop development with C++") |
| **Node.js 18+** and npm | Frontend toolchain |
| **WebView2 runtime** | Ships with Windows 11; the installer bootstraps it on Windows 10 |

**No CUDA toolkit needed** — Poiesis downloads prebuilt engine binaries rather than
compiling anything. GPU acceleration (CUDA / Vulkan / ROCm) is auto-selected from
your hardware.

---

## Commands

| Command | What it does |
|---|---|
| `npm run tauri dev` | Full app in dev mode (Rust + frontend, hot-reload UI) |
| `npm run dev` | **Frontend-only** preview in a browser at `http://localhost:1420` (mock data, no backend) — fast UI iteration |
| `npm run tauri build` | Production build + Windows installer (`.msi` / `.exe`) |
| `npm run build` | Type-check + build the frontend bundle only |
| `npx tsc --noEmit` | Type-check the frontend without emitting |
| `npm test` | Frontend unit tests (vitest) |
| `cd src-tauri && cargo test` | Run the Rust backend test suite |
| `cd src-tauri && cargo build` | Compile the backend only |

### Building a release installer

```sh
npm run tauri build
```

Output lands in `src-tauri/target/release/bundle/` (the `.msi` and/or NSIS `.exe`
installer, with the WebView2 bootstrapper embedded).

---

## Features

### Local models & runtime
- Hardware survey (GPU / VRAM / RAM / CPU ISA) → automatic llama.cpp **backend
  selection** (CUDA 12/13, Vulkan, HIP/ROCm, SYCL, CPU AVX-512/AVX2/baseline).
- Runtime provisioning: streamed, resumable **download** of the matching prebuilt
  `llama-server`, unpack, and a manual **backend override** in the Engine view.
- Engine lifecycle: dynamic loopback port, **per-session random auth token**,
  health-gated readiness, and kill-on-exit — including a Win32 **Job Object** so an
  ungraceful crash can't orphan the engine and leak VRAM.
- A **watchdog** restarts a crashed engine with backoff, capped per rolling hour
  so it can't loop forever, and says so in **Self → Health**.
- **Marketplace:** curated recommendations + Hugging Face / GitHub discovery,
  fit badges, quant slider, one-click download, local library management.

### Chat & persistence
- Streaming responses with a **Stop** control and an inline **agent-run timeline**.
- SQLite-backed history with **FTS5 full-text search** in the sidebar.
- Markdown + code rendering; adjustable reading size; light/dark ("Paper / Slate").
- **"What I'm working from"** — a read-only panel showing every layer that fed a
  turn: instructions, what it remembers about you, what it recalled, which files
  it read. Nothing about the prompt is hidden from you.

### Tabs and the sidebar
Everything you have open sits in **one tab strip** in the header: chats, single
items (a file, an artifact, one agent's run, one file's patch) and routes such
as Settings or a project. Opening something never throws away what you were
looking at, and the strip comes back as you left it after a restart.

- **One thing is a tab; an overview is the sidebar.** The right sidebar has its
  own row of sections: Files, Artifacts, Agents, Browser and **Changes**. A row
  in any of them opens that one item as a tab, shown full width where the chat
  is shown.
- **The agent may move the sidebar, never your tabs.** When it starts browsing,
  starts a child agent, makes an artifact or first edits a file, the sidebar
  switches to the matching section. It never opens or focuses a tab, because a
  tab can take the chat you are typing in off the screen.
- **An item remembers its chat.** Pressing a tab that belongs to another chat
  makes that chat live first. Closing a tab never switches chats.

### Projects
A project is **a named group of chats that share a context**. A folder is
something a project may have, not what it is: a project about a book, a job
search or a person you are helping works without one.

- **New project** in the left rail makes one; attaching a folder to a chat
  creates or joins the project for that folder, so nobody has to think about
  projects to benefit from them.
- The **project view** (a tab) holds its name, **instructions** added to every
  chat in it, its working folder and trust level, and its chats. Chats can be
  moved in, between and out of projects; that never touches their messages or
  anything on disk.
- **Memory can belong to a project.** A fact saved during a chat in a project is
  tagged with it and is not recalled in other projects. Untagged memory stays
  shared everywhere.
- Archiving hides a project. There is no "delete project", because it would read
  as "delete my files".

### Plans a run works through
For work with several parts, the agent writes a short plan first and ticks it
off as it goes. The plan card sits above the timeline, and a dropped item says
why. A run that tries to finish with its plan half done is asked once to either
continue or revise the plan, so the card does not end up claiming work that
never happened.

### Working on code
Poiesis is not an IDE, but in a project with a folder it can check its own work
and show you exactly what it changed.

- **It knows the project.** A project card is detected from the folder:
  languages, manifests (`package.json`, `Cargo.toml`, `pyproject.toml`,
  `go.mod`, `Makefile`, `*.sln`), the tasks they declare, and the git branch.
  `AGENTS.md` or `CLAUDE.md` is read into every chat there. The folder header
  shows this as chips, with a **Tasks** list.
- **Read before edit.** It cannot edit a file it has not read in this run, or
  one that changed since. File reads come back with line numbers.
- **It runs the project's own tasks** (`run_task`): build, check, test, lint,
  as the project declares them, in the project folder. Output streams into the
  timeline; the result reads `passed`, `2 failed` or `12 errors`, and each
  finding is a `file:line` row that opens the file at that line. Output from
  rustc/cargo, tsc, msbuild, eslint, pytest, Python tracebacks, go and Node's
  test runner is understood; anything else falls back to the last lines.
- **Asking is the default.** Per project: *Off*, *Ask* (with "always allow this
  task") or *Allow*; the default for all projects is in Settings → Tools. A
  read-only folder never runs a task. Free-form commands (`run_command`) exist
  only in "Show me everything" mode, per project, always ask, and never go
  through a shell.
- **The Changes view.** Every file the agent changed since you last pressed
  **Keep all**, with `+N / −M`, a two-gutter patch, **Undo** per file and
  **Undo all**. A file's patch opens as a tab; a file tab the agent changed shows
  a dot in place of its ×, and the dot jumps to the patch. The patch is built
  from the agent's own undo snapshots, never from `git diff`, so your own
  uncommitted work is never shown as the agent's.
- **It says when it did not check.** A run that edited code with a check task
  available and never ran it is reminded once; if tasks are off, it is told to
  say plainly that the change is unverified.
- **Code navigation.** `find_symbol` lists where a function, type or constant is
  defined, then where it is used, for Rust, TypeScript, JavaScript, Python and
  Go (tree-sitter, no language servers). The folder index chunks code on
  function boundaries.
- **Run a code artifact.** A Python or Node artifact gets a **Run** button, with
  its output and any traceback shown below the source.
- A **Builder** persona preset bundles the tools for this: file access, folder
  reading, code execution, project tasks and delegation.

### Running work you can steer

A turn is not a black box you wait out. While the agent works you can talk to
it, watch what it is doing, and stop it without losing what it found.

- **Type while it works.** What you send lands between steps — never inside a
  half-finished tool call — and the agent picks it up on its next move. The
  message shows in the transcript straight away, marked as sent mid-run.
- **Stop keeps the work.** Stopping ends the run with whatever it had, labelled
  as stopped rather than passed off as a finished answer.
- **Limits end with an answer.** When a run reaches its step or time budget it
  gets one last tool-free turn to say what it found, instead of failing and
  throwing the work away. The same applies when a model goes quiet after doing
  real work: it is asked once for the answer rather than showing you a blank.
- **A run meter** under the timeline says what the run is doing right now —
  `searched job market · 0:41 · context 38%`, or `thinking`, or `working`
  between steps. It says `about 12k in context` rather than inventing a
  percentage when the model's window is unknown. It deliberately does *not* show
  the step budget: that is a limit, and a limit is not a status.
- **Several things at once.** Independent tool calls — searches, reads — run
  together and are drawn as one `3 things at once` band, settling back into
  ordinary rows in the order they were asked for. Anything that writes, or
  drives a live session, stays strictly in order.
- **Big results are kept, not pasted.** A tool output over 8 KB goes to disk;
  the model gets a preview and a handle it can read or search, and you get the
  whole thing behind a disclosure, with a **Save to the working folder** action.

### Handing work to other agents
The agent can split a job across several agents working at the same time, each
with its own fresh context, and it writes the final answer itself.

- **A child is a real run in its own chat** — its own transcript, artifacts and
  timeline — not a prompt trick. You can open it and read it live.
- **A child is never more powerful than its parent.** Toolsets, working folder
  and trust are inherited and can only be narrowed, and by default a child
  cannot delegate again.
- **Watch, redirect, or stop each one.** The Fleet card in the turn shows what
  each agent is doing right now; the **Agents** panel opens any of them. Stop on
  the composer stops the whole tree, and says so before you press it.
- **Permission prompts say who is asking** — *the researcher agent I started
  wants to…* — and queue rather than replacing one another.
- **In the background.** Ask for it and the agents keep working after the turn
  ends; the conversation stays usable and a toast points back when one finishes.
  More than three at a time queue. Personas marked *spawnable* become agent
  types the lead can pick by description.

### Going back, and carrying on
Every run is written to an append-only session log — the model's own view of the
conversation, kept apart from what you read.

- **Try again from here** branches the chat just before any answer, keeping the
  persona, model, working folder, trust and summary, and re-asks the question.
- **Continue where I stopped** resumes a run that was stopped, timed out or hit
  its step limit, with every tool result intact, so it does not repeat work you
  already paid for.
- **Honest compaction.** When a long chat is summarized, the divider says how
  many messages stand behind the summary and that they are all still there. Every
  compaction is kept, so a summary written from an earlier summary says so and
  the earlier versions stay readable.

### Handing work to agent apps you already have
Claude Code, Codex and OpenCode are excellent at long coding jobs, and many
people pay for them by subscription. Poiesis can hand them a job and keep the
conversation, the memory and the final word.

- **A delegation target, not a replacement engine.** The agent app runs as a
  child of the run, the same way a child agent does: its own conversation, a
  Fleet card with what it is doing right now, Stop, a transcript you can open,
  and a report back to the lead. Each runs **headless over its own structured
  protocol** (no screen scraping); its approval requests come to Poiesis's own
  permission panel, labelled with who is asking.
- **Settings → External agents** finds the ones you have installed (including
  the Claude Code that ships inside the VS Code extension), probes them, shows
  who pays (*your plan*, *an API key*, *free*), and has a **Try it** button
  that can't change anything on disk.
- **You see the rules for when it hands work over.** Per agent: a mode (*Only
  when you ask* / *Suggest it, I confirm* / *Hand over on its own*), plain
  language **"Use it for" / "Don't use it for"** lines the lead reads and cites,
  and hard **guards** checked in Rust (code projects only, never in a read-only
  folder, a cap per chat). In *suggest* mode a **handover card** asks first, and
  *Always for this* relaxes one rule at a time. A rule you keep overriding
  becomes a proposal to edit it, never a silent change. **Try a request**
  previews what the lead would do.
- **Ask for one directly.** `/delegate [agent] <task>` always wins over mode and
  rules (never over guards), `/external off` turns agents off for a chat, and a
  persona can be set to *run on* one agent.
- **They can use your memory, and your connected apps, if you let them.** Two
  separate switches per agent, both off: *Let it use my memory* and *Let it use
  my connected apps*. Either way the agent talks to a loopback endpoint Poiesis
  runs with a per-run token; it never sees your Composio key or your other
  secrets, and a connected-app call goes through the same read/change/send gate
  as one the lead makes.
- **Cost stays visible.** Settings → Usage has an *Agent apps* section.
- **Honest about what's flaky.** Codex's model skips the tools Poiesis offers
  about half the time; Claude Code and OpenCode pass. Windows with no Codex
  sandbox set up makes every step ask.

### Connected apps (Settings → Apps)
Notion, Slack, Calendar, Linear, Sheets and hundreds more, connected from the
chat or the Apps page — nothing sends, posts or pays without you seeing it.

- **Composio Connect** is the default: paste the `ck_` key from your own
  Composio account and the model is offered four meta-tools (search, read the
  schema, run, manage connections) instead of a thousand. There is no hosted
  Poiesis broker; the key stays in the OS credential store. A developer
  `ak_` key still works as the advanced option.
- **Connect from the conversation.** Ask for something in an app that isn't
  connected and a **connect card** appears in the turn; the run pauses, you sign
  in, and it carries on. Several accounts per app are supported (a name each);
  with more than one, the model has to say which.
- **Every connector call is sorted into read, change or send** — Composio's and
  any MCP server you add. Reads run. A change asks, with **Allow once / Always
  for this app's changes / No**. A send (post, pay, reply, publish) asks every
  time, or "up to N a day for this app" (default cap 25). A call's kind is the
  strictest of the tools inside it, and you can correct a tool's kind per app.
  An unattended run never hangs on one: it leaves the approval in your inbox.
- **What comes back is outside text.** Connector output is wrapped and marked
  *from outside* like a web page, and any sign-in link is redacted before the
  model sees it.
- **Personas can be limited to some apps** ("Apps: Notion, Linear"), and Usage
  shows connected-app calls per app. No price is invented: Composio bills you
  per call on your own plan.

### Quick decisions, behind Experiments
Most of the questions an agent *should* ask before acting are small — which
tools does this message need, is this call what you asked for, does this answer
claim something no step did — and a full chat turn per question is too slow and
too expensive for the hot path. A **decision model** answers them in tens of
milliseconds, each with a probability, either on your machine or through
OpenRouter's decision models (zero-retention providers only; background
questions stay on your computer unless you opt one in by name).

Everything that changes behaviour is an opt-in switch in **Settings →
Experiments**, each with a plain card, a 30-day report from local data, and a
**Reset its data**. Nothing here is on by default, and a computer with no
decision model loses nothing: every place that asks keeps its old path.

- **Fast & slow thinking** (needs a decision model; Settings → Models installs
  one, Settings → Runtime says where it runs and what it costs):
  - holds back tool groups a message clearly doesn't need (the model can ask
    for one back, and so can you), including connected apps, MCP servers and
    other agents, which for a small model are most of the tool specs;
  - reads a message sent to a running agent (stop, redirect, addition,
    question) and holds only a confident *stop* for your yes;
  - checks, before a run ends, whether the plan's items are really done, and
    whether it claims a change works when files changed and nothing was run;
  - adds caution before a tool call: a question of its own when a call looks
    irreversible or not what you asked for, or one line on a question that was
    coming anyway. It only ever adds caution, never overrules a deterministic
    gate; it never touches connected-app or agent-app approvals except to add
    a line;
  - matches your message to a handover rule before the lead runs;
  - gives the lead a **Sort many items** tool (`classify`): one closed question
    asked about up to 500 files, tickets or photos, answered as a table with a
    confidence per row;
  - checks pages for the browser agent (a login or a captcha hands the browser
    to you) and, in a coding chat, whether an answer's claims are backed by
    what the run actually read or did;
  - reads the composer as you type and shows at most one line before the send
    button ("This reads as a stop", a handover that may apply).
- **Answers check themselves.** After the answer, each sentence that can be
  checked is matched against what *this run* read or did — no new search —
  and a sentence that says "I ran the tests" when no step did is flagged.
  **Check more deeply** adds a web search and a slower model. A one-line strip
  above the first text says what the quick decisions understood, and a receipt
  after the answer says what they changed. How loud this should be is still
  open: the next revision in the plan quiets the chat down to the answer plus
  one line only when a statement looks wrong, and it is not built yet.
- **Mail triage** (needs a *local* decision model and a Mail account): a small
  inbox with kind, urgency and "wants a reply" marks, sorted in the background.
- **Let quick decisions skip low-risk prompts** (needs Fast & slow thinking and
  a calibrated backend): skips one question only — editing one file inside the
  attached folder — and says so on the step.
- **Workspace mode** now lives here too.

It measures itself from what you actually do (undoing a held tool group, saying
no to an added question) rather than asking a model whether it was right, and
until a purpose has measured evidence it can only make things *stricter* or
*cheaper*, never looser. Every decision is on the **Activity** page.

### Talking to it
**Voice mode** turns the orb into the face of the conversation: you speak, it
answers out loud, you can cut in, and it stops. Recognition and synthesis run
on your machine through one in-process speech engine (Parakeet or Moonshine for
hearing, Kokoro and Piper voices for speaking, Silero for knowing when you've
stopped) — no cloud model is needed. Install the voice parts from **Settings →
Runtime → Voice**. A spoken turn is an ordinary agent turn, marked as spoken.

### The `/` menu
A `/` button beside the `+` opens a menu of about 45 commands. `+` adds things
to the message; `/` does things. Most are bindings to actions the app already
has, and the agent can use a few of them on itself, inside the autonomy ladder.

| Group | Examples |
|---|---|
| The turn | `/effort`, `/steps`, `/stop`, `/continue`, `/retry`, `/plan` (read-only until you approve), `/btw` (a side question that stays out of the conversation), `/goal` (keep working until a stated condition is met) |
| The conversation | `/new`, `/rename`, `/fork`, `/export`, `/rewind` (conversation and files), `/compact`, `/context`, `/why` |
| Memory and self | `/remember`, `/forget`, `/always`, `/reflect`, `/checkup`, `/skillify`, `/tidy`, `/self`, `/autonomy` |
| Reach | `/tools`, `/apps`, `/connect`, `/external`, `/delegate`, `/agents`, `/schedule`, `/image`, `/video`, `/persona`, `/model` |
| Code | `/init`, `/review`, `/verify`, `/changes`, `/undo`, `/keep` |
| Information | `/permissions`, `/usage`, `/activity`, `/help` |

The agent gets two tools of its own for this: `harness` (compact its context,
*suggest* one of your slash commands, ask you to switch a mode, propose
scheduling a task — you decide each) and `ask_user` (a real fork with 2–4
options, which pauses the run until you answer). The `/` menu is the only place
commands are added: there is no `@` mention or other composer syntax.

### Cost, in the open
Token counts are captured per turn and totalled per run. **Settings → Usage**
breaks spend down by day, by model and by chat, and has sections for quick
decisions (how many ran on this computer vs the cloud, and what the cloud
charged), connected-app calls and agent apps. Local runs read *on your device,
no cost*; a model with no known price says so, and the total describes itself as
a floor rather than pretending to be complete.

### The Workspace (an interface, not a wall of prose)
The agent can compose a **live interface for the task** rather than describing
things in paragraphs — a comparison table, a checklist, a form, a picker, a
progress tracker — and revise it as the work moves rather than accumulating
chat. Blocks render inline in the turn; a larger composed surface gets the
Workspace view. Checking a step or picking an option updates the agent's state
without spending a model turn on it.

### Reading your folders
Attach a folder to a chat and the agent reads it in the background, then answers
from it — citing the files it used, which you can open in the Workbench viewer.
Questions don't have to share vocabulary with the files; matching is by meaning.
When the best it can find is weak, it says so instead of dressing a guess up as
an answer. It can also **group duplicates and near-duplicates** (images by how
they look, documents by what they say) — grouping only; it never deletes
anything. Optionally, a second local engine **re-reads the best candidates** more
carefully before answering (Settings → Recall → *Sharper*). Nothing is uploaded
— both engines run on your machine.

### Tasks (scheduled work)
Work the agent does on its own schedule, hourly through weekly:

- **Each run happens in its own chat** you can open and read — not a summary you
  have to take on faith.
- **"Schedule this"** in the Workbench turns the chat you're in into a task,
  carrying its first request across as the instructions.
- A built-in, off-by-default **nightly reflection** reads back over the day's
  conversations and leaves a short digest in the morning.
- A running task always shows in the left rail with a **Stop**.
- Unattended runs are deliberately narrow: they can **read** a folder you point
  them at, but never write, move or delete, and they never block on a permission
  prompt nobody is there to answer — they stop and report instead. They run a
  project task only when that project itself is set to *Allow*; the default from
  Settings does not count.

### Tools (what the agent can do)
Toggle the **⚒ tools** button in a chat to let the assistant act. Each group is
independently switchable in **Settings → Tools** (Everything mode). Anything
that leaves your device or runs code defaults **off**:

| Tool group | Default | Notes |
|---|---|---|
| **File access** | on | Read/write in folders you allow; every change asks first unless you say otherwise. Also reviews its own patch (`changes`) and finds definitions and uses of a symbol (`find_symbol`) |
| **Artifacts** | on | Renders HTML / SVG / markdown / code in the Workbench side panel |
| **Workspace UI** | on | Lets it compose a live interface for the task — a board, a checklist, a picker — instead of describing things in prose |
| **Recall** | on | Searches your past conversations and saved memories. Stays on your device |
| **Memory** | on | Lets the agent keep durable notes about you, visibly and undoably |
| **Folder reading** | on | Reads an attached folder ahead of time so it can find things by meaning. Grants no new folder on its own |
| **Skills** | on | Reads and proposes Agent Skills (below). Reading is free; keeping a new one still asks |
| **Delegation** | on | Splits a job across several agents at once, each with its own fresh context. A child never gets more reach than the chat it came from. Also the door to [agent apps](#handing-work-to-agent-apps-you-already-have) such as Claude Code |
| **Sort many items** | on, but needs Fast & slow thinking and a decision model | One closed question asked about up to 500 items (files, tickets, photos), answered as a table with a confidence per row. See [Quick decisions](#quick-decisions-behind-experiments) |
| **Image generation** | off | The on-device diffusion path as a chat tool. Asking for a picture in conversation doesn't depend on this toggle |
| **Web search** | off | No-key DuckDuckGo query issued from your machine (your query leaves the device) |
| **Code execution** | off | Runs Python / Node in a confined, time- and memory-limited sandbox — also the data-analysis path over an attached folder |
| **Run project tasks** | off | Runs a project's declared build, check, test and lint tasks in its folder, with a time limit, memory and process caps, and secret-looking environment variables left out. Not a full sandbox: a task can reach the network and write where your account can. See [Working on code](#working-on-code) |
| **Scripts calling tools** | off | Lets a snippet the agent wrote call the same tools it has, so a job over many items is one script instead of one model turn each. Every call it makes appears in the timeline, indented under the script, and still asks whatever it would normally ask |
| **Mail** | off | Reads, searches and sends over an account you connect directly (IMAP/SMTP, no relay). Sending always asks first |
| **Browser** | off | Opens pages and clicks around in your *installed* Chrome or Edge, in its own profile. The first visit to a new site asks first |
| **Screen & apps** | off | A screenshot, or launching an app by name. Both ask first. Deliberately not GUI automation |

Connected apps and your own MCP servers are not toolsets: their tools join the
menu from **Settings → Apps**, each call sorted into read, change or send.

Personas can carry their **own tool set**, their own skill set and their own
list of apps, so a "writing" persona need not have the same reach as a
"research" one — a persona can narrow what's enabled globally, never widen it.
A tool group that can't work yet (Mail with no account, Image generation with no
engine or key, Sort many items with no decision model) is left off the menu and
says why in Settings → Tools, instead of being offered and failing.

**Text from outside is marked as coming from outside.** Web results, fetched
pages, retrieved file excerpts, mail bodies and skill content are all wrapped as
data before the model sees them, and the step carries a *from outside* marker
you can expand to read the raw source. Nothing is refused on a suspicion score —
a heuristic isn't precise enough to silently drop your legitimate mail. The one
place a score does block is durable memory: risky text can't become a saved fact
or lesson, because that would re-enter every future conversation.

### Agent Skills (Settings → Skills)
Step-by-step procedures the agent reads before doing work it covers — a folder
with a `SKILL.md`, in the open [agentskills.io](https://agentskills.io) format,
so a skill written for another agent works here unchanged.

- The agent sees only each skill's **name and one-line description** until it
  decides one is relevant, then reads the full text. Skills don't cost you
  context until they're used.
- Files bundled with a skill (`references/`, `assets/`) become readable for the
  rest of that run.
- Skills live in `~/.poiesis/skills/` and `<your folder>/.poiesis/skills/`.
  **Poiesis reads its own directories and only its own** — it will not scan
  another agent's config folder behind your back. Importing one is a copy, and
  that copy is your decision.
- The agent can **propose** a skill after a multi-step task it thinks you'll
  repeat. Keeping it asks first.

### Personas (Settings → Personas)
Saved profiles bundling a **system prompt** + optional **pinned model** +
**temperature** + **tool set**. Pick one per chat from the composer dropdown; a
one-off temperature override is also supported per conversation.

### Making images and video
**There is no image mode.** You ask for a picture in the conversation the way
you'd ask for anything else — "draw a fox in the snow" — and it arrives inline,
under your message. Say "make it warmer" and a better one arrives underneath.
Generated media is a real artifact: it lands in the Library, keeps what made it
and what it cost, and remembers which image it was refined from.

**Where it's made is your choice, in the ordinary model picker.** Image and
video models appear as their own group alongside chat models; picking one
retargets the composer. If you haven't picked, a short chip offers the obvious
route and you can ignore it.

- **On your device** — `stable-diffusion.cpp`. Install it once in
  **Engine → Image**, then download a diffusion model in **Models → Image**
  (Stable Diffusion 1.5 ~4 GB, SDXL), add one by URL, set a default, or delete.
  Downloads are **resumable** — they stream to a `.part` file and continue via
  HTTP range requests if interrupted, and a truncated file is never mistaken for
  a finished model. Nothing is uploaded.
- **With your own key** — OpenRouter (image *and* video) or OpenAI, for models
  no single local checkpoint can reach. Cost is shown before and after, and a
  first use of a paid route asks.

Generation runs **in the background**: the conversation stays usable while a
clip renders, the placeholder shows elapsed time with a Cancel, and the result
lands in the turn that asked for it even if you reloaded in the meantime.

**GPU / VRAM notes.** To fit consumer cards, the text encoder and VAE run on CPU
RAM while the UNet stays on the GPU, the VAE decodes in tiles, and flash attention
is used on CUDA. This keeps **Stable Diffusion 1.5 at 512×512** within ~6 GB of
VRAM (verified on a GTX 1060 6 GB). Larger resolutions (768×768) or SDXL need
more VRAM and may not fit on 6 GB cards. On high-VRAM GPUs this default is safe but
slightly conservative (the VAE could run on-GPU for a little more speed).

### Your own MCP servers (Settings → Apps → MCP servers)
Connect [Model Context Protocol](https://modelcontextprotocol.io) servers so their
tools join the assistant's toolbox, behind the same read/change/send gate as
[Connected apps](#connected-apps-settings--apps):
- **Remote link** (Streamable HTTP) — paste a URL; optional bearer token stored in
  the OS credential store.
- **Local command** (stdio) — run a local MCP server process (e.g.
  `npx -y @modelcontextprotocol/server-filesystem C:/path`). It starts without
  Poiesis's own variables or your model providers' keys.
- **Export / Import** your connector setup as JSON (secrets excluded).

### Models, Providers and Runtime (Settings)
Three sections answer three questions:
- **Models** — *what can I use?* One list of local models, cloud models and
  models on your own servers, with a default and favorites (a cloud model can
  be the default), plus the decision model and the image, voice and recall
  engines' models.
- **Providers** — *where does it come from?* Add an **OpenAI**, **OpenRouter**
  or **Anthropic** key (verified before it says "saved"), or your own model
  servers. Keys live in Windows Credential Manager, never in a file or your
  chats. A **Local-only** filter hides cloud models entirely.
- **Runtime** — *what runs on this PC?* The llama.cpp engine and its backend,
  the image, voice, recall and decision engines, and where each runs.

---

## Privacy & security

- **Local by default.** Nothing leaves your device unless you use a tool or model
  that inherently requires the network (web search, browser, mail, connected
  apps, cloud models, engine and model downloads). Those are clearly labeled and
  every one of them defaults off. Reading your folders, recall, reranking,
  voice, quick decisions and on-device image generation are all local unless you
  choose otherwise.
- **Quick decisions go to the cloud only if you pick that.** The cloud backend
  (OpenRouter) is off until chosen in Settings → Runtime, sends only to
  providers that keep nothing, and never takes a background question (mail
  triage, the reflection gate, the outside-text flag) unless you opt that one in
  by name. A typed draft is never written to the log.
- **Agent apps run with a scrubbed environment.** Claude Code, Codex and OpenCode
  start without Poiesis's variables or your providers' keys, can't read your
  Composio key, and can use your memory or your connected apps only through a
  per-run loopback token that works while Poiesis runs the job. Their approval
  requests come to Poiesis's panel, not their own.
- **Connected-app calls are sorted before they run.** Read runs; change and send
  ask (a send every time, or under a daily cap you set); an unattended run leaves
  the approval for you rather than guessing. A sign-in link never reaches the
  model or the transcript.
- **Text from outside is treated as data**, not instructions — marked at every
  intake, shown to you with its source, and barred from becoming a durable
  memory.
- **The agent's memory is yours** — plain markdown in the app-data directory. You
  can read, edit or delete any of it without the app running, and every write the
  agent makes is shown as it happens, with an Undo.
- **Secrets** (cloud keys, MCP tokens) are stored in the **OS credential store** —
  never in SQLite or plaintext.
- The local engine binds **127.0.0.1 only** with a per-session random token.
- The **browser** tool drives your installed Chrome/Edge in a dedicated profile
  under app-data — your real cookies and sessions are never touched — and the
  first visit to a new domain asks. **Mail** connects your account directly over
  IMAP/SMTP with credentials in the OS credential store; there is no hosted
  relay, and sending always asks.
- File access is **deny-by-default**: you whitelist folders per-chat or permanently,
  Read-Only / Ask-first / Full, with path-traversal + symlink-escape protection.
  Anything that destroys bytes is snapshotted first and restorable from the
  **Changes** view in the sidebar. Every file/tool action is written to a
  visible **Activity log**.
- **Project tasks run your project's own commands** and are off by default.
  They are confined by a Windows Job Object (time limit, memory and process
  caps, the whole process tree ends on Stop) with secret-looking environment
  variables removed. That is not network or filesystem isolation, and the
  settings say so. The agent never commits, pushes or installs globally on its
  own, and free-form commands never run through a shell.
- **Scheduled tasks are read-only** and never prompt — see Tasks, above.
- Telemetry is **off by default**, content-free, and stays on your PC.

---

## Project layout

```
poiesis/
├── src/                      # React + TypeScript frontend
│   ├── routes/               # Chat, Workspace, Models, Providers, Runtime, Engines
│   │                         #   (External agents), Apps, Library, Skills, Self,
│   │                         #   Tasks, Mail, Activity, Usage, Tools, Experiments,
│   │                         #   ProjectView, SettingsHub
│   ├── components/           # TopBar (the tab strip), Rail (chats + projects),
│   │                         #   Apps (Composio + MCP servers), Engines (rule
│   │                         #   lists), DecisionModel, Voice, CommandPalette,
│   │                         #   Updates,
│   │                         #   Workbench (the right sidebar: folder header,
│   │                         #   Files/Artifacts/Agents/Browser/Changes, item
│   │                         #   tabs, diff view), Conversation (timeline, task
│   │                         #   step, diagnostics, permission prompts),
│   │                         #   Composer, Blocks, Memory, Self, Personas, Context
│   ├── lib/                  # api.ts (Tauri invoke wrappers), store.ts (Zustand:
│   │                         #   tabs, projects, change sets, streams),
│   │                         #   context.ts, personaPresets.ts, types.ts
│   └── styles/tokens.css     # "Paper / Slate" design tokens
├── src-tauri/                # Rust backend
│   └── src/
│       ├── runtime/          # hardware, download, process, proxy, jobobject, watchdog,
│       │                     #   imageengine, embedserver, rerankserver,
│       │                     #   decideserver (the decision model), voice*
│       ├── engines/          # agent apps: catalog + detection, one driver per
│       │                     #   protocol (claude, codex, acp), approvals, rules,
│       │                     #   tools Poiesis serves to them, Try it, run
│       ├── connectors/       # Composio Connect / Project transports
│       ├── experiments.rs    # the Experiments registry (+ experiments_report.rs)
│       ├── agent/            # the loop (run.rs) + toolsets (filesystem, websearch,
│       │                     #   codeexec, artifacts, imagegen, present, recall,
│       │                     #   memory_skill, retrieval, mail, browser, screen,
│       │                     #   skillpack, subagents, classify), connector_gate +
│       │                     #   app_calls (connected apps), commands (the `/`
│       │                     #   menu's harness and ask_user tools), goal, rewind,
│       │                     #   verify + autocheck + sentences (answers check
│       │                     #   themselves), decide/ (the decision seam, policy,
│       │                     #   controller, outcomes, calibration and every site
│       │                     #   that asks), folder index, untrusted,
│       │                     #   golden, perceptual hashing, duplicates, sandbox,
│       │                     #   trash, plus the harness: fleet.rs (a handle per
│       │                     #   run), log.rs (the session log), context.rs (prompt
│       │                     #   assembly), results.rs (big tool output on disk),
│       │                     #   background.rs (agents that outlive the turn),
│       │                     #   toolrpc.rs (scripts calling tools), plan.rs (the
│       │                     #   run plan); and for code: project.rs (project
│       │                     #   card, AGENTS.md), ledger.rs (read-before-edit),
│       │                     #   coderun.rs (run_task/run_command + policy),
│       │                     #   diagnostics.rs (build/test output parsers),
│       │                     #   diff.rs + changes.rs (the Changes view),
│       │                     #   symbols.rs (tree-sitter symbols, find_symbol)
│       ├── memory/           # the durable self on disk (facts, lessons, SOUL, PROFILE)
│       ├── media/            # media backend seam + background jobs (local/OpenRouter/OpenAI)
│       ├── autonomy.rs       # what the agent may change about itself without asking
│       ├── mcp/              # MCP client (HTTP + stdio transports)
│       ├── cloud/            # BYOK providers (OpenAI/OpenRouter/Anthropic) + turn driver
│       ├── db/               # SQLite schema (v42) + access (FTS5 + vector store)
│       ├── commands/         # Tauri command surface (the IPC API), incl. scheduler,
│       │                     #   reflect and projects
│       ├── permissions/      # file-access grants + guards
│       └── secrets.rs        # OS credential store
├── shared/                   # catalogs both sides read: commands.json (the `/` menu),
│                             #   engines.json (agent apps), apps.json +
│                             #   composio-catalog.json (connected apps)
├── fixtures/                 # prompt-assembly golden, recorded agent-app sessions
│                             #   (engines/), Composio replies, decision replies, voice
├── docs/                     # technical documentation (agent harness architecture)
└── plans/                    # plans, specs, task checklist, PRD
```

Where local state lives at runtime (under the app-data directory,
`%APPDATA%\com.projectpoiesis.app`): `poiesis.db` (SQLite), `memory/` (the agent's
durable self, as markdown), `skills/` (Agent Skills), `runtimes/` (engines),
`models/` (weights), `generated-images/` (image and video outputs).

---

## Tests

```sh
cd src-tauri && cargo test --lib    # 1,586 backend tests (17 ignored: they need real models)
npm test                            # 1,363 frontend tests (vitest)
npx tsc --noEmit                    # frontend type safety
```

(Counted 2026-10-27 on the working tree after `6b1e851`: `cargo test --lib` 1,586
passed; vitest 1,362 of 1,363, the one failure being a stale expectation in
`ThoughtStrip.test.tsx`; `tsc` reports one error in `AgentRun.tsx`, an extra
`messageId` prop on the plan card. Both are in uncommitted work.)

Covers hardware/runtime selection against **real** llama.cpp and
stable-diffusion.cpp release asset names, SQLite + FTS5 + vector search,
permission path-traversal guards, memory round-trips and caps, retrieval
scoring, the loop's prose/tool-call flush heuristic and fail→fix mining, the
autonomy gate, untrusted scanning, Agent Skill parsing and disclosure caps, the
media backend seam, the scheduler, the MCP stdio command parser, and web-search
HTML parsing. One test asserts that unattended runs refuse rather than block on
a prompt — its failure mode is hanging, so it asserts under a timeout.

The harness adds: a mid-run instruction landing as a plain user message at the
top of the next turn and never inside a tool round; a spent step or time budget
buying one tool-free closing turn rather than an abort; a run that did work and
then said nothing being asked once for the answer; parallel dispatch keeping the
model's own call order whatever order the work finished in; the 8 KB
inline/offload boundary; cancelling a tree of agents without touching a sibling
tree; a child's toolsets being the intersection of its own and its parent's;
replaying a logged run reproducing the exact message array that was sent;
forking copying the log up to a point and nothing after it; and every compaction
being kept with the stretch of conversation it stands for.

The shell, projects and coding work add: one tablist holding chats, items and
routes; the agent moving the sidebar but never a tab; restoring a saved tab set
and dropping what no longer resolves; the project migration and the three
attach cases; project memory fenced from other projects; project detection per
manifest type; read-before-edit refusing an unread or changed file; a read-only
project, a headless run and shell metacharacters all refusing a task; an
allowlisted `git status` never letting `git push` through; every build and test
output parser against sample output; identical patches for a git and a non-git
folder; the unverified-change reminder firing once; symbol extraction per
grammar; a code chunk never splitting a function; and definitions ranking above
uses.

Agent apps, connected apps and quick decisions add: an agent app driven end to
end against a fake of each protocol (and real recordings of Claude Code, Codex
and OpenCode) with its approvals reaching the panel, an unattended run being
refused rather than asked, a stopped run withdrawing its open question, and a
Codex run on Windows with no sandbox asking about every step; a handover rule
being cited, refused when unknown or "avoid", and relaxed one rule at a time;
the child environment carrying no Poiesis variable or provider key; every
connector call sorted into read, change or send by name, hint and batch, the
strictest of a batch winning, the daily cap counting targets, and a headless
send being left for the person; a sign-in link never reaching the model; the
decision seam against the real reply recorded from Clef-flash and against a mock
OpenRouter (each error status is the fault it means); the tool menu never
narrowing a child, a resumed run or a scheduled task; a tool-call check that can
add a question but never remove one; typed words never being written to the log;
and an answer check that marks "I ran the tests" only when no step did.

**Prompt assembly is built in two places and pinned together.** The backend
(`agent/context.rs`) and the frontend (`lib/store.ts`) each render
`fixtures/prompt-assembly.json` and must match `fixtures/prompt-assembly.golden.txt`
**byte for byte** — one Rust test and one vitest against one file. Editing
either side alone fails the build. `UPDATE_PROMPT_GOLDEN=1 cargo test --lib
context_golden` rewrites the golden; the vitest then has to agree without being
told.

Two integration harnesses are `#[ignore]`d by default because they need real
models or fixtures on the machine: `tests/eval.rs` (drives a real agent run
against fixtures and dispatches tool calls for real) and `tests/embed.rs`. They
still have to *compile* on every `cargo test`, which is the point — `eval.rs`
was silently left behind by a loop signature change and nobody noticed, because
`cargo test --lib` never builds it.

A separate lint (`tests/copy_lint.rs`) fails the build if engineering
vocabulary ("embedding", "vector", "chunk", "semantic", "threshold"…) appears
in user-facing frontend copy.

Some behaviour can only be checked by hand against real models — see **Status**.

---

## Troubleshooting

- **"No model is loaded yet."** Download/select a local model in **Models**, or
  pick a cloud model (with a key set). The engine starts on demand.
- **Backend won't compile / linker error.** Install the MSVC C++ Build Tools
  ("Desktop development with C++") so Rust has a linker.
- **Tools do nothing in chat.** Turn on the **⚒** toggle in the composer, and enable
  the specific group in **Settings → Tools** (web search, code execution, mail,
  browser, screen and image generation all default to off).
- **It doesn't remember anything between chats.** Recall needs the embedding
  engine; install it from **Settings → Recall** (or Engine → Recall in Everything
  mode). Until then it falls back to keyword matching.
- **A scheduled task never runs.** Tasks only run while the app is open — there is
  no background service. Check the task is enabled, and that another task isn't
  already running (only one runs at a time).
- **Asking for a picture gets a description instead.** No image route is
  available yet. Either install the on-device engine (**Engine → Image**) and
  download a model (**Models → Image**), or add an OpenRouter/OpenAI key — then
  pick the media model in the model picker.
- **A skill never fires.** Check it's enabled in **Settings → Skills**, and that
  its `when_to_use` line actually describes the request — that one line is all
  the agent sees before deciding to read the skill.
- **The browser tool says it's unavailable.** It drives an installed
  Chromium-family browser and won't download one; install Chrome or Edge.
- **Image model download stops partway.** It's resumable — re-click download and it
  continues from where it left off (`.part` file + HTTP range). If the catalog URL
  fails entirely, use **Add by URL** in **Models → Image** with a direct
  `.safetensors` link.
- **Image generation fails with "CUDA out of memory" / OOM.** Your GPU ran out of
  VRAM. Stick to Stable Diffusion 1.5 at 512×512 on ~6 GB cards; larger sizes or
  SDXL need more VRAM. Closing other GPU-heavy apps frees headroom.
- **A cloud/MCP capability says it needs authorization.** Add the provider key
  (Settings) or the connector token (Apps).
- **A run says "thinking" for a long time.** That is a reasoning model. Open the
  **Thinking** line beneath to watch it — that is the model's reasoning, not its
  answer, which is why it stays folded. Three clocks end a run that is going
  nowhere, so none of these waits is forever: three minutes for a provider that
  never starts answering, two minutes of silence mid-stream, and two minutes of
  thinking that never produces a word. The last one exists because a model
  looping in its own reasoning keeps the other two alive.
- **A model thinks far too long, or costs more than it should.** Set how hard it
  thinks in the composer, next to the model: *No thinking / Think briefly /
  Think / Think hard*. It defaults to **Think briefly** — every provider's own
  default is its maximum, which is slow, expensive, and the setting most likely
  to end in a model that never finishes. *Model's default* sends nothing and
  takes theirs. A provider that does not accept the setting is asked again
  without it, so it can never cost you an answer.
- **How many steps does a run get?** Twelve by default, and it is yours to set
  in **Settings → Tools**. A step is one round of tool calls. Raise it for long
  builds; most questions never get near it. Running out is not a failure — the
  run stops and answers with what it has.
- **The agent won't run the tests, or there is no Tasks chip.** Turn on **Run
  project tasks** in Settings → Tools, check the folder's access is not *Read
  only*, and check the project's own setting under **Tasks** in the folder
  header is not *Off*. Tasks come from the manifests; if you added a script,
  press **Re-detect** there.
- **A turn shows its steps and then "I did the steps above but ended without
  writing an answer".** The model returned an empty reply after its tool
  results, even after being asked once more. Ask it what the steps showed, or
  use a stronger model: free tiers often do this after a tool call.
- **An agent finishes with "(it produced no text)".** The child ran but its model
  returned an empty reply. The lead is asked once to write the answer from what
  came back; if that is empty too, try a different model for the children — free
  tiers do this under load.
- **An agent app isn't listed, or says it can't start.** Settings → External
  agents finds Claude Code, Codex and OpenCode by looking where they install
  (and in the Claude Code extension folder of VS Code); PATH alone is not proof.
  Use **Try it** on the card: it runs a task that cannot change anything and
  shows the agent's last words if it fails. Sign in inside the agent app itself
  first; the card shows who pays (your plan, an API key, or free).
- **The lead never hands work to an agent app.** Its mode may be *Only when you
  ask*, the folder may not be a code project (a guard), the chat may have
  external agents off (`/external`), or the folder may be read-only. Settings →
  External agents → **Try a request** says which.
- **Codex asks about every step on Windows.** Its sandbox is not set up on this
  computer, so Poiesis keeps it on the strict setting and says so in the report.
- **The model says it can't use an app.** Check Settings → Apps: the key must be
  the `ck_` key from Composio (For You → Settings → Sessions & API Key), the app
  must be connected, and the persona must not be limited to other apps. A call
  to an app with two accounts needs to name one.
- **A connected-app action waits for me.** A change or a send asks. In a
  scheduled task or a background agent nobody is there to ask, so it is left in
  your inbox instead of being sent.
- **Fast & slow thinking does nothing.** It needs a decision model: download one
  in Models, or choose OpenRouter in Settings → Runtime (this also needs a key
  in Providers). On a graphics card too small for it, the quick decisions that a
  person waits for are skipped rather than slowing the chat; the Runtime card
  says where it runs.
- **A tool group I need is gone from this turn.** With Fast & slow thinking on, a
  group the message clearly didn't need is held back; the model can ask for it,
  or press the held-tools chip in the run bar. Turn the experiment off if it gets
  in the way.

---

## Status

Implemented: core phases 0–9, the **durable self** and its maintenance loop
(phases 10–11), **recall, retrieval and tasks** (phase 12), **multimodal
creation** (phase 13), the **capabilities** work — Agent Skills, mail, browser
control, screen/app access, untrusted-content marking, fail→fix mining and the
golden-set self-check — and all six phases of the **harness and subagents**
programme: steering, graceful budgets, delegation you can watch, parallel tool
calls, cost accounting, the session log with fork and resume, background agents,
scripts calling tools, and prompt assembly moved into the backend behind the
byte-identical gate. Since then: the **shell** (one tab strip, a sidebar that
navigates itself), **projects** (with or without a folder, instructions,
project memory), **run plans**, and the **coding** work (project card and
`AGENTS.md`, read-before-edit, project tasks with an execution policy, parsed
build and test output, the Changes view with undo, `find_symbol`, and running
code artifacts). Then came **models, providers and runtime** (one question per
settings section), the **`/` menu** with the agent's own `harness` and
`ask_user` tools, **voice**, **auto-update** (a Tauri updater against GitHub
Releases that always asks before installing), **agent apps** (Claude Code,
Codex and OpenCode as delegation targets, with a visible rule set for when work
is handed over), **connected apps** (a gate on every MCP connector, then
Composio Connect) and the **dual-process work** (a decision engine, Experiments,
and steps 1–19 of the plan). The database schema is at v42.

**Verified live:** local chat and local image generation (CUDA on a GTX 1060
6 GB — Stable Diffusion 1.5 at 512×512), cloud chat via OpenRouter, and the
folder / recall / memory surfaces. Also run for real: **Claude Code and OpenCode**
through `delegate` with the Poiesis tools mounted, **Codex 0.163** through
`delegate`'s path (it mounts the tools but its model skips them about half the
time), and **Google Calendar** connected from the Apps page with a Composio
`ck_` key and read from the chat.

**Quick decisions are barely exercised on a real model.** The decision engine,
the OpenRouter backend, the policy table, calibration and every use of them are
built and tested against recordings, fakes and mock servers. The engine ran
against a real Clef-flash in step 2, and the first hand use (2026-10-26) showed
the tool-hold chip and little else, which is why v3 made the decisions visible;
after v3 the chat was found too loud, and the v4 revision that quiets it is
written but not built. No request has reached OpenRouter, step 20 of the plan
(a deliberate checklist run with a decision model) has not been done, and every
threshold is a starting value that nobody has measured.

**Agent apps and connected apps** are built, backend and frontend, and checked
against fakes and recordings; what's left is a hand pass: the handover card
(including pressing Stop with it open), the in-chat connect card, a second
account for one app, and delegating a connected-app read and a send through a
real agent. Real agent trials cost the user's quota, so they are few and
deliberate. The agent-app work is committed (`6b1e851`); the review fixes
since, the decision-engine work and the connected-apps follow-ups are not.

**Built but not exercised end-to-end.** Much of the last several phases is only
checkable against real models, real mailboxes and real websites — leaving a
nightly task running overnight, teaching a lesson in one chat and seeing it
applied in another, a full browse-and-extract run. Those haven't been run. The
first manual pass over the scheduler found five defects in code that compiled
and passed its tests, one of which hung it until restart; the first manual pass
over delegation found two more, including a run that finished its work and then
discarded all of it behind an empty turn. Compile-clean and test-green is not
the same as working, and this README says so on purpose.

The coding work has had one short manual run so far (a small test project in
[coding-testbed/](coding-testbed/)). It found three defects, all fixed: every
message was being sent to the model twice, Node's test runner output was not
understood, and a turn that ran a task and then got an empty reply looked like
a crash. The Changes view, the project chips and `find_symbol` have not been
exercised against a real project yet.

**Reasoning models.** Providers that stream a model's thinking in a field of its
own — several OpenRouter free tiers do — now show it as thinking: a folded line
under the run meter, which you can open. It is never presented as the answer.
A turn that goes nowhere gives up on its own rather than hanging until you press
Stop: three minutes for a provider that never starts answering, two minutes of
silence mid-stream, and two minutes of thinking that produces no answer — that
last one because a model looping in its own reasoning is *busy*, and so keeps
the other two clocks alive indefinitely.

A second manual pass found both halves of that: the thinking was being captured
and thrown away, and the timeout that was supposed to catch a dead provider only
guarded the gaps *between* chunks — so a provider that never sent a first chunk
was never caught by it at all.