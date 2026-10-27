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
> **Status: v2, in progress. Order steps 1 (`DEC-1`, the pin bump), 2 (the
> decision engine, seam and the five sites, local only), 3 (the Experiments
> framework, with Workspace mode first), 4 (policy, budgets, outcomes,
> calibration, Activity rows), 5 (the tool menu, `fast-slow`'s first
> feature), 6 (the handover pre-match), 7 (the two background filters), 8
> (the `classify` tool), 9 (plan completion, steering, the unverified-claim
> check and a task's "only run if") and 10 (mail triage, with its small inbox) are
> done 2026-10-23.**
> See "Implementation status" below for what was built and what was found. Written 2026-10-13. Revised 2026-10-20:
> Jev 1.14 added as a candidate decision model (`DEC-8`, `CTL-T-5`). **Revised
> 2026-10-23 (v2):** brought up to date after `CLI_ENGINES_PLAN` (Phases 0-5,
> LRN-1..5) and `CONNECTED_APPS_PLAN` (MCG, Composio, CAP-14) were built. Read
> "What v2 changed" below first.
>
> **Revised 2026-10-26 (v3):** after Erich's own test, the fast model is made
> visible: answers check themselves, a first-thought strip and a receipt,
> a composer hint, words instead of numbers, and Verify folded into
> `fast-slow`. Read "v3: make the fast model felt" below. Order steps 16-20,
> not started.

---

## What v2 changed

Two plans landed between v1 and v2. Neither breaks the design, but both built
things this plan has to sit on top of, not beside. In short:

1. **Facts.** Schema is v36, not v30, so the outcome table is v37. The engine
   pin is still `b10333`, and the llama.cpp facts are 10 days older. The image
   engine (`sd.cpp`) also takes GPU memory now.
2. **New settled decisions 12-14.** Background purposes never go to the cloud
   unless each one is opted in. The existing deterministic gates (the connector
   gate, agent-app approvals, `Toolset::ready`, persona app grants, engine
   guards) run first, and a decision only adds to them. Outcome labels are read
   from tables that already exist (`engine_signals`, `app_call_log`, the gate's
   activity rows, `Goal.stalled`) before anything new is recorded.
3. **`USE-4` sees the whole tool menu.** Connected apps, each MCP connector and
   `delegate` (with its agent-app rule blocks) are now units it can hold back,
   because for a small model they are most of the tool specs.
4. **`USE-12` stops duplicating the connector gate.** Read / change / send for
   app calls is already decided in Rust (`connector_gate.rs`). The risk check
   adds the "is this what you asked for?" line and a reason line, also on
   agent-app approvals. `relax-prompts` never touches app calls or agent-app
   approvals. A new add-only purpose, `AppKind`, re-checks app tool names the
   name rules call harmless.
5. **Two more existing sites for the swap (`DEC-6`):** the `/goal` check and the
   rule preview behind *Try a request*.
6. **New `USE-13` handover pre-match**, the site `CLI_ENGINES_PLAN` RTE-6
   already points at.
7. **Mail triage has no surface to land on.** There is no inbox list and no
   background fetch (`Mail.tsx` is account setup, `mail.rs` fetches only when a
   tool asks). `USE-6` now includes a small inbox (`USE-6a`, `USE-UI-4`), and
   moves later in the order.
8. **More untrusted-text intake** (connector output, agent-app reports, recall
   over the MCP endpoint) means `USE-8`'s flag gets its own budget.

---

## v3: make the fast model felt (2026-10-26)

**Why.** Erich turned `fast-slow` on and used it. The decision model ran (the
`tools held back` chip showed on every turn), and the result was still
nothing a user can feel. Three causes, all in what steps 1-15 built:

- **It shows mechanics, not meaning.** `3 tools held back`, hover *web 0.03*.
  The user can't do anything with that: it says what did *not* happen, in
  units the user never chose, with a number nobody can read.
- **Its one visible feature needs a click.** Verify was its own experiment, behind an
  action on the message, and its first step is a slow language-model call.
  So it feels like a slow model with a fast model attached.
- **It acts before or beside the turn, never in front of the user.** Faster,
  shorter and quieter happen without a trace.

v3 keeps the engine, seam, policy, outcomes and every `USE-*` built so far.
It changes what the user sees, and adds four things (`SEE-1`..`SEE-5`).
Settled decisions 15-17 are new. Chosen by Erich 2026-10-26: answers check
themselves (`SEE-1`), the first-thought strip (`SEE-2`), the composer hint
(`SEE-3`), the receipt (`SEE-2`'s second state), and Verify folded into
`fast-slow` (`SEE-5`). The live run monitor was offered and not chosen
(see Deferred).

### `SEE-0` The rule

Every fast decision that changes something shows **at the spot where it
changed it**, saying **what I will do or did, in words about the user's
task**. Never a count of things not done, never a unit name the user didn't
choose, never a bare probability on the main surface. The number moves to the
detail card and the hover. See settled decisions 15-17.

### `SEE-1` Answers check themselves

With `fast-slow` on, every finished chat answer is checked on its own, while
it streams. There's no click and no language-model call.

**Evidence: what the turn already read.** The pool is the results of this
turn's own tool calls, taken from the session log: web search results and
fetched pages, files read and folder-search passages, connected-app reads,
mail read with `read_mail`, and agent-app reports. Each passage keeps its
source and when it was read. The auto check adds no search and no fetch, so
nothing leaves the computer that the turn didn't already send. Passages that
`UntrustedFlag` marked are fenced as in `USE-10`.

**Claims without a slow call.** The answer's plain text is split into
sentences in code. Code blocks, headings, tables and quoted user text are
skipped, and a list item counts as a sentence. A sentence counts as **final**
once the stream has moved past its end, so checks start mid-stream, in order,
and marks appear a moment after each sentence. Per sentence, one ask with
two questions:

1. **`ClaimCheckable`** (new purpose, `noul` + `choice`): does this sentence
   state something that could be checked (a number, a name, a date, a quote, a
   result), and is it a **fact about the world** or **something I say I did**?
2. For a fact: the existing **`ClaimVerdict`** against the 2 best passages
   from the pool (`best_window` from `agent/verify.rs`, similarity ≥ 0.5). A
   sentence with no passage over the threshold is "nothing I read backs this"
   and is not asked.

**"Something I say I did"** is matched in code against this run's tool calls
(the ledger in `agent/ledger.rs`). The table maps phrases to call families:
sent → `send_mail` or an app send, created or wrote a file → `write_file` or
`edit_file`, ran or tested → `run_command` or a check run, searched → `web_search`.
When nothing in the run matches, the mark is red: *"No step in this run did
this."* The existing `UnverifiedClaim` result (`loopctl.rs`, "says it works,
no check ran") becomes a mark on its sentence instead of only a nudge.

**Limits and order.** At most 40 sentences an answer. Past that, the trust
line says "first 40 sentences checked". Hot-path decisions (`ToolSelect`,
`HandoverRoute`, the tool-call gate) always go before check asks in the
engine's queue, so a check never slows the next turn. On a CPU-only machine,
checks finish after the answer, and the trust line counts them down.

**Escalation.** The blind slow review (`Live::review`, unchanged) runs by
itself only for a **sure refuted** fact, at most 2 an answer. Every other
mark has **Review this** in its card. The 4-review cap of a deep check is
unchanged.

**Nothing read.** When the pool is empty (the model answered from what it
knows), no sentence is marked. The trust line says *"Answered from what I
know · nothing checked"*, with **Check against the web** when the Web search
tool is available. That runs the deep check.

**The deep check** is today's Verify run (`USE-10`: slow listing, web, up to 4
reviews), renamed **Check more deeply** and kept in the message action row.
Its result replaces the auto result for that message, as checking again does
today.

**Storage.** The `verifications` table gets a `mode` column (`auto` | `deep`)
in the next schema version. One row per message, as now. Labels from
**This is right** / **This is wrong** work as in `USE-10`, source `verify-mark`.

### `SEE-UI-1` Marks and the trust line *(SEE-1)*

In `Conversation/VerifyPanel.tsx` (`CheckedAnswer`) and `RunText.tsx`:

- **The sweep.** Each checked sentence gets its mark when its answer arrives:
  - **Backed** sentences show a faint green dotted underline that fades out
    over 1.5 s. You see the check pass over the text, then it settles back to
    clean text.
  - **Contradicted** sentences: red underline. **Nothing backs it**: amber
    dotted. **Sources disagree**: amber solid. **"I said I did this, no step
    did it"**: red with a small ⚑.

  Under `prefers-reduced-motion`, backed sentences get no fade. The existing
  **Underline supported** toggle keeps backed sentences marked.
- **The trust line**, one line under the answer, in a fixed-height row, so
  the answer never jumps:
  - while checking: `Checking against what I read · 4 of 9`
  - after: `✓ 7 backed by what I read · ⚠ 1 nothing backs it · ✕ 1
    contradicts nytimes.com`
  - nothing read: `Answered from what I know · nothing checked · [Check
    against the web]`

  Each segment filters the claim list, which stays collapsed by default.
  **Revise** sits at the line's right end when anything is red or amber.
- **The card** is the existing one: verdict, how it was reached, passages with
  source and date, **This is right** / **This is wrong**, plus **Review this**
  when it wasn't reviewed. The number appears only here ("quick check, not
  sure · 0.61").
- **Action row:** **Verify** becomes **Check more deeply**. It's shown
  while `fast-slow` is on and hidden while an auto or deep check runs.

### `SEE-2` The first-thought strip, then the receipt

One component, `Conversation/ThoughtStrip.tsx`, in two states.

**State 1, before the slow model's first token: what I understood.** It
appears in `AgentRun.tsx` above the run's first text as soon as the turn's
hot-path decisions settle (`ToolSelect`, `HandoverRoute`, plus `RetSufficiency`
when the folder is attached). Within the deadline means tens of milliseconds
locally. A decision that missed its deadline shows nothing; there's never a
spinner. Parts, in this order, each shown only if it applies:

| Part | Words | Click |
|---|---|---|
| Where I'll look | `Looking in Google Calendar` · `in your files` · `on the web` (units kept, by label, not the core tools) or `Answering directly` | – |
| What I'm leaving out | `Not searching the web`, **only for a unit the user turned on themselves** (Web search on, a connected app the user named). Units the user never chose are not mentioned. | **Search the web too** (same as `need_tools`, a label) |
| Who does it | `Claude Code's job, by your rule U2` (the handover card follows as `USE-UI-10`) | – |
| Speed | `⚡ 38 ms`, muted, last | – |

Example: `⚡ Looking in Google Calendar · not searching the web · 38 ms`.

**State 2, when the run ends: the receipt.** The strip moves to the foot of the
run and becomes one muted line:
`⚡ 6 quick decisions · prompt 2,900 tokens shorter · 9 sentences checked`.
Only measured facts go here: tokens from the held-back specs' size, sentences
from `SEE-1`, a decision count from the log. It never says "saved n seconds"
unless that was measured. Expanding it lists one row per decision that
**acted or was shown**, from the decision log for this turn (`DEC-7`, keyed by
turn id):

```
⚡ 6 quick decisions                                              ▾
  Left out web, browser and mail; you didn't ask for them      [That was wrong]
  Matched your rule U2: Claude Code takes test fixes           [That was wrong]
  Looks finished from the answer: plan step 3                  [That was wrong]
  Checked 9 sentences against 4 pages, 1 contradicted          [Show]
  Asked before send_mail: recipient isn't in this chat         –
  3 more that changed nothing
```

- **That was wrong** writes an outcome label (`CTL-3`, source `receipt`) and,
  where an undo exists, offers it: *add the tools back and retry*, *reopen the
  step*, *do it myself*.
- The hover on a row shows backend, model, confidence number and time.
- **Missing engine:** once per conversation, the strip says `⚡ Quick
  thinking is off: the decision model isn't running [Start]` (the start path
  of `DEC-UI-2`). It's never silent.

**Removed:** the RunBar chip (`USE-UI-1`, `heldSegment` in `RunBar.tsx`) and its
tests (`RunBar.held.test.tsx`). Its click (add the tools back) is now the
strip's **Search the web too** and the receipt's **That was wrong**.

### `SEE-3` The composer reads your message as you type

In `Composer/Composer.tsx`, 400 ms after the last keystroke, when the text is
12 characters or more, isn't a `/` command, and the **local** engine is warm.
It's one ask with the questions that already exist: `HandoverRoute`,
`ToolSelect` for units that are off but would be needed, and `SteerClass`
while a run is live.

At most one hint is shown, in this priority order. A hint shows only when
the decision is sure (≥ 0.85):

| When | Hint | Action |
|---|---|---|
| A run is live and the text is a stop | `→ This will stop the run` | **Add it instead** |
| A handover rule matches | `→ Claude Code will take this (your rule U2)` | **Do it yourself** |
| The web is needed and Web search is off | `→ Needs the web, which is off` | **Turn on for this message** |

- **Placement:** inside the composer's bottom row, right-aligned before the
  send button, one line, truncated with the full text on hover. **The composer
  never changes height** (the fixed-size rule).
- **No flicker:** a hint changes only after the new answer has held for a
  second debounce. Text edits that don't flip the answer leave it alone.
- **Actions** use the existing per-message modifiers (`ModifierChips.tsx`). They
  are not new syntax. Taking or dismissing a hint is a label.
- **Reuse at send:** if the text is unchanged since the last ask, the send
  reuses those answers, and the strip (`SEE-2`) appears at once.
- **Local only** (settled decision 16). With only a cloud backend, there are
  no hints, and Runtime's quick decisions card says so in one line.

### `SEE-4` Words, not numbers, on every surface `fast-slow` touches

A shared helper `sureWord(p, calibrated)` in `lib/decisions.ts`: sure → stated
plainly; middling (0.6-0.85, or any uncalibrated value under 0.9) →
"probably"; below that, the decision didn't act and isn't shown. Applied to:

- `USE-UI-2` steer tag: unchanged (it's already words).
- `USE-UI-3` plan tick hover: `Looks finished from my answer`, number in the title.
- `USE-UI-6` task skip row: `Skipped: no new release since last time`.
- `USE-UI-8` prompt reason: `Flagged: the recipient isn't in this
  conversation`. The number moves to the line's hover.
- `USE-UI-10`: `Matches your rule U2`. The doubt line becomes `I'm not sure this
  matches U2`.
- `USE-UI-11`: `not yet` / `met`, number in the title.
- `USE-UI-4` mail bar hover: `Looks urgent`.
- `USE-UI-5` classify table: **keeps** its confidence column. It's a data
  table the user sorts by.

### `SEE-5` Verify is part of `fast-slow`

- `verify` leaves `experiments.rs` `REGISTRY`. `fast-slow`'s `what_changes` gets
  two lines: *"I check each answer against what I read for it, and mark what
  nothing backs or what contradicts it. This adds no search and nothing
  leaves this computer"* and *"Check more deeply searches the web for an
  answer's claims (their words leave this computer) and has my slower model
  review up to four"*.
- `Requirement::Evidence` is dropped. The auto check needs none. **Check more
  deeply**'s web part needs the Web search tool, said on its hover when off.
- **Migration:** if `experiment.verify` was on, turn `fast-slow` on. Then
  delete the key either way.
- `commands/verify.rs` gates on `fast-slow`. The `ClaimVerdict` policy row's
  experiment becomes `fast-slow`. The new `ClaimCheckable` row: Verify mode,
  visible, not background, 0.85, no escalation, `fast-slow`.
- `experiments_report.rs`: `fast-slow`'s report adds "checked n answers ·
  m contradicted". Its **Reset its data** also clears `verifications`.
- `EXP-UI-1`: the Verify answers card is gone.

### `SEE-T` Tests

- `SEE-T-1` sentences: splitting skips code, headings, tables and quotes;
  finality while streaming (a sentence isn't final until the stream moves past
  it); the 40 cap.
- `SEE-T-2` action claims: each phrase family matched and missed against a
  fake ledger. `UnverifiedClaim` lands on the right sentence.
- `SEE-T-3` evidence pool from a recorded session log: only this turn's
  results, sources kept, flagged passages fenced, nothing fetched.
- `SEE-T-4` queue order: a hot-path ask overtakes queued check asks.
- `SEE-T-5` the strip: each part's words, the "user turned it on" rule for
  *not searching*, nothing on a missed deadline, the missing-engine line once.
- `SEE-T-6` the receipt: rows only for decisions that acted, measured figures
  only, **That was wrong** writes a `receipt` label and offers the undo.
- `SEE-T-7` composer hint: debounce, priority, the 0.85 floor, no flicker,
  reuse at send, nothing with a cloud-only backend, fixed composer height.
- `SEE-T-8` `sureWord` and every surface in `SEE-4`.
- `SEE-T-9` the fold: migration both ways, gating, report and reset.
- `SEE-T-10` **a real run, by Erich, with Clef local:** the checklist in the
  Order's step 20.

---

## v4: the stream goes quiet (2026-10-27)

**Why.** Erich ran v3 and the chat was worse than before it. Under one calendar
answer there were a thought line, a receipt, a trust line (`1 nothing backs it`),
a claim list, a `Not confirmed` row, a toggle, four actions and a dotted underline
on every sentence. `no events tomorrow` came back `Not confirmed`. A menu question
said `Left out code execution, artifacts, image generation ...` with `That was wrong`.
Nobody who did not read this plan could tell what any of it meant, and most of it
was true but not useful.

**The rule (`SEE-0`, replaced).** The stream shows the answer. The quick decisions
work without a trace, and say something only when it changes what the person
should do: a statement that looks wrong. Everything else is on the Activity page.
A check that finds nothing says nothing. Words the person would not use
(`claims`, `refuted`, `sentences checked`, `tokens shorter`, `decisions`) do not
appear in the chat.

**What the stream shows now.**

| Before the answer | While it runs | After it |
|---|---|---|
| Nothing added | The run's own steps and text | The answer, then at most one line |

- **One line, only when something looks wrong:** `1 statement may be wrong`, with
  `Why?` and `Fix it`. The statement is underlined in the text (red when it is
  contradicted or no step did it, amber when sources disagree). `Why?` or the
  underline opens one card: the statement, the reason, the passage and where it
  came from. A statement nothing backs up is only unknown and is never shown.
- **One action under an answer for checking it:** `Double-check` (web search plus the
  slower model, as before). It replaces `Check more deeply` and `Check against the
  web`. When the person asks, the check always answers in a line:
  `Checked: nothing looks wrong`. The other actions are the ones the chat always had:
  `why this answer?`, `Try again from here`, `Rewind to before this`.

**Removed from the chat** (the data and the backend stay; only the surfaces go):
the first-thought strip and its engine-off line, the receipt and `That was wrong`,
the trust line and its filters, the claim list, `Not confirmed`, `Underline
supported`, the sweep, `This is right` / `This is wrong` / `Review this` on the
card, the `stop / redirect / addition / question` pill on a message sent
mid-run, the ringed plan tick (`USE-UI-3`) and the dot on a rule chip (`USE-13`).
A held stop keeps its question, `Stop the run? Stop / Keep going`, because it is
a real decision. Unchanged: the composer hint (`SEE-3`), the handover card, the
plan ticks themselves (now a plain tick), outside-text marking.

**Consequences to know.**

- No UI writes `markClaim`, `reviewClaim`, `markReceiptWrong` or `restoreHeldTools`
  labels any more, so the over-time measure `fewer than 15% of answer verdicts are
  marked wrong by you` cannot be taken and is out of the graduation line. The
  calibration now learns only from what the person does anyway (`Fix it`, `Try again`).
- Held-back tool groups are no longer named to the person. The model can still ask
  for a group back on its own, which the graduation line already measures.
- `SEE-2` (strip, receipt), `SEE-1`'s trust line and `SEE-4`'s hover numbers on
  these surfaces are superseded by this section. `sureWord` stays for the composer
  hint and the handover card.

**Test.** `VerifyPanel.test.tsx` (the one action, silence when nothing looks
wrong, the line, the card, `Fix it`, a failed fix), `verify.test.ts` (what is worth
saying and drawing), `UserTurn.steer.test.tsx` (no pill, held stop kept).

---

## Implementation status

One entry per Order step, written when the step ends. Each step stops for
review before the next one starts.

### Order step 0 - prerequisites: partly done (2026-10-23)

- **Not done: committing the CLI_ENGINES review fixes.** They are still
  uncommitted (`subagents.rs`, `engines/*`, `fake-codex.mjs`,
  `CLI_ENGINES_PLAN.md`). Nothing was committed, because a commit was not
  asked for. `DEC-1` below sits in the same working tree, but touches only
  `runtime/manifest.rs` and `runtime/download.rs`, so the two can be committed
  separately.
- **Done: the llama.cpp facts were re-checked** against GitHub on 2026-10-23.
  Results are in "Shell facts" and in `DEC-1` below.

### Order step 1 - `DEC-1` engine pin bump: DONE (2026-10-23)

**Changed**

- `manifest.rs`: `PINNED_BUILD_TAG` `b10333` -> **`b11418`**.
- `manifest.rs`: the Windows HIP backend now matches `win` + `rocm` + `x64`.
  Upstream renamed `llama-bNNNNN-bin-win-hip-radeon-x64.zip` to
  `llama-bNNNNN-bin-win-rocm-10.0-x64.zip` between the two builds. The old
  keywords would have found no asset (HIP is opt-in, so nobody would have
  noticed until they chose it).
- `download.rs`: the recorded asset list and the matcher tests now use the
  real `b11418` Windows assets. They also cover the two new `arm64` zips
  (`cuda-13.4-arm64`, `vulkan-arm64`), which the `x64` keyword already keeps
  out.

**Why `b11418`.** It is the first tag whose commit is the merge of #29969.
All three PRs are merged: #29818 `/v1/systemone` (2026-10-02, `a4cb4c6`),
#29831 Clef text (2026-10-03, `99b9548`), #29969 Clef vision (2026-10-05,
`9871df5`). The release `b11418` targets `9871df5` exactly. Latest tag on the
day was `b11836`; a newer pin was not taken, because the plan asks for the
first build and every extra day is extra drift to re-verify.

**Verified**

| Plan item | Result |
|---|---|
| CUDA zip naming | CUDA 12 is still `cuda-12.4`; CUDA 13 moved `13.3` -> `13.4`; matched by `cuda-12` / `cuda-13`, so no code change. Matcher tests updated and pass. |
| `rerankserver.rs`: `--reranking` | Still in `common/arg.cpp` at `b11418` (`{"--rerank", "--reranking"}`). Smoke test on the real build: `/rerank` returns raw logits (6.18, 5.10, -10.20 for a match, a near miss and an unrelated passage), same JSON shape `rerank()` reads. |
| `embedserver.rs`: `--embeddings --pooling mean` | Both flags still present. `tests/embed.rs` (200 strings, unit norm) passes against the `b11418` binary. |
| Chat streaming and tool-call format | Streaming works (8 chunks + `[DONE]`). Gemma 3 4B did not call the tool in the smoke request, on **either** build, with identical output, so that is the model, not the bump. Prompt-assembly golden gate passes (it does not depend on the binary). |
| Spawn flags (`--jinja`, `--api-key`, `--ctx-size`, `--n-gpu-layers`) | All still present. |

Tests: `cargo test --lib runtime::` 85 pass. Full `cargo test --lib`: 1061 pass,
1 fail. The failure is `db::tests::a_fork_keeps_the_summary_it_can_still_account_for`.
It fails the same way on a clean `HEAD` (checked with the changes stashed), so it
is **not caused by this step**. It is unrelated to the runtime and is left alone.

**Not verified, and why**

- The smoke tests ran on the **CPU** build (19 MB), not the CUDA 12 build
  (about 650 MB with its DLLs). The flags and endpoints are the same code; a
  GPU load of the new build is still worth one manual check.
- `DEC-T-4` was done by hand with a throwaway script, not as a checked-in test.
  The repo has no automated smoke for rerank or chat (only `embed.rs`). Adding
  one is cheap if wanted.

**Findings that change later steps**

1. **Existing installs keep `runtimes/b10333/`** (about 600 MB per backend).
   The app provisions `runtimes/b11418/<backend>/` on next engine load, so
   nothing breaks, but the old folder is never cleaned up. A cleanup would
   belong with whatever next touches `commands/runtime.rs`.
2. **`/v1/systemone` as built** (from `tools/server/README.md` and
   `server-decision.h` at `b11418`). `DEC-5` should code to this, not to the
   TypeSafe docs:
   - Request: `state` (string, object or array; non-strings go to the model as
     JSON), optional `images` (data URLs only), `questions` map with `type`,
     `instructions` and `criteria`. For `noul`, `criteria` is **optional**
     locally (OpenRouter still wants both sides).
   - Response: `answers.<id>`. `choice` has `choice`, `probabilities` (a map
     keyed by option) and `confidence`. `score` has `score`, `legend` and
     `probabilities` keyed by level **index as a string**. `noul` has `noul`
     (the probability of true). `usage.input_tokens`; `output_tokens` is 0.
   - Errors: 400 bad request; **501** if the model is not a decision model
     (this is the `DEC-3` head check, for free); 501 for images when the model
     can't take them or no projector is loaded.
   - **Clef reads all questions of a request in one prompt and decides them
     jointly** (`is_joint`), the other models answer each independently. So
     batching several questions into one Clef ask is not free of interaction.
     `USE-4`'s "one Noul per unit" is one joint prompt on Clef.
   - **A server running Clef serves only this endpoint.** There is no text
     generation on it. This confirms `DEC-4`: a separate sidecar, never the
     chat engine.
   - The whole prompt must fit `--ubatch-size`, so the sidecar must be started
     with a `--ubatch-size` at least as large as the state cap (16k for Clef).
   - Probabilities are already scaled by temperatures stored in the model
     file. `CTL-4` fits on top of those, so a fitted `t` is a correction to the
     model's own, not a replacement.
   - Vision needs `--mmproj`, as the plan's catalog already says.
3. `spawn_engine` always passes `--jinja`, `--ctx-size` and `-ngl`. Whether a
   Clef server accepts `--jinja` is **not verified** (no Clef GGUF was
   downloaded in this step). `DEC-4` must test the real argument list against
   a Clef GGUF before reusing `spawn_engine` unchanged.

### Order step 2 - engine, catalog, seam, the five sites: DONE (2026-10-23)

Local backend only. Everything below was run against a real Clef-flash Q4_K_M
(`ggml-org/Clef-Flash-GGUF`, 6,486,448,288 bytes) and its projector on the
`b11418` CPU build, unless it says otherwise.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| `DEC-2` seam types | `agent/decide/mod.rs` | `Question`, `Ask<'a>`, `Answer`, `Verdict`, `DecideError`, `Opts`. `confidence` is 1 minus the normalised entropy, computed here for every answer. `BackendCaps` is defined but unused until the cloud step. `Verdict.id` is a plain string, not a `DecisionId` type. |
| `DEC-3` catalog + head check | `runtime/decideserver.rs`, `commands/decide.rs` | Two entries pinned to file names, byte sizes and URLs read from the Hugging Face tree API (Clef-flash 6.49 GB, Clef 19.2 GB, each with its `mmproj`). The download checks the byte count, installs the row (role `decide`), then runs the head check. A file that fails is deleted again. |
| `DEC-4` sidecar | `runtime/decideserver.rs` | Lazy start, idle stop after 5 minutes, one instance, shared binary, `DecideManager` in app state, stopped on exit. Placement `Auto`/`GPU`/`CPU` (`plan_fit`): the card only when the model fits beside the loaded chat model and a local image backend (counted as 4 GB) with 1 GB spare. Pre-warm when a chat turn starts (`agent_chat_cmd`). A hot purpose never waits for a cold engine: it returns `Deadline`, starts one for next time, and the caller keeps its old path. A background purpose waits for the start, and on the card waits for running chat turns to end (up to a minute). After a failed start nothing retries for a minute. |
| `DEC-5` local backend, resolution, limits | `agent/decide/local.rs`, `policy.rs`, `mod.rs` | `/v1/systemone` request builder and parser. `resolve_backend` implements purpose override, then `decide.backend`, then local, and background purposes ignore the global setting (decision 12). State is cut from the middle to the smaller of 16k and the purpose's own cap, and the cut is logged to Activity. Cloud half is step 12. |
| `DEC-6` the five sites | `retrieval.rs`, `memory_skill.rs` + `commands/memory.rs`, `commands/reflect.rs`, `commands/agent.rs` (`goal_check_cmd`), `commands/engines.rs` + `engines/rules.rs` | Each asks the decision first and falls back to its old call when `ask` says no. Mapping lives in `agent/decide/sites.rs`, `agent/goal.rs` (`combine`) and `engines/rules.rs` (`preview_from_option`). See "Behaviour notes" below. |
| `DEC-7` decision log | `agent/log.rs` | A `decision` row (purpose, backend, ms, calibrated, answers with probabilities, input tokens, 200-character preview, state hash, action) per call **when a conversation is known**. |
| `DEC-UI-1` | `components/DecisionModel/DecisionModel.tsx`, mounted in `routes/Models.tsx` (Chat tab, below "On this PC") | Local rows only: name, size, fit line with the loaded chat model's name, Download / Use this one / Delete, progress bar, the head-check message on failure. |
| `DEC-UI-2` | same component, mounted in `routes/Runtime.tsx` (Chat tab, under Status) | Status line (grey / amber / green with where it runs and the median time / red with the reason), model row linking to Models, "Runs on" Automatic / Graphics card / Processor, calibration text, read-only Advanced table. |
| `USE-UI-11` | `components/Composer/RunBar.tsx`, `lib/api.ts` | `GoalCheck.p` (optional). The bar shows `not yet (0.12)` while a goal runs and `Done: <evidence> (0.91)` when met. No number without a decision. |

**Behaviour notes** (what the swap does that the old path did not)

- **Hot sites** (`RET-4` sufficiency, goal check, rule preview): skipped while
  the engine is cold or on the processor, so they behave as before until it is
  warm on the card.
- **Memory scope** now works when no chat model is loaded, because the decision
  engine does not need one. `unsure` or confidence below 0.6 stores nothing; the
  backfill then asks again and defaults to `global`, as before.
- **Critic**: the winning option decides. `ok` clears the lesson; any other
  option demotes it to a proposal with that option's sentence as the reason.
  The `ok` text says imprecise wording is fine, to keep the `CRT-4` leniency.
- **Goal check**: the decision and the written check run at the same time. With
  no calibration (always, today) "met" needs the decision at 0.8 or more **and**
  the written check agreeing, so the decision can only make it stricter.
- **Rule preview**: the options are the user's enabled "use it for" rules
  (`claude-code:U2`) plus `none` and `unsure`; the "do not use" rules are quoted
  in the question. `unsure`, or confidence below 0.6, asks the model the old way.
  A confident `none` is "I would do it myself". Guards run in Rust as before.
- **Retrieval**: the model-facing warning now says the chance in percent. The
  step line the user reads is unchanged.

**Verified**

- `cargo test --lib`: **1120 pass, 1 fail**, 15 ignored. The one failure is
  `db::tests::a_fork_keeps_the_summary_it_can_still_account_for`, the same test
  that failed in step 1. It is flaky: run alone it failed 3 times in 4, then
  passed in the next full run (1134 pass, 0 fail, 15 ignored, after step 3). Not
  touched or fixed by this work; it looks timing-dependent.
- New tests: 40 under `agent::decide` (one is the ignored end-to-end test), 10 in
  `runtime::decideserver`, 2 in `engines::rules`, 7 in `agent::goal`, 1 in
  `agent::log`; frontend 10 more
  (`DecisionModel.test.ts`, `RunBar.goal.test.tsx`). `tsc --noEmit` is clean.
  Full `vitest`: 956 of 957 pass (see "Not caused by this step").
- `DEC-T-1` (local half): request body (both `noul` criteria, `choice` map,
  `score` array, images only when present), response mapping against the **real
  reply** recorded from Clef-flash, truncation, confidence.
- `DEC-T-2`: parity at 0.5 for sufficiency, scope and critic; fallback
  (`Unavailable`) for every purpose; the goal cases from the plan (uncalibrated
  0.9 with a prose "no" is "not yet"; a calibrated "met" with broken prose is
  "met"; `GoalCheck` keeps its fields and gains `p`); rule preview maps back to
  the same `Preview`, `unsure` falls back, a guard still blocks.
- `DEC-T-3`: a mock server answering 501 is rejected, and so is a real Gemma 3
  GGUF (integration test, `DECIDE_NOT_A_DECISION_MODEL`).
- `DEC-T-5`: a background purpose resolves local with the cloud chosen, and with
  no model is `Unavailable` (policy test and an `ask` test on a mock app).
- Real engine, `tests/decide.rs` (ignored, needs files): the head check passes;
  sufficiency answers correctly on an answering and a non-answering excerpt; a
  ~3,000-token state goes through; a red square is read as red through the
  projector. `ask_end_to_end_skips_cold_then_answers_and_logs` (ignored, in
  `agent::decide`): a cold hot ask returns `Deadline` and starts the engine, a
  background ask waits and is answered, decisions are logged.
- **Argument list**: the one `spawn_engine` builds (`--jinja`, `--ctx-size`,
  `-ngl`) is accepted by a Clef server unchanged. This closes finding 3 of step 1.

**Not verified, and why**

- **A GPU run.** This machine has a GTX 1060 with 6 GB. Clef-flash plus its
  projector is about 6.9 GB, so `Auto` puts it on the processor here, and the
  three hot sites stay on today's path on this computer. Only the two background
  sites use it. On the processor one 200-token decision took about 12 s (8
  threads), so a hot purpose cannot meet its 5 s deadline there. Clef-flash needs
  an 8 GB card for the hot path, as the plan says. Nobody has measured it on a
  card.
- **GPU memory cost of `--ubatch-size 16384`.** The server log at the default
  verbosity does not print buffer sizes.
- **The screens were not opened in the running app.** They are covered by
  typecheck, helper tests and the existing suites. The download button through
  the Tauri command (progress bar, head check, delete on failure) was not
  driven end to end; its pieces were.
- **`DEC-T-1` cloud half, `DEC-T-1a`, `DEC-T-1b`**: step 12.

**Not built from the plan text (decided, not forgotten)**

- Vision is reported from "a projector sits beside the model", not from a probe
  of `/props`. `/props` does report `modalities.vision` (checked), so the probe
  can replace this when a purpose needs it (`USE-11`).
- The headroom check is **not** repeated when the image engine starts. Nothing
  hooks image-job start yet. Placement is decided at engine start only.
- `DEC-7` writes no row for decisions without a conversation: the rule preview
  and the memory backfill. `CTL-3`'s `decision_outcomes` table (schema v37) is the
  natural home, because it is not tied to a conversation. The `action` field is
  always empty until `CTL-2`.
- `DEC-UI-1`/`DEC-UI-2`: no cloud group, no cloud radio, no "Calibrate now"
  button, no per-purpose "Allow the cloud" control, and the Advanced table has
  only the columns that exist (question, background, if not ready, gives up
  after). They arrive with steps 4 and 12.
- `decide.backend` and `decide.backend.<purpose>` are read but nothing sets them.

**Findings that change later steps**

1. **`replay` would have fed `decision` rows to the model.** `agent/log.rs`
   replays every unknown row kind as a message on resume. `decision` is now on
   its skip list, with a test. Any later new row kind (`escalation`, `label`) must
   be added there too.
2. **The 0.6 confidence gate is strict.** Confidence is entropy-based. For three
   options, 0.6 needs a top probability near 0.93. Clef-flash gave `global`
   0.60, `topical` 0.22, `unsure` 0.18 for "Always give distances in kilometres"
   (confidence 0.14), so that fact would stay unclassified. That is the rule
   working as written, but expect many `unsure` outcomes. `CTL-4` should look at
   where 0.6 sits before the thresholds are tuned.
3. **The server's own `confidence` is a different number** (0.96 where the
   probabilities give 0.875). It is ignored, as the plan says.
