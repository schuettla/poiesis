//! Mail account commands (`MAIL-UI-1`): add, list, test, enable/disable and
//! remove IMAP/SMTP accounts. Passwords live in the OS credential store, never
//! SQLite — mirrors `commands/connectors.rs`'s shape for MCP tokens.

use tauri::State;

use crate::agent::decide::mailtriage::{self, Triage};
use crate::agent::mail_sync::{self, LastSync};
use crate::db::mail_seen::SeenRow;
use crate::db::{Db, MailAccount, NewMailAccount};
use crate::secrets::{self, SERVICE_MAIL};
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// Add a mail account: store its connection details and stash the password in
/// the credential store. Does not verify the connection — call
/// `test_mail_account_cmd` for that (kept a separate step so a typo in the
/// password doesn't block saving the rest of the form).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn add_mail_account_cmd(
    db: State<'_, Db>,
    label: String,
    email: String,
    imap_host: String,
    imap_port: i64,
    smtp_host: String,
    smtp_port: i64,
    username: String,
    password: String,
    security: String,
) -> Cmd<MailAccount> {
    let account = db
        .add_mail_account(&NewMailAccount {
            label,
            email,
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            username,
            security,
        })
        .map_err(err)?;
    secrets::set_secret(SERVICE_MAIL, &account.id, &password).map_err(err)?;
    let _ = db.log_activity(None, "mail", &format!("added the account {}", account.label));
    Ok(account)
}

#[tauri::command]
pub fn list_mail_accounts_cmd(db: State<'_, Db>) -> Cmd<Vec<MailAccount>> {
    db.list_mail_accounts().map_err(err)
}

/// Result of testing an account: IMAP login + SMTP handshake, no send
/// (`MAIL-UI-1`).
#[derive(serde::Serialize)]
pub struct MailTestResult {
    pub ok: bool,
    pub message_count: Option<u32>,
    pub error: Option<String>,
}

/// IMAP login + `SELECT INBOX` + SMTP connect, blocking (see `agent/mail.rs`'s
/// note on why these clients run under `spawn_blocking` rather than an async
/// IMAP crate).
fn test_blocking(account: MailAccount, password: String) -> MailTestResult {
    // Both legs go through `agent::mail`'s connectors, so a successful Test
    // means exactly what a successful `list_mail` would — same TLS mode, same
    // loopback handling, no second implementation to drift.
    let mut session = match crate::agent::mail::imap_connect(&account, &password) {
        Ok(s) => s,
        Err(e) => return MailTestResult { ok: false, message_count: None, error: Some(e) },
    };
    let mailbox = match session.select("INBOX") {
        Ok(m) => m,
        Err(e) => {
            return MailTestResult { ok: false, message_count: None, error: Some(format!("couldn't open INBOX: {e}")) }
        }
    };
    let count = mailbox.exists;
    let _ = session.logout();

    match crate::agent::mail::smtp_transport(&account, password) {
        Ok(mailer) => {
            // `test_connection` opens the connection, runs EHLO/AUTH, and closes —
            // the SMTP equivalent of the IMAP `SELECT` above, without sending mail.
            match mailer.test_connection() {
                Ok(true) => MailTestResult { ok: true, message_count: Some(count), error: None },
                Ok(false) => MailTestResult {
                    ok: false,
                    message_count: Some(count),
                    error: Some("reached your inbox, but the send server didn't answer".to_string()),
                },
                Err(e) => MailTestResult {
                    ok: false,
                    message_count: Some(count),
                    error: Some(format!("the send server refused the connection: {e}")),
                },
            }
        }
        Err(e) => MailTestResult { ok: false, message_count: Some(count), error: Some(e) },
    }
}

#[tauri::command]
pub async fn test_mail_account_cmd(db: State<'_, Db>, id: String) -> Cmd<MailTestResult> {
    let account = db.get_mail_account(&id).map_err(err)?.ok_or_else(|| PoiesisError::Message("That account no longer exists.".into()))?;
    let password = secrets::get_secret(SERVICE_MAIL, &id)
        .map_err(err)?
        .ok_or_else(|| PoiesisError::Message("No password stored for that account.".into()))?;
    tokio::task::spawn_blocking(move || test_blocking(account, password)).await.map_err(err)
}

#[tauri::command]
pub fn set_mail_account_enabled_cmd(db: State<'_, Db>, id: String, enabled: bool) -> Cmd<()> {
    db.set_mail_account_enabled(&id, enabled).map_err(err)
}

#[tauri::command]
pub fn delete_mail_account_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    db.delete_mail_account(&id).map_err(err)?;
    // Its inbox rows went with it (`ON DELETE CASCADE`); this is where its last look ended.
    let _ = db.set_setting(&mail_sync::state_key(&id), "");
    let _ = secrets::delete_secret(SERVICE_MAIL, &id);
    Ok(())
}

// ---- the small inbox (`USE-6a`, `USE-UI-4`) ----

/// One row of the Inbox: the message and what the check said about it. A part the
/// model was not sure of is `None`, and the screen shows nothing for it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct InboxRow {
    pub account_id: String,
    pub account_label: String,
    pub uid: u32,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub sent_at: Option<i64>,
    pub unread: bool,
    /// A check has looked at it, even when it found nothing to show.
    pub checked: bool,
    pub category: Option<String>,
    pub category_p: Option<f32>,
    /// 1 (can wait) to 4 (urgent).
    pub urgency: Option<u8>,
    pub urgency_p: Option<f32>,
    pub reply: Option<f32>,
    /// The person set the kind themselves.
    pub retagged: bool,
}

