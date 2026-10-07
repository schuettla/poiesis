# Project Poiesis - Coding Plan

**Poiesis can already write code. It cannot yet find out whether the code works,
and it forgets the folder every time you start a new chat.**

Two gaps, one plan. The first is verification: the agent edits and then guesses.
The second is structure: there is no thing that holds a working directory and
the sessions that happened in it. Every modern agent that is good at code has
both. This plan builds both, in that order of importance but shipped together,
because the project entity is what the coding surfaces hang off.

> ID prefixes: **PRJ** the project entity - **COD** the coding capability -
> **-UI** frontend - **-T** tests.
>
> **Status: Phase 0 built, then moved out.** Phases 1-5 not started.
>
> **Status update, 2026-09-14: Phases 1 to 5 built.** Rust lib tests pass
> (562), the eval tests build, TypeScript type-checks clean, and the frontend
> suite passed (240) before the last test additions; the four touched test
> files pass (93). Not yet done: a click-through in the running app.
>
> | Item | State | Where |
> |---|---|---|
> | `COD-1`/`COD-2`/`COD-3` | built, tested | `agent/project.rs`, via `insert_briefs` |
> | `COD-4`/`COD-5` | built, tested | `agent/ledger.rs`, `filesystem.rs` |
> | `COD-6`/`COD-7`/`COD-8` | built, tested | `agent/coderun.rs` (`Toolset::CodeRun`), `sandbox.rs` |
> | `COD-9` | built | `sandbox.rs`, `codeexec.rs` module docs |
> | `COD-10` | built, tested | `agent/diagnostics.rs` |
> | `COD-11` | built, tested | `agent/diff.rs`, `agent/changes.rs`, `changes` tool |
> | `COD-12`/`COD-13` | built, tested | nudge in `run.rs` `classify`; plan sentence in the project block |
> | `COD-14`/`COD-15`/`COD-16` | built, tested | `agent/symbols.rs`; chunking in `index.rs`; `find_symbol` in `filesystem.rs` |
> | `COD-17` | built | Builder preset in `personaPresets.ts` |
> | `COD-18` | built | `artifacts::run_code`, `check_preview` for code |
> | `PRJ-UI-2` dirty dot | built, tested | `TabStrip.tsx` |
> | `PRJ-UI-3` | built, tested | `ChangesPanel.tsx`, `DiffView.tsx`, `diff` item |
> | `COD-UI-1` | built, tested | `ProjectCodeRow` in `FolderHeader.tsx` |
> | `COD-UI-2`/`-3`/`-4`/`-5` | built | `Timeline.tsx`, `Diagnostics.tsx`, `PermissionPanel.tsx`, `Viewer.tsx` |
> | `COD-UI-6` | built | `TaskPolicy` in `routes/Tools.tsx`, under Run project tasks |
>
> Schema is v29: `projects.allow_json`, and `exec_policy` gains `inherit`
> (the Settings default, `code_run.default_policy`).
>
> New crates for Phase 4: `tree-sitter` pinned `=0.25.10` and
> `tree-sitter-language` pinned `=0.1.5`, because newer releases need Rust
> 1.90 and `Cargo.toml` declares 1.77. Grammars: rust, typescript,
> javascript, python, go, json, md.
>
> **Left:** frontend tests for the task step, the two permission prompts and
> the code artifact Run button (built, not covered); a pass through the real
> app.
>
> **Deviations, deliberate:**
> - The project block and `AGENTS.md` sit in the Rust-only working-folder
>   brief, not behind the golden gate: every fact in them comes off the disk,
>   which the frontend mirror cannot see.
> - Changes come from the undo snapshots, never `git diff`. A diff against
>   `HEAD` would show the user's own uncommitted work as the agent's patch,
>   with an Undo beside it.
> - The Changes view lists everything since the last "Keep all"; the header
>   says "this run" only when that is true.
> - No syntax colour in diffs yet.
> - An unattended run needs the project's *own* `allow`; one inherited from
>   Settings does not count.
> - `COD-14` symbols are parsed on demand and cached per file (mtime, size),
>   not stored at index time. `find_symbol` then works in a folder that was
>   never indexed and with no recall model installed. Definitions come from
>   each grammar's own `tags.scm`; uses are whole-word text matches, which is
>   also the fallback for languages with no grammar.
> - `COD-15`: a code chunk may hold several symbols up to 2400 bytes. A single
>   symbol past 4800 bytes is cut on line boundaries and labelled "part i of
>   n", because an embedding request has a limit too.
> - `COD-UI-1` sits under the folder name in the existing header rather than
>   replacing it. The project name row (`PRJ-UI-5`) was already above it.
>
> **The project entity now lives in `PROJECTS_PLAN.md`.** It stopped being a
> coding concept: a project is a named group of sessions that share a context,
> and a working directory is one thing it may have. `PRJ-1` to `PRJ-9` and the
> `PRJ-UI-*` items are specified and maintained there. This plan keeps only
> what is genuinely about code — the project card (`COD-1`), the execution
> policy (`COD-7`) and the task allowlist — and reads `projects.root_path`,
> which is now nullable. **A project with no folder runs no tasks**, the same
> way a read-only one does not; there is nothing to run them in.
>
> The Phase 0 sections below are kept for their history and their notes on
> what shipped. Where they and `PROJECTS_PLAN.md` disagree, that plan wins.
>
> Three departures in Phase 0, each noted at its item: the `trust` column uses
> the app's own vocabulary (`auto`, not the plan's `trusted`); `PRJ-UI-1`'s
> collapsed rail shows one Projects button rather than a column of language
> dots, because there is no language to draw until `COD-1`; and `PRJ-5` is
> deferred whole to Phase 1, since the project notes and prompt blocks it
> governs do not exist yet. `PRJ-6` needed no code — see its item.
>
> **Prerequisite: `PROJECTS_PLAN.md`**, which owns the entity everything here
> hangs off.
>
> **Prerequisite: `SHELL_PLAN.md`.** The navigation this plan's surfaces live
> in (the header tab strip, the end of the Viewer takeover) is specified there
> and is built. This plan **adds tab kinds to that strip**; it does not define
> it.
>
> **The strip stopped being split, and this plan leaned on the half that went
> away.** There is now **one tab area**, and the right sidebar navigates itself
> from its own inline column, the way Settings does. So the three places this
> plan said "document tab" no longer mean the same thing, and the two places it
> said "the dock's own tab" now mean a sub-view. The rule that replaces both,
> stated once here and applied at each item:
>
> | Thing | Where it opens |
> |---|---|
> | An **overview** — Files, Artifacts, Agents, Browser, **Changes** | a sub-view of the right sidebar, chosen from the sidebar's own nav |
> | A **single item** — one file, one artifact, one agent's run, one diff | a tab in the one strip, beside the chats |
>
> `SHELL_PLAN`'s `SHL-20`..`SHL-23` own that change. What it costs this plan is
> written into `PRJ-UI-2`, `PRJ-UI-3` and `COD-UI-2` below. The exact shell
> this plan builds on is under **Shell facts to build against** below.
>
> Everything else it needs already exists:
> `Toolset` dispatch, the folder trust levels (`FILESYSTEM_PLAN`), the
> permission gate, the activity log, `TurnCtx` and `AgentEvent`
> (`HARNESS_PLAN`), the run plan (`PLANNING_PLAN`), personas with tool
> allowlists (`PER-2`), and the schema migration ladder in `db::Db::migrate`.

