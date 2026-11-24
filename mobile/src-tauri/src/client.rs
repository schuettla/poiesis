//! REM-13: the phone's end of the link. It holds the phone's key, the desktop it is paired with and one
//! iroh connection, and speaks the frames of `remote-proto`. It knows nothing about Tauri, so it can be
//! tested on a computer. The page never sees a key or an address.

use std::path::PathBuf;
use std::time::Duration;

use iroh::endpoint::{presets, Connection, VarInt};
use iroh::{Endpoint, EndpointAddr, RelayUrl, SecretKey, TransportAddr};
use remote_proto::{
    parse_reply, read_frame, write_audio, write_json, Frame, Hello, Payload, Reply, Request, ALPN, HELLO, MAX_JSON,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

/// How long to wait for the desktop to answer a connect.
#[cfg(not(test))]
const CONNECT_WAIT: Duration = Duration::from_secs(20);
#[cfg(test)]
const CONNECT_WAIT: Duration = Duration::from_secs(8);
/// How long a plain call may take.
const CALL_WAIT: Duration = Duration::from_secs(30);

pub const NOT_REACHED: &str = "I can't reach your desktop. It may be asleep, or Poiesis may not be running.";

/// The desktop this phone is paired with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Desktop {
    pub id: String,
    pub relay: Option<String>,
    pub name: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    /// The phone's own key, hex. It stays in the app's private folder.
    secret: String,
    desktop: Option<Desktop>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkState {
    Connecting,
    Connected,
    NotFound,
}

#[derive(Debug, Serialize)]
pub struct State {
    pub paired: bool,
    pub name: Option<String>,
    pub link: LinkState,
}

struct Inner {
    saved: Saved,
    endpoint: Option<Endpoint>,
    conn: Option<Connection>,
}

pub struct Link {
    file: PathBuf,
    /// Test only: no relay and no public lookup.
    offline: bool,
    inner: Mutex<Inner>,
    /// The upstream half of the live voice stream: the microphone goes down it, one frame at a time.
    voice: Mutex<Option<iroh::endpoint::SendStream>>,
    state: std::sync::Mutex<LinkState>,
    on_state: Box<dyn Fn(LinkState) + Send + Sync>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

impl Link {
    /// Read the saved key and desktop from `dir`, or make a key. The file is private to the app.
    pub fn open(dir: PathBuf, offline: bool, on_state: Box<dyn Fn(LinkState) + Send + Sync>) -> Result<Link, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join("phone.json");
        let mut saved: Saved = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        if unhex(&saved.secret).is_none() {
            saved.secret = hex(&SecretKey::generate().to_bytes());
            write(&file, &saved)?;
        }
        Ok(Link {
            file,
            offline,
            inner: Mutex::new(Inner { saved, endpoint: None, conn: None }),
            voice: Mutex::new(None),
            state: std::sync::Mutex::new(LinkState::Connecting),
            on_state,
        })
    }

    pub async fn state(&self) -> State {
        let inner = self.inner.lock().await;
        State {
            paired: inner.saved.desktop.is_some(),
            name: inner.saved.desktop.as_ref().map(|d| d.name.clone()),
            link: *self.state.lock().unwrap(),
        }
    }

    fn set_state(&self, state: LinkState) {
        let changed = {
            let mut now = self.state.lock().unwrap();
            let changed = *now != state;
            *now = state;
            changed
        };
        if changed {
            (self.on_state)(state);
        }
    }

    /// The phone's public key, for tests and for nothing else.
    #[cfg(test)]
    pub async fn id(&self) -> String {
        let inner = self.inner.lock().await;
        SecretKey::from_bytes(&unhex(&inner.saved.secret).unwrap()).public().to_string()
    }

    async fn endpoint(&self, inner: &mut Inner) -> Result<Endpoint, String> {
        if let Some(endpoint) = &inner.endpoint {
            return Ok(endpoint.clone());
        }
        let key = SecretKey::from_bytes(&unhex(&inner.saved.secret).ok_or("The phone's key is damaged.")?);
        let builder = if self.offline { Endpoint::builder(presets::N0DisableRelay) } else { Endpoint::builder(presets::N0) };
        let endpoint = builder.secret_key(key).bind().await.map_err(|e| format!("I could not start the link. {e}"))?;
        inner.endpoint = Some(endpoint.clone());
        Ok(endpoint)
    }

    /// A live connection to the desktop, made when there is none. `NotFound` when it cannot be had.
    pub async fn connection(&self) -> Result<Connection, String> {
        let mut inner = self.inner.lock().await;
        if let Some(conn) = &inner.conn {
            if conn.close_reason().is_none() {
                return Ok(conn.clone());
            }
            inner.conn = None;
        }
        let desktop = inner.saved.desktop.clone().ok_or("This phone is not paired yet.")?;
        self.set_state(LinkState::Connecting);
        let endpoint = self.endpoint(&mut inner).await?;
        match dial(&endpoint, &desktop.id, desktop.relay.as_deref()).await {
            Ok(conn) => {
                inner.conn = Some(conn.clone());
                self.set_state(LinkState::Connected);
                Ok(conn)
            }
            Err(_) => {
                self.set_state(LinkState::NotFound);
                Err(NOT_REACHED.into())
            }
        }
    }

    async fn drop_connection(&self) {
        let mut inner = self.inner.lock().await;
        if let Some(conn) = inner.conn.take() {
            conn.close(VarInt::from_u32(0), b"reconnect");
        }
    }

    /// A stream to the desktop. One retry on a fresh connection when the old one has died, which is what
    /// a switch from Wi-Fi to mobile data looks like.
    async fn open_stream(&self) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream), String> {
        for _ in 0..2 {
            let conn = self.connection().await?;
            match tokio::time::timeout(Duration::from_secs(10), conn.open_bi()).await {
                Ok(Ok(streams)) => return Ok(streams),
                _ => self.drop_connection().await,
            }
        }
        self.set_state(LinkState::NotFound);
        Err(NOT_REACHED.into())
    }

    /// Pair with the desktop whose QR (or pasted text) this is. The first frame carries the code.
    pub async fn pair(&self, text: &str, label: &str) -> Result<String, String> {
        let payload = Payload::parse(text).ok_or("That is not a Poiesis pairing code.")?;
        let hello = Hello { code: payload.code.clone(), label: label.trim().chars().take(40).collect() };
        let mut inner = self.inner.lock().await;
        let endpoint = self.endpoint(&mut inner).await?;
        self.set_state(LinkState::Connecting);
        let conn = dial(&endpoint, &payload.id, payload.relay.as_deref()).await.map_err(|_| {
            self.set_state(LinkState::NotFound);
            NOT_REACHED.to_string()
        })?;
        let answer = async {
            let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
            let request = Request { cmd: HELLO.into(), args: serde_json::to_value(&hello).unwrap_or(Value::Null) };
            write_json(&mut send, &serde_json::to_value(request).unwrap_or(Value::Null)).await.map_err(|e| e.to_string())?;
            let _ = send.finish();
            match read_frame(&mut recv, MAX_JSON).await {
                Ok(Some(Frame::Json(v))) => Ok(parse_reply(&v)),
                _ => Err("The desktop did not answer.".to_string()),
            }
        };
        let reply = tokio::time::timeout(CALL_WAIT, answer).await.map_err(|_| "The desktop did not answer.".to_string())??;
        match reply {
            Some(Reply::Ok(v)) => {
                let name = v.get("name").and_then(Value::as_str).unwrap_or("your desktop").to_string();
                inner.saved.desktop = Some(Desktop { id: payload.id, relay: payload.relay, name: name.clone() });
                write(&self.file, &inner.saved)?;
                inner.conn = Some(conn);
                self.set_state(LinkState::Connected);
                Ok(name)
            }
            Some(Reply::Err(words)) => {
                conn.close(VarInt::from_u32(0), b"not paired");
                Err(words)
            }
            _ => Err("The desktop did not understand.".into()),
        }
    }

    /// One command, one answer.
    pub async fn call(&self, cmd: &str, args: Value) -> Result<Value, String> {
        let (mut send, mut recv) = self.open_stream().await?;
        let request = serde_json::to_value(Request { cmd: cmd.into(), args }).unwrap_or(Value::Null);
        let run = async {
            write_json(&mut send, &request).await.map_err(|e| e.to_string())?;
            let _ = send.finish();
            match read_frame(&mut recv, MAX_JSON).await {
                Ok(Some(Frame::Json(v))) => match parse_reply(&v) {
                    Some(Reply::Ok(v)) => Ok(v),
                    Some(Reply::Err(words)) => Err(words),
                    _ => Err("The desktop's answer was not understood.".into()),
                },
                _ => Err(NOT_REACHED.to_string()),
            }
        };
        match tokio::time::timeout(CALL_WAIT, run).await {
            Ok(result) => result,
            Err(_) => {
                self.drop_connection().await;
                Err(NOT_REACHED.into())
            }
        }
    }

    /// A command that answers with events until it is done. Each event goes to `on_event`.
    pub async fn stream(&self, cmd: &str, args: Value, mut on_event: impl FnMut(Value) + Send) -> Result<(), String> {
        let (mut send, mut recv) = self.open_stream().await?;
        let request = serde_json::to_value(Request { cmd: cmd.into(), args }).unwrap_or(Value::Null);
        write_json(&mut send, &request).await.map_err(|e| e.to_string())?;
        let _ = send.finish();
        loop {
            match read_frame(&mut recv, MAX_JSON).await {
                Ok(Some(Frame::Json(v))) => match parse_reply(&v) {
                    Some(Reply::Event(ev)) => on_event(ev),
                    Some(Reply::Done) => return Ok(()),
                    Some(Reply::Err(words)) => return Err(words),
                    _ => {}
                },
                // The link broke mid-turn. The turn goes on at the desktop; the chat has it when we are back.
                _ => {
                    self.drop_connection().await;
                    return Err(NOT_REACHED.into());
                }
            }
        }
    }

    /// The live voice stream (REM-19). The request goes out and the upstream half stays open for `voice_push`;
    /// events come back until the desktop ends the session or `voice_close` closes the stream.
    pub async fn voice_open(&self, args: Value, mut on_event: impl FnMut(Value) + Send) -> Result<(), String> {
        let (mut send, mut recv) = self.open_stream().await?;
        let request = serde_json::to_value(Request { cmd: "voice_start".into(), args }).unwrap_or(Value::Null);
        write_json(&mut send, &request).await.map_err(|e| e.to_string())?;
        *self.voice.lock().await = Some(send);
        let ended = loop {
            match read_frame(&mut recv, MAX_JSON).await {
                Ok(Some(Frame::Json(v))) => match parse_reply(&v) {
                    Some(Reply::Event(ev)) => on_event(ev),
                    Some(Reply::Done) => break Ok(()),
                    Some(Reply::Err(words)) => break Err(words),
                    _ => {}
                },
                _ => break Err(NOT_REACHED.to_string()),
            }
        };
        *self.voice.lock().await = None;
        ended
    }

    /// One batch of microphone audio, as the desktop's own window sends it.
    pub async fn voice_push(&self, bytes: &[u8]) -> Result<(), String> {
        let mut voice = self.voice.lock().await;
        let send = voice.as_mut().ok_or("Voice is not running.")?;
        write_audio(send, bytes).await.map_err(|e| e.to_string())
    }

    /// End the voice stream. The desktop stops listening and the open call returns.
    pub async fn voice_close(&self) {
        if let Some(mut send) = self.voice.lock().await.take() {
            let _ = send.finish();
        }
    }

    /// Forget the desktop: the key stays (it is only a name), the pairing goes.
    pub async fn forget(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        if let Some(conn) = inner.conn.take() {
            conn.close(VarInt::from_u32(0), b"forgotten");
        }
        inner.saved.desktop = None;
        write(&self.file, &inner.saved)?;
        self.set_state(LinkState::Connecting);
        Ok(())
    }
}

