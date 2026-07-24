# Project Nexus — Implementation Plan

A local-first, agentic desktop LLM app for Windows. Tauri (Rust) shell + React/TypeScript
frontend + external `llama-server` runtime + SQLite. This document maps the PRD (v4.1) and the
UI mockup onto a concrete architecture, a phased build order, and milestones.

> Source of truth: `Project_Nexus_PRD.md` (requirements + IDs) and `Project_Nexus_UI_Mockup.html`
> (visual direction). Granular checklist lives in `docs/TASKS.md`.

---

## 1. Architecture Overview

```
┌────────────────────────────────────────────────────────────────────┐
│  React + TypeScript (WebView)                                        │
│  Chat · Marketplace · Connectors · Settings · Permissions UI         │
│  Paper/Slate design system, agent-run timeline, model picker         │
└───────────────▲───────────────────────────────┬────────────────────┘
                │ Tauri events (token stream)    │ Tauri commands (invoke)
┌───────────────┴───────────────────────────────▼────────────────────┐
│  Rust backend (Tauri core)                                          │
│                                                                     │
│  ┌────────────┐  ┌─────────────┐  ┌──────────────┐  ┌────────────┐  │
│  │ Routing    │→ │ Agent loop  │→ │ Tool dispatch│→ │ MCP client │  │
│  │ local/cloud│  │ (tool calls)│  │ fs/web/code  │  │ HTTP/SSE   │  │
│  └─────┬──────┘  └─────────────┘  └──────┬───────┘  └────────────┘  │
│        │                                  │                          │
│  ┌─────▼───────┐  ┌──────────────┐  ┌─────▼──────┐  ┌─────────────┐  │
│  │ Local       │  │ Cloud        │  │ Permissions│  │ SQLite +    │  │
│  │ provider    │  │ providers    │  │ + activity │  │ FTS5        │  │
│  │ (llama.cpp) │  │ (BYOK)       │  │ log        │  │             │  │
│  └─────┬───────┘  └──────┬───────┘  └────────────┘  └─────────────┘  │
│        │                 │                                            │
│  ┌─────▼───────────┐  ┌──▼────────────────┐  ┌────────────────────┐  │
│  │ Runtime mgr     │  │ OS credential     │  │ Hardware detection │  │
│  │ download/spawn  │  │ store (keyring)   │  │ GPU/VRAM/CPU ISA   │  │
│  └─────┬───────────┘  └───────────────────┘  └────────────────────┘  │
└────────┼─────────────────────────────────────────────────────────────┘
         │ loopback HTTP (127.0.0.1:dynamic-port, per-session token)
┌────────▼────────────┐
│  llama-server.exe   │  external child process, hardware-matched binary
│  (GGUF model)       │
└─────────────────────┘
```

### Key architectural commitments (from PRD §7)
- **External runtime over HTTP (Option B).** Never compile/embed llama.cpp. Download the
  prebuilt `llama-server` binary matched to hardware, spawn as a child process, talk over a
  dynamic loopback port guarded by a per-session token.
- **Single internal provider interface.** Local (llama.cpp) and cloud (OpenRouter/OpenAI/
  Anthropic) sit behind one trait. OpenAI-compatible adapter covers llama.cpp + OpenRouter +
  OpenAI; a dedicated adapter handles Anthropic Messages API.
- **SQLite for all local state**, FTS5 for conversation search. Attachment binaries on disk,
  paths in DB.
- **Provenance lives in the model picker**, not per-message decoration. `Local only` is a
  picker filter, not a global mode.
- **API keys in OS credential store** (Windows Credential Manager via `keyring`), never SQLite.

---

## 2. Tech Stack & Key Crates/Packages

### Frontend
- React 18 + TypeScript + Vite
- `@tauri-apps/api` (invoke, event), `@tauri-apps/plugin-*` as needed
- State: Zustand (lightweight, fits the calm/simple ethos)
- Routing: in-app view switching (no router lib needed for 4 surfaces) or `react-router`
- Markdown render: `react-markdown` + `remark-gfm`; code highlight: `shiki`
- Fonts: Newsreader, Inter, JetBrains Mono (bundled locally for offline, not CDN)

