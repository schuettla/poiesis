//! REM-5, REM-6, REM-7: the Telegram bot as a way in. Plan, II.1 and IV.2.
//!
//! Plain `reqwest` calls to the Bot API, no new dependency. The desktop only makes outgoing
//! connections: one long poll (`getUpdates`) and the sends. Only a paired Telegram id is answered; every
//! other message gets silence, except while a pairing code is on the desktop screen, when the person's
//! own message with the code pairs them.
//!
//! The rules live in `Ctx`, which talks to two traits: `TelegramApi` (the network) and `Host` (the
//! gateway). The tests give it fakes of both, so nothing here needs the network or a window to be tested.
//! `AppHost` and `Telegram` at the bottom are the thin parts that hold the app.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Notify};
use tokio::task::JoinHandle;

use super::render::{self, FileOut, Fold, Out};
use super::{Gateway, Intake};
use crate::agent::turn::TurnAttachment;
use crate::db::Db;

/// The transport name in the senders list.
pub const TRANSPORT: &str = "telegram";
/// Where the bot token lives: the credential store, service `poiesis-remote`.
pub const TOKEN_ACCOUNT: &str = "telegram-token";
/// The last update that was handed on, so a restart does not replay it.
pub const OFFSET_KEY: &str = "remote.telegram.offset";
/// The bot the offset belongs to. Update numbers are counted per bot.
const BOT_KEY: &str = "remote.telegram.bot";

/// What Telegram lets a bot download (`getFile`).
const MAX_DOWNLOAD: u64 = 20 * 1024 * 1024;
/// What Telegram lets a bot send as a document, and as a photo.
const MAX_DOCUMENT: u64 = 50 * 1024 * 1024;
const MAX_PHOTO: u64 = 10 * 1024 * 1024;
/// How a sender's pairing attempt may look: a code, maybe after `/start`.
const MAX_ATTEMPT: usize = 64;

// ---- Bot API shapes (only what is read) ----

#[derive(Debug, Clone, Deserialize, Default)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Chat {
    pub id: i64,
    #[serde(default, rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PhotoSize {
    pub file_id: String,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub width: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Document {
    pub file_id: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReplyTo {
    pub message_id: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Message {
    pub message_id: i64,
    #[serde(default)]
    pub from: Option<User>,
    #[serde(default)]
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub photo: Vec<PhotoSize>,
    #[serde(default)]
    pub document: Option<Document>,
    #[serde(default)]
    pub reply_to_message: Option<Box<ReplyTo>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub data: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub callback_query: Option<CallbackQuery>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Me {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub first_name: String,
}

impl Me {
    /// How the bot is named to the person: `@name`, or its first name.
    pub fn name(&self) -> String {
        self.username.as_deref().map(|u| format!("@{u}")).unwrap_or_else(|| self.first_name.clone())
    }
}

/// One inline button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub text: String,
    pub data: String,
}

/// Rows of buttons.
pub type Keyboard = Vec<Vec<Button>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Photo,
    Document,
}

/// A failed call, in the words Telegram gave (its `description`), or ours when it never answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub text: String,
    /// Telegram's own "wait this many seconds".
    pub retry_after: Option<u64>,
}

impl ApiError {
    pub fn new(text: impl Into<String>) -> Self {
        ApiError { text: text.into(), retry_after: None }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

/// The calls the adapter makes. The real one is `HttpApi`; the tests have a fake.
#[async_trait]
pub trait TelegramApi: Send + Sync {
    async fn get_me(&self) -> Result<Me, ApiError>;
    async fn delete_webhook(&self) -> Result<(), ApiError>;
    async fn get_updates(&self, offset: i64, timeout_secs: u64) -> Result<Vec<Update>, ApiError>;
    /// The new message's id.
    async fn send_message(&self, chat: i64, html: &str, keyboard: Option<Keyboard>) -> Result<i64, ApiError>;
    /// Edits the text and, because no keyboard is given, takes the buttons away.
    async fn edit_message_text(&self, chat: i64, message: i64, html: &str) -> Result<(), ApiError>;
    async fn answer_callback(&self, callback_id: &str, text: Option<&str>) -> Result<(), ApiError>;
    async fn send_typing(&self, chat: i64) -> Result<(), ApiError>;
    async fn set_my_commands(&self, commands: &[(&str, &str)]) -> Result<(), ApiError>;
    /// `getFile`, then the download.
    async fn download(&self, file_id: &str) -> Result<Vec<u8>, ApiError>;
    async fn send_file(&self, chat: i64, kind: FileKind, name: &str, bytes: Vec<u8>) -> Result<(), ApiError>;
}

// ---- the real API ----

pub struct HttpApi {
    client: reqwest::Client,
    token: String,
}

#[derive(Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    parameters: Option<Value>,
}

impl HttpApi {
    pub fn new(token: &str) -> Self {
        let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).build().unwrap_or_default();
        HttpApi { client, token: token.trim().to_string() }
    }

    fn url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{method}", self.token)
    }

    /// A transport failure in words, with the request's address (which holds the token) left out.
    fn unreachable(e: reqwest::Error) -> ApiError {
        ApiError::new(format!("I could not reach Telegram. {}", e.without_url()))
    }

    async fn read(resp: reqwest::Response) -> Result<Value, ApiError> {
        let status = resp.status();
        let body = resp.text().await.map_err(Self::unreachable)?;
        let Ok(env) = serde_json::from_str::<Envelope>(&body) else {
            return Err(ApiError::new(format!("Telegram answered with status {} and something I could not read.", status.as_u16())));
        };
        if env.ok {
            return Ok(env.result);
        }
        Err(ApiError {
            text: env.description.unwrap_or_else(|| format!("Telegram answered with status {}.", status.as_u16())),
            retry_after: env.parameters.and_then(|p| p["retry_after"].as_u64()),
        })
    }

    async fn call(&self, method: &str, body: Value, wait: Duration) -> Result<Value, ApiError> {
        let resp = self.client.post(self.url(method)).timeout(wait).json(&body).send().await.map_err(Self::unreachable)?;
        Self::read(resp).await
    }

    fn quick() -> Duration {
        Duration::from_secs(30)
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ApiError> {
    serde_json::from_value(value).map_err(|_| ApiError::new("Telegram sent an answer I could not read."))
}

#[async_trait]
impl TelegramApi for HttpApi {
    async fn get_me(&self) -> Result<Me, ApiError> {
        parse(self.call("getMe", json!({}), Self::quick()).await?)
    }

    async fn delete_webhook(&self) -> Result<(), ApiError> {
        self.call("deleteWebhook", json!({}), Self::quick()).await.map(|_| ())
    }

    async fn get_updates(&self, offset: i64, timeout_secs: u64) -> Result<Vec<Update>, ApiError> {
        let body = json!({ "offset": offset, "timeout": timeout_secs, "allowed_updates": ["message", "callback_query"] });
        parse(self.call("getUpdates", body, Duration::from_secs(timeout_secs + 15)).await?)
    }

    async fn send_message(&self, chat: i64, html: &str, keyboard: Option<Keyboard>) -> Result<i64, ApiError> {
        let mut body = json!({ "chat_id": chat, "text": html, "parse_mode": "HTML", "link_preview_options": { "is_disabled": true } });
        if let Some(rows) = keyboard {
            let rows: Vec<Vec<Value>> = rows
                .into_iter()
                .map(|row| row.into_iter().map(|b| json!({ "text": b.text, "callback_data": b.data })).collect())
                .collect();
            body["reply_markup"] = json!({ "inline_keyboard": rows });
        }
        let sent = self.call("sendMessage", body, Self::quick()).await?;
        sent["message_id"].as_i64().ok_or_else(|| ApiError::new("Telegram sent an answer I could not read."))
    }

    async fn edit_message_text(&self, chat: i64, message: i64, html: &str) -> Result<(), ApiError> {
        let body = json!({ "chat_id": chat, "message_id": message, "text": html, "parse_mode": "HTML" });
        self.call("editMessageText", body, Self::quick()).await.map(|_| ())
    }

    async fn answer_callback(&self, callback_id: &str, text: Option<&str>) -> Result<(), ApiError> {
        let mut body = json!({ "callback_query_id": callback_id });
        if let Some(text) = text {
            body["text"] = json!(text);
        }
        self.call("answerCallbackQuery", body, Self::quick()).await.map(|_| ())
    }

    async fn send_typing(&self, chat: i64) -> Result<(), ApiError> {
        self.call("sendChatAction", json!({ "chat_id": chat, "action": "typing" }), Self::quick()).await.map(|_| ())
    }

    async fn set_my_commands(&self, commands: &[(&str, &str)]) -> Result<(), ApiError> {
        let list: Vec<Value> = commands.iter().map(|(c, d)| json!({ "command": c, "description": d })).collect();
        self.call("setMyCommands", json!({ "commands": list }), Self::quick()).await.map(|_| ())
    }

    async fn download(&self, file_id: &str) -> Result<Vec<u8>, ApiError> {
        let file = self.call("getFile", json!({ "file_id": file_id }), Self::quick()).await?;
        if file["file_size"].as_u64().is_some_and(|n| n > MAX_DOWNLOAD) {
            return Err(ApiError::new("That file is bigger than the 20 MB Telegram lets me take."));
        }
        let path = file["file_path"].as_str().ok_or_else(|| ApiError::new("Telegram did not say where the file is."))?;
        let url = format!("https://api.telegram.org/file/bot{}/{path}", self.token);
        let resp = self.client.get(url).timeout(Duration::from_secs(120)).send().await.map_err(Self::unreachable)?;
        if !resp.status().is_success() {
            return Err(ApiError::new(format!("Telegram answered with status {} for the file.", resp.status().as_u16())));
        }
        resp.bytes().await.map(|b| b.to_vec()).map_err(Self::unreachable)
    }

    async fn send_file(&self, chat: i64, kind: FileKind, name: &str, bytes: Vec<u8>) -> Result<(), ApiError> {
        let (method, field) = match kind {
            FileKind::Photo => ("sendPhoto", "photo"),
            FileKind::Document => ("sendDocument", "document"),
        };
        let part = reqwest::multipart::Part::bytes(bytes).file_name(name.to_string());
        let form = reqwest::multipart::Form::new().text("chat_id", chat.to_string()).part(field, part);
        let resp = self
            .client
            .post(self.url(method))
            .timeout(Duration::from_secs(180))
            .multipart(form)
            .send()
            .await
            .map_err(Self::unreachable)?;
        Self::read(resp).await.map(|_| ())
    }
}

// ---- the gateway, as the adapter sees it ----

/// Why a file could not be read for sending.
#[derive(Debug, PartialEq, Eq)]
pub enum MediaError {
    TooBig,
    Unavailable,
}

/// What the adapter needs from the app. `AppHost` is the real one.
#[async_trait]
pub trait Host: Send + Sync {
    fn enabled(&self) -> bool;
    fn is_paired(&self, id: &str) -> bool;
    /// A pairing code is on the desktop screen right now.
    fn pairing_open(&self) -> bool;
    /// `Err` is a line for the person.
    fn pair(&self, id: &str, label: &str, code: &str) -> Result<(), String>;
    async fn message(
        &self,
        id: &str,
        text: String,
        attachments: Vec<TurnAttachment>,
        events: mpsc::UnboundedSender<Value>,
    ) -> Intake;
    fn answer(&self, id: &str, request_id: &str, allow: bool) -> Result<(), String>;
    fn stop(&self, id: &str) -> bool;
    fn new_chat(&self, id: &str) -> Result<(), String>;
    fn open_chat(&self, id: &str, conversation_id: &str) -> Result<String, String>;
    /// The sender's last chats, newest first: `(id, title)`.
    fn chats(&self, id: &str) -> Vec<(String, String)>;
    async fn who(&self, id: &str) -> String;
    /// A thing that waits for the person, answered from here. `text` is the answer to a question.
    fn waiting_answer(&self, proposal_id: &str, accept: bool, text: Option<&str>) -> Result<(), String>;
    fn offset(&self) -> i64;
    fn save_offset(&self, offset: i64);
    async fn accept_file(&self, name: &str, mime: Option<&str>, bytes: Vec<u8>) -> Result<TurnAttachment, String>;
    fn read_media(&self, path: &str, max: u64) -> Result<Vec<u8>, MediaError>;
}

// ---- timing ----

/// The waits, so a test does not have to sit through the real ones.
#[derive(Clone, Copy)]
pub struct Timing {
    /// Between failed polls: after the first failure, and after every later one (plan: 5 s, then 30 s).
    pub backoff: [Duration; 2],
    /// Telegram forgets `typing` after about 5 seconds.
    pub typing: Duration,
    /// At most one message per second to one chat.
    pub spacing: Duration,
    /// The long poll.
    pub poll_secs: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            backoff: [Duration::from_secs(5), Duration::from_secs(30)],
            typing: Duration::from_secs(4),
            spacing: Duration::from_secs(1),
            poll_secs: 30,
        }
    }
}

