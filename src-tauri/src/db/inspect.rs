//! `DBG-2`: the reads the session report needs and nothing else does. Each is about one
//! conversation. The report itself is put together in `agent::inspect`.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::{ActivityEntry, Db, DbError, SessionEvent};

/// One `run_usage` row, whole.
#[derive(Debug, Clone, Serialize)]
pub struct RunUsageRow {
    pub run_id: String,
    pub model_name: String,
    pub provenance: String,
    pub prompt_tokens: u64,
    pub output_tokens: u64,
    pub turns: u64,
    pub created_at: i64,
    pub paid_by: Option<String>,
    pub notional_usd: Option<f64>,
    pub duration_ms: Option<i64>,
}

/// How often one tool worked in a conversation.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCount {
    pub tool_name: String,
    pub ok: i64,
    pub total: i64,
}

/// One activation of a skill.
#[derive(Debug, Clone, Serialize)]
pub struct SkillRunRow {
    pub skill_name: String,
    pub tool_failures: i64,
    pub corrected: bool,
    pub created_at: i64,
}

impl Db {
    pub fn usage_rows_in(&self, conversation_id: &str) -> Result<Vec<RunUsageRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT run_id, model_name, provenance, prompt_tokens, output_tokens, turns, created_at,
                    paid_by, notional_usd, duration_ms
             FROM run_usage WHERE conversation_id = ?1 ORDER BY created_at, rowid",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| {
                Ok(RunUsageRow {
                    run_id: r.get(0)?,
                    model_name: r.get(1)?,
                    provenance: r.get(2)?,
                    prompt_tokens: r.get::<_, i64>(3)?.max(0) as u64,
                    output_tokens: r.get::<_, i64>(4)?.max(0) as u64,
                    turns: r.get::<_, i64>(5)?.max(0) as u64,
                    created_at: r.get(6)?,
                    paid_by: r.get(7)?,
                    notional_usd: r.get(8)?,
                    duration_ms: r.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn tool_counts_in(&self, conversation_id: &str) -> Result<Vec<ToolCount>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT tool_name, SUM(ok), COUNT(*) FROM tool_stats WHERE conversation_id = ?1
             GROUP BY tool_name ORDER BY tool_name",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| Ok(ToolCount { tool_name: r.get(0)?, ok: r.get(1)?, total: r.get(2)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn skill_runs_in(&self, conversation_id: &str) -> Result<Vec<SkillRunRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT skill_name, tool_failures, corrected, created_at FROM skill_runs
             WHERE conversation_id = ?1 ORDER BY created_at, rowid",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| {
                Ok(SkillRunRow {
                    skill_name: r.get(0)?,
                    tool_failures: r.get(1)?,
                    corrected: r.get::<_, i64>(2)? != 0,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// What the activity log says happened in a conversation, oldest first.
    pub fn activity_in(&self, conversation_id: &str) -> Result<Vec<ActivityEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, kind, detail, created_at FROM activity_log
             WHERE conversation_id = ?1 ORDER BY created_at, rowid",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| {
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

    /// The chats started below this one (helpers and members), oldest first.
    pub fn child_conversation_ids(&self, conversation_id: &str) -> Result<Vec<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id FROM conversations WHERE parent_conversation_id = ?1 ORDER BY created_at, rowid")?;
        let rows = stmt.query_map([conversation_id], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
        Ok(rows)
    }

    /// One row of a conversation's log.
    pub fn session_event(&self, conversation_id: &str, seq: i64) -> Result<Option<SessionEvent>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT seq, kind, payload_json, created_at FROM session_events
                 WHERE conversation_id = ?1 AND seq = ?2",
                params![conversation_id, seq],
                |r| Ok(SessionEvent { seq: r.get(0)?, kind: r.get(1)?, payload_json: r.get(2)?, created_at: r.get(3)? }),
            )
            .optional()?;
        Ok(row)
    }

    /// Which run wrote each row of a conversation's log, by `seq`. Rows written between
    /// runs (a compaction) have none and are left out.
    pub fn session_event_runs(&self, conversation_id: &str) -> Result<std::collections::HashMap<i64, String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT seq, run_id FROM session_events WHERE conversation_id = ?1 AND run_id IS NOT NULL",
        )?;
        let rows = stmt
            .query_map([conversation_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<std::collections::HashMap<i64, String>, _>>()?;
        Ok(rows)
    }
}
