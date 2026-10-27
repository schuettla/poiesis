//! `EXP-4`: what an experiment did in the last 30 days, said from the data it keeps
//! on this computer, and the reset that forgets that data.
//!
//! Nothing here is measured again and nothing leaves the machine: a report is a
//! count over the decision log, its labels and the tables an experiment owns. A
//! number that the data cannot give is left out, not guessed (the plan's "0.6 s
//! faster to first token" is one: no turn records what it would have cost without
//! the experiment).

use serde::Serialize;

use crate::agent::decide::policy::{Purpose, ALL};
use crate::agent::decide::toolgate::SKIPPED;
use crate::agent::mail_sync;
use crate::agent::verify;
use crate::db::{now_ms, Db};

/// The window a report covers.
pub const DAYS: i64 = 30;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// One card's "Last 30 days".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub days: i64,
    /// Short facts, joined with a dot on the card. Empty when nothing happened.
    pub lines: Vec<String>,
    /// What "Reset its data" would remove, over all time. Zero hides the button.
    pub records: i64,
}

/// The purposes whose decisions belong to an experiment, as the Activity filter
/// reads them.
fn purposes_of(id: &str) -> Vec<&'static str> {
    ALL.iter().filter(|p| p.policy().experiment == Some(id)).map(|p| p.name()).collect()
}

pub fn report(db: &Db, id: &str) -> Report {
    report_at(db, id, now_ms())
}

fn report_at(db: &Db, id: &str, now: i64) -> Report {
    let since = now - DAYS * DAY_MS;
    let lines = match id {
        "fast-slow" => fast_slow(db, since),
        "mail-triage" => mail(db, since),
        "relax-prompts" => relaxed(db, since),
        "workspace" => workspace(db, since),
        _ => Vec::new(),
    };
    Report { days: DAYS, lines, records: records(db, id) }
}

/// `1820` as `1,820`.
fn n(count: i64) -> String {
    let digits = count.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if count < 0 {
        out.insert(0, '-');
    }
    out
}

fn plural(count: i64, one: &str, many: &str) -> String {
    format!("{} {}", n(count), if count == 1 { one } else { many })
}

/// Whole percent, rounded; `part` of `whole`.
fn pct(part: i64, whole: i64) -> i64 {
    if whole <= 0 {
        0
    } else {
        (part as f64 * 100.0 / whole as f64).round() as i64
    }
}

fn dollars(usd: f64) -> String {
    if usd >= 0.01 {
        format!("${usd:.2}")
    } else if usd > 0.0 {
        "under $0.01".into()
    } else {
        "free".into()
    }
}

fn fast_slow(db: &Db, since: i64) -> Vec<String> {
    let mine = purposes_of("fast-slow");
    let stats: Vec<_> =
        db.decision_stats_since(since).unwrap_or_default().into_iter().filter(|s| mine.contains(&s.purpose.as_str())).collect();
    let total: i64 = stats.iter().map(|s| s.answered).sum();
    if total == 0 {
        return checked(db, since);
    }
    let mut lines = Vec::new();
    let menu = Purpose::ToolSelect.name();
    if let Some(t) = stats.iter().find(|s| s.purpose == menu) {
        lines.push(format!("Held back tools on {} of {}", n(t.acted), plural(t.answered, "turn", "turns")));
        // A miss is a held group that came back, so it is counted against the turns
        // that held something, the gate the card names.
        if t.acted > 0 {
            lines.push(format!("brought back on {} of them ({}%)", n(t.missed), pct(t.missed, t.acted)));
        }
    }
    let cloud: i64 = stats.iter().map(|s| s.cloud).sum();
    let cost: f64 = stats.iter().map(|s| s.cost_usd).sum();
    lines.push(if cloud == 0 {
        format!("{} all on this computer", plural(total, "quick decision", "quick decisions"))
    } else {
        format!(
            "{}, {} in the cloud ({})",
            plural(total, "quick decision", "quick decisions"),
            n(cloud),
            dollars(cost)
        )
    });
    // The menu's own misses are the line above. The rest are corrections, which only
    // some purposes ever get (a reopened plan step, a corrected row, a "no" to a card).
    let missed: i64 = stats.iter().filter(|s| s.purpose != menu).map(|s| s.missed).sum();
    lines.push(format!("{} corrected or marked wrong", n(missed)));
    lines.extend(checked(db, since));
    lines
}

