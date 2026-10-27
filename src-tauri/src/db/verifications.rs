//! The result of checking one answer (`USE-10`, schema v39; `mode` v41, `summary` v42).
//!
//! One row a message: its claims with their verdicts, the passages each was judged
//! against and what the person said about each, kept as JSON because the screen
//! reads the whole of it at once and nothing queries into it. Checking an answer
//! again replaces the row. It goes with the message.

use rusqlite::{params, OptionalExtension, Row};

use super::{Db, DbError};

#[derive(Debug, Clone, PartialEq)]
pub struct StoredCheck {
    pub message_id: String,
    pub conversation_id: String,
    /// The claims as `agent::verify` writes them.
    pub claims_json: String,
    pub note: Option<String>,
    /// Opinions left out of the check.
    pub skipped: u32,
    pub created_at: i64,
    /// `auto` (the check every answer gets) or `deep` (the one asked for). A check
    /// kept before there was a mode was a deep one.
    pub mode: String,
    /// What an automatic check counted, as JSON (`agent::autocheck::Summary`).
    pub summary: Option<String>,
}

fn stored(r: &Row) -> rusqlite::Result<StoredCheck> {
    Ok(StoredCheck {
        message_id: r.get(0)?,
        conversation_id: r.get(1)?,
        claims_json: r.get(2)?,
        note: r.get(3)?,
        skipped: r.get::<_, i64>(4)?.max(0) as u32,
        created_at: r.get(5)?,
        mode: r.get(6)?,
        summary: r.get(7)?,
    })
}

const COLUMNS: &str = "message_id, conversation_id, claims_json, note, skipped, created_at, mode, summary";

impl Db {
    /// Keep a check, replacing an earlier one of the same message. `false` when the
    /// message is gone (deleted while the check ran).
    pub fn save_verification(&self, c: &StoredCheck) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row("SELECT 1 FROM messages WHERE id = ?1", [&c.message_id], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        if !exists {
            return Ok(false);
        }
        conn.execute(
            "INSERT OR REPLACE INTO verifications(message_id, conversation_id, claims_json, note, skipped, created_at, mode, summary)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![c.message_id, c.conversation_id, c.claims_json, c.note, c.skipped as i64, c.created_at, c.mode, c.summary],
        )?;
        Ok(true)
    }

    pub fn verification(&self, message_id: &str) -> Result<Option<StoredCheck>, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(&format!("SELECT {COLUMNS} FROM verifications WHERE message_id = ?1"), [message_id], stored)
            .optional()?)
    }

    /// Every check of a conversation, oldest first.
    pub fn verifications_for(&self, conversation_id: &str) -> Result<Vec<StoredCheck>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM verifications WHERE conversation_id = ?1 ORDER BY created_at, rowid"
        ))?;
        let rows = stmt.query_map([conversation_id], stored)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Every check made since `since` (epoch ms), for the experiment's report.
    pub fn verifications_since(&self, since: i64) -> Result<Vec<StoredCheck>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare(&format!("SELECT {COLUMNS} FROM verifications WHERE created_at >= ?1 ORDER BY created_at, rowid"))?;
        let rows = stmt.query_map([since], stored)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn count_verifications(&self) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT COUNT(*) FROM verifications", [], |r| r.get(0))?)
    }

    /// Forget every check. The answers themselves are untouched.
    pub fn delete_verifications(&self) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM verifications", [])?)
    }

    /// Write back the claims after the person answered one of them.
    pub fn set_verification_claims(&self, message_id: &str, claims_json: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE verifications SET claims_json = ?2 WHERE message_id = ?1",
            params![message_id, claims_json],
        )?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewMessage;

    fn message(db: &Db) -> (String, String) {
        let conv = db.create_conversation("Facts", None, false).unwrap();
        let m = db.append_message(&conv.id, &note("Paris is the capital of France.")).unwrap();
        (conv.id, m.id)
    }

    fn note(content: &str) -> NewMessage {
        NewMessage {
            role: "assistant".into(),
            content: content.into(),
            model_name: None,
            model_provenance: None,
            steps_json: None,
            attachments: vec![],
        }
    }

    fn check(conv: &str, msg: &str, claims: &str, at: i64) -> StoredCheck {
        StoredCheck {
            message_id: msg.into(),
            conversation_id: conv.into(),
            claims_json: claims.into(),
            note: None,
            skipped: 1,
            created_at: at,
            mode: "deep".into(),
            summary: None,
        }
    }

    #[test]
    fn a_check_is_kept_and_checking_again_replaces_it() {
        let db = Db::open_in_memory().unwrap();
        let (conv, msg) = message(&db);
        assert!(db.save_verification(&check(&conv, &msg, "[1]", 10)).unwrap());
        assert!(db.save_verification(&check(&conv, &msg, "[1,2]", 20)).unwrap());
        let got = db.verification(&msg).unwrap().unwrap();
        assert_eq!((got.claims_json.as_str(), got.created_at, got.skipped), ("[1,2]", 20, 1));
        assert_eq!(db.verifications_for(&conv).unwrap().len(), 1);
    }

    #[test]
    fn a_message_that_is_gone_keeps_nothing_and_the_row_goes_with_its_conversation() {
        let db = Db::open_in_memory().unwrap();
        let (conv, msg) = message(&db);
        assert!(!db.save_verification(&check(&conv, "missing", "[]", 1)).unwrap());
        db.save_verification(&check(&conv, &msg, "[]", 1)).unwrap();
        db.delete_conversation(&conv).unwrap();
        assert!(db.verification(&msg).unwrap().is_none(), "the conversation took the check with it");
    }

    #[test]
    fn a_conversations_checks_come_oldest_first_and_an_answer_is_written_back() {
        let db = Db::open_in_memory().unwrap();
        let (conv, first) = message(&db);
        let second = db.append_message(&conv, &note("More.")).unwrap().id;
        db.save_verification(&check(&conv, &second, "[\"b\"]", 20)).unwrap();
        db.save_verification(&check(&conv, &first, "[\"a\"]", 10)).unwrap();
        let order: Vec<String> = db.verifications_for(&conv).unwrap().into_iter().map(|c| c.message_id).collect();
        assert_eq!(order, [first.clone(), second]);
        assert!(db.set_verification_claims(&first, "[\"a2\"]").unwrap());
        assert_eq!(db.verification(&first).unwrap().unwrap().claims_json, "[\"a2\"]");
        assert!(!db.set_verification_claims("missing", "[]").unwrap());
    }
}
