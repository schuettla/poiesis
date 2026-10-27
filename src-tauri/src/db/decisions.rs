//! The decision log and its outcome labels (`CTL-3`, schema v37).
//!
//! `decisions` holds what the small model said and what the controller did with
//! it. `decision_outcomes` holds only labels that nothing else records; the ones
//! that already live in `engine_signals` and the connector tables are read from
//! there when calibration runs (`agent::decide::outcomes`), so the two cannot
//! drift.

use rusqlite::{params, Row};

use super::{now_ms, Db, DbError};

/// What a caller writes when a decision was made. The state itself is not kept.
#[derive(Debug, Clone)]
pub struct NewDecision<'a> {
    pub id: &'a str,
    pub conversation_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub purpose: &'a str,
    pub backend: &'a str,
    pub model: &'a str,
    pub ms: u32,
    pub calibrated: bool,
    pub answers: &'a serde_json::Value,
    pub input_tokens: u32,
    pub state_preview: &'a str,
    pub state_hash: &'a str,
    pub action: Option<&'a str>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DecisionRow {
    pub id: String,
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
    pub purpose: String,
    pub backend: String,
    pub model: String,
    pub ms: u32,
    pub calibrated: bool,
    pub answers: serde_json::Value,
    pub input_tokens: u32,
    pub state_preview: String,
    pub state_hash: String,
    pub action: Option<String>,
    pub at: i64,
    /// `SEE-2`: what the decision changed, in words, when it acted or was shown.
    pub note: Option<String>,
    pub outcomes: Vec<OutcomeRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OutcomeRow {
    pub decision_id: String,
    /// `right` | `wrong` | `weak_right` | `weak_wrong` | `corrected`.
    pub label: String,
    pub value: Option<String>,
    pub source: String,
    pub at: i64,
}

const COLUMNS: &str = "id, conversation_id, run_id, purpose, backend, model, ms, calibrated, answers_json, \
     input_tokens, state_preview, state_hash, action, at, note";

fn decision_row(r: &Row) -> rusqlite::Result<DecisionRow> {
    let answers: String = r.get(8)?;
    Ok(DecisionRow {
        id: r.get(0)?,
        conversation_id: r.get(1)?,
        run_id: r.get(2)?,
        purpose: r.get(3)?,
        backend: r.get(4)?,
        model: r.get(5)?,
        ms: r.get::<_, i64>(6)?.max(0) as u32,
        calibrated: r.get::<_, i64>(7)? != 0,
        answers: serde_json::from_str(&answers).unwrap_or(serde_json::Value::Null),
        input_tokens: r.get::<_, i64>(9)?.max(0) as u32,
        state_preview: r.get(10)?,
        state_hash: r.get(11)?,
        action: r.get(12)?,
        at: r.get(13)?,
        note: r.get(14)?,
        outcomes: Vec::new(),
    })
}

fn outcome_row(r: &Row) -> rusqlite::Result<OutcomeRow> {
    Ok(OutcomeRow { decision_id: r.get(0)?, label: r.get(1)?, value: r.get(2)?, source: r.get(3)?, at: r.get(4)? })
}

