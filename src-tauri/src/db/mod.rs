//! SQLite persistence (PRD §7.1.1): conversations, messages, settings, the model
//! library, and connector config, plus FTS5 search over history (CHT-3).
//!
//! A single connection guarded by a mutex is sufficient for a single-user desktop
//! app and keeps the access model simple. Attachment binaries live on disk; only
//! their paths are stored here.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// The vector store (Perception, VEC) — encode/decode, similarity, search.
pub mod vectors;
/// Indexed folder roots (Perception, IDX) — what got built, and how it went.
pub mod index_roots;
/// Cached perceptual image hashes (Perception, PHS).
pub mod phash;

const SCHEMA: &str = include_str!("schema.sql");
const SCHEMA_VERSION: i64 = 17;

/// The rationale a skill-revision proposal is written with (`OUT-2`). Only
/// display text — the proposal is *identified* by its `skill-revision` target,
/// never by matching this string.
pub const SKILL_REVISION_RATIONALE: &str = "This skill has been rough the last few times I used it.";

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
    /// `PER-1`: a JSON array of allowed toolset ids. `NULL` means "every
    /// enabled toolset" — the pre-`PER` behaviour, unchanged for every persona
    /// that never touches the tool list.
    pub tools_json: Option<String>,
    /// `SKL-6`: a JSON array of allowed Agent Skill names, the same shape as
    /// `tools_json` — `NULL` means every enabled skill.
    #[serde(default)]
    pub skills_json: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NewPersona {
    pub name: String,
    pub system_prompt: String,
    pub model_id: Option<String>,
    pub params_json: Option<String>,
    pub tools_json: Option<String>,
    #[serde(default)]
    pub skills_json: Option<String>,
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
    /// The artifact this attachment renders, when one backs it (`ART-2`).
    /// Without it a reloaded transcript still shows the picture but loses
    /// everything that made it an artifact: Save, download, the provider line.
    pub artifact_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewAttachment {
    pub kind: String,
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub artifact_id: Option<String>,
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
    /// "chat" | "embed" | "rerank" (schema v7) — one library, three engines.
    pub role: String,
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

/// One fail→fix pair (`FIX-1`): a tool call that failed, and the corrected
/// call to the same tool that succeeded right after, in the same run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFix {
    pub tool_name: String,
    pub failed_args: String,
    pub error: String,
    pub fixed_args: String,
}

/// One activation of a skill and how the conversation went afterwards
/// (`OUT-1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRunRow {
    pub conversation_id: String,
    pub tool_failures: i64,
    pub corrected: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    pub path: String,
    pub mode: String,
    pub created_at: i64,
}

/// A persisted "Always allow" answer to a capability consent prompt
/// (`BRW-3`/`SYS-1`) — see `capability_grants`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub id: String,
    pub kind: String,
    pub value: String,
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
    /// Provider/cost/dimensions for a generated image or video (Phase 13,
    /// `ART-1`) — a JSON object, opaque to everything but the media/Library UI.
    pub meta_json: Option<String>,
    /// The artifact this one was refined from (Path B), if any.
    pub parent_id: Option<String>,
}

/// What generated media has cost and how much of it there is (`CST-2`).
/// No budget enforcement — just the number, because a number nobody shows is
/// a number nobody trusts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaSpend {
    pub usd: f64,
    pub images: i64,
    pub videos: i64,
}

/// A media generation running in the background (`JOB-1`). One row per
/// request, written at submit and updated once at completion — the agent loop
/// never waits on it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaJob {
    pub id: String,
    pub conversation_id: Option<String>,
    /// The assistant turn that asked for this, so a result arriving after the
    /// run has already finished still lands in the right place.
    pub message_id: Option<String>,
    pub modality: String,
    /// `running` | `done` | `failed` | `cancelled`.
    pub status: String,
    pub prompt: String,
    pub model_id: Option<String>,
    pub aspect_ratio: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub artifact_id: Option<String>,
    pub error: Option<String>,
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

/// A configured mail account (`MAIL-1`). The password is **not** stored here —
/// it lives in the OS credential store (`secrets::SERVICE_MAIL`, account =
/// this row's `id`); only connection metadata lives in SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailAccount {
    pub id: String,
    pub label: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub username: String,
    pub auth: String,
    /// 'tls' (implicit) | 'starttls' (upgrade). See `agent::mail::Security`.
    pub security: String,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct NewMailAccount {
    pub label: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub username: String,
    pub security: String,
}

/// A self-change the agent proposed and the user hasn't answered yet (SOUL-2).
/// `target` is 'soul' | 'lesson' | 'lesson-critic' | 'skill' | 'skill-revision'
/// | 'email' | 'recipe' (legacy); the `persona_id` column future-proofs
/// per-persona prompt proposals, which are out of scope for v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeProposal {
    pub id: String,
    pub target: String,
    /// The entry name, when the target is a recipe, lesson or skill. For
    /// `target = 'soul'` this is `None` for an ordinary standing-instruction
    /// edit but `Some(lesson_name)` for `RPT-2`'s recurrence escalation — the
    /// frontend's structural way to tell the two apart without matching on
    /// rationale text.
    pub slug: Option<String>,
    /// The complete replacement text for the target.
    pub proposed_text: String,
    /// Why the change is being asked for — shown to the user while the
    /// proposal is pending, and thrown away once it is answered. For a
    /// critic-demoted lesson (`CRT-2`) this is the critic's objection, which
    /// is precisely why it must not be reused as the entry's own summary.
    pub rationale: String,
    /// The entry's own one-line summary, kept if the proposal is applied.
    /// `None` for targets that have no separate summary (and for rows written
    /// before schema v8).
    pub description: Option<String>,
    /// pending | applied | dismissed
    pub status: String,
    pub created_at: i64,
}

