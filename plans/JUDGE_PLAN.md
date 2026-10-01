# Project Poiesis - Judge Plan

**Poiesis asks small questions all the time and answers them badly.**

"Do these excerpts answer the question?" "Is this fact global or topical?"
"Is this lesson supported?" Each one is a full chat turn to the local engine,
answered in prose and parsed with `contains("yes")`. It is slow for what it
is, it has no idea how sure it is, and the parser reads "I don't know" as
*no*. Meanwhile the questions the harness *should* be asking before each turn
(which tools does this need? is this call risky?) are not asked at all,
because a chat turn per question is too expensive to put on the hot path.

A new class of model fixes this: **System One models**, led by TypeSafe's
Jev. Instead of generating text they take a state plus typed questions and
return every answer at once, each with a probability. Jev itself is cloud
only, but the open reproductions (SemIf, reflex, jqv, Winnow) show the same
contract can be served locally by reading option logits from an ordinary
model in one forward pass. Poiesis already runs that ordinary model.

This plan builds one **judge**: a typed decision seam with a local backend by
default and Jev as an opt-in upgrade, then moves the harness's small questions
onto it and asks the new ones that only become affordable at ~100 ms.

> ID prefixes: **JDG** the judge seam and its backends - **USE** a harness
> site that asks the judge - **-UI** frontend - **-T** tests.
>
> **Status: not started.** Written 2026-09-21.

---

## What this is not

- **Not a text generator.** Anything that produces prose stays on
  `drive_turn`: `rephrase_query` (RET-3), the profile synthesis, memory
  consolidation, reflection's lesson drafting, skill healing, compaction.
- **Not a security boundary.** A 4B model reading option logits can be talked
  into things. No judge answer ever *removes* a safeguard on its own
  authority unless the backend and its calibration earn it (`JDG-8`), and
  even then only above a threshold the user can see.
- **Not a new place data goes.** The default backend is local. Jev is a
  cloud call and is off until the user turns it on, per the rule in
  `memory_skill.rs` that calls the user did not ask for never leave the
  machine.
- **Not a Python sidecar.** jeff, SemIf and reflex ship PyTorch servers. We
  take their *method*, not their runtime: Poiesis already has `llama-server`.

---

## Where Poiesis stands

### The small questions today

| Site | Question | Answer shape | Where |
|---|---|---|---|
| RET-4 | Do these excerpts answer the query? | yes / no | `agent/retrieval.rs` `judges_sufficient` |
| SCP-1 | Does this fact apply to every answer? | global / topical | `agent/memory_skill.rs` `classify_scope` |
| Reflect critic | Is this lesson supported, actionable, right? | JSON `{ok, reason}` | `commands/reflect.rs` (~line 600) |

All three: `drive_turn` against the local engine, temperature 0, prose back,
substring parse, `None` on anything odd. None of them knows how confident it
is, so every caller treats the answer as certain.

### The questions nobody asks

- **Which toolsets does this turn need?** `run.rs` `build()` advertises every
  enabled toolset's specs on every turn. `toolsets.rs` already names the cost:
  a small model is "tempted to over-call". The prompt is also longer than it
  needs to be.
- **Did the answer finish plan item N?** `nudge_unfinished_plan` goes by plan
  state, not by what the answer says.
- **Is this steering message a stop, a redirect, or an addition?** `fleet.rs`
  queues it as text.
- **Is this conversation worth a reflection pass?** Reflection runs the full
  drafting prompt and lets it return `{"lessons":[]}`.
- **Is this mail urgent / does it need a reply?** `mail.rs` has no triage.
- **Is this tool call risky, and is it what the user asked for?** Permission
  prompts are decided by trust level and impact class alone.

### Shell facts to build against

- `RuntimeManager::engine_endpoint()` (`runtime/manager.rs`) gives the local
  engine's `(base_url, token)` or `None` when it isn't running.
- `llama-server` serves `/v1/chat/completions` with `logprobs` and
  `top_logprobs`, and `/completion` with `n_probs`. Nothing in the codebase
  requests either yet. **Verify against the bundled llama.cpp build** before
  `JDG-3`: if `top_logprobs` is capped below the option count, use
  `/completion` with `n_probs`.