---

## Shell facts to build against

Read from the source as of `SHELL_PLAN` Phase 6. Check these before starting
any `-UI` item below; every UI surface in this plan attaches to them.

### The two shapes, in code

| Shape | Type | State | Rendered by | Opened with |
|---|---|---|---|---|
| Single item (tab) | `ItemRef` in `src/lib/types.ts` | `itemTabs`, `activeItemId` | `src/components/Workbench/ItemView.tsx`, over the chat cell | `openItem(ref)`, or `selectNode` for file/artifact |
| Overview (sidebar) | `DockView` in `src/lib/types.ts` | `dockView` | `src/components/Workbench/Workbench.tsx`, the `wb-tabs` row | `setDockView(view)` |

```ts
// src/lib/types.ts, today
export type ItemRef = (
  | { kind: "file"; id: string }      // id is the absolute path
  | { kind: "artifact"; id: string }
  | { kind: "run"; id: string }       // one child agent
) & { conversationId?: string };      // stamped by the store on open

export type DockView = "files" | "artifacts" | "agents" | "browser";
```

- **A file tab already exists.** `kind: "file"` is built, with a mono label, a
  file icon and a dirty dot (`ts-dirty`, read from `touchedFiles`). A project
  file needs **no new variant**. See `PRJ-UI-2`.
- **`diff` is the one variant this plan adds.** It was named in `SHL-22` and
  left out until something produces one (`PRJ-UI-3`).
- **`changes` is the one `DockView` value this plan adds** (`PRJ-UI-3`).

### The strip is global

- **One tab set for the whole app**, under the settings key `shell.tabs`.
  Nothing is scoped by project or by conversation. Session tabs, item tabs and
  route tabs all survive a chat switch and a project switch.
- **Every item tab carries its `conversationId`.** `focusItem` in `store.ts`
  stamps it from the live chat when the caller leaves it out. Pressing an item
  tab from another chat makes that chat live (`setActiveConversation`), then
  focuses the item. So `ItemView`, `FileView` and every lookup keyed on
  `activeConversationId` always run against the item's own chat.
- **Consequence for a `diff` item:** the diff must be resolvable from its
  `conversationId` plus its id. Do not assume the run that produced it is the
  live one. Choose an id that stays valid after a reload (for example
  `<run id>:<path>`), because `validateTabSet` restores item tabs on launch.
- **Closing an item tab never switches chats.** Focus goes to a neighbour from
  the live chat only, or back to the chat.
- **Deleting a chat drops its item tabs.** Restore (`validateTabSet`) drops an
  item whose chat is gone, and a file item that is no longer inside its own
  chat's folder. A new variant needs its own drop rule added there.
- **`projects.tabs_json` is not written any more.** It is read once on upgrade
  when no global set exists. Do not build anything on it.