impl Timing {
    /// How long to wait after the `failures`-th failure in a row (1 is the first). Telegram's own
    /// "retry after" wins when it is longer.
    pub fn backoff_for(&self, failures: u32, retry_after: Option<u64>) -> Duration {
        let ours = self.backoff[(failures.max(1) as usize - 1).min(1)];
        retry_after.map_or(ours, |s| ours.max(Duration::from_secs(s)))
    }
}

/// One message per second to one chat: the next send waits for its turn.
pub struct Pacer {
    gap: Duration,
    next: Mutex<HashMap<i64, tokio::time::Instant>>,
}

impl Pacer {
    pub fn new(gap: Duration) -> Self {
        Pacer { gap, next: Mutex::default() }
    }

    pub async fn wait(&self, chat: i64) {
        let at = {
            let mut next = self.next.lock().unwrap();
            let now = tokio::time::Instant::now();
            let at = next.get(&chat).map_or(now, |n| (*n).max(now));
            next.insert(chat, at + self.gap);
            at
        };
        tokio::time::sleep_until(at).await;
    }
}

// ---- the adapter ----

/// A question sent to the chat, so its buttons can be taken away when it is answered or gone.
#[derive(Clone)]
struct Card {
    chat: i64,
    message: i64,
    html: String,
}

/// What a turn leaves for its end: the files it made (each artifact once, as it is last), and the
/// questions it asked.
#[derive(Default)]
struct TurnOut {
    files: Vec<FileOut>,
    asked: Vec<String>,
}

/// What the bot says in answer to `/start` once paired, and to `/new` and the others.
const COMMANDS: [(&str, &str); 4] = [
    ("new", "Start a new chat"),
    ("chats", "Pick one of your last chats"),
    ("stop", "Stop what I am doing"),
    ("who", "Which model and chat I am using"),
];

/// Told after each poll: the error line, or `None` when it worked.
pub type Report = Arc<dyn Fn(Option<&str>) + Send + Sync>;

#[derive(Clone)]
pub struct Ctx {
    api: Arc<dyn TelegramApi>,
    host: Arc<dyn Host>,
    pacer: Arc<Pacer>,
    timing: Timing,
    cards: Arc<Mutex<HashMap<String, Card>>>,
    /// Waiting pings that want a typed answer: `(chat, message)` to the entry.
    asks: Arc<Mutex<HashMap<(i64, i64), String>>>,
    /// Per chat, the signal that the last message was handed to the gateway. The next one waits for it,
    /// so two quick messages reach the gateway in the order they were written.
    handed: Arc<Mutex<HashMap<i64, oneshot::Receiver<()>>>>,
    report: Report,
}

