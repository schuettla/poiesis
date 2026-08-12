# Poiesis Agent

*(formerly Project Nexus — renamed 2026-07; internal identifiers were
migrated to `poiesis` on 2026-08-11, see `plans/POIESIS_PLAN.md`.)*

A **local-first, agentic desktop LLM application** for Windows. Chat with local
models that run entirely on your machine, give the assistant real capabilities
(files, folder reading, web search, code execution, image generation, external
apps), and optionally bring your own cloud API keys — all in a calm, editorial
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
- **Image engine:** prebuilt `stable-diffusion.cpp` (downloaded on demand)
- **Storage:** embedded SQLite (FTS5 full-text search + a vector store), plus markdown for the agent's memory
- **Secrets:** OS credential store (Windows Credential Manager) — never in files or the database

**How the agent actually works** — the loop, the toolsets, prompt assembly,
consent and the event protocol — is written up in
[docs/AGENT_HARNESS.md](docs/AGENT_HARNESS.md).

Plans and specs live in [plans/](plans/): `IMPLEMENTATION_PLAN.md`
(architecture, phases 0–8), `FILESYSTEM_PLAN.md` (working folders, trust,
undo), `POIESIS_PLAN.md` (the durable self and its maintenance loop),
`PERCEPTION_PLAN.md` (recall, retrieval, tasks), `CAPABILITIES_PLAN.md`
(Agent Skills, mail, browser, untrusted content, the self-checking harness),
`MULTIMODAL_PLAN.md` (media backends and in-stream creation), `TASKS.md`
(checklist), and `Project_Nexus_PRD.md` (product spec).

---

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
3. **Attach a folder** in the right-hand Workbench panel and the agent reads it,
   so you can ask questions phrased in your own words rather than the file's.
