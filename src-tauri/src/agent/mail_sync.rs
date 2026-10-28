//! `USE-6a`: the fetch loop of the small inbox. While **Mail triage** is on, every
//! 15 minutes each enabled mail account (IMAP, the ones in Settings -> Mail) is
//! asked for what arrived in its INBOX since the last look, and the newest messages
//! no check has seen get one (`decide::mailtriage`).
//!
//! What it keeps, in `mail_seen`: who wrote, the subject and the date of up to 200
//! messages, for 30 days, and the tags. **Never a body.** A body is read for the
//! check, 4 KB of it, and dropped. Nothing is moved, marked read or deleted on the
//! server: the looks use `BODY.PEEK` and envelopes.
//!
//! What it does not read: mail that is connected through an app (Gmail or Outlook
//! over Composio). There is no new-mail signal there without triggers, and polling
//! it would be calls nobody asked for going through a third party. The loop starts
//! from `Db::list_mail_accounts`, which holds only accounts added here.
//!
//! Nothing runs while the experiment is off, and a switched-off account is left
//! alone. Rows stay when the experiment goes off, until they are 30 days old.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use super::decide::mailtriage::{self, MailText, Triage};
use super::decide::DecideError;
use super::mail::{self, NewMail, INBOX_KEEP};
use crate::db::mail_seen::NewSeen;
use crate::db::{now_ms, Db, MailAccount};
use crate::runtime::{decideserver, RuntimeManager};

/// How often an account is looked at.
pub const EVERY_MINUTES: u32 = 15;
const EVERY_MS: i64 = EVERY_MINUTES as i64 * 60_000;
/// Wait this long after start before the first look, so the app is up first.
const FIRST_AFTER: Duration = Duration::from_secs(90);
/// The loop wakes this often to see whether a look is due or the switch changed.
const TICK: Duration = Duration::from_secs(30);
/// Rows older than this are dropped.
pub const KEEP_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// Messages one look checks, newest first. A first look finds up to 200, and the
/// background share of an hour is 60, so the rest wait for the next look.
pub const TRIAGE_PER_LOOK: usize = 15;
/// What the Inbox shows of an account at most.
pub const SHOWN: i64 = INBOX_KEEP as i64;

const STATE_KEY: &str = "mail.sync.state.";
const LAST_KEY: &str = "mail.sync.last";
/// Told to the screen when a look found mail or a check wrote a tag.
pub const EVENT: &str = "poiesis-mail-inbox";

/// Where an account's last look ended: the mailbox's `UIDVALIDITY` and the newest
/// UID kept. Kept apart from the rows, so pruning them never makes the next look
/// fetch what it already saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncState {
    #[serde(default)]
    pub uid_validity: Option<u32>,
    #[serde(default)]
    pub last_uid: Option<u32>,
}

/// How the last look went, for the line under the Inbox's title.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LastSync {
    pub at: i64,
    pub fetched: usize,
    pub triaged: usize,
    /// One line for each account that could not be read.
    pub errors: Vec<String>,
}

pub fn state_key(account_id: &str) -> String {
    format!("{STATE_KEY}{account_id}")
}

pub fn state_of(db: &Db, account_id: &str) -> SyncState {
    db.get_setting(&state_key(account_id))
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(db: &Db, account_id: &str, state: &SyncState) {
    if let Ok(raw) = serde_json::to_string(state) {
        let _ = db.set_setting(&state_key(account_id), &raw);
    }
}

pub fn last_sync(db: &Db) -> Option<LastSync> {
    db.get_setting(LAST_KEY).ok().flatten().and_then(|raw| serde_json::from_str(&raw).ok())
}

fn save_last(db: &Db, last: &LastSync) {
    if let Ok(raw) = serde_json::to_string(last) {
        let _ = db.set_setting(LAST_KEY, &raw);
    }
}

/// Whether a look is due: none yet, or the last one is 15 minutes old.
pub fn due(last_at: Option<i64>, now: i64) -> bool {
    last_at.is_none_or(|at| now - at >= EVERY_MS)
}

/// Keep what a look found. A renumbered mailbox first loses the rows kept for it;
/// the new messages are added (one already there is left alone); and the newest UID
/// is remembered for the next look. Returns how many messages were new.
pub fn keep(db: &Db, account_id: &str, mail: &NewMail, now: i64) -> usize {
    let before = state_of(db, account_id);
    if mail.reset {
        let _ = db.clear_mail_seen(account_id);
    }
    let rows: Vec<NewSeen> = mail
        .rows
        .iter()
        .map(|r| NewSeen {
            account_id,
            uid: r.uid,
            message_id: r.message_id.as_deref(),
            from: &r.from,
            subject: &r.subject,
            date: &r.date,
            sent_at: r.sent_at,
            unread: r.unread,
        })
        .collect();
    let added = db.add_mail_seen(&rows, now).unwrap_or(0);
    let newest = mail.rows.iter().map(|r| r.uid).max();
    let last_uid = if mail.reset { newest } else { newest.max(before.last_uid) };
    save_state(db, account_id, &SyncState { uid_validity: mail.uid_validity.or(before.uid_validity), last_uid });
    added
}

/// One look at a time: the timer and the refresh button share this.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// While one is held, no other look starts. Only [`hold`] and a look make one.
pub struct Running(());

impl Running {
    fn take() -> Option<Running> {
        RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| Running(()))
    }
}

