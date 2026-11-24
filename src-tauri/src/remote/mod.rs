//! Remote control: the gateway that lets a phone (and later a Telegram bot) use this Poiesis.
//! See `plans/REMOTE_PLAN.md`. Nothing here is reachable from outside until the phone link exists.
//!
//! One gateway, thin adapters. An adapter (the phone link, later Telegram) hands `Gateway::on_message`
//! a paired sender's text and a channel to read the turn's events from. The turn itself is the
//! window's own `run_turn`, so the chat is an ordinary conversation on the desktop.

pub mod approvals;
pub mod cmd;
pub mod iroh_host;
pub mod model;
pub mod pairing;
pub mod phone_api;
pub mod render;
pub mod session;
pub mod telegram;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::json;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{broadcast, mpsc::UnboundedSender, watch};

use crate::agent::fleet::Steer;
use crate::agent::turn::{TurnAttachment, TurnInput};
use crate::agent::AgentEvent;
use crate::commands::agent::TurnEnv;
use crate::db::{now_ms, Db};
use pairing::{Codes, PairError, Sender};
use session::Busy;

/// The kill switch. Off unless the person turned it on in Settings > Remote.
pub const ENABLED_KEY: &str = "remote.enabled";
/// App event for the window: `{enabled, connected}`.
pub const STATUS_EVENT: &str = "poiesis-remote";
/// App event: every event of a remote turn, as `{conversation_id, event}`, so the desktop can show
/// a turn it did not start.
pub const TURN_EVENT: &str = "poiesis-remote-turn";
/// The kind of the audit rows in `session_events`.
pub const AUDIT_KIND: &str = "remote";

/// What came of a message.
#[derive(Debug, PartialEq, Eq)]
pub enum Intake {
    /// A turn ran to its end. Its events went to the sender's channel.
    Ran,
    /// A turn was already running: the text was added to it as steering.
    Steered,
    Refused(Refusal),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Remote is switched off.
    Off,
    /// Not a paired sender. An adapter answers this with silence.
    Unpaired,
    /// The last message is still being started.
    Busy,
    /// No model to run on. The line is fit to show in a chat.
    Model(String),
}

impl Refusal {
    /// One plain line for the sender, or `None` where silence is the answer.
    pub fn words(&self) -> Option<String> {
        match self {
            Refusal::Off => Some("Remote is switched off on the desktop.".into()),
            Refusal::Unpaired => None,
            Refusal::Busy => Some("I am still starting your last message. Try again in a moment.".into()),
            Refusal::Model(line) => Some(line.clone()),
        }
    }
}

/// Why a pairing did not happen.
#[derive(Debug, PartialEq, Eq)]
pub enum PairFail {
    Off,
    Code(PairError),
    Save(String),
}

impl PairFail {
    pub fn words(&self) -> String {
        match self {
            PairFail::Off => "Remote is switched off on the desktop.".into(),
            PairFail::Code(e) => e.words().into(),
            PairFail::Save(e) => format!("I could not save the pairing. {e}"),
        }
    }
}

/// How a phone finds this desktop: its public key and the relay it is reached through. Known once
/// the phone link has started (`iroh_host`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostAddr {
    pub id: String,
    pub relay: Option<String>,
}

pub struct Gateway {
    app: AppHandle,
    codes: Codes,
    busy: Busy,
    /// Questions sent out and not yet answered (`REM-4`).
    pending: Arc<approvals::Pending>,
    enabled: watch::Sender<bool>,
    /// Live links, counted by the adapters (`link_up`, `link_down`).
    links: AtomicUsize,
    /// A sender id that was removed: its open links are closed at once.
    kicks: broadcast::Sender<String>,
    /// Where the phone link can be reached, for the pairing code.
    host_addr: watch::Sender<Option<HostAddr>>,
}

