//! Ported from oh-my-hermes apps/omher/src-tauri/src/past_chats.rs (MIT, commit b5cbcc9). See THIRD_PARTY_NOTICES.md.
//!
//! `XA-12`: the sessions Claude Code and Codex made outside Poiesis, listed for one folder so a chat on that
//! folder can continue one (`/resume`). Read-only: only the CLIs' own conversation files are opened, never
//! written. Claude Code keeps `<claude config>/projects/<folder name>/<uuid>.jsonl`, Codex
//! `<codex home>/sessions/YYYY/MM/DD/rollout-*.jsonl` and `session_index.jsonl` for thread names. A link anywhere
//! under those folders is refused, and every file read must lie inside the CLI's own home once its links are
//! resolved. No credential, token or config file is opened. A file is read in two bounded pieces (its head and its
//! tail), so a long conversation costs the same as a short one.
//!
//! Left out from the original, because Poiesis does not use it: the messages of a conversation (omher shows them
//! before continuing), the check that a conversation's file exists, the Codex rollout path to id lookup, and
//! everything about accounts. Changed: a Windows folder name compares without case or slash direction, the
//! home comes from the engine row's own `env` (Settings, Advanced), and a session Poiesis itself handed over is
//! named by its task (the text inside `<<<POIESIS_TASK`), not skipped as text the CLI wrote for itself.
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use serde_json::Value;

/// Conversations listed per agent.
pub const LIST_MAX: usize = 20;
/// Characters of a title or first words.
const TITLE_MAX: usize = 80;
/// Bytes read from the start of a file (its folder, first message, early title).
const HEAD: u64 = 256 * 1024;
/// Bytes read from the end of a file (its latest title).
const TAIL: u64 = 1024 * 1024;
/// Codex rollouts looked at for one listing, newest first.
const CODEX_FILES_MAX: usize = 400;
/// `session_index.jsonl` is read whole only up to this size.
const INDEX_MAX: u64 = 4 * 1024 * 1024;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct PastChat {
    /// The engine kind: `claude-code` or `codex`.
    pub agent: &'static str,
    /// The CLI's own id: a Claude Code session or a Codex thread (a UUID).
    pub id: String,
    pub cwd: String,
    /// The conversation's name (Claude Code's title, a Codex thread name) or its first words.
    pub title: String,
    /// `title` is a name the CLI or the person gave it, not the first words.
    pub named: bool,
    /// Last change, seconds since the epoch.
    pub at: f64,
}

/// The two agents whose conversations can be listed, by engine kind.
pub fn lists(kind: &str) -> bool {
    matches!(kind, "claude-code" | "codex")
}

/// A UUID, as Claude Code names its sessions and Codex its threads.
pub fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5 && parts.iter().zip([8, 4, 4, 4, 12]).all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The folder a CLI keeps its conversations in, as a new session of it would use: the variable in the engine's own
/// `env` (Settings, Advanced), else the one Poiesis itself runs with, else `~/.claude` / `~/.codex`.
pub fn cli_home(home: &Path, kind: &str, own_env: &[(String, String)], env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let (var, dir) = match kind {
        "claude-code" => ("CLAUDE_CONFIG_DIR", ".claude"),
        "codex" => ("CODEX_HOME", ".codex"),
        _ => return None,
    };
    let own = own_env.iter().find(|(k, v)| k == var && !v.is_empty()).map(|(_, v)| v.clone());
    Some(own.or_else(|| env(var).filter(|v| !v.is_empty())).map(PathBuf::from).unwrap_or_else(|| home.join(dir)))
}

/// Claude Code's project folder name for a cwd: every character that is not a letter or a digit becomes `-`.
pub fn claude_slug(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// `p` when it is a real file (not a link) inside `root` once both are resolved; else `None`.
pub fn inside(root: &Path, p: &Path) -> Option<PathBuf> {
    let meta = std::fs::symlink_metadata(p).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    let real = p.canonicalize().ok()?;
    let base = root.canonicalize().ok()?;
    real.starts_with(&base).then_some(real)
}

/// A real folder, not a link.
fn plain_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.is_dir()).unwrap_or(false)
}