/// Reach the desktop by its key and, when known, its relay. Without a relay the public address lookup is
/// the way; with it the connection needs no lookup at all.
async fn dial(endpoint: &Endpoint, id: &str, relay: Option<&str>) -> Result<Connection, String> {
    let id = id.trim().parse().map_err(|_| "bad id".to_string())?;
    let relay: Option<RelayUrl> = relay.and_then(|r| r.trim().parse().ok());
    let addr = EndpointAddr::from_parts(id, relay.into_iter().map(TransportAddr::Relay));
    dial_addr(endpoint, addr).await
}

async fn dial_addr(endpoint: &Endpoint, addr: EndpointAddr) -> Result<Connection, String> {
    match tokio::time::timeout(CONNECT_WAIT, endpoint.connect(addr, ALPN)).await {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("timed out".into()),
    }
}

fn write(file: &PathBuf, saved: &Saved) -> Result<(), String> {
    let text = serde_json::to_string(saved).map_err(|e| e.to_string())?;
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loopback endpoints started all at once can miss their handshake window on a busy machine, so
    /// the tests that open them take turns.
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    use serde_json::json;
    use std::sync::{Arc, Mutex as StdMutex};

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("poiesis-companion-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn quiet() -> Box<dyn Fn(LinkState) + Send + Sync> {
        Box::new(|_| {})
    }

    /// A stand-in desktop: pairs anyone who says "GOODCODE", then answers `status` and streams two events
    /// for anything else. It listens on loopback only.
    async fn fake_desktop() -> (Endpoint, EndpointAddr) {
        let host = Endpoint::builder(presets::N0DisableRelay).alpns(vec![ALPN.to_vec()]).bind().await.unwrap();
        let port = host.bound_sockets().into_iter().find(|s| s.is_ipv4()).unwrap().port();
        let addr = EndpointAddr::from_parts(
            host.id(),
            [TransportAddr::Ip(std::net::SocketAddr::from(([127, 0, 0, 1], port)))],
        );
        let accepting = host.clone();
        tokio::spawn(async move {
            while let Some(incoming) = accepting.accept().await {
                tokio::spawn(async move {
                    let Ok(conn) = incoming.await else { return };
                    while let Ok((mut send, mut recv)) = conn.accept_bi().await {
                        let Ok(Some(Frame::Json(v))) = read_frame(&mut recv, MAX_JSON).await else { continue };
                        let request: Request = serde_json::from_value(v).unwrap();
                        if request.cmd == "voice_start" {
                            // Report every audio frame, and say done when the phone closes its side.
                            while let Ok(Some(frame)) = read_frame(&mut recv, MAX_JSON).await {
                                if let Frame::Audio(bytes) = frame {
                                    let _ = write_json(&mut send, &remote_proto::reply_event(json!({ "heard": bytes.len() }))).await;
                                }
                            }
                            let _ = write_json(&mut send, &remote_proto::reply_done()).await;
                            let _ = send.finish();
                            continue;
                        }
                        let replies: Vec<Value> = match request.cmd.as_str() {
                            "hello" if request.args["code"] == "GOODCODE" => vec![remote_proto::reply_ok(json!({ "name": "Test desktop" }))],
                            "hello" => vec![remote_proto::reply_err("That code is not right.")],
                            "send" => vec![
                                remote_proto::reply_event(json!({ "type": "token", "text": "he" })),
                                remote_proto::reply_event(json!({ "type": "token", "text": "llo" })),
                                remote_proto::reply_done(),
                            ],
                            "nope" => vec![remote_proto::reply_err("That is not available on the phone.")],
                            other => vec![remote_proto::reply_ok(json!({ "cmd": other }))],
                        };
                        for r in replies {
                            let _ = write_json(&mut send, &r).await;
                        }
                        let _ = send.finish();
                    }
                });
            }
        });
        (host, addr)
    }

    /// The QR text for the stand-in desktop. The direct address is used instead of a relay, so the phone
    /// under test is given it through a payload with no relay and a connect by address is simulated by
    /// dialing the address directly.
    async fn pair_with(link: &Link, host: &Endpoint, addr: &EndpointAddr, code: &str) -> Result<String, String> {
        // Dial by address (a loopback test has no relay), then run the same hello a real pair runs.
        let mut inner = link.inner.lock().await;
        let endpoint = link.endpoint(&mut inner).await?;
        let conn = dial_addr(&endpoint, addr.clone()).await?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        let hello = Hello { code: code.into(), label: "Pixel".into() };
        write_json(&mut send, &json!({ "cmd": "hello", "args": hello })).await.unwrap();
        let _ = send.finish();
        let Ok(Some(Frame::Json(v))) = read_frame(&mut recv, MAX_JSON).await else { return Err("no answer".into()) };
        match parse_reply(&v) {
            Some(Reply::Ok(v)) => {
                let name = v["name"].as_str().unwrap().to_string();
                inner.saved.desktop = Some(Desktop { id: host.id().to_string(), relay: None, name: name.clone() });
                write(&link.file, &inner.saved)?;
                inner.conn = Some(conn);
                Ok(name)
            }
            Some(Reply::Err(w)) => Err(w),
            _ => Err("?".into()),
        }
    }

    #[tokio::test]
    async fn the_phone_keeps_its_identity_and_its_pairing_across_restarts() {
        let _turn = SERIAL.lock().await;
        let d = dir("identity");
        let first = Link::open(d.clone(), true, quiet()).unwrap();
        let id = first.id().await;
        assert!(!first.state().await.paired);
        let again = Link::open(d.clone(), true, quiet()).unwrap();
        assert_eq!(again.id().await, id, "the same key after a restart");

        let (host, addr) = fake_desktop().await;
        assert_eq!(pair_with(&again, &host, &addr, "GOODCODE").await.unwrap(), "Test desktop");
        let third = Link::open(d, true, quiet()).unwrap();
        let state = third.state().await;
        assert!(state.paired);
        assert_eq!(state.name.as_deref(), Some("Test desktop"));
    }

    #[tokio::test]
    async fn a_wrong_code_is_said_in_words_and_pairs_nothing() {
        let _turn = SERIAL.lock().await;
        let link = Link::open(dir("wrong"), true, quiet()).unwrap();
        let (host, addr) = fake_desktop().await;
        assert_eq!(pair_with(&link, &host, &addr, "WRONG").await, Err("That code is not right.".into()));
        assert!(!link.state().await.paired);
    }

    #[tokio::test]
    async fn a_pairing_text_that_is_not_ours_is_refused_before_any_network() {
        let link = Link::open(dir("junk"), true, quiet()).unwrap();
        assert_eq!(link.pair("hello", "x").await, Err("That is not a Poiesis pairing code.".into()));
        assert_eq!(link.pair(r#"{"v":9,"id":"a","code":"b"}"#, "x").await, Err("That is not a Poiesis pairing code.".into()));
    }

    #[tokio::test]
    async fn a_call_gets_one_answer_and_a_stream_gets_its_events() {
        let _turn = SERIAL.lock().await;
        let link = Link::open(dir("calls"), true, quiet()).unwrap();
        let (host, addr) = fake_desktop().await;
        pair_with(&link, &host, &addr, "GOODCODE").await.unwrap();

        let answer = link.call("status", json!({})).await.unwrap();
        assert_eq!(answer["cmd"], "status");
        assert_eq!(link.call("nope", json!({})).await, Err("That is not available on the phone.".into()));

        let mut seen = Vec::new();
        link.stream("send", json!({ "text": "hi" }), |e| seen.push(e["text"].as_str().unwrap().to_string())).await.unwrap();
        assert_eq!(seen, vec!["he", "llo"]);
    }

    #[tokio::test]
    async fn an_unpaired_phone_says_so_and_a_missing_desktop_is_not_found() {
        let _turn = SERIAL.lock().await;
        let states = Arc::new(StdMutex::new(Vec::new()));
        let seen = states.clone();
        let link = Link::open(dir("gone"), true, Box::new(move |s| seen.lock().unwrap().push(s))).unwrap();
        assert_eq!(link.call("status", json!({})).await, Err("This phone is not paired yet.".into()));

        // Paired with a desktop that then goes away.
        let (host, addr) = fake_desktop().await;
        pair_with(&link, &host, &addr, "GOODCODE").await.unwrap();
        host.close().await;
        link.drop_connection().await;
        let result = tokio::time::timeout(Duration::from_secs(60), link.call("status", json!({}))).await.unwrap();
        assert_eq!(result, Err(NOT_REACHED.into()));
        assert_eq!(link.state().await.link, LinkState::NotFound);
        assert!(states.lock().unwrap().contains(&LinkState::NotFound));
    }

    #[tokio::test]
    async fn the_microphone_goes_down_the_voice_stream_until_the_phone_closes_it() {
        let _turn = SERIAL.lock().await;
        let link = std::sync::Arc::new(Link::open(dir("voice"), true, quiet()).unwrap());
        let (host, addr) = fake_desktop().await;
        pair_with(&link, &host, &addr, "GOODCODE").await.unwrap();
        assert_eq!(link.voice_push(&[1, 2, 3]).await, Err("Voice is not running.".into()), "nothing to push to yet");

        let heard = std::sync::Arc::new(StdMutex::new(Vec::new()));
        let open = {
            let (link, heard) = (link.clone(), heard.clone());
            tokio::spawn(async move { link.voice_open(json!({}), |e| heard.lock().unwrap().push(e["heard"].as_u64().unwrap())).await })
        };
        // The stream needs a moment to open before audio can go down it.
        for _ in 0..50 {
            if link.voice.lock().await.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        link.voice_push(&[0u8; 64]).await.unwrap();
        link.voice_push(&[0u8; 32]).await.unwrap();
        for _ in 0..100 {
            if heard.lock().unwrap().len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(*heard.lock().unwrap(), vec![64, 32]);
        link.voice_close().await;
        assert_eq!(open.await.unwrap(), Ok(()), "the call returns when the desktop ends the session");
        assert_eq!(link.voice_push(&[1]).await, Err("Voice is not running.".into()));
    }

    #[tokio::test]
    async fn forgetting_the_desktop_unpairs_the_phone() {
        let _turn = SERIAL.lock().await;
        let link = Link::open(dir("forget"), true, quiet()).unwrap();
        let (host, addr) = fake_desktop().await;
        pair_with(&link, &host, &addr, "GOODCODE").await.unwrap();
        link.forget().await.unwrap();
        assert!(!link.state().await.paired);
    }
}
