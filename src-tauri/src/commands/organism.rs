//! The organism made visible (ORG-1) — one read-only snapshot of how Poiesis
//! is doing, plus the Health-tab actions that go with it.
//!
//! Deliberately counts and words, never gauges or green/red: this is a page
//! about a self, not a server dashboard.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::agent::log::{CommandTrace, COMMAND_KIND};
use crate::db::{Db, ToolStatRow};
use crate::memory::MemoryStore;
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

/// Everything the Self view's vitality strip and Health tab need, in one call.
#[derive(Debug, Serialize)]
pub struct Vitality {
    pub facts: usize,
    pub lessons: usize,
    /// Agent Skills currently discoverable and switched on (`SKL-5`).
    pub skills: usize,
    /// How many times a skill has been read this install — the same honesty
    /// the recipe use count carried, now sourced from the activity log.
    pub skill_uses: u32,
    pub quarantined: Vec<String>,
    /// Engine restarts the watchdog performed since launch (HEAL-1).
    pub engine_restarts_session: u32,
    pub pending_proposals: usize,
    /// Facts in dispute (`contested`): the Self header says so (`BLF-UI-1`).
    pub unsure: usize,
    pub last_reflection: Option<i64>,
    /// 7-day per-tool reliability for the model in play (HEAL-2).
    pub tool_health: Vec<ToolStatRow>,
}

#[tauri::command]
pub async fn get_vitality_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    mgr: State<'_, RuntimeManager>,
    model_name: Option<String>,
) -> Result<Vitality, PoiesisError> {
    // Reading the Health tab is also the moment to notice damaged files.
    mem.quarantine_scan(&db);

    let skills = crate::agent::skillpack::discover(mgr.app_data_dir(), None);
    let skills_on = skills.iter().filter(|p| crate::agent::skillpack::is_enabled(&db, p)).count();
    let model = match model_name {
        Some(m) if !m.trim().is_empty() => Some(m),
        _ => mgr.engine_model_name().await,
    };
    let tool_health = model
        .and_then(|m| db.tool_health(&m, 7).ok())
        .unwrap_or_default();

    Ok(Vitality {
        facts: mem.list().len(),
        lessons: mem.list_lessons().len(),
        skills: skills_on,
        skill_uses: db.count_activity("skill").unwrap_or(0),
        quarantined: mem.quarantined(),
        engine_restarts_session: mgr.restarts_session(),
        pending_proposals: db.pending_proposal_count().unwrap_or(0),
        unsure: mem.list().iter().filter(|f| f.status.as_deref() == Some("contested")).count(),
        last_reflection: db.last_reflection().ok().flatten(),
        tool_health,
    })
}

// ---- `MEV-1`: is it learning? (Cognition plan, Part V) ----

const DAY_MS: i64 = 86_400_000;
/// A "month" for the comparison: the window and the spacing of snapshots.
const MONTH_MS: i64 = 30 * DAY_MS;
/// Under this much history the Health tab says "No data yet".
const MIN_HISTORY_MS: i64 = 14 * DAY_MS;
const SNAPSHOTS_KEY: &str = "learning.snapshots";
const MAX_SNAPSHOTS: usize = 6;

/// The numbers that say whether the self-learning works. Counts and sizes
/// only, never text from a conversation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LearningNumbers {
    /// Fail-then-fix pairs for a tool and error class that an earlier
    /// conversation already had, while a lesson mentioning the tool exists.
    pub repeat_mistakes: usize,
    /// Things the user told me more than once: the same `/forget` subject, or
    /// the same suggestion turned down, again.
    pub told_twice: usize,
    /// Characters of memory carried into every message with no query
    /// (`recall_for(None)`).
    pub always_on_chars: usize,
    /// Of the facts and lessons that reached an answer, the share nobody pressed
    /// Outdated or Not relevant on. `None` under 10 recalled entries in the window.
    pub recall_precision: Option<f32>,
    /// Outdated presses per 100 recalled facts. `None` under 10 recalled facts.
    pub stale_per_100: Option<f32>,
}

/// The first line of an error up to its first `:` or `(`, lowercased: "no such
/// file or directory: /tmp/a" and "...: /tmp/b" are one class of mistake.
fn error_class(error: &str) -> String {
    let line = error.lines().next().unwrap_or("").to_lowercase();
    let cut = line.find([':', '(']).unwrap_or(line.len());
    line[..cut].trim().chars().take(40).collect()
}

