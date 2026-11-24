//! REM-11: what a paired phone may ask of this desktop. The names are `remote_proto::CALLS` and
//! `STREAMS`; anything else is refused before it gets here. Each command reads or changes only what
//! a chat needs. Nothing here touches settings, pairing, the kill switch, files or the updater
//! (plan, Part III.8).
//!
//! The commands that only read the database are plain functions of `&Db`, so they are tested on an
//! in-memory database. `AppBackend` is the thin part that holds the `AppHandle`.

use std::path::Path;

use async_trait::async_trait;
use base64::Engine as _;
use remote_proto::Hello;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Listener, Manager};
use tokio::sync::{broadcast, mpsc::{UnboundedReceiver, UnboundedSender}};

use super::iroh_host::Backend;
use super::{pairing, session, Gateway, Intake, STATUS_EVENT, TURN_EVENT};
use crate::agent::turn::TurnAttachment;
use crate::db::Db;

/// The transport name in the senders list.
pub const TRANSPORT: &str = "phone";

/// How many chats the phone gets in its list.
const CHATS: usize = 50;
/// How many messages of one chat the phone reads at once.
const MESSAGES: usize = 200;
/// Longest text the phone may send in one message.
const MAX_TEXT: usize = 20_000;
/// At most this many photos per message, each at most this many bytes of data URI.
const MAX_ATTACHMENTS: usize = 4;
const MAX_ATTACHMENT_BYTES: usize = 6 * 1024 * 1024;
/// The biggest file `read_media` sends. Base64 grows it by a third, and a frame holds 8 MB.
const MAX_MEDIA_BYTES: u64 = 5 * 1024 * 1024;
/// A text artifact is cut at this many characters for the phone.
const MAX_ARTIFACT_CHARS: usize = 100_000;

/// The app events a phone watches. Others stay on the desktop.
const WATCHED: [&str; 2] = [crate::waiting::AWAY_EVENT, TURN_EVENT];
/// Of a remote turn's events, the feed carries only these. The phone that started the turn already has the rest on
/// its own stream, and every token sent twice would double the traffic over the relay.
const FEED_TURN_EVENTS: [&str; 3] = ["permission", "permission_gone", "turn_finalized"];

/// Does this app event go to the phone's feed?
fn feed_carries(name: &str, payload: &Value) -> bool {
    name != TURN_EVENT || payload["event"]["type"].as_str().is_some_and(|t| FEED_TURN_EVENTS.contains(&t))
}

pub struct AppBackend(pub AppHandle);

impl AppBackend {
    fn gateway(&self) -> tauri::State<'_, Gateway> {
        self.0.state::<Gateway>()
    }

    fn db(&self) -> tauri::State<'_, Db> {
        self.0.state::<Db>()
    }

    /// A paired phone, and remote on. The refusal is a line for the phone.
    fn admit(&self, id: &str) -> Result<pairing::Sender, String> {
        super::admit(&self.db(), self.gateway().is_enabled(), TRANSPORT, id).map_err(|r| r.words().unwrap_or_default())
    }
}

#[async_trait]
impl Backend for AppBackend {
    fn is_paired(&self, id: &str) -> bool {
        pairing::find(&self.db(), TRANSPORT, id).is_some()
    }

    fn pairing_open(&self) -> bool {
        self.gateway().pairing_open()
    }

    async fn hello(&self, id: &str, hello: Hello) -> Result<Value, String> {
        self.gateway().pair(TRANSPORT, id, &hello.label, &hello.code).map_err(|f| f.words())?;
        Ok(json!({ "name": desktop_name() }))
    }

