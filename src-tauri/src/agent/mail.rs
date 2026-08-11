//! The Mail toolset (`MAIL-2`): read, search and send email over an account
//! the user connects directly (IMAP/SMTP), credentials in the OS credential
//! store. No hosted relay — nothing about a mailbox touches a third party.
//!
//! `imap`/`lettre` are blocking clients (real async IMAP in Rust means
//! `async-imap`, which is built for `async-std`, not this app's `tokio`
//! runtime). Every network call here runs inside `tokio::task::spawn_blocking`
//! rather than pulling in a second async ecosystem for one toolset. A fresh
//! IMAP session is opened per call rather than pooled per run (`LOOP-1`'s
//! pattern for MCP) — simpler, and correct first; pooling is a follow-up if
//! per-call `LOGIN` proves too slow in practice.

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message as SmtpMessage, SmtpTransport, Transport};
use mail_parser::MimeHeaders;

use crate::autonomy::{autonomy_gate, Rung};
use crate::db::{Db, MailAccount};

use super::toolsets::{mark_untrusted, ToolContext};

/// Cap on a read message's body before it is wrapped as untrusted content.
const BODY_CAP: usize = 8000;
/// Cap on how many envelopes `list_mail`/`search_mail` return in one call.
const MAX_LIST: usize = 25;

pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "list_mail",
                "description": "List recent email envelopes (from, subject, date) — never bodies. Read one with read_mail once you know which.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "account": { "type": "string", "description": "Account label or email, if more than one is set up." },
                        "folder": { "type": "string", "description": "Defaults to INBOX." },
                        "limit": { "type": "integer", "description": "Up to 25, default 10." },
                        "unread_only": { "type": "boolean" }
                    },
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "search_mail",
                "description": "Search email by text (subject/body/sender). Returns envelopes, not bodies.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "account": { "type": "string" },
                        "folder": { "type": "string", "description": "Defaults to INBOX." },
                        "limit": { "type": "integer", "description": "Up to 25, default 10." }
                    },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_mail",
                "description": "Read one email's body by id (from list_mail/search_mail). Attachments are named, not fetched.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "account": { "type": "string" },
                        "folder": { "type": "string", "description": "Defaults to INBOX." }
                    },
                    "required": ["id"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "send_mail",
                "description": "Send a new email on the user's behalf. The user reviews and approves before anything is sent.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "to": { "type": "string" },
                        "subject": { "type": "string" },
                        "body": { "type": "string" },
                        "cc": { "type": "string" },
                        "account": { "type": "string" }
                    },
                    "required": ["to", "subject", "body"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "reply_mail",
                "description": "Reply to an email by id (from list_mail/search_mail/read_mail). Threading headers are set automatically. The user reviews and approves before anything is sent.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "body": { "type": "string" },
                        "reply_all": { "type": "boolean" },
                        "account": { "type": "string" }
                    },
                    "required": ["id", "body"]
                }
            }
        }
    ])
}

pub fn handles(name: &str) -> bool {
    matches!(name, "list_mail" | "search_mail" | "read_mail" | "send_mail" | "reply_mail")
}

/// Human-readable (verb, target) for the timeline (`MAIL-4`: reading is honest
/// about volume — the count and the account name, never silent).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    let account = args.get("account").and_then(|a| a.as_str()).unwrap_or("your mail");
    match name {
        "list_mail" => {
            let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(10);
            ("read".into(), format!("{limit} messages from {account}"))
        }
        "search_mail" => {
            let q = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
            ("searched mail for".into(), format!("\u{201c}{q}\u{201d}"))
        }
        "read_mail" => ("read a message from".into(), account.to_string()),
        "send_mail" => {
            let to = args.get("to").and_then(|t| t.as_str()).unwrap_or("someone");
            ("sent mail to".into(), to.to_string())
        }
        "reply_mail" => ("replied to a message in".into(), account.to_string()),
        other => (other.into(), String::new()),
    }
}

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    match name {
        "list_mail" => list_mail(ctx, args).await,
        "search_mail" => search_mail(ctx, args).await,
        "read_mail" => read_mail(ctx, args).await,
        "send_mail" => send_mail(ctx, args).await,
        "reply_mail" => reply_mail(ctx, args).await,
        other => Err(format!("Mail doesn't handle '{other}'.")),
    }
}

fn required<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("missing '{key}' argument"))
}

fn folder_arg(args: &serde_json::Value) -> String {
    args.get("folder")
        .and_then(|f| f.as_str())
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .unwrap_or("INBOX")
        .to_string()
}

