//! `HRN-8`: a big tool result gets a handle, not a paste.
//!
//! Today every byte a tool returns goes into the transcript. One
//! `search_folder` over a large repo, or one `read_file` on a big file, can
//! spend a third of the context window in a single call — and the model
//! usually wanted four lines of it.
//!
//! So: anything past `INLINE_CAP` is written to disk, and the model is handed
//! the first `PREVIEW` bytes plus a reference it can page through with
//! `read_result` or grep with `search_result`. The user is handed the whole
//! thing, because the point of keeping it is that they can read what the model
//! chose not to.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Past this, a result is kept on disk instead of pasted. 8 KB is roughly two
/// thousand tokens: big enough that no ordinary answer trips it, small enough
/// that a runaway one cannot quietly eat the window.
pub const INLINE_CAP: usize = 8 * 1024;

/// How much the model still sees inline. Enough to tell whether the rest is
/// worth paging through, which is the only decision it has to make here.
pub const PREVIEW: usize = 2 * 1024;

/// How much one `read_result` window may return, so paging cannot undo the
/// saving one page at a time.
const WINDOW_CAP: usize = 8 * 1024;

/// How many matching lines `search_result` returns.
const MATCH_LIMIT: usize = 40;

/// The results one run kept on disk.
///
/// Per run rather than per app: a reference is only ever quoted back inside the
/// run that produced it, and a run-scoped map means a stale handle from an
/// earlier run reads as "I do not have that" rather than as somebody else's
/// file.
pub struct ResultStore {
    dir: PathBuf,
    refs: Mutex<HashMap<String, PathBuf>>,
    /// Set the first time anything is kept. Until then the two tools below are
    /// not advertised at all — a tool that can only fail is worse than no tool.
    any: std::sync::atomic::AtomicBool,
}

impl ResultStore {
    /// `<data_dir>/results/<conversation_id>/<run_id>/`. Nested under the
    /// conversation so deleting a conversation takes its kept results with it,
    /// the same way artifacts go.
    pub fn new(data_dir: &Path, conversation_id: &str, run_id: &str) -> Self {
        Self {
            dir: data_dir.join("results").join(conversation_id).join(run_id),
            refs: Mutex::new(HashMap::new()),
            any: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Has this run kept anything yet?
    pub fn has_any(&self) -> bool {
        self.any.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Keep `output` and return `(reference, what the model sees instead)`.
    /// `None` when the output is small enough to paste, which is the common
    /// case and costs one length check.
    pub fn keep(&self, call_id: &str, output: &str) -> Option<(String, String)> {
        if output.len() <= INLINE_CAP {
            return None;
        }
        // Derived from the call id so the handle and the timeline step it came
        // from can be lined up later, and short so it costs the model nothing
        // to quote back.
        let tail: String = call_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .take(6)
            .collect();
        let reference = format!("res_{tail}");
        if std::fs::create_dir_all(&self.dir).is_err() {
            return None;
        }
        let path = self.dir.join(format!("{reference}.txt"));
        if std::fs::write(&path, output).is_err() {
            return None;
        }
        self.refs.lock().ok()?.insert(reference.clone(), path);
        self.any.store(true, std::sync::atomic::Ordering::Relaxed);
        Some((reference.clone(), preview_for(&reference, output)))
    }

    fn path_of(&self, reference: &str) -> Option<PathBuf> {
        self.refs.lock().ok()?.get(reference).cloned()
    }

    fn read(&self, reference: &str) -> Result<String, String> {
        let path = self
            .path_of(reference)
            .ok_or_else(|| format!("No kept result called {reference} in this run."))?;
        std::fs::read_to_string(&path).map_err(|e| format!("Could not read {reference}: {e}"))
    }
}

/// What replaces a kept result in the transcript.
fn preview_for(reference: &str, output: &str) -> String {
    let head = clip(output, PREVIEW);
    format!(
        "{head}\n\n[truncated: {} total. Use read_result {{\"ref\": \"{reference}\", \"offset\": N, \"limit\": N}} \
or search_result {{\"ref\": \"{reference}\", \"query\": \"...\"}} to see more.]",
        human_size(output.len())
    )
}

/// Cut on a character boundary, never a byte one.
fn clip(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub fn human_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn handles(name: &str) -> bool {
    matches!(name, "read_result" | "search_result")
}

/// Advertised only once this run has actually kept something (see
/// `ResultStore::any`).
pub fn tool_specs() -> Vec<serde_json::Value> {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "read_result",
                "description": "Read part of a large tool result that was kept on disk instead of pasted. Use the reference from the [truncated: ...] note.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "ref": { "type": "string", "description": "the reference, e.g. res_7f3a" },
                        "offset": { "type": "integer", "description": "line to start at, 0-based, default 0" },
                        "limit": { "type": "integer", "description": "how many lines, default 200" }
                    },
                    "required": ["ref"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "search_result",
                "description": "Find the lines matching a query inside a large tool result that was kept on disk. Prefer this over paging through the whole thing.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "ref": { "type": "string" },
                        "query": { "type": "string", "description": "plain text, matched case-insensitively" }
                    },
                    "required": ["ref", "query"]
                }
            }
        }
    ])
    .as_array()
    .cloned()
    .unwrap_or_default()
}

pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    let reference = args.get("ref").and_then(|v| v.as_str()).unwrap_or("");
    match name {
        "search_result" => {
            let q = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            ("searched the kept result".into(), format!("\u{201c}{q}\u{201d}"))
        }
        _ => ("read the kept result".into(), reference.to_string()),
    }
}

pub fn execute(
    store: &ResultStore,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let reference = args
        .get("ref")
        .and_then(|v| v.as_str())
        .ok_or("`ref` is required.")?;
    let body = store.read(reference)?;
    match name {
        "read_result" => {
            let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
            Ok(window(&body, offset, limit))
        }
        "search_result" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or("`query` is required.")?;
            Ok(search(&body, query))
        }
        other => Err(format!("Unknown result tool: {other}")),
    }
}

/// `limit` lines from `offset`, capped so paging cannot spend the window one
/// page at a time.
fn window(body: &str, offset: usize, limit: usize) -> String {
    let lines: Vec<&str> = body.lines().collect();
    if offset >= lines.len() {
        return format!("(past the end: this result has {} lines)", lines.len());
    }
    let end = (offset + limit.max(1)).min(lines.len());
    let slice = lines[offset..end].join("\n");
    let text = clip(&slice, WINDOW_CAP).to_string();
    let more = if end < lines.len() {
        format!("\n\n[{} more lines. Next offset: {end}.]", lines.len() - end)
    } else {
        String::new()
    };
    format!("{text}{more}")
}

fn search(body: &str, query: &str) -> String {
    let needle = query.to_lowercase();
    let hits: Vec<String> = body
        .lines()
        .enumerate()
        .filter(|(_, line)| line.to_lowercase().contains(&needle))
        .take(MATCH_LIMIT)
        .map(|(i, line)| format!("{i}: {}", clip(line.trim(), 400)))
        .collect();
    if hits.is_empty() {
        return format!("No line matches \u{201c}{query}\u{201d}.");
    }
    format!(
        "{} matching line(s), as `line number: text`:\n{}",
        hits.len(),
        hits.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> ResultStore {
        let run = format!("run_{}", uuid::Uuid::new_v4().simple());
        ResultStore::new(&std::env::temp_dir().join("poiesis-test"), "conv", &run)
    }

    /// `HRN-T6`: the boundary itself stays inline. An off-by-one here would
    /// offload results that were never a problem, and every one of those costs
    /// the model an extra round trip to read what it already had.
    #[test]
    fn the_cap_itself_is_still_pasted_and_one_byte_past_it_is_not() {
        let store = store();
        assert!(store.keep("call_a", &"x".repeat(INLINE_CAP)).is_none());
        assert!(!store.has_any(), "nothing kept means the tools stay unadvertised");
        assert!(store.keep("call_b", &"x".repeat(INLINE_CAP + 1)).is_some());
        assert!(store.has_any());
    }

    #[test]
    fn what_the_model_sees_is_a_preview_and_a_handle() {
        let store = store();
        let body = "line\n".repeat(4000);
        let (reference, shown) = store.keep("call_c", &body).unwrap();
        assert!(shown.len() < body.len() / 2, "the point is that it is smaller");
        assert!(shown.starts_with("line\n"));
        assert!(shown.contains(&reference), "the handle has to be in the text");
        assert!(shown.contains("read_result"));
    }

    #[test]
    fn a_window_returns_the_right_slice_and_says_what_is_left() {
        let store = store();
        let body: String = (0..5000).map(|i| format!("line {i}\n")).collect();
        let (reference, _) = store.keep("call_d", &body).unwrap();
        let out = execute(
            &store,
            "read_result",
            &serde_json::json!({ "ref": reference, "offset": 10, "limit": 3 }),
        )
        .unwrap();
        assert!(out.starts_with("line 10\nline 11\nline 12"));
        assert!(out.contains("Next offset: 13."));
    }

    #[test]
    fn searching_a_kept_result_gives_line_numbers_to_page_back_to() {
        let store = store();
        let mut body: String = (0..3000).map(|i| format!("line {i}\n")).collect();
        body.push_str("the needle is here\n");
        let (reference, _) = store.keep("call_e", &body).unwrap();
        let out = execute(
            &store,
            "search_result",
            &serde_json::json!({ "ref": reference, "query": "NEEDLE" }),
        )
        .unwrap();
        assert!(out.contains("3000: the needle is here"), "got: {out}");
    }

    /// A handle from some other run must read as "I do not have that", never as
    /// a file belonging to a different conversation.
    #[test]
    fn an_unknown_handle_is_refused_rather_than_guessed_at() {
        let store = store();
        let err = execute(&store, "read_result", &serde_json::json!({ "ref": "res_nope" }))
            .unwrap_err();
        assert!(err.contains("No kept result"));
    }
}
