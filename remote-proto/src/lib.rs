//! The phone link wire format (REM-11). Shared by the desktop host and the phone app.
//!
//! One bidirectional stream per request. The phone sends a request frame, the desktop answers with
//! one reply frame (`call`) or with event frames until `done` (`stream`). A frame is a 4 byte
//! big endian length, one kind byte, then the body. Kind `0` is JSON, kind `1` is audio bytes
//! (voice, REM-19).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The protocol name both sides speak. A new incompatible format gets a new number.
pub const ALPN: &[u8] = b"poiesis/remote/1";

/// Largest JSON frame. A conversation with long answers fits well inside.
pub const MAX_JSON: usize = 8 * 1024 * 1024;
/// Largest audio frame: two seconds of 16 kHz 16 bit mono plus the counter, with room to spare.
pub const MAX_AUDIO: usize = 128 * 1024;
/// A phone that is not paired yet may only say hello, and hello is small.
pub const MAX_HELLO: usize = 1024;

const KIND_JSON: u8 = 0;
const KIND_AUDIO: u8 = 1;

/// What one frame holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Json(Value),
    Audio(Vec<u8>),
}

#[derive(Debug)]
pub enum FrameError {
    Io(std::io::Error),
    TooBig(usize),
    Bad(&'static str),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Io(e) => write!(f, "{e}"),
            FrameError::TooBig(n) => write!(f, "a frame of {n} bytes is too big"),
            FrameError::Bad(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<std::io::Error> for FrameError {
    fn from(e: std::io::Error) -> Self {
        FrameError::Io(e)
    }
}

async fn write_raw<W: AsyncWrite + Unpin>(w: &mut W, kind: u8, body: &[u8]) -> Result<(), FrameError> {
    let len = u32::try_from(body.len() + 1).map_err(|_| FrameError::TooBig(body.len()))?;
    let mut out = Vec::with_capacity(body.len() + 5);
    out.extend_from_slice(&len.to_be_bytes());
    out.push(kind);
    out.extend_from_slice(body);
    w.write_all(&out).await?;
    Ok(())
}

pub async fn write_json<W: AsyncWrite + Unpin>(w: &mut W, value: &Value) -> Result<(), FrameError> {
    let body = serde_json::to_vec(value).map_err(|_| FrameError::Bad("not JSON"))?;
    if body.len() > MAX_JSON {
        return Err(FrameError::TooBig(body.len()));
    }
    write_raw(w, KIND_JSON, &body).await
}

pub async fn write_audio<W: AsyncWrite + Unpin>(w: &mut W, bytes: &[u8]) -> Result<(), FrameError> {
    if bytes.len() > MAX_AUDIO {
        return Err(FrameError::TooBig(bytes.len()));
    }
    write_raw(w, KIND_AUDIO, bytes).await
}

/// Read the next frame. `Ok(None)` is the other side finishing cleanly between frames. `max` caps
/// a JSON frame, so a stranger cannot make the reader allocate megabytes.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R, max: usize) -> Result<Option<Frame>, FrameError> {
    let mut head = [0u8; 4];
    if r.read(&mut head[..1]).await? == 0 {
        return Ok(None);
    }
    r.read_exact(&mut head[1..]).await?;
    let len = u32::from_be_bytes(head) as usize;
    if len == 0 {
        return Err(FrameError::Bad("an empty frame"));
    }
    let mut kind = [0u8; 1];
    r.read_exact(&mut kind).await?;
    let body_len = len - 1;
    // `max` holds for every kind, so a stranger's one hello frame stays small whatever it claims to be.
    let cap = if kind[0] == KIND_AUDIO { MAX_AUDIO.min(max) } else { max };
    if body_len > cap {
        return Err(FrameError::TooBig(body_len));
    }
    let mut body = vec![0u8; body_len];
    r.read_exact(&mut body).await?;
    match kind[0] {
        KIND_JSON => serde_json::from_slice(&body).map(|v| Some(Frame::Json(v))).map_err(|_| FrameError::Bad("not JSON")),
        KIND_AUDIO => Ok(Some(Frame::Audio(body))),
        _ => Err(FrameError::Bad("an unknown frame kind")),
    }
}

// ---- requests and replies ----

/// The first frame of every stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    pub cmd: String,
    #[serde(default)]
    pub args: Value,
}

pub fn reply_ok(value: Value) -> Value {
    json!({ "ok": value })
}

pub fn reply_err(words: impl Into<String>) -> Value {
    json!({ "err": words.into() })
}

pub fn reply_event(event: Value) -> Value {
    json!({ "ev": event })
}

pub fn reply_done() -> Value {
    json!({ "done": true })
}

/// What the phone found in a reply frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Ok(Value),
    Err(String),
    Event(Value),
    Done,
}

pub fn parse_reply(frame: &Value) -> Option<Reply> {
    if let Some(v) = frame.get("ok") {
        return Some(Reply::Ok(v.clone()));
    }
    if let Some(e) = frame.get("err") {
        return Some(Reply::Err(e.as_str().unwrap_or("Something went wrong.").to_string()));
    }
    if let Some(ev) = frame.get("ev") {
        return Some(Reply::Event(ev.clone()));
    }
    if frame.get("done").is_some() {
        return Some(Reply::Done);
    }
    None
}

// ---- pairing ----

