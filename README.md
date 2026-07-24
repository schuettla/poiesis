# Poiesis

*(formerly Project Nexus — renamed 2026-07; internal identifiers still say
`nexus` by design, see `docs/POIESIS_PLAN.md`.)*

A **local-first, agentic desktop LLM application** for Windows. Chat with local
models that run entirely on your machine, give the assistant real capabilities
(files, web search, code execution, image generation, external apps), and
optionally bring your own cloud API keys — all in a calm, editorial "Paper /
Slate" interface.

**Poiesis** takes its name from *autopoiesis* (Maturana & Varela): a system
that continuously produces and repairs the components that constitute it.
Poiesis doesn't just use its memory, instructions, and procedures — it
maintains them: it observes its own mistakes, distills lessons, repairs
degraded parts, and evolves its own way of working, with you as the boundary
that decides what may change. See `docs/POIESIS_PLAN.md` for the full concept
and implementation plan.

## What Poiesis remembers

*(filled in once Part III (10C) of `docs/POIESIS_PLAN.md` ships — durable
memory facts, lessons learned from mistakes, standing instructions, and
recipes, all stored as plain markdown files on your device.)*

- **Shell:** Tauri v2 (Rust backend + system WebView)
- **Frontend:** React 18 + TypeScript + Vite + Zustand
- **Local engine:** externally-managed, prebuilt `llama-server` (llama.cpp) over loopback HTTP
- **Image engine:** prebuilt `stable-diffusion.cpp` (downloaded on demand)
- **Storage:** embedded SQLite (with FTS5 full-text search)
- **Secrets:** OS credential store (Windows Credential Manager) — never in files or the database

See [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) for architecture,
[docs/TASKS.md](docs/TASKS.md) for the full task checklist, and
[Project_Nexus_PRD.md](Project_Nexus_PRD.md) for the product spec.

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
3. **To create images:** install the image engine in **Engine → Image**, download a
   diffusion model in **Models → Image**, then use the **◲** button in the chat
   composer (see [Image generation](#image-generation-engine--image--models--image)).
4. **Settings** → turn on individual skills, add cloud keys, create personas, and set
   reading size.

### Prerequisites

| Requirement | Notes |
|---|---|
| **Rust** (via [rustup](https://rustup.rs)) | Backend language |
| **MSVC C++ Build Tools** | The Rust linker on Windows (Visual Studio Build Tools → "Desktop development with C++") |
| **Node.js 18+** and npm | Frontend toolchain |
| **WebView2 runtime** | Ships with Windows 11; the installer bootstraps it on Windows 10 |

**No CUDA toolkit needed** — Nexus downloads prebuilt engine binaries rather than
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
- **Marketplace:** curated recommendations + Hugging Face / GitHub discovery,
  fit badges, quant slider, one-click download, local library management.

### Chat & persistence
- Streaming responses with a **Stop** control and an inline **agent-run timeline**.
- SQLite-backed history with **FTS5 full-text search** in the sidebar.
- Markdown + code rendering; adjustable reading size; light/dark ("Paper / Slate").

### Skills (agentic capabilities)
Toggle the **⚒ tools** button in a chat to let the assistant act. Each skill is
independently switchable in **Settings → Skills**:

| Skill | Default | Notes |
|---|---|---|
| **File access** | on | Read/write in folders you allow; every access asks first |
| **Artifacts** | on | Renders HTML / SVG / markdown / code in the **Canvas** side panel |
| **Web search** | off | No-key DuckDuckGo query issued from your machine (your query leaves the device) |
| **Code execution** | off | Runs Python / Node in a confined, time- and memory-limited sandbox |
| **Image generation** | off | Optional chat tool. The main way to make images is the composer **◲** mode (see below), which doesn't depend on this toggle |

### Personas (Settings → Personas)
Saved profiles bundling a **system prompt** + optional **pinned model** +
**temperature**. Pick one per chat from the composer dropdown; a one-off
temperature override is also supported per conversation.

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
  that inherently requires the network (web search, cloud models, image-engine
  download). Those are clearly labeled.
- **Secrets** (cloud keys, MCP tokens) are stored in the **OS credential store** —
  never in SQLite or plaintext.
- The local engine binds **127.0.0.1 only** with a per-session random token.
- File access is **deny-by-default**: you whitelist folders per-chat or permanently,
  Read-Only or Read-Write, with path-traversal + symlink-escape protection. Every
  file/tool action is written to a visible **Activity log**.
- Telemetry is **off by default**, content-free, and stays on your PC.

---

## Project layout

```
nexus/
├── src/                      # React + TypeScript frontend
│   ├── routes/               # Chat, Models, Apps, Settings, Engine
│   ├── components/           # Composer, Conversation, Canvas, Personas, ModelPicker, SidePanel
│   ├── lib/                  # api.ts (Tauri invoke wrappers), store.ts (Zustand), types.ts
│   └── styles/tokens.css     # "Paper / Slate" design tokens
├── src-tauri/                # Rust backend
│   └── src/
│       ├── runtime/          # hardware, manifest, download, process, proxy, jobobject, imageengine
│       ├── agent/            # loop + skills (filesystem, websearch, codeexec, artifacts, imagegen), sandbox
│       ├── mcp/              # MCP client (HTTP + stdio transports)
│       ├── cloud/            # BYOK providers (OpenAI/OpenRouter/Anthropic)
│       ├── db/               # SQLite schema + access (FTS5)
│       ├── commands/         # Tauri command surface (the IPC API)
│       ├── permissions/      # file-access grants + guards
│       └── secrets.rs        # OS credential store
├── docs/                     # IMPLEMENTATION_PLAN.md, TASKS.md
└── Project_Nexus_PRD.md      # product spec
```

Where local state lives at runtime (under the app-data directory):
`nexus.db` (SQLite), `runtimes/` (engines), `models/` (weights),
`generated-images/` (image outputs).

---

## Tests

```sh
cd src-tauri && cargo test
```

Covers hardware/runtime selection against **real** llama.cpp and
stable-diffusion.cpp release asset names, SQLite + FTS5 search, permission
path-traversal guards, the MCP stdio command parser, and web-search HTML parsing.

Frontend type safety:

```sh
npx tsc --noEmit
```

---

## Troubleshooting

- **"No model is loaded yet."** Download/select a local model in **Models**, or
  pick a cloud model (with a key set). The engine starts on demand.
- **Backend won't compile / linker error.** Install the MSVC C++ Build Tools
  ("Desktop development with C++") so Rust has a linker.
- **Tools do nothing in chat.** Turn on the **⚒** toggle in the composer, and enable
  the specific skill in **Settings → Skills** (web search / code execution / image
  generation default to off).
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

All 8 core phases plus the Phase-9 v2 capabilities (skills framework, personas, web
search, code execution, artifacts/Canvas, chat-integrated local image generation,
MCP stdio + config import/export) are implemented; the backend test suite and
frontend type-check pass clean. Local chat **and** local image generation are
verified live (CUDA on a GTX 1060 6 GB — Stable Diffusion 1.5 at 512×512); the
remaining newer capabilities are compile-/test-verified and benefit from a live
smoke-test.
```
