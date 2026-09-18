# Project Poiesis — Perception Plan (Phase 12): recall and retrieval

Phase 10 gave Poiesis a **durable self**; Phase 11 gave it the **loop** that
maintains that self. Both store what they learn as markdown and find it again
by keyword. This phase gives Poiesis **perception**: the ability to find the
right memory by *meaning* rather than by matching words, and to answer
questions about a folder it has never been told the vocabulary of. Seeing the
images and scans *inside* that folder is specified here in full (Part IV) but
deferred to the phase after this one — the same primitive carries it, and it
needs fixtures and a vision path this phase does not build.

The centrepiece is not folder search. It is this: **a lesson Poiesis learned
from its own mistake must surface at the moment it applies.** Today every
lesson is listed in an always-injected index trimmed to a character budget
(`INDEX_CAP_LESSONS = 1000`), so as reflection does its job and lessons
approach `LESSON_CAP = 40`, the budget — not relevance — decides which ones
reach the model. Nothing errors. The lesson stays on disk. It simply stops
arriving when it matters. That is the autopoietic promise failing silently,
and one capability (embeddings) fixes it, unlocks folder retrieval, and — in
the phase after this one — makes images searchable, all from the same
primitive.

> Companions: `plans/POIESIS_PLAN.md` (Phases 10–11, the self and its loop) ·
> `plans/FILESYSTEM_PLAN.md` (working folders, trust, undo) ·
> `plans/IMPLEMENTATION_PLAN.md` (Phases 0–8, built). Checklist:
> `plans/TASKS.md`.
> Written to be implementable task-by-task without further design decisions:
> every task names its files, signatures, SQL, and acceptance check.
>
> ID prefixes — **RND** tool-emitted renders · **EVL** eval harness ·
> **EMB** embedding engine · **VEC** vector store · **SEM** semantic recall ·
> **SCP** memory scoping · **PRO** synthesized profile · **IDX** folder index ·
> **RET** retrieval skill · **RRK** reranking · **VIS** vision captioning ·
> **OCR** document OCR · **XTR** table extraction · **PHS** perceptual
> hashing · **PER** persona tool sets · **DAT** data analysis · **CRT** critic
> gate · **SCH** scheduler · **SMP** simplification · **WHY** the layered
> context panel. `-UI-` tasks are frontend.
>
> **Build order (revised 2026-07-31):** (RND ∥ EVL) → EMB → VEC → SEM →
> **WHY** → SCP → IDX → RET. **RRK** any time after RET; **PRO** any time
> after SCP. PER / DAT / CRT are independent and can land any time after RND;
> **PHS** after IDX (its image half is standalone, its document half needs
> chunk vectors). SCH last — it composes everything.
> **Part IV (VIS / OCR / XTR) is deferred out of this phase** — see the note
> at its heading.
> **Part VII is not last.** `SMP-1` (the Simple/Everything switch) must land
> **before the first `-UI-` task**, because every later UI task declares which
> mode it belongs to. The remaining `SMP` tasks amend their targets and land
> *with* them, not after — **except `SMP-2`, which is now urgent rather than
> optional.** `SMP-1` has landed, and Engine → Recall is the only route to
> installing the embedder, so until `SMP-2` lands a default (Simple) install
> can never reach semantic recall at all. It goes next, alongside `WHY`.
>
> **Why `WHY` moved ahead of `SCP`/`IDX`/`RET`.** It was originally scheduled
> after `SEM` + `RET`, "once there are retrieved layers worth showing". In
> practice it is not a display task at all: `WHY-1` is where prompt
> composition moves out of `lib/store.ts` and into the backend. Until that
> happens every prompt-touching task — `SCP`, `PRO`, half of `SMP` — has to
> be orchestrated client-side and then unwound again, and `SEM`'s exit
> criterion cannot become an `EVL` case at all, because `run_agent` never
> sees a system prompt (see `src-tauri/tests/eval/README.md`). Landing `WHY`
> early costs the refactor once instead of once plus everything built on the
> old shape. Its `about_you` and `from_files` layers simply have no content
> until `PRO` and `RET` exist; an empty layer is absent, not broken.
>
> **Landed (2026-08-04):** RND · EVL · schema v7 · EMB · VEC · SEM · WHY ·
> SCP · IDX · RET · RRK · PRO · PHS · PER · DAT · CRT · SCH · SMP-1…SMP-8.
> That is every part of this phase except **Part IV (VIS / OCR / XTR)**,
> which was deferred out of it by design.
>
> **What "landed" means here, and what it doesn't.** Everything above is
> built, unit-tested (185 Rust tests + 3 copy-lint), and clean through
> `tsc --noEmit` and `vite build`. Almost none of it has been through its own
> **exit criterion**, because every one of those is a runtime check against
> real models — attach a folder and ask a question phrased differently, leave
> the app open overnight, teach a lesson in chat A and ask in chat B. The
> first manual pass over `SCH` found five defects in code that compiled and
> passed its tests, one of which wedged the scheduler until the app was
> restarted, plus a UI placement that made the feature unfindable. Treat the
> list above as *ready to be exercised*, not as proven.
>
> **PRES-0 (first-person copy) binds every `-UI-` task here**, exactly as in
> `POIESIS_PLAN.md` Part I §5. In particular principle 4 (*quiet biology*)
> forbids the obvious retrieval UI: **no confidence badges, no match
> percentages, no relevance scores shown to the user, ever.** Provenance is
> quiet inline prose. Scores exist in the backend and in `EVL` output only.

**Settled decisions (2026-07-30):**

- **No vector-database dependency.** Vectors are `BLOB`s in SQLite and
  similarity is a linear scan in Rust. At our ceiling (500 files × 60 chunks
  × 384 dims) that is ~9M multiply-adds per query — single-digit milliseconds
  in release. `sqlite-vec` would buy nothing and cost a C dependency in the
  build.
- **The embedding engine runs CPU-only, in its own process.** It must never
  compete with the chat model for VRAM. Our verified floor is a GTX 1060
  6 GB; a 130 MB embedder on CPU keeps that floor intact.
- **Vectors are stored pre-normalised**, so similarity is a dot product.
- **The embedding model is recorded on every index and every vector.** If it
  changes, all affected vectors are discarded and rebuilt — never migrated.
  Queries embed with *the index's recorded model*, not the current setting.
  Mixing two embedding spaces in one comparison is silently wrong, not stale.
- **Images are indexed by vision caption**, in the same text embedding space.
  No CLIP, no second embedding space.
- **Explicitly out of scope:** MCP *server* mode (no value for a single-user
  local app — we stay an MCP client) · face detection and clustering · photo
  maps and EXIF geo-plotting · social-post generation · a multi-agent
  planner/researcher/drafter/critic roster (too slow on one GPU; we take only
  its critic gate, as `CRT`) · a bespoke CSV query tool (we have a code
  sandbox — see `DAT`) · Projects as a container (indexes key off the folder
  path, so retrieval ships without them; revisit when an index, instructions
  and scoped memory genuinely need to travel together).

---

# Part 0 — Enablers

Neither is a headline feature. Both make everything after them land reliably
and stay landed, so both come first.

## RND — Tool-emitted renders

Today a workspace block reaches the screen only if the *model* emits
well-formed block JSON. On a 7B local model that is a coin flip, and every
feature in this plan wants to show structured output (retrieval hits,
extracted tables, image grids). The tool already holds the structured data;
it should be able to render directly.

`agent/present.rs` already writes a reserved blocks row from the backend, so
the path exists — this generalises it.

- `RND-1` Extend the internal tool-result type in `agent/skills.rs` with an
  optional `render: Option<BlockSpec>`, where `BlockSpec { kind, title,
  data_json }` matches the existing `blocks` table columns.
- `RND-2` In `agent/run.rs`, when a tool result carries `render`, insert a
  `blocks` row (`conversation_id`, `message_id` = the in-flight assistant
  message) and emit the existing block event. No new event variant — reuse
  what the Workspace already listens to.
- `RND-3` Guard rails: one render per tool call; drop with a logged warning if
  `data_json` exceeds 64 KB; renders are skipped entirely in headless runs
  (see `SCH-3`).
- `RND-UI-1` No new component: `components/Blocks/BlockRenderer.tsx` already
  renders a `DbBlock`. Verify that a backend-authored block anchored to the
  in-flight assistant message appears in place while streaming *and* survives
  a conversation reload; fix the anchoring if it assumes the model authored
  it. Add no new `BlockKind` — `RET`, `XTR` and `PHS` all reuse the existing
  `collection` and `comparison` kinds.

**Exit for RND:** a tool returns a `collection` render and it appears inline,
correctly anchored, after a reload, with a model that has never emitted a
block itself.

## EVL — Agent regression harness

Everything in this plan changes agent behaviour, and `CRT` lets the agent
change its *own* behaviour. `cargo test` covers runtime selection, FTS, and
path guards — infrastructure, not conduct. Nothing today catches "reflection
wrote a bad lesson and the agent got worse."

- `EVL-1` New `src-tauri/tests/eval/` with `fixtures/` (a small folder: two
  markdown notes, a CSV, a text-layer PDF, a scanned PDF, three photos —
  two of them near-duplicates) and `golden.json`: `[{ id, question,
  must_contain[], must_not_contain[], expect_tool? }]`.
