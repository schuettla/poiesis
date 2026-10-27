# Cognition plan: memory and self-learning for Poiesis

Status: concept and plan, not built. Written 2026-10-27.

This plan replaces the idea that "self-learning" means "reflection writes
lessons". It has two halves that only work together:

1. **Memory** (Parts II and III.1 to III.8): what the harness keeps, taken
   from how human memory works, with the known flaws of human memory
   designed out.
2. **The self-improvement loop** (Part III.9 to III.12): how the harness
   changes its own setup, tests the change, keeps what works, and learns
   which kinds of change work. This half is built on Google's Harness²,
   adapted so that no live user task is ever run twice.

It also takes what is useful from HyperSkill (see Part I.4), and leaves out
what does not fit a user-facing assistant.

Rules for every task here (unchanged from earlier plans):

- The self is plain markdown the user owns. SQLite holds only derived data
  (episodes, usage, scores), never something the user wrote.
- Nothing is destroyed. Forget means archive or trash. Every self-write is
  visible and reversible.
- The autonomy rungs (`Off` / `Ask` / `Auto`) gate every self-change class.
- UI copy is first person (`PRES-0`), dialogs have a fixed size, shared
  components are reused. No "dream" or "brain" words in the UI.
- Background thinking is local by default and is cancelled by any user input.

## What this plan uses from the Dual-process plan

This plan uses the **decision model** (the `agent/decide` seam), never the
`fast-slow` experiment. Nothing here needs **Fast & slow thinking** switched
on.

- **No experiment flag.** Every purpose this plan adds is its own row in
  `policy.rs` with no `experiment` named, like the five `DEC-6` sites. The
  `ask_with` gate only refuses a purpose whose row names an experiment, so
  these run whatever the Experiments page says. The autonomy rungs are this
  plan's only switches.
- **All three question kinds are used**, not only yes/no:

  | Purpose | Kind | Options / levels | Used by |
  |---|---|---|---|
  | `TaskKind` | `Choice` | a fixed list (`report`, `code`, `mail`, `files`, `research`, `writing`, `chat`, ...) plus `unsure` | `EPI-2`, `CUE-1`, `EPI-UI-1`, `BND-2`, `MET-1` |
  | `BeliefRelation` | `Choice` | `same`, `refines`, `contradicts`, `unrelated`, `unsure` | `BLF-2`, `SLP-1` |
  | `CandidateGroup` | `Choice` | the ids of existing beliefs and other episodes' candidates, plus `new` | `SLP-1` step 2 |
  | `RehearsalMatch` | `Score` | `repeats the mistake` .. `matches the fix` (5 levels) | `RHS-2`, `IMP-3`, when the args can't be compared exactly |
  | `RecallUseful` | `Noul` | "Did the answer use this item?" | `CRD-2`, the fact row |

- **All of them are background purposes**: they wait for the engine to start,
  run on the processor too, and go to the cloud only on their own override
  (decision 12 of the Dual-process plan). None is on the hot path, so none
  needs a warm engine on the graphics card.
- **Every one has a fallback without a decision model**, as `DEC-6` requires
  ("users without the model lose nothing"):

  | Purpose | Without a decision model, or `unsure`, or confidence under 0.6 |
  |---|---|
  | `TaskKind` | Rules: the toolsets the run used (mail tools: `mail`; code tools or a repo: `code`; an artifact of kind document: `report`); else `unknown`. `unknown` episodes are kept but never shown as "Last time" and never counted for bundles |
  | `BeliefRelation` | Similarity over the SEM floor: both `contested`, never two silent actives. Under it: `unrelated` |
  | `CandidateGroup` | Similarity over the SEM floor groups; under it, `new` |
  | `RehearsalMatch` | Only the exact args comparison; candidates that need more get `not_run` and stay `confidence: medium` |
  | `RecallUseful` | `unknown` (not judged, as with no chances) |

- **UI pieces are reused, not their gates.** "Last time" (`EPI-UI-1`) uses
  the `SEE-3` hint line component, but its own trigger: a similarity lookup
  at conversation start, with no decision model and no experiment.
  `composer_hint_cmd` and its gate stay as they are. `sureWord` (`SEE-4`) is
  a plain formatter and is used directly.
