//! Starting, talking to and stopping an agent app's process (`CLI-3`, `CLI-9`).
//!
//! Rules that hold for every agent:
//! - No user or model text ever goes in argv. The prompt travels over stdin, so
//!   the quoting of an npm `.cmd` shim cannot be abused.
//! - The child gets a cleaned environment, never Poiesis's own secrets and never
//!   a provider key the user exported for something else.
//! - Stop kills the whole tree. An npm shim starts `node`, which starts the real
//!   program, and killing only the first leaves the rest running.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::runtime::proxy::CancelFlag;

use super::catalog::CatalogEntry;
use super::locate::is_shim;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// One line longer than this is dropped and counted, never buffered without bound.
pub const MAX_LINE: usize = 1024 * 1024;
/// How much of an agent's stderr is kept for an error message.
const TAIL_BYTES: usize = 8 * 1024;

/// Variables that carry a model provider's key. Taken out of every agent's
/// environment: with one in place, an agent that should use the user's plan
/// would quietly bill the key instead, and an agent would see another agent's
/// key. An API key the user chose for an agent is added back by name.
const PROVIDER_KEYS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
    "OPENROUTER_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_GENERATIVE_AI_API_KEY",
    "GROQ_API_KEY",
    "MISTRAL_API_KEY",
    "XAI_API_KEY",
    "DEEPSEEK_API_KEY",
    "TOGETHER_API_KEY",
    "FIREWORKS_API_KEY",
    "PERPLEXITY_API_KEY",
    "COMPOSIO_API_KEY",
];

/// Poiesis's own variables and the model providers' keys: what no child process
/// inherits unless it was handed on purpose (`CLI-9`, `LRN-3`). Other
/// variables, a `GITHUB_TOKEN` the user exported for a tool, stay.
pub fn withheld_from_children(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    up.starts_with("POIESIS_") || PROVIDER_KEYS.contains(&up.as_str())
}

#[cfg(test)]
mod withheld_tests {
    use super::withheld_from_children;

    #[test]
    fn poiesis_variables_and_provider_keys_are_withheld_other_tokens_stay() {
        assert!(withheld_from_children("POIESIS_FOLDER"));
        assert!(withheld_from_children("openai_api_key"));
        assert!(withheld_from_children("ANTHROPIC_API_KEY"));
        assert!(!withheld_from_children("GITHUB_TOKEN"));
        assert!(!withheld_from_children("PATH"));
    }
}

/// The user's environment minus what an agent must not inherit (`CLI-9`), plus
/// what was configured for it. `own_key` is `(variable, value)` for an engine
/// set to pay with an API key.
pub fn child_env(
    entry: Option<&CatalogEntry>,
    configured: &BTreeMap<String, String>,
    own_key: Option<(&str, &str)>,
) -> Vec<(String, String)> {
    let strip_prefixes: Vec<String> = entry
        .map(|e| e.strip_env_prefixes.iter().map(|p| p.to_ascii_uppercase()).collect())
        .unwrap_or_default();
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| {
            let up = k.to_ascii_uppercase();
            !(withheld_from_children(k)
                || strip_prefixes.iter().any(|p| up.starts_with(p.as_str()) && (up.ends_with("_KEY") || up.ends_with("_TOKEN") || up.ends_with("_SECRET"))))
        })
        .collect();
    let mut set = |k: &str, v: &str| {
        env.retain(|(ek, _)| !ek.eq_ignore_ascii_case(k));
        env.push((k.to_string(), v.to_string()));
    };
    if let Some(path) = super::locate::child_path() {
        set("PATH", &path.to_string_lossy());
    }
    if let Some(e) = entry {
        for (k, v) in &e.ask_everything_env {
            set(k, v);
        }
    }
    for (k, v) in configured {
        set(k, v);
    }
    if let Some((k, v)) = own_key {
        set(k, v);
    }
    env
}

