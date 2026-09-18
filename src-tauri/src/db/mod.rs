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
const SCHEMA_VERSION: i64 = 29;

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
pub struct MessageHit {
    pub conversation_id: String,
    pub snippet: String,
}

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
    /// `SUB-3`: set when this conversation is a delegated child's workspace.
    /// The Rail keeps these out of the top-level list — they belong to the turn
    /// that started them, not beside it.
    #[serde(default)]
    pub parent_conversation_id: Option<String>,
    /// `PRJ-1`: the project this conversation belongs to, if any. When it is
    /// set, the project owns the working folder and the trust level and the
    /// two columns above are ignored — see `Db::conversation_folder`.
    #[serde(default)]
    pub project_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// `PRJ-1`: a named group of sessions that share a context.
///
/// A working directory is something a project may *have* (`PRJ-1a`), not what
/// it is — a project about a book, a job or a person is as real as one about a
/// repository, and none of those live in a folder.
///
/// The user never has to think about this either way. Attaching a folder to a
/// loose chat creates or joins that folder's project (`PRJ-3`), so the second
/// time you open a folder the trust you granted is already there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Canonical, and unique across projects — this is what makes attaching a
    /// known folder a join rather than a duplicate. `None` for a project that
    /// is not about a directory, which is most of them.
    pub root_path: Option<String>,
    /// `PRJ-7`: free text injected into every session in this project, after
    /// the standing instructions and before the memory index. The persona
    /// still governs voice, format and depth.
    pub instructions: Option<String>,
    /// "read-only" | "confirm" | "auto", the same vocabulary
    /// `permissions::Trust` parses. The plan's schema wrote "trusted" for the
    /// top level; the app has always called it "auto" and one name is better
    /// than two.
    pub trust: String,
    /// `COD-7`: "off" | "ask" | "allow", or "inherit" to follow the default
    /// set in Settings -> Tools. A new project inherits.
    pub exec_policy: String,
    /// `COD-1` detection result, unread until Phase 1.
    pub card_json: Option<String>,
    pub card_built_at: Option<i64>,
    /// `COD-7`/`COD-8`: what the user said "always allow" to in this project,
    /// as `{"tasks": [...], "commands": [...], "run_command": bool}`.
    pub allow_json: Option<String>,
    /// `SHELL_PLAN`'s `SHL-17` scope seam: the open tab set for this project.
    pub tabs_json: Option<String>,
    /// Archived hides the project and its sessions from the Rail. Nothing on
    /// disk is ever touched — there is no delete, because the word would be
    /// read as "delete my code".
    pub archived: bool,
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
    /// `SUB-3`: one line saying when to hand this agent a job. This is what the
    /// lead reads to choose. Empty or `NULL` means it is not offered.
    #[serde(default)]
    pub description: Option<String>,
    /// `SUB-3`: may the agent delegate to this one at all. Off by default.
    #[serde(default)]
    pub spawnable: bool,
}