/// Keep looks from starting while the inbox is reset (`EXP-4`). `None` while a look
/// is running: a reset waits for it rather than race it, because the look would
/// write back what the reset removes.
pub fn hold() -> Option<Running> {
    Running::take()
}

/// Tests that take the one look in turn, so they never see each other's.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Whether a failed check is worth asking about again later. An engine that is
/// not there, not warm in time or over budget will do better next look. A failed
/// answer is about this message, so it is closed with nothing to show and never
/// blocks the ones behind it.
fn is_this_messages_fault(e: &DecideError) -> bool {
    matches!(e, DecideError::Failed(_))
}

/// The newest messages without a tag, across the enabled accounts, at most `limit`.
fn waiting(db: &Db, accounts: &[MailAccount], limit: usize) -> Vec<(MailAccount, Vec<crate::db::mail_seen::SeenRow>)> {
    let mut left = limit as i64;
    let mut out = Vec::new();
    for a in accounts {
        if left <= 0 {
            break;
        }
        let rows = db.mail_seen_untriaged(Some(&a.id), left).unwrap_or_default();
        left -= rows.len() as i64;
        if !rows.is_empty() {
            out.push((a.clone(), rows));
        }
    }
    out
}

/// One look: fetch what is new, prune, and check the newest unchecked messages.
/// `None` when the experiment is off or another look is running.
pub async fn run_cycle<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<LastSync> {
    let _running = Running::take()?;
    let db = app.state::<Db>();
    if !crate::experiments::is_on(&db, "mail-triage") {
        return None;
    }
    let accounts: Vec<MailAccount> = db.list_mail_accounts().unwrap_or_default().into_iter().filter(|a| a.enabled).collect();
    let mut last = LastSync { at: now_ms(), ..LastSync::default() };

    for account in &accounts {
        let password = match crate::secrets::get_secret(crate::secrets::SERVICE_MAIL, &account.id) {
            Ok(Some(p)) => p,
            _ => {
                last.errors.push(format!("{}: no password is stored for it", account.label));
                continue;
            }
        };
        let state = state_of(&db, &account.id);
        let (a, known, after) = (account.clone(), state.uid_validity, state.last_uid);
        let got = tokio::task::spawn_blocking(move || mail::fetch_new_blocking(a, password, after, known, INBOX_KEEP))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
        match got {
            Ok(new_mail) => {
                last.fetched += keep(&db, &account.id, &new_mail, now_ms());
                // `INT-2`: a reminder waiting for this sender is ready for the next turn.
                // A renumbered mailbox hands back old mail as new, which says nothing.
                if let (false, Some(mem)) = (new_mail.reset, app.try_state::<crate::memory::MemoryStore>()) {
                    let senders: Vec<String> = new_mail.rows.iter().map(|r| r.from.clone()).collect();
                    for name in mem.mail_arrived(&db, &senders) {
                        let _ = db.log_activity(None, "mail", &format!("a message came for the reminder {name}"));
                    }
                }
                let _ = app.emit(EVENT, ());
            }
            Err(e) => last.errors.push(format!("{}: {e}", account.label)),
        }
    }
    let _ = db.prune_mail_seen(now_ms() - KEEP_MS);

    last.triaged = triage_waiting(app, &accounts, &mut last.errors).await;
    // Looks and checks are done: say so, even when the Inbox was empty.
    last.at = now_ms();
    save_last(&db, &last);
    let _ = app.emit(EVENT, ());
    if last.fetched > 0 || last.triaged > 0 {
        let _ = db.log_activity(None, "mail", &format!("Inbox: {} new, {} checked", last.fetched, last.triaged));
    }
    Some(last)
}