/// A command that runs `program` with `args`, ready to spawn. An npm shim
/// (`.cmd`) goes through `cmd.exe`; its path and arguments are ours (the catalog
/// and the validated settings), never text from the user or the model.
pub fn command(program: &Path, args: &[String], env: &[(String, String)]) -> Result<Command, String> {
    let mut std_cmd;
    if is_shim(program) {
        let path = program.to_string_lossy();
        // `cmd` would expand or run these. A real install path has none of them.
        if path.chars().any(|c| matches!(c, '"' | '%' | '^' | '&' | '|' | '<' | '>' | '!')) {
            return Err("The path has a character I can't pass on safely. Move the app to a simpler path.".into());
        }
        if let Some(bad) = args.iter().find(|a| a.chars().any(|c| matches!(c, '"' | '%' | '^' | '&' | '|' | '<' | '>' | '!' | '\n' | '\r'))) {
            return Err(format!("The argument {bad:?} has a character I can't pass on safely."));
        }
        std_cmd = std::process::Command::new("cmd.exe");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let quote = |a: &str| if a.is_empty() || a.contains([' ', '\t']) { format!("\"{a}\"") } else { a.to_string() };
            let mut line = format!("/d /s /c \"{}", quote(&path));
            for a in args {
                line.push(' ');
                line.push_str(&quote(a));
            }
            line.push('"');
            std_cmd.raw_arg(line);
        }
    } else {
        std_cmd = std::process::Command::new(program);
        std_cmd.args(args);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std_cmd.creation_flags(CREATE_NO_WINDOW);
    }
    std_cmd.env_clear();
    std_cmd.envs(env.iter().map(|(k, v)| (k, v)));
    let mut cmd = Command::from(std_cmd);
    cmd.kill_on_drop(true);
    Ok(cmd)
}

/// Start the child with all three pipes, in `cwd`, inside the kill-on-close job.
pub fn spawn(mut cmd: Command, cwd: Option<&Path>) -> Result<Child, String> {
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let child = cmd.spawn().map_err(|e| format!("It would not start: {e}"))?;
    #[cfg(windows)]
    if let Some(h) = child.raw_handle() {
        crate::runtime::jobobject::assign_raw(h);
    }
    Ok(child)
}

/// Stop the process and everything it started. Only our own child is named, so
/// the user's other copies of the same app are never touched.
pub async fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        use std::os::windows::process::CommandExt;
        let mut kill = std::process::Command::new("taskkill");
        kill.args(["/T", "/F", "/PID", &pid.to_string()]).creation_flags(CREATE_NO_WINDOW);
        let _ = Command::from(kill).stdout(Stdio::null()).stderr(Stdio::null()).status().await;
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
}

/// The last few KiB of an agent's stderr.
#[derive(Clone, Default)]
pub struct Tail(Arc<Mutex<String>>);

impl Tail {
    /// Read `stream` to its end in the background.
    pub fn collect(stream: impl AsyncRead + Unpin + Send + 'static) -> Self {
        let tail = Tail::default();
        let sink = tail.clone();
        tokio::spawn(async move {
            let mut stream = stream;
            let mut chunk = [0u8; 4096];
            while let Ok(n) = stream.read(&mut chunk).await {
                if n == 0 {
                    break;
                }
                sink.push(&String::from_utf8_lossy(&chunk[..n]));
            }
        });
        tail
    }

    fn push(&self, s: &str) {
        let mut buf = self.0.lock().unwrap();
        buf.push_str(s);
        if buf.len() > TAIL_BYTES {
            let mut cut = buf.len() - TAIL_BYTES;
            while !buf.is_char_boundary(cut) {
                cut += 1;
            }
            buf.drain(..cut);
        }
    }

    pub fn text(&self) -> String {
        self.0.lock().unwrap().trim().to_string()
    }
}

/// Reads lines, dropping (and counting) any line longer than `MAX_LINE`.
pub struct LineReader<R> {
    reader: R,
    buf: Vec<u8>,
    skipping: bool,
    pub dropped: u32,
}

impl<R: AsyncRead + Unpin> LineReader<R> {
    pub fn new(reader: R) -> Self {
        Self { reader, buf: Vec::new(), skipping: false, dropped: 0 }
    }

    pub async fn next_line(&mut self) -> std::io::Result<Option<String>> {
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
                if std::mem::replace(&mut self.skipping, false) {
                    continue;
                }
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }
            if self.buf.len() > MAX_LINE {
                if !self.skipping {
                    self.dropped += 1;
                }
                self.skipping = true;
                self.buf.clear();
            }
            let mut chunk = [0u8; 16 * 1024];
            let n = self.reader.read(&mut chunk).await?;
            if n == 0 {
                let rest = std::mem::take(&mut self.buf);
                if self.skipping || rest.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(String::from_utf8_lossy(&rest).into_owned()));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }
}