impl Db {
    /// Record a decision. A decision whose conversation is gone is not an error
    /// worth stopping for: the caller drops it.
    pub fn add_decision(&self, d: &NewDecision) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO decisions(id, conversation_id, run_id, purpose, backend, model, ms, calibrated,
                                   answers_json, input_tokens, state_preview, state_hash, action, at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                d.id,
                d.conversation_id,
                d.run_id,
                d.purpose,
                d.backend,
                d.model,
                d.ms,
                d.calibrated as i64,
                d.answers.to_string(),
                d.input_tokens,
                d.state_preview,
                d.state_hash,
                d.action,
                now_ms()
            ],
        )?;
        Ok(())
    }

    /// Move a decision to another time, so a test can age it.
    #[cfg(test)]
    pub(crate) fn set_decision_at(&self, id: &str, at: i64) {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE decisions SET at = ?2 WHERE id = ?1", params![id, at]).unwrap();
    }

    /// What the controller did with a decision, once the site has read it.
    pub fn set_decision_action(&self, id: &str, action: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE decisions SET action = ?2 WHERE id = ?1", params![id, action])?;
        Ok(())
    }

    /// What a decision changed, in words, for the receipt (`SEE-2`).
    pub fn set_decision_note(&self, id: &str, note: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE decisions SET note = ?2 WHERE id = ?1", params![id, note])?;
        Ok(())
    }

    /// Every answered decision of one turn, oldest first, with its labels (`SEE-2`'s
    /// receipt). The ones that carry the run's id, and the ones that carry none but
    /// were made in this conversation while the turn ran (the untrusted-page flag,
    /// the browser check).
    pub fn decisions_of_turn(
        &self,
        conversation_id: &str,
        run_id: &str,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<DecisionRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM decisions
             WHERE conversation_id = ?1 AND answers_json != '{{}}'
               AND (run_id = ?2 OR (run_id IS NULL AND at BETWEEN ?3 AND ?4))
             ORDER BY at ASC, rowid ASC"
        ))?;
        let mut rows = stmt.query_map(params![conversation_id, run_id, from_ms, to_ms], decision_row)?.collect::<Result<Vec<_>, _>>()?;
        attach_outcomes(&conn, &mut rows)?;
        Ok(rows)
    }

    /// Newest first, each with its outcome labels. `purpose` narrows it.
    pub fn decisions_recent(&self, limit: i64, purpose: Option<&str>) -> Result<Vec<DecisionRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM decisions WHERE (?1 IS NULL OR purpose = ?1) ORDER BY at DESC, rowid DESC LIMIT ?2"
        ))?;
        let mut rows = stmt.query_map(params![purpose, limit], decision_row)?.collect::<Result<Vec<_>, _>>()?;
        attach_outcomes(&conn, &mut rows)?;
        Ok(rows)
    }

    /// The newest decision of one purpose in one conversation, if any.
    pub fn latest_decision(&self, conversation_id: &str, purpose: &str) -> Result<Option<DecisionRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM decisions WHERE conversation_id = ?1 AND purpose = ?2 AND answers_json != '{{}}'
             ORDER BY at DESC, rowid DESC LIMIT 1"
        ))?;
        let mut rows = stmt.query_map(params![conversation_id, purpose], decision_row)?.collect::<Result<Vec<_>, _>>()?;
        attach_outcomes(&conn, &mut rows)?;
        Ok(rows.pop())
    }

    /// Decisions of one purpose after `since` that actually carry an answer.
    pub fn decisions_since(&self, purpose: &str, since: i64) -> Result<Vec<DecisionRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM decisions WHERE purpose = ?1 AND at > ?2 AND answers_json != '{{}}' ORDER BY at ASC, rowid ASC"
        ))?;
        let mut rows = stmt.query_map(params![purpose, since], decision_row)?.collect::<Result<Vec<_>, _>>()?;
        attach_outcomes(&conn, &mut rows)?;
        Ok(rows)
    }

    /// Label a decision. A source labels a decision once: a second write from the
    /// same source is ignored, so a screen that fires twice cannot count twice.
    /// Returns whether a row was written.
    pub fn add_outcome(&self, decision_id: &str, label: &str, value: Option<&str>, source: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "INSERT OR IGNORE INTO decision_outcomes(id, decision_id, label, value, source, at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![super::new_id(), decision_id, label, value, source, now_ms()],
        )?;
        Ok(n > 0)
    }

    /// Label a decision, replacing what the same source said before. For a label
    /// a person gives and may change (`USE-UI-5`: the answer they correct a row to),
    /// which `add_outcome` would ignore the second time.
    pub fn set_outcome(&self, decision_id: &str, label: &str, value: Option<&str>, source: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO decision_outcomes(id, decision_id, label, value, source, at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(decision_id, source) DO UPDATE SET label = excluded.label, value = excluded.value, at = excluded.at",
            params![super::new_id(), decision_id, label, value, source, now_ms()],
        )?;
        Ok(())
    }

    /// Take back what one source said about a decision (a person who changes a
    /// mark to one that carries no label). Nothing to take back is not an error.
    pub fn delete_outcome(&self, decision_id: &str, source: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM decision_outcomes WHERE decision_id = ?1 AND source = ?2",
            params![decision_id, source],
        )?;
        Ok(())
    }

    /// One decision by its id, with its labels.
    pub fn decision_by_id(&self, id: &str) -> Result<Option<DecisionRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM decisions WHERE id = ?1"))?;
        let mut rows = stmt.query_map([id], decision_row)?.collect::<Result<Vec<_>, _>>()?;
        attach_outcomes(&conn, &mut rows)?;
        Ok(rows.pop())
    }

    /// How many labels of each kind one source wrote on one purpose's decisions,
    /// by the value they carry: `(value, label, count)`. For a screen that says how
    /// a rule has fared ("matched early 12 times").
    pub fn outcome_counts(&self, purpose: &str, source: &str) -> Result<Vec<(String, String, i64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT o.value, o.label, COUNT(*) FROM decision_outcomes o JOIN decisions d ON d.id = o.decision_id
             WHERE d.purpose = ?1 AND o.source = ?2 AND o.value IS NOT NULL GROUP BY o.value, o.label",
        )?;
        let rows = stmt.query_map(params![purpose, source], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Labels written after `since`, for the "enough new labels to refit" test.
    pub fn outcomes_since(&self, purpose: Option<&str>, model: Option<&str>, since: i64) -> Result<u32, DbError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM decision_outcomes o JOIN decisions d ON d.id = o.decision_id
             WHERE o.at > ?1 AND (?2 IS NULL OR d.purpose = ?2) AND (?3 IS NULL OR d.model = ?3)",
            params![since, purpose, model],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as u32)
    }

    /// Every decision of one purpose and model that carries an answer, with the
    /// labels written for it. Calibration adds the labels read from elsewhere.
    pub fn decisions_for(&self, purpose: &str, model: &str) -> Result<Vec<DecisionRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM decisions WHERE purpose = ?1 AND model = ?2 AND answers_json != '{{}}' ORDER BY at ASC"
        ))?;
        let mut rows = stmt.query_map(params![purpose, model], decision_row)?.collect::<Result<Vec<_>, _>>()?;
        attach_outcomes(&conn, &mut rows)?;
        Ok(rows)
    }

    /// Drop decisions older than `before` (epoch ms). Their labels go with them.
    pub fn prune_decisions(&self, before: i64) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM decisions WHERE at < ?1", [before])?)
    }

    /// What the cloud charged for a decision, when its reply said (`CTL-UI-2`).
    pub fn set_decision_cost(&self, id: &str, cost_usd: f64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE decisions SET cost_usd = ?2 WHERE id = ?1", params![id, cost_usd])?;
        Ok(())
    }

    /// Answered decisions since `since` (epoch ms), by where they were asked and
    /// by which model. A row a budget skipped is not a decision that was made.
    pub fn decision_usage_since(&self, since: i64) -> Result<Vec<DecisionUsageRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT CASE WHEN backend LIKE 'openrouter:%' THEN 'openrouter' ELSE 'local' END AS kind,
                    model, COUNT(*), COALESCE(SUM(cost_usd), 0.0), COALESCE(SUM(input_tokens), 0), COALESCE(AVG(ms), 0.0)
             FROM decisions WHERE at >= ?1 AND answers_json != '{}'
             GROUP BY kind, model ORDER BY COUNT(*) DESC, model ASC",
        )?;
        let rows = stmt
            .query_map([since], |r| {
                Ok(DecisionUsageRow {
                    backend: r.get(0)?,
                    model: r.get(1)?,
                    count: r.get(2)?,
                    cost_usd: r.get(3)?,
                    input_tokens: r.get(4)?,
                    avg_ms: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// What each purpose decided since `since` (epoch ms), for an experiment's
    /// report (`EXP-4`). Only answered decisions count; a row a budget skipped is not
    /// one. `rated` and `missed` read the labels that say something firm: a weak
    /// label is a guess from what did not happen, and counting it would make a
    /// purpose that is mostly left alone look rated.
    pub fn decision_stats_since(&self, since: i64) -> Result<Vec<PurposeStats>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT d.purpose, COUNT(*),
                    COALESCE(SUM(d.backend LIKE 'openrouter:%'), 0),
                    COALESCE(SUM(d.cost_usd), 0.0),
                    COALESCE(SUM(d.action = 'acted'), 0),
                    COALESCE(SUM(EXISTS(SELECT 1 FROM decision_outcomes o
                                        WHERE o.decision_id = d.id AND o.label IN ('right', 'wrong', 'corrected'))), 0),
                    COALESCE(SUM(EXISTS(SELECT 1 FROM decision_outcomes o
                                        WHERE o.decision_id = d.id AND o.label IN ('wrong', 'corrected'))), 0)
             FROM decisions d WHERE d.at >= ?1 AND d.answers_json != '{}' GROUP BY d.purpose",
        )?;
        let rows = stmt
            .query_map([since], |r| {
                Ok(PurposeStats {
                    purpose: r.get(0)?,
                    answered: r.get(1)?,
                    cloud: r.get(2)?,
                    cost_usd: r.get(3)?,
                    acted: r.get(4)?,
                    rated: r.get(5)?,
                    missed: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Every decision of these purposes, answered or not, for the count a reset
    /// would remove.
    pub fn count_decisions_of(&self, purposes: &[&str]) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut n = 0;
        for p in purposes {
            n += conn.query_row("SELECT COUNT(*) FROM decisions WHERE purpose = ?1", [p], |r| r.get::<_, i64>(0))?;
        }
        Ok(n)
    }

    /// Forget every decision of these purposes. Their labels go with them.
    pub fn delete_decisions_of(&self, purposes: &[&str]) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut n = 0;
        for p in purposes {
            n += conn.execute("DELETE FROM decisions WHERE purpose = ?1", [p])?;
        }
        Ok(n)
    }

    /// Activity rows of one kind whose text starts with `prefix`, since `since`.
    pub fn count_activity_since(&self, kind: &str, prefix: &str, since: i64) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM activity_log WHERE kind = ?1 AND substr(detail, 1, length(?2)) = ?2 AND created_at >= ?3",
            params![kind, prefix, since],
            |r| r.get(0),
        )?)
    }

    pub fn delete_activity_starting(&self, kind: &str, prefix: &str) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "DELETE FROM activity_log WHERE kind = ?1 AND substr(detail, 1, length(?2)) = ?2",
            params![kind, prefix],
        )?)
    }
}

