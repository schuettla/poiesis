//! What a finished conversation turned into (`EPI-1`, schema v43), and how far
//! each remembered item helped (`CRD-2`).
//!
//! An episode is not a table. It is columns on the `conversations` row: the
//! chat is the episode, and deleting the chat deletes it. The columns are
//! written in two steps by the reflection pass. First the rules (outcome, task
//! kind, signals: no model). Then, when a model was reachable, the draft (goal,
//! digest, what worked, what went wrong, candidate lessons).
//!
//! Credit is three counters on the `memory_usage` row an item already has.

use rusqlite::{params, OptionalExtension};

use super::{Db, DbError};

/// Bumped when the encoding rules change, so a later pass can tell old rows from
/// new ones. Credit is applied once per conversation and version.
pub const ENCODE_VERSION: i64 = 1;

/// The `vectors` scope an episode's text is embedded under.
pub const EPISODE_SCOPE: &str = "episode";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Episode {
    pub conversation_id: String,
    pub goal: Option<String>,
    /// `done | partial | failed | abandoned | unknown`.
    pub outcome: Option<String>,
    pub task_kind: Option<String>,
    pub digest: Option<String>,
    pub worked: Option<String>,
    pub went_wrong: Option<String>,
    pub signals_json: Option<String>,
    pub candidates_json: Option<String>,
    /// 0 until the conversation was encoded.
    pub encode_version: i64,
    pub consolidated_at: Option<i64>,
}

/// How far one remembered item has helped. `chances` counts the answers it
/// reached; `helped` and `hurt` count the ones where it clearly did one or the
/// other. The rest had no effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Credit {
    pub chances: i64,
    pub helped: i64,
    pub hurt: i64,
    pub last_outcome_at: Option<i64>,
}

/// Under this many chances an item is not judged either way.
pub const MIN_CHANCES: i64 = 3;

impl Credit {
    /// `(helped + 1) / (helped + hurt + 2)`: an unknown item sits at one half.
    pub fn usefulness(&self) -> f64 {
        (self.helped + 1) as f64 / (self.helped + self.hurt + 2) as f64
    }

    pub fn judged(&self) -> bool {
        self.chances >= MIN_CHANCES
    }
}

const EPISODE_COLUMNS: &str = "id, goal, outcome, task_kind, digest, worked, went_wrong, \
     signals_json, candidates_json, COALESCE(encode_version, 0), consolidated_at";

fn episode_row(r: &rusqlite::Row) -> rusqlite::Result<Episode> {
    Ok(Episode {
        conversation_id: r.get(0)?,
        goal: r.get(1)?,
        outcome: r.get(2)?,
        task_kind: r.get(3)?,
        digest: r.get(4)?,
        worked: r.get(5)?,
        went_wrong: r.get(6)?,
        signals_json: r.get(7)?,
        candidates_json: r.get(8)?,
        encode_version: r.get(9)?,
        consolidated_at: r.get(10)?,
    })
}

