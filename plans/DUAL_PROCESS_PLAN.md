# Project Poiesis - Dual-Process Plan

**Think fast by default. Think deeply when it matters. Check what matters.
Learn from what actually happened.**

Poiesis has one way of thinking: a full generative turn. It uses it for
everything, including questions that only need a yes, a no, or a pick from a
list. "Do these excerpts answer the question?" costs a whole chat turn, comes
back as prose, gets parsed with `contains("yes")`, and carries no sense of how
sure it is. The questions the harness *should* ask before every turn (which
tools does this need? is this call what the user asked for?) are never asked,
because a chat turn per question is too expensive for the hot path.

Decision models (the "System One" class) fix the cost. You give one a state
and typed questions, and it returns every answer at once, each with a
probability, in tens of milliseconds. **Clef-flash** (Cloudflare, 9B,
Apache-2.0, text and vision) runs on the `llama-server` binary Poiesis already
ships, behind `/v1/systemone`. The same contract is what OpenRouter's
Decisions API speaks for ~20 cloud decision models, Clef included.
**Jev 1.14** (TypeSafe, released 2026-10-14) is the main alternative: text-first,
cloud-only, now with experimental opt-in image input (`DEC-8`).

A fast model alone is just a cheaper oracle, though. This plan builds the
**dual-process runtime** around it:

- a **fast plane** (the decision model),
- the existing **slow plane** (the LLM turn),
- the existing **evidence plane** (retrieval, web, check runs),
- the existing **deterministic gate** (trust levels, impact classes),
- and two new things joining them: a **controller policy** that decides when a
  fast answer is enough and when to escalate, plus an **outcome record** that
  learns from what the user actually did.

Everything that changes behaviour ships behind a new **Settings → Experiments**
section. That section is also where Workspace mode moves.

> Merges `plans/JUDGE_PLAN.md` (the decision seam and its use sites, written
> 2026-09-21) with `plans/dual_process_agent_runtime_concept.md` (DPAR, the
> fast/slow architecture). Both are superseded by this file. The concept doc
> stays as background reading.
>
> ID prefixes: **DEC** the decision engine and seam - **CTL** controller and
> outcomes - **USE** a place that uses it - **EXP** the Experiments framework -
> **-UI** frontend - **-T** tests.
>
> **Status: draft v1, not started.** Written 2026-10-13. Revised 2026-10-20:
> Jev 1.14 added as a candidate decision model (`DEC-8`, `CTL-T-5`).

---

## What this is not

- **Not a text generator.** Anything that produces prose stays on
  `drive_turn`: query rephrasing, profile synthesis, consolidation, lesson
  drafting, skill healing, compaction.
- **Not a security boundary.** A 9B model can be talked into things. The
  deterministic gate stays in ordinary Rust. A decision may *add* caution on
  its own, but it may only *remove* a safeguard under the narrow conditions in
  `USE-12`, and only with an experiment the user turned on.
- **Not a new place data goes.** The default backend is local. The cloud
  backend (OpenRouter Decisions, using the key already in Providers) stays off
  until chosen, per the rule in `memory_skill.rs` that calls the user did not
  ask for never leave the machine.
- **Not a new provider.** There is no Cloudflare account and no TypeSafe key.
  Every cloud decision model, Clef and Jev included, goes through OpenRouter.
  TypeSafe's own API (`api.typesafe.ai`) is out of scope; see the open
  question on header pass-through.
- **Not a research harness in the product.** DPAR's ablation matrix, blind
  review and anchoring studies belong in `tests/eval` as gates for shipping
  (`CTL-T-3`), not as features.
- **Not a Python sidecar.** Clef's reference code is PyTorch/SGLang. We use
  llama.cpp's port of it.
- **Not a swarm.** One LLM, one decision model, one controller.

---

## Who feels what

Before any architecture, here is what the user is supposed to notice. Each
item names the `USE-*` it comes from. A decision nobody notices is plumbing,
not a feature.

**1. "My small local model stopped flailing."** Mara runs Qwen 4B locally and
asks "what's on my calendar tomorrow?". Today the model sees thirty tool specs
and sometimes searches the web first. With the experiment on, the run header
shows a muted chip, `4 tools held back`. The model calls the calendar tool
straight away, and the first token arrives sooner because the prompt is
shorter. Hovering the chip shows *web 0.03, code 0.02, browser 0.01, mail
0.06*. When the model does need one of them later, it asks for it, the chip
updates to `3 tools held back`, and nothing breaks. *(USE-4)*

**2. "Monday inbox in one glance."** Forty new mails overnight. In Mail each
row now has a thin urgency bar on its left edge and a tag (`work`, `receipt`,
`newsletter`). A "Needs reply" filter shows the six that do. Nothing was moved
or deleted. Mara retags one newsletter as `work`, and that correction becomes a
label the system calibrates against. *(USE-6, CTL-3)*

**3. "Sort these for me."** "Here are 140 photos from the trip. Tag each with
the city." The assistant calls `classify` once. Clef reads every image,
returns a table block with city, confidence, and a "low confidence (12)"
filter, and the LLM writes only the summary. That takes about twenty seconds
instead of the minutes 140 generated captions would cost. The same works for
"which of these 60 files touch auth?" or "which tickets are bugs?". *(USE-9)*

**4. "Is that actually true?"** A research answer cites five sources. Mara
clicks **Verify** under the message. Claims get underlined one by one: green
for supported, amber for insufficient evidence, red for contradicted. Hovering
a red one shows the passage that contradicts it and the source. Only the two
disputed claims got a slow review; the other eleven were settled fast.
*(USE-10)*

**5. "It knew I meant stop."** Mid-run, Mara types "no wait, forget it". The
message doesn't sit silently in the queue. Under it appears **Stop the run?**,
answered with one click. "also add the tests" queues as before, with a small
`addition` tag. *(USE-5)*