/// One purpose's share of an experiment's report.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PurposeStats {
    pub purpose: String,
    pub answered: i64,
    /// Answered by a cloud model.
    pub cloud: i64,
    pub cost_usd: f64,
    /// What the controller did with the answer: it acted on it.
    pub acted: i64,
    /// Decisions a firm label was written for.
    pub rated: i64,
    /// Of those, the ones that were wrong or corrected.
    pub missed: i64,
}

/// One line of the Usage screen's "Decisions" section.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DecisionUsageRow {
    /// `local` | `openrouter`
    pub backend: String,
    pub model: String,
    pub count: i64,
    pub cost_usd: f64,
    pub input_tokens: i64,
    pub avg_ms: f64,
}

fn attach_outcomes(conn: &rusqlite::Connection, rows: &mut [DecisionRow]) -> Result<(), DbError> {
    let mut stmt = conn.prepare(
        "SELECT decision_id, label, value, source, at FROM decision_outcomes WHERE decision_id = ?1 ORDER BY at ASC",
    )?;
    for row in rows {
        row.outcomes = stmt.query_map([&row.id], outcome_row)?.collect::<Result<Vec<_>, _>>()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new<'a>(id: &'a str, purpose: &'a str, answers: &'a serde_json::Value) -> NewDecision<'a> {
        NewDecision {
            id,
            conversation_id: None,
            run_id: None,
            purpose,
            backend: "local:Clef-flash",
            model: "Clef-Flash-Q4_K_M",
            ms: 40,
            calibrated: false,
            answers,
            input_tokens: 100,
            state_preview: "p",
            state_hash: "h",
            action: None,
        }
    }

    #[test]
    fn a_decision_without_a_conversation_is_kept_and_read_back_with_its_labels() {
        let db = Db::open_in_memory().unwrap();
        let answers = serde_json::json!({ "enough": { "p": 0.9 } });
        db.add_decision(&new("d1", "ret-sufficiency", &answers)).unwrap();
        db.set_decision_action("d1", "acted").unwrap();
        assert!(db.add_outcome("d1", "right", None, "test").unwrap());
        let rows = db.decisions_recent(10, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action.as_deref(), Some("acted"));
        assert_eq!(rows[0].answers["enough"]["p"], 0.9);
        assert_eq!(rows[0].outcomes[0].label, "right");
    }

    /// `CTL-UI-2`: the Usage section counts answered decisions by where they were
    /// asked and what the cloud charged. A decision a budget skipped is not one.
    #[test]
    fn usage_splits_local_from_cloud_and_adds_up_what_the_cloud_charged() {
        let db = Db::open_in_memory().unwrap();
        let answers = serde_json::json!({ "x": { "p": 0.5 } });
        for id in ["a", "b"] {
            db.add_decision(&new(id, "goal-met", &answers)).unwrap();
        }
        for (id, cost) in [("c", 0.000_021), ("d", 0.000_042)] {
            let mut d = new(id, "tool-select", &answers);
            d.backend = "openrouter:cloudflare/clef-flash@DeepInfra";
            d.model = "cloudflare/clef-flash";
            d.ms = 200;
            db.add_decision(&d).unwrap();
            db.set_decision_cost(id, cost).unwrap();
        }
        let skipped = serde_json::json!({});
        db.add_decision(&new("e", "goal-met", &skipped)).unwrap();

        let rows = db.decision_usage_since(0).unwrap();
        assert_eq!(rows.len(), 2);
        let local = rows.iter().find(|r| r.backend == "local").unwrap();
        assert_eq!((local.count, local.cost_usd, local.input_tokens), (2, 0.0, 200));
        let cloud = rows.iter().find(|r| r.backend == "openrouter").unwrap();
        assert_eq!(cloud.model, "cloudflare/clef-flash");
        assert_eq!(cloud.count, 2);
        assert!((cloud.cost_usd - 0.000_063).abs() < 1e-12, "{}", cloud.cost_usd);
        assert!((cloud.avg_ms - 200.0).abs() < 1e-9);
        assert!(db.decision_usage_since(i64::MAX).unwrap().is_empty(), "nothing since the future");
    }

    /// A source labels a decision once, so a button pressed twice counts once.
    #[test]
    fn the_same_source_labels_a_decision_only_once() {
        let db = Db::open_in_memory().unwrap();
        let answers = serde_json::json!({ "x": { "p": 0.5 } });
        db.add_decision(&new("d1", "goal-met", &answers)).unwrap();
        assert!(db.add_outcome("d1", "weak_right", None, "goal-stalled").unwrap());
        assert!(!db.add_outcome("d1", "weak_wrong", None, "goal-stalled").unwrap());
        assert!(db.add_outcome("d1", "wrong", None, "other").unwrap());
        assert_eq!(db.decisions_recent(1, None).unwrap()[0].outcomes.len(), 2);
    }

    /// `USE-UI-5`: a person who changes their mind replaces their own label and
    /// leaves every other source's alone.
    #[test]
    fn a_set_label_replaces_the_same_sources_earlier_one() {
        let db = Db::open_in_memory().unwrap();
        let answers = serde_json::json!({ "answer": { "choice": "bug" } });
        db.add_decision(&new("d1", "classify", &answers)).unwrap();
        db.set_outcome("d1", "corrected", Some("feature"), "classify-user").unwrap();
        db.set_outcome("d1", "right", Some("bug"), "classify-user").unwrap();
        db.add_outcome("d1", "weak_right", None, "other").unwrap();
        let row = db.decision_by_id("d1").unwrap().unwrap();
        assert_eq!(row.outcomes.len(), 2);
        let mine = row.outcomes.iter().find(|o| o.source == "classify-user").unwrap();
        assert_eq!((mine.label.as_str(), mine.value.as_deref()), ("right", Some("bug")));
        assert!(db.decision_by_id("nope").unwrap().is_none());
    }

    /// How a rule fared is counted per value and label, for one source and one
    /// purpose only.
    #[test]
    fn labels_are_counted_by_value_for_one_source_and_purpose() {
        let db = Db::open_in_memory().unwrap();
        let answers = serde_json::json!({ "route": { "choice": "claude-code:U2" } });
        for id in ["d1", "d2", "d3"] {
            db.add_decision(&new(id, "handover-route", &answers)).unwrap();
        }
        db.add_decision(&new("g1", "goal-met", &answers)).unwrap();
        db.add_outcome("d1", "right", Some("claude-code:U2"), "handover-card").unwrap();
        db.add_outcome("d2", "right", Some("claude-code:U2"), "handover-card").unwrap();
        db.add_outcome("d3", "wrong", Some("claude-code:U2"), "handover-card").unwrap();
        db.add_outcome("d3", "wrong", Some("claude-code:U2"), "engine-override").unwrap();
        db.add_outcome("g1", "right", Some("claude-code:U2"), "handover-card").unwrap();
        db.add_outcome("d1", "weak_right", None, "kept-out").unwrap();

        let mut counts = db.outcome_counts("handover-route", "handover-card").unwrap();
        counts.sort();
        assert_eq!(
            counts,
            vec![
                ("claude-code:U2".to_string(), "right".to_string(), 2),
                ("claude-code:U2".to_string(), "wrong".to_string(), 1),
            ]
        );
        assert!(db.outcome_counts("handover-route", "nothing").unwrap().is_empty());
    }

    /// A conversation takes its decisions with it, and their labels.
    #[test]
    fn deleting_a_conversation_takes_its_decisions_and_labels() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("t", None, false).unwrap().id;
        let answers = serde_json::json!({ "x": { "p": 0.5 } });
        let mut d = new("d1", "goal-met", &answers);
        d.conversation_id = Some(&conv);
        db.add_decision(&d).unwrap();
        db.add_outcome("d1", "right", None, "s").unwrap();
        db.delete_conversation(&conv).unwrap();
        assert!(db.decisions_recent(10, None).unwrap().is_empty());
        assert_eq!(db.outcomes_since(None, None, 0).unwrap(), 0);
    }

    #[test]
    fn only_decisions_that_carry_an_answer_are_the_latest() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("t", None, false).unwrap().id;
        let answered = serde_json::json!({ "met": { "p": 0.1 } });
        let none = serde_json::json!({});
        let mut a = new("a", "goal-met", &answered);
        a.conversation_id = Some(&conv);
        db.add_decision(&a).unwrap();
        let mut b = new("b", "goal-met", &none);
        b.conversation_id = Some(&conv);
        b.action = Some("budget: decisions");
        db.add_decision(&b).unwrap();
        assert_eq!(db.latest_decision(&conv, "goal-met").unwrap().unwrap().id, "a");
        assert_eq!(db.decisions_recent(10, Some("goal-met")).unwrap().len(), 2, "the skipped one still shows");
    }

    /// `SEE-2`: the receipt reads a turn's decisions by its run id, and those that
    /// carry none by the time the turn ran. Another chat's decisions and skipped
    /// ones are not in it, and the note comes back.
    #[test]
    fn a_turn_is_its_runs_decisions_and_the_unattached_ones_made_while_it_ran() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("t", None, false).unwrap().id;
        let other = db.create_conversation("o", None, false).unwrap().id;
        let answered = serde_json::json!({ "x": { "p": 0.9 } });
        let none = serde_json::json!({});
        let add = |id: &str, c: &str, run: Option<&str>, answers: &serde_json::Value, at: i64| {
            let mut d = new(id, "tool-select", answers);
            d.conversation_id = Some(c);
            d.run_id = run;
            db.add_decision(&d).unwrap();
            db.set_decision_at(id, at);
        };
        add("mine", &conv, Some("r1"), &answered, 100);
        add("loose", &conv, None, &answered, 150);
        add("late", &conv, None, &answered, 900);
        add("skipped", &conv, Some("r1"), &none, 120);
        add("theirs", &other, Some("r1"), &answered, 110);
        add("older-run", &conv, Some("r0"), &answered, 50);
        db.set_decision_note("mine", "Left out web search; you did not ask for it").unwrap();
        let rows = db.decisions_of_turn(&conv, "r1", 90, 200).unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["mine", "loose"]);
        assert_eq!(rows[0].note.as_deref(), Some("Left out web search; you did not ask for it"));
        assert_eq!(rows[1].note, None);
    }

    #[test]
    fn labels_are_counted_and_old_decisions_are_pruned() {
        let db = Db::open_in_memory().unwrap();
        let answers = serde_json::json!({ "x": { "p": 0.5 } });
        db.add_decision(&new("d1", "mem-scope", &answers)).unwrap();
        db.add_outcome("d1", "wrong", None, "s").unwrap();
        assert_eq!(db.outcomes_since(Some("mem-scope"), Some("Clef-Flash-Q4_K_M"), 0).unwrap(), 1);
        assert_eq!(db.outcomes_since(Some("goal-met"), None, 0).unwrap(), 0);
        assert_eq!(db.decisions_for("mem-scope", "Clef-Flash-Q4_K_M").unwrap().len(), 1);
        assert!(db.decisions_for("mem-scope", "Another").unwrap().is_empty());
        assert_eq!(db.prune_decisions(i64::MAX).unwrap(), 1);
        assert!(db.decisions_for("mem-scope", "Clef-Flash-Q4_K_M").unwrap().is_empty());
    }
}
