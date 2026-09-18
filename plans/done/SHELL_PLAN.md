# Project Poiesis - Shell Plan

**The app has four places to look and no way to hold two of them at once.**

A conversation, an artifact, a file, a browser session, a fleet of subagents,
Settings. Every one of them is reachable, and every one of them costs you the
last thing you were looking at. Opening Settings replaces the whole window.
Opening a file swallows the Workbench. Switching chats loses your place in the
old one.

This plan replaces that with one navigation model: **a tab strip in the app
header holding whatever you have open**, over a shell that no longer navigates
away from itself.

> ID prefix: **SHL** - tests **-T**.
>
> **Status: Phases 1-7 built. Read Phase 7 first.** It withdraws the central
> claim of everything above it — that the strip is the app's navigation model —
> and everything from `SHL-3`, `SHL-5`..`SHL-8` and `SHL-14`..`SHL-15` is read
> through `SHL-24`. What survives is the *second* tab area: one strip holding
> what was opened out of the right sidebar, plus the conversation itself as its
> first, permanent tab (`SHL-27`).
>
> **Phases 1-6 as built.** Two departures
> from the text below, both noted at their items: `SHL-12` widens the dock
> across the conversation instead of opening a modal over the window, and
> **`SHL-16` is withdrawn** — Settings navigates itself, from its own inline
> column, and the Rail keeps listing conversations and projects inside every
> route.
>
> **The two-zone strip is withdrawn, and Phase 6 replaces it.** Splitting the
> strip was the central idea of Phases 1 and 3, and in use it was the wrong
> one — see `SHL-20`. There is one tab area, and the right sidebar navigates
> itself from its own inline column, exactly as `SHL-16` concluded Settings
> should. Every item below that says "document zone" or "document tab" is read
> through `SHL-20`..`SHL-23`.
>
> **Known limit.** `SHL-17` drops a restored file tab whose folder was
> detached or swapped, but not one whose file was deleted while the folder
> stayed put. That case surfaces as a read error in the viewer instead, which
> says more than a tab disappearing for invisible reasons.
>
> **Prerequisite: none.** This plan is deliberately self-contained and depends
> on nothing that is not in the tree today. It is written to be built first.
> Other plans consume it; it consumes none of them.

---

## What this is not

**Not an editor.** No terminal, no split panes, no file explorer as the primary
surface, no editing files in place. The tab strip is borrowed from editors
because it is the right answer to "I have several things open", not because
Poiesis should feel like one.

**Not a coding feature.** The strip holds a poster artifact and a browser
session exactly as it holds a source file. `CODING_PLAN.md` adds tab *kinds*
later; it does not own the model.

**Not a rewrite.** Almost everything below is a relocation of existing state,
not a replacement for it. Where a mechanism already exists (`view`,
`activeConversationId`, `selected`, `dockOpen`), the strip becomes a
presentation of it rather than a second copy.

---

## Where the shell stands today

Read from the source.

- **`App.tsx`** renders `TopBar`, `Rail`, then one of `Chat` /
  `SettingsHub` (via `isHubView(view)`) / `Library`, then `Workbench`. The
  grid is in `App.css`: `grid-template-columns: var(--rail-w) 1fr var(--dock-w)`
  and `grid-template-rows: 48px 1fr auto`. The 48px header already spans the
  full width. **The strip has a home already; it is currently near-empty.**
- **`view: View`** in the store is the router. `View` is a 16-member union
  (`chat`, `models`, `engine`, `apps`, `settings`, `library`, `self`, `tasks`,
  `activity`, `skills`, `workingdir`, `mail`, `tools`, `usage`, `about`).
  Anything that is not `chat` replaces the entire route area, and
  `showDock = view === "chat"` means it also takes the Workbench away.
- **`TopBar.tsx`** holds the `PoiesisMark`, `SidebarToggle`,
  `WorkbenchToggle` and `EngineStatus`. Everything between the two toggles is
  empty space.
- **`Workbench.tsx`** has its own local `type Tab = "artifacts" | "files" |
  "browser" | "agents"`, built conditionally (Files only with a folder,
  Browser only with a session, Agents only with subruns), plus
  `useFollowTheAgent`, which moves the panel when the agent starts browsing,
  makes an artifact, or spawns a child.
- **`selected: WorkbenchSelection | null`** (`{ kind: "file" | "artifact"; id }`)
  drives the Viewer, and the Viewer **takes over the whole panel**. There is no
  way back except deselecting, and no way to hold two files.
- **`viewerExpanded`** already lets the Viewer cover more than the dock.
  **`dockOpen` / `toggleDock`** already show and hide the dock, and
  `--dock-w` is already drag-resizable (`.app.dock-dragging`).