- **Data, not behaviour.** `MET-1` and III.7 read the decisions table and
  `CTL-3` outcomes. With `fast-slow` off there are fewer decision rows (only
  this plan's purposes and the `DEC-6` sites), so the self-model is built
  from episodes first and calibration is extra when present.
- **Unproven seam.** No real model has answered a decision yet (Dual-process
  step 20). Each purpose above ships with a seed set under
  `tests/eval/decide/` so "Calibrate now" picks it up, and is used
  uncalibrated only with the stricter bar, like every other purpose.

---

# Part I - Why

## I.1 What we have today, honestly

| Part | What it does | Main weakness |
|---|---|---|
| Session log (`agent/log.rs`) | Every message a run saw, append-only, replayable | Nothing learns from it except reflection |
| Messages FTS + `search_history` | The agent can search old chats by keyword | Raw text only. No digest of what happened or how it ended |
| Facts (`memory/facts/`) | Durable notes about the user, global or topical scope, TTL | Flat. No evidence link, no "this replaced that", no contradiction check |
| Lessons (`memory/lessons/`) | Reflection draws up to 3 from a finished chat, critic checks them | Learned only from mistakes. Never scored after use. Cap 40, oldest go first |
| SOUL, profile | Standing instructions, style summary | A lesson that does not stick becomes more prose in SOUL |
| Skills | `propose_skill` during a run, revision when 3 of 5 runs are rough | Text only, no scripts. Nothing proposes a skill from work that went well |
| Golden set (`GLD`) | Checks generic cases before and after a self-change | Checks for damage, never for benefit |
| Decisions and outcomes (`CTL-3`) | Small-model predictions with what happened next | Used for calibration only, not for memory |
| Scheduler (`SCH`) and its built-in job | 60 s ticker while the app is open, headless runs, writes refused. The built-in `nightly-reflection` job (Tasks page) sweeps expired facts and reflects on up to 8 unreflected conversations, once a day | Off by default. Not night-bound but named as if it were. Starts even while the user is typing |

In one sentence: Poiesis stores a lot, but it does not remember episodes, it
does not know which of its memories help, and it learns only from failure.

## I.2 The human model we take

Cognitive science splits memory into systems that work together. We take the
split because each part solves a different problem.

| Human system | What it does | Poiesis part |
|---|---|---|
| Sensory buffer | Raw input, kept briefly | Session log (exists) |
| Working memory | The few things held in mind right now | The turn's context, assembled to a budget |
| Episodic memory (hippocampus) | "What happened, when, where, how it ended" | **New: episodes** |
| Semantic memory (neocortex) | Facts and concepts, cut loose from the episode | Beliefs: facts and lessons, with evidence |
| Procedural memory (basal ganglia) | Skills and habits that run without thinking | Skills with scripts, and **new: habits** |
| Prospective memory | "Remember to do X when Y" | **New: intentions** |
| Metacognition | Knowing what you know, and how sure you are | **New: self-model** from outcomes |
| Sleep consolidation | Replay of the day, patterns move from episodic to semantic | **New: consolidation in the daily tidy-up** |
| Prediction error (dopamine) | Learning is strongest when the outcome surprises | Outcome signals weighted by surprise |

The key idea from the human model is **complementary learning systems**: a
fast store that records single experiences (episodes), and a slow store that
only takes on what repeats across experiences (beliefs, habits). Poiesis today
skips the fast store and writes straight into the slow one from one
conversation. That is why one bad moment can become a permanent rule.

## I.3 The flaws of human memory, and what we do instead

This is the part where the harness can be better than a person.

| # | Human flaw | Design response |
|---|---|---|
| F1 | **Source amnesia.** You remember a fact but not where it came from | Every belief links to the episodes that support it. "Where did you get that?" always has an answer |
| F2 | **Recall rewrites the memory** (reconsolidation) | Episodes are immutable. A belief changes only by a new version that supersedes the old one, and the old one stays readable |
| F3 | **Confabulation.** Gaps get filled with plausible inventions, with full confidence | A belief carries a confidence that is checked against outcomes. Low or contested beliefs are said as "I think", with a date |
| F4 | **Forgetting by decay.** Useful things fade just because they are old | Forgetting is by usefulness and by contradiction, never by age alone. Forgotten means archived, and the archive is searchable |
| F5 | **Cannot forget on purpose.** One painful event dominates | One episode never creates a durable rule (except a hard fail-then-fix). The user can forget anything in one click |
| F6 | **Confirmation bias** | A new note is checked against what is already believed. Contradictions are surfaced, not merged away |
| F7 | **Over-generalising from few cases** | Promotion ladder: a pattern must show up in 2+ episodes before it becomes a belief, 3+ before it becomes a habit |
| F8 | **Hindsight bias.** "I knew it all along" | The prediction is stored before the outcome (the decisions table already does this). Learning compares the two |
| F9 | **Recency and availability bias** | Recall ranks by relevance times usefulness, not by what is newest |
| F10 | **Forgetting to remember** (prospective failure) | Intentions are stored with explicit triggers and fired by the scheduler or the recall pass, not by luck |
| F11 | **Bad habits stick** | Every habit and skill is scored on every use and can be unlearned |
| F12 | **Tip of the tongue.** The memory is there but the cue is wrong | Recall uses several cues at once: words, meaning, people and projects, task type, time |
| F13 | **Contexts bleed into each other** | Memory is scoped by project and persona, and a recall says which scope it came from |
| F14 | **Cannot look inside your own mind** | Every recall is visible in the turn, with why it was recalled. The user can correct it right there |
| F15 | **Cannot test a belief safely before acting** | Rehearsal: replay the old situation with the new belief, with no tool executed, and see if behaviour changes |
| F16 | **Sleep cannot be scheduled, and is not optional** | Consolidation runs when idle, is bounded, and the user sees what it did |

## I.4 What we take from Harness² and HyperSkill

From **Harness²** (Google, Oct 2026):

- Evidence discipline for any learned change: verdicts `helped`, `hurt`,
  `no observed effect`, `confounded`. "Nothing to learn" is a valid result.
  Being adopted is not evidence that a change helped.
- A lesson has a `cue`: the shape of situation it applies to, not this
  conversation's facts.
- Prefer a mechanism over prose. A script or a triggered check beats a
  sentence in the always-on prompt. If you add always-on text, cut some.
- Workspace facts (verified, labelled as such) are separate from procedure.
- Test a change by running it, not by reading it. We cannot rerun live tasks,
  so we rehearse without side effects (Part III.6).
- **Two loops, not one.** Harness² improves the setup for the task in front
  of it (task level), and it also improves the process that makes those
  improvements (improver level). Poiesis today has neither: it writes notes.
  Part III.9 to III.12 builds both:
  - Task level: before a task starts, the harness assembles the best setup
    it has for this kind of task (III.10).
  - Improvement step: Propose, Probe, Compose. Several candidate changes
    compete in rehearsal and only the evidence picks the winner (III.9).
  - Improver level: an improvement record of which kinds of change helped
    for which kinds of problem, read before every new proposal (III.11).
  - The editable surface: the explicit list of what the harness may change
    about itself, and the gate on each (III.12).

From **HyperSkill** (Aug 2026):

- Every memory item has a usefulness score from its own track record, and is
  pruned on that score.
- Learn from success (skills) and from failure (mistakes), both.
- Retrieve few items. Their best setting was 2.
- Unmaintained memory is worse than no memory.

Not taken: rerunning the user's task (side effects), ground-truth labels (we
have none), hypergraph co-occurrence merging (needs hundreds of similar
tasks; a personal assistant's stream is too sparse and too varied).

---

# Part II - The architecture

```text
            ┌───────────────── awake (during a run) ─────────────────┐
  input ──▶ │ Working memory  ◀── Recall (cues, budget, why) ◀── stores │
            │      │                                                  │
            │      ▼                                                  │
            │   act ──▶ outcome signals ──▶ Session log (raw record)  │
            └──────────────────────────────┬─────────────────────────┘
                                           │ end of conversation
                                           ▼
                                 Encode: one Episode
                                           │
            ┌──────── idle (built-in daily job, Tasks page) ─────────┐
            │ Consolidate: replay episodes ─▶ promote / merge /       │
            │ contradict / score / archive                            │
            │                                                         │
            │ Improve: problem ─▶ Propose 2-3 candidates ─▶ Probe     │
            │ (rehearse, no tools run) ─▶ Compose ─▶ gate ─▶ ship     │
            │        ▲                                   │            │
            │        └──── Improvement record ◀──────────┘            │
            └─────────────────────────────────────────────────────────┘
   next task: Prepare = load the setup bundle for this project / task kind
   stores: Episodes · Beliefs (facts, lessons) · Procedures (skills,
           habits) · Intentions · Self-model · Bundles · Improvement record
```

## II.1 The stores

### Episodes (new, SQLite, derived)

One row per finished conversation (or per background run). Immutable. It is
the bridge between the raw log and everything learned.

```sql
CREATE TABLE IF NOT EXISTS episodes (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  project_id      TEXT,
  persona_id      TEXT,
  task_kind       TEXT,              -- TaskKind purpose (or its rule fallback): 'report', 'code', 'mail', ..., 'unknown'
  goal            TEXT NOT NULL,     -- one line: what the user wanted
  outcome         TEXT NOT NULL,     -- done | partial | abandoned | failed | unknown
  signals_json    TEXT NOT NULL,     -- deterministic: failures, fixes, rewinds, verify verdicts, user reactions
  summary         TEXT NOT NULL,     -- <= 600 chars: what happened
  worked          TEXT,              -- <= 3 lines: what went well, concretely
  went_wrong      TEXT,              -- <= 3 lines
  entities_json   TEXT,              -- people, files, accounts, projects named
  artifacts_json  TEXT,              -- artifact ids, scripts that ran successfully
  candidates_json TEXT,              -- observations NOT yet promoted (Part III.4)
  digest_version  INTEGER NOT NULL,
  consolidated_at INTEGER,           -- NULL until the daily tidy-up has read it
  created_at      INTEGER NOT NULL
);
```

Episodes get FTS and a vector, like messages. Deleting a conversation
deletes its episode. Evidence links in beliefs that point to a deleted
episode stay, marked "source deleted".

### Beliefs (facts and lessons, markdown, user-owned)

Same files as today. New optional frontmatter, backwards compatible:

```yaml
evidence: [ep_3f2a, ep_91c0]   # episodes that support it
confidence: medium             # low | medium | high
status: active                 # active | contested | superseded | archived
supersedes: lives-in-hamburg   # the version this one replaced
valid_from: 2026-09-12
cue: "reading a spreadsheet export from a bank"   # lessons only
```

A superseded belief moves to `memory/archive/` with `superseded_by` set. It
is not in any prompt, but `search_history` and the Memory page can show it
("You used to live in Hamburg, until 12 Sep").

### Procedures

- **Skills** (exist): gain bundled scripts when the agent proposes them
  (Part III.5).
- **Habits** (new): a check that runs by itself at a fixed point in the tool
  loop, instead of a sentence the model may ignore. Stored as
  `memory/habits/<slug>.md`, frontmatter plus a short rule. Three kinds,
  smallest first:
  1. `note`: when tool X is about to be called and the cue matches, the
     note is put into that tool call's context just in time. Not always-on.
  2. `check`: after tool X returns, a deterministic check runs (for example
     "the CSV has a header row where the habit says"). A failed check is
     handed to the model as a tool note.
  3. `rule`: a deterministic argument rule (for example "paths under
     Downloads/exports: read with header_row=3"). Only the user can turn
     a habit into a `rule`.

### Intentions (new)

```sql
CREATE TABLE IF NOT EXISTS intentions (
  id              TEXT PRIMARY KEY,
  text            TEXT NOT NULL,      -- what to do, first person
  trigger_kind    TEXT NOT NULL,      -- time | project_open | topic | mail_from | task_kind
  trigger_value   TEXT NOT NULL,
  source_conv     TEXT,
  status          TEXT NOT NULL,      -- waiting | fired | done | dropped
  expires_at      INTEGER,
  created_at      INTEGER NOT NULL
);
```

### Self-model (new, derived)

Per `task_kind` and model: how many episodes ended `done` versus `failed` or
`abandoned`, and the decision-model calibration that already exists. It is
read, not written by hand. It drives Part III.7.

### Usage and credit (new, SQLite)

The piece that makes everything else measurable.

```sql
CREATE TABLE IF NOT EXISTS memory_uses (
  id              TEXT PRIMARY KEY,
  item_kind       TEXT NOT NULL,      -- fact | lesson | habit | skill | episode
  item_ref        TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  run_id          TEXT,
  how             TEXT NOT NULL,      -- always | recalled | fired | tool | bundle
  why             TEXT,               -- the cue that matched, in words
  outcome         TEXT,               -- helped | hurt | no_effect | unknown
  outcome_source  TEXT,
  at              INTEGER NOT NULL
);
```

`skill_runs` and `memory_usage` fold into this over time. Old readers keep
working until they are moved.

---

# Part III - The processes

## III.1 Encode (end of every conversation)

Replaces the start of today's reflection, at about the same cost, and runs
exactly where reflection runs today (`reflect_conversation_cmd`):

- when the user leaves a chat with auto-reflect on (`reflectOnLeaving` in
  `store.ts`), fire-and-forget;
- by hand (Rail, Self panel, the `/` command);
- as catch-up in the built-in daily job (III.4) for every finished
  conversation that was never reflected on.

No new trigger. A conversation is encoded once; `reflected_at` stays the
marker.

1. **Deterministic first, no model.** Collect signals: tool failures,
   fail-then-fix pairs, rewinds, edits of the agent's output, verify
   verdicts, decisions with outcomes, "thanks"/"no" style reactions already
   caught by `user_signals`, artifacts kept, scripts that ran clean.
2. **Outcome.** `done` if the last user turn accepts or moves on with no
   correction and nothing failed at the end. `abandoned` if rewound or left
   mid-task. `failed` if the last tool or verify step failed. Else
   `partial` or `unknown`. Rules, not a model.
3. **One local model call** writes `goal`, `summary`, `worked`,
   `went_wrong`, `entities`, and up to 3 `candidates`. The prompt uses the
   Harness² contract: every candidate names its evidence (a signal or a
   turn), `cue` is the shape of the situation, `NO_ENTRY` is fine.
4. Candidates stay in the episode. They are not beliefs yet (F5, F7).
   One exception keeps today's behaviour: a hard fail-then-fix pair may
   promote at once, because it is direct evidence.

## III.2 Recall (before and during each turn)

1. **Cues.** Words (FTS), meaning (vector), entities named in the message
   (people, projects, files), current project and persona, task kind (the
   `TaskKind` purpose), time words ("last week").
2. **Fusion.** Reciprocal-rank fusion over the cues, then multiply by
   usefulness (Part III.3). Small budgets: at most 3 lessons, 5 topical
   facts, 1 episode. Always-on stays small: SOUL, profile, global facts
   ranked by usefulness under the existing cap.
3. **Why.** Each recalled item carries the cue that matched, in words
   ("same project", "you named Anna", "last time you made a report").
4. **Log.** Each recalled item writes a `memory_uses` row.
5. **Habits are not recalled into the prompt.** They fire in the tool loop.
6. **Source and certainty in the answer.** If the reply relies on a belief
   with `confidence: low` or `status: contested`, the prompt says so, and
   the model says "I think" with the date (F3).

## III.3 Credit (after each conversation)

Every `memory_uses` row gets an outcome, by rules, weak labels allowed:

| Item | helped | hurt | no_effect |
|---|---|---|---|
| Lesson with a tool cue | The cued tool ran and the failure it warns about did not happen | The same failure happened anyway, or the user rewound the turn | The cued situation did not come up |
| Fact | Used in the answer, no correction | The user corrected it ("no, I moved") | Not used |
| Habit | Fired, and the check caught something the model then fixed | Fired, and the user undid the result | Fired, nothing to catch |
| Skill | Run, episode `done` | Run, episode `failed` or `abandoned` | - |
| Episode ("last time") | User chose "same way" and episode `done` | User chose "start fresh" | Not shown |

Usefulness = (helped + 1) / (helped + hurt + 2), with the count of real
chances beside it. An item with no chances yet sits at 0.5 and is not judged.

## III.4 Consolidate (the built-in daily job)

Poiesis is a desktop app, not a server: nothing runs while it is closed. So
there is no separate night job. Consolidation goes **into the job that already
holds today's reflection**: the built-in `nightly-reflection` job
(`commands/scheduler.rs`, `run_nightly_reflection_digest`), shown at the top
of the Tasks page (`routes/Tasks.tsx`).

What that job already gives us, unchanged:

- The 60-second ticker runs only while the app is open. A job is due when
  `next_run_at` is in the past, so after a closed night it runs about a
  minute after the app opens. Catch-up is free.
- Concurrency 1, a Stop button, a stop check between items, a digest
  (`scheduler.digest`) and an Activity line.
- It already sweeps expired facts (TTL-2) and reflects on up to 8
  unreflected conversations.

What the job does after this plan, in order, all in one pass:

1. Expired facts (as today).
2. Encode the unreflected conversations (III.1, the cap of 8 stays).
3. Consolidate: steps 1 to 8 below, at most 5 episodes per pass.
4. From Phase 6: the improvement step (III.9), at most 2 problems per pass.
5. One digest for all of it (IV.4).

What the job lacks today and this plan adds (`SLP-1`):

- **Idle start.** A due pass starts only after 2 minutes without input in
  the app, and not while a chat turn is streaming. Until then it stays due.
- **Stop on input.** Any key press or click in the app raises the job's
  cancel flag. The pass finishes the item in flight and stops; the rest
  stays due (`consolidated_at IS NULL`) for the next pass.
- **Words that are true.** It runs once a day when due, not at night. The
  name becomes **Daily tidy-up**, the toggle "Do this once a day", the
  description "Once a day, while you're not using me, I go over finished
  conversations, keep what I learned, merge duplicate notes and let unused
  ones go." The digest opens "While you were away" or "Earlier today", never
  "Last night". The job id `nightly-reflection` stays, so saved settings
  carry over.
- **On by default** once this plan's memory ships (today it is off, SCH-5).
  Without it memory never tidies itself. What it may write is still decided
  by the autonomy rungs, so with `Ask` it only leaves proposals. **Decision
  for Erich** before `SLP-1` is built.

Local engine only unless the user chose a cloud target for background work.

1. **Replay.** Read episodes with `consolidated_at IS NULL`.
2. **Group candidates.** Match each candidate against other episodes'
   candidates and against existing beliefs (vector plus the decision model
   choosing `same` / `refines` / `contradicts` / `unrelated`).
3. **Promotion ladder.**

   | Level | Needs | Lives in | In the prompt? |
   |---|---|---|---|
   | Observation | 1 episode | `episodes.candidates_json` | No |
   | Belief | 2+ episodes, or 1 hard fail-then-fix, or the user said it | `facts/`, `lessons/` | When recalled |
   | Habit / skill | 3+ episodes, or recurrence 3 of a lesson, and rehearsal passed | `habits/`, `skills/` | Fires in the loop |
   | Standing instruction | The user only | `SOUL.md` | Always |

4. **Contradiction.** `contradicts` with a user statement as the newer
   evidence: supersede. Otherwise mark both `contested` and put them in the
   Memory page's "Unsure" section. Never merge a contradiction away (F6).
5. **Merge duplicates.** `same` adds evidence to the existing belief, one
   file stays. Snapshot first, as today.
6. **Forget.** Archive a belief when it has 5+ chances and usefulness under
   0.3, or 120 days with no chance. Archived items are searchable. Expired
   TTL facts go as today.
7. **Skills from success.** 2+ `done` episodes of the same task kind, with
   similar `worked` lines: draft a skill (steps from `worked`, scripts from
   `artifacts_json` that ran clean). Goes through the `skills` rung.
8. **Journal and digest.** One first-person line per change, collected for
   the return moment (Part IV.4).

All writes go through the existing autonomy rungs: `lessons`, `skills`,
`consolidate`, and a new `habits` class (default `Ask`).

## III.5 Procedures grow up

- `propose_skill` takes optional `files` (path to content, `scripts/` only,
  size cap). A script is accepted only if the same content ran without error
  in this conversation (from `artifacts_json` or the code-run log).
- The RPT-2 escalation changes. At recurrence 3 the agent proposes a
  **habit** (note or check) instead of a SOUL line. SOUL escalation stays
  possible, but only as the user's choice in the proposal ("make it a
  standing instruction instead").
- Habits of kind `note` replace most tool-related lessons in the prompt.
  They cost nothing until the tool is called (Harness²: always-on text is
  paid on every run).

## III.6 Rehearse (the safe version of Harness² probing)

The question: does a candidate lesson or habit actually change behaviour in
the situation it came from? Rehearsal answers it without side effects.

1. The source episode's session log replays the exact message array up to
   the point of the mistake (`log.rs` replay exists).
2. Run the model on it twice per arm, without and with the candidate, in a
   **rehearsal sandbox**: no tool executes. The model's first tool call or
   answer is captured and the run stops.
3. Compare. For a fail-then-fix candidate: does the with-arm's call move
   toward `fixed_args` and away from `failed_args`? Deterministic where the
   args allow it, decision model otherwise.
4. Verdict: `helped` if the with-arm is right in 2 of 2 and the without-arm
   is wrong in at least 1. `no_effect` if both arms behave the same: the
   candidate is not needed, or not strong enough as prose (a sign to make it
   a habit). `hurt` if the with-arm gets worse.
5. Only `helped` candidates may reach the habit level. Beliefs without
   rehearsal are kept with `confidence: medium` at most.
6. The golden set still runs before and after (`GLD-2`), so a change that
   helps here but breaks something else is caught.

Cost: about 4 short local calls per candidate, in the daily tidy-up, capped
per pass.

## III.7 Know what you know

- **Self-model in the loop.** For a task kind where the self-model shows
  many `failed` or `abandoned` episodes, the agent verifies more (the
  existing `verify` and auto-check paths) and asks a clarifying question
  earlier. For strong areas it does not add checks.
- **Prediction before outcome** (F8). The decisions table already stores
  the prediction. Episodes link to it, so consolidation can compare what
  was expected with what happened.
- **Surprise weights learning.** A failure where the agent was sure counts
  more than one where it was unsure. A success where it was unsure counts
  more than an expected one.

## III.8 Intentions

- Created by a `remember_to` tool (gated by a new `intentions` class, `Ask`
  by default) or by the user from the `/` menu (`shared/commands.json`, no
  new composer syntax).
- Fired by: the scheduler (time), the recall pass (topic, project open,
  task kind), mail sync (mail from).
- A fired intention appears as a short first-person message in the right
  conversation, or opens a new one for time triggers.

## III.9 The improvement step: Propose, Probe, Compose

This is Harness²'s core step, moved from "during a live task" to "while
idle, on a replayed episode". It runs in the daily tidy-up (III.4, step 4 of
the pass) for every problem
that qualifies: a repeated mistake (recurrence 2+), a told-twice correction,
a task kind with 2+ `failed` or `abandoned` episodes, or a skill whose
usefulness dropped under 0.4.

1. **Propose: competing candidates.** For one problem, draft up to three
   changes on different parts of the setup:
   - **How I work** (Harness² INV): a habit `note` or `check`, a tool-use
     fix, a step added to a skill.
   - **What I know** (Harness² DOM): a belief, a domain rule inside a
     skill, a verified workspace fact for this project ("in this project,
     invoices are in `/Belege/2026`").
   - **What already worked**: copy a component from the improvement record
     (III.11) or from another project's bundle (III.10) that fixed a similar
     problem before. Its workspace facts are re-checked, never trusted.

   Each candidate must name the episode moment it is aimed at. A candidate
   with no evidence is not drafted.
2. **Probe: rehearse all of them.** Each candidate and the unchanged setup
   run in the rehearsal sandbox (RHS-1) on the source episodes: up to 3
   episodes, 2 samples each. No tool executes.
3. **Compose: let the evidence decide.** A compose call reads the
   rehearsal results and picks one of:
   - **Adopt one** candidate.
   - **Merge** two that each fixed a different part. The merge is rehearsed
     once more before it counts.
   - **Keep as is** (Harness² `KEEP_BASE`). A real answer, not a failure.

   Every decision is one line with its evidence, in the Harness² shape:
   `kept|merged|dropped -> <candidate> -> <what the rehearsal showed>`.
4. **Gate and ship.** The winner goes through the golden guard (`GLD-2`)
   and the autonomy rung of its class. The proposal card shows the
   competition ("I tried 3 fixes. The check worked in 4 of 4 replays, the
   written lesson in 1 of 4, so I'm proposing the check.").
5. **Record.** The result, including losing candidates, goes into the
   improvement record (III.11). Losers are evidence too.

Bounds: at most 2 problems per pass, at most 3 candidates each, local
engine, cancelled by user input.

## III.10 Task-level setup: assemble before acting

Harness² starts each task by adapting the best setup it has. Poiesis does
the same at the start of a conversation, cheaply and without extra runs.

1. **Setup bundle.** For each project, and for each task kind seen 3+
   times, the harness keeps a bundle: the project instructions, the skills,
   habits, beliefs and persona that were active in its `done` episodes,
   with usefulness. A bundle is a list of references, not a copy.
2. **Prepare.** When a conversation starts and the task kind is known
   (`TaskKind`) or the project is known, the bundle's skills and habits are
   switched on for this run. Beliefs from the bundle get a recall boost.
   Cost: no model call, only a lookup.
3. **Adapt on the fly.** If the self-model (III.7) marks this task kind as
   weak, the run also gets the verify path and an early clarifying question.
4. **Carry over to new projects.** When a new project looks like an
   existing one (name, folder contents, first task kind), offer its bundle.
   Workspace facts are left out; only procedure and skills travel
   (Harness² rule: facts are per workspace, procedure is per domain).

## III.11 The improver learns: the improvement record

Harness²'s second loop: the process that makes changes also improves. In
Poiesis this is a record of every self-change attempt and what it did, read
before every new proposal.

```sql
CREATE TABLE IF NOT EXISTS improvements (
  id              TEXT PRIMARY KEY,
  problem_kind    TEXT NOT NULL,     -- repeat_mistake | told_twice | weak_task_kind | rough_skill
  problem_cue     TEXT NOT NULL,     -- the shape of the situation, Harness² 'cue'
  tool_or_area    TEXT,              -- e.g. 'read_file', 'mail', 'report'
  candidate_form  TEXT NOT NULL,     -- lesson | habit_note | habit_check | skill_step | belief | workspace_fact | copied
  candidate_ref   TEXT,
  rehearsal       TEXT,              -- helped | hurt | no_effect | confounded | not_run
  decision        TEXT NOT NULL,     -- adopted | merged | dropped | kept_base
  live_outcome    TEXT,              -- filled later from memory_uses: helped | hurt | no_effect
  evidence        TEXT NOT NULL,     -- one line, the Harness² notes shape
  created_at      INTEGER NOT NULL
);
```

How it is used:

1. **Before proposing** (III.9 step 1): read the records with a similar
   `problem_cue` or the same `tool_or_area`. If one form keeps winning
   ("for `read_file` problems, `habit_check` helped 5 of 5, `lesson` 1 of
   4"), draft that form first and skip forms that keep losing.
2. **After going live:** CRD-2 fills `live_outcome`. A change that helped in
   rehearsal but not live lowers trust in rehearsal for that area, and the
   next proposal in that area needs more replays.
3. **Rules of thumb.** Once a month the daily tidy-up turns the record into at
   most 5 one-line rules for itself ("written lessons rarely fix tool
   argument mistakes; use a check"). These rules go into the propose and
   compose prompts only, never into the chat prompt. They are listed on the
   Memory page under "How I improve", and the user can delete any of them.
4. **Calibrating the gates.** If rehearsal verdicts and live outcomes agree
   for an area 5 times in a row, that area may promote to `Auto` when the
   user's rung for the class is `Auto`. If they disagree, the area drops to
   `Ask`.

This is what makes the system learn how to learn: the first fix for a new
kind of problem is a guess, the tenth is informed by nine results.

## III.12 The editable surface

What the harness may change about itself, mapped to Harness²'s eight parts.
Anything not in this table it may not change.

| Harness² part | Poiesis part | Changed by | Gate |
|---|---|---|---|
| System prompt | `SOUL.md` | Proposal only | User accepts, always |
| Guardrails | Habits `check` and `rule` | Improvement step | `habits` rung; `rule` needs the user |
| Memory | Beliefs, profile, workspace facts | Encode, consolidate, improvement step | `lessons`, `consolidate` rungs |
| Tool guidance | Habits `note` (just in time per tool) | Improvement step | `habits` rung. Built-in tool descriptions stay fixed (prompt golden gate) |
| Skills | Skills, App-source copies | Improvement step, success path | `skills` rung. User files never edited in place |
| Scripts | `scripts/` inside skills | Only scripts that ran clean | `skills` rung |
| Subagents | Persona per task kind in a bundle (III.10) | Bundle choice only, never persona text | User picks |
| Plugins / middleware | Habit firing points in the tool loop | Fixed set of points in code | Not editable by the harness |

---

# Part IV - What the user feels

Each of these is a concrete moment. If a phase does not produce its moment,
the phase is not done.

## IV.1 "Last time" (episodes)

Start a conversation that matches a past `done` episode of the same task
kind. Above the composer, in the composer-hint slot (reuse the `SEE-3`
component):

> Last time (12 Oct) I made the expense summary per category from your
> Sparkasse CSV. **Do it the same way** · **Start fresh**

"Same way" adds the episode digest and its skill (if any) to the turn.

## IV.2 "You only have to tell me once" (habits)

The user corrected the header row in October. In November, during a run, the
timeline shows one quiet step:

> Checked the header row first. You showed me on 4 Oct.

## IV.3 "Why did you think that?" (visible recall)

Every turn that used memory has a collapsed row in the timeline:

> Remembered 2 things

Open it: each item shows its line, why it was recalled, its source
conversation and date as a link, and two buttons: **Outdated** and **Not
relevant here**. "Outdated" opens one inline line: "What is right now?". The
answer supersedes the belief. No dialog.

## IV.4 "While you were away" (consolidation)

In the return-moment slot (`PRES-5`):

> While you were away I went over 3 conversations. I learned 1 thing,
> merged 2 notes, and let 1 old one go. **Show me**

"Show me" opens the Memory page on "Recent changes", each change undoable.

## IV.5 The Memory page as a self, not a list

Sections, in this order:

1. **About you**: global facts, profile.
2. **Your projects and people**: topical facts grouped by entity.
3. **How I work**: habits, lessons, skills I made.
4. **Things I'll do**: waiting intentions.
5. **Unsure**: contested beliefs, each with both sides and "Which is right?".
6. **Archive**: superseded and let-go items, searchable.

Each card: the statement; "from <conversation title>, <date>" as a link;
"helped 4 times, last 3 days ago" or "not tested yet"; certainty in words
(`sureWord`); "Earlier versions" when superseded versions exist.

## IV.6 "How I work" self-portrait

At the top of "How I work", generated from the self-model, in words not
numbers:

> I'm reliable with reports and file clean-up. I often get spreadsheet
> imports wrong on the first try, so I check them before I go on.

## IV.7 Health line

In Settings > Health (existing tab), four lines, month over month:

> Mistakes I repeated: 2 (last month 5)
> Things you had to tell me twice: 1 (last month 3)
> Memory I carry in every message: 1.1k characters (last month 2.4k)
> Fixes I made that held up: 7 of 8 (last month 3 of 6)

## IV.8 "I tried 3 fixes" (the improvement step)

A proposal from the improvement step uses the existing proposal card, with
one added block, fixed height, collapsed to its first line:

> I kept reading your bank exports wrong. I tried 3 fixes on the
> conversations where it happened:
> - a check before reading: right in 4 of 4 replays
> - a written reminder: right in 1 of 4
> - the import skill from your *Taxes* project: right in 3 of 4
>
> I'm proposing the check. **Use it** · **Not now** · **Show replays**

"Show replays" lists the replayed conversations as links with the first
action each candidate took. With `Auto` on for the class, the same text
appears in the return digest (IV.4) with **Undo** instead of **Use it**.

## IV.9 "Start with what worked there" (bundles)

When a new project looks like an existing one, the project's empty state
shows:

> This looks like your *Client website* project. I can start with what
> worked there: 2 skills, 3 checks, the *Web dev* persona.
> **Start with that** · **Start empty** · **Show what's in it**

When a conversation starts with a known bundle, the timeline's first step
says, collapsed: "Set up for reports: 1 skill, 2 checks." Expand shows the
list, each item with an off switch for this conversation.

## IV.10 "How I improve"

A section on the Memory page, after "How I work": the rules of thumb from
III.11 as plain sentences, each with its evidence count and a delete
button:

> Written reminders rarely fix how I call a tool. I use checks for that
> now. (5 of 5 times)

---

# Part V - Measuring it (do this first)

Self-learning that is not measured is a story. These numbers decide whether
each phase stays. The first five exist from Phase 0; the last three start
with Phase 6.

| Metric | Definition | Goal |
|---|---|---|
| Repeat-mistake rate | Same tool + same error class in a later conversation, after a lesson or habit about it exists | Goes down |
| Told-twice rate | A correction by the user that matches an existing belief or one already corrected | Goes down |
| Always-on memory size | Characters of memory in every prompt | Goes down while the store grows |
| Recall precision | Recalled items with outcome `helped` or `no_effect`, versus `hurt` and "Not relevant here" clicks | Goes up |
| Stale belief rate | "Outdated" clicks per 100 recalls | Goes down |
| Fix hold rate | Shipped self-changes whose `live_outcome` is `helped`, out of all with a live outcome | Goes up |
| Rehearsal agreement | Rehearsal verdict matches live outcome | Goes up; if it stays under 60%, rehearsal is not trusted for that area |
| First-fix rate | Problems fixed by the first shipped change, per quarter | Goes up over time: this is the "learns how to learn" number |

---

# Part VI - Tasks

Prefixes: `MEV` measure, `EPI` episodes, `CRD` credit, `CUE` recall, `BLF`
beliefs, `SLP` consolidation, `HAB` procedures and habits, `RHS` rehearsal,
`IMP` improvement loop, `BND` setup bundles, `INT` intentions, `MET`
self-model.

## Phase 0 - Baseline

### MEV-1 - The numbers

Compute the five metrics of Part V from existing data (`tool_fixes`,
`tool_stats`, `user_signals`, rewinds, memory sizes). One function, a test
per metric with synthetic rows. Store a monthly snapshot in `settings`.

**Accept:** the function returns all five numbers on the dev database, and a
test with planted repeats counts them correctly.

### MEV-UI-1 - Health lines

The three lines of IV.7 in the Health tab. "No data yet" when under 2 weeks.

## Phase 1 - Episodes and credit (the foundation)

### EPI-1 - Schema and store

`episodes` table, FTS, vector job (reuse the embed queue). Delete cascades.

### EPI-2 - Encode

Deterministic signals and outcome rules (III.1 steps 1 and 2), then the
model call (step 3). Runs where reflection runs today; reflection's draft
call is replaced by this call, so the cost stays the same. Keep the critic
and the fail-then-fix fast path.

**Accept:** a conversation with one fail-then-fix and an accepting last turn
produces an episode with `outcome = done`, the fix in `signals_json`, and a
candidate that cites it. A rewound conversation produces `abandoned`.

### CRD-1 - Usage rows

`memory_uses` table. `recall_for` and the always-on index write rows. Skill
activations write rows (keep `skill_runs` until CRD-3).

### CRD-2 - Outcome attribution

The rules of III.3, run at the end of encoding. Usefulness computed on read.

**Accept:** a lesson cued to `read_file`, a later conversation where
`read_file` runs clean: one `helped` row. Same lesson, same failure again:
one `hurt` row.

### CRD-3 - Fold old tables

`skill_runs` and `memory_usage` readers move to `memory_uses`. Old tables
stay until a later migration removes them.

### EPI-UI-1 - "Last time"

The IV.1 hint. Shown when episode similarity is over a threshold (measured,
start at the SEM floor), same task kind, outcome `done`, less than 180 days
old. Max once per conversation. "Same way" logs `helped`/`hurt` via CRD-2.

**Accept (felt):** do a report task, finish it, start the same kind of task a
day later in a new chat: the hint appears with the right date and goal.

## Phase 2 - Recall you can see

### CUE-1 - Multi-cue recall

Entity, task-kind and time cues added to the existing FTS and vector
retrieval, fused by RRF, ranked by usefulness. Budgets from III.2.

### CUE-2 - Why

Every recalled item carries its cue in words. Sent with the existing
retrieval event (`SEM-5`).

### CUE-3 - Uncertain beliefs said as uncertain

Low-confidence or contested beliefs reach the prompt with a marker that
makes the model hedge and give the date.

### CUE-UI-1 - "Remembered" row

The IV.3 row in the timeline, collapsed by default. "Outdated" and "Not
relevant here" as described. Reuse the timeline step component.

**Accept (felt):** state "I live in Hamburg", later say "I moved to Berlin"
via "Outdated" on the recalled fact: the next recall says Berlin, the Memory
page shows Hamburg under "Earlier versions".

**Gate after Phase 2:** compare MEV numbers with Phase 0. If recall precision
did not rise, fix recall before building Phase 3.

## Phase 3 - Beliefs with evidence

### BLF-1 - Frontmatter and archive

New fields (II.1), `memory/archive/`, supersede and restore functions. Old
files without fields read as `active`, `confidence: medium`, no evidence.

### BLF-2 - Relation check on write

Every write of a fact or lesson checks the top 5 similar beliefs with the
decision model: `same` / `refines` / `contradicts` / `unrelated`. Applies
III.4 step 4 and 5 at write time, not only in the daily tidy-up.

**Accept:** save "prefers tea" then "prefers coffee": the second is either a
supersede (user said it) or both become `contested`. Never two silent
actives.

### BLF-UI-1 - Memory page sections

The six sections of IV.5. Card layout as described. Reuse the existing
Memory page shell; fixed card height with "More" for long bodies.

### BLF-UI-2 - "Unsure" resolution

Each contested pair shows both sides, the source of each, and two buttons
(one per side) plus "Both, in different contexts" (sets scope).

## Phase 4 - Consolidation

### SLP-1 - Consolidation in the built-in daily job

No new job. Extend `run_nightly_reflection_digest` (`commands/scheduler.rs`)
with III.4 steps 1 to 6, after encoding, with the bounds given there. Add the
idle start and the stop on input (III.4), rename the job and its copy in
`routes/Tasks.tsx`, keep the id. Default on is Erich's decision (III.4).

**Accept:** with the app open and untouched for 2 minutes, a due pass runs;
a key press during it stops it after the current item, and the next pass
picks up the rest. Closed overnight, the pass runs within a minute or two of
opening once the app is idle.

### SLP-2 - Skills from success

III.4 step 7. Uses HAB-1 for scripts.

### SLP-3 - Journal and return digest

III.4 step 8 and the IV.4 moment. Extends the job's existing digest
(`scheduler.digest`, `compose_digest`) instead of adding a second one, with
the same rule: never claim more than was done. Reuses `OPT-12`'s journal
file format if it exists, otherwise defines it here.

**Accept (felt):** three conversations with the same small correction, leave
the app idle: on return the digest says one thing was learned, and the
Memory page shows one belief with 3 evidence links.

## Phase 5 - Procedures and rehearsal

### HAB-1 - Scripts in proposed skills

`propose_skill` gains `files` (III.5). Validation: `scripts/` only, size
cap, content must match a script that ran clean in this conversation.

### HAB-2 - Habits store and the `note` kind

`memory/habits/`, the `note` kind fired just in time at tool dispatch.
Autonomy class `habits`, default `Ask`.

### HAB-3 - `check` kind

Deterministic post-tool checks from a small fixed set (header row, path
exists, file non-empty, JSON parses, row count matches). No model-written
code runs as a check.

### HAB-4 - New escalation

RPT-2 proposes a habit instead of a SOUL line. The proposal card offers
"Make it a standing instruction instead".

### RHS-1 - Rehearsal sandbox

A run mode where no tool executes and the run stops at the first tool call
or answer. Built on the session-log replay. Every side-effect tool,
including connectors and mail, is stubbed, not just file writes.

### RHS-2 - Rehearsal of one candidate

III.6 for each candidate that wants the habit level, against the unchanged
setup. This is the single-candidate case of the improvement step; Phase 6
widens it to competing candidates.

**Accept:** a planted fail-then-fix episode, a candidate `check` for it: the
with-arm matches `fixed_args` in 2 of 2, the without-arm repeats the
failure, the verdict is `helped`.

### HAB-UI-1 - Habit step

The IV.2 step in the timeline when a habit fires.

**Gate after Phase 5:** repeat-mistake rate must have dropped against Phase
0. If it did not, stop and find out why before Phase 6.

## Phase 6 - The self-improvement loop

### IMP-1 - Problem queue

The daily tidy-up collects problems (III.9 opening list) from episodes,
`memory_uses` and recurrence counts, deduplicated by cue, ranked by how
often they hurt the user (told-twice first, then repeat mistakes, then weak
task kinds, then rough skills).

### IMP-2 - Propose

III.9 step 1: up to 3 candidates on different parts of the editable surface
(III.12), each naming its target episode moment. Reads the improvement
record first (IMP-5). Prompt in the Harness² shape: evidence required,
`KEEP_BASE` allowed, workspace facts labelled.

### IMP-3 - Probe

III.9 step 2 on RHS-1: every candidate and the unchanged setup, up to 3
source episodes, 2 samples each. Results stored per candidate.

### IMP-4 - Compose and ship

III.9 steps 3 and 4: adopt, merge (rehearsed again) or keep as is, one
evidence line per decision. Winner through `GLD-2` and the class rung.

**Accept:** a planted problem with three candidates where only the `check`
fixes the replay: the `check` is proposed, the other two are recorded as
`dropped` with their rehearsal results. A planted problem where no
candidate beats the unchanged setup: nothing is proposed, one `kept_base`
row.

### IMP-5 - Improvement record

The `improvements` table (III.11), written by IMP-4 including losers.
`live_outcome` filled by CRD-2. Read by IMP-2 to order candidate forms.

### IMP-6 - Rules of thumb and gate calibration

III.11 steps 3 and 4: monthly, at most 5 one-line rules into the propose and
compose prompts only. Per-area agreement between rehearsal and live outcome
moves that area between `Ask` and `Auto` (never above the user's rung).

**Accept:** seed 5 records where `habit_check` helped and `lesson` did not
for `read_file`: the next `read_file` problem drafts the check first, and
the rules list contains a sentence saying so.

### BND-1 - Setup bundles

III.10 step 1: per project and per task kind with 3+ episodes, a list of
references to the skills, habits, beliefs and persona active in `done`
episodes, with usefulness. Rebuilt by the daily tidy-up.

### BND-2 - Prepare

III.10 steps 2 and 3 at conversation start: no model call, a lookup. Each
item switched on writes a `memory_uses` row with `how = 'bundle'`. Step 3
(weak task kinds) needs MET-1; until Phase 8 it uses the raw count of
`failed` and `abandoned` episodes for the task kind.

### BND-3 - Carry over

III.10 step 4: similarity between a new project and existing ones, bundle
offered without workspace facts.

### IMP-UI-1 - "I tried 3 fixes"

The IV.8 block on the proposal card, and its digest version.

### BND-UI-1 - Bundle moments

The IV.9 empty-state offer and the "Set up for ..." timeline step with
per-item off switches.

### IMP-UI-2 - "How I improve"

The IV.10 section. Deleting a rule records it so it is not regenerated from
the same records.

**Gate after Phase 6:** fix hold rate must be above 60% and rehearsal
agreement above 60% in at least one area. If not, ship Phase 6 changes only
under `Ask` and fix rehearsal before Phase 7.

## Phase 7 - Intentions

### INT-1 - Store, tool, `/` entry

Table, `remember_to` tool, `/remind` in `shared/commands.json`.

### INT-2 - Triggers

Scheduler (time), recall pass (topic, project, task kind), mail sync (from).

### INT-UI-1 - Chips and list

"I'll remember to ..." chip when created, "Things I'll do" section, the fired
message.

## Phase 8 - Self-model

### MET-1 - Self-model

Per task kind and model, from episodes and decision calibration.

### MET-2 - Behaviour

Weak area: more verify, earlier clarifying question. Strong area: no extra
checks. Surprise weighting in CRD-2 and SLP-1.

### MET-UI-1 - Self-portrait

The IV.6 paragraph, regenerated weekly, in words.

---

# Part VII - Risks

| Risk | Answer |
|---|---|
| Local model cost | Encoding replaces the reflection call. The daily tidy-up is capped, starts only when idle and stops on input. Rehearsal only for habit candidates |
| Weak, noisy credit labels | Usefulness needs real chances before it acts. Archive needs 5+ |
| Over-building | Phase 0 numbers first, two hard gates. A phase that moves no number is cut |
| Privacy | Episodes are local, deleted with their conversation, never sent unless the user picked a cloud target for background work |
| Users confused by more memory UI | Everything new is either in the timeline (collapsed) or on the Memory page. No new panels |
| The loop drifts: many small self-changes that each pass but together make it worse | Every shipped change goes through `GLD-2`. The improvement record links each change to its live outcome, and a monthly check archives changes whose live outcome is `hurt` or `no_effect` after 5 chances |
| Rehearsal says "helped" but live use does not | Rehearsal agreement is measured per area (Part V). Low agreement moves the area to `Ask` and asks for more replays |
| The improver learns wrong rules of thumb | At most 5, visible, deletable, used only in improvement prompts, never in the chat prompt |
| Contested beliefs pile up | "Unsure" shows a count in the Memory nav; older than 30 days without an answer, the newer one wins and the older is archived |

# Part VIII - Not in this plan

- Training model weights.
- Rerunning live user tasks.
- Hypergraph or graph-database storage.
- Sharing memory between users or devices (export exists; sync is a
  separate decision).