    async fn call(&self, id: &str, cmd: &str, args: Value) -> Result<Value, String> {
        let sender = self.admit(id)?;
        let db = self.db();
        match cmd {
            "status" => {
                let fleet = self.0.state::<crate::agent::fleet::Fleet>();
                let running: Vec<String> = fleet.roots().iter().map(|r| r.conversation_id.clone()).collect();
                Ok(status(&desktop_name(), &sender, &running))
            }
            "conversations" => conversations(&db),
            "messages" => messages(&db, &text_arg(&args, "conversation_id")?),
            "artifacts" => artifacts(&db, &text_arg(&args, "conversation_id")?),
            "new_chat" => {
                let id = self.gateway().new_chat(TRANSPORT, &sender.id)?;
                Ok(json!({ "conversation_id": id }))
            }
            "open_chat" => {
                let cid = text_arg(&args, "conversation_id")?;
                self.gateway().open_chat(TRANSPORT, &sender.id, &cid)?;
                Ok(json!({ "conversation_id": cid }))
            }
            "stop" => Ok(json!({ "stopped": self.gateway().stop(TRANSPORT, &sender.id) })),
            "answer" => {
                let request = text_arg(&args, "request_id")?;
                let allow = args.get("allow").and_then(Value::as_bool).ok_or("Say yes or no.")?;
                self.gateway().answer(TRANSPORT, &sender.id, &request, allow)?;
                Ok(json!({ "answered": true }))
            }
            "waiting" => serde_json::to_value(db.list_waiting().map_err(|e| e.to_string())?).map_err(|e| e.to_string()),
            "resolve_waiting" => {
                let waiting_id = text_arg(&args, "id")?;
                let accept = args.get("accept").and_then(Value::as_bool).ok_or("Say yes or no.")?;
                let text = args.get("text").and_then(Value::as_str).map(str::to_string);
                let mgr = self.0.state::<crate::runtime::RuntimeManager>();
                let said = crate::commands::waiting::answer(&db, mgr.app_data_dir(), &waiting_id, accept, text.as_deref())?;
                Ok(json!({ "said": said }))
            }
            "voice_status" => {
                let (mgr, voice) = (self.0.state::<crate::runtime::RuntimeManager>(), self.0.state::<crate::runtime::voice::VoiceManager>());
                serde_json::to_value(crate::commands::voice::voice_status_cmd(mgr, voice)).map_err(|e| e.to_string())
            }
            "voice_stop" => {
                crate::commands::voice::voice_stop_cmd(self.0.state::<crate::runtime::voice_session::VoiceSessions>());
                Ok(json!({ "stopped": true }))
            }
            "voice_assistant" => {
                let state = text_arg(&args, "state")?;
                let generation = args.get("generation").and_then(Value::as_u64).ok_or("That request was not complete.")?;
                crate::commands::voice::voice_assistant_cmd(self.0.state::<crate::runtime::voice_session::VoiceSessions>(), state, generation)
                    .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "voice_speak" => {
                let generation = args.get("generation").and_then(Value::as_u64).ok_or("That request was not complete.")?;
                let text = args.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
                if text.chars().count() > MAX_TEXT {
                    return Err("That is too long to speak at once.".into());
                }
                let done = args.get("done").and_then(Value::as_bool).unwrap_or(false);
                let notice = args.get("notice").and_then(Value::as_bool);
                let language = args.get("language").and_then(Value::as_str).map(str::to_string);
                crate::commands::voice::voice_speak_cmd(
                    self.0.state::<crate::runtime::voice_session::VoiceSessions>(),
                    generation,
                    text,
                    done,
                    notice,
                    language,
                );
                Ok(Value::Null)
            }
            "voice_cancel_speech" => {
                let generation = args.get("generation").and_then(Value::as_u64).ok_or("That request was not complete.")?;
                crate::commands::voice::voice_cancel_speech_cmd(self.0.state::<crate::runtime::voice_session::VoiceSessions>(), generation);
                Ok(Value::Null)
            }
            "read_media" => {
                let path = text_arg(&args, "path")?;
                let mgr = self.0.state::<crate::runtime::RuntimeManager>();
                read_media(&db, &mgr.generated_media_dir(), &path)
            }
            _ => Err("That is not available on the phone.".into()),
        }
    }

    async fn stream(
        &self,
        id: &str,
        cmd: &str,
        args: Value,
        out: UnboundedSender<Value>,
        audio: UnboundedReceiver<Vec<u8>>,
    ) -> Result<(), String> {
        let sender = self.admit(id)?;
        match cmd {
            "send" => self.send(sender, args, out).await,
            "voice_start" => self.voice(args, out, audio).await,
            "subscribe" => {
                self.subscribe(out).await;
                Ok(())
            }
            _ => Err("That is not available on the phone.".into()),
        }
    }

    fn link_up(&self) {
        self.gateway().link_up();
    }

    fn link_down(&self) {
        self.gateway().link_down();
    }

    fn kicks(&self) -> broadcast::Receiver<String> {
        self.gateway().watch_kicks()
    }
}

impl AppBackend {
    /// A message from the phone. The turn runs on its own task, so a phone that drops out does not
    /// cut the work short; the events it misses are in the chat when it comes back.
    async fn send(&self, sender: pairing::Sender, args: Value, out: UnboundedSender<Value>) -> Result<(), String> {
        let input = parse_send(&args)?;
        let gw = self.gateway();
        if let Some(cid) = &input.conversation_id {
            gw.open_chat(TRANSPORT, &sender.id, cid)?;
        }
        // The phone learns which chat this is before the first token.
        let fresh = pairing::find(&self.db(), TRANSPORT, &sender.id).unwrap_or(sender);
        let conversation_id = session::conversation_for(&self.db(), &fresh)?;
        let _ = out.send(json!({ "type": "chat", "conversation_id": conversation_id }));

        let app = self.0.clone();
        let id = fresh.id.clone();
        let told = out.clone();
        let task = tauri::async_runtime::spawn(async move {
            let gw = app.state::<Gateway>();
            gw.on_message(TRANSPORT, &id, input.text, input.attachments, input.spoken, out).await
        });
        match task.await.map_err(|_| "The turn stopped unexpectedly.".to_string())? {
            Intake::Ran => Ok(()),
            // The words went into the turn that is running; its answer keeps coming on the first stream.
            Intake::Steered => {
                let _ = told.send(json!({ "type": "steered" }));
                Ok(())
            }
            Intake::Refused(r) => Err(r.words().unwrap_or_default()),
        }
    }