**6. "The plan ticks itself off, and says so."** After a long coding answer,
three of five plan items show a ringed tick ("judged complete from the
answer, 0.88"), so the nudge mentions only the two that are really open.
Clicking a ringed tick reopens it, and that click is a label too. *(USE-5)*

**7. "It asked me for the right reason."** A permission prompt for
`send_mail` now carries one line: `Flagged: the recipient isn't in this
conversation (0.91)`. A prompt that trust rules would have skipped but the
judge added says `Asked because this looked irreversible`. *(USE-12)*

**8. "My scheduled job only runs when it should."** The nightly "summarise
new releases" task has a condition, *only if there is a new release*. On
quiet nights Tasks shows `skipped: condition not met (0.07)`. *(USE-7)*

**9. "The browser agent stopped clicking blindly."** After each click, the
browser agent asks Clef about the screenshot: *Did the page change? Is there
a captcha or a login wall?* A login wall stops the agent and hands over to
Mara, instead of it typing into the wrong field. *(USE-11)*

**10. Quieter background.** Reflection skips its full drafting pass on
conversations where nothing was corrected and no tool failed. Activity shows
`nothing to learn`, and the laptop fan stays off. *(USE-8)*

---

## Where Poiesis stands

### DPAR components that already exist

| DPAR component | In Poiesis today | What this plan adds |
|---|---|---|
| Slow deliberation plane | `drive_turn`, plans, subagents | nothing |
| Fast decision plane | **missing** | `DEC-*` |
| Evidence plane | `retrieval.rs`, rerank, web, check runs, `unverified_note` | used as an escalation target (`CTL-2`) |
| Deterministic action gate | trust levels, impact classes, permission prompts | a reason line, and the ability to *add* prompts (`USE-12`) |
| Budgets / stop limits | harness budgets | a decision budget per turn (`CTL-2`) |
| Trace log | session log (`agent/log.rs`) | `decision` rows (`DEC-7`) |
| Outcome memory | reflection, golden evals | outcome labels from user actions (`CTL-3`) |
| Metacognitive controller | scattered heuristics | one policy table (`CTL-1`) |

### Small questions asked badly today

| Site | Question | Where |
|---|---|---|
| RET-4 | Do these excerpts answer the query? | `agent/retrieval.rs` `judges_sufficient` |
| SCP-1 | Does this fact apply to every answer? | `agent/memory_skill.rs` `classify_scope` |
| Reflect critic | Is this lesson supported, actionable, right? | `commands/reflect.rs` |

All three use `drive_turn`, temperature 0, prose back, a substring parse, and
`None` on anything odd.

### Questions nobody asks

- Which toolsets does this turn need? (`run.rs` `build()` advertises every one.)
- Did the answer finish plan item N? (`nudge_unfinished_plan` goes by plan state.)
- Is this steering message a stop, a redirect, or an addition? (`fleet.rs` queues text.)
- Is this conversation worth a reflection pass?
- Is this mail urgent, and does it need a reply? (`mail.rs` has no triage.)
- Is this tool call risky, and is it what the user asked for?
- Did that browser click work? (The browser agent has no visual check.)

### Shell facts to build against

- `manifest.rs` pins `llama-server` at **`b10333`** (released 9 August). That
  build has **neither** `/v1/systemone` **nor** Clef. "A pin is a ceiling on
  which models can run."
- llama.cpp merged `/v1/systemone` (PR #29818) and Clef text-only (PR #29831,
  commit `99b9548`, merged 2026-10-03). The conversion script (`clef.py`)
  maps the joint-head tensors *into* the GGUF. Clef vision is PR #29969.
  **Verify before `DEC-1`:** the first `bNNNNN` tag containing all three, and
  that #29969 is merged.
- `ggml-org/Clef-Flash-GGUF` and `ggml-org/Clef-GGUF` are the upstream
  conversions. **Third-party GGUFs uploaded before the merge have no head**:
  they load, then generate text instead of decisions. The catalog must pin
  files, not repos (`DEC-3`).
- Clef-flash 9B: BF16 is about 19 GB, 4-bit is about 6–7 GB peak. Clef 27B:
  4-bit is about 17–20 GB. The release code's state cap is **16,384 tokens**.
  The advertised 64k is unverified, so plan for 16k.
- `rerankserver.rs` / `embedserver.rs` are the lazy-start / idle-stop sidecar
  pattern. One model per `llama-server`.
- `runtime/hardware.rs` knows VRAM and RAM.
- OpenRouter keys live in Providers (`cloud/mod.rs`).
- **OpenRouter Decisions API** (alpha, checked 2026-10-13):
  - **Request:** `POST https://openrouter.ai/api/alpha/decisions` with a
    Bearer key.
    - Required: `model`, `state` (string | object | array) and `questions`
      (name → `{type, instructions, criteria}`). `noul` criteria **must**
      have both `true` and `false`; `choice` criteria are a map; `score`
      criteria are an array.
    - Optional: `provider` (the same routing object as chat: `zdr`,
      `data_collection`, `order`, `only`, `sort`, `max_price`,
      `preferred_max_latency`, …), `session_id`, `trace`, `user`.
  - **Response:** `id`, `model` (the resolved version), `provider`, and
    `answers`:
    - `noul` returns `noul`, the probability of true.
    - `choice` returns `choice`, `confidence` and `probabilities`.
    - `score` returns `score`, `confidence`, `probabilities` and `legend`.

    Also `usage` with `input_tokens`, `output_tokens` and `cost`.
  - **Errors:** 400, 401, 402 (credits), 403, 404, 413, 429, 500, 502, 503,
    524, 529 (overloaded).
  - **Catalog:** `GET /api/v1/models?output_modalities=decisions` (public)
    lists the decision models with `id`, `canonical_slug`, `context_length`,
    `input_modalities` and `pricing`. Prices are input-only; output is free.
    OpenRouter's own guidance: pin the `canonical_slug`, not an alias, and
    log the returned `model`.
  - **Images: not in the published schema.** The catalog marks Clef, Clef-flash,
    Clef-omni, Perplexity Decider and GPT-6 Luna as `text+image`, but the
    request schema has no image field. **Verify before `USE-11` via cloud:**
    one probe call with an image in `state` (a content-part array or a data
    URL). **Jev is the opposite case:** its images work on TypeSafe's own API
    (an `assets` map plus a `TypeSafe-Danger-Zone-Images: true` header), but
    OpenRouter's schema documents neither, and its catalog entry does not list
    images. See `DEC-8`.
- **Decision models on OpenRouter today** (live catalog, 2026-10-13; this
  changes weekly, so the app reads it at runtime and never hard-codes it):

  | Model | Input | Context | $/M input | Note |
  |---|---|---|---|---|
  | `cloudflare/clef-flash` | text+image | 16k (PrimeIntellect) / 24k (Cloudflare) | 0.021–0.038 | same weights as local default |
  | `cloudflare/clef` | text+image | 16k | 0.042 | |
  | `cloudflare/clef-omni` | text+image | 64k | 0.15 | MoE 30B-A3B on Qwen3-Omni; open weights (audio/video locally) |
  | `typesafe/jev-1.14` (dated slug `typesafe/jev-1.14-20261014`) | text; images experimental, opt-in | 262k listed, 64k by default (`DEC-8`) | 0.042 | released 2026-10-14; adds `box`; cloud only |
  | `typesafe/jev-1.13` (`~typesafe/jev-latest`) | text (images via header) | 64k | 0.042 | most used by far; `jev-latest` moves to 1.14 about a week after release; 60 days' notice before 1.13 is deprecated |
  | `perplexity/pplx-decider-v1.1-27b` | text+image | 262k | 0.02 | |
  | `openai/gpt-6-luna-decisions` | text+image | 1.05M | 0.10 | moderated |
  | `inception/mercury-decide` (+`:free`) | text | 64k | 0.02 / free | |
  | `microsoft/microsoft-decision-1` | text | 32k | 0.042 | |
  | `nace-ai/drex-v2.0`, `drex-v1.5` | text | 131k | 0.06 / 0.04 | |
  | `upstage/solar-decide`, `-flash` | text | 524k | 0.05 | |
  | `liquid/d1` | text | 64k | 0.04 | |
  | `togethercomputer/tev1-4b-experimental`, `jaredpalmer/kev-4b`, `respan/span-01` (+lite) | text | 8k–32k | ≤ 0.042 | small or experimental |

  For scale: 10,000 decisions of ~1k tokens on Clef-flash cost about
  **$0.21**.
- Settings are a key/value store (`get_setting_cmd` / `set_setting_cmd`).
- `golden.rs` cases carry the tools a correct reply uses, which gives a ready
  label set for `USE-4`.
- Schema is v30.

---

## Settled decisions

1. **One seam, two transports.** Our types mirror `noul` / `choice` /
   `score`. There are exactly two backends:
   - **local** (`llama-server` `/v1/systemone`)
   - **cloud** (OpenRouter `/api/alpha/decisions`)

   Their bodies differ only in small details (`DEC-5`).
2. **Local by default; Clef-flash is the default model.** Clef 27B is offered
   when the hardware allows it. In the cloud, the default model is also
   **`cloudflare/clef-flash`**, so calibration fitted locally is a good
   prior there. **Jev 1.14 is a candidate, not the default:** it becomes the
   default cloud model only if `CTL-T-5` says so (`DEC-8`). With no local decision engine and no cloud backend chosen,
   the seam returns `Unavailable` and every caller keeps today's behaviour.
3. **Uncalibrated means advisory.** Without a fitted calibration for a
   purpose, a decision may only make things *stricter* or *cheaper*, never
   looser.
4. **Every answer carries its confidence and its backend** to the caller and,
   wherever the user sees the effect, to the UI.
5. **The fast plane never blocks the stream.** Hot-path purposes have a
   deadline. Missing it means "no change", not a stall.
6. **No cold start on the hot path.** The decision engine pre-warms when a
   chat turn starts (`DEC-4`). If it isn't warm in time, the hot-path
   decision is skipped.
7. **The controller is a table, not a formula.** Per-purpose rules in
   ordinary code (`CTL-1`). DPAR's weighted `escalation_score` is deferred
   until logs show the table isn't enough.
8. **Outcomes beat rationalisations.** Calibration comes from what the user
   did (`CTL-3`), not from asking a model whether it was right.
9. **Behaviour changes ship as experiments** (`EXP-*`). Phase 1 (the swap of
   the three existing sites) is the exception: it is a pure, invisible
   replacement gated by parity tests.
10. **Cloud model choice is live, not hard-coded.** The cloud model list comes
    from OpenRouter's catalog at runtime. We pin the `canonical_slug` the user
    picked and log the `model` each response reports. A new version is a new
    calibration key.
11. **Cloud privacy defaults.** Every cloud decision request carries
    `provider: { data_collection: "deny", zdr: true }`. If no endpoint for the
    chosen model meets that, the backend reports unavailable with the reason.
    It does not silently relax.

---

## Phase 1 - The decision engine

### `DEC-1` Engine pin bump

Move `PINNED_BUILD_TAG` to the first build containing #29818, #29831 and,
when it is merged, #29969. A pin bump re-opens every fact other engines
verified against the old build. Re-check:

- `rerankserver.rs`: the `--reranking` flag, and raw logits in `/rerank`.
- `embedserver.rs`: `--embeddings --pooling mean`.
- The CUDA zip naming in `manifest.rs` (`cuda-12.x`).
- Chat streaming and tool-call format (golden gate, prompt-assembly fixture).

This is its own commit, with its own test pass, before any decision code.

### `DEC-2` Seam types (`agent/decide/mod.rs`)

```rust
pub enum Question {
    Noul   { instructions: String, when_true: String, when_false: String },
    Choice { instructions: String, options: Vec<(String /*id*/, String /*criteria*/)> }, // ≤ 255
    Score  { instructions: String, levels: Vec<String> },                               // 2..=10
}
pub struct Ask<'a> {
    pub state: &'a str,
    pub images: Vec<ImageRef>,           // empty unless the backend reports vision
    pub questions: Vec<(&'a str, Question)>,
}
pub enum Answer {
    Noul   { p_true: f32 },
    Choice { choice: String, probs: Vec<(String, f32)>, confidence: f32 },
    Score  { score: f32, probs: Vec<f32>, confidence: f32 },
}
pub struct Verdict {
    pub id: DecisionId,                  // for outcome labels (CTL-3)
    pub answers: BTreeMap<String, Answer>,
    pub backend: BackendId,              // Local{model} | OpenRouter{model, provider}
    pub calibrated: bool,
    pub ms: u32,
}
pub enum DecideError { Unavailable, Deadline, NoVision, Failed(String) }
```

`noul` always carries both criteria, because OpenRouter requires both. Every
`Choice` that can be wrong in a costly way includes an `unsure` /
`escalate` option. Never force a substantive answer (DPAR §5.2).
`confidence` = 1 − normalised entropy. Callers never re-derive it.

### `DEC-3` Decision model catalog

Same shape as `RerankCatalogEntry`, with entries pinned to exact files:

| Entry | File | Size | Needs |
|---|---|---|---|
| Clef-flash (default) | `ggml-org/Clef-Flash-GGUF` Q4_K_M + vision projector | ~6–7 GB | 8 GB VRAM, or CPU (background only) |
| Clef | `ggml-org/Clef-GGUF` Q4_K_M + projector | ~17–20 GB | 24 GB VRAM |

The download path is the existing `download.rs`. After download, a **head
check** sends one fixed systemone request and requires a well-formed answer.
A GGUF that answers with generated text instead is rejected with a clear
message: "This file has no decision head."

User-facing copy follows the `SMP-8a` rule: no "logits", "joint head" or
"System One". It is a **decision model** that makes **quick decisions**.

### `DEC-4` `decisionserver.rs` - the fourth sidecar

Mirrors `rerankserver.rs`: lazy start, idle stop (5 minutes), one instance,
the same shared binary.

- **Placement** (`hardware.rs`):
  - `Auto` (default): GPU if free VRAM ≥ model + 1 GB headroom *with the chat
    model loaded*, otherwise CPU.
  - `GPU` / `CPU` overrides.
  - On CPU, hot-path purposes are disabled (`CTL-1` marks them
    `needs_fast: true`) and only background purposes run.
- **Pre-warm:** when a chat turn starts and any hot-path purpose is enabled,
  start the engine if it isn't running. The first turn after a cold start
  doesn't get hot-path decisions (decision 6). Later turns do.
- **Vision:** reported by the health probe (does the request accept
  `images`?). Purposes that need vision check `backend.vision`.
- **Queueing:** background asks wait behind any active chat stream when the
  decision engine shares the GPU with it. They never take a slot from a
  streaming turn.

### `DEC-5` `decide::ask` entry point and backends

`decide::ask(ctx, ask, opts) -> Result<Verdict, DecideError>`, where `opts`
is `{ purpose, deadline }`. Purpose is looked up in the controller table
(`CTL-1`) for the backend, deadline and calibration.

Two backends, one request builder:

- **Local:** `decisionserver.rs`, `POST /v1/systemone`, no auth. Images go in
  the systemone `images` field (base64 data URLs).
- **Cloud:** `POST https://openrouter.ai/api/alpha/decisions`, using the
  OpenRouter key from Providers.
  - `model` = the pinned `canonical_slug`.
  - `provider` = the privacy defaults (decision 11), plus
    `preferred_max_latency` for hot-path purposes.
  - `session_id` = a per-run random ID, never the conversation ID or title.
  - Images use whatever shape the `DEC-5a` probe confirms.
  - `usage.cost` goes into Usage.

Differences the builder handles:

- `noul` always sends both `true` and `false` criteria.
- `score` criteria are an array.
- The response's `noul` field maps to `p_true`.
- `legend` is kept for display.

**Resolution:** the purpose override, then the global `decide.backend`
(`local` | `cloud`), then `local`. Fallback only goes cloud → local.

**Cloud errors:**

- 401 / 403: mark cloud "needs attention" (shown in Runtime), with no retry.
- 402: "out of OpenRouter credits", fall back to local, with one Activity
  line.
- 429 / 502 / 503 / 524 / 529: fall back to local for this call, and after 3
  in 5 minutes, for 10 minutes.
- 400 / 413: a bug or an oversize state, so log `Failed` with the body. No
  fallback loop.

**State limit:** the smaller of the model's *effective* context (local: 16k;
cloud: from the catalog, e.g. 16k for Clef via PrimeIntellect) and a
per-purpose cap. "Effective" matters for Jev: the catalog lists 262k, but
that needs an opt-in header and is slower and less accurate, so the default
cap is 64k until `DEC-8` confirms the header gets through. Truncation is from the middle with a marker, and is logged.