- `EVL-2` A `--ignored` integration test (`cargo test --ignored eval`) that
  points the agent at a temp app-data dir and the fixture folder, runs each
  question, and asserts. Ignored by default because it needs a live engine.
- `EVL-3` `--filter <id>` and a summary line per question. Non-zero exit on
  any failure.
- `EVL-4` **Threshold calibration mode** (`cargo test --ignored eval_calibrate`):
  prints the score distribution for known-relevant vs known-irrelevant pairs
  from the fixtures. Every floor in this plan (`SEM-3`, `RET-2`) is stated as
  a *starting* value measured for one embedding model; this is how they get
  re-measured when the model changes.

**Exit for EVL:** the suite passes on the current build, and deliberately
corrupting a lesson file turns a question red.

---

# Part I — The vector substrate

## EMB — The embedding engine

A second `llama-server` process, CPU-only, on its own loopback port. This is
architecturally the same move as the image engine: a separate engine with its
own lifecycle, reusing `runtime/process.rs`, `runtime/jobobject.rs`, and the
resumable downloader.

- `EMB-1` `runtime/embedserver.rs`: spawn `llama-server` with
  `--embeddings --pooling mean -ngl 0` (no GPU layers — deliberate), dynamic
  port, per-session random token, health-gated readiness, job-object bound,
  killed on exit. Model this file on the existing image-engine module.
- `EMB-2` **Lazy start, idle stop.** Started on the first embedding request;
  shut down after 5 minutes idle. Indexing a large folder holds it open.
- `EMB-3` `embed(texts: &[String]) -> Result<Vec<Vec<f32>>>` posting to
  `/v1/embeddings` (OpenAI-compatible — reuse the shapes in `cloud/`), in
  batches of 32. **Normalise every returned vector to unit length before
  returning it.**
- `EMB-4` Catalog + library. `model_library.role` (schema v7) distinguishes
  `chat` | `embed` | `rerank` — one column covers all three engines, so
  `RRK` needs no further migration. Default catalog entry:
  **bge-small-en-v1.5, F16, 384 dims, ~130 MB**. Second option:
  nomic-embed-text-v1.5 (768 dims, ~275 MB). Quantised embedders degrade
  noticeably — offer F16 only.
- `EMB-5` Every embedding path degrades to "unavailable" rather than
  erroring: no model installed, engine down, or request failed ⇒ callers fall
  back to today's keyword behaviour. This is a hard requirement — Poiesis must
  stay fully usable with no embedder installed.
- `EMB-UI-1` **Engine → Embedding**, a third section in `routes/Engine.tsx`.
  Copy the structure of `components/ImageModels/ImageEngine.tsx` into a new
  `components/EmbedEngine/EmbedEngine.tsx` — install state, model picker,
  `Install` / `Remove`, resumable download progress — plus one plain
  sentence: *"I use this to recall things by meaning instead of by keyword.
  It runs on the CPU, so it never takes memory from the model you chat
  with."*
- `EMB-UI-2` Where a feature needs the engine and it is absent, the
  affordance **explains instead of disabling**: in `FolderHeader.tsx`
  (`IDX-UI-1`) the `Read it` button is replaced by *"I can only match words
  right now — install my recall engine"*, linking to Engine → Embedding.
  Never a dead control with no explanation.
- `EMB-UI-3` `lib/api.ts`: add `embedEngineStatus()`, `installEmbedEngine()`,
  `removeEmbedEngine()` wrappers following the existing image-engine
  wrappers, with mock values behind `inTauri()` so `npm run dev` still
  renders the tab without a backend.

**Exit for EMB:** install from a cold app, embed 200 strings, confirm unit
norms, confirm the process dies on app exit and after idle, and confirm the
chat engine's VRAM is untouched throughout.

## VEC — The vector store

- `VEC-1` One table for both memory and file vectors (schema v7 below). The
  same cosine helper then serves recall and retrieval.
- `VEC-2` `db/vectors.rs`: encode/decode `Vec<f32>` ⇄ `BLOB` as little-endian
  f32, no new dependency. `fn similarity(a: &[f32], b: &[f32]) -> f32` is a
  plain dot product (both sides are pre-normalised — assert equal `dim`).
- `VEC-3` `fn search(db, owner_kind, scope_key, query_vec, k) -> Vec<Hit>`:
  load candidate rows, dot-product, sort, take k. Straight linear scan;
  revisit only if a real corpus exceeds ~100k chunks.
- `VEC-4` **Model guard.** Every row carries `model` and `dim`. Any read that
  encounters a row whose `model` differs from the caller's expectation
  discards the whole `scope_key` and signals "needs rebuild" — never silently
  compares across spaces, never partially migrates.

### Schema v7

`db/mod.rs` is already at `SCHEMA_VERSION = 6` (v5 Poiesis compaction, v6
working folder), so this phase is **v7**. Set `SCHEMA_VERSION: i64 = 7` and
append to `migrate()`:

```rust
if current < 7 {
    // v7 (Perception): model role (chat | embed | rerank); per-persona tool sets;
    // per-message context manifest so a past answer can be explained (WHY-2).
    // The `vectors` and `index_roots` tables are created by SCHEMA above.
    Self::add_column(&conn, "model_library", "role", "TEXT NOT NULL DEFAULT 'chat'")?;
    Self::add_column(&conn, "personas", "tools_json", "TEXT")?;
    Self::add_column(&conn, "messages", "context_json", "TEXT")?;
}
```

And append to `db/schema.sql` (idempotent, so fresh installs are covered):

```sql
-- Perception: one vector store for durable memory and indexed files.
-- `vec` is a little-endian f32 array, pre-normalised to unit length, so
-- similarity is a dot product. `model`/`dim` are recorded per row: a change
-- of embedding model discards and rebuilds rather than migrating (VEC-4).
CREATE TABLE IF NOT EXISTS vectors (
  id         TEXT PRIMARY KEY,
  owner_kind TEXT NOT NULL,           -- 'memory' | 'file'
  scope_key  TEXT NOT NULL,           -- memory: collection; file: canonical index root
  ref_key    TEXT NOT NULL,           -- memory: entry slug; file: absolute path
  chunk_ix   INTEGER NOT NULL DEFAULT 0,
  text       TEXT NOT NULL,           -- the chunk (or the memory line) that was embedded
  model      TEXT NOT NULL,
  dim        INTEGER NOT NULL,
  vec        BLOB NOT NULL,
  mtime      INTEGER,                 -- files only: source mtime, for incremental reindex
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_vectors_scope ON vectors(owner_kind, scope_key);
CREATE INDEX IF NOT EXISTS idx_vectors_ref   ON vectors(ref_key);

-- Perception: one row per indexed folder root — what built it, and when.
CREATE TABLE IF NOT EXISTS index_roots (
  path        TEXT PRIMARY KEY,       -- canonical folder path
  model       TEXT NOT NULL,
  dim         INTEGER NOT NULL,
  file_count  INTEGER NOT NULL DEFAULT 0,
  chunk_count INTEGER NOT NULL DEFAULT 0,
  skipped     TEXT,                   -- JSON: [{path, reason}] surfaced in IDX-UI-2
  state       TEXT NOT NULL DEFAULT 'idle',  -- idle|building|stale|error
  updated_at  INTEGER NOT NULL
);
```

---

# Part II — Semantic recall (the payoff)

## SEM — Recall by meaning

This replaces the always-injected index with relevance-gated selection. It is
the smallest surface in the plan (tens of entries, no file walking) and the
one the user feels most.

- `SEM-1` On every memory write in `memory/mod.rs` (fact, lesson, recipe),
  embed `"{name}\n{description}"` — *what it is about and when it applies*,
  not the body, so retrieval matches the trigger — and upsert into `vectors`
  with `owner_kind='memory'`, `scope_key=<collection>`, `ref_key=<slug>`.
- `SEM-2` Backfill lazily: entries missing a vector are embedded in one
  batched call on the next turn. `.trash/` and `.quarantine/` are never
  embedded.
- `SEM-3` `recall_for(query) -> RecallSet` replaces wholesale index injection:
  - **Always injected:** `SOUL.md` + facts scoped `global` (see `SCP`).
  - **Retrieved:** top-4 facts scoped `topical`, top-3 lessons, top-2 recipes,
    each above a floor of **0.58** (starting value, measured for
    nomic-embed-text; re-measure per model with `EVL-4`).
  - Recipes and lessons still contribute name + description only; the body is
    fetched by the existing `use_recipe` / read verbs.
- `SEM-4` Degrade honestly: with no embedder, `recall_for` returns today's
  behaviour (whole index, char-capped) unchanged. One code path, one flag.
- `SEM-5` **Reuse the existing `recall` event — do not add a variant.**
  `AgentEvent` already carries `{ type: "recall", id, matches: SearchHit[] }`
  (built for RCL-UI) and `Timeline.tsx` already renders it. Semantic recall
  emits that same variant once per turn, listing only the *retrieved*
  entries. Always-injected entries (`SOUL`, global facts) emit nothing —
  they are ambient context, not events.

### SEM-UI — the recall moment

This is where the concept becomes feelable, and **most of the mechanism
already exists**: `components/Conversation/Timeline.tsx` renders an
`AgentStep`'s `matches: SearchHit[]` through its `Provenance` sub-component
(chip + title + date + snippet, behind a `⌄` disclosure with
`aria-expanded`), fed by the `recall` event. Semantic recall **extends that
path** — it does not add a parallel one.

