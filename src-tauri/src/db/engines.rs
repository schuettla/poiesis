//! External agent apps (`CLI-1`, `CLI-12`): one row per agent Poiesis has found
//! or been told about. What the user chose (`save_engine_settings`) and what a
//! probe found (`save_engine_probe`) are written by separate statements, so a
//! probe finishing never overwrites a switch the user just flipped. No secret
//! lives here: an API key is in the credential store.

use rusqlite::{params, OptionalExtension, Row};

use super::{now_ms, Db, DbError};

#[derive(Debug, Clone)]
pub struct EngineRow {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    /// The path the user chose. `None` = find it.
    pub program: Option<String>,
    /// What the last search resolved. `None` = not found.
    pub found_path: Option<String>,
    pub args_json: String,
    pub env_json: String,
    pub enabled: bool,
    pub version: Option<String>,
    pub signed_in: Option<bool>,
    pub paid_by: Option<String>,
    pub last_probe_at: Option<i64>,
    pub last_error: Option<String>,
    pub payment: String,
    pub trust_ceiling: String,
    pub headless: bool,
    pub memory_tools: bool,
    /// `CAP-14`: the agent may use the person's connected apps through Poiesis.
    pub app_tools: bool,
    pub model: Option<String>,
    pub extra_args: String,
    /// `RTE-1`: `on_request` | `suggest` | `auto`.
    pub use_mode: String,
    /// `RTE-3`: the guards as JSON. Parsed in `engines::rules`.
    pub guards_json: String,
    /// The default rules were written once. A user who deletes them all keeps
    /// an empty list instead of having them come back.
    pub rules_seeded: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "id, kind, display_name, program, found_path, args_json, env_json, enabled, \
     version, signed_in, paid_by, last_probe_at, last_error, payment, trust_ceiling, headless, \
     memory_tools, model, extra_args, use_mode, guards_json, rules_seeded, created_at, updated_at, app_tools";

/// One line of the user's rules for an agent app (`RTE-2`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EngineRule {
    pub id: String,
    pub engine_id: String,
    /// `use` | `avoid`.
    pub kind: String,
    pub text: String,
    pub enabled: bool,
    /// "Always for this": a handover that cites this rule starts without asking.
    pub auto: bool,
    pub position: i64,
    /// `default` | `user` | `proposed`.
    pub source: String,
    pub hits: i64,
    pub overrides: i64,
}

const RULE_COLUMNS: &str = "id, engine_id, kind, text, enabled, auto, position, source, hits, overrides";

fn map_rule(r: &Row) -> rusqlite::Result<EngineRule> {
    Ok(EngineRule {
        id: r.get(0)?,
        engine_id: r.get(1)?,
        kind: r.get(2)?,
        text: r.get(3)?,
        enabled: r.get::<_, i64>(4)? != 0,
        auto: r.get::<_, i64>(5)? != 0,
        position: r.get(6)?,
        source: r.get(7)?,
        hits: r.get(8)?,
        overrides: r.get(9)?,
    })
}

fn map_engine(r: &Row) -> rusqlite::Result<EngineRow> {
    Ok(EngineRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        display_name: r.get(2)?,
        program: r.get(3)?,
        found_path: r.get(4)?,
        args_json: r.get(5)?,
        env_json: r.get(6)?,
        enabled: r.get::<_, i64>(7)? != 0,
        version: r.get(8)?,
        signed_in: r.get::<_, Option<i64>>(9)?.map(|v| v != 0),
        paid_by: r.get(10)?,
        last_probe_at: r.get(11)?,
        last_error: r.get(12)?,
        payment: r.get(13)?,
        trust_ceiling: r.get(14)?,
        headless: r.get::<_, i64>(15)? != 0,
        memory_tools: r.get::<_, i64>(16)? != 0,
        model: r.get(17)?,
        extra_args: r.get(18)?,
        use_mode: r.get(19)?,
        guards_json: r.get(20)?,
        rules_seeded: r.get::<_, i64>(21)? != 0,
        created_at: r.get(22)?,
        updated_at: r.get(23)?,
        app_tools: r.get::<_, i64>(24)? != 0,
    })
}

impl EngineRow {
    /// A new row. Finding an agent is not consent to hand it work, so it starts off.
    pub fn new(id: &str, kind: &str, display_name: &str) -> Self {
        let now = now_ms();
        EngineRow {
            id: id.into(),
            kind: kind.into(),
            display_name: display_name.into(),
            program: None,
            found_path: None,
            args_json: "[]".into(),
            env_json: "{}".into(),
            enabled: false,
            version: None,
            signed_in: None,
            paid_by: None,
            last_probe_at: None,
            last_error: None,
            payment: "plan".into(),
            trust_ceiling: "confirm".into(),
            headless: false,
            memory_tools: false,
            app_tools: false,
            model: None,
            extra_args: "[]".into(),
            // An agent whose use nobody has described is only used on request
            // (`RTE-2`); `engines::rules` sets `suggest` for the two that ship
            // with default rules.
            use_mode: if kind == "acp" { "on_request".into() } else { "suggest".into() },
            guards_json: "{}".into(),
            rules_seeded: false,
            created_at: now,
            updated_at: now,
        }
    }
}

