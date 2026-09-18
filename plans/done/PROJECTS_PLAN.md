# Project Poiesis - Projects Plan

**A project is a place to work, and most places to work are not folders.**

Poiesis has projects today, and they are folders wearing a project's clothes.
`projects.root_path` is `NOT NULL UNIQUE`: you cannot have a project without a
directory on disk, which means you cannot have a project about a book you are
writing, a job you are applying for, or a person you are helping — the three
things people actually keep coming back to.

Every other agent got this right by making the project the general thing and
the folder one of its properties. This plan does the same: a project is **a
named group of sessions that share a context**, and a working directory is
something a project may or may not have.

> ID prefix: **PRJ** - **-UI** frontend - **-T** tests.
>
> **Status: Phase 0 built.** Every item, tested.
>
> One correction the build forced, recorded at `PRJ-3a`: detaching a folder
> from a chat takes *the chat* out of the project rather than clearing the
> project's folder. A control on one chat must never change what its sibling
> sessions are working in.
>
> **History.** `PRJ-1` to `PRJ-6` and `PRJ-UI-1`/`PRJ-UI-2` were specified in
> `CODING_PLAN.md` and built there, as the folder-bound entity described above.
> This plan takes ownership of them, generalises them, and renumbers nothing —
> an item that already shipped keeps its id and gains a "what changes" note.
> `CODING_PLAN.md` keeps only what is genuinely about code: the project card,
> execution policy, and the task allowlist.
>
> **Prerequisite: `SHELL_PLAN.md`**, for the route-tab machinery `PRJ-UI-4`
> opens the project view in. Built.

---

## What this is not

**Not a workspace switcher.** There is no "you must pick a project to begin"
screen, no empty state to design around, and no mode. A loose chat stays a
loose chat forever if that is what it is.

**Not folders with extra steps.** A project with no folder must be genuinely
useful on its own, or this is just a rename. The test is whether someone who
never attaches a directory still wants one.

**Not a second memory store.** Project memory is the memory that already
exists, tagged. A separate store would mean two things to search, two things
to prune, and two places for a fact to hide.

---

## The model

| A project has | Required | Owned since |
|---|---|---|
| A name | yes | `PRJ-1` |
| Its sessions | yes (possibly none) | `PRJ-1` |
| Instructions | no | `PRJ-7`, this plan |
| Project-scoped memory | no | `PRJ-8`, this plan |
| A working folder + trust | **no** | `PRJ-1`, generalised here |
| A project card, exec policy, task allowlist | no | `CODING_PLAN` |

> **The folder is a property, not the identity.** Everything above the folder
> row in that table works with `root_path` null. Everything below it is the
> coding plan's, and reads the folder when there is one.

Two ways in, and the user never has to know there were two:

- **Explicitly.** `New project` in the Rail makes a folderless project and
  opens its view. This is now the primary gesture, because it is the one that
  works for every kind of project.
- **Implicitly.** Attaching a folder to a loose chat still creates or joins
  that folder's project (`PRJ-3`), unchanged. The user who only ever works in
  directories never learns the word "project" and still gets one.

---

## Settled decisions

- **`root_path` becomes nullable.** It stays `UNIQUE`, which in SQLite already
  permits any number of NULLs — so one folder still maps to exactly one
  project, and folderless projects do not collide.
- **A project's instructions are a prompt block, not a persona.** Personas are
  a *voice* the user picked and can switch. Project instructions are *context
  about this work* that applies whatever voice is speaking. They are injected
  together and the persona still wins on tone, format and depth.
- **Project memory is tagged, never separated.** A fact or lesson learned in a
  project carries its id. Untagged memory stays shared, because most of what
  the agent learns about a user is true everywhere.
- **A project never hides memory from itself.** Tagging narrows what *other*
  projects see; inside the project, everything untagged is still there.
- **The project view is a route tab.** `SHELL_PLAN` already built routes that
  open beside the chats and close without losing them. A project view that took
  the window over would re-introduce the dead end that plan just removed.
