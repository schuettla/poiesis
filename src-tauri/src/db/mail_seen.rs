//! The rows of the small inbox (`USE-6a`, schema v38).
//!
//! One row per message in an account's INBOX: who wrote, the subject, the date,
//! and what the decision model said about it (`triage_json`). Never the body.
//! A row is kept for 30 days.

use rusqlite::{params, OptionalExtension, Row};

use super::{Db, DbError};

/// A message as the sync found it.
#[derive(Debug, Clone)]
pub struct NewSeen<'a> {
    pub account_id: &'a str,
    pub uid: u32,
    pub message_id: Option<&'a str>,
    pub from: &'a str,
    pub subject: &'a str,
    pub date: &'a str,
    pub sent_at: Option<i64>,
    pub unread: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SeenRow {
    pub account_id: String,
    pub uid: u32,
    pub message_id: Option<String>,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub sent_at: Option<i64>,
    pub unread: bool,
    /// What the check said, as `agent::decide::mailtriage` writes it. `None`
    /// until a check ran.
    pub triage: Option<serde_json::Value>,
    pub triaged_at: Option<i64>,
    pub fetched_at: i64,
}

const COLUMNS: &str = "account_id, uid, message_id, from_addr, subject, date, sent_at, unread, triage_json, triaged_at, fetched_at";

fn seen_row(r: &Row) -> rusqlite::Result<SeenRow> {
    let triage: Option<String> = r.get(8)?;
    Ok(SeenRow {
        account_id: r.get(0)?,
        uid: r.get::<_, i64>(1)?.max(0) as u32,
        message_id: r.get(2)?,
        from: r.get(3)?,
        subject: r.get(4)?,
        date: r.get(5)?,
        sent_at: r.get(6)?,
        unread: r.get::<_, i64>(7)? != 0,
        triage: triage.and_then(|t| serde_json::from_str(&t).ok()),
        triaged_at: r.get(9)?,
        fetched_at: r.get(10)?,
    })
}

