# Project Poiesis - Connected Apps Plan

**Notion, Slack, Calendar, Linear, Sheets and hundreds more, connected in
one click from the chat. Nothing sends, posts or pays without the person
seeing it first.**

Poiesis reaches the world through its own toolsets (files, web, Mail over
IMAP, the browser) and through MCP connectors the person adds by URL or
command line. That covers people who know what an MCP server is. Everyone else
asks "can you put this in my calendar" or "what's new in Linear" and gets a
polite no.

OpenMausBot (github.com/milind-soni/OpenMausBot, read at `a67daf0`,
2026-10-13) closes that gap with **Composio**: one hosted MCP endpoint that
fronts about a thousand app toolkits, owns their OAuth sign-ins, and shows the
model four meta-tools instead of a thousand tools. The interesting part of OMB
is not the Composio call. It's the safety layer around it: per-bot grants
enforced per call, a send/post/pay classifier that always asks, labelled
multi-account sign-ins, and a key that never leaves the harness. This plan
brings both to Poiesis, in Rust, on seams that already exist.

It also fixes two gaps found in Poiesis's **existing** MCP path while reading
for this plan. Any connector the person adds today can change things without
asking, and its output enters the prompt without the untrusted wrapping.
That work (Part I) ships first and on its own, because it's live now.

