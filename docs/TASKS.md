> Project renamed to Poiesis (2026-07); internal identifiers still say nexus
> by design. Phase 10 entries below are superseded by `docs/POIESIS_PLAN.md`
> and should eventually be regenerated from it.

# Project Nexus — Task List

Granular, checkable tasks per phase. IDs in parentheses trace to PRD requirements.
Status: `[ ]` todo · `[~]` partial · `[x]` done.

> **Snapshot:** All **8 core phases + Phase 9 (v2) implemented**; backend + frontend
> compile clean (`tsc --noEmit` + `cargo build`, no warnings). Local path **verified
> live** (CUDA engine → streamed chat → tools on a GTX 1060); cloud path **verified
> live via OpenRouter**; **local image generation verified live** (auto-download →
> SD-1.5 → inline chat image on a GTX 1060 6 GB). Only one v1 task is intentionally
> left open (PDF page-image/OCR — needs a heavy external binary). **git not
> initialized yet** (owner will do baseline commit).
>
> | Phase | State |
> |---|---|
> | 0 Scaffold + shell | ✅ done (git not initialized) |
> | 1 Runtime loop | ✅ **verified live** — CUDA stream on a GTX 1060; Job-Object orphan guard added |
> | 2 Chat + persistence | ✅ done — rail FTS search surfaced |
> | 3 Marketplace + library | ✅ done — tok/s estimate, quant slider, library mgmt, URL/GitHub add, first-run |
> | 4 Tools + permissions | ✅ done — tools opt-in; native + content-JSON fallback (TOOL-2) |
> | 5 Multimodal | ✅ image (picker + drag/drop + paste) + PDF text + persisted; **OCR/page-image open** |
> | 6 MCP connectors | ✅ done — Streamable HTTP client, dashboard, keyring tokens |
> | 7 BYOK cloud | ✅ done — **verified live via OpenRouter**; OpenAI/OpenRouter/Anthropic, keyring keys |
> | 8 Polish + installer | ✅ done — reading size (Smaller→Larger), telemetry, licenses, WebView2, focus ring, WCAG, icon rail |
> | 9 v2 capabilities | ✅ done — skills framework, personas, web search, code exec, Canvas, **chat-integrated image gen (verified live)**, MCP stdio + import/export |
> | 10 Agent memory & context | 🔲 planned — see `docs/AGENT_MEMORY_PLAN.md` (compaction, recall, durable memory, soul, grammar) |
>
> **Engine view** (LM Studio-style runtime management): status/start-stop, backend
> override (CUDA/Vulkan/CPU), per-backend install detection, update check.
> **Live-run fixes:** manifest asset-name matching (real `b4585` names), `--jinja`
> for tool calls, engine-readiness gating + status pill.
>
> **Open (v1):** PDF page-image/OCR (needs pdfium/tesseract binary); relaunch +
> live smoke-test of the latest batch; OpenAI-/Anthropic-direct unverified (no key).

---

## Phase 0 — Scaffold & toolchain
- [x] Install Tauri CLI as dev dependency (`@tauri-apps/cli`)
- [x] Scaffold Tauri v2 + React + TS (Vite) project structure
- [x] Verify toolchain healthy (Rust/cargo/node confirmed; app builds + launches)
- [x] Confirm empty app builds and window opens (`npx tauri dev`)
- [x] Add design tokens `src/styles/tokens.css` (Paper/Slate palette, type scale from mockup)
- [x] Bundle fonts locally (Newsreader, Inter, JetBrains Mono) for offline use (via `@fontsource`)
- [x] Set up Zustand stores + typed `lib/api.ts` invoke wrappers
- [x] App shell layout: topbar / rail / main grid matching mockup
- [x] Light/dark (Paper/Slate) mode switch wired to `data-mode`
- [~] `.gitignore` added; **git repo not initialized / no baseline commit yet**