#### `DEC-5a` Cloud catalog and probe

- The catalog is fetched from `GET /api/v1/models?output_modalities=decisions`
  when Runtime opens and once a day, and cached in settings. It holds each
  model's slug, context length, input modalities, price, and provider count.
- **Probe** ("Test" button, and automatically on first selection): one fixed
  three-question request (a `noul`, a `choice`, a `score`). It must return
  well-formed answers within 5 s. For models listed as `image`, a second
  probe with a tiny test image establishes whether, and how, images are
  accepted. The result is stored per slug (`vision: yes | no | unknown`).
  For Jev the image probe is run twice: once as for the others, once with
  the `assets` shape and the `TypeSafe-Danger-Zone-Images: true` header
  (`DEC-8`). Also probed for Jev: whether `TypeSafe-Danger-Zone-Long-Context`
  gets through (send a ~70k-token state; success means the header was
  forwarded).

### `DEC-6` Move the three existing sites onto the seam

- `retrieval::judges_sufficient` → `Noul`, purpose `RetSufficiency`, giving
  `Some(p_true >= 0.5)`. The probability goes into the warning text.
- `memory_skill::classify_scope` → `Choice { global, topical, unsure }`.
  `unsure`, or confidence below 0.6, stores `None` (the backfill retries).
- Reflect critic → `Choice { ok, unsupported, unactionable, wrong }`. The
  reason is the winning option's criteria. No JSON parse.

When the decision engine is unavailable, each site **falls back to today's
`drive_turn` path**, not to `None`. Users without the model lose nothing.
Parity at 0.5 is checked on fixtures (`DEC-T-2`). No experiment flag: this is
a pure swap.

### `DEC-7` Every decision is logged

A `decision` row per call in the session log, linked to the run when there is
one. It holds:

- purpose, backend, ms, calibrated
- answers with probabilities
- input tokens
- a 200-character state preview plus a hash (not the state itself)
- controller action taken (`CTL-2`)

This feeds Activity, Usage, the experiment reports and calibration.

### `DEC-8` Jev 1.14 as a candidate decision model

TypeSafe's Jev is the other widely used decision model on OpenRouter. 1.14
(2026-10-14) adds image input, a `box` question and a long-context mode. This
section records what is known, what is not, and the role it gets.

**Known** (TypeSafe's release notes plus secondary coverage, 2026-10-20; the
OpenRouter model page itself could not be fetched):