### Backend (Rust)
- `tauri` v2
- `tokio` (async runtime), `reqwest` (HTTP client, streaming), `eventsource-stream` (SSE)
- `rusqlite` + `r2d2_sqlite` or `sqlx` (SQLite + FTS5); choose `rusqlite` for simplicity/bundled
- `serde` / `serde_json`
- `keyring` (OS credential store)
- `sha2` (download verification), `sysinfo` + `wgpu`/`nvml-wrapper` or custom WMI for HW detect
- `pdf-extract` / `lopdf` + `pdfium-render` for PDF (text + page images)
- MCP: implement client over `reqwest` SSE (no official Rust MCP SDK dependency required)

---

## 3. Project Structure

```
nexus/
├─ docs/
│  ├─ IMPLEMENTATION_PLAN.md      (this file)
│  └─ TASKS.md
├─ index.html
├─ package.json
├─ vite.config.ts
├─ tsconfig.json
├─ src/                            # React frontend
│  ├─ main.tsx
│  ├─ App.tsx
│  ├─ styles/tokens.css           # Paper/Slate palette + type scale (from mockup)
│  ├─ styles/global.css
│  ├─ lib/
│  │  ├─ api.ts                    # typed wrappers over Tauri invoke
│  │  ├─ events.ts                 # token-stream event subscriptions
│  │  └─ store.ts                  # Zustand stores
│  ├─ components/
│  │  ├─ TopBar/                   # brand, ModelPicker, mode switch
│  │  ├─ ModelPicker/              # grouped local/cloud, Local-only filter
│  │  ├─ Rail/                     # new chat, history, Models/Apps/Settings
│  │  ├─ Conversation/
│  │  │  ├─ UserTurn.tsx
│  │  │  ├─ AgentRun.tsx           # run header + timeline + prose
│  │  │  ├─ Timeline.tsx           # step list, running/done dots
│  │  │  └─ RunText.tsx            # serif prose, streaming cursor, markdown
│  │  ├─ Composer/                 # input, attach, send/stop
│  │  └─ SidePanel/                # permissions prompt, activity log
│  └─ routes/
│     ├─ Chat.tsx
│     ├─ Models.tsx                # marketplace
│     ├─ Apps.tsx                  # connectors dashboard
│     └─ Settings.tsx
└─ src-tauri/                      # Rust backend
   ├─ Cargo.toml
   ├─ tauri.conf.json
   ├─ capabilities/
   └─ src/
      ├─ main.rs
      ├─ lib.rs
      ├─ commands/                 # Tauri command handlers (the IPC surface)
      ├─ runtime/                  # llama-server lifecycle
      │  ├─ hardware.rs            # GPU/VRAM/RAM/CPU ISA detection
      │  ├─ manifest.rs            # hardware→asset mapping
      │  ├─ download.rs            # streamed download, resume, SHA-256
      │  ├─ process.rs            # spawn, dynamic port, health gate, kill-on-exit
      │  └─ proxy.rs               # streaming HTTP proxy + cancellation
      ├─ providers/
      │  ├─ mod.rs                 # Provider trait
      │  ├─ router.rs              # local-vs-cloud routing
      │  ├─ openai_compat.rs       # llama.cpp + OpenRouter + OpenAI
      │  └─ anthropic.rs           # Anthropic Messages API
      ├─ agent/
      │  ├─ loop.rs                # tool-call detection + loop
      │  ├─ parse.rs               # native + ReAct/JSON fallback
      │  └─ tools/
      │     ├─ filesystem.rs
      │     ├─ web.rs
      │     └─ code.rs
      ├─ mcp/
      │  └─ client.rs              # HTTP/SSE MCP client + discovery
      ├─ marketplace/
      │  ├─ huggingface.rs
      │  └─ github.rs
      ├─ db/
      │  ├─ mod.rs                 # pool + migrations runner
      │  ├─ schema.sql
      │  └─ models.rs              # row structs + queries
      ├─ permissions/              # whitelist, path-traversal guard, activity log
      ├─ keystore/                 # keyring wrapper for BYOK keys
      └─ attachments/              # image + PDF ingestion
```