- `SEM-UI-1` `lib/api.ts`: widen `SearchHit.source` from `"chat" | "memory"`
  to `"chat" | "memory" | "file"` (`RET` needs the third), and add
  `kind?: "fact" | "lesson" | "recipe"` so a lesson is labelled differently
  from a fact.
- `SEM-UI-2` `Timeline.tsx` → `Provenance`: the chip currently reads
  `◆ memory`, which is third-person and breaks PRES-0. Drive its text from
  `kind` instead — `◆ remembered` (fact), `◆ learned` (lesson),
  `◆ procedure` (recipe), `from your files` (file), `earlier chat` (chat).
  Keep the existing `recall-chip` class and its per-source modifier.
- `SEM-UI-3` The step line itself (`verb` / `target` / `result`, produced by
  the backend `describe()`) reads *"I remembered — {description}"* for one
  entry and *"I remembered 3 things"* for several. **No new collapse logic**
  — `Step` in `Timeline.tsx` already gives the summary line plus disclosure.
- `SEM-UI-4` `components/Memory/MemoryPanel.tsx`: each fact card gains a
  quiet *"last surfaced {relative date}"* line (extend the `Fact` shape from
  `list_memory_facts_cmd` with `last_used_at`). **No usefulness score, no
  ranking, no sparkline** — a date is enough to see what is alive and what
  has gone quiet.
- `SEM-UI-5` `Conversation.css`: new chip variants reuse the existing
  `recall-*` ink tokens; add no new colour. Confirm the
  `prefers-reduced-motion` rule covers `.recall-matches` and extend it if
  not — the disclosure must open without transition.
- `SEM-UI-6` a11y: `announce()` in `Timeline.tsx` already composes the
  screen-reader sentence from verb/target/result, so a recall step is spoken
  as *"I remembered, metric units"* for free. **Verify, don't rebuild.**

**Exit for SEM:** teach Poiesis a lesson in chat A. Fill the store with 39
unrelated lessons. In chat B, ask something the lesson applies to — the
lesson surfaces, the timeline says so in the first person, and the other 39
never enter the prompt. Verified by an `EVL` case.

## SCP — Global vs topical scope

Always-injecting every preference is too blunt, and the failure is not
theoretical: a topical instruction bleeds into unrelated answers, and a
narrow fact colours unrelated prose. `SEM-3` needs this split to exist.

- `SCP-1` Add `scope: global | topical` to fact frontmatter. Classified once
  at save time by one local call: *does this apply to every response
  regardless of subject, or only when a specific subject comes up?*
- `SCP-2` On failure or no model, default to **global** — today's behaviour,
  not a new silent omission.
- `SCP-3` Backfill at most 3 unscoped entries per turn (one call each, unlike
  the batched embed) to bound added latency. Missing `scope` reads as
  `global` until backfilled, so nothing regresses mid-migration.
- `SCP-4` Lessons and recipes are always relevance-gated; they take no scope.
- `SCP-UI-1` `components/Memory/MemoryPanel.tsx`: each fact card shows scope
  as a plain word — *"always"* / *"when relevant"* — as a two-option control
  the user can change (→ `set_fact_scope_cmd`), since they are the final
  authority on their own standing instructions. No icon, no colour coding,
  no chip.

**Exit for SCP:** a topical instruction ("when I ask about pricing, always
show the currency") does not appear in the prompt for an unrelated question;
a global one ("be concise") always does. Both are `EVL` cases.

---

# Part III — Folder retrieval

## IDX — The folder index

- `IDX-1` `agent/index.rs`. Index root = a canonical folder path, and only one
  the permission layer already grants (`permissions/`). Never index outside a
  grant; re-check on every build, not just at start.
- `IDX-2` Walk honouring the existing `IGNORED_DIRS` and hidden-file rules
  from `agent/filesystem.rs` — reuse `is_ignored`, do not fork it. Caps:
  500 files per root, depth 6, 60 chunks per file.
- `IDX-3` Extract text: existing text sniffing for text-like files; PDF text
  layer via the existing extractor; images and scanned PDFs via `VIS`/`OCR`
  when available, otherwise recorded as skipped with a reason. With Part IV
  deferred, "when available" is simply always false for this phase: every
  image and scan becomes a skipped entry with a stated reason, which
  `IDX-UI-2` already surfaces. No branch here changes when `VIS`/`OCR` land.
- `IDX-4` Chunk at 1200 chars with 200 overlap, after collapsing runs of
  whitespace.
- `IDX-5` Incremental: a file whose `mtime` matches its stored rows is reused.
  Changed files have their rows deleted and re-embedded. Deleted files have
  their rows dropped.
- `IDX-6` Model change ⇒ full rebuild of that root (`VEC-4`), never a partial
  update.
- `IDX-7` Runs on a background task with cancellation; emits progress
  (`files_done / files_total`). Indexing never blocks a chat turn.
- `IDX-8` Indexing is a **skill toggle, default off**, consistent with web
  search and code execution. Enabling it does not implicitly grant folders.

### IDX-UI — indexing is visible, never mysterious

All of this lives in `components/Workbench/FolderHeader.tsx`, which already
owns the attached-folder header, its overflow menu, the `wb-primary` /
`wb-error` styling and store access via `useAppStore`. Styles go in
`Workbench.css` beside the existing `wb-head-*` rules.

- `IDX-UI-1` A new `wb-head-row` line beneath the folder path, driven by
  `index_roots.state` for the attached path (new store field `indexState`,
  fed by an `indexStatus(path)` wrapper in `lib/api.ts`):
  - never built → *"I haven't read this folder yet"* + `Read it`
    (→ `build_index_cmd`).
  - `building` → *"Reading… 34 of 120"* + `Stop` (→ `cancel_index_cmd`), fed
    by the `IDX-7` progress event. **No progress bar, no percentage, no
    spinner** — a counting line only.
  - built → *"I've read 120 files here · 2 hours ago"* + `Read again`.
  - `error` → the message plus `Try again`, using the existing `wb-error`
    class.
- `IDX-UI-2` When `index_roots.skipped` is non-empty, append *"12 files I
  couldn't read"* as a `<button>` toggling an inline list of name + reason
  (*"needs my eyes — no vision model loaded"*, *"too large"*, *"not text"*).
  Follow the existing open/close pattern in this file (`menuRef` +
  `mousedown` listener). This list is the entry point that makes `VIS`
  discoverable — a user with no vision model finds out here.
- `IDX-UI-3` `state = 'stale'` → *"3 files changed since I read this"* +
  `Read again`. Never auto-rebuild silently; never serve stale results
  without saying so.
- `IDX-UI-4` `routes/Settings.tsx`: **Folder reading** joins Settings →
  Skills automatically through the skills framework (as `MEM-UI-6` notes for
  memory). Beneath it, list the indexed roots with size on disk and a
  `Forget this folder` action (→ `forget_index_cmd`, dropping the
  `index_roots` row and its `vectors`).

## RET — The retrieval skill

- `RET-1` New tool `search_folder { query, path?, max_results? }`, sitting
  alongside the existing exact-match `search_files`. Tool descriptions must
  make the division obvious: `search_files` for a known string, glob, or
  filename; `search_folder` for a question in the user's own words.
- `RET-2` Scoring, in order:
  1. Dot product against the query vector (embedded with the *index's*
     recorded model).
  2. **Keyword bonus, never a discount**: `min(1.0, sim + 0.2 * kw)` where
     `kw` is the weighted fraction of distinctive query terms present in the
     chunk (terms containing a digit or ≥4 chars, minus a stoplist; weight
     1.6 for terms with digits or ≥7 chars). Computed in Rust over candidate
     chunks — no second FTS table. Because it can only add, the floors below
     keep their meaning.
  3. **MMR** diversification, λ = 0.7, with a **per-file cap of 2** so one
     large document cannot occupy the result set.
  4. Floor **0.40** to be returned at all (starting value; `EVL-4`).
- `RET-3` **Corrective pass.** If the best hit is below 0.50, ask the local
  model once to rephrase the query "as a document would state it, keeping
  names and numbers exactly", search again, merge, dedupe, re-sort.
- `RET-4` **Sufficiency check.** If the best hit is still below 0.55, ask
  whether the excerpts actually answer the question, not merely share its
  subject. If not, the tool result is prefixed with an explicit instruction
  not to present them as a confident answer. A weak retrieval must reach the
  model as a *warning*, not as ordinary context.
- `RET-5` The result carries a `render` (`RND`) listing matched files, and
  `sources` for provenance.
- `RET-6` Every retrieval writes one `activity_log` row, like every other
  file access.

### RET-UI — grounding, quietly

heapchat surfaces grounding as a `✓ Grounded · N sources` badge. **PRES-0
principle 4 forbids badges**, so we take the substance and drop the form —
and reuse `Provenance` from `SEM-UI` rather than building a second
provenance UI.

- `RET-UI-1` `search_folder` returns its hits as `SearchHit[]` with
  `source: "file"` on the existing `recall` event, so `Timeline.tsx` renders
  them through `Provenance` with **no new component**: `title` = filename,
  `snippet` = the matched chunk, `conversation_id` = null. Extend the
  `convId ? <button> : <div>` branch in `Provenance` with a third case for
  `source === "file"` that opens the file in
  `components/Workbench/Viewer.tsx` instead of switching conversation.
- `RET-UI-2` The step line (backend `describe()`, past-tense style as in
  `agent/filesystem.rs`) *is* the grounding statement: *"I read 3 files in
  Notes"*. For the `RET-4` weak case its `result` field reads *"— I'm not
  sure they answer this"*, which the `.result` span already renders and
  `announce()` already speaks. **The user must be able to tell a grounded
  answer from a guess without reading the answer critically** — that is the
  entire reason `RET-3`/`RET-4` exist, and this is where it surfaces.
- `RET-UI-3` Empty result: *"I couldn't find anything about that in Notes"*,
  plus, when `index_roots.state = 'stale'`, *"— 3 files changed since I read
  it"* so `IDX-UI-3`'s offer is one glance away.
- `RET-UI-4` **No scores anywhere in the UI.** `SearchHit` gains no score
  field; ranking stays in the backend and in `EVL` output.

**Exit for RET:** on the `EVL` fixtures, a question phrased with none of the
document's vocabulary returns the right passage; a question about something
genuinely absent returns the empty state rather than a confident paragraph;
and the near-miss fixture triggers `RET-UI-2`.

## RRK — Reranking (optional, default off)

Embedding similarity is a **bi-encoder** score: query and chunk are embedded
independently, so it measures topical closeness but not whether a passage
actually *answers* the question. A **cross-encoder** reads both together and
is markedly better at that final ordering. This is the one place heapchat's
retrieval is genuinely stronger than what `RET` alone gives us.

**We do not need their workaround.** They smuggle reranking through Ollama's
`/api/embed`, reading `embeddings[0][0]` as a raw relevance logit, because
Ollama exposes no rerank API. We own the server: `llama-server` has a native
rerank endpoint behind `--reranking`, returning proper scores.

- `RRK-1` **Verify the endpoint before building.** Confirm the pinned
  `llama-server` build exposes `/rerank` (or `/v1/rerank`) with
  `--reranking`. If the pinned build predates it, either bump the runtime
  manifest or fall back to the logit trick — POST the pair to the embedding
  endpoint and squash `embeddings[0][0]` through a sigmoid. Record which path
  was taken in the module doc comment; the two produce different score
  scales and are not interchangeable afterwards.
- `RRK-2` A **third** CPU-only engine (`runtime/rerankserver.rs`), identical
  in lifecycle to `EMB-1`: lazy start, 5-minute idle stop, job object,
  `-ngl 0`. One model per `llama-server` process, so this cannot share the
  embedder's process.
- `RRK-3` Catalog: **bge-reranker-base** as default, bge-reranker-v2-m3 as the
  quality option, both `role = 'rerank'`. *(Built: the ~280 MB / ~600 MB
  figures were for quantised weights. Both ship at full precision, matching
  `EMB`'s "compressed versions get noticeably worse" policy — reranking is the
  one pass where precision decides the final ordering — so the real sizes are
  **~540 MB** and **~1.1 GB**, verified against the published blobs.)*
- `RRK-4` **Selective, not always-on.** Rerank the top 20 candidates only
  when the ranking is actually in doubt — when the gap between the 1st and
  5th score is under 0.08. A confident retrieval skips it and pays nothing.
  (Heapchat reranks unconditionally; on CPU that is 20 cross-encoder passes
  routinely spent to produce no reordering at all.)
- `RRK-5` Best-effort, always: any failure — model absent, engine down,
  unexpected output shape — logs once and leaves the embedding-only ranking
  untouched. **Reranking must never be able to fail a search.**
- `RRK-6` Reranked scores replace the hybrid score for the reranked
  candidates only; everything then re-sorts together. `RET-2`'s floors must
  be **re-measured on the reranked scale** via `EVL-4` — a cross-encoder's
  0.4 is not a bi-encoder's 0.4, and reusing the old floor here would
  silently change what counts as a match.
- `RRK-UI-1` A second engine card in `components/EmbedEngine/EmbedEngine.tsx`
  beneath the embedder, following `ImageEngine.tsx`'s existing state model
  exactly — `status` / `busy` / `prog` (`DownloadProgress`) / `error`, the
  `engine-state-badge` for Installed / Not installed, `dl-progress wide` for
  the resumable download, `btn-primary` becoming `btn-secondary` once
  installed, `hw-note error` for failures. (The Engine view is
  infrastructure, not an organism surface: PRES-0's no-badges rule governs
  the Self view, the timeline and the mark — here, matching the established
  Engine conventions wins.) Copy: *"Sharper ranking — I re-read the best
  matches before answering. Slower, and it needs another 540 MB."* *(size
  corrected per `RRK-3` above)*
