# Project Nexus

**A Local-First, Agentic Desktop LLM Application**

Product Requirements Document
Version 4.0 · Windows-First Release
Status: Draft for Engineering Review
June 2026

> **v4 changes:** Frontend fixed to React; conversation and app state stored in SQLite; marketplace sources both Hugging Face and direct GitHub releases; single system prompt for v1 (Personas deferred); minimum target Windows 10; v1 multimodal scope is image **and** PDF input. BYOK cloud providers (OpenRouter, Anthropic, OpenAI) promoted into v1 scope. Added a User Experience & Design chapter (Section 5) with a committed editorial "paper-and-ink" visual direction (light + dark).
>
> **v4.1 revision (post-mockup review):** the UI direction was reworked after reviewing an HTML mockup of the original design. The earlier "provenance margin" (per-message local/cloud color-coding) read as decorative and was replaced with the **agent run timeline** — tool calls, file reads, and edits now render inline as a visible, build-log-style sequence, which is what makes the product read as an agent rather than a chatbot (new requirement CHT-9, Section 5.2a). The global **Private Mode** toggle was replaced with a simpler model: provenance lives only in the model picker (a dot per model, grouped "On this device" vs "Cloud · your key"), with a one-click **Local only** filter chip rather than a separate mode to manage (CLD-7 updated accordingly). Section 7.6 (Model Routing) and Section 6 (Security & Permissions) were updated to match; a numbering error in Section 6's subsections (mislabeled 5.x) was also corrected to 6.x. A standalone HTML mockup implementing this direction is available alongside this document. Setup prerequisites for development were added as Section 12.

---

## Contents