    /// A live voice session for this phone: events go up the stream, the phone's microphone comes down it. The
    /// stream lives as long as the phone keeps it open; closing it, or losing the link, ends the session.
    async fn voice(&self, args: Value, out: UnboundedSender<Value>, mut audio: UnboundedReceiver<Vec<u8>>) -> Result<(), String> {
        let language = args.get("ui_language").and_then(Value::as_str).unwrap_or("en").chars().take(16).collect::<String>();
        let ready = out.clone();
        let events = tauri::ipc::Channel::<crate::runtime::voice_session::VoiceEvent>::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(text) = body {
                if let Ok(event) = serde_json::from_str::<Value>(&text) {
                    let _ = out.send(event);
                }
            }
            Ok(())
        });
        crate::commands::voice::voice_start_cmd(
            self.0.clone(),
            self.0.state::<Db>(),
            self.0.state::<crate::runtime::RuntimeManager>(),
            self.0.state::<crate::runtime::voice_session::VoiceSessions>(),
            language,
            events,
        )
        .await
        .map_err(|e| e.to_string())?;
        // The phone waits for this before it opens its microphone.
        let _ = ready.send(json!({ "type": "ready" }));
        // However this ends, the microphone session ends with it.
        let _end = EndVoice(self.0.clone());
        let sessions = self.0.state::<crate::runtime::voice_session::VoiceSessions>();
        while let Some(bytes) = audio.recv().await {
            // A lost batch is nothing; the session goes on.
            let _ = crate::commands::voice::push_audio(&sessions, &bytes);
        }
        Ok(())
    }

    /// Forward the few app events a phone cares about until the phone goes away.
    async fn subscribe(&self, out: UnboundedSender<Value>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
        let _listening = Listening::new(&self.0, tx);
        // A first line tells the phone the feed is live, and the status of the link.
        let _ = out.send(json!({ "name": STATUS_EVENT, "payload": { "enabled": true, "connected": true } }));
        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    if out.send(event).is_err() {
                        break;
                    }
                }
                _ = out.closed() => break,
            }
        }
    }
}