- `rerankserver.rs` is the lazy-start / idle-stop sidecar pattern if a
  dedicated decision model is ever added (`JDG-10`, deferred).
- OpenRouter keys live in the Providers store (`cloud/mod.rs`,
  `Provider::OpenRouter`, verified via `/api/v1/key`).
- `golden.rs` cases carry the tools a correct reply uses. That is a ready
  labelled set for tool pre-selection (`USE-4`).
- Schema is v29.

---

## How the others do it

- **Jev (TypeSafe).** `{model, state, questions:{id:{type, instructions,
  criteria}}}` → `{answers, usage}`. Three primitives: `noul` (yes/no
  probability), `choice` (≤255 options, full distribution + confidence),
  `score` (2-10 described levels, probability-weighted mean). All questions
  on one state run in parallel. 70-500 ms, $0.042 / M input, output free,
  ~32k per question. Calibrated by training (RLCD). Cloud only, early access.
- **OpenRouter** serves Jev at `POST /api/alpha/decisions`, model
  `typesafe/jev-latest`. Same body; `noul` criteria need both `true` and
  `false`; response adds `id`, `provider`, `usage.cost`. Alpha.
- **SemIf / jqv / reflex.** One forward pass, read the logits of the declared
  option tokens, softmax over only those, no sampling. jqv does this zero-shot
  on Qwen3-32B with one fitted temperature and reaches 79 calibration against
  Jev's 83. reflex ships a per-primitive calibration file.
- **Winnow-12B, kev.** Serve Jev's `/v1/systemone` wire format from a local
  llama.cpp model.
- **LangChain** puts Jev in two places in a harness: a model router
  middleware and an "auto mode" gate that classifies tool calls before they
  run.

Benchmark reference (benchmarkheaven.com, 534 decisions): Jev 90 intelligence /
83 calibration; open 4B-12B reproductions 86-88 / 65-79. The gap is real but
small, and it is mostly calibration, which is the part we can fit.

---

## Settled decisions

1. **One seam, Jev's contract.** Our types mirror `noul` / `choice` / `score`
   so the Jev backend is a thin transport and a future `/v1/systemone` local
   server is a drop-in.
2. **Local by default, Jev opt-in.** No setting means local. With no local
   engine running and Jev off, the judge returns `Unavailable` and every
   caller keeps today's behaviour.
3. **Uncalibrated means advisory.** A backend without a fitted calibration
   for a primitive may only make things *stricter* or *cheaper*, never looser.
4. **Every answer carries its confidence and its backend** to the caller and,
   where the user can see the effect, to the UI.
5. **No judge on the path of a run that doesn't need one.** A decision that
   would need the local engine cold-started while the chat model is cloud is
   skipped, not waited on (`JDG-6`).
6. **The judge never blocks the stream.** Hot-path questions (`USE-4`) have a
   deadline; missing it means "no narrowing", not a stall.

---

## Phase 1 - The seam and the local backend

### `JDG-1` Types (`agent/judge/mod.rs`)

```rust
pub enum Question {
    Noul   { instructions: String, when_true: Option<String>, when_false: Option<String> },
    Choice { instructions: String, options: Vec<(String /*id*/, String /*criteria*/)> }, // ≤ 255
    Score  { instructions: String, levels: Vec<String> },                               // 2..=10
}
pub struct Ask<'a> { pub state: &'a str, pub questions: Vec<(&'a str /*id*/, Question)> }

pub enum Answer {
    Noul   { p_true: f32 },
    Choice { choice: String, probs: Vec<(String, f32)>, confidence: f32 },
    Score  { score: f32, probs: Vec<f32>, confidence: f32 },
}
pub struct Verdict {
    pub answers: BTreeMap<String, Answer>,
    pub backend: BackendId,      // Local { model } | Jev { via: TypeSafe | OpenRouter }
    pub calibrated: bool,        // JDG-5 fit present for every primitive used
    pub ms: u32,
}
pub enum JudgeError { Unavailable, Deadline, Failed(String) }
```

`confidence` is Jev's definition: 1 minus normalised entropy of the
distribution. Callers read `confidence` and `calibrated`, never re-derive them.