/// Window `[from, to)` of the two table-backed numbers, plus the size now.
pub fn learning_numbers(db: &Db, mem: &MemoryStore, from: i64, to: i64) -> LearningNumbers {
    // Repeat mistakes. An earlier conversation having the same tool and class
    // is what makes this one a repeat; a lesson about the tool is what makes
    // it a mistake I was supposed to have learned from.
    let lessons: Vec<(String, String)> = mem
        .list_lessons()
        .iter()
        .map(|l| (l.created.clone(), format!("{} {} {}", l.name, l.description, l.body).to_lowercase()))
        .collect();
    let mut seen: HashMap<(String, String), HashSet<String>> = HashMap::new();
    let mut repeat_mistakes = 0;
    for row in db.tool_fix_log().unwrap_or_default() {
        let key = (row.tool_name.clone(), error_class(&row.error));
        let earlier = seen.entry(key).or_default();
        let repeated = earlier.iter().any(|c| *c != row.conversation_id);
        // A lesson written on the day of the repeat, or later, did not exist
        // to be learned from; `created` is a date, so "before" is by day.
        let day = crate::memory::civil_date(row.created_at / DAY_MS);
        let tool = row.tool_name.to_lowercase();
        let covered = lessons.iter().any(|(created, text)| created.as_str() < day.as_str() && text.contains(&tool));
        if row.created_at >= from && row.created_at < to && repeated && covered {
            repeat_mistakes += 1;
        }
        earlier.insert(row.conversation_id);
    }

    // Told twice: group the user's corrections by subject; every one after the
    // first in a group is a time I made them say it again.
    let mut subjects: HashMap<String, usize> = HashMap::new();
    for row in db
        .session_events_of_kind_between(COMMAND_KIND, from, to)
        .unwrap_or_default()
    {
        let Ok(t) = serde_json::from_str::<CommandTrace>(&row.payload_json) else { continue };
        let key = if t.name == "forget" && t.by == "user" {
            format!("forget:{}", t.note.as_deref().unwrap_or(&t.args).trim().to_lowercase())
        } else if t.name == "suggest" && t.outcome == "declined" {
            format!("suggest:{}", t.args.trim().to_lowercase())
        } else {
            continue;
        };
        *subjects.entry(key).or_default() += 1;
    }
    let told_twice = subjects.values().map(|n| n.saturating_sub(1)).sum();

    let (recall_precision, stale_per_100) = recall_quality(db, from, to);
    LearningNumbers {
        repeat_mistakes,
        told_twice,
        always_on_chars: mem.recall_for(db, None).index.chars().count(),
        recall_precision,
        stale_per_100,
    }
}

/// Recalled entries needed in a window before the two recall numbers mean anything.
const MIN_RECALLED: usize = 10;