impl Gateway {
    /// Reads the switch from the settings. Needs `app.manage(db)` first.
    pub fn new(app: AppHandle) -> Self {
        let on = app
            .try_state::<Db>()
            .and_then(|db| db.get_setting(ENABLED_KEY).ok().flatten())
            .is_some_and(|v| v == "true");
        Gateway {
            app,
            codes: Codes::default(),
            busy: Busy::default(),
            pending: Arc::default(),
            enabled: watch::channel(on).0,
            links: AtomicUsize::new(0),
            kicks: broadcast::channel(8).0,
            host_addr: watch::channel(None).0,
        }
    }

    /// The link to this sender must close now (it was removed in the window).
    pub fn kick(&self, id: &str) {
        let _ = self.kicks.send(id.to_string());
    }

    pub fn watch_kicks(&self) -> broadcast::Receiver<String> {
        self.kicks.subscribe()
    }

    /// The phone link says where it can be reached (or `None` when it is down).
    pub fn set_host_addr(&self, addr: Option<HostAddr>) {
        publish_addr(&self.host_addr, addr);
    }

    pub fn watch_host_addr(&self) -> watch::Receiver<Option<HostAddr>> {
        self.host_addr.subscribe()
    }

    /// Where the phone link can be reached. Waits up to `wait` for the relay to be known, then answers
    /// with what there is.
    pub async fn reachable(&self, wait: std::time::Duration) -> Option<HostAddr> {
        let mut rx = self.host_addr.subscribe();
        let _ = tokio::time::timeout(wait, rx.wait_for(|a| a.as_ref().is_some_and(|a| a.relay.is_some()))).await;
        let addr = rx.borrow().clone();
        addr
    }

    /// A pairing code is on the screen right now: a stranger may knock, with that code.
    pub fn pairing_open(&self) -> bool {
        self.is_enabled() && self.codes.is_open(now_ms())
    }

    pub fn is_enabled(&self) -> bool {
        *self.enabled.borrow()
    }

    /// An adapter watches this to start and stop within a second of the switch.
    pub fn watch_enabled(&self) -> watch::Receiver<bool> {
        self.enabled.subscribe()
    }

    pub fn is_connected(&self) -> bool {
        self.links.load(Ordering::Relaxed) > 0
    }

    pub fn link_up(&self) {
        self.links.fetch_add(1, Ordering::Relaxed);
        self.announce();
    }

