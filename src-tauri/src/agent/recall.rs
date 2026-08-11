//! Built-in Recall toolset (RCL-2): the agent can search its own past — this
//! device's conversations and its durable memory — instead of pretending a
//! reference to "what we decided last time" is unrecoverable.
//!
//! Nothing here leaves the machine: it's an FTS query against the local SQLite
//! file. Every search is logged in the visible activity list, and every result
//! is emitted with provenance so the user can click back to the source.

use super::toolsets::ToolContext;
use super::AgentEvent;

/// Default and hard-cap on results, so a broad query can't flood the context.
const DEFAULT_LIMIT: usize = 5;
const MAX_LIMIT: usize = 8;
/// Caps on what a single tool result may return to the model.
const SEARCH_OUTPUT_CAP: usize = 2000;
const READ_OUTPUT_CAP: usize = 4000;
/// How many turns `read_conversation` shows, and how much of each.
const WINDOW_TURNS: usize = 12;
const WINDOW_MESSAGE_CAP: usize = 400;

pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "search_history",
                "description": "Search the user's past conversations and saved memories on this device. Use when the user references something from before ('like we discussed', 'my usual', a past decision) that is not in the current context. Returns matches with source and date.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "2-6 keywords, not a sentence" },
                        "limit": { "type": "integer", "description": "max results, default 5" }
                    },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_conversation",
                "description": "Read a short window of a past conversation found via search_history.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "conversation_id": { "type": "string" }
                    },
                    "required": ["conversation_id"]
                }
            }
        }
    ])
}

pub fn handles(name: &str) -> bool {
    matches!(name, "search_history" | "read_conversation")
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    match name {
        "search_history" => (
            "recalled".into(),
            args.get("query").and_then(|q| q.as_str()).unwrap_or("earlier").to_string(),
        ),
        "read_conversation" => ("reread".into(), "an earlier conversation".to_string()),
        other => (other.into(), String::new()),
    }
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

fn truncate(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n(truncated)", &s[..cut])
}

/// Format an epoch-ms timestamp as `YYYY-MM-DD`, so dates in results are
/// comparable without pulling in a date library.
fn as_date(ms: i64) -> String {
    if ms <= 0 {
        return "saved".to_string();
    }
    // Days since the Unix epoch → civil date (Howard Hinnant's algorithm).
    let days = ms / 86_400_000;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    match name {
        "search_history" => search_history(ctx, args),
        "read_conversation" => read_conversation(ctx, args),
        other => Err(format!("Recall doesn't handle '{other}'.")),
    }
}

fn search_history(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let query = args
        .get("query")
        .and_then(|q| q.as_str())
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or("missing 'query' argument")?;
    let limit = args
        .get("limit")
        .and_then(|l| l.as_u64())
        .map(|l| l as usize)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);

    let mut hits = ctx.db.search_messages_fts(query, limit).map_err(|e| e.to_string())?;
    hits.extend(ctx.db.search_memory_fts(query, limit).map_err(|e| e.to_string())?);
    hits.truncate(limit);

    // The user sees what was searched and what came back, with provenance.
    ctx.sink.emit(AgentEvent::Recall {
        id: ctx.call_id.to_string(),
        matches: hits.clone(),
    });
    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "recall", &format!("searched history: {query}"));

    if hits.is_empty() {
        return Ok(format!("No matches for '{query}' in past conversations or saved memories."));
    }

    let mut out = String::new();
    for (i, h) in hits.iter().enumerate() {
        out.push_str(&format!(
            "{}. [{} · {} · \"{}\"] {}\n",
            i + 1,
            h.source,
            as_date(h.created_at),
            h.title,
            clip(&h.snippet, 200)
        ));
    }
    out.push_str("Use read_conversation(conversation_id) for more.");
    Ok(truncate(out, SEARCH_OUTPUT_CAP))
}

fn read_conversation(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let id = args
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or("missing 'conversation_id' argument")?;

    let messages = ctx.db.list_messages_window(id, WINDOW_TURNS).map_err(|e| e.to_string())?;
    if messages.is_empty() {
        return Err("No conversation with that id, or it has no messages.".into());
    }

    let body = messages
        .iter()
        .map(|m| format!("{}: {}", m.role, clip(&m.content, WINDOW_MESSAGE_CAP)))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(truncate(body, READ_OUTPUT_CAP))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_dates_from_epoch_ms() {
        assert_eq!(as_date(0), "saved", "memory entries carry no timestamp");
        assert_eq!(as_date(1_752_624_000_000), "2025-07-16");
        assert_eq!(as_date(1_000), "1970-01-01");
    }

    #[test]
    fn clips_on_char_boundaries() {
        assert_eq!(clip("hello", 10), "hello");
        assert_eq!(clip("hello", 3), "hel…");
        // Multi-byte input must not panic or split a character.
        assert_eq!(clip("übermäßig", 3), "übe…");
        assert!(truncate("ü".repeat(100), 11).ends_with("(truncated)"));
    }
}