/// One hit from the agent's own search over its past (RCL-1) — a chat message
/// or a durable memory entry, always with provenance the user can click.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    /// "chat" | "memory" | "file"
    pub source: String,
    pub conversation_id: Option<String>,
    /// Conversation title, or the memory entry's name.
    pub title: String,
    pub created_at: i64,
    pub snippet: String,
    /// "fact" | "lesson" | "recipe" — set only for `source: "memory"`, so the
    /// timeline can label a lesson differently from a fact (SEM-UI-1/2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Absolute file path, set only for `source: "file"` (`RET-UI-1`) — lets
    /// `Provenance` open the match in `Viewer.tsx` instead of switching
    /// conversation, the way `conversation_id` does for a chat hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
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
        if current < 7 {
            // v7 (Perception): model role (chat | embed | rerank); per-persona tool
            // sets; per-message context manifest so a past answer can be explained
            // (WHY-2). The `vectors` and `index_roots` tables are created by SCHEMA
            // above.
            Self::add_column(&conn, "model_library", "role", "TEXT NOT NULL DEFAULT 'chat'")?;
            Self::add_column(&conn, "personas", "tools_json", "TEXT")?;
            Self::add_column(&conn, "messages", "context_json", "TEXT")?;
        }
        if current < 8 {
            // v8 (CRT-2): a proposal's rationale is the argument for making the
            // change — for a critic-demoted lesson it is the critic's
            // *objection*. That must never become the entry's own description
            // when the user accepts it, so the description travels separately.
            Self::add_column(&conn, "change_proposals", "description", "TEXT")?;
        }
        if current < 9 {
            // v9 (`TSET-3`): `agent::skills::Skill` was renamed to `Toolset` —
            // migrate, don't orphan. A user who turned a toolset off before the
            // upgrade (key `skill.<name>.enabled`) must still have it off after
            // (key `toolset.<name>.enabled`); leaving the old key behind would
            // silently turn it back on for everyone under the new key.
            let mut stmt = conn.prepare("SELECT key, value FROM settings WHERE key LIKE 'skill.%'")?;
            let rows: Vec<(String, String)> = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            for (old_key, value) in rows {
                let new_key = format!("toolset.{}", &old_key["skill.".len()..]);
                conn.execute(
                    "INSERT INTO settings(key, value) VALUES(?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![new_key, value],
                )?;
                conn.execute("DELETE FROM settings WHERE key = ?1", params![old_key])?;
            }
        }
        if current < 10 {
            // v10 (`MAIL-1`/`SKL-6`): the `mail_accounts` table is created by
            // SCHEMA above. Personas gain the same allowlist shape for skills
            // that `tools_json` (v7) gave them for toolsets.
            Self::add_column(&conn, "personas", "skills_json", "TEXT")?;
        }
        if current < 11 {
            // v11 (`SKL-5`): recipes became skills, so the autonomy class did
            // too. A user who turned procedure-keeping off must not silently
            // get it back under a new name — the *choice* migrates, not just
            // the label. Only fills `skills` if it was never set explicitly.
            let old: Option<String> = conn
                .query_row(
                    "SELECT value FROM settings WHERE key = 'autonomy.recipes'",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(value) = old {
                conn.execute(
                    "INSERT OR IGNORE INTO settings (key, value) VALUES ('autonomy.skills', ?1)",
                    params![value],
                )?;
                conn.execute("DELETE FROM settings WHERE key = 'autonomy.recipes'", [])?;
            }
            // The Recipes toolset no longer exists; leaving its switch behind
            // would keep a dead row the Settings surface can never show again.
            conn.execute("DELETE FROM settings WHERE key = 'toolset.recipes.enabled'", [])?;
        }
        if current < 12 {
            // v12: mail accounts learn whether their ports speak implicit TLS
            // or STARTTLS. Existing rows are backfilled from the port, which is
            // the same inference the account form now makes for a new one —
            // 993/465 are the two implicit-TLS ports, everything else upgrades.
            Self::add_column(&conn, "mail_accounts", "security", "TEXT NOT NULL DEFAULT 'tls'")?;
            conn.execute(
                "UPDATE mail_accounts SET security = 'starttls'
                 WHERE imap_port NOT IN (993) OR smtp_port NOT IN (465)",
                [],
            )?;
        }
        // v13 (`FIX-1`): the `tool_fixes` table is created by SCHEMA above —
        // a brand-new table needs no `ALTER TABLE` migration block. Same for
        // v14's `browser_sessions` (`BRW-UI-1`).
        if current < 15 {
            // v15 (Phase 13, `ART-1`): a generated image/video artifact carries
            // its provider, cost and dimensions (`meta_json`), and — once a
            // refinement produces a new artifact from an old one (Path B) — a
            // `parent_id` link so Library can show the lineage.
            Self::add_column(&conn, "artifacts", "meta_json", "TEXT")?;
            Self::add_column(&conn, "artifacts", "parent_id", "TEXT")?;
        }

        if current < 16 {
            // v16 (Phase 13, `ART-2`): the link from an inline attachment back
            // to the artifact it renders. Without it a reloaded conversation
            // shows a generated image with no actions under it, because nothing
            // on disk remembered the two were the same thing.
            Self::add_column(&conn, "attachments", "artifact_id", "TEXT")?;
        }
        // v17 (Phase 13, `JOB-1`): the `media_jobs` table is created by SCHEMA
        // above — a brand-new table needs no `ALTER TABLE` block, same as v13's
        // `tool_fixes` and v14's `browser_sessions`.
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
                "INSERT INTO attachments(id, message_id, kind, name, path, artifact_id)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![aid, id, a.kind, a.name, a.path, a.artifact_id],
            )?;
            saved.push(Attachment {
                id: aid,
                kind: a.kind.clone(),
                name: a.name.clone(),
                path: a.path.clone(),
                artifact_id: a.artifact_id.clone(),
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
    /// `context_json` is the compact WHY-2 manifest (persona id, soul presence,
    /// fact/lesson/recipe/file slugs) that lets a past answer be explained
    /// later — `None` on a turn nothing was recorded for (pre-`WHY-2` history,
    /// or the no-Tauri dev path).
    pub fn finalize_message(
        &self,
        id: &str,
        content: &str,
        steps_json: Option<&str>,
        context_json: Option<&str>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET content = ?2, steps_json = ?3, context_json = ?4 WHERE id = ?1",
            params![id, content, steps_json, context_json],
        )?;
        Ok(())
    }

    /// The stored WHY-2 manifest for one message, if any was recorded. `None`
    /// covers both "no such message" and "predates WHY-2" — `context_manifest_cmd`
    /// treats both as the same honest "I didn't record this one" (WHY-5).
    pub fn message_context_json(&self, id: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row: Option<Option<String>> = conn
            .query_row("SELECT context_json FROM messages WHERE id = ?1", [id], |r| r.get(0))
            .ok();
        Ok(row.flatten())
    }

    /// One conversation by id — used to resolve the active persona and rolling
    /// summary for the live context manifest (`WHY-1`).
    pub fn get_conversation(&self, id: &str) -> Result<Option<Conversation>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, title, model_id, persona_id, overrides_json, workspace, created_at, updated_at,
                        summary, summary_upto_message_id, reflected_at, folder_path, folder_trust
                 FROM conversations WHERE id = ?1",
                [id],
                Self::map_conversation,
            )
            .ok();
        Ok(row)
    }

    /// One persona by id — used to rehydrate the `persona` context layer.
    pub fn get_persona(&self, id: &str) -> Result<Option<Persona>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, name, system_prompt, model_id, params_json, is_default, created_at, updated_at, tools_json, skills_json
                 FROM personas WHERE id = ?1",
                [id],
                Self::map_persona,
            )
            .ok();
        Ok(row)
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
            "SELECT a.id, a.message_id, a.kind, a.name, a.path, a.artifact_id
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
                    artifact_id: r.get(5)?,
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
        description: Option<&str>,
    ) -> Result<ChangeProposal, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO change_proposals(id, target, slug, proposed_text, rationale, description, status, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
            params![id, target, slug, proposed_text, rationale, description, ts],
        )?;
        Ok(ChangeProposal {
            id,
            target: target.to_string(),
            slug: slug.map(str::to_string),
            proposed_text: proposed_text.to_string(),
            rationale: rationale.to_string(),
            description: description.map(str::to_string),
            status: "pending".to_string(),
            created_at: ts,
        })
    }

    /// Proposals still awaiting an answer, newest first.
    pub fn list_change_proposals(&self) -> Result<Vec<ChangeProposal>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, target, slug, proposed_text, rationale, description, status, created_at
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
                    description: r.get(5)?,
                    status: r.get(6)?,
                    created_at: r.get(7)?,
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
            "SELECT id, target, slug, proposed_text, rationale, description, status, created_at
             FROM change_proposals WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], |r| {
            Ok(ChangeProposal {
                id: r.get(0)?,
                target: r.get(1)?,
                slug: r.get(2)?,
                proposed_text: r.get(3)?,
                rationale: r.get(4)?,
                description: r.get(5)?,
                status: r.get(6)?,
                created_at: r.get(7)?,
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

    /// `MAIL-UI-2`'s `Edit`: rewrite a still-pending proposal's text before it
    /// is accepted — the user's edit is what goes out, not what the model
    /// first drafted. Refuses a proposal that has already been answered.
    pub fn update_change_proposal_text(&self, id: &str, proposed_text: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE change_proposals SET proposed_text = ?2 WHERE id = ?1 AND status = 'pending'",
            params![id, proposed_text],
        )?;
        Ok(())
    }

    /// `RPT-2`'s escalation guard: a lesson relearned a fourth, fifth, … time
    /// must not queue the same standing-instruction proposal again.
    ///
    /// Deliberately **any status, not just pending**. A dismissed escalation
    /// is the user having said no to exactly this; asking again on the next
    /// recurrence would be nagging, and an applied one is already in force.
    /// `slug` on a `target = 'soul'` row is otherwise unused, but nothing
    /// stops reusing it as the lookup key for this one question.
    pub fn has_soul_escalation(&self, slug: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM change_proposals WHERE target = 'soul' AND slug = ?1",
            params![slug],
            |r| r.get(0),
        )?;
        Ok(n > 0)
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
                    kind: None,
                    path: None,
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
            "SELECT name, snippet(memory_fts, 2, '', '', '…', 16), description, kind
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
                    kind: r.get(3)?,
                    path: None,
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

    /// Record that every named entry in `collection` reached a prompt just
    /// now (SEM-UI-4) — wholesale-injected or retrieved, `recall_for` doesn't
    /// distinguish. Best-effort: a failed touch only costs a stale "last
    /// surfaced" date, never a turn.
    pub fn touch_memory_usage(&self, collection: &str, names: &[String]) -> Result<(), DbError> {
        if names.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO memory_usage(collection, ref_key, last_used_at) VALUES(?1, ?2, ?3)
                 ON CONFLICT(collection, ref_key) DO UPDATE SET last_used_at = excluded.last_used_at",
            )?;
            let ts = now_ms();
            for name in names {
                stmt.execute(params![collection, name, ts])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Drop one entry's "last surfaced" mark. Called when it's forgotten: the
    /// slug can be saved again later, and a brand-new fact must not inherit the
    /// old one's history.
    pub fn delete_memory_usage(&self, collection: &str, name: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM memory_usage WHERE collection = ?1 AND ref_key = ?2",
            params![collection, name],
        )?;
        Ok(())
    }

    /// Every recorded "last surfaced" timestamp in one collection, by name.
    pub fn memory_usage_map(&self, collection: &str) -> Result<std::collections::HashMap<String, i64>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT ref_key, last_used_at FROM memory_usage WHERE collection = ?1")?;
        let rows = stmt
            .query_map(params![collection], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
        Ok(rows)
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

    /// Add a "chat" model — the library's default role. Embedding/reranking
    /// models go through `add_model_with_role` instead (EMB-4).
    pub fn add_model(&self, m: &NewModelEntry) -> Result<ModelEntry, DbError> {
        self.add_model_with_role(m, "chat")
    }

    /// Add a model under a specific role ("chat" | "embed" | "rerank",
    /// schema v7). "First one added becomes the default" is scoped per role,
    /// so installing an embedder never disturbs the chat default.
    pub fn add_model_with_role(&self, m: &NewModelEntry, role: &str) -> Result<ModelEntry, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM model_library WHERE role = ?1",
            [role],
            |r| r.get(0),
        )?;
        let is_default = count == 0;
        conn.execute(
            "INSERT INTO model_library(id, name, path, quant, size_bytes, vision, role, is_default, added_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, m.name, m.path, m.quant, m.size_bytes, m.vision as i64, role, is_default as i64, ts],
        )?;
        Ok(ModelEntry {
            id,
            name: m.name.clone(),
            path: m.path.clone(),
            quant: m.quant.clone(),
            size_bytes: m.size_bytes,
            vision: m.vision,
            role: role.to_string(),
            is_default,
            added_at: ts,
        })
    }

    /// Chat models only — the Models view's list. See `list_models_by_role`
    /// for the embed/rerank catalogs.
    pub fn list_models(&self) -> Result<Vec<ModelEntry>, DbError> {
        self.list_models_by_role("chat")
    }

    pub fn list_models_by_role(&self, role: &str) -> Result<Vec<ModelEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, quant, size_bytes, vision, role, is_default, added_at
             FROM model_library WHERE role = ?1 ORDER BY added_at DESC",
        )?;
        let rows = stmt
            .query_map([role], Self::map_model)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Delete a model and return the file path to clean up, if any.
    ///
    /// Deleting the role's default promotes the next-newest model of that role
    /// in its place. Without this, removing the default embedder while a second
    /// one was still installed left the role with no default at all: the setup
    /// status reported "not installed" while the other model sat on disk and
    /// still appeared in the library.
    pub fn delete_model(&self, id: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(String, String, bool)> = conn
            .query_row(
                "SELECT path, role, is_default FROM model_library WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)),
            )
            .ok();
        conn.execute("DELETE FROM model_library WHERE id = ?1", [id])?;
        if let Some((_, role, was_default)) = &row {
            if *was_default {
                conn.execute(
                    "UPDATE model_library SET is_default = 1 WHERE id = (
                         SELECT id FROM model_library WHERE role = ?1 ORDER BY added_at DESC LIMIT 1
                     )",
                    params![role],
                )?;
            }
        }
        Ok(row.map(|(path, _, _)| path))
    }

    /// Make `id` the default within its own role — installing an embedder
    /// default never clears the chat default, and vice versa.
    pub fn set_default_model(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let role: String = conn.query_row("SELECT role FROM model_library WHERE id = ?1", [id], |r| r.get(0))?;
        conn.execute("UPDATE model_library SET is_default = 0 WHERE role = ?1", params![role])?;
        conn.execute("UPDATE model_library SET is_default = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    // Used by first-run model auto-selection in a follow-up; kept ready.
    #[allow(dead_code)]
    pub fn default_model(&self) -> Result<Option<ModelEntry>, DbError> {
        self.default_model_by_role("chat")
    }

    /// The default model for a role (EMB-4's `model_library.role`), if any.
    pub fn default_model_by_role(&self, role: &str) -> Result<Option<ModelEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, name, path, quant, size_bytes, vision, role, is_default, added_at
                 FROM model_library WHERE role = ?1 AND is_default = 1 LIMIT 1",
                [role],
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

    /// `BRW-3`/`SYS-1`: "Always allow {domain}" / "Always allow {app}" —
    /// persists the answer so the consent prompt for this exact domain/app
    /// never fires again.
    pub fn add_capability_grant(&self, kind: &str, value: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO capability_grants(id, kind, value, created_at) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(kind, value) DO NOTHING",
            params![new_id(), kind, value, now_ms()],
        )?;
        Ok(())
    }

    pub fn has_capability_grant(&self, kind: &str, value: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM capability_grants WHERE kind = ?1 AND value = ?2",
            params![kind, value],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Revocable in Settings, like a folder grant.
    pub fn list_capability_grants(&self) -> Result<Vec<CapabilityGrant>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, kind, value, created_at FROM capability_grants ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(CapabilityGrant {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    value: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_capability_grant(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM capability_grants WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- activity log (§6.1, §6.3) ----

    /// How many activity rows of one kind have ever been written. Used for
    /// `Vitality.skill_uses` (`SKL-5`): a skill lives in a folder the user
    /// owns and could edit outside the app, so its use count belongs in the
    /// log rather than in a counter written back into the file.
    pub fn count_activity(&self, kind: &str) -> Result<u32, DbError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM activity_log WHERE kind = ?1",
            params![kind],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as u32)
    }

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

    /// Every tool called during one conversation, most-used first (EVL-3).
    /// `add_tool_stat` already records the raw tool name per turn, so the eval
    /// harness can assert *which* tool answered a question without a second
    /// observability hook — the timeline events only carry prose descriptions.
    pub fn tools_used_in(&self, conversation_id: &str) -> Result<Vec<(String, i64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT tool_name, COUNT(*) FROM tool_stats
             WHERE conversation_id = ?1 GROUP BY tool_name ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| Ok((r.get(0)?, r.get(1)?)))?
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

    // ---- the record of a conversation's browsing (BRW-UI-1) ----

    /// Remember where this conversation's browsing got to. Called on every
    /// panel update, so the record survives the live session by construction
    /// rather than depending on a clean shutdown. Best-effort: losing the
    /// record must never break a browsing turn.
    pub fn save_browser_session(
        &self,
        conversation_id: &str,
        domain: &str,
        title: &str,
        screenshot: Option<&str>,
        trail: &[String],
    ) {
        let Ok(conn) = self.conn.lock() else { return };
        let Ok(trail_json) = serde_json::to_string(trail) else { return };
        let _ = conn.execute(
            "INSERT INTO browser_sessions(conversation_id, domain, title, screenshot, trail_json, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(conversation_id) DO UPDATE SET
               domain = excluded.domain, title = excluded.title,
               screenshot = excluded.screenshot, trail_json = excluded.trail_json,
               updated_at = excluded.updated_at",
            params![conversation_id, domain, title, screenshot, trail_json, now_ms()],
        );
    }

    /// What this conversation last browsed, if anything.
    ///
    /// Returns the raw row; the caller decides what to do about a `screenshot`
    /// path that no longer exists, because only it knows whether it's about to
    /// render the image or just report the visit.
    pub fn browser_session(
        &self,
        conversation_id: &str,
    ) -> Option<(String, String, Option<String>, Vec<String>)> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(
            "SELECT domain, title, screenshot, trail_json FROM browser_sessions
             WHERE conversation_id = ?1",
            params![conversation_id],
            |r| {
                let trail: String = r.get(3)?;
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    serde_json::from_str(&trail).unwrap_or_default(),
                ))
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Forget the record — the panel's "Dismiss", and cleanup when a
    /// conversation is deleted.
    pub fn delete_browser_session(&self, conversation_id: &str) {
        let Ok(conn) = self.conn.lock() else { return };
        let _ = conn.execute(
            "DELETE FROM browser_sessions WHERE conversation_id = ?1",
            params![conversation_id],
        );
    }

    // ---- fail→fix pairs (FIX-1): the mistake and the correction that followed ----

    /// Record one fail-then-succeed pair for the same tool in the same run.
    /// Best-effort, like `add_tool_stat` — this must never break a turn.
    pub fn add_tool_fix(
        &self,
        conversation_id: &str,
        tool_name: &str,
        failed_args: &str,
        error: &str,
        fixed_args: &str,
    ) {
        let Ok(conn) = self.conn.lock() else { return };
        let _ = conn.execute(
            "INSERT INTO tool_fixes(id, conversation_id, tool_name, failed_args, error, fixed_args, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![new_id(), conversation_id, tool_name, failed_args, error, fixed_args, now_ms()],
        );
    }

    /// Every fail→fix pair recorded in one conversation, newest first (FIX-2).
    pub fn tool_fixes_in(&self, conversation_id: &str) -> Result<Vec<ToolFix>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT tool_name, failed_args, error, fixed_args FROM tool_fixes
             WHERE conversation_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| {
                Ok(ToolFix {
                    tool_name: r.get(0)?,
                    failed_args: r.get(1)?,
                    error: r.get(2)?,
                    fixed_args: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Drop fail→fix rows older than `days` — content-bearing, so it is pruned
    /// on a much shorter horizon than the content-free `tool_stats`.
    pub fn prune_tool_fixes(&self, days: i64) -> Result<usize, DbError> {
        let cutoff = now_ms() - days * 24 * 60 * 60 * 1000;
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM tool_fixes WHERE created_at < ?1", params![cutoff])?;
        Ok(n)
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

    // ---- skill outcomes (OUT-1) ----

    /// Record one activation of a skill. `tool_failures` starts at 0 and is
    /// filled in by `backfill_skill_run_failures` once the run that activated
    /// it has finished.
    pub fn record_skill_run(&self, skill_name: &str, conversation_id: &str) {
        let Ok(conn) = self.conn.lock() else { return };
        let _ = conn.execute(
            "INSERT INTO skill_runs(id, skill_name, conversation_id, tool_failures, corrected, created_at)
             VALUES(?1, ?2, ?3, 0, 0, ?4)",
            params![new_id(), skill_name, conversation_id, now_ms()],
        );
    }

    /// Every skill activated in one conversation, without duplicates
    /// (`OUT-1`/`OUT-2`: reflection walks these to decide what to mark
    /// `corrected` and what to check for a rough run).
    pub fn skills_used_in(&self, conversation_id: &str) -> Result<Vec<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT skill_name FROM skill_runs WHERE conversation_id = ?1",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Fill in `tool_failures` for every skill activation in this conversation,
    /// counting tool calls that failed after each activation's own timestamp
    /// (so a skill used twice in one run is scored on what happened after
    /// *that* activation, not the whole conversation). Best-effort, called
    /// once at the end of `run_agent`.
    pub fn backfill_skill_run_failures(&self, conversation_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE skill_runs SET tool_failures = (
                SELECT COUNT(*) FROM tool_stats
                WHERE tool_stats.conversation_id = skill_runs.conversation_id
                  AND tool_stats.created_at >= skill_runs.created_at
                  AND tool_stats.ok = 0
             ) WHERE conversation_id = ?1",
            params![conversation_id],
        )?;
        Ok(())
    }

    /// Mark every skill activation in this conversation as having produced a
    /// lesson — reflection found something worth correcting here (`OUT-1`).
    pub fn mark_skill_runs_corrected(&self, conversation_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE skill_runs SET corrected = 1 WHERE conversation_id = ?1",
            params![conversation_id],
        )?;
        Ok(())
    }

    /// A skill's most recent activations, newest first (`OUT-2`'s "last 5
    /// runs" window).
    pub fn recent_skill_runs(&self, skill_name: &str, limit: i64) -> Result<Vec<SkillRunRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT conversation_id, tool_failures, corrected, created_at FROM skill_runs
             WHERE skill_name = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![skill_name, limit], |r| {
                Ok(SkillRunRow {
                    conversation_id: r.get(0)?,
                    tool_failures: r.get(1)?,
                    corrected: r.get::<_, i64>(2)? != 0,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// `(used, rough)` totals for a skill's row in the Skills tab (`SKL-UI-1`):
    /// every activation ever, and how many of them had at least one tool
    /// failure afterwards.
    pub fn skill_run_totals(&self, skill_name: &str) -> Result<(i64, i64), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*), SUM(tool_failures > 0) FROM skill_runs WHERE skill_name = ?1",
            params![skill_name],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0))),
        )
        .map_err(DbError::from)
    }

    /// Has a revision already been proposed for this skill (`OUT-2`)? Checked
    /// regardless of status — like `has_soul_escalation`, this is a once-ever
    /// guard so a skill the user already said "not now" to doesn't get
    /// re-proposed every night it stays rough. `slug` is the slugified skill
    /// name, matching what `propose_skill_revisions` stores.
    pub fn has_skill_revision_proposal(&self, slug: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM change_proposals WHERE target = 'skill-revision' AND slug = ?1",
            params![slug],
            |r| r.get(0),
        )?;
        Ok(n > 0)
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

    // ---- mail accounts (MAIL-1) ----

    pub fn add_mail_account(&self, a: &NewMailAccount) -> Result<MailAccount, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO mail_accounts(id, label, email, imap_host, imap_port, smtp_host, smtp_port, username, auth, security, enabled, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'password', ?9, 1, ?10)",
            params![id, a.label, a.email, a.imap_host, a.imap_port, a.smtp_host, a.smtp_port, a.username, a.security, ts],
        )?;
        Ok(MailAccount {
            id,
            label: a.label.clone(),
            email: a.email.clone(),
            imap_host: a.imap_host.clone(),
            imap_port: a.imap_port,
            smtp_host: a.smtp_host.clone(),
            smtp_port: a.smtp_port,
            username: a.username.clone(),
            auth: "password".to_string(),
            security: a.security.clone(),
            enabled: true,
            created_at: ts,
        })
    }

    pub fn list_mail_accounts(&self) -> Result<Vec<MailAccount>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, label, email, imap_host, imap_port, smtp_host, smtp_port, username, auth, security, enabled, created_at
             FROM mail_accounts ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], Self::map_mail_account)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_mail_account(&self, id: &str) -> Result<Option<MailAccount>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, label, email, imap_host, imap_port, smtp_host, smtp_port, username, auth, security, enabled, created_at
                 FROM mail_accounts WHERE id = ?1",
                [id],
                Self::map_mail_account,
            )
            .ok();
        Ok(row)
    }

    pub fn set_mail_account_enabled(&self, id: &str, enabled: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE mail_accounts SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )?;
        Ok(())
    }

    pub fn delete_mail_account(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM mail_accounts WHERE id = ?1", [id])?;
        Ok(())
    }

    fn map_mail_account(row: &rusqlite::Row) -> rusqlite::Result<MailAccount> {
        Ok(MailAccount {
            id: row.get(0)?,
            label: row.get(1)?,
            email: row.get(2)?,
            imap_host: row.get(3)?,
            imap_port: row.get(4)?,
            smtp_host: row.get(5)?,
            smtp_port: row.get(6)?,
            username: row.get(7)?,
            auth: row.get(8)?,
            security: row.get(9)?,
            enabled: row.get::<_, i64>(10)? != 0,
            created_at: row.get(11)?,
        })
    }

    // ---- artifacts (CHT-6) ----

    pub fn add_artifact(
        &self,
        conversation_id: Option<&str>,
        title: &str,
        kind: &str,
        content: &str,
    ) -> Result<Artifact, DbError> {
        self.add_artifact_with(conversation_id, title, kind, content, None, None)
    }

    /// Like `add_artifact`, plus the media metadata and lineage a generated
    /// image/video carries (Phase 13, `ART-1`).
    #[allow(clippy::too_many_arguments)]
    pub fn add_artifact_with(
        &self,
        conversation_id: Option<&str>,
        title: &str,
        kind: &str,
        content: &str,
        meta_json: Option<&str>,
        parent_id: Option<&str>,
    ) -> Result<Artifact, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO artifacts(id, conversation_id, title, kind, content, created_at, meta_json, parent_id)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, conversation_id, title, kind, content, ts, meta_json, parent_id],
        )?;
        Ok(Artifact {
            id,
            conversation_id: conversation_id.map(|s| s.to_string()),
            title: title.to_string(),
            kind: kind.to_string(),
            content: content.to_string(),
            created_at: ts,
            saved_path: None,
            meta_json: meta_json.map(|s| s.to_string()),
            parent_id: parent_id.map(|s| s.to_string()),
        })
    }

    /// What media has cost, this month and in total (`CST-2`). Read straight
    /// out of each artifact's `meta_json` rather than kept in a running column:
    /// the cost is already recorded per generation, and a derived total can
    /// never drift from the rows it came from.
    ///
    /// `since_ms` bounds the window; pass 0 for all time. Local generations
    /// have no `cost_usd` and simply contribute nothing to the money, which is
    /// itself the argument for local.
    pub fn media_spend(&self, since_ms: i64) -> Result<MediaSpend, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT kind, json_extract(meta_json, '$.cost_usd')
             FROM artifacts
             WHERE kind IN ('image', 'video') AND created_at >= ?1",
        )?;
        let rows = stmt.query_map(params![since_ms], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<f64>>(1)?))
        })?;
        let mut spend = MediaSpend::default();
        for row in rows {
            let (kind, cost) = row?;
            if kind == "video" {
                spend.videos += 1;
            } else {
                spend.images += 1;
            }
            spend.usd += cost.unwrap_or(0.0);
        }
        Ok(spend)
    }

    // ---- media jobs (`JOB-1`) ----

    pub fn add_media_job(
        &self,
        conversation_id: Option<&str>,
        message_id: Option<&str>,
        modality: &str,
        prompt: &str,
        model_id: Option<&str>,
        aspect_ratio: Option<&str>,
    ) -> Result<MediaJob, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let started_at = now_ms();
        conn.execute(
            "INSERT INTO media_jobs(id, conversation_id, message_id, modality, status, prompt,
                                    model_id, aspect_ratio, started_at)
             VALUES(?1, ?2, ?3, ?4, 'running', ?5, ?6, ?7, ?8)",
            params![id, conversation_id, message_id, modality, prompt, model_id, aspect_ratio, started_at],
        )?;
        Ok(MediaJob {
            id,
            conversation_id: conversation_id.map(str::to_string),
            message_id: message_id.map(str::to_string),
            modality: modality.to_string(),
            status: "running".to_string(),
            prompt: prompt.to_string(),
            model_id: model_id.map(str::to_string),
            aspect_ratio: aspect_ratio.map(str::to_string),
            started_at,
            finished_at: None,
            artifact_id: None,
            error: None,
        })
    }

    /// Close a job out. Only ever applied to a job still `running`, so a
    /// cancellation that lands at the same moment as a completion can't be
    /// overwritten by it — whichever gets there first is the outcome.
    pub fn finish_media_job(
        &self,
        id: &str,
        status: &str,
        artifact_id: Option<&str>,
        error: Option<&str>,
    ) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE media_jobs SET status = ?2, artifact_id = ?3, error = ?4, finished_at = ?5
             WHERE id = ?1 AND status = 'running'",
            params![id, status, artifact_id, error, now_ms()],
        )?;
        Ok(n > 0)
    }

    pub fn get_media_job(&self, id: &str) -> Result<Option<MediaJob>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, message_id, modality, status, prompt, model_id,
                    aspect_ratio, started_at, finished_at, artifact_id, error
             FROM media_jobs WHERE id = ?1",
        )?;
        Ok(stmt.query_row(params![id], Self::map_media_job).optional()?)
    }

    /// Jobs still `running` for a conversation — what the UI re-attaches to
    /// after a reload so a generation in flight isn't lost from the screen.
    pub fn list_running_media_jobs(&self, conversation_id: &str) -> Result<Vec<MediaJob>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, message_id, modality, status, prompt, model_id,
                    aspect_ratio, started_at, finished_at, artifact_id, error
             FROM media_jobs WHERE conversation_id = ?1 AND status = 'running'
             ORDER BY started_at",
        )?;
        let rows = stmt.query_map(params![conversation_id], Self::map_media_job)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Restart safety: nothing survives a process death, so any job still
    /// `running` at startup is one whose worker no longer exists. Marking it
    /// failed is what keeps a placeholder from spinning forever.
    pub fn fail_interrupted_media_jobs(&self) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE media_jobs SET status = 'failed', error = 'interrupted by restart',
                                   finished_at = ?1
             WHERE status = 'running'",
            params![now_ms()],
        )?)
    }

    fn map_media_job(r: &rusqlite::Row) -> rusqlite::Result<MediaJob> {
        Ok(MediaJob {
            id: r.get(0)?,
            conversation_id: r.get(1)?,
            message_id: r.get(2)?,
            modality: r.get(3)?,
            status: r.get(4)?,
            prompt: r.get(5)?,
            model_id: r.get(6)?,
            aspect_ratio: r.get(7)?,
            started_at: r.get(8)?,
            finished_at: r.get(9)?,
            artifact_id: r.get(10)?,
            error: r.get(11)?,
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
            meta_json: r.get(7)?,
            parent_id: r.get(8)?,
        })
    }

    pub fn list_artifacts(&self, conversation_id: &str) -> Result<Vec<Artifact>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, title, kind, content, created_at, saved_path, meta_json, parent_id
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
            "SELECT id, conversation_id, title, kind, content, created_at, saved_path, meta_json, parent_id
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
            "SELECT id, conversation_id, title, kind, content, created_at, saved_path, meta_json, parent_id
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
    /// Attach a file to an already-persisted message. The composer's paths send
    /// their attachments with the message itself; a toolset only learns it made
    /// one midway through the turn, so it needs this (`ART-2`).
    pub fn add_attachment(
        &self,
        message_id: &str,
        kind: &str,
        name: &str,
        path: &str,
        artifact_id: Option<&str>,
    ) -> Result<Attachment, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        conn.execute(
            "INSERT INTO attachments(id, message_id, kind, name, path, artifact_id)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, message_id, kind, name, path, artifact_id],
        )?;
        Ok(Attachment {
            id,
            kind: kind.to_string(),
            name: name.to_string(),
            path: path.to_string(),
            artifact_id: artifact_id.map(str::to_string),
        })
    }

    pub fn is_known_attachment(&self, path: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM attachments WHERE path = ?1",
            [path],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    /// Is `path` still an artifact's `content`? Checked after a conversation
    /// delete (which cascades that conversation's own artifact rows) before a
    /// generated file is removed from disk (`FIX-2`) — a still-referenced file
    /// must survive even if the conversation that first made it is gone.
    pub fn is_known_artifact_content(&self, path: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE content = ?1",
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
            "INSERT INTO personas(id, name, system_prompt, model_id, params_json, tools_json, skills_json, is_default, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)",
            params![id, p.name, p.system_prompt, p.model_id, p.params_json, p.tools_json, p.skills_json, ts],
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
            tools_json: p.tools_json.clone(),
            skills_json: p.skills_json.clone(),
        })
    }

    pub fn list_personas(&self) -> Result<Vec<Persona>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, system_prompt, model_id, params_json, is_default, created_at, updated_at, tools_json, skills_json
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
            "UPDATE personas SET name = ?2, system_prompt = ?3, model_id = ?4, params_json = ?5,
                                 tools_json = ?6, skills_json = ?7, updated_at = ?8
             WHERE id = ?1",
            params![p.id, p.name, p.system_prompt, p.model_id, p.params_json, p.tools_json, p.skills_json, now_ms()],
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
            tools_json: row.get(8)?,
            skills_json: row.get(9)?,
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
            role: row.get(6)?,
            is_default: row.get::<_, i64>(7)? != 0,
            added_at: row.get(8)?,
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

    /// `RPT-2`: the escalation is offered once per lesson, whatever the user
    /// answered. Re-asking after a "Not now" on every later recurrence would
    /// be nagging about a decision they already made.
    #[test]
    fn a_soul_escalation_is_only_ever_offered_once() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.has_soul_escalation("check-paths").unwrap());

        let p = db
            .add_change_proposal("soul", Some("check-paths"), "text", "learned 3×", None)
            .unwrap();
        assert!(db.has_soul_escalation("check-paths").unwrap());

        // Dismissed is still answered — it must not come back at recurrence 4.
        db.resolve_change_proposal(&p.id, "dismissed").unwrap();
        assert!(db.has_soul_escalation("check-paths").unwrap());

        // A different lesson is its own question.
        assert!(!db.has_soul_escalation("some-other-lesson").unwrap());
    }

    /// `OUT-1`: a skill activation's `tool_failures` counts only tool calls
    /// that happened *after* that activation, not ones already in the
    /// conversation before it fired — otherwise a skill would be blamed for
    /// mistakes that happened before it was ever read.
    #[test]
    fn skill_run_failures_are_backfilled_after_activation_not_before() {
        let db = Db::open_in_memory().unwrap();

        db.add_tool_stat("local", "read_file", "conv-1", false); // before activation
        db.record_skill_run("weekly-report", "conv-1");
        db.add_tool_stat("local", "write_file", "conv-1", false); // after — counts
        db.add_tool_stat("local", "write_file", "conv-1", true); // after, ok — doesn't count

        // Force a strict ordering independent of millisecond clock resolution.
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("UPDATE tool_stats SET created_at = 100 WHERE tool_name = 'read_file'", [])
                .unwrap();
            conn.execute("UPDATE skill_runs SET created_at = 200", []).unwrap();
            conn.execute("UPDATE tool_stats SET created_at = 300 WHERE tool_name = 'write_file'", [])
                .unwrap();
        }

        db.backfill_skill_run_failures("conv-1").unwrap();

        let runs = db.recent_skill_runs("weekly-report", 5).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].tool_failures, 1);
        assert!(!runs[0].corrected);

        let (used, rough) = db.skill_run_totals("weekly-report").unwrap();
        assert_eq!(used, 1);
        assert_eq!(rough, 1);

        assert_eq!(db.skills_used_in("conv-1").unwrap(), vec!["weekly-report".to_string()]);

        db.mark_skill_runs_corrected("conv-1").unwrap();
        assert!(db.recent_skill_runs("weekly-report", 5).unwrap()[0].corrected);
    }

    /// `OUT-2`: a revision is proposed at most once ever per skill — the same
    /// once-ever guard `RPT-2`'s soul escalation uses, so a "not now" doesn't
    /// bring the proposal back on the next rough run.
    #[test]
    fn skill_revision_proposal_guard_is_once_ever() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.has_skill_revision_proposal("flaky-skill").unwrap());

        let p = db
            .add_change_proposal(
                "skill-revision",
                Some("flaky-skill"),
                "revised body",
                SKILL_REVISION_RATIONALE,
                Some("does a thing"),
            )
            .unwrap();
        assert!(db.has_skill_revision_proposal("flaky-skill").unwrap());

        // Dismissed is still answered — it must not come back on the next
        // rough run.
        db.resolve_change_proposal(&p.id, "dismissed").unwrap();
        assert!(db.has_skill_revision_proposal("flaky-skill").unwrap());

        // A plain *install* proposal for a skill is a different question and
        // must not trip the revision guard.
        db.add_change_proposal("skill", Some("other-skill"), "body", "a fresh skill I wrote", None)
            .unwrap();
        assert!(!db.has_skill_revision_proposal("other-skill").unwrap());
    }

    /// `BRW-3`/`SYS-1`: "Always allow" persists, is idempotent under a second
    /// answer for the same domain, and is revocable like a folder grant.
    #[test]
    fn capability_grants_persist_and_revoke() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.has_capability_grant("domain", "example.com").unwrap());

        db.add_capability_grant("domain", "example.com").unwrap();
        assert!(db.has_capability_grant("domain", "example.com").unwrap());
        // A domain and an app can share a value without colliding.
        assert!(!db.has_capability_grant("open-app", "example.com").unwrap());

        // Answering "Always allow" twice for the same domain doesn't duplicate.
        db.add_capability_grant("domain", "example.com").unwrap();
        assert_eq!(db.list_capability_grants().unwrap().len(), 1);

        let grant = db.list_capability_grants().unwrap().into_iter().next().unwrap();
        db.delete_capability_grant(&grant.id).unwrap();
        assert!(!db.has_capability_grant("domain", "example.com").unwrap());
    }

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
                    artifact_id: None,
                }],
            },
        )
        .unwrap();

        assert!(db.is_known_attachment(r"C:\pics\shot.png").unwrap());
        assert!(!db.is_known_attachment(r"C:\secrets\passwords.txt").unwrap());
    }

    /// `ART-2`: the link from an inline attachment back to its artifact has to
    /// survive a reload, or a restarted conversation shows a generated image
    /// with no Save, no download and no provider line under it.
    #[test]
    fn an_attachment_remembers_the_artifact_it_renders() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Media", None, false).unwrap();
        let art = db
            .add_artifact_with(
                Some(&c.id),
                "a fox reading a map",
                "image",
                r"C:\media\fox.png",
                Some(r#"{"provider_label":"SDXL-Turbo","width":512,"height":512}"#),
                None,
            )
            .unwrap();
        let msg = db
            .append_message(
                &c.id,
                &NewMessage {
                    role: "assistant".into(),
                    content: String::new(),
                    model_name: Some("SDXL-Turbo".into()),
                    model_provenance: Some("local".into()),
                    steps_json: None,
                    attachments: vec![],
                },
            )
            .unwrap();
        db.add_attachment(&msg.id, "image", "fox.png", r"C:\media\fox.png", Some(&art.id))
            .unwrap();

        let reloaded = db.list_messages(&c.id).unwrap();
        let att = reloaded
            .iter()
            .flat_map(|m| &m.attachments)
            .find(|a| a.path == r"C:\media\fox.png")
            .expect("the attachment came back");
        assert_eq!(att.artifact_id.as_deref(), Some(art.id.as_str()));

        // And the artifact it points at still carries what the caption reads.
        let stored = db.list_artifacts(&c.id).unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].meta_json.as_deref().unwrap().contains("SDXL-Turbo"));
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
        assert_eq!(mem[0].kind.as_deref(), Some("fact"), "SEM-UI-1: kind rides along");
        assert_eq!(chat[0].kind, None, "a chat hit has no memory kind");

        // Raw user text must not break MATCH syntax (fts_escape).
        assert!(db.search_messages_fts("what about \"ZFS\" ?", 5).is_ok());
        assert!(db.search_messages_fts("NOT (broken", 5).unwrap().is_empty());
    }

    /// SEM-UI-4: "last surfaced" is tracked per collection+entry and updates
    /// in place rather than accumulating history.
    #[test]
    fn memory_usage_tracks_the_most_recent_surface() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.memory_usage_map("facts").unwrap().is_empty());

        db.touch_memory_usage("facts", &["a".to_string(), "b".to_string()]).unwrap();
        let first = db.memory_usage_map("facts").unwrap();
        assert_eq!(first.len(), 2);
        let first_a = first["a"];

        // A different collection is tracked separately.
        assert!(db.memory_usage_map("lessons").unwrap().is_empty());

        // Touching again updates in place, not a second row.
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.touch_memory_usage("facts", &["a".to_string()]).unwrap();
        let second = db.memory_usage_map("facts").unwrap();
        assert_eq!(second.len(), 2, "still one row per entry");
        assert!(second["a"] >= first_a);
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

    /// v7 (Perception): fresh installs get the vector tables and every v7
    /// column exists, on a plain SCHEMA-only apply (no upgrade path involved).
    #[test]
    fn schema_v7_tables_and_columns_exist() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();

        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        for table in ["vectors", "index_roots"] {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "table {table} should exist");
        }

        for (table, column) in [
            ("model_library", "role"),
            ("personas", "tools_json"),
            ("messages", "context_json"),
        ] {
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).unwrap();
            let names: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(names.contains(&column.to_string()), "{table}.{column} should exist");
        }
    }

    /// `TSET-3`: a toolset a user disabled before the `Skill`→`Toolset` rename
    /// (stored under the old `skill.<name>.enabled` key) must still be disabled
    /// after upgrading — the v9 migration copies it to `toolset.<name>.enabled`
    /// and deletes the old key, rather than orphaning it and silently letting
    /// the toolset default back on under the new key.
    #[test]
    fn a_pre_upgrade_disabled_toolset_stays_disabled_after_the_skill_to_toolset_migration() {
        let db = Db::open_in_memory().unwrap();
        // Simulate a pre-`TSET-3` install: user_version at 8, and the toolset
        // disabled under the old key (bypassing today's `Toolset::set_enabled`,
        // which already writes the new key).
        {
            let conn = db.conn.lock().unwrap();
            conn.pragma_update(None, "user_version", 8).unwrap();
        }
        db.set_setting("skill.web_search.enabled", "false").unwrap();

        db.migrate().unwrap();

        assert_eq!(db.get_setting("skill.web_search.enabled").unwrap(), None, "old key removed");
        assert_eq!(
            db.get_setting("toolset.web_search.enabled").unwrap(),
            Some("false".to_string()),
            "value carried over to the new key"
        );
        assert!(!crate::agent::toolsets::Toolset::WebSearch.is_enabled(&db));
    }

    /// `BRW-UI-1`: the browsing record has to outlive the live session, or a
    /// re-opened chat shows an empty panel beside a transcript full of visits.
    #[test]
    fn a_browsing_record_survives_the_session() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.browser_session("c1").is_none(), "nothing browsed yet");

        let trail = vec!["visited cloudflare.com".to_string(), "clicked \"Sign in\"".to_string()];
        db.save_browser_session("c1", "cloudflare.com", "Cloudflare", Some("/shots/a.png"), &trail);

        let (domain, title, shot, got) = db.browser_session("c1").expect("the visit is recorded");
        assert_eq!(domain, "cloudflare.com");
        assert_eq!(title, "Cloudflare");
        assert_eq!(shot.as_deref(), Some("/shots/a.png"));
        assert_eq!(got, trail);
    }

    /// One row per conversation: the panel shows where browsing *got to*, not
    /// every page it passed through.
    #[test]
    fn a_later_page_replaces_the_earlier_one() {
        let db = Db::open_in_memory().unwrap();
        db.save_browser_session("c1", "a.com", "A", None, &["visited a.com".to_string()]);
        db.save_browser_session("c1", "b.com", "B", None, &["visited b.com".to_string()]);

        let (domain, _, _, trail) = db.browser_session("c1").unwrap();
        assert_eq!(domain, "b.com");
        assert_eq!(trail, vec!["visited b.com".to_string()]);
    }

    #[test]
    fn dismissing_forgets_the_record_for_good() {
        let db = Db::open_in_memory().unwrap();
        db.save_browser_session("c1", "a.com", "A", None, &[]);
        db.delete_browser_session("c1");
        assert!(db.browser_session("c1").is_none(), "dismiss must not come back on re-open");
    }

    /// `SKL-5`/`SKL-4`: recipes became skills, and so did the autonomy class.
    /// A user who turned procedure-keeping off must not silently get it back
    /// under the new name — the *choice* migrates, not just the label.
    #[test]
    fn an_off_recipes_rung_stays_off_as_the_skills_rung() {
        let db = Db::open_in_memory().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.pragma_update(None, "user_version", 10).unwrap();
        }
        db.set_setting("autonomy.recipes", "off").unwrap();

        db.migrate().unwrap();

        assert_eq!(db.get_setting("autonomy.recipes").unwrap(), None, "old key removed");
        assert_eq!(
            crate::autonomy::autonomy_gate(&db, "skills"),
            crate::autonomy::Rung::Off,
            "the user's refusal carries across the rename"
        );
    }

    /// The rename must not overwrite a choice the user already made about
    /// skills — hence `INSERT OR IGNORE` rather than a plain insert.
    #[test]
    fn an_explicit_skills_rung_survives_the_recipes_migration() {
        let db = Db::open_in_memory().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.pragma_update(None, "user_version", 10).unwrap();
        }
        db.set_setting("autonomy.recipes", "off").unwrap();
        db.set_setting("autonomy.skills", "auto").unwrap();

        db.migrate().unwrap();

        assert_eq!(crate::autonomy::autonomy_gate(&db, "skills"), crate::autonomy::Rung::Auto);
    }

    fn a_model(name: &str) -> NewModelEntry {
        NewModelEntry {
            name: name.into(),
            path: format!("/models/{name}.gguf"),
            quant: None,
            size_bytes: None,
            vision: false,
        }
    }

    /// One table, three engines: installing an embedder must not disturb the
    /// chat default, and the Models view must never list it.
    #[test]
    fn model_roles_are_scoped_independently() {
        let db = Db::open_in_memory().unwrap();
        let chat = db.add_model(&a_model("qwen")).unwrap();
        let embed = db.add_model_with_role(&a_model("bge"), "embed").unwrap();

        assert!(chat.is_default, "the first chat model becomes the chat default");
        assert!(embed.is_default, "the first embedder becomes the embed default");
        assert_eq!(db.default_model_by_role("chat").unwrap().unwrap().id, chat.id);
        assert_eq!(db.default_model_by_role("embed").unwrap().unwrap().id, embed.id);

        let listed = db.list_models().unwrap();
        assert_eq!(listed.len(), 1, "the Models view shows chat models only");
        assert_eq!(listed[0].id, chat.id);
    }

    #[test]
    fn setting_a_default_does_not_cross_roles() {
        let db = Db::open_in_memory().unwrap();
        let chat = db.add_model(&a_model("qwen")).unwrap();
        let second = db.add_model_with_role(&a_model("nomic"), "embed").unwrap();
        db.add_model_with_role(&a_model("bge"), "embed").unwrap();

        db.set_default_model(&second.id).unwrap();
        assert_eq!(db.default_model_by_role("embed").unwrap().unwrap().id, second.id);
        assert_eq!(
            db.default_model_by_role("chat").unwrap().unwrap().id,
            chat.id,
            "changing the embedder must leave the chat default alone"
        );
    }

    /// Deleting the default used to leave the role with none, so the setup
    /// status read "not installed" while another model sat on disk.
    #[test]
    fn deleting_a_default_promotes_the_next_model_in_that_role() {
        let db = Db::open_in_memory().unwrap();
        let first = db.add_model_with_role(&a_model("bge"), "embed").unwrap();
        let second = db.add_model_with_role(&a_model("nomic"), "embed").unwrap();
        assert!(first.is_default && !second.is_default);

        let path = db.delete_model(&first.id).unwrap();
        assert_eq!(path.as_deref(), Some("/models/bge.gguf"));
        assert_eq!(
            db.default_model_by_role("embed").unwrap().unwrap().id,
            second.id,
            "the survivor should have been promoted"
        );
    }

    #[test]
    fn deleting_the_last_model_in_a_role_leaves_no_default() {
        let db = Db::open_in_memory().unwrap();
        let only = db.add_model_with_role(&a_model("bge"), "embed").unwrap();
        db.delete_model(&only.id).unwrap();
        assert!(db.default_model_by_role("embed").unwrap().is_none());
    }
}