/// Ends the live voice session when dropped, however the stream that held it ended.
struct EndVoice(AppHandle);

impl Drop for EndVoice {
    fn drop(&mut self) {
        self.0.state::<crate::runtime::voice_session::VoiceSessions>().replace(None);
    }
}

/// App listeners that are removed when this is dropped, however the stream ends.
struct Listening {
    app: AppHandle,
    ids: Vec<tauri::EventId>,
}

impl Listening {
    fn new(app: &AppHandle, tx: UnboundedSender<Value>) -> Self {
        let ids = WATCHED
            .iter()
            .map(|name| {
                let (tx, name) = (tx.clone(), *name);
                app.listen(name, move |event| {
                    let payload = serde_json::from_str::<Value>(event.payload()).unwrap_or(Value::Null);
                    if feed_carries(name, &payload) {
                        let _ = tx.send(json!({ "name": name, "payload": payload }));
                    }
                })
            })
            .collect();
        Listening { app: app.clone(), ids }
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        for id in self.ids.drain(..) {
            self.app.unlisten(id);
        }
    }
}

/// What the desktop calls itself on the phone.
pub fn desktop_name() -> String {
    sysinfo::System::host_name().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "your desktop".into())
}

fn text_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() < 200)
        .map(str::to_string)
        .ok_or_else(|| "That request was not complete.".to_string())
}

pub fn status(name: &str, sender: &pairing::Sender, running: &[String]) -> Value {
    json!({
        "name": name,
        "label": sender.label,
        "active": sender.conversation_id,
        "running": running,
    })
}

/// The chats the phone lists: the top-level ones, newest first, with only what a list row needs. A
/// delegated helper's workspace belongs to the turn that started it and is left out, as in the sidebar.
pub fn conversations(db: &Db) -> Result<Value, String> {
    let mut all = db.list_conversations().map_err(|e| e.to_string())?;
    all.retain(|c| c.parent_conversation_id.is_none());
    all.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
    all.truncate(CHATS);
    let rows = all
        .into_iter()
        .map(|c| {
            json!({
                "id": c.id,
                "title": c.title,
                "updated_at": c.updated_at,
                "remote_sender": c.remote_sender,
                "remote_transport": c.remote_transport,
            })
        })
        .collect();
    Ok(Value::Array(rows))
}

/// A message as the phone needs it. The agent timeline and the plan are for the desktop to draw.
pub fn trim_message(message: &crate::db::Message) -> Value {
    let mut v = serde_json::to_value(message).unwrap_or(Value::Null);
    if let Some(map) = v.as_object_mut() {
        map.remove("steps_json");
        map.remove("plan_json");
    }
    v
}

pub fn messages(db: &Db, conversation_id: &str) -> Result<Value, String> {
    let all = db.list_messages_window(conversation_id, MESSAGES).map_err(|e| e.to_string())?;
    Ok(Value::Array(all.iter().map(trim_message).collect()))
}

/// Artifacts of a chat. A picture or a video is a path on the desktop; the phone reads it with
/// `read_media`. Text is cut so one huge file cannot fill a frame.
pub fn artifacts(db: &Db, conversation_id: &str) -> Result<Value, String> {
    let all = db.list_artifacts(conversation_id).map_err(|e| e.to_string())?;
    let rows = all
        .into_iter()
        .map(|a| {
            let media = matches!(a.kind.as_str(), "image" | "video");
            let content: String = if media { a.content.clone() } else { a.content.chars().take(MAX_ARTIFACT_CHARS).collect() };
            json!({ "id": a.id, "title": a.title, "kind": a.kind, "content": content, "created_at": a.created_at })
        })
        .collect();
    Ok(Value::Array(rows))
}

