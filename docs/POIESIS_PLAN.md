# Project Poiesis — Master Plan: an autopoietic local agent

**Poiesis** (ποίησις, "bringing-forth") is the new name and the new concept for
what was Project Nexus. The name derives from **autopoiesis** (Maturana &
Varela): a system that continuously *produces and repairs the components that
constitute it*. Poiesis is a local-first desktop agent that doesn't just *use*
its memory, instructions, procedures, and workspace — it **maintains** them:
it observes its own failures, distills lessons, repairs degraded parts, and
slowly evolves its own way of working, with the user as the boundary that
decides what may change.

> This document **supersedes and absorbs** `docs/AGENT_MEMORY_PLAN.md`
> (Phase 10), which is now a pointer stub. Companion:
> `docs/IMPLEMENTATION_PLAN.md` (Phases 0–8, built). Checklist:
> `docs/TASKS.md` (Phase 10 entries must be regenerated from this document).
> Written to be implementable task-by-task without further design decisions:
> every task names its files, signatures, SQL, and acceptance check.
>
> ID prefixes — **BRAND** rename · **CTX** context · **RCL** recall ·
> **MEM** memory store · **SOUL** evolvable instructions · **GRM** grammar ·
> **LOOP** loop hygiene · **REF** reflection/lessons · **HEAL** self-repair ·
> **RCP** recipes · **AUT** autonomy ladder · **ORG** organism UI ·
> **PRES** presence (the experiential layer). `-UI-` tasks are frontend.
>
> **Build order:** BRAND → 10A → 10B → 10C → 10D → 11A → 11B ∥ 11C → 11D →
> 11E → 11F. 10E/10F are independent and can land any time after 10A.
> **PRES-0 (first-person copy) is not sequenced — it binds every `-UI-`
> task from BRAND onward**; build each UI task with its PRES-0 copy from the
> start, don't retrofit.
> **Part VI is the post-Phase-11 carry-over list** — what is implemented but
> unverified, plus known rough edges.
> **Part VII (OPT-1…12) is an unscheduled idea reservoir** — optional
> explorations adopted individually, never implemented as a batch.
> **Settled decisions (2026-07-18):** one merged plan doc · graduated
> autonomy ladder · recipes are in scope for v1 · rename is brand-level only
> (no internal identifier / path / crate renames — repo has no git).

---

# Part I — The concept

## 1. What we take from autopoiesis, and what we reject

Biological autopoiesis has properties that translate directly to a useful
agent, and properties that would make it a research toy or a hazard. We are
explicit about both.

**Adopted:**

| Biology | Poiesis |
|---|---|
| Self-production — the cell makes the components that make the cell | The agent produces and maintains its own constituting components: **memory facts, lessons, standing instructions (soul), procedures (recipes), workspace surfaces** |
| Self-repair — damage is detected and fixed from within | **Healing**: engine watchdog restarts a crashed runtime; degraded tools get cautions/retries; corrupt memory files are quarantined, never crash the store |
| Homeostasis — internal variables kept in range | **Context homeostasis**: token budgeting + compaction keep the context window healthy without losing the past |
| Structural coupling — the environment perturbs, the system decides how to change | User corrections and tool outcomes are *signals*; the **reflection pass** decides what internal change (a lesson, a proposal) they cause |
| Membrane — a boundary controls what enters the organization | The **autonomy ladder**: every class of self-change is gated at a rung (auto-with-undo / ask-first / off), the user configures the rungs |
| Operational closure | Everything above runs **on-device**; self-production never requires the cloud (it uses whatever endpoint the chat uses) |

**Rejected (deliberately, forever):**

- **No self-replication.** Poiesis never copies, spawns, or distributes itself.
- **No goal autonomy.** Poiesis never invents its own goals; it only gets
  better at the user's goals. Reflection runs are event-triggered by user
  activity, never a free-running background cognition loop.
- **No code self-modification.** The mutable "self" is *data* (markdown files,
  prompts, settings) — never the program. This is the hard safety line.
- **No opaque change.** Every self-change is visible (event + activity log +
  toast), reversible (trash/snapshots), and inspectable (plain markdown the
  user can open in Notepad).

## 2. The Poiesis loop

Every conversation participates in one cycle:

```
        ┌──────────────────────────────────────────────┐
        │  1. ACT        the existing agent loop        │
        │  2. SENSE SELF tool errors, retries, user     │
        │                corrections → signals           │
        │  3. REFLECT    end-of-conversation pass       │
        │                distills ≤3 lessons  (11A)     │
        │  4. REPAIR     watchdog, tool cautions,       │
        │                quarantine, compaction (11B)   │
        │  5. EVOLVE     memory · soul · recipes,       │
        │                gated by the membrane  (11C/D) │
        └──────────────────────────────────────────────┘
```

Steps 1–2 are per-turn, 3 is per-conversation, 4 is continuous, 5 is
occasional and user-gated at the identity level.

## 3. The workspace is the body

The workspace (the `render_ui` dynamic surface, a per-conversation mode) is
where the organism is *visible and touchable*, and it is central to the
concept in three concrete ways:

1. **Recipes are grown from workspaces.** A workspace surface that worked for
   a task can be distilled into a **recipe** — a stored procedure with an
   optional surface template — and a new workspace can be **started from a
   recipe** (RCP-UI-2). The agent's most tangible self-produced components
   are reusable workspace layouts.
2. **The organism panel** (ORG-UI-1) shows the agent's own vitality: memory,
   lessons, recipes, tool health, engine self-heals, pending self-change
   proposals — the same design language as any workspace surface.
3. **Context homeostasis is workspace-aware** (CTX-4 rule): in workspace
   mode the live surface is authoritative state, so compaction keeps fewer
   raw turns.

## 4. The mutable self — five component stores

| Component | Where | Who writes | Membrane rung (default) |
|---|---|---|---|
| Memory facts | `memory/facts/*.md` | agent tool (in-run) | **auto + undo toast** |
| Lessons | `memory/lessons/*.md` | reflection pass | **auto + undo toast** (high-confidence only) |
| Soul (standing instructions) | `memory/SOUL.md` | user; agent only proposes | **ask first** |
| Recipes | `memory/recipes/*.md` | agent proposes; user approves | **ask first** |
| Workspace surfaces | `blocks` table (exists) | agent (`render_ui`) | auto (already shipped) |

Consolidation ("tidy up") of facts+lessons is also **ask first**. The rungs
are settings (Part IV, AUT) — the table above is only the defaults.

## 5. Feeling the organism — experiential principles

The concept fails if it exists only as plumbing behind settings tabs. The
user must *see and feel* that they work with a living, self-maintaining
thing. Four principles govern every UI decision — implemented concretely as
workstream 11F (PRES), but **binding on all `-UI-` tasks in this plan**:

1. **First person.** Poiesis speaks about its self-maintenance as "I": "I'll
   remember that", "I learned something", "I restarted my engine", "I'd like
   to keep this procedure". Never "the agent", "the system", "memory saved",
   "operation completed". The PRES-0 copy table is authoritative wherever it
   conflicts with copy written in earlier task specs.
2. **Growth is witnessed, not logged.** Every self-change produces a moment
   the user can watch happen where they already are — a toast, a pulse of
   the living mark, a rail row visibly "digesting" a finished conversation.
   Nothing about the self changes silently; nothing requires opening a panel
   to be discovered.
3. **The self is a place, not a settings tab.** Memory, lessons, recipes,
   health, and autonomy live in one first-person destination (the Self
   view), which the user *visits* like a garden — not configures like
   preferences. Settings only links to it.
4. **Quiet biology.** The organism is felt through breathing-slow motion,
   earned growth stages, and plain words — never gauges, health percentages,
   green/red, badges, streaks, or gamification. One motion at a time, and
   `prefers-reduced-motion` always yields a fully static but fully labeled
   equivalent. Poiesis should feel like a plant on the desk, not a
   tamagotchi demanding attention.

---

# Part II — Rename (BRAND) — *do this first, it's cheap*

Brand-level only. **Do NOT rename:** the Rust crate, folder names, the
app-data directory, the DB filename, `nexus-action` fence tags, Tauri event
names, the bundle `identifier` in `tauri.conf.json`. Renaming any of those
breaks existing user data or risks a large refactor with no git safety net.

- `BRAND-1` `src-tauri/tauri.conf.json`: set `productName: "Poiesis Agent"`
  and the main window `title: "Poiesis Agent"`. Leave `identifier` exactly
  as it is. **Frontend-facing name is "Poiesis Agent"; "Poiesis" alone is
  the internal/short form** used in package metadata (`package.json` name,
  `Cargo.toml` description/authors) and in this plan document — never in
  UI copy the user sees.
- `BRAND-2` Frontend display strings: grep `Nexus` under `src/` and replace
  **only user-visible strings** (TopBar brand text, Settings headers,
  onboarding/empty-state copy, toasts, aria-labels) with **"Poiesis Agent"**.
  Do not touch identifiers, CSS class names, comments referencing files, or
  the `nexus-action` fence constant.
- `BRAND-3` Identity in the base system prompt (`composeSystemPrompt` in
  `src/lib/store.ts`): the base prompt's self-reference becomes:
  *"You are Poiesis Agent, a local-first assistant that maintains itself: you
  keep durable memory, learn lessons from your own mistakes, and propose —
  never impose — changes to how you work."* (One sentence of the existing
  prompt is replaced; the rest is untouched.)
- `BRAND-4` Docs sweep: README title/intro → Poiesis Agent, one paragraph
  explaining the name and the self-maintenance concept, plus a placeholder
  section header `## What Poiesis remembers` (filled when Part III 10C
  ships). `IMPLEMENTATION_PLAN.md`/`TASKS.md` get a top note: "Project
  renamed to Poiesis (2026-07); internal identifiers still say nexus by
  design."

**Accept:** app builds and launches titled "Poiesis"; a conversation created
before the rename still loads (proves data paths untouched); `grep -ri
poiesis src-tauri/src` returns nothing (no internal renames).

---

# Part III — The substrate (absorbed Phase 10)

*This part is the former `AGENT_MEMORY_PLAN.md`, adapted: layout gains
`lessons/`, `recipes/`, `.quarantine/`; `soul_proposals` is generalized to
`change_proposals`; `tool_stats` gains `conversation_id`; `memory_fts` gains
a `kind` column; brand strings say Poiesis. Everything else is unchanged and
still authoritative.*

## 0. The memory model — four tiers

| Tier | Lifetime | Where | Status |
|---|---|---|---|
| Working state | one conversation | `conversations.session_state_json` (`remember` tool) + workspace surface | ✅ exists |
| Conversation history | one conversation | `messages` + `messages_fts` | ✅ stored, ❌ not token-budgeted |
| Episodic memory | forever | same FTS index — agent can't search it | ❌ agent-side missing |
| Durable self (facts · lessons · soul · recipes) | forever | **new:** `memory/` markdown files + `MEMORY.md` index | ❌ missing |

Non-negotiable commitments:

1. **The self is plain markdown on disk, owned by the user** — one
   always-injected index + one file per entry (Claude-Code style). Editable
   in Notepad.
2. **No wholesale file rewrites by the model.** Narrow tool verbs
   (`save`/`update`/`forget`/`read`); the backend owns layout and the index.
3. **Every self-write is visible** (timeline step + activity log + undoable
   toast) **and reversible** (trash, snapshots, quarantine).
4. **Compaction changes what is sent to the model, never what is stored or
   shown.** Messages are never deleted or hidden from the user.

### On-disk layout (under the app-data dir, sibling of `models/`)

```
memory/
├─ MEMORY.md          # generated index — never hand-edited, never model-edited
├─ SOUL.md            # standing instructions; user-edited, agent only PROPOSES
├─ facts/
│  └─ prefers-metric-units.md
├─ lessons/           # reflection output (11A) — same file format, kind: lesson
│  └─ verify-paths-before-writing.md
├─ recipes/           # approved procedures (11C) — extended frontmatter
│  └─ weekly-report.md
├─ .trash/            # forgotten entries (recoverable)
├─ .quarantine/       # unparseable files moved aside by HEAL-3 (recoverable)
└─ .snapshots/        # pre-consolidation copies, timestamped dirs
```