impl Db {
    /// Keep new messages. A message already there is left as it is, so a second
    /// look at the same UID never wipes what a check said. Returns how many were new.
    pub fn add_mail_seen(&self, rows: &[NewSeen], now: i64) -> Result<usize, DbError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut added = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO mail_seen(account_id, uid, message_id, from_addr, subject, date, sent_at, unread, fetched_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for r in rows {
                added += stmt.execute(params![
                    r.account_id,
                    r.uid,
                    r.message_id,
                    r.from,
                    r.subject,
                    r.date,
                    r.sent_at,
                    r.unread as i64,
                    now
                ])?;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// The newest messages no check has looked at yet. `account_id` narrows it.
    pub fn mail_seen_untriaged(&self, account_id: Option<&str>, limit: i64) -> Result<Vec<SeenRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM mail_seen WHERE triage_json IS NULL AND (?1 IS NULL OR account_id = ?1)
             ORDER BY uid DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![account_id, limit], seen_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Newest first. `account_id` narrows it.
    pub fn mail_seen_list(&self, account_id: Option<&str>, limit: i64) -> Result<Vec<SeenRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM mail_seen WHERE (?1 IS NULL OR account_id = ?1)
             ORDER BY COALESCE(sent_at, fetched_at) DESC, uid DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![account_id, limit], seen_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn mail_seen_get(&self, account_id: &str, uid: u32) -> Result<Option<SeenRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM mail_seen WHERE account_id = ?1 AND uid = ?2"),
                params![account_id, uid],
                seen_row,
            )
            .optional()?)
    }

    /// Write what a check said. `false` when the message is gone (pruned, or its
    /// account removed) while the check ran.
    pub fn set_mail_triage(&self, account_id: &str, uid: u32, triage_json: &str, now: i64) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE mail_seen SET triage_json = ?3, triaged_at = ?4 WHERE account_id = ?1 AND uid = ?2",
            params![account_id, uid, triage_json, now],
        )?;
        Ok(n > 0)
    }

    /// The newest UID kept for an account, for a sync that fetches only newer mail.
    pub fn mail_seen_newest_uid(&self, account_id: &str) -> Result<Option<u32>, DbError> {
        let conn = self.conn.lock().unwrap();
        let uid: Option<i64> =
            conn.query_row("SELECT MAX(uid) FROM mail_seen WHERE account_id = ?1", [account_id], |r| r.get(0))?;
        Ok(uid.map(|u| u.max(0) as u32))
    }

    /// Forget one account's rows: its server renumbered the mailbox (`UIDVALIDITY`),
    /// so the UIDs kept no longer mean the same messages.
    pub fn clear_mail_seen(&self, account_id: &str) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM mail_seen WHERE account_id = ?1", [account_id])?)
    }

    /// Drop what was fetched before `before`.
    pub fn prune_mail_seen(&self, before: i64) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM mail_seen WHERE fetched_at < ?1", [before])?)
    }

    /// Forget every row of every account (`EXP-4`, "Reset its data").
    pub fn clear_all_mail_seen(&self) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM mail_seen", [])?)
    }

    pub fn count_mail_seen(&self) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT COUNT(*) FROM mail_seen", [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewMailAccount;

    fn account(db: &Db) -> String {
        db.add_mail_account(&NewMailAccount {
            label: "Work".into(),
            email: "me@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            username: "me@example.com".into(),
            security: "tls".into(),
        })
        .unwrap()
        .id
    }

    fn msg<'a>(account_id: &'a str, uid: u32, subject: &'a str) -> NewSeen<'a> {
        NewSeen {
            account_id,
            uid,
            message_id: Some("<a@b>"),
            from: "ana@example.com",
            subject,
            date: "Thu, 23 Oct 2026 08:52:00 +0000",
            sent_at: Some(1_000 + uid as i64),
            unread: true,
        }
    }

    #[test]
    fn a_new_message_is_kept_and_a_second_look_changes_nothing() {
        let db = Db::open_in_memory().unwrap();
        let a = account(&db);
        assert_eq!(db.add_mail_seen(&[msg(&a, 1, "One"), msg(&a, 2, "Two")], 10).unwrap(), 2);
        db.set_mail_triage(&a, 1, r#"{"category":"work"}"#, 11).unwrap();
        // The same UIDs again, one of them changed: nothing is overwritten.
        assert_eq!(db.add_mail_seen(&[msg(&a, 1, "One, edited"), msg(&a, 3, "Three")], 20).unwrap(), 1);
        let one = db.mail_seen_get(&a, 1).unwrap().unwrap();
        assert_eq!(one.subject, "One");
        assert_eq!(one.triage.unwrap()["category"], "work");
        assert_eq!(db.count_mail_seen().unwrap(), 3);
    }

    #[test]
    fn untriaged_comes_newest_first_and_a_checked_message_leaves_it() {
        let db = Db::open_in_memory().unwrap();
        let a = account(&db);
        db.add_mail_seen(&[msg(&a, 1, "a"), msg(&a, 2, "b"), msg(&a, 3, "c")], 10).unwrap();
        let waiting: Vec<u32> = db.mail_seen_untriaged(None, 2).unwrap().iter().map(|r| r.uid).collect();
        assert_eq!(waiting, [3, 2]);
        assert!(db.set_mail_triage(&a, 3, "{}", 11).unwrap());
        let waiting: Vec<u32> = db.mail_seen_untriaged(Some(&a), 10).unwrap().iter().map(|r| r.uid).collect();
        assert_eq!(waiting, [2, 1]);
        assert!(!db.set_mail_triage(&a, 99, "{}", 11).unwrap(), "no such message");
    }

    #[test]
    fn the_list_is_newest_first_and_can_be_narrowed_to_one_account() {
        let db = Db::open_in_memory().unwrap();
        let a = account(&db);
        let b = account(&db);
        db.add_mail_seen(&[msg(&a, 1, "old"), msg(&a, 2, "new"), msg(&b, 1, "other")], 10).unwrap();
        assert_eq!(db.mail_seen_list(None, 10).unwrap().len(), 3);
        let mine: Vec<String> = db.mail_seen_list(Some(&a), 10).unwrap().into_iter().map(|r| r.subject).collect();
        assert_eq!(mine, ["new", "old"]);
        assert_eq!(db.mail_seen_list(None, 2).unwrap().len(), 2);
        assert_eq!(db.mail_seen_newest_uid(&a).unwrap(), Some(2));
        assert_eq!(db.mail_seen_newest_uid("none").unwrap(), None);
    }

    /// `USE-T-5`: rows older than 30 days go, newer ones stay.
    #[test]
    fn rows_fetched_before_the_cut_are_pruned() {
        let db = Db::open_in_memory().unwrap();
        let a = account(&db);
        db.add_mail_seen(&[msg(&a, 1, "old")], 100).unwrap();
        db.add_mail_seen(&[msg(&a, 2, "new")], 5_000).unwrap();
        assert_eq!(db.prune_mail_seen(1_000).unwrap(), 1);
        assert!(db.mail_seen_get(&a, 1).unwrap().is_none());
        assert!(db.mail_seen_get(&a, 2).unwrap().is_some());
    }

    #[test]
    fn removing_an_account_removes_its_messages_and_clearing_one_leaves_the_others() {
        let db = Db::open_in_memory().unwrap();
        let a = account(&db);
        let b = account(&db);
        db.add_mail_seen(&[msg(&a, 1, "x"), msg(&b, 1, "y")], 10).unwrap();
        assert_eq!(db.clear_mail_seen(&a).unwrap(), 1);
        assert_eq!(db.count_mail_seen().unwrap(), 1);
        db.delete_mail_account(&b).unwrap();
        assert_eq!(db.count_mail_seen().unwrap(), 0, "the rows follow their account");
    }
}
