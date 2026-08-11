# EVL — agent regression harness

`plans/PERCEPTION_PLAN.md` Part 0. Catches "the agent got worse" the way
`cargo test` alone can't: everything in `fixtures/` is real content with a
verifiably correct answer, listed in `golden.json`, and asserted against a
real agent turn — not mocked.

## Running it

Needs a live local engine (this is why it's `#[ignore]`d): start your dev
`llama-server` (or point at any OpenAI-compatible endpoint), then:

```
EVAL_ENGINE_URL=http://127.0.0.1:8080 cargo test --ignored eval -- --nocapture
```

`EVAL_ENGINE_TOKEN` is optional. `EVAL_FILTER=<id>` runs a single case.

## Threshold calibration (EVL-4)

Every similarity floor in `plans/PERCEPTION_PLAN.md` — `SEM-3`'s 0.58,
`RET-2`'s 0.40 / 0.50 / 0.55 — is a *starting* value measured for one
embedding model. Change the model and those numbers mean something else, so
they get re-measured rather than inherited:

```
EMBED_SERVER_BIN=...\llama-server.exe \
EMBED_MODEL_PATH=...\bge-small-en-v1.5-f16.gguf \
cargo test --ignored eval_calibrate -- --nocapture
```

It scores every pair in `calibration.json` (a query, the passages that should
match it, and the passages that shouldn't), prints both distributions, and
says whether a single floor separates them — and if so, where. It fails only
if relevant passages don't outscore irrelevant ones on average, which would
mean the model can't support retrieval at all.

Pairs are drawn from `fixtures/`, deliberately phrased in words the documents
don't use — a floor calibrated on paraphrases is the one that matters.

## Scope, honestly

- `must_contain` / `must_not_contain` are checked against the agent's final
  prose, and `expect_tool` against `tool_stats` (which records raw tool names
  per conversation). All three are enforced.
- Fixtures are text-only for now (two notes, one CSV). A text-layer PDF, a
  scanned PDF, and a handful of photos (two near-duplicates) round the set
  out per the plan. They are **deliberately not stubbed**: a synthetic PDF or
  a generated gradient image would exercise the plumbing while telling us
  nothing about whether `OCR` can read a real scan or `PHS` can tell two real
  photos apart, which is the only question those fixtures exist to answer.
  Add real ones under `fixtures/docs/` and `fixtures/photos/`, with matching
  `golden.json` cases, when `OCR`/`VIS`/`PHS` land.
- `SEM`'s "teach a lesson, bury it under 39 others, watch it surface
  unprompted" exit check is **not** an EVL case, and that's deliberate rather
  than an oversight. `run_case` (above) sends the model no system prompt at
  all — memory injection is composed client-side (`lib/store.ts`'s
  `composeSystemPrompt`/`recallForPrompt`), so `run_agent` never sees a lesson
  regardless of what's in the store; this harness tests tool use, not prompt
  assembly. The floor-gating mechanism itself — a relevant lesson clears
  `0.58`, an unrelated one doesn't — is unit-tested directly instead, in
  `memory::tests::recall_for_gates_lessons_and_recipes_by_the_similarity_floor`.
  A true end-to-end version belongs here once `WHY` moves prompt composition
  server-side; bolting a second, Rust-only prompt assembler onto EVL just to
  test SEM early would duplicate logic that's about to move.