So: the header is empty, the router is a single-slot replacement, the Workbench
tabs are sections rather than open things, and the Viewer is a dead end.

---

## The model

Two structures, and the difference between them is the whole idea.

**The Rail is a source list.** Everything that *exists*: conversations, and the
routes. You browse here. Opening something from it puts it in the strip. A row
already open carries a dot.

**The strip is what is open.** In the header, spanning the width, persisted.

The strip has **three tab kinds** and two zones:

| Kind | Zone | Drives | Type face |
|---|---|---|---|
| Session | left | the conversation pane | serif, matching the Rail |
| Document | right | the dock | mono for a file, sans for a panel or artifact |
| Route | left, after the sessions | the whole route area | sans, with the route's icon |

> **Routes sit in the session zone, not in one of their own.** Settings is one
> more thing you have open, reached and closed exactly like a chat; giving it
> its own compartment would say it is a different kind of thing, and it is
> not. Its icon and its sans face are what keep it legible as a section. This
> is what the mockup draws, and the `+` stays to the right of the whole run.

> ~~**The rule the layout rests on: each zone of tabs sits directly above the
> pane it drives.**~~ **Withdrawn — see `SHL-20`.** Session tabs stood on the
> conversation, document tabs on the dock, and the divider between the zones
> was the divider between the panes continued upward. It is a good rule about
> alignment and it was answering the wrong question: what a tab drives is not
> the problem, and paying for the answer with a 340px tab area was too much for
> it. **The table above collapses to two kinds in one zone:** session and route
> as they are, plus **item** — one file, one artifact, one run, one diff — in
> the same run of tabs, in mono for a file and sans for the rest.

And the one rule that is about trust rather than layout:

> **The agent may move the document zone. It may never move the session zone.**
> An agent that switches which conversation you are in while you are typing is
> a bug, not a feature.
>
> **Restated for one zone, and it is the same rule.** With no document zone to
> aim at, the agent's target is **the right sidebar's sub-view**: it may put the
> sidebar on Agents, Browser or Changes. It may **never focus a tab**, because
> the tabs now include the one you are typing in, and the prohibition above was
> never really about the session zone — it was about not being moved. This is
> the one thing `SHL-22` takes away from the agent that it has today: a new
> artifact opens the Artifacts sub-view instead of opening itself.

---

## Settled decisions

- **The strip presents existing state; it does not duplicate it.** A focused
  session tab *is* `activeConversationId`. A focused route tab *is* `view`.
  Only the open-tab lists and the active document are new state.
- **`view` stays the router.** `isHubView(view)` and the `App.tsx` route
  rendering are untouched. What changes is that `view` is now reachable without
  losing the chat behind it.
- **The Viewer stops taking over.** A file or artifact becomes a document tab.
  `viewerExpanded` is repurposed to "maximise the dock across the conversation",
  which is the thing it was reaching for anyway.
- **A closed strip is today's app.** With one session tab and one document tab,
  this is the current layout with the Workbench tabs moved upward. The model
  only starts paying for itself at two.
- **The dock is narrow, and that is accepted.** The document zone scrolls
  horizontally rather than wrapping, and the dock edge is already draggable for
  when a document needs room.
- **Nothing about the Composer, the Rail's search, or the conversation
  rendering changes.**

---

## Phase 1 - The strip exists, holding what already exists

The safest possible first move: a pure relocation. Nothing new becomes
openable.

**`SHL-1` The strip replaces the Workbench's tab row.** `Workbench.tsx`'s local
`Tab` union and its `wb-tabs` markup move into a new `TabStrip` component
rendered by `TopBar`, in the document zone. The same four conditional entries,
the same counts, the same live dots. `Workbench` keeps rendering the active
section and loses its header.

**`SHL-2` The header becomes the strip's frame.** `TopBar` today is three
parts: `topbar-left` (`SidebarToggle`, `PoiesisMark`, the wordmark), an empty
middle, and `topbar-right` (`EngineStatus`, `WorkbenchToggle`). The strip takes
the empty middle. Nothing crosses to the other side of the window.

The row stays 48px and gains five segments, left to right:

| Segment | Width | Holds |
|---|---|---|
| Brand | `var(--rail-w)` | `SidebarToggle`, `PoiesisMark`, wordmark |
| Session zone | flexible | session tabs |
| Divider | 1px | aligned with the conversation/dock boundary |
| Document zone | `var(--dock-w)` | document tabs |
| Status | fixed, 45px | `WorkbenchToggle` (the engine state moved to the Rail — see below) |