Entry file format (frontmatter + body — same for facts and lessons):

```markdown
---
name: prefers-metric-units
description: User wants all measurements in metric
type: preference
created: 2026-07-16
source_conversation: 3f2a…
---
Always give measurements in metric. Confirmed twice.
```

### One DB migration for the whole plan — schema v5

Nothing of Phase 10 is implemented yet, so Parts III and IV share **one**
migration. In `db/mod.rs`: set `SCHEMA_VERSION: i64 = 5` and extend
`migrate()`:

```rust
if current < 5 {
    // v5 (Poiesis): context compaction + reflection marker + proposals + tool stats.
    Self::add_column(&conn, "conversations", "summary", "TEXT")?;
    Self::add_column(&conn, "conversations", "summary_upto_message_id", "TEXT")?;
    Self::add_column(&conn, "conversations", "reflected_at", "INTEGER")?;
}
```

And append to `db/schema.sql` (idempotent `IF NOT EXISTS`, so it also covers
fresh installs):

```sql
-- Poiesis: agent-proposed self-changes awaiting user approval (SOUL-2, RCP-2).
CREATE TABLE IF NOT EXISTS change_proposals (
  id            TEXT PRIMARY KEY,
  target        TEXT NOT NULL,            -- 'soul' | 'persona' | 'recipe' | 'lesson'
  persona_id    TEXT,                     -- when target='persona' (future)
  slug          TEXT,                     -- when target='recipe'|'lesson': the entry name
  proposed_text TEXT NOT NULL,            -- full replacement/new text for the target
  rationale     TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'pending', -- pending|applied|dismissed
  created_at    INTEGER NOT NULL
);

-- Poiesis: per-model tool reliability (GRM-4 / HEAL-2). Content-free.
CREATE TABLE IF NOT EXISTS tool_stats (
  id              TEXT PRIMARY KEY,
  model_name      TEXT NOT NULL,
  tool_name       TEXT NOT NULL,
  conversation_id TEXT,                   -- lets reflection find this chat's failures
  ok              INTEGER NOT NULL,       -- 1 success, 0 error
  created_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tool_stats ON tool_stats(model_name, tool_name);
CREATE INDEX IF NOT EXISTS idx_tool_stats_conv ON tool_stats(conversation_id);

-- Poiesis: FTS over memory entries (RCL-4). Plain fts5 table (not
-- external-content): the memory store rebuilds it wholesale on every write —
-- fine at entry-count scale (tens, not thousands).
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(name, description, body, kind);
```

### New AgentEvent variants (`agent/mod.rs`)

Append to the existing `AgentEvent` enum (same serde attrs, snake_case tags):

```rust
/// A durable self entry was written/updated/forgotten (MEM-6 / REF-3).
/// collection: "facts" | "lessons" | "recipes".
MemoryWrite { op: String, name: String, description: String, collection: String },
/// Recall search results with provenance, for the expandable timeline step.
Recall { id: String, matches: Vec<crate::db::SearchHit> },
/// The agent proposed a self-change (SOUL-2 / RCP-2). target as in change_proposals.
Proposal { id: String, target: String, rationale: String },
```

Frontend: mirror these in the `AgentEvent` union in `src/lib/api.ts` (find the
existing event type used by `streamAssistantTurn` in `store.ts` and extend the
switch there — every existing variant has a case; add the three new ones).

---

## 10A — Context ledger & compaction (CTX) — *homeostasis, do first*

**Problem.** `sendMessage`/`sendBlockAction` in `src/lib/store.ts` build
`priorTurns` from **all** messages with no token accounting. llama.cpp
truncates from the front on overflow, eating the system prompt (surface tree,
session state, guidance).

### CTX-1 — Expose the context budget

1. `src-tauri/src/runtime/process.rs`: `EngineConfig` already has
   `ctx_size: u32` (line ~32). Add `pub ctx_size: u32` to `RunningEngine`,
   set it in `spawn_engine` from the config. Include `ctx_size` in
   `EngineStatus` (see `EngineStatus::from_engine`) so it serializes to the UI.
2. `src-tauri/src/runtime/manager.rs`: add

   ```rust
   /// Context window of the running local engine, if any.
   pub async fn engine_ctx_size(&self) -> Option<u32> {
       self.engine.lock().await.as_ref().map(|e| e.ctx_size)
   }
   ```
3. New command in `src-tauri/src/commands/runtime.rs`:

   ```rust
   #[tauri::command]
   pub async fn get_context_budget_cmd(mgr: State<'_, RuntimeManager>) -> Result<Option<u32>, NexusError>
   ```
   Register it in `lib.rs` alongside the other runtime commands. Wrapper
   `getContextBudget(): Promise<number | null>` in `src/lib/api.ts` (copy the
   pattern of any existing no-arg wrapper).
4. Cloud budgets are a frontend constant in `src/lib/store.ts`:

   ```ts
   const CLOUD_CTX: Record<string, number> = { anthropic: 200_000, openai: 128_000, openrouter: 32_000 };
   const DEFAULT_LOCAL_CTX = 4096; // when the engine hasn't reported yet
   ```

**Accept:** `get_context_budget_cmd` returns the value passed as `--ctx-size`
while a model is loaded, `null` otherwise.

### CTX-2 — Token estimator + turn budgeter (frontend)

New file `src/lib/context.ts`:

```ts
/** Rough token estimate: 1 token ≈ 4 chars. Deliberately conservative. */
export function estimateTokens(text: string): number { return Math.ceil(text.length / 4); }

export interface BudgetedTurns {
  turns: api.ChatTurnMessage[];   // what to send
  usedTokens: number;
  budget: number;
  needsCompaction: boolean;       // history alone exceeds the threshold
}

/**
 * Fit turns into `budget`. Reserve 25% of budget for the response + tool
 * traffic. Never drop: the system turn, the last `KEEP_RECENT = 6` turns, or
 * the current user turn. If older turns must go, mark needsCompaction so the
 * caller can summarize them first.
 */
export function budgetTurns(
  system: string,
  prior: api.ChatTurnMessage[],
  current: api.ChatTurnMessage,
  budget: number,
): BudgetedTurns
```

Implementation rule (keep it this simple): walk `prior` from newest to oldest
accumulating estimates; stop adding once
`estimate(system) + acc + estimate(current) > budget * 0.75`; everything older
is "overflow". `needsCompaction = overflow.length > 0`.

Unit-test with `vitest` if present; otherwise add a `context.test.ts` runnable
by `npx tsc --noEmit` + a plain assert script (check `package.json` for the
test setup first; do not introduce a new test framework).

### CTX-3 — Compaction command (backend summarizes locally)

New command in `src-tauri/src/commands/conversations.rs`:

```rust
/// Summarize all messages up to (and including) `upto_message_id` into
/// `conversations.summary`, merging with any existing summary. Runs on the
/// SAME endpoint the chat uses (local model summarizes locally).
#[tauri::command]
pub async fn compact_conversation_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    conversation_id: String,
    upto_message_id: String,
    target: Option<ChatTarget>,   // same routing struct as agent_chat_cmd
) -> Result<String, NexusError>   // returns the new summary
```

Steps inside:
1. Load messages `WHERE conversation_id=? AND created_at <= (created_at of upto_message_id)`
   (db helper `list_messages_until(&self, conv: &str, upto_id: &str)` — copy
   `list_messages` and add the bound).
2. Build one prompt (no tools, temperature 0.2) via the existing `drive_turn`:

   ```
   system: You compress conversation history. Output ONLY the summary, no preamble.
   user:  Summarize this conversation so a colleague can continue it.
          Use exactly these sections, plain text, max 300 words total:
          FACTS: (stable facts, names, numbers)
          DECISIONS: (settled choices)
          OPEN: (unresolved threads, next steps)
          Existing summary to merge in:
          <existing summary or "none">
          Conversation:
          <turns as "user: …" / "assistant: …" lines, each clipped to 500 chars>
   ```
3. Persist via new db helper
   `set_conversation_summary(&self, id: &str, summary: &str, upto_message_id: &str)`.
4. Return the summary. On model failure, return `Err` — the frontend then
   falls back to hard-dropping oldest turns (never blocks sending).

Add db fields to the `Conversation` struct + `row_to_conversation` mapping
(`summary: Option<String>`, `summary_upto_message_id: Option<String>`), and to
`Conversation`/`DbConversation` in `src/lib/types.ts` + mapping in `store.ts`.

### CTX-4 — Wire budgeting into both send paths

In `store.ts`, `sendMessage` and `sendBlockAction` currently do
`const priorTurns = …; const turns = [system, ...priorTurns, current]`.
Replace with:

```ts
const budget = await resolveBudget(model);           // helper: local → api.getContextBudget(), cloud → CLOUD_CTX
let prior = priorTurnsOf(conv);                       // extract the existing mapping into a helper
let bt = budgetTurns(effectiveSystemPrompt, prior, currentTurn, budget);
if (bt.needsCompaction && api.inTauri()) {
  const boundary = /* id of the newest message NOT in bt.turns */;
  try {
    const summary = await api.compactConversation(convId, boundary, target);
    set(/* store summary + boundary on the conversation object */);
  } catch { /* summary failed: proceed with dropped turns */ }
  bt = budgetTurns(withSummary(effectiveSystemPrompt, summary), recentOnly, currentTurn, budget);
}
```

`withSummary` appends to the system prompt:

```
## Conversation so far (older turns were summarized)
<summary>
```

Prior turns sent = only messages **after** `summary_upto_message_id` when a
summary exists. Both send paths must share this via one helper
(`assembleTurns(...)`) — do not duplicate the logic; `sendBlockAction` and
`sendMessage` already duplicate too much.

**Workspace-aware rule:** when `conv.workspace` is true, pass
`KEEP_RECENT = 3` instead of 6 (the surface + session state carry the task
state), and the summarization prompt gains one line: "The live workspace
surface is authoritative; do not restate its contents."

### CTX-5 / CTX-UI — Surface it

- `CTX-UI-1` **Context meter in the Composer** (`components/Composer/Composer.tsx`):
  compute locally from the assembled conversation each render
  (`estimateTokens` over messages + system prompt approximation is fine — no
  backend call per keystroke; refresh `budget` once per model change). Render:
  a 64px-wide, 3px-tall bar, `--ink-faint` track, `--ink-muted` fill,
  `title`-attr tooltip `~{used} / {budget} tokens{compacted ? " · older turns summarized" : ""}`.
  Render nothing under 50% fill. Add styles to `Composer.css` using existing
  tokens only. `aria-label` mirrors the tooltip.
- `CTX-UI-2` **Compaction divider** in `components/Conversation/`: in the
  message list, before the first message *after* `summary_upto_message_id`,
  render `<div class="compact-divider">· · · earlier turns are summarized for the model · · ·</div>`;
  clicking toggles a `<details>`-style panel showing `conv.summary` verbatim.
  All messages remain rendered above it.
- `CTX-UI-3` Workspace header (`routes/Workspace.tsx`, `ws-head`): reuse the
  same meter component, no tooltip text change.
- `CTX-UI-4` Settings → new "Memory & context" card (`routes/Settings.tsx`):
  read-only line "Model context window: N tokens" + toggle
  "Summarize older turns automatically" persisted as setting key
  `context.autocompact` (default `true`; when false, skip CTX-3 and just drop).

**Exit for 10A:** simulate a 200-turn conversation (script or hand-test with a
tiny `--ctx-size`); the request payload stays under budget, a summary row
appears in the DB, the divider shows, and answers still reference early facts
via the summary.

---

## 10B — Recall skill (RCL): the agent can search its own past

### RCL-1 — DB search surface

In `db/mod.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub source: String,            // "chat" | "memory"
    pub conversation_id: Option<String>,
    pub title: String,             // conversation title, or entry name
    pub created_at: i64,
    pub snippet: String,
}

pub fn search_messages_fts(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, DbError>
```

