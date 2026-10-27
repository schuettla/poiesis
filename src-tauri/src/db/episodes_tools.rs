//! Tool-result reads the encoding rules and credit need (`EPI-2`, `CRD-2`).
//!
//! `tool_stats` already holds one row per tool call. These read it by order and
//! by time window, which `tool_failures_in` and `tools_used_in` (counts only) do
//! not.

use rusqlite::{params, OptionalExtension};

use super::{Db, DbError};

impl Db {
    /// Did the last tool call of this conversation succeed? `None` when no tool
    /// ran. The reflection bookkeeping rows (`reflect.*`) are not tool calls.
    pub fn last_tool_ok(&self, conversation_id: &str) -> Result<Option<bool>, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT ok FROM tool_stats
                 WHERE conversation_id = ?1 AND tool_name NOT LIKE 'reflect.%'
                 ORDER BY created_at DESC, rowid DESC LIMIT 1",
                [conversation_id],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .map(|ok| ok != 0))
    }

    /// `CRD-1`: a resumed run answers from the transcript of the run it continues,
    /// so what reached the interrupted answer reached this one. Copies the record of
    /// the latest earlier answer onto `message_id`, if that answer has none.
    pub fn carry_context_to(&self, conversation_id: &str, message_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET context_json = (
                 SELECT context_json FROM messages
                 WHERE conversation_id = ?1 AND role = 'assistant' AND id <> ?2 AND context_json IS NOT NULL
                 ORDER BY rowid DESC LIMIT 1)
             WHERE id = ?2 AND context_json IS NULL",
            params![conversation_id, message_id],
        )?;
        Ok(())
    }

    /// The `context_json` of every assistant answer written in `[from, to)`: what
    /// reached each answer, for the recall numbers.
    pub fn context_records_between(&self, from: i64, to: i64) -> Result<Vec<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT context_json FROM messages
             WHERE role = 'assistant' AND context_json IS NOT NULL AND created_at >= ?1 AND created_at < ?2",
        )?;
        let rows = stmt.query_map(params![from, to], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// `(ok, failed)` calls of one tool in a conversation between two times.
    /// `to` is exclusive; `None` means no end.
    pub fn tool_results_between(
        &self,
        conversation_id: &str,
        tool: &str,
        from: i64,
        to: Option<i64>,
    ) -> Result<(i64, i64), DbError> {
        let conn = self.conn.lock().unwrap();
        let (ok, failed): (Option<i64>, Option<i64>) = conn.query_row(
            "SELECT SUM(ok = 1), SUM(ok = 0) FROM tool_stats
             WHERE conversation_id = ?1 AND tool_name = ?2 AND created_at >= ?3 AND created_at < ?4",
            params![conversation_id, tool, from, to.unwrap_or(i64::MAX)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((ok.unwrap_or(0), failed.unwrap_or(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(db: &Db, conv: &str, tool: &str, ok: bool, at: i64) {
        db.add_tool_stat("m", tool, conv, ok);
        db.exec_for_tests("UPDATE tool_stats SET created_at = ?1 WHERE rowid = (SELECT MAX(rowid) FROM tool_stats)", [at]);
    }

    #[test]
    fn the_last_tool_call_decides_how_the_run_ended() {
        let db = Db::open_in_memory().unwrap();
        let id = db.create_conversation("A chat", None, false).unwrap().id;
        assert_eq!(db.last_tool_ok(&id).unwrap(), None);
        stat(&db, &id, "read_file", false, 10);
        stat(&db, &id, "read_file", true, 20);
        assert_eq!(db.last_tool_ok(&id).unwrap(), Some(true));
        stat(&db, &id, "write_file", false, 30);
        stat(&db, &id, "reflect.critic", true, 40);
        assert_eq!(db.last_tool_ok(&id).unwrap(), Some(false), "the critic row is not a tool call");
    }

    #[test]
    fn a_resumed_answer_inherits_the_record_of_the_one_it_continues() {
        let db = Db::open_in_memory().unwrap();
        let id = db.create_conversation("A chat", None, false).unwrap().id;
        let say = |content: &str| {
            db.append_message(
                &id,
                &crate::db::NewMessage {
                    role: "assistant".into(),
                    content: content.into(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap()
            .id
        };
        let first = say("interrupted");
        db.finalize_message(&first, "interrupted", None, Some(r#"{"lessons":["a"]}"#), None, None).unwrap();
        let resumed = say("");
        db.carry_context_to(&id, &resumed).unwrap();
        // Finishing the answer without a record of its own keeps the inherited one.
        db.finalize_message(&resumed, "done", None, None, None, None).unwrap();
        assert_eq!(db.message_context_json(&resumed).unwrap().as_deref(), Some(r#"{"lessons":["a"]}"#));
        // A record the answer already has is never replaced.
        let own = say("");
        db.finalize_message(&own, "x", None, Some(r#"{"lessons":["b"]}"#), None, None).unwrap();
        db.carry_context_to(&id, &own).unwrap();
        assert_eq!(db.message_context_json(&own).unwrap().as_deref(), Some(r#"{"lessons":["b"]}"#));
    }

    #[test]
    fn tool_results_are_counted_inside_the_window() {
        let db = Db::open_in_memory().unwrap();
        let id = db.create_conversation("A chat", None, false).unwrap().id;
        stat(&db, &id, "read_file", false, 10);
        stat(&db, &id, "read_file", true, 20);
        stat(&db, &id, "read_file", true, 30);
        stat(&db, &id, "write_file", true, 25);
        assert_eq!(db.tool_results_between(&id, "read_file", 0, None).unwrap(), (2, 1));
        assert_eq!(db.tool_results_between(&id, "read_file", 15, Some(30)).unwrap(), (1, 0));
        assert_eq!(db.tool_results_between(&id, "nothing", 0, None).unwrap(), (0, 0));
    }
}