> **Superseded: the engine state left the header.** Everything from here to the
> end of `SHL-2` was about protecting one indicator from a crowded strip. It
> now sits beside the `Settings` row at the foot of the Rail, so there is
> nothing left to protect: the segment holds only `WorkbenchToggle` and is a
> fixed 45px, which the document zone gets back permanently.
>
> The reasoning below was not wrong about *what* the indicator is — global,
> true on every route, and most important before a conversation can start. It
> was wrong about what that buys: those properties argue for somewhere always
> visible, and the Rail's footer is always visible too. What the header cost
> was 175px of tab strip, every minute of every session, for something that
> reads "idle" nearly all of the time.
>
> The Rail's footer also answers the one thing the header could not: it is the
> row you press when the engine is what you want to *do something about*, with
> the Engine section one click away in the same column. The label drops the
> word "Engine" there — standing next to the cog it is the only thing with a
> state — and the full sentence stays in the `title` and `aria-label`.
>
> Kept below rather than deleted, because the dot-only degradation and the
> "reserve the hidden toggle's width" rule both survived the move.

**The status segment survives a full strip.** This is the part of `SHL-2` that
breaks if it is left to flexbox, so it gets its own rules.

- ~~**The segment is reserved, not flexible.**~~ **Gone.** There is nothing in
  the segment left to protect; it is a fixed 45px holding one toggle.
- **The dot is the state; the label is the affordance.** `EngineStatus` renders
  a dot plus a text label. Where there is no room for words the label is
  dropped and the dot stands alone, keeping its `title` and `aria-label`, which
  already carry the full sentence. Nothing is lost but width. **Survived the
  move**, with the trigger changed: it is now the collapsed Rail rather than a
  narrow window, because the Rail's width is the thing that actually decides
  whether words fit there.
- ~~**One exception, and it is the important one.**~~ **Gone with the segment.**
  While `loadingModel` was set the header kept the label and the *document
  zone* gave up the width. In the Rail's footer there is room for the model
  name without taking anything from anyone, so the trade this managed no
  longer exists — the label simply shows the name.
- **`WorkbenchToggle`'s width is reserved even when it is hidden.**
  It currently returns `null` outside `view === "chat"`, so its width would
  appear and disappear as tabs are focused and the whole strip would jitter
  sideways. The segment keeps its box; only the button's visibility changes.
- **`EngineStatus` returning `null` outside Tauri is left alone.** The
  frontend-only dev preview has no engine to report, and reserving space for a
  control that can never exist there would be dead air in the one place the UI
  is iterated fastest.

**Considered and rejected: moving it to the composer.** There is a real
argument for it, and the code even sets the precedent, since the model chooser
moved out of the window chrome to sit under the composer next to the message it
applies to. Engine state is not the same kind of thing. It is global, it is
true on Settings and Library where there is no composer, and the one moment it
matters most is before a conversation can start at all.

> Still rejected, and for exactly these reasons — but they rule out the
> *composer*, not the chrome-versus-Rail question. The Rail's footer is global
> too, and visible on every route. That is where it went.

**Considered and rejected: folding it into `PoiesisMark`.** The mark already
carries the *agent's* state (idle, active, reflecting, healing). The engine
being down is not the agent thinking, and merging the two would make one
signal mean two unrelated things.

**`SHL-3` One session tab.** The session zone shows the active conversation as
a single tab, not closable, plus a `+` that calls the existing
`newConversation`. This does nothing useful yet. It exists so the two-zone
layout and the divider alignment ship and get looked at before any new state
lands behind them.

**`SHL-4` `useFollowTheAgent` targets the strip.** Same transitions (browsing
started, artifact count rose, subrun count rose), same "only transitions move
the panel" rule, new target. It moves the document zone. It is wired so that it
*cannot* touch the session zone, and a test asserts that.

**Tests.** `SHL-1-T` the existing `Workbench.tabs.test.tsx` is rewritten
against the strip, keeping its cases: a tab that disappears does not leave the
panel blank; a fresh chat with a folder lands on Files. `SHL-2-T` the status
segment keeps its width with a full strip, and with `loadingModel` set the
engine label survives while the document zone narrows. `SHL-2-T2` focusing and
unfocusing a route leaves the status segment's box the same width even though
`WorkbenchToggle` is hidden in one of them. `SHL-4-T` an agent event moves the
document zone and leaves `activeConversationId` untouched.

> **`SHL-2-T` and `SHL-2-T2` were never written, and are now moot** — both
> tested the width negotiation that went away with the segment. `SHL-1-T` and
> `SHL-4-T` are in `Workbench.tabs.test.tsx` and `TabStrip.test.tsx`. What
> replaced the `SHL-2` pair is a Rail test asserting the engine readout renders
> in the Settings row and nowhere else.