### The trust rule, enforced

- **The agent never focuses a tab.** `useFollowTheAgent` in `Workbench.tsx`
  receives `setDockView` and nothing else. A new artifact's stream event calls
  `setDockView("artifacts")` in the store.
- **For this plan:** the first file edit of a run calls
  `setDockView("changes")`. It must not open a `diff` tab. Only a user click
  opens a `diff` tab.
- **Transitions only.** The hook moves the sidebar when a count rises or a
  state starts, never because a state exists. A `changes` transition follows
  the same rule, and re-arms on a chat switch.

### Adding a sub-view (`changes`)

1. Add `"changes"` to `DockView` and to `DOCK_VIEWS` in `store.ts`, so a
   stored value restores.
2. Add an entry to the `views` array in `Workbench.tsx` with its availability
   condition: the live chat has a project with a folder, and the run changed
   at least one file. Give it a count (files changed) like Agents has.
3. Render its panel in the `wb-tabpanel` branch.
4. Move `RecentChanges` out of the Files branch into this panel (it is
   rendered under the tree today).
5. A stored `dockView` the chat cannot show falls back to Files or Artifacts
   in the dock and is not rewritten. Nothing extra is needed for that.

### Adding an item kind (`diff`)

1. Add the variant to `ItemRef`, and to `isItemRef` in `store.ts` so it
   restores.
2. `itemToSelection` returns `null` for it (the Viewer does not render it).
3. Render it in `ItemView.tsx`, and add its "gone" rule there (for example the
   change set no longer holds that path).
4. Give it a label, a tooltip and an icon in `ItemTab` in `TopBar/TabStrip.tsx`.
   Every item kind has an icon: file uses `FileIcon`, artifact `SparkleIcon`,
   run `AgentIcon`. Icons live in `components/Icons/Icons.tsx`, never in their
   own component file.
5. Add a restore drop rule in `validateTabSet`.

### Where a file line opens (`COD-UI-2`)

`openItem({ kind: "file", id: absolutePath })` opens the tab. **Scrolling to a
line is not built:** `FileView` has no line target. `COD-UI-2` needs a way to
pass one, for example an optional `line` on the file variant that is not part
of `itemKey`, so the same file stays one tab.

### Tests that cover the shell

`src/lib/store.shell.test.ts` (item tabs, restore, migration),
`src/lib/store.projects.test.ts` (the strip stays global across projects),
`src/components/Workbench/Workbench.tabs.test.tsx` (sub-views, the trust
rule), `src/components/TopBar/TabStrip.test.tsx` (one tablist),
`src/App.smoke.test.tsx` (`Ctrl+W` order). A new sub-view or item kind adds
its cases to these files rather than a new one.

---

## What this is not

**This is not Claude Code inside Poiesis.** No terminal pane, no `/commands`,
no session that begins by opening a repo. Poiesis is not an IDE and must not
grow into one.

**This is the Hermes / OpenClaw altitude.** Both are general personal agents
that are also competent at code. They get there the same way: real edit tools,
real command execution against the real project, a workspace the agent knows
about across sessions, and a sandbox policy that is configuration rather than
personality. Hermes exposes `read` / `write` / `edit` / shell / git and treats
coding as one workflow next to browsing and scheduled automations. OpenClaw
exposes `exec` / `read` / `write` / `edit`, keeps its gateway on the host and
moves only tool execution into a sandbox backend, with the workspace mounted
`none` / `ro` / `rw`. Neither became a coding IDE by doing this.

Poiesis already has the file half. It is missing the execution half and the
workspace half.

---

## Where Poiesis actually stands

Read from the source, not the README.

### Already good

- **The edit surface is right.** `filesystem.rs` ships `read_file` (windowed),
  `list_directory`, `search_files`, `write_file`, `edit_file` (exact-snippet
  search-and-replace), `create_dir`, `move_file`, `delete_file`. Every write
  goes through the permission gate, lands in the activity log, and gets an undo
  token. The 2026 scaffold taxonomy names three edit paradigms
  (search-and-replace, unified diff, whole-file rewrite); you ship two, and the
  right two.
- **Retrieval exists.** `index.rs` plus `retrieval.rs` give embedding search
  over an attached folder, with a reranker. Repository-level context is the
  factor that paper ties most directly to fewer invalid edits.
- **The self-contained-creation lane is done.** `artifacts.rs` gives
  `create_artifact` / `read_artifact` / `update_artifact` / `check_preview`, and
  `check_preview` genuinely runs an HTML artifact in a browser and reports its
  console back. A real verification loop, covering one file type.
- **Delegation, plans and the session log are in.**

### Missing

1. **No project entity.** `folder_path` and `folder_trust` are columns on
   `conversations`. The attachment is per chat. Nothing holds a working
   directory across sessions, so the user re-attaches the same folder forever
   and the agent starts from zero every time. The only folder-keyed state in
   the app is `folder_index`, which is a cache, not an entity.
2. **No execution against the project.** `run_code` writes a snippet into a
   throwaway temp directory and runs it there. The working folder reaches it
   only as the `POIESIS_FOLDER` environment variable, withheld entirely when
   the folder is read-only or the run is headless. Ad-hoc timeout: 10 seconds.
   So `cargo check`, `npm test` and `pytest` are all impossible.