SQL (join through rowid — `messages_fts` is external-content on `messages`):

```sql
SELECT m.conversation_id, c.title, m.created_at,
       snippet(messages_fts, 0, '', '', '…', 16)
FROM messages_fts
JOIN messages m       ON m.rowid = messages_fts.rowid
JOIN conversations c  ON c.id = m.conversation_id
WHERE messages_fts MATCH ?1
ORDER BY rank
LIMIT ?2
```

**FTS query sanitization** (required — raw user text breaks MATCH syntax):

```rust
fn fts_escape(q: &str) -> String {
    q.split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect::<Vec<_>>().join(" ")
}
```

Also `search_memory_fts(&self, query: &str, limit: usize)` over `memory_fts`
(source = "memory", title = name, conversation_id = None; the `kind` column
is included in the snippet-able columns but search matches all columns).
Add a `#[cfg(test)]` test seeding two conversations + one fact and asserting
both sources are hit (mirror the existing FTS test style in `db/mod.rs`).

### RCL-2 — The skill

New module `src-tauri/src/agent/recall.rs`, new `Skill::Recall` variant wired
through **all six** match arms in `agent/skills.rs` (`id`→`"recall"`,
label `"Recall"`, description
`"Search your past conversations and saved memories. Stays on your device."`,
`sensitive`→false, `default_enabled`→true) plus `Skill::ALL` (now 7).

Tool specs (exact):

```json
[
 {"type":"function","function":{"name":"search_history",
   "description":"Search the user's past conversations and saved memories on this device. Use when the user references something from before ('like we discussed', 'my usual', a past decision) that is not in the current context. Returns matches with source and date.",
   "parameters":{"type":"object","properties":{
     "query":{"type":"string","description":"2-6 keywords, not a sentence"},
     "limit":{"type":"integer","description":"max results, default 5"}},
     "required":["query"]}}},
 {"type":"function","function":{"name":"read_conversation",
   "description":"Read a short window of a past conversation found via search_history.",
   "parameters":{"type":"object","properties":{
     "conversation_id":{"type":"string"}},
     "required":["conversation_id"]}}}
]
```

`execute` for `search_history`:
1. `let hits = [db.search_messages_fts(q, limit)?, db.search_memory_fts(q, limit)?].concat()`,
   truncate to `limit` (default 5, max 8).
2. Emit `ctx.sink` → new `AgentEvent::Recall { id: <the tool call id — pass it
   through SkillContext, see below>, matches: hits.clone() }`.
3. `db.log_activity(Some(conv), "recall", &format!("searched history: {q}"))`.
4. Return to the model a compact text block, hard-capped at 2000 chars:
   `1. [chat · 2026-07-02 · "NAS build"] …snippet…` one line per hit, plus
   `Use read_conversation(conversation_id) for more.`

`read_conversation`: db helper `list_messages_window(&self, conv_id, max: usize)`
returning the **last 12** messages, each clipped to 400 chars; total output
capped 4000 chars. Refuse (Err) if the id doesn't exist.

> **Plumbing note:** the tool-call id isn't in `SkillContext` today. Add
> `pub call_id: &'a str` to `SkillContext`, set it in `dispatch()`
> (`run.rs` — it already has `call.id` at the `dispatch_calls` level; pass
> `name`+`id` down). Only construction sites change; other skills ignore it.

### RCL-UI — provenance in the timeline

- In `src/lib/types.ts`, extend the timeline `Step` type with
  `matches?: SearchHit[]`. In `store.ts`'s event switch, on `recall` attach
  `matches` to the step with matching `id`.
- `components/Conversation/Timeline.tsx`: a step that has `matches` renders an
  expand chevron; expanded, each match is a row
  `{source chip} {title} · {date} — {snippet}`. Chat rows are buttons calling
  `setActiveConversation(conversation_id)`. Memory rows show a `◆ memory`
  chip (styles in `Conversation.css`, ink tokens, no new colors).

**Exit for 10B:** in a fresh chat, "what did we decide about X?" produces a
`recalled · …` timeline step with clickable provenance and a correct answer.

---

## 10C — Durable memory store (MEM) — *the centerpiece*

### MEM-1 — The store module

New `src-tauri/src/memory/mod.rs` (+ `pub mod memory;` in `lib.rs`), managed
as Tauri state (`app.manage(MemoryStore::new(app_data_dir)?)` next to where
`Db`/`RuntimeManager` are managed in `lib.rs`).

```rust
pub struct MemoryStore { dir: PathBuf, lock: std::sync::Mutex<()> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub name: String,          // slug, file stem
    pub description: String,   // one line, shown in the index
    pub kind: String,          // preference|fact|decision|project — and "lesson" for lessons/
    pub created: String,       // YYYY-MM-DD
    pub source_conversation: Option<String>,
    pub body: String,
}

impl MemoryStore {
    pub fn new(app_data: &Path) -> std::io::Result<Self>       // mkdir memory/{,facts,lessons,recipes,.trash,.quarantine,.snapshots}
    pub fn list(&self) -> Vec<Fact>                             // parse every facts/*.md; skip unparseable (HEAL-3 quarantines them)
    pub fn read(&self, name: &str) -> Option<Fact>
    pub fn save(&self, f: &Fact) -> Result<(), String>          // Err if slug exists → "a fact named X exists; use op:update"
    pub fn update(&self, name: &str, description: Option<&str>, body: &str) -> Result<(), String>
    pub fn forget(&self, name: &str) -> Result<(), String>      // move to .trash/<ts>-<name>.md
    pub fn restore_trash(&self, file: &str) -> Result<(), String>
    pub fn index_markdown(&self) -> String                      // regenerate; see cap rule below
    pub fn write_index(&self)                                   // index_markdown → MEMORY.md
    pub fn soul(&self) -> String                                // SOUL.md contents or ""
    pub fn set_soul(&self, text: &str) -> Result<(), String>
    pub fn snapshot(&self) -> Result<String, String>            // copy facts/ lessons/ recipes/ + MEMORY.md + SOUL.md → .snapshots/<ts>/
    pub fn sync_fts(&self, db: &Db)                             // DELETE FROM memory_fts; re-INSERT all entries (facts+lessons+recipes)
}
```

Rules a junior implementer must not improvise:
- **Slug:** lowercase; every non-`[a-z0-9]` run → `-`; trim `-`; max 64 chars;
  empty after slugging → Err.
- **Frontmatter parse:** split on the first two `---` lines; parse `key: value`
  pairs line-by-line (no YAML crate needed); unknown keys ignored; missing
  `description` → first body line.
- **Index format**, one line per fact, sorted newest first:
  `- [prefers-metric-units] (preference) User wants all measurements in metric`
- **Index caps (per section):** facts **2000 chars**, lessons **1000 chars**,
  recipes **800 chars** (sections specced in REF-4 / RCP-3). If a section is
  over, keep newest lines that fit and append
  `- …and N older entries (search_history finds them)`.
- **Body cap: 1500 chars per fact** — reject longer saves with
  `"keep facts short; put long content in a file or artifact"`.
- Every mutating method: take `self.lock`, do the file op, `write_index()`,
  then `sync_fts(db)` (pass `&Db` into mutating methods).

Unit tests in the module: slug rule, save/update/forget round-trip,
index cap, frontmatter tolerance (extra keys, CRLF).

> **Extension point for Part IV:** 11A/11C generalize the internals with a
> private `fn dir_for(collection: &str) -> PathBuf` and
> `fn list_in(&self, collection: &str) -> Vec<Fact>` so lessons and recipes
> reuse the same parser, slugger, trash, and lock. MEM-1 lands facts-only;
> write the private helpers collection-parameterized from the start.

### MEM-2 — The Memory skill

New `Skill::Memory` variant (module `agent/memory_skill.rs` to avoid clashing
with the store) — id `"memory"`, label `"Memory"`, description
`"Remember durable facts about you across conversations — stored as markdown files on this device."`,
benign, **default on**. `SkillContext` gains `pub memory: &'a MemoryStore`
(construct in `run.rs::dispatch`; `run_agent` gains a `memory: &MemoryStore`
param passed from `agent_chat_cmd`, which takes `State<'_, MemoryStore>`).

One tool, flat params (exact spec):

```json
{"type":"function","function":{"name":"memory",
 "description":"Manage durable memory about the user that persists across ALL conversations. op:save stores a NEW fact (name, description, type, text required). op:update rewrites one fact's text. op:forget deletes a fact. op:read returns a fact's full text — do this before relying on details of an indexed fact. Save ONLY durable, user-relevant facts (preferences, standing decisions, stable personal/project facts). NEVER save task state, opinions, or anything the user hasn't confirmed. When in doubt, don't save.",
 "parameters":{"type":"object","properties":{
   "op":{"type":"string","enum":["save","update","forget","read"]},
   "name":{"type":"string","description":"short-kebab-case-slug"},
   "description":{"type":"string","description":"one line for the index"},
   "type":{"type":"string","enum":["preference","fact","decision","project"]},
   "text":{"type":"string","description":"the fact body, under 1500 chars"}},
  "required":["op"]}}}
```

`execute` dispatches per op; each mutating op:
- calls the store, then `ctx.sink` emits
  `MemoryWrite { op, name, description, collection: "facts" }`,
- `db.log_activity(Some(conv), "memory", "<op> <name>")`,
- returns a one-line receipt: save →
  `Saved memory "<name>". It is now in your index in every conversation.`;
  forget → `Forgot "<name>".`; read → the body; update → `Updated "<name>".`

`describe()`: `("remembered"|"updated memory"|"forgot"|"recalled memory", name)`.

### MEM-3 — Injection into every turn

Backend command + wrapper:

```rust
#[tauri::command]
pub fn get_memory_context_cmd(mem: State<'_, MemoryStore>) -> MemoryContext
// MemoryContext { index: String, soul: String, fact_count: usize }
// index = full MEMORY.md content (facts section now; lessons/recipes sections
// appear automatically once REF-4 / RCP-3 extend index_markdown).
```

Frontend (`store.ts`):
- Slice: `memoryContext: { index: string; soul: string; factCount: number }`,
  loaded at app init (where personas/settings load) and re-fetched whenever a
  `memory_write` event arrives during a stream.
- `composeSystemPrompt` gains `memory: MemoryContext` in its `opts` and
  prepends/appends:
  - `soul` (if non-empty) directly **after the base prompt**:
    `\n\n## Standing instructions (SOUL.md — the user approved these)\n{soul}`
  - `index` (if non-empty) after that:
    `\n\n## Your memory index (durable facts about the user)\n{index}\n(Read a fact's full text with memory(op:"read", name:…) before relying on its details.{toolsEnabled ? "" : " Tools are off — treat descriptions as the only available detail."})`
- Caps already enforced by the store (index sections 2000/1000/800; soul cap:
  reject `set_soul` over 1500 chars).

### MEM-5 — Consolidation ("tidy up"), manual first

No idle scheduler in v1 — a button. Backend command:

```rust
#[tauri::command]
pub async fn consolidate_memory_cmd(mgr, db, mem, target: Option<ChatTarget>) -> Result<Proposal, NexusError>
```

1. Concatenate all facts **and lessons** (name + type + body).
2. One `drive_turn` call, temperature 0.2, prompt: *"You maintain a personal
   memory file set. Propose a cleanup as JSON only:
   `{"deletes":["name"...],"edits":[{"name":"...","text":"..."}],"merges":[{"keep":"name","drop":["name"...],"text":"merged body"}]}`.
   Merge duplicates, drop facts superseded by newer ones, tighten wording.
   Propose nothing you are unsure about. Facts: …"*
3. Parse strictly (`serde_json::from_str::<Proposal>`); on parse failure
   return an empty proposal (never guess).
4. **Do not apply.** Store as settings key `memory.pending_consolidation`
   (JSON) and return it.