### `JDG-2` The `Judge` entry point

`judge::ask(ctx, ask, opts) -> Result<Verdict, JudgeError>` where
`opts = { deadline: Option<Duration>, allow_cold_start: bool, purpose: Purpose }`.
`Purpose` is an enum naming the calling site (`RetSufficiency`, `MemScope`,
`ReflectCritic`, `ToolSelect`, ...), used for logging (`JDG-7`), calibration
lookup (`JDG-5`) and the per-purpose backend override (`JDG-UI-1`).

Backend resolution, in order: the purpose's override → the global setting
`judge.backend` (`local` | `jev`) → `local`. If the chosen backend is
unavailable, fall back to the other **only** in the direction cloud → local.

### `JDG-3` Local backend: logit scoring over `llama-server`

For each question:

1. Build a prompt: a short fixed system line, the state, the question's
   instructions, then the options labelled with **single-token labels**
   (`A`, `B`, `C`, ...; for `noul`, `A` = true, `B` = false; for `score`,
   `1`..`N`). End with `Answer:`.
2. One request, `max_tokens: 1`, `temperature: 0`, logprobs for the top
   candidates (see the fact above for which endpoint).
3. Take the logprobs of the label tokens only, renormalise, apply the
   purpose's temperature (`JDG-5`, default 1.0).
4. A label missing from the returned top set gets the floor probability
   (`1e-4`). If *every* label is missing, `Failed("no label in top logprobs")`.

Multi-question asks send one request per question **concurrently**; the
shared prefix (system line + state) goes first so `llama-server`'s prompt
cache reuses it. Label order is randomised per request and mapped back,
because the benchmark caught a 4B reproduction dropping to 21% with reversed
options; order randomisation plus `JDG-T-3` keeps us honest about it.

Choice over > 26 options: labels `A`..`Z` then `AA`.. are multi-token, so
above 26 options split into a two-stage choice (group, then member), as Jev
itself does above its cardinality. Nothing in this plan needs more than ~20.

### `JDG-4` Move the three existing sites onto the judge

- `retrieval::judges_sufficient` → `Noul`, purpose `RetSufficiency`. Keep the
  `Option<bool>` signature for now: `Some(p_true >= 0.5)`, and pass the
  probability into the warning text the tool result already carries.
- `memory_skill::classify_scope` → `Choice { global, topical }`. Below 0.6
  confidence store `None` (unclassified), which `recall_for` already treats
  as global and the backfill retries.
- Reflect critic → `Choice { ok, unsupported, unactionable, wrong }`. The
  `reason` line is the winning option's criteria text. No JSON parse. Unreachable
  keeps today's `ok:false, "the critic couldn't be reached"`.

Behaviour at `p = 0.5` must match today's on the fixtures (`JDG-T-2`), so this
phase is a pure swap before anything uses the probabilities.

---

## Phase 2 - Calibration, logging, and Jev

### `JDG-5` Calibration

A probability of 0.9 has to mean roughly 90% before any threshold in this
plan means anything.

- Labelled sets per purpose under `src-tauri/tests/eval/judge/<purpose>.jsonl`
  (`{state, question, expected}`), 40-100 rows each. Seed them from: the RET
  and SCP test fixtures, reflection critic fixtures, and `golden.rs` cases for
  `ToolSelect`.
- `cargo test --test eval -- judge_calibrate` (ignored by default, needs a
  running engine) fits one temperature per (model file stem, purpose) by
  minimising NLL on a grid, and reports ECE before/after.
- Fits are stored in `settings` as `judge.calibration.<model>.<purpose>` =
  `{t, ece, n, fitted_at}`. `Verdict.calibrated` is true only when every
  question's purpose has a fit for the current model with `ece <= 0.08`.
- Model changes → no fit → `calibrated: false` → advisory mode (decision 3).
  The Runtime UI says so and offers the fit (`JDG-UI-1`).

Jev is treated as calibrated out of the box (that is what it is trained for);
its ECE is still measured by the same eval so the claim is checked, not
assumed.

### `JDG-6` Cost rules

- `allow_cold_start: false` (the default for every background and hot-path
  purpose) returns `Unavailable` immediately if `engine_endpoint()` is `None`
  and Jev is off.