/// Two names of a file or folder. On Windows the drive letter and every other letter may differ in case
/// (Claude Code writes `c--Users-...` for `C:\Users\...`).
fn same_name(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// A folder path as one comparable string: no trailing separator, and on Windows one slash direction and no `\\?\`.
fn folded(s: &str) -> String {
    let t = s.trim_end_matches(['/', '\\']);
    if cfg!(windows) {
        t.strip_prefix(r"\\?\").unwrap_or(t).replace('/', "\\")
    } else {
        t.to_string()
    }
}

fn same_folder(a: &str, b: &str) -> bool {
    !a.is_empty() && same_name(&folded(a), &folded(b))
}

fn mtime(p: &Path) -> f64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Whole lines from `[start, start + len)` of `path`: a line cut by either edge is dropped.
fn lines_in(path: &Path, start: u64, len: u64) -> Vec<Value> {
    let Ok(mut f) = File::open(path) else { return Vec::new() };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = start.min(size);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.take(len).read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let mut parts: Vec<&[u8]> = buf.split(|b| *b == b'\n').collect();
    if start > 0 && !parts.is_empty() {
        parts.remove(0); // begins mid-line
    }
    if start + (buf.len() as u64) < size {
        parts.pop(); // ends mid-line
    }
    parts.into_iter().filter(|l| !l.is_empty()).filter_map(|l| serde_json::from_slice(l).ok()).collect()
}

fn head(path: &Path) -> Vec<Value> {
    lines_in(path, 0, HEAD)
}

/// The tail of a file, or nothing when the head already covered it.
fn tail(path: &Path) -> Vec<Value> {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size <= HEAD {
        return Vec::new();
    }
    lines_in(path, size.saturating_sub(TAIL).max(HEAD), TAIL)
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// One line, control characters out, at most `n` characters.
fn clip(t: &str, n: usize) -> String {
    let one = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", one.chars().take(n - 1).collect::<String>())
    }
}

/// Text the person typed, not what the CLI put in the user turn for itself (command tags, reminders, the
/// AGENTS.md and environment blocks Codex sends first, interrupted-request notes).
fn typed(text: &str) -> bool {
    let t = text.trim_start();
    !t.is_empty() && !t.starts_with('<') && !t.starts_with("# AGENTS.md instructions") && !t.starts_with("Caveat:") && !t.starts_with("[Request interrupted")
}

// ---- Claude Code ------------------------------------------------------------------------------

/// The text of a Claude Code user line the person typed (not a tool result, a meta line or a subagent's).
fn claude_user_text(o: &Value) -> Option<String> {
    if s(o, "type") != "user" || o.get("isMeta").and_then(Value::as_bool) == Some(true) || o.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let c = &o["message"]["content"];
    let text = match c {
        Value::String(t) => t.clone(),
        Value::Array(blocks) => blocks.iter().filter(|b| s(b, "type") == "text").map(|b| s(b, "text")).collect::<Vec<_>>().join("\n"),
        _ => return None,
    };
    // A job Poiesis handed over starts with its fenced task (`<<<POIESIS_TASK`): the task is the words.
    if let Some(task) = super::run::unfenced_task(&text) {
        return (!task.is_empty()).then(|| task.to_string());
    }
    typed(&text).then_some(text)
}

/// The latest name in `lines`: a `custom-title` (a rename in Claude Code) wins over an `ai-title`.
fn claude_title(lines: &[Value]) -> (String, String) {
    let (mut custom, mut ai) = (String::new(), String::new());
    for o in lines {
        match s(o, "type") {
            "custom-title" if !s(o, "customTitle").trim().is_empty() => custom = clip(s(o, "customTitle"), TITLE_MAX),
            "ai-title" if !s(o, "aiTitle").trim().is_empty() => ai = clip(s(o, "aiTitle"), TITLE_MAX),
            _ => {}
        }
    }
    (custom, ai)
}

/// One Claude Code transcript as a list row, when it ran in `cwd` and the person said something in it.
pub fn claude_row(path: &Path, cwd: &str) -> Option<PastChat> {
    let id = path.file_stem()?.to_str()?.to_string();
    if !is_uuid(&id) {
        return None;
    }
    let first = head(path);
    // the folder name is lossy ("/a/b-c" and "/a/b/c" share it): the transcript's own cwd decides
    let seen = first.iter().map(|o| s(o, "cwd")).find(|c| !c.is_empty())?;
    if !same_folder(seen, cwd) {
        return None;
    }
    let words = first.iter().find_map(claude_user_text)?;
    let (mut custom, mut ai) = claude_title(&first);
    let (c2, a2) = claude_title(&tail(path));
    if !c2.is_empty() {
        custom = c2;
    }
    if !a2.is_empty() {
        ai = a2;
    }
    let named = !custom.is_empty() || !ai.is_empty();
    let title = if !custom.is_empty() {
        custom
    } else if !ai.is_empty() {
        ai
    } else {
        clip(&words, TITLE_MAX)
    };
    Some(PastChat { agent: "claude-code", id, cwd: cwd.trim_end_matches(['/', '\\']).to_string(), title, named, at: mtime(path) })
}

/// The project folder of `cwd` under `projects`: the exact name, else (Windows) the one that differs only in case.
fn claude_dir(projects: &Path, cwd: &str) -> Option<PathBuf> {
    let slug = claude_slug(cwd);
    let exact = projects.join(&slug);
    if plain_dir(&exact) {
        return Some(exact);
    }
    if !cfg!(windows) {
        return None;
    }
    std::fs::read_dir(projects).ok()?.flatten().map(|e| e.path()).find(|p| plain_dir(p) && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| same_name(n, &slug)))
}

/// The recent Claude Code conversations in each of `cwds`, newest first.
pub fn claude_list(root: &Path, cwds: &[String]) -> Vec<PastChat> {
    let projects = root.join("projects");
    if !plain_dir(&projects) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for cwd in cwds {
        let Some(dir) = claude_dir(&projects, cwd) else { continue };
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<(f64, PathBuf)> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl") && p.file_stem().and_then(|x| x.to_str()).is_some_and(is_uuid))
            .filter_map(|p| inside(root, &p))
            .map(|p| (mtime(&p), p))
            .collect();
        files.sort_by(|a, b| b.0.total_cmp(&a.0));
        // a few more than shown: empty ones (only /clear) and other folders' are skipped
        out.extend(files.iter().take(LIST_MAX * 2).filter_map(|(_, p)| claude_row(p, cwd)).take(LIST_MAX));
    }
    newest(out)
}

// ---- Codex ------------------------------------------------------------------------------------

/// Codex rollout files, newest day first (names carry the time, so newest first within a day too), at most
/// `max`. Links are skipped at every level.
fn codex_rollouts(root: &Path, max: usize) -> Vec<PathBuf> {
    let sessions = root.join("sessions");
    if !plain_dir(&sessions) {
        return Vec::new();
    }
    let digits = |p: &Path, n: usize| p.file_name().and_then(|x| x.to_str()).is_some_and(|x| x.len() == n && x.bytes().all(|b| b.is_ascii_digit()));
    let sub = |d: &Path, n: usize| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(d).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| plain_dir(p) && digits(p, n)).collect()).unwrap_or_default();
        v.sort();
        v.reverse();
        v
    };
    let mut out = Vec::new();
    for y in sub(&sessions, 4) {
        for m in sub(&y, 2) {
            for d in sub(&m, 2) {
                let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
                    .map(|rd| {
                        rd.flatten()
                            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                            .map(|e| e.path())
                            .filter(|p| p.file_name().and_then(|x| x.to_str()).is_some_and(|x| x.starts_with("rollout-") && x.ends_with(".jsonl")))
                            .collect()
                    })
                    .unwrap_or_default();
                files.sort();
                files.reverse();
                for f in files {
                    if out.len() >= max {
                        return out;
                    }
                    out.push(f);
                }
            }
        }
    }
    out
}