    pub fn link_down(&self) {
        let _ = self.links.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1));
        self.announce();
    }

    /// Tell the window (the mark, the Remote tab) and the tray.
    fn announce(&self) {
        let (enabled, connected) = (self.is_enabled(), self.is_connected());
        let _ = self.app.emit(STATUS_EVENT, json!({ "enabled": enabled, "connected": connected }));
        crate::tray::sync_remote(&self.app, enabled);
    }

    /// Call once after setup, so the window and the tray start in step with the setting.
    pub fn announce_start(&self) {
        self.announce();
    }

    /// Something the Remote tab shows changed (the Telegram bot connected, or failed): tell the window.
    pub fn status_changed(&self) {
        self.announce();
    }

    /// The kill switch. Off: no code can be redeemed, and every remote turn is stopped.
    pub fn set_enabled(&self, on: bool) -> Result<(), String> {
        let db = self.app.state::<Db>();
        db.set_setting(ENABLED_KEY, if on { "true" } else { "false" }).map_err(|e| e.to_string())?;
        // `send_replace`: the value is kept even when nobody is listening at this moment.
        self.enabled.send_replace(on);
        if !on {
            self.codes.clear();
            self.stop_all();
        }
        self.announce();
        Ok(())
    }

    /// A new pairing code. Only while remote is on.
    pub fn new_code(&self) -> Result<pairing::PairingCode, String> {
        if !self.is_enabled() {
            return Err("Turn remote on first.".into());
        }
        Ok(self.codes.issue(now_ms()))
    }

    /// The code is off the screen (the dialog closed).
    pub fn cancel_code(&self) {
        self.codes.clear();
    }

    /// Pair a sender that proves it has the code on the desktop's screen.
    pub fn pair(&self, transport: &str, id: &str, label: &str, code: &str) -> Result<Sender, PairFail> {
        let sender =
            pair_sender(&self.app.state::<Db>(), &self.codes, self.is_enabled(), transport, id, label, code, now_ms())?;
        // The Remote tab lists the new phone.
        self.announce();
        Ok(sender)
    }

    /// Make an existing chat the sender's active one, so the phone can carry on an older chat.
    pub fn open_chat(&self, transport: &str, id: &str, conversation_id: &str) -> Result<(), String> {
        let db = self.app.state::<Db>();
        db.get_conversation(conversation_id).map_err(|e| e.to_string())?.ok_or("That chat is gone.")?;
        pairing::update(&db, transport, id, |s| s.conversation_id = Some(conversation_id.to_string()))
    }

    /// Stop every turn this gateway started.
    fn stop_all(&self) {
        let fleet = self.app.state::<crate::agent::fleet::Fleet>();
        for run in fleet.roots() {
            if self.busy.is_busy(&run.conversation_id) {
                fleet.cancel_tree(&run.id);
            }
        }
    }

    /// Stop the sender's running turn, if any (`/stop`). `true` when there was one.
    pub fn stop(&self, transport: &str, id: &str) -> bool {
        let db = self.app.state::<Db>();
        let Some(conv) = pairing::find(&db, transport, id).and_then(|s| s.conversation_id) else {
            return false;
        };
        let fleet = self.app.state::<crate::agent::fleet::Fleet>();
        let runs: Vec<_> = fleet.roots().into_iter().filter(|r| r.conversation_id == conv).collect();
        for run in &runs {
            fleet.cancel_tree(&run.id);
        }
        !runs.is_empty()
    }

    /// Start another chat for the sender (`/new`).
    pub fn new_chat(&self, transport: &str, id: &str) -> Result<String, String> {
        let db = self.app.state::<Db>();
        let sender = pairing::find(&db, transport, id).ok_or("Not a paired sender.")?;
        session::new_conversation(&db, &sender)
    }

    /// A paired sender's text. Runs the turn to its end, sending each event to `events` and to the
    /// window (`poiesis-remote-turn`). A message that arrives while a turn runs steers it.
    pub async fn on_message(
        &self,
        transport: &str,
        id: &str,
        text: String,
        attachments: Vec<TurnAttachment>,
        spoken: bool,
        events: UnboundedSender<serde_json::Value>,
    ) -> Intake {
        let db = self.app.state::<Db>();
        let sender = match admit(&db, self.is_enabled(), transport, id) {
            Ok(sender) => sender,
            Err(refusal) => return Intake::Refused(refusal),
        };
        let conversation_id = match session::conversation_for(&db, &sender) {
            Ok(id) => id,
            Err(e) => return Intake::Refused(Refusal::Model(format!("I could not open a chat. {e}"))),
        };

        // A turn already runs in this chat (started here or in the window): the text steers it.
        let fleet = self.app.state::<crate::agent::fleet::Fleet>();
        if let Some(run) = fleet.roots().into_iter().find(|r| r.conversation_id == conversation_id) {
            run.steer(Steer::user(text));
            return Intake::Steered;
        }
        let Some(_turn) = self.busy.try_enter(&conversation_id) else {
            return Intake::Refused(Refusal::Busy);
        };

        let mgr = self.app.state::<crate::runtime::RuntimeManager>();
        let resolved = match model::default_turn_model(&db, &mgr).await {
            Ok(r) => r,
            Err(line) => return Intake::Refused(Refusal::Model(line)),
        };

        audit(&db, &conversation_id, &sender, "turn", &text);
        // The first message names the chat, as it does for a chat typed at the desktop.
        if db.list_messages(&conversation_id).is_ok_and(|m| m.is_empty()) {
            if let Some(title) = session::title_from(&text) {
                let _ = db.rename_conversation(&conversation_id, &title);
            }
        }

        let input = TurnInput {
            conversation_id: conversation_id.clone(),
            text,
            action: None,
            attachments,
            refs: Vec::new(),
            pending_actions: Vec::new(),
            last_time: None,
            spoken,
            model: resolved.model,
            target: resolved.target,
            options: crate::agent::run::RunOptions { remote: true, ..Default::default() },
            tools_enabled: true,
            saved: None,
        };
        let channel = self.tee(conversation_id, &sender, events);
        let embed = self.app.state::<crate::runtime::EmbedManager>();
        let rerank = self.app.state::<crate::runtime::RerankManager>();
        let perms = self.app.state::<crate::permissions::PermissionManager>();
        let memory = self.app.state::<crate::memory::MemoryStore>();
        let pool = self.app.state::<crate::agent::browser::BrowserPool>();
        let grants = self.app.state::<crate::commands::files::DialogGrants>();
        let env = TurnEnv {
            mgr: &mgr,
            embed_mgr: &embed,
            rerank_mgr: &rerank,
            db: &db,
            perms: &perms,
            memory: &memory,
            browser_pool: &pool,
            fleet: &fleet,
            grants: &grants,
        };
        crate::agent::turn::run_turn(&env, input, channel).await;
        Intake::Ran
    }

    /// The channel a turn writes to: each event goes to the adapter and, wrapped with its chat, to
    /// the window.
    ///
    /// A `permission` event is a question for the sender (`REM-4`): it is noted as theirs, and a ten
    /// minute clock starts. When the clock wins, the run is told no and the chat is told why.
    fn tee(&self, conversation_id: String, sender: &Sender, events: UnboundedSender<serde_json::Value>) -> Channel<AgentEvent> {
        let app = self.app.clone();
        let pending = self.pending.clone();
        let sender = sender.clone();
        Channel::new(move |body: InvokeResponseBody| {
            if let InvokeResponseBody::Json(text) = body {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(&text) {
                    if event["type"] == "permission" {
                        if let Some(request_id) = event["request"]["id"].as_str() {
                            let owner = approvals::Owner {
                                transport: sender.transport.clone(),
                                id: sender.id.clone(),
                                conversation_id: conversation_id.clone(),
                            };
                            pending.insert(request_id, owner, events.clone());
                            watch_clock(app.clone(), pending.clone(), request_id.to_string(), sender.clone(), events.clone());
                        }
                    }
                    let _ = app.emit(TURN_EVENT, json!({ "conversation_id": conversation_id, "event": event }));
                    let _ = events.send(event);
                }
            }
            Ok(())
        })
    }

    /// The sender's answer to a question of theirs: a button, yes once or no. A question that is
    /// not theirs, or no longer open, is refused. The desktop's copy of the card is closed.
    pub fn answer(&self, transport: &str, id: &str, request_id: &str, allow: bool) -> Result<(), String> {
        let db = self.app.state::<Db>();
        admit(&db, self.is_enabled(), transport, id).map_err(|r| r.words().unwrap_or_default())?;
        let owner = self
            .pending
            .claim(request_id, transport, id)
            .ok_or("That question is not open any more.")?;
        let perms = self.app.state::<crate::permissions::PermissionManager>();
        // The run already went on (it was stopped): a tap must not report an answer that changed nothing.
        if !perms.is_pending(request_id) {
            return Err("That question is not open any more.".into());
        }
        perms.resolve(request_id, approvals::decision_for(allow));
        if let Some(sender) = pairing::find(&db, transport, id) {
            audit(&db, &owner.conversation_id, &sender, if allow { "allowed" } else { "declined" }, request_id);
        }
        let _ = self.app.emit(
            TURN_EVENT,
            json!({ "conversation_id": owner.conversation_id, "event": { "type": "permission_gone", "id": request_id } }),
        );
        Ok(())
    }

    /// The desktop answered a question. If a remote sender had it too, its card closes there: in the
    /// chat that asked (the turn's own line) and on other phones (the feed).
    pub fn answered_at_desktop(&self, request_id: &str) {
        let Some((owner, events)) = self.pending.answered_elsewhere(request_id) else { return };
        let gone = json!({ "type": "permission_gone", "id": request_id });
        let _ = events.send(gone.clone());
        let _ = self.app.emit(TURN_EVENT, json!({ "conversation_id": owner.conversation_id, "event": gone }));
    }
}