impl Db {
    pub fn list_engines(&self) -> Result<Vec<EngineRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM engines ORDER BY created_at, id"))?;
        let rows = stmt.query_map([], map_engine)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_engine(&self, id: &str) -> Result<Option<EngineRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(&format!("SELECT {COLUMNS} FROM engines WHERE id = ?1"), [id], map_engine)
            .optional()
            .map_err(Into::into)
    }

    /// Add the row unless one with that id exists. `true` = it was new.
    pub fn insert_engine_if_absent(&self, row: &EngineRow) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "INSERT OR IGNORE INTO engines(id, kind, display_name, program, found_path, args_json,
               env_json, enabled, payment, trust_ceiling, headless, memory_tools, model, extra_args,
               use_mode, guards_json, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                row.id,
                row.kind,
                row.display_name,
                row.program,
                row.found_path,
                row.args_json,
                row.env_json,
                row.enabled as i64,
                row.payment,
                row.trust_ceiling,
                row.headless as i64,
                row.memory_tools as i64,
                row.model,
                row.extra_args,
                row.use_mode,
                row.guards_json,
                row.created_at,
                row.updated_at,
            ],
        )?;
        Ok(n > 0)
    }

    /// Write what the user chose. Not the probe columns.
    pub fn save_engine_settings(&self, row: &EngineRow) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE engines SET display_name = ?2, program = ?3, args_json = ?4, env_json = ?5,
               enabled = ?6, payment = ?7, trust_ceiling = ?8, headless = ?9, memory_tools = ?10,
               model = ?11, extra_args = ?12, use_mode = ?13, guards_json = ?14, updated_at = ?15,
               app_tools = ?16
             WHERE id = ?1",
            params![
                row.id,
                row.display_name,
                row.program,
                row.args_json,
                row.env_json,
                row.enabled as i64,
                row.payment,
                row.trust_ceiling,
                row.headless as i64,
                row.memory_tools as i64,
                row.model,
                row.extra_args,
                row.use_mode,
                row.guards_json,
                now_ms(),
                row.app_tools as i64,
            ],
        )?;
        Ok(())
    }

    /// Write what a search and probe found. Not the user's choices.
    pub fn save_engine_probe(
        &self,
        id: &str,
        found_path: Option<&str>,
        version: Option<&str>,
        signed_in: Option<bool>,
        paid_by: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE engines SET found_path = ?2, version = ?3, signed_in = ?4, paid_by = ?5,
               last_error = ?6, last_probe_at = ?7
             WHERE id = ?1",
            params![id, found_path, version, signed_in.map(|b| b as i64), paid_by, error, now_ms()],
        )?;
        Ok(())
    }

    /// Only the path: a search that found nothing new does not need a probe.
    pub fn save_engine_found_path(&self, id: &str, found_path: Option<&str>) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE engines SET found_path = ?2 WHERE id = ?1", params![id, found_path])?;
        Ok(())
    }

    pub fn delete_engine(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM engine_rules WHERE engine_id = ?1", [id])?;
        conn.execute("DELETE FROM engine_signals WHERE engine_id = ?1", [id])?;
        conn.execute("DELETE FROM engines WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- the user's rules for when to hand work over (`RTE-2`) ----

    /// Every rule of one agent, in the order the user put them.
    pub fn list_engine_rules(&self, engine_id: &str) -> Result<Vec<EngineRule>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {RULE_COLUMNS} FROM engine_rules WHERE engine_id = ?1 ORDER BY position, created_at, id"
        ))?;
        let rows = stmt.query_map([engine_id], map_rule)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_engine_rule(&self, id: &str) -> Result<Option<EngineRule>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(&format!("SELECT {RULE_COLUMNS} FROM engine_rules WHERE id = ?1"), [id], map_rule)
            .optional()
            .map_err(Into::into)
    }

    /// Add a rule at the end of its list. `rule.id` and `rule.position` are set here.
    pub fn insert_engine_rule(&self, rule: &EngineRule) -> Result<EngineRule, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = super::new_id();
        let position: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM engine_rules WHERE engine_id = ?1",
                [&rule.engine_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let now = now_ms();
        conn.execute(
            "INSERT INTO engine_rules(id, engine_id, kind, text, enabled, auto, position, source,
               hits, overrides, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, ?9, ?9)",
            params![id, rule.engine_id, rule.kind, rule.text, rule.enabled as i64, rule.auto as i64, position, rule.source, now],
        )?;
        Ok(EngineRule { id, position, hits: 0, overrides: 0, ..rule.clone() })
    }

    /// Write what the user can edit on a rule: its words, kind, switches. Not the counts.
    pub fn save_engine_rule(&self, rule: &EngineRule) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE engine_rules SET kind = ?2, text = ?3, enabled = ?4, auto = ?5, source = ?6, updated_at = ?7
             WHERE id = ?1",
            params![rule.id, rule.kind, rule.text, rule.enabled as i64, rule.auto as i64, rule.source, now_ms()],
        )?;
        Ok(())
    }

    pub fn delete_engine_rule(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM engine_rules WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Put the rules in the given order. Ids that are not this agent's are ignored.
    pub fn reorder_engine_rules(&self, engine_id: &str, ids: &[String]) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        for (i, id) in ids.iter().enumerate() {
            conn.execute(
                "UPDATE engine_rules SET position = ?3 WHERE id = ?1 AND engine_id = ?2",
                params![id, engine_id, i as i64],
            )?;
        }
        Ok(())
    }

    pub fn clear_engine_rules(&self, engine_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM engine_rules WHERE engine_id = ?1", [engine_id])?;
        Ok(())
    }

    pub fn mark_engine_rules_seeded(&self, engine_id: &str, mode: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE engines SET rules_seeded = 1, use_mode = ?2 WHERE id = ?1",
            params![engine_id, mode],
        )?;
        Ok(())
    }

    /// A handover cited this rule (`hits`), or the user said no to one (`overrides`).
    pub fn bump_engine_rule(&self, id: &str, hit: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let sql = if hit {
            "UPDATE engine_rules SET hits = hits + 1 WHERE id = ?1"
        } else {
            "UPDATE engine_rules SET overrides = overrides + 1 WHERE id = ?1"
        };
        conn.execute(sql, [id])?;
        Ok(())
    }

    // ---- what teaches the rules (`RTE-6`) ----

    /// Note that something happened that the rules may need to learn from.
    pub fn add_engine_signal(&self, engine_id: &str, rule_id: Option<&str>, kind: &str, task: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO engine_signals(id, engine_id, rule_id, kind, task, at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![super::new_id(), engine_id, rule_id, kind, task, now_ms()],
        )?;
        Ok(())
    }

    /// A signal with a time of its own, for tests of the 30-day window.
    #[cfg(test)]
    pub fn add_engine_signal_at(&self, engine_id: &str, rule_id: Option<&str>, kind: &str, task: &str, at: i64) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO engine_signals(id, engine_id, rule_id, kind, task, at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![super::new_id(), engine_id, rule_id, kind, task, at],
        )
        .unwrap();
    }

    /// Signals of one kind after `since` (epoch ms), newest first, with the task
    /// each carried. `rule_id` narrows it to one rule; `None` takes the agent's.
    pub fn engine_signals_since(
        &self,
        engine_id: &str,
        rule_id: Option<&str>,
        kind: &str,
        since: i64,
    ) -> Result<Vec<(i64, String)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT at, task FROM engine_signals
             WHERE engine_id = ?1 AND kind = ?2 AND at > ?3 AND (?4 IS NULL OR rule_id = ?4)
             ORDER BY at DESC",
        )?;
        let rows = stmt
            .query_map(params![engine_id, kind, since, rule_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// When the last signal of this kind came in, if any.
    pub fn last_engine_signal_at(&self, engine_id: &str, kind: &str) -> Result<Option<i64>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT MAX(at) FROM engine_signals WHERE engine_id = ?1 AND kind = ?2",
            params![engine_id, kind],
            |r| r.get::<_, Option<i64>>(0),
        )
        .map_err(Into::into)
    }

    /// The newest proposal about this `slug` (a rule's id, or `use:<agent>`):
    /// its status and when it was raised.
    pub fn latest_engine_proposal(&self, slug: &str) -> Result<Option<(String, i64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT status, created_at FROM change_proposals
             WHERE target = 'engine-rule' AND slug = ?1 ORDER BY created_at DESC LIMIT 1",
            [slug],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_row_starts_off_and_a_second_insert_changes_nothing() {
        let db = Db::open_in_memory().unwrap();
        let row = EngineRow::new("claude-code", "claude-code", "Claude Code");
        assert!(db.insert_engine_if_absent(&row).unwrap());
        let mut again = row.clone();
        again.enabled = true;
        assert!(!db.insert_engine_if_absent(&again).unwrap(), "found twice is still one row");
        let got = db.get_engine("claude-code").unwrap().unwrap();
        assert!(!got.enabled, "finding an agent is not consent");
        assert_eq!(got.payment, "plan");
        assert_eq!(got.trust_ceiling, "confirm");
        assert!(!got.headless && !got.memory_tools);
    }

    #[test]
    fn a_probe_and_a_settings_save_do_not_overwrite_each_other() {
        let db = Db::open_in_memory().unwrap();
        db.insert_engine_if_absent(&EngineRow::new("codex", "codex", "Codex")).unwrap();
        let mut row = db.get_engine("codex").unwrap().unwrap();
        row.enabled = true;
        row.model = Some("gpt-x".into());
        row.app_tools = true;
        db.save_engine_settings(&row).unwrap();
        db.save_engine_probe("codex", Some(r"C:\a\codex.cmd"), Some("0.163.0"), Some(true), Some("plan"), None)
            .unwrap();
        let got = db.get_engine("codex").unwrap().unwrap();
        assert!(got.enabled && got.model.as_deref() == Some("gpt-x"), "the probe kept the user's choices");
        assert!(got.app_tools && !got.memory_tools, "`CAP-14`: the apps switch is saved on its own");
        assert_eq!(got.version.as_deref(), Some("0.163.0"));
        assert_eq!(got.signed_in, Some(true));
        assert!(got.last_probe_at.is_some());
        // And back: saving settings keeps the probe's facts.
        row.enabled = false;
        db.save_engine_settings(&row).unwrap();
        assert_eq!(db.get_engine("codex").unwrap().unwrap().version.as_deref(), Some("0.163.0"));
    }

    #[test]
    fn schema_v33_adds_mode_guards_rules_and_the_handover_columns() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        for (table, column) in [
            ("engines", "use_mode"),
            ("engines", "guards_json"),
            ("engines", "rules_seeded"),
            ("conversations", "engines_off"),
            ("subagent_runs", "engine_id"),
            ("subagent_runs", "because"),
            ("subagent_runs", "paid_by"),
            ("engine_rules", "auto"),
        ] {
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).unwrap();
            let names: Vec<String> =
                stmt.query_map([], |r| r.get::<_, String>(1)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
            assert!(names.contains(&column.to_string()), "{table}.{column}");
        }
    }

    fn rule(engine: &str, kind: &str, text: &str) -> EngineRule {
        EngineRule {
            id: String::new(),
            engine_id: engine.into(),
            kind: kind.into(),
            text: text.into(),
            enabled: true,
            auto: false,
            position: 0,
            source: "user".into(),
            hits: 0,
            overrides: 0,
        }
    }

    #[test]
    fn rules_keep_their_order_and_their_counts_and_go_with_their_agent() {
        let db = Db::open_in_memory().unwrap();
        db.insert_engine_if_absent(&EngineRow::new("claude-code", "claude-code", "Claude Code")).unwrap();
        let a = db.insert_engine_rule(&rule("claude-code", "use", "a")).unwrap();
        let b = db.insert_engine_rule(&rule("claude-code", "avoid", "b")).unwrap();
        let c = db.insert_engine_rule(&rule("claude-code", "use", "c")).unwrap();
        assert_eq!((a.position, b.position, c.position), (0, 1, 2));

        db.reorder_engine_rules("claude-code", &[c.id.clone(), a.id.clone(), b.id.clone()]).unwrap();
        let texts: Vec<String> = db.list_engine_rules("claude-code").unwrap().into_iter().map(|r| r.text).collect();
        assert_eq!(texts, ["c", "a", "b"]);

        db.bump_engine_rule(&a.id, true).unwrap();
        db.bump_engine_rule(&a.id, true).unwrap();
        db.bump_engine_rule(&a.id, false).unwrap();
        let mut got = db.get_engine_rule(&a.id).unwrap().unwrap();
        assert_eq!((got.hits, got.overrides), (2, 1));
        // Saving the words never touches the counts.
        got.text = "a2".into();
        got.auto = true;
        db.save_engine_rule(&got).unwrap();
        let again = db.get_engine_rule(&a.id).unwrap().unwrap();
        assert_eq!((again.text.as_str(), again.auto, again.hits, again.overrides), ("a2", true, 2, 1));

        db.delete_engine("claude-code").unwrap();
        assert!(db.list_engine_rules("claude-code").unwrap().is_empty(), "the rules go with the agent");
    }

    #[test]
    fn schema_v32_adds_the_persona_and_conversation_columns() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        for (table, column) in [("personas", "engine_id"), ("conversations", "engine_session")] {
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).unwrap();
            let names: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(names.contains(&column.to_string()), "{table}.{column}");
        }
    }
}