## Phase 1 — Core runtime loop (riskiest first)
- [x] Hardware detection: GPU vendor/model, VRAM, RAM, CPU ISA (AVX2/AVX-512) (MKT-4) — verified on host
- [x] Runtime manifest: hardware → llama.cpp asset (CUDA12/13, Vulkan, HIP, SYCL, CPU) (7.3.2)
- [x] Download subsystem: streamed, resumable, SHA-256 verify, unpack to app-data (7.3.3)
- [x] Download CUDA runtime DLL package when NVIDIA path selected
- [x] Spawn `llama-server` child process with model + ctx + GPU-offload args (7.4)
- [x] Dynamic free loopback port selection; record it
- [x] Per-session random auth token on the engine
- [x] Health-check poll readiness gating before sending requests
- [x] Streaming HTTP proxy: engine → Rust → WebView via Tauri channels
- [x] Stop/cancel control wired through proxy (CHT-2)
- [x] Lifecycle safety: kill on exit (Drop + ExitRequested) **plus Win32 Job-Object kill-on-close** for ungraceful parent death (`jobobject.rs`)
- [x] Manual backend override — Engine view "Acceleration" picker + `set_backend_override_cmd`, per-backend install dirs
- [x] Demo: single streamed completion end-to-end — **verified live** (Llama 3.2 3B on CUDA, ~39 tok/s on a GTX 1060)

## Phase 2 — Chat surface + persistence
- [x] SQLite setup: schema, migrations runner, schema-version (7.1.1)
- [x] Schema: conversations, messages, attachments(refs), settings, model_library, connectors, permissions, activity_log
- [x] FTS5 virtual table + triggers over message content (CHT-3) — tested
- [x] Commands: create/list/rename/delete conversation; append/finalize/list messages; search
- [x] Chat UI: user turn (sans) + agent run (timeline + serif prose) per mockup (CHT-1, CHT-9)
- [x] Agent-run timeline component: steps with verb/target/result, running/done dots (CHT-9)
- [x] Streaming cursor on in-progress prose; reduced-motion static fallback
- [x] Markdown rendering (gfm) + code blocks (mono) + inline code
- [x] Rail: New chat, history grouped by recency, active state, **FTS search box** (debounced, content + title)
- [x] Composer: input, attach (+), send/stop toggle, keyboard submit
- [x] Single global system prompt: edit in Settings, applied to turns (CHT-4)
- [x] Persist + restore conversations across restart

## Phase 3 — Marketplace + library
- [x] Hugging Face API client: search + per-repo GGUF files with sizes/quant (MKT-1, MKT-2)
- [x] GitHub Releases client: list GGUF release assets (MKT-1)
- [x] Normalize HF + GitHub + curated into one unified catalog model (MKT-1)
- [x] Hardware fit classifier: "Runs great / slowly / Won't fit" from detection (MKT-4)
- [x] Estimated tokens/sec band per model/quant (coarse fit+size heuristic, labelled approximate) (MKT-2)
- [x] Models surface: app-store grid, fit badge, size, **quant quality·speed·size slider** on discovered repos (5.4.2)
- [x] One-click download wired to Phase-1 download subsystem, with progress (MKT-3)
- [x] Local Library: list + size + "Use" + **Set-default / Delete buttons + total disk-usage roll-up** (MKT-5)
- [x] Add model by HF repo id, **GitHub `owner/repo`, or direct `.gguf` URL** (auto-detected) (MKT-6)
- [x] Curated "recommended" overlay for consumer persona (D-5)
- [x] First-run flow: empty library → recommend best-fit model → one-click download → into chat (5.4.1)

## Phase 4 — Tool calling + File System skill + permissions
- [x] Tool-calling orchestrator: detect tool request, execute, feed result back, loop (TOOL-1)
- [x] Native structured tool-call parsing (streamed `tool_calls` accumulation) (TOOL-2)
- [x] Content-JSON tool-call fallback (handles engines that stream tool calls as text, e.g. `b4585`) (TOOL-2)
- [x] Unified tool dispatch table — built-in skills **and MCP connectors** share one table (7.5)
- [x] Tools are opt-in per chat (composer ⚒ toggle) so small models don't over-call on plain chat
- [x] File System skill: read / list / write within whitelisted dirs (TOOL-3)
- [x] Whitelist model: per-directory Read-Only / Read-Write (6.1)
- [x] Path-traversal + symlink-escape guards (6.1) — unit-tested
- [x] Permission side-panel: Allow once / for this chat / always / Deny, plain-language scope (5.4.4)
- [x] Activity log: every file op recorded; shown in Settings (6.1, 6.3)
- [x] Granted permissions listed + revocable in Settings (5.4.4)

