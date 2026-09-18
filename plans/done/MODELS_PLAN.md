# Models, Providers & Runtime — Settings rework

Status: built (2026-09-18). The four open questions are settled under
*Decisions*; where the build differs from the plan, see *Build notes* at the end.

## Why

Today the answer to *"what can I use, and where does it come from?"* is spread
over three places, none of which answers it:

| Where | What it holds | Problem |
|---|---|---|
| **Models** | Local GGUF + diffusion catalog, "Your PC" panel | Only local. Cloud models are invisible outside the composer picker. |
| **General** | BYOK keys, "Your own model servers", Recall | Keys sit between *Reading size* and *Memory*. Doesn't scale past 3 providers. |
| **Engine** | llama.cpp status, acceleration, build, hardware (again), image + recall engines | "Engine" is jargon; hardware panel duplicated from Models. |

Concrete gaps found in the code:

- *Default model* is local-only (`is_default` on library rows). A cloud or
  own-server model can be *selected* but never *the default*.
- `Credential::Media` backends have no key row (open since `BKD-2`), so a
  media-only provider can't be connected at all.
- Backend errors say "Add one in Settings → Cloud" (`media/backends/openai.rs:49`,
  `openrouter.rs:86`) — no such place exists.
- The picker's empty-cloud links go to General, the key UI has no inline error
  (`/* keep simple */`), and nothing verifies a key before saying "Key saved".
- The Anthropic list is curated and stale (Claude 3.x).

## The idea — one question per section

The user shouldn't have to know *how* a model runs to use it. So split by the
question the user is asking, not by the technology:

| Section | Question it answers | Audience |
|---|---|---|
| **Models** | *What can I think / draw with, and which is my default?* | everyone |
| **Providers** | *Which cloud accounts are connected?* | anyone with a key |
| **Runtime** | *What runs my models locally — this PC's runtime or my own servers?* | pros (shown to everyone, untagged) |

Models becomes the **joint view**: local and cloud models side by side,
distinguished by a *where it runs* badge, not by living on different screens.
Providers is where you connect cloud accounts. Runtime is where you tune the
local machinery, including your own servers (Ollama, LM Studio), which are
runtimes you already run. Each links to the others at the exact moment you'd
need it.

Hub nav after: `General · Models · Providers · Runtime · Tools · …`

## Vocabulary (plain words first)

One set of words, used identically in settings, picker, errors and onboarding.

| Internal | User-facing | Notes |
|---|---|---|
| provenance `local` | **On this PC** | green dot, as in the picker today |
| provenance `endpoint` | **Your server** · *label* | e.g. "Your server · Ollama" |
| provenance `cloud` | **Via** *Provider* | e.g. "Via OpenRouter" |
| engine / llama-server | **local runtime** | only on the Runtime page and its tooltips |
| BYOK | **Connect your account** | "your key" stays as secondary text |
| `tools: false` | **Chat only** | tooltip keeps the existing explanation |
| `vision: true` | **Sees images** | |
| quant, GGUF, cfg, steps | hidden in Models | shown on Runtime / behind "Details" |
| fit `great`/`ok`/`wont-fit` | **Runs great / Runs OK / Too big for this PC** | reuse `FIT_LABEL` |

---

## Part A — Rename Engine → Runtime (`RTM-*`)

Small and mechanical. Ship first; it touches the files the later parts edit.

- **`RTM-1` view id + nav.** `View` `"engine"` → `"runtime"`; `HUB_SECTIONS`
  label "Runtime". Update `SettingsHub.tsx`, `Rail.tsx:357`, the smoke test
  and the routing test. No persistence migration is needed: `SHL-24`
  removed persisted route tabs.
- **`RTM-2` files.** `routes/Engine.tsx` → `routes/Runtime.tsx` (+ css),
  `ImageEngine` → `ImageRuntime`, `EmbedEngine` → `RecallRuntime`. CSS
  classes `engine-*` → `runtime-*` in the same move.
- **`RTM-3` strings.** Page title "Runtime"; lede: *"The local runtime runs open
  models on your PC. It downloads by itself and is matched to your hardware —
  llama.cpp for chat and recall, stable-diffusion.cpp for images."* Buttons
  "Start runtime / Stop runtime", card "Image runtime", "Recall runtime".
  `About.tsx` "Local model runtime (llama-server)".