/// Start the ten minute clock on a question. When it runs out unanswered the run is told no, the
/// chat is told why, and the audit has a line.
fn watch_clock(
    app: AppHandle,
    pending: Arc<approvals::Pending>,
    request_id: String,
    sender: Sender,
    events: UnboundedSender<serde_json::Value>,
) {
    tauri::async_runtime::spawn(async move {
        let perms = app.state::<crate::permissions::PermissionManager>();
        if let Some(owner) = approvals::expire(&pending, &perms, &request_id, approvals::EXPIRY).await {
            let _ = events.send(json!({ "type": "remote_notice", "text": approvals::EXPIRED_WORDS }));
            let _ = app.emit(
                TURN_EVENT,
                json!({ "conversation_id": owner.conversation_id, "event": { "type": "permission_gone", "id": request_id } }),
            );
            audit(&app.state::<Db>(), &owner.conversation_id, &sender, "expired", &request_id);
        }
    });
}

/// Keep the address for whoever asks later. `watch::Sender::send` drops the value when no receiver exists, and the
/// pairing dialog subscribes only when it is opened, so the address is stored with `send_replace`.
fn publish_addr(tx: &watch::Sender<Option<HostAddr>>, addr: Option<HostAddr>) {
    tx.send_replace(addr);
}

/// "Last seen" is kept to the minute. A phone asks many times a second in voice mode, and each note is a settings write.
const SEEN_EVERY_MS: i64 = 60_000;