fn limit_arg(args: &serde_json::Value) -> usize {
    args.get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(10)
        .clamp(1, MAX_LIST)
}

/// Resolve which account a call runs against: the named one, or the sole
/// enabled account, or a clear ask-again error when there's a real choice.
fn resolve_account(db: &Db, account_arg: Option<&str>) -> Result<(MailAccount, String), String> {
    let enabled: Vec<MailAccount> = db
        .list_mail_accounts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|a| a.enabled)
        .collect();

    let account = match account_arg {
        Some(needle) => enabled
            .into_iter()
            .find(|a| a.id == needle || a.label.eq_ignore_ascii_case(needle) || a.email.eq_ignore_ascii_case(needle))
            .ok_or_else(|| format!("no mail account matching \"{needle}\""))?,
        None => match enabled.len() {
            0 => return Err("no mail account is set up yet — add one in Settings \u{2192} Mail".to_string()),
            1 => enabled.into_iter().next().expect("checked len == 1"),
            _ => {
                let names: Vec<&str> = enabled.iter().map(|a| a.label.as_str()).collect();
                return Err(format!(
                    "more than one mail account is set up — say which one ({})",
                    names.join(", ")
                ));
            }
        },
    };
    let password = crate::secrets::get_secret(crate::secrets::SERVICE_MAIL, &account.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no password stored for {}", account.label))?;
    Ok((account, password))
}

// ---- blocking IMAP/SMTP (run under spawn_blocking) ----

#[derive(Debug, Clone)]
struct EnvelopeRow {
    uid: u32,
    from: String,
    subject: String,
    date: String,
    seen: bool,
}

struct ReadResult {
    from_display: String,
    from_addr: String,
    to_addrs: Vec<String>,
    cc_addrs: Vec<String>,
    subject: String,
    date: String,
    body: String,
    attachments: Vec<String>,
    message_id: Option<String>,
}

/// How a connection reaches TLS. Implicit ("wrapper") TLS and STARTTLS are not
/// interchangeable: offering one where the server expects the other doesn't
/// degrade, it hangs or fails the handshake — which is why the account carries
/// the answer rather than the code guessing per attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// Implicit TLS from the first byte — IMAPS 993, SMTPS 465.
    Tls,
    /// Plaintext greeting, then `STARTTLS` — IMAP 143, submission 587, and
    /// every local bridge.
    StartTls,
}

impl Security {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "starttls" => Security::StartTls,
            _ => Security::Tls,
        }
    }
}

/// A local bridge (Proton Mail Bridge, Mailpit, a dev server) terminates TLS
/// with a certificate it generated for itself, which no trust store will ever
/// accept. Loosening verification for **loopback only** is what makes those
/// usable at all; for any other host this stays strict, because there the same
/// switch would be the difference between private mail and an open door.
fn is_loopback(host: &str) -> bool {
    let h = host.trim().trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase();
    h == "localhost" || h == "127.0.0.1" || h == "::1" || h.starts_with("127.")
}

fn tls_connector(host: &str) -> Result<native_tls::TlsConnector, String> {
    let mut builder = native_tls::TlsConnector::builder();
    if is_loopback(host) {
        builder.danger_accept_invalid_certs(true);
        builder.danger_accept_invalid_hostnames(true);
    }
    builder.build().map_err(|e| e.to_string())
}

pub fn imap_connect(
    account: &MailAccount,
    password: &str,
) -> Result<imap::Session<native_tls::TlsStream<std::net::TcpStream>>, String> {
    let host = account.imap_host.as_str();
    let tls = tls_connector(host)?;
    let addr = (host, account.imap_port as u16);
    let client = match Security::parse(&account.security) {
        Security::Tls => imap::connect(addr, host, &tls),
        Security::StartTls => imap::connect_starttls(addr, host, &tls),
    }
    .map_err(|e| format!("couldn't reach {host}: {e}"))?;
    client
        .login(&account.username, password)
        .map_err(|(e, _)| format!("IMAP login failed: {e}"))
}

/// Build the SMTP transport for an account, honouring its security mode. Not
/// `SmtpTransport::relay` + `.port()`: `relay` pins implicit TLS, so pointing
/// it at a STARTTLS submission port produces a handshake failure the user can
/// only read as "it doesn't work".
pub fn smtp_transport(account: &MailAccount, password: String) -> Result<SmtpTransport, String> {
    let host = account.smtp_host.as_str();
    let mut params = lettre::transport::smtp::client::TlsParameters::builder(host.to_string());
    if is_loopback(host) {
        params = params
            .dangerous_accept_invalid_certs(true)
            .dangerous_accept_invalid_hostnames(true);
    }
    let params = params.build().map_err(|e| format!("couldn't set up TLS for {host}: {e}"))?;
    let tls = match Security::parse(&account.security) {
        Security::Tls => lettre::transport::smtp::client::Tls::Wrapper(params),
        Security::StartTls => lettre::transport::smtp::client::Tls::Required(params),
    };
    Ok(SmtpTransport::builder_dangerous(host)
        .port(account.smtp_port as u16)
        .tls(tls)
        .credentials(Credentials::new(account.username.clone(), password))
        .build())
}