---

## Phase 2 - Session tabs

**`SHL-5` Open sessions.** New store state:

```ts
sessionTabs: string[];              // ordered conversation ids that are open
openSession(id: string): void;      // adds if absent, then focuses
closeSession(id: string): void;
```

Focusing a session tab calls the **existing** conversation-selection path.
`activeConversationId` stays the single source of truth for which chat is
live; `sessionTabs` only records which ones are open. Selecting a conversation
in the Rail opens it as a tab.

**`SHL-6` Closing rules.** Closing the active session focuses its right-hand
neighbour, then its left. Closing the last one leaves the zone with the `+`
and an empty conversation pane carrying the existing empty state. A session is
never deleted by closing its tab; the Rail row stays exactly where it was.

**`SHL-7` The Rail marks what is open.** A small dot on a conversation row that
has a tab. Cheap, and it is what stops the Rail and the strip reading as two
unrelated lists.

**`SHL-8` Overflow.** The zone scrolls horizontally; the active tab is scrolled
into view on focus. It never wraps to a second row. A tab has a minimum and a
maximum width and ellipsises its title.

**Tests.** `SHL-5-T` opening an already-open conversation focuses rather than
duplicates. `SHL-6-T` closing the active tab focuses the neighbour, and closing
the last leaves a valid empty state. `SHL-7-T` the dot follows the tab list.

---

## Phase 3 - Document tabs, and the end of the Viewer takeover

**`SHL-9` The document reference.** Today `WorkbenchSelection` is
`{ kind: "file" | "artifact"; id: string }`. It grows one variant rather than
being replaced, so every existing consumer keeps working:

```ts
export type DocRef =
  | { kind: "file"; id: string }
  | { kind: "artifact"; id: string }
  | { kind: "panel"; id: "files" | "browser" | "agents" };
```

The three panel ids are exactly the sections Phase 1 relocated. They become
ordinary document tabs, which is what makes a file and the browser sit in one
strip without a special case.

**`SHL-10` Open documents.**

```ts
docTabs: DocRef[];
activeDocId: string | null;
openDoc(ref: DocRef): void;
closeDoc(id: string): void;
```

`selected` is **kept and driven by** `activeDocId` for the `file` and
`artifact` kinds. `Viewer`, `Tree` and `Artifacts` are not rewritten; they keep
reading `selected` exactly as they do now. This is the compatibility seam that
keeps Phase 3 small.

**`SHL-11` Clicking a file opens a tab.** In `Tree`, selecting a file calls
`openDoc` instead of `selectNode`. Same for an artifact in `Artifacts`. The
panel no longer becomes a dead end you have to back out of.

**`SHL-12` `viewerExpanded` is repurposed.** It stops meaning "the Viewer
covers the panel" and starts meaning "the dock is maximised across the
conversation". The existing Escape handler and the existing expand control
carry over unchanged in behaviour from the user's side: the thing gets bigger,
Escape puts it back.

**`SHL-13` The empty dock is not empty.** With no document tab open, the dock
renders the folder tree when a folder is attached and the artifact list
otherwise. Today's landing behaviour, preserved, so closing the last tab is
never a blank column.

**Tests.** `SHL-10-T` `selected` tracks `activeDocId` for file and artifact
refs and is null for a panel ref. `SHL-11-T` clicking two files leaves two
tabs open and the second focused. `SHL-13-T` closing every document tab lands
on the tree with a folder and the artifact list without one.

---

## Phase 4 - Routes stop being a destination

**`SHL-14` Route tabs.** `routeTabs: View[]`, rendered in the session zone
after the open chats and before the `+`, each with its own icon and in sans so
it still reads as a section.

> **One tab per surface, not per `View`.** As first built this pushed a tab for
> every route, and the settings hub is thirteen routes — so looking around
> inside Settings filled the strip with a tab per section, each wearing that
> section's own glyph, and none of them saying "Settings". The hub is *one*
> surface whose sections are navigation inside it (see the withdrawn
> `SHL-16`), so it gets one tab: the cog, labelled Settings, focused whenever
> `view` is any of its sections, and closing from any of them returns `view` to
> `"chat"`. `routeTabFor(view)` in `types.ts` is the single place that mapping
> lives, and `SHL-17`'s restore runs a persisted set through it so an older
> build's per-section tabs collapse instead of coming back. `view` remains the router and the source of truth: a focused
route tab means `view` is that route, and `view === "chat"` means no route tab
is focused. Opening Settings from the Rail pushes `"settings"` onto `routeTabs`
and sets `view`. Closing it pops the tab and returns `view` to `"chat"`, which
restores the conversation and the dock exactly as they were, because neither
was ever unmounted from the store's point of view.