- Hot-path purposes get a deadline (`USE-4`: 400 ms local, 800 ms Jev).
- Background purposes (`USE-6`) queue behind any active run on the local
  engine: a judge request never takes an engine slot from a streaming turn.

### `JDG-7` Every decision is logged

A `judge` row per call into the session log (`agent/log.rs`), linked to the
run when there is one: purpose, backend, ms, answers with probabilities,
`calibrated`, and the tokens in. Not the state itself (it can be large and
can hold private text); a 200-char preview plus a hash. This is what the
Activity view (`JDG-UI-2`) and the usage accounting (`JDG-UI-3`) read.

### `JDG-8` Jev backend

- Two transports behind one type: TypeSafe native (`api.typesafe.ai`, key in
  `secrets.rs` as `typesafe`) and OpenRouter decisions
  (`POST https://openrouter.ai/api/alpha/decisions`, model
  `typesafe/jev-latest`, reusing the existing OpenRouter key). OpenRouter is
  the default transport since most users already have that key.
- Map `JDG-1` types 1:1 onto the wire. `noul` always sends both `true` and
  `false` criteria (OpenRouter requires both; TypeSafe accepts it).
- State over ~30k tokens: truncate from the middle with a marker, and log it.
- The endpoint is alpha: any 4xx other than auth marks Jev unavailable for
  10 minutes and falls back to local, with one Activity line saying so.
- Cost goes into the existing usage accounting (`usage.cost` from OpenRouter).

---

## Phase 3 - The high-value uses

Ranked. Each says what the user feels, because a judge nobody notices is
plumbing, not a feature.

### `USE-4` Tool and skill pre-selection (hot path)

Before the first request of each user turn (not each loop step), one ask:
state = the user message plus the last assistant message (clipped to 2k
chars); questions = one `Noul` per enabled optional toolset ("Does answering
this need to search the web?", "...read or change files in the working
folder?", "...run code?", "...read or send mail?", "...drive a browser?",
"...delegate to other agents?") and one `Choice` over enabled skills plus
`none`.

Applied in `run.rs` `build()`, **after** prompt assembly so the golden gate is
untouched:

- Drop a toolset's specs when `p_true < 0.15` and the verdict is calibrated,
  or `< 0.05` uncalibrated. Always keep File System, Plan and Recall.
- Never narrow for a subagent (its set is already narrowed by persona) or a
  resumed run.
- A dropped toolset can be recovered mid-run: add one tiny always-present
  tool, `need_tools { toolsets: [...] }`, that re-adds specs from the next
  step on. The model is told in one line that other tools exist on request.
- Skill `Choice` above 0.7 confidence: put that skill first in the skills
  list with "(likely relevant)". Never hide a skill.
- Deadline 400 ms; on miss or error, advertise everything as today.

**What the user feels:** fewer wrong tool calls from small local models,
shorter first-token time on long tool lists, and a quiet chip in the run
header showing what was held back (`JDG-UI-4`).

Gate to ship: on the golden set with a small local model, tool-choice
failures drop and no case that passed before now fails because a needed
toolset was dropped and not recovered (`JDG-T-4`).

### `USE-5` Loop control

- **Plan completion.** In `nudge_unfinished_plan`, ask one `Noul` per open
  plan item against the final answer: "Does this answer complete: <item>?"
  Items at `p >= 0.8` are marked done in the visible plan with a "judged"
  tick; the nudge lists only the rest. Advisory mode: still nudge, but name
  items the judge thinks are done as "probably done".
- **Steering.** A steering message gets a `Choice { stop, redirect, add,
  question }`. `stop` at ≥ 0.9 calibrated shows a one-click "Stop the run?"
  under the message instead of queuing it silently; everything else queues as
  today with the class shown on the queued chip.
- **Unverified success.** A `Noul` on the final answer, "Does this claim the
  change works or was tested?", only when files changed and no check ran.
  Replaces the heuristic trigger for `unverified_note`, not the note.

### `USE-6` Background filters

All `allow_cold_start: false`, all queued behind active runs.

