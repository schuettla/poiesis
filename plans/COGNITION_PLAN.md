# Cognition plan: grow the memory and self-learning Poiesis already has

Status: in progress. v1 written 2026-10-27; **v2 rewritten 2026-10-27**.

## Implementation status

Work goes to `master` in one commit after the last phase and a review. No
commit per phase.

| Phase | State | Date |
|---|---|---|
| 0 - Fix what exists, and measure | done (see notes) | 2026-10-27 |
| 1 - The conversation becomes the episode | done (see notes) | 2026-10-27 |
| 2 - Recall you can see | done (see notes) | 2026-10-27 |
| 3 - Beliefs with evidence | done (see notes) | 2026-10-27 |
| 4 - The daily tidy-up | done (see notes) | 2026-10-27 |
| 5 - Lessons that fire, and rehearsal | done (see notes) | 2026-10-27 |
| 6 - The self-improvement loop | not started | |
| 7 - Intentions | not started | |
| 8 - Self-model | not started | |
| 9 - Project setup | not started | |

### Phase 0 notes

- **FIX-1.** `reflect_conversation_cmd` is now a thin wrapper that emits
  `poiesis-reflecting {conversation_id, on, learned}` before and after the old
  body (`reflect_pass`). `store.ts` `applyReflecting` is the one place that moves
  `reflectingIds`, `digestedIds`, `presence` and `reflectedAt`; the frontend
  `reflectConversation` no longer sets them. A reflection that starts while a
  turn streams keeps the mark on `active`. Tests: `store.reflecting.test.ts`,
  `the_reflecting_event_names_the_chat_and_what_it_learned`.
- **FIX-2.** The daily job reads `unreflected_conversations(CATCH_UP_MIN_MESSAGES)`
  (made `pub(crate)`), so a one-liner is no longer stamped.
- **FIX-3.** `compose_digest` takes a `DigestWhen`. Local hour 22 to 5 says
  "While you were away", any other hour says "Earlier today". Uses the `chrono`
  crate (clock feature only, already in the lock file). Phase 4 makes the job
  idle-only, after which the wording can always be "While you were away".