/// Thread names from `session_index.jsonl` (the last name of each id wins).
fn codex_names(root: &Path) -> std::collections::HashMap<String, String> {
    let mut names = std::collections::HashMap::new();
    let idx = root.join("session_index.jsonl");
    let Some(p) = inside(root, &idx) else { return names };
    if std::fs::metadata(&p).map(|m| m.len() > INDEX_MAX).unwrap_or(true) {
        return names;
    }
    for o in lines_in(&p, 0, INDEX_MAX) {
        let (id, name) = (s(&o, "id"), s(&o, "thread_name"));
        if !id.is_empty() && !name.trim().is_empty() {
            names.insert(id.to_string(), clip(name, TITLE_MAX));
        }
    }
    names
}

/// The text of a Codex user message the person typed (Codex's own AGENTS.md and environment blocks left out).
fn codex_user_text(o: &Value) -> Option<String> {
    let p = &o["payload"];
    if s(o, "type") != "response_item" || s(p, "type") != "message" || s(p, "role") != "user" {
        return None;
    }
    let blocks: Vec<&str> = p["content"].as_array()?.iter().filter(|b| s(b, "type") == "input_text").map(|b| s(b, "text")).collect();
    // A job Poiesis handed over: its fixed words, then the fenced task. The task is the words.
    if let Some(task) = blocks.iter().find_map(|t| super::run::unfenced_task(t)) {
        return (!task.is_empty()).then(|| task.to_string());
    }
    let text = blocks.into_iter().filter(|t| typed(t)).collect::<Vec<_>>().join("\n");
    (!text.trim().is_empty()).then_some(text)
}

/// A rollout's `session_meta`: (thread id, cwd), when it is a conversation of its own (not a subagent's thread).
fn codex_meta(lines: &[Value]) -> Option<(String, String)> {
    let first = lines.first()?;
    if s(first, "type") != "session_meta" {
        return None;
    }
    let p = &first["payload"];
    // a subagent's thread names its parent in `source`; a fork names where it came from
    if p.get("source").is_some_and(Value::is_object) || p.get("forked_from_id").is_some_and(|v| !v.is_null()) {
        return None;
    }
    let id = s(p, "id").to_string();
    is_uuid(&id).then(|| (id, s(p, "cwd").to_string()))
}