> ID prefixes: **MCG** gates for every MCP connector (ships first) - **CAP**
> Composio backend - **-UI** frontend - **-T** tests - **CAP-P** parked.
>
> **Status (2026-10-19, end of day): ALL CODE IS BUILT, Phase 6 included, and the
> review fixes below are in. NOTHING IS COMMITTED.** Phase 0 (MCG), Phases 1-5
> (Composio: key, session, accounts, connect-from-chat card, grants, catalog,
> usage, checkup) and Phase 6 (Connect `ck_` key as the default) are implemented,
> backend and frontend. First real use with a `ck_` key worked: Google Calendar
> connected from the Apps page and "what's on my schedule" answered in chat.
> Open items, in order: (1) the rest of the manual pass (the in-chat connect card
> has not been tried for real yet), (2) record a successful `add` and a `list`
> answer, (3) commit. Details in "Resume here" below. Written 2026-10-13.
>
> **Update 2026-10-23: CAP-14 is BUILT (Phase 5).** An agent app (Claude Code,
> Codex, an ACP agent) can use the person's connected apps through Poiesis. Not
> committed, not tried with a real agent. As built, and where it differs from
> CAP-14 above:
> - **A switch of its own.** *Let it use my connected apps* in Settings, External
>   agents, beside *Let it use my memory*, off for every agent (schema v36,
>   `engines.app_tools`). Reaching a person's calendar and mail is a bigger thing
>   than reading their memory, so it is not folded into the memory switch. Each
>   switch now decides its own tools: the four memory tools need `memory_tools`,
>   the apps need `app_tools`.
> - **Same endpoint, same gate.** The loopback MCP endpoint (`agent/toolrpc.rs`)
>   lists the Composio meta-tools for a token armed with them
>   (`arm_mcp_with`; the token carries the exact specs it may call). The call is
>   handed to `engines::tools::Tools::app`, which runs the run loop's own code:
>   `run.rs` `relay_composio` was pulled out of `TurnCtx::composio_call` so there
>   is one copy. So CAP-5 and MCG-3 apply exactly: persona grants (found through
>   the job's conversation and its parent), read/change/send, the person's
>   "always" rules, the daily cap, one question naming every app on the same
>   panel, the headless "left for you" parking, `redact_links`, untrusted
>   wrapping, the scope hint, and usage rows.
> - **Four tools offered, but `COMPOSIO_MANAGE_CONNECTIONS` is answered by
>   Poiesis itself.** An agent cannot show a sign-in card, so the call is never
>   forwarded to Composio (no address is ever made). It gets the connected apps
>   and a sentence to say in its report which app is missing. Its description
>   says so too (`app_calls::agent_prefix`; Composio's own text for it is replaced).
> - **The key stays in Poiesis.** The agent holds a per-run loopback token that
>   works only while Poiesis runs the job; it never sees the key, a session id or a
>   Composio address (`engines/process.rs` already strips `COMPOSIO_API_KEY` from
>   its environment). The hang-up of the agent or Stop withdraws an open question.
> - **Timeline and report.** Steps read as in the run's own loop ("sent Slack:
>   send message"), the report the lead reads says "used your connected apps N
>   times", and the agent's system note (`tools::APPS_NOTE`) says to report a
>   missing app instead of asking to sign in.
> - Tests: Rust 1057 pass (new, all in `engines::tools`, `agent::toolrpc`,
>   `engines::settings`, `db::engines`: apps refused without the switch; memory
>   tools off when only apps are on; exactly the four, only with a key and a
>   grant; connecting answered by Poiesis with no address; an unreadable batch
>   refused with no call; a headless send left for the person; a refused app
>   that lists nothing granted; a token lists and allows only what it was armed
>   with; the switch is saved on its own). Frontend: one new case in
>   `Engines.test.tsx`; `tsc` clean.
> - **Still to do by hand, with a real agent and a real key:** switch it on for
>   Claude Code, delegate "what is on my calendar tomorrow" and check the read
>   runs quietly; delegate "post hi in #general" and check the panel appears and a
>   Task (headless) leaves it in the inbox. Real agent trials cost quota, so do
>   this once, deliberately.
>
> **Update 2026-10-19 (review against the code and the recording). Fixed:**
> - **In-chat connect read only plain slugs.** The model is shown Composio's own
>   schema, `toolkits: [{name, action}]`, and every such call was refused.
>   `connect_request` (`app_calls::connection_ops`) now reads both shapes. Only
>   `add` can lead to a card; `list` is answered from the mirror; `remove` and
>   `rename` are declined in words and never reach Composio. The step line reads
>   both shapes too ("checked my sign-ins for" when nothing is added).
> - **A stale "connected" could not be fixed from chat (Connect mode).** The mirror
>   only learned and was never lowered, and `connect_request` trusted its ACTIVE.
>   Now: in Connect mode, with someone watching, an ACTIVE app still gets a card
>   and Composio decides (`settle` turns "connected" back into a note); a headless
>   run still gets "already connected". A `list` entry with an empty account list
>   (or `not_connected` / `disconnected`) is the new `Conn::Gone`, and
>   `apply_connections` deletes that app's mirror rows and remembered link.
> - **485 of 1,608 apps were named by their first word.** `known_apps` used only the
>   24 curated apps, so every `google_*` app was "google" and one "always" covered
>   all of them. It now uses the whole bundled catalog (`app_calls::catalog_slugs`),
>   in `describe` too.
> - **Asking for an app again deleted its open sign-in.** `start_sign_ins` now hands
>   back a sign-in Poiesis opened in the last ten minutes (`reopen_pending`) instead
>   of clearing its `initializing` account and adding another. Expired accounts are
>   still cleared when a new sign-in replaces them, on purpose: a dead account must
>   not stay Composio's default for the app. This is a Connect-mode exception to
>   CAP-7's "never deletes", which stays true for Project mode.
> - **Smaller:** the batch entry's `account` (beside `arguments`) is kept on
>   `Target` and shown in the panel; usage leaves out targets Composio refused
>   before running ("Tool not found"; `composio::target_outcomes`), but counts
>   ones the app answered with an error; step lines keep a name's noun
>   (`GOOGLECALENDAR_EVENTS_LIST` reads "events list", not "list"; `action_of`);
>   the card's poll reuses one Connect client (`check_with_shared_client`, keyed
>   by a hash of address and key), so each poll is one request and two polls never
>   overlap.
> - Tests: 192 green across `composio`, `app_calls`, `connector_gate`, `agent::run`.
>   The full suite has two failing `db::tests` fork tests
>   (`a_fork_keeps_the_summary_it_can_still_account_for`,
>   `a_spoken_turn_stays_marked_and_a_fork_keeps_the_mark`). They fail one at a time,
>   by turns: `fork_conversation_at` cuts by `created_at` in milliseconds, and
>   messages appended in the same millisecond tie. Not part of this plan.
>
> **Update 2026-10-19 (night): four of the "not done on purpose" items are BUILT.**
> Erich asked why they had been skipped. Only diagnostics redaction had a reason
> that still held (there is no export). The other reasons were gone (the alias)
> or weak (no tool list). Not committed, not run in the real app.
> 1. **"Apps: Notion, Linear" tooltip (CA-UI-6).** `components/Apps/personaAppsLine.ts`
>    (`usePersonaAppsLine`, `appsLine`, `forgetPersonaApps`). The persona chip under
>    the composer (`ModeChips`) shows "Apps: Notion, Linear · Change this", and
>    a Fleet row whose agent is a persona shows it under the task. A persona that
>    is not narrowed has no line; the explicit none reads "Apps: none". It is read
>    again after the persona editor saves.
> 2. **Per-tool read/change/send for Composio apps (MCG-7).**
>    `set_tool_kind_cmd` takes `app`, and the override is kept with that app's
>    policy, which `resolve` already read. `composio_app_tools_cmd(app)`
>    (`app_calls::known_tools`) lists the tools that ran, from `app_call_log`,
>    busiest first, then any given a kind without having run. Each Composio app card
>    shows them as "Tools I've used:" with the same chips and menu as an ordinary
>    connector (`ToolChips` takes `app` and `lead`). The approval panel names each
>    app tool of a Composio call (`PermissionRequest.tools`, `app_calls::panel_tools`)
>    with the same chips (`ToolKinds`). A correction applies from the next call.
>    Loosening still asks first.
> 3. **Several accounts per app with a `ck_` key (CAP-C6).** Rows: an app with
>    one account keeps `connect:{app}` (the card and the toast watch that id),
>    named by its alias. With two or more, each has `connect:{app}:{Composio account
>    id}`. A sign-in for one more waits as `connect:{app}:new`. `apply_accounts`
>    shapes the rows after every `list`. An extra sign-in is done when an active
>    account appears that was not listed when it began (`Pending::before`).
>    One whose link is gone is dropped. A name Poiesis knows is kept when
>    Composio's answer carries none (where Composio puts the alias is not
>    recorded; `alias_of` looks under `alias`, `account_alias`, `label`,
>    `nickname`). New: `add_account` (`add` with `alias`, CAP-7's `plan_label`
>    rules, max five working), `rename_account` (`rename`), `remove_one`.
>    Disconnect on one of several removes only that account, and on an open
>    sign-in it gives that up (it never removes every account of the app).
>    Reconnect for an expired account beside a working one adds one more under
>    its old name. UI: Add account and Browse apps' name field in Connect mode
>    too, Name/Rename in place (`composio_rename_cmd`), Cancel for an open
>    sign-in, "unnamed" for an unnamed account among several. The card and the
>    toast also count any working account of the app as done once rows split.
> 4. **"Which account?" without the recording.** Poiesis knows the sign-ins, so
>    `app_calls::plan` stops a batch target for an app with two or more working
>    accounts that names none (in `account` or the arguments). Nothing is asked or
>    sent. The model is told the names to choose from (alias, else Composio's
>    id, which `account` takes too) and to ask the person if it can't tell. The tool
>    description adds one sentence when an app has several accounts. Composio's own
>    refusal is still unrecorded. It can now only come from accounts Poiesis
>    doesn't know of, and it reaches the model wrapped, as before.
> - Tests: Rust 846 pass (the two fork tests above fail, as before); new ones:
>   `a_second_account_is_added_under_its_name_and_each_gets_a_row`,
>   `an_extra_sign_in_that_cannot_be_finished_goes`,
>   `accounts_named_in_composio_are_shown_and_a_rename_reaches_it`,
>   `a_name_is_kept_when_composios_answer_carries_none`,
>   `disconnecting_one_of_two_accounts_leaves_the_other`,
>   `a_call_to_an_app_with_two_accounts_must_name_one`,
>   `an_unnamed_account_is_offered_by_composios_id`,
>   `the_several_accounts_sentence_is_said_only_when_it_applies`,
>   `an_apps_tools_are_the_ones_that_ran_and_the_ones_given_a_kind`,
>   `the_panel_is_offered_each_tool_once_with_its_kind`. The fake Composio
>   (`world()`) now keeps an alias and answers `rename`. Frontend 822 pass. New:
>   `personaAppsLine.test.tsx`, `PermissionPanel.kinds.test.tsx`, and more cases
>   in `ComposioApps.test.tsx` and `BrowseApps.test.tsx`. Two fail: the old
>   `icons.test.ts` one (`WindowControls.tsx`), and `highlight.test.ts` only
>   under the full parallel run (it passes alone). `tsc` is clean.
> - **Apps page in two tabs (Erich, same night).** `routes/Apps.tsx` has the
>   shared `.model-tabs` with **Composio** and **MCP servers**, each with a count.
>   The Composio tab gets a dot when a sign-in needs attention. It opens on
>   Composio unless only MCP servers are in use. The MCP part is
>   `components/Apps/McpServers.tsx`, redesigned:
>   - A short intro, then one toolbar with the server count, a Manage menu
>     (Import/Export servers) and Add server.
>   - The add form opens on demand (at once when there are no servers), with
>     the shared segmented control, and keeps its fields when connecting fails.
>   - Remote/Local and "Token saved" badges replace the 🔒 emoji.
>   - Remove now asks first.
>
>   Test: `routes/Apps.test.tsx`.
> - **Still needs Erich, with a `ck_` key:** add a second Google Calendar account
>   named "work" from the Apps page; check both rows show and the names are right;
>   ask "what's on my work calendar"; ask without naming one (the model should ask
>   which); rename one; disconnect one (the other stays). Then record the `list`
>   answer with two accounts and one with an alias set (the alias field name), and
>   a `MULTI_EXECUTE` naming no account (Composio's refusal), with
>   `composio_record_cmd`.
>
> **Update 2026-10-19 (later): Phase 6 (Part IX, CAP-C) is BUILT, backend and
> frontend, except what needs Erich's real `ck_` key.** Composio Connect is the
> default; the developer `ak_` key stays as the advanced option. Green: 820 Rust
> tests (one flaky and unrelated: `db::tests::a_fork_keeps_the_summary_it_can_still_account_for`
> fails about half the time on its own, in code Phase 6 never touched), 802 frontend
> tests, `tsc` clean (the one `icons.test.ts` failure is the old
> `WindowControls.tsx` one). Nothing has been run in the real app and nothing is
> committed. **Phase 6 as built, and where it differs from Part IX:**
> - **CAP-C9: partly recorded (2026-10-19, see the later update).**
>   `fixtures/composio/connect/` holds `tools_list.json` and
>   `manage_connections_googlecalendar.json`. Still to record: a successful `add`
>   and a `list` answer. The scrubber replaces the key, links, `ln_`/`ca_`/`ac_`/`ct_`
>   ids and emails; Connect's account ids (`googlecalendar_vole-negate`) and
>   `log_id` are word pairs, not secrets, and stay in the file.
>   To record: run a debug build with a `ck_` key saved, open the webview console,
>   and call `window.__TAURI_INTERNALS__.invoke("composio_record_cmd", { steps: [
>   { name: "manage_connections_gmail", tool: "COMPOSIO_MANAGE_CONNECTIONS",
>   arguments: { toolkits: ["gmail"] } } ] })`. It also writes `tools_list`. Files
>   go to `fixtures/composio/connect/`, scrubbed by `scrub_for_fixture` (key, links,
>   ids, emails). **Read every file before committing.** Then rewrite the
>   `connections_*` tests from them, and check whether asking twice mints a new
>   link (the card polls every 3 s; move it to 5 s if it does).
> - `composio.rs`: `Mode` (`mode_of`, `current_mode`), `connect_client`,
>   `validate_key(db, http, key)`, `parse_connections`, `redact_links`,
>   `scrub_for_fixture`, an in-memory link cache (`remember_link`/`recall_link`,
>   10 min, keyed by connector and app), `start_sign_ins` (a broken app is asked
>   with `reinitiate_all` in a call of its own), `check_connect`, `connect_connect`,
>   `learn_from_results`, `remove_from_list`, `note_key_kind` (a key of the other
>   kind empties the mirror), and one `*_any` entry per action. Connect mode keeps
>   one mirror row per app, id `connect:{app}`, no label. An app that was ACTIVE and
>   is now asked about with a sign-in wanted reads EXPIRED.
> - Commands: `composio_connect_cmd` opens nothing for an app that is connected
>   already; new `composio_reconnect_cmd`, `composio_cancel_cmd`,
>   `composio_record_cmd` (debug only); `composio_accounts_cmd(refresh, apps)`;
>   `ComposioStatus.mode`. The Connect catalog is the 24 curated apps plus a typed
>   slug card. The address is checked again (`safe_to_open`) before it is opened.
> - Run loop: `MANAGE_CONNECTIONS` is async and, with a Connect key, asks Composio
>   first (`ConnectRequest::settle` turns the answer into cards and notes); every
>   Composio output and error is redacted before wrapping; `learn_from_results`
>   reads the raw answer.
> - Review fixes: F1 (link reuse in both modes; Reconnect revokes and links again
>   under its label; Cancel drops the half-made sign-in; a label held only by a
>   sign-in that never worked can be reused), F2 (FAILED says "didn't finish" and
>   never raises the hub dot), F3 (guard keyed on user and setups), F4 (one
>   background ask at startup), F5, F6 (`Planned.shown`, the panel lists each
>   target's own arguments), F7 (`COMPOSIO_*` inside a batch is refused), F8, F9
>   (cached curated list; key check uses the install's own user id; the batch
>   runner is offered while planning first and refuses anything but a read at the
>   call), F10 (one monogram `connect-mono`, one success style `provider-card-ok`,
>   persona apps saved with the editor's Save; `createPersona` now returns the
>   persona).
> - Not done: the manual pass below (now with a `ck_` key first, then an `ak_`
>   key); CAP-C-T3 against real recordings.
> - **Terms (open task 8), checked 2026-10-19 and inconclusive:** Composio's Connect
>   page (`docs.composio.dev/docs/composio-connect`) says nothing for or against
>   third-party agents using a person's own key; it only shows the key sent in
>   `x-consumer-api-key` to `connect.composio.dev/mcp` and tells people to copy it
>   from "the AI Clients setup in the Composio dashboard". Composio's own install
>   page lists other agents. Still worth a line to Composio before a public release.
> - **Open Composio** points at `https://dashboard.composio.dev` (a real sign-in
>   page, 2026-10-19). Whether the consumer and developer dashboards differ at that
>   address is unconfirmed.
>
> **Update 2026-10-19 (first real try with a `ck_` key).** The key worked. Three
> faults found and changed:
> - **Browse apps showed 24 apps and no marks.** A `ck_` key cannot list toolkits,
>   so the list is now bundled: `scripts/build-composio-catalog.mjs` reads Composio's
>   public docs (1,608 apps: slug, name, one-line description, category, whether
>   Composio sets up the sign-in) into `shared/composio-catalog.json`, and copies
>   each app's logo (SVG, 24 KB cap, 1,531 of 1,608 fit) into `public/app-logos/`.
>   The page asks only for same-origin `/app-logos/<slug>.svg`; the CSP is unchanged.
>   Apps with no mark show their first letter (`AppLogo.tsx`, the one tile class
>   `app-logo`; `connect-mono` is gone). Re-run the script to refresh. The marks are
>   the apps' own trademarks, kept only to name the app.
> - **The dialog changed height with each category.** It is now a fixed size in the
>   confirm dialog's shell; only the list scrolls; categories are the shared
>   `Select`; the header shows the count ("1,608 apps", "12 of 1,608 apps"); cards
>   load 60 at a time with Show more.
> - **Sign-in finished in the browser but Poiesis never noticed, and then "couldn't
>   start the sign-in".** Recorded 2026-10-19 (`fixtures/composio/connect/`, via
>   `composio_record_cmd`) and **found**: `COMPOSIO_MANAGE_CONNECTIONS` takes
>   `toolkits: [{name, action, account_id?, alias?}]` with `action` one of `add`
>   (the default), `list`, `rename`, `remove`. **`add` always makes a new link and a
>   new account, ten at most per app.** The poll and "Check again" called it with the
>   default, so every 3 s made one more `initializing` account (nine of them, plus
>   the one the person really signed in) until Composio refused: "Maximum of 10
>   accounts per toolkit reached." The entry then said `status: "failed"` while its
>   `accounts` held an `active` one, which the parser read as failed. The old
>   `reinitiate_all` flag is not in the tool's schema at all. Now: checking uses
>   `list` (no side effects); starting a sign-in is `list` first, an app with a
>   working account is "Connected" and gets no `add`; accounts that never worked are
>   `remove`d (also the clutter beside a working one); only then `add`; a working
>   account inside an entry beats the entry's own `status`; `entry_app` no longer
>   drops an entry named by a display name; the poll shows its errors ("I couldn't
>   check: ...") and also runs when the window is focused again. Tests use a fake
>   Composio that follows these rules (`world()`), and the recording itself.
>   Not recorded yet: what a successful `add` and a `list` answer look like (the
>   parser reads `accounts[].status` and any `composio.dev` link leniently).
>   **Composio Connect has a real `remove`, and Disconnect now uses it** (CAP-C6 below
>   said it had none): `remove_everywhere` lists the app's accounts, removes all of
>   them, lists again, and only if none is left drops the local row; otherwise it
>   says "Composio still lists X as connected" and keeps the row. Connect keeps one
>   set of sign-ins for all of the person's AI clients, so the confirm dialog says
>   other clients lose the app too. Cancel on an open sign-in also clears its
>   half-made account at Composio. The button is "Disconnect" in both modes.
>   A print-to-terminal trace was tried and removed:
>   `the_key_is_stored_nowhere_and_never_printed` forbids `println!`/`eprintln!` in
>   `composio.rs`.
> - **Google Calendar read failed in chat with a real Google 403** (`Request had
>   insufficient authentication scopes`, `ACCESS_TOKEN_SCOPE_INSUFFICIENT`, method
>   `calendar.v3.CalendarList.List`; read from the stored step in `poiesis.db`). The
>   account Composio holds has no calendar scope. Likely cause (not proven): a
>   permission left unticked on Google's sign-in page. The Connect-mode scope hint
>   no longer sends people to "Manage Auth" (never verified to exist for a `ck_`
>   key) and no longer says reconnecting won't help; it says Disconnect, then
>   connect again and allow everything. Check after the fix: disconnect and
>   reconnect Google Calendar with every box ticked and ask "do I have a meeting
>   tomorrow".
> - Tests no longer read the real keychain (`has_key`/`current_mode` under `cfg(test)`),
>   so `agent::run::tests::composio_offers_exactly_its_four_meta_tools_and_only_with_a_key`
>   passes on a machine with a real key saved.
>
> **Resume here (read this first). What is left:**
> 0. **Phase 6, first:** record what is still missing (a successful `add`, a `list`
>    answer; command in the Phase 6 update above), read the files, add tests from
>    them, then do the manual pass below with a `ck_` key and again with an `ak_`
>    key. Done already: Apps page connect for Google Calendar and a chat read of the
>    schedule. In Connect mode also check: ask the model to connect an app that
>    isn't connected (the in-chat card, with Composio's `{name, action}` shape);
>    remove an app in Composio's dashboard, then ask about it in chat (it must get a
>    card, not "already connected"); a sign-in card for an app connected in Composio
>    elsewhere flips straight to "Connected."; "Open again" reopens the same
>    sign-in; Cancel leaves nothing red afterwards; Remove from list; a Gmail send
>    shows the message text in the panel; the `/connect` menu lists a connected app
>    that is not on the common list. *(Items 1-4 below were written before Phase 6;
>    where they say the persona apps editor saves as you click, that is fixed: it
>    saves with the editor's Save.)*
> 1. **Manual pass in the running app (Erich, needs the real key):** Apps page
>    (disclosure above key field, save key, Browse apps, Connect, Disconnect,
>    Reconnect, What I may do per app); sign-in card in chat (ask the model to
>    "add a calendar event" with Google Calendar unconnected; card appears, the
>    turn ends, sign in, chat resumes); Phase 0 permission panel with a real
>    connector; `/connect gmail`, `/apps`; persona "Apps this persona may use";
>    Usage "Connected apps"; `/checkup` Composio line; hub dot on Apps when a
>    sign-in expires. Record one real `tools/list`, one `MULTI_EXECUTE` reply
>    and one explicit-selection refusal under `fixtures/composio/` (CAP-0).
> 2. **Commit** (nothing committed yet; the working tree also holds earlier
>    unrelated changes, so stage by file).
> 3. **Not done on purpose / known gaps:** *(CAP-14 was built 2026-10-23, see above.)*
>    per-tool "Choose tools..." in the persona editor (grants are per app, tools
>    `*`; a `ck_` key has no tool list, so it would have to pick from tools used or
>    a typed slug; lowest value, parked); app logos are bundled
>    (`public/app-logos/`, 1,531 of 1,608), so the CSP stays without remote
>    images; no diagnostics export exists in the app, so nothing to redact (when
>    one is added, put the key, user id and session id on its redaction list); a
>    Composio checkup probe may hit the 4 s area timeout on a slow link. *(Built
>    2026-10-19 night, see the update above: the "Apps:" tooltip, per-tool kinds
>    for Composio apps, several accounts with a `ck_` key, and "which account?"
>    without a recording. The persona apps editor saves with the editor's Save.)*
> 4. **Frontend step 5 as built:** `lib/api.ts` (types, `composio*` commands,
>    `accountWaiting/accountBroken`), `components/Apps/{ComposioSection,
>    BrowseApps,PersonaApps,useComposio}.tsx` + `ComposioApps.css`,
>    `components/Blocks/{ConnectBlock,Frame}.tsx` (`connect` BlockKind),
>    `routes/Apps.tsx` (composio row filtered out, "Add your own server"
>    folded once anything is connected), store `appAccounts` /
>    `appsNeedAttention` / `refreshApps` + hub badge in `SettingsHub.tsx`,
>    `lib/signin.ts` (toast watcher for `/connect`), `/apps` and `/connect` in
>    `shared/commands.json` with ArgSource `apps` (connected first), Usage
>    "Connected apps" section, tests in `components/Apps/ComposioApps.test.tsx`.
>    Policy per app: `set_connector_policy_cmd` takes an optional `app`.
>
> *Everything below this line in the status block is the older step-by-step
> record; steps 1-6 and 5 are now all built, kept for the design notes.*
>
> *CAP-0 is no longer a blocker.* The API shapes were checked against
> Composio's v3.1 reference through Context7 (`/websites/composio_dev`) and are
> encoded in `connectors/composio.rs`. Fixtures under `fixtures/composio/`
> (recorded with a real key) are still wanted as regression tests, but nothing
> waits on them. The key is entered **in the app** (Settings, Apps) and stored
> in the keychain (`secrets::SERVICE_COMPOSIO`), like provider keys; never
> read it from `.env`, never ask for it in chat. Differences found against the
> plan: the key is **not** required to start with `ak_` (the docs don't say so;
> validation is "a session can be created"); the REST base is **v3.1** for
> session, link, connected_accounts, auth_configs and toolkits; the link
> response is `{link_token, redirect_url, connected_account_id}`; account
> statuses are `INITIALIZING | INITIATED | ACTIVE | FAILED | EXPIRED |
> INACTIVE | REVOKED`.
>
> *Built and green (759 Rust tests, 783 frontend tests, `tsc` clean):*
> - `src-tauri/src/connectors/composio.rs`: `Api` (REST, `x-api-key` header,
>   15 s/30 s timeouts, 20 MB cap, https-on-`composio.dev` trust check,
>   debug-only `POIESIS_COMPOSIO_API` stub), `ensure_session` (reuse, recreate
>   on 404 / multi-account off / new custom auth config, once per boot per
>   pair), `mcp_client`, `validate_key`, `refresh_tools`, `refresh_accounts`,
>   `catalog` (10 min cache by key fingerprint), `plan_label` (CAP-7 rules),
>   `scope_hint` (CAP-9), `link_error_text`, `curated()` over
>   `shared/apps.json` (24 apps).
> - `mcp/client.rs`: `McpClient::new_with_header` (header auth, 10 min call
>   timeout) and the 20 MB response cap for every HTTP connector.
> - DB (schema v31, same version as MCG, not yet released): `app_accounts`,
>   `persona_app_grants`, `app_call_log` tables plus `Db` helpers
>   (`composio_connector`, `add_composio_connector` with NULL url,
>   `list/replace/upsert/delete_app_account(s)`, `persona_app_grants`,
>   `set_persona_app_grants`, `log_app_call`, `app_calls_since`). Deleting a
>   persona deletes its grants.
>
> *Step 1 BUILT (2026-10-18, 20 composio tests green):* `commands/composio.rs`
> registered in `lib.rs` (`composio_status/save_key/remove_key/refresh/accounts/
> connect/disconnect/catalog/usage_cmd`, `get/set_persona_app_grants_cmd`);
> testable logic in `composio::{connect, disconnect, sync_accounts, app_label,
> valid_slug, health_of, set_last_error}`; `probe_connector` branches on
> composio. Still owed from step 1: the Apps page must filter the composio row
> out of the ordinary list (frontend, step 5).
>
> *Steps 2, 3 and 4 BUILT (2026-10-18, 789 Rust tests green):* new
> `agent/app_calls.rs` (grants incl. persona walk via `parent_conversation_id`,
> `offered`, `description_prefix`, `targets`, `app_of`, `resolve`, `plan`,
> `parts_of`, `send_counts`, `connect_request`, `describe`). `connector_gate`
> `Call` gained `parts: &[Part]` and `said`, `admit` is per app and counts
> targets, `run_approved` handles composio. `run.rs`: `ToolRegistry::build`
> composio branch, `TurnCtx::composio_call` / `connect_apps`,
> `waiting_on_person` ends the turn after the batch, `call_mcp_tool` takes a
> hint closure (scope note) and builds the composio client, `describe()` speaks
> app. A test seam: `composio::tests_pretend::key(bool)` (per thread) fakes
> `has_key()`. The `connect` block data is `{app, name, label, state:"waiting"}`
> with no address; the frontend must create the link by calling
> `composio_connect_cmd(app, label)` when the person clicks. Remaining
> backend: none. **Step 6 BUILT too** (791 Rust tests green): the checkup's
> Composio line comes from `probe_connector` (which now also refreshes the
> accounts), `checkup::expired_lines` adds "My sign-in to X (label) has
> expired." with Open Apps, and CAP-T-10 is `the_key_is_stored_nowhere_and_never_printed`.
> There is no diagnostics export in the app, so there is nothing to redact yet:
> when one is added, put the key, user id and session id in its redaction list.
> Remaining everything else: step 5 (frontend).
>
> *Not built, in this order (each is a done-feature to stop at):*
> 1. (DONE, see above) **Tauri commands** in a new `commands/composio.rs`, registered in
>    `lib.rs`: save key (validate with `validate_key`, then `store_key`,
>    `ensure_row`, `refresh_tools`, `refresh_accounts`), remove key, status
>    (`{configured, user_id_present, last_error}` plus accounts; never the key,
>    user id or session id), accounts, connect (CAP-6/7: `plan_label`, then
>    `Api::link`, upsert an INITIATED row, open `redirect_url` with the opener
>    plugin; the URL is never returned to the frontend), disconnect (check the
>    account belongs to this user id and app, then `delete_account`), catalog
>    (curated fallback, partial flag, query-scoped second call), persona
>    grants get/set, usage (`app_calls_since`). `probe_connector` in
>    `commands/connectors.rs` must branch on `transport == "composio"` to call
>    `refresh_tools`; `list_connectors_cmd` already returns the row (url NULL)
>    and the Apps page must filter it out of the ordinary list.
> 2. **Registry (CAP-4, CAP-10, CAP-11)** in `ToolRegistry::build` (`run.rs`):
>    today it skips a connector with no `url`, so branch on composio first.
>    Offer only `composio::OFFERED` from the cached tools, and only when
>    `has_key()`. Prefix each description with the CAP-4 sentence (connected
>    apps from `app_accounts` ACTIVE, narrowed by persona grants; walk
>    `parent_conversation_id` so a GENERAL child inherits the lead's persona).
>    Append the CAP-11 mail sentence when an enabled Mail account exists. A
>    persona whose grants are only the `''`/`[]` row gets no Composio tools.
>    Add `McpBinding.transport == "composio"` handling in `call_mcp_tool`
>    (client from `composio::mcp_client`).
> 3. **Verdict (CAP-5)** in `agent/connector_gate.rs` or a new
>    `agent/app_calls.rs`: `targets(tool, args)` for `COMPOSIO_MULTI_EXECUTE_TOOL`
>    (`args.tools[].{tool_slug, arguments}`; anything else is `Unreadable`,
>    refused without a network call), app slug = longest connected slug that
>    prefixes the tool slug, grants check first (refusal never lists what is
>    granted), call kind = strictest target (`classify` already handles
>    `GMAIL_SEND_EMAIL`), send cap counts targets. This needs `admit` to take
>    several apps: add `parts: &[Part{app, kind}]` and `said: Option<String>`
>    to `connector_gate::Call`, check every app's policy, ask once naming all,
>    and save "always" for each. Dispatch must route composio tools around the
>    generic gate: discovery tools run; MULTI_EXECUTE goes through the verdict;
>    after a failed result append `scope_hint`; log each target to
>    `app_call_log`. CAP-11's "I'd normally use your Mail account" line goes in
>    the summary when asking for a Gmail/Outlook target and a Mail account
>    exists (the plan says "same address"; Composio does not tell us the
>    address, so this is by app, not by address).
> 4. **Connect from chat (CAP-6)**: intercept `COMPOSIO_MANAGE_CONNECTIONS` in
>    `dispatch` (do not forward it to Composio). For each toolkit with no
>    ACTIVE account, add a `connect` block (`db.add_block` + `sink.block`;
>    data `{app, name, logo, label}`, no URL). Tell the model to stop. Add an
>    `AtomicBool` on `TurnCtx` set by that handler and, in the loop at
>    `Next::Calls(calls) => cx.dispatch_batch(..)` (`run.rs`, near line 1879),
>    end the run with `StopReason::Completed` after the batch when set (no new
>    `StopReason` variant: the UI treats unknown ones as interrupted). On
>    connect, the block calls `sendBlockAction` (store.ts) with "{App} is
>    connected ({label})."
> 5. **Frontend**: `api.ts` types and commands; `Apps.tsx` (disclosure before
>    the key field, key card, connected list grouped by app with status dots,
>    Browse modal, Add your own server collapsed); `connect` in
>    `lib/types.ts` `BlockKind` and `BlockRenderer.tsx` (ConnectBlock: label
>    field, Sign in, polling accounts every 3 s for 10 min and on window focus,
>    Try again); `appsNeedAttention` + `badgeFor` hub badge; persona editor
>    "Apps I may use"; `/connect` and `/apps` in `shared/commands.json` with a
>    new `ArgSource` `apps` (look at how `argChoices` in `lib/commands.ts`
>    sources are done) and handlers; step lines (`describe()` in `run.rs` for
>    composio: "Read"/"Changed"/"Sent" plus the app label, batch summary);
>    Usage row (`composio_usage`); CheckupCard line.
> 6. **CAP-13**: `connector_line("Composio", ...)` in `agent/checkup.rs`
>    (5 s session get, expired accounts as a second line) and the diagnostics
>    export (kind, session yes/no, accounts, last 20 errors; add the key, user
>    id and session id to the redaction list). CAP-T-10: a grep test that the
>    key appears nowhere.
> 7. **Tests still owed**: CAP-T-3 (registry offers exactly `OFFERED`),
>    CAP-T-4 (`targets()` table incl. malformed), CAP-T-5 (batch with one send
>    asks once naming both apps; cap counts targets), CAP-T-6 (grants), CAP-T-7
>    (connect block + stop + no URL in stored rows), CAP-T-10, CAP-T-11,
>    CAP-UI-T-1, CAP-UI-T-2.
> 8. **Skipped on purpose**: CAP-14 (needs CLI_ENGINES `MCPS-1`, which does not
>    exist). Turning an explicit-selection refusal ("which Gmail?") into an
>    `ask_user` question needs a recorded refusal to match on; until then the
>    refusal reaches the model wrapped and it can ask with `ask_user` itself.
>
> *Already-known gotchas:* `cargo test --lib` takes about 2 minutes to build;
> run `npx tsc --noEmit -p .` and `npx vitest run` for the frontend. The one
> failing frontend test, `icons.test.ts` (inline SVG in `WindowControls.tsx`),
> fails without these changes too. Two design-hook findings were fixed by
> dropping thick side borders; use text colour (`--cloud`) for "outbound".
>
> **Phase 0 as built, and where it differs from the text below:**
> - Schema is v31. `connector_policy` and `app_send_log` are in `schema.sql`;
>   the `app` column is always `''` until Composio exists.
> - The send cap's day is the **UTC** date (`connector_gate::send_day`, over
>   `memory::today`), not local: no date crate is a dependency. It is one
>   function if a local boundary is wanted.
> - `DRAFT` cancels only the create+noun rule. `GMAIL_SEND_DRAFT` is still a
>   send, because it sends. A leading removal verb (DELETE, REMOVE, ARCHIVE,
>   CANCEL...) makes `DELETE_POST` a change, not a send.
> - The action starts at the first known verb, so a toolkit prefix
>   (`GOOGLE_CALENDAR_`, `notion-`) is not read as the verb.
> - A send is counted when it is admitted, not when the server answers.
> - A headless call is parked as a `change_proposals` row with target `app`;
>   accepting it runs the exact stored arguments (`run_approved`).
> - The autonomy class `app_send`: `off` withdraws send tools from the
>   registry, `auto` lets every connector send up to its cap, `ask` (default)
>   leaves it to each connector's own policy.
> - The outbound accent is `--cloud`; no email-specific colour existed.
>
> **Settled decisions (Erich, 2026-10-13):**
> 1. A third-party relay is acceptable for SaaS apps, **opt-in, labelled
>    plainly**, with the person's own Composio key. Native Mail (IMAP direct)
>    stays the default for email.
> 2. The connector gaps (MCG) ship first, independent of Composio.
>
> PRES-0 (first-person copy, `plans/done/POIESIS_PLAN.md`) binds every `-UI-`
> task here. Composer interactions go through the `/` menu
> (`shared/commands.json`) and never new syntax.

---

## The decision this plan makes

**Composio is a connector, not a subsystem.**

Poiesis already has MCP connectors: a `connectors` table, Streamable HTTP and
stdio in `mcp/client.rs`, a per-run client pool in `run.rs`, and Settings →
Apps. Composio becomes a connector of a new **kind** (`composio`) whose URL and
credentials come from a managed session, not a pasted value. Everything an MCP
tool already gets (the run pool, the activity log, the timeline step, plan-first
filtering) it gets for free. Everything new (the per-call verdict, accounts,
the connect card) is built so the ordinary MCP connectors benefit too, wherever
that applies.

What this buys:
- Hundreds of apps without one line of per-app code.
- Sign-in in the system browser. OAuth tokens never touch Poiesis.
- One approval language for every outside app, local MCP server or hosted.

What it deliberately gives up (for now):
- A Poiesis-run broker that hides a shared key (`CAP-P1`). Each person brings
  their own free Composio key, like a provider key.
- Triggers ("when a new email arrives"), which need a public webhook
  (`CAP-P2`).
- Composio's remote code sandbox (workbench/bash meta-tools). Never offered.
  Poiesis has its own sandbox.

---

## What this is not

- **Not a replacement for Mail.** Mail over IMAP is local, direct, and never
  touches a third party. When a Mail account is set up, mail goes there. See
  CAP-11.
- **Not a replacement for the browser.** BRW drives a page. Composio calls an
  API. Where both could work, the API is the better route; where only a page
  exists, BRW stays the route.
- **Not a store for tokens.** Poiesis stores one secret, the Composio project
  key, in the OS keychain. Provider tokens live at Composio.
- **Not silent.** Every app call shows in the timeline with the app's name.
  Every change asks, unless the person has said otherwise for that app.
  Every send asks, unless the person has said otherwise **and** a daily cap
  holds.

---

## Who feels what

1. **First connect, from the chat.** "Put lunch with Ana on Thursday in my
   calendar." I'm not connected to Google Calendar yet. Instead of telling
   the person to go to Settings, I show a card in the chat: *"I need your
   Google Calendar for this. Sign in once and I'll carry on."* with
   **Sign in to Google Calendar**. The browser opens Google's own consent
   page. The card turns to *"Connected as work."* and the turn continues by
   itself. The event is a change, so it still asks (story 3).
2. **Reading is quiet.** "What did the team ship in Linear this week?" The
   timeline shows *"Read Linear · list issues"* and the answer arrives. No
   prompts for reading, by default.
3. **Changing asks, once or always.** "Move the standup to 10." The panel
   shows *"Change an event in Google Calendar (work): Standup, 09:30 → 10:00"*
   with **Allow once / Always for Google Calendar changes / No**.
4. **Sending always asks.** "Post the release notes in #general." The panel
   is the send variant, marked in the outbound colour: *"Post to Slack
   (acme): #general"*, with the message text in full. "Always" here means
   *"Allow up to 25 a day without asking"*, and the cap is visible.
5. **Two accounts.** The person connects Gmail twice, as `work` and
   `personal`. A request that could go to either asks which one. A new
   sign-in never quietly replaces the old one.
6. **Personas keep their lane.** The "Research" persona may read Notion and
   Linear and nothing else. A Task that runs at night under it can't post to
   Slack, even though Slack is connected. It says so in its report instead of
   trying.
7. **A wall I can explain.** Gmail refuses a filter change for a missing
   permission. I don't retry and I don't say "try reconnecting". I say what's
   missing and where to add it.
8. **Honesty about the relay.** Settings → Apps says, before the key field:
   *"Apps you connect here run through Composio's servers. Composio keeps the
   sign-ins and sees what I send and receive. Mail you set up under Mail never
   goes through them."*

---

# Part I - MCG: gates for every MCP connector (ships first)

Found while reading for this plan (2026-10-13):

- `run.rs` `dispatch` sends any `registry.mcp` tool straight to
  `call_mcp_tool`. `McpBinding.read_only` is consulted only in plan-first
  (`refuses_in_plan_first`). **A connector tool that changes or sends
  something runs without any approval today.** Native Mail, by contrast, is
  gated by `autonomy_gate("email_send")`.
- `call_mcp_tool` returns the server's text as-is. It never passes through
  `toolsets::mark_untrusted`, so a mail body or web page fetched through an
  MCP server enters the prompt **unwrapped**. That breaks Capabilities §3
  ("outside text looks like outside text").

Both apply to every connector, Composio or not.

### MCG-1 - Connector output is outside text

- `call_mcp_tool` gains the `ToolContext`-equivalent bits it needs (sink,
  call id) and wraps every successful result with
  `mark_untrusted(ctx, &format!("{connector_name} · {tool}"), &output)`.
- Error text from the server is wrapped too. Errors are where injected text
  likes to hide.
- The risk scan, the `Untrusted` event and the activity row come along
  unchanged, so the existing untrusted UI lights up for connectors with no
  frontend work.
- Results still go through HRN-8 capping after wrapping, as built-in results
  do.

### MCG-2 - Three kinds of connector call

A pure function in a new `src-tauri/src/agent/connector_gate.rs`:

```rust
pub enum CallKind { Read, Change, Send }

/// What one target tool does. `hint` is the server's own annotation when it
/// gave one; the name is the fallback.
pub fn classify(tool: &str, hint: &ToolHints) -> CallKind;
```

Order of evidence:
1. `readOnlyHint: true` → `Read`.
2. The name's action tokens (split on `_`, `-`, camelCase, and with any
   `mcp__server__` or `TOOLKIT_` prefix dropped) say **send** →
   `Send`. Token lists ported from OMB `shared/outbound.ts`:
   - send verbs: SEND, REPLY, RESPOND, FORWARD, POST, PUBLISH, TWEET,
     RETWEET, BROADCAST, INVITE, SHARE, REFUND, CHARGE, PAY, PAYOUT,
     TRANSFER.
   - create + sent noun: CREATE/ADD/NEW together with MESSAGE, COMMENT,
     POST, EMAIL, MAIL, INVOICE, PAYMENT, REFUND, REPLY, TWEET, DM.
   - DRAFT anywhere → not a send.
3. First token is a read verb (GET, LIST, FETCH, SEARCH, READ, FIND,
   RETRIEVE, LOOKUP, DOWNLOAD, VIEW, CHECK, COUNT) → `Read`. A read verb
   first wins over a send noun later (`GET_POST` reads a post).
4. Anything else → `Change`.

`destructiveHint` or `openWorldHint` never downgrades the result. A server
saying "read-only" is trusted for `Read` only when the name doesn't say send.
Precedence: send by name > read by hint > read by name > change.

### MCG-3 - The gate in `dispatch`

Before `call_mcp_tool`:

| Kind | Default | Where the person decides |
|---|---|---|
| Read | runs | Settings → Apps per connector: *Ask before reading* (off) |
| Change | asks every time | the panel: **Allow once / Always for {app} changes / No** |
| Send | asks every time | the panel: **Send once / Up to N a day for {app} / No** |

- Asking uses `PermissionRequest::capability(id, "app", summary, target)` (a
  new kind beside `domain`, `screen`, `open-app`), the same panel and the same
  `Decision` oneshot. **No second consent mechanism.**
- `summary` is first person and names the app, the account and the target
  (`"Post to Slack (acme): #general"`). The `diff` field carries the
  arguments, pretty-printed and capped at 4 KB, so the person sees what goes
  out.
- New autonomy class `("app_send", "ask")` in `AUTONOMY_DEFAULTS`. `Off`
  withdraws send-kind connector tools from the registry entirely. `Auto` is
  only offered together with a daily cap (MCG-4).
- "Always for {app} changes" is stored per connector (`connector_policy`,
  MCG-5) and is visible and revocable in Settings → Apps.
- **Headless runs** (Tasks, background children with nobody watching):
  - Read runs.
  - Change runs only where the policy is already "always".
  - Send runs only where the policy is "allow" and the cap holds. Otherwise
    the call returns *"I need you to approve this: {summary}. I've left it
    for you."* and an approval waits in the inbox (as MAIL-3's proposal does
    for mail).
  - Never a hang.
