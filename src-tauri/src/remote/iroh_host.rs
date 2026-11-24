//! REM-10: the desktop end of the phone link.
//!
//! One iroh endpoint, alive only while remote is switched on. A connection from a key that is not
//! paired is closed before any frame is read, with one exception: while a pairing code is on the
//! screen, a stranger may send one small `hello` frame (the code and a name). Everything else about
//! what a phone may do lives behind `Backend`, so these rules can be tested without the app.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iroh::endpoint::{presets, Connection, VarInt};
use iroh::{Endpoint, RelayMode, RelayUrl, SecretKey};
use remote_proto::{
    is_allowed, is_stream, read_frame, reply_done, reply_err, reply_event, reply_ok, write_json, Frame, Hello, Request,
    HELLO, MAX_HELLO, MAX_JSON,
};
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinSet;

use super::HostAddr;

/// How long a stranger has to knock once it is connected.
const KNOCK_WAIT: Duration = Duration::from_secs(10);
/// How long the kill switch lets the endpoint close politely.
const CLOSE_WAIT: Duration = Duration::from_secs(1);

const CLOSE_NOT_PAIRED: u32 = 1;
const CLOSE_REMOVED: u32 = 2;

/// What the phone link needs from the app. The real one is `phone_api::AppBackend`.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    fn is_paired(&self, id: &str) -> bool;
    /// A pairing code is on the screen, so a stranger may knock.
    fn pairing_open(&self) -> bool;
    /// A stranger's knock. `Ok` is the answer for the phone, and the phone is paired from now on.
    async fn hello(&self, id: &str, hello: Hello) -> Result<Value, String>;
    /// A command with one answer.
    async fn call(&self, id: &str, cmd: &str, args: Value) -> Result<Value, String>;
    /// A command that answers with events. They go to `out`; it returns when the command is done. `audio` carries
    /// the binary frames the phone keeps sending on the same stream (voice); it ends when the phone closes it.
    async fn stream(
        &self,
        id: &str,
        cmd: &str,
        args: Value,
        out: mpsc::UnboundedSender<Value>,
        audio: mpsc::UnboundedReceiver<Vec<u8>>,
    ) -> Result<(), String>;
    fn link_up(&self);
    fn link_down(&self);
    /// Ids that were removed while connected.
    fn kicks(&self) -> broadcast::Receiver<String>;
}

/// Counts a link while it lives, so the count is right however a connection task ends (also when it
/// is aborted by the kill switch).
struct LinkGuard(Arc<dyn Backend>);

impl LinkGuard {
    fn new(backend: &Arc<dyn Backend>) -> Self {
        backend.link_up();
        LinkGuard(backend.clone())
    }
}

impl Drop for LinkGuard {
    fn drop(&mut self) {
        self.0.link_down();
    }
}

/// Stops a task when the request that started it is over.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Serve one connection until it closes.
pub async fn serve_connection(backend: Arc<dyn Backend>, conn: Connection) {
    let id = conn.remote_id().to_string();
    if !backend.is_paired(&id) && (!backend.pairing_open() || !knock(&*backend, &conn, &id).await) {
        conn.close(VarInt::from_u32(CLOSE_NOT_PAIRED), b"not paired");
        return;
    }
    let _link = LinkGuard::new(&backend);
    let mut kicks = backend.kicks();
    let mut streams = JoinSet::new();
    loop {
        tokio::select! {
            opened = conn.accept_bi() => {
                let Ok((send, recv)) = opened else { break };
                // A phone that was removed is not served, even if its link was missed by the kick.
                if !backend.is_paired(&id) {
                    conn.close(VarInt::from_u32(CLOSE_REMOVED), b"removed");
                    break;
                }
                streams.spawn(serve_stream(backend.clone(), id.clone(), send, recv));
            }
            kicked = kicks.recv() => match kicked {
                Ok(who) if who == id => {
                    conn.close(VarInt::from_u32(CLOSE_REMOVED), b"removed");
                    break;
                }
                Err(broadcast::error::RecvError::Closed) => break,
                _ => {}
            },
            // Finished streams are collected so the set does not grow.
            Some(_) = streams.join_next(), if !streams.is_empty() => {}
        }
    }
    // Streams that are still open die with the connection. A turn does not: it runs on its own task.
    streams.abort_all();
}

