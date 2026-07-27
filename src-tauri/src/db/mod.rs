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
const SCHEMA_VERSION: i64 = 6;

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
    /// Rolling summary of the older turns (CTX-3). Changes only what is *sent*
    /// to the model — the messages themselves are never deleted or hidden.
    pub summary: Option<String>,
    /// Newest message covered by `summary`; turns after it are sent verbatim.
    pub summary_upto_message_id: Option<String>,
    /// When this conversation was last reflected on (REF-2). `None` means the
    /// agent hasn't yet tried to learn anything from it.
    pub reflected_at: Option<i64>,
    /// The real folder on disk this conversation works in, if one is attached.
    /// Everything the file tools do resolves against it.
    pub folder_path: Option<String>,
    /// How much the agent may do inside `folder_path`: "read-only" | "confirm"
    /// | "auto". Reads are always silent; this governs writes and deletes.
    pub folder_trust: String,
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

/// One tool's success record over a window (LOOP-UI-1), aggregated from
/// `tool_stats`. Content-free — just counts.
#[derive(Debug, Clone, Serialize)]
pub struct ToolStatRow {
    pub tool_name: String,
    pub ok: i64,
    pub total: i64,
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
    /// Where this artifact was materialised in the working folder, if the user
    /// ever saved it. Once set, the Workbench stops listing it as "made in this
    /// chat" and shows the real file in the tree instead.
    pub saved_path: Option<String>,
}

/// One reversible file operation. Recorded before the bytes change, so undo can
/// put them back. `blob_path` is `None` when the file did not exist before —
/// undoing that entry deletes the created file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashEntry {
    pub id: String,
    pub conversation_id: String,
    pub op: String,
    pub path: String,
    pub prev_path: Option<String>,
    pub blob_path: Option<String>,
    pub created_at: i64,
    pub undone: bool,
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

/// A self-change the agent proposed and the user hasn't answered yet (SOUL-2).
/// `target` is 'soul' | 'persona' | 'recipe' | 'lesson'; the `persona_id` column
/// future-proofs per-persona prompt proposals, which are out of scope for v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeProposal {
    pub id: String,
    pub target: String,
    /// The entry name, when the target is a recipe or lesson.
    pub slug: Option<String>,
    /// The complete replacement text for the target.
    pub proposed_text: String,
    pub rationale: String,
    /// pending | applied | dismissed
    pub status: String,
    pub created_at: i64,
}

/// One hit from the agent's own search over its past (RCL-1) — a chat message
/// or a durable memory entry, always with provenance the user can click.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    /// "chat" | "memory"
    pub source: String,
    pub conversation_id: Option<String>,
    /// Conversation title, or the memory entry's name.
    pub title: String,
    pub created_at: i64,
    pub snippet: String,
}