**`SHL-15` `showDock` stops asking about `chat` specifically.** `App.tsx`
currently computes `showDock = view === "chat"`. It becomes "no route tab is
focused", which is the same condition stated in terms of the new model and
survives a future route being added.

**`SHL-16` The Rail follows the focused route. — Withdrawn.** Built, used, and
taken out again.

The idea was that the Rail lists "what exists where you are", so inside
Settings it should list Settings' sections. In the product it read wrong in two
ways at once. The sections became another top-level place to be, when they are
the *inside of one place*; and showing them cost the conversation list, so
being in Settings meant losing sight of your chats.

`SettingsHub` owns its section navigation, always: the small inline column on
its left, which is where the inside of Settings belongs. The Rail lists
conversations and projects whatever route is focused. Nothing here is
conditional on `railCollapsed` any more, in either component.

> Kept as a paragraph rather than deleted, because the reasoning that led to it
> is sound in the abstract and will be proposed again otherwise. The answer is
> that a *route's own inside* is not the same question as *what exists*.

**Tests.** `SHL-14-T` opening and closing a route tab returns `view` to
`"chat"` and leaves `activeConversationId` untouched. `SHL-15-T` the dock is
hidden with a route focused and reappears on close, with the same document tab
still active.

---

## Phase 5 - Persistence, keyboard, and the seams

**`SHL-17` The tab set persists.** Serialised to `settings` under one key as
`{ sessionTabs, docTabs, routeTabs, activeDocId }`. On load, every entry is
validated against what still exists (a deleted conversation, a moved file, a
dropped artifact) and silently dropped if it does not resolve. A tab set that
fails to parse is discarded rather than quarantined; it is worth nothing.

**One scope for now.** There is exactly one tab set for the app. This is the
right answer while conversations are a flat list. It is also the seam: see
Extension points.

**`SHL-18` Keyboard.** `Ctrl+Tab` and `Ctrl+Shift+Tab` cycle within the focused
zone. `Ctrl+W` closes the focused tab. `Ctrl+1`..`Ctrl+9` focus the nth session
tab. `Ctrl+\` already toggles the dock and is left alone. Middle-click closes.
Every binding is skipped while the Composer has focus.

**`SHL-19` Accessibility.** The strip is a `tablist` with two labelled groups.
Arrow keys move within a zone, `Home` and `End` jump to its ends, and the close
control is a real button with its own label rather than a click handler on a
glyph. Focus is visible, using the existing `:focus-visible` treatment.

**Tests.** `SHL-17-T` a persisted set with a deleted conversation and a missing
file loads with those entries dropped and the rest intact. `SHL-18-T` bindings
do not fire while the Composer holds focus.

---

## Phase 6 - One tab area

**`SHL-20` The second zone is removed, not shrunk.**

The two-zone strip was built, used, and is wrong. Three reasons, in the order
they show up in use:

1. **The narrow zone was the one that needed width.** A document tab carries a
   path; a session tab carries a title you already know. The zone that got
   `var(--dock-w)` was the zone holding the long labels, and the one that got
   the flexible remainder was holding "New chat".
2. **Two tab areas is two navigation models.** The plan's own argument for
   putting routes in the session zone — filing something separately says it is
   a different kind of thing, and it is not — applies with more force to a
   whole second strip.
3. **Half of what was in it was never an open thing.** Files, Browser and
   Agents are the *sections of the dock*. `SHL-1` moved them into the strip as
   `{ kind: "panel" }` document tabs, and a section that can be closed but not
   opened from anywhere, and comes back when the dock decides it should, is not
   a tab. This is `SHL-16` again, one surface down: **the inside of a place is
   navigated by that place.**

`TabStrip` renders one `ScrollZone`. The divider, `ts-zone-docs`, `showDocZone`
and the `--dock-w`-derived width go with it. The `+` stays where it is, outside
the zone, at the end.

**`SHL-21` The sidebar navigates itself.** The dock grows its own navigation,
the way Settings has: `Files`, `Artifacts`, `Agents`, `Browser`, and later `Changes`
(`PRJ-UI-3`). One state field, `dockView`, persisted with the tab set. The
conditional availability `SHL-1` preserved stays exactly as it is — Files with
a folder, Browser with a session, Agents with subruns — and `SHL-13`'s rule
becomes its default rather than its fallback: with nothing to show but the
tree, the tree is what shows.

> **As built.** A row of tabs under the folder header (`wb-tabs`, a real
> `tablist`), not a column: at 340px a column costs the tree a quarter of its
> width, and the row is what the dock had before `SHL-1`. Artifacts is always
> offered. The row is not drawn when only one section exists. A stored
> `dockView` the chat cannot show falls back in the dock and is not rewritten,
> so it comes back when its section does.

**`SHL-22` `DocRef` becomes `ItemRef`, and loses the panel.**

```ts
export type ItemRef =
  | { kind: "file"; id: string }
  | { kind: "artifact"; id: string }
  | { kind: "run"; id: string }      // one child agent, not the fleet
  | { kind: "diff"; id: string };    // one file's patch (PRJ-UI-3)
