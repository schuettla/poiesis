# Project Poiesis - CLI Engines Plan

**Hand a job to the agent apps you already have installed. Watch them work.
Answer their questions in one place.**

Claude Code, Codex, OpenCode and a growing list of other agent CLIs are
excellent at long coding jobs, and most people who have them pay for them by
subscription, not per token. Poiesis can't use any of that today. The only way
to get Claude-quality coding inside Poiesis is an Anthropic API key, billed per
token, running through our own loop.

OpenMausBot (github.com/milind-soni/OpenMausBot, read at `a67daf0`,
2026-10-13) shows a way to do it properly. Each CLI runs **headless over its own
structured stdio protocol**, and its events are translated into one internal
event stream. No PTY and no screen scraping. Its approval requests become the
app's own approval cards. This plan brings that idea to Poiesis, in Rust, on
seams that already exist.

> ID prefixes: **CLI** engines backend - **MCPS** the MCP endpoint Poiesis
> serves to engines - **LRN** lessons from OpenMausBot that are not about CLIs -
> **-UI** frontend - **-T** tests.
>
> **Status: draft v5. Phases 0-5 done 2026-10-20 and LRN-1..5 on 2026-10-23, all committed (`6b1e851`); five review fixes on 2026-10-23, see Part X (read it first when resuming). Gemini CLI removed, `CLI-P3`; Qwen Code removed 2026-10-23.** v5: detection made concrete (catalog
> `shared/engines.json`, install locations, PATH read fresh from the registry,
> triggers, new-agent badge). v4: the invented `@` mention is replaced
> by the existing `/` menu (`/delegate [agent] task`, `/external on|off`);
> engine approvals reach the panel in background `/delegate` runs. Written 2026-10-13. v2 (same day):
> terms checked (CLI-0b → TRM rules); added the settings model (CLI-12), Try it
> (CLI-13), the UI integration map, and the Settings → External agents section
> (CLI-UI-1). v3: the visible rule set for when Poiesis hands work over:
> modes, rules, guards, handover card, proposals (RTE-1..7, RTE-UI-1..4).
>
> PRES-0 (first-person copy, `plans/done/POIESIS_PLAN.md`) binds every `-UI-`
> task here.

---

## The decision this plan makes

**Engines are delegation targets, not chat engines.**

OpenMausBot is a *meta-harness*. The CLI owns the agent loop, and the app is
the envelope around it. Poiesis is the opposite. Its value *is* the loop:
prompt assembly behind the golden gate, memory and recall, blocks, planning,
folder trust, the dual-process work that's coming. Making Claude Code the
engine of a chat would bypass every one of those.

So an external engine is a kind of **agent type**, which is where the Fleet
already puts "someone else does this part". The lead (our loop, any model)
calls `delegate`. One of the tasks names an engine-backed agent type. That
child runs as a CLI process instead of a `run_agent` call. Everything that
makes a delegated child legible already exists and is reused unchanged: its
own conversation row, the Fleet card, Stop, the transcript, the report back to
the lead.

What this buys:
- Heavy coding on the person's own Claude Max or ChatGPT plan, with Poiesis
  keeping the conversation, the memory and the final word.
- Every ACP-speaking CLI for the price of one driver.
- One permission language across all of them.

What it deliberately gives up (for now):
- Chatting *directly* with Claude Code inside Poiesis as the main engine
  (parked as `CLI-P1`).
- Token-level control inside an engine turn. We see the engine's events, not
  its model calls.

---

## What this is not

- **Not a terminal emulator.** No PTY, no ANSI parsing. If a CLI has no
  structured headless protocol, it is not supported.
- **Not a key broker.** Poiesis never reads, copies or stores an engine's
  login. The CLI is spawned with its own home folder, and its login stays where
  the CLI put it. We read *whether* it is signed in, only through the CLI's own
  status command.
- **Not a sandbox.** An engine runs outside `agent/sandbox.rs`. Confinement is
  the engine's working folder plus its own permission system, which we drive
  (see `CLI-6`). The plan is honest about that in the UI (`CLI-UI-3`).
- **Not a Node dependency.** Everything is `tokio::process` plus
  newline-delimited JSON, like `mcp/client.rs` already does.

---

## Who feels what

**1. "The refactor went to Claude Code, and I watched it."** Erich asks Poiesis
to move the voice settings into their own module and fix the tests. The lead
plans, then delegates the coding to the `Claude Code` agent type. In the turn,
the Fleet card shows a row with the Claude Code mark and an `on your plan`
chip. Its steps stream in Poiesis's own words: *read `src/lib/voice/controller.ts`*,
*edited 3 files*, *ran `npm test`*. When Claude Code wants to run
`cargo test`, the same permission panel as always slides up, headed **Claude
Code wants to run a command**, with the argv one token per line. Erich clicks
Allow for this run. The child finishes. The lead reads its report, checks the
diff in Changes, and answers in its own voice. *(CLI-4, CLI-6, CLI-UI-2, CLI-UI-3)*

**2. "It used my subscription, not my API key."** Every engine row in the
Fleet card and in Usage says how it was paid for: `on your plan` or
`API key`. The notional cost Claude Code reports is shown in grey, and it is
never added to cloud spend. *(CLI-9, CLI-UI-7)*

**3. "I can pick it up in my terminal."** The child conversation opened from
the Fleet card has a strip at the top: *I handed this to Claude Code ·
Continue there*. Clicking it copies `claude --resume <session>` and opens the
folder in a terminal. The work doesn't get trapped in Poiesis. *(CLI-UI-5)*

**4. "Anything I install shows up, and I decide what it may do."** Erich
installs OpenCode. In **Settings → External agents** it appears after
**Check again** as *OpenCode · 2.x · ready*, with its switch off. He
turns it on and clicks **Try it**: *Ready in 3.8 s · free model*. He sets
Claude Code to *At most: Ask me first*, even though his repo folder lets
Poiesis edit freely, and leaves Codex on an API key. *(CLI-2, CLI-12, CLI-13,
CLI-UI-1)*

**4b. "I can see why it handed that over, and change the rule."** Claude
Code's settings open on **When I use it**. The mode is *Suggest it, I
confirm*, and there are readable rules: *Use it for: running the tests and
fixing what fails…* and *Don't use it for: one small edit in one file.* Erich
asks for a typo fix and Poiesis does it itself. He asks to fix the failing
voice tests, and a card appears: *Hand this to Claude Code? Because of your
rule U2.* He clicks **Always for this**, and from then on test-fixing goes
straight over, with a *because U2* chip on the Fleet row. A week later Poiesis
proposes: *I keep being told not to use Claude Code for README edits. Add that
as a "Don't use it for" rule?* He clicks Add. *(RTE-1…6, RTE-UI-1…3)*

**5. "I said who should do it."** Erich types `/delegate ` and the usual
argument popover lists *General*, his personas, *Claude Code* and *Codex*
(greyed out: *Off. Turn it on in External agents*). He picks Claude Code,
types the task, and keeps talking while it works. Its approvals still reach
him, and its Fleet row says *you asked*. In a chat about his tax papers he
types `/external off`, and an *External agents off* chip sits next to the
footer like *Tools off* does. *(CLI-UI-6)*

**6. "When it breaks, I know why."** When an update changes the CLI's output, the
row in Settings → External agents says *Claude Code 2.3.0, a version I haven't been tested
with*. If a run fails, the child shows the CLI's last stderr lines, not
"something went wrong". *(CLI-3, CLI-UI-8)*

---

# Part I - How OpenMausBot does it (verified in source)

| Engine | Process and protocol | Approvals | Session |
|---|---|---|---|
| Claude Code (`server/drivers/claude.ts`) | `claude -p --input-format stream-json --output-format stream-json --verbose --permission-mode <m> --mcp-config <file> --allowedTools ... --append-system-prompt-file <f>`. The prompt goes in **over stdin** as a stream-json user message, never in argv. Turn end is the `result` event. | `--permission-prompt-tool mcp__ogb__approve`. OMB adds its own MCP server (`ogb`) to the CLI's MCP config, and that server forwards each request to the app over a socket. | `--resume <session_id>` from the `init` event |
| Codex (`codex.ts`) | `codex app-server`: JSON-RPC 2.0, newline-delimited, stdio. `thread/start`, `thread/resume`, `turn/start`. Turn end is the `turn/completed` notification. | Server→client requests `item/commandExecution/requestApproval`, `item/fileChange/requestApproval`, answered `{action: "accept"/"decline"}` on the same pipe. | Codex thread id |
| ACP family (`drivers/acp/core.ts` + one quirks file per CLI): Gemini, Grok, Cursor, Cline, Qwen, Kimi, opencode, any custom | Agent Client Protocol over stdio: `initialize`, `authenticate`, `session/new`, `session/prompt`. Turn end is the **result** of `session/prompt` (it carries `stopReason` and usage). | Server→client `session/request_permission`. **Fails closed**: only an explicitly offered `allow`-kind option is ever chosen, and the order of options is never trusted. | `session/load`, which replays history as updates; OMB drops `_meta.isReplay` updates |

Shared ideas worth taking:
- **One internal event stream** (`shared/runtime-events.ts`) that every
  driver emits into.
- **Capability flags per driver** (`ProviderAdapter.capabilities`). A bot is
  never told about tools its engine can't actually provide.
- **Secrets never on argv.** The MCP config, which carries tokens, goes into a
  temp file only the user can read and is deleted when the turn ends. Other
  engines' provider keys are removed from each child's environment, so a CLI
  can never bill against another engine's login.