/// Check the newest messages that have no tag. Stops at the first answer that
/// says the decision model is not there, not ready or over budget: asking the rest
/// would only fail the same way.
async fn triage_waiting<R: tauri::Runtime>(app: &AppHandle<R>, accounts: &[MailAccount], errors: &mut Vec<String>) -> usize {
    let db = app.state::<Db>();
    let batches = waiting(&db, accounts, TRIAGE_PER_LOOK);
    if batches.is_empty() {
        return 0;
    }
    // Reading the bodies costs a connection each. Do not, when nothing could be asked.
    if decideserver::resolve_local(&app.state::<RuntimeManager>(), &db).await.is_none()
        && !crate::agent::decide::cloud::allowed_for(&db, crate::agent::decide::Purpose::MailTriage)
    {
        return 0;
    }
    let kinds = mailtriage::categories(&db);
    let mut done = 0;
    for (account, rows) in batches {
        let Ok(Some(password)) = crate::secrets::get_secret(crate::secrets::SERVICE_MAIL, &account.id) else {
            continue;
        };
        let uids: Vec<u32> = rows.iter().map(|r| r.uid).collect();
        let a = account.clone();
        let texts = match tokio::task::spawn_blocking(move || mail::triage_texts_blocking(a, password, uids)).await {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => {
                errors.push(format!("{}: {e}", account.label));
                continue;
            }
            Err(_) => continue,
        };
        for row in rows {
            let text = texts.iter().find(|(uid, _)| *uid == row.uid).map(|(_, t)| t.as_str()).unwrap_or("");
            let mail = MailText { from: &row.from, subject: &row.subject, date: &row.date, body: text };
            let triage = match mailtriage::check(&kinds, &mail).await {
                Ok(t) => t,
                // A message the model cannot answer is closed with nothing to show.
                Err(e) if is_this_messages_fault(&e) => Triage::default(),
                Err(_) => return done,
            };
            if db.set_mail_triage(&account.id, row.uid, &triage.to_json(), now_ms()).unwrap_or(false) {
                done += 1;
                let _ = app.emit(EVENT, ());
            }
        }
    }
    done
}