3. **No project sense.** Nothing reads `package.json`, `Cargo.toml`,
   `pyproject.toml`, `Makefile`, `AGENTS.md`, or the git branch.
4. **No code-shaped navigation.** No tree-sitter, no LSP, no symbol index.
   `search_files` is filename glob plus case-insensitive substring.
5. **No git awareness.** `.git` appears three times in the whole agent module,
   all as an ignore rule.
6. **No read-before-edit guard.** `edit_file` can be called on a file the model
   never read, or one that changed since it read it.
7. **The sandbox promises what it does not enforce.** `codeexec.rs` opens with
   "no network"; `sandbox.rs` states that outbound network is *not* blocked on
   Windows because that needs an AppContainer profile. The Job Object gives
   real memory, process-count and lifetime limits. The network claim is not
   true today and gets fixed as part of this work.

---

## How other harnesses solve it

**Two independent layers, not one setting.** Codex CLI separates *what the
agent can technically do* (sandbox mode: read-only, workspace-write,
full-access) from *when it must ask* (approval policy: untrusted, on-request,
never). Both must agree. Its orchestrator runs approval, then sandbox
selection, then attempt, then retry with escalation on denial. Poiesis has the
second layer already, in the permission gate and folder trust. It has no first
layer for execution.

**Sandboxing is a backend choice, not a product decision.** OpenClaw defaults
Docker to `network: "none"`, a read-only root filesystem and dropped
capabilities, and its own docs say this "is not a perfect security boundary,
but it materially limits filesystem and process access". Copy the honesty.

**Verification is the differentiator, not the edit format.** The scaffold
taxonomy finds shell + edit + test agents beat single-modality ones, and
integrated planning beats reactive tool-calling. The harness-engineering
literature is sharper: coding self-improvement works because the evaluator is
real. A compiler is not a fuzzy judge.

**Durable state belongs in files and rows, not in the transcript.** Which is
the argument for the project entity as much as for memory.

---

## Settled decisions

- **A project is a first-class row, and it owns the working directory.** Not
  the conversation. A conversation belongs to a project, or to none.
- **Attaching a folder to a chat creates or joins a project.** There is no new
  gesture to learn and no empty-project state to design around. The user who
  never thinks about projects still gets one, silently, and benefits from it
  the second time they open that folder.
- **A project is a workspace, not a container the user must manage.** No
  project settings screen with twelve fields. It has a name, a root, a trust
  level, an execution policy, and a task allowlist. That is all. (It once
  owned a set of open tabs too; the strip is global now, see `PRJ-UI-2`.)
- **Two lanes, and the user never picks.** *Make* is self-contained code in the
  Canvas with no project involved. *Work in a project* is editing a project's
  files with verification. Opening a project picks the second.
- **No free-form shell as the primary tool.** The primary execution tool is
  `run_task`: the model names a task the project itself declares. Free-form
  `run_command` exists behind expert mode with an argv-level allowlist.
- **Execution inherits the project's trust level.** A read-only project never
  runs a task.
- **The agent never commits, pushes, or installs globally on its own.**
- **The check the agent runs is the check the user sees.**
- **No new mode switch in the Composer.**

---

## Phase 0 - The project entity

### `PRJ-1` Schema

```sql
CREATE TABLE projects (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  root_path     TEXT NOT NULL UNIQUE,   -- canonical, as folder_index uses
  trust         TEXT NOT NULL DEFAULT 'confirm',   -- read-only | confirm | auto
  exec_policy   TEXT NOT NULL DEFAULT 'ask',       -- off | ask | allow
  card_json     TEXT,                   -- COD-1 detection result
  card_built_at INTEGER,
  tabs_json     TEXT,                   -- unused: the strip is global (PRJ-UI-2)
  archived      INTEGER NOT NULL DEFAULT 0,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

ALTER TABLE conversations ADD COLUMN project_id TEXT REFERENCES projects(id);
CREATE INDEX idx_conversations_project ON conversations(project_id);
```

`trust` moves off the conversation and onto the project, because it is a
property of the folder and always was. Granting it once per folder instead of
once per chat is most of the daily annoyance gone.

> **Built as v27**, with the top trust level spelled `auto`. The plan wrote
> `trusted`; `permissions::Trust` has always parsed `auto`, and a second name
> for one level is a bug waiting to be written. `exec_policy`, `card_json` and
> `tabs_json` are declared here and left unread until the phases that own
> them — one migration is cheaper than three.

### `PRJ-2` Migration, and one resolver

The v-next migration walks every distinct non-null `conversations.folder_path`,
creates a project named after the folder's last path segment, carries that
conversation's `folder_trust` onto it, and backfills `project_id`. Conflicting
trust levels across chats on the same folder resolve to the **most restrictive**
one.

`conversations.folder_path` and `folder_trust` stay in the table and are not
dropped. They are the fallback for a conversation with no project. One function
resolves the pair:

```rust
/// The working folder and trust for a conversation: the project's when it has
/// one, the legacy per-conversation columns when it does not.
pub fn conversation_folder(&self, id: &str) -> Result<(Option<String>, String), DbError>
```