impl Db {
    /// The episode columns of one conversation; `None` when it does not exist.
    pub fn episode(&self, conversation_id: &str) -> Result<Option<Episode>, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                &format!("SELECT {EPISODE_COLUMNS} FROM conversations WHERE id = ?1"),
                [conversation_id],
                episode_row,
            )
            .optional()?)
    }

    /// Step one of encoding: what the rules found. Leaves the draft columns alone,
    /// so running the rules again never erases a digest.
    pub fn save_episode_rules(
        &self,
        conversation_id: &str,
        outcome: &str,
        task_kind: &str,
        signals_json: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET outcome = ?2, task_kind = ?3, signals_json = ?4, encode_version = ?5
             WHERE id = ?1",
            params![conversation_id, outcome, task_kind, signals_json, ENCODE_VERSION],
        )?;
        Ok(())
    }

    /// Step two: what the model drafted. A stale vector goes with the old digest.
    pub fn save_episode_draft(
        &self,
        conversation_id: &str,
        goal: &str,
        digest: &str,
        worked: &str,
        went_wrong: &str,
        candidates_json: &str,
    ) -> Result<(), DbError> {
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "UPDATE conversations SET goal = ?2, digest = ?3, worked = ?4, went_wrong = ?5, candidates_json = ?6
                 WHERE id = ?1",
                params![conversation_id, goal, digest, worked, went_wrong, candidates_json],
            )?;
        }
        self.delete_vectors_for_ref("episode", EPISODE_SCOPE, conversation_id)
    }

    /// Where each candidate got to, once the pass has acted on them.
    pub fn set_episode_candidates(&self, conversation_id: &str, candidates_json: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET candidates_json = ?2 WHERE id = ?1",
            params![conversation_id, candidates_json],
        )?;
        Ok(())
    }

    /// Stamp the conversations a tidy-up has folded in (`SLP`, later phases).
    pub fn mark_episode_consolidated(&self, conversation_id: &str, at: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE conversations SET consolidated_at = ?2 WHERE id = ?1", params![conversation_id, at])?;
        Ok(())
    }

    /// Encoded episodes with a digest that no tidy-up has folded in yet, newest
    /// first (`SLP-1`, step 3 of the daily pass).
    pub fn unconsolidated_episodes(&self, limit: usize) -> Result<Vec<Episode>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {EPISODE_COLUMNS} FROM conversations
             WHERE encode_version > 0 AND consolidated_at IS NULL AND digest IS NOT NULL AND TRIM(digest) <> ''
             ORDER BY updated_at DESC LIMIT ?1"
        ))?;
        let rows = stmt.query_map([limit as i64], episode_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Encoded episodes with a digest whose conversation was last worked on at or
    /// after `since` (ms), newest first. A candidate that waited on one night's
    /// conversation can be promoted by a later one, so the pool reaches back.
    pub fn episodes_since(&self, since: i64) -> Result<Vec<Episode>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {EPISODE_COLUMNS} FROM conversations
             WHERE encode_version > 0 AND updated_at >= ?1 AND digest IS NOT NULL AND TRIM(digest) <> ''
             ORDER BY updated_at DESC"
        ))?;
        let rows = stmt.query_map([since], episode_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// `(conversation id, text)` for every episode with a digest and no vector
    /// under `model`. The text is what gets embedded: the goal and the digest, so
    /// a new task is matched against what the old one was for.
    pub fn episodes_missing_vector(&self, model: &str) -> Result<Vec<(String, String)>, DbError> {
        let have = self.vector_ref_keys_for_scope("episode", EPISODE_SCOPE, model)?;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, COALESCE(goal, ''), digest FROM conversations
             WHERE digest IS NOT NULL AND TRIM(digest) <> ''",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter(|(id, _, _)| !have.contains(id))
            .map(|(id, goal, digest)| (id, format!("{goal}\n{digest}")))
            .collect())
    }

    /// How the conversations worked on since `since` (ms) ended, by task kind
    /// and outcome: `(task_kind, outcome, count)`. Only encoded conversations
    /// with a known kind and outcome (`MET-1`).
    pub fn outcomes_by_kind(&self, since: i64) -> Result<Vec<(String, String, i64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT task_kind, outcome, COUNT(*) FROM conversations
             WHERE encode_version > 0 AND updated_at >= ?1
               AND task_kind IS NOT NULL AND task_kind <> 'unknown' AND outcome IS NOT NULL
             GROUP BY task_kind, outcome",
        )?;
        let rows = stmt
            .query_map([since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The credit of one collection, by name.
    pub fn memory_credit_map(&self, collection: &str) -> Result<std::collections::HashMap<String, Credit>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT ref_key, chances, helped, hurt, last_outcome_at FROM memory_usage WHERE collection = ?1",
        )?;
        let rows = stmt
            .query_map([collection], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Credit { chances: r.get(1)?, helped: r.get(2)?, hurt: r.get(3)?, last_outcome_at: r.get(4)? },
                ))
            })?
            .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
        Ok(rows)
    }

    /// Add to one item's credit. A row is made when the item has none, dated at
    /// `at`: the moment it reached the answer being judged, which is when it was
    /// last surfaced. An existing row keeps its own `last_used_at`.
    pub fn add_memory_credit(
        &self,
        collection: &str,
        name: &str,
        chances: i64,
        helped: i64,
        hurt: i64,
        at: i64,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO memory_usage(collection, ref_key, last_used_at, chances, helped, hurt, last_outcome_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?3)
             ON CONFLICT(collection, ref_key) DO UPDATE SET
               chances = chances + excluded.chances,
               helped = helped + excluded.helped,
               hurt = hurt + excluded.hurt,
               last_outcome_at = excluded.last_outcome_at",
            params![collection, name, at, chances, helped, hurt],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(db: &Db) -> String {
        db.create_conversation("A chat", None, false).unwrap().id
    }

    #[test]
    fn a_new_chat_has_an_empty_episode() {
        let db = Db::open_in_memory().unwrap();
        let id = chat(&db);
        let e = db.episode(&id).unwrap().unwrap();
        assert_eq!(e.encode_version, 0);
        assert!(e.outcome.is_none() && e.digest.is_none());
        assert!(db.episode("missing").unwrap().is_none());
    }

    #[test]
    fn the_rules_and_the_draft_are_written_apart() {
        let db = Db::open_in_memory().unwrap();
        let id = chat(&db);
        db.save_episode_rules(&id, "done", "report", "{}").unwrap();
        db.save_episode_draft(&id, "expense summary", "I made it.", "the csv", "", "[]").unwrap();
        // Running the rules again keeps the draft.
        db.save_episode_rules(&id, "partial", "report", "{\"a\":1}").unwrap();
        let e = db.episode(&id).unwrap().unwrap();
        assert_eq!(e.outcome.as_deref(), Some("partial"));
        assert_eq!(e.digest.as_deref(), Some("I made it."));
        assert_eq!(e.encode_version, ENCODE_VERSION);
    }

    #[test]
    fn an_episode_without_a_vector_is_listed_and_one_with_is_not() {
        let db = Db::open_in_memory().unwrap();
        let a = chat(&db);
        let b = chat(&db);
        db.save_episode_draft(&a, "goal a", "digest a", "", "", "[]").unwrap();
        db.save_episode_draft(&b, "goal b", "digest b", "", "", "[]").unwrap();
        assert_eq!(db.episodes_missing_vector("m1").unwrap().len(), 2);
        db.insert_vectors(&[super::super::vectors::NewVector {
            owner_kind: "episode".into(),
            scope_key: EPISODE_SCOPE.into(),
            ref_key: a.clone(),
            chunk_ix: 0,
            text: "goal a\ndigest a".into(),
            model: "m1".into(),
            dim: 2,
            vec: vec![1.0, 0.0],
            mtime: None,
        }])
        .unwrap();
        let missing = db.episodes_missing_vector("m1").unwrap();
        assert_eq!(missing, vec![(b.clone(), "goal b\ndigest b".to_string())]);
        // A new digest drops the old vector, so it is embedded again.
        db.save_episode_draft(&a, "goal a", "digest a2", "", "", "[]").unwrap();
        assert_eq!(db.episodes_missing_vector("m1").unwrap().len(), 2);
    }

    #[test]
    fn deleting_a_chat_deletes_its_episode_and_vector() {
        let db = Db::open_in_memory().unwrap();
        let a = chat(&db);
        db.save_episode_draft(&a, "g", "d", "", "", "[]").unwrap();
        db.insert_vectors(&[super::super::vectors::NewVector {
            owner_kind: "episode".into(),
            scope_key: EPISODE_SCOPE.into(),
            ref_key: a.clone(),
            chunk_ix: 0,
            text: "g\nd".into(),
            model: "m1".into(),
            dim: 2,
            vec: vec![1.0, 0.0],
            mtime: None,
        }])
        .unwrap();
        db.delete_conversation(&a).unwrap();
        assert!(db.episode(&a).unwrap().is_none());
        assert!(db.vector_ref_keys_for_scope("episode", EPISODE_SCOPE, "m1").unwrap().is_empty());
    }

    #[test]
    fn credit_adds_up_and_is_not_judged_before_three_chances() {
        let db = Db::open_in_memory().unwrap();
        db.add_memory_credit("lessons", "check-header", 1, 1, 0, 100).unwrap();
        db.add_memory_credit("lessons", "check-header", 1, 0, 1, 200).unwrap();
        let c = db.memory_credit_map("lessons").unwrap()["check-header"];
        assert_eq!((c.chances, c.helped, c.hurt, c.last_outcome_at), (2, 1, 1, Some(200)));
        assert!(!c.judged());
        assert!((c.usefulness() - 0.5).abs() < 1e-9);
        db.add_memory_credit("lessons", "check-header", 1, 1, 0, 300).unwrap();
        assert!(db.memory_credit_map("lessons").unwrap()["check-header"].judged());
    }

    #[test]
    fn credit_keeps_the_surfaced_date_of_an_item_that_already_had_one() {
        let db = Db::open_in_memory().unwrap();
        db.touch_memory_usage("facts", &["a".to_string()]).unwrap();
        let before = db.memory_usage_map("facts").unwrap()["a"];
        db.add_memory_credit("facts", "a", 1, 0, 1, 5).unwrap();
        assert_eq!(db.memory_usage_map("facts").unwrap()["a"], before);
        assert_eq!(db.memory_credit_map("facts").unwrap()["a"].hurt, 1);
    }
}