fn address_str(a: &imap_proto::types::Address) -> String {
    let mailbox = a.mailbox.as_ref().map(|m| String::from_utf8_lossy(m).into_owned()).unwrap_or_default();
    let host = a.host.as_ref().map(|h| String::from_utf8_lossy(h).into_owned()).unwrap_or_default();
    if mailbox.is_empty() {
        String::new()
    } else if host.is_empty() {
        mailbox
    } else {
        format!("{mailbox}@{host}")
    }
}

fn envelope_row(f: &imap::types::Fetch) -> EnvelopeRow {
    let uid = f.uid.unwrap_or(0);
    let env = f.envelope();
    let subject = env
        .and_then(|e| e.subject.as_ref())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .unwrap_or_default();
    let from = env
        .and_then(|e| e.from.as_ref())
        .and_then(|addrs| addrs.first())
        .map(address_str)
        .unwrap_or_default();
    let date = env
        .and_then(|e| e.date.as_ref())
        .map(|d| String::from_utf8_lossy(d).into_owned())
        .unwrap_or_default();
    let seen = f.flags().iter().any(|fl| matches!(fl, imap::types::Flag::Seen));
    EnvelopeRow { uid, from, subject, date, seen }
}

fn list_blocking(
    account: MailAccount,
    password: String,
    folder: String,
    limit: usize,
    unread_only: bool,
) -> Result<Vec<EnvelopeRow>, String> {
    let mut session = imap_connect(&account, &password)?;
    session.select(&folder).map_err(|e| format!("couldn't open {folder}: {e}"))?;
    let query = if unread_only { "UNSEEN" } else { "ALL" };
    let mut uids: Vec<u32> = session.uid_search(query).map_err(|e| e.to_string())?.into_iter().collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids.truncate(limit);
    let rows = if uids.is_empty() {
        Vec::new()
    } else {
        let set = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let fetches = session.uid_fetch(&set, "(UID ENVELOPE FLAGS)").map_err(|e| e.to_string())?;
        let mut rows: Vec<EnvelopeRow> = fetches.iter().map(envelope_row).collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.uid));
        rows
    };
    let _ = session.logout();
    Ok(rows)
}

fn search_blocking(
    account: MailAccount,
    password: String,
    folder: String,
    query: String,
    limit: usize,
) -> Result<Vec<EnvelopeRow>, String> {
    let mut session = imap_connect(&account, &password)?;
    session.select(&folder).map_err(|e| format!("couldn't open {folder}: {e}"))?;
    let escaped = query.replace('\\', "\\\\").replace('"', "\\\"");
    let mut uids: Vec<u32> = session
        .uid_search(format!("TEXT \"{escaped}\""))
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids.truncate(limit);
    let rows = if uids.is_empty() {
        Vec::new()
    } else {
        let set = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let fetches = session.uid_fetch(&set, "(UID ENVELOPE FLAGS)").map_err(|e| e.to_string())?;
        let mut rows: Vec<EnvelopeRow> = fetches.iter().map(envelope_row).collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.uid));
        rows
    };
    let _ = session.logout();
    Ok(rows)
}