/// `SEE-5`: the answers that were checked, which `fast-slow` owns now. A check is
/// stored whether the check on the answer or the deep one made it.
fn checked(db: &Db, since: i64) -> Vec<String> {
    let checks = db.verifications_since(since).unwrap_or_default();
    if checks.is_empty() {
        return Vec::new();
    }
    let claims: Vec<_> = checks.iter().flat_map(|c| verify::claims_from_json(&c.claims_json)).collect();
    let contradicted = claims.iter().filter(|c| c.finding == "refuted").count() as i64;
    let rated = claims.iter().filter(|c| c.mark.is_some()).count() as i64;
    let wrong = claims.iter().filter(|c| c.mark.as_deref() == Some("wrong")).count() as i64;
    let mut lines = vec![format!(
        "checked {} \u{b7} {} contradicted",
        plural(checks.len() as i64, "answer", "answers"),
        n(contradicted)
    )];
    if rated > 0 {
        lines.push(format!("you rated {} verdicts, {} wrong", n(rated), n(wrong)));
    }
    lines
}

fn mail(db: &Db, since: i64) -> Vec<String> {
    let name = Purpose::MailTriage.name();
    let Some(s) = db.decision_stats_since(since).unwrap_or_default().into_iter().find(|s| s.purpose == name) else {
        return Vec::new();
    };
    vec![
        format!("Triaged {}", plural(s.answered, "mail", "mails")),
        format!("you retagged {} ({}%)", n(s.missed), pct(s.missed, s.answered)),
    ]
}

fn relaxed(db: &Db, since: i64) -> Vec<String> {
    match db.count_activity_since("decide", SKIPPED, since).unwrap_or(0) {
        0 => Vec::new(),
        count => vec![format!("Skipped {}", plural(count, "question", "questions"))],
    }
}

fn workspace(db: &Db, since: i64) -> Vec<String> {
    match db.workspace_conversations_since(since).unwrap_or(0) {
        0 => Vec::new(),
        count => vec![format!("{} used", plural(count, "Workspace chat", "Workspace chats"))],
    }
}

/// What a reset would remove, over all time.
fn records(db: &Db, id: &str) -> i64 {
    let decisions = db.count_decisions_of(&purposes_of(id)).unwrap_or(0);
    decisions
        + match id {
            "fast-slow" => db.count_verifications().unwrap_or(0),
            "mail-triage" => db.count_mail_seen().unwrap_or(0),
            "relax-prompts" => db.count_activity_since("decide", SKIPPED, 0).unwrap_or(0),
            _ => 0,
        }
}

/// What a mail reset says while the inbox is being checked.
pub const BUSY: &str = "Your inbox is being checked right now. Try again in a minute.";