/// `(recall precision, stale per 100)` for a window. What reached each answer is
/// in `context_json` (`CRD-1`); what the person said about it is the Outdated
/// and Not relevant traces (`CUE-UI-1`). Counts only: no entry's text is read.
fn recall_quality(db: &Db, from: i64, to: i64) -> (Option<f32>, Option<f32>) {
    let (mut facts, mut lessons) = (0usize, 0usize);
    for raw in db.context_records_between(from, to).unwrap_or_default() {
        if let Ok(refs) = serde_json::from_str::<crate::commands::memory::ContextRefs>(&raw) {
            facts += refs.facts.len();
            lessons += refs.lessons.len();
        }
    }
    let (mut outdated, mut not_relevant) = (0usize, 0usize);
    for row in db.session_events_of_kind_between(COMMAND_KIND, from, to).unwrap_or_default() {
        let Ok(t) = serde_json::from_str::<CommandTrace>(&row.payload_json) else { continue };
        if t.by != "user" {
            continue;
        }
        if t.name == crate::agent::credit::OUTDATED {
            outdated += 1;
        } else if t.name == crate::agent::credit::NOT_RELEVANT {
            not_relevant += 1;
        }
    }
    let recalled = facts + lessons;
    let precision = (recalled >= MIN_RECALLED)
        .then(|| 1.0 - ((outdated + not_relevant).min(recalled) as f32 / recalled as f32));
    let stale = (facts >= MIN_RECALLED).then(|| 100.0 * outdated as f32 / facts as f32);
    (precision, stale)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Snapshot {
    taken_at: i64,
    numbers: LearningNumbers,
}

/// What the Health tab shows. `previous` is the snapshot from about a month
/// ago; `enough_history` is false until two weeks of measuring have passed.
#[derive(Debug, Serialize)]
pub struct LearningReport {
    pub current: LearningNumbers,
    pub previous: Option<LearningNumbers>,
    pub enough_history: bool,
}

fn load_snapshots(db: &Db) -> Vec<Snapshot> {
    db.get_setting(SNAPSHOTS_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Measure now, and keep one snapshot a month in `settings` so next month has
/// something to compare to. Derived data only: a deleted snapshot costs the
/// comparison, nothing the user wrote.
pub fn learning_report(db: &Db, mem: &MemoryStore, now: i64) -> LearningReport {
    let current = learning_numbers(db, mem, now - MONTH_MS, now + 1);
    let mut snaps = load_snapshots(db);
    let previous = snaps
        .iter()
        .rev()
        .find(|s| s.taken_at <= now - 25 * DAY_MS)
        .map(|s| s.numbers.clone());
    let enough_history = snaps.first().is_some_and(|s| s.taken_at <= now - MIN_HISTORY_MS);
    if snaps.last().is_none_or(|s| s.taken_at <= now - MONTH_MS) {
        snaps.push(Snapshot { taken_at: now, numbers: current.clone() });
        let drop = snaps.len().saturating_sub(MAX_SNAPSHOTS);
        snaps.drain(..drop);
        if let Ok(raw) = serde_json::to_string(&snaps) {
            let _ = db.set_setting(SNAPSHOTS_KEY, &raw);
        }
    }
    LearningReport { current, previous, enough_history }
}

#[tauri::command]
pub fn get_learning_report_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>) -> LearningReport {
    learning_report(&db, &mem, crate::db::now_ms())
}

/// The Health tab's Golden section (`GLD-UI-1`): the last recorded run,
/// without running a fresh one — matches how the rest of the tab is read
/// passively on open.
#[tauri::command]
pub fn get_golden_status_cmd(db: State<'_, Db>) -> Option<crate::agent::golden::GoldenStatus> {
    crate::agent::golden::load_status(&db)
}

/// "Check me now" (`GLD-UI-1`): always runs a fresh pass over the golden set,
/// independent of `golden.enabled` — that setting only gates the automatic
/// guard around a self-change, never the user's own button.
#[tauri::command]
pub async fn check_golden_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    mgr: State<'_, RuntimeManager>,
    target: Option<crate::commands::agent::ChatTarget>,
) -> Result<crate::agent::golden::GoldenStatus, PoiesisError> {
    crate::agent::golden::check_now(&mgr.client, &mgr, &db, &mem, target.as_ref())
        .await
        .map_err(PoiesisError::Message)
}

/// `CHK-1`: "is anything wrong with me", in my own voice. Every area is probed
/// on its own with a four-second clock, all at once; the behaviour check is a
/// real model call, so it gets longer and the card says so while it runs.
#[tauri::command]
pub async fn checkup_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    mgr: State<'_, RuntimeManager>,
    target: Option<crate::commands::agent::ChatTarget>,
) -> Result<crate::agent::checkup::Checkup, PoiesisError> {
    use crate::agent::checkup::{self, within};

    let using_remote = matches!(
        target.as_ref().and_then(|t| t.provenance.as_deref()),
        Some("cloud") | Some("endpoint")
    );
    let engine = within(mgr.status())
        .await
        .unwrap_or_else(|_| crate::runtime::process::EngineStatus {
            running: false,
            port: None,
            model_path: None,
            ctx_size: None,
            structured_tool_output: false,
            restarts_session: 0,
            self_heal_gave_up: false,
        });

    let enabled: Vec<_> = db
        .list_connectors()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.enabled)
        .collect();
    let probes = enabled.iter().map(|c| async {
        let outcome = match within(crate::commands::connectors::probe_connector(&db, &mgr.client, &c.id)).await {
            Err(timed_out) => Err(timed_out),
            Ok(Err(e)) => Err(e.to_string()),
            Ok(Ok(status)) if status.ok => Ok(()),
            Ok(Ok(status)) => Err(status.error.unwrap_or_else(|| "It didn't answer.".into())),
        };
        (c.name.clone(), outcome)
    });
    let golden = async {
        // A model call, so not the four-second clock.
        match tokio::time::timeout(
            std::time::Duration::from_secs(90),
            crate::agent::golden::check_now(&mgr.client, &mgr, &db, &mem, target.as_ref()),
        )
        .await
        {
            Err(_) => Err("It took too long.".to_string()),
            Ok(Err(e)) => Err(e),
            Ok(Ok(s)) => Ok((s.passed, s.total, s.failing)),
        }
    };
    let (connectors, behaviour) = tokio::join!(futures_util::future::join_all(probes), golden);
    Ok(checkup::run(&db, engine, using_remote, connectors, behaviour).await)
}