/// Who may talk: remote must be on and the sender paired. A paired sender is noted as seen.
pub(crate) fn admit(db: &Db, enabled: bool, transport: &str, id: &str) -> Result<Sender, Refusal> {
    if !enabled {
        return Err(Refusal::Off);
    }
    let Some(sender) = pairing::find(db, transport, id) else {
        return Err(Refusal::Unpaired);
    };
    let now = now_ms();
    if now - sender.last_seen >= SEEN_EVERY_MS {
        let _ = pairing::update(db, transport, id, |s| s.last_seen = now);
    }
    Ok(sender)
}

/// Pairing: the switch is on and the code is the one on the screen.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pair_sender(
    db: &Db,
    codes: &Codes,
    enabled: bool,
    transport: &str,
    id: &str,
    label: &str,
    code: &str,
    now: i64,
) -> Result<Sender, PairFail> {
    if !enabled {
        return Err(PairFail::Off);
    }
    codes.redeem(code, now).map_err(PairFail::Code)?;
    let sender = Sender {
        transport: transport.into(),
        id: id.into(),
        label: label.trim().chars().take(40).collect(),
        paired_at: now,
        last_seen: now,
        level: "chat".into(),
        conversation_id: None,
    };
    pairing::add(db, sender.clone()).map_err(PairFail::Save)?;
    Ok(sender)
}