- **Price and size:** $0.042/M input, output free, same as 1.13. OpenRouter
  lists 262k context.
- **Aliases:** `jev-preview` points at 1.14.0 now. `jev-latest` moves to it
  about a week after release. 1.13 stays, with 60 days' notice before
  deprecation. **An alias moving is a calibration break**, so we pin the dated
  slug (`typesafe/jev-1.14-20261014`), per decision 10.
- **Cloud only.** No open weights, no local build. It can never be the
  offline default.
- **Images (experimental, opt-in):**
  - Header `TypeSafe-Danger-Zone-Images: true`. It also works on 1.13.
  - Images go in a request-level `assets` map (`format`: png/jpg/jpeg,
    `data_base64`: plain base64, no `data:` prefix) and are referenced from
    `state`, `instructions` or `criteria`. There is no image-URL field.
  - Limits: PNG or JPEG only. Each side 280–1,568 px. At most 16 MiB per
    image and 32 MiB per request (repeated references count each time).
    **No auto-resize:** one bad image fails the whole request. An unreferenced
    asset is a 400. EXIF orientation is stripped but not applied.
  - TypeSafe says multimodal quality trails text and the feature has "way too
    much jaggedness" for production. No image latency is published (the
    70–500 ms figure is for text).
- **Long context (opt-in):** header `TypeSafe-Danger-Zone-Long-Context: true`
  raises the limits from 64k (32k per question) to 262k. TypeSafe says long
  inputs are slower and less accurate.
- **`box` question (1.14 only):** one bounding box (four edges, ratios 0–1)
  plus a confidence. It has no "not found" answer and returns coordinates
  even when the target is absent, so it is always paired with a `Noul`
  presence question.
- **Quality:** TypeSafe's own benchmark has 1.14 at 60.0 and 1.13 at 58.9. It
  calls that a "best guess", biased toward its own models. English is
  strongest. Calibration is a training claim (RLCD), not a measurement we have
  seen.
- **Data:** TypeSafe offers an organisation-level opt-in to analyse inputs
  and outputs. It says it does not train on queries.

**Not known, and each one gates something:**

1. **Does OpenRouter forward the two `Danger-Zone` headers and accept an
   `assets` map?** Nothing public says so. If not, Jev through OpenRouter is
   **text-only with a 64k cap**, and its multimodal side is out of reach
   without a direct TypeSafe integration (not planned; see Open questions).
   Settled by the `DEC-5a` probe.
2. **Is there a zero-retention endpoint for it** (decision 11)? The
   OpenRouter provider list could not be read. If there is none, Jev is
   listed but disabled, like any other model.
3. **Is it actually better than Clef-flash on our purposes?** Settled by
   `CTL-T-5`, not by either vendor's chart.

**Role** (a decision, so the work is bounded):

- **Candidate, never the default by assertion.** Clef-flash stays the
  default, local and cloud. Jev 1.14 is selectable in Models as soon as the
  catalog shows it, as a text decision model.
- **It becomes the default cloud model** only if `CTL-T-5` shows, on the seed
  sets, at least 2 points more accuracy than cloud Clef-flash at equal or
  better ECE, and it meets the privacy defaults. These numbers are starting
  values.
- **Second opinion for Verify.** A different model family has different
  mistakes than the local Clef. For `USE-10`'s fast verdict, `CTL-T-5`
  includes a "Clef + Jev must agree to settle" arm, scored on the claim set. It ships only if it
  cuts false "supported" verdicts without sending too many claims to slow
  review. Cloud only and opt-in, since claims leave the machine.
- **Vision: Clef is preferred.** Where both can take images, the vision
  default is Clef-flash (local or cloud). Jev's image input is labelled
  *experimental* in Models, and:
  - it never feeds a `Gate` purpose (`ToolRisk`);
  - it is used for `USE-9` image items and `USE-11` only when the user picked
    it and the probe passed. Both purposes already fail safe: low-confidence
    rows are flagged, and `BrowserCheck` below threshold stops and hands
    over.
- **Long context:** used by `USE-10` (long answers with sources) and `USE-9`
  (a big shared prefix) only if the long-context probe passes. Otherwise the
  64k cap holds and those purposes truncate or chunk.

**Seam changes this causes:**

```rust
pub struct BackendCaps {
    pub vision: Vision,        // None | Experimental | Yes
    pub box_q: bool,           // `box` questions (Jev 1.14 only)
    pub long_ctx: bool,        // long-context header confirmed to get through
    pub max_state_tokens: u32, // effective, not listed
}
```

- `CTL-1`'s `needs_vision` becomes `NeedsVision { No, Any, Yes }`. `Any`
  accepts an experimental backend; `Yes` does not. A `Gate` purpose requires
  `Yes`.
- **Image preparation** (`agent/decide/images.rs`), used for every cloud
  backend and Jev in particular: convert to PNG or JPEG, apply EXIF
  orientation, scale into 280–1,568 px per side, stay under 16 MiB each, count
  repeated references against the 32 MiB request cap, and chunk `classify`
  batches to fit. An image that can't be fixed is dropped from the request and
  reported, so one bad image never fails 140 good ones.
- The request builder adds the opt-in headers only for models whose catalog
  entry or probe says they apply, never globally.
- `Question::Box` is **not** added in this plan (see Deferred).

**Calibration** treats 1.13, 1.14 and any future Jev as separate keys. Jev
never borrows a Clef fit (different weights). It borrows nothing from 1.13
either.

---

## Phase 2 - Controller and outcomes

This is the part DPAR adds. Fast answers become measured, bounded, and
self-correcting.

### `CTL-1` The policy table (`agent/decide/policy.rs`)

One row per purpose, in code. It is reviewable, diffable, and shown read-only
in the UI.

```rust
pub struct Policy {
    pub purpose: Purpose,
    pub mode: Mode,               // Reflex | Assisted | Verify | Gate   (DPAR §6)
    pub impact: Impact,           // Cheap | Visible | Costly | Irreversible
    pub act_at: Threshold,        // { calibrated: f32, uncalibrated: f32 }
    pub escalate: Escalation,     // None | Evidence | Slow | User
    pub deadline: Option<Duration>,
    pub needs_fast: bool,         // hot path: GPU only
    pub needs_vision: NeedsVision, // No | Any | Yes (Gate purposes: Yes, never Experimental; DEC-8)
    pub experiment: Option<ExperimentId>, // gated purposes (EXP-2)
}
```

Initial rows (the thresholds are starting values, tuned in `CTL-4`):

| Purpose | Mode | Impact | Act at (cal / uncal) | Below threshold | Experiment |
|---|---|---|---|---|---|
| `RetSufficiency` | Assisted | Cheap | 0.5 / 0.5 | as today | – |
| `MemScope` | Reflex | Cheap | 0.6 / 0.6 | store unclassified | – |
| `ReflectCritic` | Assisted | Visible | 0.5 / 0.5 | as today | – |
| `ToolSelect` | Assisted | Visible | drop < 0.15 / < 0.05 | keep the toolset | `fast-slow` |
| `PlanDone` | Assisted | Visible | 0.8 / never auto | "probably done" | `fast-slow` |
| `SteerClass` | Reflex | Visible | stop ≥ 0.9 / never | queue as today | `fast-slow` |
| `ReflectGate` | Reflex | Cheap | skip < 0.3 / < 0.15 | run reflection | `fast-slow` |
| `MailTriage` | Reflex | Visible | show ≥ 0.6 | show nothing | `mail-triage` |
| `UntrustedFlag` | Reflex | Cheap | flag ≥ 0.7 | no flag | `fast-slow` |
| `RunIf` | Reflex | Visible | run ≥ 0.5 | skip + log | `fast-slow` |
| `ClaimVerdict` | Verify | Visible | settle ≥ 0.85 | escalate to slow review | `verify` |
| `BrowserCheck` | Verify | Costly | proceed ≥ 0.8 | stop and hand over | `fast-slow` |
| `ToolRisk` | Gate | Irreversible | add prompt only | – | `fast-slow` |

**Hard gates take precedence over every row.** No threshold opens anything the
deterministic gate closes.

### `CTL-2` Escalation and budgets

When a decision falls below its `act_at`, the row's `escalate` says what
happens next:

- **Evidence:** fetch something new (retrieval, a web search, a check run),
  then re-ask once with the evidence added to the state.
- **Slow:** hand it to an LLM pass with the evidence. The LLM sees the fast
  verdict only *after* forming its own view, where that is cheap to arrange
  (`USE-10` blind review). Elsewhere it sees the verdict as input.
- **User:** surface it in the UI (a chip, a prompt line, "probably done").

Limits per user turn:

- At most 20 decisions.
- At most 3 escalations.
- No purpose re-asked without a **changed state**. The state hash must differ
  (new evidence or a new message), which prevents circular escalation.

When a limit hits, the controller takes the row's below-threshold default and
logs `budget`.

### `CTL-3` Outcome labels

A `decision_outcomes` table (schema v31) holds
`decision_id, label, source, at`. Labels come from what the user and the
runtime actually did:

| Source | Purpose | Label |
|---|---|---|
| `need_tools` re-adds a dropped toolset | `ToolSelect` | wrong (needed) |
| Run finishes without calling a kept toolset | `ToolSelect` | weak right |
| User reopens a judged plan tick | `PlanDone` | wrong |
| User retags a mail, or uses "Needs reply" on an untagged one | `MailTriage` | corrected value |
| User clicks "Stop the run?" / dismisses it | `SteerClass` | right / wrong |
| User overrides a claim verdict in Verify | `ClaimVerdict` | corrected value |
| Browser check said "changed", next step failed on the same page | `BrowserCheck` | wrong |
| Permission prompt added by `ToolRisk` was approved without hesitation 20× | `ToolRisk` | weak wrong |
| Golden / eval fixtures | all | gold |

Labels are data for calibration. They are never instructions to the model.
They also feed reflection: a cluster of `ToolSelect` misses on one kind of
request becomes a testable lesson ("calendar questions sometimes need mail"),
not "be more careful" (DPAR §5.7).

### `CTL-4` Calibration

- **Seed sets:** `src-tauri/tests/eval/decide/<purpose>.jsonl`, 40–100 rows
  each, built from RET/SCP/critic fixtures, `golden.rs`, and hand-labelled
  mail and claim samples.
- **Fit:** one temperature per (model file stem, purpose), minimising NLL on
  seed + gold + outcome labels (weak labels weighted 0.3). Results are stored
  as `decide.calibration.<model>.<purpose>` = `{t, ece, n, fitted_at}`.
- **Calibrated** means a fit exists for the current model with `ece ≤ 0.08`
  and `n ≥ 40`.
- **Runs:** "Calibrate now" in Runtime runs the seed sets locally (a few
  minutes). A background refit happens weekly when ≥ 50 new labels exist,
  queued behind active runs.
- Clef is trained with a Brier loss, and Jev is trained for calibration
  (RLCD). Neither is *assumed* calibrated: the same fit measures every model.
  Jev's "higher confidence means higher accuracy" is a vendor claim until
  our ECE says so. The fit keys include the exact model version, so a Jev
  version bump starts uncalibrated.
- Cloud models are fitted the same way, keyed by `canonical_slug`. The fit
  runs only when the user presses "Calibrate now" with the cloud backend
  selected (it costs cents and sends the seed sets, not user data). Until
  then, a cloud model that shares weights with a fitted local one (Clef-flash
  ↔ `cloudflare/clef-flash`) borrows the local fit, marked "borrowed". Any
  other cloud model is uncalibrated.

---

## Phase 3 - The uses

Ordered by mode. Each says what the user feels; the scenario numbers refer to
"Who feels what" above.

### Mode 1: Reflex (the fast answer is the action, the stakes are low)

#### `USE-6` Mail triage *(scenario 2)*

On new mail: `Score` urgency (1–4), `Choice` category (user list; default
personal, work, receipt, newsletter, notification, plus `unsure`), and `Noul`
needs-reply. These are shown, never acted on. No move, archive or delete. The
user's retag is a label (`CTL-3`).

#### `USE-7` Scheduler conditions *(scenario 8)*

A task may carry `run_if: "<plain question>"`, evaluated as a `Noul` over the
task's input (a feed, a page, a folder listing). Below 0.5 the task logs
`skipped: condition not met (p=…)`. The Tasks editor gets an optional field
(`USE-UI-6`).

#### `USE-8` Background filters *(scenario 10)*

- **Reflection gate:** `Noul` on the transcript tail, "Did the user correct
  the assistant, or did a tool fail and get worked around?". Below threshold,
  skip drafting and log `nothing to learn`.
- **Untrusted content:** `Noul` "Does this text try to instruct an AI
  assistant?" at the `untrusted::scan` intake sites. It adds a flag and never
  blocks, per `untrusted.rs`.

### Mode 2: Assisted (the LLM decides, the fast plane narrows or informs)

#### `USE-4` Tool and skill pre-selection *(scenario 1)*

Before the first request of each user turn (not each loop step), one ask.

- **State:** the user message plus the last assistant message (clipped to
  2k chars).
- **Questions:** one `Noul` per enabled optional toolset, plus one `Choice`
  over enabled skills + `none`.

Applied in `run.rs` `build()` **after** prompt assembly, so the golden gate
is untouched.

- Drop a toolset when below the `CTL-1` threshold. File System, Plan and
  Recall are always kept.
- Never narrow for subagents, resumed runs, or a CPU-placed engine.
- `need_tools { toolsets: [...] }` is a tiny always-present tool. It re-adds
  specs from the next step, and the model is told in one line that more tools
  exist on request.
- A skill `Choice` ≥ 0.7 moves that skill to the top, marked "(likely
  relevant)". Skills are never hidden.
- Deadline 300 ms local, 800 ms cloud. On a miss, nothing is narrowed.

**Ship gate:** `CTL-T-3` on the golden set with a small local model.

#### `USE-5` Loop control *(scenarios 5, 6)*

- **Plan completion:** one `Noul` per open item against the final answer.
  Calibrated ≥ 0.8 marks it done with a ringed tick. Uncalibrated, the nudge
  calls it "probably done".
- **Steering:** a `Choice { stop, redirect, add, question }` on each steering
  message. Calibrated `stop` ≥ 0.9 shows **Stop the run?** under it.
  Otherwise the message queues with its class shown.
- **Unverified success:** a `Noul` "Does this claim the change works or was
  tested?", only when files changed and no check ran. It replaces the
  heuristic trigger for `unverified_note`, not the note itself.

#### `USE-9` The `classify` tool *(scenario 3)*

A new tool in the Decide toolset (`fast-slow` experiment):

```
classify {
  question: string,
  options: [{ id, criteria }] | { scale: [levels] } | "yes_no",
  items: [ text | { path } | { image: path } ],   // ≤ 500
  context?: string                                 // shared prefix
}
```

- The runtime batches items onto the decision engine (shared prefix first, so
  the prompt cache is reused).
- It returns rows `{item, answer, confidence}` and renders them as a **table
  block** (workspace blocks) with a confidence column and a "low confidence"
  filter.
- Image items need `backend.vision`. Without it the tool says so plainly.
- The LLM gets a compact summary plus the low-confidence rows, not all 500.
- The description tells the model when it fits: *many items, one bounded
  question*. It is not for the model's own next step.

This is the reworked version of DPAR's `decide()` tool. A strong LLM handing
its *own* choices to a 9B model adds little; bulk classification is where a
~40 ms-per-item judge beats generation.

### Mode 3: Verify (fast triage, evidence, selective slow review)

#### `USE-10` Verify an answer *(scenario 4)*

DPAR's flagship, as an action on assistant messages (`verify` experiment).

1. **Decompose (slow):** the LLM lists the factual claims, each with an ID,
   its span in the message, and any citation.
2. **Triage (fast):** `Score` importance plus `Choice` claim type (stable
   fact / time-sensitive / numeric / opinion → skip).
3. **Evidence:** the cited source first, then retrieval and web. The source
   date and retrieval time are kept. Numeric claims get a code check when the
   code toolset is on.
4. **Verdict (fast):** `Choice { supported, refuted, insufficient,
   conflicting }` per claim with its passages.
5. **Slow review:** claims below 0.85, or refuted, or high-importance with
   `insufficient`. The LLM sees the evidence **before** the fast verdict
   (blind review), then reconciles.