- Plan-first keeps working: `refuses_in_plan_first` switches from
  `!binding.read_only` to `classify(..) != Read`.

### MCG-4 - Daily send cap

- `app_send_log(connector_id, day, count)`. Every `Send` that ran increments
  it, whoever approved it.
- Default cap 25 per app per day (OMB's default), editable 1-1000 in
  Settings → Apps.
- At the cap, an "allow" policy falls back to asking, with the reason in the
  summary: *"I've already sent 25 things through Slack today."*

### MCG-5 - Schema

Next free schema version (v31 if `CLI_ENGINES_PLAN` hasn't taken it; else
v32):

```sql
CREATE TABLE IF NOT EXISTS connector_policy (
  connector_id TEXT NOT NULL,
  app          TEXT NOT NULL DEFAULT '',  -- '' = the whole connector; else a Composio toolkit slug
  read_policy  TEXT NOT NULL DEFAULT 'run',   -- 'run' | 'ask'
  change_policy TEXT NOT NULL DEFAULT 'ask',  -- 'ask' | 'allow'
  send_policy  TEXT NOT NULL DEFAULT 'ask',   -- 'ask' | 'allow'
  send_cap     INTEGER NOT NULL DEFAULT 25,
  tool_overrides TEXT,                        -- MCG-7: JSON {tool_name: 'read'|'change'|'send'}
  updated_at   INTEGER NOT NULL,
  PRIMARY KEY (connector_id, app)
);
CREATE TABLE IF NOT EXISTS app_send_log (
  connector_id TEXT NOT NULL,
  app          TEXT NOT NULL DEFAULT '',
  day          TEXT NOT NULL,   -- local date, YYYY-MM-DD
  count        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (connector_id, app, day)
);
```

`app` lets Composio hold a policy per toolkit (Gmail ≠ Slack) inside one
connector row. Ordinary connectors use `''`.

### MCG-6 - Activity log with substance

`call_mcp_tool`'s activity row today is `"{connector}: {tool}"`. It becomes
`"{connector} · {kind} · {tool}"`. Approvals and refusals are logged with who
decided (`once`, `always`, `cap`, `headless-refused`). This is the decision
log OMB writes per allow/deny, on the table Poiesis already has.

### MCG-UI-1 - The panel learns "app"

`PermissionPanel` draws `capability === "app"`:
- The headline is `summary`.
- Below it, the arguments as a compact key/value list (from `diff`), with
  long values folded.
- Buttons per kind as in MCG-3. Send uses the outbound accent (the colour
  `email_send` proposals use) and states the cap on the "Up to N a day"
  button.

### MCG-UI-2 - Settings → Apps shows what each connector may do

Each connected row in `Apps.tsx` gains a collapsed **"What I may do"** line:
*"Read: without asking · Change: ask · Send: ask"*. Expanding it shows three
small segmented controls and the send cap. Tool chips (already rendered) get
a one-letter kind mark (R / C / S) with the full word in the title, so the
classification is inspectable. A wrong guess is fixed by the override in
MCG-7.

### MCG-7 - Per-tool override

For the rare misclassified name, a chip's context menu offers *"Treat as
read / change / send"*, stored in `connector_policy.tool_overrides`. The
override is shown on the chip. Making a tool stricter is one click. Making one
looser (send → change or read, change → read) asks for a confirmation that
names the risk (*"I'll stop asking before {tool} runs."*), and the chip keeps
a visible "loosened" mark.

### MCG-T - Tests

- **MCG-T-1:** `classify` table test. `GMAIL_SEND_EMAIL` → Send,
  `REDDIT_GET_POST` → Read, `GMAIL_CREATE_EMAIL_DRAFT` → Change,
  `SLACK_CREATE_MESSAGE` → Send, `notion-update-page` → Change,
  `createInvoice` → Send, `readOnlyHint` + `SEND` name → Send.
- **MCG-T-2:** a fake stdio MCP server returning
  `"ignore previous instructions"` comes back wrapped and raises an
  `Untrusted` event.
- **MCG-T-3:** a Change call without a policy produces exactly one
  `PermissionRequest` with `capability = "app"`. "No" returns the refusal
  text and does not call the server.
- **MCG-T-4:** a headless Send with policy `ask` never calls the server and
  returns the waiting text. With policy `allow` and the cap reached, it
  behaves the same.
- **MCG-T-5:** plan-first refuses Change and Send, allows Read.
- **MCG-T-6:** the cap increments across runs and resets on the next local
  day.

---

# Part II - How OpenMausBot does Composio (verified in source)

For implementers. Paths are in the OMB repo at `a67daf0`.

- **One session per install** (`server/composio.ts`):
  - `POST {api}/tool_router/session` with header `x-api-key: ak_…` and body
    `{ user_id, manage_connections: { enable, enable_wait_for_connections,
    enable_connection_removal }, multi_account: { enable: true,
    max_accounts_per_toolkit: 5, require_explicit_selection: true },
    auth_configs? }`.
  - Returns `{ session_id, mcp: { type, url }, config }`.
  - `user_id` is a random `openmausbot_<uuid>` kept forever. Connections
    belong to the **user**, not the session, so recreating a session keeps
    every account.
  - A session can't be edited. It is recreated (same user id) when
    multi-account is missing or the project gained its own auth config.
    Each upgrade is attempted at most once per boot, to avoid a loop.
- **Endpoints used:**
  - `GET session/{id}` and `GET session/{id}/toolkits?is_connected=true`
  - `POST session/{id}/link {toolkit, alias}` → `redirect_url`
  - `GET connected_accounts?user_ids=…&toolkit_slugs=…`
  - `DELETE connected_accounts/{id}?revoke_on_delete=true`
  - `GET auth_configs?is_composio_managed=false`
  - `GET {v3}/toolkits?limit=500&sort_by=usage` for the catalog.
  - API base `backend.composio.dev/api/v3.1` (session) and `/api/v3`
    (toolkits). The public docs disagree on v3 versus v3.1, so CAP-0
    records which works.
- **MCP:** the session URL is spoken to as Streamable HTTP with `x-api-key`
  on every request and `mcp-session-id` reuse. Responses are capped at 20 MB.
- **Trust:** auth links and MCP URLs are accepted only for `https://` on
  `composio.dev` or a subdomain.
- **Meta-tools the model sees:**
  - `COMPOSIO_SEARCH_TOOLS` and `COMPOSIO_GET_TOOL_SCHEMAS` (discovery)
  - `COMPOSIO_MULTI_EXECUTE_TOOL` (1-50 `{ tool_slug, arguments }`)
  - `COMPOSIO_MANAGE_CONNECTIONS` and `COMPOSIO_WAIT_FOR_CONNECTIONS`
  - `COMPOSIO_REMOTE_WORKBENCH` and `COMPOSIO_REMOTE_BASH_TOOL`, which run
    arbitrary code. OMB treats them as "unrestricted, always ask".
- **Verdict** (`server/connector-verdict.ts`): unpack `MULTI_EXECUTE`, judge
  each `tool_slug` against the bot's grants, and deny any shape it can't read.
  Refusals name the refused tool and never list what *is* granted.
- **Prompt** (`server/system-prompt.ts`): one sentence. Find with SEARCH,
  read the schema, run with MULTI_EXECUTE, and reach for these before saying
  there's no access.
- **In-chat connect** (`server/setup-mode.ts`): the model makes
  `MANAGE_CONNECTIONS` its last call and ends the turn. The UI shows a
  Connect card, and the run continues once connected.
- **Scope hint** (`server/connector-scope-hint.ts`): when a call fails with
  `ACCESS_TOKEN_SCOPE_INSUFFICIENT`, a note is appended telling the model not
  to retry and naming the permission and where to add it.
- **The key stays in the harness:** CLI agents reach Composio only through a
  loopback relay with a per-boot token.
- **Managed mode:** a Cloudflare Worker holds the project key and issues each
  install a bearer token (hash in D1). Poiesis does not copy this (`CAP-P1`).

---

# Part III - CAP: Composio as a connector kind

### CAP-0 - Fixtures and facts first

Before code:
1. With a free Composio key (Erich's), record the raw request and response
   bodies for: session create, session get, toolkits (connected), link,
   connected_accounts, delete, auth_configs, MCP `initialize`, `tools/list`,
   one SEARCH, one GET_TOOL_SCHEMAS, one MULTI_EXECUTE (Gmail list), and one
   MANAGE_CONNECTIONS. Save them as `fixtures/composio/<name>.json`. **Strip the key and
   any token before saving.** Fixtures are hand-checked for secrets before
   commit.
2. Note which API base answers (v3 or v3.1) and the exact meta-tool names
   `tools/list` returns. CAP-4 and the verdict key off these names.
3. Re-check the pricing page and Composio's terms for use inside a
   distributed desktop app where each user brings their own key. Record the
   date and findings in this plan (as CLI-0b did for engines).

### CAP-1 - Schema

Same version as MCG-5:

```sql
-- connectors.transport gains 'composio'. url stays NULL for that kind: the
-- URL comes from the live session. config_json holds the cached tools/list
-- plus {"user_id": "...", "session_id": "..."} — non-secret identifiers.

CREATE TABLE IF NOT EXISTS app_accounts (       -- mirror of Composio's inventory, for the UI
  id           TEXT PRIMARY KEY,                -- Composio connected-account id
  connector_id TEXT NOT NULL,
  app          TEXT NOT NULL,                   -- toolkit slug, canonical ('twitter', never 'x')
  label        TEXT,                            -- the person's alias: 'work', 'personal'
  status       TEXT NOT NULL,                   -- ACTIVE | INITIATED | EXPIRED | FAILED ...
  updated_at   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS persona_app_grants (  -- CAP-10
  persona_id   TEXT NOT NULL,
  connector_id TEXT NOT NULL,
  app          TEXT NOT NULL DEFAULT '',         -- '' = whole connector
  tools        TEXT NOT NULL DEFAULT '*',        -- '*' | JSON array of exact tool names | '[]'
  PRIMARY KEY (persona_id, connector_id, app)
);
```

At most one `composio` connector row may exist. `app_accounts` is a cache,
refreshed from Composio. Composio is the truth.

### CAP-2 - Key and session lifecycle

New `src-tauri/src/connectors/composio.rs`:

- **Key:**
  - Stored with `secrets::set_secret(SERVICE_COMPOSIO, "project", key)` (new
    constant `"poiesis-composio"`), never in SQLite, settings or logs.
  - Validated on save: it must start with `ak_`, and a session create must
    succeed. The error is said in plain words (*"Composio didn't accept
    this key."*).
- **User id:**
  - `poiesis_<uuid>`, created once, stored in `config_json`. It is never
    regenerated while the row exists.
  - Clearing the key keeps it, so pasting the key again finds the same
    accounts.
- **Session:**
  - `ensure_session()` gets the stored session. It recreates the session
    with the same user id when the session is missing (404), when
    multi-account is off, or when the project's own auth configs
    (`auth_configs?is_composio_managed=false`, the newest enabled one per
    toolkit) aren't all in the session.
  - The upgrade is attempted once per app start per (session, config set).
- **Multi-account:** `{ enable: true, max_accounts_per_toolkit: 5,
  require_explicit_selection: true }`.
- **Trust:** auth and MCP URLs are accepted only for `https://*.composio.dev`.
  A debug-only `POIESIS_COMPOSIO_API` override, compiled out of release
  builds, may point at a local stub for tests.
- **Timeouts:** 15 s for reads, 30 s for writes, 10 min for one MCP call.
  Responses over 20 MB are refused.
- **Where the key goes:** it never reaches the frontend, a child process,
  the prompt, a skill script, or an external agent (CAP-14). The Tauri
  commands return only `{configured, user_id_present, last_error}`.

### CAP-3 - MCP transport with `x-api-key`

- `McpClient::new` gains an `HttpAuth` enum: `Bearer(String)` (today) or
  `Header { name, value }`. The `composio` kind uses
  `Header { "x-api-key", key }`.
- The URL is `ensure_session().mcp.url`, resolved when the run pool first
  touches the connector (LOOP-1). It is not stored.
- When the session is recreated, the `mcp-session-id` is dropped with it.
  An MCP session id is only reused with the backend that issued it.

### CAP-4 - What the model is offered

From the cached `tools/list`, only:
- `COMPOSIO_SEARCH_TOOLS`
- `COMPOSIO_GET_TOOL_SCHEMAS`
- `COMPOSIO_MULTI_EXECUTE_TOOL`
- `COMPOSIO_MANAGE_CONNECTIONS`
- `COMPOSIO_WAIT_FOR_CONNECTIONS`, only if CAP-6 needs it; otherwise hidden

**Never offered:** `COMPOSIO_REMOTE_WORKBENCH`, `COMPOSIO_REMOTE_BASH_TOOL`,
or any other `COMPOSIO_*` meta-tool not on the list. The registry filter is
an allowlist, so a new upstream meta-tool stays hidden until someone reviews
it.

Each offered spec's description is prefixed with one sentence, so it renders
with the tool specs (outside the golden-gated system prompt, like the
CLI_ENGINES rule blocks):

> *"The person's connected apps ({named list of connected toolkits, or "none
> yet"}). Find a tool with COMPOSIO_SEARCH_TOOLS, read its arguments with
> COMPOSIO_GET_TOOL_SCHEMAS, run it with COMPOSIO_MULTI_EXECUTE_TOOL. If an app
> isn't connected, call COMPOSIO_MANAGE_CONNECTIONS for it as your last call
> this turn; the person signs in and you continue."*

The CAP-11 routing sentence is appended to the same description.

### CAP-5 - The verdict for Composio calls

`connector_gate.rs` grows `targets(tool, args) -> Result<Vec<Target>, Unreadable>`:

- `COMPOSIO_MULTI_EXECUTE_TOOL`:
  - `args.tools` is a non-empty array whose entries each carry a string
    `tool_slug` (or the legacy single `{tool_slug}`) → one `Target` per
    entry, with that entry's `arguments`.
  - Anything else → `Unreadable`, refused with *"I couldn't read which app
    tools this call runs, so I didn't run it."*
- Discovery and connection meta-tools → no targets (they run no app tool).
- A direct toolkit tool name (if Composio ever lists one) → itself.

Then, per call:
1. **Grants (CAP-10).** Every distinct target must be granted to the run's
   persona. Otherwise refuse *"{TOOL} isn't one I'm allowed to use here. Ask
   me to use it and you can grant it under this persona."* The refusal never
   lists what is granted.
2. **Kind.** Classify each target with MCG-2. **The call's kind is the
   strictest of its targets.** A batch with one send is a send.
3. **Gate.** MCG-3 with policy looked up per `(connector, app)`, where `app`
   is the toolkit slug from the target name (longest connected slug that
   prefixes it, so `bland_ai` beats `bland`; fallback: the first segment).
   A batch spanning two apps asks once and names both.
4. **Summary.** Built from the targets: app label, account label when the
   arguments name one, and the most telling argument (recipient, channel,
   event title), e.g. *"Post to Slack (acme): #general"*.

Send-cap counting counts **targets**, not calls. A batch of 10 sends counts 10.

### CAP-6 - Connecting from the chat

When the model calls `COMPOSIO_MANAGE_CONNECTIONS` for a toolkit:
- Poiesis does **not** forward the raw result to the model. It reads the
  `redirect_url` (validated per CAP-2) and emits a `connect` block (new
  `BlockKind`, CA-UI-4) on the assistant message: `{ app, label, logo,
  state: "waiting" }`.
- The URL goes only to the block's backend handle. It is never written into
  the transcript, the prompt, or a stored message. The block asks the
  backend for the link at click time (OMB: links stay in UI memory, and a
  link older than 10 minutes is minted again).
- The model gets back: *"I've asked the person to sign in to {App}. Stop
  here; I'll continue when it's connected."* The run ends its turn
  (`StopReason::WaitingOnPerson`, the same family as `ask_user`).
- **When connected** (detected by polling `connected_accounts` every 3 s
  while the block is visible and for up to 10 min, plus a check on window
  focus), the block turns *"Connected as {label}."* and the conversation
  continues with a system note *"{App} is connected ({label})."*, the same
  mechanism `ask_user` answers use to resume.
- **If it fails or expires**, the block says so and offers **Try again**,
  which mints a new link under the same label. It never creates a second
  account silently.

### CAP-7 - Accounts and labels

- **Labels:**
  - Every new sign-in for an app that already has an account needs a label.
    The first one gets `default` unless the person types one.
  - Labels are 1-64 printable characters and unique per app,
    case-insensitively.
  - Retrying when every existing account is `INITIATED`, `INITIALIZING` or
    `EXPIRED` uses a generated label, `retry-<short>`, and never deletes
    anything. `EXPIRED` can describe a formerly working grant.
- **Limits:** at most 5 usable accounts per app. At the limit, the person is
  told to disconnect one.
- **Disconnect:**
  - Removes exactly one account (`DELETE …?revoke_on_delete=true`) after
    checking it belongs to this user id and app.
  - The confirmation names the account (*"Disconnect Gmail (work)? I'll ask
    Google to revoke my access too."*).
- **Choosing between accounts:** when a call could use more than one account
  and doesn't name one, Composio's explicit-selection mode refuses it.
  Poiesis turns that refusal into an `ask_user` choice (*"Which Gmail: work
  or personal?"*) when someone is watching. In headless runs it refuses with
  the same question in the report.

### CAP-8 - The catalog

- `connectors_catalog(query?, category?)`:
  - One call to `toolkits?limit=500&sort_by=usage`, cached 10 minutes per
    key fingerprint (SHA-256 of key + endpoint, never the key itself).
  - Search is client-side over the cached list.
  - When Composio is unreachable, it falls back to a bundled curated list in
    `shared/apps.json`: about 24 common apps with slug, label, blurb, domain
    and category.
- The catalog says when it's partial (*"Showing the 500 most used apps.
  Search finds the rest."*). That needs a second, query-scoped call when
  local search finds nothing.
- Logos: Composio's `meta.logo` URL, loaded by the webview. If that's
  missing, a monogram. No favicon fetch to third-party hosts.
- Apps with no Composio-managed sign-in (e.g. `twitter`) say so on the card
  and link to "how to add your own sign-in" (CAP-9).

### CAP-9 - Errors the model can act on

The result passes through `scope_hint` after MCG-1 wrapping:
- On a failed call containing `ACCESS_TOKEN_SCOPE_INSUFFICIENT`,
  `insufficient authentication scopes` or `insufficientPermissions`, append
  a note: *"Poiesis note: {App} refused this because the connected account
  hasn't granted {permission}. Reconnecting won't add it. The person can
  create their own {App} sign-in setup (an auth config) in their Composio
  project, then reconnect under Settings → Apps. Tell them this; don't
  retry."*
  - Gmail filters, vacation, IMAP or POP need
    `https://www.googleapis.com/auth/gmail.settings.basic`.
  - Forwarding, send-as and delegates need `gmail.settings.sharing`.
  - Other apps: "the permission this action needs".
- Only failed calls get the note. Composio's discovery results quote the
  same wording as documentation (OMB #2467).
- On a `link` error mentioning missing managed auth: say *"{App} needs its
  own sign-in setup in your Composio project"*, with the steps. Don't echo
  Composio's raw hint.

### CAP-10 - Grants per persona

- **No rows for a persona means every connected app** (today's behaviour for
  connectors, kept so nothing changes on upgrade).
- **The first grant switches that persona to "only what's listed".** An
  empty list means none.
- **Grants apply to:**
  - the run's own persona,
  - delegated children (their agent type's persona; `GENERAL` inherits the
    lead's grants),
  - Tasks (the persona they run under).
- **Narrowing happens twice:**
  - When building the registry: the `{named list}` in CAP-4's description
    shows only granted apps. A persona with no app grants gets no Composio
    tools at all.
  - At call time: CAP-5 step 1.
- **Never exported or imported.** Persona exports (if any) and skills don't
  carry grants. A grant is always the person's own choice on this machine.

### CAP-11 - Routing against local reach

Appended to CAP-4's description, and rendered only when the condition holds:
- **Mail is configured:** *"For email, use the mail tools (send_mail,
  read_mail…). They go directly from this computer. Use connected apps for
  email only if the person asks for that account specifically."* With no
  Mail account, nothing is said.
- **Browser on:** nothing extra. API before page is the default the
  description already implies.

The verdict enforces the Mail rule too:
- A Gmail target while a Mail account with the same address exists asks
  with an extra line: *"I'd normally use your Mail account for this."*
- Not a refusal. The person may have reasons.

### CAP-12 - Usage

- Every Composio target that ran adds a row to the usage ledger:
  `source = "composio"`, `app`, `kind`, no cost figure. Composio bills per
  call against the person's own plan, and Poiesis doesn't know their rate.
- The Usage view shows *"Connected apps: 412 calls this month"* with a
  per-app split and a link to the Composio dashboard. No price is invented.

### CAP-13 - Checkup and diagnostics

- **Checkup:** `connector_line("Composio", …)` from `checkup.rs` with a
  5-second session get. Expired accounts become a second line: *"Your Google
  Calendar (work) sign-in has expired."*, with **Open Apps**.
- **Diagnostics export:** connector kind, session present yes/no, the
  account list (app, label, status) and the last 20 error lines. **Never**
  the key, user id or session id (the redaction list grows).

### CAP-14 - External agents get apps through Poiesis (with CLI_ENGINES)

> **Built 2026-10-23.** See the update at the top for what differs: its own
> switch, and `MANAGE_CONNECTIONS` answered by Poiesis.

Once `MCPS-1` (the MCP endpoint Poiesis serves to engines) exists:
- It may list the same four meta-tools for a delegated engine, filtered by
  the engine's persona grants.
- Calls are relayed by Poiesis, which applies CAP-5 and MCG-3 exactly as for
  its own loop. Approvals reach the same panel.
- The engine process never receives the key, the session URL or a token
  that works without Poiesis.

This is OMB's "key stays in the harness" rule. Sequenced after CLI Phase 2.

---

# Part IV - UI

### UI integration map

| Where | What changes | Task |
|---|---|---|
| Settings → Apps (`Apps.tsx`) | Disclosure, key card, catalog, connected accounts, "What I may do" per connector | CA-UI-1..3, MCG-UI-2 |
| Permission panel | `capability: "app"` variant with Change/Send buttons | MCG-UI-1 |
| Chat stream | `connect` block | CA-UI-4 |
| Timeline step lines | App name and kind, not raw slugs | CA-UI-5 |
| Persona editor | "Apps I may use" | CA-UI-6 |
| `/` menu | `/connect`, `/apps` | CA-UI-7 |
| Usage view | Connected-apps calls | CA-UI-8 |
| Checkup card | Composio and expired-account lines | CAP-13 (reuses existing card) |
| Settings hub nav | Badge when an account has expired (`badgeFor`) | CA-UI-3 |

### CA-UI-1 - Settings → Apps, new top card

`Apps.tsx` today: "Connect an app" (URL / command) then "Connected". New
order:

1. **"Hundreds of apps"**:
   - **Not configured:**
     - The disclosure (story 8) as a plain paragraph, not small print.
     - *"Composio is free to start. Make a project there, copy its key
       (it starts with ak_) and paste it here."*, with **Open Composio** (the
       system browser to the dashboard).
     - A key field (password type), **Save**, and the result inline:
       ✓ *"Composio accepted it."* or the error.
   - **Configured:**
     - One line: *"Connected through Composio · 6 apps · 8 accounts"*
     - **Browse apps**, **Change key**, **Remove key**. Remove explains:
       *"Your sign-ins stay at Composio. Paste the key again and they're
       back."*
2. **"Connected"**: Composio accounts and ordinary connectors in one list,
   grouped by app. Each row: logo or monogram, app name, account label,
   status dot, **What I may do** (MCG-UI-2), **Disconnect**. Composio rows
   carry a small "via Composio" mark. Ordinary connectors are unchanged
   apart from MCG-UI-2.
3. **"Add your own server"**: today's URL/command form, moved down and
   collapsed by default once anything is connected.

### CA-UI-2 - Browse apps

A modal over Settings → Apps:
- **Layout:**
  - A search field.
  - Category chips (from Composio's categories, at most 12, plus "All").
  - A grid of cards, each with logo, name, one-line blurb, and **Connect**
    or *"Connected · 2 accounts"* with **Add account**.
- **Connect / Add account:**
  - Asks for a label inline when the app already has an account (CAP-7).
  - Opens the system browser, and the card shows *"Waiting for you in the
    browser… · Open again"*.
  - On success, the card flips to connected without closing the modal.
- **Footer:** *"Showing the 500 most used. Search finds the rest."* when the
  catalog is partial.
- **No Composio key:** the modal shows the curated list read-only, with the
  key card's call to action at the top.

### CA-UI-3 - Account states

- Status dot colours:
  - ACTIVE: green
  - INITIATED / INITIALIZING: amber, *"waiting for sign-in"*
  - EXPIRED / FAILED: red, *"sign-in expired"*, with **Reconnect** (same
    label)
- Any expired account sets `appsNeedAttention` in the store, so `badgeFor`
  puts a dot on **Apps** in the hub nav (the same pattern as the
  External agents badge in CLI_ENGINES).

### CA-UI-4 - The `connect` block

- New `BlockKind` `"connect"`, rendered by `BlockRenderer`:
  - **Waiting:** logo, *"I need your {App} for this. Sign in once and I'll
    carry on."*, a primary button **Sign in to {App}**, and a quiet line
    *"Opens {App}'s own sign-in page in your browser. Runs through
    Composio."*
  - **After the click:** *"Waiting for you in the browser…"* with **Open
    again** and **Cancel**. Cancel ends the wait, and the model is told
    *"The person didn't connect {App}."*
  - **Connected:** a check, *"Connected as {label}."* The block stays in the
    transcript as a record.
  - **Failed / expired:** the reason in one sentence, **Try again**.
- **Label:** if the app already has an account, the block shows a small
  label field before the button, prefilled `personal` / `work` by whichever
  is missing.
- **No key:** the block says *"I can connect apps once you've added a
  Composio key."* with **Open Apps settings**. In that case the model never
  gets `MANAGE_CONNECTIONS` at all, because CAP-4 offers no Composio tools
  without a key. This state only shows for a block restored from history.

### CA-UI-5 - Step lines speak app

- `describe()` for Composio calls returns the verb from the kind and the app
  label: `"Read" · "Linear: list issues"`, `"Changed" · "Google Calendar:
  update event"`, `"Sent" · "Slack: post message to #general"`.
- The humanised action comes from the tool name: drop the toolkit prefix,
  lowercase, underscores to spaces.
- Batches read *"Read Linear ×3, Notion ×1"*.
- Raw slugs stay in the expandable detail.

### CA-UI-6 - Persona editor: "Apps I may use"

- A section under the persona's tools:
  - *"Every connected app"* (default) / *"Only these"*. "Only these" shows
    the connected apps with a toggle each and **Choose tools…** per app (a
    checklist from CAP-8's per-app tool list, fetched via `tools/list`
    through the session and grouped by app).
  - An empty selection reads *"No apps."*
- The Fleet card and the persona chip tooltip show *"Apps: Notion, Linear"*
  when narrowed.

### CA-UI-7 - The `/` menu

Two manifest entries in `shared/commands.json`, following existing patterns:

```json
{
  "name": "connect",
  "aliases": [],
  "section": "work",
  "kind": "action",
  "args": { "type": "choice", "source": "apps", "hint": "app", "required": true },
  "summary": "Connect an app for me to use",
  "who": "user",
  "needs": ["conv"],
  "trace": true
},
{
  "name": "apps",
  "aliases": [],
  "section": "work",
  "kind": "ui",
  "args": { "type": "none" },
  "summary": "Apps I can reach",
  "who": "user",
  "needs": []
}
```

- **New `ArgSource` `"apps"`:** the cached catalog, with connected apps
  first and marked.
- **`/connect gmail`:** posts a `connect` block for Gmail directly, without
  a model turn. It is the same block CAP-6 creates, so there's one flow.
- **Without a key:** `/connect` shows the "add a Composio key" state.
- **`/apps`:** opens Settings → Apps.

### CA-UI-8 - Usage

A "Connected apps" row in the Usage view: calls this month (CAP-12), the
per-app split on expand, and **Open Composio dashboard**. No money figure.

---

# Part V - Tests (beyond MCG-T)

- **CAP-T-1:** session lifecycle against a stub server.
  - Create, reuse, recreate on 404, and recreate on a new auth config, with
    the same user id each time.
  - A second recreate in the same boot does not happen.
- **CAP-T-2:** an auth link or MCP URL on a non-`composio.dev` host is
  refused (release config). An `http://` URL is refused.
- **CAP-T-3:** the registry offers exactly the allowlisted meta-tools.
  `COMPOSIO_REMOTE_WORKBENCH` in `tools/list` never reaches the specs.
- **CAP-T-4:** `targets()` table test, with the fixtures from CAP-0 and
  malformed shapes (empty array, entry without `tool_slug`, string args).
  Malformed → refused without a network call.
- **CAP-T-5:** a batch with one send among three reads raises one Send
  request naming both apps. The cap counts each target.
- **CAP-T-6:** grants.
  - A persona with `tools: '[]'` gets no Composio specs.
  - A persona granted `notion` only is refused `SLACK_SEND_MESSAGE` with a
    message that doesn't list Notion.
- **CAP-T-7:** `MANAGE_CONNECTIONS` produces a `connect` block and a
  `WaitingOnPerson` stop.
  - The stored transcript and message rows don't contain the
    `redirect_url`.
  - The turn resumes on the connected signal.
- **CAP-T-8:** account labels.
  - A duplicate label is refused.
  - A sixth account is refused.
  - An expired-only retry gets a `retry-` label and deletes nothing.
- **CAP-T-9:** scope hint on a failed 403, and no hint on a successful
  SEARCH result quoting the same text.
- **CAP-T-10:** the key never appears in the diagnostics export, activity
  rows, `app_accounts`, `config_json`, or any Tauri command result. Grep test
  over a recorded run.
- **CAP-T-11:** Mail routing. With a Mail account for `a@x.com` and Gmail
  `a@x.com` connected, the Gmail send panel shows the extra line. Without a
  Mail account, it doesn't.
- **CAP-UI-T-1:** `/connect` with source `apps` lists connected apps first.
  Choosing one renders the block without a model call.
- **CAP-UI-T-2:** Apps settings disclosure is visible before the key field in
  the not-configured state (snapshot).

---

# Part VI - Sequencing

| Phase | Ships | Depends on |
|---|---|---|
| **0** | MCG-1..7, MCG-UI-1..2, MCG-T. **Standalone release.** | nothing |
| **1** | CAP-0 fixtures and terms; CAP-1 schema; CAP-2 key and session; CAP-3 transport; CAP-4 offered tools; CAP-5 verdict; CA-UI-1 (key card, connected list) | Phase 0 |
| **2** | CAP-6 connect flow; CAP-7 accounts; CA-UI-2 browse; CA-UI-3 states; CA-UI-4 block; CA-UI-7 `/connect`, `/apps` | 1 |
| **3** | CAP-10 grants and CA-UI-6; CAP-11 routing; CA-UI-5 step lines | 2 |
| **4** | CAP-8 catalog polish; CAP-9 hints; CAP-12 and CA-UI-8 usage; CAP-13 checkup and diagnostics | 2 |
| **5** | CAP-14 relay to external agents (built 2026-10-23) | 3 and CLI_ENGINES MCPS-1 |
| **6** | CAP-C: Composio Connect (`ck_`) as the default, review fixes (Part IX) | 1-4 (built) |

Phase 1 is usable on its own for a person who connects apps in Composio's
dashboard. Phase 2 is when the story-1 experience exists. Don't call the
feature done before Phase 2.

---

# Part VII - Parked

- **CAP-P1 - Managed broker.** A Poiesis-run service holding one project key
  so people needn't make a Composio account. It's revisited only with the
  public release (`AUTOUPDATE_PLAN` PUB part) and real evidence that the key
  step loses people. It would need its own threat model: a shared key,
  per-install tokens, rate limits, and Erich running a service.
- **CAP-P2 - Triggers.** "When a new email from X arrives…" needs a webhook
  Poiesis can't receive without a relay. Polling via Tasks is the local
  alternative and needs no Composio feature.
- **CAP-P3 - White-labelled sign-in pages.** A paid Composio add-on. Not
  needed for a person's own key.
- **CAP-P4 - Other aggregators** (e.g. Pipedream Connect, Zapier MCP). The
  connector kind and the verdict are written so a second aggregator is a new
  `transport` value plus its own `targets()` unpacking. Not built until
  someone asks.

---

# Part VIII - Risks

- **Composio changes its API or meta-tool names.** The allowlist (CAP-4)
  hides new tools by default. The fixtures (CAP-0) make breakage a failing
  test, not a silent change. The verdict refuses shapes it can't read.
- **Misclassification lets a send through as a change.** The two still
  differ only in wording and the cap. Both ask by default. Per-tool
  overrides (MCG-7) fix any wrong guess, and the classifier errs toward the
  stricter kind.
- **Privacy.** Everything routed through Composio is seen by Composio. The
  disclosure is placed before the key field, Mail stays local by default,
  and the "via Composio" mark is on every row and step line.
- **Pricing changes.** Third-party trackers already disagree (20K vs 100K
  free calls a month). The person's own plan is billed, so Poiesis states no
  numbers in the UI and links to the dashboard.
- **Prompt injection through app data.** MCG-1 wraps every result. A
  fetched message that says "send this to …" still meets the Send gate,
  which shows the real recipient and text.
- **Loops in session upgrades.** Guarded once per boot (CAP-2), as OMB
  learned the hard way.

---

# Part IX - Phase 6: Composio Connect is the default (CAP-C)

> ID prefix **CAP-C**. Written 2026-10-19. **Built the same day** (see the status block at the top for what differs and what still needs a real key). This part changes
> settled decision 1 ("with the person's own Composio key"): the key is still
> the person's own, but by default it is their **Connect** key (`ck_`), not a
> developer project key (`ak_`).

## Why the approach changes

Phases 1-5 were built on Composio's **developer platform**: a project key
(`ak_`), `POST tool_router/session`, `.../link`, `connected_accounts`,
`toolkits`. Erich's first real try used the key Composio's own app shows under
**For You → Settings → Sessions & API Key**. That key starts with `ck_`, and
the platform API refused it ("Invalid API key").

What was found (2026-10-19):

- **Composio has two products with separate keys.**
  - The developer platform (`ak_`) is for someone who builds an app for their
    own users. Its docs say to "store the Composio API key only on your
    server", and one key serves many `user_id`s.
  - **Composio Connect** (`ck_`) is for one person's own AI client: Claude,
    Codex, Hermes, n8n, Cursor and others are listed on the dashboard's
    install page. Poiesis is exactly that, a person's own agent.
- **OpenMausBot** (read at `0adb029`, 2026-10-19) ships its desktop app with a
  **hosted broker** by default: a Cloudflare Worker holds the maintainer's
  `ak_` key and gives each install an anonymous token. A person's own `ak_` key
  is only its self-host fallback. It never uses `ck_`.
- **Erich's decision:** no server run for Poiesis users. A hosted service is
  against privacy-first, where the only cloud connection is one the person
  chooses. So the broker (CAP-P1) stays parked, now for good. Asking ordinary
  people for a developer project key is a developer task. The `ck_` key is the
  one Composio gives a person for their own agent, so it becomes the default.

What stays exactly as built, because Connect exposes the **same meta-tools with
the same names** (the recording lists eleven: the seven below plus
`COMPOSIO_SUBMIT_FEEDBACK`, `COMPOSIO_MANAGE_SKILL`, `COMPOSIO_SEARCH_SKILLS`,
`COMPOSIO_USE_SKILL`, all hidden by the allowlist):
- the CAP-4 allowlist (four tools offered; workbench, bash and wait hidden)
- the CAP-5 verdict (targets inside `COMPOSIO_MULTI_EXECUTE_TOOL`, strictest
  kind, one question naming every app)
- the MCG gate (read, change and send, the daily cap, headless parking, plan-first)
- persona grants (CAP-10), the CAP-11 mail sentence, the step lines (CA-UI-5),
  the scope hint (CAP-9), usage (CAP-12)
- MCG-1 untrusted wrapping, the keychain storage, CAP-T-10

## Verified facts about Connect

Checked 2026-10-19 against Composio's docs (`docs.composio.dev/llms-full.txt`,
the Composio Connect page, the Meta Tools reference, the consumer/developer KB
article) and GitHub issue #3485. Context7 was not available (monthly quota
used up); re-check through it when it is.

- **Address:** `https://connect.composio.dev/mcp`, Streamable HTTP.
- **Auth:** header `x-consumer-api-key: ck_...`. The key does not go in the
  URL. Composio: "These steps apply to a Connect consumer key with the `ck_*`
  prefix, sent as `x-consumer-api-key`. They do not apply to a Platform Project
  API key with the `ak_*` prefix."
- **Where the person finds the key:** For You / Connect → Settings → Sessions &
  API Key ("Your API Key"); the docs also call it the "AI Clients setup".
  Regenerating it invalidates the old one at once.
- **Tools:** `COMPOSIO_SEARCH_TOOLS`, `COMPOSIO_GET_TOOL_SCHEMAS`,
  `COMPOSIO_MULTI_EXECUTE_TOOL` (up to 50), `COMPOSIO_MANAGE_CONNECTIONS`
  ("create, list, rename, or remove OAuth connections"),
  `COMPOSIO_WAIT_FOR_CONNECTIONS`, `COMPOSIO_REMOTE_WORKBENCH`,
  `COMPOSIO_REMOTE_BASH_TOOL`.
- **`COMPOSIO_MANAGE_CONNECTIONS` input (corrected from the recording,
  2026-10-19; the docs were wrong):** `toolkits: [{name, action?, account_id?,
  alias?}]` (required), `session_id?`. `action` is `add` (the default; always
  makes a new link and a new account, at most ten per app), `list` (no side
  effects), `rename` or `remove` (both need `account_id`). There is no
  `reinitiate_all`.
- **`COMPOSIO_MANAGE_CONNECTIONS` output:** `{successful, error?, data:
  {message, results: {per toolkit}, summary?: {total_toolkits,
  active_connections, initiated_connections, failed_connections}, session?}}`.
  **The fields inside each `results` entry are not documented.** Connect Links
  look like `https://connect.composio.dev/link/ln_...`. Composio's own example
  bot "extracts it, redacts it from the tool output" before the model sees it.
- **`COMPOSIO_MULTI_EXECUTE_TOOL` input:** `tools: [{tool_slug, arguments,
  account?}]`, `sync_response_to_workbench` (required, default false), `thought?`,
  `current_step?`, `current_step_metric?`, `session_id?`. The recording shows the
  per-entry `account` (alias or account id, required when several accounts fit);
  the docs did not.
- **`COMPOSIO_MULTI_EXECUTE_TOOL` output:** `{successful, error?, data:
  {results: [{index, tool_slug, response?, error?}], error_count,
  success_count, total_count, ...}}`. `scope_hint`'s `failed()` already reads
  this shape.
- **Separate accounts:** Connect's connections live in the person's consumer
  project. Developer-project connections do not appear there, and the reverse.
  Connected accounts belong to the person who authorised them.
- **Not available with `ck_`:** the REST endpoints Phases 1-5 use (session,
  link, `connected_accounts`, `toolkits`, `auth_configs`). Composio's
  reference marks meta-tool schemas as "reference only", with no promise of
  backward compatibility.

## How Connect mode works

### CAP-C1 - The kind of key picks the mode
- `mode_of(key)`: `ck_...` → **Connect**; `ak_...` / `uak_...` → **Project**
  (today's code, unchanged). Derived from the stored key each time, never
  stored separately, so it cannot drift.
- `auth_header`: `ck_` → `x-consumer-api-key`.
- `mcp_client`: in Connect mode, a client on `CONNECT_MCP` with the header. No
  session, no user id needed. A debug-only `POIESIS_COMPOSIO_CONNECT` override
  points at a stub, compiled out of release builds like `POIESIS_COMPOSIO_API`.
- `validate_key`: in Connect mode, `initialize` + `tools/list`. The key is
  accepted only if `COMPOSIO_MULTI_EXECUTE_TOOL` is listed. A 401 or 403
  reads: *"Composio didn't accept this key. Copy it again in Composio under For
  You, Settings, Sessions & API Key; it starts with ck_."*
- `ComposioStatus` gains `mode: "connect" | "project" | null`. Never the key.

### CAP-C2 - Reading `MANAGE_CONNECTIONS`
- `parse_connections(text) -> Vec<ConnState{app, status, link, label}>`,
  lenient because the per-app fields are undocumented:
  - `results` may be an object keyed by toolkit or a list. The app comes from
    the entry's `toolkit` / `toolkit_slug` / `app`, or else its key, and must
    pass `valid_slug`.
  - The status comes from `status` / `connection_status` / `state`, booleans
    like `is_active` / `connected`, or a nested `accounts[]`. It is normalised
    to `ACTIVE | INITIATED | FAILED | EXPIRED | UNKNOWN`; a link with no status
    means `INITIATED`.
  - The link is the first `https://` string on `composio.dev` (the CAP-2 trust
    check), preferring fields named `*url*` / `*link*` and never `logo` /
    `icon`.
- **Must be confirmed with a recording (CAP-C9)** before the parser is
  trusted. Its tests are written from the recording, not guesses.

### CAP-C3 - The connect card in Connect mode
- The model's `MANAGE_CONNECTIONS` still goes through `connect_request`:
  grants, already-connected, headless and slug checks stay as built. The
  apps that need a card are then **sent to Composio** (in Project mode they are
  not).
- What comes back is written into `app_accounts` with one row per app, id
  `connect:{app}`. Any link is kept **in memory only**, per app, for 10 minutes
  (the documented link lifetime).
- An app Composio reports `ACTIVE` gets no card: *"Gmail is already
  connected."* An entry with neither a link nor `ACTIVE` becomes a note:
  *"Composio couldn't start the sign-in for X."*
- The model never sees Composio's raw answer. It gets the same reply as today
  ("I've asked the person to sign in to Gmail. Stop here; ..."), and the turn
  ends (`waiting_on_person`).
- The card's **Sign in** calls `composio_connect_cmd(app)`, which reopens the
  remembered link. When the link is older than 10 minutes, or after a restart,
  it starts again: `list`, `remove` the accounts that are not a working sign-in,
  then `add`. *(As built; the first draft said `reinitiate_all`, which does not
  exist.)* A sign-in opened in the last 10 minutes is always handed back as it
  is, whichever path asks (`start_sign_ins`).
- Polling (3 s, and on window focus, for 10 min) asks `MANAGE_CONNECTIONS` with
  `list` about the waiting app only, over one kept client. **Settled by the
  recording:** `add` mints a new link and account each time, so it is never used
  to check.

### CAP-C4 - Which apps are connected
Connect has no account list. The mirror is filled from three places:
1. answers to `MANAGE_CONNECTIONS` (CAP-C3 and **Check again**);
2. **learned from use:** every `MULTI_EXECUTE` entry that comes back without an
   `error` marks its app `ACTIVE`, including an app the person connected in
   Composio for another AI client (`learn_from_results`);
3. **Check again** on the Apps page, which asks about every app already in the
   mirror.

The CAP-4 sentence in Connect mode says *"The person's connected apps that I
know of: ... (others may be connected in Composio too)."* It must never say an
app is unconnected only because the mirror doesn't know it.

### CAP-C5 - Redaction
- `redact_links(text)` replaces every `https://*.composio.dev/...link...`
  address with *"[sign-in link shown to the person]"*.
- It runs on **every** Composio output and error before MCG-1 wrapping
  (`call_mcp_tool`), because a `MULTI_EXECUTE` refusal may carry a link too.
  CAP-T-7's "no address in stored rows" check extends to Connect.

### CAP-C6 - Disconnect, labels, catalog in Connect mode
- **Disconnect (as built, after the recording):** `MANAGE_CONNECTIONS` has a real
  `remove`. **Disconnect** lists the app's accounts, removes all of them, lists
  again, and only drops the local row when none is left (`remove_everywhere`).
  The confirmation says other AI clients lose the app too, since Connect keeps
  one set of sign-ins for all of them. *(The first draft's "Remove from list"
  button is gone.)*
- **Labels / several accounts per app (built 2026-10-19 night):**
  `MANAGE_CONNECTIONS` takes an `alias`, and Poiesis uses it. An app with one
  account keeps the row `connect:{app}`. With two or more, each account has
  `connect:{app}:{id}`. A sign-in for one more waits as `connect:{app}:new`.
  **Add account** (the CAP-7 label rules), **Name/Rename** (`rename`) and
  Disconnect of one account (`remove` with its `account_id`) work as in Project
  mode. A batch target for an app with several working accounts must name one;
  Poiesis says so before anything is asked or sent (`app_calls::plan`). A
  refusal from Composio for accounts Poiesis doesn't know of reaches the model
  wrapped.
- **Catalog:** there is no toolkit list with a `ck_` key. **Browse apps** shows
  the bundled catalog (`shared/composio-catalog.json`, 1,608 apps, built by
  `scripts/build-composio-catalog.mjs`). *(The first draft showed only the 24
  curated apps.)* The same list names the app a tool slug belongs to
  (`app_calls::catalog_slugs`).
- **Session upgrades, auth configs, `validate_key`'s throwaway user:**
  Project-only, untouched.
- **CAP-9 scope note** in Connect mode says to Disconnect, connect again and allow
  every permission on the app's sign-in page. *(The first draft pointed to
  "Manage Auth", which was never shown to exist for a `ck_` key.)*

### CAP-C7 - Commands and checkup
- `composio_connect_cmd`, `composio_accounts_cmd(refresh)`,
  `composio_disconnect_cmd`, `composio_catalog_cmd`: branch on the mode (one
  `connect_any` / `refresh_accounts` entry each). `Started.redirect_url`
  becomes `Option`: `None` means "already connected", and the browser is not
  opened.
- `probe_connector` and the checkup: `refresh_tools` works over MCP for both
  modes; `refresh_accounts` in Connect mode is the "ask about known apps" call.
  The 4 s checkup timeout risk stays.

## UI (CAP-C-UI)

### CAP-C-UI-1 - Apps page, not configured (Connect first)
- The disclosure, unchanged in substance, first person (PRES-0).
- *"Composio is free to start. In Composio, open For You, then Settings,
  Sessions & API Key, and copy your key (it starts with ck_). Paste it here."*
  with **Open Composio →** (`https://dashboard.composio.dev`; the exact
  consumer-dashboard address must be checked, CAP-C9).
- A quiet line: *"Building on Composio's developer platform? A project key
  (ak_) works too."*
- Key field, **Save**, inline result. A pasted `ak_` key works as Project mode.

### CAP-C-UI-2 - Configured, and consistency with Providers
- Header in the Providers card pattern: status dot, name, state word
  (*Connected* / *Needs attention* / *Not connected*).
- Line: *"Connected through Composio Connect · N apps"* (Project mode keeps
  "· M accounts").
- Actions: **Browse apps** (primary), and **Manage ▾** with *Replace key*,
  *Open Composio*, *Remove key* (danger). This is the same menu Providers uses,
  and it replaces today's flat "Change key / Remove key".
- Connected list in Connect mode: one row per app, status words, **Reconnect**
  when broken, **Remove from list**, the **What I may do** line, and
  **Check again**. Hint: *"Apps you connected in Composio for other AI clients
  show up here once I've used them."*
- Update the Apps page intro. Today it reads *"Poiesis Agent speaks the open
  Model Context Protocol — paste a server link..."*, which is outdated, third
  person, and doesn't mention connected apps.

### CAP-C-UI-3 - Connect card, Browse, `/connect`
- **Connect card:** no label field in Connect mode. A connect answer that comes
  back `ACTIVE` flips the card to connected at once. *"Connected."* when there
  is no label (not "Connected as default").
- **Browse:** no "Add account" or label prompt in Connect mode; it shows the
  typed-slug card.
- **`/connect`:**
  - the menu lists connected apps that are not on the curated list too
    (review bug 4);
  - in Connect mode a typed name after the app is ignored, not sent as a label.

## Review fixes carried into Phase 6 (2026-10-19 review)

These apply to both modes:

- **CAP-C-F1, sign-in retries create accounts.** "Open again"
  (`ConnectBlock.tsx`, `BrowseApps.tsx`) and "Reconnect" each start a new
  Composio account instead of reopening the pending one, and Cancel leaves it
  `INITIATED`. They later turn `EXPIRED` / `FAILED` and show as red rows,
  checkup lines and the hub dot. Fix: the per-app in-memory link cache from
  CAP-C3 for Project mode too (reuse within 10 min under the same label); and
  Reconnect in Project mode revokes the stopped account and links again under
  its label (OMB's rule). A label held only by an inactive account may be
  reused, after that account is deleted.
- **CAP-C-F2, FAILED counted as expired.** `checkup::expired_lines` and
  `accountBroken` treat `FAILED` (often an abandoned first sign-in) like a sign-in
  that stopped working. Say *"didn't finish"* for FAILED, and don't raise the
  hub dot for a failed first attempt.
- **CAP-C-F3, session recreate guard.** `ensure_session` keys its
  once-per-boot guard on `{session id}|{setups}`. A recreate makes a new id, so
  a session that keeps coming back looking wrong is recreated on every run.
  Key it on the setups alone.
- **CAP-C-F4, hub dot never lights.** `appsNeedAttention` reads the local copy,
  which is only refreshed while Apps is open. Refresh accounts once in the
  background at startup when a key is saved.
- **CAP-C-F5, `/connect` misses connected apps** that are not in
  `shared/apps.json` (`commands.ts` `appChoices`).
- **CAP-C-F6, the send panel for a batch shows the wrapper.** The panel lists
  `tools` as one JSON line folded at 120 characters, so the message text is not
  readable "in full" (MCG-3). Pass each target's own arguments (app, action,
  arguments) as the panel's `diff` for Composio calls.
- **CAP-C-F7, `COMPOSIO_*` inside a batch.** A `tool_slug` naming a Composio
  meta-tool inside `MULTI_EXECUTE` is judged as an ordinary change and can be
  allowed once. Refuse it in `plan()`. This matters more in Connect mode,
  where the workbench cannot be switched off at session creation.
- **CAP-C-F8, the card pre-fills "work"** even when that label is taken; use
  the backend's suggestion (`ConnectBlock.tsx`).
- **CAP-C-F9, small:**
  - `app_label` re-parses `apps.json` on every call (cache it);
  - Project mode's `validate_key` creates a throwaway Composio user on each
    save;
  - in plan-first no app can be read at all, because `MULTI_EXECUTE` is
    withheld. Consider offering it with reads-only enforcement in `plan()`.
- **CAP-C-F10, UI consistency:**
  - one monogram class (`connect-mono` / `signin-mono`) and one success style
    (`composio-ok` / `provider-card-ok`);
  - the persona "Apps this persona may use" should save with the editor's
    **Save** like tools and skills do (already listed as a known gap).

## Tests (CAP-C-T)

- **CAP-C-T1:**
  - `mode_of` / `auth_header` table (`ck_` → Connect + `x-consumer-api-key`;
    `ak_` / `uak_` unchanged);
  - a Connect-mode client sends exactly that header to `CONNECT_MCP`, and no
    REST call is made.
- **CAP-C-T2:** `validate_key` against a stub MCP server: a 401 gives the
  `ck_` sentence; a `tools/list` without `MULTI_EXECUTE` is refused.
- **CAP-C-T3:** `parse_connections` from the **recorded** answers (CAP-C9)
  plus malformed ones (no `results`, untrusted link host, logo URLs, an invalid
  slug).
- **CAP-C-T4:**
  - the model's `MANAGE_CONNECTIONS` in Connect mode goes to the stub;
  - the card data has no address;
  - the model's reply, the stored message and the block rows contain no
    `composio.dev/link`;
  - an `ACTIVE` app gets no card.
- **CAP-C-T5:** `redact_links` table (link, plain composio.dev page left alone,
  other hosts left alone, links inside JSON strings); every Composio output
  passes through it.
- **CAP-C-T6:** learned from use: a `MULTI_EXECUTE` answer with one success
  and one error marks only the successful app `ACTIVE`.
- **CAP-C-T7:**
  - "Open again" within 10 minutes reuses the link and makes no second
    `MANAGE_CONNECTIONS` / `link` call (both modes);
  - after 10 minutes, one fresh call.
- **CAP-C-T8:** Remove from list deletes only the mirror row, and only one of
  this install's.
- **CAP-C-T9:** the CAP-T-10 key test repeated with a `ck_` key.
- **CAP-C-UI-T1:** Apps page not configured: the `ck_` instructions and
  disclosure come before the key field; the `ak_` line is present.
- **CAP-C-UI-T2:** Connect-mode card hides the label, shows "Connected." on an
  immediate `ACTIVE`, and "Open again" calls connect once.

## Open tasks, in order

1. **CAP-C9, record first (blocks CAP-C2/C3 details).** With Erich's `ck_` key,
   in the running app, from a debug-only recorder command (the key never
   leaves the keychain, and the recorder strips links and ids before writing):
   - `tools/list`: confirm the seven names and each `inputSchema`;
   - `MANAGE_CONNECTIONS {toolkits:["gmail"]}` for an unconnected app;
   - the same again within a minute: new link or the same?
   - the same after signing in (the `ACTIVE` shape);
   - `MANAGE_CONNECTIONS` for an app connected in Composio's dashboard;
   - one `MULTI_EXECUTE` success and one failure (signed-out app);
   - whether `MANAGE_CONNECTIONS` can list all connections or remove one;
   - the consumer dashboard address for **Open Composio**.

   Save them under `fixtures/composio/connect/`, hand-checked for secrets.
2. CAP-C1 + CAP-C-T1/T2 (mode, header, validate, status `mode`).
3. CAP-C2/C3/C5 + CAP-C-T3/T4/T5 (parse, card, redaction).
4. CAP-C4/C6/C7 + CAP-C-T6/T7/T8 (mirror, disconnect, catalog, commands).
5. CAP-C-UI-1..3 + UI tests.
6. Review fixes CAP-C-F1..F10.
7. Re-run the manual pass in the "Resume here" block with a `ck_` key first,
   then with an `ak_` key. Then commit (stage by file).
8. **Terms:** check Composio's terms for using Connect from a distributed
   desktop agent with each person's own key. Composio lists many third-party
   agents on its install page; record the date and the finding here.

## Risks (Phase 6)

- **Undocumented answer shapes.** `MANAGE_CONNECTIONS`'s per-app fields and
  `results` layout are not documented and "not guaranteed backward
  compatible". Mitigation: the lenient parser, recorded fixtures, and a
  fallback note to the model ("Composio couldn't start the sign-in for X")
  instead of a crash.
- **Connect availability.** Issue #3485: in May 2026 Connect refused valid
  `ck_` keys with 401 until Composio restored the service (May 27). Some users
  still reported 401s in July and August. The Apps page must show Composio's
  refusal plainly and keep the key, so a temporary refusal doesn't lose the
  setup.
- **A thinner Apps page than Project mode** (no account list, no revoke, no
  labels, short catalog). This is accepted: the person's sign-ins live in
  their Composio account, which they manage there.
- **Privacy is unchanged from Phases 1-5.** Requests and answers pass through
  Composio either way, and the disclosure stays before the key field. No
  Poiesis-run server exists in any mode.

## Sources (Phase 6)

- Composio Connect: https://docs.composio.dev/docs/composio-connect
- Consumer and developer project boundaries (key prefixes, header, rotation):
  https://docs.composio.dev/kb/guide/consumer-project-boundaries-and-auth-selection
- Meta tools: https://docs.composio.dev/toolkits/meta-tools/manage_connections,
  https://docs.composio.dev/toolkits/meta-tools/multi_execute_tool
- In-chat auth and redacting Connect Links: https://docs.composio.dev/docs/authentication
  (section "In-chat authentication"), example "Redirect auth links"
- Full docs dump read 2026-10-19: https://docs.composio.dev/llms-full.txt
- Connect outage and fix: https://github.com/ComposioHQ/composio/issues/3485
- OpenMausBot `0adb029` (2026-10-19): `docs/composio.md`,
  `apps/docs/content/docs/connected-apps/self-hosted-composio.mdx`,
  `cloudflare/composio-broker/README.md`, `electron/managed-composio.mjs`

---

## Sources

- OpenMausBot `a67daf0`: `server/composio.ts`, `server/connector-verdict.ts`,
  `shared/outbound.ts`, `server/connector-scope-hint.ts`,
  `server/system-prompt.ts`, `server/setup-mode.ts`, `docs/composio.md`,
  `cloudflare/composio-broker/README.md`.
- Composio pricing, read 2026-10-13: https://composio.dev/pricing.md
- Composio users and sessions: https://docs.composio.dev/tool-router/users-and-sessions
- Composio with MCP clients: https://docs.composio.dev/tool-router/using-with-mcp-clients
- Session API reference: https://docs.composio.dev/reference/v3/api-reference/tool-router/getToolRouterSessionBySessionId.md