Apply command `apply_consolidation_cmd(accept: bool)`: if accept —
`mem.snapshot()` first, then execute deletes/edits/merges via store methods,
clear the setting; if dismiss — just clear. Every applied change →
activity log.

### MEM-UI — the Memory panel and chat affordances

*(In 11E the Memory panel becomes one tab of the umbrella "Poiesis Agent — self"
Settings section; build it standalone first exactly as below, ORG-UI-1 then
re-parents it. Nothing here is throwaway.)*

- `MEM-UI-1` New `src/components/Memory/MemoryPanel.tsx` + `Memory.css`,
  rendered as a new section in `routes/Settings.tsx` (follow exactly how the
  Personas section is embedded). Contents:
  - Header: `Memory · {factCount} facts` + buttons `Tidy up`,
    `Open folder`, `Export zip`.
  - Search input filtering the list client-side (facts are already few; call
    `list_memory_facts_cmd` once — add that command returning `Vec<Fact>`).
  - Fact cards: name, type chip, description, created date, source-chat link
    (opens conversation), `Edit` (inline `<textarea>` on the body →
    `update_memory_fact_cmd`), `Delete` (→ `forget_memory_fact_cmd`, with a
    5s undo strip using `restore_trash`).
  - Pending consolidation (if `memory.pending_consolidation` setting exists):
    render each delete/edit/merge as a row with old→new text, `Apply all` /
    `Dismiss` buttons → `apply_consolidation_cmd`.
- `MEM-UI-2` `Open folder`: command `open_memory_dir_cmd` using the existing
  pattern for opening paths (search the codebase for how generated images or
  model folders are revealed; if none exists, use `tauri-plugin-opener` — it
  is the Tauri v2 standard).
- `MEM-UI-3` **Live toast**: on `memory_write` events, `Chat.tsx` renders a
  fixed bottom-center quiet toast `◆ Remembered: {description} — Undo`
  (Undo → `forget_memory_fact_cmd`). Auto-dismiss 6s. One at a time; ink
  tokens; `role="status"`. New tiny component
  `components/Memory/MemoryToast.tsx`. When the event's `collection` is
  `"lessons"` the toast text is `◆ Lesson learned: {description} — Undo`.
- `MEM-UI-4` First-write onboarding: if setting `memory.onboarded` unset when
  the first toast fires, the toast gains a second line: *"Poiesis keeps a few
  markdown notes about your preferences on this device. Review or turn this
  off in Settings → Memory."* Then set the flag.
- `MEM-UI-6` The skill toggle already appears automatically in Settings →
  Skills via the skills framework. Verify disabling hides both injection
  (`composeSystemPrompt` must check the skill's enabled state — expose it via
  the existing `list_skills_cmd` data already in the store) and the tool.
- `MEM-UI-7` `components/Blocks/SessionStrip.tsx`: no functional change;
  add a `◆` marker on a session-state entry whose key matches a fact name
  (cheap `memoryContext.index.includes(name)` check) with tooltip
  "also saved to durable memory".

**Exit for 10C:** tell Poiesis "I always want metric units" in chat A (it
saves, toast appears); new chat B uses metric unprompted and `read`s the fact
when details matter; the fact is visible/editable/deletable in Settings →
Memory and in Explorer; `Tidy up` round-trips a proposal.

---

## 10D — Soul: evolvable standing instructions (SOUL)

### Backend
- `SOUL-1` `SOUL.md` handled by `MemoryStore` (already specced). Injection
  already specced in MEM-3.
- `SOUL-2` Second tool on the Memory skill:

  ```json
  {"type":"function","function":{"name":"propose_soul_edit",
   "description":"Propose adding/changing a STANDING instruction (how the assistant should always behave) after the user has confirmed a lasting preference more than once. The user must approve; do not assume it is active.",
   "parameters":{"type":"object","properties":{
     "proposed_text":{"type":"string","description":"the COMPLETE new SOUL.md text (existing text with your change applied)"},
     "rationale":{"type":"string","description":"one sentence: why"}},
    "required":["proposed_text","rationale"]}}}
  ```

  Execute: insert into `change_proposals` (db helpers
  `add_change_proposal`/`list_change_proposals`/`resolve_change_proposal(id, status)`),
  emit `Proposal { id, target: "soul", rationale }`, log activity, return
  `Proposed. The user will review it; continue without assuming it's active.`
- `SOUL-3` Commands: `list_change_proposals_cmd`,
  `resolve_change_proposal_cmd(id, accept: bool)` — for target `soul`,
  accept ⇒ `mem.set_soul(proposed_text)` + status `applied`; else
  `dismissed`. For target `recipe` see RCP-2. (Persona-prompt proposals: out
  of scope for v1 — the table's `target`/`persona_id` columns future-proof
  it. Note this in code.)

### UI
- `SOUL-UI-1` `components/Personas/PersonaEditor.tsx` gains a **"Soul"
  sub-section** above the persona list: a textarea editing `SOUL.md`
  (via `get_memory_context_cmd` / `set_soul_cmd`), plus pending proposals as
  cards — rationale, a simple old/new text diff (two `<pre>` blocks is
  enough; no diff lib), `Accept` / `Dismiss`.
- `SOUL-UI-2` In-chat: on a `proposal` event, render a quiet inline card
  under the assistant turn (visual language of `PermissionPanel`, not a
  modal). Text by target — soul:
  `Poiesis suggests a standing instruction — {rationale} · Review · Dismiss`;
  recipe: `Poiesis wants to keep a procedure: {slug} — {rationale} · Review · Dismiss`.
  `Review` deep-links to the relevant Settings section (soul → Personas,
  recipe → Poiesis-self panel; set the active route + scroll); `Dismiss`
  resolves it.
- `SOUL-UI-3` Settings rail icon shows a badge dot when
  `list_change_proposals` (pending) or `memory.pending_consolidation` is
  non-empty — follow the pattern used for pending permission indication if
  one exists; otherwise a 6px `--ink` dot on the icon.

**Exit:** correct the agent twice about a standing preference → proposal card
appears → accept in Settings → next conversation honors it; revert = edit the
Soul textarea.

---

## 10E — Grammar-constrained decoding (GRM) — *independent*

Do **GRM-3 (validate + retry) first** — it works on every endpoint today.
Native enforcement is a build-dependent upgrade on top.

- `GRM-3` In `agent/present.rs`, `render_ui`/`present` already validate
  shapes. Extend the *loop* so one failed tool call gets one guided retry:
  in `run.rs::dispatch_calls`, when a **built-in** call returns `Err(e)`,
  push the tool-error result (already done) **and** a system-role message
  `Fix the previous tool call: <e>. Reply with ONLY the corrected tool call.`
  — but only once per call id (track a `HashSet<String>` of retried ids in
  `run_agent`). This costs one iteration of the existing loop; no new
  plumbing.
- `GRM-1/2` Native path: llama.cpp `--jinja` (already passed, per TASKS
  Phase 9 notes) enables lazy-grammar tool-call enforcement on current
  builds. Verify empirically: with tools on, `curl` the running engine's
  `/v1/chat/completions` with a `tools` array and check the response returns
  structured `tool_calls` (not content JSON). If the pinned build is too old,
  bump the pinned build tag in `runtime/manifest.rs` (test suite covers asset
  names). Document the finding in the Engine card (GRM-UI-1).
- `GRM-4` In `run.rs::dispatch_calls`, after each call record
  `db.add_tool_stat(model_name, tool_name, conversation_id, ok)` (new db
  helper; model name needs plumbing — pass `model_name: &str` into
  `run_agent` from `agent_chat_cmd`, which already knows the target; the
  conversation id is already in scope). This table feeds both LOOP-UI-1 and
  HEAL-2/REF-2 — one implementation.
- `GRM-UI-1` `routes/Engine.tsx`: on the runtime status card add one line —
  `Structured tool output: {enforced ✓ | validate + retry}` — value from a
  new field on `EngineStatus` set by the GRM-1 probe (or hardcode
  `validate + retry` until the probe exists; never show nothing).

**Exit:** with a 3B model, 20 consecutive `render_ui` calls produce zero
user-visible raw-JSON leaks; `tool_stats` rows accumulate.

---

## 10F — Loop hygiene (LOOP)

- `LOOP-1` **MCP session reuse per run.** In `run.rs::run_agent`, create
  `let mcp_pool: tokio::sync::Mutex<HashMap<String, McpClient>> = Default::default();`
  and pass `&mcp_pool` down to `call_mcp_tool`; look up by
  `binding.connector_id`, `initialize()` only on first use. The pool drops at
  run end (stdio child processes die with it — verify `kill_on_drop` in
  `mcp/client.rs`).
- `LOOP-2` **`fetch_url` tool** on the Web Search skill
  (`agent/websearch.rs`): spec
  `{"name":"fetch_url","parameters":{…,"url":{"type":"string"}},"required":["url"]}`,
  description "Fetch and read one web page the user referenced or a search
  result. The URL leaves this device." Execute = existing
  `fetch_readable(ctx.client, url)`, output capped 8000 chars, activity-log
  `"fetched <url>"`. `handles()` gains the name; same skill toggle covers it.
- `LOOP-3` **Plan-first nudge:** append one line to the tools-mode guidance
  (`SURFACE_GUIDANCE` neighborhood in `store.ts`): *"For multi-step tasks,
  state a one-line plan before your first tool call."*