/// One Codex rollout as a list row, when it ran in one of `cwds` and the person said something in it.
pub fn codex_row(path: &Path, cwds: &[String], names: &std::collections::HashMap<String, String>) -> Option<PastChat> {
    let first = head(path);
    let (id, seen) = codex_meta(&first)?;
    let cwd = cwds.iter().find(|c| same_folder(&seen, c))?;
    let words = first.iter().find_map(codex_user_text)?;
    let name = names.get(&id).cloned().unwrap_or_default();
    let named = !name.is_empty();
    let title = if named { name } else { clip(&words, TITLE_MAX) };
    Some(PastChat { agent: "codex", id, cwd: cwd.trim_end_matches(['/', '\\']).to_string(), title, named, at: mtime(path) })
}

/// The recent Codex conversations in any of `cwds`, newest first.
pub fn codex_list(root: &Path, cwds: &[String]) -> Vec<PastChat> {
    let names = codex_names(root);
    let rows = codex_rollouts(root, CODEX_FILES_MAX).iter().filter_map(|p| inside(root, p)).filter_map(|p| codex_row(&p, cwds, &names)).collect();
    newest(rows)
}

// ---- both -------------------------------------------------------------------------------------

/// `XA-13`: one session found outside Poiesis, as the `/resume` list shows it.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct OutsideSession {
    pub engine_id: String,
    /// The name `/delegate` takes for this agent.
    pub agent: String,
    pub name: String,
    pub session_id: String,
    pub title: String,
    pub last_at_ms: i64,
    /// One word the person can type to pick it: see [`token`].
    pub token: String,
}

/// The one word that picks a session in `/resume`: `cc-` (Claude Code) or `cx-` (Codex) and the **last** 8
/// characters of its id. The first 8 would not do: a Codex thread id begins with the time, so threads started
/// within the same hour share them.
pub fn token(kind: &str, id: &str) -> String {
    let tail: String = id.chars().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{}-{tail}", if kind == "codex" { "cx" } else { "cc" })
}