```

`{ kind: "panel" }` does not move to another name; it stops existing, and
`dockView` is what replaced it. `docTabs` / `activeDocId` become `itemTabs` /
`activeItemId`, and `selected` keeps being driven by the active item exactly as
`SHL-10` set up, so `Viewer`, `Tree` and `Artifacts` still read what they read.

**An item tab renders in the route area, not in the dock.** This is the part
that is a real change rather than a rename: a file, an artifact or a patch is
shown where the conversation is shown, at the width of the window less the two
sidebars, and the dock keeps its own sub-view beside it. `SHL-12`'s "maximise
the dock across the conversation" is what a 340px viewer needed, and an item
tab does not need it.

The dock's overviews open items into the strip: a row in `Tree` opens a file
tab, a card in `Artifacts` opens an artifact tab, a row in `AgentsPanel` opens
that run's tab. Every one of them is `openItem`, and none of them takes the
sidebar over any more.

> **As built.** Each item kind carries its own glyph before the label — a file
> its page corner, an artifact the sparkle, a run the branching-agent icon —
> so an item tab reads apart from a chat tab before the label is read, the
> same reasoning `SHL-2`'s route icon already rested on.
>
> `ItemRef` ships with `file`, `artifact` and `run`; `diff`
> lands with `PRJ-UI-3`, which is the first thing that produces one. The item
> renders in `ItemView` over the conversation cell, composer row included.
> The chat is hidden with `display: none`, not unmounted, so a half-typed
> message survives. Strip order is chats, items, routes. Pressing the live
> chat's tab unfocuses the item and keeps its tab. `viewerExpanded` and the
> maximised dock are removed, and so is `focusedSubRunId`: a Fleet card's
> Open button opens the run's tab, and the composer's "agents working" pill
> puts the sidebar on Agents.

**`SHL-23` `useFollowTheAgent` re-aims at `dockView`.** Its transitions are
unchanged (browsing started, a child agent started, and `PRJ-UI-3`'s first
edit); what each one does is set `dockView` rather than push a tab. The hook
loses the ability to focus a tab at all, which is how the model's trust rule is
enforced in one place rather than remembered.

> **As built.** The hook is handed `setDockView` and nothing else. A new
> artifact's stream event calls `setDockView("artifacts")` in the store
> instead of opening a tab.

**Tests.** `SHL-20-T` the strip renders one `tablist`, and a file, a chat and
Settings sit in it together. `SHL-21-T` the dock's nav offers only the sections
its state allows, and `dockView` survives a reload. `SHL-22-T` opening a file
from the tree adds an item tab and does not change `dockView`; the tab renders
over the conversation, not inside the dock. `SHL-23-T` a subrun starting moves
`dockView` and leaves `activeItemId` and `activeConversationId` untouched.

**Migration.** A persisted set written by Phase 3 holds `docTabs` with `panel`
entries. On restore, panel entries are dropped and the last one seen sets
`dockView`, so the sections the user had open come back as the sub-view they
now are instead of vanishing.

---

## Phase 7 - The strip stops being the navigation model

**`SHL-24` A chat, a project and Settings are destinations. Only the sidebar's
items are tabs.**

Phase 6 reduced two tab zones to one and kept everything in it: chats, items
and routes together. In use that was the wrong half to keep. The strip and the
Rail were two lists of where you might be, and they disagreed constantly —
opening a chat from the Rail put it in the strip, so the Rail's list was the
one that mattered and the strip's copy of it was clutter that grew all day. A
Settings tab said Settings was a thing you hold open, when nobody ever holds it
open. Nothing was ever *closed*, so the strip only grew, and with it the
feeling that the app had lost track of where you were.

The answer is the thing Phase 6 deleted, kept and nothing else: **one tab area,
over a pane of its own.**

- **The strip holds exactly what came out of the sidebar.** A file, an
  artifact, one child agent, one patch — `ItemRef`, unchanged. This is the one
  thing the two-zone layout was right about (`SHL-2`), and it costs nothing now
  that it is the only zone. (`SHL-27` adds one tab that did not come out of the
  sidebar: the conversation those items were taken from.)

**`SHL-26` The open item gets its own column.** *Withdrawn by `SHL-27`, kept
because its two rejected placements are still the reasons.* It made
`grid-template-columns` `var(--rail-w) 1fr var(--item-w) var(--dock-w)` — rail,
conversation, open item, sidebar — with the pane carrying its own divider and
its own persisted `itemWidth`.

**`SHL-27` The item fills the main column, and the conversation becomes a tab.**

This is the fourth placement tried for an open file, and the first that gives
it enough room. The three before it each failed on width or on loss:

1. **Over the conversation (`SHL-22`).** Opening a file cost you the chat, with
   no way back but closing the file.
2. **Inside the sidebar.** A diff at 340px is not readable at all, and it
   covered the tree it was picked out of.
3. **Its own column (`SHL-26`).** A document and a conversation splitting one
   window, and neither getting a usable measure of it. On a laptop the pane's
   floor and the conversation's floor are most of the window between them.

What changes the arithmetic is not the layout but **the session tab**: the
conversation joins the strip as its first tab, so it is one click away rather
than something an open file takes from you. Once going back is that cheap, the
item can have everything:

- **The shell is three columns again** — `var(--rail-w) 1fr var(--dock-w)`. The
  item takes the main column's cells, both rows, and the conversation is hidden
  rather than unmounted (`.app.item-open`): its scroll position, the composer's
  draft and any running turn all survive a look at a file, and what you come
  back to is what you left. The composer hides with it — a text box addressed
  to the agent, under a file that is not the conversation, invites a message
  into a stream nobody can see being written.
- **The session tab appears only once something else is open.** Alone it would
  be a control pointing at the page already in front of you. It cannot be
  closed — a chat is a destination, not something held open — and that is
  exactly what makes it safe for an item to take the whole width.
- **The strip is one chat's, whole.** Switching chats takes the session tab and
  every item tab with it, because all of them belong to the chat you left. This
  was already true of the items (`SHL-24`); the session tab makes it visible.
- **`showConversation` is the action, and it is not `closeItem`.** The tabs
  stay open; coming back to one costs a click rather than finding it in the
  sidebar again.
- **`useLiveItems` no longer falls back to the last item.** `activeKey === null`
  now *means* something — the session tab is selected — so the fallback that
  `SHL-26` needed (a zero-width column with tabs rendered into it) is not just
  unnecessary, it would make the session tab unreachable. An `activeItemId`
  naming something gone lands on the conversation, which always exists.
- **The header is one header.** Four segments each pinned to the width of a
  column below it made sense while every divider in the row continued a divider
  in the shell; with the item pane gone they were dividing the header into
  boxes matching nothing, and the strip's box was the narrowest of them. The
  header is now brand, strip, toggles — and the strip takes every pixel between
  them, which is what a file path in a tab actually needs. The toggle goes back
  into the flow: there is no pinned segment left for its 41px to steal from.
- **The location label yields to the strip.** They would otherwise name the
  same chat twice in one row, one of them pressable and one not. With nothing
  open the label shows; with anything open the session tab is the label.
- **`itemWidth`, `setItemWidth`, `itemDragging` and the `item.width` setting
  are gone**, with the `--item-w` property and the pane's resizer. A column
  that is either the whole main column or nothing has no width to remember.

**`SHL-25` The trust rule stops being a rule and becomes a fact of the
layout.** The agent may move `dockView`; it may never take you off what you
opened, and it may never change which chat is live. There is nothing left to
enforce: `setDockView` names a section of the right sidebar, and nothing in the
sidebar can change what the main column is showing. The prohibition is now
true because of where the panes are rather than because something remembers
it.

**The strip shows only the live chat's items.** A tab that silently switched
which conversation was live is the one move the shell must never make, and a
global strip could only offer one by making it. Items for other chats stay in
the store and come back when you do.

**Tests.** `SHL-24-T` the strip is one tablist holding only the live chat's
items, with no chat, route or `+` in it; a run opened from the Agents list
leaves `dockView` and the list itself alone, because the run renders in a
surface this panel does not own. `SHL-24-T2` a persisted set written when chats
and routes were tabs restores as items only. `SHL-27-T` the strip is the live
chat followed by its items; the session tab is first, has no × and ignores
middle-click; pressing it clears `activeItemId` without closing a tab.
`SHL-27-T2` with nothing open there is no strip at all, session tab included.
`SHL-27-T3` an `activeItemId` naming nothing lands on the conversation, not on
a stray item. `SHL-27-T4` opening an item sets `item-open` on the shell and
leaves `.chat-body` mounted behind it — the conversation is hidden, never torn
down.

**Migration.** `sessionTabs` and `routeTabs` are still on disk for anyone
upgrading. `validateTabSet` reads and drops them: there is nowhere to restore
them into, and reading them back as anything would rebuild the model this
removes.

---

## Extension points

Written down so later plans can attach without editing this one.

**A new openable thing is one of two shapes, and picking the wrong one is the
mistake `SHL-20` had to undo.** A **single item** is one `ItemRef` variant plus
one renderer in the route area: one file, one artifact, one run, one patch, one
task's output. An **overview** — a list of many of them — is one `dockView`
value plus one panel. If it can be closed but not meaningfully opened on its
own, it is an overview.

**A new route tab is already free.** Any member of `View` can be opened as a
route tab with no new code.

**The tab set has one scope, and that is settled.** `SHL-17` stores one set
under one key. It was once scoped per project (`PRJ-UI-2`); in use, switching to
a chat in another project emptied the strip, which read as losing everything.
Nothing open is *closed* because the live chat changed: every item carries its
own `conversationId` and is still there when you come back. **What changed in
`SHL-24` is only what is shown** — the strip belongs to one chat, so it draws
that chat's items and no others, behind that chat's own session tab (`SHL-27`).

**`useFollowTheAgent` has one target and a hard limit.** New agent events point
at `dockView` by adding a transition. The hook is handed `setDockView` and
nothing else, so it cannot focus a tab or unfocus one — the prohibition is
enforced by what it is given rather than by being remembered, and it is
tested, so no later plan can quietly weaken it.

---

## What could regress, and what is done about it

**The document zone is as narrow as the dock. — Solved by removing it.**
Roughly 340px by default, which is three or four short tabs, mitigated with
horizontal scroll, ellipsised titles and a draggable dock edge, and accepted
rather than solved. It stayed unsolved because it was not solvable at that
width: `SHL-20` gives those tabs the whole strip instead.

**One strip can hold more kinds than one strip can label.** A chat, a section
and a file now sit in the same run of tabs, and the only thing telling them
apart is the type face and the icon each already carries. Watch for this rather
than pre-solve it: grouping them back apart is how the two zones were arrived
at the first time.

**The header gets crowded, and the engine status is what loses. — Solved by
removing it.** It was the only variable-width thing up there, and it was also
the one indicator that must never be pushed off. `SHL-2` answered that with a
reserved segment and three widths; the answer that actually held was to move
the indicator to the foot of the Rail, where it is just as always-visible and
costs the strip nothing. The header now holds one toggle at a fixed 45px, and
the "crowded header" risk is gone rather than managed.

**`Workbench.tabs.test.tsx` is invalidated.** It tests the local tab union this
plan removes. It is rewritten in `SHL-1`, not deleted: its cases (a
disappearing tab must not blank the panel; a fresh chat with a folder lands on
Files) are the exact regressions this refactor risks.

**Opening things becomes easier than closing them.** A strip that only grows is
worse than no strip. `Ctrl+W`, middle-click, and a per-tab close control all
land in the same phase as the tab kind they close, never later.

**The dot in the Rail can drift from the strip.** Both read the same
`sessionTabs` array. There is no second list.

---

## Order

**Phase 1 alone is shippable and is worth shipping alone.** It moves the
Workbench tabs into the header and proves the two-zone layout with no new
state behind it. If the alignment idea does not survive contact, it is found
here, cheaply.

> It did not survive contact, and it was found here — just later than this
> paragraph hoped, in use rather than at the end of the phase. Phase 6 is what
> the finding costs, and it is smaller than Phase 3 was: one zone deleted, one
> inline nav added, one union renamed.

**Phase 6 comes before anything in `CODING_PLAN.md` Phase 3.** The Changes
screen is that plan's most important surface and it is specified as a sidebar
sub-view, which does not exist until `SHL-21`. Building Changes first would
mean building it twice.

**Phase 3 is the one that changes how the app feels**, because the Viewer
dead end is the sharpest daily edge in the current shell. Phase 2 is smaller
and can land either side of it.

**Phase 4 is separable.** Routes as tabs is a real improvement and touches
`App.tsx` routing, which nothing else here does. It can wait without blocking
anything.

**Phase 5 is not optional.** A strip that forgets itself on restart, or that
can only be closed with the mouse, will read as unfinished no matter how good
the first four phases are.