/// A stranger's one frame. `true` when it paired; the connection then carries on as a paired one.
async fn knock(backend: &dyn Backend, conn: &Connection, id: &str) -> bool {
    let Ok(Ok((mut send, mut recv))) = tokio::time::timeout(KNOCK_WAIT, conn.accept_bi()).await else {
        return false;
    };
    let frame = tokio::time::timeout(KNOCK_WAIT, read_frame(&mut recv, MAX_HELLO)).await;
    let hello = match frame {
        Ok(Ok(Some(Frame::Json(v)))) => serde_json::from_value::<Request>(v)
            .ok()
            .filter(|r| r.cmd == HELLO)
            .and_then(|r| serde_json::from_value::<Hello>(r.args).ok()),
        _ => None,
    };
    let Some(hello) = hello else { return false };
    match backend.hello(id, hello).await {
        Ok(answer) => {
            let _ = write_json(&mut send, &reply_ok(answer)).await;
            let _ = send.finish();
            true
        }
        Err(words) => {
            let _ = write_json(&mut send, &reply_err(words)).await;
            let _ = send.finish();
            // Give the phone time to read the answer before the connection goes.
            let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
            false
        }
    }
}

/// One request of a paired phone. Generic over the stream so a test can use a plain pipe.
pub async fn serve_stream<R, W>(backend: Arc<dyn Backend>, id: String, mut send: W, mut recv: R)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send,
{
    let request = match read_frame(&mut recv, MAX_JSON).await {
        Ok(Some(Frame::Json(v))) => serde_json::from_value::<Request>(v).ok(),
        _ => None,
    };
    let Some(request) = request else {
        let _ = write_json(&mut send, &reply_err("I did not understand that request.")).await;
        return;
    };
    if !is_allowed(&request.cmd) {
        let _ = write_json(&mut send, &reply_err("That is not available on the phone.")).await;
        return;
    }
    if !is_stream(&request.cmd) {
        let answer = match backend.call(&id, &request.cmd, request.args).await {
            Ok(v) => reply_ok(v),
            Err(words) => reply_err(words),
        };
        let _ = write_json(&mut send, &answer).await;
        return;
    }

    // What the phone sends after its request, audio for voice, is passed on until it closes the stream.
    let (audio_in, audio) = mpsc::unbounded_channel::<Vec<u8>>();
    let reader = tokio::spawn(async move {
        while let Ok(Some(frame)) = read_frame(&mut recv, MAX_JSON).await {
            if let Frame::Audio(bytes) = frame {
                if audio_in.send(bytes).is_err() {
                    break;
                }
            }
        }
    });
    let _reader = AbortOnDrop(reader);
    let (out, mut events) = mpsc::unbounded_channel::<Value>();
    let work = backend.stream(&id, &request.cmd, request.args, out, audio);
    tokio::pin!(work);
    let ended = loop {
        tokio::select! {
            Some(event) = events.recv() => {
                // A phone that went away stops reading; the command goes on without it.
                if write_json(&mut send, &reply_event(event)).await.is_err() {
                    return;
                }
            }
            result = &mut work => break result,
        }
    };
    while let Ok(event) = events.try_recv() {
        if write_json(&mut send, &reply_event(event)).await.is_err() {
            return;
        }
    }
    let last = match ended {
        Ok(()) => reply_done(),
        Err(words) => reply_err(words),
    };
    let _ = write_json(&mut send, &last).await;
}