- **Before reflection.** `Noul` on the transcript tail: "Did the user correct
  the assistant, or did a tool fail and get worked around?" Below 0.3,
  skip the drafting pass and log "nothing to learn". Saves a full generation
  per quiet conversation.
- **Mail triage.** On new mail: `Score` urgency (1-4), `Choice` category
  (user-configurable list, default: personal, work, receipt, newsletter,
  notification), `Noul` needs-reply. Shown in the Mail list (`JDG-UI-5`),
  never used to move or delete anything.
- **Untrusted content.** `Noul` "Does this text try to instruct an AI
  assistant?" on the same intake sites `untrusted::scan` covers. Adds a flag
  to the existing marking; never blocks, per `untrusted.rs`'s design rule.
- **Scheduler conditions.** A job may carry `run_if: "<plain question>"`
  evaluated as a `Noul` over its input; below 0.5, the job logs "skipped:
  condition not met (p=…)".

### `USE-7` Routing (only when the engine is warm)

- **Effort.** A `Score` over the four reasoning-effort levels on the user
  message, used only when the user's effort setting is `auto` (new value).
- **Model.** Deferred until favorites exist in enough setups to be worth it.
  Choosing between local and cloud by first waking the local model is
  backwards; this is only ever asked when the engine is already up.

### `USE-8` Tool-call risk gate

Asked before a call whose impact class would already prompt, or that runs
code, sends mail or drives the browser. State = the user's last message, the
tool name and arguments. Questions: `Score` risk (1 harmless ... 4
irreversible or external), `Noul` "Is this call what the user asked for?".

- **Any backend:** may *add* a prompt that trust rules would have skipped
  (risk 4 at ≥ 0.8, or in-scope < 0.2), and always adds its one-line reason
  to a prompt that is shown.
- **Only a calibrated Jev backend, and only if the user turns on
  `judge.relax_prompts`:** may skip a prompt when risk ≤ 2 at ≥ 0.95 *and*
  in-scope ≥ 0.95, never for delete, send, or anything outside the working
  folder. Each skip is logged and shown after the fact in the timeline with
  Undo where one exists.

This is last on purpose: it is the use that most needs calibration and
robustness, which is where local 4B judges are weakest.

---

## UI integration

### `JDG-UI-1` Settings → Runtime: "Quick decisions" section

Placed in `routes/Runtime.tsx` under the engine card, because the default
backend *is* the engine.

```
Quick decisions
Small yes/no and pick-one questions the assistant asks itself — "do these
search results answer the question?", "which tools does this need?".

Backend   (•) This computer — uses the loaded model        [status dot]
          ( ) Jev via OpenRouter — faster, calibrated, cloud
              Sends the question and a short excerpt to OpenRouter.
              [Needs an OpenRouter key → Providers]   (disabled without one)

Calibration  Qwen3.5-4B-Q4_K_M · fitted 2026-09-22 · 6 of 7 purposes
             [Calibrate now]  (runs ~400 test questions, about 2 minutes)
             Uncalibrated purposes only make the assistant more careful.

[ ] Let calibrated decisions skip low-risk permission prompts   (Jev only)
    Off by default. Every skip is listed in Activity.

Advanced ▸  per-purpose backend override (table: purpose · backend · last p · avg ms)
```

States: engine not running → status dot grey, text "Decisions resume when
the engine is running". Jev selected and a 4xx fallback active → amber line
"Jev unavailable, using this computer until 14:32".

### `JDG-UI-2` Activity: decision rows

In `routes/Activity.tsx` and the run timeline, a `judge` row renders as one
line: `Decided · tools for this turn · web 0.04, files 0.91, code 0.12 · 83 ms · local`.
Collapsed by default under the run; expanding shows the question text and the
full distribution as a small bar per option. Rows from background purposes
group per hour ("12 decisions · mail triage").

### `JDG-UI-3` Usage

`routes/Usage.tsx` gains a "Decisions" line: count, backend split, and Jev
cost. Local decisions show "free · on this computer".

### `JDG-UI-4` Run header chip

When `USE-4` narrowed a turn, the run header shows a muted chip
`3 tools held back`. Hover lists them with their probabilities; click adds
them back for this run (same effect as `need_tools`). No chip when nothing
was held back.