- `RRK-UI-2` States, in full: not installed → `Install reranker`;
  downloading → `dl-progress` + `Stop`; installed but off → a toggle reading
  *"Use it when the best matches are close"*; installed and on → that toggle
  set, plus `Remove`; error → `hw-note error` + `Try again`.
- `RRK-UI-3` **Reranking without the embedder is meaningless.** With no
  embedding model installed the card explains rather than disabling (per
  `EMB-UI-2`): *"Install my recall engine first — there's nothing to re-read
  without it."*
- `RRK-UI-4` When reranking ran, the timeline step's `result` gains
  *"— re-read the closest ones"*. When `RRK-4` skipped it, nothing is said:
  silence is the correct signal for "the ranking was already clear."
  **No score is ever shown** (`RET-UI-4`).

**Exit for RRK:** on an `EVL` fixture where the right passage ranks 4th by
embedding alone, it ranks 1st after reranking; disabling the feature restores
the previous order; killing the rerank engine mid-query still returns
results.

---

# Part IV — Sight — **deferred to a later phase (2026-07-31)**

> **Deferred, not cut.** Nothing below is withdrawn or weakened; the tasks
> stand as written and are the natural next phase. Two things moved them out
> of this one.
>
> First, their size is misleading. `VIS`, `OCR` and `XTR` are among the
> shortest sections in this document, but shortness here reflects *unresolved*
> work, not small work: all three depend on a vision path we have no plumbing
> for — the same from-scratch engine-lifecycle cost that `EMB` turned out to
> be, and `OCR-1` additionally needs PDF rasterisation we do not have.
>
> Second, they cannot be honestly verified yet. `EVL`'s own scope note already
> records that a synthetic PDF or a generated gradient image would exercise the
> plumbing while telling us nothing about whether `OCR` can read a real scan.
> The fixtures those tasks need are real scans and real photos, and until they
> exist an "implemented" `OCR` would be an untested claim.
>
> This phase therefore delivers **recall and retrieval**. Sight follows, with
> its own fixtures, once `IDX`/`RET` have proven the pipeline that captions and
> transcriptions would feed.

Our engine holds **one** chat model at a time (unlike Ollama, which swaps
freely), so vision work is only possible when the loaded model has vision, or
when the user accepts a swap. Every task here states its behaviour when it
cannot see, and that behaviour is always "say so", never "silently skip".

## VIS — Vision captioning on index

- `VIS-1` During `IDX`, an image file is captioned by the loaded model when
  `model_library.vision = 1`: a dense factual paragraph covering subjects,
  people and their actions, **visible text**, setting, and notable detail.
  The caption is the chunk text; it is embedded like any other text.
- `VIS-2` Cache by path + mtime. Never re-caption an unchanged image.
- `VIS-3` No vision model ⇒ the image is recorded as skipped with reason
  *"needs my eyes"*, surfaced by `IDX-UI-2`. Installing a vision model and
  re-reading the folder picks them up.
- `VIS-4` New tool `look_at { path, question? }`: with a question, answer just
  that (far more accurate than a general description); without, describe and
  cache. Distinct from `search_folder` — this is for an image the user
  names or attaches.
- `VIS-UI-1` Captioned images render through `RND` as a `collection` block in
  `components/Blocks/BlockRenderer.tsx`, with thumbnails loaded by the
  existing `components/Conversation/ChatImage.tsx` (it already handles local
  paths). The timeline step reads *"I looked at 3 photos"*; a thumbnail click
  opens the file in `components/Workbench/Viewer.tsx`.

## OCR — Documents that are pictures

Closes a gap our own code already flags: `commands/attachments.rs` notes that
scanned PDFs return nothing and page-image rendering is a follow-up.

- `OCR-1` When a PDF's text layer yields under ~40 characters, rasterise up to
  8 pages and transcribe each with the vision model. Cache by path + mtime.
- `OCR-2` Same honest fallback as `VIS-3` when there is no vision model.
- `OCR-3` Transcribed text flows into `IDX` like any other text — a scanned
  invoice becomes searchable prose.

## XTR — Fields out of a pile of documents

The clearest demonstration that a folder is *understood* rather than listed.

- `XTR-1` Tool `extract_table { fields? }`: over every document and image in
  the attached folder (cap 30), extract the named fields — or auto-detect
  sensible ones — into columns. Receipts → vendor / date / amount. Cards →
  name / company / email.
- `XTR-2` Returns a table `render` (`RND`) plus `sources`, so every row is
  traceable to its file.
- `XTR-3` Missing fields are `null`, never invented. A file that could not be
  read is listed as skipped, not silently dropped.
- `XTR-UI-1` The table is a `comparison` block via `RND`, so
  `BlockRenderer.tsx` gives persistence, interaction state and export for
  free. Each row carries its source path and opens that file in `Viewer.tsx`
  on click. Files that could not be read appear as one quiet line beneath the
  table — *"2 files I couldn't read"* — never as blank rows.

