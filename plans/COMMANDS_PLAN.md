# Project Poiesis - Commands Plan

**A `/` button beside the `+`. `+` adds things to the message, `/` does things.
The user controls the harness with commands, and the agent uses the same
commands on itself, inside the membrane.**

Today the `/` key only lists skills, and everything else the composer can do
(Workspace mode, Tools, Create image, Create video, Skills, Start from a skill,
Persona) hangs off the `+` next to "Attach files". The harness underneath has
grown much further than that menu shows: steering, forking, resuming, compaction,
plans, delegation, background agents, a session log, usage, undoable file
changes, reflection, golden checks and an autonomy ladder. Most of it can only
be reached by hovering a finished turn or opening a settings page, and the
agent cannot reach any of it except `plan`, `delegate` and `memory`.

This plan adds:

1. **One command registry** that both the user and the agent read.
2. **A `/` button and a real command menu** in the composer, with `+` reduced
   to adding content.
3. **About 45 commands**, most of them bindings to store actions that already
   exist.
4. **Commands the agent runs on itself**: a `harness` tool, an `ask_user` tool,
   and context clearing in the middle of a run. Each one is gated by the
   autonomy ladder.
5. **Five new harness capabilities** that SOTA agents have and Poiesis lacks:
   *Plan first* (read-only runs that need approval), *Rewind* (conversation and
   files), *By the way* (a side question that stays out of the conversation),
   *Checkup*, and *Goal* (keep working until a stated condition is met).
6. **The Poiesis experience**: self-commands speak in first person, the
   agent's own actions are witnessed, the menu grows as Poiesis learns skills,
   and the user's corrections (rewind, retry) become things Poiesis learns from.
7. **A run bar** (`RUN`): one line above the composer that gathers what a run
   is doing and what it did. Today the plan, the agents, the changed files and
   the cost are in four different places.
8. **Settings become defaults** (`DEF`): plan, step budget and effort are
   per-turn decisions that today only exist as global settings. The composer
   chips become the normal way to set them, and Settings holds the default.

> ID prefixes: **REG** registry - **CMP** composer (`+`, `/`, menu, chips) -
> **UCM** user commands - **SKC** skills as commands - **AGC** agent commands -
> **CLR** context clearing - **PLF** plan first - **RWD** rewind - **BTW** side
> question - **CHK** checkup - **GOL** goal - **HOK** hooks - **RUN** run bar -
> **DEF** settings as defaults - **CPX** Poiesis experience - **-UI** frontend -
> **-T** tests.
>
> **Status: Phases 1 and 2 built 2026-10-06, awaiting manual verification.
> Phases 3-6 not started.** Written 2026-10-06.
>
> Deviations from the text above, taken while building Phases 1-2:
> - `who: both` and `agentClass` are not set on any manifest entry yet, so the
>   `◆` glyph (`CPX-6`) is built but inert. It would claim "I can do this myself"
>   before the `harness` tool exists. Phase 3 flips `compact`, `reflect`,
>   `skillify`, `checkup` and the rest.
> - Skills do not open an argument row in the menu (Enter on a skill completes
>   `/name `, as the old slash list did, so `Composer.slash.test.tsx` holds). One
>   test changed: the arrow-key test narrows with `/-` because `/` now lists
>   every command.
> - `/` lists commands through `useCommandInput`; command notes exist from
>   Phase 1 (in memory) and are persisted from Phase 2 via `record_command_cmd`.
> - Bundled App skills are seeded once into `<app-data>/skills/` (a skill the
>   user deletes stays deleted); `/init` ends in a new `propose_project_instructions`
>   tool and a `project_instructions` proposal target.
> - `/schedule` with words prefilled the task only in Phases 1-2; `parseWhen`
>   arrived in Phase 5 (below).
>
> Deviations taken while building Phase 5:
> - The goal is kept in the store only, not in `session_state`. `session_state` is
>   what the model reads every turn and can write with `remember`, which would let
>   the agent change a goal. Nothing runs a goal after the app closes, so there is
>   nothing to read back either.
> - `parseWhen` only maps to what the scheduler can run (hourly, 6-hourly, daily,
>   weekly, counted from the first run). It has no time of day, no weekdays and no
>   one-off runs, so a phrase it can only roughly honour fills the field and says
>   what differs (`TaskDraft.whenNote`). One it cannot honour leaves the field
>   empty and focused, and Save stays off until a rhythm is picked.
> - `goal_check_cmd(conversation_id, objective, until, target)` reads the last
>   answer and the change list itself. `met` without evidence reads as not met, and
>   so does anything that is not well-formed JSON.
> - A goal ends when the user leaves the chat, when a run ends `aborted`, `error` or
>   `timeout`, and while a plan waits for approval it simply waits.
> - The goal segment follows "I asked you something" on the run bar, not before it:
>   nothing matters more than a question I am waiting on.
> - MCP prompts are cached in the connector's `config_json` (`prompts`) by the
>   same probe that caches tools, and listed by `list_mcp_prompts_cmd`. A name that
>   is a built-in, a skill or another prompt gets its connector's name in front.
>   Arguments: `name=value` fills that argument, the rest of the words go to the
>   first argument still empty.

---

## What this is not

- **Not a second command palette.** `Ctrl K` (`CommandPalette.tsx`) stays the
  way to *go somewhere*: chats, projects, library, settings. `/` is the way to
  *do something to this conversation or this run*. The two share a fuzzy
  matcher (`CMP-2`) and nothing else. A `/` command can open a view, but only
  as a side effect of doing something in the conversation.
- **Not a shell.** `/` commands never run programs. `run_command` and
  `run_task` stay the agent's tools, behind their own gates.