#[derive(Debug)]
pub struct SendInput {
    pub text: String,
    /// The words were heard, and the answer will be read out (voice mode).
    pub spoken: bool,
    pub conversation_id: Option<String>,
    pub attachments: Vec<TurnAttachment>,
}

#[derive(Deserialize)]
struct SendWire {
    #[serde(default)]
    text: String,
    #[serde(default)]
    spoken: bool,
    #[serde(default)]
    conversation_id: Option<String>,
    #[serde(default)]
    attachments: Vec<TurnAttachment>,
}

pub fn parse_send(args: &Value) -> Result<SendInput, String> {
    let wire: SendWire = serde_json::from_value(args.clone()).map_err(|_| "That message was not complete.".to_string())?;
    let text = wire.text.trim().to_string();
    if text.is_empty() && wire.attachments.is_empty() {
        return Err("There is nothing to send.".into());
    }
    if text.chars().count() > MAX_TEXT {
        return Err("That message is too long for the phone.".into());
    }
    if wire.attachments.len() > MAX_ATTACHMENTS {
        return Err(format!("Send at most {MAX_ATTACHMENTS} photos at a time."));
    }
    // A phone sends photos as data, never as a path on the desktop.
    for a in &wire.attachments {
        if a.kind != "image" || a.data_uri.as_deref().is_none_or(|d| !d.starts_with("data:image/")) || !a.path.is_empty() {
            return Err("The phone can send photos only.".into());
        }
        if a.data_uri.as_deref().map_or(0, str::len) > MAX_ATTACHMENT_BYTES {
            return Err("That photo is too big.".into());
        }
    }
    let conversation_id = wire.conversation_id.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
    Ok(SendInput { text, spoken: wire.spoken, conversation_id, attachments: wire.attachments })
}