/// What the QR code on the desktop holds. The phone needs nothing else to find the desktop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Payload {
    pub v: u32,
    /// The desktop's public key.
    pub id: String,
    /// The relay the desktop is reachable through, when it has one.
    #[serde(default)]
    pub relay: Option<String>,
    /// The one-time code. It is typed by hand too, so it is short and readable.
    pub code: String,
}

impl Payload {
    pub const VERSION: u32 = 1;

    pub fn to_text(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn parse(text: &str) -> Option<Payload> {
        let p: Payload = serde_json::from_str(text.trim()).ok()?;
        (p.v == Self::VERSION && !p.id.is_empty()).then_some(p)
    }
}

/// The only command a phone that is not paired yet may send. Its `args` are a `Hello`.
pub const HELLO: &str = "hello";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    pub code: String,
    pub label: String,
}

// ---- the commands a paired phone may call ----

/// Answered with one reply frame.
pub const CALLS: &[&str] = &[
    "status",
    "conversations",
    "messages",
    "artifacts",
    "new_chat",
    "open_chat",
    "stop",
    "answer",
    "waiting",
    "resolve_waiting",
    "read_media",
    // Voice (REM-19): the desktop hears, thinks and speaks; these are the controls the phone has.
    "voice_status",
    "voice_stop",
    "voice_assistant",
    "voice_speak",
    "voice_cancel_speech",
];

/// Answered with event frames until `done`.
pub const STREAMS: &[&str] = &["send", "subscribe", "voice_start"];

pub fn is_allowed(cmd: &str) -> bool {
    CALLS.contains(&cmd) || STREAMS.contains(&cmd)
}

pub fn is_stream(cmd: &str) -> bool {
    STREAMS.contains(&cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn json_and_audio_frames_round_trip() {
        let (mut a, mut b) = tokio::io::duplex(1 << 16);
        write_json(&mut a, &json!({"cmd": "status"})).await.unwrap();
        write_audio(&mut a, &[1, 2, 3]).await.unwrap();
        drop(a);
        assert_eq!(read_frame(&mut b, MAX_JSON).await.unwrap(), Some(Frame::Json(json!({"cmd": "status"}))));
        assert_eq!(read_frame(&mut b, MAX_JSON).await.unwrap(), Some(Frame::Audio(vec![1, 2, 3])));
        assert_eq!(read_frame(&mut b, MAX_JSON).await.unwrap(), None, "a clean end between frames");
    }

    #[tokio::test]
    async fn a_stranger_cannot_make_the_reader_allocate() {
        let (mut a, mut b) = tokio::io::duplex(1 << 16);
        // A JSON frame that claims 2 MB, read with the hello cap.
        a.write_all(&2_000_000u32.to_be_bytes()).await.unwrap();
        a.write_all(&[KIND_JSON]).await.unwrap();
        let err = read_frame(&mut b, MAX_HELLO).await.unwrap_err();
        assert!(matches!(err, FrameError::TooBig(_)));
        // Calling it audio does not get past the hello cap either.
        let (mut a, mut b) = tokio::io::duplex(1 << 16);
        a.write_all(&(MAX_HELLO as u32 + 100).to_be_bytes()).await.unwrap();
        a.write_all(&[KIND_AUDIO]).await.unwrap();
        assert!(matches!(read_frame(&mut b, MAX_HELLO).await.unwrap_err(), FrameError::TooBig(_)));
    }

    #[tokio::test]
    async fn broken_frames_are_errors_not_panics() {
        for bytes in [vec![0, 0, 0, 0], vec![0, 0, 0, 2, 9, 0], vec![0, 0, 0, 3, 0, b'x', b'y'], vec![0, 0]] {
            let (mut a, mut b) = tokio::io::duplex(64);
            a.write_all(&bytes).await.unwrap();
            drop(a);
            assert!(read_frame(&mut b, MAX_JSON).await.is_err(), "{bytes:?}");
        }
    }

    #[test]
    fn replies_are_read_back() {
        assert_eq!(parse_reply(&reply_ok(json!(1))), Some(Reply::Ok(json!(1))));
        assert_eq!(parse_reply(&reply_err("no")), Some(Reply::Err("no".into())));
        assert_eq!(parse_reply(&reply_event(json!({"type": "token"}))), Some(Reply::Event(json!({"type": "token"}))));
        assert_eq!(parse_reply(&reply_done()), Some(Reply::Done));
        assert_eq!(parse_reply(&json!({"x": 1})), None);
    }

    #[test]
    fn the_payload_survives_the_qr_and_refuses_junk() {
        let p = Payload { v: 1, id: "abc".into(), relay: Some("https://r.example/".into()), code: "K7M2QX9P".into() };
        assert_eq!(Payload::parse(&p.to_text()), Some(p));
        assert_eq!(Payload::parse("not json"), None);
        assert_eq!(Payload::parse(r#"{"v":2,"id":"a","code":"x"}"#), None, "a newer format is not guessed at");
        assert_eq!(Payload::parse(r#"{"v":1,"id":"","code":"x"}"#), None);
    }

    #[test]
    fn nothing_dangerous_is_on_the_list() {
        for cmd in ["remote_set_enabled_cmd", "run_code", "settings", "pair", "hello"] {
            assert!(!is_allowed(cmd), "{cmd}");
        }
        assert!(is_allowed("send") && is_stream("send") && !is_stream("status"));
    }
}