## Phase 5 — Multimodal
- [x] Image input: file-picker **+ drag-and-drop + clipboard paste**; attachments **persisted to DB** + restored on reload (CHT-5)
- [x] Route image to vision-capable model; vision capability flag respected (CHT-5)
- [x] Clear UX when active model lacks vision (CHT-5)
- [x] PDF ingestion: text extraction for text-based PDFs (CHT-8)
- [ ] PDF page-image extraction for vision models; scanned-PDF/OCR path (CHT-8) — **the only open v1 task**; needs a bundled pdfium/tesseract binary (recommend v1.x)
- [x] Inject extracted content into context

## Phase 6 — MCP client + connectors
- [x] MCP client: connect remote MCP server from pasted URL — Streamable HTTP transport (MCP-1)
- [x] Capability discovery (`initialize`/`tools/list`) + tool registration into unified dispatch (MCP-1, MCP-4)
- [x] Connector Dashboard ("Apps"): list, status/test, tools, enable/disable, token entry (MCP-3)
- [x] Inject MCP tool results into context (`tools/call`); surfaced as timeline steps + activity log (MCP-4, 5.4.3)
- [x] Plain-language connect flow + Advanced setup (token) section (5.4.3)
- [x] Auth tokens stored in OS credential store (keyring), never SQLite (NFR security)
- [~] stdio MCP transport deferred to v1.x (MCP-2); OAuth flow is token-paste for v1

## Phase 7 — BYOK cloud providers
- [x] Keyring-backed provider keys in OS credential store (CLD-2) — reuses the Phase-6 secrets module
- [x] OpenAI-compatible adapter (llama.cpp + OpenRouter + OpenAI) — reuses the streaming proxy (7.6)
- [x] Anthropic Messages API adapter — translates OpenAI-shaped history + tools, parses SSE (7.6)
- [x] `ChatEndpoint` + `drive_turn` routing layer (local vs cloud by selected model) (7.6)
- [x] Key-entry per provider in Settings with reassuring explainer + console links (5.4.5)
- [x] Per-provider model discovery — OpenRouter catalog, OpenAI `/v1/models`, curated Anthropic (CLD-4)
- [x] Model picker: grouped "On this device" / "Cloud · your key", provenance dots, **cloud group populated** (CLD-3)
- [x] Local-only filter chip hides cloud group (CLD-7)
- [x] Run header states model + provenance dot once per turn (CLD-6)
- [x] Cloud tool-calling/MCP/skills parity — same agent loop + unified dispatch on either endpoint (CLD-5)
- [x] **Verified live** via an OpenRouter key (streamed cloud chat working); picker filters long catalogs (~300) via search
- [~] OpenAI-direct + Anthropic-direct adapters compile but not yet exercised against their own keys

## Phase 8 — Polish & release
- [x] WCAG AA contrast — audited text tiers; `--ink` 16.5:1, `--ink-muted` 4.73/5.75:1 (AA body), `--ink-faint` darkened to 3.0–3.4:1 (AA large/UI) (5.5)
- [x] Provenance never color-alone (name + group label always paired) (5.5)
- [x] Keyboard operability + visible `:focus-visible` ring across interactive elements (5.5)
- [x] Reduced-motion: static timeline dot + cursor (5.5)
- [x] Adjustable reading-size base in Settings (Standard/Large/Larger), reflow not truncate (5.5)
- [x] Screen-reader labels; timeline step announced as one sentence (5.5)
- [x] Responsive: rail narrows at small width **+ manual icon-only collapse** (`«`/`»` toggle, nav icons) (5.5)
- [x] Settings surface: system prompt + file access + activity + cloud keys + reading size + telemetry
- [x] WebView2 Evergreen bootstrap in installer (`embedBootstrapper`) (7.1.2)
- [x] License attribution section (llama.cpp MIT, Tauri, React, fonts, per-model) (NFR licensing)
- [x] Opt-in, content-free telemetry plumbing — off by default, counts only, local (6.3)

## Phase 9 — v2 capabilities

> **Scope:** the deferred v1.x/v2 backlog, re-grouped into dependency-ordered
> workstreams. Build order: **foundations (9A, 9B) → skills (9C, 9D) →
> surfaces (9E, 9F) → connectors (9G)**. Foundations first because each is a
> *generalization of code that already ships* (one skill → framework; one
> system prompt → personas), so they unlock everything downstream at low risk.
> macOS/Linux ports remain out of scope. Local **video** generation is out of
> scope (too heavy for consumer GPUs) — video is connector-only.