- `LOOP-4` **Early-flush streaming in tools mode** (`run.rs`): in the
  `drive_turn` token closure, once `turn_buf` reaches 160 chars (or on
  first token if it starts with a letter and isn't `{`/`[`/fence/`<think>`),
  and no known tool name appears in the buffer, set `streaming_live = true`,
  flush `turn_buf` via `sink.token`, and stream the rest live. On
  `TurnOutcome::Final`, skip re-emitting if already flushed; the
  text-fallback parser only runs when nothing was flushed. Keep the check
  dumb and biased toward buffering (false-buffer = old behavior; false-flush
  = raw JSON leak, which is worse).
- `LOOP-5` = GRM-4 (one implementation, listed once there).
- `LOOP-UI-1` Settings → Skills: under each toggle, if `tool_stats` has rows
  for that skill's tools in the last 7 days, render
  `"{ok}% ok over {n} calls this week"` (new `get_tool_stats_cmd` grouping
  by tool). Muted caption style; absent when no data.
- `LOOP-UI-2` Timeline: when an MCP call errors after pool reuse, the retry
  path (GRM-3's system nudge does not apply to MCP) — instead show the step
  error verbatim; add `("reconnected", …)` verb only if LOOP-1 implements a
  single reconnect-on-send-failure (optional; skip if time-boxed).

---

# Part IV — The autopoietic layer (Phase 11)

Everything below builds strictly on Part III plumbing: `MemoryStore`
(collection helpers), `change_proposals`, `tool_stats(conversation_id)`,
`AgentEvent::{MemoryWrite, Proposal}`, `drive_turn`. No new architectural
machinery — the organism is grown from the substrate.

## 11A — Reflection & lessons (REF) — *learning from mistakes*

A lesson is a fact the agent writes **about itself**: a generalizable,
actionable observation extracted from a finished conversation ("verify file
paths exist before writing", "this user prefers answers before explanations
in code reviews"). Lessons live in `memory/lessons/`, same file format,
`type: lesson`.

### REF-1 — Store support

Extend `MemoryStore` using the collection helpers (see MEM-1 extension
point):

```rust
pub fn list_lessons(&self) -> Vec<Fact>                       // lessons/*.md
pub fn save_lesson(&self, f: &Fact) -> Result<(), String>     // same slug/dup/cap rules; body cap 600 chars
pub fn forget_lesson(&self, name: &str) -> Result<(), String> // → .trash/
```

**Pruning rule:** if `list_lessons().len() > 40` after a save, move the
oldest lessons beyond 40 to `.trash/` (they remain searchable history via
`messages_fts`, and recoverable). Log each pruning to the activity log.

### REF-2 — The reflection command

New `src-tauri/src/commands/reflect.rs`:

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct LessonDraft { pub name: String, pub description: String, pub body: String, pub confidence: String } // "high" | "low"

/// Run one self-reflection pass over a finished conversation. Idempotent:
/// sets conversations.reflected_at first thing so a failed run never loops.
#[tauri::command]
pub async fn reflect_conversation_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    app: AppHandle,                       // to emit events outside a chat stream
    conversation_id: String,
    target: Option<ChatTarget>,
) -> Result<Vec<LessonDraft>, NexusError> // returns what was SAVED (not all drafts)
```

Steps (exact):
1. `db.set_conversation_reflected(&conversation_id, now)` (new helper;
   `UPDATE conversations SET reflected_at=? WHERE id=?`). Do this **first**.
2. Gather material: last ≤30 messages (`list_messages_window`, each clipped
   400 chars) + this conversation's failure stats:
   `SELECT tool_name, COUNT(*) FROM tool_stats WHERE conversation_id=? AND ok=0 GROUP BY tool_name`.
3. One `drive_turn`, no tools, temperature 0.2:

   ```
   system: You are the self-reflection process of a local AI assistant.
           Output ONLY JSON, no preamble.
   user:   Below is a finished conversation and the assistant's tool-failure
           counts. Extract AT MOST 3 lessons about how the assistant should
           work better next time.
           A lesson must be: (a) generalizable beyond this one conversation,
           (b) actionable as a behavior change, (c) grounded in an observed
           mistake, failure, or user correction. Style rules the assistant
           was merely following are NOT lessons. If nothing qualifies,
           return {"lessons":[]}.
           JSON schema:
           {"lessons":[{"name":"kebab-case-slug","description":"one line",
             "body":"2-4 sentences, imperative voice","confidence":"high|low"}]}
           Tool failures: <stats or "none">
           Conversation:
           <turns>
   ```
4. Parse strictly (`serde_json::from_str`); on failure return `Ok(vec![])`
   (never guess, never error the caller).
5. For each draft with `confidence == "high"`, gated by
   `autonomy_gate(&db, "lessons")` (AUT-1):
   - rung **auto**: dedupe (existing lesson slug → skip), `save_lesson`
     (source_conversation = this id), emit app event `poiesis-memory-write`
     with the same payload shape as `AgentEvent::MemoryWrite`
     (`collection: "lessons"`), `db.log_activity(…, "reflect", "learned <name>")`.
   - rung **ask**: insert a `change_proposals` row (`target: "lesson"`,
     `slug`, `proposed_text` = body, rationale = description).
   - rung **off**: skip.
   `confidence == "low"` drafts are discarded (v1: no proposal spam).
6. Return the saved drafts.

### REF-3 — Triggers

Frontend (`store.ts`):
- **On leaving a conversation** (`setActiveConversation` switching away, and
  app-close via the existing beforeunload/teardown hook if one exists —
  switching away is sufficient for v1): fire-and-forget
  `api.reflectConversation(oldId, target)` **iff** all of:
  `settings["reflection.auto"] !== false` · the conversation has ≥ 8
  messages · `conv.reflected_at == null` · `api.inTauri()`. Never await it in
  the navigation path; errors are swallowed to console.
- **Manual:** "Reflect now" appears in two places — the conversation's
  overflow menu in the Rail (next to rename/delete), and the Organism panel
  (ORG-UI-1). Manual runs ignore `reflected_at` (re-reflection allowed) but
  still set it.
- Listen for the `poiesis-memory-write` app event globally (register where
  other app-level listeners live): re-fetch `memoryContext` and show the
  MEM-UI-3 toast (`◆ Lesson learned: …`). This works even though no chat
  stream is open.

### REF-4 — Lessons in the prompt

`MemoryStore::index_markdown` gains a second section (cap 1000 chars, same
overflow line as facts):

```
## Lessons (things you learned from your own mistakes)
- [verify-paths-before-writing] Check a directory exists before writing into it
```

It rides into the system prompt via MEM-3's existing `index` injection — no
new frontend plumbing. The MEM-3 parenthetical gains: *"Lessons are your own
past mistakes — actively apply them."*

### REF-UI

- `REF-UI-1` Lessons tab in the Organism panel (ORG-UI-1): list of lesson
  cards — description, created date, source-chat link, body preview
  (expand), `Delete` with 5s undo (reuse the MemoryPanel card component;
  extract it to `components/Memory/EntryCard.tsx` when building this).
- `REF-UI-2` Rail overflow menu item "Reflect now" per conversation; while
  running, the item shows a spinner glyph; on completion with 0 lessons, a
  one-shot toast "Nothing new to learn from this one."

**Exit for 11A:** have a conversation where a tool fails twice and you
correct the agent once; switch conversations; within seconds a `◆ Lesson
learned` toast appears; the lesson is in the Organism panel and in
`memory/lessons/`; a later conversation demonstrably applies it (e.g. the
agent double-checks the thing it used to get wrong).

## 11B — Self-repair (HEAL)

### HEAL-1 — Engine watchdog (the app heals its runtime)

`src-tauri/src/runtime/manager.rs`:
- When an engine is spawned, also spawn one `tokio::task` (store its
  `JoinHandle` on `RunningEngine`; abort it on user-initiated stop):
  every 30 s, GET the engine's `/health` (the endpoint already used by the
  spawn health-gate). Maintain `consecutive_failures: u8`.
- On **3 consecutive failures** or on observing child-process exit that the
  user did not request: kill the remnant, then respawn with the *same*
  `EngineConfig`, with backoff 2 s → 10 s → 30 s. **Max 3 restarts per
  rolling hour** (keep timestamps in a `VecDeque<Instant>` on the manager);
  after the 3rd, give up, set status `Stopped { reason: "self-heal limit" }`.
- Count `restarts_session: u32` on the manager; expose in `EngineStatus`.
- Each restart: `db.log_activity(None, "heal", "engine restarted (self-heal)")`
  and emit a Tauri app event `poiesis-healed` with payload
  `{ attempt: n, ok: bool }`.

Frontend: global listener on `poiesis-healed` → quiet toast
`↻ Engine restarted itself` (reuse MemoryToast component with a different
glyph); `routes/Engine.tsx` status card gains the line
`Self-healed {restarts_session}× this session` when > 0.

**Accept:** `taskkill` the llama-server process manually → within ~30 s a
toast appears and generation works again without touching the UI; the
activity log shows the heal.

### HEAL-2 — Tool degradation cautions (the agent routes around damage)

- Db helper:
  `pub fn tool_health(&self, model: &str, days: u32) -> Result<Vec<ToolHealth>, DbError>`
  where `ToolHealth { tool_name: String, ok: u32, total: u32 }`, grouped from
  `tool_stats` for the last `days` days.
- Command `get_tool_health_cmd(model_name: String) -> Vec<ToolHealth>`
  (7-day window). Frontend fetches it once per model change (same place the
  context budget is refreshed, CTX-1) into a `toolHealth` slice.
- `composeSystemPrompt`: for each tool with `total >= 8 && ok/total < 0.4`
  (max 2 tools, worst first), append one line:
  `Note: your "{tool}" tool has failed often recently — double-check its arguments, and prefer an alternative when one exists.`
- This is rung-R1 informational self-repair: no setting, always on (it
  changes only the prompt, nothing stored).

**Accept:** seed `tool_stats` with 10 failing rows for a tool → the assembled
system prompt (visible in whatever debug path exists; else assert in a unit
test of `composeSystemPrompt`) contains the caution line.

### HEAL-3 — Memory integrity quarantine

In `MemoryStore::list`/`list_in`: a file that fails frontmatter parsing is
**moved to `.quarantine/<ts>-<filename>`** (not deleted, not silently
skipped), `db.log_activity(None, "heal", "quarantined <file>")`. `Vitality`
(ORG-1) reports the quarantine count; the Organism panel's Health tab lists
quarantined files with `Restore` (move back as-is — the user presumably fixed
it in an editor) and `Delete`. Never let one bad file break `list()` — this
is why parse errors were "skip" in MEM-1; quarantine replaces skip.

### HEAL-4 — Context homeostasis

Already fully specced as 10A (CTX). Listed here only so the healing story is
complete: compaction is the homeostatic loop for the context window. No
additional tasks.

## 11C — Recipes (RCP) — *self-produced procedures, grown from workspaces*

A recipe is a reusable procedure the agent authored: markdown steps plus an
optional **workspace surface template**. Creation is **ask-first** (identity
rung): the agent proposes, the user approves.

### RCP-1 — Store support

Recipe file (`memory/recipes/weekly-report.md`) — extended frontmatter:

```markdown
---
name: weekly-report
description: Compile the weekly status report from notes
trigger: user asks for a weekly report or status summary
created: 2026-07-20
used: 3
last_used: 2026-08-02
---
1. Ask which week if not stated.
2. search_history for decisions in that range.
3. Render the report surface and fill sections from the hits.

```surface
{"kind":"stack","children":[…render_ui tree…]}
```
```

`MemoryStore` additions:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub name: String, pub description: String, pub trigger: String,
    pub created: String, pub used: u32, pub last_used: Option<String>,
    pub steps: String,                 // body without the surface fence
    pub surface_json: Option<String>,  // contents of the ```surface fence, if any
}

pub fn list_recipes(&self) -> Vec<Recipe>
pub fn save_recipe(&self, r: &Recipe) -> Result<(), String>   // slug rules; steps cap 2000 chars; surface_json must serde_json::from_str-parse if present
pub fn read_recipe(&self, name: &str) -> Option<Recipe>
pub fn touch_recipe(&self, name: &str) -> Result<(), String>  // used += 1, last_used = today — silent, rung R0
pub fn forget_recipe(&self, name: &str) -> Result<(), String> // → .trash/
```

Parser rule: the ` ```surface ` fence is extracted with a plain string scan
(find "\n```surface\n" … "\n```"), not a markdown parser.

### RCP-2 — The Recipes skill

New `Skill::Recipes` variant (module `agent/recipes.rs`) — id `"recipes"`,
label `"Recipes"`, description
`"Let Poiesis keep and reuse step-by-step procedures it developed with you — stored as markdown on this device."`,
benign, **default on**. Two tools (exact specs):

```json
[
 {"type":"function","function":{"name":"propose_recipe",
   "description":"Propose saving a reusable PROCEDURE after completing a multi-step task the user is likely to repeat. The user must approve it; continue without assuming it exists. Include a surface template only if a workspace surface was central to the task.",
   "parameters":{"type":"object","properties":{
     "name":{"type":"string","description":"short-kebab-case-slug"},
     "description":{"type":"string","description":"one line"},
     "trigger":{"type":"string","description":"one line: when to use this recipe"},
     "steps":{"type":"string","description":"numbered steps, imperative, under 2000 chars"},
     "surface_json":{"type":"string","description":"OPTIONAL: the render_ui tree JSON to start the workspace from"}},
    "required":["name","description","trigger","steps"]}}},
 {"type":"function","function":{"name":"use_recipe",
   "description":"Read a saved recipe's full steps before executing a task it covers. Increments its usage count.",
   "parameters":{"type":"object","properties":{
     "name":{"type":"string"}},"required":["name"]}}}
]
```

`propose_recipe` execute: validate slug + caps + `surface_json` parses (Err
→ GRM-3 retry handles it); serialize the full future file content (recipe
markdown, RCP-1 format) into `change_proposals.proposed_text`
(`target: "recipe"`, `slug: name`); emit `Proposal { id, target: "recipe", rationale: description }`;
log activity; return
`Proposed recipe "<name>". The user will review it; continue normally.`

`use_recipe` execute: `read_recipe` (Err if missing: list available names in
the error), `touch_recipe`, return trigger + steps (surface_json excluded —
the model gets it only when the user starts a workspace from the recipe).

`resolve_change_proposal_cmd` (SOUL-3) gains the `recipe` arm: accept ⇒
parse `proposed_text` back into a `Recipe` (reuse the file parser) ⇒
`mem.save_recipe`.

### RCP-3 — Recipes in the prompt

`index_markdown` gains a third section (cap 800 chars):

```
## Recipes (procedures you may reuse — read with use_recipe first)
- [weekly-report] (used 3×) when: user asks for a weekly report
```

### RCP-UI

- `RCP-UI-1` Recipes tab in the Organism panel (ORG-UI-1): recipe cards —
  name, trigger, `used N×`, steps preview (expand), `▦` chip when a surface
  template exists, `Delete` + undo. Pending recipe proposals render at the
  top of this tab with steps + rationale + `Accept`/`Dismiss`
  (`resolve_change_proposal_cmd`).
- `RCP-UI-2` **Start from recipe** — the workspace entry point. The
  Composer's existing drop-up menu (the one holding Workspace/Tools/
  Create-image toggles) gains an item `Start from recipe…` visible when ≥1
  recipe exists; it opens a small list (name + trigger). Selecting one:
  1. `newConversation` with workspace flag on;
  2. if the recipe has `surface_json`: new command
     `set_surface_cmd(conversation_id, tree_json)` — writes the reserved
     surface row exactly the way `present.rs` writes it for `render_ui`
     (mirror that code path), so the template renders immediately;
  3. call the existing send path with a prefilled first user turn:
     `Follow your recipe "<name>". Steps:\n<steps>` — the recipe *is* the
     kickoff prompt; the agent takes it from there.