This is the existing signature. Every one of its callers
(`codeexec.rs`, `filesystem.rs`, `retrieval.rs`, `skillpack.rs`) keeps working
untouched. That is the whole point of doing it this way: the project entity
lands under the app rather than through it.

> **Built.** The resolver is one `LEFT JOIN` plus two `COALESCE`s, and not a
> line changed in any of those four callers. `set_conversation_trust` writes
> the project's row as well as the conversation's, so granting trust in one
> session grants it for the folder.

### `PRJ-3` Lifecycle

- **Create implicitly.** Attaching a folder to a chat looks for a project with
  that canonical root. Found: join it, and the chat inherits its trust,
  policy and card. Not found: create one, named from the folder, and join it.
- **Create explicitly.** `New project` in the Rail: pick a folder, name it,
  and get a first empty session in it.
- **Leave.** Detaching the folder sets `project_id` to null. The project and
  its other sessions are untouched.
- **Archive, not delete.** Archiving hides the project and its sessions from
  the Rail. Nothing on disk is touched, ever. There is no "delete project"
  action, because the word would be read as "delete my code".

> **Built**, with one addition the plan did not call for: attaching a folder
> whose project is archived **un-archives it**. Working in a folder again is
> the plainest possible statement that you still want it, and the alternative
> was a folder that silently rejoins a project the user cannot see.

### `PRJ-4` What a project owns

| Owned | Where it was before |
|---|---|
| Root path, trust | `conversations.folder_path` / `folder_trust` |
| Execution policy, task allowlist | did not exist (`COD-7`) |
| Project card | did not exist (`COD-1`) |
| Folder index | `folder_index`, keyed by path; now reached through the project |
| Its sessions | flat conversation list |

### `PRJ-5` Project scope in the prompt and in memory

The project card block (`COD-2`) and the project's `AGENTS.md` (`COD-3`) are
injected for every session in the project, not re-derived per chat.

Agent-authored project notes live in app data keyed by project id, **never
written into the user's repository**. `AGENTS.md` is the user's file; the agent
reads it and does not edit it. Lessons learned in a project are tagged with its
id so recall can prefer them when working in it, reusing the existing memory
store rather than adding a second one.

> **Deferred to Phase 1, whole.** Everything this item governs — the card
> block, the `AGENTS.md` read, project notes — comes into existence in `COD-1`
> to `COD-3`. There is no project note to keep out of anyone's repository yet,
> so `PRJ-5-T` has nothing to assert; it ships with the writer it guards.

### `PRJ-6` Scope for the rest of the app

`search_folder`, `find_similar` and the Recall toolset gain an implicit project
scope when the conversation has one: search this project first, say so, and
widen only when asked. No new tool arguments; the scope comes from the run's
context the same way the folder does today.

> **Needed no code for the folder half, and that is the point.**
> `search_folder` and `find_similar` both scope through
> `conversation_folder`, which now answers with the project's root — so they
> are project-scoped already, with no tool argument added and no caller
> touched. The Recall (memory) half waits on `PRJ-5`'s project tagging, since
> there is nothing tagged with a project id yet to prefer.

**Tests.** `PRJ-2-T` migration over a fixture db with three chats on two
folders produces two projects, correct backfill, and the most restrictive trust
wins. `PRJ-2-T2` `conversation_folder` returns the project's values when
`project_id` is set and the legacy columns when it is not. `PRJ-3-T` attaching
an already-known folder joins rather than duplicates, comparing canonical
paths. `PRJ-5-T` a project note never lands inside `root_path`.

> **Built**, in `db/mod.rs`, plus one for archive-and-reopen. `PRJ-5-T` waits
> for `PRJ-5`. The frontend half is `store.projects.test.ts` (which project a
> new session lands in, that the strip stays global across projects,
> folder-wide trust, archiving) and `Rail.projects.test.tsx` (the group is a group: no session
> listed twice, and no group at all without projects).

---

## Phase 1 - Project sense and edit hygiene

No new execution risk. Do this even if nothing else ships.

**`COD-1` Project card.** On project create, and on demand, detect and cache
into `projects.card_json`: primary languages by file-extension counts, package
manager and manifest (`package.json`, `Cargo.toml`, `pyproject.toml`,
`go.mod`, `Makefile`, `*.sln`), declared scripts and targets, git work tree and
current branch, and the presence of `AGENTS.md` / `CLAUDE.md` / `README.md`.
Filename plus a shallow manifest parse. Nothing deeper.

**`COD-2` Project block in the prompt.** Compact, capped near 400 tokens,
present only when the conversation has a project and detection found something:

```
Project: nexus (Rust + TypeScript, git branch master)
Build: npm run tauri build | Check: npm run build, cargo check
Test: cargo test | Lint: -
Conventions: AGENTS.md is present and has been read into context.
```

Assembled in Rust beside the other prompt blocks, behind the existing
byte-identical golden gate.

**`COD-3` Agent-instruction files are read, not guessed.** `AGENTS.md` or
`CLAUDE.md` at the project root is read once per run and placed in the prompt
as project instructions, marked project-authored. Cheapest correctness win
available.