/// Start the loop. It does nothing while the experiment is off.
pub fn start<R: tauri::Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_AFTER).await;
        loop {
            let ready = {
                let db = app.state::<Db>();
                crate::experiments::is_on(&db, "mail-triage") && due(last_sync(&db).map(|l| l.at), now_ms())
            };
            if ready {
                let _ = run_cycle(&app).await;
            }
            tokio::time::sleep(TICK).await;
        }
    });
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

    fn envelope(uid: u32, subject: &str) -> mail::InboxEnvelope {
        mail::InboxEnvelope {
            uid,
            message_id: Some(format!("<{uid}@example.com>")),
            from: "Ana Ruiz".into(),
            subject: subject.into(),
            date: "Thu, 23 Oct 2026 08:52:00 +0000".into(),
            sent_at: Some(1_000 + uid as i64),
            unread: true,
        }
    }

    fn found(uid_validity: Option<u32>, reset: bool, uids: &[u32]) -> NewMail {
        NewMail { uid_validity, reset, rows: uids.iter().rev().map(|u| envelope(*u, &format!("mail {u}"))).collect() }
    }

    #[test]
    fn a_look_is_due_when_there_was_none_or_the_last_is_fifteen_minutes_old() {
        assert!(due(None, 5));
        assert!(!due(Some(0), EVERY_MS - 1));
        assert!(due(Some(0), EVERY_MS));
        assert!(!due(Some(1_000_000), 1_000_000));
    }

    /// `USE-T-5`: only new UIDs are fetched, because the newest one kept is
    /// remembered apart from the rows.
    #[test]
    fn a_look_remembers_the_newest_uid_and_the_mailbox_it_belongs_to() {
        let db = Db::open_in_memory().unwrap();
        let a = account(&db);
        assert_eq!(state_of(&db, &a), SyncState::default());
        assert_eq!(keep(&db, &a, &found(Some(7), false, &[3, 4, 5]), 100), 3);
        assert_eq!(state_of(&db, &a), SyncState { uid_validity: Some(7), last_uid: Some(5) });
        // A look that found nothing leaves the state where it was.
        assert_eq!(keep(&db, &a, &found(Some(7), false, &[]), 200), 0);
        assert_eq!(state_of(&db, &a), SyncState { uid_validity: Some(7), last_uid: Some(5) });
        // And a server that never says what its mailbox number is does not erase it.
        assert_eq!(keep(&db, &a, &found(None, false, &[6]), 300), 1);
        assert_eq!(state_of(&db, &a), SyncState { uid_validity: Some(7), last_uid: Some(6) });
    }

    #[test]
    fn what_a_look_keeps_has_no_body_and_a_second_look_adds_nothing_twice() {
        let db = Db::open_in_memory().unwrap();
        let a = account(&db);
        keep(&db, &a, &found(Some(1), false, &[1, 2]), 100);
        let rows = db.mail_seen_list(Some(&a), 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].from, "Ana Ruiz");
        assert!(rows.iter().all(|r| r.triage.is_none()), "nothing is checked by keeping it");
        db.set_mail_triage(&a, 2, r#"{"category":"work"}"#, 150).unwrap();
        assert_eq!(keep(&db, &a, &found(Some(1), false, &[2, 3]), 200), 1);
        let two = db.mail_seen_get(&a, 2).unwrap().unwrap();
        assert!(two.triage.is_some(), "a message seen twice keeps its tag");
    }

    /// A renumbered mailbox: the UIDs kept no longer mean the same messages.
    #[test]
    fn a_renumbered_mailbox_starts_over() {
        let db = Db::open_in_memory().unwrap();
        let a = account(&db);
        keep(&db, &a, &found(Some(1), false, &[40, 41]), 100);
        let new = found(Some(2), true, &[1, 2]);
        assert_eq!(keep(&db, &a, &new, 200), 2);
        let uids: Vec<u32> = db.mail_seen_list(Some(&a), 10).unwrap().iter().map(|r| r.uid).collect();
        assert_eq!(uids.len(), 2);
        assert!(!uids.contains(&40), "the old numbers are gone: {uids:?}");
        assert_eq!(state_of(&db, &a), SyncState { uid_validity: Some(2), last_uid: Some(2) });
        // A renumbered, empty mailbox forgets the newest UID too.
        keep(&db, &a, &found(Some(3), true, &[]), 300);
        assert_eq!(state_of(&db, &a), SyncState { uid_validity: Some(3), last_uid: None });
    }

    #[test]
    fn the_unchecked_are_taken_newest_first_across_accounts_up_to_the_limit() {
        let db = Db::open_in_memory().unwrap();
        let a = account(&db);
        let b = account(&db);
        keep(&db, &a, &found(Some(1), false, &[1, 2, 3]), 100);
        keep(&db, &b, &found(Some(1), false, &[1, 2]), 100);
        let accounts = db.list_mail_accounts().unwrap();
        let got = waiting(&db, &accounts, 4);
        let shape: Vec<(String, Vec<u32>)> =
            got.iter().map(|(acc, rows)| (acc.id.clone(), rows.iter().map(|r| r.uid).collect())).collect();
        assert_eq!(shape, [(a.clone(), vec![3, 2, 1]), (b.clone(), vec![2])]);
        assert!(waiting(&db, &accounts, 0).is_empty());
        // A switched-off account is not in the list the loop passes, so it is never read.
        assert!(waiting(&db, &accounts[1..], 10).iter().all(|(acc, _)| acc.id == b));
    }

    #[test]
    fn only_a_failed_answer_closes_a_message_and_an_engine_that_is_not_there_does_not() {
        assert!(is_this_messages_fault(&DecideError::Failed("bad".into())));
        assert!(!is_this_messages_fault(&DecideError::Unavailable));
        assert!(!is_this_messages_fault(&DecideError::Deadline));
        assert!(!is_this_messages_fault(&DecideError::Budget(crate::agent::decide::control::Limit::MailShare)));
    }

    #[test]
    fn one_look_runs_at_a_time() {
        let _turn = test_lock();
        let first = Running::take().expect("free");
        assert!(is_running());
        assert!(Running::take().is_none(), "a second look waits for nobody: it is skipped");
        drop(first);
        assert!(!is_running());
        drop(Running::take().expect("free again"));
    }

    /// `USE-T-5`: the loop reads only the accounts added in Settings -> Mail, which
    /// is all `list_mail_accounts` holds. A Gmail that is connected through an app
    /// is not one of them, so it is never polled.
    #[test]
    fn only_mail_accounts_added_in_settings_are_read() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.list_mail_accounts().unwrap().is_empty());
        let a = account(&db);
        assert_eq!(db.list_mail_accounts().unwrap().iter().map(|x| x.id.clone()).collect::<Vec<_>>(), [a]);
    }

    /// The loop's own source never reaches an app connector. A guard on the file,
    /// since the unit cannot drive the loop without a server.
    #[test]
    fn the_sync_source_does_not_touch_the_app_connectors() {
        let src = include_str!("mail_sync.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        for forbidden in ["composio", "app_calls", "connectors::"] {
            assert!(!code.contains(forbidden), "mail_sync reaches {forbidden}");
        }
    }

    #[test]
    fn the_triage_share_of_a_look_fits_inside_the_hours_budget() {
        // Four looks an hour at 15 messages each is the share, no more.
        assert_eq!(TRIAGE_PER_LOOK as u32 * (60 / EVERY_MINUTES), crate::agent::decide::control::MAIL_PER_HOUR as u32);
    }

    /// With the experiment off, no look runs and nothing is read.
    #[tokio::test]
    async fn a_look_with_the_experiment_off_does_nothing() {
        let _turn = test_lock();
        let app = tauri::test::mock_app();
        app.manage(Db::open_in_memory().unwrap());
        assert!(run_cycle(app.handle()).await.is_none());
        assert!(last_sync(&app.state::<Db>()).is_none());
        assert!(!is_running());
    }
}