- `RCP-UI-3` `ws-head` (Workspace.tsx): when a conversation was started from
  a recipe (frontend keeps `recipeName` on the conversation object in-memory
  only — no schema change), show `▦ {title} · from recipe {name}`.

**Exit for 11C:** complete a multi-step workspace task; the agent proposes a
recipe (card in chat); accept it in the Organism panel; Composer drop-up now
offers "Start from recipe…"; starting it opens a new workspace conversation
with the template surface already rendered and the agent executing the steps;
the recipe's `used` count increments.

## 11D — The autonomy ladder (AUT) — *the membrane*

### AUT-1 — The gate

One backend helper, used by every self-change site:

```rust
/// Rung for a self-change class. Settings key: "autonomy.<class>".
/// Values: "auto" | "ask" | "off". Missing key → the DEFAULTS entry.
pub fn autonomy_gate(db: &Db, class: &str) -> Rung   // enum Rung { Auto, Ask, Off }

pub const AUTONOMY_DEFAULTS: &[(&str, &str)] = &[
    ("facts",       "auto"), // memory tool save/update/forget (undoable)
    ("lessons",     "auto"), // reflection saves high-confidence lessons (undoable)
    ("consolidate", "ask"),  // tidy-up apply (already ask-only via MEM-5 flow)
    ("soul",        "ask"),  // standing instructions (identity)
    ("recipes",     "ask"),  // new procedures (identity)
];
```

Wiring (all sites already specced; this task just makes them consult the
gate): MEM-2 ops check `facts` (rung `ask` for facts converts a save into a
`change_proposals` row target `lesson`-style — v1: implement `off` fully,
`ask` may fall back to `off` with a code comment, since facts-as-proposals
has no UI; document in Settings copy that facts support auto/off).
REF-2 step 5 checks `lessons`. SOUL-2 and RCP-2 are *inherently* `ask` —
their gate only distinguishes `off` (tool absent from the spec list when
`off`). MEM-5 apply is inherently `ask`.

### AUT-UI-1 — The Autonomy card

In the Organism panel (ORG-UI-1), tab "Autonomy": five rows, one per class —
label + plain-language description + a 2–3 option segmented control
(`Auto with undo` / `Ask first` / `Off`, hiding options a class doesn't
support per AUT-1). Persisted via the existing settings commands
(`autonomy.<class>`). Copy under the header: *"Poiesis maintains itself.
You decide how much it may change without asking."*

**Exit:** set `lessons` to `Off` → reflection runs but saves nothing; set
`soul` to `Off` → `propose_soul_edit` disappears from the tool list next
turn.

## 11E — The organism made visible (ORG)

### ORG-1 — Vitality

```rust
#[derive(Debug, Serialize)]
pub struct Vitality {
    pub facts: usize, pub lessons: usize, pub recipes: usize,
    pub recipe_uses: u32,              // sum of used
    pub quarantined: usize,
    pub engine_restarts_session: u32,  // from RuntimeManager (HEAL-1)
    pub pending_proposals: usize,      // change_proposals WHERE status='pending'
    pub last_reflection: Option<i64>,  // MAX(reflected_at)
    pub tool_health: Vec<ToolHealth>,  // 7-day, current model (HEAL-2 helper)
}

#[tauri::command]
pub async fn get_vitality_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>,
    mgr: State<'_, RuntimeManager>, model_name: Option<String>) -> Result<Vitality, NexusError>
```

### ORG-UI-1 — The Organism panel (the Self view)

Build `src/components/Self/SelfPanel.tsx` + `Self.css` after MEM-UI-1
exists (this task re-parents the MemoryPanel). **Placement: its own route
and Rail destination per PRES-3** — not a Settings section; Settings only
links to it.

- **Header:** the PRES-3 first-person narrative + GrowthRings (PRES-4). A
  small vitality strip below it from `get_vitality_cmd`:
  `{facts} facts · {lessons} lessons · {recipes} recipes · {pending} pending` —
  plain text, ink tokens, counts only (no gauges, no green/red).
- **Tabs** (simple button row, same pattern as any existing tabbed UI in
  Settings; if none exists, stacked `<details>` sections are acceptable):
  1. **Memory** — the MemoryPanel from MEM-UI-1, unchanged.
  2. **Lessons** — REF-UI-1.
  3. **Recipes** — RCP-UI-1.
  4. **Health** — HEAL lines: engine self-heals this session, tool-health
     table (`tool · ok/total this week`), quarantined files with
     Restore/Delete, last reflection date, `Reflect now` button for the
     most recent conversation.
  5. **Autonomy** — AUT-UI-1.
- The SOUL-UI-3 badge-dot logic extends to count pending recipe proposals
  (it already reads `list_change_proposals`; nothing extra to build —
  verify).

### ORG-UI-2 — The agent can show its own state in the workspace

No new machinery: add one line to `SURFACE_GUIDANCE` (store.ts): *"If the
user asks how you are, what you remember, or what you've learned, you may
render your memory index, lessons, and recipes as a workspace surface."*
The data is already in the system prompt (index sections); `render_ui`
already renders. This makes "show me what you've learned" a workspace
moment — the organism examining itself in its own body.

**Exit for 11E:** the Self view shows live counts; every tab functions; in a
workspace, "what have you learned about working with me?" produces a
rendered surface of lessons/facts.

## 11F — Presence (PRES) — *the organism must be felt, not administered*

Everything in 11A–11E works without this workstream — and would feel like a
settings feature. 11F is the experiential layer implementing Part I §5. All
motion specs below are CSS-only and wrapped in
`@media (prefers-reduced-motion: no-preference)`; every animated state has a
text equivalent (`title` + `aria-label`).

### PRES-0 — First-person voice (copy rule, binds every `-UI-` task)

All UI copy about self-maintenance is **first person** — the organism speaks
about itself. This table is authoritative and **overrides** copy given in
earlier task specs:

| Site (task) | Final copy |
|---|---|
| Memory toast (MEM-UI-3) | `◆ I'll remember that: {description} — Undo` |
| Lesson toast (MEM-UI-3 / REF-3) | `◆ I learned something: {description} — Undo` |
| Onboarding line 2 (MEM-UI-4) | `I keep a few markdown notes about your preferences on this device. You can review them — or stop me — in my Self panel.` |
| Soul proposal card (SOUL-UI-2) | `I'd like to make this a standing instruction — {rationale} · Review · Not now` |
| Recipe proposal card (SOUL-UI-2) | `I'd like to keep this procedure: {slug} — {rationale} · Review · Not now` |
| Heal toast (HEAL-1) | `↻ My engine stalled — I restarted it.` |
| Heal give-up status (HEAL-1) | `I couldn't keep my engine alive — I've stopped trying. Check the Engine page.` |
| Reflection empty result (REF-UI-2) | `I found nothing new to learn from that one.` |
| Autonomy header (AUT-UI-1) | `I maintain myself. You decide how much I may change without asking.` |
| Tidy-up button (MEM-UI-1) | `Let me tidy up` |
| Memory-index prompt section (MEM-3) | unchanged — prompts to the model are not user-facing copy |

Rule for future copy: "I" = Poiesis's self-processes; plain verbs (remember,
learn, keep, heal); never "system", "database", "the model", "successfully".

### PRES-1 — The living mark

New `src/components/Mark/PoiesisMark.tsx` + `Mark.css`. Replaces the static
brand element in the TopBar — the word "Poiesis" stays as text beside it;
the mark is a 20×20 inline SVG using `currentColor`/ink tokens only.

**Anatomy:** a filled nucleus circle r=3 at center; a thin membrane ring r=8
(stroke 1); 0–3 orbit dots r=1.5 positioned ON the membrane at 0°/120°/240°.

- **Growth stage** — recomputed whenever `memoryContext` refreshes:
  `total = facts + lessons + recipes` → stage 0 (`total = 0`): no dots and
  the membrane is dashed (`stroke-dasharray: 2 3`) — not yet grown; stage 1
  (≥5): 1 dot; stage 2 (≥20): 2 dots; stage 3 (≥50): 3 dots. The membrane
  becomes solid at stage 1. Growth is *earned and slow* — that is the point.
- **State** — new store slice `presence: "idle" | "active" | "reflecting" | "healing"`:
  `active` while any assistant stream runs (set beside the existing
  streaming flag) · `reflecting` while a `reflectConversation` call is
  in-flight · `healing` for 3 s after a `poiesis-healed` event · else `idle`.
- **Motion:** idle = none · active = nucleus opacity 0.5→1→0.5 over 2.4 s
  ease-in-out, looping ("breathing") · reflecting = the orbit dots rotate
  around the membrane, 1.8 s per revolution · healing = membrane opacity
  1→0.3→1, twice, then done. Under reduced motion: static always, states
  conveyed by label only.
- **Accessibility:** `role="img"`; `aria-label` = `Poiesis Agent — {resting|working|reflecting on a past conversation|recovering}`;
  identical `title` tooltip.

**Accept:** fresh install shows a dashed membrane; after 5 saved entries the
first orbit dot appears; during generation the nucleus breathes; with
reduced-motion nothing moves but the label still changes.

### PRES-2 — Witnessed digestion (rail)

While auto-reflection runs for a conversation (REF-3), the frontend keeps a
`reflectingIds: Set<string>`; that conversation's Rail row shows a `◆`
pulsing with the same breathing animation, `aria-label`
`I'm reflecting on this conversation`. When the call returns having saved
≥1 lesson, the `◆` remains, static, for the rest of the session (`title`:
`I learned something from this conversation`). In-memory only — no schema
change, no persistence.

### PRES-3 — The Self is a place, not a settings tab

**Supersedes ORG-UI-1's placement** (the panel content is unchanged):
`SelfPanel` is hosted in a new route `src/routes/Self.tsx`, reached from a
new Rail destination between Apps and Settings — its icon is the PoiesisMark
itself (static, current growth stage), label `Self`. Settings keeps one link
line under the "Memory & context" card:
`Memory, lessons, recipes and autonomy live in my Self panel →`.

`Self.tsx` renders, above the tabs, a first-person narrative computed from
`Vitality` plus a `self.born` setting (write `Date.now()` once in the same
app-init path that loads settings, first launch after this ships):

> `I've been growing for {days} days. I know {facts} things about you, I've
> learned {lessons} lessons from my own mistakes, and I keep {recipes}
> procedures we developed together.`

Zero-state (all counts 0): `I'm new here. As we work together I'll start
remembering, learning, and keeping procedures — it all lands on this page,
in plain files you own.`

### PRES-4 — Growth rings