/// What came of waiting for the next message.
pub enum Wait {
    Message(Value),
    /// The agent closed its output.
    Closed,
    Cancelled,
    TimedOut,
}

/// A started agent: lines in, lines out, and a tail of what it said on stderr.
pub struct Wire {
    pub child: Child,
    stdin: Option<ChildStdin>,
    lines: LineReader<ChildStdout>,
    pub tail: Tail,
    /// Lines that were not JSON, counted rather than fatal.
    pub unknown: u32,
}

impl Wire {
    pub fn start(cmd: Command, cwd: Option<&Path>) -> Result<Wire, String> {
        let mut child = spawn(cmd, cwd)?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or("It gave me no output to read.")?;
        let tail = Tail::collect(child.stderr.take().ok_or("It gave me no error output to read.")?);
        Ok(Wire { child, stdin, lines: LineReader::new(stdout), tail, unknown: 0 })
    }

    pub async fn send(&mut self, v: &Value) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("The app has already been told it is finished.")?;
        let mut line = v.to_string();
        line.push('\n');
        stdin.write_all(line.as_bytes()).await.map_err(|e| format!("It stopped listening: {e}"))?;
        stdin.flush().await.map_err(|e| format!("It stopped listening: {e}"))
    }

    /// Let the app know no more input is coming. A well-behaved one exits.
    pub fn close_input(&mut self) {
        self.stdin = None;
    }

    pub fn dropped_lines(&self) -> u32 {
        self.lines.dropped
    }

    /// The next JSON line, or why there is none. Checks the cancel flag every
    /// 80 ms: a blocked read must never make Stop wait.
    pub async fn next_until(&mut self, cancel: &CancelFlag, deadline: Instant) -> Wait {
        loop {
            if let Some(w) = self.next_tick(cancel, deadline).await {
                return w;
            }
        }
    }

    /// Like [`next_until`], but gives up after at most 80 ms with `None`, so a
    /// caller that has other things to look at (a steer, a question) can do so
    /// between messages.
    pub async fn next_tick(&mut self, cancel: &CancelFlag, deadline: Instant) -> Option<Wait> {
        loop {
            if cancel.is_cancelled() {
                return Some(Wait::Cancelled);
            }
            let now = Instant::now();
            if now >= deadline {
                return Some(Wait::TimedOut);
            }
            let slice = (deadline - now).min(Duration::from_millis(80));
            match tokio::time::timeout(slice, self.lines.next_line()).await {
                Err(_) => return None,
                Ok(Err(_)) | Ok(Ok(None)) => return Some(Wait::Closed),
                Ok(Ok(Some(line))) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(&line) {
                        Ok(v) => return Some(Wait::Message(v)),
                        Err(_) => self.unknown += 1,
                    }
                }
            }
        }
    }

    /// End the run: wait briefly for a clean exit, then stop the whole tree.
    pub async fn finish(mut self) -> Option<i32> {
        self.close_input();
        match tokio::time::timeout(Duration::from_secs(3), self.child.wait()).await {
            Ok(Ok(status)) => status.code(),
            _ => {
                kill_tree(&mut self.child).await;
                None
            }
        }
    }
}

