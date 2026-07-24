//! SQLite persistence (PRD §7.1.1): conversations, messages, settings, the model
//! library, and connector config, plus FTS5 search over history (CHT-3).
//!
//! A single connection guarded by a mutex is sufficient for a single-user desktop
//! app and keeps the access model simple. Attachment binaries live on disk; only
//! their paths are stored here.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

const SCHEMA: &str = include_str!("schema.sql");
const SCHEMA_VERSION: i64 = 4;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct Db {
    conn: Mutex<Connection>,
}

// ---- row models (mirror the frontend `types.ts`) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub model_id: Option<String>,
    /// Persona this conversation uses (CHT-4), if any.
    pub persona_id: Option<String>,
    /// One-off overrides for this conversation (CHT-7): {system_prompt?, params?}.
    pub overrides_json: Option<String>,
    /// Workspace mode (W): the conversation is pinned to the composed-interface
    /// layout rather than the classic message stream.
    pub workspace: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A saved persona (CHT-4): a reusable bundle of system prompt + optional pinned
/// model + optional sampling params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub model_id: Option<String>,
    pub params_json: Option<String>,
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct NewPersona {
    pub name: String,
    pub system_prompt: String,
    pub model_id: Option<String>,
    pub params_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub model_name: Option<String>,
    pub model_provenance: Option<String>,
    /// Agent-run timeline, serialized JSON (CHT-9).
    pub steps_json: Option<String>,
    pub created_at: i64,
    /// Attachments on this turn (CHT-5). Populated by `list_messages`.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

/// A persisted attachment reference (the binary stays on disk; only metadata +
/// path live in SQLite).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewAttachment {
    pub kind: String,
    pub name: String,
    pub path: String,
}

/// Input for appending a message (id/created_at assigned by the DB layer).
#[derive(Debug, Deserialize)]
pub struct NewMessage {
    pub role: String,
    pub content: String,
    pub model_name: Option<String>,
    pub model_provenance: Option<String>,
    pub steps_json: Option<String>,
    #[serde(default)]
    pub attachments: Vec<NewAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub quant: Option<String>,
    pub size_bytes: Option<i64>,
    pub vision: bool,
    pub is_default: bool,
    pub added_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    pub path: String,
    pub mode: String,
    pub created_at: i64,
}

/// A model-produced artifact (CHT-6) rendered in the Canvas panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub conversation_id: Option<String>,
    pub title: String,
    pub kind: String,
    pub content: String,
    pub created_at: i64,
}

/// A typed, interactive workspace block (Generative UI) rendered inline in an
/// assistant turn. `data_json` is the model-provided payload; `state_json` holds
/// user interaction state (pins, filters, checks, form values).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub data_json: String,
    pub state_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub id: String,
    pub conversation_id: Option<String>,
    pub kind: String,
    pub detail: String,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct NewModelEntry {
    pub name: String,
    pub path: String,
    pub quant: Option<String>,
    pub size_bytes: Option<i64>,
    pub vision: bool,
}