/// The key that is this desktop's identity. Kept in the credential store; a new one is made once.
/// Returns the key and, when it is new, the text to store.
pub fn key_from(stored: Option<String>) -> (SecretKey, Option<String>) {
    if let Some(key) = stored.as_deref().and_then(decode_key) {
        return (key, None);
    }
    let key = SecretKey::generate();
    let text: String = key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
    (key, Some(text))
}

fn decode_key(text: &str) -> Option<SecretKey> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(SecretKey::from_bytes(&bytes))
}

/// Where the phone link may be reached, as far as the endpoint knows.
fn addr_of(endpoint: &Endpoint) -> HostAddr {
    HostAddr { id: endpoint.id().to_string(), relay: endpoint.addr().relay_urls().next().map(|u| u.to_string()) }
}

pub struct HostConfig {
    pub key: SecretKey,
    /// Our own relay instead of n0's free ones (`remote.relay`).
    pub relay: Option<RelayUrl>,
    /// Test only: no relay and no public lookup, so two endpoints in one process find each other.
    pub offline: bool,
    /// Test only: where the running endpoint listens, so a test can dial it.
    pub bound: Option<Arc<std::sync::Mutex<Vec<std::net::SocketAddr>>>>,
}

async fn bind(config: &HostConfig) -> Result<Endpoint, String> {
    let builder = if config.offline {
        Endpoint::builder(presets::N0DisableRelay)
    } else {
        Endpoint::builder(presets::N0)
    };
    let mut builder = builder.secret_key(config.key.clone()).alpns(vec![remote_proto::ALPN.to_vec()]);
    if let Some(url) = &config.relay {
        builder = builder.relay_mode(RelayMode::custom([url.clone()]));
    }
    builder.bind().await.map_err(|e| e.to_string())
}

/// Keep the endpoint up exactly while `enabled` is true. Off closes it and every connection within a
/// second. `publish` is told where the endpoint can be reached, and `None` when it is down.
pub async fn run(
    backend: Arc<dyn Backend>,
    config: HostConfig,
    publish: impl Fn(Option<HostAddr>) + Send + Sync + 'static,
    mut enabled: watch::Receiver<bool>,
) {
    let publish = Arc::new(publish);
    loop {
        if enabled.wait_for(|on| *on).await.is_err() {
            return;
        }
        let endpoint = match bind(&config).await {
            Ok(endpoint) => endpoint,
            Err(e) => {
                eprintln!("remote: the phone link could not start: {e}");
                // Try again in a while, unless it is switched off meanwhile.
                tokio::select! {
                    _ = enabled.wait_for(|on| !*on) => {}
                    _ = tokio::time::sleep(Duration::from_secs(15)) => {}
                }
                continue;
            }
        };
        if let Some(bound) = &config.bound {
            *bound.lock().unwrap() = endpoint.bound_sockets();
        }
        publish(Some(addr_of(&endpoint)));

        // The relay is known a moment after the bind.
        let online = {
            let (endpoint, publish) = (endpoint.clone(), publish.clone());
            tokio::spawn(async move {
                endpoint.online().await;
                publish(Some(addr_of(&endpoint)));
            })
        };
        let accepting = {
            let (endpoint, backend) = (endpoint.clone(), backend.clone());
            tokio::spawn(async move {
                let mut connections = JoinSet::new();
                loop {
                    tokio::select! {
                        incoming = endpoint.accept() => {
                            let Some(incoming) = incoming else { break };
                            let backend = backend.clone();
                            connections.spawn(async move {
                                if let Ok(conn) = incoming.await {
                                    serve_connection(backend, conn).await;
                                }
                            });
                        }
                        Some(_) = connections.join_next(), if !connections.is_empty() => {}
                    }
                }
            })
        };

        let _ = enabled.wait_for(|on| !*on).await;
        online.abort();
        accepting.abort();
        publish(None);
        let _ = tokio::time::timeout(CLOSE_WAIT, endpoint.close()).await;
    }
}