- **Tested versions are written down in code** ("verified against claude
  2.1.211", "codex-cli 0.144.4"), and the large test files exist for exactly
  that.
- ACP processes are **kept alive per thread** and closed after 10 minutes idle.
  Claude Code is one process per turn, resumed with `--resume`.

# Part II - What Poiesis already has

| Need | Already here | Gap |
|---|---|---|
| Spawn a process, speak JSON-RPC on stdio | `mcp/client.rs` (stdio transport) | Long-lived duplex: we also receive *requests* from the child |
| Kill the process tree if the app dies | `runtime/jobobject.rs::assign(&std::process::Child)` | Needs an overload for `tokio::process::Child` (raw handle) |
| A run you can stop, steer, watch | `agent/fleet.rs` (`open`, `steer`, per-run `CancelFlag`) | None |
| Delegation, child conversations, reports | `agent/subagents.rs` (`delegate`, `agent_types`, `execute`) | A branch for engine-backed types |
| A child shown live in the turn | `FleetCard.tsx`, `Workbench/AgentsPanel.tsx` | Engine badge, payment chip |
| An approval the loop waits on | `permissions::PermissionRequest` + `Decision` oneshot; already reused for `capability` and `argv` requests | `engine` and `tool` fields |
| A loopback endpoint whose calls go back to the run | `agent/toolrpc.rs`: one port, a per-call `Ticket` revoked on drop, `Gate` channel into the run's `dispatch` | Speaks plain POST, not MCP; armed only by `run_code` and gated by `tools.script_rpc` |
| Big results don't flood context | `agent/results.rs` (`HRN-8`) | Engine reports go through `REPORT_CLIP` already |
| Agent types from personas | `personas.spawnable` | No `engine_id` |

So the new code is: drivers, an engines registry, a small MCP server on the
existing loopback port, and one branch in `delegate`.

# Part III - The shape

```
lead run (run_agent, any model)
  └─ delegate { tasks: [{ agent: "Claude Code", task: "…" }] }
       └─ subagents::delegate
            ├─ persona/agent type has engine_id? ── no ──> run_agent (unchanged)
            └─ yes ──> engines::run_external(ExternalTurn)
                         ├─ driver.spawn (tokio::process, jobobject, scrubbed env)
                         ├─ stdout lines ──> driver.parse ──> AgentEvent ──> child sink
                         │                                       (FleetCard, transcript)
                         ├─ approval request ──> permissions panel ──> Decision ──> driver.answer
                         │     (Claude: via MCPS approve tool; Codex/ACP: on the pipe)
                         ├─ Fleet steer ──> driver.steer   ·   Fleet stop ──> driver.interrupt, then kill
                         └─ ExternalOutcome { text, session, usage, paid_by } ──> report to lead
```

---

# Part IV - Backend tasks

## Phase 0 - Recordings and the terms gate

### CLI-0 - Record real sessions as fixtures

Before writing a parser, record real sessions. For each of Claude Code, Codex
and OpenCode (ACP), run one small coding task in a scratch folder that
includes one command approval, one file-edit approval, one denial and one
interrupt. Save the raw stdio both ways as
`fixtures/engines/<engine>-<version>.jsonl`, one JSON object per line with
`{"dir":"in"|"out","t":<ms>,"msg":{…}}`. Remove account ids and emails by hand.

These fixtures are the contract. `CLI-T-1` replays them, and they are what
"tested with version X" means.

**Acceptance:** three fixture files exist, each covering session start, text
streaming, tool start and end, an approval round trip, completion with usage,
and an interrupt.

### CLI-0b - Terms (checked 2026-10-13, re-check before each release)

**Result: plan mode is allowed for both engines under the conditions below.
Neither is unclear enough to force API-key only.** Both vendors' pages have
changed several times in 2026, so the release checklist re-reads them
(`TRM-6`).

**Anthropic.** Source: the Claude Code "Legal and compliance" page
(code.claude.com/docs/en/legal-and-compliance), read 2026-10-13.
- Forbidden: *"Anthropic does not permit third-party developers to offer
  Claude.ai login into their own applications, or to route requests through
  Free, Pro, or Max plan credentials on behalf of their users. Moreover,
  developers may not collect, store, or intermediate Claude.ai credentials or
  session tokens — sign-in to a Claude account must complete through
  Anthropic's own flow."*
- Explicitly allowed: the restriction does not *"prevent an end user from
  signing in to the unmodified Claude Code binary with their own Claude
  subscription, including where a platform hosts Claude Code"*.
- Conditions for running Claude Code inside a product: *"The Claude Code binary
  must not be modified"*, no removing or disabling its built-in sign-in
  methods, no paying for or reselling usage, and each end user authenticates
  with their own credentials. Doing so *"requires agreeing to our Commercial
  Terms of Service"*.
- Names and logos: you may say in plain text that a product *"runs Claude
  Code"*, but you may not use the name or logo as your own feature name or
  logo, or suggest endorsement.
- Limits: *"Advertised usage limits for Pro and Max plans assume ordinary,
  individual usage of Claude Code and the Agent SDK."*

Poiesis spawning the user's own installed, unmodified `claude`, which the user
signed into through Anthropic's own flow, is the explicitly allowed case. Using
the Agent SDK's OAuth login from our code, or reading `~/.claude` tokens,
would not be.

**OpenAI.** Sources: the Codex app-server docs (learn.chatgpt.com/docs/app-server)
and the ChatGPT plan usage docs (developers.openai.com/siwc), read 2026-10-13.
- The app-server is documented as what you use for *"a deep integration inside
  your own product"*. Its implementation is open source (Apache-2.0).
- Clients identify themselves with `clientInfo.name`. Enterprise integrations
  should *"contact OpenAI to get it added to a known clients list"*.
- No prohibition was found on running the user's own signed-in `codex`.
- Separately, "Sign in with ChatGPT" plus a plan-usage grant is OpenAI's
  sanctioned way for an app to bill a user's plan *directly*. It covers
  open-source and locally hosted apps; paid or remotely hosted apps need an
  interest form. Poiesis doesn't need it, because it never calls OpenAI with
  the user's plan itself. The `codex` process does.

**Binding rules that follow (TRM).** Every task in this plan obeys them:
- **TRM-1** Run the CLI exactly as installed. Never patch, wrap-replace or
  bundle a modified binary. Never pass flags that disable or hide its sign-in
  methods.
- **TRM-2** Poiesis never touches engine credentials. It never reads
  `~/.claude`, `~/.codex` or any token file, never proxies their auth traffic,
  and never builds its own login UI for them. Sign-in state comes only from the
  CLI's own status command (`CLI-2`). The **Sign in** button opens a terminal
  running the CLI's own login (`claude`, `codex login`), so sign-in completes
  in the vendor's own flow.
- **TRM-3** Engines are listed by their plain-text name ("Claude Code",
  "Codex"). No vendor logos or marks. Generic glyphs only, and no Poiesis
  feature is named after them. The section is "External agents", not "Claude
  Code".
- **TRM-4** Ordinary individual use. Engine children count toward the existing
  delegation caps, there are no automatic retry loops (at most one resume
  retry, `CLI-7`), and engines are **off by default** in headless/scheduled runs
  (`CLI-12`, `engines.<id>.headless`).
- **TRM-5** Codex `initialize` sends `clientInfo: { name: "poiesis", version }`.
- **TRM-6** The release checklist (`plans/done/AUTOUPDATE_PLAN.md` PUB part)
  gets a line: re-read both pages, and update this section if they changed.
- **TRM-7 (Erich, before a public release):** Anthropic says running Claude
  Code in a product requires agreeing to its Commercial Terms. Accept them
  through a Claude Console account in Erich's name before a public build ships
  with this feature on. This doesn't block development or personal use.

API-key mode stays available for both (`CLI-9`). It's simply no longer the
forced fallback.

## Phase 1 - The engines registry

### CLI-1 - Schema v31: `engines` table, two columns

```sql
CREATE TABLE engines (
  id            TEXT PRIMARY KEY,          -- "claude-code", "codex", "acp:opencode", "acp:custom-<uuid>"
  kind          TEXT NOT NULL,             -- "claude-code" | "codex" | "acp"
  display_name  TEXT NOT NULL,
  program       TEXT,                      -- resolved absolute path; NULL = not found
  args_json     TEXT NOT NULL DEFAULT '[]',-- extra argv (ACP: e.g. ["--experimental-acp"])
  env_json      TEXT NOT NULL DEFAULT '{}',-- NON-secret env only; secrets via secrets.rs
  enabled       INTEGER NOT NULL DEFAULT 0,
  version       TEXT,                      -- from the last probe
  signed_in     INTEGER,                   -- NULL unknown, 0/1
  paid_by       TEXT,                      -- "plan" | "api-key" | NULL
  last_probe_at INTEGER,
  last_error    TEXT,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
ALTER TABLE personas      ADD COLUMN engine_id TEXT;      -- NULL = runs on Poiesis
ALTER TABLE conversations ADD COLUMN engine_session TEXT; -- resume cursor: claude session id / codex thread id / ACP session id
```

Bump `SCHEMA_VERSION` to 31 in `db/mod.rs` with the usual `add_column`
migration. `enabled` defaults to **0**. Finding a CLI is not consent to hand it
work.

### CLI-2 - Detection and probe (`src-tauri/src/engines/mod.rs`, `engines/probe.rs`)

New top-level module `engines` (it isn't part of the agent loop):

```rust
pub enum EngineKind { ClaudeCode, Codex, Acp }

pub struct Probe {
    pub program: Option<PathBuf>,
    pub version: Option<String>,
    pub tested: Tested,              // Yes | Newer | Older | Unknown
    pub signed_in: Option<bool>,
    pub paid_by: Option<PaidBy>,     // Plan | ApiKey
    pub error: Option<String>,
}

pub async fn detect_all(db: &Db) -> Vec<(String, Probe)>;   // known ids
pub async fn probe(db: &Db, engine_id: &str) -> Probe;      // one, writes the row
```

**How Poiesis knows an agent exists: a catalog plus a search, never a disk
scan.**

*The catalog* is `shared/engines.json`, bundled with the app and updated with
each release, like `shared/commands.json`. One entry per agent Poiesis knows
how to drive:

```json
{ "id": "claude-code", "kind": "claude-code", "name": "Claude Code",
  "binaries": ["claude"],
  "install_url": "https://code.claude.com/docs/en/setup",
  "login": ["claude"],                       // what Sign in opens (TRM-2)
  "status": ["auth", "status"],              // confirmed in CLI-0
  "tested": [">=2.1.200 <2.3"] }
{ "id": "acp:opencode", "kind": "acp", "name": "OpenCode",
  "binaries": ["opencode"], "acp_args": ["acp"], … }
```

Initial entries: Claude Code, Codex, OpenCode. More
can be added in a release without code (ACP quirks are data, CLI-8). An agent
that isn't in the catalog can't be found automatically. The user adds it with
**Add another agent (ACP)…**, and that's the honest limit.

*The search*, for each catalog entry, in this order (first hit wins):
1. The `program` the user set.
2. Known install locations per binary:
   - native installers (`%USERPROFILE%\.local\bin\<name>.exe`),
   - npm global (`%APPDATA%\npm\<name>.cmd`),
   - WinGet links (`%LOCALAPPDATA%\Microsoft\WinGet\Links\<name>.exe`),
   - Scoop shims (`%USERPROFILE%\scoop\shims\<name>.exe`).

   Exact paths are confirmed in `CLI-0`.
3. `PATH`, **read fresh from the registry**: `HKCU\Environment\Path` plus
   `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\Path`,
   with variables expanded. Not the app's own `PATH`, which is frozen when
   Poiesis starts, so an agent installed while Poiesis is open would otherwise
   stay invisible until a restart. The same fresh `PATH` is used when
   spawning (`CLI-3`).

Safety: detection only checks whether those exact files exist. It runs
nothing except `<found binary> --version` and the catalog's status command,
both with a 10 s timeout and the scrubbed environment from CLI-9. It never
walks the disk and never runs a binary the catalog doesn't name.

A `.cmd`/`.bat` npm shim is accepted but recorded as `shim`. It is spawned as
`cmd /d /s /c "<shim>" <fixed args>`, and **no user or model text ever goes in
argv** (the prompt always goes over stdin), so shim quoting can't be exploited.

The probe runs `<program> --version`, plus the CLI's own auth status command
(`claude auth status`, `codex login status`; exact commands confirmed in
`CLI-0`), with a 10 s timeout. It never reads credential files (global rule).
`paid_by` comes from that status output only.

`tested` compares the version against the catalog entry's `tested` ranges.

**When detection runs.** The file checks are cheap (milliseconds), so they run
often. Only the probe, which spawns `--version` and the status command, runs
when something changed:

| Trigger | What runs |
|---|---|
| App start (`engines.detect_on_start`, background, never blocks boot) | Full detect, then probe each found engine. |
| Opening Settings → External agents | File checks. Probe only engines whose file path or modified time changed, or whose last probe is older than 1 h. |
| The app window regains focus (throttled to once per 5 min) | File checks only. This is how "I just installed it in a terminal" gets noticed without a restart. |
| A delegate to an engine | Probe that one engine if its last probe is older than 10 min, so a run doesn't start on a CLI that was uninstalled or signed out. |
| **Check again** / **Sign in** finished | Full probe of that engine (or all). |

**When something new turns up.** A catalog agent found for the first time (no
`engines` row yet) gets a row with `enabled = 0` (CLI-1: found is not
consent), and Poiesis says so once:
- A dot badge on **Settings → External agents** in the hub nav, the same
  mechanism as the Skills/Self/About badges in `SettingsHub.tsx`'s
  `badgeFor`.
- On the section, the intro reads *I found Codex. Turn it on when you want me
  to hand work to it.* (RTE defaults are seeded at the same moment.)

The badge clears when the section is opened. Nothing pops up in a
conversation, and nothing is turned on by itself. An engine that disappears
(uninstalled) keeps its row and its rules, shows *I can't find it any more*,
and isn't offered as an agent type (LRN-5). Reinstalling it brings it back as
it was. Tauri commands go in `commands/engines.rs`:
`engines_list`, `engines_probe(id)`, `engines_set_enabled(id, bool)`,
`engines_set_program(id, path)`, `engines_add_acp(display_name, program, args)`,
`engines_remove(id)` (custom only).

### CLI-3 - The driver contract (`engines/driver.rs`)

```rust
#[async_trait]
pub trait EngineDriver: Send + Sync {
    fn kind(&self) -> EngineKind;
    fn capabilities(&self) -> Capabilities;
    async fn run_turn(&self, turn: ExternalTurn<'_>) -> Result<ExternalOutcome, EngineError>;
}

pub struct Capabilities {
    pub resume: bool,          // native session continues across turns
    pub steer_mid_turn: bool,  // can take a user message while working
    pub interrupt: bool,       // has a protocol-level cancel (else: kill)
    pub mcp_tools: bool,       // can mount MCPS (Poiesis tools, Phase 5)
    pub approvals: Approvals,  // PromptTool (Claude) | InBand (Codex, ACP)
}

pub struct ExternalTurn<'a> {
    pub engine: &'a EngineRow,
    pub task: String,               // the delegate task + output shape
    pub system_append: String,      // CLI-5
    pub cwd: PathBuf,               // the conversation folder; required (CLI-4)
    pub trust: Trust,               // CLI-6
    pub resume: Option<String>,     // conversations.engine_session
    pub headless: bool,
    pub sink: AgentEventSink,       // child's sink, as run_agent gets
    pub run: Arc<RunHandle>,        // cancel + steer from fleet
    pub permissions: PermissionPort,// raise PermissionRequest, await Decision
    pub mcps: Option<McpsTicket>,   // MCPS-1, Claude only in v1
}

pub struct ExternalOutcome {
    pub text: String,
    pub session: Option<String>,
    pub usage: Option<Usage>,       // input/output/cached tokens
    pub notional_cost_usd: Option<f64>,
    pub paid_by: Option<PaidBy>,
    pub stop_reason: String,
    pub denials: Vec<String>,
    pub unknown_events: u32,        // protocol drift counter, CLI-UI-8
}
```

Shared plumbing in `engines/process.rs`:
- Spawn with `kill_on_drop(true)`, piped stdio, `CREATE_NO_WINDOW`.
- `jobobject::assign_raw(handle)`, a new function next to `assign`.
- A line reader with a **1 MiB per-line cap**. Longer lines are dropped and
  counted, never buffered without bound.
- A rolling 8 KiB stderr tail for errors.
- `kill_tree()`: protocol interrupt first, then `taskkill /T /F /PID` after a
  3 s grace.

**Unknown events are ignored and counted, never fatal.** This is what lets an
untested version mostly work.

### CLI-4 - Delegate branch (`agent/subagents.rs`)

- `AgentType` gains `engine_id: Option<String>`.
- `agent_types(db)` adds one built-in type per **enabled, found** engine, for
  example `{ name: "Claude Code", description: "A coding agent with its own
  tools that works in the attached folder. Best for multi-file code changes and
  running tests. It cannot see our conversation." }`. It also adds every
  spawnable persona with `engine_id` set. **A disabled or missing engine is not
  listed** (LRN-5), so the lead can't pick something that will fail.
- Which engines are listed for a given run also depends on the rule set: its
  `use_mode`, the conversation's `engines_off`, and the up-front guards
  (RTE-1, RTE-3, RTE-5). `agent_types` takes a `RouteCtx { conversation_id,
  headless, delegate_to: Option<String> }` for that (`delegate_to` from
  `/delegate <agent>`, CLI-UI-6).
- In `delegate`, after the child conversation is opened and `fleet.open` has
  returned a run, branch: `if let Some(e) = matched.engine_id { engines::run_external(...) } else { run_agent(...) }`.
  Both paths settle the same way (status, report, `REPORT_CLIP`).
- **Engines need a folder.** If the parent conversation has no attached
  folder, the task returns an error the lead can read: *"Claude Code works in a
  folder and this conversation has none. Ask the user to attach one, or do it
  yourself."* Engines never run in the app's data dir or home.
- Background (`SUB-10`) works unchanged. A headless parent's engine child
  gets `headless: true` (see `CLI-6`). For a headless parent, `agent_types`
  lists only engines with `engines.headless = 1` (`CLI-12`).
- Limits: an engine child also needs a free `engines.max_parallel` slot. If none
  is free, it waits as `queued`, which the Fleet already shows. Its wall clock
  is `engines.timeout_secs`.
- The child gets no tools from us in v1 (Phase 5 adds them), so toolset
  narrowing doesn't apply. Folder and trust are inherited exactly as today.

### CLI-5 - What the engine is told

`system_append` is harness-written and fixed. No model text goes in it except
the task, which is fenced (LRN-4):

```
You were given one task by Poiesis, a desktop assistant, on behalf of its user.
You cannot see the conversation it came from. Work only inside the current folder.
Finish with a short report: what you changed (files), what you ran and its result,
and anything left undone. Your final message is the whole report the user's
assistant receives.
<<<POIESIS_TASK
{task}
{output shape, if any}
POIESIS_TASK>>>
```

Claude: `--append-system-prompt-file` (temp file, deleted after) plus the task
as the stdin user message. Codex: `turn/start` input text. ACP: the
`session/prompt` text, since ACP has no system slot. The engine's own project
memory (`CLAUDE.md`, `AGENTS.md`) loads as it normally would. That's intended,
and it's why the folder must be the real project.

### CLI-6 - Permissions: one panel, the engine's own modes

`PermissionRequest` gains:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub engine: Option<String>,   // display name: "Claude Code"
#[serde(skip_serializing_if = "Option::is_none")]
pub tool: Option<String>,     // engine's tool name: "Bash", "Edit", "execCommand", ACP toolCall kind
```

`argv` (commands), `diff` (edits) and `path` are filled from the engine's
request so the existing panel renders them. The answers are *Allow once*,
*Allow for this run* (remembered in the run's memory only, keyed by
tool + exact command or path), and *Deny*.

The trust used is `min(folder trust, engines.trust_ceiling)` (`CLI-12`).
Trust → engine mode. Never the engine's "skip all permissions" mode in v1:

| Poiesis trust | Claude Code | Codex (app-server) | ACP |
|---|---|---|---|
| read-only | `--permission-mode plan` | sandbox read-only, approval on-request | deny every edit/execute request automatically |
| confirm (default) | `--permission-mode default` + prompt tool | workspace-write, approval untrusted | every request to the panel |
| auto | `--permission-mode acceptEdits` + prompt tool (commands still ask) | workspace-write, approval on-request | allow edit-kind requests inside the folder; everything else to the panel |

Exact Codex parameter names are confirmed against the `CLI-0` recording.

Every answer **fails closed**:
- A timeout, Stop, a dropped panel or a headless run is a deny, with message
  *"Not allowed: nobody is here to approve it"* for headless runs.
- ACP: only pick an option whose `kind` is `allow_once`/`allow_always`, and
  only when the user allowed. Never pick by position.

**What we can't see:** an engine's *own* allow rules (e.g. the user's
`~/.claude/settings.json` permissions) let some steps through without asking
us. The driver counts tool starts that never raised an approval, and the
child's report footer lists them: *"Claude Code's own settings allowed 4 steps
without asking."* (`CLI-UI-3`).

### CLI-7 - Claude Code driver (`engines/claude.rs`)

- Spawn: `-p --input-format stream-json --output-format stream-json --verbose
  --permission-mode <m> --append-system-prompt-file <f> [--resume <id>]
  --strict-mcp-config --mcp-config <file> --permission-prompt-tool mcp__poiesis__approve`.
  Write the user message to stdin as one stream-json line, keep stdin open for
  steering, and close it after `result`.
- `--strict-mcp-config` keeps the user's global MCP servers out of a delegated
  run. This is a deliberate narrowing, revisited in Phase 5.
- The MCP config file contains `{"mcpServers":{"poiesis":{"type":"http","url":"http://127.0.0.1:<port>/<token>/mcp"}}}`
  (MCPS-1). It is written to the run's temp dir and deleted on settle. The
  token dies with the run.
- Event mapping (confirm in `CLI-0`):

| stream-json | AgentEvent |
|---|---|
| `system` / `init` | store `session_id` → `conversations.engine_session` |
| `assistant` content `text` | `Token` |
| `assistant` content `thinking` | `Thinking` |
| `assistant` content `tool_use` | `StepStart { verb, target }` via `describe_engine_tool` (Bash → *ran* `cmd`; Edit/Write → *edited* path; Read → *read* path; Grep/Glob → *searched*; Task → *started a helper*) |
| `user` content `tool_result` | `StepDone` / `StepError` (clipped to 2 KB) |
| `result` | outcome: text, `total_cost_usd` → `notional_cost_usd`, usage, `is_error` |

- Steer: write a stream-json user message to stdin mid-turn
  (`steer_mid_turn: true`). Confirm the CLI accepts it in `CLI-0`, otherwise
  queue it for a `--resume` follow-up turn.
- Interrupt: send the control interrupt message if `CLI-0` confirms one, else
  `kill_tree`.
- Resume failure (session gone): start fresh with a one-line note to the lead.
  Never retry silently more than once.

### CLI-8 - Generic ACP driver (`engines/acp.rs`)

Evaluate Zed's `agent-client-protocol` crate first. If its version, MSRV
(we're pinned at rust-version 1.77, see the coding plan) or async runtime
don't fit, write the ~400 lines of JSON-RPC ourselves on top of
`mcp/client.rs`'s framing.

- `initialize` (advertise `fs` capabilities **off** in v1: the agent uses
  its own file tools inside cwd), `authenticate` only if the agent asks and a
  method needs no browser, `session/new { cwd, mcpServers: [] }` or
  `session/load` when resuming, then `session/prompt`.
- `session/update` mapping: `agent_message_chunk` → `Token`,
  `agent_thought_chunk` → `Thinking`, `tool_call` → `StepStart`,
  `tool_call_update` (completed/failed) → `StepDone`/`StepError`, `plan` →
  ignored in v1. Drop any update with `_meta.isReplay` and anything before our
  prompt was sent.
- `session/request_permission` → `CLI-6`.
- Turn end is the `session/prompt` result `{ stopReason }`. Stop is
  `session/cancel`, then `kill_tree` after 3 s.
- Pooling: one process per child conversation, kept for 10 minutes idle, so a
  follow-up delegate to the same child skips the handshake. Killed on app exit
  by the job object.
- Quirks per agent go in a small table (`acp_quirks.rs`): spawn args,
  "needs `--experimental-acp`", how the version is reported. Adding a CLI means
  adding a row.

### CLI-9 - Environment and payment

The child env is the user's env:
- **minus** every variable in Poiesis's own secret namespace,
- **minus** other engines' provider keys (`OPENAI_API_KEY` for Claude Code,
  `ANTHROPIC_API_KEY` for Codex, and so on),
- **plus** `env_json`.

API-key mode (`engines.payment = 'api-key'`, `CLI-12`) injects the key from `secrets.rs` into the
child env only. It never goes in argv or files.

`paid_by` for the run is the probe's answer. `ExternalOutcome` records it.
Usage rows (`telemetry.rs` / Usage view) store engine runs with
`paid_by = plan` **outside** cloud spend. Their notional cost is kept for
display only.

### CLI-10 - Diagnostics

Per engine, keep in memory with the last 20 runs:
- unknown event count,
- the last stderr tail,
- the last error.

Expose through `engines_list`. A run that fails puts the stderr tail (last 20
lines) in the child conversation as an error step, so it's visible from the
Fleet card.

## Phase 2 - MCP endpoint for engines (MCPS)

### MCPS-1 - Streamable-HTTP MCP on the toolrpc port (`agent/mcpserve.rs`)

Extend `toolrpc.rs`'s router:
- `/<token>/tool` (existing, `run_code`) and `/<token>/mcp` (new) are told
  apart by a `kind` on `Live`.
- An MCP ticket is armed by `run_external`, **independent of
  `tools.script_rpc`**. That switch is about model-written code. This endpoint
  is reachable only by a process we spawned and handed the token to.
- Methods: `initialize` (protocol `2025-06-18`, same as our client),
  `notifications/initialized` (202, no body), `tools/list`, `tools/call`.
  Reply with `application/json` only; no SSE needed for one-shot calls.
- v1 tools: `approve` only.

`approve` implements Claude Code's permission-prompt-tool contract. Input
`{ tool_name, input, tool_use_id? }`; it returns a text content whose body is
JSON: `{"behavior":"allow","updatedInput":<input>}` or
`{"behavior":"deny","message":"<why>"}`. Confirm the exact shape in `CLI-0`.
It goes through the run's `Gate` to the same `PermissionPort` as `CLI-6`.

**Acceptance:** with a recorded Claude session replayed against the endpoint,
one allow and one deny round-trip produce the right JSON, and a revoked token
gets a 404 before any parsing.

### MCPS-2 - Poiesis tools for engines (Phase 5, after everything else is in)

Expose a narrow, read-mostly set through the same endpoint and the same
`dispatch` that `toolrpc` uses:
- `recall` (memory search, read-only),
- `present` (post a block into the child conversation),
- `ask_user` (only when not headless; a question card in the child),
- `remember` (proposes a memory, shown in the lead's turn for the user to keep).

Gated by a per-engine switch, *Let it use my memory*, default off. Same
untrusted marking and activity log as every other call.

## Phase 3 - Codex driver

### CLI-11 - `engines/codex.rs`

`codex app-server` over JSON-RPC, using `CLI-0`'s recording:
- `initialize` with `clientInfo` (TRM-5), then
  `thread/start { cwd, approvalPolicy, sandbox }` or `thread/resume`, then
  `turn/start { input }`.
- The documented method list (read 2026-10-13) has `turn/steer` and
  `turn/interrupt`, so Codex gets `steer_mid_turn: true` and
  `interrupt: true`. It also lists `item/permissions/requestApproval` and
  `tool/requestUserInput`. The first goes to `CLI-6`. The second becomes a
  question card when not headless, and an answer of *"Nobody is here to
  answer"* when headless.
- Notifications map to events like `CLI-7`'s table: agent message delta →
  `Token`, reasoning → `Thinking`, item started/completed for command
  execution and file change → steps.
- `item/commandExecution/requestApproval` / `item/fileChange/requestApproval`
  go to `CLI-6`, answered `{action:"accept"|"decline"}`.
- Turn end is `turn/completed` (carries usage). Stop is the protocol's turn
  interrupt (confirm the method), then `kill_tree`.
- A watchdog: no message for 120 s while a turn is open and no approval
  pending means a stall. Interrupt, then report `stalled` (OMB has the same
  guard; a wedged app-server accepts stdin and never replies).

### CLI-12 - What the user can configure (the settings model)

Everything the Settings section (`CLI-UI-1`) edits is stored in one of two
places. Nothing is hard-coded that the user might reasonably want to change.

**Per engine: columns on `engines` (CLI-1), plus these added in the same v31
migration:**

```sql
ALTER TABLE engines ADD COLUMN payment       TEXT NOT NULL DEFAULT 'plan';  -- 'plan' | 'api-key'
ALTER TABLE engines ADD COLUMN trust_ceiling TEXT NOT NULL DEFAULT 'confirm'; -- 'read-only' | 'confirm' | 'auto'
ALTER TABLE engines ADD COLUMN headless      INTEGER NOT NULL DEFAULT 0;   -- may run in scheduled/background-headless runs
ALTER TABLE engines ADD COLUMN memory_tools  INTEGER NOT NULL DEFAULT 0;   -- MCPS-2 tools mounted
ALTER TABLE engines ADD COLUMN model         TEXT;                         -- passed as --model / thread model; NULL = engine default
ALTER TABLE engines ADD COLUMN extra_args    TEXT NOT NULL DEFAULT '[]';   -- JSON array, appended last, validated (no prompt text, no auth-disabling flags per TRM-1)
```

| Setting | Effect |
|---|---|
| `enabled` | Listed as an agent type at all (`CLI-4`). |
| `program` | Path override; probe re-runs on change. |
| `payment` | `plan`: the CLI's own login. `api-key`: key from `secrets.rs` (service `poiesis.engines`, account = engine id) injected into the child env only (`CLI-9`). Switching to `api-key` with no key stored opens the key field. |
| `trust_ceiling` | The highest trust this engine ever gets. Effective trust = `min(folder trust, trust_ceiling)` in `CLI-6`. Lets the user give a folder `auto` for Poiesis but keep Claude Code at `confirm`. |
| `headless` | Off: a headless parent can't pick this engine (it isn't listed), per TRM-4. On: allowed, and every approval request is denied as usual. |
| `memory_tools` | MCPS-2 on/off. |
| `model` | Optional. The picker is filled from the CLI where it can list models (Codex `model/list`, ACP session config), else a free text field. Empty = "the engine's default". |
| `extra_args`, `env_json` | Advanced. Free-form, validated. |

**Global: `settings` keys.**

| Key | Default | Effect |
|---|---|---|
| `engines.max_parallel` | 2 | Engine children running at once, across the app. Separate from `subagents.max_parallel`, because each is a heavy process with its own limits. Clamped 1..4. |
| `engines.timeout_secs` | 1800 | Wall clock per engine child. Engines do long jobs, so this is separate from `subagents.timeout_secs`. Clamped 60..7200. |
| `engines.idle_close_secs` | 600 | ACP pool idle close (`CLI-8`). |
| `engines.detect_on_start` | true | Run `detect_all` in the background at boot. |

Tauri commands (`commands/engines.rs`, extending `CLI-2`):
`engines_update(id, patch: EnginePatch)` (any column above, validated
server-side), `engines_set_key(id, key)` / `engines_clear_key(id)` (secret
store only; the key is never returned to the frontend, only `has_key: bool`),
`engines_open_login(id)` (opens a terminal running the CLI's own login, TRM-2),
`engines_test(id)` (`CLI-13`), `engines_models(id)`.

### CLI-13 - "Try it" test run

`engines_test(id)` runs a real, tiny, tool-free turn in a fresh temp folder.
The prompt is *"Reply with the single word ready."* with trust `read-only`. It
returns `{ ok, reply, seconds, paid_by, version, error, stderr_tail }`. This is
what the **Try it** button shows. It proves the whole path (spawn, protocol,
sign-in, parse) before the user relies on it in a real job. The temp folder is
deleted afterwards.

## When Poiesis hands work over: the rule set (RTE)

Whether the lead *should* hand a job to an engine can't be left to the lead's
judgement alone, invisibly. The user needs to see the rules, change them, and
see which rule applied each time. Three layers do that, from loosest to
strictest:

1. **A mode per engine.** How much the lead may decide on its own.
2. **Rules per engine.** Plain-language *Use it for* / *Don't use it for*
   lines that the lead reads, the user edits, and every handover cites.
3. **Guards per engine.** Hard conditions checked in Rust before anything
   runs, whatever the model says.

An explicit `/delegate <agent> …` (CLI-UI-6) always wins over mode and rules,
but never over guards or `/external off`.

### RTE-1 - Mode per engine

`ALTER TABLE engines ADD COLUMN use_mode TEXT NOT NULL DEFAULT 'suggest';`
(in the v31 migration). Values:

| `use_mode` | Label in Settings | What happens |
|---|---|---|
| `on_request` | Only when you ask | Not offered to the lead at all. Used only through `/delegate <agent>` or a persona that runs on it. |
| `suggest` (default) | Suggest it, I confirm | The lead may pick it. Before anything runs, a **handover card** asks the user (RTE-4). |
| `auto` | Hand over on its own | The lead may pick it and the child starts at once. The Fleet row still shows which rule it cited. |

This replaces the global `engines.lead_hint` key from CLI-12 (removed). Headless
parents treat `suggest` as `on_request`, because nobody is there to confirm. Only
`auto` engines with `headless = 1` are offered to scheduled jobs.

### RTE-2 - Rules (schema and defaults)

```sql
CREATE TABLE engine_rules (
  id          TEXT PRIMARY KEY,
  engine_id   TEXT NOT NULL REFERENCES engines(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL,            -- 'use' | 'avoid'
  text        TEXT NOT NULL,            -- plain language, ≤ 140 chars, shown to the lead verbatim
  enabled     INTEGER NOT NULL DEFAULT 1,
  auto        INTEGER NOT NULL DEFAULT 0, -- 'Always for this': skip the handover card for this rule (RTE-4)
  position    INTEGER NOT NULL,
  source      TEXT NOT NULL,            -- 'default' | 'user' | 'proposed' (accepted from RTE-6)
  hits        INTEGER NOT NULL DEFAULT 0, -- handovers that cited it
  overrides   INTEGER NOT NULL DEFAULT 0, -- times the user declined/undid a handover citing it
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX engine_rules_engine ON engine_rules(engine_id, position);
```

Each rule gets a short stable label in the prompt, `U1`, `U2`… / `A1`… by
position, so the lead can cite it and the UI can show it.

**Defaults** (seeded when an engine row is first created; `source = 'default'`;
**Reset to defaults** restores exactly these):

*Claude Code* and *Codex*:
- Use: Changes to code that touch more than one file.
- Use: Running the tests or the build and fixing what fails.
- Use: Refactors, renames and migrations across a project.
- Use: Tracking down a bug that needs running code to find.
- Avoid: Explaining or reviewing code when nothing needs to change. I can read it myself.
- Avoid: One small edit in one file.
- Avoid: Anything that isn't code: mail, calendar, research, writing.

*ACP engines (OpenCode, custom…)*: no default rules, and `use_mode`
defaults to `on_request`. Until the user says what one is for, it's only used on
request.

### RTE-3 - Guards (checked in Rust, not by the model)

`ALTER TABLE engines ADD COLUMN guards_json TEXT NOT NULL DEFAULT '{}';`

```json
{
  "needs_folder": true,            // always true, not editable (CLI-4); shown as a locked chip
  "code_projects_only": true,      // the folder has .git, Cargo.toml, package.json, pyproject.toml, go.mod, *.sln …
  "min_trust": "confirm",          // never in a read-only folder unless the ceiling is read-only too
  "max_per_conversation": 5,       // stops a lead that hands everything over
  "not_in_projects": []            // project ids where it is never used
}
```

`code_projects_only` defaults to true for Claude Code and Codex. A failed guard
means:
- the engine is **not listed** for that run when it can be known up front
  (folder, project),
- or `delegate` returns a readable refusal naming the guard, e.g. *"Claude Code
  only works in code projects, and this folder isn't one (no .git or project
  file). Do it yourself or ask the user."*

### RTE-4 - The lead sees the rules and cites one

`subagents::tool_specs()` becomes `tool_specs(types: &[AgentType])`. Today it
is static, so the lead never sees which types exist or what they're for. Each
call to it is built from the run's agent types:

- `tasks[].agent` gets an `enum` of the type names.
- A new `tasks[].because` field (string, **required when `agent` is an
  engine**): *"Which of the user's rules this matches, by its label, e.g.
  'U2'."*
- The tool description gains one block per engine type, rendered
  deterministically (a fixed format, no model text):

```
Claude Code — a coding agent in its own process. Mode: suggest (the user confirms first).
  Use it for:  U1 Changes to code that touch more than one file.
               U2 Running the tests or the build and fixing what fails.
  Don't use it for:  A1 Explaining or reviewing code when nothing needs to change.
  These are the user's own rules. Follow them; cite the one that applies in `because`.
```

Tool specs are outside the prompt-assembly golden gate (`fixtures/prompt-assembly.*`
contain no tool specs), so no golden change is needed. Add a unit golden for
the rendered block instead (RTE-T-1).

In `delegate`, an engine task with a missing or unknown `because`, or one that
cites an `avoid` rule, is refused with the reason, so the lead can fix it in
one step. A valid one is stored on the child (`conversations.engine_because`,
v31) and `hits += 1`.

**Suggest mode: the handover card.** Before the child starts, raise a
`PermissionRequest` with `capability: "handover"`. That reuses the panel and
the `Decision` oneshot, like `domain`/`screen` already do. Fields: `engine`,
the task text, and the cited rule (label and text). Three answers:
- **Hand it over**: starts the child.
- **Do it yourself**: refused, and the lead is told *"The user wants you to do
  this yourself."* `overrides += 1` on the rule.
- **Always for this**: starts the child and switches *this rule* to
  auto-confirm. It does not switch the whole engine. Stored as
  `engine_rules.auto = 1` (add the column), and the card is skipped next time
  this rule is cited.

So `suggest` can be relaxed one rule at a time, which is the visible way trust
builds.

### RTE-5 - Per-conversation switch

`conversations.engines_off INTEGER NOT NULL DEFAULT 0` (v31). When it's on, no
engine is offered in that conversation, and `/delegate <engine>` is refused with *"You
turned external agents off for this chat."* It's set from the session menu and
from the composer chip (RTE-UI-3).

### RTE-6 - Rules learn, but only by proposal

After **3** overrides of the same rule within 30 days, or when the user
uses `/delegate <engine>` three times for the same kind of task the lead had kept for itself, make a
change proposal through the existing proposals mechanism:
- New target `"engine-rule"` in `changeProposals`. Badge on Settings →
  External agents, like Skills/Self.
- Payloads: *edit rule U1 to "…"*, *add avoid rule "…"*, or *turn rule off*.
  Wording is drafted by the lead's model on a one-shot call.
- Never applied without the user's click. Accepted ones get
  `source = 'proposed'`.

Later, behind Settings → Experiments, the DUAL_PROCESS plan's decision model
can pre-match a request against rules before the lead runs (a `USE-*` site).
This is optional and not required here.

### RTE-7 - Rule preview

`engines_route_preview(text: String) -> RoutePreview` runs one cheap one-shot
call on the default chat model. It gets the rendered rule blocks from RTE-4 and
the instruction *"Would you hand this request to one of these agents? Answer
JSON {agent|null, because, reason}"*. Guards are evaluated in Rust against the
current conversation, or against "a code project folder" when called from
Settings. It returns what the lead would most likely do, which rule, and any
guard that would block it. This powers **Try a request** in Settings.

## Lessons from OpenMausBot that aren't about CLIs (LRN)

### LRN-1 - Stop on evidence, not on step count

> **Built 2026-10-23**, in the goal loop only (`src/lib/goal.ts`). See Part X, "LRN report".

Wherever Poiesis continues a run on its own (goal runs in `agent/goal.rs`,
background children, plan-driven continuation), add OMB's two guards. They
read the run's own step records, never model text:
- **No progress:** two continuations in a row with zero successful tool calls.
- **Same wall:** the same error text three times across continuations.

Either ends the run `stalled` with the reason in plain words: *"I stopped after
three identical errors: …"*. Feeds the outcome labels in
`DUAL_PROCESS_PLAN.md` (CTL).

### LRN-2 - Never redo something that left the machine

> **Built 2026-10-23** as a refusal on resume (`agent/log.rs::outward_done`). See Part X, "LRN report".

On restart after a crash, a run that was in flight is re-run **at most once**,
and **not at all** if its activity log shows a completed outward action
(`send_mail`, a publish, a `git push` through a task, a payment-like tool). It
ends `interrupted`, with *"I was cut off after I had already sent the email,
so I didn't start over."* First check what `background.rs` and goal runs do
today on restart. If they never re-run, this becomes only the "tell the user"
half.

### LRN-3 - Clean environments for every child process

> **Built 2026-10-23.** The code sandbox and the project tasks were already scrubbed; stdio MCP
> servers were the gap and are closed. See Part X, "LRN report".

Apply `CLI-9`'s scrubbing to `coderun.rs`, `codeexec.rs` and stdio MCP servers
too. None of them should inherit a provider key or an internal Poiesis
variable they weren't given on purpose.

### LRN-4 - Harness-written envelopes, escaped imitations

> **Built.** Agent-app children with the engine work (`engines/run.rs`, `defang`); ordinary
> children's reports on 2026-10-23 (`agent/subagents.rs::escape_frame`). Part X, "LRN report".

A child's report reaches the lead wrapped in a fixed harness frame (from whom,
which task, what the harness *observed*: files changed, commands run, denials).
Any text in the report that imitates the frame's markers (`<<<POIESIS_`,
`POIESIS_TASK>>>`) is escaped before the lead sees it. Same idea as
`untrusted.rs`, applied to reports between agents.

### LRN-5 - Capability truth

> **Built.** Agent types and agent apps with the engine work (`agent/subagents.rs::agent_types`,
> `engines/rules.rs`); the toolset assembly on 2026-10-23 (`Toolset::ready`). Part X, "LRN report".

Never list a tool, agent type or engine the current run can't actually use.
`CLI-4` applies it to engines. Audit `agent_types` and the toolset assembly for
the same rule.

### Already covered, recorded so nobody redoes it

- OMB wraps third-party MCP servers to cap result size (one call returned
  60-140 KB). Poiesis already caps large tool results (`HRN-8`, `results.rs`).
  Verify that MCP tool results go through `ResultStore` like built-ins do.

---

# Part V - Frontend tasks

### UI integration map

Where every setting and surface lives. **All configuration is in one place:
Settings → External agents.** Every other surface links into it rather than
holding its own copy.

| Surface | File(s) | What changes | Task |
|---|---|---|---|
| **Settings → External agents** (new hub section) | `src/lib/types.ts` (`HUB_SECTIONS`, `View`), `src/routes/SettingsHub.tsx`, new `src/routes/Engines.tsx` + `Engines.css`, `src/components/Icons/Icons.tsx` (`SectionIcon` case `"engines"`) | The whole configuration UI | CLI-UI-1 |
| Settings → External agents → *When I use it* | new `src/components/Engines/RuleList.tsx`, `Engines.tsx` | Mode, rules, guards, proposals | RTE-UI-1 |
| Settings → External agents → *Try a request* | `Engines.tsx` | Routing preview | RTE-UI-4 |
| Handover card | the `PermissionRequest` panel (`capability: "handover"`) | Suggest-mode confirm with the cited rule | RTE-UI-2 |
| `/` menu | `shared/commands.json`, `src/lib/commands.ts` (`CommandArgs.lead`, `ArgSource "agents"`, `argChoices`, `parseCommandLine`, `skillSend`), `src-tauri/bundled_skills/delegate/SKILL.md` | `/delegate [agent] task`, `/external on\|off` | CLI-UI-6 |
| Mode chip and session menu | `Composer/ModeChips.tsx`, `Conversation/SessionMenu.tsx` | *External agents off* chip (only when off) | CLI-UI-6, RTE-UI-3 |
| Settings hub nav badge | `src/routes/SettingsHub.tsx` (`badgeFor`), store flag `enginesNewlyFound` | Dot when a new agent was found | CLI-2 |
| Settings → Providers | `src/routes/Providers.tsx` | One link row under Cloud accounts: *Coding agents you've installed (Claude Code, Codex…) are set up in External agents →* | CLI-UI-1 |
| Settings → Tools | `src/routes/Tools.tsx` (`DelegationCaps`) | A line under the delegation caps: *External agents have their own limits →* (link) | CLI-UI-1 |
| Settings → General → Personas | `src/routes/Settings.tsx`, `Personas/PersonaEditor.tsx` | "Runs on" select | CLI-UI-4 |
| Settings → Usage | `src/routes/Usage.tsx` | Agent apps group | CLI-UI-7 |
| Turn | `Conversation/FleetCard.tsx`, `Workbench/AgentsPanel.tsx` | Engine rows; the gear on a row opens that engine's settings | CLI-UI-2 |
| Permission panel | the `PermissionRequest` panel | Attributed header; *Change what Claude Code may do →* link to its Permissions group | CLI-UI-3 |
| Child conversation | `Conversation/AgentRun.tsx` header | Continue there | CLI-UI-5 |
| Diagnostics | `Conversation/Diagnostics.tsx` | Engine health; the full detail is in the engine's Settings page | CLI-UI-8 |
| Store | `src/lib/store.ts`, `src/lib/api.ts`, `src/lib/types.ts` | `engines: EngineRow[]`, `loadEngines()`, `updateEngine(id, patch)`, `EngineRow`/`EnginePatch` types mirroring `CLI-12` | CLI-UI-1 |

### CLI-UI-1 - Settings → External agents

**Place.** A new hub section `{ view: "engines", label: "External agents", icon: "⇄" }`
in `HUB_SECTIONS`, directly after `providers`. It's a peer of Models and
Providers because it answers the same question ("what can think for me?").
Add `"engines"` to the `View` union, render `<Engines />` in `SettingsHub.tsx`,
and add a `SectionIcon` case. Deep link: `setView("engines")` plus an optional
`engineFocus: string` in the store, which scrolls to and expands that engine.
The FleetCard gear, the permission-panel link and the Diagnostics link all use
it.

**Layout** (`src/routes/Engines.tsx`; same card and section classes as
`Runtime.tsx` / `Providers.tsx`):

```
External agents
I can hand work to agent apps you've installed. They work in your folder with
their own model and tools; I keep the conversation and the final word.

┌ Installed ──────────────────────────────────────────── [Check again] ┐
│ ◆ Claude Code   2.1.290 · signed in · on your plan    [● Let me use it] ▸ │
│ ◆ Codex         0.150.0 · not signed in               [○]  [Sign in]   ▸ │
└───────────────────────────────────────────────────────────────────────┘
┌ Not found ────────────────────────────────────────────────────────────┐
│ ◇ OpenCode      I didn't find it on this computer.  [How to install] [Set path…] │
└───────────────────────────────────────────────────────────────────────┘
[+ Add another agent (ACP)…]

┌ Limits for all external agents ──────────────────────────────────────┐
│ At once            [ 2 ▾ ]     Longest job        [ 30 min ▾ ]       │
└───────────────────────────────────────────────────────────────────────┘
┌ Try a request ───────────────────────────────────────────────────────┐
│ [ fix the failing voice tests and clean up controller.ts        ] [Try] │
│ → I'd hand this to Claude Code, because of U2 "Running the tests…"  │
│   It would ask you first (Suggest).                                  │
└───────────────────────────────────────────────────────────────────────┘
```

**Engine row, collapsed:** generic glyph (TRM-3: no vendor logos), plain-text
name, one status line, the enable switch, a chevron. The switch is disabled
while the engine isn't found or isn't signed in (in plan mode), with the
reason as its tooltip. **Sign in** calls `engines_open_login` (a terminal
running the CLI's own login, TRM-2). When the terminal closes or the window
regains focus, the row re-probes automatically.

**Engine row, expanded (▸).** Six groups, each a short form that saves on
change (no Save button, same as Settings). The first group is the
most-used one:

0. **When I use it** (RTE-UI-1, specified below): mode, rules, guards.
1. **Connection**
   - *Program*: the resolved path, read-only text, with **Change…** (file
     picker) and **Reset** (back to auto-detect). Shows `npm shim` as a muted
     tag when it applies (`CLI-2`).
   - *Version*: `2.1.290 · tested`, or an amber *newer than I've been tested
     with*.
   - **Try it** (`CLI-13`): spinner, then *Ready in 4.2 s · on your plan* or the
     error with its stderr tail.
2. **Billing**
   - *Pay with*: radio, **My plan (the app's own sign-in)** /
     **An API key**.
   - Choosing API key shows a password field (*Paste a key*). The field is
     write-only: once saved it shows *Key saved · Remove*, and the key is never
     shown again (`engines_set_key`).
   - A muted line under **My plan**: *Its usage counts toward your plan's
     limits, the same as using it yourself.* (TRM-4)
3. **Permissions**
   - *At most*: a select, **Read only** / **Ask me first** (default) /
     **Edits without asking**, with help text: *Even in a folder where I may
     edit freely, it never gets more than this. Commands always ask.*
     (`trust_ceiling`)
   - *In scheduled and background jobs*: switch, default off. Help: *Nobody is
     there to approve anything, so it can only do what needs no approval.*
     (`headless`)
   - *Let it use my memory*: switch, default off, enabled from Phase 5
     (MCPS-2). Before then it's hidden.
   - A muted line: *It runs outside my sandbox, in the folder of the
     conversation. Its own settings file may allow some steps without asking
     me, and I'll tell you when that happens.*
4. **Model** (shown only when the engine can list models or takes `--model`)
   - *Model*: a select filled by `engines_models`, first entry **Its own
     default**.
5. **Advanced** (collapsed by default)
   - *Extra arguments* (one per line, validated with an inline error),
     *Environment* (key/value rows; values that look like secrets get a
     warning: *Use Billing → API key for keys, so I keep them in the vault*).
   - *Recent runs*: the last 5 with outcome, duration and unknown-event count,
     plus **Copy diagnostics** (version, last error, stderr tail; no env, no
     paths outside the folder) (`CLI-10`).
   - Custom ACP agents only: **Remove**.

**Add another agent (ACP)…** opens an inline card with *Name*, *Program*
(file picker), *Arguments* (default `acp`) and **Add and check**. Add means
`engines_add_acp`, then probe, then the row appears expanded with Try it
focused.

**Empty states.**
- Nothing found at all: *I didn't find any agent apps on this computer. If you
  install Claude Code, Codex or OpenCode, I'll find it here.* with install
  links (plain text names, TRM-3).
- First visit with something found: the found rows are shown with their
  switches **off**, and the intro says *I found Claude Code. Turn it on when you
  want me to hand work to it.* Finding an engine is not consent (`CLI-1`).

**Acceptance:**
- Every column in `CLI-12` and every global key is editable here and nowhere
  else.
- Reloading the app shows the same values.
- A disabled engine never appears as an agent type (checked through
  `agent_types`).
- The key field never receives the stored key back from the backend.
- Keyboard: every control is reachable by Tab, and the row expands with
  Enter/Space.

### RTE-UI-1 - "When I use it" (in each engine's settings)

The first group inside an expanded engine row in `src/routes/Engines.tsx`.
Rule editing is its own component, `src/components/Engines/RuleList.tsx`.

```
When I use it
  ( Only when you ask )  (● Suggest it, you confirm )  ( On its own )

  Use it for                                            used  declined
  U1 ⠿ Changes to code that touch more than one file.    12     0    [●] ⋯
  U2 ⠿ Running the tests or the build and fixing…   ⚡   8     1    [●] ⋯
  U3 ⠿ Refactors, renames and migrations…                3     0    [●] ⋯
  + Add a rule

  Don't use it for
  A1 ⠿ Explaining or reviewing code when nothing…        —     —    [●] ⋯
  A2 ⠿ One small edit in one file.                       —     —    [○] ⋯
  + Add a rule

  Only when   [🔒 there's a folder]  [● it's a code project]  [● folder isn't read-only]
              At most [ 5 ▾ ] handovers per chat   Never in projects [ choose… ]

  Reset to defaults
```

- **Mode:** a segmented control writing `use_mode`. The help line under it
  changes with the choice:
  - on_request: *I won't pick it myself. Use /delegate claude-code … when you
    want it.*
  - suggest: *I'll suggest it when a rule fits, and you confirm each time.*
  - auto: *I'll hand work over when a rule fits, and tell you which rule.*
- **Rules:**
  - The text is edited inline (click to edit, Enter saves, Esc cancels,
    140-char counter). ⠿ drags to reorder (`position`), and labels renumber.
  - The switch writes `enabled`. ⋯ offers *Delete* and *Move to Don't use*.
  - `⚡` marks a rule set to *Always for this* (`auto = 1`); clicking it turns
    that off again.
  - *used / declined* are the `hits` / `overrides` counts. A rule declined more
    often than used gets an amber count, the visible hint that it's wrong.
  - Rules with `source = 'proposed'` carry a small *suggested by me* tag.
- **Guards:** chips for `guards_json`. The folder chip is locked with the
  tooltip *Agents always work in a folder*. *Never in projects* uses the
  existing project picker.
- **Reset to defaults:** asks for confirmation via the existing `Confirm`
  component, then restores RTE-2's defaults. Counts are cleared.
- **Pending proposals** (RTE-6) appear at the top of this group as a proposal
  card (reuse `ProposalCard.tsx`): *I keep being told not to use Claude Code
  for small fixes. Add "Don't use it for: one-line fixes"?* with **Add** /
  **Not now**.

### RTE-UI-2 - The handover card (suggest mode)

The permission panel with `capability: "handover"`:

```
◆ Hand this to Claude Code?
  "Fix the failing voice tests and clean up controller.ts"
  Because of your rule U2: Running the tests or the build and fixing what fails.
  [ Hand it over ]   [ Do it yourself ]   [ Always for this ]
  Change when I use Claude Code →
```

The link deep-links to RTE-UI-1 (`engineFocus`). When the user doesn't answer,
the card behaves like every other permission prompt: it waits, and the run
shows *waiting for you*.

### RTE-UI-3 - Visible "why" everywhere a handover appears

- **Fleet row** (`FleetCard.tsx`): a muted *because U2* chip after the engine
  badge. Hovering shows the rule text. Clicking opens RTE-UI-1 with that rule
  highlighted.
- **Child conversation header** (CLI-UI-5 strip): *I handed this to Claude
  Code because of your rule: "Running the tests or the build and fixing what
  fails."*
- **Undo as a signal:** the Fleet row's ⋯ menu gets *This shouldn't have gone
  to Claude Code*. It stops the child if it's still running, counts an
  override on the cited rule, and tells the lead *"The user says this
  shouldn't have been handed over. Do it yourself."*
- **Per-chat off** goes through `/external off` and the *External agents off*
  mode chip (CLI-UI-6 §2). The session menu (`SessionMenu.tsx`) gets the same
  toggle as *Use external agents in this chat*, for people who don't use
  commands.

### RTE-UI-4 - Try a request (Settings)

At the bottom of Settings → External agents: a text field and **Try**, which
calls `engines_route_preview`. The answer is shown in one line:
- *I'd hand this to Claude Code, because of U2.* plus the mode consequence, or
- *I'd do this myself. No rule fits.*, or
- *I'd want Claude Code (U1), but your guard "code projects only" would stop
  it.*

Each named rule is a link that scrolls to and highlights it. This is how the
user checks their rules without waiting for a real job.

### CLI-UI-2 - Fleet card rows for engine children

`FleetCard.tsx` and `Workbench/AgentsPanel.tsx`:
- Engine badge next to the agent name. The dot colour still comes from the
  existing hash.
- A chip: `on your plan` (muted) or `API key`.
- `nowDoing` uses the engine-mapped step verbs from `CLI-7` unchanged.
- The steer box is shown only when `capabilities.steer_mid_turn`. Otherwise the
  placeholder says *I'll pass this on when it finishes this step.*
- Stop works as today. While the process is being killed the row says
  *stopping Claude Code…*.
- A small gear on an engine row opens Settings → External agents with that
  engine expanded (`engineFocus`).

### CLI-UI-3 - Attributed permission panel

The existing permission panel (the component that renders `PermissionRequest`;
same one `SUB-UI-4` attributed):
- When `engine` is set, the header is **Claude Code wants to run a command** /
  **…wants to edit `path`**, with the engine glyph.
- Body: `argv` one token per line, `diff` as today.
- Buttons: *Allow once* · *Allow for this run* · *Deny*.
- A single muted line under it: *It runs outside my sandbox, in
  `<folder>`.*
- The child report footer lists self-allowed steps (`CLI-6`).
- A text link, *Change what Claude Code may do*, opens its Permissions group in
  Settings → External agents. The panel itself never changes settings.

### CLI-UI-4 - Personas editor: "Runs on"

`Personas/PersonaEditor.tsx`, for spawnable personas only:
- A **Runs on** select: *Poiesis (choose a model)* | each enabled engine.
- Choosing an engine hides the model and tools fields and shows: *It brings its
  own model and tools. I give it this persona's instructions with the task, and
  the folder.* The persona's system prompt goes in front of the task in
  `CLI-5`'s fenced block.
- `DelegationCaps.tsx` gets no new caps, only the link line from the
  integration map. Engine children count toward the existing
  `subagents.max_parallel` / per-turn limits **and** toward
  `engines.max_parallel`, which is set in External agents. The tighter one
  wins. Their wall clock is `engines.timeout_secs`, not
  `subagents.timeout_secs`.
- If the chosen engine is later disabled or uninstalled, the select shows it
  struck through with *(off — turn it on in External agents)*, and the persona
  isn't offered as an agent type (LRN-5).

### CLI-UI-5 - Child conversation: "Continue there"

The header strip of a child conversation whose `engine_session` is set:
*I handed this to Claude Code.* **Continue there** copies
`claude --resume <id>` (or the engine's equivalent) and opens a terminal in the
folder through the existing opener plugin. The command is copied whatever
happens, so it works even if the terminal can't be opened.

### CLI-UI-6 - Choosing an agent through the `/` menu (no new syntax)

Poiesis has one command surface: the `/` menu (`shared/commands.json` →
`src/lib/commands.ts`, `COMMANDS_PLAN`). It already has `/delegate` (section
*Agents and time*, the bundled skill `src-tauri/bundled_skills/delegate/SKILL.md`,
which calls `delegate` with `background: true`). It also already has the
on/off mode pattern (`/tools`, `/workspace`) with a `ModeChips` chip that
appears only when a mode differs from the default. Engines use both. **There is
no `@` syntax.**

**1. `/delegate` gets an optional "who".**

- Manifest change (`shared/commands.json`):
  ```json
  { "name": "delegate", "section": "agents", "kind": "skill",
    "args": { "type": "text", "hint": "[agent] task", "required": true, "lead": "agents" },
    "summary": "Hand this to an agent and keep talking", … }
  ```
- `CommandArgs` gains `lead?: ArgSource`, meaning *an optional first word
  chosen from a source, then free text*. `ArgSource` gains `"agents"`.
- `argChoices` for `"agents"` returns the run's agent types (`agent_types`
  through a new `api.listAgentTypes()`, already served by
  `commands/subagents.rs`):
  - *General*,
  - spawnable personas,
  - every **found** engine.

  A found but disabled engine, or one blocked by a guard in this conversation,
  is listed with `disabledReason`: *Off. Turn it on in External agents*, or
  *Only in code projects*. Choosing a disabled one opens Settings → External
  agents on that engine instead of inserting it.
- The choice popover works like other choice arguments: it opens when the user
  types `/delegate `, filters as they type, and Tab/Enter inserts the agent's
  slug (`claude-code`, `codex`, `opencode`, persona slug). The rest of the
  line is the task.
- `parseCommandLine` splits the leading word off when it matches an agent
  slug, else the whole text is the task (today's behaviour, general agent).
- `skillSend` passes it on: `{ kind: "send", text, skill: "delegate",
  skillArgs: task, delegateTo: "claude-code" }`. `TurnModifiers` gains
  `delegateTo?: string`, cleared after send like every modifier.
- Backend: `delegate_to: Option<String>` on the turn.
  - `SKILL.md` gets `$AGENT`, rendered as *"Use the agent named "Claude
    Code"."*, or removed when there's none.
  - **Enforced in Rust:** in that turn, `delegate` tasks must name that agent
    (anything else is refused with *"The user asked for Claude Code."*). The
    `because` is recorded as `asked`. Mode and rules are skipped, because the
    user chose. Guards and `engines_off` are not.
  - In the Fleet row, the "why" chip reads *you asked*.
- `/delegate` stays background (the user keeps talking). For an **engine**
  child, the SUB-10 rule "anything needing approval is refused" is replaced:
  the run isn't headless, the user is in the app, and CLI-UI-3's attributed
  panel makes it clear who is asking. So engine approvals still reach the
  panel. A headless parent keeps the old rule.

**2. `/external on|off` for this chat.**

- A new manifest entry beside `/tools`:
  ```json
  { "name": "external", "aliases": [], "section": "modes", "kind": "action",
    "args": { "type": "choice", "source": "onoff", "hint": "on|off", "required": true },
    "summary": "Let me hand work to the agent apps you've set up", "who": "user",
    "needs": ["conv"], "trace": true }
  ```
- Its handler sets `conversations.engines_off` (RTE-5) via a new
  `api.setConversationEnginesOff(id, off)`.
- Hidden when no engine is enabled, with the menu's usual `disabledReason`:
  *No external agents are set up. See Settings → External agents.*
- `ModeChips.tsx`: when `engines_off` is on (a deviation from the default), it
  shows the chip **External agents off**. The label opens `/external `, and ×
  turns them back on. That's the same pattern as *Tools off*. No chip in the
  normal state.

**3. `/help` and discovery.** Both entries appear in `/help` automatically
from the manifest. The `/delegate` summary changes from *background agent* to
*an agent*, because the user can now pick which. Nothing else in the
composer changes.

### CLI-UI-7 - Usage

`routes/Usage.tsx`: an **Agent apps** group lists runs, tokens and duration per
engine. Notional cost is shown grey with a tooltip: *Reported by Claude Code.
On your plan this isn't charged per run.* Excluded from cloud totals.

### CLI-UI-8 - Diagnostics

`Conversation/Diagnostics.tsx` (or the existing diagnostics surface) shows,
per engine: tested/untested, unknown events in the last 20 runs, last error.
A rising unknown count is the early warning that a CLI update changed its
protocol.

---

# Part VI - Tests

- **CLI-T-1 Parser goldens.** Replay each `fixtures/engines/*.jsonl` through
  its driver's parser and compare the `AgentEvent` sequence and the outcome
  to a checked-in golden. A new CLI version means a new fixture and golden;
  the old ones stay.
- **CLI-T-2 Fake engine end to end.** A test binary (`src-tauri/tests/bin/fake_engine.rs`)
  replays a fixture over stdio, answering our writes by matching ids. Then
  `delegate` → child conversation → Fleet events → report to lead, with no real
  CLI installed. This runs in CI.
- **CLI-T-3 Approval round trip.** Allow, allow-for-run (second identical
  request is not shown), deny, timeout = deny, headless = deny. For each
  approval type: MCPS `approve`, Codex in-band, ACP. ACP: options in reversed
  order still choose the allow-kind option; no allow option means deny.
- **CLI-T-4 Stop kills the tree** within 3 s on Windows, including the
  CLI's own children (fake engine spawns a grandchild).
- **CLI-T-5 Environment.** The child sees no Poiesis secret vars and no other
  engine's key; API-key mode puts the key only in env.
- **CLI-T-6 No folder, no engine.** `delegate` to an engine type without a
  folder returns the readable error; the engine type isn't listed when its
  engine is disabled.
- **CLI-T-7 Untested version** still runs; unknown events are counted, not
  fatal; a 2 MiB line is dropped and counted.
- **CLI-T-9 Settings take effect.** Run with the fake engine:
  - `trust_ceiling = confirm` in an `auto` folder: an edit request reaches
    the panel.
  - `enabled = 0`: the type isn't listed.
  - `headless = 0`: not listed for a headless parent.
  - `payment = api-key`: the key is in the child env, and `engines_list` never
    returns it.
  - `engines.max_parallel = 1` with two tasks: the second shows `queued`
    until the first settles.
- **CLI-T-10 Settings UI** (`src/routes/Engines.test.tsx`, Vitest): renders
  found / not found / untested / not signed in rows. The switch is disabled
  when it can't work. Edits call `updateEngine` with the right patch. The key
  field is write-only. A deep link via `engineFocus` expands the right row.
- **RTE-T-1 Rendered rules golden.** `tool_specs(types)` for a fixed set of
  engines and rules matches a checked-in text: labels, order, mode line,
  disabled rules left out.
- **RTE-T-2 Delegate enforcement.** The following are each refused with a
  readable reason:
  - an engine task with no `because`,
  - an unknown label,
  - an `avoid` label,
  - an engine in `on_request` mode without `/delegate <engine>`,
  - `/delegate claude-code` while the lead names a different agent,
  - `engines_off` set,
  - each guard failing.

  Valid ones store `engine_because` and increment `hits`.
- **RTE-T-3 Handover card.** `suggest` raises `capability: "handover"`.
  - *Do it yourself*: refusal text to the lead and `overrides += 1`.
  - *Always for this*: sets `auto` on that rule only, and the next citation of
    it skips the card while a different rule still shows it.
  - Headless + `suggest`: the engine isn't listed.
- **RTE-T-4 Proposal trigger.** Three overrides of one rule in 30 days create
  exactly one `engine-rule` proposal. Accepting it writes the rule with
  `source = 'proposed'`; rejecting it changes nothing.
- **RTE-T-5 Rule editor** (`RuleList.test.tsx`): inline edit and save, the
  140-char limit, reorder renumbers labels, toggle, move to *Don't use*,
  reset restores defaults after confirm, amber count when declined > used.
- **CLI-T-12 Detection.** Using a temp dir as fake `%USERPROFILE%` and
  `%APPDATA%`, plus a fake registry-`PATH` provider:
  - each install location is found,
  - user `program` beats everything,
  - a binary added to the registry `PATH` *after* startup is found on the next
    focus check,
  - a first find creates a disabled row and sets the badge,
  - an uninstalled engine is kept but not listed,
  - a non-catalog binary on `PATH` is never run.
- **CLI-T-11 Commands** (`src/lib/commands.test.ts`):
  - `/delegate claude-code fix the tests` parses to `delegateTo` +
    `fix the tests`.
  - `/delegate fix the tests` has no `delegateTo`.
  - A disabled engine appears with its `disabledReason`.
  - `/external off` sets `engines_off`, and the *External agents off* chip
    appears; × clears it.
  - `/external` is hidden with no engine enabled.
- **CLI-T-8 Manual pass (Erich).** Real Claude Code on a real repo: a
  refactor with one approval and a Stop in the middle, then Continue there in
  a terminal.

# Part VII - Copy (PRES-0)

| Where | Copy |
|---|---|
| Settings section name | External agents |
| Section intro | I can hand work to agent apps you've installed. They work in your folder with their own model and tools; I keep the conversation and the final word. |
| Switch | Let me use it |
| Not signed in | Installed, but not signed in. Sign in opens its own login, and I never see your password or tokens. |
| Plan billing note | Its usage counts toward your plan's limits, the same as using it yourself. |
| Trust ceiling help | Even in a folder where I may edit freely, it never gets more than this. Commands always ask. |
| Headless help | Nobody is there to approve anything, so it can only do what needs no approval. |
| Try it, success | Ready in 4.2 s · on your plan |
| First found | I found Claude Code. Turn it on when you want me to hand work to it. |
| Rules group title | When I use it |
| Modes | Only when you ask · Suggest it, you confirm · On its own |
| Mode help (suggest) | I'll suggest it when a rule fits, and you confirm each time. |
| Handover card | Hand this to Claude Code? Because of your rule U2: … |
| Handover buttons | Hand it over · Do it yourself · Always for this |
| Fleet chip | because U2 |
| Undo | This shouldn't have gone to Claude Code |
| Guard refusal | Claude Code only works in code projects, and this folder isn't one. |
| Chat off | You turned external agents off for this chat. |
| Rule proposal | I keep being told not to use Claude Code for small fixes. Add "Don't use it for: one-line fixes"? |
| Try a request | I'd hand this to Claude Code, because of U2. · I'd do this myself. No rule fits. |
| Fleet chip | on your plan · API key |
| Permission header | Claude Code wants to run a command |
| Sandbox line | It runs outside my sandbox, in `<folder>`. |
| No folder | Claude Code works in a folder and this conversation has none. Attach one and I'll hand it over. |
| Self-allowed footer | Claude Code's own settings allowed 4 steps without asking. |
| Untested version | Version 2.3.0 is newer than I've been tested with. It should still work, and if it doesn't, the error will say why. |
| Continue there | I handed this to Claude Code. Continue there |
| Stalled (LRN-1) | I stopped after three identical errors: … |

# Part VIII - Sequencing

1. **Phase 0:** CLI-0 recordings. CLI-0b is done (2026-10-13); TRM-7 is
   needed only before a public release.
2. **Phase 1:** CLI-1, CLI-2, CLI-3, CLI-12, CLI-13, CLI-UI-1 (the whole
   Settings section, including Try it). Engines are visible, configurable and
   testable from Settings, and none is offered to the lead yet.
3. **Phase 2:** CLI-4, CLI-5, CLI-6, CLI-7, MCPS-1, CLI-9, CLI-10, **RTE-1..5,
   RTE-7**, CLI-UI-2, CLI-UI-3, CLI-UI-5, **RTE-UI-1..4**, CLI-T-1..T-6,
   RTE-T-1..3, RTE-T-5. **Claude Code works end to end, under rules the user
   can see and change.** This is the first thing worth shipping. The rule set
   ships with the first engine, not after it: an engine that gets used
   without visible rules is the thing this plan avoids.
4. **Phase 3:** CLI-8 (ACP) + CLI-UI-4, CLI-UI-6, CLI-T-7.
5. **Phase 4:** CLI-11 (Codex), CLI-UI-7, CLI-UI-8.
6. **Phase 5:** MCPS-2, RTE-6 (proposals; it needs real override counts to
   be useful), RTE-T-4.
7. **LRN-1..5** are independent of the engines and can go in any gap; LRN-3
   and LRN-5 are small and worth doing early.

# Part IX - Risks and parked items

- **Terms move (CLI-0b).** Today the user's own unmodified CLI with their own
  sign-in is explicitly allowed (Anthropic) or documented for embedding
  (OpenAI). Both vendors changed their stance several times in 2026. If
  either tightens, flip that engine's default `payment` to `api-key` in a
  patch release. The Settings UI already supports it, so no redesign is
  needed. TRM-6 makes the re-check part of every release.
- **Protocol drift.** CLIs ship weekly. Mitigations: fixtures per version,
  unknown events counted rather than fatal, a visible "untested" state, and
  the stderr tail on failure. OMB carries ~4k-line test files per engine for
  this reason. Expect ongoing maintenance, not a one-off build.
- **Outside the sandbox.** An engine can do anything its own permission system
  allows in the folder, and its own allow rules can skip us. The panel line and
  the self-allowed footer say so. Default trust stays `confirm`, and
  `bypassPermissions`-style modes are never used in v1.
- **Windows shims.** npm `.cmd` shims and argument quoting. Prompt text never
  goes in argv, and native `.exe` paths are preferred.
- **Two memories.** The engine reads its own `CLAUDE.md`/`AGENTS.md` and keeps
  its own session. Poiesis memory reaches it only through MCPS-2. That is
  acceptable; the report back is what Poiesis remembers.
- **CLI-P1 (parked): an engine as a chat engine.** Choosing Claude Code in the
  model picker for a whole conversation. Parked because it bypasses prompt
  assembly, memory and blocks. Revisit after Phase 5, when MCPS can give an
  engine our tools.
- **CLI-P2 (parked): ACP the other way.** Poiesis *as* an ACP agent, so Zed or
  other editors could drive it. Interesting, and out of scope here.
- **CLI-P3 (parked): Gemini CLI.** Removed from this plan on 2026-10-20 at
  Erich's request. Nothing in the build depends on it. To add it back: one
  catalog entry in `shared/engines.json` (kind `acp`, binary `gemini`, ACP
  argument `--experimental-acp`) plus a quirks row, a Google sign-in done by the
  user, and a recording that shows a permission request for an edit and for a
  command (the rule for every ACP agent, see Part X).

---

# Part X - Implementation status

Last updated 2026-10-23 (Phases 0-5 done; LRN-1..5 built; review fixes: see "Review report"). This part records what is built and what was learned.
Read it before Parts I-IX when you resume. Where it disagrees with an earlier
part, **this part wins**.

## Where we are

| Phase | State |
|---|---|
| 0 - Recordings, terms | **Done 2026-10-20** for Claude Code, Codex and OpenCode (the ACP engine). Gemini CLI was removed from the plan on Erich's request (parked, `CLI-P3`). CLI-0b was already done. |
| 1 - Registry, probe, driver contract, Settings | **Done 2026-10-20** for Claude Code, Codex and OpenCode. Found, probed, configurable and testable from Settings → External agents. Nothing is offered to the lead yet. See "Phase 1 report". |
| 2 - Claude Code end to end, MCPS-1, rules | **Done 2026-10-20** for Claude Code (backend, frontend and tests; checked against a fake agent, not yet by eye or with the real Claude Code on a repo). See "Phase 2 status and report" and its Open items. |
| 3 - ACP (OpenCode), `/delegate [agent]`, `/external`, persona "Runs on" | **Done 2026-10-20** (backend, frontend and tests; checked against a fake ACP agent and the OpenCode recordings, not yet by eye or with the real OpenCode through `delegate`). See "Phase 3 report". |
| 4 - Codex driver, Usage "Agent apps", diagnostics | **Done 2026-10-20** (backend, frontend and tests; checked against a fake Codex, all five recordings and the **real Codex 0.163.0 through `delegate`'s path**; not yet by eye). See "Phase 4 report". |
| 5 - MCPS-2 and proposals | **Done 2026-10-20.** RTE-6 and the four tools built for Claude Code, Codex and OpenCode. Real Claude Code and OpenCode pass; real Codex mounts and works but the model skips the tools about half the time (open item 1). See "Phase 5 report". |
| LRN-1..5 - lessons that are not about CLIs | **LRN-1..5 all built 2026-10-23** (LRN-4 and the agent-type half of LRN-5 with Phase 2-4). Not built earlier because no phase touched their files (Part VIII had them "in any gap"). See "LRN report". |
| Review 2026-10-23 | **Five bugs fixed** (OpenCode permission display, Codex sandbox on Windows, handover card and Stop, relative paths, a job that never started). Not committed yet. See "Review report". |

Qwen Code was dropped from the plan on 2026-10-23 (Erich: no longer a goal). The catalog is
Claude Code, Codex and OpenCode.

**Committed.** Phases 0-5 and LRN-1..5 are in commit `6b1e851`. The lines "Nothing is
committed" in the reports below were true when each report was written; they are history now.
The review fixes are not committed yet.

## Review report: 2026-10-23

A code review of the whole feature (backend, frontend, the plan against the code) found five
bugs. All five are fixed, with tests where a test can reach them.

| # | Bug | Fix | Test |
|---|---|---|---|
| 1 | **OpenCode permission panel could hide code.** `acp::tool_of` renamed any `execute` whose code contained `tools.poiesis.` to our tool with input `{}`, on the permission panel too. Code that called our tool *and* did something else would have been approved unseen. Also, an `execute` of kind `execute` with `code` and no `command` showed only its title. | The shortcut is for the timeline only (`tool_of`). A permission request uses the new `asked_tool_of`, which shows the call as sent. `execute` shows `command`, else `code`, else the title. An "other" tool's input is shown up to `SHOW_MAX` (was 1500 chars). | `acp::a_question_about_code_shows_the_code_even_when_it_calls_our_tool` |
| 2 | **Codex sandbox on Windows was never checked.** In an `auto` folder Codex gets `on-request` + `workspace-write` and runs commands without asking; only its sandbox keeps them in the folder. The probe recorded `windowsSandbox/readiness` but no code read it. | On Windows, an `auto` run asks `windowsSandbox/readiness` (no params, as recorded) between `initialized` and `thread/start`/`thread/resume`. Anything but `ready`, or an error, makes the run use the `confirm` policy (`untrusted`), so every step is asked about, and the report says *"Codex's sandbox is not set up on this computer, so it had to ask before each step."* `setupStart` is still never called. New `ExternalOutcome.sandbox_off`. `fake-codex.mjs` answers readiness (`notConfigured` when the folder holds `sandbox-not-ready`). | `codex::only_a_ready_answer_counts_as_a_ready_sandbox`, `e2e::codex_without_a_ready_sandbox_an_auto_folder_asks_about_every_step` (Windows only) |
| 3 | **The handover card ignored Stop.** `subagents::prepare_engine` waited on `rx.await` with no Stop check, so a stopped run waited for a click. | The wait also watches the parent run's cancel flag. On Stop the card is withdrawn (`PermissionGone`), the lead reads "The user stopped this run.", and no override is recorded (Stop is not "do it yourself"). | None: it needs a full `ToolContext`. On the manual list. |
| 4 | **Relative paths counted as outside the folder.** `path_within_root` read a relative path from Poiesis's own working folder, so `src/a.rs` or `.` was "outside" and asked about. The "create or edit" word had the same mistake. | `approve::outside` joins the path to the agent's folder first (an absolute path is unchanged; `..` out of the folder is still outside). | `approve::a_relative_path_is_read_from_the_agents_folder` |
| 5 | **A job that never started left no trace.** `run::fail` (for example `launch_for` failed) wrote nothing to the child's conversation. | New `run::keep_transcript`, used by both paths: the task and "X did not start: why" are kept. Usage is still written only for jobs that ran. | `e2e::a_job_that_never_started_keeps_the_task_and_why` |

**Tests, as run at the end.** `cargo test --lib`: **1062 passed, 0 failed, 14 ignored** (5 new
tests in this review). `cargo clippy --lib --tests`: no warning in the lines this review
changed (the one it raised, `unwrap_or_default` in `prepare_engine`, is fixed). No frontend file
changed; `npx tsc --noEmit` clean and `npx vitest run` 946 passed, 1 failed (the known
`icons.test.ts`) before the fixes.

**Still open from the review (not changed, on purpose):**
- **Resume is never used.** `Request.resume` is `None` in the foreground and the background, so
  the resume code in all three drivers (retry on "No conversation found", `session/load`,
  `thread/resume`, "started fresh") runs only in tests. Decide: wire it (a second `/delegate` to
  the same app in the same chat continues its session) or remove it.
- `Capabilities.resume`, `.interrupt` and `.approvals` are set but never read.
- `process::tests::the_child_env_has_no_poiesis_vars_and_no_provider_keys` sets process-wide env
  vars while other tests run in parallel; it can make other tests flaky.
- Not checked: what a real Codex answers to `windowsSandbox/readiness` on a computer where the
  sandbox is not set up (only `ready` was recorded). Any other answer is treated as not ready,
  which is the safe side.

**Manual pass (add to the list for Erich):** open a handover card (suggest mode), press Stop,
and check that the card goes away and the lead says it was stopped.

## LRN report: 2026-10-23

The five lessons from OpenMausBot that are not about CLIs. They were not built with Phases 0-5
because Part VIII said "any gap" and no phase opened the files they live in. Nothing is committed.

| Item | State | Where |
|---|---|---|
| LRN-1 stop on evidence | **Built** for the goal loop, the only place Poiesis re-sends turns on its own | `src/lib/goal.ts`, `store.ts::roundEvidence`, `RunBar.tsx` |
| LRN-2 never redo what left the machine | **Built** as a refusal on resume | `agent/log.rs::outward_done`, `commands/agent.rs::resume_run_cmd` |
| LRN-3 clean child environments | **Built**; the stdio MCP servers were the only gap | `engines/process.rs::withheld_from_children`, `mcp/client.rs` |
| LRN-4 escaped imitations in reports | **Built**; engine children were done in Phase 2, ordinary children now too | `agent/subagents.rs::escape_frame` |
| LRN-5 capability truth | **Built**: agent types and agent apps (Phases 2-4), and now the toolsets | `agent/toolsets.rs::ready`, `agent/run.rs`, `routes/Tools.tsx` |

**LRN-1.** Two guards, read from the round's own step records and never from model text. *No
progress*: two continuations in a row in which no tool call finished without an error (round 1 is
not a continuation, and the ambient recall step that comes with a turn is not counted). *Same
wall*: the same error (case and spacing ignored) in three different rounds; three of it in one
round is one. Either ends the goal as `stalled` with the reason as a note in the chat
(*"I stopped after three identical errors: …"* / *"I stopped because the last two rounds got
nowhere: none of my tool calls worked."*), and the Run bar reads `stalled` with the reason as
its title. A goal that the check finds met is still met. The guards are checked after the check, so
a stalled goal never overrides a finished one.
Only the goal loop was found to continue on its own. Background children are one run each and are
never re-queued (their queue is in memory), a plan waits for the person's yes, and the in-run
retries are bounded by steps. If another self-continuing loop is added, it needs `weighRound`.
The plan's "feeds the outcome labels in `DUAL_PROCESS_PLAN.md` (CTL)" is not wired: those labels
do not exist yet. `Goal.stalled` and the status are what the controller would read.

**LRN-2.** Nothing re-runs by itself after a crash: background children are marked `stopped`, and
the only restart path is the person pressing **Resume** (`HRN-UI-5`), which replays the session
log. That path now looks at the run's rows first. A tool call counts when its result row came back
without an error and it is `send_mail`, `reply_mail`, a `git push` through `run_command`, a
connector tool the gate would hold as a send (`CallKind::Send`: post, send, pay, publish…), or a
Composio batch containing one. If one is found, Resume does not start and the reply says *"I was
cut off after I had already sent the email, so I didn't start over."* A send that errored, or that
never got a result row, does not block a resume. Limits, honestly: a send that went out but was
cut off *before its result was written* looks unfinished, so Resume would offer it again, and the
classification of a connector tool is by its name (the gate's own rule), so a send with an
innocent name is missed. The plan's "at most once" half has nothing to do: there is no automatic
re-run.

**LRN-3.** The code sandbox (`Minimal` env) and project tasks (`InheritWithoutSecrets`) were
already scrubbed. The stdio MCP servers inherited everything. They now lose `POIESIS_*` and the
model providers' keys (`PROVIDER_KEYS`, the same list the agent apps use). Other variables, for
example a `GITHUB_TOKEN` the person exported on purpose for a server that reads it, are kept,
because a connector has no field to hand a server an env var in, and removing every token would
break servers that work today. If that is wanted, the connector needs an env field first.

**LRN-4.** A child's words are put in the frame `Agent N (name) — headline`. `escape_frame`
defangs the `<<<POIESIS_` fences and puts `> ` in front of any line that opens like a frame line
(`Agent 2 (` or `It made:`), for both the foreground report and `collect_agents`.

**LRN-5.** `agent_types` lists only agent apps that can take a job right now (Phases 2-4). The
audit of the toolsets found two that were offered whenever their switch was on, with nothing
behind them: **Mail** (no account) and **Image generation** (no local engine and model, and no
cloud image backend). Every other toolset works with just its switch (their folder and site
permissions are asked at the moment of use), so only these two were changed.
`Toolset::ready(db)` is read for every run, in `run.rs` right after the persona and delegation
narrowing: Mail needs one account that is not switched off, Image generation asks the same
question its own tool asks (`media::resolve_backend` for images, so a cloud key counts). A toolset
that is not ready is left off the tool menu; its switch in Settings is not touched, and the tool
is back on the next run after the setup. Settings → Tools shows the reason under a switched-on
tool that is not ready (`ToolsetInfo.needs`: *"I can't use this yet. Add a mail account in
Settings → Mail."*). Not changed: the system prompt, which does not name these tools, and the
model is not told why a tool is missing; with no `generate_image` it simply cannot make one.
No test for the image half of `ready` in isolation: it depends on the machine's stored keys, so
only the Mail half has a unit test.

**Tests, as run at the end.** `cargo test --lib`: **1048 passed, 0 failed, 14 ignored**. New: 4 in
`agent::log` (LRN-T2), 1 in `engines::process`, 1 in `agent::subagents` (LRN-T4), 1 in
`agent::run` (LRN-T5, mail tools only with an account). `npx vitest run`: **945 passed, 1 failed**
(the known `icons.test.ts`); new: 6 in `goal.test.ts` (LRN-T1), 1 in `RunBar.goal.test.tsx`, 2 in
`routes/Tools.needs.test.tsx`. `npx tsc --noEmit`: clean. `cargo clippy --lib --tests`: checked
after the LRN-1..4 work only (nothing in the new code); not re-run after LRN-5. Not run by hand in
the app: a real goal that stalls, a Resume after a real send, and Settings → Tools with no mail
account.

## Phase 5 report: Done 2026-10-20

Nothing is committed. Every part of Phase 5 is built; what could not be proved is listed
under "Open items", not hidden.

### Tests, as run at the end

- `cargo test --lib`: **1041 passed, 0 failed, 14 ignored (the ignored ones need a real app, a model or a voice file)**.
- `npx vitest run`: **935 passed, 1 failed (936 tests in 89 files)**. The one failure, `icons.test.ts` (`WindowControls.tsx`
  holds a raw icon), was failing before this phase and is not touched by it.
- `npx tsc --noEmit`: clean.
- `cargo clippy --lib --tests`: no warning in any file this phase touched (one older one at `commands/memory.rs:570` is not mine).
- `cargo test --test copy_lint`: fails only on the known word "chunk" in `playbackQueue.test.ts:45`; no word from this phase is flagged.
- Real binaries, ignored tests, all run by hand today on this computer:
  - `real_claude_uses_our_four_tools_without_a_permission_question` (Claude Code 2.1.304):
    **passes** in `default` and in `plan` (read-only) mode. All four tools are called, no
    permission panel appears, the steps read recalled / showed / asked you / suggested
    remembering, and the report counts them. Claude Code loads the tools through its own
    `ToolSearch` first (the schemas are deferred), which shows as one extra step.
  - `real_opencode_uses_our_four_tools_without_a_permission_question` (OpenCode 2.1.0, a free
    model): **passes**. See "What was learned".
  - `real_codex_uses_our_four_tools_without_a_permission_question` (Codex 0.163.0): **did not
    pass, and cannot be re-run today.** See "Open items".

### What was learned from the real apps

- **Claude Code**: the tools work through the existing endpoint. `--permission-prompt-tool` is
  still asked about them, so `approve` allows `mcp__poiesis__*` itself when they are mounted.
- **OpenCode (ACP)**: `session/new { mcpServers: [{type: "http", name: "poiesis", url,
  headers: []}] }` makes it connect at once (`initialize`, `tools/list`). It runs a call
  with **no** `session/request_permission`. It does not call the tool directly: it shows
  its own `execute` tool whose `rawInput.code` reads `tools.poiesis.recall({...})`.
  `acp::tool_of` reads that code and the step becomes "recalled" (a name outside our four
  stays `execute`). The model once tried `tools.mcp__poiesis__recall` first, got "Unknown
  tool" and corrected itself. Mounted only when `initialize` says `mcpCapabilities.http`.
- **Codex**: `thread/start` (and `thread/resume`) `config: {mcp_servers: {poiesis: {url,
  tool_timeout_sec}}}` mounts the server (`mcpServer/startupStatus/updated` goes
  `starting`, `ready`). Each call shows as an `mcpToolCall` item `{server, tool, arguments,
  status, result}`. Before each call Codex sends **`mcpServer/elicitation/request`** with
  `_meta.codex_approval_kind: "mcp_tool_call"`; answering `{action: "accept", content:
  null}` lets it run (in `untrusted` + `workspace-write`). The default tool timeout is 60 s,
  so `tool_timeout_sec` is set to 660 (a question waits up to 10 minutes); the key was
  proven to be honoured (a 3 s limit failed a 6 s call with "timed out awaiting tools/call
  after 3s"). Only our server's elicitation is answered; any other server's still gets
  `-32601` as before. Codex 0.163 always defers MCP tools behind its own tool search.

### Added in this session (on top of "What was built" below)

- `engines/tools.rs`: `serve()`, one place that answers a call of ours; Claude, Codex and
  ACP all use it.
- `engines/codex.rs`: `capabilities().mcp_tools = true`; `thread_params(.., mcp_url)`; the
  token and `calls` arm in the loop (a tool waiting for the person is not a stall);
  `elicitation_reply`; `mcpToolCall` items as steps (`is_ours`, `our_step`, not counted as
  self-allowed). 3 new unit tests.
- `engines/acp.rs`: `mcp_tools = true`; `mcp_servers(url, http)`; `Run.mcp_http`; the same
  token and `calls` arm; `our_tool_in` / `tool_of` for the `execute` code. 2 new unit tests.
- `fixtures/engines/fake-codex.mjs` and `fake-acp.mjs`: scenario `memory`, shaped like the
  recordings above. `engines/e2e.rs`: 3 new tests per app (switch off, switch on, nobody
  there) for Codex and ACP, plus the three real-binary tests.
- The switch *Let it use my memory* now shows for all three apps (it follows
  `memory_ready`). An ACP agent added by hand is still not `delegate_ready`.

### Deviations from the plan

1. `present` offers only `table` and `progress`, and `remember` raises a proposal. The old
   seven block kinds and the session-state patch no longer exist in Poiesis as they did.
2. The wording of a rule proposal comes from the **local** model only, with a template
   fallback for "turn that rule off". A call the user did not ask for never goes to the cloud.
3. `engine-memory` is its own proposal target (not `lesson`), so Self can say where it came
   from. Accepting it writes a lesson.

### Open items

1. **Codex does not use the tools every time.** In my real runs the server mounted, was
   `ready`, and listed the tools, and when the model called them everything worked (the
   `elicitation` was accepted, the result came back, the timeout key held). But in roughly
   half of the tiny runs (the model used them in about 16 of 29 runs with our real tool
   list) it answered "I can't access the poiesis MCP tools" without trying. A tiny one-tool
   server worked both times it was tried (2 of 2), and one run each with `annotations`
   removed, and with the `anyOf` in `ask_user.options` and the empty `items: {}` in
   `present.rows` simplified, also worked. That is far too few runs to say our schema is
   the cause, but it is the first thing to test. Waiting for `ready` before `turn/start`
   did not change it, and a sentence telling the model to search for "poiesis" did not
   either. Cause **not found**. Next step, once there is quota again: run the real Codex
   test several times with `annotations` removed, then with `anyOf` and `items: {}`
   simplified, and compare. Nothing breaks when the model does not use the tools: the job
   runs as before.
2. **I used up the Codex free quota on this account** while checking this (about 40 small
   turns; Codex now says to try again on Nov 19th). That was too many turns for one check.
   `real_codex_uses_our_four_tools_...` needs a working sign-in to run.
3. `engines_undo_handover_cmd` (the *This shouldn't have gone to ...* trigger for RTE-6) has
   no automated test: it needs Tauri's managed state. The logic it calls is covered by the 9
   `engines::learn` tests. It is on the manual list.
4. No test for the External agents tab badge in `SettingsHub` (it renders every route).
5. A hand-added ACP agent that cannot reach an HTTP server would still be told by the fixed
   words that it has the tools. Only OpenCode is delegate-ready today, and it can.

### Manual pass for Erich

1. Settings, External agents: open Claude Code (and OpenCode, and Codex). Switch on **Let it
   use my memory**. Read the sentence under it.
2. Hand a small job to Claude Code: "Ask me which package manager to use, show a progress
   bar, then suggest remembering my answer." Check: the question card says "Claude Code asks
   you", the progress bar sits on the job's Fleet row, the report says what it used.
3. Under Self, find the suggested memory. Read the full text. Say no once and yes once.
4. Say no to three handovers of the same rule (*Do it yourself*), within a few minutes.
   Check: exactly one suggestion appears at the top of *When I use it* and a dot on the
   External agents tab. It changes nothing until you click.
5. Press *This shouldn't have gone to ...* three times on one rule. Same check (this is the
   path with no automated test).
6. Press Stop while a question from the agent is open. The card goes away.
7. Run the same job from a scheduled task. The agent is told nobody is there and decides.

### What was built

**RTE-6, rules learn by proposal** (`engines/learn.rs`, new):
- Schema **v35**: table `engine_signals` (`override` | `asked` | `checked`), helpers in
  `db/engines.rs` (`add_engine_signal`, `engine_signals_since`, `last_engine_signal_at`,
  `latest_engine_proposal`, test-only `add_engine_signal_at`). `delete_engine` clears them.
- Trigger 1: `record_override` after a handover card's *Do it yourself* (in
  `subagents::prepare_engine`) and after *This shouldn't have gone to ...*
  (`engines_undo_handover_cmd`, now async). Three in 30 days, counted only after the
  last proposal about that rule, and none while one is pending. Proposal `slug` = rule id.
- Trigger 2: `record_asked` on a `/delegate <agent>` (`Via::Asked`). Third in 30 days
  needs wording from a model, else nothing is raised. A model "none" writes a `checked`
  signal, so it is asked again only after three new ones. Slug `use:<engine_id>`.
- Wording: `follow_up` asks the **local** model only (8 s limit), because a call the user
  did not ask for never goes to the cloud (same rule as `ToolContext::local_endpoint`).
  With no local model: an override proposal falls back to "turn that rule off"; an asked
  pattern raises nothing. This is a **deviation** from "drafted by the lead's model".
- Target `engine-rule`; `proposed_text` is JSON `{engine_id, action: edit|add|off, rule_id?,
  kind?, text?}`. `commands/memory.rs::resolve_change_proposal_cmd` has the `engine-rule`
  branch (calls `learn::apply`, rule gets `source = 'proposed'`, errors in words if the rule
  is gone or the list is full). Model text is rejected if `untrusted::scan` risk >= 2.
- Tests: 9 in `engines::learn` pass, including RTE-T-4 (three overrides -> exactly one
  proposal, a fourth while pending -> none, nothing applied until accepted).

**MCPS-2, Poiesis tools for agent apps:**
- `engines/tools.rs` (new): `recall`, `present`, `ask_user`, `remember`; `specs()`,
  `describe()`, `short_name()`, `Tools` (counts calls, `summary()` for the report).
  - `recall`: `search_text` / `window_text` (new pub fns in `agent/recall.rs`, the old
    tool functions now call them). Result wrapped with `untrusted::wrap`. Logged.
  - `present`: **only `table` and `progress`** (the other block kinds send chat turns when
    clicked, so they are not offered). Max 6 per job, same title updates in place, kept
    with the child conversation via `add_block(child, None, ...)`.
  - `ask_user`: 0 or 2-4 options, max 4 per job, headless -> "Nobody is here to answer",
    10-minute timeout, Stop/hang-up -> new `AgentEvent::QuestionGone { id }`.
  - `remember`: raises a `change_proposals` row, target **`engine-memory`** (new), slug
    `<engine>-<name>-<4 hex>`, 8-500 chars, refused if scan risk >= 2, max 3 per job,
    emits `AgentEvent::Proposal`. Accepting runs the same branch as `lesson`
    (`commands/memory.rs`). **Deviation:** the plan's `present` (seven block kinds) and
    `remember` (session-state patch) no longer exist in Poiesis as they did, so these two
    are the rebuilt equivalents above.
- `agent/toolrpc.rs`: `McpKit { approve, memory }` on the token; `arm_mcp(gate, run_id, kit)`;
  `tools/list` and `tools/call` follow the kit (a token without memory cannot call them,
  a memory-only token cannot call `approve`).
- `engines/driver.rs`: `Job.tools: Option<&Tools>`, `memory_ready(kind, id)` (= driver
  `mcp_tools`). `engines/mod.rs` `EngineInfo.memory_ready`.
- `engines/run.rs`: builds `Tools` when `row.memory_tools && driver.mcp_tools`;
  `system_append(headless, memory_tools)` adds `tools::SYSTEM_NOTE` only then; the report
  gets "It also searched your memory 1 time, asked you 1 question, ..." (`Tools::summary`).
- `engines/claude.rs`: arms the token with `memory: j.tools.is_some()`; `answer(job, call,
  stop)` serves our tools and **auto-allows** `mcp__poiesis__*` in `approve` when mounted
  (no panel question). `approve::describe_tool` maps those names to the timeline words
  (recalled / showed / asked you / suggested remembering). `approve::until` is `pub(super)`.
- `fixtures/engines/fake-claude.mjs`: scenario `memory`. `engines/e2e.rs`: 3 new tests
  (switch off -> only `approve` listed and calls refused; switch on -> all four work, no
  permission panel, report counts them; headless -> no question). **`cargo test --lib
  engines::e2e`: 51 passed, 0 failed, 2 ignored.** `engines::tools`: 11 unit tests pass.

**Frontend:**
- `api.ts`: `EngineInfo.memory_ready`, `AgentEvent` `question_gone`. `types.ts`:
  `PendingQuestion.agent`, `SubRun.blocks`.
- `store.ts`: child `question` -> `pendingQuestion` in the lead's message (a second one goes
  to new `queuedQuestions`), `question_gone`, `run_ended` drops that job's questions, `block`
  / `block_update` into `SubRun.blocks`, a child `proposal` is added to the lead's
  `proposalIds` and refreshes proposals. Helpers `dropQuestion`, `dropQuestionsOf`;
  `answerQuestion` brings the next queued question up.
- `QuestionCard.tsx`: header "<agent> asks you", free-answer box open when no options.
- `FleetCard.tsx` + `.fleet-blocks` css: renders `run.blocks` with `BlockRenderer`.
- `routes/Engines.tsx` Permissions group: **Let it use my memory** switch, only when
  `memory_ready`; copy says what it finds goes to its model and each suggestion is read first.
- `ProposalCard.tsx`: `EngineMemoryProposal` (shows the full text, Keep it / Not now),
  `EngineRuleProposal` (Add it / Change it / Turn it off / Not now, then `loadEngines`),
  exported `parseRuleChange`. `RuleList.tsx` shows pending `engine-rule` cards at the top of
  *When I use it*. `SettingsHub.tsx`: External agents tab badge for a pending `engine-rule`
  (the Self badge ignores it). `SelfPanel.tsx`: `engine-memory` listed with the lessons
  ("Keep it").
- New Vitest: `components/Conversation/ProposalCard.engine.test.tsx` (9: the memory card, the
  rule card with its three verbs, the refusal shown in words, an unreadable payload shows
  nothing, the `QuestionCard` header for an agent and the open answer box), a pending rule
  proposal in `RuleList.test.tsx`, the switch in `Engines.test.tsx`, three in
  `store.subruns.test.ts`. Fixtures got `memory_ready`.

### Decisions to keep
- The memory switch is shown only where the way to mount the tools has been seen working.
- `remember` never writes a memory; it only raises a proposal the user reads in full.
- Nothing about rules is applied without the click; the 30-day window and the "after the
  last proposal" floor are in `engines/learn.rs`.

## Phase 4 report: Done 2026-10-20

Nothing is committed. Built in one session on Erich's "continue with phase 4".

### Recorded first (the plan asked for it)

`fixtures/engines/codex-0.163.0-on-request.jsonl` and `codex-0.163.0-read-only.jsonl`
(Codex 0.163.0, signed in with ChatGPT, tiny prompts in scratch folders). What they
showed, and what the CLI-6 table now rests on:
- **`on-request` + `workspace-write` asked for nothing at all.** A file add and two
  commands ran inside Codex's own sandbox with no approval request. So in an `auto`
  folder Codex asks us only if it wants out of its sandbox; the steps are counted in
  the report footer (*Codex's own settings allowed N steps without asking*).
- **`on-request` + `read-only` asked about the file add** (`fileChange` approval,
  no path or diff as before) and let commands run, unable to write (`mkdir` ended
  `failed`, access denied). Our Port refuses the file edit at read-only anyway.
- Still not recorded: Codex with an API key (`login status` text), `tool/requestUserInput`
  (needs the experimental API, which we leave off).

### Built

Backend (`src-tauri/`):
- **CLI-11, `engines/codex.rs`** rewritten as a full driver over the same shape as
  `acp.rs`. `initialize` (`experimentalApi: false`), `initialized`, `thread/start` or
  `thread/resume` (with `excludeTurns: true`; a thread that is gone starts fresh once
  and says so), `turn/start` with the fixed words and the fenced task as two input
  items. `approvalsReviewer` is always `"user"`. **Trust → Codex** (`policy_for`):
  read-only = `on-request` + `read-only`, confirm = `untrusted` + `workspace-write`,
  auto = `on-request` + `workspace-write`; never `never`, never `danger-full-access`.
  Try it stays `untrusted` + `read-only` with every approval declined.
- **Approvals** go to the shared `Port`: commands as the command a person would type
  (the shell wrapper is stripped using `commandActions`), file changes joined on
  `itemId` with the `fileChange` item that started before (a new file shows as `Write`
  with its words, anything else as the new `Patch` tool with the unified diff, delete as
  `Delete`). Answered `accept` / `decline` / `cancel` only, never `acceptForSession` or an
  amendment. Decline lets Codex carry on, so the report lists what was refused and does
  not say it stops its whole job. A request for a whole folder (`grantRoot`) says so on
  the panel. Every other server request, `account/chatgptAuthTokens/refresh` included,
  gets `-32601` and is counted as an unknown event.
- **Events**: streamed `agentMessage` deltas (a new paragraph after each step), command and
  file steps with start / done / error (`declined` and `failed` are error steps), the report
  is the `final_answer` message, usage from `thread/tokenUsage/updated`, who paid from
  `account/updated`. Unknown notification families, item types and server requests are
  counted, never fatal (CLI-T-7).
- **Steering**: a message typed while it works goes in as `turn/steer` into the same turn
  (`steer_mid_turn` true). **Stop**: pending questions are answered `cancel`, `turn/interrupt`
  is sent, three seconds of grace, then the tree is killed.
- **Watchdog**: 120 s with no message, no question open and no Stop ends the run with
  *It went quiet for two minutes* (after one best-effort interrupt). A question waiting on
  a person never counts as quiet. A test hook (`driver::STALL_FOR_TESTS`) shortens it.
- `driver::delegate_ready` is true for Codex. It gets the same default rules and `suggest`
  mode as Claude Code.
- **CLI-UI-7 backend**: schema **v34** (`run_usage.paid_by`, `notional_usd`, `duration_ms`).
  Every agent job writes a row (`record_agent_app_usage`, provenance `agent-app`, the
  app's name as the model), tokens reported or not. `usage_summary` keeps those rows out of
  the totals, by-day, by-model and by-chat and returns them as `agent_apps`.

Frontend (`src/`):
- **CLI-UI-7**: Settings → Usage has an **Agent apps** group (jobs, time, tokens, who paid,
  and a grey *≈ $x at list prices* only where the app reports one; Codex never does). The
  tooltip says why it is not charged (*On your plan this isn't charged per job* / billed to
  your key / the app doesn't report a price). Shown even when the range holds only agent jobs.
- **CLI-UI-8**: the engine's *Recent jobs* block (runs, failures, messages I didn't
  understand, last problem, what it printed) already existed from Phase 2. Added the early
  warning sentence when the count is above zero (*If this number keeps rising after an
  update, Codex now talks to me differently...*). Tested / newer / older is on the status line.

### Test state at the end of Phase 4

- **Rust: `cargo test --lib` 1006 passed, 0 failed, 11 ignored** (the new ignored one is
  `real_codex_does_a_small_job_under_our_questions`). New: 14 unit tests in `codex.rs`
  replaying all five Codex recordings plus the two new ones; 17 end-to-end tests against
  `fixtures/engines/fake-codex.mjs` (allow, no, allow for this run, read-only and headless,
  edit diff, own-sandbox steps counted, trust → sandbox/policy and nothing on argv, fence,
  Stop kills the tree, interrupt withdraws an open question, steer, resume or start fresh,
  not signed in, model sent and listed, untested version, watchdog, failed run); a usage test
  in `db`. `cargo clippy --lib --tests`: nothing in files this phase touched.
- **The real Codex 0.163.0 through the same path `delegate` uses** (ignored test, scratch
  folder, ChatGPT sign-in): a file add and two commands, each shown on the panel as
  **Codex wants to ...** with the right diff and command, all allowed, completed in 31 s,
  nothing left running.
- **Frontend: `tsc` clean. Vitest 921 passed, 1 failed** (the known `icons.test.ts`).
  New: `routes/Usage.agentapps.test.tsx` (5), one in `RuleList.test.tsx`.
  `copy_lint` still fails only on `lib/voice/playbackQueue.test.ts:45`.

### Deviations (this part wins)

- **`tool/requestUserInput` and `item/permissions/requestApproval` are refused** (`-32601`),
  not turned into a question card. Both belong to the experimental API, which we do not
  switch on, and neither was ever seen. If a future Codex sends them, the unknown counter
  shows it.
- **The user's own Codex MCP servers (`config.toml`) still load** with the thread, unlike
  Claude Code where `--strict-mcp-config` keeps them out. Their tool calls would arrive as
  item types we don't label, so they are counted as unknown events rather than shown as
  steps. Revisit with Phase 5.
- **The 120 s watchdog is the plan's number.** A very long silent reasoning stretch could
  trip it; the recordings never went quiet for more than ~10 s.
- A steer that arrives just as the turn ends is dropped quietly.
- Agent-app jobs are not part of the per-chat usage row, only of Agent apps.

### Open items for Erich (Phase 4)

1. Settings → External agents → Codex: the line *I can't hand work to Codex yet* should
   be gone. Switch it on, and in a chat with a code folder ask for a small change; expect
   *Codex wants to create ...* / *run a command* on the panel and a report starting
   *Seen by Poiesis*.
2. `/delegate codex ...` in a folder set to **auto**: Codex will ask for nothing (its own
   sandbox), so the footer should say its own settings allowed N steps. Check you are fine
   with that, or set Codex's trust ceiling to *confirm* in its settings.
3. Say no to a command and check Codex carries on and the report says what was refused.
4. Settings → Usage: after two or three agent jobs, the **Agent apps** group; check the
   grey figure and tooltip read right, and that the cloud totals did not move.
5. Press Stop with a question open, and with Codex mid-command.
6. Codex with an API key is still unrecorded (`login status` text); the Billing switch for
   Codex stays off until that is recorded.
7. Say if the copy sounds wrong anywhere.

Then wait for Erich's go on Phase 5 (MCPS-2 memory tools, RTE-6 rule proposals, RTE-T-4).

## Phase 3 report: Done 2026-10-20

Nothing is committed. The first session that built this ran out of usage before
it wrote this report; a second session checked every item below against the
code, ran the whole suite, and added the one missing test (CLI-T-7).

### Built

Backend (`src-tauri/`):
- **CLI-8, `engines/acp.rs`.** Our own JSON-RPC over `engines/process.rs`'s
  `Wire` (no `agent-client-protocol` crate: not needed for ~1000 lines with the
  tests). `initialize` with `fs` off, never `authenticate`, `session/new` or
  `session/load` (only when `loadSession` is true in the reply, else a new
  session and `resume_failed`), `session/set_config_option` for the chosen model
  (a model the agent doesn't offer is a plain failure), `session/prompt`. Updates
  before our prompt are dropped. A tool step is labelled from its first
  `in_progress` update. `session/request_permission` goes to `Port` and is
  answered by option **kind** (`allow_once` / `reject_once`, never `allow_always`).
  A reject that ends the turn is reported as `declined`, not an error, with
  `ExternalOutcome.denials`. Stop is `session/cancel`, then the tree is killed.
  What the user types mid-turn is sent as the next prompt of the same session
  (`steer_mid_turn = false`). No stall watchdog. `list_models` reads the new
  session's `configOptions`. Try it keeps its path (`job: None`, every request
  refused).
- **Making the agent ask (Part X, "Corrections").** `shared/engines.json` entry
  for OpenCode has `ask_everything_env` (inline `OPENCODE_CONFIG_CONTENT`,
  edit and bash `ask`), `auto_edits_env` (edit `allow`, bash `ask`, used for
  `Trust::Auto`), `paid_by: own`, and `strip_env_prefixes` (provider keys are
  removed from the child env). `driver::delegate_ready` is true for an ACP agent
  only when the catalog records `ask_everything_env`, so an ACP agent added by
  hand is configurable and never offered. `PaidBy` gained `Own` and `Free`
  (`-free` model ids). Read-only is **not** OpenCode's `plan` mode (the
  recording `opencode-2.1.0-plan-mode.jsonl` showed it writing outside the
  folder); it is our own refusal of every edit and command.
- **CLI-UI-4 backend.** `agent_types` offers a persona that runs on an agent app
  only while that app can take a job (`rules::offered_for_persona`); it runs
  through `run_external` with `Via::Persona`, keeps the persona's name on the
  Fleet card, and the persona's system prompt goes in front of the task inside
  the fenced block. The `because` is `persona` (no rule, no handover card).
- **CLI-UI-6 backend.** `delegate_to` on the turn options (`run.rs`),
  `RouteCtx.delegate_to`, `rules::cite(asked)`. In that turn every `delegate`
  task must go to the named agent (others are refused with *The user asked for X
  in this message, so this task goes there and not to Y.*), `because` is
  `asked`, mode and rules are skipped, guards and `engines_off` still apply.
  `list_delegate_targets_cmd` (`commands/subagents.rs`, `agent::subagents::
  delegate_targets`) returns General, spawnable personas and every found agent
  with `disabled_reason`. Engine children run in the background with the panel
  still reachable (a headless parent keeps the old refusal).
- **CLI-T-7.** Nothing gates on the probe's `tested` state. New
  `acp_an_untested_version_still_runs_and_what_is_new_is_counted` (version 9.9.9,
  a never-seen update and method: the run completes, `diag` counts both); the
  fake agent got a `novel` scenario. The 2 MiB line is `process::tests::
  a_line_over_the_cap_is_dropped_and_counted_not_buffered`.

Frontend (`src/`):
- `shared/commands.json`: `/delegate` has `args.lead = "agents"`, summary *Hand
  this to an agent and keep talking*; new `/external on|off` in *modes* with
  `needs: ["conv", "engines"]`. `lib/commands.ts`: `ArgSource "agents"`,
  `parseCommandLine` splits a leading agent slug, `skillSend` returns
  `delegateTo`. `store.ts` / `api.ts` / `types.ts`: `delegateTo` on the turn
  options, `listDelegateTargets`, `setEnginesOff`.
- `Composer/CommandMenu.tsx`, `useCommandInput.ts`, `Composer.tsx`: the agent
  popover after `/delegate `, disabled agents shown with their reason (choosing
  one opens Settings → External agents on it), `ModeChips.tsx` chip **External
  agents off** (label opens `/external `, × turns them back on).
- `Personas/PersonaEditor.tsx`: **Runs on** select, model and tools hidden for an
  agent app, the sentence from CLI-UI-4, struck-through off agent.
- Fleet row: queued look (`waiting its turn`) until `run_started`, chips *you
  asked* / *its persona*, `own provider` / `free model` pay chips (`lib/engines.ts`).

### Test state at the end of Phase 3

- **Rust: `cargo test --lib` 979 passed, 0 failed, 10 ignored** (the 10th is
  `real_opencode_does_a_small_job_under_our_questions`; run it with `-- --ignored
  --nocapture` on a machine with OpenCode). The ACP end-to-end set is in
  `engines/e2e.rs` (`acp_*`, 15 tests against `fixtures/engines/fake-acp.mjs`, plus the ignored real one):
  allow once, no ends the job, allow for this run, read-only, edit diff, trust
  picks what it is made to ask, nothing on argv, fence, Stop kills the tree,
  polite cancel and withdrawn question, mid-turn message, session continued or
  started fresh, not signed in, model set, models listed, failed run. Plus 12
  unit tests in `acp.rs` over the OpenCode recordings. `cargo clippy --lib
  --tests`: nothing in the files this phase touched.
- **Frontend: `npx tsc --noEmit` clean. Vitest 915 passed, 1 failed**, the same
  `icons.test.ts` (WindowControls.tsx) as before. New: `lib/commands.engines.
  test.ts`, `Composer/Composer.delegate.test.tsx`, `Personas/PersonaEditor.
  runson.test.tsx`; additions in `engines.test.ts` and `FleetCard.engine.test.tsx`.
- `cargo test --test copy_lint` still fails on one line that is not ours:
  `lib/voice/playbackQueue.test.ts:45` ("chunk" in a test name). The
  `CommandMenu.tsx` hit that was ours is fixed.

### Deviations (this part wins)

- **No process pool (CLI-8).** One agent process per run, as Claude Code does.
  A follow-up `delegate` to the same child resumes the session (`session/load`)
  and pays the ~2.5 s handshake. `engines.idle_close_secs` has no consumer. Do it
  only if Erich finds the wait annoying.
- **`$AGENT` in `SKILL.md` is not used.** The bundled `delegate` skill is
  unchanged; the lead learns the user named an agent from a paragraph appended to
  the `delegate` tool's description (`delegate_spec`), and Rust enforces it.
- **No `plan` mode for read-only**, see above.
- **`steer_mid_turn` is false for ACP.** Words typed meanwhile wait for the end of
  the turn (the Fleet placeholder already says so).

### Open items for Erich (Phase 3)

1. In a chat with a folder, type `/delegate ` and look at the popover: General,
   your personas, Claude Code, OpenCode. Pick OpenCode, give it a small job
   ("create hello.txt with hi and run `mkdir x`"); expect an approval for the
   write and for the command, headed **OpenCode wants to ...**, and a report that
   starts *Seen by Poiesis*. Say no to the command and check the row says it
   stopped because you declined.
2. `/external off`: the chip **External agents off** appears and `/delegate
   claude-code ...` is refused; × brings it back. With no agent turned on,
   `/external` should be hidden.
3. Settings → Personas: pick **Runs on** → Claude Code on a persona that may be
   delegated to; hand it a task by name. Turn Claude Code off and check the select
   shows it struck through and the persona is no longer offered.
4. Start three agent jobs with `engines.max_parallel` = 1: the later ones should
   read *waiting its turn* and start when the first ends.
5. `CLI-T-8` additions: your global OpenCode config
   (`%USERPROFILE%\.config\opencode`) must not override our `ask`: run a job and
   confirm every edit and command still reaches the panel. Then
   `cargo test --lib real_opencode -- --ignored --nocapture`.
6. Say if the copy sounds wrong anywhere.

Phase 4 followed; see its report above.

## Phase 2 status and report

Nothing is committed. Phase 2 is finished; the lists below say what was built.

### Built and compiling (backend, `src-tauri/`)

- **Schema v33** (`db/schema.sql`, `db/mod.rs`): `engines.use_mode`,
  `engines.guards_json`, `engines.rules_seeded`; table `engine_rules`;
  `conversations.engines_off` (also on the `Conversation` struct, copied by
  fork); `subagent_runs.engine_id / because / paid_by` (on `SubagentRun`, plus
  `engine_session` read from the child conversation). DB helpers: rule CRUD,
  `set_subagent_engine`, `set_subagent_paid_by`, `set_conversation_engine_session`,
  `conversation_engine_session`, `set_conversation_engines_off`,
  `count_engine_runs`. **Difference from RTE-4:** `because` lives on
  `subagent_runs`, not `conversations.engine_because`.
- `engines/rules.rs` (RTE-1..5, RTE-7 backend): `UseMode`, `Guards` (resolve with
  kind defaults, validate), `default_rules`, `ensure_seeded` (once; ACP rows
  with the migration default `suggest` become `on_request`), `reset`, `labels`
  (U1.. / A1.., by position, a switched-off rule keeps its number),
  `render_block` (the exact text the lead reads), `Place`, `RouteCtx`, `Block`
  (every refusal in words), `blocked`, `offered(db, ctx)`, `cite` (needs a
  `because`; refuses unknown, `A*` and switched-off labels; says when the card is
  needed), `within_limit`, `candidates`/`preview_prompt`/`read_preview`
  (Try a request), `is_code_project` (project file, or `.git` up to 6 folders up).
  `delegate_ready()` in `driver.rs` is true for Claude Code only: Codex and
  OpenCode stay configurable but are never offered to the lead until Phases 3/4.
- `engines/approve.rs` (CLI-6): `Port` puts an agent's question on the panel
  (capability `engine`, fields `engine`, `tool`, `argv`, `diff`), fails closed
  (read-only trust, headless, 10 min timeout, withdrawn), remembers
  "Allow for this run" per tool + exact command or path, counts asked/allowed/
  denied; `describe_tool` (Bash, PowerShell, Edit, Write, Read, Grep, Task...).
  New `AgentEvent::PermissionGone { id }` withdraws a question; `SubSpawned`
  gained `engine: Option<EngineBadge>`.
- `permissions/mod.rs`: `PermissionRequest` gained `engine`, `tool`, `rule`
  (`RuleRef`); constructors `engine_ask` and `handover` (capability `handover`:
  Once = hand over, Deny = do it yourself, Forever = Always for this).
- **MCPS-1** in `agent/toolrpc.rs`: `/<token>/mcp` beside `/<token>/tool`; token
  kinds are separate (an agent's token cannot call tools; a script token gets a
  404 on `/mcp`; a dead token gets 404 before any parsing). `server/discover`
  answered -32601, `initialize` echoes the client's protocol version,
  notifications get 202, `tools/list` offers `approve`, `tools/call` forwards to
  the run and watches the socket (hang-up sets `McpCall.gone`). GET/DELETE give
  405. `arm_mcp`, `mcp_url`, and a test-only `serve_for_tests()`.
- `engines/claude.rs` rewritten (CLI-5, CLI-7): permission mode from trust
  (`plan` / `default` / `acceptEdits`, never bypass), `--mcp-config` +
  `--strict-mcp-config` + `--permission-prompt-tool mcp__poiesis__approve` +
  `--append-system-prompt-file` (files in a private scratch folder, deleted on
  end), task over stdin, `--resume` with one fresh retry, steer forwarded,
  interrupt on Stop, questions answered concurrently (`select!` with a
  `FuturesUnordered`), event mapping to `Ev` (text, thinking, steps, session),
  self-allowed counter from `tool_result_meta ... source:"config"`. Try it keeps
  its narrow path (`job: None`).
- `engines/run.rs` (CLI-4/5/9/10): `run_external` (slot wait with `queued`,
  effective trust = min(folder, ceiling), fenced task with `defang` (LRN-4),
  report with "Seen by Poiesis" first, child transcript written, session and
  paid_by stored, `RunStarted`/`RunEnded`, diagnostics). `engines.max_parallel`
  and `engines.timeout_secs` now have a consumer. `engines/diag.rs`: last 20 runs
  per agent in memory, `Summary` in `EngineInfo.diagnostics`.
- `agent/subagents.rs`: `TaskSpec.because`, `AgentType.engine_id`,
  `delegate_spec` / `specialise` (the lead's `delegate` tool lists the agent apps
  with their rules; plain `delegate` when none is on offer; called from
  `run.rs` after `ToolRegistry::build`), a pre-pass in `delegate` that cites,
  limits and raises the handover card before any child opens (refused tasks are
  reported, the rest run), engine children run through `run_external` in the
  foreground and, via `background::EngineSpawn`, in the background (not
  headless: the panel gets their questions). A plan-first lead makes the agent
  read-only.
- `engines/mod.rs`: `EngineInfo` gained `use_mode`, `guards`, `rules` (with
  labels), `delegate_ready`, `diagnostics`. `settings.rs`: patch accepts
  `use_mode` and `guards`.
- `commands/engines.rs` + `lib.rs`: `engines_rule_add_cmd`,
  `engines_rule_update_cmd`, `engines_rule_delete_cmd`,
  `engines_rules_reorder_cmd`, `engines_rules_reset_cmd`,
  `engines_undo_handover_cmd`, `engines_route_preview_cmd`,
  `set_conversation_engines_off_cmd`. `commands/agent.rs`: `resolve_turn` and
  `TurnSetup` are `pub(crate)`.
- Test double: `fixtures/engines/fake-claude.mjs` (Node) + `engines/e2e.rs`
  (Windows, skips without Node): allow once, deny, allow for this run, headless
  and read-only refuse without asking, edit diff, self-allowed footer, mode per
  trust and ceiling, nothing secret or free text in argv, Stop kills the tree
  (grandchild) in a few seconds, open question withdrawn on Stop, steer, gone
  session, session and transcript kept, failed run diagnostics, max_parallel
  queueing.

### Test state at the end of Phase 2

- **Rust: `cargo test --lib` green, 952 passed, 0 failed, 9 ignored** (includes
  the 15 end-to-end tests in `engines/e2e.rs` against the fake agent).
  `cargo clippy --lib --tests`: no warning in any file this phase touched; the
  others are in older code.
- **Frontend: `npx tsc --noEmit` clean. Vitest: 883 passed, 1 failed**, the
  known `icons.test.ts` (WindowControls.tsx), which is not ours. New Vitest
  files: `components/Engines/RuleList.test.tsx` (RTE-T-5),
  `components/SidePanel/PermissionPanel.engine.test.tsx`,
  `components/Conversation/FleetCard.engine.test.tsx`,
  `components/Conversation/SessionMenu.engines.test.tsx`; additions to
  `lib/engines.test.ts` and `lib/store.subruns.test.ts` (`permission_gone`).
  `routes/Engines.test.tsx` got the new `EngineInfo` fields in its fixture.
- `cargo test --test copy_lint` was not run this session.
- **Not verified:** the running app (nobody has looked at the new screens by
  eye), and any real Claude Code run through `delegate`. Everything about
  approvals, Stop, steer and sessions is proven only against the fake agent.

### Frontend (Phase 2)

- `src/lib/api.ts`, `types.ts`, `store.ts`: new types and wrappers (see the
  Phase 2 list above), `permission_gone`, `sub_spawned.engine`,
  `Conversation.enginesOff` (from `DbConversation.engines_off`) and the store
  action `setEnginesOff(conversationId, off)`.
- `src/lib/engines.ts`: `payChip`, `becauseChip`, `MODE_LABELS`, `modeHelp`,
  `previewLine`, `ruleLooksWrong`.
- `PermissionPanel.tsx`: `EnginePrompt` (CLI-UI-3) and `HandoverPrompt`
  (RTE-UI-2).
- `Conversation/EngineChips.tsx`: `EngineChips`, `UndoHandover`, `ContinueThere`.
  Used by `FleetCard.tsx` (chips beside the agent name, a gear that opens that
  agent's settings, steer placeholder *I'll pass this on when it finishes this
  step* when `can_steer` is false, take-back and **Continue there** under a
  finished row) and by `Workbench/AgentsPanel.tsx` `RunView`. CSS in
  `Conversation.css` (`.fleet-who`, `.fleet-chip`, `.fleet-chip-why`,
  `.fleet-gear`, `.engine-after`, `.engine-continue`).
- `components/Engines/RuleList.tsx` + `RuleList.css` (new), used by
  `routes/Engines.tsx`: **When I use it** first in an expanded row (mode control,
  *Use it for* / *Don't use it for* with labels, inline edit with the 140 counter,
  on/off, up/down, move to the other list, Delete, *starts without asking* mark,
  used / declined counts in amber, *suggested by me* tag, **+ Add a rule**, the
  guards, **Reset to defaults** behind `ConfirmDialog`); an agent with
  `delegate_ready === false` shows *I can't hand work to {name} yet. It is set up
  and testable here.* instead; **Recent jobs** (from `diagnostics`) before
  Advanced; **Try a request** under the limits, shown while at least one agent
  that can take work is on.
- `Conversation/SessionMenu.tsx`: *Keep external agents out of this chat* / *Use
  external agents in this chat*, shown only when an agent that can take work is on.

### Phase 2 report: Done 2026-10-20

Deviations and choices:
- Guards in the UI: "In a folder" (locked), "Code projects only", "Folder not
  read-only" (sets `min_trust` to `confirm` or `read-only`) and a "per chat"
  number. `not_in_projects` has no control yet; it is kept as it is when a guard
  is saved.
- Rules move with up/down buttons and a "Move to ..." button; Delete is
  immediate (**Reset to defaults** is the way back). No drag, no menu popover.
- Try a request shows the matched rule's words after the line.
- The composer chip, `/external`, `/delegate [agent]` and the persona "Runs on"
  line are still Phase 3.

### Open items for Erich (Phase 2)

1. Open Settings → External agents → Claude Code and look at **When I use it**:
   change the mode, edit a rule, add one, reorder, **Reset to defaults**, **Try a
   request**. Check it looks right.
2. In a chat with a folder on a real repo, ask for a change across many files.
   Expect a handover card naming the rule; **Hand it over**; then an approval
   panel for Claude Code's first command or edit; **Allow once**. Press **Stop**
   on another run mid-way and check no `claude.exe` of yours is left running.
3. Leave one approval open for about a minute and answer it (`CLI-T-8`).
4. On a finished row: **Continue there** opens a terminal in the folder and
   copies the command; **This shouldn't have gone to Claude Code** stops it and
   counts a no on the rule.
5. Try the chat menu line *Keep external agents out of this chat* and ask for the
   same change again.
6. Say if the copy sounds wrong anywhere.

Then wait for Erich's go on Phase 3 (ACP / OpenCode, `/delegate [agent]`,
`/external`, persona "Runs on").

### Decisions made in Phase 2 (this part wins)

- `use_mode` default `suggest` for Claude Code and Codex, `on_request` for ACP.
- `delegate_to` (the `/delegate <agent>` word) is plumbed in `RouteCtx` and
  `rules::cite(asked)` but nothing sets it yet: CLI-UI-6 is Phase 3. Until then
  an `on_request` agent cannot be used at all.
- Personas that run on an agent app (`personas.engine_id`) are not offered yet
  (CLI-UI-4 is Phase 3).
- Engine children waiting for a slot are `queued` in the database; the Fleet row
  has no separate queued look for them yet.
- No usage row is written for agent-app runs yet (CLI-UI-7, Phase 4).
- Approval wait is 10 minutes, then a no. Longer open questions are still
  untested with the real Claude Code (`CLI-T-8`, Erich's pass).

## Phase 1 report

### What was built

Backend (`src-tauri/`):
- **Schema v32** (`db/schema.sql`, `db/mod.rs`): table `engines` with every
  `CLI-1` and `CLI-12` column, plus `personas.engine_id` and
  `conversations.engine_session` (not used until Phase 2).
- `db/engines.rs`: `EngineRow` and the queries. The user's choices
  (`save_engine_settings`) and what a probe found (`save_engine_probe`) are
  separate statements, so a probe never undoes a switch the user just flipped.
- `shared/engines.json`: the catalog (see decisions below).
- `engines/`:
  `catalog.rs` (the catalog), `locate.rs` (install places and the fresh PATH
  from the registry), `process.rs` (spawn, npm shim, cleaned env, line reader
  with a 1 MiB cap, stderr tail, `kill_tree`, `run_capture`), `probe.rs`
  (version and sign-in), `driver.rs` (the `EngineDriver` trait, `Capabilities`,
  `ExternalTurn`, `ExternalOutcome`, `Launch`), `claude.rs`, `codex.rs`,
  `acp.rs` (one driver each), `settings.rs` (`EnginePatch` and every check),
  `tryit.rs` (`CLI-13`), `mod.rs` (`detect_all`, `refresh`, `EngineInfo`).
- `commands/engines.rs`: `engines_list`, `engines_detect`, `engines_mark_seen`,
  `engines_probe`, `engines_update`, `engines_add_acp`, `engines_remove`,
  `engines_set_key`, `engines_clear_key`, `engines_open_login`, `engines_test`,
  `engines_models` (all with the `_cmd` suffix). Every command that changes
  something returns the whole fresh list. The key goes in and never comes back
  (`has_key` only).
- Small changes elsewhere: `runtime/jobobject.rs` gained `assign_raw` (agent
  processes join the kill-on-close job); `secrets.rs` gained `SERVICE_ENGINES =
  "poiesis-engines"`; `Cargo.toml` gained the `windows-sys` features
  `Win32_System_Registry` and `Win32_System_Environment`; `lib.rs` registers the
  commands and starts the boot detection.

Frontend (`src/`):
- `lib/api.ts` (types and wrappers), `lib/store.ts` (`engines`,
  `enginesNewlyFound`, `engineFocus`, `openEngines`, `loadEngines`,
  `detectEngines`, `updateEngine`, `adoptEngines`, `markEnginesSeen`; boot load,
  the `poiesis-engines-found` event, and a focus check at most every 5 min).
- `lib/engines.ts`: the wording (status line, why the switch is off, the Try it
  line). `routes/Engines.tsx` + `Engines.css`: the section.
- `lib/types.ts` (`engines` view and hub section), `routes/SettingsHub.tsx`
  (render and the dot), `components/Icons/Icons.tsx` (`SectionIcon` case),
  links from `routes/Providers.tsx` and `components/Personas/DelegationCaps.tsx`.

### Proven on this machine

`cargo test --lib real_agents -- --ignored --nocapture` finds, probes and tries
the real apps (Claude Code needs `POIESIS_TEST_CLAUDE=<path to claude.exe>`,
because it is not on PATH). Result on 2026-10-20:

| Agent | Found through | Version | Signed in | Try it |
|---|---|---|---|---|
| Codex | npm shim `%APPDATA%\npm\codex.cmd` | 0.163.0, tested | yes, plan | ready, 13.6 s, model from its config |
| OpenCode | npm shim `%APPDATA%\npm\opencode.cmd` | 2.1.0, tested | not applicable, own provider | ready, 11.0 s, `opencode/big-pickle` |
| Claude Code | **Set path** (VS Code copy) | 2.1.301, tested | yes, plan | ready, 10.3 s, `claude-sonnet-5-5` |

No process was left behind and no scratch folder remained. The user's own
`codex.exe` and `claude.exe` processes were not touched.

### Tests added

- Rust, 44 tests, all pass (45 with the ignored real one); the whole lib suite is
  892 passed. They include: parser replays of the Phase 0 recordings for all
  three drivers (`CLI-T-1`), detection with a temp `%USERPROFILE%`/`%APPDATA%`
  (`CLI-T-12`), the cleaned environment (`CLI-T-5`), a 2 MiB line (`CLI-T-7`),
  settings validation, a first find making a disabled row, and an uninstalled
  agent keeping its choices.
- Vitest: `Engines.test.tsx` (`CLI-T-10`), `engines.test.ts`, and one new routing
  test. The whole suite is 842 passed, 1 failed. The failure is
  `components/Icons/icons.test.ts` about `TopBar/WindowControls.tsx`, a file this
  work did not touch. `cargo test --test copy_lint` also fails, on
  `CommandMenu.tsx` and `playbackQueue.test.ts`, also not touched. Both were
  failing before and are not part of this work.
- **Not checked by eye.** The screen was not opened in the running app. Erich
  should open Settings → External agents once (see "Open items").

### Decisions and differences from Parts I-IX (this part wins)

1. **Hub position.** External agents sits right after **Runtime**, not right
   after Providers. An existing test (`RTM-9`) fixes Runtime directly after
   Providers.
2. **`engines.program` is the path the user set.** The path the search found is
   a new column, `found_path`. `CLI-1` had one column for both. With two, Reset
   (back to auto-detect) is just clearing `program`.
3. **The catalog has the three recorded agents only** (Qwen Code was dropped
   from the plan on 2026-10-23 and is no longer a goal). `tested` ranges are `tested_from` (the recorded version) and `tested_below`
   (the first version nobody looked at): Claude 2.1.301 to 2.3.0, Codex 0.163.0 to
   0.170.0, OpenCode 2.1.0 to 3.0.0. A newer version shows an amber note and
   still runs.
4. **Catalog fields.** `status` = `{args, reads: stdout|stderr, format}`;
   `login_args` (what Sign in runs: `auth login`, `login`, `auth login`);
   `acp_args`; `ask_everything_env` (OpenCode's inline `permission` config);
   `strip_env_prefixes`; `paid_by` for an agent that has no plan (`own`);
   `api_key_env`. **OpenCode has no status command**: `auth list` tells us
   nothing we use, so it is not called. `install_url` for Codex and OpenCode is
   their repository (read from the installed `package.json`).
5. **API-key mode exists only where the catalog names the variable.** That is
   Claude Code (`ANTHROPIC_API_KEY`). Codex's API-key login was never recorded,
   so no option is shown for it until Phase 4. OpenCode has none.
6. **Environment (`CLI-9`).** The child gets the user's env minus `POIESIS_*`
   and a fixed list of provider keys. In plan mode the agent's **own** key
   variable is removed too, so a stray `ANTHROPIC_API_KEY` cannot bill the API
   behind the user's back. In API-key mode the saved key is added by name. The
   fresh registry PATH is set for the child.
7. **A secret cannot be saved as an environment variable.** A name containing
   KEY, TOKEN, SECRET, PASSWORD or CREDENTIAL is refused with a pointer to
   Billing. The plan said "warning"; an error is stricter, because SQLite must
   never hold a secret.
8. **Extra arguments have a deny list** (`settings.rs`): flags that would take
   a decision from the panel, change who pays or where the agent works, or turn
   off its sign-in (`--dangerously*`, `--permission*`, `--allowed*`, `--sandbox`,
   `--config`/`-c`, `--add-dir`, `--model`, `--resume`, `--api-key`, ...). Shell
   characters are refused too, because of the `.cmd` shim.
9. **`ExternalTurn` is the narrow form.** Phase 1 has `{launch, task, cwd,
   tool_free, cancel, timeout}`. `sink`, `run`, `permissions`, `mcps`, `trust`,
   `resume` and `system_append` join in Phase 2, when the delegate tool and the
   panel exist to connect them. Until then **every approval request is
   declined**: Claude Code is started without a prompt tool (and, for Try it,
   with `--tools ""`); Codex with `untrusted` and a `read-only` sandbox; ACP
   rejects by option kind. Try it therefore cannot change anything on disk.
   `ExternalOutcome` gained `model`.
10. **Detection triggers built:** boot (`engines.detect_on_start`), opening the
    section, window focus (at most every 5 min, file checks plus whatever probe
    is due), **Check again** (forces). A probe is due when the found path
    changed, there was none yet, or the last one is over 1 h old. The modified
    time of the file is not tracked. "A delegate probes an agent older than 10
    min" is Phase 2.
11. **Models (`engines_models`).** Claude Code: the two aliases its help names
    (`sonnet`, `opus`). Codex: `model/list`. ACP: none (the picker is hidden;
    choosing an ACP model needs a recorded set-model call, Phase 3).
12. **Global keys.** `engines.max_parallel`, `engines.timeout_secs` and
    `engines.detect_on_start` have controls. The first two have no consumer until
    Phase 2. `engines.idle_close_secs` has neither control nor consumer until
    Phase 3 (no ACP pool yet). `memory_tools` has no control until Phase 5.
13. **Not built in Phase 1 on purpose:** "When I use it" (RTE-UI-1), Try a
    request (RTE-UI-4), Recent runs and Copy diagnostics (`CLI-10`). They need
    Phase 2.
14. **Sign in** opens `cmd /c start ... cmd /k "<program>" <login_args>`: a real
    terminal running the app's own login. After it, the row looks again when the
    window comes back to the front. Poiesis never sees what is typed there.

## Phase 0 report

### What was built

- `scripts/record-engine.mjs claude ...` spawns Claude Code headless exactly as
  CLI-7 lists it. It serves the `approve` MCP tool on loopback (this is the
  MCPS-1 contract) and logs both directions. Options: `--deny-match <regex>`,
  `--interrupt`, `--steer <text>`, `--resume <id>`, `--approve-delay-ms <n>`.
- `scripts/record-acp.mjs` does the same for an ACP agent (JSON-RPC on stdio).
  Used with OpenCode. Options: `--deny-match`, `--cancel`, `--prompt2`,
  `--load <sessionId>`, `--auth <methodId>`.
- `scripts/record-codex.mjs` does the same for `codex app-server`. Options:
  `--deny-match`, `--cancel` (sends `turn/interrupt` at the first approval),
  `--steer <text>`, `--resume <threadId>`, `--policy`, `--sandbox`. It only
  reads the Windows sandbox readiness and never starts a sandbox setup.
- `scripts/engine-rec.mjs` holds the shared part: row format and the scrub of
  home, user name and the scratch folder, also inside nested JSON strings.
  `--verbose` prints rows live.
- Re-record on every new CLI version with these scripts. A new version gets
  new files. Old files stay (CLI-T-1).

### Fixtures (`fixtures/engines/`)

Row format: `{"dir":"in"|"out"|"err","t":<ms>,"ch"?:"mcp"|"spawn","msg":{...}}`.
This extends CLI-0: `ch:"mcp"` rows are the approve tool (`in` = request from
the CLI, `out` = our reply), `ch:"spawn"` is the argv, `err` rows are stderr
lines and recorder notes. **One session per file, with a scenario suffix**,
not one file per engine.

| File | Scenario | Proves |
|---|---|---|
| `claude-2.1.301.jsonl` | Write allowed, `mkdir` allowed, `mkdir` denied | start, text, tool start and end, two allows, one deny, usage, cost |
| `claude-2.1.301-interrupt.jsonl` | Interrupt while an approval is pending | protocol interrupt, pending tool rejected, run end |
| `claude-2.1.301-steer.jsonl` | Second user message after the first tool call | mid-turn steer |
| `claude-2.1.301-resume.jsonl` | `--resume <id>` of the steer session | resume keeps the session id |
| `claude-2.1.301-resume-missing.jsonl` | `--resume` of an unknown id | the failure shape |
| `claude-2.1.301-auth-status.json` | Output of `claude auth status`, ids replaced | the probe input |
| `opencode-2.1.0.jsonl` | ACP. `permission` set to `ask`. Write allowed, `mkdir` allowed, `mkdir` rejected | handshake, session, streaming, three permission round trips, usage |
| `opencode-2.1.0-default-never-asks.jsonl` | ACP with OpenCode's default config, same prompt | **no permission request at all**; `denied-dir` was created |
| `opencode-2.1.0-cancel.jsonl` | `session/cancel` while a permission request is open | cancel round trip |
| `opencode-2.1.0-load.jsonl` | `session/load` of the first session, then a question | resume and history replay |
| `codex-0.163.0.jsonl` | `approvalPolicy: untrusted`. File add accepted, `mkdir` accepted, `mkdir` declined | handshake, thread, streaming, three approval round trips, usage, completion |
| `codex-0.163.0-interrupt.jsonl` | `turn/interrupt` while a file approval is open | interrupt round trip |
| `codex-0.163.0-steer.jsonl` | `turn/steer` after the turn started | mid-turn steer |
| `codex-0.163.0-resume.jsonl` | `thread/resume` of the main thread, then a question | resume |
| `codex-0.163.0-resume-missing.jsonl` | `thread/resume` of an unknown id | the failure shape |
| `codex-0.163.0-login-status.txt` | `--version` and `login status` output | the probe input (note: stderr) |

All were checked for the e-mail, org id, user name, host name, install id and
home path. None remain.
The `init` event is trimmed: this user's plugin, skill and path lists are
replaced by `<n items>`.

### Findings (confirmed against claude 2.1.301; they replace guesses above)

**Stdio and events**
1. The spawn line in CLI-7 works as written. First `system` event arrives after
   about 4 s. The prompt goes in as
   `{"type":"user","message":{"role":"user","content":[{"type":"text","text":"..."}]}}`.
2. Event types seen, in order: `system/hook_started`, `system/hook_response`
   (the user's own SessionStart hook, so **ignore `system` subtypes other than
   `init`**), `system/init`, `rate_limit_event` (ignore), `assistant`, `user`,
   `result`.
3. `assistant` events hold **whole content blocks** (`text`, `tool_use`). There
   is no token stream, because we do not pass `--include-partial-messages`.
   Decision for v1: `Token` events are one per text block. A delegated child
   shows steps, so this is enough.
4. `init` carries `session_id`, `model`, `permissionMode`, `tools`,
   `claude_code_version`, and `apiKeySource` (`"none"` = plan login). Use
   `apiKeySource` as a second hint for `paid_by`, next to `auth status`.
5. `user` events with a `tool_result` carry `is_error` and a
   `tool_result_meta[]` entry per tool. Its `permission_decision` is
   `{decision: "accept"|"reject", source}`.
   - `source: "user_temporary"` or `"user_reject"` = our approve tool answered.
   - `source: "config"` = Claude's own settings allowed it and **we were never
     asked**. This is the self-allowed counter for the CLI-6 footer. It really
     happens: `echo first` and `echo second` ran with `source:"config"` here.
6. `result`: `subtype` (`success` | `error_during_execution`), `is_error`,
   `result` (the report text, null on error), `stop_reason`, `terminal_reason`
   (`completed` | `aborted_tools`), `total_cost_usd`, `usage`
   (`input_tokens`, `output_tokens`, `cache_read_input_tokens`,
   `cache_creation_input_tokens`), `permission_denials[]`
   (`tool_name`, `tool_use_id`, `tool_input`), `num_turns`, `session_id`,
   `errors[]` on failure. Map `total_cost_usd` to `notional_cost_usd`.

**MCPS-1 (the approve tool)**
7. Claude first sends `server/discover` with protocol `2026-07-28`. Reply
   JSON-RPC error `-32601` and it falls back to `initialize` with protocol
   `2025-11-25`. **MCPS-1 must echo the client's `protocolVersion`.** Do not
   hard-code `2025-06-18`. Then `notifications/initialized` (reply 202),
   `tools/list`, `tools/call`.
8. `tools/call` arguments are `{tool_name, input, tool_use_id}` plus
   `_meta.claudecode/toolUseId`. Our reply is one text content holding JSON.
   Both shapes worked:
   `{"behavior":"allow","updatedInput":<input>}` and
   `{"behavior":"deny","message":"..."}`. The deny message reaches the model as
   the tool result text.
9. Tool names seen: `Write` (input `file_path`, `content`), `Bash` (input
   `command`, `description`). This machine also lists a `PowerShell` tool.
   `describe_engine_tool` must cover `PowerShell`.
10. The reply may be delayed: a 1.5 s delay worked (steer fixture). Longer
    waits are untested, so `CLI-T-8` (Erich's manual pass) must include an
    approval left open for about a minute.

**Stop, steer, resume**
11. **Interrupt:** write `{"type":"control_request","request_id":"..","request":{"subtype":"interrupt"}}`.
    Claude answers `control_response` `success` within about 50 ms, rejects the
    pending tool, **closes the open approve HTTP request**, emits `result` with
    `error_during_execution` and `terminal_reason:"aborted_tools"`, and exits
    with code 1. The approve handler must treat a closed connection as
    "cancelled" and drop the panel request. `capabilities.interrupt = true`.
12. **Steer:** a second user message written to stdin mid-turn is folded into
    the same turn. One `result`, and the extra file was made.
    `capabilities.steer_mid_turn = true`. No `--resume` fallback is needed.
13. **Resume:** `--resume <id>` continues the session and keeps the same
    `session_id`. An unknown id gives `result` `error_during_execution` with
    `errors:["No conversation found with session ID: ..."]`, one stderr line,
    exit code 1. Detect it by that `errors` text and start fresh once (CLI-7).

**Payment and sign-in**
14. `claude auth status` prints JSON: `loggedIn`, `authMethod`
    (`"claude.ai"` = plan), `apiProvider`, `subscriptionType`, plus the e-mail
    and org ids. **Read only the four non-identifying fields. Never log or
    store the rest** (global rule, TRM-2).

**Machine facts that change the build**
15. On Erich's machine `claude` is **not on PATH and not in any CLI-2 install
    location**. The only copy is the VS Code extension's bundled binary:
    `%USERPROFILE%\.vscode\extensions\anthropic.claude-code-<ver>-win32-x64\resources\native-binary\claude.exe`.
    CLI-2 would report Claude Code as "not found" here. Decision: do **not**
    add that folder to the search list (the version is in the path and it is
    not a user install). The user sets it with **Set path...** (`program`).
    Phase 1 must make Set path... work well, and Erich uses it for testing.
16. The user's own hooks run in a delegated session (`SessionStart` here).
    `--strict-mcp-config` does not stop hooks. Their output is noise on
    stdout and must be ignored, never shown as agent text.

### Codex 0.163.0 (recorded over `app-server`; added 2026-10-20)

The first check found the April 2025 legacy CLI, which has no `app-server`.
Erich then installed a current build. **This is the record of the current one.**

**What it is on this machine.** `codex-cli 0.163.0`, npm package
`@openai/codex`, signed in with ChatGPT. The shim `%APPDATA%\npm\codex.cmd`
starts the native exe
`%APPDATA%\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe`.
Other `codex.exe app-server` processes run on this machine (the ChatGPT app and
the VS Code extension). They are not ours. We always start our own with
`codex app-server` (stdio is the default transport) and never attach to
another, never use `app-server daemon`, `proxy` or `--listen`.
The CLI can write its own protocol schema:
`codex app-server generate-json-schema --out <dir>`. Use it when a new version
arrives, together with a new fixture.

**Probe (CLI-2).** `codex --version` prints `codex-cli 0.163.0` on stdout.
`codex login status` exits 0 and prints `Logged in using ChatGPT` **on stderr,
stdout is empty**. Read stderr. The not-signed-in and API-key texts were not
seen. Treat a non-zero exit as not signed in and match on `ChatGPT` for
`paid_by = plan`. After `initialize`, the server also sends
`account/updated {authMode, planType}` (here `chatgpt`, `free`). Show
`planType` in the row, because a free plan has small limits.

**Handshake.** JSON-RPC lines, **no `"jsonrpc"` field**. `initialize
{clientInfo:{name:"poiesis", title:null, version}}` answers in about 0.7 s with
`{userAgent, codexHome, platformFamily, platformOs}`. `userAgent` starts with
`poiesis/`, so TRM-5 is met. Then the notification `{"method":"initialized"}`.
`windowsSandbox/readiness` returned `{status:"ready"}`. It is read-only and fine
for the probe. **Never call `windowsSandbox/setupStart`** (it can raise a
system prompt).

**Thread and turn.**
- `thread/start {cwd, approvalPolicy, sandbox, approvalsReviewer}`. The reply
  has `thread.id`, the effective `approvalPolicy`, `sandbox`, `model`,
  `modelProvider`.
- `approvalPolicy`: `untrusted` | `on-request` | `never` | `{granular:...}`.
  `sandbox`: `read-only` | `workspace-write` | `danger-full-access`.
  `untrusted` asked for the file add and for **every** command. `on-request`
  and `read-only` were not recorded. Phase 4 records them before it relies on
  the CLI-6 table.
- **`approvalsReviewer` must be `"user"`.** The other values (`auto_review`,
  `guardian_subagent`) let Codex answer its own approvals without us.
- `thread/start` took the model from the user's `config.toml`, not from the
  model marked default by `model/list`. Pass `model` explicitly when the
  engine setting is set. `model/list` returns `data[]` with `id`,
  `displayName`, `isDefault`, `supportedReasoningEfforts`: this is
  `engines_models`.
- `turn/start {threadId, input:[{type:"text", text, text_elements:[]}]}` returns
  `{turn:{id, status:"inProgress"}}`. Per-turn overrides exist
  (`approvalPolicy`, `sandboxPolicy`, `model`, `effort`, `cwd`).
- The user's own MCP servers from `config.toml` start with the thread
  (`mcpServer/startupStatus/updated`, up to about 10 s). First model output came
  after about 10 s. Same family as Claude's hooks: ignore the noise.

**Events.** Ignore: `mcpServer/*`, `remoteControl/*`, `account/*`,
`thread/started`, `serverRequest/resolved`, `turn/diff/updated`,
`item/reasoning/*` (empty summaries here).
- `item/started` and `item/completed` carry `item.type`: `userMessage`,
  `reasoning`, `agentMessage`, `commandExecution`, `fileChange`.
- `agentMessage` has `phase`: `commentary` or `final_answer`. **The report to
  the lead is the `final_answer` message.** `item/agentMessage/delta {delta}`
  streams real tokens.
- `commandExecution {command, status, exitCode, aggregatedOutput}`. `command`
  is the full string including the shell wrapper
  (`"...powershell.exe" -Command 'mkdir allowed-dir'`). Show the inner command
  as the step target. `status` is `inProgress` | `completed` | `declined`.
- `fileChange {changes:[{path, kind:{type:"add"|...}, diff}], status}`.
- `thread/tokenUsage/updated {tokenUsage:{total, last, modelContextWindow}}`
  with `inputTokens`, `cachedInputTokens`, `outputTokens`,
  `reasoningOutputTokens`, `totalTokens`. There is no cost field, so
  `notional_cost_usd` is None for Codex.
- `thread/status/changed` with `activeFlags:["waitingOnApproval"]` is a good
  "waiting for you" signal for the Fleet row.
- `turn/completed {turn:{status:"completed"|"interrupted"|"failed", items}}`
  ends the turn.

**Approvals (CLI-6).** These are server-to-client requests with a numeric `id`.
- `item/commandExecution/requestApproval {itemId, command, cwd, reason,
  commandActions, kind:"command", ...}`
- `item/fileChange/requestApproval {itemId, reason, grantRoot}`: **it has no
  path and no diff.** Join on `itemId` with the `fileChange` item from the
  `item/started` event just before it (`changes[].path`, `changes[].diff`).
- Reply `{decision}` where `decision` is `accept` | `acceptForSession` |
  `decline` | `cancel`. **This replaces `{action: ...}` in CLI-11.** Send
  `accept`, `decline` or `cancel` only. Never send `acceptForSession` or the
  object forms `acceptWithExecpolicyAmendment` and `applyNetworkPolicyAmendment`:
  they save rules in Codex's own files. "Allow for this run" is our memory.
- After a `decline`, the item ends `declined`, stderr shows `rejected by user`,
  and **the model carried on** and reported. The turn ended `completed`. This
  is like Claude and unlike OpenCode.
- Other server requests exist: `item/permissions/requestApproval`,
  `item/tool/requestUserInput`, `mcpServer/elicitation/request`,
  `item/tool/call`, `attestation/generate`, the old `applyPatchApproval` and
  `execCommandApproval`, and **`account/chatgptAuthTokens/refresh`**. None
  appeared. The driver answers any unknown one with JSON-RPC error `-32601` so a
  turn never blocks. It **always refuses `account/chatgptAuthTokens/refresh`**:
  we never supply tokens (TRM-2). `tool/requestUserInput` stays as CLI-11 says.

**Stop, steer, resume.**
- `turn/interrupt {threadId, turnId}` replies `{}`, then `turn/completed` with
  `status:"interrupted"` about 15 ms later. A pending approval must be answered
  `cancel` first. Nothing was written. `interrupt = true`.
- `turn/steer {threadId, expectedTurnId, input}` replies `{turnId}`. The text
  joins the same turn (a new `userMessage` item appears after the current
  step), one `turn/completed`, both files made. A steered action asks for its
  own approval. `steer_mid_turn = true`.
- `thread/resume {threadId, cwd, approvalPolicy, sandbox, approvalsReviewer}`
  keeps the context. It sends a `deprecationNotice`: **pass `excludeTurns:
  true`** so it does not load the full history. An unknown id gives a JSON-RPC
  error `{code:-32600, message:"no rollout found for thread id ..."}`: start
  fresh once (CLI-7 rule).
- **Clean exit:** ending stdin makes `app-server` exit with code 0. Use that
  first, then the 3 s kill.

**Catalog entry:** `{ "id":"codex", "kind":"codex", "name":"Codex",
"binaries":["codex"], "status":["login","status"], "tested":[">=0.163 <0.200"] }`.
The status text is on stderr (above). Spawn the native exe when the shim
resolves to one, otherwise the shim, with the job object either way.

### ACP lessons that are not about one agent

These came from the first ACP agent tested and hold for any ACP engine.
- There is no cheap, safe way to know that an ACP agent is signed in.
  `signed_in` stays NULL (unknown) for ACP engines. **Try it (CLI-13) is the
  real check.**
- `authenticate` can open a browser or hang. CLI-8 never calls it during
  detection, and wraps it in a 20 s timeout elsewhere. On timeout it kills the
  process and reports "not signed in" with the agent's own stderr text.
- Killing the shim (`cmd.exe`) left the agent's Node processes running (shim,
  then two Node levels). **Stop must kill the whole tree** (job object, then
  `taskkill /T /F`). This confirms CLI-3 and CLI-T-4.
- Read `loadSession` and `mcpCapabilities` from the `initialize` reply at run
  time. Agents differ and old versions say `false`.

### OpenCode 2.1.0 (recorded over ACP; added 2026-10-20)

Erich has it installed, and it is a common agent, so it is the first real ACP
engine and the ACP reference for CLI-8.

**What it is on this machine.** npm global package `@opencode/cli` 2.1.0
(repo `anomalyco/opencode`). Files in `%APPDATA%\npm`: `opencode.cmd` and
`opencode2.cmd`, both shims for one native exe,
`%APPDATA%\npm\node_modules\@opencode\cli\bin\opencode.exe`. `--version`
prints `opencode v2.1.0`. This is the v2 line. The older `opencode-ai` v1
package is a different build, so the catalog `tested` range is `>=2.1 <3`.
Subcommands we use: `acp`. Others exist (`run --format json`, `serve`, a
background service). ACP covers everything we need, so they stay unused.

**Handshake** (`opencode acp`, ACP protocol 1, about 2.5 s to answer
`initialize`):
- `agentCapabilities`: `loadSession: true`, `mcpCapabilities.http: true`,
  `promptCapabilities` image and embeddedContext, `sessionCapabilities`
  `resume`, `fork`, `list`, `close`, `delete`. `agentInfo` has the version.
- `authMethods`: one, `opencode-login` ("Run `opencode auth login` in the
  terminal"). We never call `authenticate`. Sign-in is the user's terminal
  (TRM-2).
- `session/new { cwd, mcpServers: [] }` returns `sessionId` (`ses_...`) and
  `configOptions`: `model` (a select, many entries) and `mode` (`build` |
  `plan`). Both can be changed with ACP session config. The `model` list is
  what `engines_models` returns. `mode: plan` is the read-only trust level.

**Events** (all as in CLI-8's table): `agent_message_chunk` and
`agent_thought_chunk` stream in small pieces, so OpenCode gives real token
streaming. `tool_call` arrives as `pending` with an empty `rawInput`, then
`tool_call_update` `in_progress` with the real `rawInput` and `title` (the
command text), then `completed` with `content`. So **take the step label from
the first `in_progress` update, not from `tool_call`.** `kind` is `edit` or
`execute`. Tool names seen: `write`, `shell`. Also `available_commands_update`
(ignore) and `usage_update` (`used`, `size`, `cost.amount`). `session/prompt`
returns `stopReason` plus `usage` (`inputTokens`, `outputTokens`,
`totalTokens`, `thoughtTokens`, `cachedReadTokens`).

**Permissions. This is the safety finding.**
1. **By default OpenCode never asks.** With its default config it wrote files
   and ran `mkdir denied-dir` with no `session/request_permission`. The plan's
   trust table (CLI-6) would silently not apply.
2. **Fix: set the permission in the child's environment.**
   `OPENCODE_CONFIG_CONTENT={"permission":{"edit":"ask","bash":"ask"}}` makes
   OpenCode send a request for every edit and command. It is inline config, so
   no file is written and nothing goes on argv. It is not an auth flag, so
   TRM-1 holds. **The driver must always set it** and must derive it from the
   effective trust: read-only = `mode: plan` plus `ask`; confirm = `ask` for
   edit and bash; auto = `edit: allow`, `bash: ask`.
3. **The inline config wins over a project file.** Test: a project
   `opencode.json` with `permission: allow` plus our inline `ask` still sent
   the request. The user's global config (`%USERPROFILE%\.config\opencode`)
   was not tested and is documented as lower priority. Add it to
   `CLI-T-8` (Erich's manual pass) as a check.
4. The request is `session/request_permission` with `toolCall` (`name`,
   `title`, `kind`, `locations`, `rawInput`, and for edits a `content` diff
   with `oldText`/`newText`) and `options`: `once` (`allow_once`), `always`
   (`allow_always`), `reject` (`reject_once`). Pick by `kind` only (CLI-6).
   Never offer `always`: "Allow for this run" is remembered by us, not by the
   engine, so no permission is stored in OpenCode's own files.
5. **A reject ends the whole turn.** `session/prompt` returned
   `stopReason: "cancelled"` right after our reject, and the model did not
   continue. Claude continues after a deny. So the driver reports it as
   *"Stopped because you declined `mkdir denied-dir`"* and the lead decides
   what to do. `ExternalOutcome.denials` carries it.

**Stop.** `session/cancel` (notification) while a request is open, plus our
reply `outcome: cancelled`, ends the turn with `stopReason: "cancelled"` in
about 50 ms. The file was not created. `capabilities.interrupt = true`.
**Steer** during a turn: not tested. Set `steer_mid_turn = false` for ACP
until a test shows otherwise (the UI already handles false).

**Resume.** `session/load { sessionId, cwd, mcpServers: [] }` works and keeps
the context (the next answer named the earlier file and the declined command).
**History is replayed as `session/update` notifications before the prompt, and
OpenCode does not mark them `_meta.isReplay`.** So CLI-8's second rule is the
one that matters: drop every update that arrives before our `session/prompt`
is sent. Sessions live in OpenCode's own store. `capabilities.resume = true`.

**Sign-in and payment.**
- `opencode auth list` prints plain text. With nothing set up it prints
  `No authenticated integrations`. **`auth export` prints stored secrets. The
  catalog must never call it, and no code path may.**
- Free models (`opencode/*-free`, default `opencode/big-pickle`) ran with no
  login at all. So "signed in" does not gate OpenCode. The probe reports
  `signed_in = NULL` (unknown, not required). `paid_by` is not `plan` for
  OpenCode: it is a free model or the user's own provider key. Add a third
  value `PaidBy::Own` ("your own provider account", chip text *own provider*;
  *free model* when the model id ends in `-free`). Notional cost is shown
  from `usage_update.cost.amount`.
- CLI-9 scrubbing: for OpenCode, strip all provider keys (`OPENAI_API_KEY`,
  `ANTHROPIC_API_KEY`, and so on) from the child env by default. Otherwise a
  key you exported for something else would be billed without you choosing
  it in External agents. The user can add keys under Billing.

**Catalog entry** (`shared/engines.json`, shape as in CLI-2):
`{ "id":"acp:opencode", "kind":"acp", "name":"OpenCode", "binaries":["opencode"],
"acp_args":["acp"], "status":["auth","list"], "tested":[">=2.1 <3"] }`.
npm global install lands in `%APPDATA%\npm\opencode.cmd`, which CLI-2 already
searches. The shim is accepted and spawned as the plan says. It starts a native
exe below it, so Stop must use the job object plus `taskkill /T /F` (see "ACP
lessons"). Default rules: none, `use_mode = on_request`
(RTE-2, ACP engines).

**Timing.** With the free model, the first permission request arrived 17 s and
38 s after the prompt in two runs, and a whole three-step turn took about 21 s.
Free models are slow,
so `engines.timeout_secs` (1800 s) is fine, but the Codex-style 120 s stall
watchdog (CLI-11) must not be copied to ACP.

### Corrections to earlier parts

- **Schema version.** CLI-1, CLI-12, RTE-1, RTE-4, RTE-5 say "v31". The
  database is **already at v31** (`MCG-5`, `db/mod.rs:22`). Use the next free
  version, **v32** at the time of writing, and check `SCHEMA_VERSION` again
  when you start.
- **MCPS-1:** echo the client protocol version and answer `server/discover`
  with `-32601` (finding 7). Handle a closed connection as cancel (finding 11).
- **CLI-6 footer:** count steps by `permission_decision.source` (finding 5).
- **CLI-3 `Capabilities` for Claude:** `resume`, `steer_mid_turn`, `interrupt`,
  `mcp_tools` all true; `approvals: PromptTool`.
- **CLI-3 `Capabilities` for ACP:** read `resume` from `loadSession` and
  `mcp_tools` from `mcpCapabilities.http` in the `initialize` reply. Do not
  hard-code them per agent (OpenCode says `loadSession: true`, older agents
  say `false`). `interrupt = true`, `steer_mid_turn = false`,
  `approvals: InBand`.
- **CLI-3 `Capabilities` for Codex:** `resume`, `steer_mid_turn`, `interrupt`
  true; `mcp_tools` unchanged (Phase 5); `approvals: InBand`.
- **CLI-11 approval answer** is `{decision: "accept"|"decline"|"cancel"}`, not
  `{action: ...}` (also correct Part I, which describes the older protocol that
  OpenMausBot read). Never `acceptForSession` or the amendment forms.
- **CLI-11 thread calls:** send `approvalsReviewer: "user"` on every
  `thread/start`, `thread/resume` and, where used, `turn/start`. Send
  `excludeTurns: true` on resume. File-change approvals carry no path or diff:
  join on `itemId` with the earlier `fileChange` item.
- **CLI-2 Codex probe:** `login status` text is on stderr (see the Codex
  section).
- **CLI-9, Codex:** `notional_cost_usd` is None. Usage rows hold tokens only.
- **CLI-6, ACP row:** "every request to the panel" holds only if the driver
  first makes the agent ask. For OpenCode that is the inline config above.
  Each ACP quirk row (`acp_quirks`, CLI-8) therefore needs an
  `ask_everything` field (env or session config) in addition to spawn args.
  A new ACP agent is not enabled until its fixture shows a permission request
  for an edit and for a command.
- **CLI-8:** a reject can end the turn (`stopReason: cancelled`). Treat
  `cancelled` after our own reject as "declined", not as an error.
- **CLI-2 `PaidBy`:** add `Own` (finding above). CLI-9 and `CLI-UI-2` chips
  gain *own provider* and *free model*.
- **CLI-0 acceptance** is met for Claude Code, Codex and OpenCode: each has a
  fixture with session start, streaming, tool start and end, an approval round
  trip, a denial, completion with usage, and an interrupt.
- **Gemini CLI is removed** from the catalog, examples, tests and sequencing.
  See `CLI-P3`.
- **CLI-1:** table as in Phase 1 decision 2 (`found_path`). **CLI-2:** catalog
  as in Phase 1 decisions 3 to 5. **CLI-3:** `ExternalTurn` as in decision 9.
- **CLI-9:** as in Phase 1 decision 6.

### Fix after the Phase 1 report: Claude Code in VS Code is found by itself

Erich's Claude Code is only the VS Code extension. Its program is in
`%USERPROFILE%\.vscode\extensions\anthropic.claude-code-<version>-win32-x64\resources\native-binary\claude.exe`
and is on no PATH, so the search missed it and he had to use **Set path...**.
`locate.rs` now looks there last (VS Code and VS Code Insiders; one folder
listing, newest version wins; a real install on the PATH still wins). Test:
`the_vs_code_extension_copy_is_found_last`. The extension's `claude.exe` is the
same one that passed **Try it** in the Phase 1 real check.

### Open items

Nothing blocks Phase 2.
1. **Erich's check of the new screen** (not done by anyone yet): open Settings →
   External agents; see Claude Code, Codex and OpenCode (Claude Code needs no
   **Set path...** any more, see the fix below);
   press **Try it** on each; turn one on and reload the app (it must stay on);
   open **Advanced** on one and add a harmless variable such as `HTTP_PROXY`.
2. **Not yet recorded** (Phases 2 to 4 record them before relying on them):
   Codex `on-request` and `read-only` modes, Codex with an API key (the
   `login status` text for that), OpenCode steering, OpenCode `mode: plan`, and
   the call that sets an ACP model.
3. **`CLI-T-8` (Erich's manual pass)** also checks: an approval left open for
   about a minute (Claude), the user's global OpenCode config not overriding our
   `ask`, and a Codex run on a folder inside a real repo.
4. **TRM-7** is still open and is needed only before a public build.
5. Two test failures that existed before this work are left alone (see "Tests
   added").

### Next step

Phase 3 (ACP / OpenCode driver, `/delegate [agent]`, `/external` and its
ModeChip, persona "Runs on", the queued look for engine rows) only after Erich
has looked at Phase 2 (see "Open items for Erich (Phase 2)") and says go. Read
"Phase 2 status and report" first, then `engines/claude.rs` and `engines/run.rs`,
which the ACP driver should mirror. `SCHEMA_VERSION` is 33 now.
