-- Project Nexus — SQLite schema (PRD §7.1.1).
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
-- emits via the `create_artifact` tool, rendered in the Canvas side panel. Text
-- content lives here; nothing binary.
CREATE TABLE IF NOT EXISTS artifacts (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
  title           TEXT NOT NULL,
  kind            TEXT NOT NULL,             -- 'html' | 'svg' | 'markdown' | 'code'
  content         TEXT NOT NULL,
  created_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_artifacts_conv ON artifacts(conversation_id, created_at);

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