/// The setting that holds our own relay URL, when the person set one up (REM-17). Empty: n0's free relays.
pub const RELAY_KEY: &str = "remote.relay";

/// The key and relay for the phone link. The key is made (and kept in the credential store) the first time remote is
/// switched on, not before: an app that never uses remote leaves nothing in the credential store.
fn load_config(app: &tauri::AppHandle) -> Option<HostConfig> {
    use tauri::Manager;
    let stored = crate::secrets::get_secret(crate::secrets::SERVICE_REMOTE, "iroh-secret").ok().flatten();
    let (key, fresh) = key_from(stored);
    if let Some(text) = fresh {
        // Without a stored key the identity would change at every start and every phone would be lost.
        if let Err(e) = crate::secrets::set_secret(crate::secrets::SERVICE_REMOTE, "iroh-secret", &text) {
            eprintln!("remote: the phone link key could not be kept, so the phone link stays off: {e}");
            return None;
        }
    }
    let relay = app
        .state::<crate::db::Db>()
        .get_setting(RELAY_KEY)
        .ok()
        .flatten()
        .and_then(|url| url.trim().parse::<RelayUrl>().ok());
    Some(HostConfig { key, relay, offline: false, bound: None })
}

/// Start the phone link for this app. It stays asleep until the switch is first turned on.
pub fn start(app: tauri::AppHandle) {
    use tauri::Manager;
    let enabled = app.state::<super::Gateway>().watch_enabled();
    tauri::async_runtime::spawn(async move {
        let mut wake = enabled.clone();
        if wake.wait_for(|on| *on).await.is_err() {
            return;
        }
        let Some(config) = load_config(&app) else { return };
        let backend: Arc<dyn Backend> = Arc::new(super::phone_api::AppBackend(app.clone()));
        let publisher = app.clone();
        run(backend, config, move |addr| publisher.state::<super::Gateway>().set_host_addr(addr), enabled).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loopback endpoints started all at once can miss their handshake window on a busy machine, so
    /// the tests that open them take turns.
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// A backend with one paired key and a code that is "on screen" on request.
    struct Fake {
        paired: Mutex<Vec<String>>,
        open: Mutex<bool>,
        links: AtomicUsize,
        kicks: broadcast::Sender<String>,
        calls: Mutex<Vec<String>>,
    }

    impl Fake {
        fn new() -> Arc<Fake> {
            Arc::new(Fake {
                paired: Mutex::default(),
                open: Mutex::new(false),
                links: AtomicUsize::new(0),
                kicks: broadcast::channel(8).0,
                calls: Mutex::default(),
            })
        }
    }

    #[async_trait]
    impl Backend for Fake {
        fn is_paired(&self, id: &str) -> bool {
            self.paired.lock().unwrap().iter().any(|p| p == id)
        }
        fn pairing_open(&self) -> bool {
            *self.open.lock().unwrap()
        }
        async fn hello(&self, id: &str, hello: Hello) -> Result<Value, String> {
            if hello.code == "GOODCODE" {
                self.paired.lock().unwrap().push(id.to_string());
                *self.open.lock().unwrap() = false;
                Ok(serde_json::json!({ "name": "Test desktop" }))
            } else {
                Err("That code is not right.".into())
            }
        }
        async fn call(&self, _id: &str, cmd: &str, _args: Value) -> Result<Value, String> {
            self.calls.lock().unwrap().push(cmd.to_string());
            Ok(serde_json::json!({ "cmd": cmd }))
        }
        async fn stream(
            &self,
            _id: &str,
            cmd: &str,
            args: Value,
            out: mpsc::UnboundedSender<Value>,
            mut audio: mpsc::UnboundedReceiver<Vec<u8>>,
        ) -> Result<(), String> {
            if cmd == "voice_start" {
                // Like the real one: every audio frame is reported, and the stream ends when the phone closes it.
                while let Some(bytes) = audio.recv().await {
                    let _ = out.send(serde_json::json!({ "type": "heard", "bytes": bytes.len() }));
                }
                return Ok(());
            }
            for n in 0..3 {
                let _ = out.send(serde_json::json!({ "type": "token", "n": n, "echo": args }));
            }
            Ok(())
        }
        fn link_up(&self) {
            self.links.fetch_add(1, Ordering::SeqCst);
        }
        fn link_down(&self) {
            self.links.fetch_sub(1, Ordering::SeqCst);
        }
        fn kicks(&self) -> broadcast::Receiver<String> {
            self.kicks.subscribe()
        }
    }

    fn config() -> HostConfig {
        HostConfig { key: SecretKey::generate(), relay: None, offline: true, bound: None }
    }

    /// A desktop host and a phone endpoint in this process, on the loopback address, no relay.
    async fn pair_of_endpoints(backend: Arc<Fake>) -> (Endpoint, Endpoint, tokio::task::JoinHandle<()>) {
        let host = bind(&config()).await.unwrap();
        let phone = Endpoint::builder(presets::N0DisableRelay).bind().await.unwrap();
        let accepting = {
            let (host, backend) = (host.clone(), backend.clone());
            tokio::spawn(async move {
                while let Some(incoming) = host.accept().await {
                    let backend: Arc<dyn Backend> = backend.clone();
                    tokio::spawn(async move {
                        if let Ok(conn) = incoming.await {
                            serve_connection(backend, conn).await;
                        }
                    });
                }
            })
        };
        (host, phone, accepting)
    }

    fn addr_for(host: &Endpoint) -> iroh::EndpointAddr {
        let port = host.bound_sockets().into_iter().find(|s| s.is_ipv4()).unwrap().port();
        iroh::EndpointAddr::from_parts(host.id(), [iroh::TransportAddr::Ip(std::net::SocketAddr::from(([127, 0, 0, 1], port)))])
    }

    async fn ask(conn: &Connection, request: Value) -> Result<Vec<Value>, String> {
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        write_json(&mut send, &request).await.map_err(|e| e.to_string())?;
        let mut frames = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), read_frame(&mut recv, MAX_JSON)).await {
                Ok(Ok(Some(Frame::Json(v)))) => {
                    let last = v.get("done").is_some() || v.get("ok").is_some() || v.get("err").is_some();
                    frames.push(v);
                    if last {
                        return Ok(frames);
                    }
                }
                Ok(Ok(_)) => return Ok(frames),
                Ok(Err(e)) => return Err(e.to_string()),
                Err(_) => return Err("no answer".into()),
            }
        }
    }

    #[tokio::test]
    async fn a_stranger_is_closed_before_any_frame_when_no_code_is_on_screen() {
        let _turn = SERIAL.lock().await;
        let backend = Fake::new();
        let (host, phone, _accepting) = pair_of_endpoints(backend.clone()).await;
        let conn = phone.connect(addr_for(&host), remote_proto::ALPN).await.unwrap();
        // The host closes the connection; a request never gets an answer.
        let result = ask(&conn, serde_json::json!({ "cmd": "status" })).await;
        assert!(result.is_err() || result.unwrap().is_empty(), "a stranger got an answer");
        assert!(backend.calls.lock().unwrap().is_empty(), "nothing ran for a stranger");
    }

    #[tokio::test]
    async fn a_stranger_with_a_code_on_screen_may_only_say_hello() {
        let _turn = SERIAL.lock().await;
        let backend = Fake::new();
        *backend.open.lock().unwrap() = true;
        let (host, phone, _accepting) = pair_of_endpoints(backend.clone()).await;
        let conn = phone.connect(addr_for(&host), remote_proto::ALPN).await.unwrap();
        // Anything but hello: closed, nothing runs.
        let result = ask(&conn, serde_json::json!({ "cmd": "conversations" })).await;
        assert!(result.is_err() || result.unwrap().is_empty());
        assert!(backend.calls.lock().unwrap().is_empty());
        assert!(backend.paired.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_wrong_code_pairs_nobody_and_the_right_one_pairs_on_the_same_connection() {
        let _turn = SERIAL.lock().await;
        let backend = Fake::new();
        *backend.open.lock().unwrap() = true;
        let (host, phone, _accepting) = pair_of_endpoints(backend.clone()).await;

        let bad = phone.connect(addr_for(&host), remote_proto::ALPN).await.unwrap();
        let said = ask(&bad, serde_json::json!({ "cmd": "hello", "args": { "code": "WRONG", "label": "Pixel" } }))
            .await
            .unwrap();
        assert_eq!(said[0]["err"], "That code is not right.");
        assert!(backend.paired.lock().unwrap().is_empty());

        let conn = phone.connect(addr_for(&host), remote_proto::ALPN).await.unwrap();
        let said = ask(&conn, serde_json::json!({ "cmd": "hello", "args": { "code": "GOODCODE", "label": "Pixel" } }))
            .await
            .unwrap();
        assert_eq!(said[0]["ok"]["name"], "Test desktop");
        // The same connection now works as a paired one.
        let said = ask(&conn, serde_json::json!({ "cmd": "status" })).await.unwrap();
        assert_eq!(said[0]["ok"]["cmd"], "status");
        assert_eq!(backend.links.load(Ordering::SeqCst), 1, "one live link");
    }

    #[tokio::test]
    async fn a_paired_phone_gets_one_reply_a_stream_of_events_and_a_refusal_for_the_rest() {
        let _turn = SERIAL.lock().await;
        let backend = Fake::new();
        let (host, phone, _accepting) = pair_of_endpoints(backend.clone()).await;
        backend.paired.lock().unwrap().push(phone.id().to_string());
        let conn = phone.connect(addr_for(&host), remote_proto::ALPN).await.unwrap();

        let said = ask(&conn, serde_json::json!({ "cmd": "conversations" })).await.unwrap();
        assert_eq!(said[0]["ok"]["cmd"], "conversations");

        let said = ask(&conn, serde_json::json!({ "cmd": "send", "args": { "text": "hi" } })).await.unwrap();
        assert_eq!(said.len(), 4, "three events then done: {said:?}");
        assert_eq!(said[0]["ev"]["n"], 0);
        assert_eq!(said[3]["done"], true);

        for cmd in ["remote_set_enabled_cmd", "settings", "hello", "run_code"] {
            let said = ask(&conn, serde_json::json!({ "cmd": cmd })).await.unwrap();
            assert_eq!(said[0]["err"], "That is not available on the phone.", "{cmd}");
        }
        assert_eq!(*backend.calls.lock().unwrap(), vec!["conversations".to_string()]);
    }

    #[tokio::test]
    async fn audio_the_phone_keeps_sending_reaches_the_command_until_it_closes_the_stream() {
        let _turn = SERIAL.lock().await;
        let backend = Fake::new();
        let (host, phone, _accepting) = pair_of_endpoints(backend.clone()).await;
        backend.paired.lock().unwrap().push(phone.id().to_string());
        let conn = phone.connect(addr_for(&host), remote_proto::ALPN).await.unwrap();
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        write_json(&mut send, &serde_json::json!({ "cmd": "voice_start", "args": {} })).await.unwrap();
        remote_proto::write_audio(&mut send, &[0u8; 100]).await.unwrap();
        remote_proto::write_audio(&mut send, &[0u8; 40]).await.unwrap();
        let mut heard = Vec::new();
        for _ in 0..2 {
            let Ok(Ok(Some(Frame::Json(v)))) = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut recv, MAX_JSON)).await else {
                panic!("no event")
            };
            heard.push(v["ev"]["bytes"].as_u64().unwrap());
        }
        assert_eq!(heard, vec![100, 40]);
        // The phone closes its side: the command ends and the stream says done.
        send.finish().unwrap();
        let Ok(Ok(Some(Frame::Json(v)))) = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut recv, MAX_JSON)).await else {
            panic!("no end")
        };
        assert_eq!(v["done"], true);
    }

    #[tokio::test]
    async fn a_phone_that_is_removed_loses_its_link_at_once() {
        let _turn = SERIAL.lock().await;
        let backend = Fake::new();
        let (host, phone, _accepting) = pair_of_endpoints(backend.clone()).await;
        let id = phone.id().to_string();
        backend.paired.lock().unwrap().push(id.clone());
        let conn = phone.connect(addr_for(&host), remote_proto::ALPN).await.unwrap();
        ask(&conn, serde_json::json!({ "cmd": "status" })).await.unwrap();
        assert_eq!(backend.links.load(Ordering::SeqCst), 1);

        backend.paired.lock().unwrap().clear();
        backend.kicks.send(id).unwrap();
        let closed = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
        assert!(closed.is_ok(), "the connection was not closed");
        // The count settles back to zero.
        for _ in 0..20 {
            if backend.links.load(Ordering::SeqCst) == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(backend.links.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn the_kill_switch_closes_the_connection_within_a_second() {
        let _turn = SERIAL.lock().await;
        let backend = Fake::new();
        let bound = Arc::new(Mutex::new(Vec::new()));
        let host_config = HostConfig { bound: Some(bound.clone()), ..config() };
        let host_id = host_config.key.public();
        let (on, enabled) = watch::channel(true);
        let published = Arc::new(Mutex::new(None::<HostAddr>));
        let running = {
            let published = published.clone();
            let backend: Arc<dyn Backend> = backend.clone();
            tokio::spawn(run(backend, host_config, move |a| *published.lock().unwrap() = a, enabled))
        };
        for _ in 0..40 {
            if published.lock().unwrap().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let announced = published.lock().unwrap().clone().expect("the host published its address");
        assert_eq!(announced.id, host_id.to_string());

        let phone = Endpoint::builder(presets::N0DisableRelay).bind().await.unwrap();
        backend.paired.lock().unwrap().push(phone.id().to_string());
        let port = bound.lock().unwrap().iter().find(|s| s.is_ipv4()).unwrap().port();
        let addr = iroh::EndpointAddr::from_parts(
            host_id,
            [iroh::TransportAddr::Ip(std::net::SocketAddr::from(([127, 0, 0, 1], port)))],
        );
        let conn = phone.connect(addr.clone(), remote_proto::ALPN).await.unwrap();
        let said = ask(&conn, serde_json::json!({ "cmd": "status" })).await.unwrap();
        assert_eq!(said[0]["ok"]["cmd"], "status");

        let switched = std::time::Instant::now();
        on.send(false).unwrap();
        let closed = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
        assert!(closed.is_ok(), "the connection stayed open after the switch was turned off");
        assert!(switched.elapsed() < Duration::from_millis(1500), "took {:?}", switched.elapsed());
        assert!(published.lock().unwrap().is_none(), "the address is withdrawn");
        // And nobody can connect any more.
        assert!(tokio::time::timeout(Duration::from_secs(3), phone.connect(addr, remote_proto::ALPN)).await.map_or(true, |r| r.is_err()));
        running.abort();
    }

    #[test]
    fn the_desktop_key_is_made_once_and_read_back() {
        let (first, to_store) = key_from(None);
        let text = to_store.expect("a new key is stored");
        assert_eq!(text.len(), 64);
        let (again, to_store) = key_from(Some(text));
        assert!(to_store.is_none());
        assert_eq!(first.public(), again.public(), "the same identity after a restart");
        // A damaged entry is replaced, not trusted.
        let (_, to_store) = key_from(Some("not a key".into()));
        assert!(to_store.is_some());
    }
}
