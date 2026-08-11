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
├─ lessons/       what it learned from its own mistakes
└─ recipes/       procedures the two of you developed together
```

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
  deleted, and surfaced in **Self → Health** so you can fix or discard it.

---

## Tech

- **Shell:** Tauri v2 (Rust backend + system WebView)
- **Frontend:** React 18 + TypeScript + Vite + Zustand
- **Local engine:** externally-managed, prebuilt `llama-server` (llama.cpp) over loopback HTTP
- **Embedding + reranking engines:** prebuilt, CPU-only, lazily started and idle-stopped
- **Image engine:** prebuilt `stable-diffusion.cpp` (downloaded on demand)
- **Storage:** embedded SQLite (FTS5 full-text search + a vector store), plus markdown for the agent's memory
- **Secrets:** OS credential store (Windows Credential Manager) — never in files or the database

Plans and specs live in [plans/](plans/): `IMPLEMENTATION_PLAN.md`
(architecture, phases 0–8), `FILESYSTEM_PLAN.md` (working folders, trust,
undo), `POIESIS_PLAN.md` (the durable self and its maintenance loop),
`PERCEPTION_PLAN.md` (recall, retrieval, tasks), `CAPABILITIES_PLAN.md`,
`TASKS.md` (checklist), and `Project_Nexus_PRD.md` (product spec).

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
   assistant use its skills.
3. **Attach a folder** in the right-hand Workbench panel and the agent reads it,
   so you can ask questions phrased in your own words rather than the file's.
4. **To create images:** install the image engine in **Engine → Image**, download a
   diffusion model in **Models → Image**, then use the **◲** button in the chat
   composer (see [Image generation](#image-generation-engine--image--models--image)).
5. Everything else lives behind the **cog** at the bottom of the left rail:
   General, Models, Engine, Apps, **Self** (memory, lessons, recipes, health,
   autonomy) and **Tasks** (scheduled work).

By default the app runs in **Simple** mode, which keeps the machinery out of
the way. **Settings → General → "Show me everything"** reveals the engine
controls, per-skill toggles and other expert surfaces.

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

### Reading your folders
Attach a folder to a chat and the agent reads it in the background, then answers
from it — citing the files it used, which you can open in the Workbench viewer.
Questions don't have to share vocabulary with the files; matching is by meaning.
Optionally, a second local engine **re-reads the best candidates** more carefully
before answering (Settings → Recall → *Sharper*). Nothing is uploaded — both
engines run on your machine.

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

### Skills (agentic capabilities)
Toggle the **⚒ tools** button in a chat to let the assistant act. Each skill is
independently switchable in **Settings → Skills** (Everything mode):

| Skill | Default | Notes |
|---|---|---|
| **File access** | on | Read/write in folders you allow; every change asks first unless you say otherwise |
| **Memory** | on | Lets the agent keep durable notes about you, visibly and undoably |
| **Artifacts** | on | Renders HTML / SVG / markdown / code in the Workbench side panel |
| **Web search** | off | No-key DuckDuckGo query issued from your machine (your query leaves the device) |
| **Code execution** | off | Runs Python / Node in a confined, time- and memory-limited sandbox — also the data-analysis path over an attached folder |
| **Image generation** | off | Optional chat tool. The main way to make images is the composer **◲** mode (see below), which doesn't depend on this toggle |

Personas can carry their **own tool set**, so a "writing" persona need not have
the same reach as a "research" one.

### Personas (Settings → Personas)
Saved profiles bundling a **system prompt** + optional **pinned model** +
**temperature** + **tool set**. Pick one per chat from the composer dropdown; a
one-off temperature override is also supported per conversation.

### Image generation (Engine → Image · Models → Image)
Local text→image, structured exactly like local chat — a separate **engine** and a
**model library**, each on its own tab:

1. **Engine → Image → "Install image engine"** — downloads the `stable-diffusion.cpp`
   build matched to your GPU (CUDA / Vulkan / ROCm / CPU). One-time, from GitHub.
2. **Models → Image** — download a diffusion model from the catalog (Stable
   Diffusion 1.5 ~4 GB, SDXL), add one by URL, set a default, or delete. Downloads
   are **resumable**: they stream to a `.part` file and continue via HTTP range
   requests if interrupted, and a truncated file is never mistaken for a finished
   model.
3. **In any chat**, click the **◲** button in the composer to enter *Create-image*
   mode, pick a model from the dropdown, type a description, and send. The image
   appears **inline in the conversation** so you can iterate — it's generated
   directly by the engine, not routed through the chat model. Fully local; nothing
   is uploaded. (Power users can point at their own binary/model under "Advanced".)

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

- **Local by default.** Nothing leaves your device unless you use a skill or model
  that inherently requires the network (web search, cloud models, engine and model
  downloads). Those are clearly labeled. Reading your folders, recall, reranking
  and image generation are all local.
- **The agent's memory is yours** — plain markdown in the app-data directory. You
  can read, edit or delete any of it without the app running, and every write the
  agent makes is shown as it happens, with an Undo.
- **Secrets** (cloud keys, MCP tokens) are stored in the **OS credential store** —
  never in SQLite or plaintext.
- The local engine binds **127.0.0.1 only** with a per-session random token.
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
│   ├── routes/               # Chat, Workspace, Models, Engine, Apps, Library,
│   │                         #   Self, Tasks, Settings (+ SettingsHub)
│   ├── components/           # Composer, Conversation, Workbench, Rail, Memory,
│   │                         #   Self, Personas, Blocks, Context, Mark, TopBar
│   ├── lib/                  # api.ts (Tauri invoke wrappers), store.ts (Zustand), types.ts
│   └── styles/tokens.css     # "Paper / Slate" design tokens
├── src-tauri/                # Rust backend
│   └── src/
│       ├── runtime/          # hardware, download, process, proxy, jobobject, watchdog,
│       │                     #   imageengine, embedserver, rerankserver
│       ├── agent/            # loop + skills (filesystem, websearch, codeexec, artifacts,
│       │                     #   imagegen, memory_skill, recipes), folder index, retrieval,
│       │                     #   recall, perceptual hashing, sandbox, trash
│       ├── memory/           # the durable self on disk (facts, lessons, recipes, SOUL, PROFILE)
│       ├── autonomy.rs       # what the agent may change about itself without asking
│       ├── mcp/              # MCP client (HTTP + stdio transports)
│       ├── cloud/            # BYOK providers (OpenAI/OpenRouter/Anthropic)
│       ├── db/               # SQLite schema + access (FTS5 + vector store)
│       ├── commands/         # Tauri command surface (the IPC API), incl. scheduler + reflect
│       ├── permissions/      # file-access grants + guards
│       └── secrets.rs        # OS credential store
└── plans/                    # plans, specs, task checklist, PRD
```