- **`RTM-4` header readout and onboarding: wording only.** No behaviour or
  structure changes. `EngineStatus.tsx`: "Engine idle" → "Runtime idle",
  "Engine ready" → "Runtime ready", "Starting engine…" → "Starting
  runtime…". The tooltip says "local model runtime (llama-server)". Onboarding:
  "Install the engine" → "Install the runtime", with the same step and the same
  body text. The component and its CSS classes keep their names (`RTM-5`).
- **`RTM-5` out of scope.** Rust module/command names (`runtime_overview`,
  `stop_engine`, …) and store fields (`engineReady`) keep their names. Renaming
  them is churn with no user-visible effect; a later cleanup can do it.

## Part B — Providers, a dedicated view (`PRV-*`)

Moves cloud keys out of General into their own hub section, built to scale
to many providers. Own servers go to Runtime instead (`RTM-10`).

### Layout

```
Providers
Connect your accounts to use hosted models. Keys stay in Windows
Credential Manager, never in a file or your chats.

CLOUD ACCOUNTS                                   Spent this month: $3.12 →
┌───────────────────────────┐ ┌───────────────────────────┐ ┌──────────────
│ ◉ OpenRouter   Connected  │ │ ○ OpenAI     Not connected│ │ ○ Anthropic
│ 312 chat · images · video │ │ Chat · images             │ │ Chat
│ $2.80 this month          │ │                           │ │
│ [See models]  [Manage ▾]  │ │ [Connect]  Get a key →    │ │ [Connect]
└───────────────────────────┘ └───────────────────────────┘ └──────────────
┌───────────────────────────┐
│ ○ fal.ai   Not connected  │   ← Credential::Media descriptors, same card
│ Images · video            │
└───────────────────────────┘

Running Ollama or LM Studio? Add it under Runtime → Your servers.
```

### Tasks

- **`PRV-1` section.** New hub view `"providers"`, icon `⌁`, between Models
  and Runtime. Route `routes/Providers.tsx`. The key block from
  `Settings.tsx` is rebuilt here as cards (below). `LocalEndpointsSettings`
  moves to Runtime (`RTM-10`). General loses both blocks and keeps a
  two-link pointer for a release: *"Cloud keys moved to Providers →, model
  servers to Runtime →"*.
- **`PRV-2` one card list from the backend.** New command `list_providers()`
  returning chat providers (`cloud::Provider::ALL`) **and** every media
  backend with `Credential::Media`, each as
  `{ id, name, kind: "cloud"|"media", key_set, status, unlocks: ["chat","image","video"],
  chat_model_count, key_hint, console_url, last_error }`. `unlocks` is derived
  from the media registry plus "chat" for chat providers. The frontend never
  hard-codes a provider. A new backend adds its card for free, which closes
  `BKD-2`'s missing UI.
- **`PRV-3` connect flow that proves it worked.** Card → **Connect** expands
  inline: key field (hint as placeholder), **Connect** button, "Get a key →".
  Pressing Connect calls new `verify_provider_key(id, key)`. It makes one cheap
  authenticated call (OpenAI/OpenRouter `GET /v1/models` with auth, Anthropic
  `GET /v1/models`) and saves only on success. States:
  - *Checking…* (button disabled)
  - *Connected — 312 chat models, images and video are now available.
    [See models →]* (the link opens Models filtered to this provider)
  - *That key was rejected (401). Check you copied all of it.* / *Couldn't
    reach OpenAI — are you online?* (inline, red, field keeps its value)