### 9A — Skills framework (TOOL-6) — *foundation* ✅
- [x] `Skill` abstraction (`id`/`tool_specs`/`handles`/`describe`/`execute` + `SkillContext`) generalizing the bespoke filesystem skill (`agent/skills.rs`) — enum over the fixed set (dyn-async-free), which achieves the generalization
- [x] Re-home `agent/filesystem.rs` as the first `Skill` (no behavior change)
- [x] `ToolRegistry::build` iterates **enabled** skills + connector tools (built-ins win collisions) (7.5)
- [x] Per-skill enable/disable persisted in `settings`; `list_skills_cmd`/`set_skill_enabled_cmd`; **Settings toggles** with off-device/runs-code flag
- [x] Web Search + Code Exec slot in as further variants (design proven against three real cases)

### 9B — Personas + per-conversation overrides (CHT-4 Later, CHT-7) — *foundation*
- [x] `personas` table: name, system_prompt, model_id?, params_json; schema v2 migration
- [x] `persona_id` + inline `overrides_json` columns on `conversations` (idempotent ALTER migration)
- [x] Persona CRUD + `set_conversation_persona` db API + commands (list/create/update/delete/set-default/attach)
- [x] Turn builder resolves prompt + temperature as **conversation override → persona → global default** (store `sendMessage`, CHT-4/CHT-7)
- [x] Persona editor in Settings (CRUD + set-default) + per-chat persona picker in the composer; persona can pin a model

### 9C — Web Search skill (TOOL-4) — *no third-party account* ✅
- [x] Provider: **no-key direct fetch** (DuckDuckGo HTML) from the Rust backend — no Nexus server, no provider key (`agent/websearch.rs`)
- [x] Dependency-free result parse (title/url/snippet, redirect-unwrap) + lead-page readability extraction; source attribution; unit-tested
- [x] "Query leaves your device" disclosure (skill opt-in + Settings flag) honoring the privacy posture (6.3)
- [x] Activity-log every search (6.1)
- [ ] (Deferred BYOK Brave/Tavily adapter — only if richer results needed)

### 9D — Code Execution skill + sandbox (TOOL-5) ✅
- [x] Sandbox: **dedicated Job-Object-confined subprocess** (`agent/sandbox.rs`) — kill-on-close, memory cap, active-process limit, scrubbed env, scratch temp dir
- [x] Execute Python / Node; capture stdout/stderr/exit; hard 10s wall-clock timeout + output-size cap
- [x] Tool surface as a `Skill` (9A); opt-in per chat; every run activity-logged (6.1)
- [ ] Network isolation (AppContainer) — **follow-up hardening**; current sandbox limits time/memory/process/fs but does not yet block outbound network
- [ ] Execution output flows into the Artifacts panel (9E) — *pending 9E*

### 9E — Artifacts / Canvas panel (CHT-6) ✅
- [x] `create_artifact` tool as a benign built-in skill (default on); short receipt to the model, full content to the UI (`agent/artifacts.rs`)
- [x] Canvas side panel renders code / markdown / SVG / HTML — HTML/SVG in a **sandboxed iframe** (`sandbox="allow-scripts"`, no same-origin), never the app WebView origin
- [x] `artifacts` table + `AgentEvent::Artifact` stream + `list_artifacts_cmd`; restored on conversation load; multi-artifact switcher + reopen affordance
- [ ] Wire Code Execution (9D) output + generated images (9F) into artifacts — *follow-up*

