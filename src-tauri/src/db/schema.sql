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