/// A callback's data, read.
#[derive(Debug, PartialEq, Eq)]
enum Tap<'a> {
    Question { request_id: &'a str, allow: bool },
    Waiting { id: &'a str, accept: bool },
    Chat(&'a str),
}

fn tap(data: &str) -> Option<Tap<'_>> {
    let mut parts = data.splitn(2, ':');
    let (kind, rest) = (parts.next()?, parts.next()?);
    match kind {
        "p" | "w" => {
            let (id, answer) = rest.rsplit_once(':')?;
            let yes = match answer {
                "y" => true,
                "n" => false,
                _ => return None,
            };
            Some(if kind == "p" { Tap::Question { request_id: id, allow: yes } } else { Tap::Waiting { id, accept: yes } })
        }
        "c" => Some(Tap::Chat(rest)),
        _ => None,
    }
}

/// `/new@Poiesis_bot extra` is the command `new`.
fn command(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('/')?;
    let (word, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    Some((word.split('@').next().unwrap_or(word), tail.trim()))
}

fn sender_label(user: &User) -> String {
    if !user.first_name.trim().is_empty() {
        user.first_name.trim().to_string()
    } else {
        user.username.clone().unwrap_or_else(|| "Telegram".into())
    }
}

fn file_name_for(title: &str, kind: &str) -> String {
    let ext = match kind {
        "markdown" => "md",
        "html" => "html",
        "svg" => "svg",
        "csv" => "csv",
        "json" => "json",
        _ => "txt",
    };
    let stem: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    format!("{}.{ext}", if stem.is_empty() { "file".to_string() } else { stem.chars().take(60).collect() })
}

impl Ctx {
    pub fn new(
        api: Arc<dyn TelegramApi>,
        host: Arc<dyn Host>,
        timing: Timing,
        report: Report,
    ) -> Self {
        Ctx {
            api,
            host,
            pacer: Arc::new(Pacer::new(timing.spacing)),
            timing,
            cards: Arc::default(),
            asks: Arc::default(),
            handed: Arc::default(),
            report,
        }
    }

    /// One message to a chat, spaced from the one before. `None` when Telegram refused it.
    async fn say(&self, chat: i64, html: &str, keyboard: Option<Keyboard>) -> Option<i64> {
        self.pacer.wait(chat).await;
        match self.api.send_message(chat, html, keyboard).await {
            Ok(id) => Some(id),
            Err(e) => {
                eprintln!("remote: telegram did not take a message: {e}");
                None
            }
        }
    }

    async fn plain(&self, chat: i64, text: &str) {
        self.say(chat, &render::escape_html(text), None).await;
    }

    /// Hand one update to its own task, so the poll goes on while a turn runs. A message waits until the
    /// one before it from the same chat is with the gateway: tasks do not start in a fixed order.
    pub fn process(&self, update: Update) -> Option<JoinHandle<()>> {
        let ctx = self.clone();
        if let Some(q) = update.callback_query {
            return Some(tokio::spawn(async move { ctx.callback(q).await }));
        }
        let m = update.message?;
        let (handed, next) = oneshot::channel();
        let before = self.handed.lock().unwrap().insert(m.chat.id, next);
        Some(tokio::spawn(async move {
            if let Some(before) = before {
                // `Err` too: the message before it ended without a turn.
                let _ = before.await;
            }
            ctx.message(m, handed).await
        }))
    }

    /// The long poll. Runs until it is dropped (the switch went off, or the token changed).
    pub async fn poll(&self) {
        let mut failures = 0u32;
        // One poller only: a webhook would make `getUpdates` refuse.
        loop {
            match self.api.delete_webhook().await {
                Ok(()) => break,
                Err(e) => {
                    failures += 1;
                    (self.report)(Some(&e.text));
                    tokio::time::sleep(self.timing.backoff_for(failures, e.retry_after)).await;
                }
            }
        }
        let _ = self.api.set_my_commands(&COMMANDS).await;
        let mut offset = self.host.offset();
        failures = 0;
        loop {
            match self.api.get_updates(offset, self.timing.poll_secs).await {
                Ok(updates) => {
                    failures = 0;
                    (self.report)(None);
                    for update in updates {
                        offset = offset.max(update.update_id + 1);
                        self.process(update);
                    }
                    self.host.save_offset(offset);
                }
                Err(e) => {
                    failures += 1;
                    (self.report)(Some(&e.text));
                    tokio::time::sleep(self.timing.backoff_for(failures, e.retry_after)).await;
                }
            }
        }
    }

    /// `handed` is fired once the message is with the gateway, or dropped when it never gets there.
    async fn message(&self, m: Message, handed: oneshot::Sender<()>) {
        let Some(from) = m.from.clone().filter(|u| !u.is_bot) else { return };
        // The bot answers a person in a chat of their own, never a group.
        if m.chat.kind != "private" || !self.host.enabled() {
            return;
        }
        let (chat, id) = (m.chat.id, from.id.to_string());
        let text = m.text.clone().or_else(|| m.caption.clone()).unwrap_or_default().trim().to_string();

        if !self.host.is_paired(&id) {
            // Silence, unless a code is on the desktop screen and this is the person's own try.
            if self.host.pairing_open() && !text.is_empty() && text.len() <= MAX_ATTEMPT {
                let attempt = text.strip_prefix("/start").map(str::trim).unwrap_or(&text);
                // Telegram sends a bare `/start` when the person first opens the bot. It is not a try.
                if attempt.is_empty() {
                    return self.plain(chat, "Send me the code from your desktop.").await;
                }
                match self.host.pair(&id, &sender_label(&from), attempt) {
                    Ok(()) => {
                        self.plain(chat, "Paired. Write to me here as you would in the app. /new starts a new chat.").await
                    }
                    Err(words) => self.plain(chat, &words).await,
                }
            }
            return;
        }

        if let Some((word, _)) = command(&text) {
            return self.command(chat, &id, word).await;
        }
        // A reply to a waiting question is its answer.
        if let Some(entry) = m.reply_to_message.as_ref().and_then(|r| self.asks.lock().unwrap().get(&(chat, r.message_id)).cloned()) {
            if !text.is_empty() {
                let said = match self.host.waiting_answer(&entry, true, Some(&text)) {
                    Ok(()) => {
                        self.asks.lock().unwrap().retain(|_, v| *v != entry);
                        "Sent. I passed your answer on.".to_string()
                    }
                    Err(words) => words,
                };
                return self.plain(chat, &said).await;
            }
        }

        let Ok(attachments) = self.take_files(chat, &m).await else { return };
        if text.is_empty() && attachments.is_empty() {
            return;
        }
        self.turn(chat, &id, text, attachments, handed).await;
    }

    async fn command(&self, chat: i64, id: &str, word: &str) {
        match word {
            "start" => self.plain(chat, "I am here. Write to me as you would in the app. /new starts a new chat.").await,
            "new" => match self.host.new_chat(id) {
                Ok(()) => self.plain(chat, "Started a new chat.").await,
                Err(words) => self.plain(chat, &words).await,
            },
            "stop" => {
                let line = if self.host.stop(id) { "Stopped." } else { "I am not doing anything right now." };
                self.plain(chat, line).await;
            }
            "who" => {
                let line = self.host.who(id).await;
                self.plain(chat, &line).await;
            }
            "chats" => {
                let chats = self.host.chats(id);
                if chats.is_empty() {
                    return self.plain(chat, "You have no chats yet. Write to me to start one.").await;
                }
                let rows: Keyboard = chats
                    .into_iter()
                    .map(|(conversation, title)| {
                        vec![Button { text: title.chars().take(40).collect(), data: format!("c:{conversation}") }]
                    })
                    .collect();
                self.say(chat, "Your last chats. Tap one to carry on in it.", Some(rows)).await;
            }
            _ => self.plain(chat, "I know /new, /chats, /stop and /who.").await,
        }
    }

    /// The photo or document of a message, ready for the turn. `Err` after the person was told why not.
    async fn take_files(&self, chat: i64, m: &Message) -> Result<Vec<TurnAttachment>, ()> {
        let mut wanted: Vec<(String, Option<String>, String, Option<u64>)> = Vec::new();
        if let Some(photo) = m.photo.iter().max_by_key(|p| (p.file_size.unwrap_or(0), p.width)) {
            wanted.push(("photo.jpg".into(), Some("image/jpeg".into()), photo.file_id.clone(), photo.file_size));
        }
        if let Some(doc) = &m.document {
            let name = doc.file_name.clone().unwrap_or_else(|| "file".into());
            wanted.push((name, doc.mime_type.clone(), doc.file_id.clone(), doc.file_size));
        }
        let mut out = Vec::new();
        for (name, mime, file_id, size) in wanted {
            if size.is_some_and(|n| n > MAX_DOWNLOAD) {
                self.plain(chat, "That file is bigger than the 20 MB Telegram lets me take.").await;
                return Err(());
            }
            let bytes = match self.api.download(&file_id).await {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.plain(chat, &e.text).await;
                    return Err(());
                }
            };
            match self.host.accept_file(&name, mime.as_deref(), bytes).await {
                Ok(attachment) => out.push(attachment),
                Err(words) => {
                    self.plain(chat, &words).await;
                    return Err(());
                }
            }
        }
        Ok(out)
    }

    /// A turn from this chat: typing while it works, questions as they come, the answer at the end.
    async fn turn(&self, chat: i64, id: &str, text: String, attachments: Vec<TurnAttachment>, handed: oneshot::Sender<()>) {
        let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
        let host = self.host.clone();
        let sender = id.to_string();
        let mut work = Box::pin(async move { host.message(&sender, text, attachments, tx).await });
        let mut fold = Fold::default();
        let mut turn = TurnOut::default();
        let mut handed = Some(handed);
        let mut typing = tokio::time::interval(self.timing.typing);
        let intake = loop {
            tokio::select! {
                done = &mut work => break done,
                Some(event) = rx.recv() => {
                    // The turn has begun: the next message from this chat may go to the gateway now.
                    if let Some(h) = handed.take() {
                        let _ = h.send(());
                    }
                    self.step(chat, fold.event(&event), &mut turn).await
                }
                _ = typing.tick() => { let _ = self.api.send_typing(chat).await; }
            }
        };
        drop(handed);
        // Whatever was said just before the end.
        while let Ok(event) = rx.try_recv() {
            self.step(chat, fold.event(&event), &mut turn).await;
        }
        match intake {
            Intake::Ran => {
                for piece in fold.finish() {
                    self.say(chat, &piece, None).await;
                }
            }
            Intake::Steered => self.plain(chat, "Got it. I will take that into account.").await,
            Intake::Refused(refusal) => {
                if let Some(words) = refusal.words() {
                    self.plain(chat, &words).await;
                }
            }
        }
        for file in turn.files {
            self.send_out(chat, file).await;
        }
        // The turn is over, so nothing it asked is still open. A card that was not closed loses its buttons.
        for request_id in turn.asked {
            let card = self.cards.lock().unwrap().remove(&request_id);
            if let Some(card) = card {
                let _ = self.api.edit_message_text(card.chat, card.message, &format!("{}\n\n{}", card.html, render::GONE_LINE)).await;
            }
        }
    }

    /// One event of a running turn: a question goes out now, a file waits for the end of the turn.
    async fn step(&self, chat: i64, out: Option<Out>, turn: &mut TurnOut) {
        match out {
            Some(Out::File(file)) => {
                turn.files.retain(|f| f.id.is_empty() || f.id != file.id);
                turn.files.push(file);
            }
            Some(Out::Question { request_id, html }) => {
                turn.asked.push(request_id.clone());
                self.apply(chat, Some(Out::Question { request_id, html })).await;
            }
            other => self.apply(chat, other).await,
        }
    }

    async fn apply(&self, chat: i64, out: Option<Out>) {
        match out {
            None => {}
            Some(Out::Question { request_id, html }) => {
                let keyboard = vec![vec![
                    Button { text: "Allow".into(), data: format!("p:{request_id}:y") },
                    Button { text: "Not now".into(), data: format!("p:{request_id}:n") },
                ]];
                if let Some(message) = self.say(chat, &html, Some(keyboard)).await {
                    self.cards.lock().unwrap().insert(request_id, Card { chat, message, html });
                }
            }
            Some(Out::Gone { request_id }) => {
                let card = self.cards.lock().unwrap().remove(&request_id);
                if let Some(card) = card {
                    let _ = self.api.edit_message_text(card.chat, card.message, &format!("{}\n\n{}", card.html, render::GONE_LINE)).await;
                }
            }
            Some(Out::Line(line)) => self.plain(chat, &line).await,
            Some(Out::File(file)) => self.send_out(chat, file).await,
        }
    }

    /// A file the turn made, sent to the chat. Over Telegram's limits it stays on the desktop.
    async fn send_out(&self, chat: i64, file: FileOut) {
        const TOO_BIG: &str = "It is too big for Telegram. It is on your desktop.";
        let (name, bytes, picture) = if matches!(file.kind.as_str(), "image" | "video") {
            let bytes = match self.host.read_media(&file.content, MAX_DOCUMENT) {
                Ok(bytes) => bytes,
                Err(MediaError::TooBig) => return self.plain(chat, TOO_BIG).await,
                Err(MediaError::Unavailable) => return,
            };
            let name = Path::new(&file.content).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
            (name, bytes, file.kind == "image")
        } else {
            (file_name_for(&file.title, &file.kind), file.content.into_bytes(), false)
        };
        if bytes.len() as u64 > MAX_DOCUMENT {
            return self.plain(chat, TOO_BIG).await;
        }
        self.pacer.wait(chat).await;
        let sent = if picture && bytes.len() as u64 <= MAX_PHOTO {
            // A picture Telegram will not show as a photo (odd shape) goes as a document instead.
            match self.api.send_file(chat, FileKind::Photo, &name, bytes.clone()).await {
                Ok(()) => Ok(()),
                Err(_) => self.api.send_file(chat, FileKind::Document, &name, bytes).await,
            }
        } else {
            self.api.send_file(chat, FileKind::Document, &name, bytes).await
        };
        if let Err(e) = sent {
            eprintln!("remote: telegram did not take a file: {e}");
        }
    }

    /// A tap on a button. The query is always answered, so the client stops its spinner.
    async fn callback(&self, q: CallbackQuery) {
        let id = q.from.id.to_string();
        if !self.host.enabled() || !self.host.is_paired(&id) {
            let _ = self.api.answer_callback(&q.id, None).await;
            return;
        }
        let place = q.message.as_ref().map(|m| (m.chat.id, m.message_id));
        let Some(action) = q.data.as_deref().and_then(tap) else {
            let _ = self.api.answer_callback(&q.id, None).await;
            return;
        };
        match action {
            Tap::Question { request_id, allow } => match self.host.answer(&id, request_id, allow) {
                Ok(()) => {
                    let line = render::answered_line(allow);
                    let _ = self.api.answer_callback(&q.id, Some(line)).await;
                    let card = self.cards.lock().unwrap().remove(request_id);
                    match (card, place) {
                        (Some(card), _) => {
                            let _ = self.api.edit_message_text(card.chat, card.message, &format!("{}\n\n{line}", card.html)).await;
                        }
                        (None, Some((chat, message))) => {
                            let _ = self.api.edit_message_text(chat, message, line).await;
                        }
                        (None, None) => {}
                    }
                }
                Err(words) => {
                    let _ = self.api.answer_callback(&q.id, Some(&words)).await;
                }
            },
            Tap::Waiting { id: entry, accept } => match self.host.waiting_answer(entry, accept, None) {
                Ok(()) => {
                    let line = if accept { "Done." } else { "Not now." };
                    let _ = self.api.answer_callback(&q.id, Some(line)).await;
                    if let Some((chat, message)) = place {
                        let _ = self.api.edit_message_text(chat, message, line).await;
                    }
                }
                Err(words) => {
                    let _ = self.api.answer_callback(&q.id, Some(&words)).await;
                }
            },
            Tap::Chat(conversation) => match self.host.open_chat(&id, conversation) {
                Ok(title) => {
                    let _ = self.api.answer_callback(&q.id, Some("Switched.")).await;
                    if let Some((chat, _)) = place {
                        self.plain(chat, &format!("Now in: {title}")).await;
                    }
                }
                Err(words) => {
                    let _ = self.api.answer_callback(&q.id, Some(&words)).await;
                }
            },
        }
    }

    /// REM-7: something waits for the person. One message to each paired chat, with buttons that
    /// answer the same row the window answers. A question that wants words is answered by replying.
    pub async fn ping_waiting(&self, chats: &[i64], ping: &WaitingPing) {
        for chat in chats {
            let html = if ping.asks {
                format!("{}\n{}\nReply to this message with your answer.", render::escape_html(&ping.title), render::escape_html(&ping.body))
            } else {
                format!("{}\n{}", render::escape_html(&ping.title), render::escape_html(&ping.body))
            };
            let keyboard = if ping.asks {
                vec![vec![Button { text: "Not now".into(), data: format!("w:{}:n", ping.id) }]]
            } else {
                vec![vec![
                    Button { text: "Yes".into(), data: format!("w:{}:y", ping.id) },
                    Button { text: "No".into(), data: format!("w:{}:n", ping.id) },
                ]]
            };
            if let Some(message) = self.say(*chat, &html, Some(keyboard)).await {
                if ping.asks {
                    self.asks.lock().unwrap().insert((*chat, message), ping.id.clone());
                }
            }
        }
    }
}

/// What a waiting entry says to the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingPing {
    pub id: String,
    pub title: String,
    pub body: String,
    /// It wants words, not a yes (`waiting-ask`).
    pub asks: bool,
}