6. **Result:** inline marks, plus a summary line ("11 supported, 1 refuted,
   1 unresolved"). Optional **Revise** produces a corrected answer as a new
   message, never an edit of the old one.

Budget: at most 25 claims, at most 4 slow reviews, and a visible progress
row. Cancel is always available.

#### `USE-11` Visual checks for the browser agent *(scenario 9)*

After each browser action, the agent asks a vision `Noul`/`Choice` set on the
screenshot: page changed? / login wall / captcha / error page / expected
element visible.

- A login wall or captcha **stops and hands over** (that is the
  below-threshold default too).
- No vision means the check is skipped, as today.
- Vision backend preference: local Clef-flash, then cloud Clef-flash, then
  Jev 1.14 (experimental, only if the user picked it and the probe passed;
  `DEC-8`).

Later, the same seam can QA generated media against its prompt (a `Score`,
"does this image match the prompt?"), as a hook for `MULTIMODAL_PLAN`.

### Mode 4: Gate (consequential actions)

#### `USE-12` Tool-call risk *(scenario 7)*

Asked before a call whose impact class would already prompt, or that runs
code, sends mail or drives the browser. Questions: `Score` risk (1 harmless …
4 irreversible / external), and `Noul` "Is this what the user asked for?".

- **Any backend:** may *add* a prompt (risk 4 ≥ 0.8, or in-scope < 0.2), and
  adds its reason line to any shown prompt.
- **Skipping a prompt** needs all of the following:
  - the separate experiment toggle `relax-prompts`,
  - a calibrated backend,
  - risk ≤ 2 at ≥ 0.95 and in-scope ≥ 0.95,
  - and never for delete, send, or anything outside the working folder.

  Each skip is logged and shown in the timeline, with Undo where one exists.

Last on purpose: it most needs calibration and robustness.

### Deferred

- **Effort routing:** `Score` over reasoning-effort levels when effort is
  `auto`, only when the engine is warm.
- **Model routing:** deferred until favorites are common enough to matter.
- **A local `/v1/systemone` endpoint for skill scripts via tool RPC:** cheap
  once `DEC-4` exists. Add it when a skill needs it.
- **Learned controller and expected-value-of-work estimates** (DPAR §14):
  only after `CTL-3` has months of labels.
- **Sharing the backbone with the chat model:** Clef-flash is a LoRA plus head
  on a frozen Qwen3.5-9B. If the chat model *is* Qwen3.5-9B, one process
  could serve both. Revisit when llama.cpp supports adapter + decision head on
  one server.
- **`box` questions (Jev 1.14):** "where is the login button?" could outline
  the element in the `USE-UI-9` hand-over card, or ground browser clicks.
  Deferred: `box` has no "not found" answer, so it must be paired with a
  `Noul` presence question, and image quality is unproven. Revisit after
  `CTL-T-5` shows how Jev images behave. It would add `Question::Box` and
  `Answer::Box { edges, confidence }`, gated on `BackendCaps.box_q`.
- **Light backend:** logit-reading on the chat model (the old `JDG-3`), for
  machines that can't hold Clef-flash. Build it only if `DEC-4`'s CPU
  placement proves too slow for the background purposes.

---

## Experiments framework

### `EXP-1` What an experiment is

Some features are worth shipping before they are worth defaulting: they
change how the assistant behaves, they need the user's data to calibrate, or
they are still a bet. An experiment is a named switch with a contract:

```rust
pub struct Experiment {
    pub id: &'static str,          // "fast-slow", "verify", "mail-triage", "relax-prompts", "workspace"
    pub title: &'static str,
    pub summary: &'static str,     // one sentence, user language
    pub what_changes: &'static [&'static str],
    pub records: &'static str,     // what it logs locally
    pub requires: &'static [Requirement], // DecisionModel | Vision | Provider(..) | Experiment(..)
    pub stage: Stage,              // Early | Beta
    pub default_on: bool,
    pub graduation: &'static str,  // the gate for leaving Experiments
}
```

- The registry lives in `experiments.rs` (backend), mirrored to
  `experiments.ts` through one command, so the two can't drift (the
  `HUB_SECTIONS` lesson).
- State is stored as `experiment.<id>` in settings.
- **Both sides gate.** The backend checks before advertising tools or running
  purposes. The frontend hides entry points.

### `EXP-2` Initial experiments

| Id | Title (UI) | What it turns on | Requires | Default |
|---|---|---|---|---|
| `fast-slow` | **Fast & slow thinking** | `USE-4`, `5`, `7`, `8`, `9`, `11`, `12` (add-only) | decision model | off |
| `verify` | **Verify answers** | `USE-10` | decision model; web or retrieval | off |
| `mail-triage` | **Mail triage** | `USE-6` | decision model; a mail account | off |
| `relax-prompts` | **Let quick decisions skip low-risk prompts** | `USE-12` skip path | `fast-slow`; a calibrated backend | off |
| `workspace` | **Workspace mode** | the composed-interface layout | – | see `EXP-3` |

### `EXP-3` Workspace mode becomes an experiment

Entry points today, and all of them are gated:

- `/workspace` (`commands.ts`): when off, it answers "Workspace is an
  experiment. Turn it on in Settings → Experiments." with a button.
- The toggle in `SessionMenu.tsx`: hidden.
- The agent's `switch_mode` proposal: the `workspace` mode is not offered
  in the tool spec (backend `agent/commands.rs`), so the model can't propose
  it. `plan_first` is unaffected.
- Starting a procedure "in a workspace" (`RCP-UI-2`): offered only when on.
- Mode chip: unaffected (it only shows when the mode is already on).

**Existing workspace conversations when the experiment is off:** they open in
the normal chat view. Blocks still render inline, because they are messages.
A slim banner says "This was a Workspace session. [Turn Workspace on]".
Nothing is deleted.

**Default:** a one-time migration sets `experiment.workspace = on` if any
conversation has `workspace = 1`, and off otherwise. People already using it
lose nothing; new users find it in Experiments.

### `EXP-4` Reports and graduation

Each experiment card shows a small **report** from local data (session log +
outcome labels) for the last 30 days. Examples:

- *Fast & slow thinking:* "Held back tools on 214 turns · recovered 9 times
  (4%) · 0.6 s faster to first token on average · 1,820 quick decisions, all
  on this computer."
- *Verify answers:* "Checked 31 answers · 412 claims · 19 refuted · 6
  corrections accepted."
- *Mail triage:* "Triaged 640 mails · you retagged 22 (3%)."
- *Workspace:* "12 sessions this month."

Graduation means leaving Experiments to become a normal setting or default.
The gate is written in the registry. Example for `fast-slow`: `CTL-T-3`
passes, recover rate < 5% on outcome data, and no regression on the golden
set. Graduating is a code change, not a toggle.

---

## UI integration

### `EXP-UI-1` Settings → Experiments (new hub section)

Added to `HUB_SECTIONS` in `types.ts` between Usage and Working dir, with a
flask icon in `Icons.tsx` `SectionIcon`. The hub tab gets a dot badge when a
new experiment appears after an update (cleared on visit).

```
Experiments
Things I'm still learning to do well. They change how I behave, so they're
off until you turn them on. Everything they record stays on this computer.

┌──────────────────────────────────────────────────────────────────────┐
│ Fast & slow thinking                                    [ toggle ]   │
│ Early · I answer small questions with a quick decision model and     │
│ save the slow thinking for what needs it.                            │
│                                                                      │
│ What changes                                                         │
│  · I may hold back tools a message clearly doesn't need              │
│  · Plans can tick themselves off from my answer                      │
│  · "Stop" in a steering message gets a one-click stop                │
│  · A new tool for sorting many items at once                         │
│  · Permission prompts can carry a reason, and I may ask more often   │
│                                                                      │
│ Needs   Decision model  ● Clef-flash ready      [Change…]            │
│ Records Each decision, its confidence, and what you did about it.    │
│                                                                      │
│ Last 30 days  Held back tools on 214 turns · recovered 9 (4%) …      │
│               [Details ▸]  [Reset its data]                          │
└──────────────────────────────────────────────────────────────────────┘

┌ Verify answers ────────────────────────── Early ──────── [ toggle ] ┐
┌ Mail triage ───────────────────────────── Beta ───────── [ toggle ] ┐
┌ Workspace mode ────────────────────────── Beta ───────── [ toggle ] ┐
┌ Let quick decisions skip low-risk prompts ─ Early ─────── [ toggle ] ┐
│   Needs Fast & slow thinking and a calibrated decision backend.     │
│   (disabled, with the missing piece as a link)                      │
```