/// One audit row per remote turn (and, in REM-4, per approval). What was asked is cut short.
pub fn audit(db: &Db, conversation_id: &str, sender: &Sender, what: &str, text: &str) {
    let asked: String = text.chars().take(200).collect();
    let _ = db.append_session_event(
        conversation_id,
        None,
        AUDIT_KIND,
        &json!({ "sender": sender.label, "transport": sender.transport, "what": what, "text": asked }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_is_kept_when_nobody_is_listening_yet() {
        let (tx, rx) = watch::channel(None::<HostAddr>);
        drop(rx); // the gateway holds only the sender
        publish_addr(&tx, Some(HostAddr { id: "k".into(), relay: None }));
        assert_eq!(tx.subscribe().borrow().as_ref().map(|a| a.id.as_str()), Some("k"));
        publish_addr(&tx, None);
        assert!(tx.subscribe().borrow().is_none());
    }

    #[test]
    fn an_audit_row_names_the_sender_and_cuts_the_text() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("Phone", None, false).unwrap().id;
        let sender = Sender {
            transport: "phone".into(),
            id: "k1".into(),
            label: "Pixel".into(),
            paired_at: 1,
            last_seen: 1,
            level: "chat".into(),
            conversation_id: Some(conv.clone()),
        };
        audit(&db, &conv, &sender, "turn", &"x".repeat(500));
        let rows = db.session_events(&conv).unwrap();
        let row = rows.iter().find(|r| r.kind == AUDIT_KIND).expect("an audit row");
        let v: serde_json::Value = serde_json::from_str(&row.payload_json).unwrap();
        assert_eq!(v["sender"], "Pixel");
        assert_eq!(v["transport"], "phone");
        assert_eq!(v["what"], "turn");
        assert_eq!(v["text"].as_str().unwrap().len(), 200);
    }

    #[test]
    fn nobody_is_let_in_while_remote_is_off_or_unless_paired() {
        let db = Db::open_in_memory().unwrap();
        let codes = Codes::default();
        let code = codes.issue(0).code;
        // Off: not even pairing works, and the code stays unused.
        assert_eq!(pair_sender(&db, &codes, false, "phone", "k1", "Pixel", &code, 1), Err(PairFail::Off));
        let paired = pair_sender(&db, &codes, true, "phone", "k1", "  Pixel  ", &code, 1).unwrap();
        assert_eq!(paired.label, "Pixel");
        assert_eq!(paired.level, "chat");

        assert_eq!(admit(&db, false, "phone", "k1").unwrap_err(), Refusal::Off, "the kill switch");
        assert_eq!(admit(&db, true, "phone", "stranger").unwrap_err(), Refusal::Unpaired);
        assert_eq!(admit(&db, true, "telegram", "k1").unwrap_err(), Refusal::Unpaired);
        assert_eq!(admit(&db, true, "phone", "k1").unwrap().id, "k1");
    }

    #[test]
    fn a_code_cannot_pair_two_phones_and_wrong_codes_pair_nobody() {
        let db = Db::open_in_memory().unwrap();
        let codes = Codes::default();
        let code = codes.issue(0).code;
        assert_eq!(
            pair_sender(&db, &codes, true, "phone", "k1", "A", "WRONGCODE", 1),
            Err(PairFail::Code(PairError::Wrong))
        );
        assert!(pair_sender(&db, &codes, true, "phone", "k1", "A", &code, 1).is_ok());
        assert_eq!(
            pair_sender(&db, &codes, true, "phone", "k2", "B", &code, 2),
            Err(PairFail::Code(PairError::NoCode)),
            "code reuse is refused"
        );
        assert_eq!(pairing::load(&db).len(), 1);
    }

    #[test]
    fn being_seen_is_noted() {
        let db = Db::open_in_memory().unwrap();
        let codes = Codes::default();
        let code = codes.issue(0).code;
        pair_sender(&db, &codes, true, "phone", "k1", "A", &code, 1).unwrap();
        let before = pairing::find(&db, "phone", "k1").unwrap().last_seen;
        assert_eq!(before, 1);
        admit(&db, true, "phone", "k1").unwrap();
        let seen = pairing::find(&db, "phone", "k1").unwrap().last_seen;
        assert!(seen > before);
        std::thread::sleep(std::time::Duration::from_millis(5));
        admit(&db, true, "phone", "k1").unwrap();
        assert_eq!(pairing::find(&db, "phone", "k1").unwrap().last_seen, seen, "not written again within the minute");
    }

    #[test]
    fn refusals_have_words_except_the_stranger() {
        assert!(Refusal::Off.words().is_some());
        assert!(Refusal::Busy.words().is_some());
        assert_eq!(Refusal::Model("x".into()).words().as_deref(), Some("x"));
        assert!(Refusal::Unpaired.words().is_none(), "an unknown sender gets silence");
    }
}