// ---- files in ----

/// A file from Telegram as the turn takes it: a picture inline, a PDF written to the inbox folder (the
/// caller grants that one path). Anything else is told no, in words.
pub fn attachment_from(
    name: &str,
    mime: Option<&str>,
    bytes: &[u8],
    inbox: &Path,
) -> Result<(TurnAttachment, Option<PathBuf>), String> {
    let lower = name.to_ascii_lowercase();
    let by_ext = |exts: &[&str]| exts.iter().any(|e| lower.ends_with(e));
    let is_image = mime.is_some_and(|m| m.starts_with("image/")) || by_ext(&[".png", ".jpg", ".jpeg", ".gif", ".webp"]);
    let is_pdf = mime == Some("application/pdf") || by_ext(&[".pdf"]);
    if is_image {
        let mime = mime.filter(|m| m.starts_with("image/")).unwrap_or(if by_ext(&[".png"]) {
            "image/png"
        } else if by_ext(&[".gif"]) {
            "image/gif"
        } else if by_ext(&[".webp"]) {
            "image/webp"
        } else {
            "image/jpeg"
        });
        let uri = format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
        return Ok((TurnAttachment { kind: "image".into(), name: name.into(), path: String::new(), data_uri: Some(uri) }, None));
    }
    if is_pdf {
        std::fs::create_dir_all(inbox).map_err(|_| "I could not keep that file.".to_string())?;
        let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).take(80).collect();
        let file = inbox.join(format!("{}-{safe}", uuid::Uuid::new_v4()));
        std::fs::write(&file, bytes).map_err(|_| "I could not keep that file.".to_string())?;
        let path = file.to_string_lossy().to_string();
        return Ok((TurnAttachment { kind: "pdf".into(), name: name.into(), path, data_uri: None }, Some(file)));
    }
    Err("I can read pictures and PDFs here. Other files stay on the desktop.".into())
}

// ---- the app ----

/// What the Remote tab shows of the bot.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct Status {
    /// The token as `...last4`, never more.
    pub token_hint: Option<String>,
    /// `@name`, once Telegram has said who the token belongs to.
    pub bot: Option<String>,
    /// What Telegram last said when it refused, in its words.
    pub error: Option<String>,
    /// The poll is running.
    pub polling: bool,
}

/// The token as the tab may show it.
pub fn token_hint(token: &str) -> String {
    let tail: String = token.trim().chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("...{tail}")
}

/// The adapter's state in the app: what to show, when to restart, the running poller (for pings).
#[derive(Default)]
pub struct Telegram {
    status: Mutex<Status>,
    /// The token was set or removed.
    changed: Notify,
    live: Mutex<Option<Ctx>>,
    told: Mutex<HashSet<String>>,
}

impl Telegram {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    fn edit(&self, change: impl FnOnce(&mut Status)) {
        change(&mut self.status.lock().unwrap());
    }

    /// A token was checked and kept.
    pub fn set_connected(&self, token: &str, bot: &str) {
        self.edit(|s| {
            s.token_hint = Some(token_hint(token));
            s.bot = Some(bot.to_string());
            s.error = None;
        });
    }

    /// The token was forgotten.
    pub fn set_disconnected(&self) {
        self.edit(|s| *s = Status::default());
    }

    /// The token was set or removed: the poller starts over.
    pub fn token_changed(&self) {
        self.changed.notify_one();
    }

    fn live(&self) -> Option<Ctx> {
        self.live.lock().unwrap().clone()
    }

    /// Each entry is told once, however many times it is announced.
    fn first_time(&self, entry: &str) -> bool {
        self.told.lock().unwrap().insert(entry.to_string())
    }
}

/// How long a message waits for the turn before it to start: 40 tries, a quarter second apart.
const BUSY_TRIES: u32 = 40;
const BUSY_WAIT: Duration = Duration::from_millis(250);

pub struct AppHost(pub tauri::AppHandle);

impl AppHost {
    fn gateway(&self) -> tauri::State<'_, Gateway> {
        use tauri::Manager;
        self.0.state::<Gateway>()
    }

    fn db(&self) -> tauri::State<'_, Db> {
        use tauri::Manager;
        self.0.state::<Db>()
    }
}

#[async_trait]
impl Host for AppHost {
    fn enabled(&self) -> bool {
        self.gateway().is_enabled()
    }

    fn is_paired(&self, id: &str) -> bool {
        super::pairing::find(&self.db(), TRANSPORT, id).is_some()
    }

    fn pairing_open(&self) -> bool {
        self.gateway().pairing_open()
    }

    fn pair(&self, id: &str, label: &str, code: &str) -> Result<(), String> {
        self.gateway().pair(TRANSPORT, id, label, code).map(|_| ()).map_err(|f| f.words())
    }

    async fn message(
        &self,
        id: &str,
        text: String,
        attachments: Vec<TurnAttachment>,
        events: mpsc::UnboundedSender<Value>,
    ) -> Intake {
        use tauri::Manager;
        // The turn runs on its own task, so a poll that is dropped does not cut the work short.
        let (app, id) = (self.0.clone(), id.to_string());
        let task = tauri::async_runtime::spawn(async move {
            // People often write two lines in a row. The second can come while the first is still
            // starting; it waits a moment, then steers the running turn, instead of being turned away.
            for _ in 0..BUSY_TRIES {
                let intake =
                    app.state::<Gateway>().on_message(TRANSPORT, &id, text.clone(), attachments.clone(), false, events.clone()).await;
                if !matches!(intake, Intake::Refused(super::Refusal::Busy)) {
                    return intake;
                }
                tokio::time::sleep(BUSY_WAIT).await;
            }
            Intake::Refused(super::Refusal::Busy)
        });
        task.await.unwrap_or_else(|_| Intake::Refused(super::Refusal::Model("The turn stopped unexpectedly.".into())))
    }

    fn answer(&self, id: &str, request_id: &str, allow: bool) -> Result<(), String> {
        self.gateway().answer(TRANSPORT, id, request_id, allow)
    }

    fn stop(&self, id: &str) -> bool {
        self.gateway().stop(TRANSPORT, id)
    }

    fn new_chat(&self, id: &str) -> Result<(), String> {
        self.gateway().new_chat(TRANSPORT, id).map(|_| ())
    }

    fn open_chat(&self, id: &str, conversation_id: &str) -> Result<String, String> {
        self.gateway().open_chat(TRANSPORT, id, conversation_id)?;
        let chat = self.db().get_conversation(conversation_id).map_err(|e| e.to_string())?;
        Ok(chat.map(|c| c.title).unwrap_or_default())
    }

