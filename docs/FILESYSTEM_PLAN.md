# Deep File System Integration + The Workbench Panel

> Final plan. Step 1 of implementation is to copy this file to `docs/FILESYSTEM_PLAN.md` so it lives with the other project plans.

## Context

Poiesis already has a real filesystem integration, but it is invisible, shallow, and conceptually split.

- **Invisible.** The only place to grant folder access is a "File access" list buried in [Settings.tsx](src/routes/Settings.tsx#L318-L343). Nothing in the conversation says which folders the agent can touch, and a folder only gets used if the model happens to guess an absolute Windows path and trips the consent panel.
- **Shallow.** [filesystem.rs](src-tauri/src/agent/filesystem.rs) exposes three tools — `read_file`, `list_directory`, `write_file`. No edit, no search, no partial read, no size cap, no binary detection, no delete/move/mkdir, no undo. `read_file` will pull a 40 MB file into context; `write_file` silently overwrites.
- **Split.** Artifacts are DB rows (`artifacts` table: `id, conversation_id, title, kind, content`) rendered by a `CanvasPanel` docked inside `.chat-body`, reachable via a floating "Canvas · N" chip. Files are real bytes on disk. To a user these are the same thing — *stuff the agent made or touched* — but they have two stores, two viewers, and two mental models.

The goal is Claude Cowork's *feel* — attach a folder, then just talk about the work — **without** the sandbox: the agent reads and writes the real folder on the real disk. Safety comes from a hard scope boundary, a per-conversation trust level, an undo trash, and an always-visible statement of what is attached.

**Outcome:** one right-hand **Workbench** panel with a single tree and a single viewer, holding both the attached folder and this chat's generated artifacts; a mirrored show/hide icon in the header; a proper file toolkit scoped to the folder; and every destructive act either confirmed or undoable.

---

## The object model (the thing that has to be clear)

One panel, one tree, one viewer, **two origins**:

| | Origin | Lives in | Persists | Path |
|---|---|---|---|---|
| **File** | on disk | the attached folder | forever, outside Poiesis | real, e.g. `docs/plan.md` |
| **Artifact** | made in this chat | `artifacts` DB table | with the conversation | none until saved |

They are not the same, and pretending otherwise would lie to the user about what is on their disk. But they are *opened the same way and rendered by the same viewer*, so the UI keeps them in one tree and one interaction:

- Artifacts sit in a **pinned virtual folder at the top** — `✦ Made in this chat (2)` — always above the real tree, visually distinct (a `✦` marker, italic name, a `kind` chip).
- Every artifact row carries a **`Save →`** action. Saving writes it into the working folder (default location suggested by kind: `.svg`/`.html` → folder root, `.md` → root, image → root; the path is editable in a small inline prompt).
- **On save the artifact promotes**: it leaves the virtual folder and appears at its real path in the tree, marked as touched. The DB row keeps a `saved_path` so the timeline link still resolves; the row is no longer listed separately.
- The agent has both tools and a clear rule in its prompt: **`create_artifact` for things to look at, `write_file` for things to keep.** If a folder is attached and the user says "save that" / "write it to the folder", it uses `write_file` directly and no artifact is created.

That is the whole concept. Everything below serves it.

---

## Part 1 — Backend: working folder + trust level

### 1.1 Schema (migration v5)

In `migrate()` at [db/mod.rs:291-325](src-tauri/src/db/mod.rs#L291-L325), following the `Self::add_column` pattern used for `workspace` at L312, bump `SCHEMA_VERSION` and add:

```rust
// v5 (Working folder): a conversation can attach one real folder on disk.
Self::add_column(&conn, "conversations", "folder_path",  "TEXT")?;
Self::add_column(&conn, "conversations", "folder_trust", "TEXT NOT NULL DEFAULT 'confirm'")?;
// Artifacts remember where they were materialised, if ever.
Self::add_column(&conn, "artifacts",     "saved_path",   "TEXT")?;
```

New table for the undo trash:

```sql
CREATE TABLE IF NOT EXISTS file_trash (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL,
  op TEXT NOT NULL,             -- 'write' | 'edit' | 'delete' | 'move'
  path TEXT NOT NULL,           -- affected path (destination for move)
  prev_path TEXT,               -- move source, else NULL
  blob_path TEXT,               -- <app_data>/trash/<uuid> holding prior bytes; NULL if newly created
  created_at INTEGER NOT NULL,
  undone INTEGER NOT NULL DEFAULT 0
);
```

Db methods beside the existing conversation accessors: `set_conversation_folder`, `set_conversation_trust`, `set_artifact_saved_path`, `add_trash_entry`, `list_trash`, `mark_trash_undone`. Extend `DbConversation` ([db/mod.rs:40](src-tauri/src/db/mod.rs#L40)) with `folder_path: Option<String>` + `folder_trust: String`, and both `SELECT` lists (L409, L594) plus the row mapper at L1415.

> Naming note: `workspace` is already taken on `conversations` for the Generative-UI layout mode. Use `folder_*` throughout.

### 1.2 Trust levels

New enum in [permissions/mod.rs](src-tauri/src/permissions/mod.rs) beside `Mode`:

```rust
pub enum Trust { ReadOnly, Confirm, Auto }   // serde kebab-case
```

Semantics **inside the attached folder**:

| Trust | UI label | read/list/search | write/edit/mkdir | move/delete |
|---|---|---|---|---|
| `read-only` | **Read only** | silent | refused, with a message telling the model to ask the user to raise access | refused |
| `confirm` | **Ask first** (default) | silent | one consent prompt per call | one consent prompt per call |
| `auto` | **Full access** | silent | silent | one consent prompt per call — never silent |

**Outside** the attached folder nothing changes: the existing `ensure_access` flow ([filesystem.rs:92-142](src-tauri/src/agent/filesystem.rs#L92-L142)) still applies — persisted grants, then per-chat grants, then an interactive `PermissionRequest`. The working folder is an *additional* grant source consulted first, not a replacement.

### 1.3 Scope enforcement

Reuse `path_within_root` and `canonicalize_lenient` ([permissions/mod.rs:111-144](src-tauri/src/permissions/mod.rs#L111-L144)) unchanged — they already collapse `..` and follow symlinks, with a passing traversal test at L151. Every tool resolves through `canonicalize_lenient` **before** any check, as `execute` does at [filesystem.rs:157](src-tauri/src/agent/filesystem.rs#L157). Two-path ops check both ends.

Hard guards, applied regardless of trust:

- **Refuse attach** for system roots: drive roots, `C:\Windows`, `C:\Program Files*`, `%APPDATA%`, and the Poiesis app-data dir. Enforced in Rust, not just the UI.
- **Ignore list** when listing/searching/walking: `.git`, `node_modules`, `target`, `dist`, `build`, `.venv`, `__pycache__`, `.next`, plus dotfiles (a `show_hidden` flag on the tree command for the UI).
- **Byte caps**: `read_file` refuses >2 MB without `offset`/`limit`; `search_files` skips files >1 MB, caps at 100 matches; `list_directory` caps at 500 entries with a truncation note.
- **Binary detection**: NUL byte in the first 8 KB → return `<binary file, N bytes>` instead of garbage. PDFs route through the existing `pdf_extract` path from [attachments.rs:64-71](src-tauri/src/commands/attachments.rs#L64-L71).

### 1.4 Undo trash

A `trash` module (`src-tauri/src/agent/trash.rs`). Before any overwriting `write_file`, any `edit_file`, `delete_file`, or `move_file`, copy current bytes to `<app_data>/trash/<uuid>` and record a `file_trash` row. Creates record `blob_path = NULL`, so undo means "delete the created file". `undo_file_op_cmd(id)` restores. Blobs older than 7 days are pruned at startup, next to the app-data setup in [lib.rs:49-76](src-tauri/src/lib.rs#L49-L76).

This is what makes **Full access** defensible and gives **Ask first** a net when the user clicks through.

---

## Part 2 — Backend: the file toolkit

Rewrite [filesystem.rs](src-tauri/src/agent/filesystem.rs) around a shared preamble — resolve path → resolve scope (working folder → grants → interactive) → check trust → snapshot to trash → act → `db.log_activity(..., "file", ...)`. Keep `handles()` and `describe()` in the same shape so [run.rs](src-tauri/src/agent/run.rs) dispatch and the timeline verbs need no structural change; extend both match arms. `execute` already receives `db` and `conversation_id`, so it loads folder + trust itself — no change to `SkillContext` ([skills.rs:25-39](src-tauri/src/agent/skills.rs#L25-L39)).

| Tool | Args | Notes |
|---|---|---|
| `read_file` | `path`, `offset?`, `limit?` | line-based window; caps + binary detection; `1→N of M lines` header when truncated |
| `list_directory` | `path`, `recursive?` (depth ≤3) | honours ignore list; `name/` for dirs; size + mtime |
| `search_files` | `path`, `glob?`, `query?`, `max_results?` | glob on names, regex/literal on contents; `path:line: text`; the orientation tool |
| `write_file` | `path`, `content` | creates parent dirs; snapshots prior bytes |
| `edit_file` | `path`, `old_string`, `new_string`, `replace_all?` | fails loudly if absent or ambiguous; returns a unified-diff excerpt |
| `create_dir` | `path` | recursive |
| `move_file` | `from`, `to`, `overwrite?` | both ends scope-checked; refuses to clobber by default |
| `delete_file` | `path`, `recursive?` | always confirmed at every trust level; always snapshots |

All under the existing `Skill::FileSystem` toggle. Each description notes: *"Paths may be absolute, or relative to the attached working folder."* Relative resolution against `folder_path` is the single biggest usability win — the model no longer has to invent absolute Windows paths.

### System prompt

Where the agent's system prompt is assembled (`src-tauri/src/agent/`), inject when a folder is attached:

```
Working folder: C:\Users\Erich\projects\thing  (access: ask first)
Paths may be relative to this folder. Use search_files to orient before reading.
Prefer edit_file over rewriting whole files.
Use create_artifact for things the user should look at; use write_file when
they want something kept on disk.
```

### Consent prompt copy

`PermissionRequest.summary` currently reads "Nexus wants to…" ([filesystem.rs:118-121](src-tauri/src/agent/filesystem.rs#L118-L121)) — stale brand, fix to Poiesis. For **in-folder** confirms the summary becomes the *operation*, not the scope: `Edit plan.md — replace 3 lines?`, `Delete draft.txt?`. Add `diff: Option<String>` to `PermissionRequest` carrying the excerpt, rendered in [PermissionPanel.tsx](src/components/SidePanel/PermissionPanel.tsx). In-folder confirms show two buttons (**Allow** / **Deny**) plus **Don't ask again in this folder** — which flips trust to Full access — rather than the current four-way Once/Chat/Forever choice, which is about scope and no longer applies once the folder is attached.

### 2.1 Commands for the UI

New `src-tauri/src/commands/files.rs`, registered in the `invoke_handler` at [lib.rs:78-181](src-tauri/src/lib.rs#L78-L181):

- `pick_folder_cmd()` — native dialog (`tauri-plugin-dialog` is already a dependency, used at [Settings.tsx:2](src/routes/Settings.tsx#L2)); validates against the denylist.
- `set_conversation_folder_cmd(id, path: Option<String>)`, `set_conversation_trust_cmd(id, trust)`
- `read_dir_tree_cmd(path, depth, show_hidden)` → `Vec<FileNode { name, path, is_dir, size, modified }>`, same ignore list and same scope check as the agent tools.
- `read_text_file_cmd(path, max_bytes)` — for the viewer.
- `open_path_cmd(path)`, `reveal_path_cmd(path)` — open with default app / reveal in Explorer via `tauri-plugin-opener` (already a dependency).
- `save_artifact_to_folder_cmd(artifact_id, dest)` — materialises an artifact into the folder, writes `saved_path`, records a trash entry.
- `list_trash_cmd(conversation_id)`, `undo_file_op_cmd(id)`

**Fix while here:** [attachments.rs](src-tauri/src/commands/attachments.rs) reads and writes arbitrary paths with no permission check and no size cap (`read_image_data_uri_cmd` L31-40, `save_artifact_cmd` L46-57). Add a shared `assert_ui_readable(path)` — allowed if inside the active conversation's folder, inside a persisted grant, or freshly returned by a native dialog — and apply it to the new commands *and* the three attachment commands.

---

## Part 3 — The Workbench panel

### 3.1 Shell

[App.css:3-28](src/App.css#L3-L28) grows a third column, mirroring the existing `rail-collapsed` mechanism:

```css
.app                              { grid-template-columns: 232px 1fr 340px; }
.app.rail-collapsed               { grid-template-columns:  60px 1fr 340px; }
.app.dock-collapsed               { grid-template-columns: 232px 1fr    0; }
.app.rail-collapsed.dock-collapsed{ grid-template-columns:  60px 1fr    0; }
@media (max-width: 1200px) { /* dock overlays: position fixed right, like PermissionPanel */ }
```

New `src/components/Workbench/` — `Workbench.tsx`, `Workbench.css`, `FolderHeader.tsx`, `Tree.tsx`, `TreeRow.tsx`, `Viewer.tsx`, `RecentChanges.tsx`. `Workbench.css` mirrors [Rail.css:1-9](src/components/Rail/Rail.css#L1-L9): `grid-column: 3; grid-row: 2 / -1; border-left: 1px solid var(--paper-edge-2)`. Spanning `2 / -1` matches the Rail and keeps the composer aligned under the conversation only.

Mounted in [App.tsx:21-30](src/App.tsx#L21-L30) as a sibling of `<Rail />`, rendered in the chat view including workspace mode ([Workspace.css:7-8](src/routes/Workspace.css#L7-L8) already sits in column 2, so it reflows for free). Width transition uses `var(--t-mode)`, which [tokens.css:63-73](src/styles/tokens.css#L63-L73) already neutralises under `prefers-reduced-motion`.

### 3.2 Header toggle

In [TopBar.tsx](src/components/TopBar/TopBar.tsx), a `WorkbenchToggle` copied from `SidebarToggle` (L9-25) with the SVG divider mirrored to `x=12.3`, placed at the **end** of `.topbar-right`. Adds `aria-pressed` and a new `.sidebar-toggle.on { color: var(--ink) }` state (there is no pressed variant today). A dot badge — reusing `.nav-badge` from [Rail.css:167-175](src/components/Rail/Rail.css#L167-L175) — appears when the panel is closed and something changed inside it. Keyboard shortcut `Ctrl+\`.

### 3.3 Anatomy

340px column, four stacked regions. Regions 1 and 4 are fixed; 2 and 3 share the remaining height.

**1 · Folder header** (always visible)

```
📁 thing                                    ⋯
   C:\Users\Erich\projects\thing
   Access:  [ Read only | Ask first ▾ | Full ]
```

Folder name in `--fs-ui-lg`, full path beneath in `--ink-faint` (title attribute for long paths, middle-truncated). The trust selector is a three-segment control — this is where "per-session configurable" lives; it defaults to **Ask first**, is stored per conversation, and changing it is one click from the conversation. `⋯` menu: *Reveal in Explorer*, *Change folder…*, *Detach*, *Show hidden files*. Detach asks for confirmation and clears `folder_path` only — nothing on disk is touched, and the copy says so.

**2 · Tree** (scrolls)

```
⌕ filter
─────────────────────────────────────
✦ Made in this chat              (2)
   ✦ Revenue chart      svg   Save →
   ✦ Draft brief         md   Save →
─────────────────────────────────────
▸ src/
▾ docs/
    plan.md                    ● 2m
  README.md
```

Lazy-expanding via `read_dir_tree_cmd` (depth 1 per expand). Row height ~26px, 15px icon slot + 9px margin matching `.rail-nav li` ([Rail.css:55-73](src/components/Rail/Rail.css#L55-L73)). Filter box reuses `.rail-search` styling ([Rail.css:31-44](src/components/Rail/Rail.css#L31-L44)) and filters across both sections. A `●` dot plus relative time marks files the agent touched this session. Artifact rows use `✦`, italic title, a mono kind chip reusing `.canvas-kind` ([CanvasPanel.css:45-55](src/components/Canvas/CanvasPanel.css#L45-L55)), and a `Save →` on hover/focus. Right-click (and a `⋯` on hover) → Open, Reveal, Copy path, Rename, Delete — all routed through the same confirm-and-snapshot path as the agent's tools, so user deletes are undoable too.

The `✦ Made in this chat` section is omitted entirely when the chat has no artifacts, and the whole tree region is replaced by the empty state below when no folder is attached.

**3 · Viewer** (split within the panel)

Clicking any row — file or artifact — opens it *below* the tree in the same column. The tree collapses to roughly its top third (a drag handle on the divider, position persisted); the viewer takes the rest.

```
─────────────────────────────────────
 plan.md                    ⤢   ×
─────────────────────────────────────
 # Deep File System…
 …rendered markdown…
```

Rendering reuses `ArtifactView`'s existing dispatch ([CanvasPanel.tsx:128-159](src/components/Canvas/CanvasPanel.tsx#L128-L159)), extended to key off file extension as well as artifact `kind`: `.html`/`.svg` → sandboxed iframe, `.md` → `react-markdown`, images → the existing `ImageArtifact` data-URI path, text/code → `<pre>`, binary → a size/type card with *Open with default app*. `⤢` expands to a full-screen overlay (Esc closes) — the escape hatch for wide content that 340px cannot hold, which is why the split option is affordable. `×` returns the tree to full height. The viewer header also carries **Download** (existing `saveArtifactFile` flow) for artifacts and **Reveal** for files.

**4 · Recent changes** (collapsed strip, expands to ~5 rows)

```
▲ Recent changes                    (3)
   edited  plan.md        2m ago   Undo
   created notes.md       5m ago   Undo
   saved   Revenue chart  6m ago   Undo
```

Driven by `list_trash_cmd`. Per-row Undo calls `undo_file_op_cmd` and refreshes the tree. Undone rows grey out rather than vanishing. This strip is the honest answer to "the agent has write access to my real disk".

### 3.4 Empty states

**No folder attached** — the tree region is replaced by:

> **Give Poiesis a folder to work in**
> It can read, search and edit files there — you choose how much it may change.
> `[ Choose folder… ]`

The same one-line affordance appears under the composer on an empty chat, near [Introduction.tsx](src/components/Conversation/Introduction.tsx), so the capability is discoverable without opening the panel.

**Folder attached, no artifacts** — no `✦` section at all; the tree simply starts at the real folder.

**Panel closed** — the header toggle carries a dot when a folder is attached and files changed.

### 3.5 User flows

**A · Attach a folder.** Empty state → `Choose folder…` → native dialog → `pick_folder_cmd` validates → header fills in, tree populates, trust shows **Ask first**. The next agent turn sees the folder in its system prompt. *Confirmation the concept landed: the user can now say "what's in here?" with no path at all.*

**B · Orientation, no friction.** "What's in this folder?" → agent calls `search_files`/`list_directory`, **no prompt**, answers with relative paths. Timeline shows `listed thing/` and `searched *.md`.

**C · An edit.** "Tighten the intro of plan.md." → agent `read_file` (silent) → `edit_file` → consent panel: *"Edit plan.md — replace 3 lines?"* with the diff excerpt, and **Allow / Deny / Don't ask again in this folder**. On Allow: the file writes, its tree row gains a `●`, Recent changes gains an `edited plan.md · Undo` row, and the timeline step in [AgentRun.tsx](src/components/Conversation/AgentRun.tsx) becomes a click target that opens the file in the viewer plus its own inline **Undo**.

**D · Raise trust mid-session.** Third prompt in a row → user clicks **Don't ask again in this folder** (or the **Full** segment). Subsequent writes go silent; Recent changes keeps accumulating; deletes still prompt.

**E · An artifact.** "Chart the revenue." → agent calls `create_artifact` → panel auto-opens, `✦ Revenue chart` appears at the top of the tree and is auto-selected in the viewer.

**F · Promote artifact to file.** User clicks `Save →` → inline path prompt pre-filled `revenue.svg` → writes into the folder → the row leaves `✦ Made in this chat`, appears in the tree at its real path with a `●`, and Recent changes gains a `saved` row. *This is the moment the merged model pays off — one gesture moves something from "made" to "kept", in one list.*

**G · Read-only mode.** User sets **Read only** before pointing at a sensitive folder. The agent's `edit_file` returns a refusal that tells it to ask the user to raise access; the agent says so in prose rather than silently failing.

**H · Outside the folder.** "Read `C:\other\thing.txt`" → the existing grant flow prompts exactly as today, unchanged, with the four-way Once/Chat/Forever choice — because that request *is* about scope.

**I · Switch conversations.** Folder, trust, tree, artifacts and Recent changes all follow the conversation. Another chat may have a different folder or none.

### 3.6 State — [src/lib/store.ts](src/lib/store.ts)

Alongside `railCollapsed` / `toggleRail` (interface L88-89, impl L448-449):

```ts
dockOpen: boolean;                        // default true
toggleDock: () => void;
folderTree: Record<string, FileNode[]>;   // path -> children, lazy
expandedDirs: Set<string>;
selected: { kind: "file" | "artifact"; id: string } | null;
viewerExpanded: boolean;                  // full-screen overlay
treeSplit: number;                        // 0..1, persisted
touchedFiles: Record<string, number>;     // path -> ts, agent-modified this session
trash: api.TrashEntry[];
attachFolder / detachFolder / setFolderTrust / refreshTree /
selectNode / saveArtifactToFolder / undoFileOp
```

The existing canvas slice ([store.ts:272-278](src/lib/store.ts#L272-L278)) folds in: `canvasOpen` → `dockOpen`, `activeArtifactId` → `selected`. The auto-open effects at [store.ts:1786-1790](src/lib/store.ts#L1786-L1790) and `viewArtifact` at L1555-1563 become "open the panel and select the artifact" — matching the chosen auto-open-and-preview behaviour. `artifacts` stays keyed by conversation id as it is today. The tree refreshes on `step_done` for any file tool.

Persist `dockOpen` and `treeSplit` via `api.setSetting` with new const keys at [store.ts:18-25](src/lib/store.ts#L18-L25), hydrated in the `bootstrap()` `Promise.all` at [store.ts:991-999](src/lib/store.ts#L991-L999) — the mechanism `READING_SCALE_KEY` uses. There is no zustand `persist` middleware or localStorage in this app.

`folder_path` / `folder_trust` ride on the `Conversation` type in [types.ts:112-114](src/lib/types.ts#L112-L114) and load with the conversation at L1114-1119.

### 3.7 What retires

- `.canvas-panel { flex: 0 0 46%; max-width: 640px }` from [Chat.css:22-25](src/routes/Chat.css#L22-L25).
- The floating `.canvas-reopen` chip — [Chat.tsx:81-85](src/routes/Chat.tsx#L81-L85) and [Chat.css:28-44](src/routes/Chat.css#L28-L44). The header toggle replaces it.
- `CanvasPanel`'s hard bail at [L72](src/components/Canvas/CanvasPanel.tsx#L72), its own header/close button, and its `border-left`. `ArtifactView` survives as the shared `Viewer` renderer.

### 3.8 Settings

The "File access" section in [Settings.tsx:318-343](src/routes/Settings.tsx#L318-L343) stays, retitled **Always-allowed folders**, with a line clarifying that these apply across all chats while a chat's working folder is set in the conversation itself. Keeps the global escape hatch without competing with the primary path.

---

## Files touched

**Rust:** `agent/filesystem.rs` (rewrite), `agent/trash.rs` (new), `permissions/mod.rs`, `db/mod.rs`, `db/schema.sql`, `commands/files.rs` (new), `commands/attachments.rs`, `commands/conversations.rs`, `lib.rs`, the system-prompt assembly in `agent/`.

**Frontend:** `App.tsx`, `App.css`, `components/Workbench/*` (new), `components/Canvas/CanvasPanel.tsx` + `.css` (reduced to the shared `ArtifactView` renderer), `components/TopBar/TopBar.tsx` + `.css`, `components/SidePanel/PermissionPanel.tsx`, `components/Conversation/AgentRun.tsx`, `routes/Chat.tsx` + `Chat.css`, `routes/Settings.tsx`, `lib/api.ts`, `lib/store.ts`, `lib/types.ts`.

## Suggested order

0. Copy this plan to `docs/FILESYSTEM_PLAN.md`.
1. Schema, trust enum, working-folder plumbing (§1.1–1.2) — nothing user-visible yet.
2. Rewrite the file skill: scope, caps, full tool set, relative paths, system prompt (§1.3, Part 2) — testable through the existing consent panel.
3. Trash + undo (§1.4).
4. Workbench shell, header toggle, Canvas relocation into the shared viewer (§3.1–3.2, §3.7) — pure UI, no new backend.
5. Folder header, tree, viewer, Recent changes against the new commands (§3.3–3.4, §2.1).
6. Artifact promotion (`Save →`), timeline links, Settings retitle, attachment-command hardening (§3.5F, §3.6, §3.8).

---

## Verification

**Rust unit tests** (`cargo test` in `src-tauri/`), extending `blocks_traversal_escape` at [permissions/mod.rs:151](src-tauri/src/permissions/mod.rs#L151):

- `..` and symlink escapes from the working folder are refused for every tool, including both ends of `move_file`.
- `read-only` refuses write/edit/delete; `confirm` raises exactly one request per call; `auto` writes silently but still prompts on delete.
- Attaching `C:\Windows`, a drive root, or the app-data dir is refused.
- `read_file` on a 5 MB file refuses without `limit` and succeeds with one; a binary file returns the descriptor, not bytes.
- `edit_file` with an absent or ambiguous `old_string` errors instead of writing.
- Trash round-trip: overwrite → undo restores exact prior bytes; create → undo removes the file; delete → undo restores; move → undo restores the original location.
- Relative path resolution: `docs/plan.md` resolves under `folder_path`, and `../escape.txt` does not.

**Manual, in the running app** (`npm run tauri dev`) — walking flows A–I:

1. Open a chat → Workbench visible on the right; header toggle and `Ctrl+\` collapse and restore it; state survives a restart.
2. Attach a folder from the empty state → header shows name, path, **Ask first**; tree populates; `.git`/`node_modules` are hidden.
3. "What's in this folder?" → answered with **no prompt**, using relative paths.
4. "Tighten the intro of plan.md" → consent panel shows the operation and a diff; approve → tree row gains `●`, Recent changes gains a row, Undo restores the original bytes on disk.
5. Click **Don't ask again in this folder** → trust flips to Full, next edit is silent; ask it to delete → still prompts.
6. Set **Read only** → an edit is refused and the agent says so in prose.
7. Ask it to read a file **outside** the folder → the old four-way grant flow appears, unchanged.
8. "Chart the revenue" → panel auto-opens, `✦ Revenue chart` appears at the top of the tree and previews in the viewer.
9. `Save →` on that artifact → it leaves `✦ Made in this chat`, appears at its real path with `●`, and the file exists on disk in Explorer.
10. Click a `.html` file → renders in the split viewer; `⤢` expands full-screen; Esc returns.
11. Click a file-tool step in the timeline → panel opens with that file selected; its inline Undo works.
12. Switch conversations → folder, trust, tree, artifacts and Recent changes all follow the conversation.
13. Narrow below 1200px → the panel overlays instead of crushing the reading column; collapse the left rail → layout stays coherent.