/// Flatten an HTML body to readable text.
///
/// The naive version — drop everything between `<` and `>` — keeps the
/// *contents* of `<style>` and `<script>`, which on an ordinary marketing email
/// means several kilobytes of CSS reaching the model ahead of the actual
/// message, and (because the body is capped) often instead of it. So those two
/// elements are skipped whole, and the handful of entities that survive the
/// round trip are decoded.
fn strip_html(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    // Byte indices are safe here: every byte examined is ASCII (`<`, `>`, tag
    // names), and non-ASCII bytes are only ever copied through as chars.
    while i < s.len() {
        if bytes[i] == b'<' {
            // `<style …>…</style>` / `<script …>…</script>`: skip to the end tag.
            let skip = ["style", "script"].into_iter().find(|tag| {
                lower[i + 1..]
                    .strip_prefix(tag)
                    .is_some_and(|rest| rest.starts_with([' ', '>', '\t', '\n', '\r']))
            });
            if let Some(tag) = skip {
                let close = format!("</{tag}");
                match lower[i..].find(&close) {
                    Some(rel) => {
                        i += rel;
                        // Fall through: the `</tag …>` itself is dropped below.
                    }
                    // Unterminated — the rest of the document is that element.
                    None => break,
                }
            }
            match lower[i..].find('>') {
                Some(rel) => {
                    i += rel + 1;
                    // A block-level boundary is whitespace, not a joined word.
                    out.push(' ');
                }
                None => break,
            }
            continue;
        }
        let ch = s[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    let text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    decode_entities(&text)
}

/// The named and numeric entities that actually turn up in mail bodies. Not a
/// full HTML entity table: anything rarer reads fine left as-is, and a wrong
/// guess would be worse than a literal `&thinsp;`.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let Some(end) = tail[..tail.len().min(12)].find(';') else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..end];
        let decoded = match entity {
            "amp" => Some("&".to_string()),
            "lt" => Some("<".to_string()),
            "gt" => Some(">".to_string()),
            "quot" => Some("\"".to_string()),
            "apos" | "#39" => Some("'".to_string()),
            "nbsp" => Some(" ".to_string()),
            "mdash" => Some("—".to_string()),
            "ndash" => Some("–".to_string()),
            "hellip" => Some("…".to_string()),
            e => e
                .strip_prefix('#')
                .and_then(|n| {
                    n.strip_prefix('x')
                        .or_else(|| n.strip_prefix('X'))
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| n.parse::<u32>().ok())
                })
                .and_then(char::from_u32)
                .map(String::from),
        };
        match decoded {
            Some(text) => {
                out.push_str(&text);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn addr_list(addrs: Option<&mail_parser::Address>) -> Vec<String> {
    let Some(addrs) = addrs else { return Vec::new() };
    addrs
        .iter()
        .filter_map(|a| a.address())
        .map(|a| a.to_string())
        .collect()
}

fn read_blocking(account: MailAccount, password: String, folder: String, uid: u32) -> Result<ReadResult, String> {
    let mut session = imap_connect(&account, &password)?;
    session.select(&folder).map_err(|e| format!("couldn't open {folder}: {e}"))?;
    // `BODY.PEEK[]`, never `RFC822`: a plain `RFC822` fetch is defined to set
    // `\Seen`, so asking the agent to read a message would silently mark it
    // read in the user's real mailbox. Reading here is a read.
    let fetches = session
        .uid_fetch(uid.to_string(), "(UID ENVELOPE BODY.PEEK[])")
        .map_err(|e| e.to_string())?;
    let fetch = fetches.iter().next().ok_or_else(|| format!("no message with id {uid}"))?;
    let row = envelope_row(fetch);
    let raw = fetch.body().ok_or("that message has no readable body")?;
    let message = mail_parser::MessageParser::default()
        .parse(raw)
        .ok_or("couldn't parse that message")?;

    let body = message
        .body_text(0)
        .map(|c| c.to_string())
        .or_else(|| message.body_html(0).map(|h| strip_html(&h)))
        .unwrap_or_default();
    let from_addr = message
        .from()
        .and_then(|f| f.first())
        .and_then(|a| a.address())
        .map(|a| a.to_string())
        .unwrap_or_default();
    let to_addrs = addr_list(message.to());
    let cc_addrs = addr_list(message.cc());
    let message_id = message.message_id().map(str::to_string);
    let attachments: Vec<String> = message
        .attachments()
        .map(|a| {
            let name = a.attachment_name().unwrap_or("attachment").to_string();
            let kb = a.contents().len().div_ceil(1024);
            format!("{name} ({kb} KB)")
        })
        .collect();

    let _ = session.logout();
    Ok(ReadResult {
        from_display: row.from,
        from_addr,
        to_addrs,
        cc_addrs,
        subject: row.subject,
        date: row.date,
        body,
        attachments,
        message_id,
    })
}

/// Threading headers — lettre has no built-in helper for `In-Reply-To`/
/// `References`, so two minimal `Header` impls carry the raw Message-ID
/// value through unchanged. Two distinct types (not one shared one) because
/// `Header`'s `name()` is an associated function with no `self`: one type
/// per header keeps the name fixed and correct regardless of how lettre
/// stores/keys a message's header set internally.
#[derive(Clone)]
struct InReplyTo(String);
impl lettre::message::header::Header for InReplyTo {
    fn name() -> lettre::message::header::HeaderName {
        lettre::message::header::HeaderName::new_from_ascii_str("In-Reply-To")
    }
    fn parse(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(InReplyTo(s.to_string()))
    }
    fn display(&self) -> lettre::message::header::HeaderValue {
        lettre::message::header::HeaderValue::new(Self::name(), self.0.clone())
    }
}

#[derive(Clone)]
struct References(String);
impl lettre::message::header::Header for References {
    fn name() -> lettre::message::header::HeaderName {
        lettre::message::header::HeaderName::new_from_ascii_str("References")
    }
    fn parse(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(References(s.to_string()))
    }
    fn display(&self) -> lettre::message::header::HeaderValue {
        lettre::message::header::HeaderValue::new(Self::name(), self.0.clone())
    }
}

#[allow(clippy::too_many_arguments)]
fn send_blocking(
    account: MailAccount,
    password: String,
    to: String,
    cc: Vec<String>,
    subject: String,
    body: String,
    reply_to_message_id: Option<String>,
) -> Result<(), String> {
    let mut builder = SmtpMessage::builder()
        .from(account.email.parse().map_err(|e| format!("bad from address: {e}"))?)
        .to(to.parse().map_err(|e| format!("bad to address ({to}): {e}"))?)
        .subject(subject);
    for addr in &cc {
        builder = builder.cc(addr.parse().map_err(|e| format!("bad cc address ({addr}): {e}"))?);
    }
    if let Some(mid) = reply_to_message_id {
        builder = builder.header(InReplyTo(mid.clone())).header(References(mid));
    }
    let email = builder
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| format!("couldn't build the message: {e}"))?;

    let mailer = smtp_transport(&account, password)?;
    mailer.send(&email).map_err(|e| format!("sending failed: {e}"))?;
    Ok(())
}

// ---- tool implementations ----

fn account_arg(args: &serde_json::Value) -> Option<&str> {
    args.get("account").and_then(|a| a.as_str())
}

fn render_envelopes(rows: &[EnvelopeRow], header: &str) -> serde_json::Value {
    serde_json::json!({
        "header": header,
        "items": rows.iter().map(|r| serde_json::json!({
            "id": r.uid.to_string(),
            "from": r.from,
            "subject": if r.subject.is_empty() { "(no subject)".to_string() } else { r.subject.clone() },
            "date": r.date,
            "unread": !r.seen,
        })).collect::<Vec<_>>(),
    })
}

fn envelopes_text(rows: &[EnvelopeRow]) -> String {
    if rows.is_empty() {
        return "No messages found.".to_string();
    }
    rows.iter()
        .map(|r| {
            format!(
                "id {} — {} — {}{}",
                r.uid,
                if r.subject.is_empty() { "(no subject)" } else { &r.subject },
                r.from,
                if r.seen { "" } else { " (unread)" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn list_mail(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let (account, password) = resolve_account(ctx.db, account_arg(args))?;
    let folder = folder_arg(args);
    let limit = limit_arg(args);
    let unread_only = args.get("unread_only").and_then(|v| v.as_bool()).unwrap_or(false);
    let label = account.label.clone();
    let folder2 = folder.clone();
    let rows = tokio::task::spawn_blocking(move || list_blocking(account, password, folder2, limit, unread_only))
        .await
        .map_err(|e| e.to_string())??;

    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "network", &format!("read {} messages from {label}", rows.len()));
    super::toolsets::set_step_note(ctx, format!("— read {} messages from {label}", rows.len()));

    let title = format!("{} in {folder} ({label})", if unread_only { "unread" } else { "recent" });
    super::toolsets::render_block(ctx, "collection", &title, &render_envelopes(&rows, &title));
    Ok(envelopes_text(&rows))
}

async fn search_mail(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let query = required(args, "query")?.to_string();
    let (account, password) = resolve_account(ctx.db, account_arg(args))?;
    let folder = folder_arg(args);
    let limit = limit_arg(args);
    let label = account.label.clone();
    let folder2 = folder.clone();
    let query2 = query.clone();
    let rows = tokio::task::spawn_blocking(move || search_blocking(account, password, folder2, query2, limit))
        .await
        .map_err(|e| e.to_string())??;

    let _ = ctx.db.log_activity(
        Some(ctx.conversation_id),
        "network",
        &format!("searched {label} for \"{query}\": {} hits", rows.len()),
    );
    let title = format!("\u{201c}{query}\u{201d} in {folder} ({label})");
    super::toolsets::render_block(ctx, "collection", &title, &render_envelopes(&rows, &title));
    Ok(envelopes_text(&rows))
}

async fn read_mail(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let id = required(args, "id")?;
    let uid: u32 = id.parse().map_err(|_| "that message id doesn't look right".to_string())?;
    let (account, password) = resolve_account(ctx.db, account_arg(args))?;
    let folder = folder_arg(args);
    let result =
        tokio::task::spawn_blocking(move || read_blocking(account, password, folder, uid))
            .await
            .map_err(|e| e.to_string())??;

    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "network", &format!("read a message from {}", result.from_display));
    super::toolsets::set_step_note(ctx, format!("— read a message from {}", result.from_display));

    let body = if result.body.chars().count() > BODY_CAP {
        result.body.chars().take(BODY_CAP).collect::<String>() + "…"
    } else {
        result.body
    };
    let wrapped = mark_untrusted(ctx, &format!("email from {}", result.from_display), &body);

    let attach_line = if result.attachments.is_empty() {
        String::new()
    } else {
        format!("\nAttachments (named only, not fetched): {}", result.attachments.join(", "))
    };
    Ok(format!(
        "From: {}\nSubject: {}\nDate: {}\n\n{}{attach_line}",
        result.from_display,
        if result.subject.is_empty() { "(no subject)" } else { &result.subject },
        result.date,
        wrapped
    ))
}

/// `MAIL-3`: sending is ask-first by construction. Renders the full message
/// as `proposed_text` in a small parseable header+body shape that
/// `parse_email_proposal` reads back at accept time — no schema change
/// needed to carry to/cc/subject/account alongside the body.
pub fn render_email_proposal(
    account_id: &str,
    to: &str,
    cc: Option<&str>,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
) -> String {
    let mut out = format!("Account: {account_id}\nTo: {to}\n");
    if let Some(cc) = cc.filter(|c| !c.trim().is_empty()) {
        out.push_str(&format!("Cc: {cc}\n"));
    }
    // Carried through the proposal, not recomputed on accept: the whole point
    // of ask-first is that what gets sent is what was reviewed, and a reply
    // that loses its threading headers starts a new thread in the recipient's
    // client — a silently different outcome from the one shown.
    if let Some(mid) = in_reply_to.filter(|m| !m.trim().is_empty()) {
        out.push_str(&format!("In-Reply-To: {mid}\n"));
    }
    out.push_str(&format!("Subject: {subject}\n\n{body}"));
    out
}

pub struct EmailProposal {
    pub account_id: String,
    pub to: String,
    pub cc: Option<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
}

/// Parse `render_email_proposal`'s output back into fields, at accept time.
///
/// A header line that doesn't parse is skipped rather than failing the whole
/// message — the user edited the body through a textarea, and one odd line
/// must not turn an approved send into "that couldn't be read back".
pub fn parse_email_proposal(text: &str) -> Option<EmailProposal> {
    let (header, body) = text.split_once("\n\n")?;
    let mut account_id = None;
    let mut to = None;
    let mut cc = None;
    let mut subject = None;
    let mut in_reply_to = None;
    for line in header.lines() {
        let Some((key, value)) = line.split_once(": ") else { continue };
        match key {
            "Account" => account_id = Some(value.to_string()),
            "To" => to = Some(value.to_string()),
            "Cc" => cc = Some(value.to_string()),
            "Subject" => subject = Some(value.to_string()),
            "In-Reply-To" => in_reply_to = Some(value.to_string()),
            _ => {}
        }
    }
    Some(EmailProposal {
        account_id: account_id?,
        to: to?,
        cc,
        subject: subject.unwrap_or_default(),
        body: body.to_string(),
        in_reply_to,
    })
}

/// Actually send an already-approved (or auto-rung) message. Used both by
/// `send_mail`/`reply_mail` at the `auto` rung and by
/// `resolve_change_proposal_cmd` when the user accepts an `email` proposal.
pub async fn send_now(
    db: &Db,
    account_id: &str,
    to: &str,
    cc: Option<&str>,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
) -> Result<(), String> {
    let account = db
        .get_mail_account(account_id)
        .map_err(|e| e.to_string())?
        .ok_or("that mail account no longer exists")?;
    // Approving a proposal is consent to send from *that* account; if it was
    // switched off in the meantime, that consent has been withdrawn.
    if !account.enabled {
        return Err(format!("{} is switched off — turn it back on to send this", account.label));
    }
    let password = crate::secrets::get_secret(crate::secrets::SERVICE_MAIL, &account.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no password stored for {}", account.label))?;
    let cc_list: Vec<String> = cc
        .map(|c| c.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let to = to.to_string();
    let subject = subject.to_string();
    let body = body.to_string();
    let in_reply_to = in_reply_to.map(str::to_string);
    tokio::task::spawn_blocking(move || {
        send_blocking(account, password, to, cc_list, subject, body, in_reply_to)
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn send_mail(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let to = required(args, "to")?;
    let subject = required(args, "subject")?;
    let body = required(args, "body")?;
    let cc = args.get("cc").and_then(|c| c.as_str());
    let (account, password) = resolve_account(ctx.db, account_arg(args))?;

    match autonomy_gate(ctx.db, "email_send") {
        Rung::Off => Ok("I'm not allowed to send mail right now.".to_string()),
        Rung::Ask => {
            let text = render_email_proposal(&account.id, to, cc, subject, body, None);
            let rationale = format!("send \"{subject}\" to {to}");
            let proposal = ctx
                .db
                .add_change_proposal("email", None, &text, &rationale, Some(&rationale))
                .map_err(|e| e.to_string())?;
            ctx.sink.emit(super::AgentEvent::Proposal {
                id: proposal.id,
                target: "email".to_string(),
                rationale,
            });
            Ok("Waiting for your approval.".to_string())
        }
        Rung::Auto => {
            let cc_list: Vec<String> = cc
                .map(|c| c.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            let (to_s, subject_s, body_s) = (to.to_string(), subject.to_string(), body.to_string());
            tokio::task::spawn_blocking(move || send_blocking(account.clone(), password, to_s, cc_list, subject_s, body_s, None))
                .await
                .map_err(|e| e.to_string())??;
            let _ = ctx.db.log_activity(Some(ctx.conversation_id), "network", &format!("sent mail to {to}"));
            ctx.sink.emit(super::AgentEvent::MailSent { to: to.to_string() });
            Ok(format!("Sent to {to}. There's no unsending."))
        }
    }
}

async fn reply_mail(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let id = required(args, "id")?;
    let body = required(args, "body")?;
    let reply_all = args.get("reply_all").and_then(|v| v.as_bool()).unwrap_or(false);
    let uid: u32 = id.parse().map_err(|_| "that message id doesn't look right".to_string())?;
    let (account, password) = resolve_account(ctx.db, account_arg(args))?;
    let folder = folder_arg(args);

    let account2 = account.clone();
    let original = tokio::task::spawn_blocking(move || read_blocking(account2, password.clone(), folder, uid))
        .await
        .map_err(|e| e.to_string())??;

    if original.from_addr.is_empty() {
        return Err("couldn't tell who that message was from".to_string());
    }
    let subject = if original.subject.to_lowercase().starts_with("re:") {
        original.subject.clone()
    } else {
        format!("Re: {}", original.subject)
    };
    let mut cc_list: Vec<String> = Vec::new();
    if reply_all {
        for addr in original.to_addrs.iter().chain(original.cc_addrs.iter()) {
            if !addr.eq_ignore_ascii_case(&original.from_addr)
                && !addr.eq_ignore_ascii_case(&account.email)
                && !cc_list.iter().any(|c| c.eq_ignore_ascii_case(addr))
            {
                cc_list.push(addr.clone());
            }
        }
    }
    let cc = if cc_list.is_empty() { None } else { Some(cc_list.join(", ")) };

    match autonomy_gate(ctx.db, "email_send") {
        Rung::Off => Ok("I'm not allowed to send mail right now.".to_string()),
        Rung::Ask => {
            let text = render_email_proposal(
                &account.id,
                &original.from_addr,
                cc.as_deref(),
                &subject,
                body,
                original.message_id.as_deref(),
            );
            let rationale = format!("reply to {}", original.from_addr);
            let proposal = ctx
                .db
                .add_change_proposal("email", None, &text, &rationale, Some(&rationale))
                .map_err(|e| e.to_string())?;
            ctx.sink.emit(super::AgentEvent::Proposal {
                id: proposal.id,
                target: "email".to_string(),
                rationale,
            });
            Ok("Waiting for your approval.".to_string())
        }
        Rung::Auto => {
            // Re-authenticate for the send leg: IMAP and SMTP are separate
            // sessions, and the password was already moved into the read call.
            let (_, password2) = resolve_account(ctx.db, Some(&account.id))?;
            let to = original.from_addr.clone();
            let (subject2, body2) = (subject.clone(), body.to_string());
            let message_id = original.message_id.clone();
            tokio::task::spawn_blocking(move || {
                send_blocking(account.clone(), password2, to, cc_list, subject2, body2, message_id)
            })
            .await
            .map_err(|e| e.to_string())??;
            let _ = ctx.db.log_activity(Some(ctx.conversation_id), "network", &format!("replied to {}", original.from_addr));
            ctx.sink.emit(super::AgentEvent::MailSent { to: original.from_addr.clone() });
            Ok(format!("Sent to {}. There's no unsending.", original.from_addr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_and_script_bodies_never_reach_the_model() {
        let html = "<html><head><style type=\"text/css\">.x{color:red;font-size:12px}</style>\
                    <script>var a = 1; if (a < 2) { track(); }</script></head>\
                    <body><p>Hello there.</p><p>Second line.</p></body></html>";
        let text = strip_html(html);
        assert!(!text.contains("color:red"), "CSS leaked into the body: {text}");
        assert!(!text.contains("track()"), "JS leaked into the body: {text}");
        assert_eq!(text, "Hello there. Second line.");
    }

    #[test]
    fn an_unterminated_style_doesnt_swallow_everything_silently() {
        // Damaged markup: better to lose the tail than to emit a stylesheet.
        let text = strip_html("<p>Kept.</p><style>.a{}");
        assert!(text.starts_with("Kept."), "got {text}");
        assert!(!text.contains(".a{}"));
    }

    #[test]
    fn entities_are_decoded_including_numeric_ones() {
        assert_eq!(strip_html("<p>Tom &amp; Jerry&nbsp;&mdash; 5 &lt; 6</p>"), "Tom & Jerry — 5 < 6");
        assert_eq!(decode_entities("caf&#233; &#x41;"), "café A");
        // A lone ampersand, and an unknown entity, survive untouched.
        assert_eq!(decode_entities("a & b &thinsp; c"), "a & b &thinsp; c");
    }

    #[test]
    fn tags_become_word_boundaries_not_joins() {
        assert_eq!(strip_html("<td>one</td><td>two</td>"), "one two");
    }

    #[test]
    fn security_parses_and_defaults_to_implicit_tls() {
        assert_eq!(Security::parse("starttls"), Security::StartTls);
        assert_eq!(Security::parse("STARTTLS"), Security::StartTls);
        assert_eq!(Security::parse("tls"), Security::Tls);
        // Anything unrecognized must not silently downgrade to plaintext-first.
        assert_eq!(Security::parse(""), Security::Tls);
        assert_eq!(Security::parse("nonsense"), Security::Tls);
    }

    #[test]
    fn only_loopback_relaxes_certificate_checks() {
        assert!(is_loopback("127.0.0.1"));
        assert!(is_loopback("localhost"));
        assert!(is_loopback("::1"));
        assert!(is_loopback("[::1]"));
        assert!(!is_loopback("imap.gmail.com"));
        // The check is not a substring match: an attacker-controlled host
        // that merely contains "localhost" must stay strict.
        assert!(!is_loopback("localhost.evil.example"));
        assert!(!is_loopback("notlocalhost"));
    }

    /// `MAIL-3`: what the user approved is what gets sent — including the
    /// threading headers, which only the proposal can carry across the gap
    /// between drafting the reply and accepting it.
    #[test]
    fn a_reply_proposal_round_trips_its_threading_header() {
        let text = render_email_proposal(
            "acct-1",
            "her@example.com",
            Some("cc@example.com"),
            "Re: lunch",
            "Sounds good.",
            Some("<abc123@example.com>"),
        );
        let parsed = parse_email_proposal(&text).expect("should parse");
        assert_eq!(parsed.account_id, "acct-1");
        assert_eq!(parsed.to, "her@example.com");
        assert_eq!(parsed.cc.as_deref(), Some("cc@example.com"));
        assert_eq!(parsed.subject, "Re: lunch");
        assert_eq!(parsed.body, "Sounds good.");
        assert_eq!(parsed.in_reply_to.as_deref(), Some("<abc123@example.com>"));
    }

    #[test]
    fn a_fresh_send_has_no_threading_header_and_still_round_trips() {
        let text = render_email_proposal("acct-1", "her@example.com", None, "Hello", "Body\n\nwith a blank line.", None);
        let parsed = parse_email_proposal(&text).unwrap();
        assert!(parsed.cc.is_none());
        assert!(parsed.in_reply_to.is_none());
        assert_eq!(parsed.body, "Body\n\nwith a blank line.", "only the first blank line splits header from body");
    }

    /// The user edits the body in a textarea before approving; a line in what
    /// they typed must never be able to fail the parse of the whole message.
    #[test]
    fn an_odd_line_in_an_edited_body_doesnt_break_the_parse() {
        let text = "Account: a\nTo: b@example.com\nnot-a-header-line\nSubject: hi\n\nbody";
        let parsed = parse_email_proposal(text).expect("skips what it can't read");
        assert_eq!(parsed.subject, "hi");
        assert_eq!(parsed.body, "body");
    }
}