### 9F — Local image generation (+ video via connector) ✅ **verified live**
- [x] `generate` runs a **`stable-diffusion.cpp` CLI** as a confined subprocess (one-shot, matches how sd.cpp actually runs); 5-min timeout, `kill_on_drop` (`agent/imagegen.rs`)
- [x] Text→image with prompt/negative/size/steps; output PNG persisted under `generated-images/`
- [x] **Chat-integrated creation** (the primary path): composer **◲** *Create-image* mode + model dropdown → `generate_image_cmd` runs the engine **directly** (not routed through the chat LLM, which small local models don't call reliably) → image rendered **inline in the conversation** (`ChatImage.tsx`, `AgentRun.tsx`) so the user can iterate
- [x] **Structured like local chat:** engine management in **Engine → Image** tab (`ImageEngine.tsx`, `install_image_engine_cmd`), model library in **Models → Image** tab (`ImageModels.tsx`, catalog + add-by-URL + set-default/delete) — mirrors the llama.cpp engine/model split; removed from Settings
- [x] **Auto-download**, hardware-matched: sd.cpp backend zip (cuda12/vulkan/cpu/rocm) + CUDA-runtime package from a pinned GitHub release, reusing the Phase-1 runtime downloader; asset keyword matcher **unit-tested against real release asset names** (`runtime/imageengine.rs`)
- [x] **Resumable model downloads:** stream to a `.part` sidecar, resume via HTTP `Range`, atomic rename on completion; short-read is now a hard `Truncated` error (not a silent partial), and a stored path whose file vanished is self-healed to "unset" (`runtime/download.rs`, `commands/imagegen.rs`)
- [x] **Fits consumer GPUs:** text-encoder + VAE offloaded to CPU (`--backend te=cpu,vae=cpu`), tiled VAE decode, CUDA-gated flash attention — SD 1.5 @ 512×512 within ~6 GB VRAM; **verified live** (dancing frog on a GTX 1060 6 GB)
- [x] Binary + model paths still overridable under **Advanced** in each tab
- [x] **Video:** connector-only (faktry/fal-style MCP) — no local engine in v2

> **Phase 9 status:** 9A–9G fully implemented (backend + UI, 12 backend tests + `tsc` green).
> Local image generation is now **verified live** end-to-end (engine auto-download → SD-1.5
> model download → inline chat image on a GTX 1060). Remaining follow-up: code-exec network
> isolation via AppContainer. If a catalog model URL ever moves, the Models → Image tab
> surfaces a clear error and the Add-by-URL / Advanced manual picker remains.

### 9G — MCP: stdio transport + config import/export (MCP-2, MCP-5) ✅
- [x] `Transport` abstraction in `mcp/client.rs`: `HttpTransport` / `StdioTransport` (shared JSON-RPC framing)
- [x] stdio transport: spawn a local MCP server (quoted-arg command line), newline-delimited JSON-RPC over stdin/stdout, `kill_on_drop` lifecycle; quoted-path split unit-tested (MCP-2)
- [x] Connector export to JSON **minus secrets**; import skips duplicates + re-prompts for tokens (`export/import_connectors_cmd`) (MCP-5)
- [x] Surfaced in Apps: Remote-link / Local-command toggle + Export/Import config panel

---

## Phase 10 — Agent memory & context

> **Design doc: `docs/AGENT_MEMORY_PLAN.md`** (file-level spec per task — read the
> matching section before implementing). Build order: 10A → 10B → 10C → 10D;
> 10E/10F independent. One schema migration (v5) covers the whole phase.

### 10A — Context ledger & compaction (CTX) — *foundation*
- [ ] Schema v5 migration: `conversations.summary` + `summary_upto_message_id`; `soul_proposals`, `tool_stats`, `memory_fts` tables in `schema.sql` (CTX/SOUL/LOOP prereq)
- [ ] `ctx_size` exposed: `RunningEngine` → `EngineStatus` → `get_context_budget_cmd` + `api.getContextBudget()` (CTX-1)
- [ ] `src/lib/context.ts`: `estimateTokens` + `budgetTurns` (keep system + last 6 + current; 75% threshold), tested (CTX-2)
- [ ] `compact_conversation_cmd`: summarize-oldest via same endpoint (FACTS/DECISIONS/OPEN prompt), persist summary + boundary (CTX-3)
- [ ] Both send paths use one shared `assembleTurns` helper: budget → compact → `withSummary` system section → post-boundary turns only (CTX-4)
- [ ] Workspace mode: `KEEP_RECENT = 3` + "surface is authoritative" summary line (CTX-4)
- [ ] Composer context meter (hidden <50%, tooltip, `aria-label`) (CTX-UI-1)
- [ ] Compaction divider in transcript; click shows the stored summary; no messages hidden (CTX-UI-2)
- [ ] Same meter in Workspace `ws-head` (CTX-UI-3)
- [ ] Settings "Memory & context" card: window size readout + `context.autocompact` toggle (CTX-UI-4)

### 10B — Recall skill (RCL)
- [ ] `SearchHit` + `search_messages_fts` (snippet, rank, `fts_escape`) + `search_memory_fts`; unit-tested (RCL-1)
- [ ] `Skill::Recall` (`agent/recall.rs`, default on): `search_history` + `read_conversation`, capped outputs, activity-logged (RCL-2)
- [ ] `SkillContext.call_id` plumbed; `AgentEvent::Recall { id, matches }` emitted (RCL-2)
- [ ] Timeline: recall steps expand to provenance rows; chat rows open the conversation; `◆ memory` chip on fact hits (RCL-UI)

### 10C — Durable memory store (MEM)
- [ ] `MemoryStore` (`src-tauri/src/memory/mod.rs`): fact CRUD, slug rules, frontmatter parse, derived `MEMORY.md` (2 KB cap), trash, snapshots, `memory_fts` sync; unit-tested (MEM-1)
- [ ] `Skill::Memory`: one `memory(op: save|update|forget|read)` tool, flat params, conservative write policy, receipts; `SkillContext.memory` plumbed (MEM-2)
- [ ] `AgentEvent::MemoryWrite` + activity log on every op (MEM-6)
- [ ] `get_memory_context_cmd`; `composeSystemPrompt` injects SOUL + memory index (+ tools-off caveat) (MEM-3)
- [ ] `consolidate_memory_cmd` → strict-JSON proposal stored in `memory.pending_consolidation`; `apply_consolidation_cmd` snapshots then applies (MEM-5)
- [ ] `MemoryPanel` in Settings: fact cards (edit/delete/undo/source link), search, Tidy up review, Open folder, Export zip (MEM-UI-1/2/5)
- [ ] Memory toast with Undo + first-write onboarding line (`memory.onboarded`) (MEM-UI-3/4)
- [ ] Skill toggle disables injection **and** tool; `◆` durable marker on SessionStrip entries (MEM-UI-6/7)

### 10D — Soul: evolvable standing instructions (SOUL)
- [ ] `SOUL.md` via MemoryStore; injected after base prompt, 1.5 KB cap (SOUL-1)
- [ ] `propose_soul_edit` tool → `soul_proposals` row + `AgentEvent::SoulProposal`; never auto-applies (SOUL-2)
- [ ] `list_soul_proposals_cmd` / `resolve_soul_proposal_cmd` (accept ⇒ `set_soul`) (SOUL-3)
- [ ] PersonaEditor "Soul" section: textarea + proposal old/new cards, Accept/Dismiss (SOUL-UI-1)
- [ ] Inline proposal card under the assistant turn (PermissionPanel tone, not a modal) (SOUL-UI-2)
- [ ] Settings rail badge dot when proposals/consolidation pending (SOUL-UI-3)

### 10E — Grammar-constrained decoding (GRM) — *independent*
- [ ] Validate + retry: one guided retry per failed built-in call (system nudge, retried-ids set) (GRM-3)
- [ ] Probe native lazy-grammar tool enforcement on the pinned llama.cpp build; bump pin if needed; record result on `EngineStatus` (GRM-1/2)
- [ ] `tool_stats` recording in `dispatch_calls` (model name plumbed into `run_agent`) (GRM-4/LOOP-5)
- [ ] Engine card line: "Structured tool output: enforced ✓ / validate + retry" (GRM-UI-1)

### 10F — Loop hygiene (LOOP)
- [ ] Per-run MCP client pool (initialize once per connector per run) (LOOP-1)
- [ ] `fetch_url` tool on Web Search skill (8 KB cap, off-device disclosure, activity-logged) (LOOP-2)
- [ ] Plan-first line in tools-mode guidance (LOOP-3)
- [ ] Early-flush streaming in tools mode (buffer-biased heuristic) (LOOP-4)
- [ ] Settings → Skills reliability captions from `tool_stats` (LOOP-UI-1)

---

## Deferred (post-v2 / out of scope)
- [ ] macOS / Linux ports (explicitly excluded for now)
- [ ] Local video generation (too heavy for consumer GPUs — connector-only)
- [ ] Self-hosted SearXNG web-search backend (dropped — no-key direct fetch only)
- [ ] BYOK web-search providers (Brave / Tavily) — only if no-key proves insufficient