**`COD-4` Read-before-edit.** `TurnCtx` grows `path -> (mtime, size)` recorded
by `read_file`. `edit_file` and `write_file` on an existing file refuse, with a
specific and actionable message, if the path was never read this run or its
mtime moved since the read.

**`COD-5` Line numbers on `read_file`.** Prefix each line with its number, so
the model can cite `run.rs:412` and so a diagnostic naming a line is usable
without counting. Stripped on the way into `edit_file` matching.

**Tests.** `COD-1-T` detection over fixture folders per manifest type,
including one with none. `COD-2-T` the block is absent without a project and
byte-stable for a fixture project. `COD-4-T` refuses an unread file; refuses a
moved mtime; succeeds after re-read. `COD-5-T` a snippet copied out of a
numbered read still matches.

---

## Phase 2 - `run_task`, the execution half

**`COD-6` The tool.** New `Toolset::CodeRun`, default **off**, `sensitive`.

```
run_task(task: string, args?: string[])
```

`task` must name a task from the project card's list; anything else is refused
with the valid names. It runs in the **project root** as the working directory,
which is the thing `run_code` structurally cannot do. Confined by the same Job
Object machinery as `sandbox.rs` with a `Profile::task()`: 300s default
timeout, raised memory cap, raised active-process limit (a build spawns many
children), and an environment that is *not* scrubbed to the minimum (a build
needs `PATH`, toolchain vars, `HOME`) but carries a denylist of secret-shaped
variables. Output streams to the UI, and the buffer fed back to the model is
capped at 64KB, tail-biased so the error at the end survives. Cancellable; Stop
kills the tree through the existing kill-on-close.

**`COD-7` The execution policy layer.** `projects.exec_policy`, three values:

| Policy | Effect |
|---|---|
| `off` | No task runs. The tool is not advertised. |
| `ask` | Every run asks, with a per-task "always allow in this project". Default. |
| `allow` | Declared tasks run without asking. `run_command` still asks. |

Crossed with `projects.trust`: a read-only project is forced to `off`, because
a build writes to `target/` and `node_modules/` and pretending otherwise
repeats the honesty problem the `DAT-2` comments already document. Headless and
scheduled runs are forced to `off` unless the user set `allow` explicitly for
that project, matching `SCH-3`.

**`COD-8` `run_command`, expert only.** A `command` plus an `args` array, never
a shell string (no `sh -c`, no `cmd /c`; the same banned-prefix idea Codex uses
to stop the model tunnelling past the tool bridge). Advertised only in "Show me
everything" mode with the project opted in. Always asks. Allowlist keyed on
`(project, command, argv[0])`, so `git status` never grants `git push`.

**`COD-9` Honest sandbox documentation.** Fix the `codeexec.rs` module doc so
it stops claiming network isolation Windows does not provide. State what the
Job Object gives: memory cap, process-count cap, kill-on-close lifetime,
scrubbed environment, scratch working directory. Say plainly that filesystem
and network confinement are advisory on Windows today. Network blocking is an
AppContainer profile and its own piece of work.

**Tests.** `COD-6-T` a non-zero exit surfaces its code and stderr tail.
`COD-6-T2` an overrun is killed and says so. `COD-7-T` a read-only project
refuses every task. `COD-7-T2` headless refuses unless explicitly allowed.
`COD-8-T` a shell metacharacter or banned prefix is refused. `COD-8-T2` the
allowlist matches on `argv[0]` and does not let `git push` through a
`git status` grant.

---

## Phase 3 - Feedback both sides can act on

**`COD-10` Diagnostics parsing.** Parse the common formats into
`{file, line, col, severity, message, code}`: rustc / cargo, tsc, eslint,
pytest, python tracebacks, go build, msbuild. Unrecognised output falls back to
the tail, never to nothing. The model gets the structured list plus a short raw
tail, not the whole log.

**`COD-11` Diff self-inspection.** `changes()` returns what the run has changed
in the project as unified diffs, from git when the root is a work tree and from
the existing undo/trash snapshots when it is not. The agent reads its own patch
before claiming to be finished. Also feeds `PRJ-UI-3`.

**`COD-12` Verification nudge.** Not a hard gate. When a run has edited files
in a project with a known check task and has not run one, the loop appends a
single system note before the final answer: *"you changed N files and have not
run `<check task>`"*. Once per run. If the policy is `off`, the note instead
tells the model to say plainly that it has not verified the change, which is
the honest ending.

**`COD-13` Plan integration.** A `plan` for a run that will edit project code
with a check task available ends with a verify step. The existing plan
machinery does the rest.

**Tests.** `COD-10-T` fixture outputs per parser; an unknown format degrades to
a tail. `COD-11-T` identical diffs for a git and a non-git project with the
same edits. `COD-12-T` the note fires once, only when files were edited and no
check ran.

---

## Phase 4 - Code-shaped navigation

**`COD-14` Tree-sitter symbols.** Grammars for Rust, TypeScript / TSX,
JavaScript, Python, Go, JSON, Markdown. At index time, extract top-level
symbols with byte ranges. **No LSP**: it would mean bundling or discovering a
server per language on Windows, and it buys diagnostics that `COD-10` gets
from the compiler more honestly.