4. **A decision model on the card competes with the image engine and the chat
   model for 6 GB cards**, which is a common size. Step 12 (cloud Clef-flash at
   $0.02/M) is more important than the plan's order suggests for such machines.
5. **A server that runs a decision model forces `n_batch = n_ubatch = 512`** and
   rejects a longer state with HTTP 500. `--batch-size` and `--ubatch-size` on the
   command line override it. Both are set to 16,384.
6. `tauri`'s `test` feature is now a dev-dependency, so `ask` can run on a mock
   app. It changes nothing in release builds.

**Not caused by this step**

- `src/components/Icons/icons.test.ts` fails on `TopBar/WindowControls.tsx`
  (an inline glyph outside the Icons module). That file is not touched here.
- Other uncommitted work in the tree that is not from this step: `index.html`,
  `website/`, `public/brand/`, `src-tauri/icons/`, `routes/About.tsx`,
  `routes/Settings.css`. The CLI_ENGINES fixes from step 0 are still uncommitted.

### Order step 3 - the Experiments framework, Workspace mode first: DONE (2026-10-23)

**Built**

| Item | Where | What it does |
|---|---|---|
| `EXP-1` registry | `src-tauri/src/experiments.rs` | `Experiment`, `Stage`, `Requirement` (`DecisionModel`, `Vision`, `Provider`, `Experiment`), `REGISTRY`, `is_on`, `set`, `list`, `mark_seen`, `in_workspace`, `migrate`. State is `experiment.<id>` = `on`/`off`. |
| Mirror | `commands/experiments.rs`, `lib/api.ts`, store | `experiments_list_cmd`, `set_experiment_cmd`, `experiments_seen_cmd`. The store holds the list the backend returns; there is no second list in TypeScript. |
| `EXP-2` | registry | **Only `workspace` is registered.** See "Decided" below. |
| `EXP-3` backend | `agent/commands.rs`, `agent/run.rs`, `agent/context.rs`, `agent/present.rs` | The `switch_mode` spec lists `workspace` only when on (`modes_offered`); `decide` refuses it when off; the chat's Workspace flag counts only while on (`experiments::in_workspace`), so `render_ui` and the canvas guidance are withdrawn from an old Workspace chat. |
| `EXP-3` frontend | `lib/store.ts`, `lib/commands.ts`, `SessionMenu`, `Chat`, `WorkspaceOffBanner` | `setWorkspaceMode(true)` is refused while off (every way in goes through it); `/workspace` answers "Workspace is an experiment" with an **Open Experiments** button; the row's switch says where to go; the session-menu toggle is hidden; a skill with a surface template starts as a plain chat; a Workspace chat opens in the normal layout with a banner "This was a Workspace session. [Turn Workspace on]". Off is never refused. |
| `EXP-3` migration | `experiments::migrate`, run at startup | Once: `workspace` on if any chat has the flag, else off. An explicit choice is never overwritten. |
| `EXP-UI-1` | `routes/Experiments.tsx`, `SettingsHub`, `types.ts`, `Icons.tsx` | New hub section between Usage and Working dir with a flask icon and a dot while an experiment is unseen (cleared on visit). Each card: title, stage tag, switch, summary, what changes, needs (with a "Set up" link), records, graduation gate. |

**Decided (not in the plan text)**

- **Only the experiments that exist are registered.** `fast-slow`, `verify`,
  `mail-triage` and `relax-prompts` were not in `REGISTRY` yet (`fast-slow` came in
  step 5, `mail-triage` in step 10). A card with a switch
  that changes nothing is a lie, and nothing reads those switches until steps 5 to
  11. Each of those steps adds its entry in the same change as its behaviour. The
  requirement kinds they need are built and tested now, with a fixture registry.
- **Turning off cascades.** Turning off an experiment turns off the ones that
  require it (`relax-prompts` needs `fast-slow`). The backend also refuses to turn
  one on while a requirement is missing, because the card can be stale.
- **An old Workspace chat with the experiment off is a normal chat for the
  agent too**, not only on screen. The plan only says it opens in the normal view;
  leaving `render_ui` on would have the model draw a canvas nobody can see.
- **Before the list is read, Workspace counts as available** (`workspaceAvailable`).
  The list is read before the first chat opens, so this is only the browser-tab
  case. Hiding a layout someone is in, because a read has not finished, is worse.

**Verified**

- `cargo test --lib`: **1134 pass, 0 fail**, 15 ignored (full run, after this step).
  New: 11 in `experiments` (registry, requirements, cascade, `new` dot, migration,
  `in_workspace`), 2 in `agent::commands` (spec offers `workspace` only when on;
  a switch is refused while off). Two existing tests were changed to turn the
  experiment on first (`agent::commands`, `agent::context`), because they assume a
  Workspace chat is one.
- Frontend: 22 new tests (`experiments.test.ts` 5, `Experiments.test.tsx` 17).
  `tsc --noEmit` clean. Full `vitest`: 978 of 979 passed. The one failure was the
  hub routing test, whose hand-written label map lacked the new tab; fixed, and
  that file re-run alone is green (not a second full run). The `icons.test.ts`
  failure noted under step 2 did not occur in this run; `WindowControls.tsx` is
  not touched by this work, so it is not known why.
- `EXP-T-1`: the registry mirrors (one command, list in the store); a gated spec
  value is absent when off (`switch_mode` has no `workspace`); `/workspace` when
  off gives the pointer; the migration turns `workspace` on only when a Workspace
  chat exists.

**Not verified, and why**

- **The screen was not opened in the running app.** Covered by typecheck and
  jsdom tests. Layout, spacing and the flask icon have not been seen by eye.
- **The migration on a real database** was tested on in-memory ones only.
- **`EXP-T-2`** (download, first-on note, reset confirm, Runtime and Models states
  for experiments): belongs to the first decision experiment, step 5.

**Not built from the plan text (decided, not forgotten)**

- **`EXP-4` reports and "Reset its data"**, the "Details" link and the "Last 30
  days" line. Workspace has no data of its own to report, and there is no outcome
  table until `CTL-3`. Arrives with step 4.
- **Inline requirement download and the first-on "I'll be careful" note.** No
  registered experiment needs a decision model yet.
- **Rail, `WorkspaceSwitch` and the mode chip are unchanged**, as the plan says
  ("Mode chip: unaffected"). They only show when the mode is already on, and with
  the experiment off the mode is off.
- **The `/workspace` command stays in the `/` menu** while off, as the plan says
  it answers with a pointer.

**Findings that change later steps**

1. **Adding an experiment is three edits:** an entry in `REGISTRY`, a gate where
   its behaviour starts (`experiments::is_on`), and a line in the hub routing test
   only if it is a new view. Steps 5 to 11 should follow `in_workspace` and
   `modes_offered`: one helper per gate, used at every site, so the frontend and
   backend cannot disagree.
2. **`Requirement::Provider` calls `cloud::has_key`**, which reads the OS keyring.
   It is built but not tested against a real key. First user: `verify` (step 11).
3. **The frontend still assembles the system prompt** for `agent_chat_cmd`
   (`composeSystemPrompt`), so a gate that changes the prompt must be made on both
   sides. `chatForPrompt` in `store.ts` is the frontend half for Workspace.

### Order step 4 - policy, budgets, outcomes, calibration, visibility: DONE (2026-10-23)

Built for the five purposes that have a site (`RetSufficiency`, `MemScope`,
`ReflectCritic`, `GoalMet`, `HandoverRoute`). The plan's other rows arrive with
the step that builds their site, because a row with no site is a threshold
nothing reads. Their values stay in the `CTL-1` table above.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| `CTL-1` policy table | `agent/decide/policy.rs` | `Mode`, `Impact`, `Threshold { measure, calibrated, uncalibrated }`, `Escalation`, `NeedsVision`, `experiment`, plus `Purpose::below` for the screen. A threshold names the number it reads (`PTrue`, `TopProb`, `Confidence`). The sites no longer hold their own constants (`SCOPE_MIN_CONFIDENCE`, `ROUTE_MIN_CONFIDENCE` and `goal::MET_AT` are gone); they read the row. |
| `CTL-2` controller | `agent/decide/control.rs` | `evaluate` (a closed gate beats every bar), `Controller` with the per-turn counters (20 decisions, 3 escalations, no repeat of the same purpose on the same state hash) and the background budget (200 an hour), `settle` (writes `decisions.action`). `ask` calls `admit` once the engine is known to be there. A hit returns `DecideError::Budget(limit)`, which every site already treats like `Unavailable`, and writes a `decisions` row with action `budget:<limit>`. |
| `CTL-3` labels | schema **v37**: `decisions`, `decision_outcomes` (`db/decisions.rs`); `agent/decide/outcomes.rs` | Every decision is now stored, with or without a conversation (this closes the step 2 gap for the rule preview and the memory backfill). Read from existing tables: `HandoverRoute` joins `engine_signals` `override` and `asked`. Written new: `record_goal_outcome_cmd` for the goal (`stalled` or `exhausted` after a confident "not yet" is a weak right; a new goal within 10 minutes of a "met" is a weak wrong). The frontend sends the event from `goal.ts` (`GoalHost.outcome`) and `startGoal`. |
| `CTL-4` calibration | `agent/decide/calibration.rs`, `seed.rs`, `tests/eval/decide/*.jsonl` | One temperature per (model file stem, purpose), fitted by NLL on the seed run plus labels (weak labels weigh 0.3). Stored as `decide.calibration.<model>.<purpose>`. Calibrated means `ece <= 0.08` and `n >= 40`. `ask` rescales the answer by a good fit and sets `Verdict.calibrated`; the log keeps the **raw** probabilities, so a refit never fits on its own output. "Calibrate now" runs the seed sets through the engine. A background check (first 10 minutes after start, then every 6 hours) refits a purpose that has a fit, a week of age and 50 new labels. |
| `CTL-UI-1` | `routes/Activity.tsx`, `lib/decisions.ts`, `Activity.css` | One collapsed line per decision (`Decided · <question> · yes 0.93, no 0.07 · 41 ms · this computer`). Expanding shows the state preview, a bar per option, what the controller did, whether it is calibrated, and the labels. Background decisions fold into one line per hour and purpose. Filter chips: Everything, Actions, Decisions, and one per experiment that owns a decision (none yet). |
| `DEC-UI-2` (the parts left in step 2) | `components/DecisionModel/DecisionModel.tsx` | "Calibrate now" with progress, a line per purpose, a summary line, and the Advanced table now shows the bar (with the uncalibrated one) and what happens below it. |

**Verified**

- `cargo test --lib`: **1192 pass, 0 fail**, 15 ignored (the step 2 flake did not fail this time).
- New backend tests: `db::decisions` (5), `agent::decide::policy` (+4),
  `control` (14), `calibration` (12), `outcomes` (13), `seed` (8), `agent::decide`
  (+2).
- `vitest`: 1007 of 1007 (new: `decisions.test.ts` 14, `Activity.test.tsx` 6,
  `DecisionModel.test.ts` +7, `goal.test.ts` +2). `tsc --noEmit` is clean.
- `CTL-T-1`: a closed gate beats every row; an uncalibrated bar is never lower
  than the calibrated one; the budget hit takes the default and logs
  `budget:escalations`; an unchanged state hash blocks a re-ask.
- `CTL-T-2`: an `engine_signals` override after a matched rule joins as `wrong`
  and nothing is copied into `decision_outcomes`; ECE on a known set (stated
  0.9, right 60 of 100, gives 0.3); weak labels pull the fit less; a child that
  asks with its lead's conversation id spends the lead's turn budget.
- The seed sets: 40 rows each, every row parses, every label is one of the
  question's options, no set has an answer above 60 percent, every row builds a
  request the server accepts.

**Not verified, and why**

- **Nothing was run on a real decision model.** The Clef-flash files from step 2
  are no longer on this computer (6.5 GB). The new opt-in test
  `cargo test --test decide -- --ignored seed_sets` measures the real model on
  the seed sets and prints accuracy, how often each bar is reached, and the fit.
  It compiles and has not run. **So no ECE number for Clef-flash exists yet**, and
  the question from step 2 (is 0.6 on the entropy confidence too strict?) is
  still open.
- **The screens were not opened in the app.** Activity and the Runtime card are
  covered by component tests with a mocked backend. "Calibrate now" end to end,
  and the weekly refit task, were not driven.
- **The budget path inside `ask`** (the code between the cold check and the
  request) is only covered by the controller's own tests and the opt-in end to
  end test, because it sits behind a running engine.
- **The seed sets were written by me, this step, and nobody has reviewed them.**
  Some labels are judgement calls (`unsure` notes for `MemScope`; `unsupported`
  against `unactionable` for the critic). A model that does badly on them may be
  right and the label wrong. They should be read before the fit is trusted.

**Behaviour changes**

- **The critic can now abstain.** `ReflectCritic` is `TopProb >= 0.5`. Below it
  the old language-model critic runs, as before the decision engine existed.
  Before this step the winning option always decided. The other four rows keep
  the values they had.
- **Every site reports what it did** (`acted`, `default`, `escalated:slow`).
  `RetSufficiency` and `GoalMet` are always `acted`: a probability on either side
  of their bar is an answer, not a doubt.
- **A turn is a conversation's turn.** `begin_turn` runs at the start of
  `agent_chat_cmd`, so a goal check after the turn counts against that
  conversation's counters until the next turn. The rule preview has no
  conversation, so it has no turn budget.

**Not built from the plan text (decided, not forgotten)**

- **`Evidence` and `User` escalations.** No row uses them yet. The three rows
  that can fall below their bar escalate to `Slow`, which is the old path.
  They arrive with `USE-5`, `USE-9`, `USE-10`. *(`USE-9` did not need them: a
  row below its bar is marked, not escalated. `USE-5` did not either: its rows
  take their default, and "probably done" is in the nudge and the ring.)*
- **Labels with no purpose yet** (`ToolSelect`, `PlanDone`, `MailTriage`,
  `SteerClass`, `ClaimVerdict`, `BrowserCheck`, `ToolRisk`, `AppKind`; *`ToolSelect`
  got its labels in step 5, `PlanDone` and `SteerClass` in step 9*), and the
  stricter-`tool_overrides` join. And the handover card's *Hand it over* answer:
  `engine_rules.hits` is a counter, so a per-decision label needs an event row,
  which step 6 should write when it makes pre-match decisions.
- **`UntrustedFlag`'s half share of the background budget.** The purpose does not exist.
  *(Built in step 7.)*
- **The run timeline** (`CTL-UI-1` says Activity and the run timeline). Only
  Activity has decision rows. The timeline is fed by agent events, and carrying
  a decision through them is its own change.
- **`CTL-UI-2`** (the Usage "Decisions" section) is not in this step's list.
- **The borrowed fit for cloud models.** Step 12.
- **Queueing a refit behind running chat turns.** A refit is arithmetic on stored
  rows, with no engine or card, so there is nothing to queue behind.

**Findings that change later steps**

1. **Nothing yet writes a decision for the hot sites on a processor.** The three
   hot purposes are skipped on this computer (step 2), so on a GTX 1060 the
   `decisions` table fills only with the two background purposes. Calibration
   for the hot three needs a card, or the opt-in test above run by hand.
2. **`Confidence` is the strict measure and `TopProb` is not.** `MemScope` and
   `HandoverRoute` use `Confidence` at 0.6 (unchanged). Moving a row to `TopProb`
   is one line in `policy.rs`; do it after the seed run says which measure the
   model's real answers support.
3. **`settle` must be called by every new site.** A site that forgets leaves
   `action` empty and its decisions never escalate or count. Steps 5 to 11 each
   add one call.
4. **Deleting a conversation deletes its decisions**, and their labels, because
   the preview of the state can hold what the user wrote. Fits and the seed run
   stay, since they hold no user text.
5. **`copy_lint` fails on a file this step did not touch**:
   `src/lib/voice/playbackQueue.test.ts:45` (the word "chunk" in a test name,
   from commit `3729b7c`).

### Order step 5 - the tool menu (`USE-4`), `fast-slow`'s first feature: DONE (2026-10-23)