4. **To create images or video:** just ask for one in the conversation — "draw a
   fox in the snow". Pick where it's made from the same model picker you pick a
   chat model from (see [Making images and video](#making-images-and-video)).
5. Everything else lives behind the **cog** at the bottom of the left rail:
   General, Models, Engine, Apps, **Skills**, **Self** (memory, lessons, health,
   autonomy) and **Tasks** (scheduled work).

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
  prompt nobody is there to answer — they stop and report instead.

### Tools (what the agent can do)
Toggle the **⚒ tools** button in a chat to let the assistant act. Each group is
independently switchable in **Settings → Tools** (Everything mode). Anything
that leaves your device or runs code defaults **off**:

| Tool group | Default | Notes |
|---|---|---|
| **File access** | on | Read/write in folders you allow; every change asks first unless you say otherwise |
| **Artifacts** | on | Renders HTML / SVG / markdown / code in the Workbench side panel |
| **Workspace UI** | on | Lets it compose a live interface for the task — a board, a checklist, a picker — instead of describing things in prose |
| **Recall** | on | Searches your past conversations and saved memories. Stays on your device |
| **Memory** | on | Lets the agent keep durable notes about you, visibly and undoably |
| **Folder reading** | on | Reads an attached folder ahead of time so it can find things by meaning. Grants no new folder on its own |
| **Skills** | on | Reads and proposes Agent Skills (below). Reading is free; keeping a new one still asks |
| **Image generation** | off | The on-device diffusion path as a chat tool. Asking for a picture in conversation doesn't depend on this toggle |
| **Web search** | off | No-key DuckDuckGo query issued from your machine (your query leaves the device) |
| **Code execution** | off | Runs Python / Node in a confined, time- and memory-limited sandbox — also the data-analysis path over an attached folder |
| **Mail** | off | Reads, searches and sends over an account you connect directly (IMAP/SMTP, no relay). Sending always asks first |
| **Browser** | off | Opens pages and clicks around in your *installed* Chrome or Edge, in its own profile. The first visit to a new site asks first |
| **Screen & apps** | off | A screenshot, or launching an app by name. Both ask first. Deliberately not GUI automation |

Personas can carry their **own tool set** and their own skill set, so a
"writing" persona need not have the same reach as a "research" one — a persona
can narrow what's enabled globally, never widen it.

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

### External apps via MCP (Apps tab)
Connect [Model Context Protocol](https://modelcontextprotocol.io) servers so their
tools join the assistant's toolbox:
- **Remote link** (Streamable HTTP) — paste a URL; optional bearer token stored in
  the OS credential store.
- **Local command** (stdio) — run a local MCP server process (e.g.
  `npx -y @modelcontextprotocol/server-filesystem C:/path`).
- **Export / Import** your connector setup as JSON (secrets excluded).

### Cloud models — bring your own key (Settings → Cloud models)
Optional. Add an **OpenAI**, **OpenRouter**, or **Anthropic** key to use hosted
models alongside local ones in the same picker (grouped by provenance). Keys live
in Windows Credential Manager, never in a file or your chats. A **Local-only**
filter hides cloud models entirely.

---

## Privacy & security

- **Local by default.** Nothing leaves your device unless you use a tool or model
  that inherently requires the network (web search, browser, mail, cloud models,
  engine and model downloads). Those are clearly labeled and every one of them
  defaults off. Reading your folders, recall, reranking and on-device image
  generation are all local.
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
  Workbench. Every file/tool action is written to a visible **Activity log**.
- **Scheduled tasks are read-only** and never prompt — see Tasks, above.
- Telemetry is **off by default**, content-free, and stays on your PC.

---

## Project layout

```
poiesis/
├── src/                      # React + TypeScript frontend
│   ├── routes/               # Chat, Workspace, Surface, Models, Engine, Apps,
│   │                         #   Library, Skills, Self, Tasks, Activity, Settings
│   ├── components/           # Composer, Conversation, Workbench, Rail, Memory,
│   │                         #   Self, Personas, Blocks, Surface, Context, TopBar
│   ├── lib/                  # api.ts (Tauri invoke wrappers), store.ts (Zustand),
│   │                         #   context.ts, mediaIntent.ts, growth.ts, types.ts
│   └── styles/tokens.css     # "Paper / Slate" design tokens
├── src-tauri/                # Rust backend
│   └── src/
│       ├── runtime/          # hardware, download, process, proxy, jobobject, watchdog,
│       │                     #   imageengine, embedserver, rerankserver
│       ├── agent/            # the loop (run.rs) + toolsets (filesystem, websearch,
│       │                     #   codeexec, artifacts, imagegen, present, recall,
│       │                     #   memory_skill, retrieval, mail, browser, screen,
│       │                     #   skillpack), folder index, untrusted, golden,
│       │                     #   perceptual hashing, duplicates, sandbox, trash
│       ├── memory/           # the durable self on disk (facts, lessons, SOUL, PROFILE)
│       ├── media/            # media backend seam + background jobs (local/OpenRouter/OpenAI)
│       ├── autonomy.rs       # what the agent may change about itself without asking
│       ├── mcp/              # MCP client (HTTP + stdio transports)
│       ├── cloud/            # BYOK providers (OpenAI/OpenRouter/Anthropic) + turn driver
│       ├── db/               # SQLite schema + access (FTS5 + vector store)
│       ├── commands/         # Tauri command surface (the IPC API), incl. scheduler + reflect
│       ├── permissions/      # file-access grants + guards
│       └── secrets.rs        # OS credential store
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
cd src-tauri && cargo test          # 286 backend tests + 3 copy lint
npm test                            # 52 frontend tests (vitest)
npx tsc --noEmit                    # frontend type safety
```

Covers hardware/runtime selection against **real** llama.cpp and
stable-diffusion.cpp release asset names, SQLite + FTS5 + vector search,
permission path-traversal guards, memory round-trips and caps, retrieval
scoring, the loop's prose/tool-call flush heuristic and fail→fix mining, the
autonomy gate, untrusted scanning, Agent Skill parsing and disclosure caps, the
media backend seam, the scheduler, the MCP stdio command parser, and web-search
HTML parsing. One test asserts that unattended runs refuse rather than block on
a prompt — its failure mode is hanging, so it asserts under a timeout.

Two integration harnesses are `#[ignore]`d by default because they need real
models or fixtures on the machine: `tests/eval.rs` (drives a real agent run
against fixtures and dispatches tool calls for real) and `tests/embed.rs`.

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

---

## Status

Implemented: core phases 0–9, the **durable self** and its maintenance loop
(phases 10–11), **recall, retrieval and tasks** (phase 12), **multimodal
creation** (phase 13), and the **capabilities & harness** work — Agent Skills,
mail, browser control, screen/app access, untrusted-content marking, fail→fix
mining and the golden-set self-check. `cargo test` (286 + 3), `npm test` (52)
and `npx tsc --noEmit` all pass clean.

**Verified live:** local chat and local image generation (CUDA on a GTX 1060
6 GB — Stable Diffusion 1.5 at 512×512), cloud chat via OpenRouter, and the
folder / recall / memory surfaces.

**Built but not exercised end-to-end.** Much of what the last two phases add is
only checkable against real models, real mailboxes and real websites — leaving a
nightly task running overnight, teaching a lesson in one chat and seeing it
applied in another, a full browse-and-extract run. Those haven't been run. The
first manual pass over the scheduler found five defects in code that compiled
and passed its tests, one of which hung it until restart. Compile-clean and
test-green is not the same as working, and this README says so on purpose.

**Specified but not built:**

- Seeing images and scanned documents inside a folder — vision captioning, OCR,
  table extraction (`plans/PERCEPTION_PLAN.md` Part IV).
- The agent seeing the media it just generated (`SEE-1`), and the fal.ai media
  backend (`BKD-3`) — both deferred in `plans/MULTIMODAL_PLAN.md`.
- Orphaned-media cleanup runs on conversation delete only.