**`COD-15` Function-shaped chunks.** Code files chunk on symbol boundaries with
path and symbol name as the header. Prose keeps the existing chunker.

**`COD-16` `find_symbol`.** Definitions first, then references, each as
`path:line` plus a line of context. Falls back to `search_files` for a language
with no grammar.

**Tests.** `COD-14-T` extraction per grammar. `COD-15-T` a chunk never splits a
function. `COD-16-T` definitions rank above references.

---

## Phase 5 - Making it feel like one thing

**`COD-17` The Builder persona.** Shipped persona, `tools_json` scoped to file
access, folder reading, code execution, code running, delegation and plans,
with a prompt that says: read before you edit, prefer `edit_file` over
rewriting, run the check before claiming it works, say so when you did not.
Personas already intersect with the global toggles (`PER-2`).

**`COD-18` Run a code artifact.** Lane A gets its verification loop. A `code`
artifact in `python` or `node` gains a Run action through the Phase 2
machinery in a scratch directory, and `check_preview` extends to cover it.

---

## UI integration

The capability is invisible unless these ship with it.

### `PRJ-UI-1` Projects in the Rail (Phase 0)

A **Projects** group above the date-grouped conversation list. Each project is
a row: name, language dot, and a session count. Expanding it lists that
project's sessions in place of the flat list. Chats with no project stay below
under the existing Today / Yesterday / Earlier groups, unchanged.

`New project` sits next to `New chat` in `rail-top-actions`. Collapsed rail
shows project rows as language dots.

The Rail keeps being the Rail. This is a group, not a second navigation model.

> **Built, with the collapsed rail departing.** There is no language to draw
> until the project card exists (`COD-1`), and a column of identical
> unlabelled dots says less than a name does. Collapsed, projects get one
> button that opens the list — the same shape the chats already take there.
> The dot on an expanded row is in place and becomes the language dot in
> Phase 1. The row also carries a session count, and its ⋯ menu offers rename
> and archive — never delete.

### `PRJ-UI-2` What this plan adds to the strip (Phase 0)

**The tab strip itself is not defined here.** It is app-wide navigation,
specified in `SHELL_PLAN.md` and built: **one** tab area in the header holding
session tabs, route tabs and item tabs, `SHL-17` for persistence.

This plan attaches to it in three places, and nothing more.

- ~~**A project scopes the tab set.**~~ **Withdrawn.** Built, used, and taken
  out: switching to a chat in another project swapped the whole strip, and in
  use that read as every open tab vanishing. **The strip is one global set**
  under `SHL-17`'s one settings key, whichever project the live chat is in.
  Each item tab carries the `conversationId` it belongs to; pressing it makes
  that chat live, then shows the item. `projects.tabs_json` is no longer
  written, and is read once on upgrade only when no global set exists yet.
- **Sessions come from the project.** The `+` at the end of the strip starts a
  new session *in the current project* rather than a loose chat. This is one
  call site, not a change to the strip.
- ~~**One new item kind: a project file.**~~ **Already built, as `kind:
  "file"`.** The shell's file tab is exactly this: path as id, mono label,
  file icon, rendered full width by `ItemView`. A project file needs no new
  variant. The new variant this plan adds is `diff` (`PRJ-UI-3`).

> **Revised: the second half of this item is gone with the second zone.** It
> read "two new document kinds — `Changes` and a project file", and only one of
> those was ever an openable thing. **Changes is an overview**, so it is a
> sub-view of the right sidebar (`PRJ-UI-3`), not a tab kind. A project file is
> a single item, so it stays a tab — in the one strip now, rendered where the
> conversation renders rather than inside the dock.

> **The `+` is built; the item kind waits for Phase 3, where the Changes view
> that gives a file tab its dot is specified.** Nothing in the strip is scoped
> by project or by conversation any more: session tabs, item tabs and route
> tabs all survive a switch. Closing an item tab only ever lands on a
> neighbour from the chat already live, so closing a tab never switches chats.
> Deleting a chat drops its item tabs.

One behaviour is genuinely new and belongs here because it is about edits, not
navigation:

- **Dirty state on a file tab.** A file the agent changed this run shows a dot
  where its `×` would be, the way an unsaved editor buffer does. Clicking the
  dot puts the right sidebar on the Changes sub-view, scrolled to that file's
  diff, rather than closing the tab. It reads from the same change set `COD-11`
  builds.

> **Partly built.** A dot exists today (`ts-dirty` in `TabStrip.tsx`), but it
> sits **beside the label**, the `×` stays, the dot is not clickable, and it
> reads `touchedFiles`, which resets on every chat switch. To finish: read the
> `COD-11` change set for the tab's own `conversationId` (the strip is global,
> so the tab may belong to a chat that is not live), put the dot in place of
> the `×` while dirty with the `×` on hover, and make a click call
> `setDockView("changes")` plus a scroll target. A click is a user action, so
> it may also make the tab's chat live first, the way pressing the tab does.

### `PRJ-UI-3` The Changes sub-view (Phase 3)