fn mime_of(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// May the phone read this file? Only a picture Poiesis made or was given: inside the generated media
/// folder, or listed as an attachment or an artifact of some chat. Never an arbitrary path.
pub fn media_allowed(db: &Db, media_dir: &Path, path: &str) -> bool {
    let known = db.is_known_attachment(path).unwrap_or(false) || db.is_known_artifact_content(path).unwrap_or(false);
    if known {
        return true;
    }
    match (std::fs::canonicalize(path), std::fs::canonicalize(media_dir)) {
        (Ok(file), Ok(dir)) => file.starts_with(dir),
        _ => false,
    }
}

pub fn read_media(db: &Db, media_dir: &Path, path: &str) -> Result<Value, String> {
    if !media_allowed(db, media_dir, path) {
        return Err("That file is not available on the phone.".into());
    }
    let file = Path::new(path);
    let mime = mime_of(file).ok_or("The phone shows pictures only.")?;
    let size = std::fs::metadata(file).map_err(|_| "That file is gone.".to_string())?.len();
    if size > MAX_MEDIA_BYTES {
        return Err("That picture is too big for the phone.".into());
    }
    let bytes = std::fs::read(file).map_err(|_| "That file is gone.".to_string())?;
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(json!({ "mime": mime, "name": name, "data": base64::engine::general_purpose::STANDARD.encode(bytes) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("poiesis-phone-api-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_command_the_phone_may_send_is_handled_here() {
        // Keeps the list in `remote_proto` and the match in `AppBackend::call` in step: a name on the list with
        // no arm would answer "not available" to a command the phone thinks it has.
        let source = include_str!("phone_api.rs");
        for cmd in remote_proto::CALLS {
            assert!(source.contains(&format!("\"{cmd}\" =>")), "no arm for the call {cmd}");
        }
        for cmd in remote_proto::STREAMS {
            assert!(source.contains(&format!("\"{cmd}\" =>")), "no arm for the stream {cmd}");
        }
    }

    #[test]
    fn a_send_needs_words_or_a_photo_and_stays_inside_the_limits() {
        assert!(parse_send(&json!({ "text": "  " })).is_err());
        let ok = parse_send(&json!({ "text": " hi ", "conversation_id": " c1 " })).unwrap();
        assert_eq!(ok.text, "hi");
        assert_eq!(ok.conversation_id.as_deref(), Some("c1"));
        assert!(parse_send(&json!({ "text": "x".repeat(MAX_TEXT + 1) })).is_err());
        assert!(parse_send(&json!({})).is_err());
    }

    #[test]
    fn a_phone_sends_photos_as_data_never_as_a_path() {
        let photo = json!({ "kind": "image", "name": "a.jpg", "dataUri": "data:image/jpeg;base64,AAAA" });
        assert!(parse_send(&json!({ "text": "look", "attachments": [photo] })).is_ok());
        let path = json!({ "kind": "image", "name": "a.jpg", "path": "C:\\Users\\secret.png", "dataUri": "data:image/jpeg;base64,AA" });
        assert!(parse_send(&json!({ "text": "x", "attachments": [path] })).is_err(), "a desktop path is refused");
        let file = json!({ "kind": "file", "name": "a.txt", "dataUri": "data:text/plain;base64,AA" });
        assert!(parse_send(&json!({ "text": "x", "attachments": [file] })).is_err());
        let five = vec![photo.clone(); MAX_ATTACHMENTS + 1];
        assert!(parse_send(&json!({ "text": "x", "attachments": five })).is_err());
    }

    #[test]
    fn the_phone_reads_pictures_from_the_media_folder_and_nothing_else() {
        let db = Db::open_in_memory().unwrap();
        let media = tmp("media");
        let elsewhere = tmp("else");
        let picture = media.join("a.png");
        std::fs::write(&picture, [137, 80, 78, 71]).unwrap();
        let secret = elsewhere.join("passwords.png");
        std::fs::write(&secret, [1, 2, 3]).unwrap();

        let got = read_media(&db, &media, picture.to_str().unwrap()).unwrap();
        assert_eq!(got["mime"], "image/png");
        assert!(got["data"].as_str().unwrap().len() > 3);

        assert!(read_media(&db, &media, secret.to_str().unwrap()).is_err(), "outside the folder");
        let sneaky = media.join("..").join(elsewhere.file_name().unwrap()).join("passwords.png");
        assert!(read_media(&db, &media, sneaky.to_str().unwrap()).is_err(), "a path that climbs out");
        assert!(read_media(&db, &media, "C:\\Windows\\win.ini").is_err());
        // Not a picture, even inside the folder.
        let text = media.join("a.txt");
        std::fs::write(&text, "hi").unwrap();
        assert!(read_media(&db, &media, text.to_str().unwrap()).is_err());
    }

    #[test]
    fn a_picture_the_chat_knows_may_be_read_from_anywhere() {
        let db = Db::open_in_memory().unwrap();
        let media = tmp("media2");
        let other = tmp("other2");
        let conv = db.create_conversation("c", None, false).unwrap().id;
        let file = other.join("shot.png");
        std::fs::write(&file, [1]).unwrap();
        db.add_artifact(Some(&conv), "shot", "image", file.to_str().unwrap(), None).unwrap();
        assert!(read_media(&db, &media, file.to_str().unwrap()).is_ok());
    }

    #[test]
    fn a_chat_list_and_its_messages_leave_the_desktop_timeline_behind() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("Phone", None, false).unwrap().id;
        let all = conversations(&db).unwrap();
        assert_eq!(all.as_array().unwrap().len(), 1);
        assert_eq!(all[0]["title"], "Phone");
        assert!(all[0].get("overrides_json").is_none(), "only what a list row needs");
        let rows = messages(&db, &conv).unwrap();
        assert!(rows.as_array().unwrap().is_empty());
        let message: crate::db::Message = serde_json::from_value(json!({
            "id": "m", "conversation_id": conv, "role": "assistant", "content": "hello",
            "steps_json": "[big]", "plan_json": "{}", "created_at": 1
        }))
        .unwrap();
        let v = trim_message(&message);
        assert_eq!(v["content"], "hello");
        assert!(v.get("steps_json").is_none() && v.get("plan_json").is_none());
    }

    #[test]
    fn the_feed_carries_questions_and_ends_of_turns_not_every_token() {
        let turn = |t: &str| json!({ "conversation_id": "c", "event": { "type": t } });
        assert!(feed_carries(TURN_EVENT, &turn("permission")));
        assert!(feed_carries(TURN_EVENT, &turn("permission_gone")));
        assert!(feed_carries(TURN_EVENT, &turn("turn_finalized")));
        assert!(!feed_carries(TURN_EVENT, &turn("token")));
        assert!(!feed_carries(TURN_EVENT, &Value::Null));
        assert!(feed_carries(crate::waiting::AWAY_EVENT, &Value::Null));
    }

    #[test]
    fn a_text_argument_must_be_there_and_short() {
        assert_eq!(text_arg(&json!({ "a": " b " }), "a").unwrap(), "b");
        assert!(text_arg(&json!({}), "a").is_err());
        assert!(text_arg(&json!({ "a": "" }), "a").is_err());
        assert!(text_arg(&json!({ "a": "x".repeat(500) }), "a").is_err());
    }

    /// The phone reads these event types and nothing else (plan, II.2). The files in `mobile/src/fixtures`
    /// are what the desktop sends today. When an event changes shape this test fails; run it with
    /// `UPDATE_FIXTURES=1` to rewrite the files, and the phone's own test then shows what to adjust.
    #[test]
    fn the_events_the_phone_reads_match_the_files_the_phone_is_tested_with() {
        use crate::agent::AgentEvent;
        let request = crate::permissions::PermissionRequest::operation(
            "req1".into(),
            "Write to report.md".into(),
            "C:/Users/x/report.md".into(),
            crate::permissions::Mode::ReadWrite,
            Some("+ a new line".into()),
        );
        let events: Vec<(&str, AgentEvent)> = vec![
            ("turn_saved", AgentEvent::TurnSaved { user_message_id: Some("u1".into()), assistant_message_id: "a1".into() }),
            ("turn_finalized", AgentEvent::TurnFinalized { message_id: "a1".into(), steps_json: None }),
            ("token", AgentEvent::Token { text: "Hello".into() }),
            ("step_start", AgentEvent::StepStart { id: "s1".into(), verb: "searched".into(), target: "the web".into(), parent: None }),
            ("error", AgentEvent::Error { message: "No model is running.".into() }),
            ("permission", AgentEvent::Permission { request }),
            ("permission_gone", AgentEvent::PermissionGone { id: "req1".into() }),
            (
                "artifact",
                AgentEvent::Artifact { id: "art1".into(), title: "Notes".into(), kind: "markdown".into(), content: "# Notes".into(), meta_json: None },
            ),
            (
                "block",
                AgentEvent::Block { id: "b1".into(), message_id: Some("a1".into()), kind: "plan".into(), title: "Plan".into(), data: json!({}) },
            ),
            ("sub", AgentEvent::Sub { run_id: "r1".into(), event: Box::new(AgentEvent::Token { text: "x".into() }) }),
        ];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mobile/src/fixtures");
        for (name, event) in events {
            let now = serde_json::to_value(&event).unwrap();
            assert_eq!(now["type"], name, "the event is named {name}");
            let file = dir.join(format!("{name}.json"));
            if std::env::var("UPDATE_FIXTURES").is_ok() {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(&file, serde_json::to_string_pretty(&now).unwrap() + "\n").unwrap();
            }
            let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap_or_else(|_| panic!("{file:?} is missing; run with UPDATE_FIXTURES=1")))
                .unwrap();
            assert_eq!(saved, now, "{name}.json is out of date; run with UPDATE_FIXTURES=1");
        }
    }
}