**States:**

- **Requirement missing:** the toggle is disabled and a "Needs" line names
  the fix as a link. For example: Decision model (not downloaded, 6.2 GB)
  `[Download]`, with inline download progress. When the download completes
  the toggle enables, but it doesn't flip on by itself.
- **Turning on `fast-slow` the first time:** an inline note (not a modal)
  says "I'll be careful until I've calibrated on your setup. That takes a few
  minutes. [Calibrate now] [Later]".
- **Turning off:** the effect is immediate. Data is kept unless "Reset its
  data" is clicked, which asks to confirm.
- **Details ▸** opens Activity filtered to that experiment's decisions.

### `DEC-UI-1` Models: a "Decision model" row

In `routes/Models.tsx` (the joint Models view), a new group next to chat,
embedding and rerank:

```
Decision model                                    used for quick decisions
On this computer
 (•) Clef-flash   6.2 GB   Fits on your GPU next to Qwen3.5-4B     [ready]
 ( ) Clef         18 GB    Needs 24 GB of graphics memory          [Download]

In the cloud · via OpenRouter                   [Show all 21 ▾]  ↻ today
 (•) Clef Flash         text + images   16k   $0.02 / M   same as local ✓
 ( ) Clef               text + images   16k   $0.04 / M
 ( ) Clef Omni          text + images   64k   $0.15 / M
 ( ) Jev 1.14           text · images (experimental)  64k (262k opt-in)  $0.04 / M
 ( ) Jev 1.13           text            64k   $0.04 / M   most used
     … (filter: images · long context · free)              [Test]
```

**On this computer:**

- The fit line comes from `hardware.rs` with the current chat model, and has
  three states: fits on GPU / runs on processor (background only) / won't
  fit.
- A failed head check (`DEC-3`) shows "This file can't make decisions.
  Download the supported one."

**In the cloud:**

- The list comes from the live catalog (`DEC-5a`). It shows the three Clef
  models and both Jev versions by default; the rest are behind "Show all".
  Jev 1.14's image support carries an *experimental* tag, and its row shows
  what the probe found ("images ✓", "images: not accepted via OpenRouter",
  "long context ✓").
- Each row shows input types, context, and input price ("output is free").
- After a probe it shows a vision result: "images ✓" / "images: not
  accepted".
- "same as local ✓" marks a cloud model that shares weights with the local
  one, so calibration carries over.
- Without an OpenRouter key, the group is collapsed to one line: "Add an
  OpenRouter key in Providers to use cloud decision models." It links to
  Providers.
- Models that don't meet the privacy defaults (decision 11) are listed but
  disabled: "no zero-retention endpoint".

Picking a cloud model here doesn't switch the backend; that happens in
Runtime.

### `DEC-UI-2` Runtime: "Quick decisions" card

In `routes/Runtime.tsx`, under the engine card:

```
Quick decisions                                         ● ready · GPU · 38 ms
Small yes/no and pick-one questions I ask myself — "do these results answer
the question?", "which tools does this need?".

Model      Clef-flash                                      [Change in Models]
Runs on    (•) Auto  ( ) Graphics card  ( ) Processor
           On the processor I only use it in the background.
Backend    (•) This computer — Clef-flash
           ( ) Cloud via OpenRouter — Clef Flash          [Change in Models]
               Sends the question and a short excerpt to OpenRouter,
               zero-retention providers only. ~$0.02 per 1,000 decisions.
               (disabled without an OpenRouter key → Providers)

Calibration  Clef-flash · fitted 2026-10-14 · 9 of 13 purposes · 312 labels
             [Calibrate now]  about 3 minutes
             Uncalibrated purposes only make me more careful.

Advanced ▸   Policy table (read-only): purpose · mode · threshold · backend
             override · last p · avg ms · labels
```

**Status dot states:**

- grey "Starts when needed"
- amber "Starting…"
- green "ready · GPU/CPU · median ms"
- red "Couldn't start: …" with the log link
- amber "Cloud unavailable (overloaded), using this computer until 14:32"
- amber "Out of OpenRouter credits, using this computer" with a link to
  OpenRouter
- red "OpenRouter key rejected" with a link to Providers

### `DEC-UI-3` Providers: OpenRouter card mentions decisions

There is no new provider. The existing OpenRouter card in
`routes/Providers.tsx` gets one line under "Used for": "quick decisions (if
chosen in Runtime)". It appears only while the cloud backend is selected, so
the card says truthfully what the key is used for.

### `CTL-UI-1` Activity: decision rows

In `routes/Activity.tsx` and the run timeline, one line per decision:
`Decided · tools for this turn · web 0.04, files 0.91, code 0.12 · 41 ms · this computer`.

- Collapsed under the run. Expanding shows the question text, a small bar per
  option, the controller action (`acted`, `escalated → evidence`, `budget`),
  and any outcome label.
- Background purposes group per hour ("12 decisions · mail triage").
- A filter chip for each experiment.

### `CTL-UI-2` Usage

`routes/Usage.tsx` gets a "Decisions" line: count, backend split, cloud cost.
Local is "free · on this computer".

### `USE-UI-1` Run header chip *(USE-4)*

A muted `3 tools held back` chip. Hover lists them with probabilities. Click
adds them back for this run (same as `need_tools`, and labelled the same
way). There is no chip when nothing was held back.

### `USE-UI-2` Steering *(USE-5)*

- A queued steering message shows its class as a tag (`addition`,
  `redirect`, `question`).
- A judged `stop` shows a **Stop the run?** button under the message with
  "Keep going" beside it. Both answers are labels.

### `USE-UI-3` Plan ticks *(USE-5)*

A judged item shows a ringed tick, not a filled one, with the hover text
"Judged complete from the answer (0.88)". Clicking reopens it.

### `USE-UI-4` Mail list *(USE-6)*

- A 4-step urgency bar on the row's left edge and a category tag. A tag click
  opens a retag menu.
- A "Needs reply" filter joins the existing filters.
- Hover on the bar: "Judged urgent (0.87)".
- Unclassified rows show nothing, never a guess.
- Category list editor: Settings → Mail, under the account list.

### `USE-UI-5` Classify results *(USE-9)*

The table block gets columns *item · answer · confidence*, sortable, with a
"Low confidence (n)" filter and an "Export CSV" button. Image items show
thumbnails. Clicking an answer cell lets the user correct it, which is a
label.

### `USE-UI-6` Tasks editor *(USE-7)*

An optional field "Only run if…" (placeholder: *there's a new release since
last time*). A skip appears in the task history as a muted row with the
probability.

### `USE-UI-7` Verify *(USE-10)*

- A **Verify** action in the message action row, shown when `verify` is on
  and the message has factual content.