- **FIX-4.** `narrative` in `Self.tsx` is exported and fixed ("and I can follow N
  skills").
- **MEV-1.** `commands/organism.rs`: `learning_numbers` (windowed) and
  `learning_report` (monthly snapshots in setting `learning.snapshots`, at most
  6, one per 30 days). New `get_learning_report_cmd`. Repeat mistakes: a
  `tool_fixes` row whose tool and error class (text before the first `:`)
  appeared in an earlier conversation, and a lesson mentioning the tool was
  written on an earlier day. Told twice: the second and later user `/forget` of
  one subject, or the second and later declined suggestion of one command.
  Always-on size: `recall_for(None)` index length.
  **Deviation from the plan:** two of the five numbers cannot be computed yet.
  Recall precision needs the credit columns (CRD-2) and stale rate needs the
  Outdated click (CUE-UI-1). Both are fields in `LearningNumbers` that stay
  `None` until then, with a test that says so. `tool_fixes` rows are pruned
  after 30 days, which is why last month's values come from the snapshot and not
  from the table.
- **MEV-UI-1.** `HealthTab` shows "Whether I'm learning" (`lib/learning.ts`).
  "No data yet" until the first snapshot is 14 days old. The fourth line says
  "not yet" until Phase 6.
- **Checks.** `vitest`: all pass except `ThoughtStrip.test.tsx` (fails on
  untouched master). `tsc`: one error in `AgentRun.tsx` (`messageId`), also on
  untouched master. `cargo test --lib`: 7 stable failures
  (`engines::acp`, `engines::approve`, `engines::codex`, `agent::checkup`) and
  2 to 3 flaky `db::tests::*fork*` tests (millisecond timestamp ties), all also
  on untouched master; every new test passes.

### Phase 1 notes

- **EPI-1.** Schema v43. `conversations` gains `goal`, `outcome`, `task_kind`,
  `digest`, `worked`, `went_wrong`, `signals_json`, `candidates_json`,
  `encode_version`, `consolidated_at`; `memory_usage` gains `chances`, `helped`,
  `hurt`, `last_outcome_at`. New `db/episodes.rs` (`Episode`, `Credit`,
  `save_episode_rules`, `save_episode_draft`, `set_episode_candidates`,
  `episodes_missing_vector`, `add_memory_credit`, `memory_credit_map`) and
  `db/episodes_tools.rs` (`last_tool_ok`, `tool_results_between`,
  `carry_context_to`). The episode vector is a `vectors` row with owner `episode`,
  ref = conversation id; `delete_conversation` removes it. The "counterpart of
  `missing_vector_texts`" is `episodes_missing_vector`; it is used by
  `last_time_cmd`, which embeds the query and up to 16 missing episodes in one
  call. Nothing embeds episodes in the background.
- **EPI-2.** New `agent/episode.rs`: `gather` (signals), `outcome`, `task_kind`,
  `accepts`. `reflect_pass` now: stamps, checks the `lessons` rung, writes the
  rules and runs credit (`encode_by_rules`, no model) when the rung is not off,
  then goes on as before. The draft call returns `goal`, `digest`, `worked`,
  `went_wrong` and `candidates` (each with `evidence` and `cue`); the old
  `lessons` key is still read. All candidates go on the row with a status
  (`saved`, `proposed`, `held`). A candidate acts at once, through today's critic
  and rung code, only when the conversation has a fail-then-fix pair, a user
  correction signal or a rewind. Saved lessons carry `cue` (new optional `Fact`
  frontmatter field).
  **Deviations and limits:**
  - `TaskKind` is by rules only (no model call), as the plan's table says. Order:
    mail tools, then code tools, then a document artifact (`report`), then an
    attached folder (`code`), else `unknown`. The plan lists "code tools or a
    folder" together; the artifact rule is placed between them so a report made in
    a project folder is a `report`.
  - Outcome: a last user turn that is a short thanks is an accept, not an
    unanswered question. Without one: rewound or unanswered is `abandoned`, last
    tool or last check failed is `failed`, corrections without a thanks is
    `partial`, fewer than two messages is `unknown`, else `done`.
  - Signals do not read "decisions with outcomes" (no per-chat query exists); the
    other signals in III.1 step 1 are read. Phase 8 can add it.
  - A conversation with failures but no fix, no correction and no rewind keeps
    its candidates on the row; before this phase the same conversation could save
    a lesson. They wait for the daily tidy-up (Phase 4). The repo has no eval
    fixtures for lesson output, so "does not drop" is covered by unit tests of the
    gate only.
  - Credit runs once per conversation and encoding version, so "Reflect now"
    again does not count answers twice. Answers added after the first reflection
    are not credited.
- **CRD-1.** `ContextRefs` has `why` (name to cue) and `fired`, both empty until
  Phase 2 and 5. Finding: **scheduled runs wrote no messages at all**, so they
  had no `context_json` and could not be read back in the rail. `run_custom_job`
  now writes the prompt and the answer before the run (`open_run_turn`) and
  finishes the answer with its `context_json` after (`close_run_turn`);
  `PromptInputs.context_refs` carries the record out of `gather`. Resumed runs:
  the new answer inherits the interrupted answer's record (`carry_context_to`),
  and `finalize_message` no longer wipes a stored record when it is given none.
  Also fixed on the way: `render_entry` never wrote `origin`, so "learned when
  you went back" was lost on disk; it writes `origin` and `cue` now (test).
- **CRD-2.** New `agent/credit.rs`. Per conversation, each fact and lesson in an
  answer's `context_json` gets a chance. A lesson with `cue: tool:<name>` is
  helped if the tool ran after that answer and before the next one without
  failing, hurt if it failed or the answer was gone back on, no effect if it did
  not run. An entry named by an `outdated` or `not-relevant` command trace
  (written by CUE-UI-1, Phase 2) is hurt on the last answer that carried it.
  Entries since forgotten get no row back.
  **Limits:** a fact that was only in the prompt is a chance and not a help (no
  "used in the answer" signal exists, and none is guessed). Lessons are credited
  only when they were retrieved by meaning (embedder on): without one they are
  injected wholesale and `context_json` has never recorded them, same as before.
  Old lessons have no cue, so only lessons written from now on can be helped or
  hurt by a tool. Skill, fired-lesson and "Last time" credit wait for their
  phases.
- **EPI-UI-1.** `last_time_cmd` (new `commands/episode.rs`): first message of a
  new chat, an episode that ended `done`, known task kind, under 180 days, cosine
  0.62 or more (the lesson floor is 0.58). The composer shows one line in the
  hint slot ("Last time (12 Oct) I made ... **Do it the same way** · **Start
  fresh**"); the existing `SEE-3` hint wins the slot. It is asked after an 800 ms
  pause, at most three times a conversation, and not again once taken up or put
  away. "Same way" sets the `lastTime` turn modifier (chip "Same way as 12 Oct"),
  and `sendMessage` adds the digest, the skill name and the conversation id to
  that message's copy for the model. The system prompt does not change, so the
  prompt golden does not move. While the line shows, `Introduction` hides its
  "Recently learned" line (`lastTimeOffer` in the store). It needs the embedder;
  without one it shows nothing. "Start fresh" is not yet written as credit.
- **Checks.** `vitest`: all pass except `ThoughtStrip.test.tsx` (untouched
  master fails too). `tsc`: only the old `AgentRun.tsx` error. `cargo test --lib`:
  the same 7 stable failures as before and the flaky `db` fork tests; every new
  test passes. `cargo clippy` shows nothing in the touched files.
- **Felt checks not run here.** The accept item "do a report task, finish it,
  start the same kind of task a day later in a new chat: the hint appears" needs
  the app, a chat model and an embed model. It was not run in this environment.

### Phase 2 notes

- **Pulled forward from Phase 3 (BLF-1, minimum).** The Outdated accept item
  needs a replaced belief to leave the prompt and still be readable, so `Fact`
  gained `confidence`, `status` (`active | contested | superseded`),
  `supersedes` and `superseded_by` now. `render_entry` writes them only when
  set. `list_in` hides superseded entries, so every list that builds a prompt,
  the vector backfill, scope backfill, FTS and the counts skip them with no
  other change; `list_superseded` reads them. `MemoryStore::supersede_fact`
  saves the new answer (with `supersedes`), marks the old file, drops its vector,
  and takes the new one back if the old cannot be marked. Left for Phase 3:
  `evidence`, `valid_from`, the relation check at the write sites (BLF-2),
  usefulness in `prune_lessons` (BLF-3), the Memory and Lessons tab sections
  (BLF-UI-1), and `Vitality` counting only active (it does, through `list_in`;
  the growth-stage rule on the frontend still needs a look).
- **CUE-1.** `recall_with_cues` (`recall_for_project` is now a wrapper). Topical
  facts and lessons are over-fetched four times, ranked by similarity times a
  usefulness factor, and cut to the old limits (5 facts, 3 lessons); the
  similarity floor still decides who is eligible. The factor is 1 for an item
  with under 3 chances and twice its usefulness otherwise, so a neutral record
  changes nothing. Global facts keep their wholesale place and, under the cap,
  the most useful come first (also on the no-embedder path). A topical fact
  whose name or description carries a capitalised word from the message, or an
  entity from the chat's session notes, is added behind the ones that matched by
  meaning (`Cues`, `names_in`). `recall_for` takes the conversation id for the
  notes. Exclusion: `Fact::in_prompt` (not superseded, not kind `intention`) in
  recall and `index_markdown`.
  **Deviations:** the plan's third cue, the task kind of the conversation, is not
  used: no memory entry carries a task kind to match it against. `fires_on`
  lessons are not excluded yet: the field arrives in Phase 5. A name at the
  start of a sentence counts as a name; the fact must carry the same word, which
  keeps false hits rare.
- **CUE-2.** `RecalledEntry.why`, `SearchHit.why`, `ContextRefs.why` (both prompt
  paths: the backend `gather` through `ContextRefs::for_turn`, the chat window
  through `buildContextRefs`). Reasons: "you named Anna", "X is in this chat's
  notes", "same project", else "about check header" (the entry's name). Matches
  also carry the source conversation (when it still exists) and the date.
  "What I'm working from" shows the reason at the end of the entry's line for a
  recorded answer.
- **CUE-3.** An entry with `confidence: low` or `status: contested` gets
  `[unsure, noted DATE]` or `[contested, noted DATE]` at the end of its index
  line, and the memory block gets one sentence ("A note marked [unsure] or
  [contested] may be wrong: say so, and give its date, when you rely on it.")
  only when a line has the marker. `context.rs`, `store.ts`, the fixture and the
  golden moved together; both gates pass. Nothing sets `low` or `contested` yet
  (Phase 3, BLF-2), so no real prompt changes today.
- **CUE-UI-1.** In the recall chip each remembered or learned entry shows its
  reason, a link to the chat it came from, and two text buttons.
  **Outdated** (facts only) asks "What is right now?", then
  `outdate_memory_fact_cmd` replaces the fact (above) and writes an `outdated`
  command trace. **Not relevant here** writes a `not-relevant` trace. Both
  traces are read by `user_signals` (so reflection sees them) and by credit
  (hurt on the last answer that carried the entry). The Memory tab lists replaced
  facts under "Earlier versions" (`list_earlier_versions_cmd`).
- **Gate.** `recall_precision` and `stale_per_100` in `LearningNumbers` are
  computed now: precision is the share of recalled entries nobody pressed
  Outdated or Not relevant on; stale is Outdated presses per 100 recalled facts;
  both are `None` under 10 recalled entries in the window. The Phase 0 snapshot
  has `None` for both, so "precision must have risen against Phase 0" can only be
  judged against the first snapshot taken from now on, after some weeks of use.
- **Checks.** `vitest`: all pass except the old `ThoughtStrip` failure. `tsc`:
  only the old `AgentRun.tsx` error. `cargo test --lib`: the same 7 stable
  failures and the flaky `db` fork tests; every new test passes; clippy shows
  nothing in the touched files.
- **Felt check not run here.** "Say I live in Hamburg, press Outdated on the
  recalled fact, type Berlin: the next recall says Berlin and the Memory tab
  shows Hamburg under Earlier versions" needs the app. The backend half is
  covered by `pressing_outdated_replaces_the_fact_and_counts_against_it` and
  `a_belief_the_user_called_outdated_is_replaced_not_deleted`.

### Phase 3 notes

- **BLF-1 (rest).** `Fact` gained `evidence` (conversation ids, oldest first, at
  most 20, written as one `evidence:` line) and `valid_from` (set on a belief
  that replaced another). `save_in` seeds `evidence` with the source conversation,
  so every new entry has its first source. Old files read as no evidence and are
  not rewritten. Growth stage (G10): `Vitality` counts through `list_in`, which
  hides superseded entries, and the trash is outside the folders, so the mark
  never grows from corrections; nothing more was needed on the frontend.
  `Vitality.unsure` counts contested facts for the Self header strip
  ("... 2 I'm unsure about").
- **BLF-2.** New `memory/belief.rs`. `MemoryStore::save_checked(fact, by_user)`
  holds a new fact against the top 5 related active facts and does one of:
  `Merged` (same belief: the old entry gains the conversation as evidence, nothing
  is written), `Replaced` (the new text extends the old one, or the person is the
  newer source and the answer differs: the old file stays as an earlier version,
  the new one carries `supersedes`, the old evidence and `valid_from`), `Contested`
  (the answer differs and the person is not the source: both marked `contested`,
  both still reach the prompt with the Phase 2 hedge marker) or `New`. A failed
  mark takes the new entry back, so two silent actives cannot happen. The three
  write sites: the `memory` tool (`by_user` is true in any turn the person is in,
  false in an unattended run), `remember_fact_cmd` (always `by_user`; its reply
  says `outcome` and `other`), and reflection's recurrence check, which adds the
  conversation to the lesson's evidence next to today's bump. `MemoryStore`
  also got `unsure_groups`, `resolve_contest` (keep one side, or keep both as
  `topical`) and `undo_change` (trash the new belief, believe the old one again).
  **Deviations:** (a) `BeliefRelation` is **not** a decision purpose yet. The
  rules fallback the plan names runs instead: word overlap over name and
  description (Jaccard 0.3 or more, same project or either shared) says "about
  the same thing", and the text says same (equal, or the old one already says
  more), refines (the new text contains the old) or differs. It does not use
  vectors: a new entry has no vector at write time and the write site must not
  wait on the embedder. A decision model would sharpen "differs" into
  `contradicts` or an addition; until then an addition worded differently is
  marked contested or, from the person, replaces the old note. Both are one tap
  to undo (toast Undo, Earlier versions, Unsure). (b) Lessons only get `same`
  (today's look-alike check) and `unrelated`; there is no contradiction check
  between lessons.
- **BLF-3.** `prune_lessons` uses `prune_order`: first lessons with 5 or more
  chances and usefulness under one half, the least useful first; then lessons with
  no chance (surfaced or written) in 120 days, the longest idle first; then the
  oldest. **Refinement:** in the last tier a lesson that has proved itself (5 or
  more chances, usefulness one half or more) goes last, so a good old lesson is not
  trashed before a new unproven one. The activity line stays "pruned an old
  lesson: X".
- **BLF-UI-1.** Memory tab, top to bottom: the profile, the pending tidy-up block
  (unchanged; the daily job's rows and "Not this one" are Phase 4), **Recent
  changes** (the last six memory or lesson lines from the activity log, in the
  first person, `list_recent_changes_cmd`; no Undo yet, since the tidy-up that
  needs it is Phase 4), **Unsure** (both sides with their sources, "This one is
  right" per side, "Both, in different situations"; a lone doubt gets "Still
  true"), the fact cards, **Earlier versions**, **Let go** (`.trash/`, searchable,
  "Bring it back", `list_let_go_cmd`). The Lessons tab gets Recent changes and Let
  go for lessons. `Evidence` replaces "Where this came from" on cards that have a
  list (3 shown, "and N more"; a chat that is gone says so); old files keep the
  single link. **Toast (IV.9):** `/remember` and the `memory` tool say "I replaced
  what I knew with ..., the old one is under Earlier versions" or "I'm not sure
  which is right about ..., it's under Unsure", with Undo (the old note comes
  back); a repeat says "I knew that already". **Introduction (G11):** the
  "Recently learned" line reads lessons and facts, a replaced fact by its
  `valid_from`, and skips contested ones; the store keeps `facts` for it
  (loaded with `refreshSelf`). Not built: "helped N times" on cards (facts get no
  helped credit, see Phase 2 limits), certainty in words (Phase 8), "Things I'll do"
  (Phase 7).
- **Checks.** `vitest`: all pass except the old `ThoughtStrip` failure. `tsc`:
  only the old `AgentRun.tsx` error. `cargo test --lib`: 1592 pass; the same 7
  stable failures and 2 flaky `db` fork tests fail as before; clippy shows nothing
  in the touched files. The prompt golden did not move: nothing sets `low`
  confidence, and contested entries use the Phase 2 marker already in the golden.
- **Felt check not run here.** "Save 'prefers tea', then 'prefers coffee'" through
  the real `memory` tool needs a chat model; the store-level behaviour is covered
  by `a_contradiction_from_the_user_replaces_and_keeps_the_old_version` and
  `a_contradiction_from_anyone_else_marks_both_contested_never_two_silent_actives`.
  The write-site wiring itself (the `memory` tool and `remember_fact_cmd`) has no
  unit test, because both need a full tool context or app state; it compiles and
  is a thin layer over `save_checked`.

### Phase 4 notes

- **SLP-1, the code.** New `commands/tidy.rs` holds the pass. Its rules need no
  model: **promotes** a held candidate (Phase 1 left every candidate without hard
  evidence as `held` on its episode) once the same thing is seen in 2 or more
  conversations, matched with the wording check reflection already uses
  (`lessons_look_alike`), from a pool reaching back 180 days; a group that matches
  an existing lesson only adds the sources it lacks. **Merges** duplicate notes
  (facts by `belief::relation == Same`, lessons by the wording check): the older
  survives, the newer folds in, and its evidence joins the survivor. **Lets go** of
  an item with 5 or more chances and usefulness under 0.3 and, for lessons, of one
  with no chance in 120 days. `Consolidation` gained `promotes`, `contests`,
  `lesson_deletes` and a `collection` on merges; `Consolidation::absorb` joins a
  second pass to the pending one without doubling a row; `apply_proposal` makes
  the changes and counts what really happened (`Applied`), which is all the digest
  may claim. Applying with the snapshot and the `GLD-2` check moved into
  `apply_guarded`, shared by the Apply button and the daily pass; it keeps the
  snapshot name for Undo.
- **One tidy-up.** `consolidate_memory_cmd` (Tidy up and `/tidy`) now runs the same
  rules over everything, then the model's own look (the old prompt, which now also
  asks for sets of notes that cannot all be true: `contests`; names it invents are
  dropped), then skill drafting. With no model loaded it still returns what the
  rules found. The daily job's step 3 runs the rules over up to 5 conversations
  that no tidy-up has folded in (`consolidated_at`), and a group is promoted only if
  one of those is in it. What `consolidate` allows decides the landing: `ask`
  adds the rows to the one pending proposal (Memory tab), `auto` applies them with
  snapshot and golden check, `off` skips the step. `consolidate` now offers all
  three rungs (default still `ask`). The Memory tab's pending block shows every kind
  of row (keep, merge, edit, unsure, drop, let go), each with **Not this one**
  (`drop_pending_row_cmd`).
- **Idle start, stop on input.** The app reports use through `useReportUse`
  (`note_user_input_cmd`): a key or click at most every 2 s, and a beat every 30 s
  while a turn streams. A due built-in pass waits until the app has been quiet for
  2 minutes (launching counts as use); your own tasks run when due, as before. A
  key or click ends a pass that started by itself after the item in flight (the
  reflection call or the stage in progress); it never ends one started with Run now.
  A pass ended that way is due again at once, so the next quiet moment picks up the
  rest instead of waiting a day. `poiesis-tending {on}` (from the pass and from Tidy
  up) moves the mark to `tending`, and back to whatever is underneath.
- **True words.** The built-in job is "Daily tidy-up", with the toggle "Do this
  once a day" and the plan's description. The id `nightly-reflection` stays; a job
  stored under the old name is shown under the new one.
- **SLP-2.** `skill_groups`: finished (`done`) conversations of one known task kind
  whose "what worked" lines read alike (word overlap 0.4), 2 or more, not used
  before. One draft per pass, written by the local model with the `/skillify` ask
  put to what they did, and filed as a `skill` proposal (the `skills` rung is `ask`,
  so a skill never keeps itself); a name that is already a skill or already waiting,
  an empty draft or one over 8000 characters is dropped. The source conversations
  are remembered, so they do not draft again.
- **SLP-3.** The digest is composed from the pass's own counts: lessons learned
  (reflection plus promotions), notes merged, old ones let go, lessons that gained
  another source, rows and skills waiting for a yes ("changes I'd like to make"), and
  a tidy-up the golden check undid ("so I put it back"). It says "before you stopped
  me" if input ended the reading. Tasks shows **Show me** (Self, Memory tab) and, when
  the pass applied changes on its own, **Undo** (`undo_last_tidy_cmd` restores the
  snapshot).
- **Deviations.** (a) `CandidateGroup` and `BeliefRelation` are not decision
  purposes yet; the rules fallback runs (as in Phase 3). (b) `contests` come only
  from the model's look during Tidy up. The daily pass finds none: two notes that
  differ in wording cannot be told apart from an addition by rules, and Phase 3
  already catches them when they are written. (c) The model's edit/merge look runs
  only on demand, not in the daily pass: it reads every note, and the plan feeds the
  daily pass only the new conversations. (d) **Default on is not changed.** The plan
  makes "on by default" a decision for Erich; the seeded job is still off, so a
  fresh install does nothing until "Do this once a day" is ticked. (e) A fact is
  never let go for idleness, only for testing badly: most facts are rarely
  surfaced by design. (f) "Recent changes" has no Undo for the pass's own changes;
  the digest's Undo restores the whole snapshot.
- **Checks.** `vitest`: all pass except the old `ThoughtStrip` failure. `tsc`: only
  the old `AgentRun.tsx` error. `cargo test --lib`: 1610 pass; the 7 stable failures
  and 1 flaky `db` fork test fail as before; clippy shows nothing in the touched
  files. The prompt golden did not move.
- **Felt check not run here.** "Three conversations with the same small
  correction, leave the app idle: the digest says one thing was learned, and the
  Memory tab shows one belief with 3 evidence links" needs the app and a model. The
  pipeline after reflection is covered by
  `rules_find_a_promotion_a_merge_and_a_forget_in_a_real_store` (three held
  candidates become one lesson with three sources, a duplicate folds in, nothing is
  left to say the next night). The idle gate, stop on input and the ticker are
  covered at the unit level (`SchedulerState`), not with a running app.

### Phase 5 notes

- **HAB-1, firing.** A lesson file may carry `fires_on: <tool>`, `fires_when:
  <words>` and `check: <id>` (`memory::Fires`, one optional field on `Fact`). Such
  a lesson is out of the index and out of relevance recall (`Fact::in_prompt`), so
  it speaks at its tool and nowhere else. New `agent/habit.rs`: `TurnCtx.firing`
  reads the firing lessons once when a run starts; after each call `dispatch_batch`
  asks `habit::speak`, which adds the lesson body (cut to 600 characters) to the
  result the model reads, for a failed call as well as a good one. The words in
  `fires_when` must be in the call's arguments (any case). A run records each
  lesson that spoke once (`RunHandle.fired`); the chat path sends the names with the
  answer's `context_json` (`ContextRefs.fired`, collected from the new `fired` event
  in `store.ts`) and a scheduled run writes them from the handle. Credit counts a
  fired lesson as a chance on that answer and judges it by the tool it fires on
  (`agent/credit.rs`).
- **HAB-2, checks.** Five checks, fixed: `header_row`, `path_exists`,
  `file_nonempty`, `json_parses`, `row_count`. They run after a good call, on the
  file the call named (`path`, `file_path`, `file` or `filename`; a relative path
  hangs off the working folder as the file tools do, and with no folder the check
  is skipped). A failed check adds one line to the result ("Check failed: ... Fix
  this before you go on.") and says so in the step line. No model-written code runs.
- **HAB-3, the escalation.** At recurrence 3 (`commands/reflect.rs`), when the
  lesson's cue is `tool:<name>` and this conversation fixed a mistake with that
  tool, reflection files a `habit` proposal instead of the standing-instruction one.
  The card (`HabitProposal` in chat, the same card on the Lessons tab) has three
  buttons: **Check it first** (sets `fires`, and a check chosen from the error text
  by `habit::check_for`), **Make it a standing instruction instead**
  (`make_standing_instead_cmd`: adds the lesson's one line to SOUL through the same
  `GLD-2` guard as any soul edit, and is refused when `soul` is off or the SOUL is
  full) and **Not now**. The soul branch of `resolve_change_proposal_cmd` moved into
  `apply_soul` so both paths share it. A lesson that was offered once does not get a
  second habit offer; if it recurs again the old standing-instruction proposal comes
  as before (unless a habit card is still waiting).
- **HAB-4, scripts in proposed skills.** `propose_skill` takes `files` (at most 3,
  `scripts/<name>.py` or `.js`, 20 000 characters each). A file is kept only if it
  is, word for word (line endings and trailing space aside), the `code` of a
  `run_code` call in this conversation that finished clean (`log::clean_scripts`:
  no error, no non-zero exit, not stopped for time). The files wait in the setting
  `proposal_files:<proposal id>` and are written beside `SKILL.md` when the person
  says yes; a no removes them. The path is checked again at the write.
- **RHS-1, rehearsal.** In `agent/golden.rs`: `build_rehearsal` finds the run that
  made a failed call (`Db::run_ids_in`) and rebuilds the messages with `log::replay`
  up to, not including, that call. `rehearse` asks twice per arm (temperatures 0.2
  and 0.6), without the candidate and with it in the system prompt, reads the first
  tool call and never runs it. `is_right` is exact against `fixed_args`, false
  against `failed_args`, and otherwise `RehearsalMatch`: right when it agrees with
  the fixed call on more of the arguments that differ between the two. Verdict:
  `helped` (right 2 of 2 with it, wrong at least once without), `hurt` (right less
  often with it), else `no_effect`. `rehearse_with_model` runs it against the loaded
  or chosen model through the tool table the golden set uses.
- **HAB-UI-1.** A step that fired a lesson opens to one plain line, "Checked the
  header row. You showed me on 4 Oct." (`fired` on `AgentStep`; the step is
  expandable, and nothing shows in the stream). The Lessons tab says "I check this
  before read file, and afterwards I look at the header row." on a lesson that
  fires.
- **Deviations.** (a) The words a call must carry are `fires_when`, not `cue`: the
  existing `cue` already means "tool:<name>" or situation words for credit, and
  reusing it would make the two clash. (b) The note reaches the model with the
  result, so after the call it is about; it helps the retry and the next call, not
  the first one. Changing the arguments of a call (`rule`) is not built: the plan
  leaves it to the person on the lesson card, and no card edits it yet. (c) Nothing
  sets `fires_when` today; it is read from the file (hand edit). The habit card sets
  the tool and the check only. (d) Only `run_code` counts as a script that ran
  clean; a script written with `write_file` and run another way does not. Skills
  drafted by the tidy-up (`SLP-2`) carry no scripts. (e) **Nothing calls
  `rehearse` yet.** The plan uses it in IMP-3 (Phase 6). It is tested with a
  stand-in model; `rehearse_with_model` has not run against a real one. (f) The
  habit offer follows the `lessons` rung (off = not offered) and is always a
  proposal, never applied by itself.
- **Checks.** `cargo test --lib`: 1635 pass; the 7 stable failures (checkup engine
  line, acp x2, approve, codex x3) and the flaky `db` fork test fail as before. `vitest`: all pass except the old
  `ThoughtStrip` failure. `tsc`: only the old `AgentRun.tsx` error. Clippy shows
  nothing new in the touched files. I also marked `save_digest` and
  `compose_digest` in `scheduler.rs` as test-only: Phase 4 left them unused in the
  real build.
- **Gate not measurable here.** "Repeat-mistake rate must have dropped against
  Phase 0" needs weeks of real use. The number is on the Health tab ("Mistakes I
  repeated"). Read it before Phase 6 starts.
- **Felt check not run here** (the plan gives none for this phase beyond RHS-1,
  which the unit test covers). To try it by hand: let a CSV step fail and get
  fixed in three chats, accept "Check it first", then open the step of the next
  CSV call and read its line.

**What v2 changes.** v1 described a second memory system beside the one that
exists: a new `episodes` table, a new `memory_uses` table, a new
`memory/habits/` store, a new `intentions` table, a new `improvements` table, a
new rehearsal sandbox and a new "Memory page". All of these already have a home
in the code. v2 is an **extension plan**: every piece below names the existing
function, table, file or component it extends, and Part 0 is the map of what
exists. **No new store** is added unless the table in Part II says why no
existing one can carry it. Today the answer is: none needs to be added.

Unchanged from v1: the goal (real self-learning, not "reflection writes
lessons"), the human-memory model, the flaws we design out, the Harness²
improvement loop, and measurement before building.

Rules for every task here:

- The self is plain markdown the user owns (`memory/`, `MemoryStore`). SQLite
  holds only derived data (outcomes, counts, scores), never something the user
  wrote.
- Nothing is destroyed. Forget means `.trash/` (`forget_in`), consolidation
  snapshots first (`snapshot`), unreadable files go to `.quarantine/`. Every
  self-write is visible and reversible.
- The existing autonomy classes (`AUTONOMY_CLASSES` in `store.ts`, `autonomy.rs`)
  gate every self-change. **No new class** is added; two existing classes gain
  a rung (Part II).
- UI copy is first person (`PRES-0`), dialogs keep a fixed size, shared
  components are reused, no "dream" or "brain" words.
- The chat stream stays quiet: an answer plus at most one plain line. Anything
  new in a conversation goes inside an existing collapsed step or chip, never a
  new row type.
- Background work is local by default and is cancelled by user input.
- Any change to prompt text goes through the prompt golden gate
  (`context_golden.rs`) and its mirror in `store.ts`.

## What this plan uses from the Dual-process plan

This plan uses the **decision model** (the `agent/decide` seam), never the
`fast-slow` experiment. Nothing here needs **Fast & slow thinking** switched on.

- **No experiment flag.** Every purpose this plan adds is its own row in
  `policy.rs` with no `experiment` named, like the `DEC-6` sites that already
  run (`MemScope` in `memory_skill.rs`, `skip_reflection` in `reflect.rs`). The
  autonomy classes are this plan's only switches.
- **The purposes, all background, all with a fallback:**

  | Purpose | Kind | Options / levels | Extends | Without a decision model, `unsure`, or confidence under 0.6 |
  |---|---|---|---|---|
  | `TaskKind` | `Choice` | `report`, `code`, `mail`, `files`, `research`, `writing`, `chat`, ... plus `unsure` | the reflection pass (`reflect_conversation_cmd`) | Rules from what the run used: mail tools → `mail`; code tools or a folder → `code`; a document artifact → `report`; else `unknown`. `unknown` is never shown as "Last time" |
  | `BeliefRelation` | `Choice` | `same`, `refines`, `contradicts`, `unrelated`, `unsure` | the three write sites (III.4) and `lessons_look_alike` | Today's `lessons_look_alike` for lessons; similarity over `SEM_FLOOR` for facts marks both `contested`, never two silent actives |
  | `CandidateGroup` | `Choice` | ids of existing beliefs and other conversations' candidates, plus `new` | `consolidate_memory_cmd` | Similarity over `SEM_FLOOR` groups; under it, `new` |
  | `RehearsalMatch` | `Score` | `repeats the mistake` .. `matches the fix` (5 levels) | the GLD check loop (`golden.rs`) | Only the exact args comparison; the rest is `not_run` |
  | `RecallUseful` | `Noul` | "Did the answer use this item?" | credit at reflection (III.3) | `unknown` |

- They run on the processor, wait for the engine, and go to the cloud only on
  their own override (decision 12 of the Dual-process plan).
- **UI pieces are reused, not their gates.** "Last time" uses the `SEE-3`
  composer hint component with its own trigger. `sureWord` (`SEE-4`) is used
  as a plain formatter.
- **Unproven seam.** No real model has answered a decision yet (Dual-process
  step 20). Each purpose ships with a seed set under `tests/eval/decide/` and is
  used uncalibrated only with the stricter bar.

---

# Part 0 - What exists today (the map this plan extends)

Everything in this part is in the code now. File names are relative to
`src-tauri/src/` for Rust and `src/` for the frontend.

## 0.1 Stores

| Store | Where | What it holds | Notes |
|---|---|---|---|
| Facts | `memory/facts/*.md`, `MemoryStore` (`memory/mod.rs`) | `Fact`: name, description, kind (`preference`/`fact`/`decision`/`project`), created, `source_conversation`, body, `scope` (global/topical, `SCP`), `expires_at` (`TTL`), `project` (`PRJ-8`) | Optional frontmatter with `#[serde(default)]`: new fields are cheap |
| Lessons | `memory/lessons/*.md`, same struct, kind `lesson` | plus `recurrence`, `last_seen` (`RPT-1`), `origin: rewind` (`CPX-5`) | `LESSON_CAP` 40, `prune_lessons` trashes the **oldest** |
| Standing instructions | `memory/SOUL.md` | user-owned, agent only proposes | `SOUL_CAP` 1500 |
| Profile | `memory/PROFILE.md` | the agent's synthesis of how you like to be talked to (`PRO`) | untitled prose on the Memory tab (`SMP-5`) |
| Index | `memory/MEMORY.md` | generated, `index_markdown` | wholesale fallback when no embedder |
| Trash / quarantine / snapshots | `memory/.trash/`, `.quarantine/`, `.snapshots/` | `forget_in`/`restore_trash`, `quarantine_scan`, `snapshot`/`restore_snapshot` | trash has no browsing UI, only the undo toast |
| Memory FTS | `memory_fts` | rebuilt wholesale on every write (`sync_fts`) | |
| Memory vectors | `vectors`, scope `memory`, collection facts/lessons | name + description embedded (`missing_vector_texts`) | |
| Last surfaced | `memory_usage(collection, ref_key, last_used_at)` | when a fact last reached a prompt (`SEM-UI-4`) | **only a timestamp**, written by `touch_memory_usage` |
| Per-answer context | `messages.context_json` (`ContextRefs`) | persona, soul present, profile present, **facts and lessons that reached this answer**, files | read only by the "What I'm working from" panel today |
| Conversation | `conversations`: `summary` (rolling, compaction), `session_state_json` (entities / constraints / decisions, written by the `remember` tool in `present.rs`), `reflected_at`, `project_id`, `persona_id`, `parent_conversation_id` | | already most of an episode |
| Session log | `session_events` (`agent/log.rs`) | every message a run saw, rewinds, command traces; `replay(db, run_id)` rebuilds the message array | `user_signals` and `was_rewound` read it |
| Tool evidence | `tool_stats` (per model and tool, ok/fail, conversation), `tool_fixes` (failed args, error, fixed args) | | `FIX-2`, `HEAL-2` |
| Skill outcomes | `skill_runs(skill_name, conversation_id, tool_failures, corrected)` | written by `record_skill_run` in `skillpack.rs` | read by `propose_skill_revisions` (`OUT-2`) |
| Proposals | `change_proposals(target, slug, proposed_text, rationale, description, status)` | targets: `soul`, `lesson`, `lesson-critic`, `skill`, `skill-revision`, `project_instructions`, `engine-memory`, `email`, `app` | the one queue for every self-change |
| Decisions | `decisions`, `decision_outcomes` (`CTL-3`) | prediction before outcome | |
| Verifications | `verifications` (`USE-10`) | per answer: claims and verdicts | |
| Activity | `activity_log` | one line per self-change (`reflect`, `memory`, `scheduler`) | |
| Pending tidy-up | setting `PENDING_CONSOLIDATION` | the last `Consolidation` proposal | |
| Project memory | `projects.instructions` (`PRJ-7`), `propose_project_instructions` | | the existing "workspace facts" |
| Persona skills | `personas.skills_json` (`SKL-6`) | a persona narrows the advertised skills | the existing "setup per kind of work" |
| CLI agent learning | `engine_rules`, `engine_signals` (`RTE-6`), `engine-memory` proposals | | out of scope here, but its proposals land in the Lessons tab |

## 0.2 Who writes memory

| Writer | Code | Gate |
|---|---|---|
| `memory` tool (save / update / forget / read) | `agent/memory_skill.rs` `memory_op`; scope by `decide_scope` (`MemScope`) or `classify_scope` | `facts` (auto / off) |
| `propose_soul_edit` tool | `memory_skill.rs` | `soul` (ask / off) |
| `propose_skill`, `propose_project_instructions` | `agent/skillpack.rs` | `skills` (ask / off) |
| `/remember`, `/forget`, `/always` | `remember_fact_cmd`, `forget_memory_fact_cmd`, `set_soul_cmd` (`commands/memory.rs`) | user |
| Memory tab edit / delete / scope | `update_memory_fact_cmd`, `forget_memory_fact_cmd`, `set_fact_scope_cmd` | user |
| Reflection | `reflect_conversation_cmd` (`commands/reflect.rs`): draft ≤3 lessons → critic (`CRT-1`) → recurrence bump (`RPT-1`) → soul escalation at 3 (`RPT-2`) → save or propose; then `propose_skill_revisions` (`OUT-2`) | `lessons` (auto / ask / off), `soul`, `skills` |
| Tidy up | `consolidate_memory_cmd` → pending proposal → `apply_consolidation_cmd` (snapshot + `GLD-2`) | `consolidate` (ask / off) |
| Profile | `rebuild_profile_cmd`, `edit_profile_cmd` | |
| Expiry | `sweep_expired` (at launch and in the daily job) | |
| CLI agents | `engines/tools.rs` → `engine-memory` proposal | user |

## 0.3 Where memory reaches a turn

| Reader | Code | Notes |
|---|---|---|
| Prompt assembly (backend) | `agent/context.rs` `gather` → `commands::memory::recall_for` → `MemoryStore::recall_for_project` | global facts wholesale, topical facts top 5 and lessons top 3 over `SEM_FLOOR`, project-fenced |
| Prompt assembly (frontend, `agent_chat_cmd`) | `store.ts` `recallForPrompt` → `api.recallFor` → same `recall_for` | the one path still assembled on the frontend |
| Recall chip in the timeline | `matches` on the step, `Timeline.tsx` `chipLabel` ("◆ remembered", "◆ learned"), `Provenance` | collapsed, `SEM-5`/`SEM-UI-2` |
| "What I'm working from" | `ContextPanel.tsx`, `context_manifest_cmd` (`live_manifest`/`historical_manifest` from `context_json`), `/why`, `/context`, the Self page link | layers: Soul, Persona, About you, Remembered, Learned, Procedures, From your files, Session |
| `search_history`, `read_conversation` | `agent/recall.rs` | raw messages only |
| `memory` op read | `memory_skill.rs` | |

## 0.4 Every place the user sees memory or self-learning

| Place | Component | Shows |
|---|---|---|
| The living mark | `Mark/PoiesisMark.tsx`, `Orb/orbState.ts` | `presence`: idle, active, **reflecting** ("reflecting on a past conversation", orb `weaving`), healing, **tending** ("tidying my head", orb `shaping`), listening; growth `stage` from facts + lessons + skills (5 / 20 / 50); slow pulse when a digest is unread (`SCH-UI-2`) |
| The big orb in an empty chat | `routes/Chat.tsx` | same presence |
| Rail chat row | `Rail.tsx` `ChatRow` | ◆ while reflecting (`reflectingIds`), ◆ after something was learned (`digestedIds`), "Reflect now" in the ⋯ menu |
| New conversation | `Conversation/Introduction.tsx` | first-run card (`PRES-6`), then one ambient line "◆ Recently learned: ..." from lessons of the last 7 days (`PRES-5`) |
| Toast | `Memory/MemoryToast.tsx` | save, forget, always, profile; undo (`poiesis-memory-write`) |
| Self page header | `routes/Self.tsx` | first-person narrative, the strip "N facts · N lessons · N skills · N pending", `GrowthRings` |
| Self page tabs | `Self/SelfPanel.tsx` | **Memory** (`MemoryPanel`: profile prose, Tidy up, Open folder, Export zip, search, fact cards with scope, "last surfaced", "Where this came from"), **Lessons** (pending proposals incl. critic and engine-memory, lesson cards with "learned N×", "learned when you went back"), **Health** (runtime restarts, last reflection, Reflect now, auto-reflect toggle, golden checks, tool health, quarantined files), **Autonomy** (one row per class) |
| Tasks page | `routes/Tasks.tsx` | the built-in `nightly-reflection` job, its toggle, and the digest |
| Slash commands | `shared/commands.json` | `/remember`, `/forget`, `/always`, `/reflect`, `/tidy`, `/self`, `/why`, `/context`, `/skillify`, `/checkup`, `/autonomy`, `/compact`, `/rewind` |
| Checkup | `agent/checkup.rs`, `/checkup` | first-person lines with one action each; the mark is `tending` while it runs |

## 0.5 The learning loops that already run

1. **Reflection** has five triggers, all into `reflect_conversation_cmd`:
   leaving a chat (`reflectOnLeaving`, 8+ messages, auto-reflect on); "Reflect
   now" on the Health tab and in the Rail menu; `/reflect`; launch catch-up
   (`scheduleCatchUpReflection` → `catch_up_reflection_cmd`, 2 per launch, 8+
   messages); the daily job (`run_nightly_reflection_digest`, up to 8).
2. **Critic** (`CRT-1..3`): a failed draft becomes a `lesson-critic` proposal.
3. **Recurrence** (`RPT-1/2`): the same lesson again bumps `recurrence`; at 3
   it proposes a SOUL line.
4. **Skill revision** (`OUT-1/2`): 3 of the last 5 runs rough → a revised
   `SKILL.md` proposal, written as an App-source copy.
5. **Tidy up** (`consolidate_memory_cmd`): one model call over the whole set →
   merges, edits, drops → user applies all, `GLD-2` around it.
6. **Golden checks** (`GLD`, `agent/golden.rs`): fixed behaviour contracts run
   through the model **without dispatching any tool**, around every
   self-change.
7. **Profile rebuild** (`PRO`), **expiry** (`TTL`), **scope backfill** (`SCP-3`),
   **lesson cap** (oldest out).

## 0.6 What the scan found that is wrong or missing today

These are fixed in Phase 0 because later phases build on them.

| # | Finding | Where |
|---|---|---|
| G1 | **The mark never shows background reflection.** `presence: "reflecting"` and the Rail ◆ are set only by the frontend `reflectConversation`. Launch catch-up and the daily job reflect with the mark idle and no Rail ◆ | `store.ts` `reflectConversation` vs `scheduleCatchUpReflection`, `scheduler.rs` |
| G2 | **The daily job does not use the 8-message floor.** It reflects any unreflected chat, including one-liners, and stamps them, while the other two paths skip them | `run_nightly_reflection_digest` vs `CATCH_UP_MIN_MESSAGES` / `REFLECT_MIN_MESSAGES` |
| G3 | **The digest always says "Last night"**, also when it ran at 3 pm after launch | `compose_digest` |
| G4 | **The Self narrative is broken**: "...and I keep I can follow N skills." | `routes/Self.tsx` `narrative` |
| G5 | **Lessons are forgotten by age.** `prune_lessons` trashes the oldest past 40, whatever their use | `memory/mod.rs` |
| G6 | **`context_json` is never used for learning.** It already records which facts and lessons reached each answer; nothing reads it except the panel | `ContextRefs` |
| G7 | **Lessons reach the timeline only as "◆ learned".** A retrieved lesson has no "why", and the user cannot say "not relevant" | `Timeline.tsx` `Provenance` |
| G8 | **Two tidy-ups that do not know each other**: the daily job reflects but never consolidates; Tidy up consolidates but never looks at conversations | `scheduler.rs`, `consolidate_memory_cmd` |
| G9 | **The trash cannot be browsed.** A forgotten or pruned entry comes back only through the 5-second undo or the folder | `MemoryPanel.tsx` |
| G10 | **Growth stage counts everything on disk**, so once superseded versions exist (Phase 3) the mark would grow from corrections | `PoiesisMark.tsx` `growthStage`, `organism.rs` `Vitality` |
| G11 | **"Recently learned" only knows lessons.** A merged or corrected fact never shows | `Introduction.tsx` |

---

# Part I - Why

## I.1 In one sentence

Poiesis stores a lot, but it does not remember how a piece of work went, it
does not know which of its memories help, it forgets lessons by age, and it
learns only from failure.

## I.2 The human model, mapped onto what exists

The key idea is **complementary learning systems**: a fast store that records
single experiences, and a slow store that only takes on what repeats. Poiesis
today skips the fast store: reflection writes straight into `lessons/` from one
conversation, so one bad moment can become a rule.

| Human system | Poiesis today | After this plan |
|---|---|---|
| Sensory buffer | Session log (`session_events`) | unchanged |
| Working memory | the turn's prompt; `session_state_json` (entities, constraints, decisions) | unchanged; the session state feeds the episode |
| Episodic memory | `conversations.summary` (only for compaction), `reflected_at` | **the conversation row becomes the episode** (new columns, III.1) |
| Semantic memory | `facts/`, `lessons/` | same files, with evidence and status (III.4) |
| Procedural memory | skills | skills, plus **lessons that fire in the tool loop** (III.5) |
| Prospective memory | scheduled tasks (`/schedule`) | plus **facts of kind `intention`** (III.8) |
| Metacognition | `tool_stats`, decision calibration | **self-model** in `Vitality` (III.7) |
| Sleep consolidation | the built-in daily job (reflection only) | the daily job also runs Tidy up (III.6) |
| Prediction error | `decisions` + `decision_outcomes` | weights credit (III.7) |

## I.3 The flaws of human memory, and the existing part that answers each

| # | Human flaw | Design response | Lands in |
|---|---|---|---|
| F1 | Source amnesia | Every belief lists the conversations that support it | `Fact.evidence`; "Where this came from" grows to a list |
| F2 | Recall rewrites the memory | A belief changes only by a new version; the old one stays readable | `Fact.status: superseded`, `superseded_by` |
| F3 | Confabulation | Low or contested beliefs are said as "I think", with a date | `recall_for` index line marker |
| F4 | Forgetting by decay | Forget by usefulness and contradiction, never by age alone | `prune_lessons` rule (G5) |
| F5 | Cannot forget on purpose | One conversation never makes a durable rule (except a hard fail-then-fix); one click forgets | candidates on the conversation row; existing Delete |
| F6 | Confirmation bias | New notes are checked against what is believed; contradictions are surfaced | `BeliefRelation` at the three write sites |
| F7 | Over-generalising | 2+ conversations before a belief, 3+ before a habit | promotion ladder in Tidy up |
| F8 | Hindsight bias | Prediction stored before the outcome | `decisions` (exists) |
| F9 | Recency bias | Rank by relevance times usefulness | `recall_for_project` ranking |
| F10 | Forgetting to remember | Intentions with explicit triggers | intention facts + scheduler |
| F11 | Bad habits stick | Every lesson and skill is scored on use and can be unlearned | `memory_usage` counters, `skill_runs` |
| F12 | Tip of the tongue | Several cues: words, meaning, entities, project, task kind | `recall_for_project` |
| F13 | Contexts bleed | Memory is scoped by project and says where it came from | `PRJ-8` (exists) + why line |
| F14 | Cannot look inside | Every recall visible with why; correct it right there | recall chip + "What I'm working from" |
| F15 | Cannot test a belief safely | Rehearsal: replay with the change, no tool runs | `GLD` runner + `log::replay` |
| F16 | Sleep is not schedulable | The daily job runs when idle, bounded, visible | `nightly-reflection` job |

## I.4 What we take from Harness² and HyperSkill

From **Harness²** (Google, Oct 2026): evidence verdicts (`helped`, `hurt`,
`no observed effect`, `confounded`; "nothing to learn" is a valid result); a
lesson carries a `cue` (the shape of the situation); prefer a mechanism over
prose (a check beats a sentence in every prompt); workspace facts are separate
from procedure; test a change by running it; and **two loops**: improve the
setup for the task (task level) and improve the process that makes the
improvements (improver level), via **Propose, Probe, Compose**.

From **HyperSkill** (Aug 2026): every memory item has a usefulness score from
its own track record and is pruned on it; learn from success and from failure;
retrieve few items (their best setting was 2); unmaintained memory is worse
than none.

Not taken: rerunning the user's task, ground-truth labels, hypergraph merging.

---

# Part II - The extension map

This is the core of the plan. Left: what v1 wanted. Middle: the existing part
that carries it. Right: what is added to that part. Nothing in the left column
gets its own new store.

| v1 concept | Extends | What is added |
|---|---|---|
| `episodes` table | **`conversations` row** + reflection pass | columns `goal`, `outcome`, `task_kind`, `digest` (≤600), `worked`, `went_wrong`, `signals_json`, `candidates_json`, `encode_version`, `consolidated_at`. Entities come from `session_state_json`, artifacts from `artifacts`, the narrative from `summary` when there is one. Vector in `vectors` (scope `episode`, ref_key = conversation id). Deleting a chat deletes it, by construction |
| `memory_uses` table | **`messages.context_json`** (what reached each answer) + **`memory_usage`** (per item) | `ContextRefs` gains `why` (name → cue in words) and `fired` (lessons fired at a tool); `memory_usage` gains `chances`, `helped`, `hurt`, `last_outcome_at` |
| Skill credit | **`skill_runs`** | nothing new: `tool_failures` and `corrected` plus the conversation's `outcome` are the credit |
| Belief evidence, status, versions | **`Fact` frontmatter** | `evidence` (conversation ids), `confidence`, `status` (`active` / `contested` / `superseded`), `supersedes`, `superseded_by`, `valid_from`, `cue` (lessons). Superseded files stay in their folder with `status: superseded`; `list_in` callers that build prompts skip them |
| "Archive" | **`.trash/`** | a "Let go" list on the Memory tab reading `.trash/` with the existing `restore_trash` (G9) |
| Contradiction check on write | **the three write sites**: `memory_op` save (next to `decide_scope`), `remember_fact_cmd`, and reflection's recurrence check (`lessons_look_alike`) | `BeliefRelation` over the top 5 by vector; fallback is today's behaviour |
| Night consolidation | **`run_nightly_reflection_digest`** + **`consolidate_memory_cmd`** | the daily job calls the same propose step as Tidy up, fed with unconsolidated conversations; one `Consolidation` shape gains `promotes` and `contests`; one pending proposal on the Memory tab |
| `memory/habits/` store | **`lessons/`** | a lesson with `fires_on: <tool>` (and optionally `check: <id>`) is left out of the prompt index and put into that tool call just in time. Same file, same tab, same `lessons` rung |
| `habits` autonomy class | **`lessons`** class | none; a `check` needs the `lessons` rung at Ask or Auto, a `rule` always needs the user |
| RPT-2 soul escalation | **`reflect.rs` recurrence ≥ 3** | proposes "check this before <tool>" (a `lesson` proposal with `fires_on`) first; the SOUL line becomes the second choice on the same card |
| Rehearsal sandbox | **`agent/golden.rs`** (never dispatches) + **`log::replay`** | a case built from a conversation: the replayed messages up to the failed call, and a check derived from `tool_fixes` (`CallsTool` with the fixed args). Runs the same loop |
| `improvements` table | **`change_proposals`** | columns `problem_cue`, `form`, `group_id` (competing candidates share it), `rehearsal_json`, `live_outcome`; status gains `dropped` and `kept_base`. Losers stay as rows nobody is shown |
| Rules of thumb | **settings** (derived) | key `improve.rules`, at most 5 lines, shown on the Lessons tab, deletable |
| `intentions` table | **facts** + **scheduled tasks** | `kind: intention` with `trigger: project:<id> | topic:<words> | mail:<from>` and `status`; time triggers become a scheduled task through the existing `schedule` path. Never in the prompt index; fired by `recall_for` |
| `remember_to` tool | **`memory` tool** | `type: intention` in its enum, plus a `trigger` field |
| Self-model | **`Vitality`** (`organism.rs`) | per task kind: done / failed / abandoned counts, from conversation outcomes |
| Setup bundles | **project instructions + project-tagged memory (`PRJ-8`) + `personas.skills_json`** | a derived view (no table): which skills and lessons were active in the project's `done` conversations |
| "Memory page" with six sections | **Self page tabs** | Memory tab and Lessons tab gain sections (IV.5); no new page, no new tab |
| Health lines | **Health tab** | one block "Whether I'm learning" |
| "Remembered 2 things" row | **recall chip + `Provenance`** in `Timeline.tsx` | the why line and two buttons inside the expanded chip; no new row |
| "While you were away" | **the daily job digest** (`save_digest`, Tasks page, mark pulse) | wording (G3) and the counts the job now has |
| Background thinking visible | **`presence`** | backend events so every reflection and tidy-up moves the mark (G1) |

**Autonomy rungs that change** (in `AUTONOMY_CLASSES` and `autonomy.rs`):

- `consolidate`: `ask / off` → `auto / ask / off`. Auto is safe because every
  apply is snapshotted (`snapshot`) and golden-checked (`GLD-2`); the digest
  carries Undo (`restore_snapshot`).
- `skills`: stays `ask / off`. The improvement step never ships a skill alone.
- No new class.

---

# Part III - The processes, as changes to existing code

Each section says what runs today, then what it does after.

## III.1 Encode = the reflection pass, extended

**Today** `reflect_conversation_cmd` stamps `reflected_at`, reads 30 turns,
`tool_failures_in`, `tool_fixes_in`, `user_signals`, `was_rewound`, may skip on
`skip_reflection` (`USE-8`), drafts ≤3 lessons, runs the critic, bumps
recurrence, saves or proposes, then `propose_skill_revisions`.

**After**, in the same function and the same five triggers, no new trigger:

1. **Signals, no model** (before the draft): add to what it already reads
   `verifications` verdicts, decisions with outcomes, artifacts kept, scripts
   that ran clean, and `session_state_json`. Write them to `signals_json`.
2. **Outcome, by rules**: `done` if the last user turn accepts or moves on
   with no correction and nothing failed at the end; `abandoned` if rewound or
   left mid-task; `failed` if the last tool or verify step failed; else
   `partial` / `unknown`.
3. **Task kind**: `TaskKind` purpose or its rules.
4. **The draft call changes shape, not count.** The same single call now
   returns `goal`, `digest`, `worked`, `went_wrong` and up to 3 `candidates`
   (each with its evidence and a `cue`) instead of up to 3 lessons. `NO_ENTRY`
   is fine. The critic still reviews.
5. **Candidates stay on the row** (`candidates_json`). They are not lessons
   yet. Exceptions, to keep today's behaviour: a hard fail-then-fix pair
   (`tool_fixes`) and an explicit user correction still save or propose at
   once, through today's rung logic. So with nothing else built, the user gets
   the same lessons as now.
6. **Credit** (III.3) runs at the end.
7. `skip_reflection` still skips the draft, but steps 1 to 3 and credit always
   run: they cost no model call except `TaskKind`.

The 8-message floor applies on every path (G2).

## III.2 Recall = `recall_for_project`, extended

**Today**: global facts wholesale, topical facts top 5, lessons top 3 over
`SEM_FLOOR`, project-fenced, `touch_memory_usage` on the facts that fit.
Both prompt paths call it, so one change covers both.

**After**:

1. **Ranking**: similarity times usefulness (III.3) for topical facts and
   lessons. Global facts stay wholesale, ordered by usefulness under the cap.
2. **More cues, same function**: entities named in the message (matched
   against fact names and the session state), the project (exists), the task
   kind of the conversation once known.
3. **Leave out** superseded entries, intention facts, and lessons with
   `fires_on` (they reach the turn at the tool, III.5).
4. **Why**: each recalled entry carries its cue in words ("same project",
   "you named Anna", "about bank exports"). It goes into `RecalledEntry`, the
   `matches` sent to the timeline, and `ContextRefs.why`.
5. **Certainty**: a `low` or `contested` belief gets a marker in its index line
   that makes the model hedge and give the date. This changes prompt bytes:
   update `context_golden.rs` and the `store.ts` mirror in the same task.
6. **Intentions** whose trigger matches add one line to the turn ("You asked
   me to remind you about ... when ...").
7. **Episodes**: at most 1 past conversation digest, only for "Last time"
   (IV.1), never silently.

## III.3 Credit = `context_json` and `memory_usage`, extended

**Today** `context_json` records what reached each answer, and `memory_usage`
records only `last_used_at`. `skill_runs` records failures and corrections.

**After**, at the end of encoding, for every assistant message in the
conversation: read its `context_json`, and for each fact, lesson and fired
lesson apply the rules below, then update that item's `memory_usage` row.

| Item | helped | hurt | no effect |
|---|---|---|---|
| Lesson with a tool cue | the cued tool ran and the failure it warns about did not happen | the same failure happened anyway, or the turn was rewound | the cued situation did not come up |
| Fact | used in the answer (`RecallUseful`), no correction | the user corrected it, or pressed **Outdated** | not used |
| Fired lesson (check) | the check caught something the model then fixed | the user undid the result | nothing to catch |
| Skill | (from `skill_runs`) conversation `done`, not `corrected` | `failed` / `abandoned` or `corrected` | - |
| "Last time" offer | "Same way" and conversation `done` | "Start fresh" | not shown |

Usefulness = (helped + 1) / (helped + hurt + 2), shown with its count of
chances. Under 3 chances an item is not judged.

**Not relevant here** and **Outdated** clicks are written as command traces in
the session log, the same way `/forget` already is, so `user_signals` reads
them without a new channel.

## III.4 Beliefs with evidence, at the existing write sites

- `Fact` gains the fields in Part II. Old files read as `active`,
  `confidence: medium`, no evidence. `render_entry` writes the new keys only
  when set, so untouched files do not change on disk.
- At each of the three write sites, before writing, `BeliefRelation` against
  the top 5 similar entries:
  - `same`: add the evidence to the existing entry; nothing new is written.
    For lessons this is today's recurrence bump.
  - `refines`: update in place, old text kept as a superseded version.
  - `contradicts` with the user as the newer source (`/remember`, Memory tab,
    or the `memory` tool right after a user statement): supersede.
  - `contradicts` otherwise: both `contested`.
  - `unrelated`: write as today.
- **Usefulness replaces age** in `prune_lessons` (G5): past the cap, trash the
  lowest usefulness with 5+ chances first, then lessons with no chance in 120
  days, and only then the oldest.

## III.5 Lessons that fire (what v1 called habits)

- A lesson with `fires_on: <tool>` (and an optional `cue`) is left out of the
  index. When the tool loop is about to run that tool and the cue matches the
  arguments, the lesson's body goes in as a tool note for that call
  (`run.rs`, at dispatch). `ContextRefs.fired` records it.
- `check: <id>` adds a deterministic post-tool check from a small fixed set
  (header row, path exists, file non-empty, JSON parses, row count matches). A
  failed check is a tool note. No model-written code runs as a check.
- `rule` (a deterministic argument rewrite) only by the user, from the lesson
  card.
- **Recurrence 3** (`reflect.rs`, today's soul escalation) first proposes
  "check this before <tool>" when the lesson has a tool and a `tool_fixes`
  pair behind it. The SOUL line is the second button on the same card ("Make
  it a standing instruction instead"), still gated by `soul`.
- `propose_skill` gains optional `files` (`scripts/` only, size cap, content
  must match a script that ran clean in this conversation).

## III.6 The daily tidy-up = the built-in job and Tidy up, merged

**Today** `run_nightly_reflection_digest`: expired facts, reflect up to 8,
digest. `consolidate_memory_cmd`: one model call over every fact and lesson,
pending proposal, apply all.

**After**, one pass in the built-in job, in order:

1. Expired facts (as today).
2. Encode unreflected conversations with 8+ messages (cap 8, G2).
3. **Consolidate**, with the same `Consolidation` proposal and apply path as
   Tidy up, now fed with up to 5 conversations where `consolidated_at` is null:
   - group candidates with each other and with existing entries
     (`CandidateGroup`, `BeliefRelation`);
   - **promotion ladder**: a candidate seen in 2+ conversations, or a hard
     fail-then-fix, or the user said it → a belief (`promotes`); 3+ and
     rehearsal passed → a lesson that fires (III.5); SOUL only by the user;
   - contradictions → `contests`; duplicates → `merges` with evidence joined;
   - forget (to `.trash/`): 5+ chances and usefulness under 0.3, or 120 days
     with no chance;
   - **skills from success**: 2+ `done` conversations of one task kind with
     similar `worked` lines → a skill draft through the same drafting code as
     `/skillify`, as a `skill` proposal.
4. From Phase 6: the improvement step (III.9), at most 2 problems.
5. One digest (IV.4).

What `consolidate` allows decides the landing: `ask` leaves one pending
proposal on the Memory tab (the existing block) and the digest says so; `auto`
applies with snapshot and `GLD-2` and the digest carries Undo. **Tidy up** and
`/tidy` run step 3 on demand, over everything instead of only the new
conversations, so there is one tidy-up, not two (G8).

Added to the job:

- **Idle start**: a due pass starts after 2 minutes without input and not
  while a turn streams.
- **Stop on input**: any key or click raises the job's existing cancel flag;
  the item in flight finishes, the rest stays due.
- **True words**: name "Daily tidy-up", toggle "Do this once a day",
  description "Once a day, while you're not using me, I go over finished
  conversations, keep what I learned, merge duplicate notes and let unused
  ones go." The id `nightly-reflection` stays.
- **On by default** once Phase 4 ships. **Decision for Erich.**

## III.7 Know what you know

- `Vitality` gains, per task kind, the counts of `done`, `failed` and
  `abandoned` conversations, and the calibration that exists.
- A task kind with many failed or abandoned conversations gets the existing
  verify / auto-check path and an earlier clarifying question; a strong one
  gets no extra checks.
- Surprise weights credit: a failure where the decision model was sure counts
  double, as does a success where it was unsure.

## III.8 Intentions = facts of kind `intention`

- Written by the `memory` tool with `type: intention` and a `trigger`, by
  `/remember` ("remind me to ... when ..."), or from the Memory tab. Gated by
  `facts` like any fact. A time trigger is handed to the existing schedule
  path (the `schedule` rung, `/schedule`), not stored twice.
- Fired by `recall_for` (topic, project), by `mail_sync` (from), shown as the
  one line in the turn (III.2 step 6). Fired once, then `status: done`.

## III.9 The improvement step: Propose, Probe, Compose, on `change_proposals`

Runs in the daily tidy-up for each problem that qualifies: a lesson at
recurrence 2+, a told-twice correction, a task kind with 2+ failed or abandoned
conversations, a skill whose `skill_runs` usefulness dropped under 0.4.

1. **Propose** up to three candidates on different parts of the setup, each
   naming the conversation moment it targets:
   - *How I work*: a lesson that fires, a check, a step added to a skill;
   - *What I know*: a belief, or a project instruction line
     (`propose_project_instructions`);
   - *What already worked*: something that fixed a similar problem before
     (read from earlier proposal rows with the same `problem_cue`, or another
     project's instructions; facts re-checked, never trusted).
   Each is a `change_proposals` row with the same `group_id`.
2. **Probe**: every candidate and the unchanged setup through the GLD runner
   on up to 3 replayed conversations, 2 samples each (III.10). No tool runs.
   Results in `rehearsal_json`.
3. **Compose**: adopt one, merge two (rehearsed once more), or keep as is.
   One evidence line per row: `kept|merged|dropped -> <candidate> -> <what
   the replays showed>`. Losers get status `dropped`, a keep-as-is gets one
   `kept_base` row.
4. **Gate and ship**: the winner goes through `GLD-2` and the rung of its
   target (`lessons`, `skills`, `soul`, project instructions = ask). It stays
   `pending` on the Lessons tab under Ask.
5. **Learn the improver**: before proposing, read earlier rows with a similar
   `problem_cue` or the same tool; draft the form that keeps winning first.
   Credit fills `live_outcome` later. Once a month, at most 5 rules of thumb
   into `improve.rules`, used only in the propose and compose prompts. If
   rehearsal and live outcome agree 5 times in a row for an area, the area may
   ship without asking when the class rung is Auto; if they disagree, the area
   drops to Ask.

## III.10 Rehearsal = the GLD runner on a replayed conversation

- `golden.rs` already runs a message array through `drive_turn` with no tool
  dispatched and checks the first answer or tool call (`CallsTool`,
  `CallsNoTool`, `Contains`, ...).
- A rehearsal case is built, not stored: `log::replay` up to the failed call,
  plus a check from the `tool_fixes` pair (the call must move toward
  `fixed_args`, away from `failed_args`; exact where the args allow it,
  `RehearsalMatch` otherwise).
- Arms: without the candidate and with it (the candidate is put in the
  rebuilt prompt or as the tool note it would be).
- Verdict: `helped` if the with-arm is right in 2 of 2 and the without-arm
  wrong in at least 1; `no_effect` if both behave the same; `hurt` if the
  with-arm is worse.
- The golden set still runs before and after any ship (`GLD-2`).

## III.11 The editable surface

What the harness may change about itself. Anything not in this table it may
not change.

| Harness² part | Poiesis part | Changed by | Gate |
|---|---|---|---|
| System prompt | `SOUL.md` | proposal only | `soul`, the user accepts |
| Guardrails | lesson `check`, `rule` | improvement step | `lessons`; `rule` by the user |
| Memory | facts, lessons, profile | encode, tidy-up, improvement step | `facts`, `lessons`, `consolidate` |
| Tool guidance | lessons with `fires_on` | improvement step | `lessons`. Built-in tool descriptions stay fixed (prompt golden gate) |
| Skills | skills, App-source copies | improvement step, skills from success | `skills`. User files never edited in place (`OUT-2` rule) |
| Scripts | `scripts/` in skills | only scripts that ran clean | `skills` |
| Workspace facts | project instructions | improvement step | proposal, the user accepts |
| Subagents | persona choice per project | the user | not editable by the harness |
| Middleware | the dispatch point where lessons fire | fixed in code | not editable |

## III.12 Project setup (what v1 called bundles)

Light, last, and derived:

- When a conversation starts in a project, the skills and firing lessons that
  were active in that project's `done` conversations get a recall boost. No
  model call, no table.
- When a new project is created and looks like an existing one (name, folder,
  first task kind), its empty state offers to copy the other project's
  instructions and persona; facts never travel.

---

# Part IV - What the user feels, and where

Each moment names the component it lives in. If a phase does not produce its
moment, the phase is not done.

## IV.0 The mark shows when I'm working on myself

- **Every reflection moves the mark**, wherever it was started (G1).
  `reflect_conversation_cmd` itself emits `poiesis-reflecting
  {conversation_id, on, learned}` at start and end; the store's existing
  `reflectingIds` / `digestedIds` / `presence` logic consumes it. So launch
  catch-up, the daily job, the Rail menu, the Health tab and `/reflect` all
  give the orb its `weaving` state ("reflecting on a past conversation") and
  the Rail row its ◆.
- **Tidy-up and rehearsal** set `tending` ("tidying my head", orb
  `shaping`) through one `poiesis-tending {on}` event from the daily job and
  from `consolidate_memory_cmd`.
- **Stop on input** drops the mark back to whatever is underneath, through the
  same fall-back rule `tendBriefly` uses.
- **Growth stage** counts only active entries (G10): superseded and trashed
  ones never grow the mark. Lessons that fire count as lessons.
- **The digest pulse** (`SCH-UI-2`) is the one sign that the tidy-up has
  something to say. No new badge.
- A lesson firing during a run changes nothing on the mark: the run is
  already `active`.

## IV.1 "Last time"

In the `SEE-3` composer hint slot, while typing the first message of a
conversation, when a past `done` conversation of the same task kind matches
over the threshold and is under 180 days old:

> Last time (12 Oct) I made the expense summary per category from your
> Sparkasse CSV. **Do it the same way** · **Start fresh**

"Same way" adds that conversation's digest and its skill (if any) to the turn.
At most once per conversation. When it shows, the Introduction's ambient line
does not (one line rule).

## IV.2 "You only have to tell me once"

A lesson fired inside an existing tool step, as one plain line in that
step's expanded detail, nothing in the stream itself:

> Checked the header row first. You showed me on 4 Oct.

## IV.3 "Why did you think that?"

Inside the existing recall chip ("◆ remembered" / "◆ learned"), expanded
`Provenance`:

- each entry with its why ("same project", "you named Anna"), its source
  conversation and date as a link;
- two text buttons: **Outdated** opens one inline line "What is right now?"
  and the answer supersedes the belief; **Not relevant here** counts as hurt
  for this item.

"What I'm working from" (`ContextPanel`) shows the same why on the Remembered
and Learned layers' sources.

## IV.4 "While you were away"

The daily job's digest, in the Tasks page and through the mark pulse, worded
by when it ran (G3):

> While you were away I went over 3 conversations. I learned 1 thing, merged
> 2 notes, and let 1 old one go. **Show me**

"Show me" opens Self → Memory on "Recent changes". With `consolidate` at
Auto, each change has **Undo**.

## IV.5 The Self page grows, it does not get replaced

**Header** (`Self.tsx`): the narrative is fixed (G4) and gains one sentence
from the self-model (IV.6). The strip stays "facts · lessons · skills ·
pending", plus "· N I'm unsure about" when there are contested beliefs.

**Memory tab** (`MemoryPanel.tsx`), top to bottom:

1. The profile prose, untitled, as today (`SMP-5`).
2. The existing pending tidy-up block, now also holding the daily job's
   proposal, with "Not this one" per row next to Apply all.
3. **Recent changes**: what the last tidy-up and the last reflections did,
   one first-person line each, Undo where it applies.
4. **Unsure**: contested pairs, both sides with their source, one button per
   side, plus "Both, in different situations" (sets scope).
5. The fact cards, as today, grouped by project and then "everywhere". Each
   card gains: "from <conversation>, <date>" for each piece of evidence (the
   existing "Where this came from" becomes this list), "helped 4 times" or
   "not tested yet" next to "last surfaced", certainty in words (`sureWord`),
   "Earlier versions" when superseded ones exist.
6. **Things I'll do**: waiting intentions, each with its trigger in words and
   Drop.
7. **Let go**: the `.trash/` entries, searchable, Restore (G9).

**Lessons tab**:

1. Pending proposals, as today. A proposal from the improvement step gains
   the IV.8 block.
2. Lesson cards, as today, plus: "I check this before <tool>" for lessons that
   fire, usefulness, evidence list.
3. **Skills I made**: skills from success and accepted skill proposals.
4. **How I improve**: the rules of thumb (IV.10).

**Health tab**: one new block "Whether I'm learning" (IV.7). "Reflect now"
says what encoding found ("I noted 2 things to watch and learned 1").

**Autonomy tab**: `consolidate` shows three choices; the `lessons` blurb says
it covers checks too.

All lists keep a fixed card height with "More" for long bodies.

## IV.6 Self-portrait

One sentence in the Self header, from the self-model, words not numbers,
rebuilt weekly:

> I'm reliable with reports and file clean-up. I often get spreadsheet
> imports wrong on the first try, so I check them before I go on.

## IV.7 Whether I'm learning (Health tab)

> Mistakes I repeated: 2 (last month 5)
> Things you had to tell me twice: 1 (last month 3)
> Memory I carry in every message: 1.1k characters (last month 2.4k)
> Fixes I made that held up: 7 of 8 (last month 3 of 6)

"No data yet" under two weeks.

## IV.8 "I tried 3 fixes"

On the existing proposal card, one block, fixed height, collapsed to its first
line:

> I kept reading your bank exports wrong. I tried 3 fixes on the
> conversations where it happened:
> - a check before reading: right in 4 of 4 replays
> - a written reminder: right in 1 of 4
> - the import steps from your *Taxes* project: right in 3 of 4
>
> I'm proposing the check. **Use it** · **Not now** · **Show replays**

## IV.9 Toast and the new-chat line

- `MemoryToast` gains two leads: "I replaced what I knew; the old one is under
  Earlier versions" (supersede) and "I'm not sure which is right, it's under
  Unsure" (contested). Undo as today.
- The Introduction's "◆ Recently learned" line (`PRES-5`) reads the newest
  promoted belief, firing lesson or lesson of the week, not only lessons (G11).

## IV.10 How I improve

On the Lessons tab, the rules of thumb as plain sentences with their count and
a delete button. A deleted rule is not regenerated from the same rows:

> Written reminders rarely fix how I call a tool. I use checks for that
> now. (5 of 5 times)

## IV.11 Slash commands (no new command)

| Command | After this plan |
|---|---|
| `/remember` | also takes intentions ("remind me ... when ..."); runs the relation check |
| `/forget` | unchanged; counts as a correction for credit |
| `/always` | unchanged |
| `/reflect` | runs the extended encode; moves the mark |
| `/tidy` | the merged tidy-up (III.6), on everything |
| `/why`, `/context` | the panel shows the why |
| `/skillify` | unchanged; its drafting code is reused by skills from success |
| `/checkup` | one more line: "I'm unsure about 2 things" → Self, Memory |
| `/self` | unchanged; `openSelf` gains nothing new, sections are inside tabs |

---

# Part V - Measuring it (do this first)

| Metric | Read from | Goal |
|---|---|---|
| Repeat-mistake rate | `tool_fixes` / `tool_stats`: same tool and error class in a later conversation while a lesson about it exists | down |
| Told-twice rate | `user_signals`, `/forget`, Outdated clicks matching an existing belief | down |
| Always-on memory size | `recall_for(None)` index length | down while the store grows |
| Recall precision | `memory_usage` helped / no effect vs hurt and Not relevant | up |
| Stale belief rate | Outdated clicks per 100 recalls | down |
| Fix hold rate | `change_proposals.live_outcome` | up |
| Rehearsal agreement | `rehearsal_json` vs `live_outcome` | up; under 60% means rehearsal is not trusted for that area |
| First-fix rate | problems fixed by the first shipped change, per quarter | up |

A monthly snapshot in `settings`. The first five exist from Phase 0.

---

# Part VI - Tasks

Prefixes: `FIX` scan findings, `MEV` measure, `EPI` encode, `CRD` credit,
`CUE` recall, `BLF` beliefs, `SLP` daily tidy-up, `HAB` firing lessons, `RHS`
rehearsal, `IMP` improvement loop, `INT` intentions, `MET` self-model, `PRJ`
project setup. Each task names the files it touches.

## Phase 0 - Fix what exists, and measure

### FIX-1 - Every reflection moves the mark (G1)

`reflect_conversation_cmd` emits `poiesis-reflecting` at start and end (with
`learned`). `store.ts` listens and updates `reflectingIds`, `digestedIds`,
`presence` with today's rules; the frontend `reflectConversation` stops setting
them itself so there is one source. **Accept:** launch with an unreflected
chat: the mark weaves and its Rail row shows ◆ while catch-up runs, then
idles; same for the daily job run from the Tasks page.

### FIX-2 - One message floor (G2)

The daily job filters on `CATCH_UP_MIN_MESSAGES`. **Accept:** a 2-message chat
is not stamped by the daily job.

### FIX-3 - Digest words (G3)

`compose_digest` gets the run's time and says "While you were away" or
"Earlier today", never "Last night". Tests for both.

### FIX-4 - Self narrative (G4)

Fix the broken sentence in `Self.tsx` `narrative`. Test.

### MEV-1 - The numbers

One function in `organism.rs` for the five Phase 0 metrics from existing
tables, a test per metric with planted rows, monthly snapshot in settings.

### MEV-UI-1 - Whether I'm learning

The IV.7 block on the Health tab, four lines (the fourth says "not yet" until
Phase 6).

## Phase 1 - The conversation becomes the episode

### EPI-1 - Columns

Schema migration on `conversations` (Part II columns), `vectors` scope
`episode` through the existing embed queue (`missing_vector_texts`
counterpart).

### EPI-2 - Encode

III.1 inside `reflect_conversation_cmd`: signals, outcome rules, `TaskKind`,
the reshaped draft call, candidates on the row, the fail-then-fix and
user-correction fast paths unchanged. **Accept:** a conversation with one
fail-then-fix and an accepting last turn gets `outcome = done`, the fix in
`signals_json`, the same lesson as today, and a candidate that cites it; a
rewound one gets `abandoned`; lesson output on today's eval fixtures does not
drop.

### CRD-1 - What reached each answer

`ContextRefs` gains `why` and `fired`; check that the backend-run path
(scheduled and resumed runs, `context.rs` `gather`) writes `context_json` too,
not only the frontend finalize path, and fix it if not.

### CRD-2 - Credit

III.3 at the end of encoding; `memory_usage` gains its columns;
`skill_runs` is read as is. **Accept:** a lesson cued to `read_file`, a later
conversation where `read_file` runs clean: helped +1; the same failure again:
hurt +1.

### EPI-UI-1 - "Last time"

IV.1 in the `SEE-3` hint slot; one line rule with `Introduction.tsx`.
**Accept (felt):** do a report task, finish it, start the same kind of task a
day later in a new chat: the hint appears with the right date and goal.

## Phase 2 - Recall you can see

### CUE-1 - Ranking and cues

III.2 steps 1 to 3 in `recall_for_project`. Existing `recall_for_*` tests stay
green; new ones for usefulness ranking and exclusions.

### CUE-2 - Why

III.2 step 4 into `RecalledEntry`, `matches`, `ContextRefs.why`.

### CUE-3 - Hedged beliefs

III.2 step 5, with `context_golden.rs` and the `store.ts` mirror updated in
the same task.

### CUE-UI-1 - Why and correction in the chip

IV.3 in `Timeline.tsx` `Provenance` and `ContextPanel.tsx`. Outdated and Not
relevant write session-log traces (III.3). **Accept (felt):** say "I live in
Hamburg", later press Outdated on the recalled fact and type Berlin: the next
recall says Berlin, the Memory tab shows Hamburg under Earlier versions.

**Gate after Phase 2:** recall precision must have risen against Phase 0.

## Phase 3 - Beliefs with evidence

### BLF-1 - Frontmatter

`Fact` fields, `parse_entry` / `render_entry`, prompt-building callers skip
superseded, `Vitality` counts only active (G10). Old files unchanged on disk.

### BLF-2 - Relation check at the write sites

III.4 in `memory_op` (next to `decide_scope`), `remember_fact_cmd`, and the
recurrence check in `reflect.rs`. **Accept:** save "prefers tea", then
"prefers coffee": a supersede (user said it) or both `contested`, never two
silent actives.

### BLF-3 - Prune by usefulness

III.4 last bullet in `prune_lessons` (G5).

### BLF-UI-1 - Memory and Lessons tab sections

IV.5 for both tabs: Recent changes, Unsure, evidence on cards, Earlier
versions, Let go (G9). IV.9 toast leads and the Introduction line (G11).

## Phase 4 - The daily tidy-up

### SLP-1 - One tidy-up

III.6: the job calls the consolidate step with conversations;
`Consolidation` gains `promotes` and `contests`; Tidy up and `/tidy` share the
code; `consolidate` gains `auto` (`AUTONOMY_CLASSES`, `autonomy.rs`); idle
start, stop on input, the new words in `Tasks.tsx`. Default on is Erich's
decision. **Accept:** with the app idle for 2 minutes a due pass runs and the
mark is `tending`; a key press stops it after the current item; the next pass
picks up the rest.

### SLP-2 - Skills from success

III.6 step 3, reusing the `/skillify` drafting code, landing as a `skill`
proposal.

### SLP-3 - Digest and Recent changes

IV.4 from the job's own counts, with the rule that the digest never claims
more than was done. **Accept (felt):** three conversations with the same small
correction, leave the app idle: the digest says one thing was learned, and the
Memory tab shows one belief with 3 evidence links.

## Phase 5 - Lessons that fire, and rehearsal

### HAB-1 - Firing at dispatch

III.5 in `run.rs`: `fires_on`, cue match, tool note, `ContextRefs.fired`;
excluded from the index (golden gate updated).

### HAB-2 - Checks

The fixed check set after the tool returns.

### HAB-3 - New escalation

Recurrence 3 in `reflect.rs` proposes a firing lesson first, SOUL second on
the same card.

### HAB-4 - Scripts in proposed skills

`propose_skill` `files`, validated against scripts that ran clean.

### RHS-1 - Rehearsal on the GLD runner

III.10 in `golden.rs` + `log::replay`. **Accept:** a planted fail-then-fix
conversation and a candidate check: the with-arm matches `fixed_args` 2 of 2,
the without-arm repeats the failure, verdict `helped`; no tool dispatched
(assert on the dispatch counter).

### HAB-UI-1 - The fired line

IV.2 inside the tool step's expanded detail; the Lessons tab line "I check
this before <tool>".

**Gate after Phase 5:** repeat-mistake rate must have dropped against Phase 0.

## Phase 6 - The self-improvement loop

### IMP-1 - Problem queue

III.9 opening list, in the daily job, deduplicated by cue, told-twice first.

### IMP-2 - Propose, on `change_proposals`

The columns of Part II; up to 3 candidates per problem, one `group_id`; reads
earlier rows first.

### IMP-3 - Probe

Each candidate and the unchanged setup through RHS-1.

### IMP-4 - Compose and ship

Adopt / merge / keep as is; losers `dropped`; winner through `GLD-2` and its
rung. **Accept:** a planted problem where only the check fixes the replay: the
check is proposed, the other two are `dropped` with their results; a problem
where nothing beats the unchanged setup: nothing proposed, one `kept_base`
row.

### IMP-5 - Improver learning

`live_outcome` from credit, rules of thumb in `improve.rules`, per-area
Ask/Auto by agreement. **Accept:** seed 5 rows where a check helped and a
lesson did not for `read_file`: the next `read_file` problem drafts the check
first, and the rules contain a sentence saying so.

### IMP-UI-1 - "I tried 3 fixes" and "How I improve"

IV.8 on the proposal card (Lessons tab) and in the digest; IV.10 on the
Lessons tab.

**Gate after Phase 6:** fix hold rate above 60% and rehearsal agreement above
60% in at least one area, or Phase 6 ships only under Ask.

## Phase 7 - Intentions

### INT-1 - Intention facts

`type: intention` and `trigger` in the `memory` tool, `/remember` parsing,
time triggers to the schedule path.

### INT-2 - Firing

`recall_for` (topic, project) and `mail_sync` (from).

### INT-UI-1 - Things I'll do

The Memory tab section and the one line in the turn.

## Phase 8 - Self-model

### MET-1 - Vitality per task kind

III.7 in `organism.rs`.

### MET-2 - Behaviour

Weak kinds get verify and an earlier question; surprise weighting in credit.

### MET-UI-1 - Self-portrait

IV.6 in the Self header.

## Phase 9 - Project setup

### PRJ-S1 - Boost and offer

III.12: recall boost from the project's `done` conversations; the new-project
offer in the project empty state.

---

# Part VII - Risks

| Risk | Answer |
|---|---|
| Local model cost | The draft call is reshaped, not added; `skip_reflection` still skips it. The tidy-up is capped, idle-only, stops on input. Rehearsal only for promotion and improvement candidates |
| Breaking today's lessons | Fail-then-fix and user corrections keep the current fast path; EPI-2's accept compares lesson output with today |
| Prompt drift | Every prompt-byte change goes through `context_golden.rs` and the `store.ts` mirror in the same task |
| Noisy credit | Not judged under 3 chances, nothing trashed under 5 |
| Users confused by more memory UI | Nothing new in the chat stream except inside existing steps and chips; everything else is in existing Self tabs |
| Drift from many small self-changes | Every ship through `GLD-2`; `live_outcome` on every proposal; monthly: changes with `hurt` or `no_effect` after 5 chances are proposed for removal |
| Rehearsal says helped, live use does not | agreement measured per area; low agreement moves the area to Ask |
| Contested beliefs pile up | the count in the Self strip; after 30 days without an answer the newer wins and the older goes to `.trash/` |

# Part VIII - Decisions for Erich

1. The daily tidy-up on by default once Phase 4 ships (today it is off,
   `SCH-5`).
2. `consolidate` gains an Auto rung (snapshot + golden check + Undo in the
   digest).
3. Firing lessons and checks live under the `lessons` class, no `habits` class.
4. Delegated child conversations (`parent_conversation_id`): encoded on their
   own, or folded into the parent's signals. The plan assumes folded.

# Part IX - Not in this plan

- Training model weights.
- Rerunning live user tasks.
- Graph storage.
- Sharing memory between users or devices.
- The CLI agents' own rule learning (`engine_rules`, `RTE-6`); their
  `engine-memory` proposals simply land as beliefs with evidence.