/// One delegated child run (`SUB-3`). The child's work is a conversation of its
/// own; this is the link back to the turn that asked for it.
#[derive(Debug, Clone, Serialize)]
pub struct SubagentRun {
    pub id: String,
    pub parent_conversation_id: String,
    pub parent_message_id: Option<String>,
    pub child_conversation_id: String,
    /// The agent type: a persona's name, or `general`.
    pub agent: String,
    pub task: String,
    /// `running` | `done` | `stopped` | `error`.
    pub status: String,
    /// `StopReason::as_str`, once it has one.
    pub stop_reason: Option<String>,
    /// The child's final text — what the lead was handed.
    pub result: Option<String>,
    pub steps: usize,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

/// One row of the session log (`CTX-2`). The payload is kept as text rather
/// than parsed here: the log's job is to hand back exactly the bytes that went
/// in, and a parse in the storage layer is a place for them to change.
#[derive(Debug, Clone, Serialize)]
pub struct SessionEvent {
    pub seq: i64,
    pub kind: String,
    pub payload_json: String,
    /// When the row was written. Carried because a `summary` row is shown to the
    /// user as a moment in the conversation ("this is where I compressed the
    /// earlier part"), and a moment with no time on it cannot be placed.
    pub created_at: i64,
}

/// One line of the Usage panel (`OBS-2`): a day, a model, or a conversation.
#[derive(Debug, Default, Clone, Serialize)]
pub struct UsageBucket {
    /// The grouping value: a UTC day start in epoch ms, a model name, or a
    /// conversation id, depending on which list this came from.
    pub key: String,
    /// A human name for `key` where one exists — a conversation's title. `None`
    /// for a conversation that has since been deleted: the spend still counts,
    /// it just has no name any more.
    pub label: Option<String>,
    /// `local` | `cloud` | `endpoint`. On a day or conversation bucket this is
    /// whichever row landed first, so the UI treats it as a hint, not a fact.
    pub provenance: String,
    pub prompt_tokens: u64,
    pub output_tokens: u64,
    pub runs: u64,
}

impl UsageBucket {
    pub fn total_tokens(&self) -> u64 {
        self.prompt_tokens + self.output_tokens
    }
}

/// What was spent over a window, grouped three ways (`OBS-2`).
#[derive(Debug, Default, Clone, Serialize)]
pub struct UsageSummary {
    pub total: UsageBucket,
    /// Newest day first.
    pub by_day: Vec<UsageBucket>,
    /// Biggest spender first.
    pub by_model: Vec<UsageBucket>,
    pub by_conversation: Vec<UsageBucket>,
}

/// The start of the UTC day `ms` falls in, as epoch ms in a string so it both
/// sorts and formats. Grouping is UTC; the frontend labels it in local time,
/// which can put a late-evening run on the next day's line. That is a smaller
/// wrong than pulling in a timezone database for a spend panel.
fn day_key(ms: i64) -> String {
    const DAY: i64 = 86_400_000;
    (ms - ms.rem_euclid(DAY)).to_string()
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
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub spawnable: bool,
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
    /// `HRN-3`: why the run behind this turn stopped. `None` on user turns and
    /// on anything written before the column existed.
    pub stop_reason: Option<String>,
    /// `PLN-UI-5`: the plan this turn's run worked to, serialized as
    /// `agent::plan::Plan`. `None` for a turn that never wrote one.
    pub plan_json: Option<String>,
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
    /// The assistant turn that produced this artifact, so a reloaded
    /// conversation can still show its inline chip in the message stream.
    pub message_id: Option<String>,
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

/// A user's own OpenAI-compatible model server (Ollama, LM Studio, or a
/// remote box). The API key, if any, is **not** stored here — it lives in the
/// OS credential store (`secrets::SERVICE_ENDPOINT`, account = this row's
/// `id`); only connection metadata lives in SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalEndpointRow {
    pub id: String,
    pub label: String,
    pub base_url: String,
    pub kind: String,
    pub ctx_size: i64,
    pub enabled: bool,
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

/// `PRJ-3`: a project created implicitly is named after the folder's last path
/// segment, which is what the user calls it anyway. Falls back to the whole
/// path for a drive root, where there is no segment to take.
pub fn project_name_for(root_path: &str) -> String {
    root_path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(root_path)
        .to_string()
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
        if current < 18 {
            // v18: collapse library rows that name the same file. Leaving the
            // Models view mid-download reverted the button to "Download", so a
            // second click started a *concurrent* fetch of the same path; each
            // one registered its own row, and one model appeared two or three
            // times over. Worse, deleting one copy removed the shared file out
            // from under the rest. Keep the earliest row per path.
            conn.execute(
                "DELETE FROM model_library
                 WHERE rowid NOT IN (SELECT MIN(rowid) FROM model_library GROUP BY path)",
                [],
            )?;
            // If the row that carried a role's default was one of the copies
            // just removed, give that role its default back.
            conn.execute(
                "UPDATE model_library SET is_default = 1
                 WHERE rowid IN (
                     SELECT MIN(rowid) FROM model_library m
                     WHERE NOT EXISTS (
                         SELECT 1 FROM model_library d
                         WHERE d.role = m.role AND d.is_default = 1
                     )
                     GROUP BY role
                 )",
                [],
            )?;
        }
        if current < 19 {
            // v19: an artifact remembers which assistant turn produced it. Without
            // this, reopening a conversation loses the link between a non-media
            // artifact (document/code/svg/html) and its message, so the inline
            // chip that opened it in the Workbench never comes back — only the
            // Workbench's own artifact list, which loads independently of any
            // message, still shows it.
            Self::add_column(&conn, "artifacts", "message_id", "TEXT")?;
        }
        if current < 20 {
            // v20 backfills what v19 only made room for, and is deliberately its
            // own version rather than more code in the block above: a database
            // that already reached 19 would never run that block again, so a
            // backfill added there strands exactly the conversations it was
            // written to rescue. Re-running this is harmless — it only ever
            // touches rows that are still NULL.
            //
            // Media needs no guesswork: the attachment row that renders it
            // already names both the artifact and the message it sits in
            // (`ART-2`).
            conn.execute(
                "UPDATE artifacts SET message_id = (
                     SELECT at.message_id FROM attachments at
                     WHERE at.artifact_id = artifacts.id AND at.message_id IS NOT NULL
                     LIMIT 1
                 )
                 WHERE message_id IS NULL
                   AND EXISTS (
                       SELECT 1 FROM attachments at WHERE at.artifact_id = artifacts.id
                   )",
                [],
            )?;
            // A document/code/svg/html artifact has no such link, so fall back to
            // time. The assistant row is persisted *before* its turn runs, so the
            // newest assistant message at or before the artifact's timestamp is
            // the turn that made it. A heuristic — but a chip on a neighbouring
            // old turn beats an artifact that never appears in the stream again.
            // `rowid` breaks the tie when two turns share a millisecond, so the
            // result is at least deterministic rather than whichever row SQLite
            // happened to reach first.
            conn.execute(
                "UPDATE artifacts SET message_id = (
                     SELECT m.id FROM messages m
                     WHERE m.conversation_id = artifacts.conversation_id
                       AND m.role = 'assistant'
                       AND m.created_at <= artifacts.created_at
                     ORDER BY m.created_at DESC, m.rowid DESC
                     LIMIT 1
                 )
                 WHERE message_id IS NULL AND conversation_id IS NOT NULL",
                [],
            )?;
        }
        // v21: `local_endpoints` is created by SCHEMA above — a brand-new
        // table needs no `ALTER TABLE` block, same as v13's `tool_fixes`,
        // v14's `browser_sessions`, and v17's `media_jobs`.
        if current < 22 {
            // `HRN-3`: why an assistant turn ended. NULL for every row written
            // before this, which reads as "finished" — the only honest guess,
            // since a stopped run used to be indistinguishable from a short one.
            Self::add_column(&conn, "messages", "stop_reason", "TEXT")?;
        }
        if current < 23 {
            // v23 (`SUB-3`): personas become agent types. `description` is what
            // the lead reads when deciding who to hand a job to, and `spawnable`
            // is the user's say in whether it may be handed one at all — off by
            // default, so an upgrade never silently makes every persona
            // delegatable. `subagent_runs` is created by SCHEMA above.
            Self::add_column(&conn, "personas", "description", "TEXT")?;
            Self::add_column(&conn, "personas", "spawnable", "INTEGER NOT NULL DEFAULT 0")?;
            // A child conversation is a real conversation, so the Rail needs a
            // way to tell one apart and keep it out of the top-level list.
            Self::add_column(&conn, "conversations", "parent_conversation_id", "TEXT")?;
        }
        if current < 26 {
            // v26 (`PLN-UI-5`): the plan a run worked to, kept on the turn that
            // ran it. A plan that vanished on reload was never state, it was
            // decoration — and the session log cannot stand in for this, since
            // its rows are keyed by run and the transcript is drawn by message.
            Self::add_column(&conn, "messages", "plan_json", "TEXT")?;
        }
        if current < 27 {
            // v27 (`PRJ-1`/`PRJ-2`): the project entity. `projects` is created
            // by SCHEMA above; this links conversations to it and backfills
            // one project per folder anyone has ever worked in.
            Self::add_column(&conn, "conversations", "project_id", "TEXT")?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_conversations_project
                 ON conversations(project_id)",
                [],
            )?;
            // Every distinct folder becomes a project named after its last
            // path segment. Where several chats attached the same folder with
            // different trust levels, the **most restrictive** one wins:
            // silently widening what the agent may do to somebody's code
            // because of an upgrade is the one outcome that is not
            // recoverable.
            let mut stmt = conn.prepare(
                "SELECT folder_path,
                        MIN(CASE folder_trust
                              WHEN 'read-only' THEN 0
                              WHEN 'auto' THEN 2
                              ELSE 1
                            END)
                 FROM conversations
                 WHERE folder_path IS NOT NULL AND folder_path <> ''
                 GROUP BY folder_path",
            )?;
            let folders: Vec<(String, i64)> = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            let ts = now_ms();
            for (path, rank) in folders {
                let trust = match rank {
                    0 => "read-only",
                    2 => "auto",
                    _ => "confirm",
                };
                let id = new_id();
                // `ON CONFLICT DO NOTHING` rather than a lookup first: a row
                // may already exist if this migration is re-reached on a
                // database that was rolled forward once before.
                conn.execute(
                    "INSERT INTO projects(id, name, root_path, trust, created_at, updated_at)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?5)
                     ON CONFLICT(root_path) DO NOTHING",
                    params![id, project_name_for(&path), path, trust, ts],
                )?;
                conn.execute(
                    "UPDATE conversations
                     SET project_id = (SELECT id FROM projects WHERE root_path = ?1)
                     WHERE folder_path = ?1 AND project_id IS NULL",
                    params![path],
                )?;
            }
        }
        if current < 28 {
            // v28 (`PRJ-1a`): a project stops being a folder wearing a
            // project's clothes. `root_path` becomes nullable so a project can
            // be about a book, a job, or a person — the things people actually
            // keep coming back to — and gains `instructions` (`PRJ-7`).
            //
            // SQLite cannot relax `NOT NULL` in place, so the table is rebuilt
            // and every row copied. Guarded on the old shape actually being
            // there: SCHEMA above already creates the new one on a fresh
            // database, and rebuilding that would be a no-op that still risks
            // the copy.
            let needs_rebuild: bool = {
                let mut stmt = conn.prepare("PRAGMA table_info(projects)")?;
                let cols: Vec<(String, i64)> = stmt
                    .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(3)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                cols.iter().any(|(name, notnull)| name == "root_path" && *notnull == 1)
            };
            if needs_rebuild {
                conn.execute_batch(
                    "CREATE TABLE projects_new (
                       id            TEXT PRIMARY KEY,
                       name          TEXT NOT NULL,
                       root_path     TEXT UNIQUE,
                       instructions  TEXT,
                       trust         TEXT NOT NULL DEFAULT 'confirm',
                       exec_policy   TEXT NOT NULL DEFAULT 'ask',
                       card_json     TEXT,
                       card_built_at INTEGER,
                       tabs_json     TEXT,
                       archived      INTEGER NOT NULL DEFAULT 0,
                       created_at    INTEGER NOT NULL,
                       updated_at    INTEGER NOT NULL
                     );
                     INSERT INTO projects_new
                       (id, name, root_path, trust, exec_policy, card_json,
                        card_built_at, tabs_json, archived, created_at, updated_at)
                       SELECT id, name, root_path, trust, exec_policy, card_json,
                              card_built_at, tabs_json, archived, created_at, updated_at
                       FROM projects;
                     DROP TABLE projects;
                     ALTER TABLE projects_new RENAME TO projects;",
                )?;
            } else {
                // A database that reached the new SCHEMA first still needs the
                // column, since `CREATE TABLE IF NOT EXISTS` skipped an
                // existing v27 table that has everything but this.
                Self::add_column(&conn, "projects", "instructions", "TEXT")?;
            }
        }
        if current < 29 {
            // v29 (`COD-7`/`COD-8`): the project's allowlist, and `exec_policy`
            // learns "inherit". Every existing row still carries the column's
            // old default, which nothing ever read or let the user set, so it
            // becomes "inherit" rather than being mistaken for a choice.
            Self::add_column(&conn, "projects", "allow_json", "TEXT")?;
            conn.execute("UPDATE projects SET exec_policy = 'inherit' WHERE exec_policy = 'ask'", [])?;
        }
        // v24 (`OBS-2`): `run_usage` is created by SCHEMA above and has no
        // columns to add to an existing table, so there is nothing to do here
        // beyond bumping the version. v25 (`CTX-2`) adds `session_events` the
        // same way — a new table, no ALTER.
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
            parent_conversation_id: None,
            project_id: None,
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
    /// in. Detaching touches nothing on disk — it only forgets the path, and
    /// leaves the project and its other sessions alone (`PRJ-3`).
    ///
    /// Attaching creates or joins the project for that folder. The path must
    /// already be canonical; the caller does that, because canonicalising is a
    /// filesystem question and this layer is not the one to ask it.
    pub fn set_conversation_folder(&self, id: &str, path: Option<&str>) -> Result<(), DbError> {
        let current = self.conversation_project(id)?;
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            // Detaching from *this chat* means this chat leaves (`PRJ-3`). It
            // deliberately does not clear the project's folder: that control
            // lives on the chat, and a control on one chat must never change
            // what every sibling session is working in. Removing the folder
            // *from the project* is `set_project_root`, in the project view,
            // where the thing being changed is visibly the project.
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "UPDATE conversations SET folder_path = NULL, project_id = NULL WHERE id = ?1",
                params![id],
            )?;
            return Ok(());
        };

        // `PRJ-3a`, the three cases. Guessing between them is how folders get
        // silently swapped out from under somebody's other sessions.
        let project_id = match current {
            // Already this folder: nothing to do but keep the columns honest.
            Some(p) if p.root_path.as_deref() == Some(path) => p.id,
            // The project adopts the folder, or moves to the new one. This is
            // what makes "start a project, add a folder later" work.
            Some(p) if self.set_project_root(&p.id, Some(path))? => p.id,
            // Another project already owns that root, and `root_path` is
            // unique, so the folder's project wins and the conversation moves
            // to it rather than the folder being stolen.
            Some(_) => self.project_for_root(path)?.id,
            // A loose chat creates or joins the folder's project (`PRJ-3`).
            None => self.project_for_root(path)?.id,
        };

        let conn = self.conn.lock().unwrap();
        // `folder_path` is written too, not just `project_id`. The project is
        // the owner now, but the legacy column is what a conversation falls
        // back to, and leaving the two disagreeing is how a detached project
        // would resurrect an old folder.
        conn.execute(
            "UPDATE conversations SET folder_path = ?2, project_id = ?3 WHERE id = ?1",
            params![id, path, project_id],
        )?;
        Ok(())
    }

    /// The project a conversation belongs to, if any.
    pub fn conversation_project(&self, id: &str) -> Result<Option<Project>, DbError> {
        let project_id: Option<String> = {
            let conn = self.conn.lock().unwrap();
            conn.query_row("SELECT project_id FROM conversations WHERE id = ?1", [id], |r| r.get(0))
                .unwrap_or(None)
        };
        match project_id {
            Some(pid) => self.get_project(&pid),
            None => Ok(None),
        }
    }

    /// Set how much the agent may do inside the attached folder.
    ///
    /// With a project attached this grants trust **for the folder**, once,
    /// rather than once per chat — which is most of the daily annoyance gone.
    pub fn set_conversation_trust(&self, id: &str, trust: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let project_id: Option<String> = conn
            .query_row("SELECT project_id FROM conversations WHERE id = ?1", [id], |r| r.get(0))
            .unwrap_or(None);
        // Both are written: the project is what `conversation_folder` reads,
        // and the legacy column keeps the chat honest if it later leaves.
        conn.execute(
            "UPDATE conversations SET folder_trust = ?2 WHERE id = ?1",
            params![id, trust],
        )?;
        if let Some(project_id) = project_id {
            conn.execute(
                "UPDATE projects SET trust = ?2, updated_at = ?3 WHERE id = ?1",
                params![project_id, trust, now_ms()],
            )?;
        }
        Ok(())
    }

    /// `PRJ-2`: the working folder and trust for a conversation — the
    /// project's when it has one, the legacy per-conversation columns when it
    /// does not.
    ///
    /// Hot path: every file tool call consults this. The signature is
    /// unchanged, which is the whole point of doing it this way — the project
    /// entity lands *under* the app rather than through it, and none of the
    /// callers in `codeexec`, `filesystem`, `retrieval` or `skillpack` had to
    /// learn a new concept.
    pub fn conversation_folder(&self, id: &str) -> Result<(Option<String>, String), DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT COALESCE(p.root_path, c.folder_path),
                        COALESCE(p.trust, c.folder_trust)
                 FROM conversations c
                 LEFT JOIN projects p ON p.id = c.project_id
                 WHERE c.id = ?1",
                [id],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .unwrap_or((None, None));
        Ok((row.0, row.1.unwrap_or_else(|| "confirm".to_string())))
    }

    // ---- projects (`PRJ-1`) ----

    /// The project for this canonical root, creating one named after the
    /// folder if it is new (`PRJ-3`). Un-archives a project that was hidden:
    /// working in a folder again is the plainest possible statement that you
    /// still want it.
    pub fn project_for_root(&self, root_path: &str) -> Result<Project, DbError> {
        if let Some(existing) = self.project_by_root(root_path)? {
            if existing.archived {
                let conn = self.conn.lock().unwrap();
                conn.execute(
                    "UPDATE projects SET archived = 0, updated_at = ?2 WHERE id = ?1",
                    params![existing.id, now_ms()],
                )?;
            }
            return Ok(Project { archived: false, ..existing });
        }
        self.create_project(&project_name_for(root_path), Some(root_path))
    }

    pub fn project_by_root(&self, root_path: &str) -> Result<Option<Project>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, name, root_path, instructions, trust, exec_policy, card_json,
                        card_built_at, tabs_json, archived, created_at, updated_at, allow_json
                 FROM projects WHERE root_path = ?1",
                [root_path],
                Self::map_project,
            )
            .ok();
        Ok(row)
    }

    pub fn get_project(&self, id: &str) -> Result<Option<Project>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, name, root_path, instructions, trust, exec_policy, card_json,
                        card_built_at, tabs_json, archived, created_at, updated_at, allow_json
                 FROM projects WHERE id = ?1",
                [id],
                Self::map_project,
            )
            .ok();
        Ok(row)
    }

    /// `PRJ-3` explicit create. `root_path` is optional (`PRJ-1a`): most
    /// projects are not about a directory.
    ///
    /// A second call for the same root returns the project that is already
    /// there rather than failing on the unique index — "New project" on a
    /// folder you already have open should land you in it. Two folderless
    /// projects with the same name are two projects, because there is nothing
    /// to say they are the same one.
    pub fn create_project(&self, name: &str, root_path: Option<&str>) -> Result<Project, DbError> {
        if let Some(root) = root_path {
            if let Some(existing) = self.project_by_root(root)? {
                return Ok(existing);
            }
        }
        let ts = now_ms();
        let project = Project {
            id: new_id(),
            name: name.to_string(),
            root_path: root_path.map(|s| s.to_string()),
            instructions: None,
            trust: "confirm".to_string(),
            exec_policy: "inherit".to_string(),
            card_json: None,
            card_built_at: None,
            allow_json: None,
            tabs_json: None,
            archived: false,
            created_at: ts,
            updated_at: ts,
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO projects(id, name, root_path, trust, exec_policy, archived, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
            params![
                project.id,
                project.name,
                project.root_path,
                project.trust,
                project.exec_policy,
                ts
            ],
        )?;
        Ok(project)
    }

    /// `PRJ-7`: the instructions every session in this project carries.
    pub fn set_project_instructions(&self, id: &str, text: Option<&str>) -> Result<(), DbError> {
        let text = text.map(str::trim).filter(|t| !t.is_empty());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET instructions = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, text, now_ms()],
        )?;
        Ok(())
    }

    /// `PRJ-3a`: give a project a working folder, or with `None` take it away.
    ///
    /// The folder is a property of the project, so removing it leaves both the
    /// project and its sessions exactly where they are. Returns `false` when
    /// another project already owns that root — the caller then moves the
    /// conversation there instead, because `root_path` is unique and the
    /// folder's own project has to win.
    pub fn set_project_root(&self, id: &str, root_path: Option<&str>) -> Result<bool, DbError> {
        if let Some(root) = root_path {
            if let Some(other) = self.project_by_root(root)? {
                if other.id != id {
                    return Ok(false);
                }
            }
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            // A different folder is a different card (`COD-1`): the old one
            // described files that are no longer the project's.
            "UPDATE projects SET root_path = ?2, card_json = NULL, card_built_at = NULL, updated_at = ?3
             WHERE id = ?1 AND root_path IS NOT ?2",
            params![id, root_path, now_ms()],
        )?;
        // The sessions' own fallback column follows, so a chat that later
        // leaves the project does not resurrect a folder the project dropped.
        conn.execute(
            "UPDATE conversations SET folder_path = ?2 WHERE project_id = ?1",
            params![id, root_path],
        )?;
        Ok(true)
    }

    /// Every project, archived ones only when asked for. Newest activity
    /// first, matching how the Rail orders everything else.
    pub fn list_projects(&self, include_archived: bool) -> Result<Vec<Project>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, root_path, instructions, trust, exec_policy, card_json,
                    card_built_at, tabs_json, archived, created_at, updated_at, allow_json
             FROM projects
             WHERE ?1 OR archived = 0
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map([include_archived], Self::map_project)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn rename_project(&self, id: &str, name: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, now_ms()],
        )?;
        Ok(())
    }

    /// `COD-1`: the detected project card, or `None` to have it detected again.
    pub fn set_project_card(&self, id: &str, card_json: Option<&str>) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET card_json = ?2, card_built_at = ?3 WHERE id = ?1",
            params![id, card_json, card_json.map(|_| now_ms())],
        )?;
        Ok(())
    }

    /// `COD-7`: "off" | "ask" | "allow" | "inherit". The caller validates.
    pub fn set_project_exec_policy(&self, id: &str, policy: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET exec_policy = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, policy, now_ms()],
        )?;
        Ok(())
    }

    /// `COD-7`/`COD-8`: the project's standing "always allow" answers.
    pub fn set_project_allow(&self, id: &str, allow_json: Option<&str>) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET allow_json = ?2 WHERE id = ?1",
            params![id, allow_json],
        )?;
        Ok(())
    }

    pub fn set_project_trust(&self, id: &str, trust: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET trust = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, trust, now_ms()],
        )?;
        Ok(())
    }

    /// `PRJ-3`: archive hides the project and its sessions. There is no
    /// delete, and nothing on disk is touched either way.
    pub fn set_project_archived(&self, id: &str, archived: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET archived = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, archived as i64, now_ms()],
        )?;
        Ok(())
    }

    /// The open tab set for a project (`SHL-17`'s scope seam, filled by
    /// `PRJ-UI-2`). One project's tabs never reach another's.
    pub fn set_project_tabs(&self, id: &str, tabs_json: Option<&str>) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET tabs_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, tabs_json, now_ms()],
        )?;
        Ok(())
    }

    /// Put a conversation in a project (or, with `None`, take it out of one).
    /// Joining carries the project's root onto the chat so the two never
    /// disagree about which folder is open.
    pub fn set_conversation_project(
        &self,
        conversation_id: &str,
        project_id: Option<&str>,
    ) -> Result<(), DbError> {
        let root = match project_id {
            Some(pid) => self.get_project(pid)?.map(|p| p.root_path),
            None => None,
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET project_id = ?2, folder_path = ?3 WHERE id = ?1",
            params![conversation_id, project_id, root],
        )?;
        Ok(())
    }

    pub fn list_conversations(&self) -> Result<Vec<Conversation>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, model_id, persona_id, overrides_json, workspace, created_at, updated_at,
                    summary, summary_upto_message_id, reflected_at, folder_path, folder_trust,
                    parent_conversation_id, project_id
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
            stop_reason: None,
            // A turn is appended before its run starts; the plan arrives with
            // `finalize_message`, once the run has one.
            plan_json: None,
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
        // `HRN-3`. `None` leaves the column alone rather than clearing it, so a
        // caller that doesn't know can't erase what the run reported.
        stop_reason: Option<&str>,
        // `PLN-UI-5`, and `COALESCE` for the same reason: most turns have no
        // plan, and none of them should be able to wipe one that exists.
        plan_json: Option<&str>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages
                SET content = ?2, steps_json = ?3, context_json = ?4,
                    stop_reason = COALESCE(?5, stop_reason),
                    plan_json = COALESCE(?6, plan_json)
              WHERE id = ?1",
            params![id, content, steps_json, context_json, stop_reason, plan_json],
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
                        summary, summary_upto_message_id, reflected_at, folder_path, folder_trust,
                        parent_conversation_id, project_id
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
                "SELECT id, name, system_prompt, model_id, params_json, is_default, created_at, updated_at, tools_json, skills_json, description, spawnable
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
            "SELECT id, conversation_id, role, content, model_name, model_provenance, steps_json, stop_reason, created_at, plan_json
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
            "SELECT id, conversation_id, role, content, model_name, model_provenance, steps_json, stop_reason, created_at, plan_json
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

    /// `REF-3b`: conversations that are finished, substantial enough to teach
    /// something (`min_messages`), and were never reflected on — newest first.
    ///
    /// The frontend cannot ask this question itself: `list_conversations` hands
    /// it rows with empty `messages`, so a client-side sweep would judge every
    /// conversation as too slight and reflect none of them. Counting here, where
    /// the messages actually live, is what makes the catch-up pass possible.
    pub fn unreflected_conversations(
        &self,
        min_messages: i64,
    ) -> Result<Vec<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT c.id FROM conversations c
             WHERE c.reflected_at IS NULL
               AND (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = c.id) >= ?1
             ORDER BY c.updated_at DESC",
        )?;
        let rows = stmt.query_map(params![min_messages], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// The most recent moment any conversation was reflected on (ORG-1).
    pub fn last_reflection(&self) -> Result<Option<i64>, DbError> {
        let conn = self.conn.lock().unwrap();
        let at: Option<i64> =
            conn.query_row("SELECT MAX(reflected_at) FROM conversations", [], |r| r.get(0))?;
        Ok(at)
    }

    /// Full-text search returning matching conversations, most-recent first (CHT-3).
    /// Best-matching message per top-level conversation, with an excerpt around
    /// the match. The match is wrapped in `\u{2}`…`\u{3}` so the frontend can
    /// mark it without ever treating message text as markup.
    pub fn search_messages(&self, query: &str, limit: usize) -> Result<Vec<MessageHit>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT m.conversation_id, snippet(messages_fts, 0, char(2), char(3), '…', 12)
             FROM messages_fts
             JOIN messages m ON m.rowid = messages_fts.rowid
             JOIN conversations c ON c.id = m.conversation_id
             WHERE messages_fts MATCH ?1 AND c.parent_conversation_id IS NULL
             ORDER BY bm25(messages_fts)
             LIMIT 500",
        )?;
        let rows = stmt.query_map([query], |r| {
            Ok(MessageHit { conversation_id: r.get(0)?, snippet: r.get(1)? })
        })?;
        let mut seen = std::collections::HashSet::new();
        let mut hits = Vec::new();
        for row in rows {
            let hit = row?;
            if seen.insert(hit.conversation_id.clone()) {
                hits.push(hit);
                if hits.len() == limit {
                    break;
                }
            }
        }
        Ok(hits)
    }

    // ---- delegated child runs (`SUB-3`) ----

    /// Mark this conversation as a delegated child's workspace.
    pub fn set_conversation_parent(&self, id: &str, parent: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET parent_conversation_id = ?2 WHERE id = ?1",
            params![id, parent],
        )?;
        Ok(())
    }

    /// Record a child run at the moment it starts, so it is visible while it
    /// works rather than only once it finishes.
    #[allow(clippy::too_many_arguments)]
    pub fn create_subagent_run(
        &self,
        id: &str,
        parent_conversation_id: &str,
        parent_message_id: Option<&str>,
        child_conversation_id: &str,
        agent: &str,
        task: &str,
    ) -> Result<SubagentRun, DbError> {
        let conn = self.conn.lock().unwrap();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO subagent_runs(id, parent_conversation_id, parent_message_id,
                                       child_conversation_id, agent, task, status, started_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7)",
            params![id, parent_conversation_id, parent_message_id, child_conversation_id, agent, task, ts],
        )?;
        Ok(SubagentRun {
            id: id.to_string(),
            parent_conversation_id: parent_conversation_id.to_string(),
            parent_message_id: parent_message_id.map(str::to_string),
            child_conversation_id: child_conversation_id.to_string(),
            agent: agent.to_string(),
            task: task.to_string(),
            status: "running".to_string(),
            stop_reason: None,
            result: None,
            steps: 0,
            started_at: ts,
            ended_at: None,
        })
    }

    /// Move a child between the states it can be in before it ends
    /// (`SUB-12`: `queued` while it waits for a pool slot, `running` once it
    /// has one). Deliberately separate from `finish_subagent_run`, which is
    /// the only writer of `ended_at` and must stay the one place a run ends.
    pub fn set_subagent_status(&self, id: &str, status: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE subagent_runs SET status = ?2 WHERE id = ?1 AND ended_at IS NULL",
            params![id, status],
        )?;
        Ok(())
    }

    pub fn finish_subagent_run(
        &self,
        id: &str,
        status: &str,
        stop_reason: &str,
        result: &str,
        steps: usize,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE subagent_runs
                SET status = ?2, stop_reason = ?3, result = ?4, steps = ?5, ended_at = ?6
              WHERE id = ?1",
            params![id, status, stop_reason, result, steps as i64, now_ms()],
        )?;
        Ok(())
    }

    /// Close out children a restart orphaned (`SUB-12`).
    ///
    /// The background queue lives in memory, so a run left `queued` or
    /// `running` when the process died is not coming back. Settling them at
    /// startup is what lets every reader simply believe the row: an unfinished
    /// one means a child that is genuinely still working.
    pub fn fail_interrupted_subagent_runs(&self) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE subagent_runs
                SET status = 'stopped', stop_reason = 'aborted', ended_at = ?1,
                    result = COALESCE(result, 'It was still going when the app closed.')
              WHERE ended_at IS NULL",
            params![now_ms()],
        )?;
        Ok(n)
    }

    /// Every child this conversation started, oldest first — the order the lead
    /// asked for them in, which is the order the Fleet card shows.
    pub fn list_subagent_runs(&self, conversation_id: &str) -> Result<Vec<SubagentRun>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, parent_conversation_id, parent_message_id, child_conversation_id,
                    agent, task, status, stop_reason, result, steps, started_at, ended_at
             FROM subagent_runs WHERE parent_conversation_id = ?1
             ORDER BY started_at ASC, rowid ASC",
        )?;
        let rows = stmt
            .query_map([conversation_id], Self::map_subagent_run)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_subagent_run(&self, id: &str) -> Result<Option<SubagentRun>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, parent_conversation_id, parent_message_id, child_conversation_id,
                        agent, task, status, stop_reason, result, steps, started_at, ended_at
                 FROM subagent_runs WHERE id = ?1",
                [id],
                Self::map_subagent_run,
            )
            .ok();
        Ok(row)
    }

    fn map_subagent_run(row: &rusqlite::Row) -> rusqlite::Result<SubagentRun> {
        Ok(SubagentRun {
            id: row.get(0)?,
            parent_conversation_id: row.get(1)?,
            parent_message_id: row.get(2)?,
            child_conversation_id: row.get(3)?,
            agent: row.get(4)?,
            task: row.get(5)?,
            status: row.get(6)?,
            stop_reason: row.get(7)?,
            result: row.get(8)?,
            steps: row.get::<_, i64>(9)? as usize,
            started_at: row.get(10)?,
            ended_at: row.get(11)?,
        })
    }

    // ---- what runs cost (`OBS-2`) ----

    /// Record one finished run's usage.
    ///
    /// A run with no reported tokens is still written. This used to return
    /// early on the reasoning that "no row" and "zero tokens" mean the same
    /// thing — they do not. Plenty of providers report no usage at all (several
    /// free tiers, and any OpenAI-compatible server that ignores
    /// `include_usage`), and dropping those rows left the Usage panel blank
    /// after a day of real work, as though nothing had been run. Zero is a
    /// measurement we did not get; the panel says so rather than showing an
    /// empty page.
    #[allow(clippy::too_many_arguments)]
    pub fn record_run_usage(
        &self,
        run_id: &str,
        conversation_id: &str,
        model_name: &str,
        provenance: &str,
        prompt_tokens: u64,
        output_tokens: u64,
        turns: usize,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO run_usage(run_id, conversation_id, model_name, provenance,
                                   prompt_tokens, output_tokens, turns, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run_id,
                conversation_id,
                model_name,
                provenance,
                prompt_tokens as i64,
                output_tokens as i64,
                turns as i64,
                now_ms()
            ],
        )?;
        Ok(())
    }

    /// Usage since `since` (epoch ms), grouped three ways. One pass over the
    /// rows rather than three queries — the table is small and the grouping is
    /// what the Usage panel shows all at once anyway.
    pub fn usage_summary(&self, since: i64) -> Result<UsageSummary, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT conversation_id, model_name, provenance, prompt_tokens, output_tokens, created_at
             FROM run_usage WHERE created_at >= ?1",
        )?;
        let rows = stmt.query_map(params![since], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)? as u64,
                r.get::<_, i64>(4)? as u64,
                r.get::<_, i64>(5)?,
            ))
        })?;

        use std::collections::HashMap;
        let mut by_day: HashMap<String, UsageBucket> = HashMap::new();
        let mut by_model: HashMap<String, UsageBucket> = HashMap::new();
        let mut by_conversation: HashMap<String, UsageBucket> = HashMap::new();
        let mut total = UsageBucket::default();

        for row in rows {
            let (conversation_id, model_name, provenance, prompt, output, created_at) = row?;
            let day = day_key(created_at);
            for (map, key, provenance) in [
                (&mut by_day, day, provenance.clone()),
                (&mut by_model, model_name.clone(), provenance.clone()),
                (&mut by_conversation, conversation_id.clone(), provenance.clone()),
            ] {
                let bucket = map.entry(key.clone()).or_insert_with(|| UsageBucket {
                    key,
                    provenance,
                    ..Default::default()
                });
                bucket.prompt_tokens += prompt;
                bucket.output_tokens += output;
                bucket.runs += 1;
            }
            total.prompt_tokens += prompt;
            total.output_tokens += output;
            total.runs += 1;
        }

        // Titles for the conversation breakdown, skipping any that have since
        // been deleted — the spend still counts, it just has no name any more.
        for (id, bucket) in by_conversation.iter_mut() {
            let title: Option<String> = conn
                .query_row("SELECT title FROM conversations WHERE id = ?1", params![id], |r| r.get(0))
                .ok();
            bucket.label = title;
        }

        let sorted = |map: HashMap<String, UsageBucket>, newest_first: bool| {
            let mut v: Vec<UsageBucket> = map.into_values().collect();
            if newest_first {
                v.sort_by(|a, b| b.key.cmp(&a.key));
            } else {
                v.sort_by_key(|b| std::cmp::Reverse(b.total_tokens()));
            }
            v
        };

        Ok(UsageSummary {
            total,
            by_day: sorted(by_day, true),
            by_model: sorted(by_model, false),
            by_conversation: sorted(by_conversation, false),
        })
    }

    /// `CTX-5`/`HRN-UI-5`: branch a conversation just before one assistant turn.
    ///
    /// The new conversation keeps everything the old one had — persona, model,
    /// overrides, working folder and its trust — because a fork the user has to
    /// re-configure is not the same question asked again. Messages are copied up
    /// to but not including the turn being redone, and the user turn that
    /// prompted it is handed back rather than copied: the caller sends it, which
    /// is the one path that also builds a fresh prompt and a fresh run.
    ///
    /// Returns `(new conversation, the user turn to resend)`. The turn is `None`
    /// when the fork point has no user message before it, which leaves the caller
    /// with an ordinary empty branch instead of a broken rerun.
    pub fn fork_conversation(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<(Conversation, Option<String>), DbError> {
        let cutoff: i64 = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT created_at FROM messages WHERE id = ?1 AND conversation_id = ?2",
                params![message_id, conversation_id],
                |r| r.get(0),
            )?
        };
        let source = self
            .get_conversation(conversation_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let upto_seq = self.seq_at_time(conversation_id, cutoff)?;

        let conn = self.conn.lock().unwrap();
        let new_conv_id = new_id();
        let ts = now_ms();
        let title = format!("{} (again)", source.title);
        conn.execute(
            "INSERT INTO conversations(id, title, model_id, persona_id, overrides_json, workspace,
                                      folder_path, folder_trust, project_id, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                new_conv_id,
                title,
                source.model_id,
                source.persona_id,
                source.overrides_json,
                source.workspace as i64,
                source.folder_path,
                source.folder_trust,
                // A fork of a project session is another session in that
                // project, not a loose chat that happens to point at the folder.
                source.project_id,
                ts
            ],
        )?;

        // Everything before the turn being redone, oldest first.
        let mut stmt = conn.prepare(
            "SELECT id, role, content, model_name, model_provenance, steps_json, stop_reason, created_at, plan_json
             FROM messages WHERE conversation_id = ?1 AND created_at < ?2 ORDER BY created_at",
        )?;
        // `PLN-T4`: `plan_json` travels with the turn, so a branch shows the
        // plan the run it copied was working to rather than a blank card.
        type Row = (String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, i64, Option<String>);
        let mut rows: Vec<Row> = stmt
            .query_map(params![conversation_id, cutoff], |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        // The trailing user turn is the one to resend, so it is not copied —
        // otherwise it would appear twice the moment the caller sends it.
        let resend = match rows.last() {
            Some(last) if last.1 == "user" => rows.pop().map(|r| r.2),
            _ => None,
        };

        // Where the source's summary boundary lands in the copy. The fork's
        // messages get fresh ids, so carrying the old boundary id across
        // unchanged would point at a message this conversation does not have.
        let mut new_boundary: Option<String> = None;
        for row in &rows {
            let new_msg_id = new_id();
            if source.summary_upto_message_id.as_deref() == Some(row.0.as_str()) {
                new_boundary = Some(new_msg_id.clone());
            }
            conn.execute(
                "INSERT INTO messages(id, conversation_id, role, content, model_name, model_provenance,
                                      steps_json, stop_reason, created_at, plan_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![new_msg_id, new_conv_id, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8],
            )?;
            conn.execute(
                "INSERT INTO attachments(id, message_id, kind, name, path, artifact_id)
                 SELECT lower(hex(randomblob(16))), ?2, kind, name, path, artifact_id
                 FROM attachments WHERE message_id = ?1",
                params![row.0, new_msg_id],
            )?;
        }

        // A summary the fork can still account for travels with it. Without
        // this, forking a long conversation throws away work the user already
        // paid a model to do: the copy would arrive with no summary, resend
        // every old turn verbatim, overflow again, and compact a second time.
        //
        // It travels only when its boundary was copied. A summary whose boundary
        // sits at or after the cut covers turns this conversation does not have,
        // and a summary of messages that are not there is worse than none.
        let forked_summary = new_boundary.as_ref().and(source.summary.clone());
        if let (Some(text), Some(boundary)) = (&forked_summary, &new_boundary) {
            conn.execute(
                "UPDATE conversations SET summary = ?2, summary_upto_message_id = ?3 WHERE id = ?1",
                params![new_conv_id, text, boundary],
            )?;
        }
        drop(conn);

        // The model's own view of the same cut. Best effort: a fork whose
        // history is intact for the user is still worth having if the log copy
        // fails, and the next run rebuilds the log from the messages anyway.
        let _ = self.fork_session_events(conversation_id, &new_conv_id, upto_seq);

        Ok((
            Conversation {
                id: new_conv_id,
                title,
                model_id: source.model_id,
                persona_id: source.persona_id,
                overrides_json: source.overrides_json,
                workspace: source.workspace,
                summary: forked_summary,
                summary_upto_message_id: new_boundary,
                reflected_at: None,
                folder_path: source.folder_path,
                folder_trust: source.folder_trust,
                parent_conversation_id: None,
                project_id: source.project_id,
                created_at: ts,
                updated_at: ts,
            },
            resend,
        ))
    }

    // ---- the session log (`CTX-2`) ----

    /// Append one event to a conversation's log and return its `seq`.
    ///
    /// The sequence is allocated inside the same lock as the insert, so two runs
    /// writing to one conversation cannot land on the same number — which the
    /// unique index would refuse anyway, but as a lost event rather than as a
    /// retry.
    pub fn append_session_event(
        &self,
        conversation_id: &str,
        run_id: Option<&str>,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        let seq: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM session_events WHERE conversation_id = ?1",
                [conversation_id],
                |r| r.get(0),
            )
            .unwrap_or(1);
        conn.execute(
            "INSERT INTO session_events(id, conversation_id, run_id, seq, kind, payload_json, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![new_id(), conversation_id, run_id, seq, kind, payload.to_string(), now_ms()],
        )?;
        Ok(seq)
    }

    /// The rows one run wrote, oldest first. This is what resume replays.
    pub fn session_events_for_run(&self, run_id: &str) -> Result<Vec<SessionEvent>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT seq, kind, payload_json, created_at FROM session_events WHERE run_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt
            .query_map([run_id], |r| {
                Ok(SessionEvent {
                    seq: r.get(0)?,
                    kind: r.get(1)?,
                    payload_json: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// A whole conversation's log, oldest first — what a fork copies and what a
    /// test checks a fork against.
    pub fn session_events(&self, conversation_id: &str) -> Result<Vec<SessionEvent>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT seq, kind, payload_json, created_at FROM session_events
             WHERE conversation_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| {
                Ok(SessionEvent {
                    seq: r.get(0)?,
                    kind: r.get(1)?,
                    payload_json: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The last run this conversation logged, and how it ended. `None` for the
    /// reason means the run never wrote a `stop` row — it was killed with the
    /// app, which is exactly the case resume exists for.
    pub fn last_logged_run(&self, conversation_id: &str) -> Result<Option<(String, Option<String>)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let run_id: Option<String> = conn
            .query_row(
                "SELECT run_id FROM session_events
                 WHERE conversation_id = ?1 AND run_id IS NOT NULL
                 ORDER BY seq DESC LIMIT 1",
                [conversation_id],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        let Some(run_id) = run_id else { return Ok(None) };
        let reason: Option<String> = conn
            .query_row(
                "SELECT payload_json FROM session_events WHERE run_id = ?1 AND kind = 'stop'
                 ORDER BY seq DESC LIMIT 1",
                [&run_id],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|p| {
                serde_json::from_str::<serde_json::Value>(&p)
                    .ok()
                    .and_then(|v| v["reason"].as_str().map(str::to_string))
            });
        Ok(Some((run_id, reason)))
    }

    /// `CTX-T3`: copy a conversation's log up to `seq` into another conversation,
    /// and nothing after it. Sequence numbers are renumbered from 1 so the fork
    /// reads as its own history rather than as a slice of someone else's.
    pub fn fork_session_events(&self, from: &str, to: &str, upto_seq: i64) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT run_id, kind, payload_json, created_at FROM session_events
             WHERE conversation_id = ?1 AND seq <= ?2 ORDER BY seq",
        )?;
        let rows = stmt
            .query_map(params![from, upto_seq], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (n, (run_id, kind, payload, created_at)) in rows.iter().enumerate() {
            conn.execute(
                "INSERT INTO session_events(id, conversation_id, run_id, seq, kind, payload_json, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![new_id(), to, run_id, (n + 1) as i64, kind, payload, created_at],
            )?;
        }
        Ok(rows.len())
    }

    /// The highest `seq` written for a conversation at or before `at_ms`. This is
    /// how a fork point expressed in the UI's terms — "this message" — becomes a
    /// point in the model's log.
    pub fn seq_at_time(&self, conversation_id: &str, at_ms: i64) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM session_events
                 WHERE conversation_id = ?1 AND created_at <= ?2",
                params![conversation_id, at_ms],
                |r| r.get(0),
            )
            .unwrap_or(0))
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
            "SELECT id, conversation_id, role, content, model_name, model_provenance, steps_json, stop_reason, created_at, plan_json
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

    /// Look up a library entry by its on-disk path — used to make registering
    /// a download idempotent. Without this, re-running a download for a model
    /// that's already fully on disk (e.g. the user re-clicking "Download"
    /// after navigating away and back) adds a second row for the same file
    /// instead of recognizing it's already in the library.
    pub fn find_model_by_path(&self, path: &str) -> Result<Option<ModelEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, name, path, quant, size_bytes, vision, role, is_default, added_at
                 FROM model_library WHERE path = ?1",
                [path],
                Self::map_model,
            )
            .optional()?;
        Ok(row)
    }

    /// Re-record an entry's size after its file was fetched again — used when
    /// a registered model turned out to be missing or half-written, so the
    /// repair reuses the existing row instead of adding a second one for the
    /// same path.
    pub fn set_model_size(&self, id: &str, size_bytes: Option<i64>) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE model_library SET size_bytes = ?2 WHERE id = ?1",
            params![id, size_bytes],
        )?;
        Ok(())
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

    // ---- local endpoints (a user's own Ollama/LM Studio/OpenAI-compatible server) ----

    pub fn insert_local_endpoint(
        &self,
        label: &str,
        base_url: &str,
        ctx_size: i64,
    ) -> Result<LocalEndpointRow, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO local_endpoints(id, label, base_url, kind, ctx_size, enabled, created_at)
             VALUES(?1, ?2, ?3, 'openai', ?4, 1, ?5)",
            params![id, label, base_url, ctx_size, ts],
        )?;
        Ok(LocalEndpointRow {
            id,
            label: label.to_string(),
            base_url: base_url.to_string(),
            kind: "openai".to_string(),
            ctx_size,
            enabled: true,
            created_at: ts,
        })
    }

    pub fn list_local_endpoints(&self) -> Result<Vec<LocalEndpointRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, label, base_url, kind, ctx_size, enabled, created_at
             FROM local_endpoints ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], Self::map_local_endpoint)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_local_endpoint(&self, id: &str) -> Result<Option<LocalEndpointRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, label, base_url, kind, ctx_size, enabled, created_at
                 FROM local_endpoints WHERE id = ?1",
                [id],
                Self::map_local_endpoint,
            )
            .ok();
        Ok(row)
    }

    pub fn update_local_endpoint(
        &self,
        id: &str,
        label: &str,
        base_url: &str,
        ctx_size: i64,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE local_endpoints SET label = ?2, base_url = ?3, ctx_size = ?4 WHERE id = ?1",
            params![id, label, base_url, ctx_size],
        )?;
        Ok(())
    }

    pub fn set_local_endpoint_enabled(&self, id: &str, enabled: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE local_endpoints SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )?;
        Ok(())
    }

    pub fn delete_local_endpoint(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM local_endpoints WHERE id = ?1", [id])?;
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
        message_id: Option<&str>,
    ) -> Result<Artifact, DbError> {
        self.add_artifact_with(conversation_id, title, kind, content, None, None, message_id)
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
        message_id: Option<&str>,
    ) -> Result<Artifact, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = new_id();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO artifacts(id, conversation_id, title, kind, content, created_at, meta_json, parent_id, message_id)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, conversation_id, title, kind, content, ts, meta_json, parent_id, message_id],
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
            message_id: message_id.map(|s| s.to_string()),
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
            message_id: r.get(9)?,
        })
    }

    pub fn list_artifacts(&self, conversation_id: &str) -> Result<Vec<Artifact>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, title, kind, content, created_at, saved_path, meta_json, parent_id, message_id
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
            "SELECT id, conversation_id, title, kind, content, created_at, saved_path, meta_json, parent_id, message_id
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
            "SELECT id, conversation_id, title, kind, content, created_at, saved_path, meta_json, parent_id, message_id
             FROM artifacts WHERE id = ?1",
        )?;
        let row = stmt.query_row([id], Self::map_artifact).ok();
        Ok(row)
    }

    /// Replace an existing artifact's content in place (`ART-4`). The id stays
    /// the same, so everything already pointing at it — the Canvas selection,
    /// the timeline chip, the message that produced it — keeps pointing at the
    /// same thing, instead of the panel filling with near-identical copies
    /// every time the model fixes a bug in one.
    pub fn update_artifact(&self, id: &str, title: Option<&str>, content: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE artifacts SET content = ?2, title = COALESCE(?3, title) WHERE id = ?1",
            params![id, content, title],
        )?;
        Ok(())
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

    /// `COD-11`: every change recorded in a conversation at or after `since`,
    /// oldest first — the order a change set is replayed in.
    pub fn trash_since(&self, conversation_id: &str, since: i64) -> Result<Vec<TrashEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, op, path, prev_path, blob_path, created_at, undone
             FROM file_trash WHERE conversation_id = ?1 AND created_at >= ?2
             ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt
            .query_map(params![conversation_id, since], Self::map_trash)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// When a run's first session event was written: the start of that run as
    /// far as anything on disk can tell.
    pub fn run_started_at(&self, run_id: &str) -> Result<Option<i64>, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT MIN(created_at) FROM session_events WHERE run_id = ?1",
                [run_id],
                |r| r.get::<_, Option<i64>>(0),
            )
            .ok()
            .flatten())
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
            "INSERT INTO personas(id, name, system_prompt, model_id, params_json, tools_json, skills_json, description, spawnable, is_default, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?10)",
            params![id, p.name, p.system_prompt, p.model_id, p.params_json, p.tools_json, p.skills_json, p.description, p.spawnable as i64, ts],
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
            description: p.description.clone(),
            spawnable: p.spawnable,
        })
    }

    pub fn list_personas(&self) -> Result<Vec<Persona>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, system_prompt, model_id, params_json, is_default, created_at, updated_at, tools_json, skills_json, description, spawnable
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
                                 tools_json = ?6, skills_json = ?7, description = ?8,
                                 spawnable = ?9, updated_at = ?10
             WHERE id = ?1",
            params![p.id, p.name, p.system_prompt, p.model_id, p.params_json, p.tools_json, p.skills_json, p.description, p.spawnable as i64, now_ms()],
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
            description: row.get(10)?,
            spawnable: row.get::<_, i64>(11)? != 0,
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

    fn map_local_endpoint(row: &rusqlite::Row) -> rusqlite::Result<LocalEndpointRow> {
        Ok(LocalEndpointRow {
            id: row.get(0)?,
            label: row.get(1)?,
            base_url: row.get(2)?,
            kind: row.get(3)?,
            ctx_size: row.get(4)?,
            enabled: row.get::<_, i64>(5)? != 0,
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
            parent_conversation_id: row.get(13)?,
            project_id: row.get(14)?,
        })
    }

    fn map_project(row: &rusqlite::Row) -> rusqlite::Result<Project> {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            root_path: row.get(2)?,
            instructions: row.get(3)?,
            trust: row.get(4)?,
            exec_policy: row.get(5)?,
            card_json: row.get(6)?,
            card_built_at: row.get(7)?,
            tabs_json: row.get(8)?,
            archived: row.get::<_, i64>(9)? != 0,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
            allow_json: row.get(12)?,
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
            stop_reason: row.get(7)?,
            created_at: row.get(8)?,
            plan_json: row.get(9)?,
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

    /// `REF-3b`: the catch-up pass has to find exactly the conversations that
    /// were left behind — long enough to have taught something, never digested.
    #[test]
    fn finds_unreflected_conversations_worth_digesting() {
        let db = Db::open_in_memory().unwrap();
        let msg = |content: &str| NewMessage {
            role: "user".into(),
            content: content.into(),
            model_name: None,
            model_provenance: None,
            steps_json: None,
            attachments: Vec::new(),
        };

        let slight = db.create_conversation("one question", None, false).unwrap();
        db.append_message(&slight.id, &msg("explain ai")).unwrap();

        let stranded = db.create_conversation("a real session", None, false).unwrap();
        for i in 0..4 {
            db.append_message(&stranded.id, &msg(&format!("turn {i}"))).unwrap();
        }

        let done = db.create_conversation("already digested", None, false).unwrap();
        for i in 0..4 {
            db.append_message(&done.id, &msg(&format!("turn {i}"))).unwrap();
        }
        db.set_conversation_reflected(&done.id, now_ms()).unwrap();

        let stale = db.unreflected_conversations(4).unwrap();
        assert_eq!(stale, vec![stranded.id.clone()], "too short, or already reflected, is not stale");

        // Once it has had its turn it never comes back, however it went.
        db.set_conversation_reflected(&stranded.id, now_ms()).unwrap();
        assert!(db.unreflected_conversations(4).unwrap().is_empty());
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

        let hits = db.search_messages("endpoint*", 20).unwrap();
        assert_eq!(hits.len(), 1, "FTS should find the conversation by message content");
        assert_eq!(hits[0].conversation_id, c.id);
        assert!(
            hits[0].snippet.contains("\u{2}endpoint\u{3}"),
            "the match is fenced for the frontend to mark: {:?}",
            hits[0].snippet
        );

        let miss = db.search_messages("nonexistentterm*", 20).unwrap();
        assert!(miss.is_empty());
    }

    #[test]
    fn message_search_is_one_hit_per_top_level_conversation() {
        let db = Db::open_in_memory().unwrap();
        let msg = |text: &str| NewMessage {
            role: "user".into(),
            content: text.into(),
            model_name: None,
            model_provenance: None,
            steps_json: None,
            attachments: Vec::new(),
        };
        let parent = db.create_conversation("parent", None, false).unwrap();
        db.append_message(&parent.id, &msg("the cache invalidation bug")).unwrap();
        db.append_message(&parent.id, &msg("still the cache, again")).unwrap();
        let child = db.create_conversation("child run", None, false).unwrap();
        db.append_message(&child.id, &msg("cache warmed by a delegated run")).unwrap();
        db.set_conversation_parent(&child.id, &parent.id).unwrap();

        let hits = db.search_messages("cache*", 20).unwrap();
        assert_eq!(hits.len(), 1, "two matching messages, one conversation, child excluded");
        assert_eq!(hits[0].conversation_id, parent.id);
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

    /// A non-media artifact (document/code/svg/html) has to remember which
    /// turn made it, or the inline chip that opens it in the Workbench is
    /// gone the moment the conversation reloads — even though the Workbench's
    /// own artifact list, which doesn't key off any message, still has it.
    #[test]
    fn an_artifact_remembers_the_message_that_made_it() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Docs", None, false).unwrap();
        let msg = db
            .append_message(
                &c.id,
                &NewMessage {
                    role: "assistant".into(),
                    content: "here you go".into(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: vec![],
                },
            )
            .unwrap();
        let art = db
            .add_artifact(Some(&c.id), "Report", "markdown", "# hi", Some(&msg.id))
            .unwrap();
        assert_eq!(art.message_id.as_deref(), Some(msg.id.as_str()));

        let reloaded = db.list_artifacts(&c.id).unwrap();
        assert_eq!(reloaded[0].message_id.as_deref(), Some(msg.id.as_str()));
    }

    /// The upgrade has to carry the conversations that already exist, or the
    /// column only ever describes artifacts made after it landed and every
    /// older chat stays missing its artifacts in the stream.
    #[test]
    fn the_upgrade_backfills_which_message_made_each_artifact() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Legacy", None, false).unwrap();
        let assistant = |content: &str| NewMessage {
            role: "assistant".into(),
            content: content.into(),
            model_name: None,
            model_provenance: None,
            steps_json: None,
            attachments: vec![],
        };
        let first = db.append_message(&c.id, &assistant("one")).unwrap();
        let second = db.append_message(&c.id, &assistant("two")).unwrap();
        let doc = db
            .add_artifact(Some(&c.id), "Report", "markdown", "# hi", None)
            .unwrap();
        let media = db
            .add_artifact_with(Some(&c.id), "fox", "image", r"C:\m\fox.png", None, None, None)
            .unwrap();
        db.add_attachment(&second.id, "image", "fox.png", r"C:\m\fox.png", Some(&media.id))
            .unwrap();

        // Rewind to the state a real install actually got stuck in: already
        // stamped 19, so the column exists but nothing ever filled it. Rewinding
        // to 18 would pass even if the backfill were buried in the v19 block,
        // which is the very mistake this guards against. Timestamps are pinned so
        // "the turn that was open when the artifact appeared" is the second one
        // beyond doubt.
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("UPDATE messages SET created_at = 100 WHERE id = ?1", [&first.id])
                .unwrap();
            conn.execute("UPDATE messages SET created_at = 200 WHERE id = ?1", [&second.id])
                .unwrap();
            conn.execute("UPDATE artifacts SET message_id = NULL, created_at = 300", [])
                .unwrap();
            conn.pragma_update(None, "user_version", 19).unwrap();
        }
        db.migrate().unwrap();

        let made_by = |id: &str| db.get_artifact(id).unwrap().unwrap().message_id;
        // Media is exact — the attachment row already knew both ends.
        assert_eq!(made_by(&media.id).as_deref(), Some(second.id.as_str()));
        // The document has only the timestamps to go on, and lands on the turn
        // that was open rather than the one before it.
        assert_eq!(made_by(&doc.id).as_deref(), Some(second.id.as_str()));
    }

    #[test]
    fn artifacts_remember_where_they_were_saved() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Art", None, false).unwrap();
        let a = db.add_artifact(Some(&c.id), "Chart", "svg", "<svg/>", None).unwrap();
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

    /// `OBS-T1`: usage lands where the panel reads it, grouped three ways, and
    /// a run that reported nothing records nothing rather than a zero row that
    /// would read as a free run.
    #[test]
    fn usage_is_recorded_per_run_and_grouped_for_the_panel() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_conversation("one", None, false).unwrap();
        let b = db.create_conversation("two", None, false).unwrap();

        db.record_run_usage("r1", &a.id, "claude-sonnet-4-5", "cloud", 1000, 200, 3).unwrap();
        db.record_run_usage("r2", &a.id, "claude-sonnet-4-5", "cloud", 500, 100, 2).unwrap();
        db.record_run_usage("r3", &b.id, "qwen3-8b", "local", 4000, 900, 5).unwrap();
        // A provider that reported nothing. The run still happened.
        db.record_run_usage("r4", &b.id, "qwen3-8b", "local", 0, 0, 1).unwrap();

        let s = db.usage_summary(0).unwrap();
        assert_eq!(s.total.runs, 4, "a run whose tokens went unreported is still a run");
        assert_eq!(s.total.prompt_tokens, 5500);
        assert_eq!(s.total.output_tokens, 1200);

        // Biggest first, so the local run leads.
        assert_eq!(s.by_model[0].key, "qwen3-8b");
        assert_eq!(s.by_model[0].provenance, "local");
        assert_eq!(s.by_model[1].runs, 2, "the two cloud runs fold into one row");

        let lead = s.by_conversation.iter().find(|r| r.key == a.id).unwrap();
        assert_eq!(lead.label.as_deref(), Some("one"));
        assert_eq!(lead.total_tokens(), 1800);

        // Every run here fell on one day, whichever day that is.
        assert_eq!(s.by_day.len(), 1);
        assert_eq!(s.by_day[0].runs, 4);

        // A window that starts after the rows sees none of them.
        assert_eq!(db.usage_summary(now_ms() + 1000).unwrap().total.runs, 0);
    }

    /// The bug behind an empty Usage panel: a run whose provider reported no
    /// usage was dropped entirely, so somebody using a free tier that reports
    /// nothing saw a blank page after a day of real work. A run counted with no
    /// tokens is honest — "I do not know what this cost" — where no row at all
    /// claims the run never happened.
    #[test]
    fn a_run_the_provider_never_priced_is_still_a_run() {
        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("silent provider", None, false).unwrap();

        db.record_run_usage("r1", &c.id, "some-free-model", "cloud", 0, 0, 4).unwrap();

        let s = db.usage_summary(0).unwrap();
        assert_eq!(s.total.runs, 1);
        assert_eq!(s.total.total_tokens(), 0, "zero means unmeasured, not free");
        assert_eq!(s.by_model.len(), 1);
        assert_eq!(s.by_model[0].key, "some-free-model");
        assert_eq!(s.by_conversation[0].label.as_deref(), Some("silent provider"));
    }

    /// `CTX-T3` at the conversation level: a fork is the same chat asked again
    /// from a point, so it keeps the settings, keeps the history before the cut,
    /// drops everything from the cut on, and hands back the question to re-ask
    /// rather than copying it — copying it would show it twice the moment the
    /// caller sends it.
    #[test]
    fn forking_branches_a_conversation_just_before_one_answer() {
        let db = Db::open_in_memory().unwrap();
        let source = db.create_conversation("Planning", None, true).unwrap();
        db.set_conversation_folder(&source.id, Some("C:\\work")).unwrap();
        db.set_conversation_trust(&source.id, "full").unwrap();

        let msg = |role: &str, content: &str| NewMessage {
            role: role.into(),
            content: content.into(),
            model_name: None,
            model_provenance: None,
            steps_json: None,
            attachments: vec![],
        };
        db.append_message(&source.id, &msg("user", "first question")).unwrap();
        db.append_message(&source.id, &msg("assistant", "first answer")).unwrap();
        db.append_message(&source.id, &msg("user", "the question to redo")).unwrap();
        let redo = db.append_message(&source.id, &msg("assistant", "the answer to redo")).unwrap();
        db.append_message(&source.id, &msg("user", "after the fork point")).unwrap();

        let (fork, resend) = db.fork_conversation(&source.id, &redo.id).unwrap();
        assert_eq!(resend.as_deref(), Some("the question to redo"));
        assert_eq!(fork.title, "Planning (again)");
        assert!(fork.workspace, "a fork of a workspace chat is a workspace chat");
        assert_eq!(fork.folder_path.as_deref(), Some("C:\\work"));
        assert_eq!(fork.folder_trust, "full", "re-granting trust is not part of asking again");

        let copied = db.list_messages(&fork.id).unwrap();
        assert_eq!(
            copied.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
            vec!["first question", "first answer"],
            "everything before the cut, and neither the turn being redone nor what followed it"
        );
        assert_eq!(
            db.list_messages(&source.id).unwrap().len(),
            5,
            "the original is untouched"
        );
    }

    /// `CTX-5`: a summary the fork can still account for travels with it.
    ///
    /// Without this, forking a long conversation silently throws away work the
    /// user already paid a model to do: the copy arrives with no summary,
    /// resends every old turn word for word, overflows, and compacts again.
    /// The boundary has to be re-pointed, because the fork's messages are copies
    /// with new ids and the old id names nothing here.
    #[test]
    fn a_fork_keeps_the_summary_it_can_still_account_for() {
        let db = Db::open_in_memory().unwrap();
        let source = db.create_conversation("Planning", None, false).unwrap();
        let msg = |role: &str, content: &str| NewMessage {
            role: role.into(),
            content: content.into(),
            model_name: None,
            model_provenance: None,
            steps_json: None,
            attachments: vec![],
        };
        let early = db.append_message(&source.id, &msg("user", "the old beginning")).unwrap();
        db.append_message(&source.id, &msg("assistant", "the old answer")).unwrap();
        db.append_message(&source.id, &msg("user", "the question to redo")).unwrap();
        let redo = db.append_message(&source.id, &msg("assistant", "the answer to redo")).unwrap();

        db.set_conversation_summary(&source.id, "FACTS: the old beginning", &early.id).unwrap();

        let (fork, _) = db.fork_conversation(&source.id, &redo.id).unwrap();
        assert_eq!(fork.summary.as_deref(), Some("FACTS: the old beginning"));

        // Re-pointed, not copied: the boundary names a message in *this*
        // conversation, and it is the same turn it named in the original.
        let boundary = fork.summary_upto_message_id.clone().expect("a boundary came across");
        assert_ne!(boundary, early.id, "the fork's messages are copies with their own ids");
        let copied = db.list_messages(&fork.id).unwrap();
        assert_eq!(
            copied.iter().find(|m| m.id == boundary).map(|m| m.content.as_str()),
            Some("the old beginning")
        );

        // And it survives a reload, not just the returned struct.
        let reloaded = db.get_conversation(&fork.id).unwrap().unwrap();
        assert_eq!(reloaded.summary_upto_message_id.as_deref(), Some(boundary.as_str()));
    }

    /// The other half: a summary whose boundary is at or past the cut covers
    /// turns the fork does not have. Carrying it across would tell the model
    /// that messages it cannot see were already summarized, so it is dropped and
    /// the fork sends its short history in full — which is correct and cheap.
    #[test]
    fn a_fork_drops_a_summary_that_covers_turns_it_does_not_have() {
        let db = Db::open_in_memory().unwrap();
        let source = db.create_conversation("Planning", None, false).unwrap();
        let msg = |role: &str, content: &str| NewMessage {
            role: role.into(),
            content: content.into(),
            model_name: None,
            model_provenance: None,
            steps_json: None,
            attachments: vec![],
        };
        db.append_message(&source.id, &msg("user", "the old beginning")).unwrap();
        let redo = db.append_message(&source.id, &msg("assistant", "the answer to redo")).unwrap();
        let late = db.append_message(&source.id, &msg("user", "after the fork point")).unwrap();

        db.set_conversation_summary(&source.id, "FACTS: everything", &late.id).unwrap();

        let (fork, _) = db.fork_conversation(&source.id, &redo.id).unwrap();
        assert!(fork.summary.is_none());
        assert!(fork.summary_upto_message_id.is_none());
    }

    /// `SUB-T8`: v23 applies on a fresh database, and children come back in the
    /// order they were started — the order the lead asked for them in, not the
    /// order they happened to finish.
    #[test]
    fn subagent_runs_come_back_in_start_order() {
        let db = Db::open_in_memory().unwrap();
        let parent = db.create_conversation("lead", None, false).unwrap();
        let other = db.create_conversation("elsewhere", None, false).unwrap();

        let mut ids = Vec::new();
        for (i, agent) in ["general", "researcher", "reviewer"].iter().enumerate() {
            let child = db.create_conversation(agent, None, false).unwrap();
            db.set_conversation_parent(&child.id, &parent.id).unwrap();
            let id = format!("run_{i}");
            db.create_subagent_run(&id, &parent.id, Some("msg_1"), &child.id, agent, "do a thing")
                .unwrap();
            ids.push(id);
        }
        // A child of another conversation must not appear in this one's list.
        let stranger = db.create_conversation("stranger", None, false).unwrap();
        db.create_subagent_run("run_x", &other.id, None, &stranger.id, "general", "elsewhere")
            .unwrap();

        let rows = db.list_subagent_runs(&parent.id).unwrap();
        assert_eq!(rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), ids);
        assert!(rows.iter().all(|r| r.status == "running" && r.ended_at.is_none()));

        // The child conversation knows whose it is, so the Rail can hide it.
        let child_conv = db.get_conversation(&rows[0].child_conversation_id).unwrap().unwrap();
        assert_eq!(child_conv.parent_conversation_id.as_deref(), Some(parent.id.as_str()));

        db.finish_subagent_run(&ids[1], "stopped", "aborted", "half of it", 4).unwrap();
        let stopped = db.get_subagent_run(&ids[1]).unwrap().unwrap();
        assert_eq!(stopped.status, "stopped");
        assert_eq!(stopped.stop_reason.as_deref(), Some("aborted"));
        assert_eq!(stopped.result.as_deref(), Some("half of it"));
        assert_eq!(stopped.steps, 4);
        assert!(stopped.ended_at.is_some());

        // Deleting the turn's conversation takes its children's records with it.
        db.delete_conversation(&parent.id).unwrap();
        assert!(db.list_subagent_runs(&parent.id).unwrap().is_empty());
    }

    /// `SUB-12`: a background child waits in a queue before it runs, and the
    /// row is what says so. The part worth pinning is the end: once a run has
    /// ended, a late status write must not reopen it — the pool and the run
    /// itself both write here, and a pump that lost a race could otherwise mark
    /// a finished child "running" forever.
    #[test]
    fn a_queued_child_becomes_running_and_a_finished_one_stays_finished() {
        let db = Db::open_in_memory().unwrap();
        let parent = db.create_conversation("lead", None, false).unwrap();
        let child = db.create_conversation("worker", None, false).unwrap();
        db.create_subagent_run("run_1", &parent.id, None, &child.id, "general", "a long job")
            .unwrap();

        db.set_subagent_status("run_1", "queued").unwrap();
        assert_eq!(db.get_subagent_run("run_1").unwrap().unwrap().status, "queued");
        db.set_subagent_status("run_1", "running").unwrap();
        assert_eq!(db.get_subagent_run("run_1").unwrap().unwrap().status, "running");

        db.finish_subagent_run("run_1", "done", "completed", "the answer", 6).unwrap();
        db.set_subagent_status("run_1", "running").unwrap();
        let row = db.get_subagent_run("run_1").unwrap().unwrap();
        assert_eq!(row.status, "done");
        assert_eq!(row.result.as_deref(), Some("the answer"));
    }

    /// `SUB-12`: the background queue lives in memory, so a child left waiting
    /// or working when the app closed is never coming back. Settling those at
    /// startup is what lets every reader believe an unfinished row — without
    /// it, one crash leaves a Fleet card spinning on nothing forever.
    #[test]
    fn a_restart_settles_children_it_orphaned_and_leaves_the_rest_alone() {
        let db = Db::open_in_memory().unwrap();
        let parent = db.create_conversation("lead", None, false).unwrap();
        for (i, status) in ["queued", "running"].iter().enumerate() {
            let child = db.create_conversation("worker", None, false).unwrap();
            let id = format!("run_{i}");
            db.create_subagent_run(&id, &parent.id, None, &child.id, "general", "a job")
                .unwrap();
            db.set_subagent_status(&id, status).unwrap();
        }
        let finished = db.create_conversation("worker", None, false).unwrap();
        db.create_subagent_run("run_done", &parent.id, None, &finished.id, "general", "a job")
            .unwrap();
        db.finish_subagent_run("run_done", "done", "completed", "all of it", 3).unwrap();

        assert_eq!(db.fail_interrupted_subagent_runs().unwrap(), 2);
        for id in ["run_0", "run_1"] {
            let row = db.get_subagent_run(id).unwrap().unwrap();
            assert_eq!(row.status, "stopped", "{id}");
            assert_eq!(row.stop_reason.as_deref(), Some("aborted"));
            assert!(row.ended_at.is_some());
            assert!(row.result.is_some(), "it has to say why it has no answer");
        }
        // A run that already ended keeps everything it said.
        let done = db.get_subagent_run("run_done").unwrap().unwrap();
        assert_eq!(done.status, "done");
        assert_eq!(done.result.as_deref(), Some("all of it"));
        // And a second startup finds nothing left to settle.
        assert_eq!(db.fail_interrupted_subagent_runs().unwrap(), 0);
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

    /// `PRJ-2-T`: three chats over two folders become two projects, every chat
    /// is backfilled onto the right one, and where the same folder was trusted
    /// differently in different chats the **most restrictive** level wins.
    /// Widening what the agent may do to somebody's code as a side effect of an
    /// upgrade is the one outcome that cannot be taken back.
    #[test]
    fn the_project_migration_makes_one_project_per_folder_and_keeps_the_tightest_trust() {
        let db = Db::open_in_memory().unwrap();
        let a1 = db.create_conversation("a1", None, false).unwrap();
        let a2 = db.create_conversation("a2", None, false).unwrap();
        let b1 = db.create_conversation("b1", None, false).unwrap();
        let loose = db.create_conversation("loose", None, false).unwrap();

        // Simulate a pre-v27 install: folders on the conversations, no
        // projects, and the version rolled back so the block runs again.
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DELETE FROM projects", []).unwrap();
            for (id, path, trust) in [
                (&a1.id, r"C:\work\alpha", "auto"),
                (&a2.id, r"C:\work\alpha", "read-only"),
                (&b1.id, r"C:\work\beta", "confirm"),
            ] {
                conn.execute(
                    "UPDATE conversations
                     SET folder_path = ?2, folder_trust = ?3, project_id = NULL
                     WHERE id = ?1",
                    params![id, path, trust],
                )
                .unwrap();
            }
            conn.pragma_update(None, "user_version", 26).unwrap();
        }

        db.migrate().unwrap();

        let projects = db.list_projects(false).unwrap();
        assert_eq!(projects.len(), 2, "one project per distinct folder, not per chat");

        let alpha = db.project_by_root(r"C:\work\alpha").unwrap().expect("alpha exists");
        assert_eq!(alpha.name, "alpha", "named from the folder's last segment");
        assert_eq!(alpha.trust, "read-only", "the most restrictive of auto and read-only");
        assert_eq!(alpha.exec_policy, "inherit", "COD-7: a project follows the Settings default until told otherwise");

        let beta = db.project_by_root(r"C:\work\beta").unwrap().expect("beta exists");
        assert_eq!(beta.trust, "confirm");

        for (id, expected) in [(&a1.id, &alpha.id), (&a2.id, &alpha.id), (&b1.id, &beta.id)] {
            let conv = db.get_conversation(id).unwrap().unwrap();
            assert_eq!(conv.project_id.as_ref(), Some(expected), "{} backfilled", conv.title);
        }
        let loose = db.get_conversation(&loose.id).unwrap().unwrap();
        assert!(loose.project_id.is_none(), "a chat with no folder gets no project");
    }

    /// `PRJ-2-T2`: the resolver every file tool already calls returns the
    /// project's folder and trust when the conversation has one, and the legacy
    /// per-conversation columns when it does not. This is what lets the project
    /// entity land under the app instead of through it.
    #[test]
    fn the_folder_resolver_prefers_the_project_and_falls_back_to_the_conversation() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("c", None, false).unwrap();

        // No project: the legacy columns answer.
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "UPDATE conversations SET folder_path = ?2, folder_trust = 'auto' WHERE id = ?1",
                params![conv.id, r"C:\legacy"],
            )
            .unwrap();
        }
        assert_eq!(
            db.conversation_folder(&conv.id).unwrap(),
            (Some(r"C:\legacy".to_string()), "auto".to_string())
        );

        // With a project, the project answers — including its trust, which is
        // the whole reason trust moved off the conversation.
        let project = db.create_project("owned", Some(r"C:\owned")).unwrap();
        db.set_conversation_project(&conv.id, Some(&project.id)).unwrap();
        db.set_project_trust(&project.id, "read-only").unwrap();
        assert_eq!(
            db.conversation_folder(&conv.id).unwrap(),
            (Some(r"C:\owned".to_string()), "read-only".to_string())
        );

        // Leaving puts the conversation back on its own columns, and leaves the
        // project standing.
        db.set_conversation_project(&conv.id, None).unwrap();
        assert_eq!(db.conversation_folder(&conv.id).unwrap(), (None, "auto".to_string()));
        assert!(db.get_project(&project.id).unwrap().is_some(), "the project survives");
    }

    /// `PRJ-3-T`: attaching a folder that is already a project joins it rather
    /// than making a second one, and the trust granted the first time is
    /// already there the second time. That is the daily payoff of the entity.
    #[test]
    fn attaching_a_known_folder_joins_its_project_and_inherits_the_trust() {
        let db = Db::open_in_memory().unwrap();
        let first = db.create_conversation("first", None, false).unwrap();
        let second = db.create_conversation("second", None, false).unwrap();

        db.set_conversation_folder(&first.id, Some(r"C:\work\shared")).unwrap();
        db.set_conversation_trust(&first.id, "auto").unwrap();

        db.set_conversation_folder(&second.id, Some(r"C:\work\shared")).unwrap();

        assert_eq!(db.list_projects(false).unwrap().len(), 1, "joined, not duplicated");
        assert_eq!(
            db.conversation_folder(&second.id).unwrap(),
            (Some(r"C:\work\shared".to_string()), "auto".to_string()),
            "the second chat inherits the trust the first one granted"
        );

        // Detaching forgets the folder for that chat only.
        db.set_conversation_folder(&second.id, None).unwrap();
        assert_eq!(db.conversation_folder(&second.id).unwrap().0, None);
        assert_eq!(
            db.conversation_folder(&first.id).unwrap().0,
            Some(r"C:\work\shared".to_string()),
            "the other session in the project is untouched"
        );
        assert_eq!(db.list_projects(false).unwrap().len(), 1, "the project is untouched");
    }

    /// Archiving hides a project without touching a byte on disk — and working
    /// in the folder again brings it back, because that is the plainest
    /// possible statement that you still want it.
    #[test]
    fn an_archived_project_is_hidden_until_the_folder_is_opened_again() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("thing", Some(r"C:\work\thing")).unwrap();
        db.set_project_archived(&project.id, true).unwrap();

        assert!(db.list_projects(false).unwrap().is_empty(), "hidden from the Rail");
        assert_eq!(db.list_projects(true).unwrap().len(), 1, "still there when asked for");

        let conv = db.create_conversation("back", None, false).unwrap();
        db.set_conversation_folder(&conv.id, Some(r"C:\work\thing")).unwrap();
        assert_eq!(db.list_projects(false).unwrap().len(), 1, "opening the folder un-archives it");
    }

    /// `PRJ-1a`: a project stops being a folder wearing a project's clothes.
    /// The rebuild has to carry every column of every row across — a migration
    /// that quietly dropped somebody's trust level or tab set would be worse
    /// than one that failed.
    #[test]
    fn the_rebuild_makes_root_path_optional_and_preserves_every_row() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("alpha", Some(r"C:\work\alpha")).unwrap();
        db.set_project_trust(&project.id, "read-only").unwrap();
        db.set_project_tabs(&project.id, Some(r#"{"sessionTabs":["a"]}"#)).unwrap();

        // Roll back so the v28 block runs again over a table that is already
        // the new shape — the idempotency the ladder relies on.
        {
            let conn = db.conn.lock().unwrap();
            conn.pragma_update(None, "user_version", 27).unwrap();
        }
        db.migrate().unwrap();

        let after = db.get_project(&project.id).unwrap().expect("still there");
        assert_eq!(after.root_path.as_deref(), Some(r"C:\work\alpha"));
        assert_eq!(after.trust, "read-only", "trust survived the rebuild");
        assert_eq!(after.tabs_json.as_deref(), Some(r#"{"sessionTabs":["a"]}"#));

        // And the point of the whole exercise: a project with no folder.
        let book = db.create_project("The book", None).unwrap();
        assert!(book.root_path.is_none());
        assert!(db.get_project(&book.id).unwrap().unwrap().root_path.is_none());

        // Two of them coexist — SQLite treats NULLs as distinct under a unique
        // index, which is the whole reason `root_path` could stay UNIQUE.
        let job = db.create_project("Job hunt", None).unwrap();
        assert_ne!(job.id, book.id);
        assert_eq!(db.list_projects(false).unwrap().len(), 3);
    }

    /// `PRJ-3a`: the three ways attaching a folder can land, each asserted.
    /// Guessing between them is how a folder gets silently swapped out from
    /// under somebody's other sessions.
    #[test]
    fn attaching_a_folder_adopts_moves_or_joins_depending_on_the_project() {
        let db = Db::open_in_memory().unwrap();

        // 1. A chat in a folderless project: the *project* adopts the folder,
        //    which is what makes "start a project, add a folder later" work.
        let book = db.create_project("The book", None).unwrap();
        let chat = db.create_conversation("c", None, false).unwrap();
        db.set_conversation_project(&chat.id, Some(&book.id)).unwrap();
        db.set_conversation_folder(&chat.id, Some(r"C:\work\book")).unwrap();

        assert_eq!(db.list_projects(false).unwrap().len(), 1, "no second project appeared");
        let book = db.get_project(&book.id).unwrap().unwrap();
        assert_eq!(book.root_path.as_deref(), Some(r"C:\work\book"));
        assert_eq!(db.conversation_project(&chat.id).unwrap().unwrap().id, book.id);

        // 2. A different folder, owned by nobody: the project moves to it.
        db.set_conversation_folder(&chat.id, Some(r"C:\work\book2")).unwrap();
        assert_eq!(
            db.get_project(&book.id).unwrap().unwrap().root_path.as_deref(),
            Some(r"C:\work\book2")
        );
        assert_eq!(db.conversation_project(&chat.id).unwrap().unwrap().id, book.id);

        // 3. A folder another project already owns: `root_path` is unique, so
        //    the folder's project wins and the *conversation* moves to it
        //    rather than the folder being stolen.
        let other = db.create_project("other", Some(r"C:\work\other")).unwrap();
        db.set_conversation_folder(&chat.id, Some(r"C:\work\other")).unwrap();
        assert_eq!(db.conversation_project(&chat.id).unwrap().unwrap().id, other.id);
        assert_eq!(
            db.get_project(&other.id).unwrap().unwrap().root_path.as_deref(),
            Some(r"C:\work\other"),
            "the folder's own project kept it"
        );
    }

    /// Two gestures, two scopes, and keeping them apart is the point.
    ///
    /// Detaching from a *chat* takes that chat out of the project and leaves
    /// its siblings working exactly where they were — a control that sits on
    /// one chat must never change what every other session is working in.
    /// Removing the folder from the *project* is the one that reaches them
    /// all, and it lives in the project view where the thing being changed is
    /// visibly the project.
    #[test]
    fn detaching_a_chat_and_removing_a_projects_folder_are_different_scopes() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_conversation("a", None, false).unwrap();
        let b = db.create_conversation("b", None, false).unwrap();
        db.set_conversation_folder(&a.id, Some(r"C:\work\thing")).unwrap();
        let project = db.conversation_project(&a.id).unwrap().unwrap();
        db.set_conversation_project(&b.id, Some(&project.id)).unwrap();

        // The chat leaves; the project and its other session stand.
        db.set_conversation_folder(&a.id, None).unwrap();
        assert!(db.conversation_project(&a.id).unwrap().is_none(), "a left the project");
        assert_eq!(db.conversation_folder(&a.id).unwrap().0, None);
        assert_eq!(
            db.conversation_folder(&b.id).unwrap().0,
            Some(r"C:\work\thing".to_string()),
            "b is untouched"
        );
        assert!(db.get_project(&project.id).unwrap().unwrap().root_path.is_some());

        // Removing the project's folder reaches every session in it, and
        // removes none of them from it — the folder was a property, and
        // dropping a property is not leaving.
        db.set_project_root(&project.id, None).unwrap();
        assert!(db.get_project(&project.id).unwrap().unwrap().root_path.is_none());
        assert_eq!(
            db.conversation_project(&b.id).unwrap().map(|p| p.id).as_ref(),
            Some(&project.id),
            "b stayed in the project"
        );
        assert_eq!(db.conversation_folder(&b.id).unwrap().0, None, "and lost the folder");
    }

    /// `PRJ-7`: instructions round-trip, and empty is stored as absent rather
    /// than as an empty string that would render a heading with nothing in it.
    #[test]
    fn project_instructions_round_trip_and_blank_reads_as_none() {
        let db = Db::open_in_memory().unwrap();
        let p = db.create_project("book", None).unwrap();
        assert!(p.instructions.is_none(), "nothing to say by default");

        db.set_project_instructions(&p.id, Some("  Quotes are in EUR.  ")).unwrap();
        assert_eq!(
            db.get_project(&p.id).unwrap().unwrap().instructions.as_deref(),
            Some("Quotes are in EUR."),
            "trimmed"
        );

        db.set_project_instructions(&p.id, Some("   ")).unwrap();
        assert!(db.get_project(&p.id).unwrap().unwrap().instructions.is_none());
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

    /// Two rows naming one file, as the duplicate-download bug produced: the
    /// Models view was left mid-download, the button reverted to "Download",
    /// and the second click registered a second row for the same path.
    fn a_duplicate() -> NewModelEntry {
        NewModelEntry {
            name: "qwen".into(),
            path: "/models/qwen.gguf".into(),
            quant: None,
            size_bytes: None,
            vision: false,
        }
    }

    #[test]
    fn duplicate_library_rows_for_one_file_collapse_on_upgrade() {
        let db = Db::open_in_memory().unwrap();
        let first = db.add_model(&a_duplicate()).unwrap();
        db.add_model(&a_duplicate()).unwrap();
        db.add_model(&a_duplicate()).unwrap();
        let other = db.add_model(&a_model("llama")).unwrap();
        assert_eq!(db.list_models().unwrap().len(), 4, "three copies plus one real second model");

        {
            let conn = db.conn.lock().unwrap();
            conn.pragma_update(None, "user_version", 17).unwrap();
        }
        db.migrate().unwrap();

        let rows = db.list_models().unwrap();
        assert_eq!(rows.len(), 2, "the three copies collapse to one");
        assert!(rows.iter().any(|m| m.id == first.id), "the earliest row is the one kept");
        assert!(rows.iter().any(|m| m.id == other.id), "a genuinely different file is untouched");
    }

    /// The default must survive the collapse: if the row carrying it was one
    /// of the copies removed, the role would otherwise be left with none.
    #[test]
    fn dedupe_gives_a_role_its_default_back() {
        let db = Db::open_in_memory().unwrap();
        db.add_model(&a_duplicate()).unwrap();
        let second = db.add_model(&a_duplicate()).unwrap();
        db.set_default_model(&second.id).unwrap();

        {
            let conn = db.conn.lock().unwrap();
            conn.pragma_update(None, "user_version", 17).unwrap();
        }
        db.migrate().unwrap();

        let rows = db.list_models().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_default, "the surviving row picks the default back up");
    }

    /// What makes a repeat download idempotent rather than duplicating.
    #[test]
    fn a_model_is_findable_by_its_path() {
        let db = Db::open_in_memory().unwrap();
        let added = db.add_model(&a_model("qwen")).unwrap();
        let found = db.find_model_by_path("/models/qwen.gguf").unwrap();
        assert_eq!(found.map(|m| m.id), Some(added.id));
        assert!(db.find_model_by_path("/models/nothing.gguf").unwrap().is_none());
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