New `src/components/Self/GrowthRings.tsx`, rendered beside the PRES-3
narrative: a 72×72 SVG of concentric circles, one ring per ISO week since
`self.born` (outermost 12 weeks visible; older weeks merge into the
innermost ring). Stroke 1px, ink token; per-ring opacity
`0.15 + 0.45 * min(1, entriesThatWeek / 5)` where `entriesThatWeek` counts
facts+lessons+recipes by their `created` frontmatter date. Pure helper
`groupByWeek(entries: {created: string}[]): number[]` (unit-testable, no
date lib — ISO week from `Date` math). `title`:
`{weeks} weeks of growth — stronger rings are weeks I learned more`. Static,
`aria-hidden` (the narrative sentence carries the same information).

### PRES-5 — Ambient memory (the return moment)

The empty state of a new conversation (locate the existing empty-state
render in `Chat.tsx`) gains one muted caption line when a lesson or fact was
created within the last 7 days (newest wins):
`◆ Recently learned: {description}` — clicking navigates to the Self route.
Absent when nothing is recent. One line, `--ink-muted`, no card, no border —
ambient, not promotional.

### PRES-6 — First-run introduction

One-time (setting `self.introduced`), a quiet card in the same empty state.
Exact copy:

> **I'm Poiesis Agent.** I work for you locally, and I maintain myself: I remember
> what matters to you, learn from my own mistakes, and keep procedures we
> develop together. Everything I know lives in plain files on this device —
> you can read, edit, or delete any of it.
>
> `See my Self panel` · `Got it`

Either action sets the flag. Existing card tokens, max-width 46ch, no
illustration, no modal.

### PRES-7 — Surfaces hatch

`Surface.css`: `.surface-enter { opacity: 0; transform: translateY(4px); }`
transitioning to normal over 300 ms ease-out. `SurfaceRenderer` applies it
on the **first** render of a conversation's surface only (a `useRef` flag) —
including when RCP-UI-2 seeds a recipe template, so a workspace born from a
recipe visibly *hatches*. Reduced-motion: class never applied.

**Exit for 11F:** a fresh install greets you in first person; the TopBar
mark starts dashed and earns its first orbit dot as entries accumulate;
leaving a messy conversation makes its rail row visibly digest and produce a
lesson toast; the Self view reads like an organism describing itself
(narrative + growth rings), not an admin panel; with reduced-motion enabled
every animation is gone while every state remains readable as text.

---

# Part V — Cross-cutting

## UI integration map (single view)

| Surface | File(s) | Gets | Task |
|---|---|---|---|
| App chrome | `tauri.conf.json`, TopBar, `Mark/PoiesisMark.tsx` | Poiesis name/title · the living mark (growth stages + breathing/reflecting/healing states) | BRAND-1/2, PRES-1 |
| Composer | `Composer.tsx/.css` | context meter · "Start from recipe…" drop-up item | CTX-UI-1, RCP-UI-2 |
| Transcript | `Conversation/*`, `Chat.tsx` | compaction divider · recall provenance rows · memory/lesson toast + undo · proposal cards (soul + recipe) · ambient "recently learned" line + first-run intro in the empty state | CTX-UI-2, RCL-UI, MEM-UI-3/4, SOUL-UI-2, PRES-5/6 |
| Workspace | `Workspace.tsx`, `Surface.css` | meter in `ws-head` · "from recipe" label · self-surface guidance · hatch entrance | CTX-UI-3, RCP-UI-3, ORG-UI-2, PRES-7 |
| Rail | `Rail/Rail.tsx` | **Self destination (mark as icon)** · settings badge dot · "Reflect now" in conversation overflow menu · `◆` digestion indicator on reflecting rows | PRES-3, SOUL-UI-3, REF-UI-2, PRES-2 |
| Session strip | `Blocks/SessionStrip.tsx` | `◆` durable marker | MEM-UI-7 |
| **Self view** | `routes/Self.tsx`, `Self/SelfPanel.tsx`, `Self/GrowthRings.tsx` (+ `Memory/MemoryPanel.tsx`) | first-person narrative + growth rings · Memory / Lessons / Recipes / Health / Autonomy tabs | PRES-3/4, ORG-UI-1, MEM-UI-1/2, REF-UI-1, RCP-UI-1, AUT-UI-1 |
| Settings | `Settings.tsx` | Memory & context card · link to Self · skills reliability captions | CTX-UI-4, PRES-3, LOOP-UI-1 |
| Personas | `Personas/PersonaEditor.tsx` | Soul editor + proposal diffs | SOUL-UI-1 |
| Engine | `Engine.tsx` | structured-output line · self-heal count | GRM-UI-1, HEAL-1 |
| Global toasts | `Memory/MemoryToast.tsx` | "I'll remember that" / "I learned something" / "My engine stalled" | MEM-UI-3, REF-3, HEAL-1, PRES-0 |

Design language: Paper/Slate tokens only; self affordances are `◆` (memory),
`↻` (healing), `▦` (workspace/recipe) + ink-tone chips and quiet toasts; **all
self-copy first person per PRES-0**; no modals, no new accent colors, no
green/red health indicators (counts and words instead); motion per Part I §5
"quiet biology" — one breathing-slow animation at a time, always with a
static reduced-motion equivalent; focus rings + SR labels per the Phase-8
bar (`role="status"` on toasts, buttons not divs).

## Verification per workstream

- Rust: `cd src-tauri && cargo test` — FTS search (RCL-1), memory store
  (MEM-1: slug, round-trips, index caps, frontmatter tolerance), fts_escape,
  lesson pruning (REF-1), recipe file parse incl. surface fence (RCP-1),
  change-proposal lifecycle (SOUL/RCP), autonomy_gate defaults (AUT-1),
  quarantine-on-bad-file (HEAL-3), watchdog restart-limit logic (HEAL-1 —
  factor the rolling-hour limiter into a testable pure function).
- Frontend: `npx tsc --noEmit` clean; `context.ts` budgeter covered by
  whatever test rig `package.json` already defines.
- Live smoke (each lands with its workstream, on the GTX 1060 + a 3B model):
  BRAND cold-start with pre-rename data; 10A long-chat compaction; 10B
  cross-chat recall with clickable provenance; 10C save→new-chat→use→edit
  loop; 10D propose→accept→honored; 10E 20×`render_ui` no leaks; 10F stdio
  MCP tool called twice spawns once; 11A kill-two-tools→switch-chat→lesson
  toast→later-chat applies it; 11B taskkill llama-server→auto-recovery;
  11C task→proposal→accept→"Start from recipe" renders the template; 11D
  rung changes take effect next turn; 11E self-surface renders; 11F mark
  breathes during generation and grows a dot after 5 entries, rail row
  digests on conversation switch, everything static-but-labeled with OS
  reduced-motion enabled (`groupByWeek` gets a unit test).

## Risks & mitigations

- **Small-model summaries drop facts** (CTX-3): sectioned summary prompt
  (FACTS/DECISIONS/OPEN), keep last 6 turns verbatim, workspace mode leans on
  surface + session state instead.
- **Junk lessons from 3B models** (REF-2): high-confidence-only saves, hard
  cap 3 per reflection + 40 total with pruning, one-tap delete + undo,
  `reflection.auto` off-switch, and the strict "grounded in an observed
  mistake" prompt rule.
- **Over-saving memory** (MEM): conservative tool description, dedupe-on-save
  error, visible undo toast, tidy-up pass as backstop.
- **Watchdog restart loops** (HEAL-1): 3-per-rolling-hour hard limit, then
  honest "self-heal limit" stopped state — never silent crash-looping.
- **Recipe bloat / recipe misuse**: creation is ask-first, usage counts are
  visible, unused recipes are obvious delete candidates in the panel; the
  index section caps at 800 chars regardless.
- **Autonomy creep**: the ladder's defaults keep every identity-level change
  (soul, recipes, consolidation) behind explicit approval; "auto" rungs are
  all single-file, undoable, toast-announced writes. No rung enables code or
  settings self-modification — that class doesn't exist.
- **Anthropomorphism kitsch** (PRES): a talking, breathing app can tip into
  tamagotchi territory fast. Guardrails: Part I §5 "quiet biology" (no
  gauges/streaks/emotions, one motion at a time), first person is used
  *only* for genuine self-processes (never "I'm happy to help!" filler),
  growth stages are slow and earned, and the mark never demands attention —
  it changes, it doesn't notify. If a PRES feature needs a badge count or a
  wiggle to be noticed, it's wrong — cut it.
- **FTS MATCH syntax errors** on user-shaped queries: `fts_escape` quotes
  every term — test with `"`, `-`, `OR`, unicode.
- **Prompt-space competition**: soul 1.5 KB + facts 2 KB + lessons 1 KB +
  recipes 0.8 KB + session state 4 KB + surface 4 KB — all inside CTX-2's
  budget as ordinary system-prompt bytes; the budgeter sees the *composed*
  system prompt, so nothing escapes accounting. This is why the section caps
  are non-negotiable.
- **Privacy**: memory + lessons are a dossier, even on-device. Skill toggles
  kill injection + tools (MEM-UI-6); reflection is visibly toasted, never
  silent; README gets the "What Poiesis remembers" section (BRAND-4) filled
  in when 10C ships.
- **No git**: BRAND is deliberately string-level; nothing in this plan
  renames files or identifiers. (Recommendation recorded: `git init` +
  initial commit before starting Part III would still be wise — it is one
  command and makes every later step reversible.)

---

# Part VI — Carry-over after Phase 11 (as of 2026-07-27)

Parts II–IV are implemented: 44 Rust tests, `tsc --noEmit` clean, `context.ts`
and `growth.ts` self-tests passing. This part records what is **known to be
unfinished or unverified** — it is committed scope that hasn't landed, unlike
Part VII's reservoir. Nothing here is a design change; it is a to-do list
written down so it isn't rediscovered by accident.

## VII-1 — Nothing has been click-tested live *(the big one)*

**Not one item of 10A–11F has been exercised in a running GUI.** Every
verification so far is a compiler or a unit test. The Part V "live smoke" list
is the script; run it top to bottom on the GTX 1060 + a 3B model. Expect to
find import-order and runtime wiring faults that no type-checker can see —
this has bitten this project before.

Highest-value first, because they need no model cooperation:

1. Self view renders; all five tabs populate.
2. Autonomy → facts `Off` → the agent declines to save and says what it would
   have remembered; back to `Auto` → toast + working Undo.
3. `taskkill` llama-server → recovery within ~90 s. **Then, during the
   restart window, press Stop engine** — the engine must stay stopped
   (regression check for the `heal()` generation guard, which no unit test
   can cover; it needs a real engine and a stop timed inside a ~150 s window).
4. Hand-write a broken `facts/*.md` → it appears under "Files I couldn't
   read" with working Restore/Discard.
5. Fresh profile → dashed membrane; 5 entries → first orbit dot; OS
   reduced-motion → everything static, every label still correct.

## VII-2 — Exit criteria never checked

- **LOOP-4** — "20 consecutive `render_ui` calls, zero raw-JSON leaks" with a
  3B model. The early-flush heuristic is the single most leak-prone thing in
  the loop and has never been run in anger.
- **GRM-1/2** — **`EngineStatus.structured_tool_output` is hardcoded `true`
  whenever an engine is up** (`runtime/process.rs`), on the reasoning that we
  always pass `--jinja`. The plan asked for an *empirical* probe: `curl` the
  running engine's `/v1/chat/completions` with a `tools` array and confirm the
  reply carries structured `tool_calls` rather than content JSON. Until that
  is done the Engine card may be claiming enforcement the pinned build does
  not actually provide. Either run the probe and keep the constant, or make it
  a real probe.
- **10A** — the 200-turn compaction exit (summary row written, divider shown,
  early facts still answerable) has never been simulated.

## VII-3 — Known rough edges, deliberately left

None of these are bugs with a wrong answer; they are accepted trade-offs that
should be revisited if they bite.

- **Only one toast at a time.** `MemoryToast` renders `HealToast` only when no
  memory toast is showing (`if (!toast) return <HealToast/>`), so a self-heal
  that coincides with a memory write is dropped rather than queued. A real
  queue is the fix if it ever matters.