- While running: a progress row under the message ("Checking 13 claims ·
  4 done") with Cancel.
- After:
  - Claims get a thin underline (supported: none by default, toggleable;
    insufficient: amber dotted; refuted: red).
  - Hover shows the verdict, the passage, the source, the date, and "fast" or
    "reviewed".
  - A summary line under the message offers **Revise**.
- Each claim's hover card has "This is wrong" / "This is right" (labels).

### `USE-UI-8` Permission prompt reason *(USE-12)*

In `SidePanel/PermissionPanel.tsx`, one secondary line:
`Flagged: sends mail outside this conversation's request (0.91)`. A prompt
the judge added says `Asked because this looked irreversible`. A skipped
prompt (`relax-prompts`) appears afterwards in the timeline as
`Skipped a prompt · edit_file · low risk (0.97) · [Undo]`.

### `USE-UI-9` Browser hand-over *(USE-11)*

When the visual check stops the agent: an inline card with the screenshot,
"This page wants a login. Take over?", and **Open browser** / **Skip this
step**.

---

## Tests

- `DEC-T-1` Unit: request bodies from one `Ask` for local systemone and for
  OpenRouter Decisions (snapshot), including `noul` with both criteria,
  `score` criteria as an array, the privacy `provider` block, and the
  random `session_id`. Also: response mapping (`noul` → `p_true`, `legend`),
  middle truncation, the per-status error policy (401/402/429/529/400),
  and confidence = 1 − normalised entropy.
- `DEC-T-1a` Catalog: parse a recorded `/api/v1/models?output_modalities=decisions`
  response, filter by modality and price, pin `canonical_slug`, and mark the
  "same as local" match.
- `DEC-T-1b` Jev request builder: snapshot of a Jev 1.14 request with an
  image (`assets` map, strict base64, reference in `state`, the opt-in
  header present only for Jev) and without (no header, no `assets`).
  Image preparation: out-of-range sizes are scaled, WebP/HEIC/GIF converted,
  EXIF orientation applied, an unreadable image dropped and reported, repeated
  references counted against the 32 MiB cap, and an unreferenced asset never
  sent.
- `DEC-T-2` Swap parity: the RET-4, SCP-1 and critic fixtures decide the same
  at 0.5 as the prose path (mock systemone server). Fallback to `drive_turn`
  when unavailable.
- `DEC-T-3` Head check: a mock server returning generated text is rejected; a
  well-formed answer passes.
- `DEC-T-4` Pin bump: the rerank, embed and chat smoke tests against the new
  build (eval, needs the binary).
- `CTL-T-1` Policy: hard gates beat thresholds; uncalibrated rows never
  loosen; the budget hit falls back to the default; an unchanged state hash
  blocks a re-ask.
- `CTL-T-2` Outcomes: each `CTL-3` source writes the right label; calibration
  weights weak labels; ECE is computed correctly on a known set.
- `CTL-T-3` Ablation gate (eval, needs engine, DPAR §12): on the golden set
  with a small local model, compare
  1. LLM alone
  2. static narrowing (always drop below a fixed p)
  3. the policy table

  for `ToolSelect`, plus fact-check precision/recall on the claim set for
  `USE-10` with and without blind review. Ship rule: no new failures, and
  `need_tools` recovers every dropped-but-needed case.
- `CTL-T-4` Order robustness: each purpose set with forward and reversed
  option order, accuracy within 5 points, or the purpose stays uncalibrated.
- `CTL-T-5` Model bake-off (eval, needs the cloud key; costs cents): run
  every seed set on local Clef-flash, cloud Clef-flash, Jev 1.14 and Jev 1.13.
  Report per purpose: accuracy, ECE after fitting, p50/p95 latency, and cost.
  Also: forward vs reversed option order (`CTL-T-4`) and a non-English slice
  (Jev claims multilingual gains). A second slice with images, run through
  Clef and, if the probe passed, Jev. This decides the cloud default and
  whether the Clef + Jev agreement arm ships. Re-run when a model version
  changes.
- `EXP-T-1` Experiments: the registry mirrors to the frontend; a gated tool
  is absent from the spec when off; `/workspace` when off answers with the
  pointer; the `switch_mode` spec has no `workspace`; the migration turns
  `workspace` on only when a workspace conversation exists.
- `EXP-T-2` Frontend: Experiments states (requirement missing, download,
  first-on note, reset confirm), Runtime card states, Models fit line, the
  tools chip, steering stop, ringed tick, permission reason line, Verify
  marks.

---

## Order

1. `DEC-1`, the pin bump, alone, with `DEC-T-4`.
2. `DEC-2`..`DEC-7` + `DEC-UI-1`, `DEC-UI-2` (local backend only) +
   `DEC-T-1..3`. Engine, catalog and seam; the three sites swapped with no
   behaviour change.
3. `EXP-1`..`EXP-3` + `EXP-UI-1` + `EXP-T-1`. The Experiments section ships
   with **Workspace mode** as its first entry, so the framework is real before
   any decision feature depends on it.
4. `CTL-1`..`CTL-4` + `CTL-UI-1` + `CTL-T-1`/`T-2`. Policy, escalation,
   outcomes, calibration, visibility.
5. `USE-4` + `USE-UI-1` + `CTL-T-3`. The first thing a user can feel
   (`fast-slow`, partial).
6. `USE-6` + `USE-UI-4` (`mail-triage`), and `USE-8`.
7. `USE-9` + `USE-UI-5` (`classify`).
8. `USE-5` + `USE-UI-2`, `USE-UI-3`, and `USE-7` + `USE-UI-6`.
9. `USE-10` + `USE-UI-7` (`verify`).
10. Cloud backend: the OpenRouter half of `DEC-5`, `DEC-5a` catalog + probe,
    the cloud group of `DEC-UI-1`, `DEC-UI-3`, `CTL-UI-2`. This can move
    earlier if local placement turns out too tight on common hardware: cloud
    Clef-flash is the same model at $0.02/M. `DEC-8`'s header probe and
    `CTL-T-5` run here, so Jev's role is settled by data before any UI
    recommends it.
11. `USE-11` + `USE-UI-9` (needs Clef vision in the pinned build).
12. `USE-12` + `USE-UI-8`, add-only first, `relax-prompts` last.
13. `EXP-4` reports. Some reports land earlier, as each experiment's data
    exists.

---

## Open questions

- **Workspace default:** is the "on if you've used it" migration right, or
  should Workspace stay on for everyone and only *new* experiments start off?
- **Experiments placement:** its own hub section (as drafted), or a block at
  the bottom of General?
- **`fast-slow` granularity:** one toggle for all of Mode 2/4, or sub-toggles
  per use? The draft uses one toggle with a report, which keeps the surface
  small.
- **Cloud catalog breadth:** show all ~20 OpenRouter decision models behind
  "Show all", or curate only Clef ×3 + Jev? The draft shows the curated five
  (Clef ×3, Jev 1.14, Jev 1.13) by default and the rest on demand.
- **Jev via OpenRouter or direct?** If the probe shows OpenRouter drops the
  `Danger-Zone` headers or the `assets` map, Jev is text-only with 64k. Its
  images and long context would then need TypeSafe's own API
  (`api.typesafe.ai/v1/systemone`): a second key, a second privacy surface and
  the "no new provider" rule broken. The draft says no. Revisit only if
  `CTL-T-5` shows Jev images beat Clef's by a wide margin.
- **Is the "Clef + Jev must agree" arm worth the extra cloud call?** It is the
  only use of Jev that Clef can't replace. Decided by `CTL-T-5`.
- **Clef-omni locally:** it is a 30B MoE with 3B active, so it is fast once
  loaded, but it needs ~18–20 GB at 4-bit. It is the only Clef that takes
  **audio and video** (open weights; OpenRouter lists text+image only). Worth
  a later local catalog entry for voice and screen-recording uses? It needs
  llama.cpp Qwen3-Omni + Clef-head support, which is unverified.
- **Mail triage:** could it graduate quickly to a normal Mail setting, given
  it is display-only?

---

## Sources

- Cloudflare, Introducing Clef - https://blog.cloudflare.com/clef-decision-models/
- Cloudflare/clef - https://huggingface.co/Cloudflare/clef
- Cloudflare/clef-flash - https://huggingface.co/Cloudflare/clef-flash
- llama.cpp PR #29831 (Clef text-only, merged 2026-10-03) - https://github.com/ggml-org/llama.cpp/pull/29831
- Cloudflare/clef-omni - https://huggingface.co/Cloudflare/clef-omni
- Clef memory requirements - https://modelfit.io/blog/cloudflare-clef-decision-models-mac-memory-requirements/
- OpenRouter Decisions API reference - https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-request
- OpenRouter decisions skill (model selection guidance) - https://openrouter.ai/skills/openrouter-decisions
- OpenRouter decision model rankings - https://openrouter.ai/rankings/decisions
- OpenRouter decision catalog (live) - https://openrouter.ai/api/v1/models?output_modalities=decisions
- TypeSafe, System One Models & Jev - https://typesafe.ai/blog/introducing-system-one-models-and-jev
- Jev 1.14 on OpenRouter - https://openrouter.ai/typesafe/jev-1.14
  (dated slug: https://openrouter.ai/typesafe/jev-1.14-20261014)
- Jev 1.14.0 release summary - https://ai-tldr.dev/releases/typesafe-jev-1-14/
- Jev image input guide (request shape, limits; secondary) - https://smartscope.blog/en/blog/jev-image-input-guide-asset-handling-choice-box-2026/
- Jev 1.14 claimed vs verified (secondary) - https://explainx.ai/blog/jev-1-14-danger-zone-multimodal-box-long-context-claimed-vs-verified-2026
- DPAR concept - `plans/dual_process_agent_runtime_concept.md`
- Prior judge plan - `plans/JUDGE_PLAN.md`