/// A configured MCP connector (MCP-1, MCP-3). The auth token is **not** stored
/// here — it lives in the OS credential store; only `config_json` (cached tool
/// list, last-checked) and metadata live in SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connector {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub transport: String,
    pub enabled: bool,
    pub config_json: Option<String>,
    pub created_at: i64,
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl Db {
    /// Open (creating if needed) the database at `path` and run migrations.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Db {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Db {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(SCHEMA)?;
        let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if current < 2 {
            // v2 (CHT-4/CHT-7): personas + per-conversation persona link and
            // one-off overrides. The `personas` table is created by SCHEMA above;
            // these add the linking columns to the existing `conversations` table.
            Self::add_column(&conn, "conversations", "persona_id", "TEXT")?;
            Self::add_column(&conn, "conversations", "overrides_json", "TEXT")?;
        }
        if current < 3 {
            // v3 (Generative UI): durable per-conversation session state. The
            // `blocks` table is created by SCHEMA above; this adds the state
            // column to the existing `conversations` table.
            Self::add_column(&conn, "conversations", "session_state_json", "TEXT")?;
        }
        if current < 4 {
            // v4 (Workspace mode): a conversation started in workspace mode is
            // pinned to it — the composed interface, not the message stream, is
            // its primary surface. 0 = classic chat, 1 = workspace.
            Self::add_column(&conn, "conversations", "workspace", "INTEGER NOT NULL DEFAULT 0")?;
        }
        if current < SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }

    /// Add a column if it isn't already present (idempotent forward migration).
    fn add_column(conn: &Connection, table: &str, column: &str, decl: &str) -> Result<(), DbError> {
        let exists = {
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
            let names: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            names.iter().any(|n| n == column)
        };
        if !exists {
            conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"), [])?;
        }
        Ok(())
    }

    // ---- settings ----

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .ok();
        Ok(value)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---- conversations ----

    pub fn create_conversation(
        &self,
        title: &str,
        model_id: Option<&str>,
        workspace: bool,
    ) -> Result<Conversation, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO conversations(id, title, model_id, workspace, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, title, model_id, workspace as i64, ts],
        )?;
        Ok(Conversation {
            id,
            title: title.to_string(),
            model_id: model_id.map(|s| s.to_string()),
            persona_id: None,
            overrides_json: None,
            workspace,
            created_at: ts,
            updated_at: ts,
        })
    }

    /// Pin (or unpin) a conversation to workspace mode.
    pub fn set_conversation_workspace(&self, id: &str, workspace: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET workspace = ?2 WHERE id = ?1",
            params![id, workspace as i64],
        )?;
        Ok(())
    }

    pub fn list_conversations(&self) -> Result<Vec<Conversation>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, model_id, persona_id, overrides_json, workspace, created_at, updated_at
             FROM conversations ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map([], Self::map_conversation)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn rename_conversation(&self, id: &str, title: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, title, now_ms()],
        )?;
        Ok(())
    }

    pub fn delete_conversation(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
        Ok(())
    }

    fn touch_conversation(conn: &Connection, id: &str) -> Result<(), DbError> {
        conn.execute(
            "UPDATE conversations SET updated_at = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
        Ok(())
    }

    // ---- messages ----

    pub fn append_message(&self, conversation_id: &str, msg: &NewMessage) -> Result<Message, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO messages(id, conversation_id, role, content, model_name, model_provenance, steps_json, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                conversation_id,
                msg.role,
                msg.content,
                msg.model_name,
                msg.model_provenance,
                msg.steps_json,
                ts
            ],
        )?;
        let mut saved = Vec::with_capacity(msg.attachments.len());
        for a in &msg.attachments {
            let aid = new_id();
            conn.execute(
                "INSERT INTO attachments(id, message_id, kind, name, path) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![aid, id, a.kind, a.name, a.path],
            )?;
            saved.push(Attachment {
                id: aid,
                kind: a.kind.clone(),
                name: a.name.clone(),
                path: a.path.clone(),
            });
        }
        Self::touch_conversation(&conn, conversation_id)?;
        Ok(Message {
            id,
            conversation_id: conversation_id.to_string(),
            role: msg.role.clone(),
            content: msg.content.clone(),
            model_name: msg.model_name.clone(),
            model_provenance: msg.model_provenance.clone(),
            steps_json: msg.steps_json.clone(),
            created_at: ts,
            attachments: saved,
        })
    }

    /// Update an assistant message's content + steps once streaming completes.
    pub fn finalize_message(&self, id: &str, content: &str, steps_json: Option<&str>) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET content = ?2, steps_json = ?3 WHERE id = ?1",
            params![id, content, steps_json],
        )?;
        Ok(())
    }

    pub fn list_messages(&self, conversation_id: &str) -> Result<Vec<Message>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content, model_name, model_provenance, steps_json, created_at
             FROM messages WHERE conversation_id = ?1 ORDER BY created_at ASC",
        )?;
        let mut rows = stmt
            .query_map([conversation_id], Self::map_message)?
            .collect::<Result<Vec<_>, _>>()?;

        // Attach this conversation's attachments to their messages (CHT-5).
        let mut astmt = conn.prepare(
            "SELECT a.id, a.message_id, a.kind, a.name, a.path
             FROM attachments a JOIN messages m ON m.id = a.message_id
             WHERE m.conversation_id = ?1",
        )?;
        let attachments = astmt
            .query_map([conversation_id], |r| {
                Ok((r.get::<_, String>(1)?, Attachment {
                    id: r.get(0)?,
                    kind: r.get(2)?,
                    name: r.get(3)?,
                    path: r.get(4)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (message_id, att) in attachments {
            if let Some(m) = rows.iter_mut().find(|m| m.id == message_id) {
                m.attachments.push(att);
            }
        }
        Ok(rows)
    }

    /// Full-text search returning matching conversations, most-recent first (CHT-3).
    pub fn search_conversations(&self, query: &str) -> Result<Vec<Conversation>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT c.id, c.title, c.model_id, c.persona_id, c.overrides_json, c.workspace, c.created_at, c.updated_at
             FROM conversations c
             JOIN messages m ON m.conversation_id = c.id
             JOIN messages_fts f ON f.rowid = m.rowid
             WHERE messages_fts MATCH ?1
             ORDER BY c.updated_at DESC",
        )?;
        let rows = stmt
            .query_map([query], Self::map_conversation)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- model library (MKT-5) ----

    pub fn add_model(&self, m: &NewModelEntry) -> Result<ModelEntry, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        // First model added becomes the default.
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM model_library", [], |r| r.get(0))?;
        let is_default = count == 0;
        conn.execute(
            "INSERT INTO model_library(id, name, path, quant, size_bytes, vision, is_default, added_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, m.name, m.path, m.quant, m.size_bytes, m.vision as i64, is_default as i64, ts],
        )?;
        Ok(ModelEntry {
            id,
            name: m.name.clone(),
            path: m.path.clone(),
            quant: m.quant.clone(),
            size_bytes: m.size_bytes,
            vision: m.vision,
            is_default,
            added_at: ts,
        })
    }

    pub fn list_models(&self) -> Result<Vec<ModelEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, quant, size_bytes, vision, is_default, added_at
             FROM model_library ORDER BY added_at DESC",
        )?;
        let rows = stmt
            .query_map([], Self::map_model)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_model(&self, id: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let path: Option<String> = conn
            .query_row("SELECT path FROM model_library WHERE id = ?1", [id], |r| r.get(0))
            .ok();
        conn.execute("DELETE FROM model_library WHERE id = ?1", [id])?;
        Ok(path)
    }

    pub fn set_default_model(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE model_library SET is_default = 0", [])?;
        conn.execute("UPDATE model_library SET is_default = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    // Used by first-run model auto-selection in a follow-up; kept ready.
    #[allow(dead_code)]
    pub fn default_model(&self) -> Result<Option<ModelEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, name, path, quant, size_bytes, vision, is_default, added_at
                 FROM model_library WHERE is_default = 1 LIMIT 1",
                [],
                Self::map_model,
            )
            .ok();
        Ok(row)
    }

    // ---- permissions (§6.1) ----

    pub fn add_permission(&self, path: &str, mode: &str) -> Result<Grant, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO permissions(id, path, mode, created_at) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET mode = excluded.mode",
            params![id, path, mode, ts],
        )?;
        Ok(Grant {
            id,
            path: path.to_string(),
            mode: mode.to_string(),
            created_at: ts,
        })
    }

    pub fn list_permissions(&self) -> Result<Vec<Grant>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id, path, mode, created_at FROM permissions ORDER BY created_at")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Grant {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    mode: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_permission(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM permissions WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- activity log (§6.1, §6.3) ----

    pub fn log_activity(
        &self,
        conversation_id: Option<&str>,
        kind: &str,
        detail: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO activity_log(id, conversation_id, kind, detail, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![new_id(), conversation_id, kind, detail, now_ms()],
        )?;
        Ok(())
    }

    pub fn list_activity(&self, limit: i64) -> Result<Vec<ActivityEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, kind, detail, created_at
             FROM activity_log ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(ActivityEntry {
                    id: r.get(0)?,
                    conversation_id: r.get(1)?,
                    kind: r.get(2)?,
                    detail: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- MCP connectors (MCP-1, MCP-3) ----

    pub fn add_connector(
        &self,
        name: &str,
        url: &str,
        transport: &str,
        config_json: Option<&str>,
    ) -> Result<Connector, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO connectors(id, name, url, transport, enabled, config_json, created_at)
             VALUES(?1, ?2, ?3, ?4, 1, ?5, ?6)",
            params![id, name, url, transport, config_json, ts],
        )?;
        Ok(Connector {
            id,
            name: name.to_string(),
            url: Some(url.to_string()),
            transport: transport.to_string(),
            enabled: true,
            config_json: config_json.map(|s| s.to_string()),
            created_at: ts,
        })
    }

    pub fn list_connectors(&self) -> Result<Vec<Connector>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, url, transport, enabled, config_json, created_at
             FROM connectors ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], Self::map_connector)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_connector(&self, id: &str) -> Result<Option<Connector>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, name, url, transport, enabled, config_json, created_at
                 FROM connectors WHERE id = ?1",
                [id],
                Self::map_connector,
            )
            .ok();
        Ok(row)
    }

    pub fn set_connector_enabled(&self, id: &str, enabled: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE connectors SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )?;
        Ok(())
    }

    pub fn set_connector_config(&self, id: &str, config_json: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE connectors SET config_json = ?2 WHERE id = ?1",
            params![id, config_json],
        )?;
        Ok(())
    }

    pub fn delete_connector(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM connectors WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- artifacts (CHT-6) ----

    pub fn add_artifact(
        &self,
        conversation_id: Option<&str>,
        title: &str,
        kind: &str,
        content: &str,
    ) -> Result<Artifact, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO artifacts(id, conversation_id, title, kind, content, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, conversation_id, title, kind, content, ts],
        )?;
        Ok(Artifact {
            id,
            conversation_id: conversation_id.map(|s| s.to_string()),
            title: title.to_string(),
            kind: kind.to_string(),
            content: content.to_string(),
            created_at: ts,
        })
    }

    pub fn list_artifacts(&self, conversation_id: &str) -> Result<Vec<Artifact>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, title, kind, content, created_at
             FROM artifacts WHERE conversation_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| {
                Ok(Artifact {
                    id: r.get(0)?,
                    conversation_id: r.get(1)?,
                    title: r.get(2)?,
                    kind: r.get(3)?,
                    content: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list_all_artifacts(&self) -> Result<Vec<Artifact>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, title, kind, content, created_at
             FROM artifacts ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Artifact {
                    id: r.get(0)?,
                    conversation_id: r.get(1)?,
                    title: r.get(2)?,
                    kind: r.get(3)?,
                    content: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- workspace blocks (Generative UI) ----

    fn map_block(r: &rusqlite::Row) -> rusqlite::Result<Block> {
        Ok(Block {
            id: r.get(0)?,
            conversation_id: r.get(1)?,
            message_id: r.get(2)?,
            kind: r.get(3)?,
            title: r.get(4)?,
            data_json: r.get(5)?,
            state_json: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
        })
    }

    const BLOCK_COLS: &'static str =
        "id, conversation_id, message_id, kind, title, data_json, state_json, created_at, updated_at";

    pub fn add_block(
        &self,
        conversation_id: &str,
        message_id: Option<&str>,
        kind: &str,
        title: &str,
        data_json: &str,
    ) -> Result<Block, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO blocks(id, conversation_id, message_id, kind, title, data_json, state_json, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?7)",
            params![id, conversation_id, message_id, kind, title, data_json, ts],
        )?;
        Ok(Block {
            id,
            conversation_id: conversation_id.to_string(),
            message_id: message_id.map(|s| s.to_string()),
            kind: kind.to_string(),
            title: title.to_string(),
            data_json: data_json.to_string(),
            state_json: None,
            created_at: ts,
            updated_at: ts,
        })
    }

    pub fn get_block(&self, id: &str) -> Result<Option<Block>, DbError> {
        let conn = self.conn.lock().unwrap();
        let block = conn
            .query_row(
                &format!("SELECT {} FROM blocks WHERE id = ?1", Self::BLOCK_COLS),
                [id],
                Self::map_block,
            )
            .ok();
        Ok(block)
    }

    /// Find the most recent block in a conversation with the given kind + title.
    /// Used as a safety net so a model that ignores the block registry and
    /// re-presents the same block updates it rather than spawning a duplicate.
    pub fn find_block_by_title(
        &self,
        conversation_id: &str,
        kind: &str,
        title: &str,
    ) -> Result<Option<Block>, DbError> {
        let conn = self.conn.lock().unwrap();
        let block = conn
            .query_row(
                &format!(
                    "SELECT {} FROM blocks WHERE conversation_id = ?1 AND kind = ?2 AND title = ?3
                     ORDER BY created_at DESC LIMIT 1",
                    Self::BLOCK_COLS
                ),
                params![conversation_id, kind, title],
                Self::map_block,
            )
            .ok();
        Ok(block)
    }

    pub fn update_block_data(&self, id: &str, title: &str, data_json: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE blocks SET title = ?2, data_json = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, title, data_json, now_ms()],
        )?;
        Ok(())
    }

    pub fn update_block_state(&self, id: &str, state_json: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE blocks SET state_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, state_json, now_ms()],
        )?;
        Ok(())
    }

    pub fn list_blocks(&self, conversation_id: &str) -> Result<Vec<Block>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM blocks WHERE conversation_id = ?1 ORDER BY created_at ASC",
            Self::BLOCK_COLS
        ))?;
        let rows = stmt
            .query_map([conversation_id], Self::map_block)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- per-conversation session state (durable workspace memory) ----

    pub fn get_session_state(&self, conversation_id: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row(
                "SELECT session_state_json FROM conversations WHERE id = ?1",
                [conversation_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten();
        Ok(value)
    }

    pub fn set_session_state(&self, conversation_id: &str, state_json: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET session_state_json = ?2 WHERE id = ?1",
            params![conversation_id, state_json],
        )?;
        Ok(())
    }

    // ---- personas (CHT-4) ----

    pub fn create_persona(&self, p: &NewPersona) -> Result<Persona, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO personas(id, name, system_prompt, model_id, params_json, is_default, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
            params![id, p.name, p.system_prompt, p.model_id, p.params_json, ts],
        )?;
        Ok(Persona {
            id,
            name: p.name.clone(),
            system_prompt: p.system_prompt.clone(),
            model_id: p.model_id.clone(),
            params_json: p.params_json.clone(),
            is_default: false,
            created_at: ts,
            updated_at: ts,
        })
    }

    pub fn list_personas(&self) -> Result<Vec<Persona>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, system_prompt, model_id, params_json, is_default, created_at, updated_at
             FROM personas ORDER BY is_default DESC, name ASC",
        )?;
        let rows = stmt
            .query_map([], Self::map_persona)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn update_persona(&self, p: &Persona) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE personas SET name = ?2, system_prompt = ?3, model_id = ?4, params_json = ?5, updated_at = ?6
             WHERE id = ?1",
            params![p.id, p.name, p.system_prompt, p.model_id, p.params_json, now_ms()],
        )?;
        Ok(())
    }

    pub fn delete_persona(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        // Detach any conversations pointing at this persona so they fall back to
        // the global system prompt rather than a dangling reference.
        conn.execute("UPDATE conversations SET persona_id = NULL WHERE persona_id = ?1", [id])?;
        conn.execute("DELETE FROM personas WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn set_default_persona(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE personas SET is_default = 0", [])?;
        conn.execute("UPDATE personas SET is_default = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Set (or clear) a conversation's persona and one-off overrides (CHT-4/CHT-7).
    pub fn set_conversation_persona(
        &self,
        id: &str,
        persona_id: Option<&str>,
        overrides_json: Option<&str>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET persona_id = ?2, overrides_json = ?3 WHERE id = ?1",
            params![id, persona_id, overrides_json],
        )?;
        Ok(())
    }

    // ---- row mappers ----

    fn map_persona(row: &rusqlite::Row) -> rusqlite::Result<Persona> {
        Ok(Persona {
            id: row.get(0)?,
            name: row.get(1)?,
            system_prompt: row.get(2)?,
            model_id: row.get(3)?,
            params_json: row.get(4)?,
            is_default: row.get::<_, i64>(5)? != 0,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    }

    fn map_connector(row: &rusqlite::Row) -> rusqlite::Result<Connector> {
        Ok(Connector {
            id: row.get(0)?,
            name: row.get(1)?,
            url: row.get(2)?,
            transport: row.get(3)?,
            enabled: row.get::<_, i64>(4)? != 0,
            config_json: row.get(5)?,
            created_at: row.get(6)?,
        })
    }

    fn map_conversation(row: &rusqlite::Row) -> rusqlite::Result<Conversation> {
        Ok(Conversation {
            id: row.get(0)?,
            title: row.get(1)?,
            model_id: row.get(2)?,
            persona_id: row.get(3)?,
            overrides_json: row.get(4)?,
            workspace: row.get::<_, i64>(5)? != 0,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    }

    fn map_model(row: &rusqlite::Row) -> rusqlite::Result<ModelEntry> {
        Ok(ModelEntry {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
            quant: row.get(3)?,
            size_bytes: row.get(4)?,
            vision: row.get::<_, i64>(5)? != 0,
            is_default: row.get::<_, i64>(6)? != 0,
            added_at: row.get(7)?,
        })
    }

    fn map_message(row: &rusqlite::Row) -> rusqlite::Result<Message> {
        Ok(Message {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            role: row.get(2)?,
            content: row.get(3)?,
            model_name: row.get(4)?,
            model_provenance: row.get(5)?,
            steps_json: row.get(6)?,
            created_at: row.get(7)?,
            attachments: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_and_searches() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Parsing CSV in Rust", Some("llama-3.1-8b"), false).unwrap();
        db.append_message(
            &c.id,
            &NewMessage {
                role: "user".into(),
                content: "find the old API endpoint in my project".into(),
                model_name: None,
                model_provenance: None,
                steps_json: None,
                attachments: Vec::new(),
            },
        )
        .unwrap();

        let convs = db.list_conversations().unwrap();
        assert_eq!(convs.len(), 1);
        assert!(!convs[0].workspace, "new conversations default to classic chat");

        db.set_conversation_workspace(&c.id, true).unwrap();
        let convs = db.list_conversations().unwrap();
        assert!(convs[0].workspace, "workspace flag persists and round-trips");

        let hits = db.search_conversations("endpoint").unwrap();
        assert_eq!(hits.len(), 1, "FTS should find the conversation by message content");

        let miss = db.search_conversations("nonexistentterm").unwrap();
        assert!(miss.is_empty());
    }

    #[test]
    fn blocks_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Trip", None, false).unwrap();

        let b = db
            .add_block(&c.id, Some("msg-1"), "plan", "Checklist", r#"{"steps":[]}"#)
            .unwrap();
        assert_eq!(b.message_id.as_deref(), Some("msg-1"));

        db.update_block_data(&b.id, "Checklist v2", r#"{"steps":[{"id":"s1"}]}"#).unwrap();
        db.update_block_state(&b.id, r#"{"checked":{"s1":"done"}}"#).unwrap();

        let got = db.get_block(&b.id).unwrap().unwrap();
        assert_eq!(got.title, "Checklist v2");
        assert_eq!(got.state_json.as_deref(), Some(r#"{"checked":{"s1":"done"}}"#));

        let all = db.list_blocks(&c.id).unwrap();
        assert_eq!(all.len(), 1);

        // Dedup lookup (W3 safety net) finds by kind + title.
        let found = db.find_block_by_title(&c.id, "plan", "Checklist v2").unwrap();
        assert_eq!(found.map(|b| b.id), Some(b.id.clone()));
        assert!(db.find_block_by_title(&c.id, "plan", "nope").unwrap().is_none());

        // Session state (v3 column) round-trips on the same conversation.
        assert!(db.get_session_state(&c.id).unwrap().is_none());
        db.set_session_state(&c.id, r#"{"constraints":{"budget":2000}}"#).unwrap();
        assert_eq!(
            db.get_session_state(&c.id).unwrap().as_deref(),
            Some(r#"{"constraints":{"budget":2000}}"#)
        );
    }
}