/// Quote every term so arbitrary user text can't break FTS5 MATCH syntax.
/// Turn a user/agent query into a safe FTS5 MATCH string: each whitespace token
/// becomes a quoted phrase. Tokens with no alphanumeric content are dropped —
/// a quoted phrase of pure punctuation is an FTS5 syntax error, not a no-match.
/// Returns `""` when nothing usable remains, which callers treat as no hits.
fn fts_escape(q: &str) -> String {
    q.split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| t.chars().any(|c| c.is_alphanumeric()))
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn now_ms() -> i64 {
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
        if current < 5 {
            // v5 (Poiesis): context compaction + reflection marker. The
            // `change_proposals`, `tool_stats` and `memory_fts` tables are created
            // by SCHEMA above; these add the compaction columns to `conversations`.
            Self::add_column(&conn, "conversations", "summary", "TEXT")?;
            Self::add_column(&conn, "conversations", "summary_upto_message_id", "TEXT")?;
            Self::add_column(&conn, "conversations", "reflected_at", "INTEGER")?;
        }
        if current < 6 {
            // v6 (Working folder): a conversation can attach one real folder on
            // disk plus a trust level governing what the agent may do inside it.
            // The `file_trash` table is created by SCHEMA above.
            Self::add_column(&conn, "conversations", "folder_path", "TEXT")?;
            Self::add_column(
                &conn,
                "conversations",
                "folder_trust",
                "TEXT NOT NULL DEFAULT 'confirm'",
            )?;
            // Artifacts remember where they were materialised on disk, if ever.
            Self::add_column(&conn, "artifacts", "saved_path", "TEXT")?;
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
            summary: None,
            summary_upto_message_id: None,
            reflected_at: None,
            folder_path: None,
            folder_trust: "confirm".to_string(),
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

    // ---- working folder ----

    /// Attach (or, with `None`, detach) the real folder this conversation works
    /// in. Detaching touches nothing on disk — it only forgets the path.
    pub fn set_conversation_folder(&self, id: &str, path: Option<&str>) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET folder_path = ?2 WHERE id = ?1",
            params![id, path],
        )?;
        Ok(())
    }

    /// Set how much the agent may do inside the attached folder.
    pub fn set_conversation_trust(&self, id: &str, trust: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET folder_trust = ?2 WHERE id = ?1",
            params![id, trust],
        )?;
        Ok(())
    }

    /// The attached folder + trust for one conversation, without loading the rest.
    /// Hot path: every file tool call consults this.
    pub fn conversation_folder(&self, id: &str) -> Result<(Option<String>, String), DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT folder_path, folder_trust FROM conversations WHERE id = ?1",
                [id],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .unwrap_or((None, None));
        Ok((row.0, row.1.unwrap_or_else(|| "confirm".to_string())))
    }

    pub fn list_conversations(&self) -> Result<Vec<Conversation>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, model_id, persona_id, overrides_json, workspace, created_at, updated_at,
                    summary, summary_upto_message_id, reflected_at, folder_path, folder_trust
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

    /// Messages up to and including `upto_id`, oldest first — the slice that
    /// compaction (CTX-3) folds into `conversations.summary`.
    ///
    /// Bounded by `rowid`, not `created_at`: turns written in the same
    /// millisecond would otherwise all fall inside the bound.
    pub fn list_messages_until(&self, conversation_id: &str, upto_id: &str) -> Result<Vec<Message>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content, model_name, model_provenance, steps_json, created_at
             FROM messages
             WHERE conversation_id = ?1
               AND rowid <= (SELECT rowid FROM messages WHERE id = ?2)
             ORDER BY rowid ASC",
        )?;
        let rows = stmt
            .query_map(params![conversation_id, upto_id], Self::map_message)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Persist a compaction result (CTX-3). Nothing is deleted — this only
    /// records what may be replaced by the summary when assembling a request.
    pub fn set_conversation_summary(
        &self,
        id: &str,
        summary: &str,
        upto_message_id: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET summary = ?2, summary_upto_message_id = ?3 WHERE id = ?1",
            params![id, summary, upto_message_id],
        )?;
        Ok(())
    }

    /// Mark a conversation as reflected on (REF-2). Set *before* the reflection
    /// turn runs, so a model that hangs or returns junk can't put the app in a
    /// loop of retrying the same conversation.
    pub fn set_conversation_reflected(&self, id: &str, at: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET reflected_at = ?2 WHERE id = ?1",
            params![id, at],
        )?;
        Ok(())
    }

    /// The most recent moment any conversation was reflected on (ORG-1).
    pub fn last_reflection(&self) -> Result<Option<i64>, DbError> {
        let conn = self.conn.lock().unwrap();
        let at: Option<i64> =
            conn.query_row("SELECT MAX(reflected_at) FROM conversations", [], |r| r.get(0))?;
        Ok(at)
    }

    /// Full-text search returning matching conversations, most-recent first (CHT-3).
    pub fn search_conversations(&self, query: &str) -> Result<Vec<Conversation>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT c.id, c.title, c.model_id, c.persona_id, c.overrides_json, c.workspace, c.created_at, c.updated_at,
                    c.summary, c.summary_upto_message_id, c.reflected_at, c.folder_path, c.folder_trust
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

    // ---- agent-proposed self-changes (SOUL-2 / RCP-2) ----

    /// Record a proposal. The agent never applies these itself — a proposal is
    /// a request for consent, and stays `pending` until the user answers.
    pub fn add_change_proposal(
        &self,
        target: &str,
        slug: Option<&str>,
        proposed_text: &str,
        rationale: &str,
    ) -> Result<ChangeProposal, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO change_proposals(id, target, slug, proposed_text, rationale, status, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
            params![id, target, slug, proposed_text, rationale, ts],
        )?;
        Ok(ChangeProposal {
            id,
            target: target.to_string(),
            slug: slug.map(str::to_string),
            proposed_text: proposed_text.to_string(),
            rationale: rationale.to_string(),
            status: "pending".to_string(),
            created_at: ts,
        })
    }

    /// Proposals still awaiting an answer, newest first.
    pub fn list_change_proposals(&self) -> Result<Vec<ChangeProposal>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, target, slug, proposed_text, rationale, status, created_at
             FROM change_proposals WHERE status = 'pending' ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ChangeProposal {
                    id: r.get(0)?,
                    target: r.get(1)?,
                    slug: r.get(2)?,
                    proposed_text: r.get(3)?,
                    rationale: r.get(4)?,
                    status: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Fetch one proposal by id regardless of status — callers decide whether a
    /// non-pending row is actionable. (`list_change_proposals` returns only
    /// pending rows, so it can't answer this.)
    pub fn get_change_proposal(&self, id: &str) -> Result<Option<ChangeProposal>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, target, slug, proposed_text, rationale, status, created_at
             FROM change_proposals WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], |r| {
            Ok(ChangeProposal {
                id: r.get(0)?,
                target: r.get(1)?,
                slug: r.get(2)?,
                proposed_text: r.get(3)?,
                rationale: r.get(4)?,
                status: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Mark a proposal `applied` or `dismissed`. Rows are kept, not deleted —
    /// what the agent asked for and what the user answered is part of the record.
    pub fn resolve_change_proposal(&self, id: &str, status: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE change_proposals SET status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    // ---- the agent's search over its own past (RCL-1) ----

    /// Full-text search over past messages, best matches first.
    pub fn search_messages_fts(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, DbError> {
        let match_expr = fts_escape(query);
        if match_expr.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT m.conversation_id, c.title, m.created_at,
                    snippet(messages_fts, 0, '', '', '…', 16)
             FROM messages_fts
             JOIN messages m      ON m.rowid = messages_fts.rowid
             JOIN conversations c ON c.id = m.conversation_id
             WHERE messages_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![match_expr, limit as i64], |r| {
                Ok(SearchHit {
                    source: "chat".to_string(),
                    conversation_id: r.get(0)?,
                    title: r.get(1)?,
                    created_at: r.get(2)?,
                    snippet: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Full-text search over the durable self (facts, lessons, recipes).
    pub fn search_memory_fts(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, DbError> {
        let match_expr = fts_escape(query);
        if match_expr.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT name, snippet(memory_fts, 2, '', '', '…', 16), description
             FROM memory_fts
             WHERE memory_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![match_expr, limit as i64], |r| {
                let snippet: String = r.get(1)?;
                let description: String = r.get(2)?;
                Ok(SearchHit {
                    source: "memory".to_string(),
                    conversation_id: None,
                    title: r.get(0)?,
                    created_at: 0,
                    snippet: if snippet.trim().is_empty() { description } else { snippet },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Replace the whole memory FTS index. The store rebuilds it on every write —
    /// fine at entry-count scale (tens, not thousands).
    pub fn replace_memory_fts(&self, rows: &[(String, String, String, String)]) -> Result<(), DbError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM memory_fts", [])?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO memory_fts(name, description, body, kind) VALUES(?1, ?2, ?3, ?4)")?;
            for (name, description, body, kind) in rows {
                stmt.execute(params![name, description, body, kind])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The last `max` messages of a conversation, oldest first (RCL-2
    /// `read_conversation`). Empty when the conversation doesn't exist.
    pub fn list_messages_window(&self, conversation_id: &str, max: usize) -> Result<Vec<Message>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content, model_name, model_provenance, steps_json, created_at
             FROM messages WHERE conversation_id = ?1
             ORDER BY rowid DESC LIMIT ?2",
        )?;
        let mut rows = stmt
            .query_map(params![conversation_id, max as i64], Self::map_message)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.reverse();
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

    // ---- Tool reliability stats (GRM-4 / LOOP-5) ----

    /// Record one tool-call outcome for the running model. Content-free — only
    /// the model name, tool name, owning conversation, and success bit. Feeds the
    /// reliability captions (LOOP-UI-1) and, later, self-repair/reflection.
    /// Best-effort: a stats write must never break a turn, so errors are dropped.
    pub fn add_tool_stat(&self, model_name: &str, tool_name: &str, conversation_id: &str, ok: bool) {
        let Ok(conn) = self.conn.lock() else { return };
        let _ = conn.execute(
            "INSERT INTO tool_stats(id, model_name, tool_name, conversation_id, ok, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![new_id(), model_name, tool_name, conversation_id, ok as i64, now_ms()],
        );
    }

    /// Per-tool success counts over the last `days` days (LOOP-UI-1).
    pub fn tool_stats_since(&self, days: i64) -> Result<Vec<ToolStatRow>, DbError> {
        let cutoff = now_ms() - days * 24 * 60 * 60 * 1000;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT tool_name, SUM(ok), COUNT(*) FROM tool_stats
             WHERE created_at >= ?1 GROUP BY tool_name",
        )?;
        let rows = stmt
            .query_map([cutoff], |r| {
                Ok(ToolStatRow {
                    tool_name: r.get(0)?,
                    ok: r.get(1)?,
                    total: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Which tools failed in one conversation, and how often (REF-2). This is
    /// the only hard evidence reflection gets about its own mistakes.
    pub fn tool_failures_in(&self, conversation_id: &str) -> Result<Vec<(String, i64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT tool_name, COUNT(*) FROM tool_stats
             WHERE conversation_id = ?1 AND ok = 0 GROUP BY tool_name ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Per-tool reliability for one model over the last `days` (HEAL-2). Same
    /// shape as `tool_stats_since`, narrowed to the model actually running —
    /// a tool that a 3B model fumbles isn't broken for a cloud model.
    pub fn tool_health(&self, model: &str, days: i64) -> Result<Vec<ToolStatRow>, DbError> {
        let cutoff = now_ms() - days * 24 * 60 * 60 * 1000;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT tool_name, SUM(ok), COUNT(*) FROM tool_stats
             WHERE model_name = ?1 AND created_at >= ?2 GROUP BY tool_name",
        )?;
        let rows = stmt
            .query_map(params![model, cutoff], |r| {
                Ok(ToolStatRow {
                    tool_name: r.get(0)?,
                    ok: r.get(1)?,
                    total: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// How many proposals are still waiting for an answer (ORG-1).
    pub fn pending_proposal_count(&self) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM change_proposals WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
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
            saved_path: None,
        })
    }

    fn map_artifact(r: &rusqlite::Row) -> rusqlite::Result<Artifact> {
        Ok(Artifact {
            id: r.get(0)?,
            conversation_id: r.get(1)?,
            title: r.get(2)?,
            kind: r.get(3)?,
            content: r.get(4)?,
            created_at: r.get(5)?,
            saved_path: r.get(6)?,
        })
    }

    pub fn list_artifacts(&self, conversation_id: &str) -> Result<Vec<Artifact>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, title, kind, content, created_at, saved_path
             FROM artifacts WHERE conversation_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map([conversation_id], Self::map_artifact)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list_all_artifacts(&self) -> Result<Vec<Artifact>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, title, kind, content, created_at, saved_path
             FROM artifacts ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], Self::map_artifact)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_artifact(&self, id: &str) -> Result<Option<Artifact>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, title, kind, content, created_at, saved_path
             FROM artifacts WHERE id = ?1",
        )?;
        let row = stmt.query_row([id], Self::map_artifact).ok();
        Ok(row)
    }

    /// Record where an artifact was materialised on disk (promotion, §3.5F).
    pub fn set_artifact_saved_path(&self, id: &str, path: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE artifacts SET saved_path = ?2 WHERE id = ?1",
            params![id, path],
        )?;
        Ok(())
    }

    /// Was this path attached to a message at some point? Attaching a file is
    /// the user handing it over, and that consent outlives the session — without
    /// this, reopening an old chat couldn't re-read its own images.
    pub fn is_known_attachment(&self, path: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM attachments WHERE path = ?1",
            [path],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    // ---- file undo trash ----

    pub fn add_trash_entry(
        &self,
        conversation_id: &str,
        op: &str,
        path: &str,
        prev_path: Option<&str>,
        blob_path: Option<&str>,
    ) -> Result<TrashEntry, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO file_trash(id, conversation_id, op, path, prev_path, blob_path, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, conversation_id, op, path, prev_path, blob_path, ts],
        )?;
        Ok(TrashEntry {
            id,
            conversation_id: conversation_id.to_string(),
            op: op.to_string(),
            path: path.to_string(),
            prev_path: prev_path.map(|s| s.to_string()),
            blob_path: blob_path.map(|s| s.to_string()),
            created_at: ts,
            undone: false,
        })
    }

    fn map_trash(r: &rusqlite::Row) -> rusqlite::Result<TrashEntry> {
        Ok(TrashEntry {
            id: r.get(0)?,
            conversation_id: r.get(1)?,
            op: r.get(2)?,
            path: r.get(3)?,
            prev_path: r.get(4)?,
            blob_path: r.get(5)?,
            created_at: r.get(6)?,
            undone: r.get::<_, i64>(7)? != 0,
        })
    }

    pub fn list_trash(&self, conversation_id: &str, limit: i64) -> Result<Vec<TrashEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, op, path, prev_path, blob_path, created_at, undone
             FROM file_trash WHERE conversation_id = ?1
             -- rowid breaks the tie: timestamps are millisecond-resolution, and a
             -- burst of writes in one turn lands inside a single millisecond.
             ORDER BY created_at DESC, rowid DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![conversation_id, limit], Self::map_trash)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_trash_entry(&self, id: &str) -> Result<Option<TrashEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, op, path, prev_path, blob_path, created_at, undone
             FROM file_trash WHERE id = ?1",
        )?;
        Ok(stmt.query_row([id], Self::map_trash).ok())
    }

    pub fn mark_trash_undone(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE file_trash SET undone = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Trash entries older than `cutoff_ms`, so startup can prune their blobs.
    pub fn expired_trash(&self, cutoff_ms: i64) -> Result<Vec<TrashEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, op, path, prev_path, blob_path, created_at, undone
             FROM file_trash WHERE created_at < ?1",
        )?;
        let rows = stmt
            .query_map([cutoff_ms], Self::map_trash)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_trash_entry(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM file_trash WHERE id = ?1", [id])?;
        Ok(())
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
            summary: row.get(8)?,
            summary_upto_message_id: row.get(9)?,
            reflected_at: row.get(10)?,
            folder_path: row.get(11)?,
            folder_trust: row
                .get::<_, Option<String>>(12)?
                .unwrap_or_else(|| "confirm".to_string()),
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
    fn working_folder_and_trust_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Folder", None, false).unwrap();

        // Nothing attached, and the safe middle setting by default.
        assert_eq!(db.conversation_folder(&c.id).unwrap(), (None, "confirm".into()));
        assert!(c.folder_path.is_none());
        assert_eq!(c.folder_trust, "confirm");

        db.set_conversation_folder(&c.id, Some(r"C:\work\thing")).unwrap();
        db.set_conversation_trust(&c.id, "auto").unwrap();
        assert_eq!(
            db.conversation_folder(&c.id).unwrap(),
            (Some(r"C:\work\thing".to_string()), "auto".to_string())
        );

        // …and it survives a full load, not just the narrow lookup.
        let convs = db.list_conversations().unwrap();
        assert_eq!(convs[0].folder_path.as_deref(), Some(r"C:\work\thing"));
        assert_eq!(convs[0].folder_trust, "auto");

        // Detaching forgets the path without disturbing the trust level.
        db.set_conversation_folder(&c.id, None).unwrap();
        assert_eq!(db.conversation_folder(&c.id).unwrap(), (None, "auto".into()));

        // An unknown conversation reads as "no folder", not an error.
        assert_eq!(db.conversation_folder("nope").unwrap(), (None, "confirm".into()));
    }

    #[test]
    fn artifacts_remember_where_they_were_saved() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Art", None, false).unwrap();
        let a = db.add_artifact(Some(&c.id), "Chart", "svg", "<svg/>").unwrap();
        assert!(a.saved_path.is_none(), "an artifact starts as chat-only");

        db.set_artifact_saved_path(&a.id, r"C:\work\thing\chart.svg").unwrap();
        let got = db.get_artifact(&a.id).unwrap().unwrap();
        assert_eq!(got.saved_path.as_deref(), Some(r"C:\work\thing\chart.svg"));
        // The promotion is visible everywhere the artifact is listed.
        let listed = db.list_artifacts(&c.id).unwrap();
        assert_eq!(listed[0].saved_path, got.saved_path);
    }

    #[test]
    fn trash_entries_list_newest_first_and_mark_undone() {
        let db = Db::open_in_memory().unwrap();
        let a = db.add_trash_entry("conv1", "write", "/a.txt", None, Some("/blob/1")).unwrap();
        let b = db.add_trash_entry("conv1", "delete", "/b.txt", None, Some("/blob/2")).unwrap();
        db.add_trash_entry("other", "write", "/c.txt", None, None).unwrap();

        let listed = db.list_trash("conv1", 10).unwrap();
        assert_eq!(listed.len(), 2, "scoped to the conversation");
        assert_eq!(listed[0].id, b.id, "newest first");
        assert!(!listed[0].undone);

        db.mark_trash_undone(&a.id).unwrap();
        assert!(db.get_trash_entry(&a.id).unwrap().unwrap().undone);
    }

    #[test]
    fn known_attachments_stay_readable() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Att", None, false).unwrap();
        db.append_message(
            &c.id,
            &NewMessage {
                role: "user".into(),
                content: "look".into(),
                model_name: None,
                model_provenance: None,
                steps_json: None,
                attachments: vec![NewAttachment {
                    kind: "image".into(),
                    name: "shot.png".into(),
                    path: r"C:\pics\shot.png".into(),
                }],
            },
        )
        .unwrap();

        assert!(db.is_known_attachment(r"C:\pics\shot.png").unwrap());
        assert!(!db.is_known_attachment(r"C:\secrets\passwords.txt").unwrap());
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

    /// RCL-1: the agent searches both its chat history and its durable self.
    #[test]
    fn recall_searches_chats_and_memory() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_conversation("NAS build", None, false).unwrap();
        let b = db.create_conversation("Dinner plans", None, false).unwrap();
        for (conv, text) in [(&a, "we settled on ZFS mirrored vdevs"), (&b, "risotto on friday")] {
            db.append_message(
                &conv.id,
                &NewMessage {
                    role: "user".into(),
                    content: text.into(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        }
        db.replace_memory_fts(&[(
            "prefers-metric-units".into(),
            "User wants all measurements in metric".into(),
            "Always give measurements in metric. Confirmed twice.".into(),
            "fact".into(),
        )])
        .unwrap();

        let chat = db.search_messages_fts("ZFS", 5).unwrap();
        assert_eq!(chat.len(), 1);
        assert_eq!(chat[0].source, "chat");
        assert_eq!(chat[0].title, "NAS build");
        assert_eq!(chat[0].conversation_id.as_deref(), Some(a.id.as_str()));

        let mem = db.search_memory_fts("metric", 5).unwrap();
        assert_eq!(mem.len(), 1);
        assert_eq!(mem[0].source, "memory");
        assert_eq!(mem[0].title, "prefers-metric-units");

        // Raw user text must not break MATCH syntax (fts_escape).
        assert!(db.search_messages_fts("what about \"ZFS\" ?", 5).is_ok());
        assert!(db.search_messages_fts("NOT (broken", 5).unwrap().is_empty());
    }

    /// CTX-3: compaction records a summary boundary without touching messages.
    #[test]
    fn compaction_summary_round_trips() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Long chat", None, false).unwrap();
        let mut ids = Vec::new();
        for i in 0..4 {
            let m = db
                .append_message(
                    &c.id,
                    &NewMessage {
                        role: if i % 2 == 0 { "user".into() } else { "assistant".into() },
                        content: format!("turn {i}"),
                        model_name: None,
                        model_provenance: None,
                        steps_json: None,
                        attachments: Vec::new(),
                    },
                )
                .unwrap();
            ids.push(m.id);
        }

        let until = db.list_messages_until(&c.id, &ids[1]).unwrap();
        assert_eq!(until.len(), 2, "inclusive of the boundary message");

        db.set_conversation_summary(&c.id, "FACTS: …", &ids[1]).unwrap();
        let conv = db.list_conversations().unwrap().remove(0);
        assert_eq!(conv.summary.as_deref(), Some("FACTS: …"));
        assert_eq!(conv.summary_upto_message_id.as_deref(), Some(ids[1].as_str()));

        assert_eq!(db.list_messages(&c.id).unwrap().len(), 4, "nothing is ever deleted");
        assert_eq!(db.list_messages_window(&c.id, 2).unwrap().len(), 2);
    }
}