- **`PRV-4` card states.** `Not connected` (muted), `Connected` (green dot +
  what it unlocks + month spend), `Needs attention` (amber: the last call
  returned 401/402, e.g. *"Out of credits — top up at OpenRouter →"*). The
  status comes from the last real request, so it's `last_error`. **Manage ▾**
  holds Replace key, Disconnect (with confirm, naming what stops working:
  *"12 chats use Claude Sonnet via Anthropic; they'll fall back to your
  default."*).
- **`PRV-5` spend.** Per-card month spend = media spend (`CST-2`) + chat usage
  (Usage view data) for that provider. The header total links to Usage.
  Hidden entirely at $0, as today.
- **`PRV-6` fix the dead links.** Backend strings "Settings → Cloud" →
  "Settings → Providers". Picker's empty-cloud row: "+ Add a provider key"
  links to `providers`, and "+ connect a local server" links to `runtime`
  (Your servers tab). Today both go to `settings`.
- **`PRV-7` Anthropic discovery.** Replace `curated_anthropic()` with
  `GET /v1/models` (with the curated list as offline fallback), so the joint
  view doesn't show a stale catalog.

## Part C — Models, the joint view (`MOD-*`)

### Layout

```
Models
Everything Poiesis can think and draw with — on this PC or through your accounts.

[ Chat ]  [ Images & video ]            [All] [On this PC] [Cloud]   🔍 Search

FAVORITES                        drag to reorder · these lead the picker
  ⠿ 1 ● Qwen2.5 7B · On this PC            Default          [Use] ★
  ⠿ 2 ○ Claude Sonnet 4.5 · Via Anthropic  [Make default]   [Use] ★
  ⠿ 3 ● mistral-nemo · Your server         [Make default]   [Use] ★
  If your default isn't available, I'll use the next favorite that is.

ON THIS PC                                                   11.2 GB on disk
┌──────────────────────────────┐ ┌──────────────────────────────┐
│ ● Qwen2.5 7B Instruct   ★    │ │ ● Llama 3.2 3B               │
│ Default · Runs great         │ │ Runs great                   │
│ Free · private · offline     │ │ Free · private · offline     │
│ [Use]           Details ▾ ⋯  │ │ [Use]  Make default     ⋯    │
└──────────────────────────────┘ └──────────────────────────────┘
YOUR SERVER · OLLAMA
  ● mistral-nemo           Chat only                     [Use] ☆
VIA OPENROUTER                                    312 models · 🔍 filter
  ○ Claude Sonnet 4.5      Sees images   $3 / $15 per 1M  [Use] ★
  ○ GPT-5 mini             Sees images   $0.25 / $2       [Use] ☆
  … show all 312

ADD MODELS
┌──────────────────────────┐ ┌──────────────────────────┐ ┌──────────────────────────┐
│ ⬇ Recommended for this PC│ │ ⧉ From Hugging Face or   │ │ ⌁ Connect an account     │
│ RTX 4070 · 12 GB · 32 GB │ │   a link                 │ │ OpenAI, Anthropic,       │
│ Picks that run great here│ │ Any GGUF repo or file URL│ │ OpenRouter…              │
│ [Browse]                 │ │ [Add]                    │ │ [Open Providers →]       │
└──────────────────────────┘ └──────────────────────────┘ └──────────────────────────┘
  (the chosen door expands below: catalog cards / repo+quant flow / provider list)
```

### Tasks

- **`MOD-1` one list, three groups.** Models reads the store's unified
  `models` (the same array the picker uses) plus `libraryModels` for disk
  details. Groups: *On this PC*, *Your server · X* (one per endpoint),
  *Via Provider* (one per provider). Group heads use the picker's provenance
  dots, so the settings page and the picker speak the same visual language.
  Local groups render as cards (they carry size/fit/delete). Cloud groups
  render as compact rows, because there can be hundreds. A *Your server*
  group's head links to its row on Runtime → Your servers, which is where
  it's added, edited and tested. Models only lists what it serves.
- **`MOD-2` modality tabs.** *Chat* and *Images & video* replace
  *Language / Image*. The image tab gets the same three groups: local
  diffusion models (from `ImageModels`) and cloud media models from
  `refreshMediaModels`. `ImageModels` is split: its library + catalog become
  groups here, and its engine tip is replaced by the setup banner in `MOD-7`.
- **`MOD-3` defaults across sources.** New settings `default_model.chat` and
  `default_model.image`, each holding any model id (`local:`, `endpoint:`,
  `cloud:`). **Video has no default.** A video model is picked per chat,
  and the Images & video tab offers *Make default* only on image models.
  These settings replace `is_default` as the source of truth. Migration: on first read, if unset, seed `chat` from the library row with
  `is_default`. `reconcileSelection` resolves in this order: default, then
  the next *available* favorite (`MOD-4`), then today's fallback. When it
  skips the default (key removed, file deleted, server off), it says so
  once in the composer: *"Claude Sonnet isn't available right now. Using
  Qwen2.5 7B, your next favorite."* The default row in Favorites shows
  *Not available* with the reason.
- **`MOD-4` Favorites: your preferred models.** The user's short list, across
  all sources, one list per tab (Chat, and Images & video).
  - **Storage.** Settings `models.favorites.chat` and `models.favorites.media`:
    ordered `string[]` of model ids. The order is the preference. Video models
    can be starred for ordering, but only image models take part in the
    default fallback, since video has no default.
  - **In Models.** The *Favorites* section is the first thing on each tab. Rows
    are reorderable (drag handle, plus ↑/↓ on keyboard focus for
    accessibility) and show position, where the model runs, and
    availability. **Make default** here is the same action as in `MOD-3`.
    The default is always a favorite and sits in slot 1; making another row
    default moves it to the top. Empty state: *"Star models you use often.
    They'll lead the model picker, and I'll fall back to them in order if
    your default isn't available."*
  - **Everywhere else.** A ★ on every model row (Models groups, picker rows)
    toggles membership, and new stars append to the end. Unstarring the
    default isn't allowed. The star is disabled with the tooltip *"Pick
    another default first."*
  - **Picker.** Favorites is the picker's first group, in the user's order.
    Then On this PC → Your servers → Cloud (searchable). `CLOUD_LIMIT`
    only truncates the cloud group, never favorites. A new install
    auto-favorites the first model it gets, so the list is never empty
    once something is usable.
  - **Personas.** A persona's pinned model (`PersonaEditor`) is chosen
    from the same list first. The full list sits behind "More models…".
- **`MOD-5` plain-language row.** Each row shows: name · where it runs ·
  up to two capability chips (*Sees images*, *Chat only*) · cost
  (*Free · private · offline* for local/own-server; `$in / $out per 1M` from
  `cloud::pricing`, or *Free tier*) · fit badge (local only). Quant, file
  size, context length, cfg/steps sit under **Details ▾**. Actions: **Use**
  (selects it and returns to chat, as today), **Make default** (for the tab's
  modality), ★, and ⋯ (local: Delete; cloud: Copy model id).
- **`MOD-6` filter + search.** Chips *All / On this PC / Cloud* share the
  picker's `modelFilter` store value, so choosing "Local only" in one place
  holds in both. Search filters name + provider + meta across all groups.
  Arriving from a Providers card applies a provider filter chip (*Via
  OpenRouter ×*).
- **`MOD-7` Add models: three equal ways in.** Three door cards under the
  groups, one per way a model gets into Poiesis. Clicking one expands its flow
  below (only one open at a time).
  1. **Recommended for this PC.** One-line hardware summary (the full panel
     moves to Runtime) plus the existing curated catalog with fit badges.
     Chat tab: GGUF catalog. Images tab: diffusion catalog.
  2. **From Hugging Face or a link.** Today's `addByRepo` flow, promoted from
     the bottom of the page. One field that accepts a Hugging Face repo
     (`bartowski/Qwen2.5-7B-Instruct-GGUF`), a full `huggingface.co/…`
     URL, a GitHub `owner/repo`, or a direct `.gguf` link. It detects which
     one it got and says so under the field (*"Hugging Face repo · 14
     files"*). A repo opens the existing quant slider, re-labelled in plain
     words: *Smaller · faster ↔ Larger · better answers*, with the fit badge
     live as it moves. A direct link skips straight to download. On the Images
     tab the same door takes `.safetensors` / `.gguf` / `.ckpt` links, and
     "point at my own file" moves in here from the current *Advanced*
     toggle. Errors are specific: *"No GGUF files in that repo — it may
     hold the original weights. Look for a repo ending in -GGUF."*
     Finished downloads land in *On this PC* with a brief highlight and an
     offer: *"Added. Star it?"*
  3. **Connect an account.** The not-yet-connected providers as small buttons
     that jump to that card on Providers.

  When the local runtime isn't installed yet, doors 1 and 2 carry one line:
  *"Your first download also sets up the local runtime (≈60 MB)."* There's no
  separate install step in the everyday path.
- **`MOD-8` empty and first-run.** Keep today's "Get started in one step"
  when nothing exists, but with a second, equal option beside it:
  *"Already have an OpenAI, Anthropic or OpenRouter account? Connect it →"*.
  This is the first place a cloud-first user is served at all.
- **`MOD-9` recall stays out.** Embedders aren't something a user "uses"
  in chat. Recall quality stays in General (Simple) and Runtime → Recall
  (Pro).

## Part D — Runtime page content (`RTM-6..9`)

Keeps its technical depth; mainly deduplication and clearer framing.

- **`RTM-6`** Hardware panel lives here only (removed from Models, replaced
  there by the one-liner).
- **`RTM-7`** Status card: "Model" row → **Loaded model**; "Endpoint" →
  **Address** (`127.0.0.1:port · this PC only`); "Structured tool output"
  stays, with a tooltip. Add a link *"Choose which model → Models"*.
- **`RTM-8`** Tabs *Chat · Images · Your servers · Recall*. Recall stays
  expert-only, as today (`SMP-1c`). *Your servers* is visible to everyone.
- **`RTM-9` visibility.** Runtime is a normal nav item for everyone, placed
  after Providers. It gets no *Pro* tag and no expert gate. It's also where
  "Couldn't keep the runtime alive" (`HEAL-1`) sends people, so it must
  always be reachable.
- **`RTM-10` Your servers.** `LocalEndpointsSettings` moves here from
  General, unchanged in behaviour (presets, test, edit, on/off, remove,
  optional key under Advanced). Only the framing changes. Title *"Your own
  servers"*. Lede: *"Already running a model server such as Ollama or LM
  Studio? Point Poiesis at it and its models appear under Models. Nothing is
  downloaded twice, and nothing leaves your machine."* Each row gets a
  *"See its models →"* link that opens Models filtered to that server. Deep
  link: `runtime` view + `tab=servers`, used by the picker (`PRV-6`) and
  Models group heads (`MOD-1`).

## Part E — Tests & verification

- Routing test: `providers` and `runtime` render; `engine` is gone. Runtime
  is in the nav with expert mode off. The `servers` tab deep link opens Your
  servers.
- General no longer renders the key or server blocks, only the pointer.
- Defaults: there's no video default. *Make default* doesn't appear on video
  rows.
- `list_providers` includes a `Credential::Media` backend row (seam test,
  like `BKD-2`'s).
- `verify_provider_key` does not save on 401 (mock server).
- `reconcileSelection` picks `default_model.chat` even when it's a cloud id.
  When the default is unavailable it takes the next *available* favorite in
  order and raises the one-time notice.
- Favorites: order persists and drives the picker's first group. Making a
  model default moves it to slot 1. Unstarring the default is refused.
  `CLOUD_LIMIT` never truncates favorites.
- Add-by-link input classifier: HF repo id, `huggingface.co` URL, GitHub
  repo, direct `.gguf`, and junk each map to the right flow and message.
- Manual: fresh install with no model → Models shows both first-run doors;
  connect OpenRouter → "See models" lands on Models filtered, star two,
  picker shows them first; remove key → default shows unavailable.

## Order

1. **A** (rename). Mechanical, and clears the ground.
2. **B** (Providers). Moves the key UI and adds verification; nothing else
   depends on the joint view yet.
3. **C** (Models joint view). The biggest part. Within it, `MOD-3` (defaults)
   and `MOD-4` (favorites) go first: they're the settings-backed pieces, and
   the picker change can ship before the new Models page does. `MOD-7`'s
   Hugging Face / link door is mostly relocation of working code, so it's
   cheap.
4. **D** (Runtime cleanup), after C removes the duplicated hardware panel.

## Decisions (2026-09-18)

1. **Runtime visibility.** Shown to everyone, with no *Pro* tag and no expert
   gate (`RTM-9`).
2. **Own servers.** They live under Runtime → Your servers, not Providers
   (`RTM-10`).
3. **Header readout.** Stays as it is. Only "engine" becomes "runtime" in its
   wording, and onboarding gets the same treatment (`RTM-4`).
4. **Video.** No video default. Chat and image defaults only (`MOD-3`).

## Build notes (2026-09-18)

Where the build differs from the tasks above:

- **`PRV-5` spend is a header total only.** "Spent this month" on Providers
  adds media spend (`CST-2`) and priced cloud chat usage, and links to Usage.
  Per-card spend needs the provider on each `run_usage` row, which isn't
  recorded (only the model name is). Adding a `provider` column is the
  follow-up.
- **`PRV-4` "Needs attention" comes from chat turns only.** `drive_turn`
  records 401/402/403 per provider (kept in memory, cleared by the next
  success or a new key). Media generation failures don't set it yet.
- **`PRV-4` disconnect confirm** doesn't count chats per provider (there's no
  per-chat model record to count). It names the effect instead, and warns when
  the default model is from that provider.
- **`PRV-3` media-only keys** go through `MediaBackend::verify_key`, which
  accepts by default. No `Credential::Media` backend exists yet; the first one
  should override it.
- **Local image files.** The local backend serves one active checkpoint, so
  *Make default* on a local image file also makes it the active checkpoint.
  Starring a local file that isn't active is allowed, but as an image fallback
  it draws with the active checkpoint.
- **Image fallback** (`default_model.image`, then `models.favorites.media`) is
  applied in `media::jobs::submit` when a request names no model.
- **`verify_provider_key` test** covers the status → message mapping
  (`verify_status`) rather than a mock HTTP server; saving only happens after
  `Ok` in `verify_provider_key_cmd`.
- **Fit labels** are "Runs great", "Runs slowly", "Too big for this PC".
  "Runs OK" would overstate what `slow` means.