The first purpose that changes what a turn sends, and the first feature behind an
experiment. With **Fast & slow thinking** on and a warm engine on the graphics
card, the first request of a user turn can leave tool groups off the menu when the
decision model is sure the message does not need them. Nothing is lost: the model
can ask for a group back, and so can the user.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| `fast-slow` experiment | `experiments.rs` | First decision experiment. Off by default, needs a decision model. Its card lists only what exists now ("I may hold back tools a message clearly doesn't need"). |
| `ToolSelect` row | `agent/decide/policy.rs` | Assisted, visible, hot (graphics card only), 300 ms deadline, `experiment: fast-slow`. New measure `PFalse` (the probability a statement is false), because this row acts by dropping: drop when sure it is **not** needed, 0.85 calibrated, 0.95 before (the plan's "drop under 0.15 / 0.05"). The uncalibrated bar stays the stricter one. |
| The experiment gate | `agent/decide/mod.rs` `ask_with` | A purpose whose row names an experiment is not asked while it is off. Until now no row named one. |
| `USE-4` units, question, answer | `agent/decide/tools.rs` | A unit is an optional toolset, Composio's meta-tools as **Connected apps** (named by the granted apps), one **MCP connector** (named by its first five tool names) or **Other agents** (`delegate` and its two companions, named by the agent types). One yes-or-no question per unit, in one request, over the user's message and the last reply (2k characters each). `pick` drops a unit on a confident no. |
| Applied in the run | `agent/run.rs` `narrow_menu`, `assemble` | After prompt assembly and every deterministic narrowing (persona, `Toolset::ready`, grants, `/external off`, plan-first). The registry is untouched; `assemble` filters the specs it sends, so dispatch and the content-form parser still know every tool. One `decide::settle` call per turn (`acted` when something was held, `default` when not). |
| Never narrowed | `tools::may_narrow`, `tools::forced` | A delegated child, a resumed run, a scheduled task, a run with tools off. `/delegate <agent>` keeps Other agents. A sign-in card in the chat (any phase but given up, for 30 minutes) keeps Connected apps. File access, Recall, Memory, Skills and Workspace UI are never units. |
| `need_tools` | `tools::need_tools_spec`, `bring_back`, `TurnCtx::need_tools` | Offered only while something is held, and its `units` enum lists only what is held. It can only bring back what was held. A group that was never held gets the reason Settings, Tools gives (`ToolsetInfo.needs`, or "switched off"), so it cannot route around a switch, an account or a grant. It is bookkeeping: a turn of nothing else is a free step. A direct call to a held tool (a model that remembers it from earlier in the chat) brings its group back and runs, every gate below still applying. |
| `USE-UI-1` run-bar chip | `RunBar.tsx` (`heldSegment`), `store.ts`, `AgentEvent::ToolsHeld` / `ToolsBack`, `restore_tools_cmd` | `3 tools held back`, muted, in the run bar. Hover lists the groups with the probabilities (`Web search 0.03 · Connected apps 0.04`). Click while the run is going puts them back from the next step. The chip counts what is still out and goes when nothing is. After the run it stays as a record, with no click. A group that was never offered is not in it. |
| Labels (`CTL-3`) | `agent/decide/outcomes.rs`, `TurnCtx::report_held`, the run's `end` | Per held group, with the group in `value`: `wrong` when it came back (the model asked, called it, or the user clicked; sources `need-tools`, `called-held`, `chip`), `weak_right` when the run completed and it never did (`kept-out`). Calibration turns each into one observation of that group's answer. |
| Seed set | `tests/eval/decide/tool-select.jsonl`, `seed.rs` | 40 rows, one group each, half yes. "Calibrate now" and the weekly refit pick up the sixth purpose with no other change. |
| `EXP-UI-1` pieces step 3 left for here | `routes/Experiments.tsx` | When a deciding experiment is on and its own questions are not calibrated: "I'll be careful until I've measured how sure my quick decisions are on your setup", with **Calibrate now** (progress inline) and **Later**. A **Show them in Activity** link opens Activity already narrowed to the experiment (`activityFilter` in the store, used once). |
| Activity wording | `lib/decisions.ts` | A decision with one question per group reads group by group (`Other agents 0.02, Web search 0.03 ...`), and its labels read `Needed after all: Web search` and `Probably not needed: Other agents`. |
| `CTL-T-3` ablation | `agent/decide/ablation.rs`, `tests/eval/decide/tool-menu.jsonl`, `tests/decide.rs` | 30 golden requests with the groups a person would say they need, in three slices (base, three granted apps, two agent apps). Scores keep-everything, a fixed 0.30 cut-off and the policy table, and the ship rule: the policy table must not miss a larger share of needed groups than the cut-off, and `need_tools` must bring back every group that was held and needed. The scoring is unit-tested; the numbers need the real model (opt-in test). |

**Verified**

- `cargo test --lib`: **1227 pass, 0 fail**, 15 ignored (was 1192). New: `tools` (15),
  `ablation` (10), `policy` (+2), `outcomes` (+2), `experiments` (+1), `run` (+5).
  `cargo test --test decide` compiles; it was not run.
- `vitest`: **1031 of 1031** (was 1007). New: `RunBar.held` (8),
  `store.toolsheld` (5), `Experiments.decides` (6), `decisions` (+4),
  `Activity` (+1). `tsc --noEmit` is clean. `copy_lint` fails only on the
  `playbackQueue.test.ts` line from step 4.
- `USE-T-1`: the groups are read from the real tool table after every narrowing (mail
  with no account is never a group; a run with no room to delegate has no Other
  agents; Connected apps and each connector are groups; the agent names come from
  the delegate spec); children, resumed runs, scheduled runs and a run with no
  tools are never narrowed; `/delegate` and a waiting sign-in card force their
  group onto the menu; `need_tools` cannot add a group that was never held and
  gives the Settings reason for one that is off; a held tool that is called comes
  back.
- `CTL-T-1` for this row: a unit goes only on a confident no (0.03 yes, 0.10 only
  with a fit, 0.5 never); no answer keeps the unit; nothing held settles as `default`.
- `CTL-T-2` for this row: a miss is a firm "needed" observation at that group's
  probability, a run that never needed it is a weak "not needed", a label with no
  known group is no observation.
- `CTL-T-3`, the scoring: a perfect model loses nothing and saves more than 10,000
  tokens over the golden set; an unsure model gets nothing held by the policy where
  a loose fixed cut-off holds everything and misses needed groups; a confidently
  wrong model is a miss that `need_tools` still recovers; the ship rule fails a
  policy that misses more than the cut-off.

**Not verified, and why**

- **Nothing ran on a real decision model**, still. So there is no measured miss rate,
  no token or first-token saving, and no number for how often a 9B-class chat model
  calls `need_tools` instead of answering badly. `cargo test --test decide --
  --ignored tool_menu_ablation` does the first two (not first-token time). It
  compiles and has not run.
- **`narrow_menu` itself** (the ask, the held state, the first request) is not driven
  end to end. Its parts are tested; it sits behind a running engine.
- **No run was watched in the app.** The chip is covered by component and store tests
  with a mocked backend. Layout, the hover text and the click have not been seen.
- **The golden set and the seed set were written by me** and have not been reviewed.
  Some "needs" are judgement calls (does "Fix the typo in the first line of the
  README" need Other agents: no).

**Behaviour changes**

- With **Fast & slow thinking** on, a fresh user turn may send fewer tools, and a
  run that held something has one extra tool, `need_tools`.
- `ask` refuses a purpose whose experiment is off (new; only `ToolSelect` has one).
- "Calibrate now" measures 40 more rows (a sixth purpose). On a processor-only
  machine that is several more minutes, for a purpose that machine never asks.
- The plan said File System, Plan and Recall are always kept. Memory, Skills and
  Workspace UI are also never held: the skill list is named in the system prompt and
  the Workspace canvas is a mode, so taking those away would contradict the prompt.

**Not built from the plan text (decided, not forgotten)**

- **The skill `Choice` and "likely relevant" ordering.** Skills are listed in the
  system prompt, which the frontend still assembles (`composeSystemPrompt`) behind the
  byte-identical gate. Moving a skill to the top changes that prompt, so it needs the
  same change on both sides. Skills are never hidden, as the plan says.
- **The 800 ms cloud deadline.** There is no cloud backend until step 12.
- **The chip is not saved.** It is a live run-summary field, so after a reload the
  chip is gone; the decision, its numbers and its labels stay in Activity.
- **`EXP-4` reports, "Reset its data" and the inline requirement download**
  (`EXP-T-2`). Reports are step 15. The download needs the Models flow in the card.
- **The run timeline.** The held list is on the run bar and in Activity, not as a
  timeline row.

**Findings that change later steps**

1. **The first turn after a cold start is not narrowed.** `prewarm` starts the engine
   as the turn starts, so the first ask finds it cold, skips, and the menu is whole
   (decision 6). In practice the chip appears from the second turn. Step 6 has the
   same exposure.
2. **The labels see only the held groups.** A group that stays on the menu is never
   labelled, so a refit sees the low-probability tail from behaviour and the whole
   range only from the seed set. A fit on that mix can look better than it is in the
   middle. Worth a second look at the 50-label refit rule once real labels exist.
3. **The seed asks one group at a time; a run asks all of them in one request.** The
   model reads them together, so the real answers may differ from the seed's. The
   ablation test asks them together and is the better measure; the fit should be
   checked against it before it is trusted.
4. **A decision's question ids are the group ids**, so the log holds `mcp:<id>` for a
   connector, which Activity shows as "A connector". The group's own name is not
   stored. If that matters, store the label next to the answer when step 6 adds a
   second multi-question purpose.
5. **`need_tools` must be classified for plan-first and read-only runs only if it is
   ever listed in `classify_mutation`.** It is not a toolset tool, so it is not today;
   `refuses_in_plan_first` never sees it. Keep it out of `classify_mutation`.

### Order step 6 - the handover pre-match (`USE-13`): DONE (2026-10-23)

The second thing behind **Fast & slow thinking**. Before the lead's first request of a
user turn, the decision model reads the message against the user's own handover rules.
When it is sure which rule fits, an app that asks first gets its handover card now,
before the lead has written a word. Nothing starts without the user, and the lead still
decides what an app does.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| The question | `decide/handover.rs` `ask_for`, `sites::route_after` | The same options as **Try a request** (every enabled "use it for" rule of the agents on offer, plus `none` and `unsure`, the "don't use it for" rules as lines in the question). The state is the message, then the assistant's last reply (2k characters each). Same purpose, so one calibration. |
| The live row | `Purpose::live_policy`, `decide::ask_live` | 300 ms, behind `fast-slow`, and a card needs a **calibrated** answer whose top probability is 0.85 or more. The preview keeps its own row (8 s, no experiment), so it is still a pure swap. |
| When it is asked | `run.rs` `prematch_turn`, before `narrow_menu` | Only when an agent app is really on this run's menu after the guards, the mode, `/external off` and `max_per_conversation`; the user did not name one with `/delegate`; not a child, a resumed run, a scheduled run or a run with no tools. Otherwise no ask at all. |
| The card | `subagents.rs` `act_on_prematch`, `ask_handover` | `suggest` mode and a rule the user has not said "always" to: the existing handover card, before the lead runs. Its rule line reads `Matches your rule U2 · quick decision (0.93): <rule>`. The wait for the answer is shared with the lead's own card, so **Stop** withdraws it the same way (the CLI_ENGINES review fix 3 path). |
| *Hand it over* / *Always for this* | same | The lead gets one system line: the user agreed, call `delegate` for agent X with `because` U2, and write the task so it can be done without seeing this chat. *Always for this* sets `auto` on that one rule, as RTE-4 does. Other agents stays on the tool menu even if `USE-4` would have held it (`narrow_menu` takes the flag). |
| *Do it yourself* | same | Counted exactly as RTE-4's card counts it (`overrides`, the `override` signal, the third-no proposal), and the lead is told to do it itself. |
| `auto` mode, or a rule with a standing yes | same | No card. The lead gets *"This looks like the user's rule U2 for Claude Code (0.91). You still decide."* |
| No second card | `Prematch::agreed_to`, `prepare_engine` | When the lead then calls `delegate` citing the rule the user just said yes to, that call does not ask again. A different rule still asks. |
| Check on the lead | `Prematch::doubt`, `RuleRef.doubt` | When the lead cites a rule the pre-match scored below 0.1 (calibrated only), the card gets a second line: `I'm not sure this matches U2 (0.06).` In `auto` mode it is written to the activity log only. Never a refusal. |
| Labels (`CTL-3`) | `decision_outcomes`, source `handover-card` | *Hand it over* and *Always for this* write `right`, *Do it yourself* writes `wrong`, with the rule (`claude-code:U2`) in `value`. Stop writes nothing. The two read-from-existing joins (`override`, `asked`) are unchanged and do not double count (calibration keeps the strongest label per decision). |
| `USE-UI-10` Fleet row | `EngineChips.tsx`, `EngineBadge.matched` | A small dot on the `because U2` chip when the rule came from the pre-match; hover says *matched before I started (0.93)*, then the rule's words. |
| `USE-UI-10` Try a request | `Preview.quick_ms`, `RuleList.tsx` | The answer ends with `quick decision · 41 ms` or `asked my chat model`. |
| `USE-UI-10` When I use it | `engines_prematch_counts_cmd`, `RuleList.tsx` | A muted counter per rule when it has data: `matched early 12× · you said do it yourself 1×`. Read from the new labels, not from `engine_rules.hits`. |
| `CTL-T-3` handover slice | `tests/eval/decide/handover-prematch.jsonl`, `handover::score`, `ship`, `tests/decide.rs` | 67 requests labelled with the rule a person would pick from the default Claude Code rules or `none` (some with the last reply). Scores the early card at 0.85; the ship rule is precision of at least 0.95. The real-model test is opt-in and has not run. |

**Verified**

- `cargo test --lib`: **1251 pass, 1 fail**, 15 ignored (was 1227). The one failure is
  `db::tests::a_spoken_turn_stays_marked_and_a_fork_keeps_the_mark`. It is not from this
  step: a fork copies messages with `created_at < cutoff`, so two messages written in
  the same millisecond lose the first. It passed in one full run and failed in the next
  with no change to that code. Left alone. New: `handover` (15), `prematch_tests` (9),
  `db::decisions` (+1). `cargo test --test decide` compiles; it was not run.
- `vitest`: **1042 of 1042** (was 1031). New: `engines` (+3), `PermissionPanel.engine`
  (+3), `FleetCard.engine` (+2), `RuleList` (+3). `tsc --noEmit` is clean.
- `USE-T-4`: suggest mode with a calibrated 0.91 on a `use` rule raises the card before
  the lead, with the rule and the number; *Hand it over* writes `right` and tells the lead
  to call `delegate`; *Do it yourself* counts the same as RTE-4 and writes `wrong`;
  *Always for this* relaxes that rule only; `auto` mode and a standing yes give a line and
  no card; uncalibrated, below 0.85, `none`, `unsure`, a rule that does not exist or is
  switched off, and an agent that is not on offer change nothing; Stop withdraws the card
  and nothing is learned or labelled; the lead citing the rule the user agreed to is not
  asked again and another rule still is; a low calibrated score is doubted and an
  uncalibrated one is not.
- The `CTL-T-3` handover slice, the scoring: a perfect model has precision and recall of
  1; an unsure or wavering one raises no card and passes with no recall; a confident wrong
  rule fails the ship rule; 19 of 20 passes and 18 of 20 fails. The golden set parses, has
  no repeated id, names only real options, and has at least six cases for each rule and
  for `none`.

**Not verified, and why**

- **Nothing ran on a real decision model**, still. So there is no precision number for
  the early card, no measured time against the 300 ms deadline, and no idea how often a
  9B-class model picks the right one of four rules. `cargo test --test decide --
  --ignored --nocapture handover_prematch` measures the first on the model's own
  probabilities. It compiles and has not run.
- **`prematch_turn` is not driven end to end** (the ask, then the card, then the lead).
  Its parts are tested: the question, the pick, the card and its answers.
- **`prepare_engine`'s three new lines are not driven**: skipping the second card, the
  doubt line and the badge. They need a full `ToolContext`. The decision they make
  (`handover::against`) is tested.
- **No run was watched in the app.** The card line, the dot, the counters and the Try a
  request line are covered by component tests with a mocked backend.
- **I wrote the golden set myself** and it is unreviewed. Some labels are judgement calls
  ("Run the formatter and the build, then fix any warnings" is `U2`).

**Behaviour changes**

- With **Fast & slow thinking** on and a calibrated model, a message that matches a rule
  at 0.85 or more raises the handover card before the lead runs, or gives the lead one
  system line. With the model uncalibrated or the experiment off, nothing changes.
- A pre-match and the tool menu are two asks in one turn (two purposes, two questions).
  Each has its own deadline.
- `RuleRef` is no longer `Eq` (it carries a float). `narrow_menu` takes one more argument.

**Not built from the plan text (decided, not forgotten)**

- **The pre-match does not start the child itself.** The plan says *Hand it over* starts
  the child with the rule as `because`. An agent app cannot see the chat, and the lead is
  the one that writes a brief; a raw user message such as "do the same in the other
  module" would fail. So the user's yes is passed to the lead, which writes the task, and
  `delegate` does not ask again. The cost is that the lead has to follow the line. If a
  real model often does not, start the child with the message as a fallback.
- **The "avoid rule at 0.85" line.** The plan puts avoid rules among the options. The
  preview's question has only `use` rules as options, with the avoid rules as text, and
  adding options would split the calibration from its seed set. `delegate` already
  refuses a `because` that cites an avoid rule, with the user's own words.
- **"Uncalibrated: hint only"** (the policy table) against **"uncalibrated changes
  nothing"** (`USE-13` and `USE-T-4`). The stricter reading is built.
- **The 800 ms cloud deadline.** No cloud backend until step 12.
- **The Fleet dot is live only.** It is not saved with the job, so after a reload the
  chip stays and the dot goes. The labels and counters are saved.
- **The Policy table in Settings shows one `HandoverRoute` row** (the preview's), not the
  live row.
- **Pre-matching for scheduled tasks**, deferred by the plan.

**Findings that change later steps**

1. **The first turn after a cold start has no pre-match**, the same exposure as the tool
   menu: the first ask finds the engine cold, starts it and skips.
2. **Two round trips before the first token.** With both purposes asking, a warm engine
   may add up to two deadlines. The decision model reads several questions of one request
   together, so a merged request is possible, but `ask` takes one purpose at a time. Look
   at the real times before building it.
3. **The seed set and the live question differ.** The handover seed offers three rules of
   two agents and one avoid line; a live turn offers up to four rules per agent. The fit
   is taken from the seed, so check it against the new slice before trusting it.
4. **The lead has to cooperate after a yes.** If it does not call `delegate`, the user
   said yes and nothing was handed over. The run ends with the lead's own answer.
5. **Labels exist only for matches that raised a card.** A request the pre-match called
   `none` or `unsure` gets no label unless the user then names an agent (`asked`), so the
   refit sees the confident tail again (step 5, finding 2).
6. **A test in `db/mod.rs` is timing dependent** (see Verified). Worth a one-line fix
   (`<=` with an id tie-break) by whoever owns that file.

### Order step 7 - the two background filters (`USE-8`): DONE (2026-10-23)

The third and fourth things behind **Fast & slow thinking**. Both run in the background on
this computer, and both only add to what already happens.

- **The reflection gate.** After a conversation ends, a model reads it to draft lessons.
  When nothing was corrected and nothing failed, that pass finds nothing and still costs a
  model call. The gate asks one yes-or-no question first. On a confident "nothing to learn"
  it skips the drafting pass and writes `nothing to learn` to Activity.
- **The outside-text flag.** Outside text is already scanned for known phrases. The flag
  asks the decision model whether the text tries to instruct an AI assistant, which catches
  what the phrases miss. It adds a flag and never blocks, as the scan does.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| Two purposes | `decide/policy.rs` `Purpose::ReflectGate`, `UntrustedFlag` | Both background (so local only, decision 12), not hot, behind `fast-slow`. `ReflectGate` skips at p(something to learn) of 0.3 or less calibrated, 0.15 or less before. `UntrustedFlag` flags at 0.7 on both bars (the plan gives one value). 30 s deadline, 6 000 token state cap. |
| Reflection gate | `decide/filters.rs`, `commands/reflect.rs` | Asked only when the conversation has **no hard evidence**: no tool failure, no self-corrected mistake, no user signal, no rewind (`has_evidence`). With any of them reflection runs, as before. The question reads the last 6 000 characters of the transcript. A skip logs `nothing to learn`, writes the decision row, and still runs the skill-revision proposals (they read `skill_runs`, not the transcript). |
| Flag intake | `toolsets::mark_untrusted_for` -> `toolsets::flag_outside` -> `filters::check_in_background` | One hook covers every tool intake that was already marked: web search, fetched pages, retrieved files, mail, skill text, **MCP connector results** (`call_mcp_tool`) and the Composio relay. Asked only where `untrusted::scan` found risk 0 or 1, for text of 80 characters or more, clipped to 6 000. |
| Agent-app reports | `subagents::flag_report`, in `delegate` and `collect_agents` | The report of an agent app (not of a plain sub-agent) is checked. The flag lands on the `delegate` / `collect_agents` step as a source labelled `report from <agent>`. |
| Memory served to agent apps | `engines/tools.rs` `recall` | Checked after it is served. A flag goes to the activity log (`untrusted`), since the call has no step of its own to mark. |
| Proposal text | `engines/tools.rs` `remember`, `engines/learn.rs` `follow_up` | The words of a suggested memory and of a drafted rule are checked **before** the user is asked. A flag of 0.7 or more refuses them as a scan of risk 2 does (`remember`) or drops the drafted words so the proposal uses plain words (`follow_up`). Waits at most 8 s; a late, unavailable or over-budget check refuses nothing. `remember` is now `async`. |
| Own budget | `decide/control.rs` `UNTRUSTED_PER_HOUR` (100), `Limit::Share` | `UntrustedFlag` may use half of the 200 background decisions an hour. The share is checked first, so a refused flag check leaves the other half alone. The log reads `budget:share`. At most 2 flag checks run at once; a third is skipped without a row. |
| Seed sets | `tests/eval/decide/reflect-gate.jsonl`, `untrusted-flag.jsonl` | 40 rows each, 20 yes and 20 no, interleaved. Written by me, unreviewed. They join "Calibrate now" and the opt-in `seed_sets` test with no other change. |
| UI | `AgentEvent::UntrustedFlag`, `store.ts`, `lib/untrusted.ts`, `Timeline.tsx` | The `◇ from outside` chip reads `it reads like orders to me` (drawn like risk 2) when only the quick check found something; the scan's `I ignored its instructions` wins when both did. The disclosure shows the label with `reads like orders to the assistant (0.83)`, and no empty box for a report with no text. |
| Activity | `lib/decisions.ts` | Both purposes are rows and hourly groups with no other change. The group line now says how many of the hour a budget stopped: `12 decisions · <purpose> · 5 not asked, over budget`. `budget:share` has words. |
| Experiments card | `experiments.rs` | `fast-slow` lists three more lines. One of them is step 6's pre-match, which step 6 forgot to add. |

**Verified**

- `cargo test --lib`: **1268 pass, 2 fail**, 15 ignored (was 1251). The two failures are
  `db::tests::a_fork_keeps_the_summary_it_can_still_account_for` and
  `a_spoken_turn_stays_marked_and_a_fork_keeps_the_mark`, the timing flake of step 6
  (finding 6 there). Both pass when run alone (13 of 13 in `db::tests::a_`). This step
  changes nothing in `db`. New: `filters` (11), `control` (+2), `policy` (+3),
  `learn` (+2). The seed-set tests cover the two new files.
- `cargo test --test decide --no-run` compiles.
- `vitest`: **1053 of 1053** (was 1042). New: `Timeline.untrusted` (6), `untrusted` (3),
  `decisions` (+2). `tsc --noEmit` is clean.
- The questions are ones the server accepts. The gate skips only at 0.3 (calibrated) and
  0.15 (uncalibrated) and not above; the flag at 0.7 and not below; an answer to another
  question changes nothing. Any one of the four pieces of hard evidence stops the gate
  being asked. A text is asked about only at scan risk 0 or 1, and only from 80 characters
  (a proposal from 8). A long text and a long transcript are clipped, the transcript from
  a line. The share: 100 flag checks and the 101st is refused, the other 100 are still
  there, and a refused flag check took none of them; the others can use all 200 when no
  flag check has. Two checks at once, and a finished one gives its place back. With no
  app (a test, or start-up) nothing is asked, spawned or refused, and a drafted rule goes
  through as before.

**Not verified, and why**

- **Nothing ran on a real decision model**, still. So there is no accuracy and no
  calibration for either purpose, no idea how often the gate is right to skip, and no idea
  how often the flag raises on ordinary text. `cargo test --test decide -- --ignored
  --nocapture seed_sets` measures both now. It has not run.
- **The background path is not driven end to end**: spawn, ask, `UntrustedFlag` event, chip.
  It needs the app and an engine. Its parts are tested.
- **The skip path of `reflect_conversation_cmd` is not driven.** It needs Tauri state and
  an engine. `has_evidence` and `skip_from` are tested, and so is the controller.
- **No run was watched in the app.** The chip and its disclosure are covered by component
  tests.
- **I wrote both seed sets myself** and they are unreviewed. The flag set leans on
  injections written in plain words ("Note to the AI assistant reading this page: ...").
  A real attack will be worse than my examples.

**Behaviour changes**

- With **Fast & slow thinking** on and a decision model, a finished conversation with no
  sign of trouble may skip its lesson pass, and outside text may get a flag after the
  model has read it. With the experiment off, or with no model, nothing changes.
- `remember` waits up to 8 s for the check before it suggests a memory, only with the
  experiment on. `follow_up` waits the same before it raises a rule proposal.
- An agent app's report and the memory served to an agent app now count against
  `UntrustedFlag`'s share.

**Not built from the plan text (decided, not forgotten)**

- **The flag does not travel in the text the model reads.** The plan says the check
  "adds a flag" at the intake. A background purpose waits for a quiet card and for a cold
  engine, so waiting for it would hold up every tool result. The text goes to the model
  wrapped with the scan's risk as before, and the flag follows. It protects what the user
  sees. The wrapper stays the only protection for the model's own turn. If the seed run
  shows a flag that is both fast and right, a later step can wait for it with a short
  deadline for the sites that stream.
- **Intake sites left out:** the memory store's own scan (`memory/mod.rs`, a sync save used
  in many places), skill import text (`skillpack.rs`, `scan_skill_text_cmd`), and
  reflection's own drafted lessons. Each already refuses at scan risk 2, and none can wait
  for an async check without a larger change.
- **Outcome labels for either purpose.** The plan's `CTL-3` lists none for them, so both
  calibrate from their seed sets only. See finding 2.
- **No ship gate.** The plan has no `USE-T` for `USE-8`. The gate and the flag use the
  general seed-set measure.
- **A flag does not tell the lead.** The model has already read the text. A line in the
  next step would be a new prompt line behind the golden gate.

**Findings that change later steps**

1. **The flag arrives after the text.** See above. Moving it in front of the model is a
   latency decision for the streaming sites, so it needs the seed run's timings first.
2. **Neither purpose will ever refit from use.** A skipped conversation is never looked at
   again, and a flag nobody confirms stays a guess. A cheap label for the gate: when it
   says run and the pass then returns no lesson, that is a weak right for "nothing to
   learn". It costs one line in `reflect_conversation_cmd`. Not built, because the pass
   returning nothing is also what a model with a bad prompt does.
3. **The two filters compete with the chat on this computer.** The engine runs on the
   processor here, at about 12 s a row. A hundred flag checks an hour is twenty minutes
   of processor time. The limit of two at once helps, but `wait_for_quiet_gpu` only waits
   for a card. If the chat feels slower with the experiment on, lower `UNTRUSTED_PER_HOUR`
   first.
4. **A page of many excerpts is mostly not checked.** Retrieval raises one source per
   excerpt, and only two checks run at once, so the rest are skipped without a row. That
   is by design, but the Activity group cannot say how many were dropped.
5. **`collect_agents` checks the same report again each time it is called.** It re-reads
   the stored reports. The share bounds it, but a state hash on the text would stop the
   repeat.
6. **The gate sees only what is in the messages.** `list_messages_window` holds the
   words, not the tool calls, so "a tool failed and was worked around" reaches the gate
   only when the assistant said so. A real tool failure is hard evidence and runs
   reflection without the gate, which is why the gate is asked only without it.

### Order step 8 - the `classify` tool (`USE-9` + `USE-UI-5`): DONE (2026-10-23)

The fifth thing behind **Fast & slow thinking**, and the first the assistant calls
itself. It is for a long list with one closed question about each item: which of
60 files touch auth, which tickets are bugs, which photos show a receipt. The
small model answers each item and the person gets a table of the whole list.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| The tool | `agent/classify.rs`, `Toolset::Decide` (id `decide`, "Sort many items") | `classify { question, options \| scale, items, context }`. Items are text, `{path}` or `{image}`, 1 to 500. Every refusal is in words the assistant can act on ("At most 500 items in one call... call again with the rest"). Read-only for plan-first mode. |
| Offered when | `Toolset::ready` | Fast & slow thinking is on and a decision model file exists. Otherwise it is off the menu (`LRN-5`), and Settings -> Tools says what is missing. It is on the tool menu always (`decide::tools::ALWAYS`), so `USE-4` never weighs it. |
| The purpose | `decide/policy.rs` `Purpose::Classify` | Not background and not hot: a person is waiting on the tool, and it runs on the processor too. `Assisted`, cheap, bar = the winner's probability, 0.6 calibrated and 0.7 before. 30 s an item, 6 000 tokens, no escalation. |
| Budget | `decide/control.rs` `CLASSIFY_PER_TURN` (1 000), `Limit::Classify` | A sorted list is not twenty decisions. It has its own count, skips the repeat rule (the tool asks a distinct item once) and leaves the turn's twenty alone. The 1 001st item stops the list and says so. |
| Batching, shared prefix | `classify::state_for` | The shared `context` comes first in every item's state, so every state starts with the same text. Identical items are asked once and answered twice. |
| Files | `filesystem::clear_read` | A `{path}` or `{image}` item is cleared exactly as `read_file` is (working folder, saved grants, chat grants, then the prompt), and a folder is asked about once per call, yes or no. At most 4 000 characters of a file go to the model. A file the person refuses, or a binary, folder or missing file, is a row that says so; the other rows go on. |
| Images | `classify::encode_image` | Needs the decision model's vision file; without it the tool refuses before reading anything, in plain words. A picture is turned upright by its EXIF orientation, scaled to at most 1 568 px, sent as PNG. Over 16 MB or not an image is a row that says so. |
| What the assistant is told | `classify::summary_for_model` | Counts per answer, the first 40 rows the model was not sure about, what was not sorted and why, and where to pick up if the list was cut. Not all 500. With no table to show (an unattended run) it also gets up to 100 confident rows. File contents never reach the assistant. |
| Time | `TIME_BUDGET` 4 min, `MAX_FAILS_IN_A_ROW` 5 | The clock starts once the first item is asked, so the engine's start is not taken out of it. Stop cancels it. A cut list says how many were not reached and from which number to call again. |
| The block | kind `classify`, `toolsets::render_block_within` | A table of up to 500 rows, with its own 512 KiB cap (the default 64 KiB is for a tool dumping text). Each row carries its decision id so a correction can be a label. |
| `USE-UI-5` | `components/Blocks/ClassifyBlock.tsx`, `lib/classify.ts` | Item, answer, confidence (a bar and the number); each header sorts (up, down, original order). `Low confidence (n)` filter. `Export CSV` through the save dialog; a cell that starts like a formula is defused. A thumbnail on an image row. The hint on the Confidence header says the numbers are the model's own until a fit exists. |
| A correction is a label | `classify_correct_cmd` -> `classify::correct`, `db.set_outcome`, `db.decision_by_id` | Clicking an answer shows the question's answers (buttons for up to 6, the shared `Select` for more). The model's own answer is a `right` label; another is `corrected` with the answer. It is kept under its own source, so changing one's mind replaces it. A corrected row is no longer low, and shows `you`. If the label was not kept the table puts the row back and says so. The label feeds calibration with no change: `outcomes::observation` already reads `corrected` by value. |
| Seed set | `tests/eval/decide/classify.jsonl` | 41 rows, five questions (ticket kind, urgency, tone, which part of the product a code line is, refund or not), interleaved. Joins "Calibrate now" and the `seed_sets` test with no other change. |
| Activity | `Purpose::is_bulk`, `lib/decisions.ts` | Sorted items fold into one row an hour like the background ones: `60 decisions · Which answer fits each item in a list?`. `budget:classify` has words. |
| Experiments card | `experiments.rs` | `fast-slow` lists a fifth line. |

**Verified**

- `cargo test --lib`: **1296 pass, 3 fail**, 15 ignored (was 1268 pass, 2 fail). The three
  failures are the `db` fork tests (`a_fork_keeps_the_summary_it_can_still_account_for`,
  `a_spoken_turn_stays_marked_and_a_fork_keeps_the_mark`,
  `forking_branches_a_conversation_just_before_one_answer`), the millisecond timing flake
  of step 6 (finding 6 there). All 14 of `db::tests::a_` and `forking` pass when run
  alone. This step changes nothing in `db` but two new methods. New: `classify` (24),
  `toolsets` (+2), `control` (+1), `policy` (+1), `db::decisions` (+1).
  `cargo test --test decide --no-run` compiles.
- `vitest`: **1086 of 1086** (was 1053). New: `lib/classify` (16), `ClassifyBlock` (17);
  `decisions` has one more assertion. `tsc --noEmit` is clean.
- The request: the three shapes of answer (yes or no, pick one, a scale), a bare string as an
  option, and every refusal; 500 items and not 501. The shared context is the start of every
  state. An answer is read as the winner's probability, a yes or no as the likelier side,
  and an answer that does not fit the question is not a row. The assistant is told the
  counts and the unsure rows only, 40 of them, and with no table it gets the rest. A cut list
  says where to pick up. 500 rows with 100 thumbnails fit the block's cap. A text file is
  read up to a limit and a binary refused; a picture becomes a PNG scaled into range and a
  JPEG thumbnail of 48 px under 4 KB, and a text file named `.png` is refused. The budget:
  the 1 001st item is stopped, the turn's twenty are untouched, another turn has its own.
  The tool is offered only with the experiment on and a model file there, and goes away
  with the file. A correction is `right` or `corrected`, replaces itself, works for yes or
  no (`true` or `false`), is refused for another purpose's decision or for an answer the
  question did not have, and is read by calibration as it is. The table: sort, filter,
  correction, undo on failure, export (what the file holds, and Cancel), thumbnails, and a
  thumbnail that is not a picture is not drawn.

**Not verified, and why**

- **Nothing ran on a real decision model**, still. So there is no accuracy and no
  calibration for `classify`, and no idea whether the question wording works for short
  items. `cargo test --test decide -- --ignored --nocapture seed_sets` measures it now.
  It has not run.
- **`classify::execute` is not driven end to end**: reading the items, asking, settling,
  rendering. It needs a `ToolContext` (nothing in the tests builds one) and an engine.
  Its parts are tested. So is `authorize`, which `clear_read` calls; `clear_read` itself
  is not.
- **The vision path.** `encode_image` is tested on a PNG. No picture was sent to the engine,
  and the engine's own limits on image size and count are unchecked.
- **No run was watched in the app.** The table is covered by component tests only.
- **I wrote the seed set myself.** It is unreviewed, all text, and the five questions are
  easy ones.
- **How slow a list is on this computer.** Step 7 measured about 12 s a row for a 6 000
  token state on the processor. A short item will be much faster; by how much is not
  known. At 12 s the 4 minute budget is 20 items.

**Behaviour changes**

- With **Fast & slow thinking** on and a decision model installed, the assistant has a
  `classify` tool, and Settings -> Tools lists "Sort many items" (on by default; with
  the experiment off it says what is missing). With either missing, nothing changes.
- A non-background purpose no longer waits for a quiet card. Nothing else was
  non-background before, so no existing purpose changes. A tool the turn itself called
  cannot wait for the turn to end.
- Activity folds a sorted list into one row an hour.

**Not built from the plan text (decided, not forgotten)**

- **A new block kind, not new columns on `table`.** The plan says "the table block gets
  columns". The generic `table` block (`DAT-3`) takes whatever the model hands it; a
  block with sorting, a filter, a correction and a decision id per row is another thing.
  `table` is unchanged.
- **A flat tool schema.** The plan has `options: [{id, criteria}] | { scale: [...] } |
  "yes_no"`. A union type is unreliable across tool-call parsers and small models, so:
  `options` is a list, `scale` is a list, and neither means yes or no. A list of bare
  strings is also accepted as options.
- **Confidence is the winner's probability**, not the entropy measure of `decide::confidence`.
  The table shows "0.83", which a person reads as about 8 in 10. The entropy measure
  stays inside the seam. The bar is on the same number.
- **A time limit** (4 minutes, then the rest is named). The plan has none; on a processor
  a list of 500 would take an hour.
- **Thumbnails travel in the block** (first 100 image rows), not by path. See finding 1.
- **No ship gate.** The plan has no `USE-T` for `USE-9`.
- **`classify` for agent apps** stays deferred (see "Deferred").
- **Images are not resized for a cloud backend** (`decide/images.rs`, step 12).

**Findings that change later steps**

1. **The app cannot show a user's picture by its path.** `tauri.conf.json` limits the
   asset protocol to `$APPDATA/generated-images/**`. A thumbnail of someone's own file
   has to travel as data (this step does that for 100 rows). `USE-11` and anything
   else that shows a user's image needs the same, or a wider scope, which is a security
   decision.
2. **`Classify` is a foreground purpose, so the global backend applies to it.** When the
   cloud half exists (step 12), "cloud" chosen globally would send the text of the files
   in a list off this computer. Step 12 should keep `path` and `image` items local unless
   the purpose's own override says cloud, as decision 12 does for background purposes.
   Nothing leaves the computer today: there is no cloud backend.
3. **Activity reads the newest 200 decisions.** One sort of 500 items fills that window
   and pushes older decisions out of view, even though they fold into one row. A count
   per group from the database would fix it. Not built.
4. **Duplicates share a decision.** Two identical items are one decision, so a correction
   on either is one label for both.
5. **A list on the processor is a short list.** Until a card is there, `classify` is for
   tens of items. The summary says how many were reached and where to start again, but
   the assistant has no way to know the speed beforehand. A speed read from
   `DecideManager::status().median_ms` could go in the tool's description.
6. **The calibration set for `classify` is five questions.** The fit is one temperature
   for every question. A real use mixes many; user corrections (`classify-user`) will
   outnumber the seed rows quickly and are the better data.

### Order step 9 - loop control and task conditions (`USE-5` + `USE-UI-2`/`UI-3`, `USE-7` + `USE-UI-6`): DONE (2026-10-23)

Four more purposes behind **Fast & slow thinking**: `PlanDone`, `SteerClass`,
`UnverifiedClaim` (all in `decide/loopctl.rs`) and `RunIf` (`decide/runif.rs`). Each only
adds to what already happens, and each keeps its old path when the engine is missing,
cold, over budget or unsure.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| Plan completion | `loopctl::judge_open_items`, `finish_from`, `apply`; `run.rs` `TurnCtx::judge_plan` | When a run ends with plan items open, one yes-or-no per open item (at most 6) asks whether the answer carried it out. Done before the existing nudge, so the nudge names only what is really open. A calibrated 0.8 ticks the item (`PlanItem.judged`, `PlanItem.decision`). **Without a fit nothing is ticked** (the plan's "never auto"): a raw 0.8 or more only makes the nudge say `(probably done: your answer seems to cover it)`. The model's own `plan update` clears the judgement. |
| `USE-UI-3` | `PlanCard.tsx`, `store.reopenPlanItem`, `reopen_plan_item_cmd` | A judged item is a ringed tick: hover says `Judged complete from the answer (0.88)`, click reopens it. The ring is a button only on a saved turn, because the stored plan changes (`db.message_plan_json`, `db.set_message_plan`). The reopen writes a `wrong` label (source `plan-reopened`). A tick the run gave itself is a plain box with nothing to reopen. |
| Steering | `loopctl::read_steer`, `steer_run_cmd` (now async, returns `{status, read}`) | A message to a running agent is read as `stop`, `redirect`, `add` or `question`, with the run's current task (the last user message, 300 characters) next to it. The class goes back for a tag, shown only when the winner is 0.6 or more. Only a **calibrated `stop` of 0.9 or more** is held: it is not queued and waits for the user. Before a fit nothing is ever held (the plan's "never"). Row: `Measure::Option("stop")` (new: one named option's probability, not the winner's), 500 ms, hot. |
| `USE-UI-2` | `UserTurn.tsx`, `store.answerSteerStop` | A tag (`addition`, `redirect`, `question`, `stop`) with `Read as an addition (0.82)` on hover. A held stop reads `not sent yet` with **Stop the run?**, **Stop**, **Keep going**. Stop is the ordinary Stop and a `right` label. Keep going sends it on with the read skipped and writes a `wrong` label. The message is saved to the conversation either way. |
| Unverified success | `loopctl::claims_nothing`, `Ledger::has_unverified`, `run.rs` `unverified_note` | The existing note still needs files changed, a project check and no check run. New: before it is spent, one yes-or-no asks whether the answer claims the change works or was tested; it is skipped only when the model is **confident it claims nothing** (0.15 calibrated, 0.05 before), as when the answer says it is untested. A skipped note stays for a later answer in the run. |
| `USE-7` | `decide/runif.rs`, `commands/scheduler.rs` | `Job.run_if`. Before each scheduled run one yes-or-no asks whether the condition holds over the task's input: the top of its folder (newest first, what changed since the last real run marked), up to two web addresses written in the task or the condition (only while Web search is on, 15 s each), and what the last run said. 0.5 or more runs; below it the run is skipped. **Run now never checks it.** |
| What a skip does | `scheduler::record_run` | A history row `skipped: condition not met (0.07)` (`JobRun.skipped`), an Activity line, and the next run is a period away. It is not a run: `last_run_at` and `last_result` stay, so "since the last run" keeps measuring from the last real one. |
| Never skipped unasked | `scheduler::skipped_by_condition` | No condition, no engine, an exhausted budget, a wrong-shaped answer, or **no input to read** (no folder, no readable page) all run the task as before. The last result alone is not an input. |
| `USE-UI-6` | `Tasks.tsx` | **Only run if…** field with the plan's placeholder, a note once it has text (what is read, that it runs on this computer, that it needs Fast & slow thinking), the condition on the task's card, and skipped runs as muted rows with no chat to open. |
| Policy rows | `decide/policy.rs` | `PlanDone` Assisted, visible, hot, 0.8 / never; `SteerClass` Reflex, visible, hot, stop 0.9 / never; `UnverifiedClaim` Assisted, visible, hot, `PFalse` 0.85 / 0.95; `RunIf` Reflex, visible, **background** (local only), 0.5. The table shows 1.0 for "never"; the sites enforce it by requiring a fit, because a probability can reach 1.0. |
| Seed sets | `tests/eval/decide/{plan-done,steer-class,unverified-claim,run-if}.jsonl` | 40 rows each, balanced. They join "Calibrate now" and the `seed_sets` test with no other change. |
| Experiments card | `experiments.rs` | `fast-slow` lists two more lines. |

**Verified**

- `cargo test --lib`: **1324 pass, 2 fail**, 15 ignored (was 1296 pass, 3 fail). The two
  failures are the `db` fork tests again (`a_fork_keeps_the_summary_it_can_still_account_for`,
  `a_spoken_turn_stays_marked_and_a_fork_keeps_the_mark`), the millisecond timing flake of
  step 6 (finding 6 there). Run alone, `db::tests::a_` and `forking` fail **one of 15 a
  run, a different one each time** (two runs), so they are not passing "alone" any more
  than in the full run; nothing this step touches reaches them (it adds two `db` methods
  and one test that forks nothing). New: `loopctl` (9), `runif` (8), `scheduler` (+4),
  `plan` (+2), `policy` (+2), `run` (+1), `ledger` (extended), `db` (+1).
- `vitest`: **1113 of 1113** (was 1086). New: `store.steer` (12), `UserTurn.steer` (6),
  `Tasks.runif` (5), `PlanCard` (+4). `tsc --noEmit` is clean.
- The sites: a plan item ticks only on a calibrated yes, and a raw 0.99 or even 1.0 is only
  "probably"; applying a verdict twice ticks once; a dropped or unknown item is never
  ticked; the model's own update clears a judgement and a reopen hands back the decision
  (and nothing for a tick the run gave itself). The nudge calls a likely item probably done
  and leaves a ticked one out. A stop prompt needs a fit, `stop` at 0.9 and not 0.89, and a
  raw 0.99 is not held; a tag needs 0.6; a spread-out answer and another question's answer
  show nothing; the message sits in the state and the question stays fixed. The claim
  check skips at 0.15 calibrated and 0.05 raw and never on a claim; looking at the ledger
  does not use the note up. The condition: 0.5 runs and 0.49 skips with the number, a folder
  is listed newest first with the changed ones marked, cut at 60, an empty or missing folder
  is no input, only addresses the user wrote are read (two, deduplicated), the last result
  alone is no input, and with no engine the task runs. A skip is a row with the probability,
  not a run, and the history keeps ten rows; a task saved before conditions loads and
  writes nothing new. The four seed sets parse, are balanced and build valid questions.
- The screens: the ring (only for the judged step, with its probability, a button only on a
  saved turn, counted as done), the tag words, the held stop and both answers (stop labels
  right and stops; keep going labels wrong and sends without a second read; a run that
  ended leaves an ordinary turn; a second click does nothing), a message taken back when the
  run is gone, the editor field (saves the trimmed text or null; filled in on edit), the card
  line and the muted skipped row with nothing to open.

**Not verified, and why**

- **Nothing ran on a real decision model**, still. No accuracy or calibration for any of
  the four, and no idea whether "probably done" or the stop reading holds up on real text.
  `cargo test --test decide -- --ignored --nocapture seed_sets` measures them now.
- **The async paths are not driven**: `judge_plan`, `unverified_note`, `steer_run_cmd`,
  `scheduler::condition_input` and `skipped_by_condition`. Each needs a `TurnCtx`, an app
  handle or a network. Their parts are tested.
- **The page fetch of a condition** reuses `websearch::fetch_readable` (a crude tag strip)
  and was not run on a real page.
- **No run was watched in the app.** The stop prompt, the ring and the editor are covered by
  component tests only.
- **I wrote the four seed sets myself.** They are unreviewed and easy: short, clean sentences.
- **On this computer three of the four never run.** `PlanDone`, `SteerClass` and
  `UnverifiedClaim` are hot, and a hot purpose is skipped on the processor (step 2). Only
  `RunIf`, which is background, runs here, at about 12 s a check.

**Behaviour changes**

- With **Fast & slow thinking** on, a decision model installed and its engine warm on a
  card, a run that ends with plan items open may tick some of them with a ring, and says
  "probably done" for the rest only before a fit exists. The plan nudge itself is unchanged
  apart from that.
- A message typed at a running agent shows a class tag when the model is 0.6 sure. After a
  fit exists, a sure stop is not sent and asks first. Before one, nothing is held.
- The "you changed files and did not run the check" reminder is skipped when the answer
  confidently claims nothing.
- A scheduled task with an "only run if" condition may be skipped, and says so. Tasks with no
  condition are unchanged, and **Run now** is unchanged.
- `steer_run_cmd` returns an object instead of a boolean.

**Not built from the plan text (decided, not forgotten)**

- **"Replaces the heuristic trigger" for the unverified note.** The trigger here is the
  ledger (files edited, no check since), not text matching. The ledger stays the trigger;
  the claim check only narrows it, and only on a confident "claims nothing".
- **A `User` escalation for "probably done".** The plan's `CTL-2` lists "probably done" as a
  `User` escalation. The row uses none: it would draw on the turn's three escalations for
  every item that really is open. The surfacing is in the nudge and the ring.
- **No "right" label for a ringed tick left alone.** The plan writes only the reopen.
  Leaving a tick is the normal case and says little.
- **A class tag does not survive a reload.** It is about the moment a message was sent.
- **The condition does not follow links or read subfolders.** Only the top of the folder and
  addresses the user wrote.

**Findings that change later steps**

1. **Three of the four need a card.** On the processor the hot purposes are skipped, so
   plan ticks, steering reads and the claim check do nothing here. Calibrating them needs
   the engine on a card or a long "Calibrate now" on the processor (40 rows at about 12 s).
2. **The stop prompt cannot appear before a fit, and its labels come from the prompt.**
   The first fit has to come from the seed set, which is mine. A prompt that never appears
   writes no labels, so `SteerClass` learns from the seed set only until a stop is held.
3. **A hostile page can move one probability.** The page text in a condition is outside text,
   and the answer is one number that can skip one run. The cost is a missed run, shown in
   the history and undone by Run now. It is not scanned by `untrusted::scan`.
4. **A condition over a page behind a sign-in cannot be judged** and runs the task, by the
   rule above. A task wanting "new mail" or "new calendar event" needs the connector's
   own read, which is not an input here.
5. **Reopen needs the plan to be saved.** A ring on a turn still streaming is only a mark.
6. **Steering and agent apps.** A steer to Claude Code or Codex is read the same way, and a
   held stop stops that engine run through the ordinary Stop. Not run against a real one.

### Order step 10 - mail triage (`USE-6a` + `USE-6` + `USE-UI-4` + `USE-T-5`, `mail-triage`): DONE (2026-10-23)

A small inbox and a purpose on top of it, behind its own experiment, **Mail triage**.
While it is on, every 15 minutes each Mail account (IMAP, the ones in Settings -> Mail) is
looked at, and the newest messages get a kind, an urgency and a reply mark from the
decision model. They are shown and never acted on: nothing is moved, answered, marked read
or deleted.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| The experiment | `experiments.rs` | `mail-triage`: Early, off, **not** behind `fast-slow`. Needs a decision model and a new requirement, `Requirement::MailAccount` ("A mail account", an enabled one; "Set up" goes to Mail). |
| `USE-6a` store | `schema.sql`, `db/mail_seen.rs`, schema **v38** | `mail_seen`: account, UID, message id, sender, subject, date, `sent_at`, unread, `triage_json`, `triaged_at`, `fetched_at`. **No body.** Rows follow their account (`ON DELETE CASCADE`) and go 30 days after they were fetched. The plan said v37; v37 was the decision log. |
| `USE-6a` fetch | `agent/mail.rs` (`fetch_new_blocking`, `triage_texts_blocking`, `new_uids`, `decode_words`), `agent/mail_sync.rs` | One look per account: select INBOX, `UID SEARCH UID n+1:*` (the first look takes `ALL`), keep the newest 200, fetch envelope and flags only. Where a look ended (`UIDVALIDITY`, newest UID) is kept apart from the rows in settings, so pruning rows never makes the next look fetch them again. A server that renumbered its mailbox starts over for that account. Names and subjects are decoded from encoded words. |
| `USE-6a` loop | `mail_sync::start`, `run_cycle` | 90 s after start, then a 30 s tick that looks when 15 minutes have passed. It does nothing while the experiment is off. One look at a time (the refresh button and the timer share the guard). Per look: fetch, prune, then check the newest 15 messages without a tag. Stops at the first sign that the decision model is not there, not ready or over budget; a message the model fails on is closed with nothing to show, so it cannot block the ones behind it. Reads only `list_mail_accounts`, so **a Gmail connected through an app is never polled** (a guard test reads the file for `composio`, `app_calls`, `connectors::`). |
| Message text | `triage_texts_blocking`, `mailtriage::state_of` | The first 64 KB of the message by `BODY.PEEK` over one connection, then 4,000 characters of its text go to the question. Not stored. The decision log keeps the header lines only (`log_preview`), not the first 200 characters of the state as every other purpose does. |
| `USE-6` the question | `decide/mailtriage.rs` | One request, three questions: `category` (a pick from the user's list plus `unsure`), `reply` (yes or no) and `urgency` (four steps). The ids are chosen so `category` sorts first, which is the answer the calibration reads. |
| What is shown | `mailtriage::triage_from` | Each part separately and only at 0.6 or more (`TopProb`; a reply mark needs 0.6 for **yes**). A message the model calls `unsure` shows no kind. A part below the bar shows nothing, never a guess. |
| Policy row | `decide/policy.rs` | `MailTriage`: Reflex, visible, **background** (local only), 0.6 for both bars, 30 s, 4,000 tokens, experiment `mail-triage`. `ALL` is 14 purposes. |
| Budget | `decide/control.rs` | One decision a message, and a first look finds up to 200, so mail has its own share of the background hour: `MAIL_PER_HOUR` 60 (`Limit::MailShare`, "mail triage had used its share of this hour's budget"). Four looks of 15 an hour fit it exactly (a test pins that). |
| Retag labels | `mailtriage::retag`, `retag_label` | A kind the person sets is `corrected` with the kind, a kind they clear is `wrong`, and setting the kind the model chose is `right` (source `mail-retag`, replaced if they change their mind). A second retag is judged against the model's call (`model_category`), not against the first retag. A message that was never checked can still be tagged by hand and writes no label. A kind has to be one of theirs or `unsure`. |
| The user's kinds | `mailtriage::categories`, `set_categories` | Setting `mail.categories`. Default: personal, work, receipt, newsletter, notification. Lower case, one line, at most 8, 24 characters, no repeats, never empty (an empty list puts the default back and is stored as nothing, so a later change to the default reaches the user). A made-up kind reaches the model with only its name. |
| `USE-UI-4` | `routes/Mail.tsx`, `components/Mail/MailInbox.tsx`, `inbox.ts`, `KindEditor.tsx`, `MailInbox.css` | **Accounts** (today's page) and **Inbox** tabs, the tabs only while the experiment is on. Filters All, Needs reply (n), Urgent (n, 3 or more); a picker per account with several. The line "Fetched 09:15 · every 15 minutes · nothing leaves this computer" with a look-now button. A row: an urgency meter, sender, subject, a "Reply" mark, the kind pill (click for the retag menu, the shared row menu) and the time; **Ask about this** starts a new chat with tools on, "Summarise the mail from Ana Ruiz, 'Contract draft for Thursday'" (the account is named only with several). Empty states: no account (to Accounts), no decision model (to Models, the mail is still listed), nothing fetched yet. The kinds editor sits on the Accounts tab under the list. |
| Commands | `commands/mail.rs`, `lib.rs` | `mail_inbox_cmd`, `mail_sync_now_cmd`, `mail_retag_cmd`, `mail_categories_cmd`, `set_mail_categories_cmd`; the event `poiesis-mail-inbox` makes an open Inbox reload as tags arrive (`listenAppEvent`, which gives back the way to stop listening). Removing an account also forgets where its last look ended. |
| Seed set | `tests/eval/decide/mail-triage.jsonl` | 40 mails, 8 work and 7 of each of the others, 4 `unsure`. It asks the **kind alone**. |
| Experiments card | `experiments.rs` | Three lines under "What changes", a graduation gate, and `records`. |

**Verified**

- `cargo test --lib`: **1365 pass, 1 fail**, 15 ignored (was 1324 pass, 2 fail). The one failure
  is the `db` fork flake again (`a_spoken_turn_stays_marked_and_a_fork_keeps_the_mark`, step 6
  finding 6); nothing here touches fork code. `cargo test --test decide --no-run` compiles. New:
  `mailtriage` (15), `mail_sync` (11), `db::mail_seen` (5), `agent::mail` (+3), `commands::mail`
  (3), `control` (+1), `experiments` (+1), `decide` (+1). After the last two edits (the
  setting cleanup on account removal) the mail tests were run again and pass.
- `vitest`: **1153 of 1153** (was 1113). New: `inbox` (6), `MailInbox` (17), `KindEditor` (7),
  `Mail.tabs` (5), `store.mail` (3), `Experiments` (+1), `decisions` (+1). `tsc --noEmit` is clean.
- The seams: only UIDs above the newest kept are fetched (and the one `N:*` always returns is
  dropped), newest first, capped; a look remembers the newest UID and the mailbox number apart
  from the rows, and a server that says nothing about its number does not erase it; a
  renumbered mailbox clears that account's rows and starts over, empty or not; a message seen
  twice keeps its tag; rows older than the cut go and newer ones stay; removing an account
  removes its rows; the newest unchecked messages come first across accounts up to the limit;
  only a failed answer closes a message, and an engine that is not there does not; one look
  runs at a time; with the experiment off a look does nothing and writes nothing; the four
  looks of an hour are exactly the mail share.
- The answer: each part shows at 0.6 and not at 0.59, a message called `unsure` shows no kind,
  a likely no shows no reply mark, the stored form leaves out what was not shown, and the
  hostile text sits in the state while the three questions stay fixed. The state is cut at
  4,000 characters, a header line cannot hold a second one, and the log keeps the header
  only. The bar, the experiment and the local-only rule of the policy row.
- The labels: a retag writes the label a fit reads (against the model's winner), a second
  retag replaces it and is judged against the model's call, clearing is `wrong`, an unknown
  kind is refused, `unsure` is allowed, a hand tag on an unchecked message writes no label.
- The screens: the row (kind, the four-bar meter with its hover words, the reply mark, the
  time, a bold unread row), nothing drawn for a part that was not sure, a message with no
  sender or subject named, the filters and their counts, the menu and its entries and a pick
  that retags, a refused retag leaving the row as it was, Ask about this (with and without
  the account), the status line, look-now and its wait while a look runs, an account that
  could not be read, a reload on the event and the end of listening on close, the account
  picker (only with several), and the three empty states. The kinds editor: add in lower case,
  no empty name or repeat or `unsure`, remove but keep the last, eight at most, back to the
  default. The tabs exist only while the experiment is on and fall back when it goes off.

**Not verified, and why**

- **Nothing ran on a real decision model**, still. No accuracy or calibration for the kind, the
  urgency or the reply mark. `cargo test --test decide -- --ignored --nocapture seed_sets`
  measures the kind now.
- **No IMAP server was reached.** `fetch_new_blocking`, `triage_texts_blocking`, `run_cycle`,
  `triage_waiting` and the loop's timing are not driven; their parts are tested. Two things
  rest on reading the crates, not on a run: `BODY.PEEK[]<0.65536>` (a partial fetch) comes back
  through `Fetch::body()` (the crate matches the section whatever the offset), and `UID n:*`
  behaves as RFC 3501 says. No real Gmail, iCloud, Fastmail or Proton Bridge was tried.
- **Encoded words** were tested on two samples (quoted-printable and base64), not on the odd
  ones real mail has (several words, mixed charsets, broken encodings).
- **The screens were not opened in the app.** Component tests only.
- **I wrote the seed set myself.** It is unreviewed and easy: plain mails, short, clean.
- **The three-question request was never asked as one.** The seed asks the kind alone, which is
  the easier case, and the live request adds two questions to the same state.
- **Speed on this computer** (a processor, no card): `MailTriage` is background, so it runs
  here, but a three-question request was not timed. At the 12 s a one-question check took in
  step 9, 15 messages is about three minutes a look and a first 200-mail look takes about
  fourteen looks, which is three and a half hours.

**Behaviour changes**

- With **Mail triage** on, Poiesis opens a connection to each enabled Mail account every 15
  minutes while it is running, and reads envelopes and flags (and 64 KB of the newest unchecked
  messages) with `PEEK`, so nothing is marked read. It keeps sender, subject, date and tags for
  30 days. With it off nothing connects.
- Mail has an Inbox tab and a kinds editor while it is on. The page is unchanged while it is off.
- The decision log has rows for `mail-triage` holding the header lines of the mail.
- The background budget has a share for mail; the rest of the hour is the others'.
- Schema is v38 (a new table; nothing altered).

**Not built from the plan text (decided, not forgotten)**

- **"Reset its data"** (`EXP-4`, step 15). Turning the experiment off stops the looks and keeps the
  rows until they are 30 days old.
- **A bar on the row's left edge.** The urgency is a four-bar meter in a leading cell. A coloured
  stripe along the edge of a row is the pattern the design review rejects.
- **A list of a fixed height with its own scroll.** The page scrolls and a count is shown. An
  inner scroller would cut off the retag menu on the last rows.
- **A confirm button.** `right` is written when the person sets the kind the model chose after
  changing it; there is nothing else that says "this tag is right".
- **Notifications and an unread count in the rail.** The plan says headless and quiet.
- **Other folders than INBOX**, and mail through an app.

**Findings that change later steps**

1. **One temperature serves three questions.** The fit comes from the kind (the seed asks only
   that, and the labels are about the kind), and `calibrate` applies it to urgency and the reply
   mark too. Their numbers are as trustworthy as the kind's, which is not known. Retags are the
   only labels there will be; there is no label for urgency or reply. If those two matter, they
   need their own purposes.
2. **A first look is slow by design.** 200 messages, 15 a look, 60 an hour. The newest are done first.
3. **Mail text is outside text and is not scanned.** The most it can do is move three probabilities,
   which gives one wrong tag on one row, undone with a click. It cannot reach a tool or the chat.
4. **The experiment is not re-checked against its needs.** If the last account is removed or switched
   off while it is on, it stays on, the loop reads nothing and the Inbox says to add an account.
5. **A message whose text part comes after a long attachment** (or is mostly markup in the first
   64 KB) is judged by sender and subject. `strip_html` drops style and script, not layout tables.
6. **The kinds a user removes stay on rows already tagged.** The editor says so.

### Order step 11 - verify an answer (`USE-10` + `USE-UI-7`, `verify`): DONE (2026-10-26)

A **Verify** action on an assistant answer, behind its own experiment, **Verify answers**
(Early, off, not behind `fast-slow`). The first session built the backend on 2026-10-23 and
hit its limit before the screens; the screens, their tests and this entry were finished on
2026-10-26. Nothing changes the answer that was checked: the result is stored beside the
message, and **Revise** sends an ordinary message that asks for a new answer.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| The experiment | `experiments.rs` | `verify`: needs a decision model and `Requirement::Evidence` (Web search on, or a folder that was read). The card says the words of a claim leave this computer when the web is used. |
| Schema **v39** | `schema.sql`, `db/verifications.rs`, `db/mod.rs` | `verifications`: message id, conversation id, `claims_json`, note, skipped, created. One row per message; checking again replaces it. Rows go with their message and conversation. |
| The run | `agent/verify.rs` (`run`, `Steps`, `Live`) | List (slow) -> evidence -> quick verdict -> blind review. The steps are a trait, so the order, the limits and the cancel are tested without an engine. At most 25 claims and 4 reviews a check; the answer is read to 12,000 characters. |
| 1. List | `list_messages`, `parse_claims`, `shape` | The language model returns a JSON array: claim, quote, source address, importance 1 to 3, type. Opinions and repeats are dropped, over 25 keeps the most important in the answer's order, and each claim's words are found in the answer's plain text (`locate_quote`) for the underline. A reply that is no list is an empty check. |
| 3. Evidence | `Live::evidence` | The cited page first, then the attached folder's index (similarity 0.5 or more, 2 passages), then the web (the Web search tool must be on; only for a claim nothing else covered or one that goes stale; 0.7 s between searches; a refusal stops the web for the rest of the check and is said once). Every passage keeps its source, its date and when it was read. A passage that scans as orders to an assistant is dropped. A claim's address must be public (`public_url` refuses localhost, private ranges and non-http). |
| 4. Verdict | `decide/claims.rs`, `Purpose::ClaimVerdict` | One pick-one question per claim: supported, refuted, insufficient, conflicting. A claim with no evidence is "insufficient" and is not asked. Policy row: Verify mode, visible, **not background**, 0.85, slow escalation, experiment `verify`. Seed set `tests/eval/decide/claim-verdict.jsonl` (40 rows) is wired into `seed.rs`. |
| 5. Review | `Live::review`, `review_messages`, `priority` | The claim and the evidence only, never the quick verdict. Evidence is fenced as outside text. Order: refuted first, then claims with no verdict, then "insufficient" on a claim the answer leans on, then the rest by importance. A claim over the limit keeps its quick verdict and says "Not reviewed". A review that names no finding keeps the quick verdict and says so. |
| Limits | `decide/control.rs` | `VERIFY_CLAIMS` 25 and `VERIFY_REVIEWS` 4, counted by chat; `begin_turn` resets them for each check. |
| Commands | `commands/verify.rs`, `lib.rs` | `verify_message_cmd` (events: progress, done, cancelled, error), `verify_cancel_cmd`, `verify_list_cmd` (empty while the experiment is off), `verify_mark_cmd`, `verify_revise_cmd`. One check runs in a chat at a time (`VerifyRuns`, a slot given back on any exit). |
| Labels | `verify::mark`, `claims::mark_label` | "This is right" or "This is wrong" is kept with the claim and writes a label on the quick decision (source `verify-mark`): the mark is the label when the shown finding is the quick one; when a review overturned it, agreeing writes `corrected` with the review's finding and disagreeing writes nothing (and takes back an earlier label). |
| Revise | `verify::revise_request` | Asks for "a corrected version as a new answer" from the refuted, disputed and unconfirmed claims, with their evidence. A claim the person marked wrong is not repeated as fact. Nothing to revise is an error the screen shows. |
| `USE-UI-7` store | `store.ts` (`verifications`, `verifyMessage`, `cancelVerify`, `markClaim`, `reviseFromVerification`, `loadVerifications`), `lib/api.ts`, `lib/verify.ts` | Progress lands per message. A cancel keeps nothing and leaves an older check. Turning the experiment on or off clears and re-reads. `lib/verify.ts` holds the words (summary, how a finding was reached, source line, progress) and the remark plugin that wraps a claim's quote in a `mark`. |
| `USE-UI-7` screens | `Conversation/VerifyPanel.tsx` (`CheckedAnswer`), `Verify.css`, `RunText.tsx`, `AgentRun.tsx` | The action is in the message action row: **Verify** (or **Verify again**), shown while the experiment is on, on a finished, saved answer of 40 characters or more, hidden while a check runs. While it runs: "Checking 13 claims · 4 done" or "Reviewing 2 of 3 claims" with **Cancel**. After: refuted is a red underline, "not confirmed" an amber dotted one, "sources disagree" an amber solid one, supported none unless **Underline supported** is ticked. A summary line ("1 supported · 1 refuted · 1 not confirmed"), "n opinions left out", the check's note, **Revise**, and a list of every claim (so a claim with no words to mark is still reachable). A click on an underline or a list row opens a card: verdict, how it was reached ("Reviewed against the evidence", "Quick verdict, not sure (0.41)"), the reason, each passage with source, date, when read and where from, and **This is right** / **This is wrong**. |

**Verified**

- `cargo test --lib`: **1403 pass, 0 fail**, 15 ignored (was 1365 pass, 1 fail; the `db` fork
  flake passed this run). New: `agent::verify` (22), `decide::claims` (5), `commands::verify` (3), `db::verifications` (3)
  and `experiments` (+1). `cargo test --test decide --no-run` compiles.
- `vitest`: **1200 of 1200** (was 1153). New: `verify` (lib, 15), `store.verify` (13),
  `VerifyPanel` (19). `tsc --noEmit` is clean.
- The run: a sure claim stands with no review and a claim with no evidence is not asked; a
  refuted claim is reviewed and the review's word is the finding; four reviews at most in the
  order above, with the rest saying so; a review with no usable answer keeps the quick verdict;
  a cancel stops before the next claim and keeps nothing; the progress row counts claims, then
  reviews; an answer with nothing to check gives an empty result, a long one is cut and says so,
  and a list the model could not give is an error and not a silent empty result.
- Safety: only a public address is read, a passage that gives orders is dropped, the review prompt
  holds the claim and fenced evidence and no verdict, and the quick question carries the same fence.
- The words: where a quote is found (case, markdown marks, line breaks, a quote not copied exactly),
  and that a quote with pattern characters ("$5.00 (about)") does not break the match.
- The screens: the action's four states (on, off, not read, too short, streaming, busy, again, running);
  progress and Cancel; the underlines and their classes, with the answer's text unchanged; the toggle;
  the summary, opinions left out and note; the list with a claim that has no mark; the card from an
  underline and from the list, closed by a second click; right and wrong reaching the store and showing
  as pressed; Revise, its failure shown, and its absence when everything was supported; an error with
  the action still there; the experiment off drawing no mark and no panel.

**Not verified, and why**

- **Nothing ran on a real decision model or a real language model.** No accuracy or calibration for the
  verdict. `cargo test --test decide -- --ignored --nocapture seed_sets` measures it. The listing and
  review prompts were never answered by a real model, so the JSON they ask for is an assumption (the
  parsers accept a fence, words around the list, and a plain-word review).
- **No real web search, page fetch or folder search ran.** `Live::evidence`, `from_web`, `from_folder`
  and `from_cited` are not driven; their pure parts (`best_window`, `public_url`, `usable`) are.
- **The screens were not opened in the app.** Component tests only. The underline colours, the card and
  the placement under the answer have not been looked at.
- **The decision log rows** for `claim-verdict` were not read in Activity.
- **I did not check the blind review's value.** The graduation gate needs the claim set to show the review
  catching more wrong claims than the quick verdict alone; the plan's `CTL-T-3` claim slice (precision and
  recall, with and without the blind review) is not written.
- **The seed set was written by Claude** and is unreviewed.

**Behaviour changes**

- With **Verify answers** on, a saved answer has a Verify action. A check calls the language model twice
  or more, the decision model once per claim, and, where Web search is on, searches the web with the claim's
  words and fetches pages. It never runs by itself.
- Schema is v39 (a new table; nothing altered).
- A `claim-verdict` row in the decision log for each claim asked.

**Not built from the plan text (decided, not forgotten)**

- **A hover card.** The plan says hover shows the verdict and the passage. A card that holds text, a source
  line and two buttons cannot be reached by hover on a touch pad or a keyboard, so a click (or Enter) opens it
  and the hover shows a one-line title.
- **A numeric code check** ("when the code toolset is on", step 3). The check does not run code.
- **A separate "triage" step** (`Score` importance plus `Choice` claim type). The language model gives both
  with the list, in the same call, so there is no second ask.
- **Links to the source.** A passage shows where it came from in words. Opening an address from here would go
  through the app's external-link path, which is not wired for this card.
- **Verify on a message in a Workspace or a fleet run.** Only the chat's own answers have the action.

**Findings that change later steps**

1. **The importance and the type come from the language model, not the decision model**, so they are not
   calibrated and have no labels. If importance matters for which claims are reviewed, it needs its own purpose.
2. **A check is slow on this computer.** The decision model runs on the processor here (as in step 10), so
   25 claims is 25 asks after 25 evidence lookups. A progress row and Cancel are the answer, not speed.
3. **A claim is only underlined if its quote is inside one piece of text.** A quote that runs across bold
   or a link is not marked; the claim is in the list.
4. **Marks are not stored on the answer.** They are drawn from the stored claims each time, so an answer
   that is edited by a later feature would lose its marks without losing its check.

### Order step 12 - the cloud backend (`DEC-5` cloud half, `DEC-5a`, `DEC-UI-1` cloud group, `DEC-UI-2`, `DEC-UI-3`, `CTL-UI-2`): DONE (2026-10-26)

OpenRouter's decision models answer the same questions the local engine does. **No decision request has been
sent to OpenRouter yet** (no key was used), so the request and reply shapes are the documented ones; see
"Not verified". The catalog and the zero-retention list were read from the live service and recorded.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| `DEC-5` cloud half | `agent/decide/cloud.rs`, `mod.rs` (`ask_under`, `record_verdict`, `log_budget_skip`) | `POST {base}/alpha/decisions` with the Bearer key from Providers, the pinned `canonical_slug` as `model`, `provider: { data_collection: "deny", zdr: true }`, and a `session_id` that is a hash of the run's id (one per launch without a run), never the conversation. The questions are built by the local builder, so both sides of a `noul`, a `choice` map and a `score` array are the same. The reply is read by the local parser once a score's probabilities (a list in the docs, a map locally) are in one shape; `usage.cost` and the answering `provider` are kept. `BackendId::OpenRouter` logs `openrouter:<model>@<provider>`. |
| Resolution and the background exception | `ask_under` | Purpose override, then `decide.backend`, then local (unchanged). A background purpose goes out only on its own override. The cloud is tried first; a cloud that cannot be used for the call (no key, a stop in force, a picture its model was not shown to read) is asked of this computer instead, never the other way round. The decision budget is counted once even when the call then falls back (`admitted`). |
| Cloud errors | `cloud.rs` (`classify`, `Health`) | 401/403: stopped for an hour, the Providers card says "rejected the key". 402: stopped 10 minutes, "Out of credits". 429/500/502/503/524/529: this call goes local, and 3 in 5 minutes stops the cloud for 10. 404: no provider meets the privacy rule, that model is marked and the call goes local. 400/413: `Failed` with the body, no fallback. A timeout of a hot purpose is `Deadline` (this computer would be slower); of another purpose it goes local. One Activity line per stop. A stop already in force is not extended. A new or removed OpenRouter key, picking a model, switching the backend and the Test button clear it. |
| 800 ms deadline | `policy.rs` (`Policy::cloud_deadline`) | 800 ms or the row's own, whichever is more. `ToolSelect`, the live `HandoverRoute` and `SteerClass` get 800 ms over the network; the local 300 ms is not moved. |
| `DEC-5a` catalog | `cloud.rs` (`parse_catalog`, `load_catalog`) | `GET /api/v1/models?output_modalities=decisions` and the public `GET /api/v1/endpoints/zdr`, cached in settings for a day, refreshed on request. Aliases (`~vendor/model-latest`) are left out. Each model has slug, name, context, images, input price (per million), free, **zero-retention (`zdr`)**, "same as local" (clef-flash and clef) and "featured" (three Clef, two Jev). |
| `DEC-5a` probe | `cloud.rs` (`probe`), `commands/decide.rs` | The fixed three-question request must come back well formed within 5 s. For a model that lists images, a second request with a 320 px red square in `state` as a content part must be answered "red". Both results are kept per model; the first selection tests automatically. A model is not given pictures unless its Test read one. |
| Commands | `commands/decide.rs`, `lib.rs` | `decide_cloud_catalog_cmd`, `decide_cloud_probe_cmd`, `decide_cloud_select_cmd` (refuses a model with no zero-retention provider), `set_decide_backend_cmd` (the cloud needs a key), `set_decide_purpose_cloud_cmd` (background purposes only), `decide_cloud_status_cmd`, `decide_usage_cmd`. `decide_policy_cmd` also says which purposes are allowed the cloud. |
| Counts as "a decision model" | `commands/experiments.rs` (`facts`), `agent/toolsets.rs` (`Toolset::Decide`), `agent/classify.rs`, `agent/mail_sync.rs` | The cloud counts once it is chosen and has a key, so the Experiments requirement, the `classify` tool and `Toolset::Decide` no longer need a model file. Pictures count once a Test read one. Mail triage runs without a local engine only when the cloud is allowed for it by name. |
| Cost | schema **v40** (`decisions.cost_usd`), `db/decisions.rs` | `set_decision_cost`, `decision_usage_since` (answered decisions by local or OpenRouter and model; a budget skip is not one). |
| `DEC-UI-1` cloud group | `components/DecisionModel/CloudDecisions.tsx` (`CloudModels`), mounted in `DecisionModelGroup` | "In the cloud · via OpenRouter": the five featured models and the chosen one, **Show all 27**, a refresh with its date, each row with input types, context, input price, "same as local ✓" and what the Test found about pictures ("images ✓", "images: not accepted via OpenRouter", "images listed, not tested"); Jev's pictures carry "images: experimental". A model with no zero-retention provider is greyed and cannot be picked. **Use this one** / **Selected** and **Test**, with the Test result under the row. Without a key the group is one line with a link to Providers. Picking a model never switches the backend. |
| `DEC-UI-2` | `CloudBackendChoice` in `QuickDecisionsCard`, `DecisionModel.tsx` | **Where**: This computer, or Cloud via OpenRouter with the model's name; the cloud is disabled without a key and links to Providers. It says what is sent, that only providers that keep nothing are used, about what 1,000 decisions cost, and that background questions stay here. A cloud line: green "Ready · 180 ms", amber "Cloud unavailable (overloaded), using this computer until 14:32" / "Out of OpenRouter credits, using this computer.", red "OpenRouter key rejected. Using this computer.". Advanced gains a **Cloud** column: "Allow the cloud" for a background purpose, "follows the choice above" otherwise. A line under Calibration says cloud answers are not measured and can only make me more careful. |
| `DEC-UI-3` | `routes/Providers.tsx` | The OpenRouter card adds "Also used for: quick decisions (chosen in Runtime)" only while the cloud is the chosen backend. A key failure shows on the card through the existing `last_error`. |
| `CTL-UI-2` | `routes/Usage.tsx`, `lib/decideUsage.ts` | A "Decisions" section: "23 quick decisions: 15 on this computer, 8 in the cloud. The cloud charged under $0.01.", a row per model with count, average time, tokens and cost ("free · on this computer", a price, or "cost not reported"). |

**Verified**

- `cargo test --lib`: **1444 pass, 0 fail**, 15 ignored (was 1403). New: `agent::decide::cloud` (33), cloud paths in
  `agent::decide` (6), `policy` (1), `db::decisions` (1). `cargo check --tests` is clean.
- `vitest`: **1228 of 1228** (was 1200). New: `CloudDecisions` (23), `decideUsage` (5). `tsc --noEmit` is clean.
  (A full run while `cargo test` was running failed every file with "no tests", from load; alone it passes.)
- Against the live service (public, no key), recorded in `fixtures/decide/`: the catalog parse (27 models, aliases
  left out, a dated model pinned by its `canonical_slug`, prices per million, images, the zero-retention flag) and
  the zero-retention list.
- Against a mock server: the request carries the Bearer key and the privacy block; each error status is the fault
  it means; a slow server is a `Deadline` for a hot purpose after 800 ms; a refused request is `Failed` and does not
  stop the cloud; a 402 falls back, is told once, is not asked again and shows on the Providers card; nothing is sent
  without a key; the cost is kept and shows in Usage; only a purpose's own override sends a background purpose out.

**Not verified, and why**

- **No decision request has reached OpenRouter.** The body (`provider` block, `session_id`, a picture as a content
  part in `state`) and the reply (a score's probabilities as a list or a map, `usage.cost`, `provider`) are built from
  the plan's notes of the alpha docs. The first run with a key is the **Test** button; read its result before
  trusting anything else here. Whether a 404 is really what "no endpoint meets zero retention" looks like is also
  unconfirmed.
- **Pictures through OpenRouter.** The probe's shape (a content-part array in `state`) is a guess. If a model fails
  it, the row says "not accepted via OpenRouter", which means "not accepted in this shape".
- **The screens were not opened in the app.** Component tests only.
- **`CTL-T-5`, the model bake-off**, was not run: it needs a key and costs cents. Which cloud model is the default is
  still Clef-flash by decision 2, and Jev's role is still undecided by data.
- **Latency.** No cloud round trip was measured, so 800 ms is the plan's number and not a finding.

**Behaviour changes**

- With the cloud chosen, every purpose someone is waiting on sends its state to OpenRouter: the message (tool menu),
  excerpts of files and pages (sufficiency), the last answer and the changed files (goal check), the claims and their
  evidence (Verify), the items (the `classify` tool). Background purposes (memory scope, the critic, reflection gate,
  untrusted flag, mail triage) stay on this computer unless allowed by name.
- The cloud counts as a decision model for the Experiments requirement, `Toolset::Decide` and the `classify` tool.
- Schema is v40 (`decisions.cost_usd`).

**Not built from the plan text (decided, not forgotten)**

- **Jev's pictures, long context and their probes** (`DEC-8`, `DEC-T-1b`): the `assets` map, the two
  `TypeSafe-Danger-Zone-*` headers and the image preparation (scaling, format conversion, EXIF). There was no key to
  show whether OpenRouter forwards either header. Jev is selectable as a text model with a 64k cap like every cloud
  model, and its pictures would go through the generic probe above.
- **`preferred_max_latency`** in the `provider` block. Its unit is not in the notes and a wrong one could filter out
  every provider. The client's own deadline does the job.
- **A borrowed calibration** ("same as local" lending the local fit). A cloud answer is always uncalibrated and so
  only makes things stricter. The weights are the same but the arithmetic is not (BF16 there, 4-bit here), and the
  plan's own wording ("borrows ... marked borrowed") needs a flag on the verdict. **Calibrate now** measures the
  local model only; a cloud fit would need a paid run, started by the person.
- **Filter chips** (images, long context, free) on the cloud list.
- **The cost on a decision's own row in Activity.** It is kept and shown in Usage.

**Findings that change later steps**

1. **The zero-retention list is public** (`GET /api/v1/endpoints/zdr`, matched by the catalog `id`). This answers open
   question 2 of `DEC-8`: Jev 1.13 and Jev 1.14 each have a zero-retention endpoint (TypeSafe), so neither is disabled.
   Six of the 27 listed models have none (`openai/gpt-6-luna-decisions`, `respan/span-01`, `span-01-lite`, `span-02`,
   `inception/mercury-decide:free`, `span-01-lite:free`); they are listed and greyed.
2. **The live catalog is not the plan's table.** Jev 1.13 and 1.14 both list text and images and 262,144 tokens;
   Clef lists 65,537 (not 16k) and Clef-flash 24,576; `typesafe/jev-latest` and two Perplexity names are aliases
   (skipped). New since the table: Baseten Mammoth 1 and Elephant 1 (free), `perplexity/pplx-decider-v1.2` and
   `-flash`, `nace-ai/drex-max`, `liquid/d1-ultra`, `scaledown/classify-1` (text, image and file, 1M tokens),
   `respan/span-02`. 30 entries in all.
3. **Pinning does not protect Clef or Clef-flash.** Their `canonical_slug` is the plain id with no date, so a silent
   change of weights behind it would keep the same calibration key. The reply's `model` (the resolved version, per the
   docs) is kept in the decision's backend label, so a change would show there if the service reports one; nobody has
   seen a real reply to know.
4. **A cloud call that fails is invisible to the site that asked.** It gets its old path, as with no engine. The only
   sign is the Runtime line, the Activity line and the Providers card. If a person runs Verify on a bad key they see a
   slower local check, not an error.
5. **`USE-11` (step 13) through the cloud** depends on the picture probe above. Run Test on a model that lists images
   before relying on it.

### Order step 13 - visual checks for the browser agent (`USE-11` + `USE-UI-9`, behind `fast-slow`): DONE (2026-10-26)

After a browse, a click or a key press the browser agent asks the quick decision model, with a picture of the page
and the start of its text, whether the page shows ordinary content, a login, a captcha or an error. A login or a
captcha, or a page the model cannot place, stops the agent and asks the person to take over. **No picture has been
sent to a real engine**: the decision model files were deleted after step 2 and no cloud key was used. The window,
the cookies and the card were exercised; how the model reads a page was not.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| `USE-11` check | `agent/decide/pagecheck.rs`, `agent/browser.rs` (`finish_action`, `land`) | One pick-one question, `fine` / `login` / `captcha` / `error`, over the screenshot the Browser panel already takes (PNG, sent as it is) and a state of the address **without its query**, the title and the first 700 characters of the page's text. The state changes when the page's text does, so the "same state is not re-asked" rule does not swallow a modal that opens in place. Asked after `browse`, `browser_click` and `browser_press`, not after a scroll or typing. A click that did not move the page still gets a fresh screenshot while the check is on. |
| Policy row `BrowserCheck` | `policy.rs` | Verify, Costly, reads the probability of `fine` (`Measure::Option`), 0.8 for both fits, escalation `User`, a turn is waiting (a cold engine or a processor is skipped), 4 s, 2 000 tokens of state, `NeedsVision::Any`, behind `fast-slow`. |
| What a verdict does | `pagecheck::read_page` | `fine` at 0.8 or more: go on. `login` or `captcha` as the winner at 0.5 or more: stop, named. `error` at 0.6 or more: go on, with one line added to the tool result. Anything else below the bar: stop as "I can't tell what this page is showing". No picture reader, a cold engine, no budget, no answer: the old path, silently. |
| Three cards a turn | `pagecheck::judge`, `control::after` | A stop uses one of the turn's three escalations. The fourth is let through, as the browser went on before the check. |
| Seed set | `tests/eval/decide/browser-check.jsonl` | 40 rows (14 `fine`, 10 `login`, 8 `captcha`, 8 `error`), **text only**: a seed run has no screenshots, so "Calibrate now" measures how the model reads the words of a page, and the live check adds the picture. Written by Claude, unreviewed, like the others. |
| `USE-UI-9` card | `permissions` (`PageRef`, `PermissionRequest::page_stop`), `SidePanel/PermissionPanel.tsx` (`PagePrompt`), `.css` | In the permission panel, like the handover card: "The browser stopped on {site}", the question ("This page wants a login. Take over?", "... wants to know you are a person ...", "I can't tell what this page is showing. Want to look?"), the screenshot, a line with what was seen and how sure ("login page · quick decision (0.91)"), **Open browser**, **Skip this step**, and a small link **This page is fine, carry on**. |
| **Open browser** | `browser.rs` (`take_over`, `close_session`, `window_closed`, `cookie_jar`, `refill_jar`, `return_url`, `launch(.., headed)`) | The agent's browser is headless, so there is no window to hand over. The quiet browser is closed cleanly and **the same profile is opened in a visible window** on the same page. The person signs in or finishes the check and closes the window (or the run is stopped, or 15 minutes pass). The agent then starts a quiet browser on that profile again, goes back to where the person ended up (only a site it was already allowed to visit, never a private address), and reads the page. The cookies are copied across (see Findings). The approved sites and the trail carry over; the host is not checked again at once. Their own browser and cookies are never touched. |
| Labels | `pagecheck::label_card` | **Open browser** writes `right`, **This page is fine** writes `wrong` (source `browser-card`). **Skip** writes nothing: it says nothing about the page. |
| Experiments card | `experiments.rs` | One more line under *Fast & slow thinking*, saying the picture stays on this computer unless a cloud decision model was chosen. |

**Verified**

- `cargo test --lib`: **1457 pass, 0 fail**, 17 ignored (was 1444, 15). New: `pagecheck` (12), the return address (1),
  and the two real-browser tests (ignored). The two seed checks now cover 16 purposes. `cargo check` is clean.
- `vitest`: **1235 of 1235** (was 1228). New: `PermissionPanel.page.test.tsx` (7). `tsc --noEmit` is clean.
- **In a real Chrome** (two opt-in tests, `cargo test --lib take_over_window a_sign_in_made_in_the_window -- --ignored`):
  a visible window opens on the agent's profile and closes cleanly; a quiet browser starts again on the same
  profile; and a sign-in cookie that lasts only until the browser closes, set in the window, is **lost** when only the
  profile is kept (the control) and **sent** once it is carried over.
- The check's reading of an answer (each branch of `fine` / `login` / `captcha` / `error` / unsure, the bar of 0.8,
  an answer of the wrong kind), the three-cards-a-turn rule, the labels, and the card (text, picture, buttons and
  what each sends) are unit tested.

**Not verified, and why**

- **No picture has gone to a real model.** Not locally (no model files; whether `/v1/systemone` takes `images` from the
  pinned build is the same unknown as for `classify`) and not through OpenRouter (the picture shape is step 12's
  guess). How well Clef-flash tells a login from a page that merely has a sign-in link, how often it stops a good
  page, and how long it takes are all unknown. **The bar of 0.8 is the plan's starting value, not a measurement.**
- **The hand-over was not driven from end to end.** `hand_over` and `take_over` need the whole run state
  (`ToolContext`), which has no test builder. Their pieces were run: the window, the cookies, the return address, the
  labels and the card.
- **The card was not opened in the app**, and the visible window was never watched by a person.
- **A person closing the window** is detected by "no tab is left" as well as "the process ended", because closing
  every tab by command did not end Chrome. A real click on the window's close button was not tried.
- **Seed rows** are text only and were not checked by anyone but their author.

**Behaviour changes**

- With *Fast & slow thinking* on and a decision model that reads pictures, every browse, click and key press of the
  browser agent costs one quick decision and a screenshot. A long browse uses the turn's twenty decisions up, after
  which the pages are not looked at.
- A page the quick check calls a login or a captcha now stops the agent until the person answers.
- With the cloud chosen for quick decisions, a **picture of the page** goes to OpenRouter, which can show things the
  agent's text view would not (an account page). Zero retention applies, as for every cloud decision. The same text
  already goes to a cloud chat model when that is the chat model.
- `PermissionRequest` has a `page` field; the permission panel handles a `"page"` capability.
- The Decision model screen lists a 16th purpose.

**Not built from the plan text (decided, not forgotten)**

- **"Did the page change?" and "is the expected element visible?"** as questions. The first is a comparison of the
  page's text before and after, which needs no model; the second needs the lead's intent, which a tool call does not
  carry. Neither stops anything.
- **The label "the check said changed and the next step failed on the same page"** (`CTL-3`). A click that misses is
  mostly the lead's wrong text, not a wrong reading of the page, and the miss is told to the model already.
- **Jev for pictures.** Not built in step 12 either, so only the generic cloud picture probe applies.
- **Local first, then cloud, then Jev** as a vision order. The seam's choice applies (cloud when chosen and it reads
  pictures, otherwise this computer).
- **A second button on the card** was added that the plan does not have: *This page is fine, carry on*. Without it a
  wrong stop could only be skipped, and the only labels would be `right`, which would teach any fit that every stop was.

**Findings that change later steps**

1. **The browser is headless, so "take over" needed a mechanism.** The plan's **Open browser** assumed a window. It
   is now a second launch of the same profile with a head, and the Browser panel shows no page while the person has
   it. A different design (keep one visible browser for the whole session) would avoid the restart but opens a window
   on every browsing run.
2. **A sign-in is a cookie that lasts only until the browser closes**, and the profile does not keep those: the
   control in the real-Chrome test proves it. Closing the window and starting a quiet browser would have lost the
   login every time. The jar is read every second while the window is open (a closed window cannot be asked) and
   written into the quiet browser. A site that ties its session to something else (a device key, the exact
   browser) may still ask again.
3. **A new window comes with two tabs**, and closing every tab by command does not end the browser. The spare tab is
   closed, and the window counts as closed when the process ended or no tab is left.
4. **A seed run cannot calibrate a picture.** The only labels a person gives are on the card, and they come from
   stops, so a page the check wrongly let through is never labelled. A fit on this purpose says how sure the model
   is about its stops, not about its passes.

### Order step 14 - the tool-call gate (`USE-12` + `USE-UI-8` + `USE-T-2`/`T-3`, behind `fast-slow`, `relax-prompts`): DONE (2026-10-26)

Before a tool call runs, a quick decision can say it looks irreversible or not what the user asked for. It can only add
caution: a question of its own, or one line on a question that was coming anyway. It never opens what a deterministic
gate closes. **No real model has answered these questions**: the decision model files were deleted after step 2 and
no cloud key was used. The wiring, the rules and the screens were exercised; how well the model rates a call was not.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| Two purposes | `policy.rs` | `ToolRisk` (Gate, Irreversible, reads the top level of a 4-level rating, 0.8, 1 s, `fast-slow`) and `AppKind` (Gate, Irreversible, reads the `send` option, 0.6, 1 s, `fast-slow`). Both are hot: a person waits, so a cold engine or a processor is skipped. New `Measure::Level(n)` and `Purpose::headless_policy()`: an unattended run reads both as background purposes (waits for the engine, runs on the processor, stays on this computer). New `decide::ask_gate`. 18 purposes now. |
| Own tools | `decide/toolgate.rs` (`before_own`, `act_on`), `run.rs` (`dispatch`) | One request, two questions: *how bad if this were a mistake* (harmless / small / serious / irreversible) and *is this what the user asked for*. The state is the user's request (first 1 500 characters, for a child run the task it was given), the tool and its arguments (a body, content or code cut to 200 characters). Asked for `run_command`, `run_task`, `run_code`, `send_mail`, `reply_mail`, `browser_click`, `browser_type`, `browser_press`, `open_app`, and for `write_file`, `edit_file`, `delete_file`, `move_file` **only where the person would be asked anyway** (trust "ask first", or any deletion or move). Never asked when unattended for the file tools (they refuse already). |
| What it does to an own tool | `toolgate::flag_own` | Not asked for at 0.8 or more, or irreversible at 0.8 or more: **a question of its own** before the call, with a line (`Asked because this is not what you asked for (0.90)`) and the arguments. *Go ahead* runs it, *Don't do it* does not, and the model is told why in words. A milder doubt (not asked for at 0.6, irreversible at 0.5) becomes **a line on the question the tool raises itself** (`Flagged: this looks irreversible (0.62)`). The request comes first when both fire. Unattended, an added question is a **refusal with the reason**, never a hang. |
| The line on a tool's own question | `toolgate::NOTE` (a task-local), `run.rs` `send_permission` | Those questions are raised in six places inside the toolsets. A note set around the call's execution is read by `AgentEventSink::send_permission`, so none of the six changed. Only the kinds a tool raises about itself take it (operation, scope, domain, screen, open-app, task, command); an app call, an agent app, a handover and a page card carry their own. |
| App calls, in-scope | `toolgate::scope_of`, `connector_gate::admit` (`GateEnv.notes`), `run.rs` (`dispatch`, `look_at_batch`) | The in-scope question alone (risk is not asked: the gate's kind is the risk). Its line is on the prompt the gate already shows. A **read** the request did not call for (0.8 or more) is asked about, the one thing it can add; a change or a send already asked. Unattended, the added question is the existing "left for you" parking. Composio batches are looked at once for the whole batch. |
| `AppKind` | `toolgate::raise_kind`, `app_calls::Planned::raise`, `run.rs` | For a connector tool whose name says read or change and whose kind the person has not set (`Resolved.overridden`, new): one pick-one over its name and what its maker says it does. `send` at 0.6 or more makes the call a send (the send layout, the daily cap, `Off` still refuses). It never loosens: `read` or `unsure` changes nothing. **One answer per tool per day** (a cache; a "keep the name's kind" answer is cached too). A tool raised to a send while planning first is refused. |
| Agent apps | `engines/approve.rs` (`Look`, `Port::with_look`), `engines/run.rs` | The in-scope question against **the task that was handed over**, as one line on the approval. The buttons do not change, a question is never added or taken away. |
| `relax-prompts` | `experiments.rs`, `toolgate::relax_of` / `skip_prompt`, `filesystem.rs` `authorize` | A new experiment, needs `fast-slow`. It skips **one** question: writing or editing one file inside the attached folder. It needs a calibrated verdict (an uncalibrated number never relaxes anything), `harmless` + `small` at 0.95 or more **and** in scope at 0.95 or more. Never a deletion, a move, a path outside the folder, sending, running, an app call, an agent app or a handover. The skip is written on the step (`Skipped a prompt · edit_file · low risk (0.97)`) and in Activity. |
| Labels | `toolgate::ask_person`, `AppNotes::label_refusal`, `approve.rs` | A *No* to a question the **rating** raised writes `right` (`tool-card`). A *No* on an app prompt that carried a line writes `right` for the in-scope decision and `weak_right` for a kind raised to a send (`app-card`); on an agent app's approval `right` (`engine-card`). A yes writes nothing. |
| Seed sets | `tests/eval/decide/tool-risk.jsonl` (52 rows: 28 ratings, 12 asked-for, 12 not), `app-kind.jsonl` (42 rows: 12 read, 12 change, 14 send, 4 unsure) | Written by Claude and unreviewed, like the others. A row asks one question, so the live request with two questions is not what "Calibrate now" measures. |
| `USE-UI-8` | `api.ts` (`reason`, capability `risk`), `PermissionPanel.tsx` (`Reason`, `RiskPrompt`), `.css` | One line under the question on every kind the panel draws (operation, scope, command, task, engine, app), hidden when there is nothing to say. A new card for an added question: "Before I do this", what would happen, why, the arguments, *Go ahead* / *Don't do it*. A tool raised to a send is drawn as any send. |
| Experiments cards | `experiments.rs` | Two new lines under *Fast & slow thinking* (what is asked, and that the start of the call and of the request go to the decision model, the cloud one if chosen) and the new `relax-prompts` card. |

**Verified**

- `cargo test --lib`: **1497 pass, 0 fail**, 17 ignored (was 1457). New: `toolgate` (30), the gate with notes (5), a batch
  target raised to a send and an override seen (2), the relaxed edit in a real folder (1), the two rows (2). The seed
  checks now cover 18 purposes. `cargo check` is clean. `tests/decide` (all ignored) builds.
- `vitest`: **1243 of 1243** (was 1235), 8 of them new (`PermissionPanel.reason.test.tsx`). `tsc --noEmit` is clean.
  `copy_lint` fails only on the `playbackQueue.test.ts` line it failed on before.
- Unit tested: what is asked and what is not (every own tool, trust level, unattended); the state, with long text and many
  arguments cut; the readings and each threshold (0.8 and 0.79, the request before the rating, one question alone);
  `relax-prompts` (a fit, 0.95, the tool list); the note reaching only the right kinds of prompt; a question added and
  answered yes, no, stopped, or unattended; labels (a no writes `right` only for a rating, a send raised writes a weak one);
  a read nobody asked for is asked about, a line never adds a question to a read or removes one from a send; an added question
  unattended is parked; a target raised to a send changes its app's part, the strictest kind and the sentence; and a
  relaxed edit in a real folder goes through while a deletion, a move, another tool and a path outside never do.

**Not verified, and why**

- **No real model has answered.** The rating, the in-scope question and the kind question have never been put to Clef-flash
  or to a cloud model. How often it calls an ordinary command irreversible, whether a 9B model tells "asked for" from "not",
  and the time one look takes are all unknown. **0.8, 0.6, the two line bars (0.5, 0.6) and 0.95 are starting values.**
- **On this computer (graphics card too small) the checks never run while somebody is watching.** `ToolRisk` and `AppKind`
  are hot, so a processor is skipped; only unattended runs (scheduled tasks, background children) reach them, on the
  processor, and wait for it.
- **`dispatch` was not driven end to end.** It needs the whole run state (`ToolContext`), which has no test builder, as in
  steps 11 and 13. Its parts were run. The engine approval (`Port::with_look`) was not run with a decision either: the
  seam has no app in a test.
- **The new card and the lines were not opened in the app.**
- **The seed rows** were not checked by anyone but their author, and one request with two questions was never measured.

**Behaviour changes**

- With *Fast & slow thinking* on and a decision model on the graphics card, every command, mail, browser click or key
  press, and every change to a file that would ask, costs one quick decision before it runs. Every connector call costs
  one in-scope decision and, once a tool a day, one kind decision. They are among the turn's twenty decisions, so a long
  turn stops being looked at.
- A call can now get a question that was not there: an irreversible or unasked-for own tool, and a read from an app that
  the request did not call for. Unattended, those calls are refused or left for you.
- A connector tool that sounds like a read or a change can now be asked about as a send.
- In cloud mode the start of the user's request and of the call's arguments go to OpenRouter (zero retention), as the
  start of a message already does for the tool menu.
- `PermissionRequest` has a `reason`, the panel handles a `"risk"` capability, `Resolved` has `overridden`, `GateEnv` has `notes`.

**Not built from the plan text (decided, not forgotten)**

- **Arguments in the kind question.** The plan says the check reads "the tool name, its description and the arguments", but
  the answer is kept per tool per day and the gate holds a tool to one kind, so the first call's arguments would decide for
  the day. It reads the name and the description. A Composio target has no description here, only its slug.
- **Scope-raised questions write no label.** One request carries both questions and the fit reads the rating (the first
  answer). A *No* to a question raised for "not what you asked for" says nothing about the rating, so none is written. The
  in-scope question has no calibration of its own for own tools; for an app call or an agent app it does, because it is
  asked alone.
- **"Approved without hesitation 20×" as a weak `wrong`** and **the `AppKind` label from a stricter per-tool override**
  (`CTL-3`): neither is written. The first needs a count of fast approvals; the second a join of an override to the
  decision about that tool.
- **Undo on a skipped prompt.** The plan says "with Undo where one exists". I did not look for or wire one: the skip is on
  the step and in Activity, and the change is a normal edit.
- **`relax-prompts` beyond one site.** Only writing or editing one file in the attached folder. Running a project task,
  the browser and mail are not relaxable by this step, and the plan's list of what is never relaxed is kept as a short list
  of what is.
- **A "calibrated backend" as a requirement of the card.** It is checked at the call (a verdict that is not calibrated
  relaxes nothing), not before the switch can be turned on.
- **The lead's turn for agent apps.** The decisions about an agent app's approvals are counted under the child's
  conversation, not the lead's, because the run has no way to name it. Each child gets its own twenty.

**Findings that change later steps**

1. **A tool's own question is raised in six places inside the toolsets**, so threading a reason into each was the wrong
   shape. A task-local note read in `send_permission` did it with one change. A later gate that needs to talk to a prompt
   raised from inside a tool can use the same note.
2. **The question we add and the question the tool raises can both appear** for one call: a command flagged irreversible
   that is not in the person's "always allow" list asks twice. Only a milder doubt avoids it (it is a line). If this is
   felt in use, the added question should be skipped for a call whose own question is certain; the toolsets do not say
   beforehand.
3. **One temperature serves a rating and a yes-or-no question**, and the labels reach only the rating. If the in-scope
   question turns out to be the one that flags most, it should be its own purpose with its own calibration (as it already is
   for app calls and agent apps).
4. **The budget of twenty decisions a turn is shared** with every other purpose. A turn with many commands will stop
   being looked at after the twentieth decision; the plan's "gate purposes draw from the lead's turn" holds, but whether
   twenty is enough for a coding turn is not known.

### Order step 15 - reports and "Reset its data" (`EXP-4`): DONE (2026-10-26)

Each experiment card says what the experiment did in the last 30 days, and can forget what it recorded. The last step of
the order. Nothing in it needs a model: it counts rows that are already kept.

**Built**

| Plan item | Where | Notes |
|---|---|---|
| The report | `experiments_report.rs` (`report`), `commands/experiments.rs` (`experiment_report_cmd`), `db/decisions.rs` (`decision_stats_since`) | One count over the decision log, its firm labels and the tables an experiment owns, for the last 30 days. Answered decisions only (a row a budget skipped is not one). Decisions are kept 90 days, so the window is always there. |
| *Fast & slow thinking* | same | `Held back tools on 214 of 640 turns · brought back on 9 of them (4%) · 1,820 quick decisions all on this computer · 3 corrected or marked wrong`. Held = the tool-menu decisions that acted, brought back = those with a `wrong` label (the model asked, called it, or the chip was clicked). Cloud decisions read `1,820 quick decisions, 40 in the cloud (under $0.01)`. The purposes counted are the ones whose row names the experiment, as the Activity chips read them. |
| *Verify answers* | same, `db/verifications.rs` | `Checked 31 answers · 412 claims · 19 refuted · you rated 6 verdicts, 1 wrong`, from the stored checks. |
| *Mail triage* | same | `Triaged 640 mails · you retagged 22 (3%)`: answered mail decisions, and the ones with a retag that changed the kind or removed it. A retag to the same kind is a confirmation and is not counted. |
| *Let quick decisions skip low-risk prompts* | same | `Skipped 4 questions`, counted from the Activity rows the skip writes (`toolgate::SKIPPED`). |
| *Workspace mode* | same | `12 Workspace chats used` (chats with the mode set and used in the window). |
| "Reset its data" | `experiments_report.rs` (`reset`), `experiment_reset_cmd`, `Experiments.tsx` | Asks first (`ConfirmDialog`): how many records go, what the card says it records, that chats and settings are untouched, that the switch stays, and, on a card that decides, that the measured calibration is kept. It deletes the decisions of the experiment's purposes (their labels go with them) and the experiment's own tables: the stored checks for *Verify*, the inbox rows and the loop's remembered newest message for *Mail triage* (so the inbox fills again), the skip rows in Activity for *relax-prompts*. The button is hidden when there is nothing to forget, so Workspace never has one. |
| The card | `Experiments.tsx`, `api.ts` | A `Last 30 days` row on every card, with the line or `Nothing recorded in the last 30 days.`. Left out without a word if the backend cannot answer. Read again when the switch changes. The existing `Show them in Activity` link is the plan's "Details". |

**Verified**

- `cargo test --lib`: **1509 pass, 0 fail**, 17 ignored (was 1497). New: 12 in `experiments_report` (every report, the 30-day
  window, a skipped row not counted, a cloud decision and its cost, retag against triaged, claims and marks, only the skip rows in
  Activity, the reset of each kind, an unknown id, a mail reset that clears the loop's memory). `cargo check` has no warnings.
- `vitest`: **1252 of 1252** (was 1243), 9 of them new (`Experiments.report.test.tsx`). `tsc --noEmit` is clean.

**Not verified, and why**

- **The numbers were only counted on made-up rows.** There is no real decision log on this computer (no real model has ever
  answered), so no report has been read against real use.
- **The card was not opened in the app.** Layout of the new row and the dialog is covered by jsdom only.
- **The reset was not run on a real database file**, only in-memory ones.

**Behaviour changes**

- A card has one more row and, when there is data, a link that deletes it after a confirm. Nothing else changes.
- Resetting *Fast & slow thinking* does not remove its calibration, so it does not send the quick decisions back to their
  careful setting.

**Not built from the plan text (decided, not forgotten)**

- **"0.6 s faster to first token".** No turn records what it would have cost without the experiment, so the number cannot
  be counted. It would need an A/B, which is not what this report is.
- **"6 corrections accepted" for *Verify*.** Revise writes a new answer; whether it was kept is not recorded.
- **A computed graduation check.** The gate is the sentence on the card (`graduation`) and the report puts the number
  beside it, but whether a gate is met is read by eye. Graduating is a code change, as the plan says. Several gates need
  things no count has (the tool-menu ablation, the golden set, `CTL-T-5`).
- **A percentage of corrections for every decision.** Only the tool menu and mail give a denominator that means something
  (held turns, mails triaged). Other purposes are rated mostly when they are wrong, so a rate of rated decisions would
  look worse than it is; the card gives the count.
- **A longer window or a chart.** 30 days, one line.

**Findings**

1. **`HandoverRoute` belongs to no experiment in the log.** Its table row has no experiment (the live policy adds
   `fast-slow`), so the Activity chips and this report leave its decisions out of *Fast & slow thinking*, and its reset
   does not remove them. Small, and the same on both screens, so left alone; fixing it means giving the decision row the
   experiment of the policy it was asked under.
2. **Decisions older than 90 days are gone, and mail rows older than 30**, so *Mail triage* can say a month and no more.
3. **A reset of *Mail triage* with the loop on is followed by a fresh fetch** of the newest 200 mails and a new run of
   checks, which on a processor-only computer takes hours (step 10 finding). It is stated nowhere on the dialog.

---

### Order step 16 - Verify folded into `fast-slow`, `sureWord` (`SEE-5` + `SEE-4` + `SEE-T-8`/`T-9`): DONE (2026-10-26)

**Built**

| Plan item | Where | Notes |
|---|---|---|
| `verify` leaves the registry | `experiments.rs` | `Requirement::Evidence` dropped. `fast-slow`'s `what_changes` has the two new lines, its `records` and `graduation` absorbed Verify's. |
| Migration | `experiments.rs` (`fold_verify`), `db/mod.rs` (`delete_setting`) | Own marker `experiments.verify-folded`, because `migrate` already ran once on every computer. `experiment.verify` on turns `fast-slow` on; the key is deleted either way. |
| Gating | `commands/verify.rs` (`allowed`), `policy.rs` | `verify_message_cmd` and `verify_list_cmd` gate on `fast-slow`. `ClaimVerdict` row names `fast-slow`. |
| Report and reset | `experiments_report.rs` | `fast-slow` report adds `checked n answers · m contradicted` (and the rating line). Reset also clears `verifications`. |
| `sureWord` | `src/lib/decisions.ts` (`sureWord`, `says`), `agent/decide/words.rs` (`sure_word`, `say`) | Sure from 0.85 fitted, 0.9 unfitted; probably from 0.6; below that nothing. Two copies, equal by their tests. |
| Surfaces | see below | Numbers moved to hovers. |

SEE-4 surfaces: steer tag unchanged; plan tick `Looks finished from my answer` (number in title, `PlanCard.tsx`); task skip row `Skipped: the condition did not hold` (`runif::skipped_note`, number in the row's title, `Tasks.tsx`); prompt reason `Flagged: this may not be what you asked for` with the number in a new `PermissionRequest.reason_hover` (`toolgate::Flag` got `calibrated`, `Upgrade` too, `Note.hover`, `hover_for`); handover `Matches your rule U2` / `Probably matches...` (`engines.ts ruleLine`, hover on the card line), doubt line `I'm not sure this matches U2.` (Activity keeps the number); goal check `not yet` / `met` with the number in the title (`RunBar.tsx`); mail hovers `Looks urgent` / `Probably urgent` (`inbox.ts`). Classify keeps its column. The `Timeline.tsx` untrusted-flag line still shows `(0.xx)`; it is not in the plan's list.

The action on an answer is now **Check more deeply** (constant label, hover says the words leave this computer). The `ClaimCheckable` policy row was NOT added here: it belongs to step 18 with its purpose.

**Verified**: `cargo test --lib` all passed after fixes (about 1540). `vitest` 1260 pass (the Workspace session-menu test failed once under load and passes alone). `tsc --noEmit` clean. New tests: `words` (2), `toolgate` hover/words, `experiments` fold (3), `commands::verify` gate, report+reset, `decisions.test` sureWord, inbox, engines, PermissionPanel reason, VerifyPanel follows fast-slow.

**Decisions / deviations**: no emoji anywhere (AGENTS.md): the plan's lightning glyph will be a small SVG bolt. The plan says "next schema version" after v39; the schema was v40, so it is now **v41** (see step 17).

### Order step 17 - the first-thought strip and the receipt (`SEE-2`): DONE (2026-10-26)

**Design chosen** (so the next session does not re-derive it)
- Backend sends `AgentEvent::Thought { run_id, thought }`, the whole `Thought` each time (`agent/decide/thought.rs`): `looking` (places in words), `direct`, `left_out` (only units the user chose: web_search, indexing, apps, mcp:*; with `words` and the click's `action`), `held` (id + measured spec tokens), `handover {agent, rule}`, `ms`, `engine` (`cold` | `unavailable`, for the once-per-conversation line). Not sent while empty.
- `rc.run.thought` (new `RunHandle.thought`) holds it; `run.rs publish_thought` mutates and emits. Callers: `narrow_menu` (menu part, engine missing), `prematch_turn` (adds ms), `subagents::act_on_prematch` (handover, before the card).
- Receipt rows come from the decision log. New column `decisions.note` (schema **v41**, also adds `verifications.mode` for step 18, default `deep`), `Db::set_decision_note`, `Db::decisions_of_run`, `decide::note(&verdict, text)`. Notes written at: tool menu (`thought::menu_note`), handover match, plan tick, steer read/stop, unverified-claim skip, tool gate (asked/flagged, app calls, kind upgrade), untrusted flag, browser stop. A decision with a note "acted or was shown"; answered decisions without one are "n more that changed nothing".

**Built (the rest of step 17)**

| Plan item | Where | Notes |
|---|---|---|
| Receipt query | `db/decisions.rs` `decisions_of_turn(conversation, run, from, to)` (replaces `decisions_of_run`), `commands/decide.rs` `run_decisions_cmd` | The run's own decisions plus those with no run id made in this chat while the turn ran; only answered ones; background purposes are left out. |
| Label | `receipt_wrong_cmd(decision_id, value)` | `wrong`, source `receipt` (or `receipt:<unit>` so each group is one label). For the tool-menu row the unit goes in `value`, as calibration reads it. |
| Start | `decide_start_cmd` | `resolve_local` + `ensure_started`; says "Install a decision model first." with none. |
| Types and calls | `api.ts` | `Thought`, `LeftOut`, event `thought`, `runDecisions`, `receiptWrong`, `decideStart`, `DecisionEntry.note`. |
| Strip and receipt words | `lib/thought.ts` | `stripParts`, `engineLine`, `tokensShorter`, `receiptOf`, `emptyThought`, `TurnThought`. |
| Store | `store.ts` | `Message.thought` (in memory). Empty record at `run_started`; `thought` event fills it; `tools_back` and `restoreHeldTools(conv, units?)` keep `back` in step; the engine line is kept on the first turn that said it and dropped from later ones; `run_ended` reads the log; `markReceiptWrong`. |
| Screen | `Conversation/ThoughtStrip.tsx` (`ThoughtStrip`, `ThoughtReceipt`), css in `Conversation.css`, `BoltIcon` in `Icons.tsx`, `AgentRun.tsx` | Strip above the timeline while streaming, receipt at the foot when done, never both. One line, no height change. |
| USE-UI-1 removed | `RunBar.tsx` (`heldSegment` and its segment), `RunBar.held.test.tsx` deleted | `store.toolsheld.test.ts` stays: the events still feed the strip. |

Receipt details: `n quick decisions` is the count of answered decisions in the log for the turn; `prompt N tokens shorter` only when held groups stayed out (`Thought.held`, measured from the specs); `n sentences checked` is NOT there yet (step 18). Rows are the decisions with a note. The hover is backend, model, ms and `sure 0.xx`. A plan-tick row can offer **Reopen the step** after it was marked wrong, parsed from the note `plan step N` (kept in step with `loopctl.rs` by `thought.test.ts`).

**Verified**: `cargo test --lib` 1526 passed, 17 ignored (before the new commands), the new `thought.rs` (5) and `db::decisions` turn test pass, `cargo check --tests` clean after the commands. `vitest` 1291 passed (121 files), `tsc --noEmit` clean. New tests: `lib/thought.test.ts` (15), `lib/store.thought.test.ts` (8), `Conversation/ThoughtStrip.test.tsx` (15).

**Not done / notes**
- Not run in the real app: no model has answered a decision here. The strip's look is untested by eye (SEE-T-10, step 20).
- The undo for "Left out ..." after the run ended does not exist (the run is over); it is a label only. While the run is live the strip's click is the undo.
- Steer rows and other notes have no undo in the receipt.
- The `Timeline.tsx` untrusted-flag line still shows a number; not in the plan's list.

### Order step 18 - answers check themselves (`SEE-1` + `SEE-UI-1` + `SEE-T-1`..`T-4`): DONE (2026-10-26)

**Built**

| Plan item | Where | Notes |
|---|---|---|
| Sentence splitter, finality, 40 cap | `agent/sentences.rs` (`split`, `finals`, `MAX_SENTENCES`) | Skips code blocks (an open one too), headings, tables, quotes, rules, questions, lead-ins ending in a colon and anything under three words. A list item is one sentence. A sentence is final once the text moved past it (another sentence began, or a line break followed a terminator). Text only grows, so the sentences already final never change (tested at every prefix of an answer). |
| `ClaimCheckable` | `decide/checkable.rs`, `policy.rs` (variant, `ALL` 19, name, label, below, row), `seed.rs`, `tests/eval/decide/claim-checkable.jsonl` (40 rows) | One request, two questions about one sentence: `checkable` (yes or no, bar 0.85 on it) and `kind` (`fact` or `did`). A sentence counts as "I did this" only at 0.85 on `did`, because a wrong "did" is a red mark on a true sentence. The seed rows name the question they ask, like `ToolRisk`'s. |
| Order in the engine's queue | `decide/lane.rs`, `decide/mod.rs` (`ask_under`), `Purpose::is_check` | A hot ask (`needs_fast`) holds a guard from the moment it is made. A check ask (`ClaimCheckable`, `ClaimVerdict`) takes the check lock (one at a time, in order) and does not send until no hot ask is in flight or waiting. A hot ask never waits for the lock. |
| Budgets | `control.rs` (`CHECK_SENTENCES` 40, `Limit::Sentences`) | Sentence asks are outside the turn's twenty, counted per check like claims. |
| Evidence pool | `agent/autocheck.rs` (`Pool`, `evidence_via`, `Db::session_events_for_run_after`) | This run's own `tool_call` and `tool_result` rows, read as the log grows. Sources come from the `<untrusted ... source=...>` envelopes the tools already wrote, else from the call. A result the scan marked risky (`risk` 2 or more) is left out. Failed calls and tools that read nothing add nothing. No search and no fetch. |
| Fact against what was read | `autocheck.rs` (`pick`, `similarity`, `Check::fact`) | Similarity is the share of the sentence's distinctive words (four letters or more, or a digit) found in the passage's best window; two best passages at 0.5 or more. None: "Nothing I read for this answer backs it", not asked. Else the existing `ClaimVerdict` ask. |
| Action claims | `autocheck.rs` (`families_of`, `RunDid`) | First-person past sentences only ("I sent", "I have created and ran"); a plan, an offer, a denial or a thought is not one. Verbs map to families Send, Write, Run, Search (web or not), Read, matched against this run's calls that came back without an error. A connector call counts for Send, Write, Read and Search (judged generously so a true sentence is never marked). No match: red, "No step in this run did this." |
| `UnverifiedClaim` as a mark | `autocheck.rs` (`works_claim`), `ledger.rs` (`unchecked_edits`) | A sentence that says the change works, passes or was tested, while files changed and no check ran after, is marked "Files changed and no check ran afterwards." It lands on that sentence only. The decision that nudges the run (`unverified_note`) is unchanged. |
| Escalation | `autocheck.rs` (`AUTO_REVIEWS` 2), `commands/verify.rs` (`verify_review_cmd`) | The slow review starts by itself only for a fact the quick verdict refuted at 0.85 or more, two an answer. Every other mark has **Review this** in its card; it reads the claim against the same passages without the quick verdict. A mark matched against the run's steps has none. |
| Nothing read | `autocheck.rs` (`Summary.factual`) | With an empty pool no fact is asked about and nothing is marked. Only a sentence that says "I did this" is still asked, because the run's steps are its evidence. The trust line says "Answered from what I know · nothing checked" only when some sentence read like a fact (a number, or a name mid-sentence); a reply with none shows nothing. |
| The run feeds it | `run.rs` (`feed_check`, `finish_check`, start in `run_agent`), `fleet.rs` (`RunHandle.check`) | The answer's text goes to a worker as it grows (the splitter runs only when new text holds a stop or a line break). Started for a top-level chat run with a message row, not for a child or an unattended run. It ends with the answer, or is dropped when the run was stopped or failed. |
| Storage | `db/verifications.rs`, schema **v42** (`verifications.summary`; `mode` from v41), `Db::save_verification` | `auto` or `deep`, one row a message. A deep check replaces an auto one; an auto one never replaces a deep one. The row is not kept when the decision model never answered, or there was nothing to say. |
| To the screen | event `poiesis-autocheck` (`progress`, `claim`, `done`), `lib/store.ts` (`applyAutoCheck`) | An app event, not the run's channel, because the check outlives the run. The answer is found by its run id (`Message.thought.runId`) because until the turn is finalized its id is the optimistic one; the entry follows the id swap. A deep check the person asked for is never overwritten. |
| Sweep, marks, trust line | `VerifyPanel.tsx`, `lib/verify.ts` (`trustSegments`, `marksOf` with `sweeping`, `answeredFromKnowledge`), `Verify.css` | Backed sentences are drawn dotted green for 1.5 s as their check arrives, then settle to plain text; under reduced motion they are not drawn; **Underline supported** keeps them. Red: contradicts, and "said done, no step did it" (with a small flag). Amber dotted: nothing backs it. Amber solid: sources disagree. The trust line is one row with a fixed minimum height: `Checking against what I read · 4 of 9`, then `✓ 7 backed by what I read · 1 nothing backs it · ✕ 1 contradicts nytimes.com`. Each part filters the claim list, which stays closed until a part is picked. **Revise** sits at the right end. A sentence that runs across bold or a link is marked from its start, as far as it goes in one piece. |
| **Check against the web** | `VerifyPanel.tsx` | On the nothing-read line, only while Web search is on (read when the line shows). It runs the deep check. **Check more deeply** is unchanged. |
| Receipt part | `lib/thought.ts` (`receiptOf(t, plan, check)`), `ThoughtStrip.tsx` | `· 9 sentences checked`, and one row `Checked 9 sentences against 4 sources, 1 contradicted` with no button. The sentence questions are not counted as "quick decisions". A mark that changed something (a sentence no step did, a contradiction) is still a row with **That was wrong**. |

**Added beyond the plan**
- `Policy.stay_local`, `Purpose::auto_policy`, `decide::ask_auto`: the automatic check **stays on this computer** even when the decision backend is the cloud, because the sentences are the answer's own words and a local answer must not be sent out by a checker. Only the purpose's own override by name sends it out. The Advanced table shows "Allow the cloud" for it (`PolicyRow.local_only`). The deep check reads the ordinary row, as before.
- `ClaimCheckable` rows are folded by the hour on the Activity screen (`is_bulk`), like `Classify`: one answer asks dozens.
- `verify::revise_request` now also names a sentence no step did.

**Verified**: `cargo test --lib` 1566 passed, 17 ignored; `vitest` 1333 passed (123 files); `tsc --noEmit` clean. New tests: `sentences` (8), `decide::checkable` (4), `decide::lane` (3), `control` and `policy` rows, `autocheck` (20: pool from a log, flagged results fenced off, only this run's rows, the phrase table, matching against steps, passage threshold, backed and unbacked, the two-review limit, sure refutation only, action claims, `UnverifiedClaim` on the right sentence, nothing read, a silent model, deep never replaced), `ledger`, `verify::revise_request`, `lib/verify.test.ts` (trust line, sweep, prefix marking), `lib/store.autocheck.test.ts` (7), `Conversation/AutoCheck.test.tsx` (18), `lib/thought.test.ts` and `ThoughtStrip.test.tsx` (the sentences part).

**Not done / notes**
- **Not run in the real app.** No model has answered a `ClaimCheckable` ask, so the 0.85 bars, the 0.5 similarity floor and the phrase table are starting values; the 40-row seed set is unreviewed and measures one question per row. The sweep, the trust line's look and the fixed-height row are untested by eye (`SEE-T-10`, step 20).
- The check starts when the answer has final sentences, but `decide` waits for a cold engine to start for this purpose (it is not hot); the engine is warmed at the turn's start, so a cold card pays once.
- Marks are placed by the sentence's plain text; a sentence the model rewrites between the stream and the stored text is not marked.
- `UnverifiedClaim`'s own decision is not what makes the mark: the mark comes from the ledger and the sentence's words, so it needs no engine.
- `decide::begin_check` resets the claim and review counts of the chat, which a deep check running at the same moment shares.

### Order step 19 (`SEE-3`, `SEE-T-7`): DONE (2026-10-26)

The composer reads the message 400 ms after the last keystroke and shows at most one line before the send button.

| Piece | Where |
|---|---|
| Typing row: same question and fit, this computer only (even against a purpose's own cloud override), hot, skipped when the engine is not warm, 500 ms, no turn budget (asked with no conversation) | `Policy.typing`, `Purpose::typing_policy`, `decide::ask_typing`, check in `ask_under` |
| The three readings, the 0.85 bar, the label rule, the order (`Reader`, `first`) and the kept answers for the send (`remember`, `reuse`) | `decide/typing.rs` |
| `composer_hint_cmd` (warm engine, experiment on, 12+ characters, not `/`), `composer_hint_label_cmd` | `commands/decide.rs`, `lib.rs` |
| Per-message choices that the actions set: `RunOptions.web` (`ToolRegistry::with_web`, the setting untouched) and `RunOptions.no_handover` (no match, the lead told in fixed words, `handover::LINE_YOURSELF`) | `agent/run.rs` |
| Send reuses the handover answer given while typing (same state, same question ids, once, 2 minutes) | `prematch_turn` |
| Line, hook, chips (`Web search on`, `Do it yourself`, `As an addition`, none a default), `steerActiveRun(text, {asAddition})` | `Composer/ComposerHintLine.tsx`, `useComposerHint.ts`, `Composer.tsx`, `ModifierChips.tsx`, `store.ts` |
| Runtime's quick decisions card says in one line that hints are off in the cloud | `DecisionModel.tsx` |

Order of hints: a message to a working run is read only as a stop (it goes to that run, so a handover or a tool group is not its business); a new message is read for a handover, then for the web. An ask that is not needed is not made.

**Verified:** Rust 1584 passed (17 ignored), vitest 1366 passed, `tsc` clean. New: 12 Rust tests in `typing.rs`, one in `policy.rs`, `mod.rs` (typed words never reach the cloud), `run.rs` (web for one message, options default off), `handover.rs`; `useComposerHint.test.tsx` (15), `Composer.hint.test.tsx` (16), two in `CloudDecisions.test.tsx`.

**Not verified:**
- **No model has answered these asks.** The 0.85 bar is on the raw answer (a hint changes nothing, so it does not wait for a fit). Whether a real model is right at that bar is step 20.
- **Height is CSS** (`.composer-hint`: one 20px line, `nowrap`, ellipsis, a flex item of the box's row). jsdom does not run CSS, so the "box never changes height" rule is checked only as DOM structure. Look at it in the app.
- **The input narrows** by the width of the line when a hint shows (up to 46%). The box does not grow, but the text can scroll.
- `vitest` here does not load CSS, so `css-conventions.test.ts` reads empty sheets and checks nothing.

**Added beyond the plan:** `Policy.typing` (a hard no to the cloud for typed words); the `As an addition` chip, because "Add it instead" needed a per-message choice that the steer path could carry; both `web` and `no_handover` options on the run.

**Different from the plan's words:** the stop line says `This reads as a stop` (the send asks first only with a fit behind it, so `will stop` was not always true) and the handover line says `may take this`, not `will take this` (a suggest-mode rule raises a card first). Say if you want the plan's words back.

**Not done:** reuse at send for the tool menu. The composer asks about the Web search group only, the send asks about every group on the run's menu, so the two questions differ and the send asks again.

### Review of steps 16-19 and its fixes (2026-10-27)

A review of steps 16-19 found these, all fixed with tests:

1. **The composer hint never showed with a cloud decision model.** `composer_hint_cmd` refused
   unless the local engine was warm, `typing_policy` pinned the asks to this computer, and
   `ask_under` refused the cloud for typed words. All three are gone: hints follow the backend
   choice (settled decision 16 revised). A cold local engine is started for next time and says
   nothing now, as every hot ask does.
2. **The automatic check was pinned local too** (`Policy.stay_local`, `auto_policy`, `ask_auto`),
   so a computer with only a cloud decision model had no check at all. Removed; the check follows
   the backend choice (decision 17 revised). The Advanced table's "Allow the cloud" cell for it is
   gone with `PolicyRow.local_only`.
3. **The strip said "the decision model isn't running" when it was running on the processor.** A hot
   ask on a processor engine now fails as `DecideError::Processor`, the strip gets `engine: "cpu"`,
   and each state has its own words: `cold` "Quick thinking is starting ... [Start]", `cpu` "Quick
   thinking is off for this: the decision model runs on the processor, too slow to wait for", none
   set up "Quick thinking is off: no decision model is set up". The last two have **Choose a decision
   model**, which opens Models on its decision tab, where a cloud one can be picked.
4. **Every typing pause wrote a decision row with the first 200 characters of the draft.**
   `record_verdict` now holds typing answers in memory (`typing::hold`, two minutes, 64 at most).
   `composer_hint_label_cmd` writes one when a hint is taken or put away, with the preview "A message
   being typed (its words are not kept)" and no conversation; the send's reuse writes it with the
   turn (`typing::log_sent`), where the receipt and its settle find it.
5. **"Files changed and no check ran" marked true descriptions red** ("works by caching", "passes the
   token"). `works_claim` now needs "tested"/"verified", a pass word with a test, check or build, or
   works/fixed/resolved as a result ("now", "is fixed", "I fixed", "it's working").
6. **An auto check and a deep check in the same chat shared their counts.** `Opts::count_key` counts the
   automatic check (which carries its run) under its own key; `decide::begin_auto_check` resets only it.
7. **The receipt vanished when you switched chats and came back**, because opening a chat reloads its
   messages and `Message.thought` lives in memory. `keepThoughts` keeps this session's thoughts on the
   reloaded messages. A restart still has none (they were never stored).
8. **The untrusted-text line in the Timeline showed a number.** Words on the line, the number on its hover.

Still open from the plan: reuse at send for the tool-menu answer, and the undo for a left-out group
after the run ended (both listed under step 17 and 19).

### Order step 20: NOT STARTED

- **20** (`SEE-T-10`): Erich's own real run with a decision model, local on a graphics card or in the
  cloud. Cannot be done by Claude.

**Tooling note for the next session**: in this environment a doubled backslash in a Python heredoc arrives as one, so `\\n` in a replacement string became a real newline. Use the Edit tool for any text with backslashes, and read the result. Files are LF in the working tree for the ones Python touched; git normalises (`autocrlf=true`).

---

## Implementation status: finished

All sixteen Order entries (0-15) are built except step 0 (commit the CLI_ENGINES review fixes), which is a git action
and has not been asked for. **No real decision model and no OpenRouter key has ever answered a question**, so every
threshold in the policy table is a starting value, and the first real check is the Calibrate, Test and seed-set runs
listed under each step's "Not verified".

### Review fixes (2026-10-26)

A review of steps 1-15 found three bugs, now fixed with tests (Rust 1514 passed, frontend 1252 passed):

1. **A "no" to an added question did not hold for a retry.** The controller refuses the same question about
   the same state twice in a turn, so the same call again went through unlooked-at. `toolgate` now keeps
   each look's answer until the person's next turn: an own tool the person declined is refused again with the
   same words, and any other repeat (own tool or app call) gets what the first call got.
2. **An answer check and the chat turn shared their counts.** A message sent during a check reset its 4
   reviews. A check now has its own counts (`control::begin_check`), and starting one leaves the turn's alone.
3. **A mail reset could race a running look at the inbox**, which would write rows back. The reset now holds
   the loop (`mail_sync::hold`) and says "Your inbox is being checked right now" if a look is running.

The em-dashes in on-screen text added by this plan were replaced (`◇ from outside: ...`, `This computer ·
Clef-flash`, "not judged" in a sorted list).

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

> **v3 (2026-10-26):** items 1-12 were written before anyone used them, and
> the first real test showed items 1 and 4 failing this section's own rule.
> v3 adds items 0a-0c and rewrites 1 and 4. All numbers in on-screen text
> become words (`SEE-4`).

**0a. "Its answers check themselves."** Mara asks for the opening hours of
three museums. The assistant searches and answers. A moment after each
sentence appears, a faint green line passes under it and fades. Under one
sentence it stays red. Below the answer: `✓ 5 backed by what I read · ✕ 1
contradicts museum-site.de`. A click on the red one shows the page saying
Mondays closed. She didn't click anything to start this. In a coding chat,
*"I ran the tests and they pass"* turns red with ⚑ *"No step in this run did
this."* *(SEE-1)*

**0b. "I can see it think fast, then slow."** The moment Mara sends "what's on
my calendar tomorrow?", before any answer text, one line appears: `⚡ Looking
in Google Calendar · 38 ms`. Then the answer streams under it. When it's
done, the line has moved to the bottom: `⚡ 4 quick decisions · prompt 2,900
tokens shorter`. A click lists them, each with **That was wrong**. *(SEE-2)*

**0c. "It knew before I sent it."** Erich types "the voice tests fail, fix
them". Before he presses Enter, the composer reads `→ Claude Code will take
this (your rule U2)`, with **Do it yourself**. *(SEE-3)*

**1. "My small local model stopped flailing."** Mara runs Qwen 4B locally and
asks "what's on my calendar tomorrow?". Her Google Calendar is connected under
Apps. Today the model sees thirty tool specs, the Composio tools, and the
`delegate` tool with the Claude Code rules in its description, and it
sometimes searches the web first. With the experiment on, the strip above
the answer says `⚡ Looking in Google Calendar · not searching the web`
(she has Web search on, so leaving it out is worth saying). The model goes
straight to her calendar, and the first token arrives sooner because the
prompt is much shorter. If she wanted the web, **Search the web too** is one
click. When the model needs a held-back tool later, it asks for it, and
nothing breaks. *(USE-4, SEE-2; the v2 chip `5 tools held back` was removed
in v3)*

**2. "Monday inbox in one glance."** Forty new mails overnight. Settings → Mail
now has an **Inbox** tab: the newest mail per account, fetched every 15
minutes while Poiesis runs. Each row has a thin urgency bar on its left edge
and a tag (`work`, `receipt`, `newsletter`). A "Needs reply" filter shows the
six that do. Nothing was moved or deleted, and nothing left the computer. Mara
retags one newsletter as `work`, and that correction becomes a label the
system calibrates against. *(USE-6, USE-6a, CTL-3)*

**3. "Sort these for me."** "Here are 140 photos from the trip. Tag each with
the city." The assistant calls `classify` once. Clef reads every image,
returns a table block with city, confidence, and a "low confidence (12)"
filter, and the LLM writes only the summary. That takes about twenty seconds
instead of the minutes 140 generated captions would cost. The same works for
"which of these 60 files touch auth?" or "which tickets are bugs?". *(USE-9)*

**4. "Check that more deeply."** An answer from memory says `Answered from
what I know · nothing checked`. Mara clicks **Check against the web** (or
**Check more deeply** on any answer). The claims are looked up on the web,
underlined the same way as 0a, and the two disputed ones get a slow review.
*(USE-10, now part of `fast-slow`)*

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
conversation (0.91)`. The Slack panel for *"Post to Slack (acme): #general"*
says `You asked for #releases, not #general (0.88)`. When Claude Code asks to
run `git push`, its panel says `Not part of the task you handed over (0.84)`.
A prompt that trust rules would have skipped but the judge added says
`Asked because this looked irreversible`. A Notion tool called
`notion-sync-page` that the name rules took for a harmless change is asked as
a send: `Asked because this looks like it sends something (0.81)`. *(USE-12)*

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

**11. "It knew that was a Claude Code job, and why."** Erich asks "the voice
tests fail, fix them". Before the lead's first token, the decision model has
already matched the request to his rule *U2 Running the tests or the build and
fixing what fails* (0.93). The handover card appears straight away with that
rule, instead of after a lead turn that reasons its way there. When he asks
"what does this function do?", the match is *A1 Explaining or reviewing code*,
so the lead is told the user's own rule says not to hand it over. *Try a
request* in Settings answers in 40 ms instead of a chat call. *(USE-13, DEC-6)*

**12. "The goal check says how sure it is."** Each `/goal` round ends with a
quick decision: *does the goal hold?* The Run bar reads `not yet (0.12)` or
`met (0.91)`, and a model answering in broken JSON no longer counts as a
silent "not yet". The slow pass only writes the one line that needs words: the
evidence when met, the next step when not. *(DEC-6)*

---

## Where Poiesis stands

### DPAR components that already exist

| DPAR component | In Poiesis today | What this plan adds |
|---|---|---|
| Slow deliberation plane | `drive_turn`, plans, subagents | nothing |
| Fast decision plane | **missing** | `DEC-*` |
| Evidence plane | `retrieval.rs`, rerank, web, check runs, `unverified_note` | used as an escalation target (`CTL-2`) |
| Deterministic action gate | trust levels, impact classes, permission prompts; **connector gate** (`agent/connector_gate.rs`: read / change / send by name, per-tool overrides, "always" rules, daily send cap); **agent-app approvals** (`engines/approve.rs`, one panel); **engine guards** (RTE-3); **capability truth** (`Toolset::ready`, persona app grants) | a reason line, an in-scope line, and the ability to *add* prompts (`USE-12`). Never a replacement for any of these. |
| Budgets / stop limits | harness budgets; goal-loop stall guards (LRN-1) | a decision budget per turn (`CTL-2`) |
| Trace log | session log (`agent/log.rs`); connector activity rows with who decided (MCG-6) | `decision` rows (`DEC-7`) |
| Outcome memory | reflection, golden evals; `engine_signals` (handover overrides, `/delegate` asks); `app_call_log`; `Goal.stalled` | outcome labels from user actions (`CTL-3`), read from those tables first |
| Metacognitive controller | scattered heuristics | one policy table (`CTL-1`) |

### Small questions asked badly today

| Site | Question | Where |
|---|---|---|
| RET-4 | Do these excerpts answer the query? | `agent/retrieval.rs` `judges_sufficient` |
| SCP-1 | Does this fact apply to every answer? | `agent/memory_skill.rs` `classify_scope` |
| Reflect critic | Is this lesson supported, actionable, right? | `commands/reflect.rs` |
| GOL check | Does the goal hold now? (plus one line of evidence or next step) | `agent/goal.rs` `check_messages` / `parse_check` |
| RTE-7 preview | Would one of these agent apps get this request, by which rule? | `commands/engines.rs` `engines_route_preview_cmd` |

All of them use `drive_turn`, temperature 0, prose or JSON back, a lenient
parse, and a fallback answer on anything odd. The first three return `None`;
the goal check returns "not yet"; the preview returns nothing.

### Questions nobody asks

- Which toolsets, connected apps, connectors and agent types does this turn
  need? (`run.rs` advertises every one that is ready and granted.)
- Does this request match one of the user's handover rules? (Only the lead
  decides, after reading every rule block in the `delegate` spec.)
- Does this app tool send something, although its name says it doesn't?
  (`connector_gate::classify` goes by name; LRN-2 notes that "a send with an
  innocent name is missed".)
- Did the answer finish plan item N? (`nudge_unfinished_plan` goes by plan state.)
- Is this steering message a stop, a redirect, or an addition? (`fleet.rs` queues text.)
- Is this conversation worth a reflection pass?
- Is this mail urgent, and does it need a reply? (`mail.rs` has no triage,
  no background fetch, and there is no inbox list in the app.)
- Is this tool call risky, and is it what the user asked for? Is this command
  an agent app asks to run part of the task it was handed?
- Did that browser click work? (The browser agent has no visual check.)

### Shell facts to build against

- `manifest.rs` pinned `llama-server` at `b10333` (released 9 August), which
  has **neither** `/v1/systemone` **nor** Clef. "A pin is a ceiling on which
  models can run." **Since 2026-10-23 the pin is `b11418`** (`DEC-1`, done).
- llama.cpp merged `/v1/systemone` (PR #29818, 2026-10-02) and Clef text-only
  (PR #29831, commit `99b9548`, merged 2026-10-03). The conversion script
  (`clef.py`) maps the joint-head tensors *into* the GGUF. Clef vision is
  PR #29969, **merged 2026-10-05** (`9871df5`). The first tag containing all
  three is `b11418`. Checked against GitHub on 2026-10-23.
- The runtime also provisions the **image engine** (`runtime/imageengine.rs`,
  `sd.cpp`, ~4 GB for the default SD 1.5) and in-process **voice** models
  (sherpa-onnx). `DEC-4` placement has to count them.
- **A call the user did not ask for never goes to the cloud.** This is already
  the rule in three places: `ToolContext::local_endpoint` ("does without rather
  than falling back to the cloud"), `engines/learn.rs` (rule proposals are
  worded by the local model only), and `memory_skill.rs`. Settled decision 12
  applies it to background decisions.
- **What `run.rs` offers a turn today**, in order: the persona's and the
  delegation's narrowing, then `Toolset::ready` (LRN-5: Mail without an account
  and Image generation without a backend are left off), then MCP connector
  tools (`registry.mcp`), the Composio meta-tools (CAP-4, only for granted apps,
  CAP-10), and `delegate` whose description renders one rule block per agent
  app (RTE-4, `subagents::tool_specs(types)`). All tool specs are outside the
  prompt-assembly golden gate.
- **Approvals that already exist and that `USE-12` must not duplicate:** the
  `app` capability (MCG-3: *Allow once / Always for {app} changes / No*, and
  *Send once / Up to N a day / No*), the `engine` approval from Claude Code,
  Codex and OpenCode (CLI-6, argv one token per line), and the `handover` card
  (RTE-4). All use `PermissionRequest` and the same panel.
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
- Schema is **v36** (v31 engines, v35 `engine_signals`, v36 `engines.app_tools`).
  This plan's first migration is v37.
- **Label data that already exists:**
  - `engine_signals` (`override` | `asked` | `checked`): *Do it yourself* on a
    handover card, *This shouldn't have gone to …*, and `/delegate <agent>`
    for a kind of task the lead kept.
  - `engine_rules.hits` / `overrides` per rule.
  - `app_call_log`: every app tool that ran, with its kind; MCG-7 per-tool
    overrides in `connector_policy.tool_overrides` (a person reclassifying a
    tool).
  - The connector activity rows say who decided an approval (`once`,
    `always`, `cap`, `headless-refused`).
  - `Goal.stalled` / goal status (`met`, `stalled`, `exhausted`), held on the
    frontend (`src/lib/goal.ts`).

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
   the five existing sites) is the exception: it is a pure, invisible
   replacement gated by parity tests.
10. **Cloud model choice is live, not hard-coded.** The cloud model list comes
    from OpenRouter's catalog at runtime. We pin the `canonical_slug` the user
    picked and log the `model` each response reports. A new version is a new
    calibration key.
11. **Cloud privacy defaults.** Every cloud decision request carries
    `provider: { data_collection: "deny", zdr: true }`. If no endpoint for the
    chosen model meets that, the backend reports unavailable with the reason.
    It does not silently relax.
12. **Background decisions stay on this computer.** A purpose that runs
    without the user asking for it in that moment (mail triage, the
    reflection gate, the untrusted flag, `AppKind` on a headless run, weekly
    refits) uses the local backend even when the global backend is cloud. It
    goes to the cloud only if the user opts that purpose in, by name, in
    Runtime's policy table. Without a local engine, such a purpose is simply
    off. This is the rule `local_endpoint` and `engines/learn.rs` already
    follow.
13. **Existing gates first, decisions on top.** The deterministic gates
    (trust levels, the connector gate, agent-app approvals and guards,
    `Toolset::ready`, persona app grants, `/external off`) run before any
    decision and are never overruled by one. A decision may narrow what they
    allow (hold a tool back, add a prompt, flag) or explain what they do (a
    reason line). It never widens it, except through `relax-prompts` under
    `USE-12`'s conditions, which exclude app calls and agent apps entirely.
14. **Labels come from what is already recorded.** Before `CTL-3` records a
    new label, it reads the tables that already hold the user's actions
    (`engine_signals`, `engine_rules`, `app_call_log`, connector overrides,
    connector activity rows, goal status). New label rows are written only
    where nothing records the action yet.
15. **Seen where it acts, in the user's words** (v3, `SEE-0`). A decision that
    changes something shows at that spot, as what I will do or did. No
    counts of things not done, no unit names the user never chose, no bare
    probabilities on the main surface. Supersedes the "to the UI" half of
    decision 4: the confidence still reaches the UI, in the hover and the card.
16. **Typing follows the backend choice** (v3, `SEE-3`, revised 2026-10-27 by
    Erich: "cloud model is always an option"). Composer hints ask the decision
    backend the person chose, so a computer without a local engine gets hints
    from a cloud decision model. A typed draft is never written to the log:
    its answers are held in memory and written only when a hint is acted on
    (without the draft's words) or the send reuses them. The Runtime card says
    in one line that typed text goes to the cloud there.
17. **Verify is part of `fast-slow`** (v3, `SEE-5`). The auto check uses only
    what the turn already read and adds no search. Its questions follow the
    backend choice like any decision the person is there for (revised
    2026-10-27, it was local-only); **Check more deeply** also searches the
    web through the Web search tool.

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

### `DEC-4` `decisionserver.rs` - the decision sidecar

Mirrors `rerankserver.rs`: lazy start, idle stop (5 minutes), one instance,
the same shared `llama-server` binary. (The image engine is a separate
`sd.cpp` binary and doesn't share it.)

- **Placement** (`hardware.rs`):
  - `Auto` (default): GPU if free VRAM ≥ model + 1 GB headroom *with the chat
    model loaded and the image engine's model counted when image generation
    is ready* (`Toolset::ImageGen` ready with a local backend), otherwise CPU.
    The headroom check is repeated when the image engine starts; if they no
    longer fit, the decision engine is stopped and the next start is placed
    again (hot-path purposes skip until then).
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
**Exception (decision 12):** a row marked `background: true` in `CTL-1`
resolves to `local` unless its own override says `cloud` (the per-purpose
opt-in). The global setting never moves it.

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

### `DEC-6` Move the five existing sites onto the seam

- `retrieval::judges_sufficient` → `Noul`, purpose `RetSufficiency`, giving
  `Some(p_true >= 0.5)`. The probability goes into the warning text.
- `memory_skill::classify_scope` → `Choice { global, topical, unsure }`.
  `unsure`, or confidence below 0.6, stores `None` (the backfill retries).
- Reflect critic → `Choice { ok, unsupported, unactionable, wrong }`. The
  reason is the winning option's criteria. No JSON parse.
- **Goal check** (`agent/goal.rs`) → `Noul` "Does the goal hold, judged from
  the last answer and the changes?", purpose `GoalMet`. The verdict comes from
  the decision; the existing completion still writes the one prose line
  (`evidence` when met, `next` when not), with the verdict given to it, so
  `GoalCheck` keeps its shape and `src/lib/goal.ts` doesn't change. The judge
  stays hard to satisfy. Until `GoalMet` is calibrated, `met` needs both the
  decision at `p_true ≥ 0.8` and the completion agreeing (it runs anyway for
  its line), so the swap can only make the check stricter (decision 3).
  Calibrated, the decision alone decides. The probability goes on
  `GoalCheck` (new optional `p`) for the Run bar.
- **Rule preview** (`engines_route_preview_cmd`) → `Choice` over the rule
  labels of every candidate agent app (`claude-code:U2`, …, `codex:A1`) plus
  `none` and `unsure`, purpose `HandoverRoute`, the same ask `USE-13` uses.
  Guards stay in Rust as today. `unsure` or confidence below 0.6 falls back
  to today's one-shot call. The `Preview` shape doesn't change.

When the decision engine is unavailable, each site **falls back to today's
`drive_turn` path**, not to `None`. Users without the model lose nothing.
Parity at 0.5 is checked on fixtures (`DEC-T-2`). No experiment flag: this is
a pure swap. (`HandoverRoute` in the preview is a swap; using it before the
lead runs is `USE-13`, behind `fast-slow`.)

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
    pub background: bool,         // runs without the user asking now: local only (decision 12)
    pub experiment: Option<ExperimentId>, // gated purposes (EXP-2)
}
```

Initial rows (the thresholds are starting values, tuned in `CTL-4`):

| Purpose | Mode | Impact | Act at (cal / uncal) | Below threshold | Experiment |
|---|---|---|---|---|---|
| Purpose | Mode | Impact | Act at (cal / uncal) | Below threshold | Background | Experiment |
|---|---|---|---|---|---|---|
| `RetSufficiency` | Assisted | Cheap | 0.5 / 0.5 | as today | no | – |
| `MemScope` | Reflex | Cheap | 0.6 / 0.6 | store unclassified | yes | – |
| `ReflectCritic` | Assisted | Visible | 0.5 / 0.5 | as today | yes | – |
| `GoalMet` | Assisted | Visible | met ≥ 0.8 / ≥ 0.8 and the completion agrees | "not yet" | no | – |
| `HandoverRoute` (preview) | Assisted | Cheap | 0.6 / 0.6 | today's one-shot call | no | – |
| `ToolSelect` | Assisted | Visible | drop < 0.15 / < 0.05 | keep the unit | no | `fast-slow` |
| `HandoverRoute` (pre-match) | Assisted | Visible | card ≥ 0.85 / hint only | the lead decides as today | no | `fast-slow` |
| `PlanDone` | Assisted | Visible | 0.8 / never auto | "probably done" | no | `fast-slow` |
| `SteerClass` | Reflex | Visible | stop ≥ 0.9 / never | queue as today | no | `fast-slow` |
| `ReflectGate` | Reflex | Cheap | skip < 0.3 / < 0.15 | run reflection | yes | `fast-slow` |
| `MailTriage` | Reflex | Visible | show ≥ 0.6 | show nothing | yes | `mail-triage` |
| `UntrustedFlag` | Reflex | Cheap | flag ≥ 0.7 | no flag | yes | `fast-slow` |
| `RunIf` | Reflex | Visible | run ≥ 0.5 | skip + log | yes | `fast-slow` |
| `ClaimVerdict` | Verify | Visible | settle ≥ 0.85 | escalate to slow review | no | `verify` |
| `BrowserCheck` | Verify | Costly | proceed ≥ 0.8 | stop and hand over | no | `fast-slow` |
| `AppKind` | Gate | Irreversible | stricter only: send ≥ 0.6 | the name rule's kind | headless runs only | `fast-slow` |
| `ToolRisk` | Gate | Irreversible | add prompt only | – | headless runs only | `fast-slow` |

**Hard gates take precedence over every row** (decision 13). No threshold
opens anything the deterministic gates close: the connector gate, agent-app
approvals and guards, `Toolset::ready`, persona grants, `/external off`.
`HandoverRoute` appears twice because the same ask has two uses with
different rules. Calibration is shared.

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

**Delegation:** the budget belongs to the user turn, not to each run. A
delegated child (ordinary or parallel) draws from its lead's budget for
gate purposes (`ToolRisk`, `AppKind`, `BrowserCheck`) and has no hot-path
purposes of its own (`USE-4` never narrows children). Agent-app children make
no decisions themselves; the decisions about their approvals are counted
against the lead's turn. Background children with no lead turn get their own
budget of 20.

**Background budget:** background purposes have a separate budget of 200 per
hour, shared by all of them. `UntrustedFlag` gets at most half of it, because
its intake now includes every connector result (MCG-1), agent-app report
(LRN-4) and memory recall over the MCP endpoint (MCPS-2). Over budget, the row
takes its below-threshold default and the hourly Activity group says how many
were skipped.

When a limit hits, the controller takes the row's below-threshold default and
logs `budget`.

### `CTL-3` Outcome labels

A `decision_outcomes` table (schema **v37**) holds
`decision_id, label, source, at`. Labels come from what the user and the
runtime actually did.

**Read from what already exists (decision 14).** These are joined to
decisions by run and time when calibration runs. Nothing is copied into
`decision_outcomes`, so the two can't drift:

| Existing record | Purpose | Label |
|---|---|---|
| `engine_signals` `override` (*Do it yourself*, *This shouldn't have gone to …*) after a pre-matched rule | `HandoverRoute` | wrong |
| Handover card answered *Hand it over* / *Always for this* for the pre-matched rule (`engine_rules.hits`, the card's `Decision`) | `HandoverRoute` | right |
| `engine_signals` `asked` (`/delegate <agent>`) on a request pre-matched `none` | `HandoverRoute` | wrong (needed) |
| `connector_policy.tool_overrides` set to a stricter kind than the name rule gave | `AppKind` | corrected value |
| Connector activity row: an `app` prompt `AppKind` raised to send, answered *No* | `AppKind` | weak right |
| Goal `stalled` or `exhausted` after a round judged `not yet` at high confidence | `GoalMet` | weak right |
| User sets a new goal at once after one judged `met` (same conversation, within 10 minutes) | `GoalMet` | weak wrong |
| Golden / eval fixtures | all | gold |

**Written new, because nothing records them yet:**

| Source | Purpose | Label |
|---|---|---|
| `need_tools` re-adds a held-back unit | `ToolSelect` | wrong (needed) |
| Run finishes without calling a kept unit | `ToolSelect` | weak right |
| User reopens a judged plan tick | `PlanDone` | wrong |
| User retags a mail, or uses "Needs reply" on an untagged one | `MailTriage` | corrected value |
| User clicks "Stop the run?" / dismisses it | `SteerClass` | right / wrong |
| User overrides a claim verdict in Verify | `ClaimVerdict` | corrected value |
| Browser check said "changed", next step failed on the same page | `BrowserCheck` | wrong |
| Permission prompt added by `ToolRisk` was approved without hesitation 20× | `ToolRisk` | weak wrong |
| Prompt carrying a `ToolRisk` "not what you asked for" line was answered *No* | `ToolRisk` | right |

The goal status lives on the frontend (`src/lib/goal.ts`). The label is
written by a small command (`record_goal_outcome_cmd`) when the status
changes, which is the hook LRN-1 left for this plan.

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
user's retag is a label (`CTL-3`). Background purpose: local only (decision
12), so mail bodies never go to OpenRouter unless the user opts this purpose
in by name.

**Scope:** Mail accounts over IMAP only. Gmail or Outlook connected through
Composio (Apps) are not triaged: Composio has no new-mail signal without
triggers (`CAP-P2`), and polling it would mean calls the user didn't ask for
going through a third party.

#### `USE-6a` A small inbox (prerequisite)

v1 assumed a mail list that doesn't exist. Today `mail.rs` fetches only when
the agent calls `read_mail` / `search_mail`, and `Mail.tsx` is account setup.
Triage needs both a place to show and a moment to run:

- **Fetch:** `agent/mail_sync.rs`, a background loop while the app runs:
  every 15 minutes per enabled account, envelopes only (`envelope_row`, the
  same IMAP code as `list_blocking`), newest 200 in INBOX, `UID`-based so only
  new mail is fetched. Bodies are fetched for triage only, capped at 4 KB of
  text (`strip_html`), and not stored.
- **Store:** `mail_seen` (v37): `account_id, uid, message_id, from, subject,
  date, triage_json, triaged_at`. Kept 30 days, never the body.
- **Off by default with the experiment.** Nothing fetches in the background
  unless `mail-triage` is on. Turning it off stops the loop and keeps the
  rows until "Reset its data".
- Headless and quiet: no notifications in this plan.

This is real new work (a sync loop, a table, a list view). If it proves too
much, the fallback is triage inside `read_mail` results only (tags in the tool
output and its step line), with no inbox. See Open questions.

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
  blocks, per `untrusted.rs`. The intake sites now include connector results
  (MCG-1, `mark_untrusted` in `call_mcp_tool` and `relay_composio`), agent-app
  reports (LRN-4, `escape_frame` / `defang`), memory recall served to agent
  apps (MCPS-2), and model text in rule and memory proposals (`learn.rs`,
  `engines/tools.rs`, which already refuse at scan risk ≥ 2). Asked only where
  `untrusted::scan` returned risk 0 or 1: the fast check is for what the
  pattern scan misses, not a second opinion on what it already caught. It
  draws from the background budget (`CTL-2`). For proposal text, a flag ≥ 0.7
  adds the same refusal the scan gives at risk 2.

### Mode 2: Assisted (the LLM decides, the fast plane narrows or informs)

#### `USE-4` Tool and skill pre-selection *(scenario 1)*

Before the first request of each user turn (not each loop step), one ask.

- **State:** the user message plus the last assistant message (clipped to
  2k chars).
- **Units** (what can be held back). Since v1, most of a turn's tool specs
  are no longer toolsets:
  - each optional toolset that is on **and ready** (`Toolset::ready`);
  - **Connected apps**: the Composio meta-tools as one unit, described to the
    decision model by the granted app names ("Google Calendar, Slack, Notion");
  - **each MCP connector** as one unit, described by its name and its first
    five tool names;
  - **Other agents**: the `delegate` tool, described by the agent types it
    would list, including the agent apps' rule texts. This is often the
    largest single spec, because of the RTE-4 rule blocks.
- **Questions:** one `Noul` per unit, plus one `Choice` over enabled skills +
  `none`.

Applied in `run.rs` **after** prompt assembly and **after** the existing
narrowing (persona and delegation narrowing, `Toolset::ready`, CAP-10 app
grants, `/external off`), so the golden gate is untouched and the decision
only ever sees what the run could really use (LRN-5, decision 13).

- Drop a unit when below the `CTL-1` threshold. File System, Plan and Recall
  are always kept.
- **Never dropped:** `delegate` when the user typed `/delegate` in this
  message (the explicit choice wins, as in RTE-1); Connected apps while a
  `connect` block in this conversation is waiting (the turn continues after
  sign-in, CAP-6).
- Never narrow for subagents, agent-app children, resumed runs, scheduled
  tasks, or a CPU-placed engine.
- `need_tools { units: [...] }` is a tiny always-present tool. It re-adds
  held-back units from the next step, and the model is told in one line that
  more tools exist on request, by unit name. **It can only re-add what was
  held back.** A unit that was never offered (not ready, not granted,
  `/external off`) is refused with the same reason Settings → Tools shows
  (`ToolsetInfo.needs`), so `need_tools` can't route around capability
  truth or grants.
- A skill `Choice` ≥ 0.7 moves that skill to the top, marked "(likely
  relevant)". Skills are never hidden.
- Deadline 300 ms local, 800 ms cloud. On a miss, nothing is narrowed.
- The decision model sees the app and agent names, never Composio
  descriptions or schemas. The state is the user's message, so this is no
  new data leaving the machine when local.

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

Since v1, three kinds of approval exist that this must sit on, not beside
(decision 13):

| Approval | Who decides the kind today | What `USE-12` adds |
|---|---|---|
| Own tools (`send_mail`, `run_command`, file edits, browser) | trust levels, impact classes | risk + in-scope; may add a prompt; reason line |
| **App calls** (MCP connectors, Composio targets; `capability: "app"`) | `connector_gate::classify` (read / change / send), per-tool overrides, "always", daily cap | **in-scope only**, as a reason line on the prompt the gate already shows; may add a prompt to a *read* the user's request didn't call for (in-scope < 0.2). Risk is not asked: the gate's kind is the risk. |
| **Agent-app approvals** (`capability: "engine"`, from Claude Code, Codex, OpenCode) | the agent's own mode, the folder ceiling (CLI-6), `approve.rs` | in-scope against **the task handed over** (not the lead's whole conversation), as a reason line. Never adds or removes a prompt: the agent app owns its own loop. |

- **Any backend:** may *add* a prompt on own tools (risk 4 ≥ 0.8, or
  in-scope < 0.2) and on app reads (in-scope < 0.2), and adds its reason
  line to any shown prompt of the three kinds.
- **Headless runs:** an added prompt can't be answered, so it becomes the
  connector gate's "left for you" parking (MCG-3) for app calls, and a refusal
  with the reason for own tools. Never a hang.
- **Skipping a prompt** needs all of the following:
  - the separate experiment toggle `relax-prompts`,
  - a calibrated backend,
  - risk ≤ 2 at ≥ 0.95 and in-scope ≥ 0.95,
  - never for delete, send, or anything outside the working folder,
  - **never for app calls** (the user's own "Always for {app} changes" and
    daily cap are the way to stop asking there, and they are visible in
    Settings → Apps),
  - **never for agent-app approvals or handover cards** (agent apps run
    outside the sandbox; on Windows, Codex may run without its sandbox, see
    `ExternalOutcome.sandbox_off`).

  Each skip is logged and shown in the timeline, with Undo where one exists.

**`AppKind`, the stricter-only kind check.** `connector_gate::classify` goes
by the tool name, and LRN-2 records that "a send with an innocent name is
missed". For an app target the name rules call `Read` or `Change`, and that
has no per-tool override yet, one `Choice { read, change, send, unsure }` over
the tool name, its description (from the cached `tools/list` or Composio's
schema) and the arguments. If `send` ≥ 0.6, the call is gated as a send
(with the reason line *"Asked because this looks like it sends something"*).
It never makes a call looser than the name rule: `read` or `unsure` changes
nothing. A per-tool override (MCG-7) always wins, and the user's "Treat as
send" after such a prompt is a corrected label. Answers are cached per
(connector, tool) for the day, so a busy tool costs one decision.

Last on purpose: it most needs calibration and robustness.

### Mode 2 (continued): agent apps

#### `USE-13` Handover pre-match *(scenario 11)*

`CLI_ENGINES_PLAN` RTE-6 already points here: *"the decision model can
pre-match a request against rules before the lead runs"*. Same ask as the
`DEC-6` preview swap (`HandoverRoute`), used in the live turn:

- **When:** before the lead's first request of a user turn, only if at least
  one agent app is offered this turn (after guards, `use_mode`, `/external
  off`, and `max_per_conversation`), and the message isn't an explicit
  `/delegate` (that already chose).
- **State:** the user message plus the last assistant message (2k chars).
- **Question:** one `Choice` over every offered rule label
  (`claude-code:U2`, `codex:A1`, …) plus `none` and `unsure`.
- **What it does with the answer:**
  - A calibrated `use` rule ≥ 0.85, on an engine in `suggest` mode: the
    handover card is raised **before** the lead runs, citing that rule.
    *Hand it over* starts the child with the rule as `because`, and the lead
    is told it was handed over. *Do it yourself* runs the lead as today and
    records the override as RTE-4 does (it's the same card, same signals).
  - A calibrated `use` rule ≥ 0.85 on an `auto` engine: no early start.
    The lead gets one line in the turn's context, *"This looks like the
    user's rule U2 for Claude Code (0.91)."*, and still decides. Starting an
    agent app without the lead is a step this plan doesn't take.
  - An `avoid` rule ≥ 0.85: the lead gets *"The user's rule A1 says not to
    hand this to Claude Code."* `delegate` already refuses a `because` that
    cites an avoid rule.
  - Uncalibrated, below threshold, `none`, `unsure` or a missed deadline:
    nothing changes. The lead decides as today.
- **Check on the lead:** when the lead calls `delegate` with a `because`
  whose rule the pre-match scored below 0.1, the handover card (in
  `suggest`) gets a reason line, *"I'm not sure this matches U2 (0.06)."*. In
  `auto` it is logged only. Never a refusal: the rule match is the user's
  rule, and the lead may be right.
- Deadline 300 ms local, 800 ms cloud. Labels from `engine_signals` and the
  card's answers (`CTL-3`).
- Experiment `fast-slow`. Ship gate: `CTL-T-3`'s handover slice.

### Deferred

- **Live run monitor** (offered in v3, not chosen 2026-10-26): after each tool
  step, *on track / repeating itself / drifting from the task* in the run
  header, stopping to ask on drift. It's `USE-11`'s page check applied to every
  run. Revisit after step 20.
- **Effort routing:** `Score` over reasoning-effort levels when effort is
  `auto`, only when the engine is warm.
- **Model routing:** deferred until favorites are common enough to matter.
- **A local `/v1/systemone` endpoint for skill scripts via tool RPC:** cheap
  once `DEC-4` exists. Add it when a skill needs it.
- **`classify` for agent apps over the MCP endpoint:** the loopback endpoint
  (MCPS-1/2, `agent/toolrpc.rs`) already serves four tools to Claude Code,
  Codex and OpenCode behind per-agent switches. `classify` could be a fifth,
  behind its own switch, so an agent app sorting 300 files uses the local
  decision model instead of its own paid tokens. Deferred until `USE-9` has
  shipped and Codex's tool use is reliable (CLI_ENGINES Part X, Phase 5 open
  item 1). *(`USE-9` is built since step 8 but has not run on a real model, so
  this waits for that as well.)*
- **Pre-matching for scheduled tasks:** `USE-13` only runs with someone
  watching. Headless parents treat `suggest` as `on_request` (RTE-1), so
  there's nothing to pre-match yet.
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
- **What is not an experiment:** the switches the other plans built are
  ordinary settings and stay where they are: an agent app's mode, rules and
  guards, *Let it use my memory*, *Let it use my connected apps* (Settings →
  External agents), and the connector policies (Settings → Apps). They are
  the user saying what is allowed, not Poiesis trying a new behaviour.

### `EXP-2` Initial experiments

| Id | Title (UI) | What it turns on | Requires | Default |
|---|---|---|---|---|
| `fast-slow` | **Fast & slow thinking** | `USE-4`, `5`, `7`, `8`, `9`, `10` (as **Check more deeply**), `11`, `12` (add-only, incl. `AppKind`), `13`; v3: `SEE-1`..`SEE-4` | decision model | off |
| ~~`verify`~~ | ~~Verify answers~~ | folded into `fast-slow` in v3 (`SEE-5`) | – | – |
| `mail-triage` | **Mail triage** | `USE-6`, `USE-6a` (the inbox and its background fetch) | a **local** decision model; a Mail (IMAP) account | off |
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
│  · I can tell sooner when a job is one for Claude Code or Codex      │
│  · Permission prompts can carry a reason, and I may ask more often   │
│                                                                      │
│ Needs   Decision model  ● Clef-flash ready      [Change…]            │
│ Records Each decision, its confidence, and what you did about it.    │
│                                                                      │
│ Last 30 days  Held back tools on 214 turns · recovered 9 (4%) …      │
│               [Details ▸]  [Reset its data]                          │
└──────────────────────────────────────────────────────────────────────┘

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
Backend    (•) This computer · Clef-flash
           ( ) Cloud via OpenRouter · Clef Flash          [Change in Models]
               Sends the question and a short excerpt to OpenRouter,
               zero-retention providers only. ~$0.02 per 1,000 decisions.
               Background decisions (mail, reflection) stay on this
               computer either way.
               (disabled without an OpenRouter key → Providers)

Calibration  Clef-flash · fitted 2026-10-14 · 9 of 13 purposes · 312 labels
             [Calibrate now]  about 3 minutes
             Uncalibrated purposes only make me more careful.

Advanced ▸   Policy table: purpose · mode · threshold · runs in background ·
             backend · last p · avg ms · labels
             Everything is read-only except one control: for a background
             purpose, "Allow the cloud for this" (off). Background purposes
             stay on this computer even with the cloud backend chosen.
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

`routes/Usage.tsx` gets a "Decisions" section beside the existing *Agent
apps* and *Connected apps* sections, in the same layout: count, backend
split, cloud cost. Local is "free · on this computer".

### `USE-UI-1` Run header chip *(USE-4)*

> **Superseded in v3 by `SEE-2`.** The chip only said what did not happen;
> the first-thought strip says where I'll look and, only for what the user
> turned on, what I'm leaving out. The text below is history.

A muted `3 tools held back` chip. Hover lists them by unit name with
probabilities (*web 0.03 · Connected apps 0.04 · Other agents 0.02*). Click
adds them back for this run (same as `need_tools`, and labelled the same
way). There is no chip when nothing was held back. Units that were never
offered (not ready, not granted) are **not** in the chip: that is Settings →
Tools' "I can't use this yet", not a quick decision.

### `USE-UI-2` Steering *(USE-5)*

- A queued steering message shows its class as a tag (`addition`,
  `redirect`, `question`).
- A judged `stop` shows a **Stop the run?** button under the message with
  "Keep going" beside it. Both answers are labels.

### `USE-UI-3` Plan ticks *(USE-5)*

A judged item shows a ringed tick, not a filled one, with the hover text
"Judged complete from the answer (0.88)". Clicking reopens it.

### `USE-UI-4` Inbox *(USE-6, USE-6a)*

There is no mail list today, so this is a new view, kept small. `routes/Mail.tsx`
gets two tabs, **Accounts** (today's page, unchanged) and **Inbox** (shown only
while `mail-triage` is on).

```
Inbox                                    [All] [Needs reply 6] [Urgent 3]
Fetched 09:15 · every 15 minutes · nothing leaves this computer   ↻
▌▌▌  Ana Ruiz        Contract draft for Thursday       work      08:52
▌▌   GitHub          [poiesis] CI failed on main       notification 08:40
▌    Lieferando      Ihre Bestellung                   receipt   07:12
     Newsletter Co   This week in AI                   newsletter 06:00
```

- A 4-step urgency bar on the row's left edge and a category tag. A tag click
  opens a retag menu.
- Filters: All, Needs reply, Urgent (≥ 3). Per account when there are
  several (the shared `Select`).
- Hover on the bar: "Judged urgent (0.87)".
- Unclassified rows show nothing, never a guess.
- A row opens nothing in the app. It offers **Ask about this**, which starts
  a chat with "Summarise the mail from Ana Ruiz, 'Contract draft for
  Thursday'" (the agent reads it with `read_mail` as today).
- Category list editor: on the Accounts tab, under the account list.
- Empty states: no account ("Add a mail account first", link to Accounts);
  no decision model ("Mail triage needs the decision model on this computer",
  link to Models).
- Fixed height list with a count, as the other Settings lists.

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

In `components/SidePanel/PermissionPanel.tsx`, one secondary line under the
headline, for every kind the panel draws:

- Own tools: `Flagged: sends mail outside this conversation's request (0.91)`.
- `app` (MCG-UI-1): under the summary, above the argument list:
  `You asked for #releases, not #general (0.88)`. An `AppKind`-raised send
  says `Asked because this looks like it sends something (0.81)` and uses the
  send layout (outbound accent, *Send once / Up to N a day / No*).
- `engine` (CLI-UI-3): under *"Claude Code wants to run a command"*:
  `Not part of the task you handed over (0.84)`. The buttons don't change.
- `handover` (RTE-UI-2): see `USE-UI-10`.

A prompt the judge added says `Asked because this looked irreversible`. A
skipped prompt (`relax-prompts`) appears afterwards in the timeline as
`Skipped a prompt · edit_file · low risk (0.97) · [Undo]`. The line is
hidden when the decision agrees with the gate (in scope, risk low): only a
flag is worth the space.

### `USE-UI-9` Browser hand-over *(USE-11)*

When the visual check stops the agent: an inline card with the screenshot,
"This page wants a login. Take over?", and **Open browser** / **Skip this
step**.

### `USE-UI-10` Handover card and Fleet row *(USE-13)*

- **Early card.** A pre-matched handover uses the existing card (RTE-UI-2)
  with one change to its rule line: `Matches your rule U2 · quick decision
  (0.93)` instead of `Because of your rule U2`. It appears where the lead's
  first token would have, before any lead text. Buttons unchanged.
- **Doubt line.** When the lead cites a rule the pre-match scored low, the
  card gets a secondary line: `I'm not sure this matches U2 (0.06)`.
- **Fleet row.** The `because U2` chip (RTE-UI-3) gets a small dot when the
  rule came from the pre-match; hover says *"matched before I started
  (0.93)"*.
- **Try a request** (RTE-UI-4) shows the same answer, with `quick decision ·
  41 ms` or `asked my chat model` so the user knows which path answered.
- **When I use it** (RTE-UI-1): each rule row gets a muted counter when it has
  pre-match data, *"matched early 12× · you said do it yourself 1×"*.

### `USE-UI-11` Goal check *(DEC-6)*

The Run bar's goal status (`RunBar.tsx`) shows the check's probability after
the verdict: `not yet (0.12)`, `met (0.91)`. The title keeps the evidence or
next-step line. No probability is shown when the check came from the prose
fallback.

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
  when unavailable. Goal check: an uncalibrated decision of 0.9 with a
  completion saying "not met" is "not yet"; `GoalCheck` keeps its fields and
  gains `p`; broken completion JSON with a calibrated `met` is still `met`.
  Rule preview: the `Choice` maps back to `{agent, because}` in the existing
  `Preview`; `unsure` falls back to the one-shot call; a guard still blocks a
  confident match.
- `DEC-T-5` Background stays local: with `decide.backend = cloud`, a
  `background: true` purpose resolves to local; with no local engine it is
  `Unavailable`, never cloud; the per-purpose override sends it to cloud.
- `DEC-T-3` Head check: a mock server returning generated text is rejected; a
  well-formed answer passes.
- `DEC-T-4` Pin bump: the rerank, embed and chat smoke tests against the new
  build (eval, needs the binary).
- `CTL-T-1` Policy: hard gates beat thresholds; uncalibrated rows never
  loosen; the budget hit falls back to the default; an unchanged state hash
  blocks a re-ask.
- `CTL-T-2` Outcomes: each `CTL-3` source writes the right label; calibration
  weights weak labels; ECE is computed correctly on a known set. The
  read-from-existing sources: an `engine_signals` override after a pre-matched
  rule joins as `wrong`; a stricter `tool_overrides` entry joins as a
  corrected `AppKind`; nothing from those tables is copied into
  `decision_outcomes`. The delegation budget: a child's gate decisions count
  against its lead's turn.
- `CTL-T-3` Ablation gate (eval, needs engine, DPAR §12): on the golden set
  with a small local model, compare
  1. LLM alone
  2. static narrowing (always drop below a fixed p)
  3. the policy table

  for `ToolSelect`, plus fact-check precision/recall on the claim set for
  `USE-10` with and without blind review. Ship rule: no new failures, and
  `need_tools` recovers every dropped-but-needed case. **v2 slices:** golden
  cases with connected apps (a fake Composio connector with three granted
  apps) and with `delegate` carrying two agent apps' rule blocks, measuring
  prompt tokens saved and first-token time; a **handover slice** (`USE-13`),
  60+ requests labelled with the rule a person would pick or `none`, from the
  default Claude Code / Codex rules, scored for the early card (precision at
  0.85 must be ≥ 0.95, since a wrong early card costs a click).
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
- `USE-T-1` Tool menu (`USE-4`): Connected apps, each MCP connector and
  `delegate` are units; a unit not ready or not granted is never asked about
  and `need_tools` refuses it with the Settings reason; `/delegate` in the
  message keeps `delegate`; a waiting `connect` block keeps Connected apps;
  agent-app children and scheduled tasks are never narrowed.
- `USE-T-2` Risk on the three approval kinds (`USE-12`): an `app` prompt gets
  only the in-scope line, never a risk-based skip; an `engine` approval gets
  the line against the handed-over task and its buttons are unchanged;
  `relax-prompts` with a calibrated backend still asks for every app call,
  agent-app approval and handover card; a headless added prompt on an app
  call is parked "left for you", never a hang.
- `USE-T-3` `AppKind`: a `Read`-by-name tool judged `send` 0.7 is gated as a
  send; judged `read` it stays read; a `Send`-by-name tool judged `read` stays
  send; a per-tool override wins over the decision; one decision per tool per
  day.
- `USE-T-4` Handover pre-match (`USE-13`): suggest mode + calibrated 0.9 on
  a `use` rule raises the card before the lead runs, with the same
  `Decision` handling as RTE-4; auto mode only adds the context line;
  `/external off`, a failed guard, or `max_per_conversation` reached means no
  ask at all; uncalibrated changes nothing; Stop withdraws an early card
  (the CLI_ENGINES review fix 3 path).
- `USE-T-5` Inbox (`USE-6a`): only new UIDs are fetched; bodies are not
  stored; the loop runs only while `mail-triage` is on; rows older than 30
  days are removed; a Composio-connected Gmail is never polled.
- `EXP-T-1` Experiments: the registry mirrors to the frontend; a gated tool
  is absent from the spec when off; `/workspace` when off answers with the
  pointer; the `switch_mode` spec has no `workspace`; the migration turns
  `workspace` on only when a workspace conversation exists.
- `EXP-T-2` Frontend: Experiments states (requirement missing, download,
  first-on note, reset confirm), Runtime card states and the background
  cloud opt-in, Models fit line, the tools chip (unit names, never a
  not-ready unit), steering stop, ringed tick, the permission reason line on
  own-tool, `app` and `engine` prompts, the early handover card and its doubt
  line, the goal probability in the Run bar, the Inbox tab (filters, empty
  states, retag), Verify marks.

---

## Order

0. **Before anything:** commit the open CLI_ENGINES review fixes (Part X,
   "Review report", not committed on 2026-10-23), so the files this plan
   touches (`subagents.rs`, `engines/approve.rs`, `engines/run.rs`) start
   clean. Re-check the llama.cpp facts in "Shell facts".
   *(Facts re-checked; the commit is still open. See "Implementation status".)*
1. `DEC-1`, the pin bump, alone, with `DEC-T-4`. **Done 2026-10-23.**
2. `DEC-2`..`DEC-7` + `DEC-UI-1`, `DEC-UI-2` (local backend only) +
   `DEC-T-1..3`, `DEC-T-5`. Engine, catalog and seam; the **five** sites
   swapped with no behaviour change (incl. the goal check, `USE-UI-11`, and
   the rule preview). **Done 2026-10-23, local only; see "Implementation status".**
3. `EXP-1`..`EXP-3` + `EXP-UI-1` + `EXP-T-1`. The Experiments section ships
   with **Workspace mode** as its first entry, so the framework is real before
   any decision feature depends on it. **Done 2026-10-23; see "Implementation status".**
4. `CTL-1`..`CTL-4` + `CTL-UI-1` + `CTL-T-1`/`T-2`. Policy, escalation,
   outcomes (reading `engine_signals` and the connector tables from the
   start), calibration, visibility. **Done 2026-10-23 for the five purposes
   that exist; see "Implementation status".**
5. `USE-4` + `USE-UI-1` + `CTL-T-3` + `USE-T-1`. The first thing a user can
   feel (`fast-slow`, partial). With connected apps and agent apps as units,
   this is also where the biggest prompt savings are. **Done 2026-10-23, not yet
   run on a real model; see "Implementation status".**
6. `USE-13` + `USE-UI-10` + `USE-T-4`. The handover pre-match, on the same
   ask as the swapped preview, while the handover labels are fresh. **Done
   2026-10-23, not yet run on a real model; see "Implementation status".**
7. `USE-8` (reflection gate, untrusted flag with its budget). **Done 2026-10-23,
   not yet run on a real model; see "Implementation status".**
8. `USE-9` + `USE-UI-5` (`classify`). **Done 2026-10-23, not yet run on a real
   model; see "Implementation status".**
9. `USE-5` + `USE-UI-2`, `USE-UI-3`, and `USE-7` + `USE-UI-6`. **Done 2026-10-23,
   not yet run on a real model; see "Implementation status".**
10. `USE-6a` + `USE-6` + `USE-UI-4` + `USE-T-5` (`mail-triage`). Moved later
    in v2: it now needs an inbox and a fetch loop of its own. **Done 2026-10-23,
    not yet run on a real model or a real mailbox; see "Implementation status".**
11. `USE-10` + `USE-UI-7` (`verify`). **Done 2026-10-26, not yet run on a real model
    or a real web search; see "Implementation status".**
12. Cloud backend: the OpenRouter half of `DEC-5` (with the background
    exception), `DEC-5a` catalog + probe, the cloud group of `DEC-UI-1`,
    `DEC-UI-3`, `CTL-UI-2`. **Done 2026-10-26, not yet run against OpenRouter
    with a key, `CTL-T-5` not run; see "Implementation status".** This can move earlier if local placement turns
    out too tight on common hardware: cloud Clef-flash is the same model at
    $0.02/M. `DEC-8`'s header probe and `CTL-T-5` run here, so Jev's role is
    settled by data before any UI recommends it.
13. `USE-11` + `USE-UI-9` (needs Clef vision in the pinned build). **Done
    2026-10-26, no picture sent to a real model; see "Implementation status".**
14. `USE-12` + `USE-UI-8` + `USE-T-2`/`T-3`: add-only first (own tools, then
    the in-scope line on `app` and `engine` prompts, then `AppKind`),
    `relax-prompts` last. **Done 2026-10-26, no real model has answered;
    see "Implementation status".**
15. `EXP-4` reports. Some reports land earlier, as each experiment's data
    exists. **Done 2026-10-26; see "Implementation status".**

**v3 (2026-10-26), not started:**

16. `SEE-5` (fold Verify into `fast-slow`, migration) + `SEE-4` (`sureWord` and
    the surfaces) + `SEE-T-8`/`T-9`. Small, and it clears the way.
17. `SEE-2` strip and receipt, removing `USE-UI-1` + `SEE-T-5`/`T-6`.
18. `SEE-1` + `SEE-UI-1` (auto check, action claims, the sweep, the trust line,
    `ClaimCheckable`, `mode` column) + `SEE-T-1`..`T-4`.
19. `SEE-3` composer hint + `SEE-T-7`.
20. `SEE-T-10`, **Erich's real run with Clef-flash local.** One chat that
    searches the web, one that answers from memory, one coding chat that
    claims tests pass without running them, one message matching a
    handover rule, typed slowly. Pass means each of 0a-0c and 1 shows up
    as written, the composer never jumps, and the strip appears before the
    first token. Whatever doesn't show up is a bug in this plan, not in the
    user.

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
  (Clef ×3, Jev 1.14, Jev 1.13) by default and the rest on demand. *Built that
  way in step 12; 27 models are listed (2026-10-26).*
- **Jev via OpenRouter or direct?** *Step 12 built no Jev-specific probe (no key
  to run one), so this is still open: the headers and `assets` map were never
  sent. Jev does have a zero-retention endpoint.* If the probe shows OpenRouter drops the
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
- **Inbox or no inbox (v2)?** `USE-6a` adds a background fetch and an Inbox
  tab, which is a small mail client's worth of new surface. The smaller
  alternative is triage only inside `read_mail` results (tags in the output
  and the step line), with no fetch loop and no list. Scenario 2 needs the
  inbox; the draft keeps it but moved it later in the order.
- **Early handover card (v2):** should a confident pre-match in `suggest`
  mode raise the card before the lead runs (as drafted), or only give the
  lead a hint and let the card come from the lead's `delegate` as today? The
  early card is faster and more legible; the hint is less surprising. The
  ship gate (≥ 0.95 precision at 0.85) is the guard either way.
- **`AppKind` and Composio descriptions (v2):** the check reads a tool's
  description from Composio's schema, which is third-party text. It only ever
  makes a call stricter, so an injected description can at worst cause an
  extra prompt. Is that acceptable, or should it read the name and arguments
  only?

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
- CLI engines (agent apps, handover rules, MCP endpoint, LRN) - `plans/CLI_ENGINES_PLAN.md`, Part X
- Connected apps (connector gate, Composio, CAP-14) - `plans/CONNECTED_APPS_PLAN.md`
- Prior judge plan - `plans/JUDGE_PLAN.md`