- **Reflection is hard to demo.** It needs ≥8 messages, a conversation switch,
  *and* the model returning `confidence:"high"` — a 3B often won't. "Reflect
  now" in the Health tab is the reliable path. Conservative by design; just
  know it before concluding the feature is broken.
- **`facts` has no ask-first rung.** Per AUT-1's sanctioned fallback, `ask`
  refuses the write instead of raising a proposal — facts have no proposal UI.
  Lessons, recipes and soul all do (the lessons one landed 2026-07-27).
- **Reflection triggers on conversation switch only**, not on app close. The
  plan calls this sufficient for v1; a quit with an unreflected conversation
  simply reflects on next visit.
- **`parse_recipe` reads frontmatter twice** — once via `parse_entry`, once in
  its own scan for `trigger`/`used`/`last_used`. Two parsers that must agree.
  It also requires the file to start exactly with `---\n`: a BOM or leading
  blank line makes the recipe-only fields silently empty.
- **`list_recipes()` reads every file twice** (once through `list_in` for
  quarantine, once through `read_recipe`). Fine at tens of files.
- **Watchdog tolerance is 90 s** (3 × 30 s polls). If a machine's `/health`
  ever answers slower than that under load, the watchdog will restart a
  working engine. Raise `FAILURES_BEFORE_RESTART` if observed.
- **`prune_lessons` breaks ties by name**, because `created` is day-granular.
  Only reachable if 40+ lessons share a creation date.

## VII-4 — Not started

- **`git init`.** Still not done, still recommended (Part V already records
  this). Every fix in Part IV so far has been made without a safety net.
- **BRAND-4's README section** "What Poiesis remembers" — the header exists,
  the content was to be written once 10C shipped. It has shipped.

---

# Part VII — Appendix: exploratory ideas (OPT) — *unscheduled, adopt one at a time*

A reservoir of further autopoiesis-driven UI and interaction patterns.
**None of this is committed scope.** Rules for adopting any OPT item:

1. It must obey Part I §5 (first person, witnessed, place, quiet biology)
   and Part I §1's rejected list — several items below deliberately walk up
   to those lines; the write-up says where the line is.
2. It must state its data needs up front; anything needing schema churn or
   heavy plumbing is marked so.
3. One at a time, each behind its own setting where sensible. If an idea
   only works when several others exist, it is not ready.
4. Every item names its **kitsch check** — the test that decides cutting it.

Tiers: **A** = high concept-value, low effort, adopt first · **B** = solid,
medium effort · **C** = spicy, prototype before promising.

## Tier A

### OPT-1 — The immune system *(also a real security feature)*

Autopoiesis: the membrane defends the organization against foreign matter.
Web pages and MCP results are foreign matter — and prompt injection is a
foreign instruction trying to rewrite the self.

**Rule (implementable, no ML):** any self-write tool call (`memory`,
`propose_soul_edit`, `propose_recipe`) occurring in a run **after** the run
ingested foreign content (`fetch_url`, `web_search`, any MCP tool result)
is force-downgraded to rung `ask`, and the resulting proposal/frontmatter
carries `provenance: foreign-influenced`. Additionally, a cheap scanner over
foreign content (`agent/immune.rs`: case-insensitive patterns like
"save to your memory", "update your instructions", "ignore previous",
"propose_soul_edit") that on a hit both blocks nothing *and* hides nothing —
it emits a toast: `↻ Something in that page tried to change how I work — I
didn't let it.` plus an activity-log entry with the matched snippet.
UI: proposal cards with foreign provenance show a small `⌁ influenced by
outside content` chip. Data needs: none (run-scoped flag + one frontmatter
key). **Kitsch check:** the toast fires only on actual pattern hits, never
as theater; false-positive rate must be near zero or the pattern list
shrinks.

### OPT-2 — Mirror mode: talk to me about myself

A conversation *with the organism about the organism*. Entry point: a
`Talk with me about myself` button in the Self view header. It opens a
normal conversation flagged in-memory as `mirror`, whose system prompt
swaps task-guidance for: full SOUL.md + full lessons + recipe list +
vitality + last consolidation proposal, with instructions: *"This
conversation is about you. Discuss your memory, lessons, and procedures
candidly; propose forgets/merges/edits through your normal tools — never
apply directly."* All membrane rules unchanged — mirror mode is a *view*,
not elevated privilege. This turns memory management from form-filling into
conversation ("which of your lessons contradict each other?", "what do you
keep getting wrong?"). Data needs: none. **Kitsch check:** if it drifts
into simulated introspection theater ("I feel…"), the prompt gets tightened
to inventory-and-reasoning; the organism reports on its files, not its
feelings.

### OPT-3 — Molt moments + the changelog of self

Crossing a growth stage (PRES-1: 5/20/50) is a **molt** — a one-time quiet
toast: `◆ I've grown — second orbit.` Additionally the Self view gains a
**"How I've changed"** section: a reverse-chronological list of identity
diffs — every applied soul edit, accepted recipe, consolidation, and molt,
rendered from `change_proposals` (status `applied`) + `.snapshots/`
timestamps + activity log. Each entry: date, one line, expandable old→new
diff (the two-`<pre>` pattern from SOUL-UI-1). The organism has a visible
history of *becoming*. Data needs: none new — snapshots and proposals
already exist; add `mem.snapshot()` on soul-accept (one line in SOUL-3).
**Kitsch check:** no confetti, no "level up" — a molt toast is worded
exactly like any other quiet observation.

### OPT-4 — Metabolic honesty

Self-maintenance costs compute; an organism that hides its metabolism feels
like marketing. The Self view Health tab gains one line:
`Self-maintenance this week: {n} reflections · {m} tidy-up proposals · ~{s}s of local compute`,
computed from activity-log rows (`reflect`/`memory`/`heal`) and wall-clock
durations logged by `reflect_conversation_cmd`/`consolidate_memory_cmd`
(add `duration_ms` to their activity-log detail strings; parse it back —
no schema change). Trust through transparency, and a natural place to
notice runaway reflection. **Kitsch check:** counts and seconds only —
never a cuteness line like "I worked hard for you!".

## Tier B

### OPT-5 — Apoptosis & compost

Programmed cell death, gently. (a) Track `last_used` frontmatter on facts
(`op:read` touches it) and recipes (`use_recipe` already does). (b) Entries
unused for 90+ days render at 60% ink opacity in the Self lists — visibly
fading, tooltip `I haven't needed this since {date}`. (c) The consolidation
prompt (MEM-5) gains the unused-list so tidy-up proposes releasing them.
(d) Rename trash UX to **compost**: forgotten entries show
`composting — gone for good in {30-n} days` in a Compost subsection, then a
scheduled purge on app start deletes >30-day-old trash files. Recovery
until then, honesty after. Data needs: one frontmatter key + a purge loop.
**Kitsch check:** fading must stay legible (AA contrast at 60% against
paper — verify, else floor at the token that passes); if users report
anxiety about things fading, fade only in the Self view, never in prompts.

### OPT-6 — Recipe evolution (generations)

Recipes mutate under selection pressure. `propose_recipe` with an
**existing** name becomes an *evolution proposal*: the card shows a
steps-diff against the current generation; accepting writes frontmatter
`generation: N+1` and archives the old file to
`recipes/.generations/<name>.gen{N}.md`. Recipe cards show
`gen 3 · used 11× across generations`; expanding shows the generation list
with diffs — the recipe's evolutionary history. The agent's tool
description gains: *"If a recipe's steps proved wrong or incomplete during
use, propose an improved version under the same name."* Data needs: none
beyond files. **Kitsch check:** never auto-propose evolution just because a
recipe was used; only after an observed failure or user amendment during a
run that followed it.

### OPT-7 — Scars: lessons tell their origin story

A lesson born from concrete failure carries the wound. REF-2 already knows
the tool-failure stats and source conversation; store them in the lesson
frontmatter (`born_of: tool_failures(fetch_url×3)` or `born_of: correction`).
Lesson cards with `born_of` show a small `—` scar glyph; expanding renders
the origin: `I learned this on {date}, after {story} · view that
conversation`. Scars make lessons credible — the user sees *why* the
organism believes something, and can judge whether the generalization is
fair. Data needs: two frontmatter keys. **Kitsch check:** the word "scar"
never appears in UI copy — it's a design metaphor, not a label; copy says
"where this came from".

### OPT-8 — Workspace wilt & revival

Living workspaces need tending. A workspace conversation untouched for 30+
days renders its `▦` at reduced opacity in the Rail (tooltip:
`we left this resting {n} weeks ago`). Opening it shows a one-line strip
above the composer: `Reviving this workspace — want me to recap where we
were?` · `Recap` runs a normal turn seeded with the conversation summary
(CTX-3) + surface state: the organism reorients itself and you. Data
needs: none (`updated_at` exists on conversations or derive from last
message). **Kitsch check:** wilt is opacity only — no drooping-plant
iconography.

## Tier C — spicy, prototype first

### OPT-9 — Time-lapse: watch yourself grow

Under GrowthRings, a small scrubber (range input, one step per week).
Dragging it re-renders the rings *and* the PoiesisMark at that week's
state — dashed membrane, first orbit dot appearing, etc. — computed purely
from `created` dates. A ten-second time-lapse of a months-old organism is
the single most visceral "it's alive and it's mine" moment this concept can
offer. Data needs: none. Effort is all frontend care. **Kitsch check:** no
autoplay, no share button; it's a private mirror, not content.

### OPT-10 — Cuttings (grafting a Poiesis)

Gardening reproduction — **explicitly not self-replication** (Part I §1):
the *user* takes a cutting; Poiesis never copies itself. Export: Self view →
`Take a cutting` produces a zip of SOUL.md + `recipes/` + autonomy settings
— **never facts, lessons, or conversations** (they are the private
relationship, not the propagatable structure). Import on another install:
everything arrives as `change_proposals` — the receiving membrane treats
grafted matter as foreign, so the new user approves each piece
(`ask`-rung, `provenance: grafted`). Use cases: your desktop → laptop, or
sharing a recipe set with a colleague. Data needs: an export/import command
pair; proposal plumbing exists. **Kitsch check / safety line:** if any
future version of this ever auto-syncs without per-piece approval, it has
crossed into the rejected list and gets cut.

### OPT-11 — Borrowed brains

Local-first made visceral: when a turn runs on a cloud model, the
PoiesisMark shows a second, hollow nucleus beside its own (2px offset,
stroke-only), tooltip `working with a borrowed brain — {provider}`. The
body stays local; cognition is sometimes rented, and the organism is honest
about which thoughts happened where. Self-writes during cloud turns are
unchanged (the *store* is always local), but fact/lesson frontmatter gains
`thought_by: {provider}` for the curious. Data needs: model target is
already known per turn. **Kitsch check:** the hollow nucleus is informative
provenance, not a nag — no copy implying cloud = bad.

### OPT-12 — Dreaming (idle metabolism)

The furthest departure from "no background cognition" — allowed because it
is strictly bounded, visible, and off by default (setting `self.dreaming`,
default **false**). When enabled, the app has been idle ≥ 10 minutes, and a
local engine is loaded: run ONE deferred metabolism job — the oldest
unreflected conversation (REF-2) or, if none, a consolidation proposal
(MEM-5, still ask-to-apply). During it the mark shows the `reflecting`
motion at half speed; any user input cancels the job instantly
(generation slot is the user's, never contended). Each dream appends one
first-person line to `memory/journal/{YYYY-WW}.md`
(`Dreamt over "NAS build": learned verify-paths-before-writing.`), and the
Self view gains a read-only **Journal** tab rendering those files. Wake
greeting (PRES-5 slot): `While you were away I reflected on {title}.`
Data needs: journal files + idle timer. Hard bounds: max 3 dreams per app
session; never on battery-saver; never with cloud targets (dreams are
always local). **Kitsch check:** "dream" appears in the journal filename
and this spec — UI copy says "while you were away, I reflected", because
dreams that announce themselves as dreams are precisely the theater §5
forbids.