- **Not goal autonomy.** The agent never sets its own goal, approves its own
  plan, changes its own autonomy rungs, or writes hooks. Those are user-only by
  construction: they are absent from the agent's enum, not refused at runtime.
  This is Part I §1 of `POIESIS_PLAN.md` ("No goal autonomy", "No code
  self-modification") applied to commands.
- **Not a new custom-command format.** User-defined commands *are* skills. The
  open Agent Skills format plus three Claude Code frontmatter keys we already
  parse but ignore covers it (`SKC`).
- **No schema migration.** Everything new fits in `session_events` kinds,
  `session_state`, `plan_json` and the MCP connector cache. Schema stays at v29.

---

## Where Poiesis stands

### Facts to build against

| Fact | Where |
|---|---|
| The `+` button opens one menu holding Attach files, Workspace mode, Tools, Create image, Create video, Skills ›, Start from a skill ›, Persona ›. | `Composer.tsx:579-863` |
| Typing `/` at the start of the input lists **enabled skills only**. The query is derived from the text (`/^\/(\S*)$/`), and a space closes it. Choosing a skill writes `/<name> ` into the input, and the message is sent as **plain text**: the model is trusted to call `skill` itself. | `Composer.tsx:191-224`, `:866-891` |
| The placeholder advertises `/ for a skill`. | `Composer.tsx:355` |
| `Ctrl K` palette with `score`/`ranked` fuzzy matching, grouped results, keyboard nav. | `CommandPalette.tsx:63-81` |
| Store actions that commands can bind to as they are: `newConversation`, `renameConversation`, `forkFromMessage`, `resumeLastRun`, `stopGenerating`, `steerActiveRun`, `selectModel`, `applyPersona`, `setToolsEnabled`, `setWorkspaceMode`, `startFromSkill`, `reflectConversation`, `undoChanges`, `keepChanges`, `openContextPanel`, `setDockView`, `setView`, `scheduleConversation`, `setPlanMode`, `setAutoCompact`, `refreshChangeProposals`, `checkGoldenNow`. | `src/lib/store.ts:163-798` |
| Auto-compaction runs **only from the chat window** (`assembleTurns` → `api.compactConversation`). A long run, a scheduled job or a background child that overflows silently drops its oldest turns. | `store.ts:4749-4754`, memory note in `harness-phase0-built` |
| `compact_conversation_cmd(conversation_id, upto_message_id, target)` has no focus argument. Every compaction is a `summary` row in `session_events`, and `CompactDivider` shows it. | `commands/conversations.rs:231`, `agent/log.rs:71,136` |
| The loop is six named phases on `TurnCtx`: `prepare_turn` → (`wrap_up_turn`) → `assemble` → `request` → `classify` → `dispatch_batch`. Loop-owned tools (`plan`, `read_result`, `search_result`) are dispatched in `TurnCtx::dispatch` ahead of the registry. | `agent/run.rs:1493-1517`, `:2417-2447` |
| Steering: `RunHandle` has a FIFO inbox drained in `open_turn`. `steer_run_cmd` / `steerActiveRun` feed it. | `agent/fleet.rs:156-193`, `run.rs:1536` |
| `RunLimits::top(db)` reads `agent.max_steps` (default 12, clamp 1..=50). There is no per-turn override. | `fleet.rs:129-138` |
| Reasoning effort is the setting `models.reasoning_effort` (default Low), read once per run by `reasoning_effort(db)`, set by `EffortPicker` in the composer footer. There is no per-turn override. | `run.rs:802`, `EffortPicker.tsx:21` |
| `PlanMode` (`always|auto|never`, setting `agent.plan_mode`) decides only **whether the `plan` tool is offered**. It is not a read-only mode. | `agent/plan.rs:247-301`, `store.ts:5562` |
| The autonomy ladder: `Rung::{Auto, Ask, Off}`, `AUTONOMY_DEFAULTS` (facts, lessons, consolidate, soul, profile, email_send, skills, screen), and `self_change_class(tool)` withdraws an `off` tool from the registry. | `autonomy.rs:13-33`, `run.rs:96-103`, `:175-183` |
| The session log replays everything except kinds `steer`, `stop`, `summary`, `plan`. New kinds have to be added to that skip list or they will be replayed into the model's view. | `agent/log.rs:193` |
| File changes are snapshots in the trash table, not git. `change_set(db, conv, since)` builds a per-file patch set from a timestamp. `undo_changes_cmd` undoes newest-first. | `agent/changes.rs`, `commands/projects.rs:251-263` |
| `TurnActions` on a finished assistant turn: **Why this answer**, **Try again from here** (fork before the turn and re-ask), **Continue where I stopped** (resume, only on an interrupted last turn). | `AgentRun.tsx:257-308` |
| Skills: `skill` / `propose_skill` tools. `argument-hint`, `disable-model-invocation` and `user-invocable` are listed in `UNSUPPORTED_KEYS` and shown as `◇ partial`. `SkillSource::{Personal, Project, App}`. | `agent/skillpack.rs:54-66`, `:420-451`, `:631` |
| Golden checks (`GLD`): fixed behavioural contracts, run around every self-change, never dispatching tools. `check_golden_cmd`. | `agent/golden.rs`, `commands/organism.rs:79` |
| Context manifest per message (`context_manifest_cmd`) behind **Why this answer** and `ContextPanel`. | `commands/memory.rs:393` |
| Self panel tabs: Memory, Lessons, Health, Autonomy. | `Self/SelfPanel.tsx:9-15` |
| The Orb maps facts to `thinking-orbs` states: `working searching solving listening connecting weaving composing breathing shaping`. Step verbs map through `orbForStep`. Presence (`idle active reflecting healing`) maps through `orbForPresence`. **`listening` is unused.** | `components/Orb/orbState.ts` |
| MCP tools are cached in the connector's `config_json` as `McpTool` without `annotations`, so `readOnlyHint` is lost. | `run.rs:140-145`, `mcp/` |
| Inside the streaming turn, `RunMeter` shows `{plan item} · {activity} · {clock} · context N%`. It never shows the step budget, and a test asserts that (`AgentRun.meter.test.tsx`). **A limit is not a status.** | `AgentRun.tsx:77-127` |
| `AgentEvent::RunProgress { step, max_steps, ms, context_tokens }` is sent each iteration. Usage and cost arrive **only** in `RunEnded`, though `RunHandle` sums usage as the run goes (`run.usage()`). `pricing::cost_usd(model, prompt, output) -> Option<f64>` exists. | `agent/mod.rs:248-283`, `fleet.rs:169-175`, `cloud/pricing.rs:53` |
| Where a run's state lives today: the plan in `PlanCard` in the turn; agents in the composer's `fleet-pill` (`N agents working`) and the dock's Agents tab; changed files in the dock's Changes tab; cost in Settings → Usage. | `Composer.tsx:949-960`, `Workbench/ChangesPanel.tsx`, `routes/Usage.tsx` |
| Per-turn decisions stored only as global settings: plan mode (Settings, `agent.plan_mode`), step budget (Settings → Tools, `agent.max_steps`), auto-compaction (Settings). Effort is already in the composer footer (`EffortPicker`). | `Settings.tsx:145-170`, `:393`, `Tools.tsx:136` |
| Schema is v29. | `db/mod.rs:22` |
| Prompt assembly is gated byte for byte (`fixtures/prompt-assembly.golden.txt`). Editing `agent/context.rs` or `store.ts`'s `composeSystemPrompt` alone breaks the gate. Both sides must change in one commit. | `agent/context_golden.rs`, `src/lib/prompt-assembly.test.ts` |

### What SOTA agents have that Poiesis doesn't (the gap this plan closes)

| Capability | Claude Code / Codex / Cursor | Poiesis today | This plan |
|---|---|---|---|
| Slash menu covering the harness | yes, ~40 built-ins | skills only | `REG`, `CMP`, `UCM` |
| Custom commands with arguments | `.claude/commands`, skills with `$ARGUMENTS` | skills, no args | `SKC` |
| Agent asks a structured question | `AskUserQuestion` | no | `AGC-3` |
| Plan mode (read-only until approved) | yes | plan *tool* only | `PLF` |
| Rewind code + conversation | `/rewind`, checkpoints | fork only, undo separate | `RWD` |
| Side question outside the context | `/btw` | no | `BTW` |
| Context editing mid-run (clear old tool results) | yes | compaction only between turns, chat window only | `CLR` |
| Manual compaction with a focus | `/compact <focus>` | auto only, no focus | `UCM`, `CLR-4` |
| Per-turn effort / budget | `/effort`, think keywords | global setting | `CMP-6` modifiers |
| Health check | `/doctor` | Health tab, golden button | `CHK` |
| Long-horizon goal loop | `/goal`, Ralph loops | no | `GOL` |
| Project init | `/init` | no | `SKC-4` |
| User hooks | yes | no | `HOK` (deferred) |
| **Self-maintenance surfaced as commands** | no | Self panel only | `UCM` "Me", `CPX` |

The last row is what Poiesis has that the others don't. The plan leans on it.

---

# Part I - Concept and binding rules

## 1. One registry, two callers

Every command is one entry in **one manifest** (`shared/commands.json`, `REG-1`).
The user reaches it through the `/` menu. The agent reaches the subset marked
`agent` through two loop-owned tools. A command means the same thing whoever
runs it, and both callers leave the same kind of trace.

```
                 shared/commands.json   (one source of truth)
                  │                         │
       frontend: src/lib/commands.ts   backend: agent/commands.rs
       handlers bound to store actions   enum for the `harness` tool,
       `/` menu, chips, notes            autonomy class per command
                  │                         │
         user types /compact          agent calls harness{name:"compact"}
                  └──────────┬──────────────┘
                       session_events kind "command"
                       → CommandNote in the transcript
```

## 2. Rules (binding on every task in this plan)

1. **`+` adds, `/` does.** `+` only puts content into the next message (files,
   a folder, a library item, an earlier conversation). Anything that changes
   mode, run, memory or conversation is a `/` command. No item appears in both.
2. **The agent proposes, the user disposes.** Every command the agent can call
   names an autonomy class (`AGC-1`). `auto` runs it and shows it, `ask` turns
   it into a proposal the user answers, `off` removes it from the agent's enum.
   Approving a plan, setting a goal, changing a rung and writing a hook are
   never in the agent's enum.
3. **Every command that changes state leaves a trace.** It is a `command` row in
   `session_events` and a `CommandNote` in the transcript, attributed to "you"
   or to Poiesis. Pure navigation (`/self`, `/usage`) leaves no trace.
4. **First person for the self** (PRES-0 of `POIESIS_PLAN.md`). Every command in
   the "Me" section, and every note or proposal from the agent, speaks as "I".
   The copy table in `CPX-1` is authoritative.
5. **A disabled command says why.** It stays in the menu, dimmed, with the
   reason as its hint ("needs a working folder", "nothing to rewind yet"). It is
   never silently missing. The only exception is commands that are meaningless
   in the current state, such as `/stop` with no run. Those are hidden.
6. **One suggestion at a time, never repeated.** The agent may suggest a command
   at most once per command per conversation. Only one suggestion chip is
   visible at a time. There are no badges, counts or streaks (quiet biology).
7. **The user's corrections are signals.** Rewind, retry, `/forget` and a
   declined proposal are written where reflection can read them (`CPX-5`).
8. **Prompt guidance lives in tool descriptions, not the system prompt.**
   `harness` and `ask_user` explain *when* to use them in their own
   `description`. This keeps the golden gate untouched except where `SKC-3`
   has to change the skill catalogue.

---

# Part II - The command catalogue

Kinds: **ui** opens or toggles something in the app · **action** calls the
backend and leaves a trace · **modifier** shapes only the next turn (shown as a
chip) · **skill** sends a message with a skill preloaded (`SKC`).

Who: **U** user only · **A+U** the agent can call it too (class in brackets).

"Needs" drives the disabled reason (Rule 5): `conv` a persisted conversation ·
`run` a live run · `norun` no live run · `folder` a working folder ·
`changes` uncommitted agent changes · `interrupted` the last run stopped early.

### This turn

| Command | Args | Kind | Who | Does | Backed by | Phase |
|---|---|---|---|---|---|---|
| `/plan` | `[task]` | modifier | A+U (`modes`) | **Plan first**: the next run may read but not change anything, writes a plan, and stops for approval | `PLF` | 4 |
| `/effort` | `low\|medium\|high` | modifier | U | Thinking effort for the next turn only | `CMP-6`, `RunOptions.effort` | 1 |
| `/steps` | `<n>` | modifier | U | Step budget for the next turn (1-50) | `RunOptions.max_steps` | 1 |
| `/stop` | | ui | U | Stop me and every agent I started | `stopGenerating` | 1 |
| `/continue` | | action | U | Continue where I stopped | `resumeLastRun` (needs `interrupted`) | 1 |
| `/retry` | | action | U | Try the last question again on a branch | `forkFromMessage(lastAssistant)` | 1 |
| `/btw` | `<question>` | action | U | Ask on the side; the answer stays out of the conversation | `BTW` | 4 |

### Conversation

| Command | Args | Kind | Who | Does | Backed by | Phase |
|---|---|---|---|---|---|---|
| `/new` (alias `/clear`) | `[skill]` | ui | U | New chat; with a skill name, start from that skill | `newConversation`, `startFromSkill` | 1 |
| `/rename` | `<title>` | action | U | Rename this chat | `renameConversation` | 1 |
| `/fork` | | action | U | Branch here, keeping everything so far | `fork_conversation_cmd` at the last message | 1 |
| `/rewind` | `[turn]` | action | U | Go back to before a turn, optionally taking back file changes | `RWD` | 4 |
| `/compact` | `[focus]` | action | A+U (`context`) | Summarise older turns now, paying attention to *focus* | `compact_conversation_cmd` + `CLR-4` | 2 / 3 |
| `/context` | | ui | U | What I'm holding in mind right now | `openContextPanel({conversationId})` | 1 |
| `/why` | | ui | U | Why I gave my last answer | `openContextPanel({conversationId, messageId})` | 1 |
| `/export` | | action | U | Save this conversation as Markdown | `export_conversation_cmd` (`UCM-9`) | 5 |

### Me (Poiesis, first person)

| Command | Args | Kind | Who | Does (first-person hint) | Backed by | Phase |
|---|---|---|---|---|---|---|
| `/remember` | `<fact>` | action | U | "I'll remember that" | memory save (`UCM-5`) | 2 |
| `/forget` | `<what>` | action | U | "I'll forget it" (pick from matches) | `list_memory_facts_cmd` → `forget_memory_fact_cmd` | 2 |
| `/always` | `<instruction>` | action | U | "I'll always do that" (appends to my standing instructions) | `set_soul_cmd` (append) | 2 |
| `/reflect` | | action | A+U (`suggest` only) | "Let me think back over this conversation" | `reflectConversation` | 2 |
| `/skillify` (alias `/make-skill`) | `[name]` | skill | A+U (`suggest` only) | "I'll turn what we just did into a skill" | App skill → `propose_skill` | 2 |
| `/tidy` | | action | U | "Let me tidy up my memory" | `consolidate_memory_cmd` (ask flow) | 2 |
| `/checkup` | | action | A+U (`suggest` only) | "Let me check myself" | `CHK` | 4 |
| `/self` | | ui | U | Visit my Self panel | `setView("self")` | 1 |
| `/autonomy` | | ui | U | How much I may change without asking | `setView("self")` + Autonomy tab | 1 |

### Modes and persona

| Command | Args | Kind | Who | Does | Backed by | Phase |
|---|---|---|---|---|---|---|
| `/workspace` | `[on\|off]` | action | A+U (`modes`) | Toggle Workspace mode | `setWorkspaceMode` (+ `setToolsEnabled(true)`) | 1 |
| `/tools` | `on\|off` | action | U | Let me use my tools | `setToolsEnabled` | 1 |
| `/persona` | `[name]` | action | U | Who I am in this chat | `applyPersona` | 1 |
| `/model` | `[name]` | action | U | Switch model (persistent, like the picker) | `selectModel` | 1 |

### Agents and time

| Command | Args | Kind | Who | Does | Backed by | Phase |
|---|---|---|---|---|---|---|
| `/delegate` | `<task>` | skill | U | Hand this to a background agent and keep talking | App skill → `delegate{background:true}` | 2 |
| `/agents` | | ui | U | Show what my agents are doing | `setDockView("agents")` + `setDockOpen(true)` | 1 |
| `/schedule` | `[when] [task]` | ui | A+U (`schedule`) | Run something later or on a repeat | `scheduleConversation` / `UCM-8` | 1 / 5 |
| `/goal` | `<objective> [until <condition>]` · `stop` | action | U | Keep working until the condition holds | `GOL` | 5 |

### Create

| Command | Args | Kind | Who | Does | Backed by | Phase |
|---|---|---|---|---|---|---|
| `/image` | `[prompt]` | modifier | U | The next message makes a picture | the existing `pinnedIntent = "image"` | 1 |
| `/video` | `[prompt]` | modifier | U | The next message makes a clip | `pinnedIntent = "video"` | 1 |

### Work (folders and code)

| Command | Args | Kind | Who | Does | Backed by | Phase |
|---|---|---|---|---|---|---|
| `/init` | | skill | U | Let me learn this project and propose its instructions | App skill (`SKC-4`) | 2 |
| `/review` | `[focus]` | skill | U | Have a reviewer go over my changes | App skill → `delegate` + `changes` | 2 |
| `/verify` | | skill | U | Run the project's check and fix what fails | App skill → `run_task` | 2 |
| `/changes` | | ui | U | Show what I changed | `setDockView("changes")` | 1 |
| `/undo` | `[file]` | action | U | Take back my file changes (confirm) | `undoChanges` (needs `changes`) | 1 |
| `/keep` | | action | U | Keep my changes | `keepChanges` (needs `changes`) | 1 |
| `/permissions` | | ui | U | What I'm allowed to touch | Permission panel | 1 |

### Inspect

| Command | Args | Kind | Who | Does | Backed by | Phase |
|---|---|---|---|---|---|---|
| `/usage` | | ui | U | What this chat has cost | Usage view focused on this conversation | 1 |
| `/activity` | | ui | U | Everything I did, in order | `setView("activity")` | 1 |
| `/help` | | ui | U | Every command, with what it does | opens the menu unfiltered, all sections expanded | 1 |

### My skills

Every enabled skill with `user-invocable` not `false`, sorted by recent use.
Skills authored by Poiesis show where they came from as hint text (`CPX-4`).

### What the agent can call, and when

This is the whole list. Anything not here is user-only.

| Agent call | Class (default rung) | When the description tells it to | What the user sees |
|---|---|---|---|
| `harness{name:"compact"}` | `context` (**auto**) | Old tool output is crowding the context and is no longer needed word for word | `◆ I made room` row, Orb `shaping` |
| `harness{name:"suggest", command}` | `suggest` (**auto**) | A user command would clearly help (`/skillify` after a reusable procedure worked, `/reflect` after many corrections, `/checkup` after repeated tool failures, `/plan` before a risky change) | Suggestion chip above the composer |
| `harness{name:"switch_mode", mode}` | `modes` (**ask**) | Workspace would serve the task better, or the change is risky enough to plan first | In-run proposal: Switch · Not now |
| `harness{name:"schedule", when, task}` | `schedule` (**ask**, `auto` not offered) | The user asked for something recurring or later | In-run proposal: Set it up · Not now |
| `ask_user{question, options}` | not gated (asking changes nothing) | A real fork it cannot decide sensibly. Not for confirming what it can decide itself | Question card, Orb `listening` |
| automatic clearing (`CLR-1`) | `context` | The harness does it with no tool call when the transcript passes 75% of the window | same as `compact` |

---

# Part III - Architecture

## REG - The registry

### `REG-1` The manifest: `shared/commands.json`

New top-level `shared/` directory (alongside `fixtures/`, which is for tests
only). One JSON array, one object per built-in command:

```json
{
  "name": "compact",
  "aliases": [],
  "section": "conversation",
  "kind": "action",
  "args": { "type": "text", "hint": "focus (optional)", "required": false },
  "summary": "Summarise older turns now",
  "summarySelf": null,
  "who": "both",
  "agentClass": "context",
  "suggestible": true,
  "needs": ["conv"],
  "trace": true
}
```

- `section`: `turn | conversation | me | modes | agents | create | work | inspect`.
  Skills are added at runtime as section `skills`.
- `kind`: `ui | action | modifier | skill`.
- `args.type`: `none | text | choice | number`. A `choice` has
  `"source": "effort" | "personas" | "models" | "turns" | "facts" | "skills" | "onoff"`
  and the frontend resolves it (`CMP-5`).
- `summary` is the menu hint. For section `me` it is written in first person
  (Rule 4). `CPX-1` lists the final copy.
- `who`: `user | both`. `agentClass` is required when `who` is `both`.
- `suggestible`: the agent may *suggest* it (`AGC-2`) even when `who` is `user`.
- `needs`: see the catalogue legend.
- `trace`: writes a `command` session event (`REG-4`).

### `REG-2` Rust side: `src-tauri/src/agent/commands.rs`

```rust
static MANIFEST: &str = include_str!("../../../shared/commands.json");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSpec {
    pub name: String,
    #[serde(default)] pub aliases: Vec<String>,
    pub section: String,
    pub kind: String,
    pub who: String,
    pub agent_class: Option<String>,
    #[serde(default)] pub suggestible: bool,
    #[serde(default)] pub needs: Vec<String>,
    #[serde(default)] pub trace: bool,
}

pub fn manifest() -> &'static [CommandSpec];          // OnceLock-parsed
pub fn agent_callable(db: &Db, headless: bool) -> Vec<&'static str>; // AGC-1 enum
pub fn suggestible() -> Vec<&'static str>;
```

`agent_callable` returns the `harness` names (`compact`, `suggest`,
`switch_mode`, `schedule`) whose class rung is not `Off`. Headless drops
everything except `compact`. Add `pub mod commands;` to `agent/mod.rs`.

### `REG-3` Frontend side: `src/lib/commands.ts`

```ts
import manifest from "../../shared/commands.json"; // resolveJsonModule; else ?raw + JSON.parse like fixtures

export type CommandKind = "ui" | "action" | "modifier" | "skill";
export interface CommandSpec { /* mirrors REG-1 */ }
export interface CommandView extends CommandSpec {
  disabledReason: string | null;   // Rule 5
  source: "builtin" | "skill";
  skill?: api.SkillView;
}
export interface ParsedCommand { name: string; args: string; spec: CommandView }

export function parseCommandLine(text: string, views: CommandView[]): ParsedCommand | null;
export function commandViews(s: AppState): CommandView[];      // builtins + skills, with needs resolved
export function rankCommands(views: CommandView[], query: string): CommandView[]; // uses lib/fuzzy
export async function runCommand(cmd: ParsedCommand, s: AppState): Promise<CommandResult>;

export type CommandResult =
  | { kind: "done"; note?: string }             // action/ui finished; note → CommandNote
  | { kind: "modifier"; modifier: TurnModifier } // added to composer chips, input cleared
  | { kind: "send"; text: string; opts: SendOpts } // skill commands → sendMessage
  | { kind: "error"; message: string };

export const HANDLERS: Record<string, (args: string, s: AppState) => Promise<CommandResult> | CommandResult>;
```

`parseCommandLine` matches only a leading `/` followed by a known name or alias.
An unknown `/word` returns `null`, and the text is sent as an ordinary message,
which keeps today's behaviour for paths like `/usr/bin`.

### `REG-4` The trace

New session-event kinds, written with the existing
`db.append_session_event(conversation_id, run_id, kind, payload)`:

| Kind | Payload | Written by |
|---|---|---|
| `command` | `{ name, args, by: "user"\|"agent", outcome: "done"\|"proposed"\|"accepted"\|"declined"\|"failed", note, message_id? }` | `record_command_cmd` (user), `TurnCtx::dispatch` (agent) |
| `rewound` | `{ message_id, preview, branch_id, files_undone }` | `rewind_cmd` (`RWD-1`), in the **original** conversation |

- Add both kinds to the skip list in `log.rs:193`. Otherwise `replay` puts them
  in front of the model.
- New `agent/log.rs` fns: `record_command(db, conv, run_id: Option<&str>, &CommandTrace)`
  and `commands(db, conv) -> Vec<CommandTrace>`.
- New Tauri commands in `commands/agent.rs`, registered in `lib.rs`:
  `record_command_cmd(conversation_id, trace)` and
  `conversation_commands_cmd(conversation_id) -> Vec<CommandTrace>`.
- `fork_conversation` copies `command` rows up to the boundary, as it already
  does for `summary`.

### `REG-5` Run options

New struct in `agent/run.rs`, passed by `agent_chat_cmd` (new optional argument
`run_options`) through `resolve_turn`/`execute_turn` into a new
`RunContext.options` field:

```rust
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOptions {
    #[serde(default)] pub plan_first: bool,               // PLF
    pub effort: Option<Effort>,                           // overrides reasoning_effort(db) for this run
    pub max_steps: Option<usize>,                         // overrides RunLimits::top, clamp 1..=50
    pub skill: Option<String>,                            // SKC-2 explicit invocation
    pub skill_args: Option<String>,
    pub approved_plan: Option<super::plan::Plan>,         // PLF-4
}
```

`TurnCtx.effort` becomes `rc.options.effort.unwrap_or_else(|| reasoning_effort(db))`.
`RunLimits` is built with the override applied before `run_agent`. A delegated
child gets `RunOptions::default()`, so modifiers never leak into children.
`resume_run_cmd` also takes `run_options` so `/continue` can carry `/steps`.

---

## CMP - The composer: `+`, `/`, the menu, chips

### Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│  plan 3 of 6 · 2 agents working · I changed 4 files · $0.04 so far  ×│ (RUN, only while there's something to say)
│ ──────────────────────────────────────────────────────────────────── │
│  ◇ Plan first ×   ◇ Think: high ×   ◇ Up to 30 steps ×   (modifiers) │
│ ┌───┐┌───┐ ┌──────────────────────────────────────────────┐  ┌───┐   │
│ │ + ││ / │ │ Message Poiesis Agent · / commands · + add    │  │ ↑ │   │
│ └───┘└───┘ └──────────────────────────────────────────────┘  └───┘   │
│  ◎ my-project · Workspace × · Researcher ×        ▮▮▯  Low ▾  Model ▾ │
└──────────────────────────────────────────────────────────────────────┘
      ContextChip + ModeChips (CMP-7)            ContextMeter Effort Model
```

### `CMP-1` Split the components

`Composer.tsx` is over 1,000 lines. Extract these before adding anything:

- `Composer/PlusMenu.tsx`: the `+` button and its menu (`CMP-3`).
- `Composer/CommandMenu.tsx` + `CommandMenu.css`: the `/` list (`CMP-4`),
  replacing the inline `composer-slash-menu` at `Composer.tsx:866-891`.
- `Composer/useCommandInput.ts`: the derived query, active index, dismissal,
  key handling (moved from `:191-224` and `:905-928`).
- `Composer/ModifierChips.tsx`, `Composer/ModeChips.tsx` (`CMP-6`, `CMP-7`).

Existing tests (`Composer.slash.test.tsx`) must pass unchanged after the
extraction, before any behaviour changes. Land the extraction as its own commit.

### `CMP-2` Shared fuzzy matcher

Move `score` and `ranked` from `CommandPalette.tsx:63-81` to `src/lib/fuzzy.ts`
unchanged. `CommandPalette` imports them from there. `rankCommands` ranks on
`name`, then aliases, then `summary`, with name hits first.

### `CMP-3` `+` becomes "add to this message"

`PlusMenu` keeps only content. Final items, in order:

| Item | Hint | Action |
|---|---|---|
| Files and images | `images and PDFs, or just paste or drop one` | existing `attach()` |
| A folder | `I'll work in it and read it when it helps` | `attachFolder()` (disabled with "already working in {name}" when one is attached) |
| From my library | `a picture, page or file I made earlier` | opens a library picker; the artifact becomes an attachment (image) or a context reference (other kinds) |
| An earlier conversation | `I'll read it before answering` | conversation picker (reuses the palette's chat list); adds a context ref that the next turn resolves with `read_conversation` |

Removed from `+`: Workspace mode, Tools, Create image, Create video, Skills ›,
Start from a skill ›, Persona ›. All of them are now `/` commands, and the
active ones show as chips (`CMP-7`).

- Button: `aria-label="Add to this message"`, `title="Add files, a folder, or something I made"`.
- The `on` class no longer lights for Workspace mode. It lights only while
  attachments are pending.

### `CMP-4` The `/` button and the command menu

**Button.** A new `icon-btn slash-btn` immediately right of `+`, same size and
style, glyph `/` in the UI mono font. `aria-label="Commands"`,
`aria-haspopup="listbox"`, `title="Commands  ( / )"`. Clicking it:

- with an empty input: sets the value to `/` and focuses the input. The menu
  opens through the derived query, so there is only one way it opens.
- with text in the input: opens the menu as a popover with its own filter field.
  Choosing a command prepends it (`/plan ` + existing text). This replaces
  today's "append `/skill` to the end" behaviour (`Composer.tsx:788-790`).

`Ctrl /` from anywhere in the app focuses the composer and opens the menu.

**Query parsing.** The regex widens to `^\/(\S*)(?:\s(.*))?$`:

- **Name phase** (no space yet): the menu lists ranked commands.
- **Argument phase** (after the space): the menu shrinks to an argument row,
  described in `CMP-5`.

**Menu structure.**

```
┌ Commands ──────────────────────────────────────────────┐
│ RECENT                                                   │
│  /compact   focus…        Summarise older turns now   ◆  │
│  /plan      task…         Read first, change nothing…  ◆  │
│ THIS TURN                                                │
│  /effort    low|medium|high   How hard to think          │
│  /btw       question…     Ask on the side                │
│ CONVERSATION                                             │
│  /rewind    turn…         Go back to before a turn       │
│  …                                                       │
│ ME                                                       │
│  /remember  fact…         I'll remember that             │
│  /reflect                 Let me think back over this… ◆ │
│ MY SKILLS                                                │
│  /weekly-report           I learned this on 2 Oct        │
│ ─────────────────────────────────────────────────────── │
│ ↑↓ move · ↵ run · tab complete · esc close     /help     │
└──────────────────────────────────────────────────────────┘
```

- Empty query: **Recent** (the last 4 run, from `localStorage`
  `poiesis.recentCommands`, every access in `try/catch`), then every section in
  catalogue order. With a query: one flat ranked list, with the section shown as
  hint text on the right.
- Each row: `/name` in mono, argument hint in muted mono, summary, and a `◆` at
  the far right when the agent can also run it (`CPX-6`).
- While a run is live, the **This turn** section moves to the top and gains
  `/stop`. `/btw` stays available. Typing plain text still steers the run as it
  does today (`Composer.tsx:232-244`).
- Disabled rows: `aria-disabled="true"`, 50% ink, the reason replaces the
  summary, and they cannot be chosen. Enter on one does nothing and announces
  the reason through a polite live region.
- Max height 360px, scrolls, and the active row stays in view.
- `role="listbox"`, rows `role="option"`, `aria-activedescendant` on the input.
  Section headers are `role="presentation"`.
- Keys: ↑↓ move, Tab completes the name and enters the argument phase, Enter
  runs (or completes when an argument is required and missing), Esc dismisses
  but keeps the text (current behaviour), Shift+Tab toggles the *Plan first*
  chip (`PLF-5`).

### `CMP-5` Argument completion

In the argument phase:

- `text` and `number` args show one muted row: `/compact ⟨focus (optional)⟩ ·
  ↵ run`. Enter runs the command with whatever was typed.
- `choice` args list their options, filtered by what is typed:
  `effort` → Low, Medium, High · `onoff` → on, off · `personas` → persona names
  plus "No persona" · `models` → favourites first, then all chat models ·
  `turns` → the last 10 user turns (preview text and time) for `/rewind` ·
  `facts` → `list_memory_facts_cmd` matches for `/forget` · `skills` → enabled
  skills for `/new`.
- A required argument that is missing: Enter keeps the menu open and the row
  says what is missing.

### `CMP-6` Modifier chips

A new store slice `turnModifiers: TurnModifiers`, cleared after every send:

```ts
interface TurnModifiers { planFirst?: boolean; effort?: Effort; maxSteps?: number }
```

- `ModifierChips.tsx` renders above the input row, in the slot where the media
  target bar sits (`Composer.tsx:390`), one chip each: `◇ Plan first ×`,
  `◇ Think: high ×`, `◇ Up to 30 steps ×`. × removes the chip.
- `/image` and `/video` keep using the existing `pinnedIntent` and its intent
  chip (`Composer.tsx:513`). They are not duplicated here.
- On send, `sendMessage(text, attachments, { modifiers })` maps the chips to
  `RunOptions` (`REG-5`).
- A modifier command with trailing text (`/plan add dark mode`) sets the chip
  **and** sends the text at once. Without text it sets the chip, clears the
  input, and waits.

### `CMP-7` Mode chips

`ModeChips.tsx` in the footer, after `ContextChip` (`Composer.tsx:997-999`),
showing state that persists across turns: `Workspace ×` when Workspace mode is
on, `Tools off ×` when tools are disabled, `{Persona} ×` when a persona is
applied, `◎ Goal ×` while a goal is active (`GOL`). × turns that state off.
Clicking the label opens the matching command with its argument phase ready
(for example `/persona `).

### `CMP-8` Placeholder and copy

- Placeholder: `Message Poiesis Agent  ·  / for commands  ·  + to add files`.
- During a run (unchanged): `Tell me something while I work`.
- While a question is pending (`AGC-3`): `Answer me, or pick an option above`.

### `CMP-9` Executing a command

In `useCommandInput`'s submit path, before today's `submit()`:

1. `parseCommandLine(value, views)`. If it returns `null`, the existing flow
   runs unchanged.
2. If the command is disabled: announce the reason and send nothing.
3. Otherwise run it with `runCommand(cmd, state)` and handle the result:
   - `done`: clear the input. If the spec has `trace`, call `record_command_cmd`
     and append a `CommandNote` optimistically.
   - `modifier`: add the chip and clear the input (or send the trailing text,
     `CMP-6`).
   - `send`: call `sendMessage(text, attachments, opts)` with the skill options
     (`SKC-2`).
   - `error`: keep the input and show the message under the composer in the
     `composer-model-notice` style.
4. Push the name onto the recent list.

---

## UCM - User commands (bindings)

Most rows in Part II are one-line handlers over existing store actions. Only
the ones that need new code are specified here.

- `UCM-1` **Phase 1 bindings.** Every row marked phase 1 gets a handler. Each
  `ui` handler is a single store call. Acceptance: the vitest parity test
  (`REG-T1`) passes, and every handler is covered by `commands.test.ts`.
- `UCM-2` `/fork`. Check what `fork_conversation_cmd` does with
  `upto_message_id`. `forkFromMessage` forks *before* a turn and re-asks. For
  `/fork`, call the command with the conversation's last message as the
  boundary so everything so far is kept, then `openSession(newId)`. If the
  command cannot include the boundary message, add an `inclusive: bool` arg.
- `UCM-3` `/compact [focus]`: add `focus: Option<String>` to
  `compact_conversation_cmd`. When it is present, append to the summarisation
  instruction: `Keep in particular everything about: {focus}.` The frontend
  computes the boundary the same way auto-compaction does (`store.ts:4749`) and
  calls `api.compactConversation(convId, boundary.id, target, focus)`. The
  existing `CompactDivider` shows the result, so no `CommandNote` is needed.
- `UCM-4` `/effort`, `/steps`: modifier handlers only. `EffortPicker` keeps
  setting the default. The chip overrides it for one turn.
- `UCM-5` `/remember <fact>`: if `update_memory_fact_cmd` cannot create a fact,
  add `remember_fact_cmd(text, scope)`. It must use the same `MemoryStore`
  write as the `memory` tool so the memory toast and undo
  (`MEM-UI-3`: `◆ I'll remember that: … — Undo`) fire unchanged. It is not
  gated by the `facts` rung, because the user is the author.
- `UCM-6` `/forget <what>`: the argument phase lists matching facts. Choosing
  one calls `forget_memory_fact_cmd`, and the toast offers
  `restore_memory_fact_cmd`.
- `UCM-7` `/always <instruction>`: read the current soul, append
  `\n- {instruction}`, call `set_soul_cmd`, and show the toast
  `◆ I'll always do that now: {instruction} — Undo`. Undo writes the previous
  text back. The user owns the soul, so this is not a proposal.
- `UCM-8` `/schedule [when] [task]`: with no args, `scheduleConversation(convId)`
  (today's draft flow). With args, open Tasks with a draft whose prompt is
  *task* and whose schedule field is prefilled from *when* by
  `parseWhen(text)` in `src/lib/when.ts`, covering `every N minutes|hours`,
  `daily at HH:MM`, `weekdays at`, `every monday`, `tomorrow at`. Anything else
  leaves the field empty and focuses it. Nothing is ever scheduled without the
  user pressing Save.
- `UCM-9` `/export`: `export_conversation_cmd(conversation_id) -> String`
  (Markdown: title, then each message as `**You**` / `**Poiesis**`, with
  attachments as file names and plans as checklists), then a save dialog via
  the existing picker path. The toast reads `Saved to {path}`.
- `UCM-10` `/usage`: open the Usage view filtered to this conversation (it
  already returns `by_conversation`). If the view cannot filter, add a
  `focusConversation` prop that scrolls to and highlights the row. This depends
  on `RUN-1d`: until child conversations roll up, the parent's row leaves out
  everything its agents spent.

---

## SKC - Skills as commands

A user-defined command *is* a skill. Supporting three frontmatter keys makes
every skill a proper command, and the prompt-style built-ins (`/init`,
`/review`, `/verify`, `/skillify`, `/delegate`) become bundled App skills
instead of hard-coded templates.

- `SKC-1` **Frontmatter.** Remove `argument-hint`, `disable-model-invocation`
  and `user-invocable` from `UNSUPPORTED_KEYS` (`skillpack.rs:54`). Add them to
  `SkillPack`:
  ```rust
  pub argument_hint: Option<String>,
  pub model_invocable: bool,   // !disable-model-invocation, default true
  pub user_invocable: bool,    // default true
  ```
  Mirror them in `api.SkillView`. A skill with `user_invocable: false` is left
  out of the `/` menu. One with `model_invocable: false` is left out of the
  model's catalogue (`SKC-3`).
- `SKC-2` **Explicit invocation preloads the skill.** Today `/name text` goes
  out as plain text and the model may or may not call `skill`. Instead, the
  frontend sends `RunOptions { skill: Some(name), skill_args }`. Before the run
  starts, `execute_turn` synthesises the pair the model would have produced
  itself: an assistant message with a `skill{name}` tool call, then a tool
  result with the skill body. The body goes through the same load path as the
  `skill` tool (trust scoring, untrusted wrapping, `loaded_skills` marking).
  `$ARGUMENTS` in the body is replaced with the args. If the body has no
  `$ARGUMENTS` and args were given, `\n\nARGUMENTS: {args}` is appended. The
  user message content is just the args, or `Run /{name}` when there are none.
  The trace row carries `message_id`, so the user bubble renders a
  `/name` chip (`CMP-UI-6`).
- `SKC-3` **Catalogue.** Skills with `model_invocable: false` are dropped from
  the skill list in the system prompt. **This is prompt assembly**: change
  `agent/context.rs` and `store.ts`'s `composeSystemPrompt` in the same commit
  and regenerate with `UPDATE_PROMPT_GOLDEN=1 cargo test --lib context_golden`.
  Add a fixture skill with `disable-model-invocation: true` to
  `fixtures/prompt-assembly.json` so the gate covers it.
- `SKC-4` **Bundled App skills.** Each has `disable-model-invocation: true`, so
  only the user runs it. Bodies are short and imperative:
  - `init`: study the working folder (README, manifests, structure, check
    commands), then propose project instructions through a `change_proposals`
    row with target `project_instructions` (new target, rendered by the existing
    `ProposalCard`; accepting calls `setProjectInstructions`). Needs `folder`.
  - `review`: `delegate` to the reviewer agent type with the output of the
    `changes` tool and the user's focus. Report findings ranked, and change
    nothing.
  - `verify`: `run_task` the project's check, fix failures, and run it again.
    Stop after 3 rounds and report.
  - `skillify`: look back over this conversation, then call `propose_skill` with
    a reusable procedure (ask rung, `ProposalCard`). The name comes from
    `$ARGUMENTS` if given.
  - `delegate`: call `delegate` with `background: true` and the args as the
    brief, then tell the user it is running and keep talking.
- `SKC-5` `render_skill_md` (`skillpack.rs:631`) writes `origin: poiesis` and
  `created: YYYY-MM-DD` into frontmatter for skills that came from
  `propose_skill`. Both keys are informational, and `CPX-4` reads them.

---

## AGC - The agent's side

### `AGC-1` The `harness` tool

Named `harness`, not `command`, because `run_command` already exists and is a
shell. The description has to make clear that this tool controls the agent's
own session, not the user's computer.

```json
{
  "type": "function",
  "function": {
    "name": "harness",
    "description": "Control my own working session, not the user's computer. compact: clear old tool output I no longer need word for word, to make room. suggest: offer the user one of their slash commands when it would clearly help them (at most once per command per conversation). switch_mode: ask the user to turn a mode on (they decide). schedule: propose running a task later or on a repeat (they decide). Never use this to do the task itself.",
    "parameters": {
      "type": "object",
      "properties": {
        "name":    { "type": "string", "enum": ["compact", "suggest", "switch_mode", "schedule"] },
        "reason":  { "type": "string", "description": "One sentence the user will read, in first person." },
        "command": { "type": "string", "description": "suggest: the slash command, e.g. /skillify" },
        "mode":    { "type": "string", "enum": ["workspace", "plan_first"], "description": "switch_mode" },
        "when":    { "type": "string", "description": "schedule: e.g. 'every weekday at 9'" },
        "task":    { "type": "string", "description": "schedule: what to do" }
      },
      "required": ["name", "reason"]
    }
  }
}
```

- The `enum` is built per run from `commands::agent_callable(db, headless)`. If
  it is empty, the tool is not offered.
- Dispatched in `TurnCtx::dispatch` before the registry, beside `plan`
  (`run.rs:2437`), because its state belongs to the run. Its spec is appended
  in `assemble` the same way `plan`'s is (`run.rs:1645-1662`), and its name is
  pushed into `tool_names`/`invocable` (`run.rs:1361-1365`).
- `harness` calls count as bookkeeping (`is_bookkeeping`, `run.rs:2012`), so
  they do not use up the step budget.
- `self_change_class`-style gating: new `fn harness_class(name) -> &str`
  (`compact→context`, `suggest→suggest`, `switch_mode→modes`,
  `schedule→schedule`). The rung decides the outcome:
  - `Auto`: execute and emit `AgentEvent::Command { by: Agent, outcome: Done }`.
  - `Ask`: emit `AgentEvent::Proposal { id, name, reason, payload }` and return
    `"I've asked the user; carry on without waiting for the answer."`
    The run does **not** block.
  - `Off`: never reached, because the name is not in the enum.
- Every call writes a `command` trace row (`REG-4`) with the outcome.

**New autonomy classes** (append to `AUTONOMY_DEFAULTS`, `autonomy.rs:24`):

```rust
("context",  "auto"),  // clearing old tool output mid-run (CLR); undo = read_result
("suggest",  "auto"),  // suggestion chips; off = I never suggest commands
("modes",    "ask"),   // turning Workspace / Plan first on
("schedule", "ask"),   // proposing scheduled jobs; auto is never offered
```

`schedule` must not offer `auto` in the Autonomy tab. Unattended runs created
without a click are out of bounds. Add `pub const NO_AUTO: &[&str] = &["schedule"];`
and have `autonomy_gate` read `auto` as `Ask` for those classes, so a
hand-edited setting cannot fail open. Add tests next to the existing two.

### `AGC-2` Suggestions

`harness{name:"suggest", command, reason}`:

- `command` must be `suggestible` in the manifest and resolvable in this
  conversation, or the call errors back to the model: `"I can't suggest {command} here."`
- One per command per conversation. Before emitting, check the conversation's
  `command` rows for an earlier `suggest` of the same command. If there is one,
  return `"Already suggested; don't repeat it."` and emit nothing.
- Emits `AgentEvent::Suggestion { command, reason }`. The frontend keeps
  **one** suggestion (`activeSuggestion`) and replaces any older one.
- Accepting runs the command as if the user typed it, and records outcome
  `accepted`. "Not now" records `declined`. Both feed `CPX-5`.

**Harness suggestions without the model.** A small set the harness raises
itself, through the same chip and the same once-per-conversation rule:

- autoCompact is off and the context passes 85% after a turn → `/compact`.
- Golden checks failed after a self-change → `/checkup`. The toast
  `goldenRevertedToast` already exists, so this only links it.

### `AGC-3` `ask_user`

```json
{
  "type": "function",
  "function": {
    "name": "ask_user",
    "description": "Ask the user to decide something you cannot sensibly decide yourself: a real fork where the options lead to different work. Do not use it to confirm something you could just do, or to ask permission (permissions are handled for you). The run pauses until they answer. Offer 2-4 concrete options; they can always write their own.",
    "parameters": {
      "type": "object",
      "properties": {
        "question": { "type": "string" },
        "options": {
          "type": "array", "minItems": 2, "maxItems": 4,
          "items": { "type": "object",
                     "properties": { "label": { "type": "string" }, "detail": { "type": "string" } },
                     "required": ["label"] }
        },
        "multi": { "type": "boolean", "description": "true if more than one option may be chosen" }
      },
      "required": ["question", "options"]
    }
  }
}
```

- Offered only to **top-level, non-headless** runs with tools on. Delegated
  children never get it: a child reports back to its lead, and the lead asks.
  This keeps one voice talking to the user.
- `RunHandle` (`fleet.rs:156`) gains
  `questions: Mutex<HashMap<String, oneshot::Sender<Answer>>>` and
  `fn ask(&self, id) -> oneshot::Receiver<Answer>` / `fn answer(&self, id, Answer) -> bool`.
- Dispatch: register, emit `AgentEvent::Question { run_id, id, question, options, multi }`,
  then `select!` on the receiver and the cancel flag. On cancel, return
  `Err("The user stopped the run before answering.")`.
- New `answer_question_cmd(run_id, question_id, answer: Answer)` with
  `Answer { choices: Vec<String>, text: Option<String> }`. It looks up the run
  in the `Fleet` and returns `false` if the run is gone.
- Tool result: `The user chose: A` / `The user chose: A, C` /
  `The user wrote: …` / `The user chose: A, and added: …`.
- Steering while a question is pending: composer text is sent as the free-text
  **answer**, not as a steer. Exactly one place decides this:
  `steerActiveRun` checks `pendingQuestion` first.
- Serial (`is_serial`), and counted as bookkeeping.
- The step row reads `asked you: {question}` and finishes with
  `— you chose {labels}`. That is the only persistence it needs, because the
  call and result are in the transcript and the session log already.

### `AGC-4` Events

Add to `AgentEvent` (`agent/mod.rs`) and mirror in `src/lib/api.ts`:

```rust
Command    { run_id: String, name: String, by: CommandBy, outcome: String, note: Option<String> },
Proposal   { run_id: String, id: String, name: String, reason: String, payload: serde_json::Value },
Suggestion { run_id: String, command: String, reason: String },
Question   { run_id: String, id: String, question: String, options: Vec<QuestionOption>, multi: bool },
```

Proposals are answered with `resolve_harness_proposal_cmd(run_id, id, accept: bool)`.
On accept, the **frontend** runs the command (`/workspace on`, or opens the
schedule draft), because the effect belongs to the user's session. The command
records the trace outcome.

---

## CLR - Context clearing during a run

The SOTA answer to long runs is not summarising the conversation mid-run. It is
clearing old tool results that are no longer needed word for word, and keeping
them retrievable. `results.rs` already keeps large outputs on disk behind
`read_result`, so most of the machinery exists.

- `CLR-1` In `prepare_turn` (`run.rs:1597`), after `open_turn`: if
  `rc.context_window` is known and `estimate_tokens(&st.messages) >
  window * 3 / 4`, call `clear_old_results(st, KEEP_RECENT_RESULTS = 4)`.
- `CLR-2` `clear_old_results`: walk `role: "tool"` messages oldest first,
  skipping the last `KEEP_RECENT_RESULTS` and any already cleared. For each
  whose content is over 1,000 chars, store the full text with a new
  `ResultStore::keep_forced(call_id, &content)` and replace the content with
  `[I cleared this result to make room. read_result("{ref}") brings it back.]`.
  Stop as soon as the estimate is under 50% of the window. Never touch
  system, user or assistant messages, or the plan line.
- `CLR-3` Report it: `sink.emit(AgentEvent::Command { name: "compact", by: Agent, outcome: "done", note: "cleared {n} old results, about {k}k tokens" })`,
  plus a `command` trace row. If the `context` rung is `Off`, nothing is
  cleared, and the run behaves exactly as it does today.
- `CLR-4` `harness{name:"compact"}` runs the same routine with no threshold and
  `KEEP_RECENT_RESULTS = 2`.
- `CLR-5` The session log is untouched. It already holds the full results as
  they were appended, so a resume replays the originals and clears again if
  needed. That is correct, and it is why clearing is safe.
- `CLR-6` This also fixes the carried-forward drift in a narrower form:
  scheduled jobs and background children now clear tool output instead of
  overflowing. Summary compaction of their *conversation* still waits for
  `agent_chat_cmd`'s assembly to move into `TurnCtx::assemble`.

---

## PLF - Plan first

`PlanMode` stays what it is: whether the `plan` tool is offered. **Plan first**
is a separate, per-turn run option. The UI never says "plan mode", to keep the
two apart.

- `PLF-1` `Toolset::mutates(self, tool) -> bool` in `toolsets.rs`, written
  out for every tool, with a test that fails on any tool name not classified:
  - mutating: `write_file edit_file create_dir move_file delete_file run_code
    run_task run_command send_mail reply_mail generate_image create_artifact
    update_artifact render_ui present remember memory propose_soul_edit
    propose_skill browser_click browser_type browser_press open_app`
  - read-only: `read_file list_directory search_files find_symbol changes
    web_search fetch_url search_folder find_similar search_history
    read_conversation list_mail search_mail read_mail read_artifact
    check_preview browse browser_read browser_scroll browser_screenshot
    screenshot skill`
  - `delegate`, `check_agents`, `collect_agents`: allowed. Children inherit the
    read-only ceiling (`DelegationContext` gains `read_only: bool`, passed into
    the child's `ToolRegistry::build`).
  - MCP tools: allowed only when the server marked them
    `annotations.readOnlyHint: true`. Add
    `#[serde(default)] annotations: Option<McpToolAnnotations>` to `McpTool`
    and keep it in the cached `config_json`. Tools cached before this change
    have no annotations, so they are excluded until the connector is re-tested.
  - Self-check: `screenshot` is read-only but still goes through its own `screen`
    rung.
- `PLF-2` `ToolRegistry::build(…, read_only: bool)` drops mutating specs.
  `dispatch` checks again and refuses with
  `"I'm planning first. Nothing changes until you approve the plan."`
  The `plan` tool is offered even when `PlanMode` is `Never`.
- `PLF-3` `execute_turn` appends one system message to the run's transcript
  (not to the assembled system prompt, so the golden gate is untouched):
  `Plan first: investigate as much as you need, but change nothing. Write the plan with the plan tool, then stop and summarise it. The user approves it before anything changes.`
  `Plan` gains `#[serde(default)] pub awaiting_approval: bool`, set when the
  run was plan-first. It persists through `messages.plan_json`.
- `PLF-4` Approval: `PlanCard` on a plan with `awaiting_approval` shows a row:
  **Go ahead** · **Change something**. Go ahead sends `Go ahead with the plan.`
  with `RunOptions { approved_plan: Some(plan) }`. The run starts with
  `rc.resuming(Some(plan))` (the `PLN-T4` path), with `awaiting_approval`
  cleared and tools unrestricted. Change something focuses the composer with
  the placeholder `What should change in the plan?` and keeps the Plan first
  chip on, so the revision is also read-only.
- `PLF-5` Shift+Tab in the composer toggles the Plan first chip. That is the
  only mode-cycling key. The footer hint row in the menu documents it.
- `PLF-6` The agent's `harness{name:"switch_mode", mode:"plan_first"}`, when
  accepted, sets the chip for the **next** turn. It never re-scopes the current
  run.

---

## RWD - Rewind

Rewind is a branch plus an optional file undo. That makes it non-destructive:
the original conversation is never edited.

- `RWD-1` `rewind_cmd(conversation_id, message_id, undo_files: bool) -> Rewind`,
  where `message_id` is a **user** message (the turn to go back before):
  1. If `undo_files`: build
     `changes::change_set(db, conv, message.created_at)` and `undo_file` each
     file newest first (the `undo_changes_cmd` loop, `projects.rs:251`).
     `log_activity("file", "undid my changes since …")`.
  2. Fork the conversation up to the message **before** `message_id`, with the
     same boundary semantics as `forkFromMessage`.
  3. Write a `rewound` row into the **original** conversation (`REG-4`).
  4. Return `Rewind { branch_id, prompt: String, files_undone: usize }`.
- `RWD-2` `changes_since_cmd(conversation_id, since_ms) -> usize` so the dialog
  can show how many files would be undone before the user commits.
- `RWD-UI-1` `TurnActions` (`AgentRun.tsx:269`) gains **Rewind to before this**
  next to Try again from here. It opens `ConfirmDialog` with:
  - title `Go back to before "{preview}"?`
  - checkbox (only when `changes_since > 0`, default on):
    `Also take back my changes to {n} file(s)`
  - checkbox (default on): `Put your message back in the box`
  - buttons `Rewind` · `Cancel`
- `RWD-UI-2` After rewind: open the branch, put the prompt in the composer if
  asked, and show the toast
  `◆ I went back to before "{preview}". The original is still in your list.`
  When files were undone, add ` I took back my changes to {n} file(s).`
- `RWD-UI-3` `/rewind` with no argument lists the last 10 user turns in the
  argument phase (`CMP-5`). Choosing one opens the same dialog.

---

## BTW - A side question

- `BTW-1` `side_question_cmd(conversation_id, question, target, on_event: Channel<AgentEvent>)`:
  assemble the conversation with the Rust path the scheduler already uses
  (`context::from_db`), append the question as a user message, and run one
  tool-free streamed completion. Nothing is persisted, logged or traced.
  It works during a live run: it neither touches the run nor steers it. On a
  single-slot local engine it queues behind the run, so the card shows
  `waiting for my engine…` until the first token.
- `BTW-UI-1` `BtwCard.tsx` floats above the composer, inside `composer-col`:
  the question in muted text, the answer streaming beneath, and the actions
  **Keep in chat** (appends both as ordinary messages through
  `append_message_cmd`) and **Dismiss** (Esc). Only one card at a time; a new
  `/btw` replaces it. It is not in the transcript unless kept.

---

## CHK - Checkup

One command for "is anything wrong with me", replacing the idea of a separate
`/doctor`. It speaks in first person, and states are words, not colours.

- `CHK-1` `checkup_cmd(target) -> Checkup { lines: Vec<CheckupLine> }`, with
  `CheckupLine { area, state: "fine" | "needs_you" | "off", text, action: Option<CheckupAction> }`.
  Areas, each with a hard 4-second timeout:
  - engine: `runtime_status` → `My engine is running {model}.` /
    `My engine isn't running.` (action: Open Runtime)
  - providers: verified keys → `I can reach {provider}.` /
    `My key for {provider} no longer works.`
  - recall: embed and rerank status → `I can search what's in your folders.` /
    `I can't search inside folders. My recall engine isn't installed.`
  - connectors: `test_connector` on each enabled one → `I can't reach {name}. It timed out.`
  - tools: 7-day reliability under 50% with at least 5 calls →
    `{tool} has been failing for me lately ({ok} of {total}).`
  - behaviour: `check_golden_cmd` (one model call, said up front) →
    `I still behave the way you set me up ({passed} of {total} checks).`
- `CHK-UI-1` The result renders as a `CheckupCard` block in the transcript,
  persisted through its `command` trace note. Lines carry state words, never
  green and red. Each line's action is a text link.
- `CHK-UI-2` While it runs, presence is `tending` (`CPX-2`).

---

## GOL - Goal

Keep working toward an objective the **user** set, across turns, until a check
says it holds or the budget runs out. The agent cannot start, extend or change a
goal.

- `GOL-1` Stored in `session_state` under key `goal` (no schema change):
  `{ text, until, maxRounds: 5, round: 0, status: "active"|"met"|"stopped"|"exhausted", lastCheck }`.
  `/goal <objective> until <condition>` creates it. Without `until`, the
  objective doubles as the condition. `/goal stop` stops it.
- `GOL-2` `goal_check_cmd(conversation_id, target) -> GoalCheck { met: bool, evidence: String, next: String }`:
  one tool-free JSON completion over the goal, the last answer, and the
  `changes` summary. When the Judge plan's decision seam lands
  (`JUDGE_PLAN.md`), this goes through it.
- `GOL-3` Loop driver in the store's run-ended handler: if a goal is active and
  the run ended `completed` or `max_steps`, run the check. If not met and
  `round < maxRounds`, increment and send
  `Keep going toward our goal: {text}. Last check: {next}` as an ordinary turn,
  labelled in the transcript as `◆ Continuing toward our goal (round {r} of {max})`.
  A run ending `aborted` or `error` sets the goal to `stopped`. Stop always
  stops the goal too.
- `GOL-UI-1` No separate goal bar. While a goal is active it is the **first
  segment of the run bar** (`RUN-2`): `◎ {text} · round {r} of {max}`, with the
  last check's `next` as its title and **Stop goal** beside it. On `met`, the
  segment reads `◆ Done: {evidence}` until the next send. The goal's mode chip
  (`CMP-7`) mirrors it.

---

## RUN - The run bar

While a run works, what it is doing is spread across four places: the plan
card in the turn, the agents pill in the composer and the dock's Agents tab,
the dock's Changes tab, and Settings → Usage for cost. After it ends, nothing
answers "what did that just do?" in one glance. The run bar is one line that
answers both questions and links to each surface. It replaces none of them.

**What it is not.** It does not repeat `RunMeter`. Activity, clock and context
stay in the turn, where they belong to the answer being written. It never
shows the step budget or "step N of M": a limit is not a status (the rule
behind `AgentRun.meter.test.tsx`). It is not a dashboard. It has no gauges, no
progress bar and no colours for good or bad.

### `RUN-1` Live data: usage during the run

`RunProgress` gains the run's spend so far, so cost can be shown live instead
of only at the end:

```rust
RunProgress {
    run_id: String, step: usize, max_steps: usize, ms: u64, context_tokens: usize,
    /// OBS-1 running total for this run. None until a provider reports usage.
    usage: Option<crate::runtime::proxy::Usage>,
    /// pricing::cost_usd for the run's model, None when the price is unknown
    /// or the run is local. Unknown is not free: the UI shows nothing.
    cost_usd: Option<f64>,
}
```

`sink.run_progress` (`run.rs:1622`) passes `run.usage()` and
`pricing::cost_usd(model_name, …)`. The cost is `None` when
`rc.provenance` is local. Mirror the fields in `api.ts`, and keep them on
`activeRun` in the store.

**Children's cost, verified 2026-10-06.** How a child's usage travels today:

| Hop | What happens | Where |
|---|---|---|
| Child run | A foreground child runs the same `run_agent` loop on `ctx.sink.child(run_id)`, so its own `RunEnded { usage }` goes out wrapped as `Sub { run_id, event: RunEnded }` on the **lead's** channel. | `subagents.rs:462,535`, `run.rs:393-410`, `:1478` |
| Background child | Same wrapping, sent on the app bus (`event_channel` → `poiesis-agent-sub`), because the lead's turn is usually over. | `background.rs:218-222`, `:242-265` |
| Lead's own usage | `RunHandle` sums only its own turns. A child has its own handle, so the lead's `run.usage()` **never includes children**. | `fleet.rs:169-175` |
| Frontend | `applySubEvent` has no `run_ended` case, so the nested usage hits `default: return {}` and is **dropped**. `SubRun` has no usage field. | `store.ts:4403-4494` |
| Database | `record_run_usage` writes each child's row under the **child's** conversation id. `usage_summary` groups by raw `conversation_id`, so the Usage page lists delegated work as separate conversations and the parent's row is too low. Totals and per-model figures are correct. | `run.rs:1463`, `db/mod.rs:1973-2024` |
| Price | `RunEnded` carries tokens only. A child may run on a different model than the lead (`spawn.model_name`), so the frontend cannot price it from the lead's model. | `agent/mod.rs:271-283`, `background.rs:257` |

So the data exists and reaches the webview, but nothing keeps it. Three
small tasks make the bar's cost cover the whole tree:

- `RUN-1a` `RunEnded` gains `cost_usd: Option<f64>` (computed in `end`,
  `run.rs:1443`, with the run's own `model_name` and `None` for local), so every
  run, child or lead, arrives priced by the side that knows its model.
- `RUN-1b` `applySubEvent` handles `run_ended`: it stores
  `usage` and `costUsd` on the `SubRun` (new optional fields in `types.ts`).
  `run_progress` from a child likewise updates a live `costUsd` on the
  `SubRun`. That needs the `RUN-1` fields on `RunProgress`, which children send
  through the same sink.
- `RUN-1c` The run bar's cost segment is the lead's `activeRun.costUsd` plus
  `costUsd` of every `SubRun` whose `parentConversationId` is this
  conversation and that started during this run (foreground and background).
  If any child's cost is unknown while the lead's is known, the segment reads
  `$0.04 so far + agents` with the title
  `I can't price what one of my agents used.` It never shows a partial sum as
  the total.
- `RUN-1d` `usage_summary` rolls child conversations into their parent for
  `by_conversation`: group by
  `COALESCE(parent conversation id, conversation_id)`, using the column
  `set_conversation_parent` writes (`subagents.rs:407`). The bucket gains
  `agents_runs: u32` so the Usage page can say `including 3 agent runs`.
  Without this, `/usage` (`UCM-10`) would point at a parent row missing its
  agents' spend.

### `RUN-2` What the bar shows

`RunBar.tsx` (new, in `Composer/`) is the first row of the composer column,
above everything else that stacks there (`BtwCard`, `SuggestionChip`,
`ModifierChips`). One line of segments, each shown only when it has something
true to say, separated by `·`:

| Segment | Live run | After the run | Click |
|---|---|---|---|
| Goal (`GOL`) | `◎ {goal} · round 2 of 5` + **Stop goal** | `◆ Done: {evidence}` | scroll to the goal's latest round |
| Plan | `plan 3 of 6` (items done of total; dropped items not counted) | `plan done` / `stopped at 4 of 6` | scroll to the `PlanCard` |
| Agents | `2 agents working` | `2 agents reported back` / `1 still working` (background) | dock → Agents |
| Files | `changed 4 files` | `changed 4 files · Undo · Keep` | dock → Changes |
| Cost | `$0.04 so far` | `$0.06` | Usage focused on this conversation (`UCM-10`) |
| Waiting on you | `I asked you something ↑` (while `ask_user` is pending) | — | scroll to the `QuestionCard` |

- Files are counted live from the existing `file_changed` events
  (`AgentEventSink::file_changed`, `run.rs:453`), deduplicated by path. After
  the run, the count comes from `conversation_changes_cmd` with `this_run`, so
  the number matches the Changes view exactly.
- **Undo · Keep** on the finished files segment call `undoChanges` /
  `keepChanges`. Undo goes through the same confirmation as `/undo`.
- Cost: no segment when the cost is unknown. A local run shows `on this machine`
  once, instead of `$0.00`.
- The agents pill in the composer row (`Composer.tsx:949-960`) is **removed**.
  Its job moves to the Agents segment, and the Stop button's title keeps saying
  how many agents Stop will take (`SUB-UI-3`).

### `RUN-3` When it shows

- It appears when a run starts and stays after the run ends, showing that run's
  summary, until the next send (which replaces it with the new live run), until
  the user dismisses it with ×, or until they switch conversations. The summary
  is in-memory per conversation (`runSummaries: Record<convId, RunSummary>`) and
  is not persisted.
- If no segment has anything to say, it isn't shown. A plain chat answer with
  no plan, agents, files or known cost renders no bar. For ordinary questions
  the composer looks exactly as it does today.
- A run that ended `max_steps`, `timeout` or `aborted` adds
  **Continue where I stopped** to the after-run bar (the same action as
  `TurnActions`), because that is the moment you want it.

### `RUN-4` Look and copy

- One line, 28px tall, `--ink-muted` text at the UI size used by
  `composer-footer`. Segments are text buttons, not chips. There's a 1px
  hairline under the bar, and it has no background of its own.
- Under 520px wide: segments past the second collapse into `+N` with the full
  list in a popover.
- Live updates do not animate. Numbers change in place. The Orb in `RunMeter`
  already carries the motion, and one motion at a time (quiet biology).
- `role="status"`, `aria-live="polite"`, with updates throttled to one per
  2 seconds so screen readers aren't flooded during a busy run.
- First person where the bar speaks about Poiesis: `I asked you something ↑`,
  `I changed 4 files`. A plain count stands for itself: `plan 3 of 6`,
  `$0.04 so far`.

---

## DEF - Settings become defaults

Plan, step budget and effort are decisions about **this** message. Today the
first two exist only as global settings in two different pages. After this,
the composer chip is the normal way to set them, and Settings says it holds
only the default. Behaviour settings that really are global (auto-compaction,
autonomy) are not touched.

- `DEF-1` **Effort.** `EffortPicker` stays in the footer as the default. While
  an effort chip is set (`CMP-6`), the picker shows the chip's value with a
  small `·` marker and the title
  `Just this message. My default is {default}.`. Picking a value in the
  dropdown while a chip is set removes the chip and sets the default, so the
  two can never disagree on screen.
- `DEF-2` **Make default.** Every modifier chip has a hover/focus affordance
  `make default`. For effort it sets `models.reasoning_effort`, for steps
  `agent.max_steps`, and for Plan first `agent.plan_first_default` (`DEF-4`).
  The chip stays for this message. The confirmation is a `CommandNote`:
  `/effort high is now my default · you`.
- `DEF-3` **Settings copy.** Each setting is relabelled as a default and points
  to the per-message control:
  - Settings, plan mode: heading `How I plan, by default`. Options stay
    `always / when it helps / never` (unchanged values). Hint:
    `For one message, type /plan to have me plan first and change nothing until you approve.`
    The page explains the difference in one sentence: this setting decides
    whether I keep a plan while I work, and `/plan` decides whether I may change
    anything before you approve.
  - Settings → Tools, step cap: heading `How many steps I take by default`.
    Hint: `For one message, type /steps 30.` The value range is unchanged.
  - Effort: no Settings page. It is already in the footer.
- `DEF-4` **Plan first by default.** New setting `agent.plan_first_default`
  (`"true"|"false"`, default false), shown in Settings under the plan-mode
  block as the checkbox `Plan first for every message (you approve before I
  change anything)`. When it is on, every new turn starts with the Plan first
  chip already set, and × removes it for that message. After **Go ahead**
  (`PLF-4`), the approval turn never gets the chip, or approval would loop
  forever.
- `DEF-5` **Where the default came from.** A chip set from a default renders
  with the label `◇ Plan first (default)`. Removing it means "not this time"
  and never changes the setting.

---

## HOK - User hooks (deferred, decide after Phase 5)

User-authored scripts on harness events (`before_tool`, `after_turn`,
`on_file_change`, `on_run_end`), in the Claude Code shape. Deferred because the
security surface is large. Constraints if it is built:

- user-authored only. The agent can never write, propose or edit a hook
  ("No code self-modification").
- they run through the existing `run_command` sandbox and exec policy, with the
  project's policy applying.
- every hook run is a `command` trace row with `by: "hook"`.

---

# Part IV - The Poiesis experience (`CPX`)

Commands are where the user meets the harness most directly, so they are also
where Poiesis has to feel like itself and not like a CLI. These tasks are the
deliverable, not garnish (see the memory note on IDX/RET).

### `CPX-1` First-person copy (authoritative, extends PRES-0)

| Site | Copy |
|---|---|
| Section header for self-commands | `Me` |
| `/remember` hint | `I'll remember that` |
| `/forget` hint | `I'll forget it` |
| `/always` hint | `I'll always do that` |
| `/reflect` hint | `Let me think back over this conversation` |
| `/skillify` hint | `I'll turn what we just did into a skill` |
| `/tidy` hint | `Let me tidy up my memory` |
| `/checkup` hint | `Let me check myself` |
| `/self` hint | `Visit me` |
| `/autonomy` hint | `How much I may change without asking` |
| `/context` hint | `What I'm holding in mind right now` |
| `/why` hint | `Why I gave my last answer` |
| Agent cleared context (note) | `◆ I made room: I cleared {n} old results I no longer need word for word.` |
| Agent suggestion chip | `◆ {reason} · /{command} · Do it · Not now` |
| Mode proposal (in run) | `◆ I'd like to switch to {Workspace\|Plan first}: {reason} · Switch · Not now` |
| Schedule proposal (in run) | `◆ I'd like to run this {when}: {task} · Set it up · Not now` |
| Question card header | `I need you to decide` |
| Question answered (step) | `asked you: {question} — you chose {labels}` |
| Plan-first approval | `This is my plan. Nothing has changed yet. · Go ahead · Change something` |
| Rewind toast | `◆ I went back to before "{preview}". The original is still in your list.` |
| `/always` toast | `◆ I'll always do that now: {instruction} — Undo` |
| Goal met | `◆ Done: {evidence}` |
| Run bar, waiting on an answer | `I asked you something ↑` |
| Run bar, files | `I changed {n} file(s)` (live) · `I changed {n} file(s) · Undo · Keep` (after) |
| Effort picker under a chip | `Just this message. My default is {default}.` |
| Chip made default (note) | `/{name} {value} is now my default · you` |
| Plan first default checkbox | `Plan first for every message (you approve before I change anything)` |
| User command note | `/{name} {args} · you · {time}` |
| Agent command note | `◆ {first-person note} · {time}` |
| Autonomy rows (Self → Autonomy) | context: `Making room in my head during long work` · suggest: `Suggesting commands to you` · modes: `Switching modes (Workspace, Plan first)` · schedule: `Scheduling work for later` |

### `CPX-2` The Orb shows self-acts

- New presence value `tending` (the type in `store.ts` and `orbForPresence`)
  → Orb `shaping`. It is set for the duration of context clearing, `/checkup`,
  `/tidy` and rewind, then returns to whatever presence was underneath. One
  motion at a time: `tending` replaces `active` while it lasts, and does not
  layer over it.
- A pending `ask_user` → `listening` (unused so far). The run's orb listens
  while it waits for the user, and returns to its step state on the answer.
- `orbForStep` gains `/^(made room|cleared)/ → "shaping"` and
  `/^asked you/ → "listening"`. Extend `orbState.test.ts`.
- Reduced motion: no change from today. Static orb, state in the label
  (`Poiesis Agent — tidying my head`, `— waiting for your answer`).

### `CPX-3` Self-acts are witnessed where they happen

Agent `harness` calls never appear as bare tool rows. `describe()`
(`run.rs:2545`) maps them to first-person verbs: `made room`,
`suggested /skillify`, `asked to switch to Workspace`, `asked to schedule …`.
The note sits in the run's timeline, not in a panel. Nothing about the agent's
own session changes without a row the user can see in the conversation (Part I
§5.2 of `POIESIS_PLAN.md`).

### `CPX-4` The menu grows

The **My skills** section is where growth shows up. A skill with
`origin: poiesis` (`SKC-5`) has the hint `I learned this on {date}` instead of
its description. There is no "new" badge, no count, and no highlight. After
`/skillify` is accepted, the next time the user opens `/`, the skill is there,
with that line. That moment is the experience. Do not add a toast for it.

### `CPX-5` Corrections become learning signals

The user's control actions feed the REFLECT step of the Poiesis loop:

- `reflect.rs` reads the conversation's `rewound` rows and `command` rows with
  outcome `declined`. For each `rewound` row it adds a line to the reflection
  input: `The user went back to before "{preview}" and abandoned what followed.`
  Abandoned work is the strongest negative signal Poiesis gets, and today it
  is invisible to reflection.
- A lesson that came from a rewind keeps where it came from:
  `lesson.origin = "rewind"`. The Lessons tab shows it as the hint
  `learned when you went back on {date}`. This adopts OPT "scars" from the
  idea reservoir, narrowly: one origin line, no visual treatment.
- A suggestion declined twice for the same command across conversations stops
  being suggested. `AGC-2` checks the last 20 `declined` rows for that
  command, across all conversations. Poiesis learns what this user doesn't want
  suggested.

### `CPX-6` The membrane is visible in the menu

The `◆` at the end of a menu row means "I can do this myself too". Its `title`
reads `I can do this on my own as well — you decide how much in my Self panel`,
and clicking the glyph opens Self → Autonomy scrolled to that class. It is a
fixed glyph, not a count or a state. It is the same for every agent-callable
row.

### Kitsch check

- No animation in the menu. No sounds. No celebratory toasts for using
  commands. No usage stats per command. No "you've used /compact 12 times".
- Suggestions are rare by construction (once per command per conversation,
  learned suppression), so they cannot become nagging.
- If a `CPX` task needs a new colour, a badge or a percentage, it is wrong.

---

# Part V - UI integration map

| Surface | Files | What the user sees | Tasks |
|---|---|---|---|
| Composer `+` | `Composer/PlusMenu.tsx` (new), `Composer.css` | Add files, folder, library item, earlier conversation | `CMP-1`, `CMP-3` |
| Composer `/` button + menu | `Composer/CommandMenu.tsx` + `.css` (new), `useCommandInput.ts` (new) | Sectioned command list, recents, disabled reasons, ◆ glyph, argument phase | `CMP-4`, `CMP-5`, `CMP-9`, `CPX-6` |
| Modifier chips | `Composer/ModifierChips.tsx` (new) | Plan first, Think: high, Up to N steps | `CMP-6`, `PLF-5` |
| Mode chips | `Composer/ModeChips.tsx` (new) | Workspace, Tools off, Persona, Goal | `CMP-7` |
| Suggestion chip | `Composer/SuggestionChip.tsx` (new) | One first-person suggestion with Do it · Not now | `AGC-2`, `CPX-1` |
| By the way | `Composer/BtwCard.tsx` (new) | Side answer, Keep in chat · Dismiss | `BTW-UI-1` |
| Run bar | `Composer/RunBar.tsx` (new), `Composer.tsx` (agents pill removed), store `activeRun` + `runSummaries` | Goal, plan progress, agents, changed files with Undo · Keep, cost, "I asked you something"; after the run, its summary until the next send | `RUN-1..4`, `GOL-UI-1` |
| Defaults | `EffortPicker.tsx`, `routes/Settings.tsx`, `routes/Tools.tsx`, `ModifierChips.tsx` | Chips are the per-message control; `make default` on each chip; Settings relabelled as defaults; Plan first by default | `DEF-1..5` |
| Command notes | `Conversation/CommandNote.tsx` (new), `routes/Chat.tsx` | One muted line per traced command, placed by time like `CompactDivider` | `REG-4`, `CPX-1` |
| User bubble chip | message renderer for user turns | `/review` chip with a "what I was asked" disclosure | `SKC-2` (`CMP-UI-6`) |
| Question card | `Conversation/QuestionCard.tsx` (new), `AgentRun.tsx` | Question, 2-4 option buttons (keys 1-4), multi-select + Send, "Something else…" | `AGC-3`, `CPX-2` |
| In-run proposals | `Conversation/HarnessProposal.tsx` (new), `AgentRun.tsx` | Switch / Set it up · Not now | `AGC-1`, `AGC-4` |
| Plan approval | `Conversation/PlanCard.tsx` | Go ahead · Change something on a plan-first plan | `PLF-4` |
| Rewind | `AgentRun.tsx` `TurnActions`, `Confirm/ConfirmDialog.tsx` | Rewind to before this, dialog with file-undo checkbox | `RWD-UI-1..3` |
| Checkup | `Conversation/CheckupCard.tsx` (new) | First-person lines with state words and links | `CHK-UI-1` |
| Orb | `Orb/orbState.ts`, presence slice in `store.ts` | `shaping` while tending, `listening` while waiting for an answer | `CPX-2` |
| Self → Autonomy | `Self/SelfPanel.tsx` (Autonomy tab) | Four new first-person rows; schedule has no "auto" | `AGC-1`, `CPX-1` |
| Self → Lessons | Lessons tab | `learned when you went back on {date}` | `CPX-5` |
| Skills view | `routes/Skills.tsx` | Argument hint and "only you can run this" for `disable-model-invocation`; the `◇ partial` chip no longer fires for the three keys | `SKC-1` |
| Palette | `CommandPalette.tsx` | Unchanged; imports `lib/fuzzy.ts` | `CMP-2` |
| Keyboard | `useCommandInput.ts`, `App.tsx` | `/`, `Ctrl /`, Tab, Shift+Tab, 1-4 on questions | `CMP-4`, `PLF-5`, `AGC-3` |

---

# Part VI - Phases

Each phase ships on its own and leaves the app better than before.

### Phase 1 - Two buttons and the registry

`REG-1..3`, `REG-5` (effort and max_steps only), `CMP-1..9`, `UCM-1`, `UCM-2`,
`UCM-4`, `UCM-10`, `DEF-1..3`, plus every phase-1 binding in Part II.

**Accept:** `+` holds only the four content items. `/` (button, key, `Ctrl /`)
opens a sectioned menu with recents. Every phase-1 command works. Disabled
rows say why. `/effort high add tests` sends one turn at high effort and the
next turn uses the default again. While the chip is set, the effort picker
shows it as "just this message". `make default` on a chip changes the setting
and leaves a note. Settings and Tools describe plan mode and the step cap as
defaults and name the commands. Workspace, Tools and Persona show as footer
chips that can be removed. Existing slash tests pass.

### Phase 2 - Poiesis commands, traces and skills as commands

`REG-4`, `UCM-3`, `UCM-5..7`, `SKC-1..5`, `CPX-1`, `CPX-4`, `CPX-6`,
`CommandNote`, `RUN-1`, `RUN-1a..d`, `RUN-2..4` (the goal and "asked you"
segments arrive with their features in Phases 5 and 3). `RUN-1d` fixes an
existing Usage-page bug and can ship on its own ahead of the rest.

**Accept:** `/remember`, `/forget` and `/always` fire the existing first-person
toasts with Undo. `/reflect` pulses the rail ◆ (PRES-2). Traced commands show
as notes that survive reload and fork. `/review` sends with the skill
preloaded (the timeline shows `used my review skill` before the first model
token). A skill with `disable-model-invocation: true` is in the menu and absent
from the system prompt, and the golden gate passes. `/skillify`, once
accepted, appears under My skills with `I learned this on …`. A run that
plans, delegates and edits shows `plan 2 of 4 · 1 agent working · I changed 3
files · $0.02 so far` above the composer, with no step count anywhere. After
it ends, the bar keeps its summary with Undo · Keep until the next send. A
plain question shows no bar. The agents pill is gone from the composer row.

### Phase 3 - The agent's side

`AGC-1..4`, `CLR-1..6`, `CPX-2`, `CPX-3`, autonomy rows.

**Accept:** a run reading many large files clears old results past 75% and
shows `◆ I made room…`, and the model can `read_result` them back. An
`ask_user` call shows the question card, the Orb listens, the answer arrives as
a tool result, and typing in the composer answers the question instead of
steering. Setting `suggest` to off removes `suggest` from the enum (check the
tool spec in the session log). `schedule` has no Auto option, and a hand-set
`autonomy.schedule=auto` still asks.

### Phase 4 - Safety and recovery

`PLF-1..6`, `RWD-1..2`, `RWD-UI-1..3`, `BTW-1`, `BTW-UI-1`, `CHK-1`,
`CHK-UI-1..2`, `CPX-5`, `DEF-4`, `DEF-5`.

**Accept:** `/plan refactor X` yields a plan with no file changes (the changes
view is empty) and a Go ahead button, and Go ahead executes that plan. A write
attempted in plan-first mode is refused with the first-person message. Rewind
with file undo restores the files and opens a branch with the prompt in the
box, and the original is untouched. The next reflection on the original sees
the rewind line. `/btw` answers during a live run without changing it.
With Plan first by default on, every new message starts with
`◇ Plan first (default)`, removing it affects only that message, and Go ahead
never loops back into planning.
`/checkup` lists every area in first person within ~5 s plus the golden call.

### Phase 5 - Longer horizons

`GOL-1..3`, `GOL-UI-1`, `UCM-8` (`parseWhen`), `UCM-9`, MCP prompts as commands
(`prompts/list` from enabled connectors, section `skills`, kind `skill`, sent
as the prompt text with arguments substituted).

**Accept:** `/goal make the tests pass until npm test exits 0` runs up to 5
rounds and stops on met, on Stop, or on exhaustion, each with the right bar
state. `/schedule every weekday at 9 summarise my mail` opens a prefilled draft
and saves nothing without Save. `/export` writes a readable Markdown file.

### Phase 6 - Hooks (decision gate)

Only after Phase 5, and only if wanted. See `HOK`.

**Decision (2026-10-06):** parked for later. Not started. If it comes back, write
a full spec for `HOK` first (events, sandbox, trace rows), and consider starting
with `after_turn` and `on_run_end` only, since `before_tool` can block the agent.

---

# Part VII - Tests

| ID | What | Where |
|---|---|---|
| `REG-T1` | Every manifest entry has a frontend handler and every handler has a manifest entry; names and aliases are unique | `src/lib/commands.test.ts` |
| `REG-T2` | `agent_callable` drops `Off` classes and drops all but `compact` when headless | `agent/commands.rs` |
| `REG-T3` | `replay` skips `command` and `rewound` rows; `fork_conversation` copies `command` rows up to the boundary | `agent/log.rs` |
| `CMP-T1` | `parseCommandLine`: known name, alias, args, unknown `/usr/bin` → null, mid-sentence `/` ignored | `commands.test.ts` |
| `CMP-T2` | Menu: `/` button opens it, sections in order, disabled row not selectable and announces its reason, Tab enters the argument phase, Esc keeps text | `Composer.commands.test.tsx` |
| `CMP-T3` | `+` menu has exactly the four content items | `Composer.plus.test.tsx` |
| `CMP-T4` | Modifiers map to `RunOptions` and clear after one send | `store.commands.test.ts` |
| `SKC-T1` | Frontmatter parsing for the three keys; the `◇ partial` list no longer includes them | `skillpack.rs` |
| `SKC-T2` | Explicit invocation synthesises the `skill` call/result pair with `$ARGUMENTS` substituted; `loaded_skills` dedupes a later model `skill` call | `run.rs` |
| `SKC-T3` | Golden fixture with a `disable-model-invocation` skill, on both sides | `context_golden.rs`, `prompt-assembly.test.ts` |
| `AGC-T1` | `harness` rungs: Auto executes, Ask emits a proposal and does not block, suggest dedupes per conversation, learned suppression after two declines | `run.rs` |
| `AGC-T2` | `ask_user`: answer resolves the call; cancel resolves with the stopped error; children are never offered it; headless is never offered it | `run.rs`, `fleet.rs` |
| `AGC-T3` | `NO_AUTO`: `autonomy.schedule=auto` reads as Ask | `autonomy.rs` |
| `CLR-T1` | Clearing keeps the last N results, never touches non-tool messages, stops under 50%, stubs are retrievable with `read_result`, and the `context` rung `Off` disables it | `run.rs` (model-free, like `open_turn`'s tests) |
| `PLF-T1` | `mutates` classifies every tool name every toolset advertises (a test iterates `tool_specs()`) | `toolsets.rs` |
| `PLF-T2` | A read-only registry has no mutating specs; dispatch refuses a mutating call anyway; a child inherits read-only | `run.rs` |
| `PLF-T3` | Go ahead resumes with the approved plan and unrestricted tools | `PlanCard.test.tsx`, `run.rs` |
| `RWD-T1` | Rewind with undo restores file bytes, forks before the turn, writes `rewound` in the original, and leaves the original's messages untouched | `commands/` integration test |
| `CPX-T1` | `orbForPresence("tending")` → shaping; `orbForStep("asked you: …")` → listening | `orbState.test.ts` |
| `CPX-T2` | Every `me`-section summary in the manifest starts with "I", "Let me" or "Visit me" (copy lint) | `commands.test.ts` |
| `GOL-T1` | The loop stops on met, Stop, abort and exhaustion, and never starts from an agent event | `store.goal.test.ts` |
| `RUN-T1` | `RunProgress` and `RunEnded` carry usage and `cost_usd`; `cost_usd` is `None` for local runs and unknown prices | `run.rs` |
| `RUN-T1b` | A nested `run_ended` and `run_progress` from a child set `usage`/`costUsd` on its `SubRun` (extends `store.subruns.test.ts`, which today only covers steps, tokens, permissions, steers and `run_started`) | `store.subruns.test.ts` |
| `RUN-T1c` | The bar's cost = lead + this run's children; one unpriced child gives `+ agents`, never a partial total | `RunBar.test.tsx` |
| `RUN-T1d` | `usage_summary` folds a child conversation's rows into its parent's `by_conversation` bucket and counts `agents_runs`; totals and `by_model` are unchanged | `db/mod.rs` (next to the existing usage tests at `:4805`) |
| `RUN-T2` | Bar segments: hidden when empty; files deduplicated by path; after-run file count equals `conversation_changes_cmd` `this_run`; **no text matching `/step|of \d+ steps|max/` ever renders** (the meter test's rule, applied here) | `RunBar.test.tsx` |
| `RUN-T3` | Summary lifetime: survives run end, cleared by next send, ×, and conversation switch | `RunBar.test.tsx` |
| `DEF-T1` | Effort picker shows the chip's value while one is set; picking a value clears the chip; `make default` writes the right setting key for each chip | `EffortPicker.test.tsx`, `ModifierChips.test.tsx` |
| `DEF-T2` | `agent.plan_first_default` pre-sets the chip; removing it doesn't change the setting; the Go ahead turn never carries it | `store.commands.test.ts` |

Run `cargo test` (not `--lib`; `tests/eval.rs` only builds that way) and
`npx vitest run`.

---

# Part VIII - Gotchas

- **The golden gate.** Only `SKC-3` touches prompt assembly. Everything else
  puts its guidance in tool descriptions or in run-transcript system messages
  (`PLF-3`) on purpose. Both sides change in one commit.
- **The replay skip list** (`log.rs:193`) must name every new kind, or the model
  sees its own command trace.
- **zustand v5 selectors.** `commandViews(state)` builds a fresh array. Call it
  inside `useMemo` over stable slices (as `enabledSkills` does at
  `Composer.tsx:82-83`), never inside a selector.
- **The `on` class on `+`** currently signals Workspace mode. Moving that to a
  footer chip is intended, so update any test that asserts it.
- **`agent_chat_cmd` still assembles on the frontend.** `RunOptions` rides
  beside the messages and does not change assembly. `/btw` and `/goal`'s check
  use the Rust `context::from_db` path, which is already gated.
- **Single-slot local engine.** `/btw` and `goal_check_cmd` queue behind a live
  run on a local engine, and the UI says so rather than looking stuck.
- **`run_command` vs `harness`.** Do not rename `harness` to anything containing
  "command" or "run". Small models conflate them.
- **The Windows manifest in `build.rs`** stays as it is. `cargo test` dies at
  load without it.
- **The run bar is not the run meter.** Activity, clock and context stay in
  `RunMeter` inside the turn. The bar must never show a step count or the step
  budget. A limit is not a status, and the user has rejected it explicitly.
- **MCP annotations** are only present after a connector is re-tested.
  Plan-first excludes un-annotated MCP tools rather than guessing.

---

# Part IX - Decisions taken (change them before Phase 1 if you disagree)

1. **`/model` is persistent**, like the picker, and not a one-turn modifier.
   It matches Claude Code and the picker the user already knows.
2. **Rewind branches; it doesn't edit in place.** It is non-destructive and
   reuses fork. The cost is one extra conversation in the rail per rewind.
3. **Children never ask the user.** `ask_user` is lead-only, so there is one
   voice talking to the user.
4. **The prompt-style built-ins are App skills**, so users can read, copy and
   adapt `/review` the same way as any skill.
5. **Schedule never gets `auto`.** Unattended runs always need a click.
6. **No separate `/doctor`.** `/checkup` covers it in Poiesis's own voice.
7. **Hooks are deferred** to a decision after Phase 5.