**A sub-view of the right sidebar**, listed in the sidebar's own nav beside
Files, Artifacts, Agents and Browser. It appears there the moment the run
changes a file in the project, and it is what `useFollowTheAgent` puts the
sidebar on at that first edit. It is **not** a tab: a list of every file the
run touched is an overview, and overviews live in the sidebar.

Header: `3 files`, `+64`, `−12`, `this run`, then `Undo all` and `Keep all`.
Per file: path, `+N / −M`, `Undo`, and a collapsed unified diff that expands to
a two-gutter patch (old line, new line) with syntax colour. Collapsed by
default past the third file. Diffs scroll sideways in their own container,
never the panel.

The sidebar is 340px by default, which is enough to read a hunk and not enough
to read a patch. So one diff is also a single item, and gets the item tab every
single item gets: the file's path in the sub-view opens **that file's diff as a
tab** in the one strip, full width, where a patch is actually readable. The
sub-view stays the list; the tab is one file's worth of it.

`Undo` calls the existing undo tokens from `filesystem.rs`. `RecentChanges` is
absorbed into this sub-view rather than kept beside it — it is already a dock
component, so this is a merge, not a move.

> **Build notes.** Steps are under **Shell facts to build against**: add
> `"changes"` to `DockView`, and `{ kind: "diff" }` to `ItemRef`. The first
> edit of a run calls `setDockView("changes")` and never opens a `diff` tab.
> A `diff` tab id must survive a reload and resolve from the tab's own
> `conversationId`, because the strip is global and restores on launch. Its
> icon goes in `Icons.tsx`. Its label is the file name, and its tooltip is the
> full path plus `+N / −M`.

**This is the most important screen in the plan.** It is where "the agent says
it edited things" becomes "here is the patch, put any of it back".

### `COD-UI-1` The project header (Phase 1)

Replaces `FolderHeader` when the conversation has a project. Project name,
root path, and one quiet row of chips: languages, git branch, trust, and a
`Tasks` disclosure. Opening it lists the detected tasks with the command each
runs and a per-task "always allow" toggle, plus a `Re-detect` action and a line
saying whether `AGENTS.md` was read. A project with nothing detected shows no
chips rather than an empty row.

### `COD-UI-2` The task step (Phases 2 and 3)

A `run_task` step reads `building the project` / `ran the tests`, with a result
line that is the outcome and not a line count: `passed`, `3 failed`,
`12 errors`. Live while running, with a spinner and the last output line as the
subtitle, so a four-minute build is not four minutes of silence. Expanding
shows the `COD-10` diagnostics first, raw tail below. Every `line:col` is a
button that opens that file as an item tab in the one strip, scrolled to the
line — one file is one item, so it is a tab and not a sidebar sub-view.

> **Build note.** Opening the tab exists (`openItem({ kind: "file", id })`).
> Scrolling to a line does not: `FileView` takes no line. Add an optional line
> target that is not part of `itemKey`, so a second click on another line of
> the same file refocuses the one tab and scrolls it.

### `COD-UI-3` A `diagnostics` block kind (Phase 3)

Rendered by `SurfaceRenderer`: severity dot, `path:line`, message, error code,
grouped by file, collapsed past five files. Reuses `render_block`, so the model
gets it by emitting one block instead of describing errors in prose.

### `COD-UI-4` The execution prompt (Phase 2)

`PermissionPanel` gains two shapes. For a declared task: a plain question
("Run the project's tests?"), the exact command in monospace, the project it
runs in, the time budget, and one checkbox, "always allow this task in this
project". For `run_command`: the command as argv, one token per line, so a long
or strange command is readable rather than a wall, and a checkbox naming the
exact `command argv[0]` pair it will remember.

Two prompts, not one. A declared task reads as something the user recognises; a
free-form command reads as what it is. Collapsing them into one generic prompt
is how people learn to click through.

### `COD-UI-5` Run in the Canvas (Phase 5)

A runnable `code` artifact gets a Run button beside the existing controls and
an output pane below the source, styled as `PreviewConsole` is, so the two
lanes read as one idea. A traceback renders in the same `diagnostics` shape as
`COD-UI-3`: one error grammar across the app.

### `COD-UI-6` Settings (Phase 2)

Under Tools: `Code execution` (the existing snippet sandbox, unchanged) and
`Run project tasks` with the three-value policy and an honest description of
what the confinement does and does not do. `Run any command` appears only in
"Show me everything" mode with its own warning. Per-project overrides live on
the project header, not here.

---

## Order

**Phase 0 first.** The project entity is a migration, a resolver, and a Rail
group. It is the least glamorous work in the plan and everything else assumes
it.

**Then Phase 1.** Small, risk-free, and `COD-3` plus `COD-4` alone will
visibly change how often the agent gets a code task right.

**Then Phase 2 with `PRJ-UI-3` and `COD-UI-2`.** Execution without the Changes
view is an agent doing things you cannot inspect. The Changes view without
execution is a review screen for unverified work. Together they are the
product.

**Phase 4 is the one to cut under pressure.** Better navigation makes a good
agent faster. Verification makes a guessing agent honest.