/// What a one-off command printed.
#[derive(Debug, Default, Clone)]
pub struct Captured {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Run a short command (`--version`, a status check) to its end, or stop it
/// after `timeout`. Output is capped. Stdin is closed, so nothing can wait for
/// a keypress.
pub async fn run_capture(cmd: Command, timeout: Duration) -> Result<Captured, String> {
    let mut cmd = cmd;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("It would not start: {e}"))?;
    #[cfg(windows)]
    if let Some(h) = child.raw_handle() {
        crate::runtime::jobobject::assign_raw(h);
    }
    let read_capped = |stream: Option<Box<dyn AsyncRead + Unpin + Send>>| {
        tokio::spawn(async move {
            let mut out = Vec::new();
            if let Some(s) = stream {
                let _ = s.take(64 * 1024).read_to_end(&mut out).await;
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    };
    let so = read_capped(child.stdout.take().map(|s| Box::new(s) as _));
    let se = read_capped(child.stderr.take().map(|s| Box::new(s) as _));
    let (code, timed_out) = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => (status.code(), false),
        Ok(Err(e)) => return Err(format!("It stopped unexpectedly: {e}")),
        Err(_) => {
            kill_tree(&mut child).await;
            (None, true)
        }
    };
    Ok(Captured {
        code,
        stdout: so.await.unwrap_or_default(),
        stderr: se.await.unwrap_or_default(),
        timed_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_line_over_the_cap_is_dropped_and_counted_not_buffered() {
        let mut data = Vec::new();
        data.extend_from_slice(b"first\n");
        data.extend(std::iter::repeat_n(b'x', 2 * 1024 * 1024));
        data.extend_from_slice(b"\nsecond\r\n");
        let mut r = LineReader::new(std::io::Cursor::new(data));
        assert_eq!(r.next_line().await.unwrap().as_deref(), Some("first"));
        assert_eq!(r.next_line().await.unwrap().as_deref(), Some("second"));
        assert_eq!(r.next_line().await.unwrap(), None);
        assert_eq!(r.dropped, 1);
    }

    #[tokio::test]
    async fn the_last_line_without_a_newline_is_still_read() {
        let mut r = LineReader::new(std::io::Cursor::new(b"a\nb".to_vec()));
        assert_eq!(r.next_line().await.unwrap().as_deref(), Some("a"));
        assert_eq!(r.next_line().await.unwrap().as_deref(), Some("b"));
        assert_eq!(r.next_line().await.unwrap(), None);
    }

    #[test]
    fn the_stderr_tail_keeps_only_the_end() {
        let t = Tail::default();
        t.push(&"a".repeat(TAIL_BYTES));
        t.push("END");
        let text = t.text();
        assert!(text.len() <= TAIL_BYTES);
        assert!(text.ends_with("END"));
    }

    /// `CLI-T-5`: no Poiesis variable and no provider key reaches the child; a
    /// key the user chose is added by name; the agent's ask-first setting wins.
    #[test]
    fn the_child_env_has_no_poiesis_vars_and_no_provider_keys() {
        std::env::set_var("POIESIS_TEST_SECRET", "x");
        std::env::set_var("OPENAI_API_KEY", "sk-leak");
        std::env::set_var("ANTHROPIC_API_KEY", "sk-leak2");
        std::env::set_var("POIESIS_TEST_KEEP_ME_NOT", "x");
        let opencode = super::super::catalog::find("acp:opencode");
        let mut configured = BTreeMap::new();
        configured.insert("MY_SETTING".to_string(), "1".to_string());
        let env = child_env(opencode, &configured, None);
        let has = |k: &str| env.iter().any(|(ek, _)| ek.eq_ignore_ascii_case(k));
        assert!(!has("POIESIS_TEST_SECRET"));
        assert!(!has("OPENAI_API_KEY"));
        assert!(!has("ANTHROPIC_API_KEY"));
        assert!(has("MY_SETTING"));
        assert!(has("OPENCODE_CONFIG_CONTENT"), "OpenCode must be made to ask");

        let claude = super::super::catalog::find("claude-code");
        let env = child_env(claude, &BTreeMap::new(), Some(("ANTHROPIC_API_KEY", "sk-mine")));
        let key: Vec<_> = env.iter().filter(|(k, _)| k == "ANTHROPIC_API_KEY").collect();
        assert_eq!(key.len(), 1);
        assert_eq!(key[0].1, "sk-mine");
        assert!(!env.iter().any(|(k, _)| k == "OPENAI_API_KEY"));
        std::env::remove_var("POIESIS_TEST_SECRET");
        std::env::remove_var("POIESIS_TEST_KEEP_ME_NOT");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    #[test]
    fn a_shim_path_with_a_dangerous_character_is_refused() {
        let env: Vec<(String, String)> = Vec::new();
        assert!(command(Path::new(r"C:\a&b\tool.cmd"), &[], &env).is_err());
        assert!(command(Path::new(r"C:\ok dir\tool.cmd"), &["--version".into()], &env).is_ok());
        assert!(command(Path::new(r"C:\ok\tool.cmd"), &["a%PATH%".into()], &env).is_err());
    }
}
