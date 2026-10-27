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

## Decision seed sets (`CTL-4`)

`decide/<purpose>.jsonl` holds 40 hand-labelled rows for each purpose of the
decision engine (`plans/DUAL_PROCESS_PLAN.md`). They are compiled into the app
(`agent/decide/seed.rs`), and "Calibrate now" in Runtime runs them through the
decision model to fit one temperature per (model file, purpose). One JSON object
per line: an `id`, the purpose's inputs, and `expect`, which must be one of the
question's own options (`yes`/`no` for a yes-or-no question). A unit test fails
if a row does not parse, an id repeats, a set has fewer than 40 rows, or one
answer is more than 60 percent of a set.

To measure the real model without the app:

```
DECIDE_SERVER_BIN=...\llama-server.exe \
DECIDE_MODEL_PATH=...\Clef-Flash-Q4_K_M.gguf \
DECIDE_MMPROJ_PATH=...\mmproj-Clef-Flash-Q8_0.gguf \
cargo test --test decide -- --ignored --nocapture seed_sets
```

`DECIDE_SEED_ROWS=10` runs only the first 10 rows of each purpose. It prints
accuracy, how often the plan's bar is reached, the error before and after the fit.

`decide/tool-select.jsonl` is the seed for the tool menu (`USE-4`). Each row asks
about one group (`web_search`, `mail`, `apps`, `subagents`, `browser`,
`code_exec`, `image_gen`) with `unit` and `expect` (`yes` means the message needs
it). A run asks all groups at once, so this set measures each answer on its own,
which is the easier case.

`decide/reflect-gate.jsonl` and `decide/untrusted-flag.jsonl` are the seeds for the two
background filters (`USE-8`). A reflect-gate row is a `transcript` and `expect` (`yes`
means something was corrected or failed, so reflection should run). An
untrusted-flag row is a `text` and `expect` (`yes` means it tries to instruct an AI
assistant). The plain-prose injections in the flag set are easy ones.

`decide/classify.jsonl` is the seed for the `classify` tool (`USE-9`). Each row carries
its own `question`, an `item`, and `expect`; `options` (`id` and `criteria`) makes it a
pick-one question, and no `options` makes it a yes or no one. Five questions share the
set (ticket kind, urgency, tone, which part of a product a code line belongs to, refund
or not), interleaved, 41 rows. All items are text: nothing here measures image items.

Four more seeds belong to `USE-5` and `USE-7`, 40 rows each, written by hand and
unreviewed:

- `decide/plan-done.jsonl`: an `item` of a plan, the `answer` a run ended with, and `expect`
  `yes` (the answer shows the item was carried out) or `no` (it is missing, only planned,
  or only mentioned). Ten answers, two done items and two open ones each.
- `decide/steer-class.jsonl`: the `task` a run is on, a `message` typed at it, and `expect`
  one of `stop`, `redirect`, `add`, `question` (ten each).
- `decide/unverified-claim.jsonl`: an `answer` and `expect` `yes` (it claims the change works
  or was tested) or `no` (it says it is untested, or only describes the change).
- `decide/run-if.jsonl`: a task's `condition`, the `input` it would be judged over (a page, or a
  folder listing with what changed) and `expect` `yes` (the condition holds, run the task)
  or `no`. Twenty conditions, each once true and once false.

`decide/mail-triage.jsonl` is the seed for mail triage (`USE-6`): 40 mails with `from`,
`subject`, `body` and `expect`, the kind of mail (`personal`, `work`, `receipt`,
`newsletter`, `notification`, or `unsure`). It measures the **kind alone**, over the
default kinds. A live check asks the kind, the urgency and the reply question in one
request, and the fit it gives is shared by all three answers. So the numbers say how well
the kind is judged, and nothing about urgency or the reply mark. Written by hand,
unreviewed; most mails are plain and short.

## Tool menu ablation (`CTL-T-3`)

`decide/tool-menu.jsonl` holds 30 golden requests, each with the groups a person
would say it needs (`needs`, empty for plain conversation) and a `slice`: `base`,
`apps` (three granted apps) or `delegate` (two agent apps). `agent/decide/ablation.rs`
scores three ways to act on the model's answers: keep everything, hold back
whatever is under a fixed 0.30, and the policy table. The scoring is unit-tested.
The numbers come from the real model:

```
DECIDE_SERVER_BIN=...\llama-server.exe \
DECIDE_MODEL_PATH=...\Clef-Flash-Q4_K_M.gguf \
cargo test --test decide -- --ignored --nocapture tool_menu_ablation
```

`DECIDE_MENU_CASES=8` runs only the first cases. It prints held, missed, saved and
the miss rate per strategy, and fails when the policy table misses a larger share
of needed groups than the fixed cut-off, or `need_tools` could not bring back a
group that was held and needed. Prompt-token savings for the built-in toolsets
come from their real specs; Connected apps (1,200) and each agent app's rule block
(600) are estimates. It does not measure first-token time or whether a chat model
really calls `need_tools`; both need a chat turn.

## Handover pre-match (`CTL-T-3`, handover slice)

`decide/handover-prematch.jsonl` holds 67 golden requests for `USE-13`: each has the
rule a person would pick from the default Claude Code rules (`claude-code:U1` to
`U4`) or `none`, and some have the assistant's last reply (`previous`) for a request
that only makes sense after it. Only Claude Code is offered, so each request has one
right answer. `agent/decide/handover.rs` asks them exactly as a live turn does and
scores the early card: a card is raised when the model names a rule with a top
probability of 0.85 or more, and it is right when that is the expected rule. The ship
rule is precision of at least 0.95, because a wrong early card costs the person a
click. The scoring is unit-tested. The numbers come from the real model, and they
are the model's own probabilities, not a fit:

```
DECIDE_SERVER_BIN=...\llama-server.exe \
DECIDE_MODEL_PATH=...\Clef-Flash-Q4_K_M.gguf \
cargo test --test decide -- --ignored --nocapture handover_prematch
```

`DECIDE_HANDOVER_CASES=10` runs only the first cases. It prints the requests, how many
wanted a rule, the cards raised, how many were right, and precision and recall.
`decide/handover-route.jsonl` (the seed set) offers three rules of two agents and one
avoid line, so it does not cover the option list a live turn builds. Check the fit
against this slice before trusting it.
