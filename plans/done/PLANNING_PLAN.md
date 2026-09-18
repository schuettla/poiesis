# Project Poiesis — Planning Plan

**A run should show you the work it means to do, not the budget it is allowed to spend.**

The run meter used to read `step 1 of 12`. That number was the step *cap* — a
guard rail — rendered as though it were a status line, and it invited exactly
the reading it deserved: that the app had worked out twelve steps in advance.
It never had. The meter was fixed (2026-09-07) to name what the run is doing
right now, which removed the false claim. **This plan builds the thing that was
missing when the false claim was removed:** a real, visible list of the work a
run intends to do, that the run then works through.

> ID prefixes: **PLN** the plan itself - **PLN-UI** frontend - **-T** tests.
>
> **Status: built.** `PLN-1`…`PLN-5`, `PLN-UI-1`…`PLN-UI-5` and every test
> listed below are in. Two things landed differently from the text and are
> marked where they appear: `plan` is dispatched by the loop rather than being a
> `Toolset` (see the module doc in `agent/plan.rs`), and the plan lives on
> `TurnCtx` rather than `RunState`, because `dispatch` holds `&self` and never
> sees `RunState`.
>
> **Prerequisite: none.** The harness seams this needs already exist
> (`HRN-6`'s `TurnCtx`, `AgentEvent`, the session log). It can start whenever.

---

## Why this is not a checklist component

The temptation is to render a to-do list and call it done. The reasons that
fails are the whole design problem, so they go first:

**A plan that lies is worse than no plan.** Models write a plan, then ignore it
and do something else. If the UI keeps showing the original list while the run
does other work, it is actively misleading — the same failure as `step 1 of 12`,
dressed better. **The plan shown must be the plan the run is actually against.**

**Not every request deserves one.** "What is 2+2" with a five-item plan is
noise. So is a plan for a single web search. The planning step must be skippable
and must skip itself most of the time.

**A plan is a cost.** It is an extra model round trip before any work starts,
which on a slow free-tier provider is the difference between a fast answer and a
wait. It must earn that.

**Re-planning is the hard part.** Real work discovers that step 3 was wrong.
A plan that cannot change is a plan that gets abandoned; a plan that changes
silently is one you cannot trust. Changes must be visible as changes.

---

## Settled decisions

- **The plan is transcript state, not UI state.** It lives in the messages the
  model sees, so the model and the user are looking at the same object. A plan
  the UI knows about and the model does not is the lying-checklist failure with
  extra steps.
- **The model writes the plan, through a tool.** Not by parsing prose, and not
  by a hand-written planner. A `plan` tool with `set` and `update` is how the
  plan gets written and revised, which means every change is already an event
  the timeline and the session log can see.
- **Planning is opt-in per run, decided by the model, bounded by a setting.**
  The system prompt says when a plan is worth writing; the setting is the user's
  override (`always` / `when it helps` / `never`), defaulting to *when it helps*.
- **One plan per run.** Not per turn, not nested. A delegated child may have its
  own; it does not touch its parent's.
- **A plan never gates execution.** The run is not blocked on having a plan, and
  a step that is not in the plan still runs. The plan describes intent; it does
  not authorise. Nothing here touches the permission membrane.
- **Copy is first person, per `PRES-0`** (see `HARNESS_PLAN.md` §Copy).

---

## PLN — the plan itself

**PLN-1 `plan` tool.** A new toolset (`agent/plan.rs`), on by default, no
consent prompt — it writes nothing outside the run.

```
plan { items: ["read the spec", "sketch the layout", "write the file"] }
plan { update: { index: 1, status: "done" } }
plan { update: { index: 2, status: "dropped", why: "the spec already had it" } }
```

Item status is `todo | doing | done | dropped`. `dropped` carries a reason and
stays visible — a plan that quietly loses items is the lying checklist again.
Adding items mid-run is allowed (`plan { add: [...] }`); the additions are
marked as such so a plan that grew is distinguishable from one that was right.

**PLN-2 plan state on the run.** `RunState.plan: Option<Plan>`, where
`Plan { items: Vec<PlanItem>, revisions: usize }`. It is rendered into the
transcript on each turn as a short system line — the current list with statuses
— so the model is always working against the live plan rather than the one it
wrote six turns ago. Cheap: a plan is a handful of short strings.

**PLN-3 when to plan.** A sentence in the system prompt, only when the setting
allows it: *write a plan first when the request has several distinct parts, or
when you will need more than a few steps; otherwise just do the work.* The
model decides. `plan.mode = never` omits the sentence and the tool entirely.

**PLN-4 the plan is in the log.** A plan write is a `session_events` row
(`kind = "plan"`), like `steer` and `summary`. `replay` skips it, the same way.
This is what makes *resume* and *fork* carry the plan across rather than
silently losing it — the bug `fork_conversation` had with summaries.

**PLN-5 a plan ends with the run.** `RunEnded` carries the final plan state, so
a run that stopped at its step cap shows which items it never reached. This is
the honest version of "I stopped at my step limit": *these three are done, these
two are not.*

---

## PLN-UI — what you see

**PLN-UI-1 the plan card.** Above the timeline in the turn that owns it, not in
a side panel — the plan and the steps that execute it belong in one column, read
top to bottom. Items with their status; the current one marked. Dropped items
struck through with their reason on the same line.

**PLN-UI-2 the meter names the plan item.** The run meter currently reads
`searched job market · 0:41`. When there is a plan, the running item is the
better label: `sketching the layout · 0:41`, with the tool line below it in the
timeline where it already is. **This is the payoff for removing `of 12`** —
the status line finally says something about the work.

**PLN-UI-3 revisions are visible.** When the model rewrites the plan, the card
says so (*revised, 2nd version*) and the previous version is behind a
disclosure, the way `CompactDivider` shows earlier summaries. A plan that
changed without saying so is untrustworthy in exactly the way this whole plan
exists to avoid.

**PLN-UI-4 the setting.** Settings → Tools, beside the step limit, since they
are the same kind of control: *Plan the work first — always / when it helps /
never.* Default *when it helps*.

**PLN-UI-5 finished runs keep their plan.** Rehydrated from the log when a
conversation reopens, alongside `steps`. A plan that vanishes on reload was
never state, it was decoration.

---

## Tests

- **PLN-T1** `plan.rs`: `set` replaces, `update` moves one item's status,
  `add` appends and marks the addition, `dropped` keeps the item and its reason.
  An out-of-range index is an error the model can read, not a panic.
- **PLN-T2** `run.rs`: the plan is rendered into the transcript each turn, and a
  run with no plan renders nothing at all (no empty scaffolding).
- **PLN-T3** `log.rs`: a plan write round-trips through `session_events`, and
  `replay` skips `kind = "plan"` — the same shape as the `summary` tests.
- **PLN-T4** fork and resume carry the plan. This is the test that would have
  caught the summary-dropping bug in `fork_conversation`; write it here for the
  same reason.
- **PLN-T5** `RunEnded` at the step cap reports which items were unreached.
- **PLN-UI-T1** the plan card renders statuses, a dropped item shows its reason,
  and a revised plan shows the earlier version behind the disclosure.
- **PLN-UI-T2** the meter prefers the running plan item over the running tool
  name, and falls back exactly as it does today when there is no plan.

---

## Risks and parked items

**The model may plan badly.** Nothing here can fix that, and this plan does not
pretend to: a bad plan visible is still better than a bad plan hidden, because
you can see it going wrong and steer (`HRN-UI-1`) while it does. That is the
actual argument for the feature.

**Small local models will not use the tool well.** Expect plans that are one
item, or three items that are the same item. `PLN-3`'s prompt sentence is the
only lever, and it should stay one sentence — this is not the place to spend
context. If a model reliably writes junk plans, *never* is the right setting for
it, and that is a per-model note rather than more machinery.

**Parked: plans that survive across turns.** A plan currently belongs to one
run. A multi-turn project plan — the thing that would let you come back
tomorrow and continue — is a different feature that wants a different home
(closer to Tasks than to the run loop). Not in scope here, and it should not be
bolted on: `RunState` is the wrong owner for something that outlives the run.

**Parked: delegation.** A lead handing out work already has the Fleet card,
which is a plan of a sort. Whether a lead's plan items map onto delegated
children is a real question and a later one; `SUBAGENTS_PLAN.md` is where it
would live.