### `JDG-UI-5` Mail list

Each message row gets a 4-step urgency bar on its left edge and a category
tag; a "Needs reply" filter joins the existing filters. Hover on the bar:
"Judged urgent (0.87)". A row the judge couldn't classify shows nothing
rather than a guess.

### `JDG-UI-6` Permission prompt reason

`SidePanel/PermissionPanel.tsx`: when a judge verdict exists for the call,
one line under the title in the existing secondary text style:
`Flagged: sends mail outside this conversation's request (0.91)`. A prompt the
judge *added* says so: `Asked because this looked irreversible`.

### `JDG-UI-7` Plan ticks

Plan items marked done by `USE-5` show the tick with a thin ring instead of a
filled circle; hover "Judged complete from the answer (0.88)". Clicking
toggles it back to open.

---

## Tests

- `JDG-T-1` Unit: label mapping, renormalisation, missing-label floor,
  randomised order maps back correctly, two-stage choice above 26 options,
  confidence = 1 - normalised entropy.
- `JDG-T-2` Swap parity: RET-4, SCP-1 and critic fixtures produce the same
  decisions at 0.5 as the prose path did (mock engine returning fixed
  logprobs).
- `JDG-T-3` Order robustness (eval, needs engine): each purpose set run with
  forward and reversed option order; accuracy may not differ by more than 5
  points, or the purpose ships uncalibrated.
- `JDG-T-4` Pre-selection (eval): golden set with narrowing on vs off; no new
  failures, and `need_tools` recovers every case where a needed toolset was
  dropped.
- `JDG-T-5` Jev transport: OpenRouter and TypeSafe request bodies from the
  same `Ask` (snapshot), `noul` criteria both present, 4xx fallback window.
- `JDG-T-6` Cost rules: no engine + Jev off → `Unavailable` in < 1 ms; a
  background ask waits while a run streams.
- `JDG-T-7` Frontend: Runtime section states (no engine, Jev without key,
  fallback active), the tools chip, the permission reason line.

---

## Order

1. `JDG-1`..`JDG-4` + `JDG-T-1`/`-T-2`. The seam, the local backend, the
   three existing sites swapped with no behaviour change.
2. `JDG-5`..`JDG-7`, `JDG-UI-1` (without the Jev option), `JDG-UI-2`.
   Calibration and visibility before anything relies on a threshold.
3. `USE-4` + `JDG-UI-4` + `JDG-T-4`. The first thing a user can feel.
4. `USE-6` + `JDG-UI-5`. Background filters; mail triage is the visible one.
5. `JDG-8` + the Jev half of `JDG-UI-1`, `JDG-UI-3`, `JDG-T-5`.
6. `USE-5` + `JDG-UI-7`.
7. `USE-7` effort only.
8. `USE-8` + `JDG-UI-6`, relax option last.

### Deferred

- `JDG-9` A local `/v1/systemone` endpoint so other apps (or a Poiesis skill
  script via tool RPC) can ask the judge.
- `JDG-10` A dedicated decision model as a fourth sidecar on the
  `rerankserver.rs` pattern (a 4B decision fine-tune at Q4, ~3 GB), for users
  whose chat model is cloud. Only if `USE-6` shows the cold-start skip is
  losing most background decisions.
- ModernBERT-class CPU judges (openJev Verdict, Laya): 512-token limit rules
  them out for everything above except mail triage; revisit if one ships an
  ONNX build worth an `ort` dependency.

---

## Sources

- TypeSafe, Introducing System One Models & Jev - https://typesafe.ai/blog/introducing-system-one-models-and-jev
- Flavio Copes, A deep dive into Jev - https://flaviocopes.com/jev/
- LangChain, Building a harness with Jev - https://www.langchain.com/blog/building-a-harness-with-jev
- OpenRouter decisions endpoint (oh-my-pi #12458) - https://github.com/can1357/oh-my-pi/issues/12458
- Jev-class benchmark - https://benchmarkheaven.com/jev-models
- SemIf / OpenJev - https://github.com/TheoLeeCJ/openjev
- Winnow-12B - https://huggingface.co/EldanRing/Winnow-12B
- jeff - https://github.com/logan-markewich/jeff
