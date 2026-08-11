//! Mail account commands (`MAIL-UI-1`): add, list, test, enable/disable and
//! remove IMAP/SMTP accounts. Passwords live in the OS credential store, never
//! SQLite — mirrors `commands/connectors.rs`'s shape for MCP tokens.

use tauri::State;

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
    let _ = secrets::delete_secret(SERVICE_MAIL, &id);
    Ok(())
}