/// The person's own folder, where `.claude` and `.codex` are.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The sessions of Claude Code and Codex in `folder`, for each of them that is found and switched on, newest
/// first, at most [`LIST_MAX`] in all. No folder, no list.
pub fn outside_sessions(db: &crate::db::Db, folder: Option<&str>, home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<OutsideSession> {
    let Some(folder) = folder.filter(|f| !f.trim().is_empty()) else { return Vec::new() };
    let mut out: Vec<(f64, OutsideSession)> = Vec::new();
    for row in db.list_engines().unwrap_or_default() {
        if !lists(&row.kind) || !row.enabled || row.found_path.is_none() {
            continue;
        }
        let own: Vec<(String, String)> = serde_json::from_str::<std::collections::BTreeMap<String, String>>(&row.env_json).unwrap_or_default().into_iter().collect();
        let Some(root) = cli_home(home, &row.kind, &own, env) else { continue };
        for c in list(&root, &row.kind, &[folder.to_string()]).unwrap_or_default() {
            out.push((
                c.at,
                OutsideSession {
                    engine_id: row.id.clone(),
                    agent: row.id.clone(),
                    name: row.display_name.clone(),
                    token: token(&row.kind, &c.id),
                    session_id: c.id,
                    title: c.title,
                    last_at_ms: (c.at * 1000.0) as i64,
                },
            ));
        }
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.truncate(LIST_MAX);
    out.into_iter().map(|(_, s)| s).collect()
}

fn newest(mut v: Vec<PastChat>) -> Vec<PastChat> {
    v.sort_by(|a, b| b.at.total_cmp(&a.at));
    let mut seen = std::collections::HashSet::new();
    v.retain(|c| seen.insert(c.id.clone()));
    v.truncate(LIST_MAX);
    v
}

/// An absolute folder path, as each CLI records its cwd (`/…` on macOS and Linux, `C:\\…` on Windows).
fn absolute(c: &str) -> bool {
    Path::new(c).is_absolute() || c.starts_with('/')
}

/// Only absolute folders, at most 12.
pub fn checked_cwds(cwds: &[String]) -> Vec<String> {
    cwds.iter().filter(|c| absolute(c)).take(12).cloned().collect()
}

/// The conversations of the agent of engine kind `kind` in `cwds`, read from `root` (its home, [`cli_home`]).
pub fn list(root: &Path, kind: &str, cwds: &[String]) -> Result<Vec<PastChat>, String> {
    let cwds = checked_cwds(cwds);
    match kind {
        "claude-code" => Ok(claude_list(root, &cwds)),
        "codex" => Ok(codex_list(root, &cwds)),
        _ => Err(format!("{kind} conversations are not listed")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const C1: &str = "11111111-2222-4333-8444-555555555555";
    const C2: &str = "22222222-2222-4333-8444-555555555555";
    const X1: &str = "01a1d060-4ab7-72b2-b02b-1dbee1d5f52c";
    const X2: &str = "01a1d060-6967-74c3-8b7c-33c05229878b";

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("poiesis-past-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, lines: &[Value]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut f = File::create(p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    fn claude_file(cwd: &str, id: &str) -> Vec<Value> {
        vec![
            serde_json::json!({"type": "user", "isMeta": true, "cwd": cwd, "message": {"role": "user", "content": "<local-command-caveat>x</local-command-caveat>"}, "timestamp": "2026-11-12T10:00:00.000Z", "sessionId": id}),
            serde_json::json!({"type": "user", "cwd": cwd, "message": {"role": "user", "content": "<command-name>/clear</command-name>"}, "timestamp": "2026-11-12T10:00:01.000Z"}),
            serde_json::json!({"type": "user", "cwd": cwd, "message": {"role": "user", "content": "Remember the word MARIGOLD for later"}, "timestamp": "2026-11-12T10:00:02.000Z"}),
            serde_json::json!({"type": "assistant", "cwd": cwd, "message": {"id": "m1", "role": "assistant", "content": [{"type": "thinking", "thinking": "hm"}]}, "timestamp": "2026-11-12T10:00:03.000Z"}),
            serde_json::json!({"type": "assistant", "cwd": cwd, "message": {"id": "m1", "role": "assistant", "content": [{"type": "text", "text": "Noted."}]}, "timestamp": "2026-11-12T10:00:04.000Z"}),
            serde_json::json!({"type": "assistant", "cwd": cwd, "message": {"id": "m1", "role": "assistant", "content": [{"type": "tool_use", "name": "Bash", "input": {}}]}, "timestamp": "2026-11-12T10:00:05.000Z"}),
            serde_json::json!({"type": "user", "cwd": cwd, "message": {"role": "user", "content": [{"type": "tool_result", "content": "ok"}]}, "timestamp": "2026-11-12T10:00:06.000Z"}),
            serde_json::json!({"type": "assistant", "cwd": cwd, "message": {"id": "m1", "role": "assistant", "content": [{"type": "text", "text": "I will remember it."}]}, "timestamp": "2026-11-12T10:00:07.000Z"}),
            serde_json::json!({"type": "assistant", "isSidechain": true, "cwd": cwd, "message": {"id": "s1", "role": "assistant", "content": [{"type": "text", "text": "subagent says"}]}}),
            serde_json::json!({"type": "ai-title", "aiTitle": "Remember a word", "sessionId": id}),
        ]
    }

    fn codex_file(id: &str, cwd: &str) -> Vec<Value> {
        vec![
            serde_json::json!({"timestamp": "2026-11-12T15:28:55.873Z", "type": "session_meta", "payload": {"id": id, "cwd": cwd, "originator": "codex-tui", "source": "cli"}}),
            serde_json::json!({"timestamp": "2026-11-12T15:28:56.000Z", "type": "response_item", "payload": {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "rules"}]}}),
            serde_json::json!({"timestamp": "2026-11-12T15:28:56.000Z", "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "# AGENTS.md instructions for /w\n..."}, {"type": "input_text", "text": "<environment_context>x</environment_context>"}]}}),
            serde_json::json!({"timestamp": "2026-11-12T15:29:00.000Z", "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Remember COBALT please"}]}}),
            serde_json::json!({"timestamp": "2026-11-12T15:29:05.000Z", "type": "response_item", "payload": {"type": "reasoning", "summary": []}}),
            serde_json::json!({"timestamp": "2026-11-12T15:29:09.000Z", "type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Remembered."}]}}),
        ]
    }

    #[test]
    fn claude_lists_the_folders_conversations_with_their_title_and_skips_empty_ones() {
        let root = tmp("claude-list");
        let cwd = "/Users/you/code/web-app";
        let dir = root.join("projects").join(claude_slug(cwd));
        write(&dir.join(format!("{C1}.jsonl")), &claude_file(cwd, C1));
        // only /clear: nothing the person said, so not listed
        write(&dir.join(format!("{C2}.jsonl")), &claude_file(cwd, C2)[..2]);
        // the lossy folder name: another folder with the same name is not this one
        let other = "/Users/you/code/web/app";
        assert_eq!(claude_slug(other), claude_slug(cwd));
        write(&dir.join("33333333-2222-4333-8444-555555555555.jsonl"), &claude_file(other, "x"));
        // not a conversation file
        write(&dir.join("notes.jsonl"), &claude_file(cwd, "y"));
        let rows = list(&root, "claude-code", &[cwd.to_string(), "relative/x".to_string()]).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!((rows[0].id.as_str(), rows[0].title.as_str(), rows[0].named, rows[0].cwd.as_str()), (C1, "Remember a word", true, cwd));
        // a rename in Claude Code wins; without any title the first words name it
        let mut lines = claude_file(cwd, C1);
        lines.push(serde_json::json!({"type": "custom-title", "customTitle": "My  word\ngame"}));
        write(&dir.join(format!("{C1}.jsonl")), &lines);
        assert_eq!(claude_list(&root, &[cwd.into()])[0].title, "My word game");
        write(&dir.join(format!("{C1}.jsonl")), &claude_file(cwd, C1)[..8]);
        let r = &claude_list(&root, &[cwd.into()])[0];
        assert_eq!((r.title.as_str(), r.named), ("Remember the word MARIGOLD for later", false));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn claude_code_names_a_windows_folder_with_dashes_and_a_lower_case_drive_letter() {
        // Erich's real folder: C:\Users\Erich\.claude\projects\c--Users-Erich-workspace-nexus
        assert_eq!(claude_slug(r"C:\Users\Erich\workspace\nexus"), "C--Users-Erich-workspace-nexus");
        assert_eq!(claude_slug(r"c:\Users\Erich\workspace\nexus"), "c--Users-Erich-workspace-nexus");
        assert_eq!(folded(r"C:\Users\Erich\workspace\nexus\"), folded(r"C:\Users\Erich\workspace\nexus"));
        // the chat's folder has a capital C, the folder and the transcript a small one: found on Windows only
        if cfg!(windows) {
            let root = tmp("win-folder");
            let (chat, seen) = (r"C:\Users\Erich\workspace\nexus", r"c:\Users\Erich\workspace\nexus");
            let dir = root.join("projects").join("c--Users-Erich-workspace-nexus");
            write(&dir.join(format!("{C1}.jsonl")), &claude_file(seen, C1));
            let rows = claude_list(&root, &[chat.to_string()]);
            assert_eq!(rows.len(), 1, "{rows:?}");
            assert_eq!(rows[0].id, C1);
            // slashes the other way round and a trailing one are the same folder
            assert!(same_folder("C:/Users/Erich/workspace/nexus/", seen));
            assert!(same_folder(r"\\?\C:\Users\Erich\workspace\nexus", seen));
            assert!(!same_folder(r"C:\Users\Erich\workspace\nexus2", seen));
            let _ = std::fs::remove_dir_all(&root);
        } else {
            assert!(!same_folder("/a/B", "/a/b"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn links_and_paths_outside_the_cli_home_are_refused() {
        use std::os::unix::fs::symlink;
        let root = tmp("links");
        let away = tmp("links-away");
        let cwd = "/w";
        let slug = claude_slug(cwd);
        // a transcript outside the home, linked in as a file
        write(&away.join(format!("{C1}.jsonl")), &claude_file(cwd, C1));
        std::fs::create_dir_all(root.join("projects").join(&slug)).unwrap();
        symlink(away.join(format!("{C1}.jsonl")), root.join("projects").join(&slug).join(format!("{C1}.jsonl"))).unwrap();
        assert!(claude_list(&root, &[cwd.into()]).is_empty());
        // a linked project folder
        let root2 = tmp("links2");
        std::fs::create_dir_all(root2.join("projects")).unwrap();
        symlink(&away, root2.join("projects").join(&slug)).unwrap();
        assert!(claude_list(&root2, &[cwd.into()]).is_empty());
        // a linked projects folder
        let root3 = tmp("links3");
        let real = tmp("links3-real");
        write(&real.join(&slug).join(format!("{C1}.jsonl")), &claude_file(cwd, C1));
        symlink(&real, root3.join("projects")).unwrap();
        assert!(claude_list(&root3, &[cwd.into()]).is_empty());
        // Codex: a linked day folder and a linked rollout
        let x = tmp("links-codex");
        let day = x.join("sessions/2026/11/12");
        std::fs::create_dir_all(&day).unwrap();
        write(&away.join("r.jsonl"), &codex_file(X1, cwd));
        symlink(away.join("r.jsonl"), day.join(format!("rollout-2026-11-12T00-00-00-{X1}.jsonl"))).unwrap();
        symlink(&away, x.join("sessions/2026/11/13")).unwrap();
        assert!(codex_list(&x, &[cwd.into()]).is_empty());
        // inside() itself
        assert!(inside(&root, &away.join(format!("{C1}.jsonl"))).is_none());
        for d in [root, away, root2, root3, real, x] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn codex_lists_its_own_threads_named_from_the_index_and_skips_subagents() {
        let root = tmp("codex-list");
        let cwd = "/Users/you/code/api";
        let day = root.join("sessions/2026/11/12");
        write(&day.join(format!("rollout-2026-11-12T15-28-54-{X1}.jsonl")), &codex_file(X1, cwd));
        write(&day.join(format!("rollout-2026-11-12T15-29-01-{X2}.jsonl")), &codex_file(X2, cwd));
        // a subagent's thread is not a conversation of its own
        let mut sub = codex_file("01a1d060-7a13-75e3-aa36-356cd0706c8a", cwd);
        sub[0]["payload"]["source"] = serde_json::json!({"subagent": {"thread_spawn": {"parent_thread_id": X1}}});
        write(&day.join("rollout-2026-11-12T15-29-06-01a1d060-7a13-75e3-aa36-356cd0706c8a.jsonl"), &sub);
        // another folder
        write(&root.join("sessions/2026/11/11").join("rollout-2026-11-11T00-00-00-01a1d060-0000-75e3-aa36-356cd0706c8a.jsonl"), &codex_file("01a1d060-0000-75e3-aa36-356cd0706c8a", "/elsewhere"));
        write(&root.join("session_index.jsonl"), &[serde_json::json!({"id": X1, "thread_name": "Old name"}), serde_json::json!({"id": X1, "thread_name": "Remember a colour"})]);
        let rows = list(&root, "codex", &[cwd.to_string()]).unwrap();
        let mut got: Vec<(&str, &str, bool)> = rows.iter().map(|r| (r.id.as_str(), r.title.as_str(), r.named)).collect();
        got.sort();
        assert_eq!(got, [(X1, "Remember a colour", true), (X2, "Remember COBALT please", false)]);
        assert!(list(&root, "omo", &[cwd.to_string()]).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_long_transcript_is_read_by_its_head_and_tail_only() {
        let root = tmp("long");
        let cwd = "/w";
        let mut lines = claude_file(cwd, C1)[..8].to_vec();
        let filler = "x".repeat(2000);
        for i in 0..1500 {
            lines.push(serde_json::json!({"type": "attachment", "cwd": cwd, "attachment": {"n": i, "pad": filler}}));
        }
        lines.push(serde_json::json!({"type": "ai-title", "aiTitle": "Late title"}));
        let p = root.join("projects").join(claude_slug(cwd)).join(format!("{C1}.jsonl"));
        write(&p, &lines);
        assert!(std::fs::metadata(&p).unwrap().len() > HEAD + TAIL);
        let r = &claude_list(&root, &[cwd.into()])[0];
        assert_eq!(r.title, "Late title");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_home_follows_the_engines_own_env_then_the_process_env_then_the_default() {
        let h = Path::new("/home/you");
        let none = |_: &str| None;
        let env = |k: &str| (k == "CLAUDE_CONFIG_DIR").then(|| "/env/claude".to_string());
        assert_eq!(cli_home(h, "claude-code", &[], &none), Some(h.join(".claude")));
        assert_eq!(cli_home(h, "codex", &[], &none), Some(h.join(".codex")));
        assert_eq!(cli_home(h, "claude-code", &[], &env), Some(PathBuf::from("/env/claude")));
        let own = [("CLAUDE_CONFIG_DIR".to_string(), "/home/you/work-claude".to_string())];
        assert_eq!(cli_home(h, "claude-code", &own, &env), Some(PathBuf::from("/home/you/work-claude")));
        // an empty value in the engine's env is not a folder
        let empty = [("CODEX_HOME".to_string(), String::new())];
        assert_eq!(cli_home(h, "codex", &empty, &none), Some(h.join(".codex")));
        assert_eq!(cli_home(h, "acp:opencode", &[], &none), None);
        assert!(lists("claude-code") && lists("codex") && !lists("acp:opencode") && !lists("../claude-code"));
    }

    fn found(db: &crate::db::Db, id: &str, kind: &str, name: &str, env: Option<(&str, &Path)>) {
        let mut row = crate::db::engines::EngineRow::new(id, kind, name);
        row.found_path = Some("C:\\x\\agent.exe".into());
        row.enabled = true;
        if let Some((k, v)) = env {
            row.env_json = serde_json::json!({ k: v.to_string_lossy() }).to_string();
        }
        db.insert_engine_if_absent(&row).unwrap();
    }

    #[test]
    fn the_list_for_a_folder_mixes_both_agents_newest_first_with_one_word_for_each() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let home = tmp("outside-home");
        let codex_home = tmp("outside-codex");
        let cwd = if cfg!(windows) { r"C:\Users\you\code\api" } else { "/Users/you/code/api" };
        // Claude Code in the default place under the person's folder
        let claude_dir = home.join(".claude").join("projects").join(claude_slug(cwd));
        write(&claude_dir.join(format!("{C1}.jsonl")), &claude_file(cwd, C1));
        // Codex in the folder its engine row names; two threads whose ids begin the same
        let day = codex_home.join("sessions/2026/11/12");
        write(&day.join(format!("rollout-2026-11-12T15-28-54-{X1}.jsonl")), &codex_file(X1, cwd));
        write(&day.join(format!("rollout-2026-11-12T15-29-01-{X2}.jsonl")), &codex_file(X2, cwd));
        found(&db, "claude-code", "claude-code", "Claude Code", None);
        found(&db, "codex", "codex", "Codex", Some(("CODEX_HOME", &codex_home)));
        let none = |_: &str| None;

        let rows = outside_sessions(&db, Some(cwd), &home, &none);
        let mut ids: Vec<&str> = rows.iter().map(|r| r.session_id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, [X1, X2, C1]);
        assert!(rows.windows(2).all(|w| w[0].last_at_ms >= w[1].last_at_ms), "newest first");
        let claude = rows.iter().find(|r| r.session_id == C1).unwrap();
        assert_eq!((claude.engine_id.as_str(), claude.name.as_str(), claude.title.as_str()), ("claude-code", "Claude Code", "Remember a word"));
        assert_eq!(claude.token, "cc-55555555");
        // the one word differs even for Codex threads that began within the same hour
        let tokens: Vec<&str> = rows.iter().map(|r| r.token.as_str()).collect();
        assert_eq!(tokens.iter().collect::<std::collections::HashSet<_>>().len(), 3, "{tokens:?}");
        assert!(rows.iter().filter(|r| r.engine_id == "codex").all(|r| r.token.starts_with("cx-")));
        // /delegate takes the same name
        let slugs: Vec<String> = crate::agent::subagents::delegate_targets(&db, None).into_iter().map(|t| t.slug).collect();
        assert!(slugs.contains(&claude.agent) && slugs.iter().any(|s| s == "codex"), "{slugs:?}");

        // another folder, or none: nothing
        assert!(outside_sessions(&db, Some("/somewhere/else"), &home, &none).is_empty());
        assert!(outside_sessions(&db, None, &home, &none).is_empty());
        assert!(outside_sessions(&db, Some("  "), &home, &none).is_empty());
        // an agent that is switched off or not found is not listed
        let mut off = db.get_engine("claude-code").unwrap().unwrap();
        off.enabled = false;
        db.save_engine_settings(&off).unwrap();
        let rows = outside_sessions(&db, Some(cwd), &home, &none);
        assert!(rows.iter().all(|r| r.engine_id == "codex"), "{rows:?}");
        // at most 20 in all
        for i in 0..30 {
            let id = format!("01a1d060-0000-75e3-aa36-{i:012}");
            write(&day.join(format!("rollout-2026-11-12T16-00-{i:02}-{id}.jsonl")), &codex_file(&id, cwd));
        }
        assert_eq!(outside_sessions(&db, Some(cwd), &home, &none).len(), LIST_MAX);
        for d in [home, codex_home] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// The plan: "Poiesis hand-offs also make sessions in the same place, so the list shows those too." Their
    /// first message is the fenced task (and, for Codex, the fixed words before it); the task names them.
    #[test]
    fn a_session_poiesis_handed_over_is_listed_by_its_task() {
        let root = tmp("poiesis-made");
        let cwd = "/w";
        let fenced = crate::engines::run::fenced_task("Add tests for the login form");
        let mut lines = claude_file(cwd, C1);
        lines[2]["message"]["content"] = serde_json::json!([{"type": "text", "text": fenced}]);
        lines.truncate(8); // no title: the first words name it
        write(&root.join("projects").join(claude_slug(cwd)).join(format!("{C1}.jsonl")), &lines);
        let rows = claude_list(&root, &[cwd.into()]);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].title, "Add tests for the login form");

        let mut lines = codex_file(X1, cwd);
        lines[3]["payload"]["content"] = serde_json::json!([
            {"type": "input_text", "text": "You were given one task by Poiesis, a desktop assistant."},
            {"type": "input_text", "text": fenced},
        ]);
        write(&root.join("sessions/2026/11/12").join(format!("rollout-2026-11-12T15-28-54-{X1}.jsonl")), &lines);
        let rows = codex_list(&root, &[cwd.into()]);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].title, "Add tests for the login form");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn typed_text_and_folders() {
        assert!(typed("hello") && !typed("<command-name>/x</command-name>") && !typed("  ") && !typed("# AGENTS.md instructions for /w"));
        assert_eq!(checked_cwds(&["/a".into(), "b".into(), "C:\\x".into()]).first().map(String::as_str), Some("/a"));
        assert!(is_uuid(C1) && !is_uuid("../x") && !is_uuid(&format!("{C1}/../{C1}")));
    }
}