1. [Executive Summary](#1-executive-summary)
2. [Goals & Non-Goals](#2-goals--non-goals)
3. [Target Audience & Personas](#3-target-audience--personas)
4. [Functional Requirements](#4-functional-requirements)
5. [User Experience & Design](#5-user-experience--design)
6. [Security & Permissions Model](#6-security--permissions-model)
7. [Technical Architecture](#7-technical-architecture)
8. [Non-Functional Requirements](#8-non-functional-requirements)
9. [Phased Release Plan](#9-phased-release-plan)
10. [Open Decisions](#10-open-decisions)
11. [Resolved Product Decisions](#11-resolved-product-decisions)
12. [Development Environment Setup](#12-development-environment-setup)

---

## 1. Executive Summary

Project Nexus is an open-source, local-first desktop AI assistant for Windows that brings state-of-the-art agentic capabilities — tool calling, Model Context Protocol (MCP) connectors, skills, and multimodal input — to open-weight models running entirely on the user's own hardware. It occupies the gap between developer-oriented model runners such as LM Studio, which emphasize raw model testing and inference metrics, and polished consumer assistants such as Claude or ChatGPT, which are cloud-bound and closed.

Nexus pairs a one-click model marketplace with a consumer-grade chat and agent interface, while remaining private by default: inference, file access, and tool execution all happen locally. The product targets a Windows-first v1, with the architecture deliberately chosen to keep later expansion to macOS and Linux straightforward.

### Architectural North Star

Nexus does not compile or embed an inference engine. Instead it manages llama.cpp as an external, swappable runtime: it downloads the appropriate prebuilt `llama-server` binary for the user's hardware, launches it as a separate local process, and communicates with it over a loopback HTTP API. This decision — analyzed in depth in Section 6 — is the single most important technical commitment in this document and shapes the entire backend. The shell is built on Tauri with a React/TypeScript frontend, and all local state (conversations, settings, model library, connector config) persists to an embedded SQLite database.

---

## 2. Goals & Non-Goals

### 2.1 Product Goals

- Let a non-technical Windows user go from install to chatting with a capable local model in under five minutes, with zero command-line interaction.
- Provide a marketplace that surfaces open-weight models and recommends the right model and quantization for the user's specific hardware.
- Deliver genuine agentic behavior: the model can call tools, read and write files, browse the web, and reach external systems through MCP connectors.
- Keep all user data local by default; the only network traffic in normal use is model downloads and explicitly invoked web/MCP actions. When a user opts into a cloud provider via their own API key, that traffic is clearly attributed to that provider.
- Offer an optional bring-your-own-key (BYOK) path to cloud models — OpenRouter, Anthropic (Claude), and OpenAI — so users can mix local and cloud models in one interface without the product losing its local-first default. Cloud models are simply absent until a key is added, and a one-click **Local only** filter in the model picker lets a user guarantee a local-only session without a separate global mode to manage.
- Remain lightweight: the UI shell should leave the overwhelming majority of system resources available for inference.

### 2.2 Non-Goals (v1)

- macOS and Linux support. The architecture must not preclude them, but they are explicitly out of scope for the first release.
- Model training or fine-tuning. Nexus is an inference and agent product only.
- Image or video generation. Noted as a future direction; v1 multimodal scope is image input only.
- Multi-user, team, or server deployment. Nexus is a single-user desktop application.

---

## 3. Target Audience & Personas

### 3.1 The Privacy-Conscious Professional

Works with sensitive documents or code and is unwilling or contractually unable to send it to a cloud provider. Values the local-only guarantee above raw model quality. Comfortable installing software but not with terminals or Python environments.

### 3.2 The Power User / Creator

Wants agentic capabilities — file manipulation, web search, automation through MCP — running for free on their own GPU. Will use the Advanced settings, swap models frequently, and connect their own MCP servers.

### 3.3 The AI Enthusiast

Wants to try the newest open-weight models the week they drop, without dealing with command-line tooling. The marketplace and one-click download are the primary draw.

---

## 4. Functional Requirements

Requirements are tagged **Must** (v1), **Should** (v1 if time allows), or **Later** (post-v1). Each carries an ID for traceability.

### 4.1 Model Marketplace (Hub)

| ID | Requirement | Priority |
|----|-------------|----------|
| MKT-1 | Browse a curated, searchable catalog of open-weight GGUF models sourced dynamically from **both** the Hugging Face API and GitHub Releases (trending, popular, recently updated). Sources are normalized into one unified model listing. | Must |
| MKT-2 | For each model, display size, parameter count, quantization options (e.g. Q4_K_M, Q8_0), license, and an estimated tokens/sec band for the user's hardware. | Must |
| MKT-3 | One-click download of a chosen quantization; downloads stream to the local app-data directory with resume support and SHA verification. | Must |
| MKT-4 | Smart hardware matching: detect RAM, VRAM, GPU vendor/model and flag each model/quant as "Runs great", "Runs slowly", or "Won't fit". | Must |
| MKT-5 | Local Library: list downloaded models, show disk usage, delete to reclaim space, set a default model. | Must |
| MKT-6 | Allow adding a model by direct Hugging Face repo ID, GitHub release asset URL, or raw GGUF URL for models not in the curated list. | Should |

### 4.2 Agentic Chat Interface

| ID | Requirement | Priority |
|----|-------------|----------|
| CHT-1 | Clean, distraction-free chat UI comparable to mainstream consumer assistants. Inference metrics (tokens/sec, temperature, context usage) hidden behind an Advanced toggle. | Must |
| CHT-9 | Agent run timeline: tool calls, file reads, web lookups, and code execution render inline as a compact, sequential timeline within the response (not hidden behind a collapsed details link), so the agent's work is visible by default. Each step shows a plain-language verb and target, with running/done states. | Must |
| CHT-2 | Real-time token streaming via Server-Sent Events from the runtime through the backend to the UI, with a working Stop/cancel control. | Must |
| CHT-3 | Persistent conversation history stored in a local SQLite database, with full-text search, rename, and delete. | Must |
| CHT-4 | A single, editable global system prompt for v1. Named multi-Persona profiles (save/switch between several) are deferred to a later release. | Must (single) / Later (Personas) |
| CHT-5 | Multimodal image input: drag-and-drop or paste an image; routed to a vision-capable model. Clear UX when the active model lacks vision support. | Must |
| CHT-8 | PDF ingestion: drag-and-drop a PDF; text (and, for vision models, page images) is extracted and injected into context. Handles both text-based and scanned PDFs. | Must |
| CHT-6 | Artifacts / Canvas side-panel that renders model-produced code, Markdown, Mermaid, or SVG separately from the chat stream. | Later |
| CHT-7 | Per-conversation model selection and parameter overrides (temperature, max tokens, context length). | Should |

### 4.3 Tool Calling & Skills

| ID | Requirement | Priority |
|----|-------------|----------|
| TOOL-1 | A tool-calling orchestrator that detects when the model requests a tool, executes the corresponding native function, and feeds the result back into the model loop. | Must |
| TOOL-2 | Native tool-calling support for models that emit structured tool calls, with a prompt-based fallback (JSON/ReAct scratchpad) for models lacking native support. Capability auto-detected per model. | Must |
| TOOL-3 | Built-in File System skill: read and write within user-whitelisted directories (see Section 6, Permissions). | Must |
| TOOL-4 | Built-in Web Search skill: headless fetch/scrape so a local model can retrieve current information. | Should |
| TOOL-5 | Built-in Code Execution skill: run model-written code on the host with an explicit per-invocation permission prompt and a configurable allowed-language list. | Should |
| TOOL-6 | Skills framework: discrete, declarative skill bundles (instructions plus optional scripts/tools) that the model can load on demand, extensible by the community. | Later |

### 4.4 MCP & Connectors

| ID | Requirement | Priority |
|----|-------------|----------|
| MCP-1 | Act as an MCP client. Support remote MCP servers over HTTP/SSE: the user pastes a server URL and Nexus handles connection, capability discovery, and tool registration. | Must |
| MCP-2 | Support local stdio MCP servers (spawned via a command). For advanced users in v1; see open issue on runtime bundling in Section 7.7. | Should |
| MCP-3 | Connector Dashboard: visual list of configured servers with connection status, available tools, enable/disable toggles, and OAuth or token entry where required. | Must |
| MCP-4 | Context injection: tools exposed by connected MCP servers are made available to the model's tool-calling loop and their results injected into context. | Must |
| MCP-5 | Config import/export: advanced users can edit MCP configuration as a file; light users use the GUI. Both write to the same underlying config. | Should |

### 4.5 Cloud Model Providers (Bring-Your-Own-Key)

| ID | Requirement | Priority |
|----|-------------|----------|
| CLD-1 | Let the user add API keys for cloud providers — OpenRouter, Anthropic (Claude), and OpenAI — and use those models alongside local ones in the same chat interface. | Must |
| CLD-2 | API keys are stored encrypted in the OS credential store (Windows Credential Manager), never in plaintext config or the SQLite database. | Must |
| CLD-3 | Cloud models appear in the same model picker as local models, each carrying a small provenance dot and grouped under "Cloud · your key" vs "On this device," so the user always knows which kind of model they're selecting before sending a message. | Must |
| CLD-4 | Per-provider model discovery: fetch the provider's available model list (e.g. OpenRouter's catalog) so the user selects from current models rather than a hardcoded list. | Should |
| CLD-5 | Tool calling, MCP, and skills work with cloud models through each provider's native tool-calling API, giving a consistent agentic experience across local and cloud. | Must |
| CLD-6 | Each response's run header (see CHT-9) states the model used and its provenance dot once per turn, and the full activity log records every outbound request, preserving the local-by-default privacy contract through transparency at the point of choice rather than repeated per-message decoration. | Must |
| CLD-7 | A **Local only** filter in the model picker that hides cloud models from selection for the duration it's active, so a user who wants a hard local-only session can guarantee it without auditing each model choice individually. Unlike a global mode, this is a filter on the existing picker, not a separate state to reconcile with model selection. | Must |

---

## 5. User Experience & Design

### 5.1 Design Principles

Nexus runs powerful, historically developer-only technology, but its interface must never feel like a generic chatbot — it is an agent that does work, and the interface must show that work, not just report a final answer. The guiding stance is **calm, literate, and visibly capable** — closer to a serious reading-and-writing instrument with a build log than to a chat bubble app. Approachability is achieved through clarity and plain language, not through decoration. Five principles govern every screen:

- **Conversation is the home.** The app opens directly into a chat ready to use, not a settings page, a model list, or a dashboard. A capable default model is pre-selected so a first-time user can type a message within seconds of launch.
- **The agent shows its work.** Tool calls, file reads, web lookups, and code execution are not hidden behind a collapsed "details" link — they render inline as a short, legible timeline as part of the response, the way a build log shows its steps before the summary. This is what makes Nexus read as an agent rather than a chatbot.
- **Progressive disclosure for configuration, not for action.** Inference settings — temperature, context length, tokens/sec, quantization, GPU offload — live behind a single "Advanced" affordance. What the agent *does*, however, is never hidden; only how it's configured is.
- **Plain language over jargon.** The interface names things by what the user controls, not how the system is built. "Add a model," not "pull a GGUF"; "Connect an app," not "register an MCP server"; "Allow file access," not "configure filesystem permissions."
- **Provenance lives at the point of choice, not the point of output.** Whether a model is local or cloud is a property of which model you picked, shown once in the model picker — not decoration repeated on every message. Trust comes from the user always knowing which model they selected, not from forensic color-coding of what already happened.

### 5.2 Visual Direction

**Thesis.** Nexus is an instrument for thinking, and a conversation with it is *work being done in front of you* — not a stream of chat bubbles. The interface is designed as a typeset editorial surface for the prose the model writes, paired with a compact, build-log register for the steps it takes. Approachability comes from clarity, generous space, and warmth of type — not from rounded bubbles and a friendly accent color. This is a deliberate departure from the prevailing AI-app look (warm-cream background, one accent, soft bubbles), which the product owner explicitly rejected.

The interface is near-monochrome ink-on-paper. Color is reserved almost entirely for function: distinguishing a model's identity in the picker, and marking the live/done state of a running step. It is not used to decorate or re-announce information the user already knows, which keeps the page calm even when the agent is doing several things in parallel.

**Palette — Light ("Paper").** Near-monochrome, warm-neutral, high-contrast.

- `--paper` `#F7F4EE` — warm paper ground, soft on the eyes for long reading.
- `--paper-edge` `#EDE8DE` — rules, dividers, timeline rail tint.
- `--ink` `#171513` — primary text; dense, near-black, warm.
- `--ink-muted` `#736C61` — metadata, timestamps, labels, step verbs.
- `--ink-faint` `#A8A093` — hairline rules, disabled text, idle timeline dots.

**Palette — Dark ("Slate").** Not black, not blue-black — a warm graphite that reads like dark paper.

- `--slate` `#1A1816` — base.
- `--slate-edge` `#272320` — rules, timeline rail tint.
- `--ink` (dark mode) `#ECE6DB` — primary text, warm off-white.
- `--ink-muted` (dark mode) `#9A9286`.
- `--ink-faint` (dark mode) `#4E4842`.

**Functional color.** Used sparingly and only where it serves a specific purpose:

- `--local` — indigo, `#3D4FA0` (light) / `#8C97E8` (dark). Marks a local model in the picker and in a response's model tag.
- `--cloud` — oxidized copper, `#B5642E` (light) / `#D6884A` (dark). Marks a cloud model the same way.
- `--ok` — a muted green, used only for a completed timeline step's dot.
- `--danger` `#A8453A` (light) / `#D6776B` (dark) — errors only.

Provenance color appears in exactly two places: the small dot beside a model's name in the picker, and the matching dot in a response's model tag at the top of that turn. It is **not** repeated as a margin, a rule, or a per-paragraph treatment — the earlier "provenance margin" concept was dropped because color-coding the output restates a decision the user already made when they picked the model, rather than helping them make it.

**Typography — the personality lives here.** Three roles, chosen to feel like a well-set book rather than an app, with a fourth register for the agent's visible work:

- Display / headings: a characterful text serif with real voice — **Newsreader** — used for the assistant's name, section titles, and empty-state lines.
- Body / reading: the same serif at reading size for the assistant's written responses, so its conclusions read like considered prose; a clean humanist sans (**Inter**) for UI labels, controls, and the user's own messages, distinguishing the two voices typographically rather than with bubbles.
- Mono: **JetBrains Mono**, used for code blocks *and* for the agent's step timeline (5.2a) — the one place "technical register" is a feature, because it signals "this is the system doing something," distinct from "this is the system telling you something."
- A deliberate editorial scale with strong contrast between levels (e.g. 11.5 timeline / 13–15 UI / 17.5 reading / 21–32 display), generous line-height (~1.65) on the reading column, and a constrained measure (~64ch) so responses never sprawl edge to edge.

**Shape, depth, motion — restraint.** Near-zero border-radius (this is paper, not plastic); structure is drawn with hairline rules and whitespace, not cards and shadows. There is essentially no elevation. Motion is minimal and functional: a running timeline step's dot pulses gently, completed steps settle to a solid dot, and the streaming cursor at the end of in-progress prose blinks plainly rather than glowing. Reduced-motion replaces all of it with instant static states.

### 5.2a Signature Element

The **agent run** is the signature: every assistant turn is rendered as a short, left-bordered timeline of mono-set steps — *searched, read, edited, checked* — each tied to a concrete target ("project files," "src/client.rs," "crates.io"), followed by the serif prose conclusion. A step in progress carries a softly pulsing dot; a finished step settles to solid. This is what separates Nexus from a chatbot: the page shows the agent thinking and acting in real time, not just arriving at an answer. It is also where the model's identity (name and a small local/cloud dot) is stated once, at the top of the run, rather than repeated as decoration throughout.

### 5.3 Information Architecture

The app is organized around a single primary surface (chat) and a small set of secondary surfaces reached from a slim left rail. The intent is that a non-technical user lives almost entirely on the chat surface and visits the others rarely.

```
┌──────────────────────────────────────────────────────────────┐
│  NEXUS                              ● Llama 3.1 8B ▾   Paper⏵ │
├──────────┬───────────────────────────────────────────────────┤
│          │  You                                                │
│  New     │   Find files using the old API endpoint…             │
│          │                                                     │
│  CHATS   │  ● Llama 3.1 8B                                    │
│  Today   │  │ searched   project files            — 3 matches │
│   Taxes  │  │ read       src/client.rs, src/sync.rs            │
│   Trip   │  │ checked    crates.io for csv          — 1.3.1   │
│  Earlier │                                                     │
│          │  Two files still call the old endpoint:             │
│          │  src/client.rs (line 41) and src/sync.rs            │
│  ──────  │  (line 12). Both use a hardcoded URL…               │
│  Models  │                                                     │
│  Apps    │  ┌─────────────────────────────────────────────┐   │
│  Settings│  │  Message Nexus                       ＋   ↑  │   │
│          │  └─────────────────────────────────────────────┘   │
└──────────┴───────────────────────────────────────────────────┘
```

- **Left rail:** New chat, searchable conversation history (grouped by recency), and three quiet entries — Models, Apps (connectors), Settings. Collapsible to icons.
- **Top bar:** the model picker (current model's name with a small local/cloud dot, click to switch — see below) and the light/dark mode control.
- **Model picker (dropdown):** grouped into "On this device" and "Cloud · your key," each option carrying its provenance dot; a **Local only** filter chip hides the cloud group entirely; an "Add a provider key" row at the bottom leads into BYOK setup. This dropdown is the single place provenance is chosen and shown — there is no separate global Private Mode, since selecting a local model already guarantees nothing leaves the machine for that turn.
- **Reading column:** user turns in the UI sans; assistant turns as an agent run — timeline first, serif prose conclusion second — within a constrained measure so text never sprawls.
- **Side panel (slides in):** used for permissions prompts, the full activity log, and the future Artifacts/Canvas — never a permanent fixture competing with the conversation.

### 5.4 Key Flows

**5.4.1 First run (time-to-first-message under five minutes).** On launch, Nexus greets the user in plain language, detects their hardware in the background, and recommends one model with a one-line rationale ("Best fit for your PC — fast and capable"). A single primary button downloads it with a friendly progress state ("Getting your model ready — about 2 minutes"). The moment it is ready, the user is dropped into a chat with a suggested first prompt. No accounts, no configuration, no terminology gate. Only local models are present until the user adds a cloud key, so the first run is local by simple default rather than by a mode switch.

**5.4.2 Adding a model (marketplace).** The Models surface presents an app-store-like grid of cards: model name, a plain one-line description, a clear fit badge ("Runs great on your PC" / "Runs slowly" / "Won't fit"), and size. Quantization choices are framed as a simple quality-vs-speed-vs-size slider rather than raw labels like Q4_K_M, with the technical label available on tap for advanced users. One button downloads; downloaded models surface a "Use" button and appear in the model picker's "On this device" group.

**5.4.3 Connecting an app (MCP / connectors).** Presented as "Apps" — a gallery of connectable services and a single field: "Paste a link to connect an app." The light user pastes a URL and Nexus handles discovery and tool registration, then confirms in plain terms what the app can now do ("GitHub is connected. Nexus can read your repositories."). Advanced users reach manual/stdio config behind an "Advanced setup" link. Once connected, the agent's use of that app's tools appears as ordinary steps in the run timeline (5.2a) — "called GitHub — opened issue #142" — so a connector's activity is never invisible.

**5.4.4 Granting permission.** When the assistant first needs a capability, a calm side-panel prompt explains in one sentence what it wants and why, scoped narrowly: "Nexus wants to read files in your Documents/Taxes folder to answer this. Allow?" with Allow once / Allow for this chat / Deny. Granted permissions are listed and revocable in Settings. The tone is a trusted assistant asking, not a security system warning.

**5.4.5 Adding cloud models (BYOK).** The model picker's "Cloud · your key" group is empty and shows "Add a provider key" until the user adds one. Tapping it opens a short, reassuring explainer of exactly what changes — that a cloud model sends messages to that provider, that it only ever applies when the user explicitly selects it, and that local models are unaffected — followed by a simple key-entry screen per provider (OpenRouter, Anthropic, OpenAI). Once added, those models simply appear in the picker like any other model choice.

**5.4.6 Choosing local vs. cloud per conversation.** Provenance is just part of picking a model: the picker shows local models first, cloud models (if configured) below, each with its dot, and a **Local only** filter to hide cloud entirely for a session where the user wants a hard guarantee without thinking about it turn by turn. Switching models mid-conversation is ordinary — the new turn's run header simply shows the newly selected model and its dot, with no separate mode state to reconcile.

### 5.5 Accessibility & Quality Floor

Non-negotiable baseline for every screen, present from v1:

- WCAG AA contrast for all text and meaningful UI; both the Paper and Slate palettes are specified to satisfy this, and the local/cloud dots are tuned per mode for AA against their grounds.
- **Provenance is never signaled by color alone.** The local/cloud dot in the picker and run header is always paired with the model's name and, in the picker, a group label ("On this device" / "Cloud · your key"), so colorblind users and screen-reader users get the same information without relying on hue.
- Full keyboard operability with a visible focus ring; the composer, model picker (including the filter chip), and all primary actions are reachable without a mouse.
- Respect for the OS reduced-motion setting (the pulsing timeline dot and streaming cursor resolve to static states; nothing animates).
- Legible defaults: a comfortable reading-size base, adjustable in Settings, with the layout reflowing rather than truncating.
- Screen-reader labels on all controls written in the same plain language as the visible copy; each timeline step is announced as a single sentence ("searched project files, 3 matches found").
- Responsive down to a small window; the timeline remains inline with the response (it never relies on a side column to render), the left rail collapses to icons, and the side panel becomes a full-height overlay.

### 5.6 Voice & Microcopy

The interface speaks in plain, warm, sentence-case language, from the user's side of the screen. Actions say what they do ("Add model," "Connect," "Allow once"). Timeline steps are written as plain past-tense verbs with concrete targets ("read src/client.rs," "searched crates.io"), never vague ("processing…") and never falsely dramatic. Empty states invite action rather than apologize ("No chats yet — say hello to get started"). Errors explain what happened and the next step, in the interface's voice, never blaming the user and never vague ("That model is too large for your PC's memory. Try a smaller size, or free up space."). Cloud and permission moments are factual and calm, never alarmist. Technical terms (GGUF, MCP, quantization) appear only in Advanced areas, and even there are paired with a plain gloss on first encounter.

---

## 6. Security & Permissions Model

Because Nexus grants a local model direct access to the operating system, the permission model is a first-class product surface, not an afterthought. The guiding principle is **least privilege with explicit, legible user consent.**

### 6.1 File System Access

- No directory is accessible by default. The user explicitly whitelists folders.
- Each whitelisted directory carries a mode: Read-Only or Read/Write.
- All file operations performed by the model are logged in a visible activity panel the user can review.
- Path-traversal and symlink-escape protections prevent the model from reaching outside whitelisted roots.

### 6.2 Code Execution

Per the product direction, code runs directly on the host OS (no mandatory container) but is gated by granular controls:

- Execution is disabled by default and must be enabled per session or per persona.
- Each execution surfaces the exact code and requires explicit approval, with an optional "always allow for this conversation" choice.
- An allowed-language list and a configurable working directory bound what can run and where.

> **Open risk:** direct host execution is the stated preference but is inherently high-risk. An optional sandbox (subprocess isolation or WebAssembly) is recommended as a fast-follow and is flagged as a decision in Section 10.

### 6.3 Network & Privacy

- Telemetry is strictly opt-in and, if enabled, never includes prompts, file contents, or generated text.
- Outbound network activity in normal operation is limited to model downloads and user-invoked web/MCP actions; these are surfaced in the activity panel.
- Cloud model use (BYOK) is the one case where prompt content intentionally leaves the machine. It happens only for models the user explicitly selects from a cloud provider, is shown via that model's identity in the picker and the run header, and never occurs for local models. API keys live in the OS credential store, never in plaintext. The model picker's **Local only** filter lets a user hide cloud models for a session, guaranteeing a local-only choice without a separate mode to manage.
- The local runtime HTTP server binds only to the loopback interface and is protected by a per-session token (see Section 7.4).

---

## 7. Technical Architecture

This section records the architectural decisions reached during product definition and the rationale behind them, so that contributors understand not just what was chosen but why.

### 7.1 High-Level Stack

| Layer | Technology | Responsibility |
|-------|-----------|----------------|
| Desktop shell | Tauri (Rust core + system WebView) | Window management, native OS integration, small footprint. |
| Frontend / UI | React + TypeScript | Chat, marketplace, connector dashboard, permissions UI. |
| Application backend | Rust (Tauri commands) | Orchestration: runtime management, tool-calling loop, MCP client, permissions enforcement, HTTP proxy to the engine. |
| Persistence | SQLite (embedded) | Conversations, messages, settings, model library, connector config; full-text search over history. |
| Inference engine | llama.cpp (prebuilt `llama-server` binary, external process) | Model loading and token generation on CPU/GPU. |
| Model format | GGUF | Quantized open-weight models. |

#### 7.1.1 Persistence (SQLite)

All local state lives in a single embedded SQLite database in the app-data directory: conversations and messages (including attachment references), application settings, the downloaded-model library, and MCP connector configuration. Conversation search (CHT-3) is served by SQLite's FTS5 full-text index. Attachment binaries (images, PDFs) are stored on disk with the database holding paths and metadata rather than blobs, to keep the database compact. A schema-version table supports forward migrations as the app evolves.

#### 7.1.2 Platform Baseline (Windows 10)

The minimum supported OS is Windows 10 (64-bit, x64). The main implication is the WebView2 runtime that Tauri depends on: it is not guaranteed present on older Windows 10 builds, so the installer must detect and, if necessary, bootstrap the Evergreen WebView2 runtime. ARM64 Windows is out of scope for v1 (x64 only); the runtime matrix in 7.3.2 is x64-only accordingly.

### 7.2 The Core Decision: External Runtime over HTTP (Option B)

Three implementation paths were considered for connecting the Tauri/Rust application to llama.cpp:

1. In-process Rust FFI bindings (e.g. `llama-cpp-2`) that compile llama.cpp into the application binary.
2. **External prebuilt `llama-server` binary launched as a child process, communicated with over loopback HTTP (chosen).**
3. Electron + `node-llama-cpp`, abandoned because it conflicts with the Tauri/Rust shell and carries the heaviest footprint.

Option B was selected. The decisive factors:

- **Engine updates without app releases.** llama.cpp ships new builds almost daily, including support for brand-new model architectures and quantization types. With an external binary, adopting a newer engine is a config change; with FFI it requires recompiling and re-releasing the whole app.
- **Crash isolation.** A malformed GGUF or an engine bug becomes a dead child process the UI can detect and report, not a segfault that takes the entire application down.
- **No native build pipeline.** The hardest cost of the FFI path — compiling llama.cpp with the correct GPU backend (CUDA/Vulkan/HIP) on CI — is avoided entirely by consuming upstream's prebuilt binaries.
- **Industry precedent.** LM Studio runs its engines as separately-installed, auto-updating, out-of-process runtimes and introduced an explicit protocol decoupling the engine from the GUI. The closest comparable product converged on this same architecture.

The accepted trade-offs of Option B — process lifecycle management, port selection, a streaming HTTP proxy, and the loopback security surface — are addressed in 7.4 and are well-understood, bounded engineering tasks.

### 7.3 Runtime Management Subsystem

This subsystem is the heart of the architecture and the main piece of original engineering. Because the v1 target is Windows only, the matrix below is scoped accordingly.

#### 7.3.1 Where binaries come from

Nexus does not build llama.cpp. The upstream `ggml-org/llama.cpp` project publishes prebuilt Windows binaries on essentially every release (cut roughly daily), covering CPU, CUDA 12, CUDA 13, Vulkan, HIP, and SYCL targets. Nexus consumes these directly. The only ongoing maintenance is: (a) a runtime manifest mapping hardware to the correct asset, (b) the hardware-detection and selection logic, and (c) periodically pinning and testing a newer upstream build.

#### 7.3.2 Windows runtime matrix (v1)

| User hardware | Selected backend | Notes |
|---------------|------------------|-------|
| NVIDIA GPU (recent driver) | CUDA 12.x build (+ CUDA runtime DLL package) | Most mature path. Requires shipping the separate cudart package alongside the engine. |
| NVIDIA GPU (newest, e.g. Blackwell) | CUDA 13.x build | Newer toolkit for newest cards; selection keyed on detected GPU generation. |
| AMD GPU | Vulkan build (preferred) / HIP | Vulkan is the default for reliability; HIP/ROCm offered as an opt-in given its history of driver-specific failures. |
| Intel Arc / iGPU | Vulkan or SYCL | Vulkan as the safe default; SYCL exposed for users who want it. |
| No discrete GPU | CPU build (AVX2 / AVX-512 variant) | Fallback; CPU instruction-set variant chosen from detection. Slow on larger models, clearly signaled in UI. |

#### 7.3.3 Runtime lifecycle

1. On first run (and on demand), survey hardware: GPU vendor/model, driver, VRAM, system RAM, CPU instruction-set support.
2. Resolve the correct entry in the runtime manifest and download the matching `llama-server` asset (plus CUDA runtime DLLs if applicable).
3. Verify the download against its published SHA-256, then unpack into the app-data runtimes directory.
4. Pin the runtime version; expose an "update available" flow rather than silently tracking every daily upstream build.
5. Allow advanced users to select an alternate backend manually (e.g. force Vulkan on an NVIDIA card for debugging).

### 7.4 Engine Process & Communication

- **Spawn model.** On "Load Model", the backend launches `llama-server` as a child process with the chosen model, context size, and GPU-offload settings.
- **Dynamic port.** The engine binds a free loopback port chosen at launch — never a hardcoded port — and the backend records it.
- **Readiness gating.** The UI is gated on a health-check poll; requests are not sent until the model has finished loading into memory.
- **Streaming proxy.** Token streams flow `llama-server` → Rust backend → WebView via Tauri's event/channel mechanism, with backpressure and user-initiated cancellation handled in the proxy.
- **Lifecycle safety.** The backend tracks the child PID and guarantees termination on app exit, including ungraceful shutdown, to prevent orphaned engine processes holding VRAM.
- **Loopback security.** The engine binds `127.0.0.1` only and is protected by a random per-session auth token, preventing other local processes from issuing requests to it.

### 7.5 Agentic Loop & MCP Client

The Rust backend owns the agent loop. On each model turn it inspects output for tool calls; for models with native tool-calling it parses structured calls, and for models without it falls back to a prompt-injected JSON/ReAct convention. Tool calls are dispatched to one of three handlers: built-in skills (file, web, code), or a connected MCP server's tools. Results are appended to context and the loop continues until the model produces a final answer. The MCP client maintains connections to configured servers, performs capability discovery, and registers their tools into the same dispatch table the built-in skills use, so the model sees one unified tool surface. The loop is deliberately model-source-agnostic: it works identically whether the model is a local `llama-server` instance or a cloud provider (see 7.6).

### 7.6 Model Routing: Local vs Cloud

A routing layer in the backend sits in front of the agent loop and directs each request to the correct provider based on the selected model. Two classes of provider exist behind a single internal interface:

- **Local provider** — the llama.cpp runtime described in 7.2–7.4 (loopback HTTP, hardware-matched binary, lifecycle-managed).
- **Cloud providers** — OpenRouter, Anthropic, and OpenAI, reached over HTTPS using the user's own API key (BYOK). These bypass the runtime-management subsystem entirely; there is no binary to download or process to manage.

Because OpenRouter, OpenAI, and llama.cpp's own server all speak an OpenAI-compatible chat-completions API, a single adapter covers most of the surface, with a dedicated adapter for Anthropic's Messages API. Keys are read from the OS credential store at request time and never persisted to SQLite or config files (CLD-2). The routing layer is also where the local-by-default privacy contract is enforced in the UI: any request bound for a cloud provider is dispatched only because the user explicitly selected that model in the picker (CLD-3), so leaving the machine is always a visible, deliberate act tied to model choice rather than a separate state. The **Local only** filter (CLD-7) is implemented at the picker/UI level — it filters which models are selectable — rather than as a backend mode, since the routing layer already only ever calls the specific model the user picked; there is no ambient "current mode" for it to override.

### 7.7 Open Architectural Issue: stdio MCP & runtime bundling

Remote HTTP/SSE MCP servers are clean to support: the user pastes a URL and the backend connects. However, many popular MCP servers are distributed as local stdio processes launched via `npx` (Node) or `uvx` (Python). Supporting those (MCP-2) requires Nexus to either rely on a runtime the user already has installed or bundle a Node/Python runtime — which reintroduces, in a smaller form, exactly the runtime-bundling problem that the engine architecture was designed to avoid. This is flagged as a decision in Section 10.

---

## 8. Non-Functional Requirements

| Category | Requirement |
|----------|-------------|
| Performance (shell) | The Tauri UI shell should consume on the order of ~150 MB RAM, leaving system resources free for inference. |
| Performance (inference) | Engine selection must exploit available acceleration (CUDA/Vulkan) and fall back to optimized CPU builds; the UI must remain responsive during generation. |
| Hardware support | Wide Windows hardware spectrum: modern NVIDIA/AMD/Intel GPUs through to CPU-only older machines. |
| Privacy | Local-by-default processing; opt-in, content-free telemetry only. |
| Offline | Core chat and local-file tooling must work with no internet; only downloads and web/MCP actions require connectivity. |
| Licensing | Open-source license; all bundled components (llama.cpp under MIT, etc.) must be license-compatible and attributed. |
| Updatability | Engine runtimes and the app update independently of each other. |

---

## 9. Phased Release Plan

A tight, shippable v1 is recommended over attempting the full feature surface at once. Suggested sequencing:

### 8.1 v1 (MVP) — Must

- Tauri Windows shell (React frontend, SQLite persistence, WebView2 bootstrap in installer); runtime management subsystem (detect → download → verify → launch) for the x64 Windows matrix in 7.3.2.
- Chat with streaming and SQLite-backed history/search; single global system prompt; marketplace browse from both Hugging Face and GitHub releases + hardware-matched one-click download + local library.
- Tool-calling orchestrator with native + fallback paths; File System skill with the permissions model; image and PDF input.
- MCP client for remote HTTP/SSE servers + connector dashboard.
- BYOK cloud providers (OpenRouter, Anthropic, OpenAI) with encrypted key storage, a unified model picker showing local and cloud models with provenance dots, and a one-click Local only filter.

### 8.2 v1.x — Should

- Web Search and Code Execution skills; multi-Persona profiles; per-conversation parameter overrides; stdio MCP support; config import/export.

### 8.3 v2+ — Later

- Artifacts/Canvas panel; full community Skills framework; optional code-execution sandbox; macOS and Linux ports; image/video generation via open-weight models.

---

## 10. Open Decisions

Items requiring a decision before or during v1 implementation:

| # | Decision needed | Recommendation |
|---|-----------------|----------------|
| D-1 | Frontend framework: React vs Svelte. | **Resolved: React.** |
| D-2 | stdio MCP in v1: bundle a Node/Python runtime, depend on a user-installed one, or defer to v1.x (remote-only first). | Defer to v1.x; ship remote HTTP/SSE MCP in v1 to avoid reintroducing runtime bundling. |
| D-3 | Code execution isolation: host-direct only vs optional sandbox. | Ship host-direct with strong consent UX in v1; add optional sandbox as fast-follow. |
| D-4 | Engine version policy: how aggressively to track upstream llama.cpp builds. | Pin a tested build; offer manual "update runtime" rather than auto-tracking dailies. |
| D-5 | Marketplace curation: fully dynamic vs a curated allow-list overlay. | **Resolved:** dynamic discovery across Hugging Face + GitHub releases, with a lightweight curated "recommended" overlay for the consumer persona. |
| D-6 | AMD default backend: Vulkan vs HIP/ROCm. | Default Vulkan for reliability; expose HIP as opt-in. |

---

## 11. Resolved Product Decisions

The following were confirmed by the product owner and are reflected throughout this document:

| # | Question | Decision |
|---|----------|----------|
| Q1 | Frontend framework | **React** (+ TypeScript). |
| Q2 | Conversation history storage | **SQLite** embedded database (FTS5 for search). |
| Q3 | Marketplace sources | **Both** Hugging Face and GitHub releases. |
| Q4 | Personas vs single system prompt for v1 | **Single global system prompt** for v1; multi-Persona deferred. |
| Q5 | Minimum Windows version | **Windows 10** (x64; ARM64 out of scope for v1). |
| Q6 | Multimodal scope for v1 | **Image and PDF** input. |
| Q7 | Cloud model access (revised) | **In scope for v1** as BYOK — OpenRouter, Anthropic, OpenAI — alongside local models (was previously deferred). |
| Q8 | UI direction (post-mockup review) | **Agent run timeline** replaces per-message provenance coloring; provenance lives only in the model picker with a **Local only** filter, replacing the global Private Mode concept. |

### Remaining items still open

- D-2, D-3, D-4, D-6 in Section 10 remain engineering-side decisions to confirm during implementation.
- Confirm whether the v1 installer should hard-require WebView2 bootstrap (recommended) given the Windows 10 baseline.

---

## 12. Development Environment Setup

This section is reference material for whoever begins implementation; it is not a product requirement. It assumes a Windows development machine with Node.js and npm already installed, per the locked architecture in Section 7 (Tauri + Rust shell, prebuilt `llama-server` runtime, React frontend).

### 12.1 What needs installing

| Tool | Why | Notes |
|------|-----|-------|
| Rust toolchain (via rustup) | Tauri's application backend is Rust. | Install from `https://rustup.rs`, not a package manager — keeps the toolchain current and easy to update. Installs `rustc` and `cargo`. |
| Microsoft C++ Build Tools | Rust on Windows requires the MSVC linker. | Install "Build Tools for Visual Studio" (not full Visual Studio) and select the **Desktop development with C++** workload. The single most common setup failure for Rust-on-Windows newcomers is skipping this — the resulting linker error doesn't obviously point at the missing workload. |
| WebView2 runtime | Tauri renders through the OS WebView rather than bundling Chromium. | Windows 11 ships with it; Windows 10 very likely has it via Windows Update but not guaranteed on older builds. For development, install Microsoft's Evergreen distributable directly. The installer-bootstrap question for end users (6.1.2 / Section 7) is a separate, later concern. |
| Tauri CLI | Scaffolding and running the app. | `npm install --save-dev @tauri-apps/cli`, callable as `npx tauri ...`. |

**Not needed:** any CUDA toolkit, cuDNN, or GPU compiler. Per the architecture decision in 7.2, Nexus consumes prebuilt `llama-server` binaries rather than compiling llama.cpp, so no native GPU build chain is required on the dev machine — only on whichever machine eventually produces the official `llama-server` release artifacts, which is upstream's responsibility, not this project's.

### 12.2 Suggested setup order

1. Install Rust via rustup; restart the terminal; confirm with `rustc --version`.
2. Install the C++ Build Tools workload (this step is mostly download time).
3. Run `npx @tauri-apps/cli info` — a built-in diagnostic that reports exactly what's missing rather than failing opaquely later.
4. Scaffold a throwaway project with `npm create tauri-app@latest`, selecting React + TypeScript. A successful build of this empty skeleton confirms the whole toolchain (Rust, linker, WebView2, Tauri CLI) works before any Nexus-specific code is written.

### 12.3 First real milestone

Once the skeleton builds, the recommended first implementation milestone is a minimal version of the architecture's core loop (Section 7.2–7.4): a Tauri command that downloads or locates a `llama-server` binary, spawns it on a dynamically chosen loopback port, and proxies a single streamed completion request back to the React UI. This exercises the riskiest part of the architecture — process lifecycle and streaming — before any marketplace, MCP, or tool-calling work begins.