/// "Reset its data": forget what the experiment recorded, and nothing else. The
/// switch stays where it is. What a fit learned (the calibration of a purpose) is
/// kept: it is about the model, not about what this person did, and throwing it
/// away would send every quick decision back to its careful setting.
///
/// An unknown id is an error; an experiment with nothing recorded is not.
pub fn reset(db: &Db, id: &str) -> Result<usize, String> {
    if crate::experiments::find(id).is_none() {
        return Err(format!("There is no experiment \"{id}\"."));
    }
    let err = |e: crate::db::DbError| e.to_string();
    // A look at the inbox that runs during a mail reset would write back what the
    // reset removes, so none may run until it is done.
    let _hold = match id {
        "mail-triage" => Some(mail_sync::hold().ok_or(BUSY)?),
        _ => None,
    };
    let mut removed = db.delete_decisions_of(&purposes_of(id)).map_err(err)?;
    match id {
        "fast-slow" => removed += db.delete_verifications().map_err(err)?,
        "mail-triage" => {
            removed += db.clear_all_mail_seen().map_err(err)?;
            // The loop remembers the newest message it saw. With the rows gone that
            // would leave an inbox that stays empty until something new arrives.
            let fresh = serde_json::to_string(&mail_sync::SyncState::default()).map_err(|e| e.to_string())?;
            for account in db.list_mail_accounts().map_err(err)? {
                db.set_setting(&mail_sync::state_key(&account.id), &fresh).map_err(err)?;
            }
        }
        "relax-prompts" => removed += db.delete_activity_starting("decide", SKIPPED).map_err(err)?,
        _ => {}
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 100 * DAY_MS;

    fn decide(db: &Db, id: &str, purpose: Purpose, backend: &str, action: &str, at: i64) {
        let answers = serde_json::json!({ "x": { "p": 0.5 } });
        db.add_decision(&crate::db::decisions::NewDecision {
            id,
            conversation_id: None,
            run_id: None,
            purpose: purpose.name(),
            backend,
            model: "Clef-Flash-Q4_K_M",
            ms: 40,
            calibrated: false,
            answers: &answers,
            input_tokens: 10,
            state_preview: "",
            state_hash: id,
            action: Some(action),
        })
        .unwrap();
        // The log stamps now; move it to where the test needs it.
        db.set_decision_at(id, at);
    }

    const LOCAL: &str = "local:Clef-flash";

    #[test]
    fn nothing_recorded_says_nothing() {
        let db = Db::open_in_memory().unwrap();
        for e in crate::experiments::REGISTRY {
            let r = report_at(&db, e.id, NOW);
            assert!(r.lines.is_empty(), "{}", e.id);
            assert_eq!(r.records, 0, "{}", e.id);
        }
    }

    #[test]
    fn numbers_read_with_thousands_and_percentages_round() {
        assert_eq!((n(0), n(999), n(1820), n(1_234_567)), ("0".into(), "999".into(), "1,820".into(), "1,234,567".into()));
        assert_eq!((pct(9, 214), pct(1, 3), pct(0, 0)), (4, 33, 0));
        assert_eq!(plural(1, "turn", "turns"), "1 turn");
        assert_eq!((dollars(0.000_063), dollars(1.234), dollars(0.0)), ("under $0.01".into(), "$1.23".into(), "free".into()));
    }

    /// The plan's example: tools held back on some turns, a few brought back, and
    /// the rest of the quick decisions counted. Only the last 30 days count.
    #[test]
    fn the_fast_slow_report_counts_held_turns_and_what_came_back() {
        let db = Db::open_in_memory().unwrap();
        for i in 0..4 {
            let held = i < 3;
            decide(&db, &format!("t{i}"), Purpose::ToolSelect, LOCAL, if held { "acted" } else { "default" }, NOW - DAY_MS);
        }
        db.add_outcome("t0", "wrong", Some("web"), "need-tools").unwrap();
        db.add_outcome("t1", "weak_right", None, "kept-out").unwrap();
        decide(&db, "r1", Purpose::RunIf, LOCAL, "default", NOW - 2 * DAY_MS);
        db.add_outcome("r1", "wrong", None, "run-if-undone").unwrap();
        // A purpose of another experiment and a decision too old to count.
        decide(&db, "m1", Purpose::MailTriage, LOCAL, "default", NOW - DAY_MS);
        decide(&db, "old", Purpose::ToolSelect, LOCAL, "acted", NOW - 31 * DAY_MS);

        let r = report_at(&db, "fast-slow", NOW);
        assert_eq!(
            r.lines,
            vec![
                "Held back tools on 3 of 4 turns",
                "brought back on 1 of them (33%)",
                "5 quick decisions all on this computer",
                "1 corrected or marked wrong",
            ]
        );
        // A reset removes the old one too: it is data of the experiment.
        assert_eq!(r.records, 6);
    }

    #[test]
    fn a_cloud_decision_is_counted_with_what_it_cost() {
        let db = Db::open_in_memory().unwrap();
        decide(&db, "a", Purpose::RunIf, LOCAL, "default", NOW);
        decide(&db, "b", Purpose::RunIf, "openrouter:cloudflare/clef-flash@DeepInfra", "default", NOW);
        db.set_decision_cost("b", 0.000_042).unwrap();
        let lines = report_at(&db, "fast-slow", NOW).lines;
        assert!(lines.contains(&"2 quick decisions, 1 in the cloud (under $0.01)".to_string()), "{lines:?}");
    }

    #[test]
    fn a_decision_a_budget_skipped_is_not_counted() {
        let db = Db::open_in_memory().unwrap();
        let none = serde_json::json!({});
        db.add_decision(&crate::db::decisions::NewDecision {
            id: "skipped",
            conversation_id: None,
            run_id: None,
            purpose: Purpose::ToolSelect.name(),
            backend: LOCAL,
            model: "m",
            ms: 0,
            calibrated: false,
            answers: &none,
            input_tokens: 0,
            state_preview: "",
            state_hash: "h",
            action: Some("budget: decisions"),
        })
        .unwrap();
        let r = report_at(&db, "fast-slow", now_ms());
        assert!(r.lines.is_empty());
        assert_eq!(r.records, 1, "it is still a row a reset removes");
    }

    #[test]
    fn the_mail_report_counts_a_retag_against_the_mails_triaged() {
        let db = Db::open_in_memory().unwrap();
        for i in 0..4 {
            decide(&db, &format!("m{i}"), Purpose::MailTriage, LOCAL, "default", NOW - DAY_MS);
        }
        db.add_outcome("m0", "corrected", Some("receipt"), "mail-retag").unwrap();
        db.add_outcome("m1", "right", None, "mail-retag").unwrap();
        let r = report_at(&db, "mail-triage", NOW);
        assert_eq!(r.lines, vec!["Triaged 4 mails", "you retagged 1 (25%)"], "a retag to the same kind is a confirmation");
        assert!(report_at(&db, "fast-slow", NOW).lines.is_empty(), "another experiment's purpose");
    }

    /// `SEE-5`: the checks are `fast-slow`'s data now, with or without a decision
    /// beside them, and a reset forgets them with the rest.
    #[test]
    fn the_fast_slow_report_counts_answers_checked_and_contradicted_and_a_reset_forgets_them() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("t", None, false).unwrap().id;
        let claim = |finding: &str, mark: Option<&str>| {
            serde_json::json!({
                "id": "c", "text": "t", "quote": "", "source": null, "importance": 2, "kind": "stable",
                "finding": finding, "p": null, "how": "fast", "settled": true, "fast": null, "slow": null,
                "reason": null, "passages": [], "decision": null, "mark": mark
            })
        };
        let claims = serde_json::json!([
            claim("supported", Some("right")),
            claim("refuted", Some("wrong")),
            claim("refuted", None),
            claim("insufficient", None),
        ]);
        let msg = |text: &str| {
            let note = crate::db::NewMessage {
                role: "assistant".into(),
                content: text.into(),
                model_name: None,
                model_provenance: None,
                steps_json: None,
                attachments: vec![],
            };
            db.append_message(&conv, &note).unwrap().id
        };
        for (id, at) in [(msg("one"), NOW - DAY_MS), (msg("two"), NOW - 40 * DAY_MS)] {
            db.save_verification(&crate::db::verifications::StoredCheck {
                message_id: id,
                conversation_id: conv.clone(),
                claims_json: claims.to_string(),
                note: None,
                skipped: 0,
                created_at: at,
                mode: "deep".into(),
                summary: None,
            })
            .unwrap();
        }
        let r = report_at(&db, "fast-slow", NOW);
        assert_eq!(r.lines, vec!["checked 1 answer \u{b7} 2 contradicted", "you rated 2 verdicts, 1 wrong"]);
        assert_eq!(r.records, 2, "the old check is still the experiment's data");

        // Beside a decision, the line comes last.
        decide(&db, "t", Purpose::ToolSelect, LOCAL, "acted", NOW - DAY_MS);
        let lines = report_at(&db, "fast-slow", NOW).lines;
        assert_eq!(lines.last().unwrap(), "you rated 2 verdicts, 1 wrong");
        assert!(lines.contains(&"checked 1 answer \u{b7} 2 contradicted".to_string()), "{lines:?}");

        assert_eq!(reset(&db, "fast-slow").unwrap(), 3, "one decision and two stored checks");
        assert_eq!(db.count_verifications().unwrap(), 0);
        assert!(report_at(&db, "fast-slow", NOW).lines.is_empty());
        assert!(reset(&db, "verify").is_err(), "it has no card any more");
    }

    #[test]
    fn the_relax_report_counts_skipped_questions_and_nothing_else_in_activity() {
        let db = Db::open_in_memory().unwrap();
        db.log_activity(None, "decide", &format!("{SKIPPED} \u{b7} edit_file \u{b7} low risk (0.97)")).unwrap();
        db.log_activity(None, "decide", &format!("{SKIPPED} \u{b7} write_file \u{b7} low risk (0.96)")).unwrap();
        db.log_activity(None, "decide", "Held back 2 tools").unwrap();
        db.log_activity(None, "file", &format!("{SKIPPED} in another kind")).unwrap();
        let r = report_at(&db, "relax-prompts", now_ms());
        assert_eq!(r.lines, vec!["Skipped 2 questions"]);
        assert_eq!(r.records, 2);
    }

    #[test]
    fn the_workspace_report_counts_chats_used_in_the_window() {
        let db = Db::open_in_memory().unwrap();
        db.create_conversation("Plain", None, false).unwrap();
        db.create_conversation("Trip", None, true).unwrap();
        let r = report_at(&db, "workspace", now_ms());
        assert_eq!(r.lines, vec!["1 Workspace chat used"]);
        assert_eq!(r.records, 0, "a Workspace chat is an ordinary chat, so there is nothing to reset");
        assert!(report_at(&db, "workspace", now_ms() + 31 * DAY_MS).lines.is_empty());
    }

    #[test]
    fn a_reset_forgets_one_experiments_data_and_labels_and_leaves_the_rest() {
        let db = Db::open_in_memory().unwrap();
        decide(&db, "t", Purpose::ToolSelect, LOCAL, "acted", NOW);
        db.add_outcome("t", "wrong", None, "need-tools").unwrap();
        decide(&db, "m", Purpose::MailTriage, LOCAL, "default", NOW);
        decide(&db, "other", Purpose::GoalMet, LOCAL, "default", NOW);
        db.set_setting(&crate::experiments::setting_key("fast-slow"), "on").unwrap();

        assert_eq!(reset(&db, "fast-slow").unwrap(), 1);
        let left: Vec<_> = db.decisions_recent(10, None).unwrap().into_iter().map(|d| d.id).collect();
        assert!(left.contains(&"m".to_string()) && left.contains(&"other".to_string()) && !left.contains(&"t".to_string()));
        assert_eq!(db.outcomes_since(None, None, 0).unwrap(), 0, "the label went with its decision");
        assert!(crate::experiments::is_on(&db, "fast-slow"), "the switch is not touched");
        assert_eq!(reset(&db, "fast-slow").unwrap(), 0, "nothing to forget is not an error");
        assert!(reset(&db, "nope").is_err());
    }

    #[test]
    fn a_mail_reset_clears_the_inbox_and_what_the_loop_remembers_so_it_fills_again() {
        let _turn = mail_sync::test_lock();
        let db = Db::open_in_memory().unwrap();
        let account = db
            .add_mail_account(&crate::db::NewMailAccount {
                label: "Work".into(),
                email: "me@example.com".into(),
                imap_host: "imap.example.com".into(),
                imap_port: 993,
                smtp_host: "smtp.example.com".into(),
                smtp_port: 465,
                username: "me@example.com".into(),
                security: "tls".into(),
            })
            .unwrap();
        db.set_setting(&mail_sync::state_key(&account.id), r#"{"uid_validity":7,"last_uid":40}"#).unwrap();
        db.add_mail_seen(
            &[crate::db::mail_seen::NewSeen {
                account_id: &account.id,
                uid: 40,
                message_id: None,
                from: "a@b.c",
                subject: "Hello",
                date: "",
                sent_at: None,
                unread: true,
            }],
            NOW,
        )
        .unwrap();
        decide(&db, "m", Purpose::MailTriage, LOCAL, "default", NOW);

        assert_eq!(reset(&db, "mail-triage").unwrap(), 2);
        assert_eq!(db.count_mail_seen().unwrap(), 0);
        assert_eq!(mail_sync::state_of(&db, &account.id), mail_sync::SyncState::default());
        assert!(!mail_sync::is_running(), "the loop is let go again");
    }

    /// While a look at the inbox runs, a mail reset waits for it and removes
    /// nothing, so the look cannot write back what the reset took away. Another
    /// experiment's reset does not wait.
    #[test]
    fn a_mail_reset_during_a_look_removes_nothing_and_says_why() {
        let _turn = mail_sync::test_lock();
        let db = Db::open_in_memory().unwrap();
        decide(&db, "m", Purpose::MailTriage, LOCAL, "default", NOW);
        decide(&db, "t", Purpose::ToolSelect, LOCAL, "acted", NOW);
        let look = mail_sync::hold().expect("no look running");
        assert_eq!(reset(&db, "mail-triage"), Err(BUSY.to_string()));
        assert_eq!(records(&db, "mail-triage"), 1, "nothing was removed");
        assert_eq!(reset(&db, "fast-slow").unwrap(), 1);
        drop(look);
        assert_eq!(reset(&db, "mail-triage").unwrap(), 1);
    }

    #[test]
    fn a_reset_of_the_skipped_questions_removes_only_their_rows() {
        let db = Db::open_in_memory().unwrap();
        db.log_activity(None, "decide", &format!("{SKIPPED} \u{b7} edit_file \u{b7} low risk (0.97)")).unwrap();
        db.log_activity(None, "decide", "Held back 2 tools").unwrap();
        assert_eq!(reset(&db, "relax-prompts").unwrap(), 1);
        assert_eq!(db.count_activity_since("decide", "Held", 0).unwrap(), 1);
    }
}