- **Archive, never delete.** Unchanged from `PRJ-3`, and now more important:
  a project with no folder has nothing on disk to reassure the user with.

---

## Phase 0 - The general entity

### `PRJ-1a` Schema, generalised

```sql
-- was: root_path TEXT NOT NULL UNIQUE
root_path    TEXT UNIQUE,     -- NULL for a project that is not a folder
instructions TEXT             -- PRJ-7: shared across the project's sessions
```

SQLite cannot relax `NOT NULL` in place, so the migration rebuilds the table
and copies every row. `card_json`, `exec_policy` and `tabs_json` come across
untouched.

### `PRJ-3a` Attaching a folder, when there is already a project

`PRJ-3` answered this for a loose chat. With a project in the picture there are
three cases, and guessing between them is how folders get silently swapped:

| The chat's project | What attaching does |
|---|---|
| none | create or join that folder's project (`PRJ-3`, unchanged) |
| has no folder | **the project adopts the folder** — this is the common case, and the one that makes "start a project, add a folder later" work |
| has a different folder | the project's folder is replaced, unless another project already owns the new one, in which case the **conversation moves** to that project, because `root_path` is unique and the folder's project must win |

**Detaching has two scopes, and keeping them apart matters more than the rule
itself.** This plan first said detaching should clear the *project's* folder;
building it showed that is wrong, because the control sits on one chat and
would silently change what every sibling session is working in — the exact
swap-out the table above exists to prevent.

| Gesture | Where | What it does |
|---|---|---|
| Detach folder | the chat's own header | **that chat leaves the project**; the project and its other sessions are untouched |
| Remove folder | the project view (`PRJ-UI-4`) | **the project loses the folder**; every session follows, and none of them leaves |

Each control changes the thing it is standing on. Neither touches disk.

### `PRJ-7` Project instructions

One free-text field, injected into the system prompt of every session in the
project, immediately after the standing instructions (SOUL.md) and before the
memory index. Capped at 4000 characters, which is the same order as the skills
block and well under any model's patience.

The block is assembled in Rust beside the others, behind the existing
byte-identical golden gate, and mirrored in `store.ts` until the port lands.

```
## Project: <name> (instructions for this project; the persona/system prompt
above still governs voice, format and depth)
<instructions>
```

Absent entirely when the conversation has no project or the field is empty. A
project with a folder but no instructions adds nothing to the prompt, so this
costs the existing folder-only projects no tokens.

### `PRJ-8` Project-scoped memory

`Fact` gains `project: Option<String>` in its frontmatter, written when the
memory was saved during a session in a project.

Recall's rule is one sentence: **an entry tagged with a different project is
not eligible; everything else is.** Untagged memory stays shared, so what the
agent knows about how the user likes to be talked to keeps working everywhere,
and only project-specific knowledge is fenced.

`recall_for` keeps its signature and delegates to `recall_for_project(…,
None)`, so the five existing callers and their tests are untouched.

### `PRJ-9` Sessions move between projects

A session can be put into a project, moved to another, or taken out, from the
project view and from the chat's own header. Moving a session never touches its
messages and never touches the folder on disk.

**Tests.** `PRJ-1a-T` the migration rebuilds the table, preserves every column
of every row, and leaves `root_path` nullable. `PRJ-3a-T` the three rows of the
table above, each asserted. `PRJ-7-T` the block is absent without a project and
without instructions, present with both, and byte-stable. `PRJ-8-T` an entry
tagged with another project is not recalled; an untagged one is; one tagged
with this project is.

> **Built**, in `db/mod.rs` (the rebuild, the three attach cases, the two
> detach scopes, instructions round-tripping), `agent/context.rs` (the block is
> absent unless there is something to say; it sits between SOUL and the memory
> index) and `memory/mod.rs` (fencing and the frontmatter round trip).
>
> `PRJ-7` also goes through `CTX-4`, the byte-identical golden gate: the shared
> fixture gained a project and the golden was regenerated, so the Rust and
> TypeScript assemblies are proven to emit the same block rather than two
> similar ones. The frontend half is `store.projects.test.ts` and
> `ProjectView.test.tsx`.