    fn chats(&self, _id: &str) -> Vec<(String, String)> {
        let mut all = self.db().list_conversations().unwrap_or_default();
        all.retain(|c| c.parent_conversation_id.is_none());
        all.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
        all.into_iter().take(5).map(|c| (c.id, c.title)).collect()
    }

    async fn who(&self, id: &str) -> String {
        use tauri::Manager;
        let db = self.db();
        let mgr = self.0.state::<crate::runtime::RuntimeManager>();
        let model = match super::model::default_turn_model(&db, &mgr).await {
            Ok(resolved) => resolved.model.name,
            Err(line) => return line,
        };
        let title = super::pairing::find(&db, TRANSPORT, id)
            .and_then(|s| s.conversation_id)
            .and_then(|c| db.get_conversation(&c).ok().flatten())
            .map(|c| c.title);
        match title {
            Some(title) => format!("I answer with {model}. This chat is \"{title}\"."),
            None => format!("I answer with {model}. There is no chat yet. Write to me to start one."),
        }
    }

    fn waiting_answer(&self, proposal_id: &str, accept: bool, text: Option<&str>) -> Result<(), String> {
        use tauri::Manager;
        let mgr = self.0.state::<crate::runtime::RuntimeManager>();
        crate::commands::waiting::answer(&self.db(), mgr.app_data_dir(), proposal_id, accept, text).map(|_| ())
    }

    fn offset(&self) -> i64 {
        self.db().get_setting(OFFSET_KEY).ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(0)
    }

    fn save_offset(&self, offset: i64) {
        let _ = self.db().set_setting(OFFSET_KEY, &offset.to_string());
    }

    async fn accept_file(&self, name: &str, mime: Option<&str>, bytes: Vec<u8>) -> Result<TurnAttachment, String> {
        use tauri::Manager;
        let inbox = self.0.state::<crate::runtime::RuntimeManager>().app_data_dir().join("remote-inbox");
        let (attachment, saved) = attachment_from(name, mime, &bytes, &inbox)?;
        if let Some(path) = saved {
            // The person sent it on purpose, as a picked file would be: this one path may be read.
            self.0.state::<crate::commands::files::DialogGrants>().remember(&path);
        }
        Ok(attachment)
    }

    fn read_media(&self, path: &str, max: u64) -> Result<Vec<u8>, MediaError> {
        use tauri::Manager;
        let media_dir = self.0.state::<crate::runtime::RuntimeManager>().generated_media_dir();
        if !super::phone_api::media_allowed(&self.db(), &media_dir, path) {
            return Err(MediaError::Unavailable);
        }
        let size = std::fs::metadata(path).map_err(|_| MediaError::Unavailable)?.len();
        if size > max {
            return Err(MediaError::TooBig);
        }
        std::fs::read(path).map_err(|_| MediaError::Unavailable)
    }
}

/// Update numbers are counted per bot. When the token belongs to another bot than before, the kept
/// offset is dropped: with the old bot's higher number, Telegram would skip the new bot's messages.
fn offset_for_bot(db: &Db, bot_id: i64) {
    let bot = bot_id.to_string();
    if db.get_setting(BOT_KEY).ok().flatten().as_deref() != Some(bot.as_str()) {
        let _ = db.set_setting(OFFSET_KEY, "0");
        let _ = db.set_setting(BOT_KEY, &bot);
    }
}

/// The bot token, if one is kept.
pub fn stored_token() -> Option<String> {
    crate::secrets::get_secret(crate::secrets::SERVICE_REMOTE, TOKEN_ACCOUNT).ok().flatten().filter(|t| !t.trim().is_empty())
}

/// Start the adapter for this app. It stays asleep until remote is on and a token is kept; the switch
/// going off, or the token changing, ends the poll within a second.
pub fn start(app: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let gateway = app.state::<Gateway>();
        let tg = app.state::<Telegram>();
        let mut enabled = gateway.watch_enabled();
        loop {
            let token = loop {
                let token = stored_token().filter(|_| *enabled.borrow());
                tg.edit(|s| {
                    s.polling = false;
                    s.token_hint = stored_token().map(|t| token_hint(&t));
                });
                if let Some(token) = token {
                    break token;
                }
                tokio::select! {
                    changed = enabled.changed() => if changed.is_err() { return },
                    _ = tg.changed.notified() => {}
                }
            };
            let api = Arc::new(HttpApi::new(&token));
            match api.get_me().await {
                Ok(me) => {
                    offset_for_bot(&app.state::<Db>(), me.id);
                    tg.edit(|s| {
                        s.bot = Some(me.name());
                        s.error = None;
                    })
                }
                Err(e) => {
                    tg.edit(|s| s.error = Some(e.text));
                    gateway.status_changed();
                    // Try again in a while, or sooner when the person fixes the token or the switch.
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(30)) => {}
                        _ = tg.changed.notified() => {}
                        _ = enabled.changed() => {}
                    }
                    continue;
                }
            }
            let reporter = app.clone();
            let report: Report = Arc::new(move |error| {
                let tg = reporter.state::<Telegram>();
                let error = error.map(str::to_string);
                if tg.status().error != error {
                    tg.edit(|s| s.error = error);
                    reporter.state::<Gateway>().status_changed();
                }
            });
            let ctx = Ctx::new(api, Arc::new(AppHost(app.clone())), Timing::default(), report);
            *tg.live.lock().unwrap() = Some(ctx.clone());
            tg.edit(|s| s.polling = true);
            gateway.status_changed();
            tokio::select! {
                _ = ctx.poll() => {}
                _ = enabled.wait_for(|on| !*on) => {}
                _ = tg.changed.notified() => {}
            }
            *tg.live.lock().unwrap() = None;
            tg.edit(|s| s.polling = false);
            gateway.status_changed();
        }
    });
}

/// REM-7: an entry started to wait for the person. When remote is on, the "tell me" switch is on, the
/// window is not in front and a Telegram account is paired, that account is told, once.
pub fn notify_waiting(app: &tauri::AppHandle, db: &Db, entry: &crate::db::ChangeProposal) {
    use tauri::Manager;
    let gateway = app.state::<Gateway>();
    let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if !gateway.is_enabled() || !crate::waiting::should_notify(crate::waiting::switched_on(db), focused) {
        return;
    }
    let chats: Vec<i64> = super::pairing::load(db)
        .into_iter()
        .filter(|s| s.transport == TRANSPORT)
        .filter_map(|s| s.id.parse().ok())
        .collect();
    let tg = app.state::<Telegram>();
    let Some(ctx) = tg.live() else { return };
    if chats.is_empty() || !tg.first_time(&entry.id) {
        return;
    }
    let mem = app.state::<crate::memory::MemoryStore>();
    let (title, body) = crate::waiting::words(&crate::waiting::who_waits(db, Some(&mem), entry), &entry.rationale);
    let ping = WaitingPing { id: entry.id.clone(), title, body, asks: entry.target == "waiting-ask" };
    tauri::async_runtime::spawn(async move { ctx.ping_waiting(&chats, &ping).await });
}

/// What a finished media job says to the chat: the file, or why there is none. A cancelled job says nothing.
fn job_out(event: &crate::media::jobs::JobEvent) -> Option<Out> {
    match (event.status.as_str(), &event.artifact) {
        ("done", Some(a)) => {
            Some(Out::File(FileOut { id: a.id.clone(), title: a.title.clone(), kind: a.kind.clone(), content: a.content.clone() }))
        }
        ("failed", _) => {
            Some(Out::Line(format!("I could not make it. {}", event.error.as_deref().unwrap_or_default()).trim_end().to_string()))
        }
        _ => None,
    }
}