**Exit for Part IV:** point Poiesis at the fixture folder containing three
scanned receipts and ask for a table of vendor, date and amount. It reads
them with its eyes, produces the table inline, and each row links to its
file. With a non-vision model loaded, it says plainly that it cannot see them
and tells the user what to do about it.

---

# Part V — Independent wins

Each is small and each stands alone. Only `PRO` has a prerequisite (`SCP`);
the rest can land in any order after `RND`.

## PRO — The synthesized profile (needs `SCP`)

`SCP` stops narrow notes from reaching unrelated questions. `PRO` solves the
other half of the same problem: once a user has a dozen global facts,
injecting them as a dozen separate lines is both **prefix-cache-hostile**
(any one changing invalidates the cached prefix for every later turn) and
**checklist-shaped** — a list the model may follow partially, where prose
describing a person tends to be applied whole.

A profile is **style only**: how this user likes answers delivered. Never
biography. Heapchat shipped a v1 that also synthesized from facts and
episodes and watched it leak — a marathon-training note changed the word
choice of an unrelated motivational quote. We start at their v2 semantics
and never write the v1.

Note the division of labour with the things we already have: `SOUL.md` is
**standing instructions the user wrote**; global facts are the **raw
observations**; `PRO` is the **agent's synthesis** of those observations into
a few stable sentences. Personas are unrelated — they describe the assistant,
not the user (see `PRO-7`).

- `PRO-1` Generated file `memory/PROFILE.md`, sibling of the generated
  `MEMORY.md` index. Frontmatter: `version`, `updated`, `source_count`,
  `edited`. Body: 1–3 plain sentences, third person, present tense.
- `PRO-2` Synthesis is one local call over **global-scoped preferences and
  instructions only** (`SCP-1`) — never facts, never lessons, never
  episodes. The prompt states explicitly that the output covers tone, format,
  length, language and units, and must not include or infer anything about
  who the user is or what they work on.
- `PRO-3` **Volume gate: 6 global sources minimum** for an automatic build.
  (Heapchat's threshold of 2 is far too eager — a "profile" synthesized from
  two notes asserts more than the evidence supports.) A user-initiated
  rebuild ignores the gate.
- `PRO-4` Rebuild triggers: debounced 8 s after a global preference or
  instruction changes, plus a daily idle tick. Never on every message.
- `PRO-5` `PROFILE_VERSION` constant. A stored profile written under older
  rules reads as **absent** rather than being deleted, so the user can still
  see it and rebuild. A profile with `edited: true` is exempt — the user
  wrote those words and owns them.
- `PRO-6` Injection at the **top** of the system prompt, above lessons and
  recipes, as the slowest-changing block — that placement is what makes it
  cache-friendly. Prefixed *"About you, as I understand it (apply it; don't
  mention it unless asked)."*
- `PRO-7` **Personas outrank the profile.** A persona is a deliberate choice
  the user just made; the profile is a background inference. Where they
  conflict ("be thorough" vs "prefers terse"), the persona wins — and the
  prompt says so in one line rather than leaving the model to arbitrate.
- `PRO-8` Autonomy: a profile is derived entirely from already-approved facts
  and is regenerable, so it sits at the `auto` rung — `AUTONOMY_DEFAULTS` in
  `autonomy.rs` gains `("profile", "auto")`. Applied with undo, never
  silently.
- `PRO-9` **Undo needs a target.** Every rebuild first snapshots the previous
  `PROFILE.md` into the memory store's existing `.snapshots/` directory
  (timestamped, exactly as consolidation already does), so `PRO-UI-5`'s
  `Undo` has something to restore. Without it the `auto` rung's
  "applied with undo" guarantee is unmet — and an auto self-change with no
  undo is really an `ask` change wearing the wrong label.
- `PRO-UI-1` The profile renders at the top of
  `components/Memory/MemoryPanel.tsx` — which is what `SelfPanel.tsx`'s
  `memory` tab already mounts — above the fact list, as prose under
  *"How I think you like to be talked to"*.
- `PRO-UI-2` Controls: `Rewrite this` (→ `rebuild_profile_cmd`, ignoring the
  `PRO-3` volume gate) and `Edit` (inline `<textarea>`, following the fact
  card's existing inline-edit pattern in this same file, → sets
  `edited: true`). While a rebuild runs — a real LLM call, several seconds —
  the prose dims and the button reads `Rewriting…`, disabled. The panel never
  goes blank.
- `PRO-UI-3` States: below the volume gate → *"I haven't formed a picture of
  this yet."* · `edited: true` → a quiet *"you wrote this"* line plus
  `Let me rewrite it`, since an edited profile is exempt from `PRO-5`'s
  version retirement and the user should be able to see that · version-
  retired → treated as absent, with `Rewrite this` still offered.
- `PRO-UI-4` `source_count` surfaces as one plain line — *"drawn from 7
  things you've told me"* — anchoring to the global-scoped facts listed
  below, so the synthesis is never a black box. No percentages, no
  confidence.
- `PRO-UI-5` `components/Memory/MemoryToast.tsx` gains a rebuild variant:
  *"◆ I updated how I picture you — Undo"* (restoring the `PRO-9` snapshot),
  opening the Self view on click. Same 6 s auto-dismiss and one-at-a-time
  rule as the existing toasts.

**Exit for PRO:** save six global preferences across separate chats; a
profile appears in the Self view stating only *how* answers should be
delivered, with no biographical detail; editing it survives a rebuild
trigger; a persona that contradicts it wins in the actual answer.

## PHS — Duplicate and near-duplicate detection

- `PHS-1` 64-bit dHash (downscale 9×8, greyscale, compare adjacent pixels),
  cached by path + mtime. Needs an image-decode dependency in `src-tauri` —
  the `image` crate.
- `PHS-2` Thresholds, taken from heapchat's calibration on ~2.8k real hashes:
  Hamming ≤ 2 identical, ≤ 10 near-duplicate, ≤ 23 visually related.
- `PHS-3` Tool `find_similar { path }` — images by pixel distance, documents
  by centroid-to-centroid cosine over their chunk vectors (whole-file meaning;
  best-chunk matching over-scores files that share one stray paragraph).
- `PHS-UI-1` `components/Workbench/Tree.tsx` gains a folder-level action
  *"Find duplicates"* (→ `find_duplicates_cmd`), rendering groups as a
  `collection` block. Each group offers `Keep this one`; the others go
  through the existing trash path so `components/Workbench/RecentChanges.tsx`
  can undo it. **Never auto-deletes**, and the copy always names which file
  is being kept.

## PER — Per-persona tool sets

Personas already bundle prompt + model + temperature but cannot constrain
tools, so every persona has every capability.

- `PER-1` `personas.tools_json` (schema v7): an allowlist of skill names;
  `NULL` means "all enabled skills", preserving current behaviour.
- `PER-2` Intersect with the global skill toggles — a persona can never
  re-enable something the user switched off globally.
- `PER-UI-1` `components/Personas/PersonaEditor.tsx` gains a checkbox list of
  skills (source: the `list_skills_cmd` data already in the store),
  persisting to `tools_json`. Header copy: *"What this persona of mine may
  do."* A skill switched off globally renders checked-but-disabled with the
  note *"turned off in Settings"*, so `PER-2`'s intersection rule is visible
  rather than mysterious.

## DAT — The sandbox is the data tool

heapchat built a bespoke `query_csv` because it has no code execution. We
have a sandbox, which generalises past anything that tool can do — the work
is making it *reachable*, not building a competitor.

- `DAT-1` Extend the code-execution tool description so spreadsheet and data
  questions route to it explicitly, with the working folder path available.