/// 7-day per-tool reliability for one model (HEAL-2). Separate from `Vitality`
/// because the frontend refreshes it on every model change and after every turn
/// that used a tool — this touches one indexed table and nothing on disk.
///
/// `model_name` is omitted for local models, whose name only the manager knows.
#[tauri::command]
pub async fn get_tool_health_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    model_name: Option<String>,
) -> Result<Vec<ToolStatRow>, PoiesisError> {
    let model = match model_name {
        Some(m) if !m.trim().is_empty() => Some(m),
        _ => mgr.engine_model_name().await,
    };
    Ok(model
        .and_then(|m| db.tool_health(&m, 7).ok())
        .unwrap_or_default())
}

/// Put a quarantined file back (HEAL-3) — the user has presumably repaired it.
#[tauri::command]
pub fn restore_quarantined_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    file: String,
) -> Result<(), PoiesisError> {
    mem.restore_quarantined(&db, &file).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "heal", &format!("restored {file}"));
    Ok(())
}

#[tauri::command]
pub fn delete_quarantined_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    file: String,
) -> Result<(), PoiesisError> {
    mem.delete_quarantined(&file).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "heal", &format!("discarded {file}"));
    Ok(())
}

/// Seed a conversation's workspace surface from a skill's bundled
/// `assets/surface.json` (`SKL-5`, carrying `RCP-UI-2` forward). Writes the
/// reserved surface row exactly the way `render_ui` does, so the template
/// renders through the ordinary surface path with nothing special about
/// having come from a skill.
#[tauri::command]
pub fn set_surface_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    tree_json: String,
) -> Result<String, PoiesisError> {
    serde_json::from_str::<serde_json::Value>(&tree_json)
        .map_err(|e| PoiesisError::Message(format!("that surface template isn't valid JSON: {e}")))?;
    crate::agent::present::write_surface(&db, &conversation_id, &tree_json)
        .map_err(PoiesisError::Message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::log::record_command;
    use crate::memory::Fact;

    fn store() -> (MemoryStore, Db, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();
        (store, Db::open_in_memory().unwrap(), tmp)
    }

    fn lesson(name: &str, body: &str) -> Fact {
        Fact {
            name: name.into(),
            description: body.into(),
            kind: "lesson".into(),
            created: "2000-01-01".into(),
            source_conversation: None,
            body: body.into(),
            scope: None,
            recurrence: None,
            last_seen: None,
            expires_at: None,
            origin: None,
            cue: None,
            confidence: None,
            status: None,
            supersedes: None,
            superseded_by: None,
            evidence: Vec::new(),
            valid_from: None,
            project: None,
        }
    }

    fn trace(name: &str, args: &str, by: &str, outcome: &str, note: Option<&str>) -> CommandTrace {
        CommandTrace {
            name: name.into(),
            args: args.into(),
            by: by.into(),
            outcome: outcome.into(),
            note: note.map(str::to_string),
            message_id: None,
        }
    }

    const ALL: (i64, i64) = (0, i64::MAX);

    fn conv(db: &Db) -> String {
        db.create_conversation("A chat", None, false).unwrap().id
    }

    #[test]
    fn a_repeat_mistake_needs_an_earlier_conversation_and_a_lesson() {
        let count = |db: &Db, mem: &MemoryStore| learning_numbers(db, mem, ALL.0, ALL.1).repeat_mistakes;

        // No lesson about the tool: the same failure again is not a repeated
        // mistake, because nothing was learned that it could repeat.
        let (mem, db, _tmp) = store();
        let (a, b, c) = (conv(&db), conv(&db), conv(&db));
        db.add_tool_fix(&a, "read_file", "{}", "No such file or directory: /x", "{}");
        db.add_tool_fix(&b, "read_file", "{}", "No such file or directory: /y", "{}");
        db.add_tool_fix(&c, "read_file", "{}", "No such file or directory: /z", "{}");
        assert_eq!(count(&db, &mem), 0);

        // With a lesson about the tool from an earlier day, each later
        // conversation with the same tool and class counts, the first does not.
        let (mem, db, _tmp) = store();
        let (a, b, c, d) = (conv(&db), conv(&db), conv(&db), conv(&db));
        mem.save_lesson(&db, &lesson("check-path", "Check the path exists before read_file.")).unwrap();
        db.add_tool_fix(&a, "read_file", "{}", "No such file or directory: /x", "{}");
        assert_eq!(count(&db, &mem), 0, "first time in a conversation: nothing to repeat yet");
        db.add_tool_fix(&b, "read_file", "{}", "No such file or directory: /y", "{}");
        assert_eq!(count(&db, &mem), 1);
        db.add_tool_fix(&c, "read_file", "{}", "No such file or directory: /z", "{}");
        assert_eq!(count(&db, &mem), 2);

        // Another error class for the same tool is a different mistake.
        db.add_tool_fix(&d, "read_file", "{}", "permission denied", "{}");
        assert_eq!(count(&db, &mem), 2);

        // The same conversation failing twice is not a repeat across conversations.
        db.add_tool_fix(&d, "read_file", "{}", "permission denied", "{}");
        assert_eq!(count(&db, &mem), 2);
    }

    #[test]
    fn a_lesson_written_today_does_not_cover_a_repeat_today() {
        let (mem, db, _tmp) = store();
        let (a, b) = (conv(&db), conv(&db));
        let mut fresh = lesson("check-path", "Check the path before read_file.");
        fresh.created = String::new(); // saved as today
        mem.save_lesson(&db, &fresh).unwrap();
        db.add_tool_fix(&a, "read_file", "{}", "boom", "{}");
        db.add_tool_fix(&b, "read_file", "{}", "boom", "{}");
        assert_eq!(learning_numbers(&db, &mem, ALL.0, ALL.1).repeat_mistakes, 0);
    }

    #[test]
    fn the_window_decides_which_repeats_count() {
        let (mem, db, _tmp) = store();
        let (a, b) = (conv(&db), conv(&db));
        mem.save_lesson(&db, &lesson("check-path", "Check the path before read_file.")).unwrap();
        db.add_tool_fix(&a, "read_file", "{}", "boom", "{}");
        db.add_tool_fix(&b, "read_file", "{}", "boom", "{}");
        let now = crate::db::now_ms();
        assert_eq!(learning_numbers(&db, &mem, now - DAY_MS, now + DAY_MS).repeat_mistakes, 1);
        assert_eq!(learning_numbers(&db, &mem, now - 3 * DAY_MS, now - 2 * DAY_MS).repeat_mistakes, 0);
    }

    #[test]
    fn told_twice_counts_the_second_time_not_the_first() {
        let (mem, db, _tmp) = store();
        let (c1, c2, c3, c4) = (conv(&db), conv(&db), conv(&db), conv(&db));
        let told = |db: &Db, mem: &MemoryStore| learning_numbers(db, mem, ALL.0, ALL.1).told_twice;

        record_command(&db, &c1, None, &trace("forget", "tea", "user", "done", Some("forget that I like tea")));
        assert_eq!(told(&db, &mem), 0);

        record_command(&db, &c2, None, &trace("forget", "tea", "user", "done", Some("Forget that I like tea")));
        assert_eq!(told(&db, &mem), 1);

        // A turned-down suggestion, twice.
        record_command(&db, &c1, None, &trace("suggest", "tidy", "agent", "declined", None));
        record_command(&db, &c3, None, &trace("suggest", "tidy", "agent", "declined", None));
        assert_eq!(told(&db, &mem), 2);

        // The agent forgetting something by itself is not the user telling me.
        record_command(&db, &c4, None, &trace("forget", "tea", "agent", "done", None));
        assert_eq!(told(&db, &mem), 2);
    }

    #[test]
    fn always_on_size_follows_what_every_message_carries() {
        let (mem, db, _tmp) = store();
        assert_eq!(learning_numbers(&db, &mem, ALL.0, ALL.1).always_on_chars, 0);
        let mut f = lesson("prefers-metric", "Always use metric units.");
        f.kind = "preference".into();
        mem.save(&db, &f).unwrap();
        assert!(learning_numbers(&db, &mem, ALL.0, ALL.1).always_on_chars > 20);
    }

    /// An answer that carried `facts` and `lessons` entries, and the user's presses.
    fn answer_with(db: &Db, conv: &str, facts: usize, lessons: usize) {
        let msg = db
            .append_message(
                conv,
                &crate::db::NewMessage {
                    role: "assistant".into(),
                    content: "ok".into(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        let refs = crate::commands::memory::ContextRefs {
            facts: (0..facts).map(|i| format!("f{i}")).collect(),
            lessons: (0..lessons).map(|i| format!("l{i}")).collect(),
            ..Default::default()
        };
        db.finalize_message(&msg.id, "ok", None, Some(&serde_json::to_string(&refs).unwrap()), None, None).unwrap();
    }

    fn press(db: &Db, conv: &str, name: &str) {
        record_command(
            db,
            conv,
            None,
            &CommandTrace {
                name: name.into(),
                args: "f0".into(),
                by: "user".into(),
                outcome: "done".into(),
                note: None,
                message_id: None,
            },
        );
    }

    #[test]
    fn the_recall_numbers_stay_empty_until_enough_has_been_recalled() {
        let (mem, db, _tmp) = store();
        let n = learning_numbers(&db, &mem, ALL.0, ALL.1);
        assert_eq!(n.recall_precision, None);
        assert_eq!(n.stale_per_100, None);
        let c = conv(&db);
        answer_with(&db, &c, 3, 2);
        let n = learning_numbers(&db, &mem, ALL.0, ALL.1);
        assert_eq!((n.recall_precision, n.stale_per_100), (None, None), "five entries are too few to say");
    }

    #[test]
    fn precision_and_stale_rate_come_from_what_reached_answers_and_what_was_pressed() {
        let (mem, db, _tmp) = store();
        let c = conv(&db);
        answer_with(&db, &c, 8, 2);
        answer_with(&db, &c, 8, 2);
        // 20 recalled entries, 16 of them facts.
        assert_eq!(learning_numbers(&db, &mem, ALL.0, ALL.1).recall_precision, Some(1.0));
        press(&db, &c, crate::agent::credit::OUTDATED);
        press(&db, &c, crate::agent::credit::OUTDATED);
        press(&db, &c, crate::agent::credit::NOT_RELEVANT);
        let n = learning_numbers(&db, &mem, ALL.0, ALL.1);
        assert!((n.recall_precision.unwrap() - 0.85).abs() < 1e-6, "{:?}", n.recall_precision);
        assert!((n.stale_per_100.unwrap() - 12.5).abs() < 1e-4, "2 of 16 facts: {:?}", n.stale_per_100);
    }

    #[test]
    fn the_report_snapshots_monthly_and_compares_to_last_month() {
        let (mem, db, _tmp) = store();
        let t0 = 1_000 * DAY_MS;

        // First look: a snapshot is taken, there is nothing to compare to, and
        // two weeks have not passed.
        let first = learning_report(&db, &mem, t0);
        assert!(first.previous.is_none());
        assert!(!first.enough_history);

        // Twenty days later: enough history, still no month-old snapshot.
        let mid = learning_report(&db, &mem, t0 + 20 * DAY_MS);
        assert!(mid.enough_history);
        assert!(mid.previous.is_none());
        assert_eq!(load_snapshots(&db).len(), 1, "no second snapshot inside a month");

        // Thirty-one days later: the first snapshot is last month's, and a new one is kept.
        let later = learning_report(&db, &mem, t0 + 31 * DAY_MS);
        assert!(later.previous.is_some());
        assert_eq!(load_snapshots(&db).len(), 2);
    }
}