---

## UI integration

### `PRJ-UI-1a` The Rail group, generalised

Built in `CODING_PLAN` and kept. Two changes:

- `New project` no longer opens a folder picker. It creates a folderless
  project named `New project` and opens its view with the name selected for
  typing, which is the same shape as making a new anything.
- A project row's dot is filled when it has a folder and hollow when it does
  not — one mark, telling the two kinds apart without a second row of chrome.

### `PRJ-UI-4` The project view

A **route tab** (`view: "project"`), opened by clicking a project row in the
Rail, closable like Settings, and carrying the project's name as its tab label.

One column, four sections, in this order:

1. **Name.** Edited in place, as a heading — no label, no Save button. Blur or
   Enter commits; Escape reverts.
2. **Instructions.** A plain textarea under a one-line explanation of what it
   does ("Added to every chat in this project"). Autosaves on blur. Empty by
   default with a placeholder, never a wall of pre-filled boilerplate.
3. **Working folder.** With no folder: one `Add a folder` button and a
   sentence saying a project does not need one. With a folder: the path, the
   trust control that already exists, and `Remove`. This is the only place in
   the app where the folder reads as belonging to the project rather than to
   the chat.
4. **Sessions.** The project's chats, newest first, each opening on click, plus
   `New session in this project`. A `Remove from project` action per row
   (`PRJ-9`), which never deletes the chat.

Then, quietly at the bottom: `Archive project`, with the same one-line
reassurance the Rail menu carries — nothing on disk is touched.

**What this view is not.** Not a dashboard. No counts, no charts, no activity
feed. It is the four things a project *is*, arranged so each is editable where
it is shown.

### `PRJ-UI-5` The project on the chat

`FolderHeader` becomes the project header when the conversation has one: the
project's name as a button that opens `PRJ-UI-4`, then the existing folder
chips when there is a folder. A chat with no project is unchanged.

**Tests.** `PRJ-UI-4-T` the view renders all four sections for a folderless
project and swaps the folder section's shape when one is attached.
`PRJ-UI-1a-T` `New project` creates a folderless project and opens its view
rather than a picker.

> **Built.** `PRJ-UI-5` puts a project row above the folder in `FolderHeader`,
> with the name as a button into `PRJ-UI-4` and a `⋯` carrying `PRJ-9`'s
> chat-side half — move this chat to another project, or take it out of one.
>
> Two things the item as written did not cover, both found by building it:
>
> - **A folderless project needed the row most, and would have got it least.**
>   The "give Poiesis a folder" offer replaced the entire panel head, so being
>   in a project with no folder looked exactly like being in no project. The
>   offer is now a block *under* the project row.
> - **The detach confirmation was under-stating itself.** Detaching from a chat
>   in a project also takes the chat out of the project (`PRJ-3a`), and the
>   copy only said "stop working in this folder" — hiding the half the user
>   cannot see. It now names both, and says the project's other chats keep the
>   folder.
>
> One thing the tests caught rather than review: the view's session list was
> filtered and sorted *inside* a zustand selector, which builds a new array on
> every call, never compares equal, and re-renders forever. The Rail already
> carries a comment warning about exactly this. It is a `useMemo` outside the
> selector now.

---

## Order

**`PRJ-1a` first**, because everything else needs `root_path` to be nullable.

**Then `PRJ-7` and `PRJ-UI-4` together.** Instructions the user cannot see or
edit are not a feature, and a project view with nothing in it but a name is not
one either. These two are the pair that make a folderless project worth having.

**`PRJ-8` last of Phase 0.** It is the most invisible of the three and the one
whose absence costs least — a project without scoped memory still works, it
just shares more than it strictly should.