Where local state lives at runtime (under the app-data directory,
`%APPDATA%\com.projectpoiesis.app`): `poiesis.db` (SQLite), `memory/` (the agent's
durable self, as markdown), `runtimes/` (engines), `models/` (weights),
`generated-images/` (image outputs).

---

## Tests

```sh
cd src-tauri && cargo test          # 185 backend tests + 3 copy lint
npx tsc --noEmit                    # frontend type safety
```

Covers hardware/runtime selection against **real** llama.cpp and
stable-diffusion.cpp release asset names, SQLite + FTS5 + vector search,
permission path-traversal guards, memory round-trips and caps, retrieval
scoring, the scheduler, the MCP stdio command parser, and web-search HTML
parsing. One test asserts that unattended runs refuse rather than block on a
prompt — its failure mode is hanging, so it asserts under a timeout.

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
  the specific skill in **Settings → Skills** (web search / code execution / image
  generation default to off).
- **It doesn't remember anything between chats.** Recall needs the embedding
  engine; install it from **Settings → Recall** (or Engine → Recall in Everything
  mode). Until then it falls back to keyword matching.
- **A scheduled task never runs.** Tasks only run while the app is open — there is
  no background service. Check the task is enabled, and that another task isn't
  already running (only one runs at a time).
- **Can't create an image / no ◲ model in the dropdown.** Install the engine in
  **Engine → Image** and download a model in **Models → Image** first; the composer
  shows a hint until both are present.
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

Core phases 0–9, the **durable self** and its maintenance loop (phases 10–11),
and **recall, retrieval and tasks** (phase 12) are implemented. The backend
test suite and frontend type-check pass clean.

**Verified live:** local chat and local image generation (CUDA on a GTX 1060
6 GB — Stable Diffusion 1.5 at 512×512), and the folder / recall / memory
surfaces.

**Built but not yet exercised end-to-end:** most of phase 12's exit criteria
are runtime checks against real models — leaving a nightly task running
overnight, teaching a lesson in one chat and seeing it applied in another —
and haven't been run. The first manual pass over the scheduler found five
defects in code that compiled and passed its tests, one of which hung it until
restart. Compile-clean and test-green is not the same as working.

**Deferred to the next phase:** seeing images and scanned documents inside a
folder (vision captioning, OCR, table extraction) — specified in full in
`plans/PERCEPTION_PLAN.md` Part IV, not built.
