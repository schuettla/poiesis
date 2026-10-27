-- Project Poiesis — SQLite schema (PRD §7.1.1).
-- All local state lives here except attachment binaries (on disk; paths stored).
-- Applied idempotently by the migrations runner; schema_version tracks forward
-- migrations.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS conversations (
  id         TEXT PRIMARY KEY,
  title      TEXT NOT NULL,
  model_id   TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_conversations_updated ON conversations(updated_at DESC);

CREATE TABLE IF NOT EXISTS messages (
  id               TEXT PRIMARY KEY,
  conversation_id  TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  role             TEXT NOT NULL,          -- 'user' | 'assistant'
  content          TEXT NOT NULL,
  model_name       TEXT,                   -- assistant turns: model used
  model_provenance TEXT,                   -- 'local' | 'cloud'
  steps_json       TEXT,                   -- serialized agent-run timeline (CHT-9)
  stop_reason      TEXT,                   -- HRN-3: completed|aborted|timeout|max_steps|error|interrupted (VTN-6: a spoken reply the user cut off)
  spoken           INTEGER NOT NULL DEFAULT 0, -- VTN-7: 1 for a spoken turn
  created_at       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(conversation_id, created_at);

CREATE TABLE IF NOT EXISTS attachments (
  id         TEXT PRIMARY KEY,
  message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL,                -- 'image' | 'pdf'
  name       TEXT NOT NULL,
  path       TEXT NOT NULL                 -- on-disk path (binary not in DB)
);

-- Model-produced artifacts (CHT-6): titled HTML/SVG/markdown/code the assistant
-- emits via the `create_artifact` tool, rendered in the Canvas side panel, plus
-- generated media (Phase 13). Text kinds keep their text in `content`; media
-- kinds keep a path to the file, which lives under `generated_media_dir()` —
-- the binary itself is never in the DB.
CREATE TABLE IF NOT EXISTS artifacts (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
  title           TEXT NOT NULL,
  kind            TEXT NOT NULL,             -- 'html' | 'svg' | 'markdown' | 'code' | 'image' | 'video'
  content         TEXT NOT NULL,             -- text, or a file path for media
  created_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_artifacts_conv ON artifacts(conversation_id, created_at);

-- Media generation runs as a background job (`JOB-1`) rather than holding the
-- agent loop: a video takes 30-180s and cloud latency makes even an image rude
-- to block on. A row is written at submit and updated once, at completion.
-- Anything still 'running' at startup was interrupted by a restart and is
-- marked failed — a job is never left spinning across a launch.
CREATE TABLE IF NOT EXISTS media_jobs (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
  -- The assistant turn this belongs to, so a result that arrives after the run
  -- has ended still lands in the turn that asked for it.
  message_id      TEXT,
  modality        TEXT NOT NULL,             -- 'image' | 'video'
  status          TEXT NOT NULL,             -- 'running' | 'done' | 'failed' | 'cancelled'
  prompt          TEXT NOT NULL,
  model_id        TEXT,
  aspect_ratio    TEXT,
  started_at      INTEGER NOT NULL,
  finished_at     INTEGER,
  artifact_id     TEXT,
  error           TEXT
);
CREATE INDEX IF NOT EXISTS idx_media_jobs_conv ON media_jobs(conversation_id, started_at);

-- Typed, interactive workspace blocks (Generative UI): comparison tables,
-- checklists, forms, progress meters, collections. Rendered inline in the
-- assistant turn. `data_json` is the model payload; `state_json` is user
-- interaction state (pins, filters, checks, form values).
CREATE TABLE IF NOT EXISTS blocks (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  message_id      TEXT,                     -- anchor assistant message (nullable)
  kind            TEXT NOT NULL,            -- comparison|collection|plan|form|progress|document
  title           TEXT NOT NULL,
  data_json       TEXT NOT NULL,
  state_json      TEXT,
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_blocks_conv ON blocks(conversation_id, created_at);

CREATE TABLE IF NOT EXISTS model_library (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  path       TEXT NOT NULL,
  quant      TEXT,
  size_bytes INTEGER,
  vision     INTEGER NOT NULL DEFAULT 0,
  is_default INTEGER NOT NULL DEFAULT 0,
  added_at   INTEGER NOT NULL
);

-- Saved personas (CHT-4): a named bundle of system prompt + optional pinned
-- model + optional sampling params. A conversation may reference one (see the
-- `persona_id` column added to `conversations` by migration v2), and may also
-- carry one-off `overrides_json` (CHT-7).
CREATE TABLE IF NOT EXISTS personas (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  system_prompt TEXT NOT NULL,
  model_id      TEXT,                       -- optional pinned model (local id or cloud "provider:model")
  params_json   TEXT,                       -- optional sampling params {temperature, ...}
  is_default    INTEGER NOT NULL DEFAULT 0,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS connectors (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  url         TEXT,
  transport   TEXT NOT NULL,               -- 'http-sse' | 'stdio'
  enabled     INTEGER NOT NULL DEFAULT 1,
  config_json TEXT,
  created_at  INTEGER NOT NULL
);

-- What a connector's tools may do without asking (`MCG-5`). One row per
-- (connector, app); a connector that is just an MCP server uses app = ''. A
-- missing row means the defaults below: read runs, change asks, send asks.
CREATE TABLE IF NOT EXISTS connector_policy (
  connector_id   TEXT NOT NULL,
  app            TEXT NOT NULL DEFAULT '',
  read_policy    TEXT NOT NULL DEFAULT 'run',   -- 'run' | 'ask'
  change_policy  TEXT NOT NULL DEFAULT 'ask',   -- 'ask' | 'allow'
  send_policy    TEXT NOT NULL DEFAULT 'ask',   -- 'ask' | 'allow'
  send_cap       INTEGER NOT NULL DEFAULT 25,
  tool_overrides TEXT,                          -- JSON {tool_name: 'read'|'change'|'send'} (MCG-7)
  updated_at     INTEGER NOT NULL,
  PRIMARY KEY (connector_id, app)
);

-- How many sends ran per connector per day (`MCG-4`), whoever approved them.
CREATE TABLE IF NOT EXISTS app_send_log (
  connector_id TEXT NOT NULL,
  app          TEXT NOT NULL DEFAULT '',
  day          TEXT NOT NULL,                   -- YYYY-MM-DD
  count        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (connector_id, app, day)
);

-- `CAP-1`: a mirror of the accounts Composio holds for this install, so the
-- Apps view and the registry need no network. Composio is the truth and this is
-- refreshed from it; it holds no token, only the person's own label for each.
CREATE TABLE IF NOT EXISTS app_accounts (
  id           TEXT PRIMARY KEY,                -- Composio connected-account id
  connector_id TEXT NOT NULL,
  app          TEXT NOT NULL,                   -- toolkit slug, as Composio spells it
  label        TEXT,                            -- the person's alias: 'work', 'personal'
  status       TEXT NOT NULL,                   -- ACTIVE | INITIATED | EXPIRED | FAILED ...
  updated_at   INTEGER NOT NULL
);

-- `CAP-10`: which apps a persona may use. No rows for a persona means every
-- connected app. A row with tools '[]' and app '' is the explicit "none".
CREATE TABLE IF NOT EXISTS persona_app_grants (
  persona_id   TEXT NOT NULL,
  connector_id TEXT NOT NULL,
  app          TEXT NOT NULL DEFAULT '',        -- '' = whole connector
  tools        TEXT NOT NULL DEFAULT '*',       -- '*' | JSON array of exact tool slugs | '[]'
  PRIMARY KEY (persona_id, connector_id, app)
);

-- `CAP-12`: every app tool that ran through Composio. No cost column: Composio
-- bills the person's own plan and Poiesis does not know their rate.
CREATE TABLE IF NOT EXISTS app_call_log (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  app        TEXT NOT NULL,
  kind       TEXT NOT NULL,                     -- read | change | send
  tool       TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_app_call_log_created ON app_call_log(created_at);

-- A user's own OpenAI-compatible model server (Ollama, LM Studio, or a remote
-- box) — a third model source alongside the integrated runtime and BYOK cloud
-- providers. The API key, if the server even needs one, is **not** stored
-- here — it lives in the OS credential store (`secrets::SERVICE_ENDPOINT`,
-- account = this row's `id`); only connection metadata lives in SQLite.
CREATE TABLE IF NOT EXISTS local_endpoints (
  id         TEXT PRIMARY KEY,
  label      TEXT NOT NULL,
  base_url   TEXT NOT NULL,               -- normalized: no trailing slash, no /v1
  kind       TEXT NOT NULL DEFAULT 'openai',
  ctx_size   INTEGER NOT NULL DEFAULT 8192,
  enabled    INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL
);

-- External agent apps (Claude Code, Codex, ACP agents) that can be handed work
-- (`CLI-1`, `CLI-12`). One row per agent Poiesis has found or been told about.
-- Finding one is not consent: `enabled` starts at 0. Nothing secret lives here:
-- an API key for `payment = 'api-key'` is in the OS credential store
-- (`secrets::SERVICE_ENGINES`, account = this row's `id`), and `env_json` holds
-- non-secret variables only.
CREATE TABLE IF NOT EXISTS engines (
  id            TEXT PRIMARY KEY,          -- 'claude-code', 'codex', 'acp:opencode', 'acp:custom-<uuid>'
  kind          TEXT NOT NULL,             -- 'claude-code' | 'codex' | 'acp'
  display_name  TEXT NOT NULL,
  program       TEXT,                      -- the path the user set; NULL = auto-detect
  found_path    TEXT,                      -- what the last search resolved; NULL = not found
  args_json     TEXT NOT NULL DEFAULT '[]',-- argv a custom ACP agent starts with
  env_json      TEXT NOT NULL DEFAULT '{}',
  enabled       INTEGER NOT NULL DEFAULT 0,
  version       TEXT,                      -- from the last probe
  signed_in     INTEGER,                   -- NULL unknown, 0/1
  paid_by       TEXT,                      -- 'plan' | 'api-key' | 'own' | NULL
  last_probe_at INTEGER,
  last_error    TEXT,
  payment       TEXT NOT NULL DEFAULT 'plan',    -- 'plan' | 'api-key'
  trust_ceiling TEXT NOT NULL DEFAULT 'confirm', -- 'read-only' | 'confirm' | 'auto'
  headless      INTEGER NOT NULL DEFAULT 0,
  memory_tools  INTEGER NOT NULL DEFAULT 0,
  app_tools     INTEGER NOT NULL DEFAULT 0,      -- may use my connected apps through Poiesis (`CAP-14`)
  model         TEXT,                      -- NULL = the agent's own default
  extra_args    TEXT NOT NULL DEFAULT '[]',
  use_mode      TEXT NOT NULL DEFAULT 'suggest', -- 'on_request' | 'suggest' | 'auto' (`RTE-1`)
  guards_json   TEXT NOT NULL DEFAULT '{}',      -- hard conditions checked in Rust (`RTE-3`)
  rules_seeded  INTEGER NOT NULL DEFAULT 0,      -- the default rules were written once (`RTE-2`)
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

-- The user's own words for when to hand work to an agent (`RTE-2`). The lead
-- reads them verbatim, the user edits them, and every handover cites one.
CREATE TABLE IF NOT EXISTS engine_rules (
  id          TEXT PRIMARY KEY,
  engine_id   TEXT NOT NULL REFERENCES engines(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL,                 -- 'use' | 'avoid'
  text        TEXT NOT NULL,                 -- plain language, at most 140 characters
  enabled     INTEGER NOT NULL DEFAULT 1,
  auto        INTEGER NOT NULL DEFAULT 0,    -- "Always for this": no handover card for this rule
  position    INTEGER NOT NULL,
  source      TEXT NOT NULL,                 -- 'default' | 'user' | 'proposed'
  hits        INTEGER NOT NULL DEFAULT 0,    -- handovers that cited it
  overrides   INTEGER NOT NULL DEFAULT 0,    -- times the user said no to one that cited it
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS engine_rules_engine ON engine_rules(engine_id, position);

-- What teaches the rules (`RTE-6`): every time the user said no to a handover that
-- cited a rule (`override`), named an agent themselves (`asked`), or a pattern was
-- looked at and left alone (`checked`). Small and local: the task is clipped.
CREATE TABLE IF NOT EXISTS engine_signals (
  id         TEXT PRIMARY KEY,
  engine_id  TEXT NOT NULL,
  rule_id    TEXT,
  kind       TEXT NOT NULL,                  -- 'override' | 'asked' | 'checked'
  task       TEXT NOT NULL DEFAULT '',
  at         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS engine_signals_engine ON engine_signals(engine_id, kind, at);

-- Granted file-access folders (§6.1). Each carries a mode; revocable in Settings.
CREATE TABLE IF NOT EXISTS permissions (
  id         TEXT PRIMARY KEY,
  path       TEXT NOT NULL UNIQUE,
  mode       TEXT NOT NULL,                -- 'read' | 'read-write'
  created_at INTEGER NOT NULL
);

-- Visible activity log of everything the agent did (§6.1, §6.3).
CREATE TABLE IF NOT EXISTS activity_log (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT,
  kind            TEXT NOT NULL,           -- 'file' | 'web' | 'code' | 'mcp' | 'network'
  detail          TEXT NOT NULL,
  created_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_activity_created ON activity_log(created_at DESC);

-- Full-text index over message content for conversation search (CHT-3, §7.1.1).
-- External-content FTS5 table mirrored from `messages` via triggers.
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
  content,
  content='messages',
  content_rowid='rowid'
);

CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
END;
CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
  INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
END;

-- Poiesis: agent-proposed self-changes awaiting user approval (SOUL-2, RCP-2).
CREATE TABLE IF NOT EXISTS change_proposals (
  id            TEXT PRIMARY KEY,
  target        TEXT NOT NULL,            -- soul|lesson|lesson-critic|skill|skill-revision|email|recipe(legacy)
  persona_id    TEXT,                     -- when target='persona' (future)
  slug          TEXT,                     -- when target='recipe'|'lesson': the entry name
  proposed_text TEXT NOT NULL,            -- full replacement/new text for the target
  rationale     TEXT NOT NULL,            -- why it is being proposed; shown to the user, never stored on the entry
  description   TEXT,                     -- the entry's own one-line summary, kept if applied (CRT-2)
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

-- Working folder: undo trash. Every destructive file operation the agent (or the
-- user, via the Workbench) performs inside the attached folder snapshots the
-- prior bytes to <app_data>/trash/<uuid> and records a row here, so it can be
-- reversed. `blob_path` is NULL when the file did not exist before — undoing
-- such an entry deletes the created file.
CREATE TABLE IF NOT EXISTS file_trash (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL,
  op              TEXT NOT NULL,           -- 'write' | 'edit' | 'delete' | 'move' | 'save'
  path            TEXT NOT NULL,           -- affected path (destination, for a move)
  prev_path       TEXT,                    -- move source, else NULL
  blob_path       TEXT,                    -- snapshot of prior bytes, NULL if newly created
  created_at      INTEGER NOT NULL,
  undone          INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_file_trash_conv ON file_trash(conversation_id, created_at DESC);

-- Perception (Phase 12): one vector store for durable memory and indexed files.
-- `vec` is a little-endian f32 array, pre-normalised to unit length, so
-- similarity is a dot product. `model`/`dim` are recorded per row: a change
-- of embedding model discards and rebuilds rather than migrating (VEC-4).
CREATE TABLE IF NOT EXISTS vectors (
  id         TEXT PRIMARY KEY,
  owner_kind TEXT NOT NULL,           -- 'memory' | 'file'
  scope_key  TEXT NOT NULL,           -- memory: collection; file: canonical index root
  ref_key    TEXT NOT NULL,           -- memory: entry slug; file: absolute path
  chunk_ix   INTEGER NOT NULL DEFAULT 0,
  text       TEXT NOT NULL,           -- the chunk (or the memory line) that was embedded
  model      TEXT NOT NULL,
  dim        INTEGER NOT NULL,
  vec        BLOB NOT NULL,
  mtime      INTEGER,                 -- files only: source mtime, for incremental reindex
  created_at INTEGER NOT NULL
);
-- One row per (scope, entry, chunk): re-embedding overwrites in place, so a
-- caller that forgets to delete first can't silently double a fact's chunks.
-- Its leading columns also serve the scope scan, which is why there is no
-- separate idx_vectors_scope (dropped below for databases that had one).
CREATE UNIQUE INDEX IF NOT EXISTS idx_vectors_chunk
  ON vectors(owner_kind, scope_key, ref_key, chunk_ix);
DROP INDEX IF EXISTS idx_vectors_scope;
CREATE INDEX IF NOT EXISTS idx_vectors_ref ON vectors(ref_key);

-- `PRJ-1`: a project is a named group of sessions that share a context. A
-- working directory is something it may have, not what it is (`PRJ-1a`) — a
-- project about a book or a job is as real as one about a repository.
--
-- `root_path` is canonical and UNIQUE, matching how `index_roots` keys the
-- same folders, so attaching a known folder joins rather than duplicates. It
-- is nullable, and SQLite already treats NULLs as distinct under a unique
-- index, so any number of folderless projects coexist.
--
-- `trust` moves off the conversation and onto the project, because it is a
-- property of the folder and always was. `conversations.folder_path` /
-- `folder_trust` stay in place as the fallback for a chat with no project;
-- `Db::conversation_folder` resolves the pair.
--
-- `card_json` (`COD-1`), `exec_policy` (`COD-7`) and `tabs_json`
-- (`SHELL_PLAN`'s `SHL-17` scope seam) are declared here and left unread until
-- the phases that own them. One migration is cheaper than three.
CREATE TABLE IF NOT EXISTS projects (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  root_path     TEXT UNIQUE,                      -- canonical; NULL = no folder
  instructions  TEXT,                             -- PRJ-7: shared by its sessions
  trust         TEXT NOT NULL DEFAULT 'confirm',  -- read-only | confirm | auto
  exec_policy   TEXT NOT NULL DEFAULT 'inherit',  -- off | ask | allow | inherit
  card_json     TEXT,
  card_built_at INTEGER,
  allow_json    TEXT,                             -- COD-7: always-allowed tasks/commands
  tabs_json     TEXT,
  archived      INTEGER NOT NULL DEFAULT 0,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

-- Perception: one row per indexed folder root — what built it, and when.
CREATE TABLE IF NOT EXISTS index_roots (
  path        TEXT PRIMARY KEY,       -- canonical folder path
  model       TEXT NOT NULL,
  dim         INTEGER NOT NULL,
  file_count  INTEGER NOT NULL DEFAULT 0,
  chunk_count INTEGER NOT NULL DEFAULT 0,
  skipped     TEXT,                   -- JSON: [{path, reason}] surfaced in IDX-UI-2
  state       TEXT NOT NULL DEFAULT 'idle',  -- idle|building|stale|error
  updated_at  INTEGER NOT NULL
);

-- SEM-UI-4: when a fact/lesson/recipe last actually reached a prompt (either
-- wholesale-injected or retrieved by `recall_for`) — "last surfaced", shown
-- on its card. Deliberately not in the entry's own frontmatter: this is
-- derived usage, not something the user wrote or owns.
CREATE TABLE IF NOT EXISTS memory_usage (
  collection    TEXT NOT NULL,
  ref_key       TEXT NOT NULL,
  last_used_at  INTEGER NOT NULL,
  PRIMARY KEY (collection, ref_key)
);

-- Perception (PHS-1): cached 64-bit dHash per image, keyed by path + mtime —
-- an unchanged file never gets rehashed on a repeat scan.
CREATE TABLE IF NOT EXISTS image_hashes (
  path       TEXT PRIMARY KEY,
  mtime      INTEGER NOT NULL,
  hash       INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- Capabilities (MAIL-1): mail accounts. The password/token NEVER lands here —
-- only in the OS credential store under service `secrets::SERVICE_MAIL`,
-- account = this row's id.
CREATE TABLE IF NOT EXISTS mail_accounts (
  id            TEXT PRIMARY KEY,
  label         TEXT NOT NULL,
  email         TEXT NOT NULL,
  imap_host     TEXT NOT NULL,
  imap_port     INTEGER NOT NULL DEFAULT 993,
  smtp_host     TEXT NOT NULL,
  smtp_port     INTEGER NOT NULL DEFAULT 465,
  username      TEXT NOT NULL,
  auth          TEXT NOT NULL DEFAULT 'password',
  -- 'tls' (implicit, ports 993/465) | 'starttls' (upgrade, ports 143/587 and
  -- local bridges). Getting this wrong is a hung handshake, not a fallback.
  security      TEXT NOT NULL DEFAULT 'tls',
  enabled       INTEGER NOT NULL DEFAULT 1,
  created_at    INTEGER NOT NULL
);

-- Harness (FIX-1): a tool call that failed and was then corrected by a later
-- call to the *same* tool in the *same* run — the most precise self-teaching
-- signal the agent produces. Unlike `tool_stats` this holds content
-- (arguments, error text), so it is pruned aggressively and never leaves the
-- machine.
CREATE TABLE IF NOT EXISTS tool_fixes (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL,
  tool_name       TEXT NOT NULL,
  failed_args     TEXT NOT NULL,
  error           TEXT NOT NULL,
  fixed_args      TEXT NOT NULL,
  created_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tool_fixes_conv ON tool_fixes(conversation_id);

-- "Always allow" answers to a capability consent prompt that isn't filesystem
-- access (BRW-3 domains, SYS-1 app launches) — the `permissions` table above
-- is path+mode shaped and doesn't fit either. Revocable the same way: delete
-- the row.
CREATE TABLE IF NOT EXISTS capability_grants (
  id         TEXT PRIMARY KEY,
  kind       TEXT NOT NULL,   -- 'domain' | 'open-app'
  value      TEXT NOT NULL,   -- registrable domain, or app name
  created_at INTEGER NOT NULL,
  UNIQUE(kind, value)
);

-- One activation of a skill, and how the conversation went afterwards (OUT-1).
-- We deliberately don't track this in a sidecar file inside the skill folder —
-- a skill folder must stay pristine and portable, especially one living in the
-- user's own skills folder.
CREATE TABLE IF NOT EXISTS skill_runs (
  id              TEXT PRIMARY KEY,
  skill_name      TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  tool_failures   INTEGER NOT NULL DEFAULT 0,  -- after activation, this conv
  corrected       INTEGER NOT NULL DEFAULT 0,  -- a lesson cited this conv
  created_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_skill_runs_name ON skill_runs(skill_name);
CREATE INDEX IF NOT EXISTS idx_skill_runs_conv ON skill_runs(conversation_id);

-- The last browsing this conversation did (`BRW-UI-1`). The live session lives
-- in the in-memory `BrowserPool`; this is the *record* of it, so re-opening a
-- chat still shows where the agent went. Without it the panel vanished on
-- restart while the transcript still showed the visits, which read as the app
-- having lost something.
--
-- One row per conversation: the panel shows the latest page and its trail, not
-- a history of every session.
CREATE TABLE IF NOT EXISTS browser_sessions (
  conversation_id TEXT PRIMARY KEY,
  domain          TEXT NOT NULL,
  title           TEXT NOT NULL,
  screenshot      TEXT,           -- path; may be gone, checked on read
  trail_json      TEXT NOT NULL,  -- ["visited x", "clicked \"Sign in\""]
  updated_at      INTEGER NOT NULL
);

-- One delegated child run (`SUB-3`). The child's *work* lives in its own
-- conversation, exactly like any other run; this row is the link back to the
-- turn that asked for it, and the record of how it ended — so the Fleet card
-- and the Agents tab still have something to show after a restart, when the
-- in-memory `Fleet` handle is long gone.
CREATE TABLE IF NOT EXISTS subagent_runs (
  id                     TEXT PRIMARY KEY,   -- the run id
  parent_conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  parent_message_id      TEXT,
  child_conversation_id  TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  agent                  TEXT NOT NULL,
  task                   TEXT NOT NULL,
  status                 TEXT NOT NULL,      -- queued|running|done|stopped|error
  stop_reason            TEXT,               -- completed|aborted|timeout|max_steps|error
  result                 TEXT,
  steps                  INTEGER NOT NULL DEFAULT 0,
  started_at             INTEGER NOT NULL,
  ended_at               INTEGER,
  engine_id              TEXT,               -- `CLI-4`: set when an agent app did the work
  because                TEXT,               -- the rule label it cited, or 'asked'
  paid_by                TEXT                -- 'plan' | 'api-key' | 'own', once known
);
CREATE INDEX IF NOT EXISTS idx_subagent_runs_parent ON subagent_runs(parent_conversation_id);

-- What one run cost (`OBS-1`/`OBS-2`). One row per run that reported anything,
-- written when the run ends. Local runs land here too with zero cost: context
-- pressure is worth seeing even when the tokens are free.
--
-- No foreign key to `conversations`: usage is the record of what was spent, and
-- deleting a conversation should not quietly erase a month's bill. The
-- conversation id is kept so a per-conversation breakdown is still possible for
-- the ones that still exist.
CREATE TABLE IF NOT EXISTS run_usage (
  run_id          TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  model_name      TEXT NOT NULL,
  provenance      TEXT NOT NULL,   -- local|cloud|endpoint
  prompt_tokens   INTEGER NOT NULL,
  output_tokens   INTEGER NOT NULL,
  turns           INTEGER NOT NULL,
  created_at      INTEGER NOT NULL,
  -- `CLI-UI-7`: set on a job done by an agent app (provenance `agent-app`, the
  -- app's name in `model_name`). Who paid, what the app says it would have cost
  -- at list prices, and how long it worked.
  paid_by         TEXT,
  notional_usd    REAL,
  duration_ms     INTEGER
);
CREATE INDEX IF NOT EXISTS idx_run_usage_created ON run_usage(created_at);

-- `CTX-2`: the model's view of a conversation, append-only.
--
-- The rule: anything the model saw must be reconstructible from these rows.
-- `messages` stays exactly what it was and remains what the UI reads; this is
-- a different shape on purpose, because a person's transcript and a model's
-- transcript are different things and forcing one table to be both is what
-- split prompt assembly across two languages in the first place.
--
-- `seq` is per conversation and monotonic, so "fork at this point" is a range
-- and not a guess. `run_id` groups the rows one run sent, which is what resume
-- replays.
CREATE TABLE IF NOT EXISTS session_events (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  run_id          TEXT,
  seq             INTEGER NOT NULL,   -- per conversation, monotonic
  kind            TEXT NOT NULL,      -- prompt|user|assistant|tool_call|tool_result|system_note|summary|steer|stop
  payload_json    TEXT NOT NULL,
  created_at      INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_session_events_seq ON session_events(conversation_id, seq);
CREATE INDEX IF NOT EXISTS idx_session_events_run ON session_events(run_id);

-- `CTL-3`: every decision the small model made, whether or not a conversation
-- asked for it, so what it said can be set beside what happened next. The state
-- is not kept, only a short preview and a hash. A conversation's rows go with it.
CREATE TABLE IF NOT EXISTS decisions (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
  run_id          TEXT,
  purpose         TEXT NOT NULL,
  backend         TEXT NOT NULL,           -- 'local:<model name>'
  model           TEXT NOT NULL,           -- the file stem: the key a calibration is fitted under
  ms              INTEGER NOT NULL,
  calibrated      INTEGER NOT NULL DEFAULT 0,
  answers_json    TEXT NOT NULL,           -- each answer with its probabilities; '{}' when nothing was answered
  input_tokens    INTEGER NOT NULL DEFAULT 0,
  state_preview   TEXT NOT NULL DEFAULT '',
  state_hash      TEXT NOT NULL DEFAULT '',
  action          TEXT,                    -- what the controller did with it (`CTL-2`)
  at              INTEGER NOT NULL,
  cost_usd        REAL,                    -- what the cloud charged (v40); NULL for a local decision
  note            TEXT                     -- v41 (`SEE-2`): what it changed, in words, when it acted or was shown
);
CREATE INDEX IF NOT EXISTS idx_decisions_at ON decisions(at);
CREATE INDEX IF NOT EXISTS idx_decisions_conv ON decisions(conversation_id, purpose, at);

-- What the user and the runtime did afterwards (`CTL-3`). Written only for
-- labels nothing else records; the ones that live in `engine_signals` and the
-- connector tables are read from there when calibration runs.
CREATE TABLE IF NOT EXISTS decision_outcomes (
  id          TEXT PRIMARY KEY,
  decision_id TEXT NOT NULL REFERENCES decisions(id) ON DELETE CASCADE,
  label       TEXT NOT NULL,               -- right | wrong | weak_right | weak_wrong | corrected
  value       TEXT,                        -- the right answer, for 'corrected'
  source      TEXT NOT NULL,               -- which observation wrote it
  at          INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_decision_outcomes_one ON decision_outcomes(decision_id, source);

-- The inbox of `mail-triage` (`USE-6a`, schema v38): one row per message seen in a
-- mail account's INBOX, with what the decision model said about it. Never the
-- body: a body is read for a check and dropped. Rows go after 30 days.
CREATE TABLE IF NOT EXISTS mail_seen (
  account_id  TEXT NOT NULL REFERENCES mail_accounts(id) ON DELETE CASCADE,
  uid         INTEGER NOT NULL,
  message_id  TEXT,
  from_addr   TEXT NOT NULL DEFAULT '',
  subject     TEXT NOT NULL DEFAULT '',
  date        TEXT NOT NULL DEFAULT '',   -- as the server wrote it
  sent_at     INTEGER,                    -- that date in ms, when it could be read
  unread      INTEGER NOT NULL DEFAULT 0,
  triage_json TEXT,                       -- null until a check ran
  triaged_at  INTEGER,
  fetched_at  INTEGER NOT NULL,
  PRIMARY KEY (account_id, uid)
);
CREATE INDEX IF NOT EXISTS idx_mail_seen_fetched ON mail_seen(fetched_at);

-- The result of checking one assistant message against evidence (`verify`,
-- `USE-10`, schema v39): its claims with their verdicts and passages, as JSON. One
-- row a message; checking again replaces it. It goes with the message.
CREATE TABLE IF NOT EXISTS verifications (
  message_id      TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  claims_json     TEXT NOT NULL,
  note            TEXT,                       -- what the check could not do, in words
  skipped         INTEGER NOT NULL DEFAULT 0, -- opinions left out
  created_at      INTEGER NOT NULL,
  mode            TEXT NOT NULL DEFAULT 'deep' -- v41 (`SEE-1`): 'auto' (checked as it streamed) or 'deep' (Check more deeply)
);
CREATE INDEX IF NOT EXISTS idx_verifications_conv ON verifications(conversation_id);
