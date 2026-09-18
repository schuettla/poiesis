# Project Poiesis — Multimodal Plan (Phase 13): making, seeing, and keeping media

> **STATUS: built, except `SEE-1` and `BKD-3`.** See
> [Status — as built](#status--as-built) for the per-task table, the places the
> implementation deliberately diverged from what is written below, and what is
> known-unverified. The task descriptions in Parts I–VII are kept as originally
> written — they are the intent, not a description of the code. Where the two
> disagree, a **Built** note under the task heading says what actually shipped.

Poiesis can already *look* at an image you hand it (`CHT-5`) and it can *make*
one on your GPU (`9F`). Neither of those is the thing a user actually wants.
What they want is the thing ChatGPT does without ever naming it: **you say what
you want, the picture arrives in the conversation, you say "make it warmer",
and a better picture arrives underneath.** No mode. No dropdown. No separate
application inside the composer.

Three things stop Poiesis doing that today, and they are independent:

1. **The quality ceiling is structural.** `imagegen::generate()` shells out to
   `stable-diffusion.cpp`, so the reachable model set is exactly what a single
   self-contained `.safetensors` checkpoint can be — SD 1.5 and SDXL, which is
   2023–2024 state of the art. The catalog comment at
   `commands/imagegen.rs:212-218` documents this honestly: Flux and SD 3.5 are
   excluded because they ship as 3–4 gated files. No amount of prompt work
   closes that gap. A provider seam does.
2. **Creation is modal.** `imageMode` (`lib/store.ts:170`) turns the composer
   into a different product — it hides the model picker, the context chip and
   personas (`Composer.tsx:581-584`) and routes to a command that the agent
   never learns about. The user leaves the conversation to make a picture and
   comes back holding one.
3. **What gets made doesn't get kept.** The two creation paths disagree about
   what an image *is*, and each one has exactly the half the other is missing.

That third point is the one to state plainly, because it is a data-loss bug
wearing the clothes of a design gap:

| | inline in the message | artifact row (Workbench) | Library |
|---|---|---|---|
| `generate_image` tool (`agent/imagegen.rs:109-114`) | ✗ — only a chip | ✓ | ✓ |
| composer *Create image* (`commands/imagegen.rs:351`) | ✓ | **✗** | **✗** |

`generate_image_cmd` calls `log_activity` and returns a path. It never calls
`add_artifact`. So every image a user makes through the composer — the
*primary* path, per `TASKS.md:204` — exists only as a row in the `attachments`
table pinned to one message. It cannot be saved to the working folder, it never
appears in Library, and if that conversation is deleted the PNG is orphaned on
disk forever. Meanwhile the tool path produces a proper artifact but shows the
user a chip instead of their picture.

This phase makes those three fixes one change, because they share a spine: a
**media backend seam** with a **single artifact-shaped result** that the
**message stream renders inline**. Video (`/api/v1/videos`) rides the same
spine and costs one extra variant rather than a second subsystem.

> Companions: `plans/POIESIS_PLAN.md` (the self and its loop) ·
> `plans/PERCEPTION_PLAN.md` (Phase 12 — `RND` tool-emitted renders and the
> deferred `VIS` vision half land next to `SEE-*` here) ·
> `plans/CAPABILITIES_PLAN.md` (Agent Skills format, provider reach) ·
> `plans/FILESYSTEM_PLAN.md` (working folders, trust, undo). Checklist:
> `plans/TASKS.md`.
>
> ID prefixes — **BKD** backend seam and its registry · **ORI** OpenRouter
> images · **OAI** OpenAI BYOK images · **VID** video · **JOB** async media
> jobs · **ART** artifacts & Library · **STR** the message stream · **PIK**
> the model chooser and intent routing · **EDT** editing and references ·
> **SEE** the agent sees its output · **CST** cost, consent, privacy · **FIX**
> defects found while surveying. `-UI-` tasks are frontend.
>
> **Build order:** `FIX-*` (any time) → **BKD-1 → BKD-2 → ART-1 → ART-2 →
> STR-1 → STR-2** (this run is the spine; nothing else is worth doing before
> it) → ORI-1 → ORI-2 → **PIK-1 → PIK-2** → CST-1 → PIK-3 → EDT-1 → EDT-2 →
> STR-3 → JOB-1 → VID-1 → SEE-1 → OAI-1 → ART-3. `BKD-3` (fal.ai) is the
> proof that the registry works and can land any time after `ORI-2`.
>
> **The spine ships alone and is already worth it.** After `STR-2` a
> locally-generated image is an artifact, appears in Library, and renders in
> the message stream with actions — with no new provider and no new key. Every
> later part is additive. Do not bundle them.
>
> Per `SMP-1` (Simple/Everything switch): all tasks here are **Simple** mode
> except `PIK-4`'s advanced-parameter disclosure, `CST-2`, and the Models →
> Image/Video engine management, which are **Everything**.

---

## Status — as built

Every task except `SEE-1` and `BKD-3` is implemented. Verification at the last
pass: `cargo test --lib` 286 passed, `cargo clippy --lib --all-targets` clean on
every touched file, `tsc --noEmit` clean, `vitest` 52 passed, `npm run build`
succeeds.

| Task | Status | Note |
|---|---|---|
| `FIX-1` char-safe truncation | ✅ | `media::ellipsize`, 3 unit tests |
| `FIX-2` orphaned media cleanup | ⚠️ partial | Conversation delete only — see below |
| `FIX-3` path-backed artifact kinds | ✅ | |
| `BKD-1` backend trait | ✅ | Trait shape diverged — see below |
| `BKD-2` registry + descriptor | ⚠️ partial | Helpers + seam test done; `Credential::Media` UI not built |
| `BKD-3` fal.ai backend | ❌ **not built** | Deferred by request |
| `ORI-1` OpenRouter images | ✅ | |
| `ORI-2` image model discovery | ⚠️ diverged | Discovery lives in `list_media_models_cmd`, not `list_image_models_cmd` |
| `OAI-1` OpenAI BYOK images | ✅ | |
| `VID-1` OpenRouter video | ⚠️ partial | Generation works; video-model *discovery* is curated only |
| `JOB-1` background jobs | ✅ | Delivery is an app event, not a run wake-up — see below |
| `ART-1` metadata + lineage | ✅ | schema v15 |
| `ART-2` **the fix** | ✅ | |
| `ART-3` Library shows media | ✅ | |
| `STR-1` one presentation | ✅ | |
| `STR-2` the media block | ✅ | Placeholder, elapsed, Cancel, Refine, metadata line all present |
| `STR-3` attachment thumbnails | ✅ | |
| `STR-4` progressive reveal | ⚠️ unverified | Implemented behind two guards — see below |
| `PIK-1` picker category | ✅ | |
| `PIK-2` composer retargeting | ✅ | All four guards |
| `PIK-3` inference | ⚠️ diverged | Regex intentionally differs from the spec below |
| `PIK-4` advanced parameters | ✅ | Everything mode only |
| `EDT-1` references | ✅ | Incl. `reference_role` |
| `EDT-2` implicit reference | ✅ | |
| `SEE-1` agent sees its output | ❌ **not built** | Deferred by request; also unverifiable — see below |
| `CST-1` consent | ✅ | |
| `CST-2` spend visibility | ✅ | |

Schema is at **v17**: `artifacts.meta_json` + `parent_id` (v15),
`attachments.artifact_id` (v16), `media_jobs` table (v17).

### Where the build diverged, and why

**`BKD-1` — the trait is descriptor-first, and carries more state.** The
signature below (`id()` / `supports()` / `supports_references()`) was replaced
by `descriptor() -> &'static BackendDescriptor` from `BKD-2`, since keeping both
meant two sources of truth about the same backend. `generate` also takes `&Db`
and a `&CancelFlag` (`JOB-1` needs the second; the local backend needs the
first to read its configured checkpoint). `MediaRequest` gained `model`,
`modality`, `width`/`height` and `job_id` — the first two because a
multi-modality backend must be told which endpoint the caller meant rather
than inferring it, `width`/`height` because the local backend wants literal
pixels, `job_id` for `STR-4`.

**`MediaError::NoBackend` is a `String`, not a typed error.** Every backend
error is already a sentence the UI shows verbatim, so a typed variant would
have carried the same string with more ceremony. Revisit if a caller ever needs
to *branch* on the reason rather than display it.

**Backend availability needed a second predicate.** `Credential::Local` is
always "satisfied", which meant the local backend reported itself available on
a machine with no engine installed — and since it is first in `Registry::new()`,
it won every `resolve_backend` call and a user with a valid OpenRouter key got
*"the image engine isn't installed"*. `MediaBackend::is_ready(&Db)` (default
`true`, overridden by the local backend to require binary **and** checkpoint on
disk) is what fixed it. Two tests pin the behaviour.

**`resolve_backend`'s precedence chain is fully built** —
`media.primary_image` / `media.primary_video` → `media.fallbacks` → first
available. A preference naming a missing backend is skipped rather than
erroring, which is the point of a fallback list. There is **no Settings UI**
for these keys yet; they are settable but not surfaced.

**`BKD-2`'s cache is hosted-only, not a blanket 6h TTL.** Only the HTTP
catalogs are cached (process-wide, 6h, invalidated when a cloud key is added or
removed). The local backend's list is a file-exists check and is always read
live — so installing an engine or picking a checkpoint shows up immediately
instead of waiting out a TTL.

**`BKD-2` step 3 is not true yet.** The shared helpers
(`normalize_aspect_ratio`, `nearest_supported`, `poll_until_done`,
`materialize`, `probe_dimensions`, `data_uri_for`, `mime_for_path`) exist and
both cloud backends are written *using* them, and the `TestBackend` acceptance
test passes with one line of registration. But `Credential::Media` is still
dead: Settings → Cloud does **not** generate a key row from a descriptor. A
`Credential::Media` backend would therefore have nowhere to paste its key.
**That is the remaining work `BKD-3` depends on** — fal.rs itself is small now
that the helpers exist.

**`ORI-2` landed in a different command than planned.** Cloud model discovery
lives in `list_media_models_cmd` (→ `Registry::all_models`), feeding the model
chooser via `PIK-1`. `list_image_models_cmd` was left alone and still returns
only local checkpoint files — it is the *engine management* list, and merging
two different questions into it would have made the picker depend on it.

**`VID-1` has no model discovery.** Generation, polling, cancellation and the
10-minute ceiling are built and used by the shared `poll_until_done`. But
`GET /api/v1/videos/models` is **not** called — video models come from the
curated fallback list, so the picker cannot yet promise that an offered
resolution/duration combination is supported by that specific model. The plan's
own "verify at implementation time" note was never discharged: no live account
was available.

**`JOB-1` delivers by app event, not by waking the run.** A completion emits
`poiesis-media-job` app-wide and the store applies it to the turn recorded in
the job row, because by the time a 90s video finishes the agent run that asked
for it has almost always ended — there is no channel left to wake. Consequences
worth knowing: the model does **not** see its own result later in the same run
(that is `SEE-1`), and progress does not flow through `AgentEventSink` as the
task text suggests. Restart safety, the `message_id` anchor, re-attaching to
running jobs on reload, and single-outcome-per-job are all built and tested.

**Cancellation is honest about its boundaries.** The job row closes immediately
on request, so the turn ends at once. A `stable-diffusion.cpp` subprocess or a
single blocking image POST already in flight finishes its work and the bytes
are deleted — cancel is enforced before a call starts and between polls, not
mid-request.

**`PIK-3`'s regex deliberately differs from the spec below.** Rule 1 as written
requires an object noun adjacent to the verb (`image|picture|photo|…`), but the
plan's own headline example — *"draw a fox reading a map in a pine forest"* —
contains no such noun, and the stated acceptance test is that typing "draw a
fox" shows the chip. Those two cannot both hold. The implementation keeps the
`^`-anchored leading verb and drops the noun requirement, which is what actually
separates *"draw a fox"* from *"how do I draw a fox in Illustrator?"*. Both of
the plan's acceptance cases are asserted in `mediaIntent.test.ts`.

**`FIX-2` is only half-reachable.** Conversation deletion sweeps generated files
under `generated_media_dir()`, guarded by the existing "still referenced"
check. The artifact half is unreachable: there is **no artifact-delete command
anywhere in the codebase**, so there is no call site to wire. Add the sweep when
artifact deletion is built.

**`STR-2`'s CSS uses this codebase's tokens, not the literal values here.**
`border-radius: var(--radius-lg)` rather than `12px`, `var(--fs-timeline)`
rather than `11px`, `var(--paper-edge)` rather than `--surface-2` — the plan was
written against token names this project does not use. Behaviour matches the
spec; the numbers come from `styles/tokens.css`.

### Known-unverified

**`STR-4` has never been seen to work.** OpenRouter's SSE frame shape and its
`supports_streaming` catalog field could not be checked against a live account.
It is implemented behind two guards so a wrong guess degrades rather than
breaks: it streams *only* when the provider's own catalog reports
`supports_streaming` (a cold cache reports nothing, which means don't), and if
the stream yields no final image it falls back to one plain POST. Worst case is
one extra request.

**`VID-1`'s endpoint shapes are equally unverified** — `polling_url`,
`unsigned_urls[0]`, `usage.cost` all come from the plan text rather than from a
response anyone has seen. Same for the curated model slugs in both cloud
backends, which exist so the picker is never empty with a key present.

### Not built

**`SEE-1`.** Beyond being deferred, it is the one task here that cannot be
written safely without a live key: array/`image_url` content on a `role: "tool"`
message is not reliably supported across providers, and a wrong guess would
silently break *every* image tool call rather than just failing the new feature.
It needs one real API round trip to settle the shape, not more code.

**`BKD-3`.** Deferred. Blocked on the `Credential::Media` Settings row noted
above; fal.rs itself is now a small file.

---

## Part 0 — The user paths, decided up front

Every task below serves one of these five. If a task cannot be traced to a
step here, it is out of scope.

**Two routes reach the same place.** There is a *declared* route (pick an image
or video model in the model chooser — explicit, sticky, and the one a user
reaches for when they know what they want) and an *inferred* route (just type
"draw a fox" while a chat model is selected). Both resolve to the same
`mediaTarget` state, and from there the paths below are identical. The declared
route always wins over inference; inference never overrides a declaration.

### Path A — creating an image (the ChatGPT path, inferred route)

1. User types **"draw a fox reading a map in a pine forest"** into the ordinary
   composer. **No mode is engaged. No dropdown is touched.**
2. The message posts as a normal user turn. An agent turn appears immediately
   with a header (`provenance-dot` + model tag reading e.g. `Nano Banana Pro`)
   and a **placeholder tile at the target aspect ratio** — not a spinner, a
   grey rounded rectangle the exact size the image will be, so the transcript
   does not jump when it resolves.
3. A single timeline step reads **"generating image"** while it runs, then
   **"generated an image"** (past tense, per §5.6).
4. The image replaces the placeholder **in place**, full message width, rounded
   corners, at its natural aspect ratio.
5. Under the image, a quiet action row: **Refine · Variation · Save · ⤓ · ↗**.
6. Simultaneously and with no user action: the image is an **artifact**. It is
   in the Workbench *Artifacts* tab, and it is in **Library** with a thumbnail.
   The Workbench does *not* steal focus for it (see `ART-2` — this is a
   deliberate exception to `useFollowTheAgent`, because the image is already
   on screen in the stream; switching panels would be redundant motion).

### Path B — refining what was just made

1. User types **"make it warmer, and lose the map"** as the very next message.
2. No attachment ceremony. The turn carries the previous image as an implicit
   reference (`EDT-2`), and the composer shows a small **reference thumbnail
   chip** above the input so the implicit is made visible before sending.
3. A new agent turn produces a new image **below** the old one. The old one is
   not replaced and not deleted — iteration is a stream, not a mutation.
4. Both are artifacts. Library shows both, the newer one carrying a
   `↳ refined from` link to its parent (`ART-1`'s `parent_id`).

### Path C — editing an image the user brought

1. User drags `photo.jpg` onto the composer and types **"remove the background
   and make it a sticker"**.
2. The attachment chip renders as a **thumbnail**, not a filename
   (`STR-3` replaces the `▣ name` text in `UserTurn.tsx:22-30`).
3. Because the draft carries an image *and* imperative edit language, the
   intent router (`PIK-3`) resolves to image editing rather than vision Q&A,
   and shows the user which it chose — with one click to switch.
4. Result behaves exactly like Path A step 4–6.

### Path D — creating a video

Identical to Path A, except: the placeholder tile shows an elapsed-time counter
(video takes 30–180s), the result renders as a `<video>` element with native
controls, poster frame, and `loop muted playsinline`, and the action row reads
**Variation · Save · ⤓ · ↗** (no Refine — no OpenRouter video model supports
edit-in-place at time of writing; re-verify at `VID-1`).

### Path E — declaring intent through the model chooser

This is the route for a user who already knows they want pictures, and it is
the reason the model chooser is a *first-class* entry point rather than a
setting buried in the intent chip.

1. User opens the model picker in the composer footer — the same control they
   already use to switch between a local Qwen and Claude.
2. Below **On this device** and **Cloud · your key** there is a third group,
   **Images & video**, with its own `provenance-dot` styling and each row
   showing modality, provider and price:
   `◈ Nano Banana Pro · image · OpenRouter · $0.04`
   `◈ Veo 3.1 · video · OpenRouter · $0.25/s`
   `◈ SDXL-Turbo · image · on this device · free`
3. User picks **Nano Banana Pro**. The picker closes. Three things change and
   nothing else does:
   - the trigger now reads `◈ Nano Banana Pro`,
   - the composer placeholder becomes **"Describe an image…"**,
   - a persistent target bar sits above the input:
     `◈ Image · Nano Banana Pro · 1:1 ▾            ← Back to chat`.
4. Every message sent now generates an image. The user types **"a fox reading
   a map"** — no verb needed, because they already declared the intent. This
   is the part inference cannot give them: once declared, prompts get to be
   pure description instead of instructions.
5. Results render exactly as Path A steps 3–6.
6. **Back to chat** (or picking any chat model) restores the previously
   selected chat model — one click, and the app remembers which one it was.

The selection is **sticky across messages but not across conversations**: a new
chat starts on the last chat model, because "make me a picture" is a session,
not a personality. Switching conversations while a media model is selected
restores that conversation's chat model.

**What is deliberately not built here:** in-canvas masking / inpainting
(select-a-region-and-describe). It needs a canvas editor and a mask upload
path, and every provider spells masks differently. `EDT-1` carries whole-image
references only. Revisit once the five paths above are solid.

---

## Part I — The backend seam

### BKD-1 — one media backend trait

> **Built ✅** — `src-tauri/src/media/mod.rs`, `backends/{mod,local,openrouter,openai}.rs`.
> The trait is descriptor-first and `generate` takes `&Db` + `&CancelFlag`; the
> selection chain is fully implemented but needed `is_ready` to work. See
> [Where the build diverged](#where-the-build-diverged-and-why).

**Files:** new `src-tauri/src/media/mod.rs`, `media/backends/mod.rs`,
`media/backends/local.rs`; `src-tauri/src/lib.rs` (`pub mod media;`).
Every provider added later is one more file under `media/backends/` and
nothing else — see `BKD-2`, which is what makes that true.

Move `agent::imagegen::generate()` behind a provider-agnostic request/response
pair. Keep `agent/imagegen.rs` as the *toolset* (specs, `handles`, `describe`,
`execute`) and let it call into `media::`.

```rust
// src-tauri/src/media/mod.rs
pub enum Modality { Image, Video }

pub struct MediaRequest {
    pub modality: Modality,
    pub prompt: String,
    pub negative: Option<String>,
    /// Normalised: "1:1" | "16:9" | "9:16" | "4:3" | "3:4" | "21:9".
    pub aspect_ratio: Option<String>,
    /// Normalised tier: "512" | "1K" | "2K" | "4K" (image) / "480p".."4K" (video).
    pub resolution: Option<String>,
    /// Local-only knob; ignored (and reported) by hosted backends.
    pub steps: Option<i64>,
    pub seed: Option<i64>,
    /// Whole-image guidance. Local backend rejects a non-empty vec today.
    pub references: Vec<MediaRef>,
    /// Video only.
    pub duration_secs: Option<u32>,
}

pub struct MediaRef { pub path: PathBuf, pub role: RefRole } // Source | Style

pub struct MediaResult {
    /// Always a real file under `generated_media_dir()`. Hosted providers are
    /// materialised here before returning — provider URLs expire (Hermes ships
    /// a local cache for exactly this reason) and Library must survive that.
    pub path: PathBuf,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_secs: Option<f32>,
    pub model_id: String,     // "local:sd_xl_turbo…" | "openrouter:google/…"
    pub provider_label: String, // "SDXL-Turbo" | "Nano Banana Pro"
    pub seed: Option<i64>,
    pub cost_usd: Option<f64>,
    /// Hints the backend could not honour, echoed for honest UI
    /// (OpenClaw's `ignoredOverrides` — normalise, report, never fail).
    pub ignored: Vec<String>,
}

#[async_trait]
pub trait MediaBackend: Send + Sync {
    fn id(&self) -> &'static str;
    fn supports(&self, m: Modality) -> bool;
    fn supports_references(&self) -> bool;
    async fn generate(&self, req: &MediaRequest, out_dir: &Path) -> Result<MediaResult, String>;
}
```

**Selection** follows OpenClaw's precedence chain, in
`media::resolve_backend(db, modality, explicit: Option<&str>)`:
explicit `model` argument → `media.primary_image` / `media.primary_video`
setting → `media.fallbacks` (comma-separated ids) → any backend whose
credentials/binary are present → a typed `MediaError::NoBackend` carrying a
sentence the UI shows verbatim.

**Model ids are provider-agnostic strings**, `media:<backend_id>/<model_slug>`
— `media:openrouter/google/veo-3.1`, `media:fal/fal-ai/flux-2-pro`,
`media:local/sd_xl_turbo_1.0_fp16.safetensors`. `resolve_backend` splits on the
first `/` after the prefix. Nothing downstream — settings, artifact
`meta_json`, the picker, `parent_id` lineage — needs to know the set of
backends, so adding one never triggers a migration.

### BKD-2 — the registry: adding a provider is one file and one line

> **Built ⚠️ partial** — helpers, registry, hosted-only 6h cache and the
> `TestBackend` one-line-registration test are all in. **Step 3 is not true
> yet:** Settings → Cloud does not generate a key row from a
> `Credential::Media` descriptor, so a media-only provider has nowhere to put
> its key. This is the blocker for `BKD-3`.

This is a first-class requirement, not a refactor. fal.ai (what Hermes uses),
Replicate, Gemini direct, a local ComfyUI, and whatever ships next quarter must
each be *one new file*. Anything that makes a new backend touch the picker, the
composer, the DB, or the agent loop is a design bug.

**Files:** `src-tauri/src/media/mod.rs` (registry + descriptor),
`src-tauri/src/media/backends/` (one module per provider).

The trait gains a **static descriptor** so the whole app can reason about a
backend without knowing which one it is:

```rust
pub struct BackendDescriptor {
    pub id: &'static str,              // "openrouter" | "fal" | "local" | …
    pub label: &'static str,           // "OpenRouter"
    pub modalities: &'static [Modality],
    pub credential: Credential,
    pub supports_references: bool,
    pub supports_edit: bool,
    pub is_async: bool,                // submit + poll, vs one blocking call
    pub console_url: Option<&'static str>, // where to get a key
}

pub enum Credential {
    /// Reuses an existing chat-provider key (OpenRouter, OpenAI).
    Cloud(crate::cloud::Provider),
    /// Media-only provider with its own key: keyring SERVICE_MEDIA / id.
    Media { key_hint: &'static str },
    /// No key — a local binary or a self-hosted endpoint.
    Local,
}

#[async_trait]
pub trait MediaBackend: Send + Sync {
    fn descriptor(&self) -> &'static BackendDescriptor;
    /// Models this backend can offer right now. Hosted backends hit their
    /// catalog endpoint; the local one scans the diffusion dir. Cached by the
    /// registry, never by the implementation.
    async fn list_models(&self) -> Result<Vec<MediaModel>, String>;
    async fn generate(&self, req: &MediaRequest, out_dir: &Path) -> Result<MediaResult, String>;
}
```

`MediaModel` is the one shape the UI ever sees:

```rust
pub struct MediaModel {
    pub id: String,                 // "media:fal/fal-ai/flux-2-pro"
    pub name: String,               // "FLUX 2 Pro"
    pub backend_id: String,
    pub backend_label: String,      // shown as the provider in the picker
    pub modality: Modality,
    pub price_label: Option<String>,// "$0.04" | "$0.25/s" | None = free/local
    pub supports_edit: bool,
    pub supported_aspect_ratios: Vec<String>,
    pub supported_resolutions: Vec<String>,
    pub max_duration_secs: Option<u32>,
}
```

**The registry** owns construction, credential checks and caching:

```rust
pub struct Registry { backends: Vec<Box<dyn MediaBackend>> }

impl Registry {
    pub fn new() -> Self { /* the one list — see below */ }
    pub fn available(&self, db: &Db) -> Vec<&dyn MediaBackend>; // credential present
    pub fn get(&self, backend_id: &str) -> Option<&dyn MediaBackend>;
    /// Union of every available backend's models, 6h TTL, one lock.
    pub async fn all_models(&self, db: &Db, modality: Option<Modality>) -> Vec<MediaModel>;
}
```

**Adding a provider is exactly this checklist** (write it into the module doc
comment of `media/backends/mod.rs`, because a checklist that lives outside the
code rots):

1. Create `media/backends/<name>.rs` with a `BackendDescriptor` const, a
   `list_models`, and a `generate`.
2. Add one line to `Registry::new()`.
3. If `Credential::Media`, nothing else — the Settings key row, the picker
   group, the "add a key" empty state and the consent copy are all generated
   from the descriptor.

**Shared helpers live in `media/mod.rs`, not in each backend**, or step 1 stops
being small: `normalize_aspect_ratio`, `nearest_supported`, `poll_until_done`
(interval, ceiling, `CancelFlag`), `materialize(url | b64) -> PathBuf`,
`probe_dimensions`. `ORI-1` and `VID-1` must be written *using* these, so the
second backend is a genuine test of whether they are the right helpers.

**Credentials for media-only providers.** fal.ai is not a chat provider, so it
has no home in `cloud::Provider`. Add `secrets::SERVICE_MEDIA` alongside
`SERVICE_CLOUD` (same keyring pattern, `secrets.rs`), keyed by `backend_id`.
Settings → Cloud renders a key row per `Credential::Media` descriptor, reusing
the existing provider-key component with `key_hint` and `console_url` from the
descriptor — no per-provider UI code.

**Acceptance:** a `TestBackend` in `#[cfg(test)]` registered with one line
appears in `all_models`, is selectable, and generates — with no change to any
file outside `media/`. That test is the guarantee; if it needs a fourth edit
somewhere, the seam is wrong.

### BKD-3 — fal.ai backend (the proof)

> **Not built ❌** — deferred by request. The `generated_images_dir()` →
> `generated_media_dir()` rename and the local-backend notes in this section
> *were* done (they belong to `BKD-1`/`BKD-2` in practice).

**File:** `src-tauri/src/media/backends/fal.rs`. `Credential::Media` with hint
`"Starts with a key id, then a colon"`, console `https://fal.ai/dashboard/keys`.
Submit to `https://queue.fal.run/<model>` → `{ request_id, status_url,
response_url }`; poll with `poll_until_done`; the completed payload carries
`images[].url` / `video.url` → `materialize`. Model list is curated per fal's
catalog (FLUX 2, Z-Image Turbo, Krea, Recraft…) since fal has no single
enumeration endpoint.

Its value is not fal itself — it is that writing it must cost one file. **If it
doesn't, fix `BKD-2` before shipping either.**

`media/backends/local.rs` is today's sd.cpp code moved verbatim, plus: it reports
`ignored: ["aspect_ratio"]` when asked for a ratio it maps to the nearest
supported `-W`/`-H`, and returns a clear error for non-empty `references`.

**New directory.** Rename `RuntimeManager::generated_images_dir()` →
`generated_media_dir()` (`runtime/manager.rs:62`), keeping a deprecated alias
for one release so nothing dangles. Video and image share it.

**Acceptance:** existing local generation works unchanged through the trait;
`cargo test` green; `resolve_backend` with no key and no binary returns the
`NoBackend` sentence, not a panic.

### ORI-1 — OpenRouter image backend

> **Built ✅** — `media/backends/openrouter.rs`. Provider `error.message` is
> surfaced verbatim. Endpoint shapes remain unverified against a live account.

**File:** `src-tauri/src/media/backends/openrouter.rs`. Key comes from the existing
keychain entry (`secrets::SERVICE_CLOUD`, `Provider::OpenRouter`) — **no new
auth surface**. This is the whole reason this backend goes first.

`POST https://openrouter.ai/api/v1/images`:

```json
{ "model": "...", "prompt": "...", "n": 1, "aspect_ratio": "16:9",
  "resolution": "2K", "quality": "high", "output_format": "png",
  "seed": 12345,
  "input_references": [{ "type": "image_url", "image_url": { "url": "data:image/png;base64,…" } }] }
```

Response: `data[0].b64_json` + `data[0].media_type` + `usage.cost`. Decode and
write to `out_dir`, then fill `MediaResult`. Do **not** implement SSE partial
images in this task — `stream: false` only; `STR-2`'s placeholder covers the
wait, and progressive reveal is a `STR-4` nice-to-have.

**Acceptance:** with an OpenRouter key set, `generate` produces a PNG on disk,
a populated `cost_usd`, and a `model_id` of the form `openrouter:<slug>`. With
no key, `NoBackend`. On HTTP 4xx, the provider's `error.message` is surfaced,
not `reqwest`'s status line.

### ORI-2 — image model discovery, and the end of the hand-curated catalog

> **Built ⚠️ diverged** — discovery landed in `list_media_models_cmd`
> (→ `Registry::all_models`), which is what feeds the picker. `list_image_models_cmd`
> deliberately still returns local checkpoints only: it is the engine-management
> list, a different question.

**Files:** `src-tauri/src/media/backends/openrouter.rs`,
`src-tauri/src/commands/imagegen.rs`, `src/lib/api.ts`.

`GET /api/v1/images/models` → normalise into the existing `ImageModel` shape
extended with `provenance: "local" | "cloud"`, `provider: String`,
`price_per_image: Option<f64>`, `supports_edit: bool`,
`supported_aspect_ratios: Vec<String>`. `list_image_models_cmd` returns local
files *and* cloud models in one list, ordered local-first.

Keep `image_catalog()` — it is the download catalog for the *local* engine and
still correct for that job. It stops being the only answer to "what can I use".

**Acceptance:** `list_image_models_cmd` with a key returns ≥20 cloud entries
each carrying a price; without a key returns only local files. Cached 6h in
memory so opening the picker is not an HTTP round trip.

### OAI-1 — OpenAI BYOK image backend

> **Built ✅** — `media/backends/openai.rs`, incl. the multipart `/images/edits`
> path and ratio substitution reported through `MediaResult::ignored`.

**File:** `src-tauri/src/media/backends/openai.rs`. `POST /v1/images/generations`
(model, prompt, size, quality, n, `output_format`) and, when `references` is
non-empty, multipart `POST /v1/images/edits`. Key from `Provider::OpenAi`.
Curated model list (no useful discovery endpoint for image models).

Deliberately late in the order: it adds one provider the user may already
reach through `ORI-1`. It exists so a user with only an OpenAI key is not
locked out.

**Acceptance:** with only an OpenAI key set, Path A completes end-to-end.

### VID-1 — OpenRouter video backend

> **Built ⚠️ partial** — generation, polling (via the shared `poll_until_done`),
> cancellation and the 10-minute ceiling are in, and clips render and play in
> the stream. **`GET /api/v1/videos/models` is not called** — video models come
> from the curated list, so unsupported combinations can still be offered.

**File:** `src-tauri/src/media/backends/openrouter.rs` (same module, `Modality::Video`).

`POST /api/v1/videos` with `model`, `prompt`, `duration`, `resolution`,
`aspect_ratio`, and `frame_images` / `input_references` for image-to-video.
Returns `{ id, polling_url, status: "pending" }`. Poll `polling_url` until
`status == "completed"`, then fetch `unsigned_urls[0]` and write the MP4 into
`generated_media_dir()`. `usage.cost` → `cost_usd`.

Discovery via `GET /api/v1/videos/models`, which reports
`supported_resolutions`, `supported_aspect_ratios` and pricing per model —
feed those into the picker so an unsupported combination is never offered.

Polling: 2s interval, 3s after the first minute, hard ceiling **10 minutes**,
`Cancel` honoured through the existing `CancelFlag`. Never block the agent
turn — this task depends on `JOB-1`.

> **Verify at implementation time.** These shapes come from OpenRouter's public
> docs as of August 2026 and the video endpoint is newer than the image one.
> Confirm field names against `/api/v1/videos/models` output before wiring the
> UI to them.

**Acceptance:** a 5s 720p clip completes, lands on disk, becomes an artifact,
and plays in the stream. Cancelling mid-poll stops the poll and marks the turn
cancelled without leaving a half-file.

### JOB-1 — media generation is a background job, not a blocked turn

> **Built ✅** — `media/jobs.rs` + the `media_jobs` table (schema v17).
> Delivery is an app-level `poiesis-media-job` event rather than waking the run,
> and progress does not flow through `AgentEventSink`; see
> [Where the build diverged](#where-the-build-diverged-and-why). Restart safety,
> re-attach-on-reload and single-outcome-per-job are tested.

**Files:** new `src-tauri/src/media/jobs.rs`;
`src-tauri/src/agent/imagegen.rs` (tool `execute`).

Today the tool blocks with a 300s timeout (`agent/imagegen.rs:24`). Video makes
that untenable and cloud latency makes it rude. Adopt OpenClaw's pattern:
`execute` submits, records `{ id, conversation_id, message_id, modality,
status, started_at }`, returns a sentence with the job id immediately, and a
completion wakes the run to deliver the result.

Emit progress on the existing sink so `STR-2`'s placeholder can show elapsed
time and a cancel affordance. Restart safety: jobs still `pending` at startup
are marked `failed` with "interrupted by restart" — never left spinning.

**Acceptance:** a 90s video generation does not hold the agent loop; the user
can send another message meanwhile; the clip lands in the right turn.

---

## Part II — Everything made is an artifact

### ART-1 — artifacts carry media metadata and lineage

> **Built ✅** — schema v15.

**Files:** `src-tauri/src/db/mod.rs`, `src-tauri/src/db/schema.sql`.

Two additive columns via the existing `Self::add_column` migration pattern
(`db/mod.rs:468`):

- `artifacts.meta_json TEXT` — `{ model_id, provider_label, prompt, negative,
  seed, cost_usd, width, height, duration_secs, mime, aspect_ratio }`.
- `artifacts.parent_id TEXT` — the artifact this one was refined from
  (Path B step 4). Null for originals.

New signature (the old one stays, delegating with `None, None`, so the six
existing call sites in `agent/artifacts.rs`, `agent/present.rs` etc. are
untouched):

```rust
pub fn add_artifact_with(
    &self, conversation_id: Option<&str>, title: &str, kind: &str, content: &str,
    meta_json: Option<&str>, parent_id: Option<&str>,
) -> Result<Artifact, DbError>
```

Add `"video"` as a recognised `kind` (the column is free-text; this is a
convention, not a constraint).

**Acceptance:** a fresh DB and a v-N DB both migrate; `list_all_artifacts`
round-trips `meta_json`; existing artifact tests green.

### ART-2 — **the fix**: every generated image is an artifact, from both paths

> **Built ✅** — both paths converge on `media::record`. Since `JOB-1`, the
> artifact is written by the job worker, which also writes the `attachments`
> row that makes it survive a reload.

**Files:** `src-tauri/src/commands/imagegen.rs` (`generate_image_cmd`),
`src-tauri/src/agent/imagegen.rs` (`execute`), `src/lib/store.ts`
(`createImage`), `src/lib/api.ts`.

Both paths converge on one helper:

```rust
// src-tauri/src/media/mod.rs
pub fn record(db: &Db, conversation_id: Option<&str>, req: &MediaRequest,
              res: &MediaResult, parent_id: Option<&str>) -> Result<Artifact, DbError>
```

which titles the artifact from the prompt (**char-safe** — see `FIX-1`), sets
`kind` to `"image"` or `"video"`, `content` to the absolute path, and fills
`meta_json` from `MediaResult`.

- `generate_image_cmd` **must** call it and **must** return the whole
  `Artifact`, not a bare path string. This is the data-loss fix; everything in
  Part III depends on the composer path producing a real artifact.
- The tool path calls the same helper, dropping its inline `add_artifact`
  (`agent/imagegen.rs:109-114`).
- `store.ts:853` stores `artifact.id` on the assistant message's
  `artifactIds` **and** builds the `Attachment` from `artifact.content`, so the
  image is simultaneously in the stream and in the panel.
- `refreshAllArtifacts()` is called after both paths so Library is live without
  a view switch.

**The focus exception.** `useFollowTheAgent` (`Workbench.tsx:75-93`) switches
the panel to *Artifacts* whenever the count rises. For media that is wrong —
the image is already visible in the stream, so switching panels is motion that
teaches nothing. Pass the new artifact's `kind` into the hook and skip the
switch for `"image"` and `"video"`. Everything else keeps today's behaviour.

**Acceptance (this is the test that proves the phase):** create an image from
the composer with `imageMode` off, restart the app — the image is in Library,
has a thumbnail, has a Save button in the Workbench, and still renders in the
conversation. Today, three of those four fail.

### ART-3 — Library shows media as media

> **Built ✅** — asset protocol enabled in `tauri.conf.json` (scope
> `$APPDATA/generated-images/**`; the on-disk directory name was left unchanged)
> and the `protocol-asset` Cargo feature added.

**Files:** `src/routes/Library.tsx`, `src/routes/Library.css`,
`src/components/Workbench/artifactFiles.ts`,
`src/components/Workbench/Viewer.tsx`.

- `ArtifactPreview` gains a `kind === "video"` branch rendering a muted,
  `preload="metadata"` `<video>` with the poster frame — not a file icon.
- `extFor()` (`artifactFiles.ts:5`) already derives image extensions from the
  path; extend the same branch to `"video"` so **Save** writes `.mp4`.
- `Viewer.tsx`: `RenderKind` gains `"video"`; `kindForPath` moves `mp4`, `mov`,
  `webm`, `mkv` **out of `BINARY_EXT`** (`Viewer.tsx:14-18`) and into a new
  `VIDEO_EXT`. Right now a generated clip in a working folder renders as
  "binary".
- Library gains a filter row: **All · Images · Video · Documents**, and image
  cards show `provider_label` from `meta_json` in the meta line, next to the
  date. Knowing an image came from Nano Banana Pro vs local SDXL is the single
  most useful thing to know about it later.

**Media loading.** `readImageDataUri` base64s the whole file over IPC — fine
for a 1.5 MB PNG, unacceptable for a 20 MB MP4 and wasteful for a grid of
thumbnails. Video (and Library grid images) must use Tauri's asset protocol via
`convertFileSrc()`. That requires, in `src-tauri/tauri.conf.json`:

- `app.security.assetProtocol = { enable: true, scope: ["<APPDATA>/media/**"] }`
- CSP (`tauri.conf.json:27`) gains `media-src 'self' asset:
  http://asset.localhost blob:`. `img-src` already permits `asset:`.

Scope it to the generated-media directory only. The asset protocol is a read
primitive that bypasses `assert_ui_readable`, so it gets exactly the directory
Poiesis itself writes and nothing else.

**Acceptance:** a 20 MB clip plays in Library without a multi-second freeze;
DevTools shows no base64 data URI for it.

---

## Part III — The message stream

This is where "like ChatGPT" is either true or it isn't.

### STR-1 — one presentation, two execution paths

> **Built ✅** — both entry points now share one `startMediaTurn` helper in
> `store.ts`, which is a stronger guarantee than two code paths agreeing.

**Files:** `src/lib/store.ts` (`createImage`), `src/components/Conversation/*`.

`createImage` currently fabricates an assistant message whose model reads
literally `"Image"` with `provenance: "local"` (`store.ts:822`) and whose text
is `"Creating image…"`. That is a second, lesser rendering of an agent turn.

Delete the special case. `createImage` produces a **normal agent message**:
a real `model` (`provider_label` from the backend, `provenance` local or
cloud), a `steps` array with one step (`generating image` → `generated an
image`), and `attachments` + `artifactIds` on completion. The direct path
remains a direct path in *execution* — that pragmatism from `TASKS.md:204` is
correct and stays — but it is indistinguishable in *presentation* from the
agent calling the tool. That is the whole trick: the user never learns there
are two paths, because there is no way to tell.

**Acceptance:** a screenshot of a composer-generated image and a
tool-generated image are pixel-identical apart from the model name.

### STR-2 — the media block: placeholder, image, actions

> **Built ✅** — `ChatMedia.tsx` + `ChatMediaPending`. Placeholder holds the
> target ratio, elapsed counter at 3s, Cancel at 10s (real since `JOB-1`),
> Refine gated on the producing model's `supports_edit`, video branch via
> `convertFileSrc`. CSS uses this project's tokens rather than the literal
> values written above.

**Files:** `src/components/Conversation/ChatImage.tsx` → rename to
`ChatMedia.tsx`; `src/components/Conversation/AgentRun.tsx:130-131`;
`src/components/Conversation/Conversation.css`; `src/lib/types.ts`.

`Attachment.kind` becomes `"image" | "pdf" | "video"`. `Attachment` gains
`artifactId?: string`, `width?`, `height?`, `durationSecs?`.

**Precise rendering spec.**

*Placeholder* (while `message.streaming` and the attachment is pending):
a `<div class="chat-media-skeleton">` with `aspect-ratio` set from the
requested ratio (default `1/1`), `max-width: 100%`, `border-radius: 12px`,
background `var(--surface-2)`, and a 2s `shimmer` sweep at 6% opacity —
`@media (prefers-reduced-motion: reduce)` replaces the sweep with a static
fill. Centred inside: the elapsed counter (`0:07`) once past 3s, plus a
**Cancel** text button once past 10s. **The tile is laid out at the final
aspect ratio so the transcript never reflows when the image lands.** This is
the single detail that separates "feels like ChatGPT" from "feels like a
web form".

*Image:* `max-width: 100%`, `max-height: 512px`, `width: auto`,
`border-radius: 12px`, `display: block`, `object-fit: contain`. Never
letterboxed, never cropped. `alt` is the prompt, truncated to 120 chars.

*Video:* `<video controls loop muted playsinline preload="metadata">` with the
same box; `src` from `convertFileSrc(path)`, never a data URI.

*Action row* — appears below the media, `opacity: 0.55`, rising to `1` on
hover/focus-within, always fully visible on touch and always reachable by
keyboard (never `display:none`):

| Action | Behaviour |
|---|---|
| **Refine** | Focuses the composer, attaches this artifact as an implicit reference chip, placeholder text `Describe the change…` (`EDT-2`). Hidden when the producing backend reports `supports_edit: false`. |
| **Variation** | Re-runs the same `MediaRequest` with a new seed. One click, no typing. |
| **Save** | Writes into the working folder (reuses `saveArtifactToFolder`). Hidden when no folder is attached — same rule as `Artifacts.tsx:56`. |
| **⤓** | `downloadArtifact()` — native save dialog. |
| **↗** | `viewArtifact()` — opens the Workbench viewer / lightbox. |

*Metadata line*, below the actions, `font-size: 11px`, `--text-dim`:
`Nano Banana Pro · 1024×1024 · $0.04`. Cost is shown **only** when
`cost_usd.is_some()` — a local generation says `SDXL-Turbo · 1024×1024` and
nothing about money, which is itself the argument for local.

If `MediaResult.ignored` is non-empty, append one quiet clause:
`· 16:9 wasn't available — made 4:3 instead`. Normalise, report, never fail.

**Acceptance:** with the network throttled, the placeholder holds its box, the
counter runs, Cancel aborts, and the resolved image causes zero layout shift.

### STR-3 — user attachments render as thumbnails

> **Built ✅** — plus a lightbox (`ImageLightbox.tsx`).

**File:** `src/components/Conversation/UserTurn.tsx:22-30`.

Replace `▣ {name}` with a 64×64 rounded thumbnail (via `convertFileSrc`, or
the `dataUri` for pasted images), name below in 11px, click to open the
lightbox. A user who dropped three reference photos should see three photos.

**Acceptance:** Path C step 2 shows the actual picture, not a filename.

### STR-4 *(optional, after the rest)* — progressive reveal

> **Built ⚠️ unverified** — SSE parsing in `openrouter.rs::stream_image`,
> partials forwarded on a `poiesis-media-partial` app event and painted into the
> placeholder. It has never been seen to work: neither the frame shape nor the
> `supports_streaming` catalog field could be checked against a live account, so
> it is guarded twice (stream only when the provider's own catalog says so; fall
> back to a plain POST if the stream yields no image). See
> [Known-unverified](#known-unverified).

OpenRouter emits `image_generation.partial_image` SSE events for models with
`supports_streaming: true`. Swap the skeleton's fill for successive partials.
Pure delight, zero capability. Do not let it block anything.

---

## Part IV — Choosing, and the end of the mode

The mode switch dies here. It is replaced by **one declaration point (the model
chooser) and one suggestion (inference)** — which is the same thing a user
already understands from picking a chat model, rather than a new concept.

### PIK-1 — media models are a category in the model chooser

> **Built ✅** — `commands/media.rs` + a third picker group, omitted entirely
> when empty.

**Files:** `src/lib/types.ts`, `src/lib/store.ts` (`composeModels`,
`refreshCloud`), `src/lib/api.ts`, `src/components/ModelPicker/ModelPicker.tsx`,
`src/components/ModelPicker/ModelPicker.css`; backend
`src-tauri/src/commands/media.rs` (`list_media_models_cmd` → `Registry::all_models`).

`Model` (`types.ts:6-19`) gains:

```ts
/** What sending a message to this model produces. Absent = "chat". */
modality?: "chat" | "image" | "video";
/** Media only: backend id + label from BackendDescriptor, and the price tag. */
backendId?: string;
backendLabel?: string;
priceLabel?: string;
supportsEdit?: boolean;
supportedAspectRatios?: string[];
```

`composeModels` (`store.ts:559`) becomes
`[...localToModels(lib), ...cloudToModels(cloud), ...mediaToModels(media)]`,
with `mediaToModels` mapping `MediaModel` 1:1 and setting
`provenance` from the backend's `Credential` (`Local` → `"local"`, otherwise
`"cloud"`, so the existing dot semantics stay honest about what leaves the
machine).

**Picker UI** (`ModelPicker.tsx`): a third group after *Cloud · your key*.

```
  Images & video
  ◈ SDXL-Turbo            image · on this device        free
  ◈ Nano Banana Pro       image · OpenRouter            $0.04
  ◈ FLUX 2 Pro            image · fal.ai                $0.05
  ◈ Veo 3.1               video · OpenRouter            $0.25/s
```

- Group label `Images & video`; rows reuse `ModelRow` with `model.meta` set to
  `"{modality} · {backendLabel}"` and a new right-aligned `.price` span.
- The `Local only` filter chip (`ModelPicker.tsx:102-108`) hides hosted media
  models exactly as it hides cloud chat models — one rule, no special case.
- Empty state when no media backend has credentials: `+ Add an image provider`
  linking to Settings, worded from the descriptors, never hard-coded.
- The group is **omitted entirely** when `all_models` is empty, so a fresh
  install with no engine and no key sees today's picker unchanged.

**Acceptance:** with an OpenRouter key and a local diffusion model, the picker
shows three groups; with neither, two.

### PIK-2 — selecting a media model retargets the composer

> **Built ✅** — including all four guards: no engine load for a media id,
> `sendMessage` reroutes a media model to `createMedia`, switching
> conversations restores `lastChatModelId`, and personas never pin media ids.

**Files:** `src/lib/store.ts` (`selectModel`, new `mediaTarget` selector),
`src/components/Composer/Composer.tsx`, `src/routes/Chat.tsx`.

Selecting a media model must change what **send** does — that is the entire
point of Path E — while changing as little else as possible.

State: derive rather than duplicate. `mediaTarget` is
`useSelectedModel().modality` when it is `"image"` or `"video"`, else `null`.
Add one piece of real state, `lastChatModelId`, set by `selectModel` whenever
the chosen model's modality is chat.

**Composer changes when `mediaTarget !== null`:**

- Placeholder → `Describe an image…` / `Describe a video…`.
- A target bar above the input, 28px, `--surface-2`, 8px radius:
  `◈ Image · Nano Banana Pro · 1:1 ▾            ← Back to chat`
  The ratio segment is a dropdown over `supportedAspectRatios`; a video target
  adds a duration segment bounded by `max_duration_secs`.
- **← Back to chat** calls `selectModel(lastChatModelId)`.
- `Esc` in an empty input does the same.
- Send routes to `createMedia({ prompt, modelId, aspectRatio, references })`
  instead of `send()`.
- **Everything else stays mounted** — `ContextChip`, `ContextMeter`, personas,
  attachments. Delete the `!imageMode &&` guards at `Composer.tsx:581-584`.
  Making a picture is not leaving the conversation.

**Guards — these are the ones that bite:**

- `selectModel` must not attempt an engine load for a media id. It looks up
  `libraryModels` (`store.ts:752`) which will miss, but make the early return
  explicit and test it — a silent `llama-server` spawn attempt here would be a
  miserable bug to chase.
- `run_agent` must never be handed a media model. `send()` asserts
  `modality === "chat"` and, if not, routes to `createMedia` instead of
  failing — belt and braces, since the composer already prevents it.
- Switching conversations restores `lastChatModelId` (Path E step 6's
  "not across conversations" rule).
- A persona pinning a model (`store.ts:938`) only ever pins chat models;
  ignore a pinned media id with a console warning rather than retargeting the
  composer behind the user's back.

**Acceptance:** pick Veo 3.1, type "a fox in snow", get a video; click Back to
chat, and the previously selected chat model is restored with the draft intact.

### PIK-3 — inference, for when nothing was declared

> **Built ⚠️ diverged** — rule 1's object-noun requirement was dropped because
> it contradicts this plan's own acceptance test; see
> [Where the build diverged](#where-the-build-diverged-and-why). Both stated
> cases are asserted in `mediaIntent.test.ts`.

**Files:** `src/components/Composer/Composer.tsx`, `src/lib/store.ts`
(delete `imageMode`), new `src/lib/mediaIntent.ts`.

Only runs when a **chat** model is selected. A declaration always wins; this
never fires against Path E.

```ts
// src/lib/mediaIntent.ts
export type MediaIntent = "chat" | "image" | "video" | "edit";
export function detectIntent(draft: string, attachments: Attachment[]): {
  intent: MediaIntent; confidence: "high" | "low";
};
```

Rules, in order (pure function, unit-tested — `mediaIntent.test.ts`):

1. Draft matches `/^(draw|generate|create|make|paint|render|zeichne|male|erstelle)\b.{0,30}\b(image|picture|photo|logo|icon|illustration|bild|foto|grafik)\b/i` → `image`, high.
2. Same verbs with `(video|clip|animation|film)` → `video`, high.
3. An image attachment **and** imperative edit language
   (`remove|replace|make it|turn.*into|entferne|mach)` → `edit`, high.
4. An image attachment and a question (`what|who|why|how|is|does|was|wer`)
   → `chat`, high (vision Q&A — this is the disambiguation Path C needs).
5. Otherwise → `chat`.

**UI — the intent chip.** When intent ≠ `chat`, a single row appears directly
above the composer input, 28px tall, `--surface-2`, 8px radius:

```
  🖼  Image  ·  Nano Banana Pro ▾  ·  1:1 ▾            Chat instead  ✕
```

- The model segment is a dropdown over `list_image_models_cmd`, grouped
  **On this device** / **Cloud**, each cloud row showing its price
  (`$0.04`). Selection persists to `media.primary_image`.
- **Chat instead** dismisses to `chat` for this message only.
- On `low` confidence the chip is styled dimmer and the label reads
  `Image? · use anyway` — a suggestion, never a silent hijack.
- Keyboard: `Esc` in the input dismisses the chip before it clears the draft.
- The `+` menu keeps **Create image** and gains **Create video** as *explicit
  overrides* that pin the intent for the next message. The `+` entry no longer
  changes what else the composer shows.

**Crucially: the model picker, context chip and personas stay mounted.** Delete
the `!imageMode &&` guards at `Composer.tsx:581-584`. Making a picture is not
leaving the conversation.

**Acceptance:** typing "draw a fox" shows the chip and does not change anything
else on screen; typing "how do I draw a fox in Illustrator?" does not (rule 1
requires the object noun adjacent to the verb, so this stays `chat` — assert
both in the unit test).

### PIK-4 — advanced parameters *(Everything mode)*

> **Built ✅** — a **More** disclosure on the target bar: resolution, seed,
> *reuse last seed*, and (local only) steps + negative prompt. `lastMediaSeed`
> records only a seed the provider actually reported back, so "reuse" is not a
> lie. Video targets also get a duration select bounded by `max_duration_secs`.

**Files:** `src/components/Composer/Composer.tsx`, `src/lib/store.ts`.

An advanced disclosure on the `PIK-2` target bar, collapsed by default and
rendered only in Everything mode: resolution tier, seed (with a *reuse last
seed* toggle, which is what makes iteration reproducible), and — local backends
only — steps and negative prompt. Every option list is populated from the
selected model's `supportedAspectRatios` / `supportedResolutions`, so an
unsupported combination is never offered rather than being offered and then
silently remapped.

---

## Part V — Editing

### EDT-1 — references reach the backends and the tool

> **Built ✅** — incl. `reference_role`. Since no hosted image API has a field
> for source-vs-style, `MediaRequest::effective_prompt()` carries a `Style`
> role in words, which works across every provider.

**Files:** `src-tauri/src/media/*`, `src-tauri/src/agent/imagegen.rs`
(`tool_specs`), `src-tauri/src/commands/imagegen.rs`.

`generate_image_cmd` gains `references: Option<Vec<String>>` (paths, each run
through `assert_ui_readable_raw` — a reference is a file read and answers to
the same consent system as everything else; see `attachments.rs:5-6`).

Tool schema gains:

```json
"reference_images": { "type": "array", "items": { "type": "string" },
  "description": "Paths or artifact ids to edit from or take style from" },
"reference_role": { "type": "string", "enum": ["source", "style"] }
```

Cap at 8 references and reject beyond it with a plain sentence. Backends that
report `supports_references() == false` fail early with
`"<model> can't edit images — pick a cloud image model to refine this one."`

### EDT-2 — implicit reference: "make it warmer" just works

> **Built ✅** — with a 3-turn decay, and **Refine** in the media block as a
> second way in (it pins the intent, shows the chip and focuses the composer).

**Files:** `src/lib/store.ts`, `src/components/Composer/Composer.tsx`.

Store `lastMediaArtifactId` per conversation. When intent resolves to `edit`,
or the draft is a bare imperative with no attachment and the previous assistant
turn produced media **within the last 3 turns**, attach that artifact as the
reference — and **show it**: a 32×32 thumbnail chip above the input reading
`↳ refining` with an ✕ to detach.

The rule is: never silently. The implicit reference is always made visible
*before* send, so "make it warmer" is unambiguous to the user, not just to
the model. The produced artifact records `parent_id`, giving Library the
lineage from `ART-1`.

**Acceptance:** Path B end to end without touching a control other than the
keyboard.

---

## Part VI — The agent sees, and the money is visible

### SEE-1 — generated media comes back as vision

> **Not built ❌** — deferred by request, and the one task here that needs a
> live API round trip before it can be written safely: a wrong guess at the
> tool-result content shape would break every image tool call, not just this
> feature. See [Not built](#not-built).

**Files:** `src-tauri/src/agent/imagegen.rs` (`execute`),
`src-tauri/src/commands/agent.rs:18` (the content-part builder),
`src-tauri/src/agent/toolsets.rs` (tool-result shape).

Today `execute` returns a sentence, so the model is blind to its own output and
cannot tell a good result from a garbled one. When the active chat model is
vision-capable — `CloudModel.vision` is already tracked at `cloud/mod.rs:102`
and populated for both OpenRouter and OpenAI — return the generated image as an
`image_url` content part alongside the sentence, downscaled to ≤768px on the
long edge to keep the token cost sane.

This is what turns a button into an agent: it can look, judge, and say *"the
text on the sign came out garbled — regenerating with the lettering spelled
out"*. It composes directly with `PERCEPTION_PLAN`'s deferred `VIS` work and
with `RND`.

Guard it: never attach for non-vision models (wasted tokens and a confusing
error), never for video, and cap at one image per tool result.

### CST-1 — consent before the first paid generation

> **Built ✅** — `MediaConsentDialog`, remembered per backend in `localStorage`
> (a UI trust decision, not a fact the DB or the agent's memory needs).

**Files:** `src/lib/store.ts`, `src/components/Confirm/*`.

The first cloud media generation in an install raises the existing confirm
dialog: *"Generating with Nano Banana Pro sends your prompt to OpenRouter and
costs about $0.04 per image. Local generation stays on your device."* Options:
**Generate · Use local instead · Cancel**, plus *don't ask again for this
provider*. Subsequent generations show cost only in the metadata line
(`STR-2`).

This is the honest version of a trade the current modal switch simply hides.
Local-first means the cloud is opt-in and *legible*, not absent.

### CST-2 — spend visibility *(Everything mode)*

> **Built ✅** — `media_spend_cmd` aggregates `meta_json.cost_usd` straight from
> the artifacts, shown in Settings → Cloud and in the Library filter row, both
> hidden when there is nothing to report. Month boundary is computed with
> civil-from-days arithmetic (no new date dependency), covered by three tests.

Sum `meta_json.cost_usd` per conversation and per month; show a line in
Settings → Cloud (*"Media this month: $2.40 · 61 images, 3 clips"*) and a
running total in the Library filter row. No budget enforcement in this phase —
just the number, because a number nobody shows is a number nobody trusts.

---

## Part VII — Defects found while surveying

### FIX-1 — byte-slicing prompts panics on non-ASCII

> **Built ✅** — `media::ellipsize`, with the umlaut and emoji boundary tests.

`&prompt[..40]` (`agent/imagegen.rs:60`), `&prompt[..48]` (`:108`), and
`&prompt[..60]` (`commands/imagegen.rs:389`) slice a `&str` at a byte index. A
prompt whose 40th byte falls inside a multi-byte character panics. `"Zeichne
eine Straße bei Nacht mit Laternen und Nebel"` is not an edge case for this
user. Replace all three with a shared
`fn ellipsize(s: &str, max_chars: usize) -> String` over `char_indices()`, and
add a unit test with an umlaut and an emoji at the boundary.

**Do this first — it is ten minutes and it is a live crash.**

### FIX-2 — orphaned media on disk

> **Built ⚠️ partial** — conversation deletion sweeps generated files. The
> artifact half is unreachable: no artifact-delete command exists anywhere in
> the codebase, so there is no call site to wire.

`delete_image_model_cmd` removes model files, but nothing ever removes a
*generated* file. When an artifact is deleted (or its conversation is), delete
the file under `generated_media_dir()` — but only after checking
`db/mod.rs:2024`'s existing "is this path still referenced by an attachment"
query, which already exists for exactly this class of question.

### FIX-3 — `save_artifact_cmd` assumes text or image

> **Built ✅**

`attachments.rs:78-88` branches on `kind == "image"` (copy) else write-string.
A video artifact would have its *path* written into the destination file as
text. Change the branch to "path-backed kinds" (`image`, `video`) vs
content-backed.

---

## What "done" looks like

A user with no API key opens Poiesis, types **"draw a fox reading a map"**, and
gets a picture in the conversation from their own GPU — which is in Library
afterwards. A user who pastes an OpenRouter key types the same words and gets a
2026-quality picture for four cents, with the price on screen. Either one then
types **"make it warmer"** and gets a second picture below the first, linked to
it. Neither of them ever flipped a switch, and neither of them could tell you
whether the agent called a tool or the app called a command.

That is the whole phase.

---

## What is left

Everything in the paragraph above is built. `imageMode` is gone, both creation
paths converge on one artifact and one presentation, and a generation no longer
holds the conversation hostage while it runs.

Three things are worth picking up next, in this order:

1. **Verify the cloud endpoints against a live account.** `VID-1`'s polling
   shape, `STR-4`'s SSE frames and `supports_streaming`, and the curated model
   slugs in both backends were all written from this document rather than from a
   response anyone has seen. Nothing is known-broken; nothing is known-working
   either. This is an afternoon with one API key, and it retires most of the
   risk in this phase.
2. **`SEE-1`.** Blocked on the same key — the tool-result content shape has to
   be confirmed before it can be written without risking every image tool call.
   It is also the task that most changes what the product *is*: it is the
   difference between a button and an agent that can look at what it made.
3. **`BKD-3`,** once Settings → Cloud generates a key row from a
   `Credential::Media` descriptor. That row is the last piece of `BKD-2`'s
   "adding a provider is one file and one line" promise, and fal.rs is the test
   of whether the promise is true.

Smaller, whenever convenient: video-model discovery (`VID-1`), a Settings
surface for `media.primary_image` / `media.fallbacks`, and the artifact-delete
half of `FIX-2` when an artifact-delete command exists to hang it on.