fn inbox_row(row: &SeenRow, account_label: &str) -> InboxRow {
    let t = row.triage.as_ref().map(Triage::from_json).unwrap_or_default();
    InboxRow {
        account_id: row.account_id.clone(),
        account_label: account_label.to_string(),
        uid: row.uid,
        from: row.from.clone(),
        subject: row.subject.clone(),
        date: row.date.clone(),
        sent_at: row.sent_at.or(Some(row.fetched_at)),
        unread: row.unread,
        checked: row.triage.is_some(),
        category: t.category,
        category_p: t.category_p,
        urgency: t.urgency,
        urgency_p: t.urgency_p,
        reply: t.reply,
        retagged: t.retagged,
    }
}

#[derive(Debug, serde::Serialize)]
pub struct InboxView {
    /// Mail triage is on. Off, the Inbox tab is not shown.
    pub on: bool,
    /// A decision model is installed. Without one nothing gets a tag.
    pub decision_model: bool,
    pub rows: Vec<InboxRow>,
    pub last: Option<LastSync>,
    pub every_minutes: u32,
    /// A look is running now.
    pub syncing: bool,
    pub categories: Vec<String>,
}

/// The Inbox, newest first. `account_id` narrows it to one account.
#[tauri::command]
pub fn mail_inbox_cmd(db: State<'_, Db>, account_id: Option<String>) -> Cmd<InboxView> {
    let accounts = db.list_mail_accounts().map_err(err)?;
    let label = |id: &str| accounts.iter().find(|a| a.id == id).map(|a| a.label.clone()).unwrap_or_default();
    let rows = db
        .mail_seen_list(account_id.as_deref(), mail_sync::SHOWN)
        .map_err(err)?
        .iter()
        .filter(|r| accounts.iter().any(|a| a.id == r.account_id))
        .map(|r| inbox_row(r, &label(&r.account_id)))
        .collect();
    Ok(InboxView {
        on: crate::experiments::is_on(&db, "mail-triage"),
        decision_model: crate::commands::experiments::facts(&db).decision_model,
        rows,
        last: mail_sync::last_sync(&db),
        every_minutes: mail_sync::EVERY_MINUTES,
        syncing: mail_sync::is_running(),
        categories: mailtriage::categories(&db),
    })
}

/// Look now instead of at the next quarter hour. `None` when Mail triage is off or a
/// look is already running.
#[tauri::command]
pub async fn mail_sync_now_cmd(app: tauri::AppHandle) -> Cmd<Option<LastSync>> {
    Ok(mail_sync::run_cycle(&app).await)
}

/// Set, change or clear the kind of one message. The change is the label the
/// calibration reads (`CTL-3`).
#[tauri::command]
pub fn mail_retag_cmd(db: State<'_, Db>, account_id: String, uid: u32, category: Option<String>) -> Cmd<InboxRow> {
    mailtriage::retag(&db, &account_id, uid, category.as_deref()).map_err(PoiesisError::Message)?;
    let row = db
        .mail_seen_get(&account_id, uid)
        .map_err(err)?
        .ok_or_else(|| PoiesisError::Message("That message is no longer in the Inbox.".into()))?;
    let label = db.get_mail_account(&account_id).map_err(err)?.map(|a| a.label).unwrap_or_default();
    Ok(inbox_row(&row, &label))
}

#[derive(Debug, serde::Serialize)]
pub struct Categories {
    pub categories: Vec<String>,
    pub defaults: Vec<String>,
}

#[tauri::command]
pub fn mail_categories_cmd(db: State<'_, Db>) -> Categories {
    Categories { categories: mailtriage::categories(&db), defaults: mailtriage::default_categories() }
}

/// Keep the user's kinds, cleaned. An empty list puts the default one back.
#[tauri::command]
pub fn set_mail_categories_cmd(db: State<'_, Db>, categories: Vec<String>) -> Cmd<Categories> {
    let kept = mailtriage::set_categories(&db, &categories).map_err(PoiesisError::Message)?;
    Ok(Categories { categories: kept, defaults: mailtriage::default_categories() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(triage: Option<serde_json::Value>) -> SeenRow {
        SeenRow {
            account_id: "a".into(),
            uid: 4,
            message_id: None,
            from: "Ana Ruiz".into(),
            subject: "Contract draft".into(),
            date: "Thu, 23 Oct 2026 08:52:00 +0000".into(),
            sent_at: Some(5_000),
            unread: true,
            triage,
            triaged_at: None,
            fetched_at: 9_000,
        }
    }

    #[test]
    fn a_row_carries_only_the_parts_the_check_was_sure_of() {
        let row = inbox_row(
            &seen(Some(serde_json::json!({ "category": "work", "category_p": 0.87, "urgency": 3, "urgency_p": 0.7, "decision": "d" }))),
            "Work",
        );
        assert_eq!((row.category.as_deref(), row.urgency, row.reply), (Some("work"), Some(3), None));
        assert!(row.checked && !row.retagged);
        assert_eq!(row.account_label, "Work");
    }

    /// "Unclassified rows show nothing, never a guess": a message nobody has
    /// checked, and one checked with nothing to show, are different rows.
    #[test]
    fn an_unchecked_row_and_an_empty_check_are_told_apart_and_both_show_no_tag() {
        let unchecked = inbox_row(&seen(None), "Work");
        let empty = inbox_row(&seen(Some(serde_json::json!({}))), "Work");
        assert!(!unchecked.checked && empty.checked);
        for r in [&unchecked, &empty] {
            assert_eq!((r.category.as_deref(), r.urgency, r.reply), (None, None, None));
        }
    }

    #[test]
    fn a_message_with_no_readable_date_is_placed_by_when_it_was_fetched() {
        let mut s = seen(None);
        s.sent_at = None;
        assert_eq!(inbox_row(&s, "Work").sent_at, Some(9_000));
    }
}