- `DAT-2` Allow the sandbox to read (not write) files inside the attached
  working folder. *(Built, with the limit stated plainly: a subprocess cannot
  be held to `permissions::gate` the way the file tools are — confining a
  Windows child's filesystem view needs an AppContainer profile, which is not
  in this phase. Reads inside the folder never prompt at any trust level, so
  an ordinary attachment is genuinely covered; a **read-only** folder is
  withheld from the sandbox entirely, since that is the one level where the
  gate refuses writes and the sandbox cannot honour it. Any file the snippet
  changes is named in the activity log after the run, so a write is recorded
  even though it isn't prevented.)*
- `DAT-3` Results return as a table or chart `render` (`RND`).
- `DAT-UI-1` The timeline step reads *"I worked out the numbers"*, with the
  code itself behind the step's existing `⌄` disclosure in `Timeline.tsx` —
  the same control `matches` uses — so it is available on demand and never
  dumped into the answer. Results render as a `comparison` or chart block
  via `RND`.

## CRT — A critic gate on what enters the self

Reflection currently writes lessons at the `auto` rung. One bad lesson
silently degrades every future turn, which is the highest-leverage quality
risk in the whole system.

- `CRT-1` Before a lesson is committed, one local call reviews it against the
  session: is it supported, specific, and generalisable? Contract: *if you
  raise any issue you must set `ok:false`* — plus a regex fallback for models
  that ignore the JSON shape.
- `CRT-2` A lesson that fails is **not discarded — it is demoted**: written as
  a `change_proposals` row instead of a fact, so it reaches the user as a
  proposal rather than vanishing. This is exactly the `Ask` rung in
  `autonomy.rs`, applied dynamically.
- `CRT-3` Record pass/fail in `tool_stats` so `EVL` can track whether
  reflection quality drifts.
- `CRT-UI-1` A demoted lesson is just a `change_proposals` row, so it already
  renders through `components/Conversation/ProposalCard.tsx` and the Self
  view's proposals list — **no new UI**. Only the copy is new: *"I nearly
  learned this, but I wasn't sure enough — should I keep it?"*, with the
  critic's objection shown as the rationale.

---

# Part VI — SCH: the quiet night shift

Poiesis maintaining itself while the user sleeps is the most feelable version
of the whole concept — and it composes every part of this plan.
`autonomy.rs` is the gate this needs; there is no runner yet.

- `SCH-1` A single process-wide ticker (60 s), concurrency **1** — one local
  GPU serialises generation anyway. Cadence presets (hourly / 6-hourly /
  daily / weekly); no cron dependency. *(Built in `commands/scheduler.rs`,
  `spawn_ticker`, mirroring `embedserver`/`rerankserver`'s idle-stop loop
  shape. Enabling a job — at creation or by flipping the toggle — schedules
  its first run at the very next tick rather than a full cadence period out;
  otherwise "enable the nightly job, leave the app open overnight" (the exit
  criterion below) wouldn't fire until the same time the *next* day.)*
- `SCH-2` Jobs: `{ id, name, prompt, cadence, scope (folder|none), enabled,
  next_run_at, last_result }`, stored in settings-backed JSON, not a new table.
  *(Built with four extra fields: `last_run_at` for the task card's "last run"
  line; `built_in`, marking the seeded nightly job as disable-only-not-
  deletable; `runs: Vec<JobRun>`, the last 10 runs as `{ conversation_id, at,
  summary }` so every run stays openable (`SCH-UI-4`); and
  `source_conversation_id`, the chat a task was made out of via "Schedule
  this" (`SCH-UI-6`). Settings-backed JSON held up — the shape changed twice
  during the build, and `#[serde(default)]` on the new fields carried old rows
  forward without a migration either time.)*
- `SCH-3` **Headless runs get no destructive and no interactive tools.** No
  delete, no move, no overwrite, no user prompt — a job that would need one
  stops and reports instead. Renders are skipped (`RND-3`).
  *(Built stricter than the literal list: `run_agent` gained a real `headless`
  flag threaded down to `SkillContext` — the `false` the render-skip code was
  already carrying as a placeholder comment — and the File System skill
  refuses **every** write/edit/create/delete/move outright the moment
  `headless` is set, before `authorize()` is ever consulted. Reads still work.
  This is simpler than replicating `permissions::gate`'s per-trust-level logic
  headlessly, and safer: a silent create/overwrite under `Trust::Auto` would
  have technically matched "no destructive tools" but not "no interactive
  tools" in spirit, since nobody could see it happen either. The sandbox
  (`DAT-2`'s `NEXUS_FOLDER`) is withheld under headless the same way it
  already was under read-only trust, for the same reason.)*
  *(**Fixed after review:** the blanket refusal covered only non-`Read`
  impact, which left the actual hole. A headless **read** is allowed and so
  reached `authorize()`, and if its path fell outside the job's folder — or
  the job had no folder — it landed on the scope-grant prompt and awaited a
  `oneshot` nobody could ever resolve: no timeout, no cleanup, and the cancel
  flag is only read between agent steps, so Stop couldn't reach it either.
  That hung the run forever, holding the concurrency-1 slot, so **no job
  could run again until the app restarted**. `authorize` now takes `headless`
  and refuses at both prompt sites, naming the folder the job was given (or
  saying it was given none). The regression test asserts via
  `tokio::time::timeout` — the failure mode isn't a wrong answer, it's never
  returning at all.)*
- `SCH-4` The ticker consults `autonomy_gate` for every self-change: `Auto`
  applies with undo, `Ask` becomes a `change_proposals` row waiting in the
  morning, `Off` is skipped. Unattended runs never widen the membrane.
  *(This was already true of every call site that writes a self-change —
  `memory_skill`, `recipes`, `reflect` all gate through `autonomy_gate` and
  land in `change_proposals` on `Ask` regardless of who's driving the turn —
  so a headless run gets it for free with no new code.)*
- `SCH-5` Built-in job, off by default: **nightly reflection + digest** — run
  reflection over the day's conversations (`CRT` gating what it writes), then
  compose a short first-person digest. *(Built as `run_nightly_reflection_digest`,
  reading up to 8 not-yet-reflected conversations — a cap, not a target — and
  calling the existing `reflect_conversation_cmd` once per conversation
  unmodified. The digest is composed from the saved/proposed counts, not a
  further model call, so it can never hallucinate what it did.)*
  *(**Fixed after review:** the pass ignored its cancel flag entirely — only
  custom jobs were given it — so `Stop` returned success while all 8
  reflections ran on regardless. The flag is now checked between
  conversations (reflection itself is one indivisible model call, so a
  half-read conversation is not a state that exists), and the digest wording
  moved into `compose_digest`, which reports what was *actually* read: "…2
  conversations before you stopped me", and a distinct sentence for stopped-
  before-anything, which is not the same event as nothing being due.)*
> **The `SCH-UI` tasks below were rewritten on 2026-08-04 to describe what is
> actually built.** They originally placed all of this inside the Self panel
> as a sixth tab, on the premise that "the self is a place, not a settings
> tab". Manual testing killed that premise: the feature could not be found.
> Half the reasoning survives — nightly reflection *is* self-upkeep — but a
> user's own tasks are not part of what Poiesis is made of, they're work it
> was asked to do on a timer. The original wording is preserved in git
> history; what follows is the current design, with the superseded shape
> named where the difference is the point.

- `SCH-UI-1` **Tasks are their own section** in the settings hub, immediately
  after Self (`routes/Tasks.tsx` + `Tasks.css`, registered in
  `SettingsHub.tsx`, `App.tsx`, `Rail.tsx`'s `inSettingsHub`).
  *(Superseded `SelfPanel.tsx`'s Schedule tab, which was deleted along with
  its `ScheduleTab`/`JobEditor` components — Self is back to five tabs.
  Discoverability was the whole reason: three levels deep behind a cog,
  inside a page about identity, as the last of six tabs, is indistinguishable
  from not shipping it.)*
- `SCH-UI-2` The digest reads as an entry at the top of Tasks, not a
  notification: *"Last night I read back over three conversations. I learned
  one thing, and there's one change I'd like to make."* Its proposals need no
  cross-link — they already surface as proposal cards in Self → Lessons.
- `SCH-UI-3` `components/Mark/PoiesisMark.tsx` carries at most one unread
  digest as a slow pulse — **no dot, no count, no badge** — cleared by opening
  Tasks. If the user never looks, nothing nags. *(A slower, shallower variant
  of the existing breathe animation, `mark-breathe-slow` at 5s, shown only
  while otherwise idle so it never competes with the working/reflecting/
  healing states. Clearing is a side effect of opening the section; there is
  no "mark read" button.)*
- `SCH-UI-4` **Each run is its own conversation**, titled `Task · run N` and
  listed in the rail like any other. A task card shows its last runs
  (`Job.runs`, capped at 10) linking straight to them.
  *(Superseded a single hidden conversation reused across runs, reported as a
  400-character `last_result` string. That made a scheduled task a black box:
  the one thing you need from unattended work is to see what it actually did,
  and a summary you have to take on faith isn't it. `last_result` survives
  only as the task card's one-line label.)*
- `SCH-UI-5` A task card carries name, cadence, instructions, folder,
  next/last run, paused state, run history, and `Run now` / `Edit` / `Delete`.
  The editor takes name, instructions, cadence, folder scope and an enable
  toggle. "Run now" shares the ticker's concurrency-1 guard, so it reports
  "another task is already running" rather than racing it.
  *(Cards, not table rows: a task carries more than a row holds without
  becoming a grid you have to decode. The editor is keyed on the task id —
  seeding form state on mount only meant that clicking Edit on a second task
  reused the first one's instance, showing its text and saving it onto the
  second.)*
- `SCH-UI-6` **"Schedule this"** in the Workbench turns the open chat into a
  task: conversation title as the name, its first user message as the
  instructions, `source_conversation_id` recorded so the task remembers where
  it came from. It opens Tasks with the editor already filled in.
  *(In the Workbench because that panel is already "everything about this
  chat", and because the moment you want a recurring task is usually just
  after you've watched the agent do the thing once by hand. It carries the
  first request, not the whole conversation — copying full history into every
  run is a real cost-and-drift decision, deliberately not made silently.)*
- `SCH-UI-7` A task that is *currently running* shows as a quiet row in
  `components/Rail/Rail.tsx` with a `Stop`. The user must always be able to
  see that Poiesis is working unattended, and end it. *(Driven by a backend
  `poiesis-job-started`/`poiesis-job-finished` event pair — `onAppEvent`, the
  same mechanism reflection and healing already announce themselves through —
  since a scheduled run isn't invoked from the UI and so has no open channel
  to stream progress over.)*

**Exit for SCH:** enable the nightly job, leave the app open overnight, and
find a digest in the morning with one applied lesson and one pending
proposal — and an activity log showing the run touched no files.

**Not yet exercised.** Every `SCH` task above is built, unit-tested, and
compiles clean, but the exit criterion is a runtime one and has not been run.
Four of the five defects found in review — including one that hung the
scheduler until app restart — were in code that already compiled and passed
its tests, and the misplaced UI was found by a human opening the app, not by
anything automated.

---

# Part VII — SMP & WHY: one product, not twenty settings

Everything above is sound engineering, and taken together it would put
roughly **thirteen new named things** in front of someone who only wants a
tool that quietly learns how they work — on top of the ten that already exist
(system prompt, persona, soul, facts, lessons, recipes, autonomy rungs, chat
engine, image engine, trust levels). Twenty-plus nouns is not a product; it
is a control panel.

> **Nothing in this part removes a feature.** Every capability in Parts 0–VI
> ships exactly as specified, with every control still reachable. This part
> changes only *how many of them have a name the user must learn before the
> product works*: obvious decisions become automatic, expert controls move
> behind one switch, and one new screen explains the whole machine at the
> moment somebody is actually curious about it.

These tasks **amend** earlier ones. Where an `SMP` task contradicts a task in
Parts 0–VI, `SMP` wins and the earlier task's ID is named so the change is
traceable.

## SMP-1 — Two modes: Simple and Everything

> **In plain words.** Poiesis arrives with everything switched on but most of
> the machinery out of sight. One switch in Settings — *"Show me
> everything"* — reveals the engine internals, the per-note controls, the
> tool lists, the indexed-folder management. Nothing is deleted by being
> hidden; anyone who wants the full machine flips one switch and has it. The
> switch is remembered, and it is the only place in the app where the word
> "advanced" would have appeared.

- `SMP-1a` Setting `ui.expert` (default `false`), exposed once in
  `routes/Settings.tsx` as a labelled switch: *"Show me everything — every
  engine, every control, every setting I usually keep out of your way."*
- `SMP-1b` A store selector `useExpert()` in `lib/store.ts`. Expert-only
  surfaces render `null` when false. **No greying out, no "upgrade to see"
  affordances** — the surface is simply absent, so Simple mode reads as a
  complete product rather than a locked one.
- `SMP-1c` Expert-only in this phase: the Engine → Recall internals
  (`EMB-UI-1`, `RRK-UI-1`/`2`/`3`), the per-fact scope control
  (`SCP-UI-1`), the per-persona tool list (`PER-UI-1`), the indexed-root
  management list (`IDX-UI-4`), and the raw layer text in `WHY-3`.
- `SMP-1d` Nothing that reports *what Poiesis did* is ever expert-only.
  Timeline steps, provenance, skipped-file lists, digests and proposals all
  render in both modes. The switch hides controls, never consequences.

## SMP-2 — Recall installs itself

> **In plain words.** As written, `EMB-UI-1` asks you to find a tab called
> "Engine → Embedding" and install something before Poiesis can remember by
> meaning. Most people will never find it, so their Poiesis stays permanently
> worse without ever knowing there was a choice. Instead: the first time it
> genuinely needs the helper, Poiesis asks for it in one sentence, in the
> place where you already are.

- `SMP-2a` **Amends `EMB-UI-1`.** The Engine → Recall tab still exists and is
  still the place to change or remove the model — it moves behind `SMP-1c`
  and stops being the *only* way in.
- `SMP-2b` First-need prompt: on the first folder attach or the first memory
  write, if no `role = 'embed'` model is installed, Poiesis asks inline —
  *"I can remember and search by meaning instead of just matching words. It
  needs a 130 MB helper that runs on your CPU. Shall I fetch it?"* — with
  `Yes, fetch it` / `Not now`. `Not now` sets `recall.declined` and is never
  asked again automatically; the Engine tab remains available.
- `SMP-2c` The download runs in the background with the existing resumable
  downloader. Poiesis stays fully usable throughout on keyword behaviour
  (`EMB-5`), and says so once when it finishes: *"I can search by meaning
  now."*
- `SMP-2d` **Honest consent.** The prompt states the size and that it uses
  disk and CPU. We do not auto-download without asking — hiding a 130 MB
  download would trade one kind of confusion for a worse kind.

## SMP-3 — "Recall" is one thing with two levels

> **In plain words.** Inside, the embedder and the reranker are two separate
> engines with two model downloads. To you they are one thing called
> **Recall**, with a quality choice: *Good* (the default) or *Sharper* (adds
> the second helper, a bit slower, a bit more disk). Nobody needs to learn
> what an embedder or a cross-encoder is to benefit from either. Both remain
> individually installable and removable for anyone who opens the full view.

- `SMP-3a` **Amends `RRK-UI-1`/`2`/`3`.** In Simple mode there is no separate
  reranker card. The single Recall control offers `Good` / `Sharper`;
  choosing `Sharper` installs and enables the reranker through the same flow.
- `SMP-3b` Copy for `Sharper`: *"Re-reads the closest matches before
  answering. A little slower, and another 540 MB."* The words *embedding*,
  *reranker*, *bi-encoder*, *cross-encoder* and *vector* appear **nowhere in
  user-facing copy** in either mode.
- `SMP-3c` In Everything mode the two engine cards render exactly as
  `EMB-UI-1` and `RRK-UI-1`/`2`/`3` specify, with their own install, model
  picker, progress and removal.
- `SMP-3d` `RRK-4`'s selective-rerank policy is unchanged and never surfaces
  as a setting in either mode.

## SMP-4 — Giving Poiesis a folder means it reads the folder

> **In plain words.** As written, you attach a folder and then have to press
> a second button that says "Read it". But handing over a folder already
> means *work here* — the second button is a decision without a real
> alternative. So it starts reading straight away, tells you plainly that it
> is doing so, and you can stop it at any moment. Two of the five states
> disappear from the interface without any capability being lost.

- `SMP-4a` **Amends `IDX-UI-1`.** On attach, indexing starts automatically.
  The header line goes directly to *"Reading… 34 of 120"* + `Stop`. The
  `Read it` state remains only for a folder whose indexing was previously
  stopped or declined.
- `SMP-4b` **Amends `IDX-8`.** Folder reading stops being a separate skill
  toggle the user must find and enable. Attaching a folder is the consent;
  the existing trust levels still govern what may be *changed*. In Everything
  mode the toggle still appears in Settings → Skills for anyone who wants to
  switch it off globally.
- `SMP-4c` First time only, one line beneath the reading state: *"I read the
  files you give me so I can answer from them. Everything stays on this
  machine."* Then a flag, and never again.
- `SMP-4d` Stopping is remembered per folder — a stopped folder is not
  re-attempted on the next attach, it offers `Read it`.

## SMP-5 — The profile has no name

> **In plain words.** Poiesis writes two or three sentences describing how you
> like to be talked to. That is genuinely useful. But calling it a *Profile*
> puts a fourth thing beside System prompt, Persona and Soul — and those
> three are already the hardest thing in the product to tell apart. So it
> gets no name at all. It is simply the first line of your memory page, with
> the notes it was drawn from listed directly underneath it.

- `SMP-5a` **Amends `PRO-UI-1`.** The words "profile", "synthesis" and
  "summary" appear nowhere in user-facing copy. The block is untitled prose
  at the top of `MemoryPanel.tsx`, under the existing page heading only.
- `SMP-5b` It never appears as a tab, a settings entry, a menu item, or a
  layer name — with one exception: in `WHY-2` it is labelled **"About you"**,
  because a labelled layer there is what *prevents* it becoming a mystery.
- `SMP-5c` `PRO-UI-4`'s *"drawn from 7 things you've told me"* becomes
  load-bearing rather than decorative: it is the only explanation of where
  the sentences came from, so it always renders and always anchors to the
  notes below.
- `SMP-5d` All `PRO` backend tasks (`PRO-1`…`PRO-9`) are unchanged. This is
  purely a naming and placement amendment.

## SMP-6 — Scope is decided for you, and still yours to change

> **In plain words.** Every note is either something that applies to every
> answer ("be concise") or something that only matters when a subject comes
> up ("when I ask about pricing, show the currency"). Poiesis works this out
> itself. Putting a control for it on every single note invites a question
> most people never wanted. So each note still *says* which kind it is in
> plain words — nothing is hidden — but the control to override it moves into
> the full view.

- `SMP-6a` **Amends `SCP-UI-1`.** Simple mode renders scope as a plain,
  non-interactive phrase on each fact card: *"applies to every answer"* /
  *"only when it's relevant"*. Everything mode renders it as the two-option
  control (→ `set_fact_scope_cmd`) exactly as `SCP-UI-1` specifies.
- `SMP-6b` A reactive path in both modes: the `WHY-2` panel lists what was
  injected and why, so *"why did you bring that up?"* is answerable without
  the user ever having seen a scope control.
- `SMP-6c` `SCP-1`…`SCP-4` are unchanged.

## SMP-7 — Each ability explains itself once, when it first happens

> **In plain words.** Nobody reads a settings page to learn how a tool
> thinks. So no capability here is explained up front. Each one says one
> sentence about itself the first time it actually occurs — the first time
> Poiesis remembers something, the first time it reads a folder, the first
> time it recalls something in a later chat, the first morning digest — and
> then never mentions it again.

- `SMP-7a` Generalise the existing `MEM-UI-4` pattern into one helper:
  `firstTime(key, message)` in `lib/store.ts`, backed by settings flags
  `onboarded.<key>`, rendering through the existing
  `components/Memory/MemoryToast.tsx` one-at-a-time queue.
  *(Built as `maybeFirstTime(key, message)`, self-clearing on a timer rather
  than component unmount so it can't get stuck behind a longer-lived toast.
  `folder.first` (`SMP-4c`, `indexExplained`) and the original `MEM-UI-4`
  explainer (`memoryOnboarded`) were left as their existing bespoke
  implementations rather than migrated: both already render *inline, where
  the ability itself is shown* — beneath the reading-progress line, attached
  to the write receipt — which is a better fit for their moment than the
  toast shell, and migrating two already-correct behaviours for uniformity
  alone risked regressing something that worked. The helper covers every
  *new* first-time explanation instead.)*
  *(**Fixed after review:** the flags were loaded fire-and-forget "a beat
  later", after `bootstrap` had already kicked off `refreshChangeProposals`
  and `refreshScheduler` — both of which call the helper. An unloaded
  `firstTimeFlags` is an empty object, which reads exactly like "never
  explained", so a first-time line could re-show on later launches, and the
  late `set` then clobbered the flag it had just written. The load moved into
  bootstrap's existing settings batch (no extra latency — it was already
  awaiting fourteen of these), plus a `firstTimeFlagsLoaded` guard so the
  helper stays silent rather than guessing if anything ever calls it earlier.
  An explanation that can't tell "first time" from "again" is worse than
  none.)*
- `SMP-7b` The full set of first-time lines in this phase — `recall.first`
  (*"I brought that up because I remembered it from an earlier chat."*),
  `folder.first` (`SMP-4c`), `retrieval.first` (*"That came from your files —
  the names under my answer show which."*), `digest.first`, `proposal.first`.
  *(Built: `recall`/`retrieval` share one `AgentEvent::Recall` payload in the
  backend, told apart in `lib/store.ts` by the hit's own `source` field
  (`"file"` vs `"chat"`/`"memory"`) rather than needing two backend events.
  `digest.first` and `proposal.first` fire from `refreshScheduler`/
  `refreshChangeProposals` respectively — the moment either is actually shown
  to the user, whether that came from a live turn or from data already
  waiting at bootstrap.)*
- `SMP-7c` **Never more than one per session.** If two would fire, the second
  waits for the next session. A burst of explanations on first launch is the
  exact failure this task exists to prevent.
- `SMP-7d` All first-time flags are resettable from Everything mode:
  *"Explain things to me again."* *(Built in `routes/Settings.tsx`'s
  Interface section, visible only when `ui.expert` is on.)*

## SMP-8 — Plain words, enforced

> **In plain words.** A rule about vocabulary, so the interface never leaks
> engineering language into the user's head.

- `SMP-8a` Banned in all user-facing copy, both modes: *embedding, vector,
  index (as a noun), reranker, cross-encoder, bi-encoder, chunk, RAG,
  semantic, cosine, threshold, corpus, OCR*.
- `SMP-8b` Their replacements, used consistently: **recall** (the ability),
  **read / has read** (indexing), **my eyes** (vision), **notes** (facts),
  **what I learned** (lessons), **how I work** (recipes).
- `SMP-8c` Enforced by an `EVL` case that greps the built frontend bundle for
  the banned list and fails on a hit — the only way a copy rule survives
  contact with a year of edits. *(Built as `tests/copy_lint.rs`, scanning
  frontend **source** rather than the built bundle — a deliberate deviation,
  found necessary by actually running the literal version against a real
  `vite build` output first: a minified bundle is full of true whole-word
  hits that aren't copy — SVG attributes (`vector-effect`), CSS class names,
  settings keys, React's own internal `.index` property accesses — and
  telling those apart from real prose needs a JS parser, not a grep. Source,
  once comments are stripped, only keeps a banned word if it sits inside a
  same-line quoted string that also contains a space (a multi-class
  `className="a b-index"` value is explicitly excluded from that rule too,
  since it's a token list, not a sentence) — checked against every false
  positive the bundle version produced. Running it turned up two genuine
  violations, both in `lib/store.ts`'s memory-notes system-prompt strings
  (shown verbatim by `WHY-3` in Everything mode) — fixed alongside the test.)*

## WHY — "What I'm working from"

> **In plain words.** This is the one screen that makes everything else
> understandable, and it replaces having to teach any of it. It shows, in
> plain labelled layers, exactly what is shaping the answer: your standing
> instructions, the persona you picked, what Poiesis thinks about how you
> like to be talked to, what it remembered, what it learned before, and which
> files it read. Each layer opens and closes. There is a small line under the
> composer showing what is currently active, and a *"why this answer?"* link
> on any reply you're unsure about. Nobody has to understand the layers in
> advance — they can look at the moment they wonder.

*(This is the layered-prompt preview scoped for `POIESIS_PLAN.md`. Phases
10–11 are implemented and that document is now historical reference, so the
task lives here in full, covering both its layers and this phase's.)*

- `WHY-1` Backend: `context_manifest_cmd(conversation_id, message_id?)`
  returning the composed prompt **broken into labelled layers**, in
  composition order: `soul`, `persona`, `about_you` (`PRO`), `remembered`
  (recalled facts), `learned` (recalled lessons), `procedures` (recalled
  recipes), `from_files` (`RET` passages), `session` (summary + recent
  turns). Each layer carries `{ label, text, sources[], always_on: bool }`.
- `WHY-2` A compact manifest is stored **per assistant message** so a past
  answer can be explained, not just the live one: new column
  `messages.context_json` (schema v7) holding slugs and paths only — persona
  id, fact/lesson/recipe slugs, file paths, soul revision — roughly 200
  bytes, not the prompt text. `WHY-1` rehydrates display text from those
  references.
- `WHY-3` New `components/Context/ContextPanel.tsx` + `Context.css`: one
  collapsible row per layer, layer label and a one-line summary collapsed,
  full text expanded. Always-on layers are marked *"in every answer"*;
  retrieved layers are marked *"brought in for this question"* — that
  distinction is the entire soul-versus-recall lesson, taught by a label
  rather than by documentation. Raw assembled text is available as one final
  `Everything mode` row (`SMP-1c`).
- `WHY-4` Entry points, all leading to the same panel:
  - a chip under the composer reading `Soul · The Editor · Notes` (the active
    stack, live) — click to open;
  - a quiet *"why this answer?"* link on any assistant message — opens the
    panel for that message's stored manifest;
  - a row in `components/Self/SelfPanel.tsx` — *"What I'm working from"*.
- `WHY-5` Empty and honest states: a layer with nothing in it renders as
  *"nothing from here"* rather than being omitted, so the user learns the
  layer exists even when it is empty. A message predating `WHY-2` shows
  *"I didn't record this one"* rather than a reconstruction that might be
  wrong.
- `WHY-6` The panel is **read-only**, but every layer links to where it is
  edited — soul and notes to `MemoryPanel.tsx`, persona to
  `PersonaEditor.tsx`, files to `Viewer.tsx`. It explains; it never becomes a
  second place to configure things.
- `WHY-7` a11y: rows are a `role="list"` of disclosures with `aria-expanded`,
  matching `Timeline.tsx`'s existing pattern; the composer chip has an
  `aria-label` naming the full active stack.

**Exit for Part VII:** a first-run user attaches a folder, is asked once about
the recall helper, watches the folder being read, gets an answer from their
files, and clicks *"why this answer?"* to see six labelled layers — without
having opened Settings once, and without the words *embedding*, *vector* or
*index* appearing anywhere on screen. Flipping *"Show me everything"* reveals
both engine cards, the scope controls, the persona tool lists and the raw
prompt text, with nothing else changed.

---

# Acceptance for the phase

The phase is done when this holds end to end:

1. Install the embedding engine from a cold app; the chat engine's VRAM is
   unchanged.
2. Teach Poiesis something in one chat; bury it under 39 unrelated lessons;
   watch it surface unprompted in a later chat, announced in the first person.
3. Attach a folder of mixed documents; let Poiesis read it; ask a question
   using none of the documents' vocabulary and get the right passage, with a
   quiet line naming the files it came from.
4. Ask something the folder does not contain and be told so plainly.
5. After six global preferences have accumulated, find a profile in the Self
   view that describes only *how* you like answers — no biography — and watch
   a persona override it where the two disagree.
6. Turn the embedding engine off entirely and confirm Poiesis still works —
   every feature degrades to keyword behaviour and says which one it's using.
7. **The first-run path, start to finish, without opening Settings once:**
   attach a folder → be asked once about the recall helper → watch the folder
   being read → get an answer from those files → click *"why this answer?"*
   and see the labelled layers. The words *embedding*, *vector*, *index*,
   *reranker* and *chunk* appear nowhere on screen.
8. Flip *"Show me everything"* and confirm both engine cards, the per-fact
   scope controls, the persona tool lists and the raw prompt text all appear,
   with nothing else about the app changed.
9. `cargo test` and `cargo test --ignored eval` both pass; `npx tsc --noEmit`
   is clean.

`RRK` is deliberately absent from this list: it is optional and off by
default, so the phase must be complete and shippable without it. Its own exit
check governs it.

The scanned-receipts check that used to sit at item 5 moves with **Part IV**
to the following phase, along with that part's own exit criterion. Deferring
it is what keeps this list honest: every item above can be demonstrated with
the fixtures we actually have.

**Nothing in Part VII is a cut.** Every control specified in Parts 0–VI is
still reachable — items 8 and 9 exist together precisely to prove that the
simple path and the complete path are the same product.