---

## 4. Phased Build Order

Maps to PRD §9. Each phase is independently demoable.

### Phase 0 — Scaffold & toolchain (foundation)
Tauri v2 + React + TS skeleton builds and runs. Design tokens + fonts wired. CI-free local
build verified. **Exit:** empty Nexus window opens.

### Phase 1 — Core runtime loop (riskiest first — PRD §12.3)
Hardware detection → resolve runtime manifest → download/verify `llama-server` → spawn on
dynamic loopback port → health-gate → proxy one streamed completion to the UI. **Exit:** type
a prompt, see tokens stream from a real local model.

### Phase 2 — Chat surface + persistence (CHT-1,2,3,9)
Full chat UI per mockup: rail history, agent-run timeline, serif prose, composer with Stop.
SQLite schema + migrations; conversations/messages persisted; FTS5 search; rename/delete.
Single global system prompt (CHT-4). **Exit:** multi-turn chats survive restart, searchable.

### Phase 3 — Marketplace + library (MKT-1..6)
HF API + GitHub releases normalized into one catalog; hardware fit badges; one-click download
with resume + SHA; local library with disk usage, delete, set default; add-by-URL. **Exit:**
browse → download → use a model without touching Phase-1 internals manually.

### Phase 4 — Tool calling + File System skill + permissions (TOOL-1,2,3; §6)
Orchestrator with native tool-calling + ReAct/JSON fallback (capability auto-detect). File
System skill scoped to whitelisted dirs with path-traversal/symlink guards. Side-panel consent
prompts (Allow once / for chat / Deny). Activity log. **Exit:** model reads/writes files with
visible timeline steps + consent.

### Phase 5 — Multimodal (CHT-5, CHT-8)
Image drag/paste → vision routing with clear no-vision UX. PDF ingestion: text extraction +
page images for vision models; scanned-PDF handling. **Exit:** drop an image and a PDF, get
grounded answers.

### Phase 6 — MCP client + connectors (MCP-1,3,4)
HTTP/SSE MCP client, capability discovery, tool registration into the shared dispatch table.
Connector dashboard with status/toggles/token entry. **Exit:** paste a remote MCP URL, its
tools appear as timeline steps.

### Phase 7 — BYOK cloud providers (CLD-1..7)
Keyring storage; OpenAI-compat + Anthropic adapters; routing layer; unified picker with
provenance dots + groups + Local-only filter; per-provider model discovery; run header
provenance. **Exit:** add a key, pick a cloud model, agent works identically.

### Phase 8 — Polish: accessibility, settings, installer
WCAG AA, full keyboard op, reduced-motion, adjustable reading size, settings surface,
WebView2 bootstrap in installer, license attribution. **Exit:** v1 quality floor met.

---

## 5. Cross-Cutting Concerns
- **Security:** least privilege; loopback-only engine + per-session token; no key in DB;
  path guards; opt-in content-free telemetry only.
- **Offline:** core chat + local-file tooling must work with no network; bundle fonts.
- **Accessibility (§5.5):** provenance never by color alone; visible focus ring; reduced-motion.
- **Performance:** shell ~150 MB RAM target; UI responsive during generation (streaming off
  the main thread / via events).
- **Licensing:** open-source; attribute llama.cpp (MIT) and bundled components.

---

## 6. Risks & Open Decisions (PRD §10)
- D-2 stdio MCP → defer to v1.x (remote HTTP/SSE only in v1).
- D-3 code execution → host-direct with strong consent UX; sandbox is fast-follow.
- D-4 engine version → pin a tested build; manual "update runtime".
- D-6 AMD backend → Vulkan default, HIP opt-in.
- WebView2 bootstrap in installer → recommended hard-require given Win10 baseline.
- **Biggest risk:** Phase 1 (process lifecycle + streaming + HW-matched binary selection).
  Built first on purpose.
```