/// REM-6: a picture or a video is done. It is made by a job after the turn has answered, so it does not
/// come with the turn's events. It goes to each Telegram account whose active chat asked for it.
pub fn media_done(app: &tauri::AppHandle, event: &crate::media::jobs::JobEvent) {
    use tauri::Manager;
    let Some(conversation) = event.conversation_id.as_deref() else { return };
    let Some(ctx) = app.state::<Telegram>().live() else { return };
    let Some(out) = job_out(event) else { return };
    let chats: Vec<i64> = super::pairing::load(&app.state::<Db>())
        .into_iter()
        .filter(|s| s.transport == TRANSPORT && s.conversation_id.as_deref() == Some(conversation))
        .filter_map(|s| s.id.parse().ok())
        .collect();
    tauri::async_runtime::spawn(async move {
        for chat in chats {
            ctx.apply(chat, Some(out.clone())).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::Refusal;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicI64, Ordering};

    const FAST: Timing = Timing {
        backoff: [Duration::from_millis(10), Duration::from_millis(30)],
        typing: Duration::from_millis(15),
        spacing: Duration::from_millis(0),
        poll_secs: 1,
    };

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Send { chat: i64, text: String, buttons: Vec<String> },
        Edit { chat: i64, message: i64, text: String },
        Answered { text: Option<String> },
        Typing,
        File { kind: FileKind, name: String, size: usize },
    }

    #[derive(Default)]
    struct FakeApi {
        batches: Mutex<VecDeque<Result<Vec<Update>, ApiError>>>,
        calls: Mutex<Vec<Call>>,
        next_id: AtomicI64,
        file: Mutex<Option<Vec<u8>>>,
        webhook_deleted: Mutex<u32>,
    }

    impl FakeApi {
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn texts(&self) -> Vec<String> {
            self.calls().into_iter().filter_map(|c| if let Call::Send { text, .. } = c { Some(text) } else { None }).collect()
        }
    }

    #[async_trait]
    impl TelegramApi for FakeApi {
        async fn get_me(&self) -> Result<Me, ApiError> {
            Ok(Me { id: 1, username: Some("poiesis_bot".into()), first_name: "Poiesis".into() })
        }
        async fn delete_webhook(&self) -> Result<(), ApiError> {
            *self.webhook_deleted.lock().unwrap() += 1;
            Ok(())
        }
        async fn get_updates(&self, _offset: i64, _timeout_secs: u64) -> Result<Vec<Update>, ApiError> {
            let next = self.batches.lock().unwrap().pop_front();
            match next {
                Some(batch) => batch,
                // The script is over: like a quiet long poll, wait for ever.
                None => std::future::pending().await,
            }
        }
        async fn send_message(&self, chat: i64, html: &str, keyboard: Option<Keyboard>) -> Result<i64, ApiError> {
            let buttons = keyboard.unwrap_or_default().into_iter().flatten().map(|b| format!("{}={}", b.text, b.data)).collect();
            self.calls.lock().unwrap().push(Call::Send { chat, text: html.into(), buttons });
            Ok(self.next_id.fetch_add(1, Ordering::SeqCst) + 100)
        }
        async fn edit_message_text(&self, chat: i64, message: i64, html: &str) -> Result<(), ApiError> {
            self.calls.lock().unwrap().push(Call::Edit { chat, message, text: html.into() });
            Ok(())
        }
        async fn answer_callback(&self, _id: &str, text: Option<&str>) -> Result<(), ApiError> {
            self.calls.lock().unwrap().push(Call::Answered { text: text.map(str::to_string) });
            Ok(())
        }
        async fn send_typing(&self, _chat: i64) -> Result<(), ApiError> {
            self.calls.lock().unwrap().push(Call::Typing);
            Ok(())
        }
        async fn set_my_commands(&self, _commands: &[(&str, &str)]) -> Result<(), ApiError> {
            Ok(())
        }
        async fn download(&self, _file_id: &str) -> Result<Vec<u8>, ApiError> {
            self.file.lock().unwrap().clone().ok_or_else(|| ApiError::new("no file"))
        }
        async fn send_file(&self, _chat: i64, kind: FileKind, name: &str, bytes: Vec<u8>) -> Result<(), ApiError> {
            self.calls.lock().unwrap().push(Call::File { kind, name: name.into(), size: bytes.len() });
            Ok(())
        }
    }

    /// A gateway that knows one paired sender, `7`, and records what it was asked.
    #[derive(Default)]
    struct FakeHost {
        paired: Vec<String>,
        pairing_open: bool,
        /// What a message does: the events it sends, then how it ends.
        script: Mutex<Vec<Value>>,
        end: Mutex<Option<Intake>>,
        asked: Mutex<Vec<String>>,
        answers: Mutex<Vec<(String, bool)>>,
        waiting: Mutex<Vec<(String, bool, Option<String>)>>,
        saved_offset: Mutex<Option<i64>>,
        pair_result: Mutex<Option<Result<(), String>>>,
        media: Mutex<Option<Result<Vec<u8>, MediaError>>>,
        stopped: Mutex<bool>,
        slow: bool,
        /// The message `first` takes a while to reach the gateway.
        first_is_late: bool,
    }

    #[async_trait]
    impl Host for FakeHost {
        fn enabled(&self) -> bool {
            true
        }
        fn is_paired(&self, id: &str) -> bool {
            self.paired.iter().any(|p| p == id)
        }
        fn pairing_open(&self) -> bool {
            self.pairing_open
        }
        fn pair(&self, id: &str, _label: &str, code: &str) -> Result<(), String> {
            self.asked.lock().unwrap().push(format!("pair {id} {code}"));
            self.pair_result.lock().unwrap().take().unwrap_or(Ok(()))
        }
        async fn message(
            &self,
            _id: &str,
            text: String,
            attachments: Vec<TurnAttachment>,
            events: mpsc::UnboundedSender<Value>,
        ) -> Intake {
            if self.first_is_late && text == "first" {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            self.asked.lock().unwrap().push(format!("message {text} +{}", attachments.len()));
            for event in self.script.lock().unwrap().drain(..) {
                let _ = events.send(event);
            }
            if self.slow {
                tokio::time::sleep(Duration::from_millis(80)).await;
            }
            self.end.lock().unwrap().take().unwrap_or(Intake::Ran)
        }
        fn answer(&self, _id: &str, request_id: &str, allow: bool) -> Result<(), String> {
            self.answers.lock().unwrap().push((request_id.into(), allow));
            Ok(())
        }
        fn stop(&self, _id: &str) -> bool {
            *self.stopped.lock().unwrap() = true;
            true
        }
        fn new_chat(&self, _id: &str) -> Result<(), String> {
            self.asked.lock().unwrap().push("new".into());
            Ok(())
        }
        fn open_chat(&self, _id: &str, conversation_id: &str) -> Result<String, String> {
            self.asked.lock().unwrap().push(format!("open {conversation_id}"));
            Ok("Shop".into())
        }
        fn chats(&self, _id: &str) -> Vec<(String, String)> {
            vec![("c1".into(), "Shop".into()), ("c2".into(), "Taxes".into())]
        }
        async fn who(&self, _id: &str) -> String {
            "I answer with Qwen. This chat is \"Shop\".".into()
        }
        fn waiting_answer(&self, proposal_id: &str, accept: bool, text: Option<&str>) -> Result<(), String> {
            self.waiting.lock().unwrap().push((proposal_id.into(), accept, text.map(str::to_string)));
            Ok(())
        }
        fn offset(&self) -> i64 {
            0
        }
        fn save_offset(&self, offset: i64) {
            *self.saved_offset.lock().unwrap() = Some(offset);
        }
        async fn accept_file(&self, name: &str, mime: Option<&str>, bytes: Vec<u8>) -> Result<TurnAttachment, String> {
            attachment_from(name, mime, &bytes, &std::env::temp_dir().join("poiesis-telegram-test")).map(|(a, _)| a)
        }
        fn read_media(&self, _path: &str, _max: u64) -> Result<Vec<u8>, MediaError> {
            self.media.lock().unwrap().take().unwrap_or(Err(MediaError::Unavailable))
        }
    }

    /// What the poll told the tab, in order.
    type Reports = Arc<Mutex<Vec<Option<String>>>>;

    fn setup(host: FakeHost) -> (Ctx, Arc<FakeApi>, Arc<FakeHost>, Reports) {
        let api = Arc::new(FakeApi::default());
        let host = Arc::new(host);
        let reports = Arc::new(Mutex::new(Vec::new()));
        let seen = reports.clone();
        let report: Report = Arc::new(move |e| seen.lock().unwrap().push(e.map(str::to_string)));
        (Ctx::new(api.clone(), host.clone(), FAST, report), api, host, reports)
    }

    fn paired() -> FakeHost {
        FakeHost { paired: vec!["7".into()], ..Default::default() }
    }

    fn text_update(update_id: i64, user: i64, text: &str) -> Update {
        Update {
            update_id,
            message: Some(Message {
                message_id: update_id,
                from: Some(User { id: user, first_name: "Erich".into(), ..Default::default() }),
                chat: Chat { id: user, kind: "private".into() },
                text: Some(text.into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn tap_update(update_id: i64, user: i64, data: &str, message: i64) -> Update {
        Update {
            update_id,
            callback_query: Some(CallbackQuery {
                id: "cb".into(),
                from: User { id: user, ..Default::default() },
                message: Some(Message { message_id: message, chat: Chat { id: user, kind: "private".into() }, ..Default::default() }),
                data: Some(data.into()),
            }),
            ..Default::default()
        }
    }

    async fn run(ctx: &Ctx, update: Update) {
        ctx.process(update).unwrap().await.unwrap();
    }

    fn token(text: &str) -> Value {
        json!({ "type": "token", "text": text })
    }

    #[tokio::test]
    async fn an_answer_goes_back_as_one_message() {
        let (ctx, api, host, _) = setup(paired());
        *host.script.lock().unwrap() = vec![token("Hello "), token("there.")];
        run(&ctx, text_update(1, 7, "hi")).await;
        assert_eq!(host.asked.lock().unwrap().clone(), vec!["message hi +0"]);
        assert_eq!(api.texts(), vec!["Hello there."]);
    }

    #[tokio::test]
    async fn an_unknown_sender_gets_nothing_at_all() {
        let (ctx, api, host, _) = setup(paired());
        run(&ctx, text_update(1, 99, "hello?")).await;
        run(&ctx, text_update(2, 99, "/new")).await;
        run(&ctx, tap_update(3, 99, "p:perm_1:y", 5)).await;
        assert!(host.asked.lock().unwrap().is_empty());
        assert!(host.answers.lock().unwrap().is_empty(), "a stranger's tap approves nothing");
        assert!(api.texts().is_empty(), "silence");
    }

    #[tokio::test]
    async fn a_stranger_is_heard_only_while_a_code_is_on_the_screen() {
        let (ctx, api, host, _) = setup(FakeHost { pairing_open: true, ..Default::default() });
        run(&ctx, text_update(1, 99, "ABCD2345")).await;
        assert_eq!(host.asked.lock().unwrap().clone(), vec!["pair 99 ABCD2345"]);
        assert!(api.texts()[0].starts_with("Paired."));
        // The link from the code's own button sends `/start CODE`.
        run(&ctx, text_update(2, 99, "/start WXYZ6789")).await;
        assert_eq!(host.asked.lock().unwrap()[1], "pair 99 WXYZ6789");
        // A long chat message is not a code.
        run(&ctx, text_update(3, 99, &"x".repeat(100))).await;
        assert_eq!(host.asked.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_wrong_code_is_answered_in_words() {
        let (ctx, api, host, _) = setup(FakeHost { pairing_open: true, ..Default::default() });
        *host.pair_result.lock().unwrap() = Some(Err("That code is not right.".into()));
        run(&ctx, text_update(1, 99, "NOPE")).await;
        assert_eq!(api.texts(), vec!["That code is not right."]);
    }

    #[tokio::test]
    async fn a_group_chat_is_not_answered() {
        let (ctx, api, host, _) = setup(paired());
        let mut update = text_update(1, 7, "hi");
        update.message.as_mut().unwrap().chat.kind = "group".into();
        run(&ctx, update).await;
        assert!(host.asked.lock().unwrap().is_empty());
        assert!(api.texts().is_empty());
    }

    #[tokio::test]
    async fn a_question_goes_out_with_two_buttons_and_a_tap_resolves_it() {
        let (ctx, api, host, _) = setup(FakeHost { slow: true, ..paired() });
        let request = json!({ "id": "perm_9", "summary": "Open it.", "path": "C:\\work", "mode": "read", "in_folder": false });
        *host.script.lock().unwrap() = vec![json!({ "type": "permission", "request": request }), token("Done.")];
        // The turn waits for the answer, as a real one does: the tap comes while it runs.
        let turn = ctx.process(text_update(1, 7, "tidy up")).unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        let calls = api.calls();
        let question = calls.iter().find_map(|c| match c {
            Call::Send { text, buttons, .. } if !buttons.is_empty() => Some((text.clone(), buttons.clone())),
            _ => None,
        });
        let (text, buttons) = question.expect("a question with buttons");
        assert_eq!(text, "Open it.\n<code>C:\\work</code>");
        assert_eq!(buttons, vec!["Allow=p:perm_9:y", "Not now=p:perm_9:n"]);

        run(&ctx, tap_update(2, 7, "p:perm_9:y", 100)).await;
        turn.await.unwrap();
        assert_eq!(host.answers.lock().unwrap().clone(), vec![("perm_9".to_string(), true)]);
        let after = api.calls();
        assert!(!after.iter().any(|c| matches!(c, Call::Edit { text, .. } if text.ends_with(render::GONE_LINE))), "{after:?}");
        assert!(after.contains(&Call::Answered { text: Some("Allowed.".into()) }));
        assert!(
            after.iter().any(|c| matches!(c, Call::Edit { text, .. } if text == "Open it.\n<code>C:\\work</code>\n\nAllowed.")),
            "the card is rewritten and its buttons go: {after:?}"
        );
    }

    #[tokio::test]
    async fn not_now_says_no_and_a_stale_tap_is_told_so() {
        let (ctx, api, host, _) = setup(paired());
        run(&ctx, tap_update(1, 7, "p:perm_1:n", 5)).await;
        assert_eq!(host.answers.lock().unwrap().clone(), vec![("perm_1".to_string(), false)]);
        // No card was kept (the app restarted): the message is still rewritten.
        assert!(api.calls().iter().any(|c| matches!(c, Call::Edit { message: 5, text, .. } if text == "Not allowed.")));
    }

    #[tokio::test]
    async fn a_question_that_is_over_loses_its_buttons() {
        let (ctx, api, host, _) = setup(paired());
        let request = json!({ "id": "perm_3", "summary": "Open it.", "path": "", "mode": "read", "in_folder": false });
        *host.script.lock().unwrap() = vec![
            json!({ "type": "permission", "request": request }),
            json!({ "type": "permission_gone", "id": "perm_3" }),
        ];
        run(&ctx, text_update(1, 7, "go")).await;
        assert!(api.calls().iter().any(|c| matches!(c, Call::Edit { text, .. } if text.ends_with(render::GONE_LINE))));
    }

    #[tokio::test]
    async fn the_commands_do_what_they_say() {
        let (ctx, api, host, _) = setup(paired());
        run(&ctx, text_update(1, 7, "/new")).await;
        run(&ctx, text_update(2, 7, "/stop@poiesis_bot")).await;
        run(&ctx, text_update(3, 7, "/who")).await;
        run(&ctx, text_update(4, 7, "/chats")).await;
        run(&ctx, text_update(5, 7, "/dance")).await;
        assert_eq!(host.asked.lock().unwrap().clone(), vec!["new"], "commands are not sent to the model");
        assert!(*host.stopped.lock().unwrap());
        let texts = api.texts();
        assert_eq!(texts[0], "Started a new chat.");
        assert_eq!(texts[1], "Stopped.");
        assert_eq!(texts[2], "I answer with Qwen. This chat is \"Shop\".");
        assert_eq!(texts[4], "I know /new, /chats, /stop and /who.");
        let chats = api.calls().into_iter().find_map(|c| match c {
            Call::Send { buttons, text, .. } if text.starts_with("Your last chats") => Some(buttons),
            _ => None,
        });
        assert_eq!(chats.unwrap(), vec!["Shop=c:c1", "Taxes=c:c2"]);

        run(&ctx, tap_update(6, 7, "c:c2", 9)).await;
        assert_eq!(host.asked.lock().unwrap().last().unwrap(), "open c2");
        assert_eq!(api.texts().last().unwrap(), "Now in: Shop");
    }

    #[tokio::test]
    async fn a_long_answer_is_split_under_the_limit() {
        let (ctx, api, host, _) = setup(paired());
        *host.script.lock().unwrap() = vec![token(&format!("{}\n\n{}", "a".repeat(4000), "b".repeat(4000)))];
        run(&ctx, text_update(1, 7, "write")).await;
        let texts = api.texts();
        assert_eq!(texts.len(), 2);
        assert!(texts.iter().all(|t| t.chars().count() <= render::TELEGRAM_LIMIT));
    }

    #[tokio::test]
    async fn typing_is_shown_while_the_turn_runs() {
        let (ctx, api, _host, _) = setup(FakeHost { slow: true, ..paired() });
        run(&ctx, text_update(1, 7, "think")).await;
        assert!(api.calls().iter().filter(|c| **c == Call::Typing).count() >= 2);
    }

    #[tokio::test]
    async fn a_message_that_steers_a_running_turn_says_so_and_a_refusal_has_words() {
        let (ctx, api, host, _) = setup(paired());
        *host.end.lock().unwrap() = Some(Intake::Steered);
        run(&ctx, text_update(1, 7, "also this")).await;
        assert_eq!(api.texts(), vec!["Got it. I will take that into account."]);
        *host.end.lock().unwrap() = Some(Intake::Refused(Refusal::Model("No default chat model is set.".into())));
        run(&ctx, text_update(2, 7, "hi")).await;
        assert_eq!(api.texts()[1], "No default chat model is set.");
    }

    #[tokio::test]
    async fn the_poll_hands_updates_on_in_order_and_saves_the_offset_after_them() {
        let (ctx, api, host, _) = setup(paired());
        api.batches.lock().unwrap().push_back(Ok(vec![text_update(10, 7, "first"), text_update(11, 7, "second")]));
        let _ = tokio::time::timeout(Duration::from_millis(200), ctx.poll()).await;
        assert_eq!(*api.webhook_deleted.lock().unwrap(), 1, "one poller only: the webhook goes first");
        assert_eq!(host.asked.lock().unwrap().clone(), vec!["message first +0", "message second +0"]);
        assert_eq!(*host.saved_offset.lock().unwrap(), Some(12));
    }

    #[tokio::test]
    async fn a_failed_poll_backs_off_and_tells_the_tab_the_error() {
        let (ctx, api, _host, reports) = setup(paired());
        for _ in 0..2 {
            api.batches.lock().unwrap().push_back(Err(ApiError::new("Unauthorized")));
        }
        api.batches.lock().unwrap().push_back(Ok(vec![]));
        let _ = tokio::time::timeout(Duration::from_millis(300), ctx.poll()).await;
        let seen = reports.lock().unwrap().clone();
        assert_eq!(seen, vec![Some("Unauthorized".to_string()), Some("Unauthorized".to_string()), None]);
    }

    #[test]
    fn the_backoff_is_five_seconds_then_thirty_and_telegrams_own_wait_wins() {
        let t = Timing::default();
        assert_eq!(t.backoff_for(1, None), Duration::from_secs(5));
        assert_eq!(t.backoff_for(2, None), Duration::from_secs(30));
        assert_eq!(t.backoff_for(9, None), Duration::from_secs(30));
        assert_eq!(t.backoff_for(1, Some(60)), Duration::from_secs(60));
        assert_eq!(t.backoff_for(2, Some(3)), Duration::from_secs(30));
    }

    #[tokio::test]
    async fn sends_to_one_chat_are_a_gap_apart_and_other_chats_are_not_held_up() {
        let pacer = Pacer::new(Duration::from_millis(40));
        let start = tokio::time::Instant::now();
        for _ in 0..3 {
            pacer.wait(1).await;
        }
        assert!(start.elapsed() >= Duration::from_millis(80), "three sends need two gaps: {:?}", start.elapsed());
        let other = tokio::time::Instant::now();
        pacer.wait(2).await;
        assert!(other.elapsed() < Duration::from_millis(30));
    }

    #[tokio::test]
    async fn a_photo_becomes_an_image_attachment_and_a_big_file_is_refused() {
        let (ctx, api, host, _) = setup(paired());
        *api.file.lock().unwrap() = Some(vec![1, 2, 3]);
        let mut update = text_update(1, 7, "");
        {
            let m = update.message.as_mut().unwrap();
            m.text = None;
            m.caption = Some("look".into());
            m.photo = vec![
                PhotoSize { file_id: "small".into(), file_size: Some(10), width: 90 },
                PhotoSize { file_id: "big".into(), file_size: Some(1000), width: 1280 },
            ];
        }
        run(&ctx, update).await;
        assert_eq!(host.asked.lock().unwrap().clone(), vec!["message look +1"]);

        let mut huge = text_update(2, 7, "");
        {
            let m = huge.message.as_mut().unwrap();
            m.text = None;
            m.document = Some(Document { file_id: "d".into(), file_name: Some("big.pdf".into()), mime_type: None, file_size: Some(MAX_DOWNLOAD + 1) });
        }
        run(&ctx, huge).await;
        assert_eq!(host.asked.lock().unwrap().len(), 1, "nothing was sent to the turn");
        assert_eq!(api.texts().last().unwrap(), "That file is bigger than the 20 MB Telegram lets me take.");
    }

    #[tokio::test]
    async fn a_picture_the_turn_made_goes_out_as_a_photo_and_a_huge_one_stays_home() {
        let (ctx, api, host, _) = setup(paired());
        let event = json!({ "type": "artifact", "id": "a", "title": "Cat", "kind": "image", "content": "C:\\media\\cat.png" });
        *host.script.lock().unwrap() = vec![event.clone()];
        *host.media.lock().unwrap() = Some(Ok(vec![0; 5]));
        run(&ctx, text_update(1, 7, "draw")).await;
        assert!(api.calls().contains(&Call::File { kind: FileKind::Photo, name: "cat.png".into(), size: 5 }));

        *host.script.lock().unwrap() = vec![event];
        *host.media.lock().unwrap() = Some(Err(MediaError::TooBig));
        run(&ctx, text_update(2, 7, "draw")).await;
        assert!(api.texts().contains(&"It is too big for Telegram. It is on your desktop.".to_string()));
    }

    #[tokio::test]
    async fn a_text_artifact_goes_out_as_a_document() {
        let (ctx, api, host, _) = setup(paired());
        *host.script.lock().unwrap() = vec![json!({ "type": "artifact", "id": "a", "title": "Q3 report!", "kind": "markdown", "content": "# Hi" })];
        run(&ctx, text_update(1, 7, "write")).await;
        assert!(api.calls().contains(&Call::File { kind: FileKind::Document, name: "Q3-report.md".into(), size: 4 }));
    }

    #[tokio::test]
    async fn a_parked_question_is_one_message_and_a_tap_answers_the_same_row() {
        let (ctx, api, host, _) = setup(paired());
        let ping = WaitingPing { id: "w1".into(), title: "Mira is waiting for you".into(), body: "Delete <old>?".into(), asks: false };
        ctx.ping_waiting(&[7], &ping).await;
        let calls = api.calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            Call::Send { chat, text, buttons } => {
                assert_eq!(*chat, 7);
                assert_eq!(text, "Mira is waiting for you\nDelete &lt;old&gt;?");
                assert_eq!(buttons, &vec!["Yes=w:w1:y".to_string(), "No=w:w1:n".to_string()]);
            }
            other => panic!("{other:?}"),
        }
        run(&ctx, tap_update(1, 7, "w:w1:y", 100)).await;
        assert_eq!(host.waiting.lock().unwrap().clone(), vec![("w1".to_string(), true, None)]);
        assert!(api.calls().iter().any(|c| matches!(c, Call::Edit { text, .. } if text == "Done.")));
    }

    #[tokio::test]
    async fn a_question_that_wants_words_is_answered_by_replying_to_it() {
        let (ctx, api, host, _) = setup(paired());
        let ping = WaitingPing { id: "w2".into(), title: "Mira is waiting for you".into(), body: "Which list?".into(), asks: true };
        ctx.ping_waiting(&[7], &ping).await;
        assert!(matches!(&api.calls()[0], Call::Send { buttons, .. } if buttons == &vec!["Not now=w:w2:n".to_string()]));
        let mut reply = text_update(5, 7, "The team list");
        reply.message.as_mut().unwrap().reply_to_message = Some(Box::new(ReplyTo { message_id: 100 }));
        run(&ctx, reply).await;
        assert_eq!(host.waiting.lock().unwrap().clone(), vec![("w2".to_string(), true, Some("The team list".to_string()))]);
        assert!(host.asked.lock().unwrap().is_empty(), "the reply is the answer, not a new turn");
    }

    #[tokio::test]
    async fn quick_messages_reach_the_gateway_in_the_order_they_were_written() {
        let (ctx, _api, host, _) = setup(FakeHost { first_is_late: true, ..paired() });
        let first = ctx.process(text_update(1, 7, "first")).unwrap();
        let second = ctx.process(text_update(2, 7, "second")).unwrap();
        first.await.unwrap();
        second.await.unwrap();
        assert_eq!(host.asked.lock().unwrap().clone(), vec!["message first +0", "message second +0"]);
    }

    #[tokio::test]
    async fn a_bare_start_while_a_code_is_open_is_not_a_try() {
        let (ctx, api, host, _) = setup(FakeHost { pairing_open: true, ..Default::default() });
        run(&ctx, text_update(1, 99, "/start")).await;
        assert!(host.asked.lock().unwrap().is_empty(), "no try was used up");
        assert_eq!(api.texts(), vec!["Send me the code from your desktop."]);
    }

    #[tokio::test]
    async fn an_artifact_changed_twice_goes_out_once_after_the_answer() {
        let (ctx, api, host, _) = setup(paired());
        *host.script.lock().unwrap() = vec![
            json!({ "type": "artifact", "id": "a", "title": "Plan", "kind": "markdown", "content": "# One" }),
            json!({ "type": "artifact", "id": "a", "title": "Plan", "kind": "markdown", "content": "# Two!" }),
            token("Here it is."),
        ];
        run(&ctx, text_update(1, 7, "plan")).await;
        let calls = api.calls();
        let files: Vec<&Call> = calls.iter().filter(|c| matches!(c, Call::File { .. })).collect();
        assert_eq!(files, vec![&Call::File { kind: FileKind::Document, name: "Plan.md".into(), size: 6 }], "the last version, once");
        let answer = calls.iter().position(|c| matches!(c, Call::Send { text, .. } if text == "Here it is.")).unwrap();
        let file = calls.iter().position(|c| matches!(c, Call::File { .. })).unwrap();
        assert!(answer < file, "{calls:?}");
    }

    #[tokio::test]
    async fn a_card_still_open_when_the_turn_ends_loses_its_buttons() {
        let (ctx, api, host, _) = setup(paired());
        let request = json!({ "id": "perm_4", "summary": "Open it.", "path": "", "mode": "read", "in_folder": false });
        *host.script.lock().unwrap() = vec![json!({ "type": "permission", "request": request })];
        run(&ctx, text_update(1, 7, "go")).await;
        let gone = format!("Open it.\n\n{}", render::GONE_LINE);
        assert!(api.calls().iter().any(|c| matches!(c, Call::Edit { text, .. } if *text == gone)), "{:?}", api.calls());
    }

    #[test]
    fn another_bot_starts_from_a_fresh_offset_and_the_same_bot_keeps_its_place() {
        let db = Db::open_in_memory().unwrap();
        offset_for_bot(&db, 1);
        db.set_setting(OFFSET_KEY, "500").unwrap();
        offset_for_bot(&db, 1);
        assert_eq!(db.get_setting(OFFSET_KEY).unwrap().as_deref(), Some("500"));
        offset_for_bot(&db, 2);
        assert_eq!(db.get_setting(OFFSET_KEY).unwrap().as_deref(), Some("0"));
    }

    #[test]
    fn a_finished_picture_is_a_file_and_a_failed_one_is_a_line() {
        use crate::media::jobs::JobEvent;
        let artifact = crate::db::Artifact {
            id: "a1".into(),
            conversation_id: Some("c".into()),
            title: "Cat".into(),
            kind: "image".into(),
            content: "C:\\media\\cat.png".into(),
            created_at: 0,
            saved_path: None,
            meta_json: None,
            parent_id: None,
            message_id: None,
        };
        let event = |status: &str, artifact: Option<crate::db::Artifact>, error: Option<&str>| JobEvent {
            job_id: "j".into(),
            conversation_id: Some("c".into()),
            message_id: None,
            status: status.into(),
            artifact,
            error: error.map(str::to_string),
        };
        assert_eq!(
            job_out(&event("done", Some(artifact), None)),
            Some(Out::File(FileOut { id: "a1".into(), title: "Cat".into(), kind: "image".into(), content: "C:\\media\\cat.png".into() }))
        );
        assert_eq!(job_out(&event("failed", None, Some("No credit left."))), Some(Out::Line("I could not make it. No credit left.".into())));
        assert_eq!(job_out(&event("cancelled", None, None)), None);
    }

    #[test]
    fn callback_data_is_read_back() {
        assert_eq!(tap("p:perm_1:y"), Some(Tap::Question { request_id: "perm_1", allow: true }));
        assert_eq!(tap("p:perm_1:n"), Some(Tap::Question { request_id: "perm_1", allow: false }));
        assert_eq!(tap("w:abc:n"), Some(Tap::Waiting { id: "abc", accept: false }));
        assert_eq!(tap("c:conv-1"), Some(Tap::Chat("conv-1")));
        assert_eq!(tap("p:perm_1:maybe"), None);
        assert_eq!(tap("x:1"), None);
        assert_eq!(tap("nonsense"), None);
    }

    #[test]
    fn commands_are_read_with_or_without_the_bots_name() {
        assert_eq!(command("/new"), Some(("new", "")));
        assert_eq!(command("/new@poiesis_bot"), Some(("new", "")));
        assert_eq!(command("/start ABCD2345"), Some(("start", "ABCD2345")));
        assert_eq!(command("hello /new"), None);
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poiesis-telegram-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn pictures_go_in_as_data_pdfs_as_a_file_and_the_rest_is_refused() {
        let inbox = tmp("inbox");
        let (picture, saved) = attachment_from("cat.PNG", None, &[1, 2, 3], &inbox).unwrap();
        assert_eq!(picture.kind, "image");
        assert!(picture.data_uri.unwrap().starts_with("data:image/png;base64,"));
        assert!(saved.is_none());

        let (pdf, saved) = attachment_from("Q3 report.pdf", Some("application/pdf"), b"%PDF", &inbox).unwrap();
        assert_eq!(pdf.kind, "pdf");
        let file = saved.expect("kept in the inbox");
        assert!(file.starts_with(&inbox));
        assert_eq!(std::fs::read(&file).unwrap(), b"%PDF");
        assert_eq!(pdf.path, file.to_string_lossy());
        assert!(!file.file_name().unwrap().to_string_lossy().contains(' '), "a safe file name");

        assert!(attachment_from("run.exe", Some("application/octet-stream"), &[0], &inbox).is_err());
        let _ = std::fs::remove_dir_all(&inbox);
    }

    #[test]
    fn the_status_never_holds_more_than_the_last_four_of_a_token() {
        let hint = token_hint("  987654321:AAH-secretSECRETsecret-x9Z4 ");
        assert_eq!(hint, "...x9Z4");
        assert!(!serde_json::to_string(&Status { token_hint: Some(hint), ..Default::default() }).unwrap().contains("secret"));
    }
}
