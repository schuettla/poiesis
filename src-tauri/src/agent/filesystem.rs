//! Built-in File System skill (TOOL-3, §6.1): read/list/write within folders the
//! user has granted, with path-traversal + symlink-escape protection.

use std::path::{Path, PathBuf};

use crate::db::Db;
use crate::permissions::{canonicalize_lenient, path_within_root, Decision, Mode, PermissionManager, PermissionRequest};

use super::run::AgentEventSink;

/// The OpenAI tool schemas advertised to the model for this skill.
pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a UTF-8 text file from the user's computer.",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "Absolute path to the file" } },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_directory",
                "description": "List the entries in a directory.",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "Absolute path to the directory" } },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Write (create or overwrite) a UTF-8 text file.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Absolute path to the file" },
                        "content": { "type": "string", "description": "The full file contents to write" }
                    },
                    "required": ["path", "content"]
                }
            }
        }
    ])
}

/// Is this a File System tool name?
pub fn handles(name: &str) -> bool {
    matches!(name, "read_file" | "list_directory" | "write_file")
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    let path = args.get("path").and_then(|p| p.as_str()).unwrap_or("?");
    let short = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    match name {
        "read_file" => ("read".into(), short.to_string()),
        "list_directory" => ("listed".into(), short.to_string()),
        "write_file" => ("wrote".into(), short.to_string()),
        other => (other.into(), short.to_string()),
    }
}

fn required_mode(name: &str) -> Mode {
    if name == "write_file" {
        Mode::ReadWrite
    } else {
        Mode::Read
    }
}

/// The folder a grant should cover for a given target path: its parent directory
/// (so granting one file opens its folder, matching the §5.4.4 copy).
fn grant_root(path: &Path) -> PathBuf {
    path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| path.to_path_buf())
}

/// Check existing grants; if none cover the path, raise an interactive request
/// and await the user's decision (§5.4.4). Returns Ok if access is allowed.
async fn ensure_access(
    db: &Db,
    perms: &PermissionManager,
    sink: &AgentEventSink,
    conversation_id: &str,
    path: &Path,
    mode: Mode,
) -> Result<(), String> {
    // Persisted whitelist?
    if let Ok(grants) = db.list_permissions() {
        for g in &grants {
            let gmode = if g.mode == "read-write" { Mode::ReadWrite } else { Mode::Read };
            if gmode.satisfies(mode) && path_within_root(path, Path::new(&g.path)) {
                return Ok(());
            }
        }
    }
    // Per-chat grant?
    if perms.chat_allows(conversation_id, path, mode) {
        return Ok(());
    }

    // Otherwise ask. Scope the grant to the folder.
    let root = grant_root(path);
    let id = format!("perm_{}", uuid::Uuid::new_v4());
    let verb = if mode == Mode::ReadWrite { "write files in" } else { "read files in" };
    let summary = format!(
        "Nexus wants to {verb} {} to answer this.",
        root.display()
    );
    let rx = perms.open_request(&id);
    sink.send_permission(PermissionRequest {
        id: id.clone(),
        summary,
        path: root.to_string_lossy().to_string(),
        mode,
    });

    match rx.await.unwrap_or(Decision::Deny) {
        Decision::Deny => Err("You declined access to that folder.".to_string()),
        Decision::Once => Ok(()),
        Decision::Chat => {
            perms.add_chat_grant(conversation_id, root, mode);
            Ok(())
        }
        Decision::Forever => {
            let _ = db.add_permission(&root.to_string_lossy(), mode.as_str());
            Ok(())
        }
    }
}

/// Execute a File System tool call. Returns the result text fed back to the model.
pub async fn execute(
    db: &Db,
    perms: &PermissionManager,
    sink: &AgentEventSink,
    conversation_id: &str,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let path_str = args
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or("missing 'path' argument")?;
    let path = canonicalize_lenient(Path::new(path_str));

    ensure_access(db, perms, sink, conversation_id, &path, required_mode(name)).await?;

    let result = match name {
        "read_file" => std::fs::read_to_string(&path)
            .map_err(|e| format!("couldn't read {}: {e}", path.display()))?,
        "list_directory" => {
            let mut entries = Vec::new();
            for e in std::fs::read_dir(&path).map_err(|e| format!("couldn't list {}: {e}", path.display()))? {
                if let Ok(e) = e {
                    let suffix = if e.path().is_dir() { "/" } else { "" };
                    entries.push(format!("{}{}", e.file_name().to_string_lossy(), suffix));
                }
            }
            entries.sort();
            entries.join("\n")
        }
        "write_file" => {
            let content = args.get("content").and_then(|c| c.as_str()).unwrap_or("");
            std::fs::write(&path, content)
                .map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
            format!("Wrote {} bytes to {}", content.len(), path.display())
        }
        other => return Err(format!("unknown file tool '{other}'")),
    };

    // Record the action in the visible activity log (§6.1).
    let (verb, _) = describe(name, args);
    let _ = db.log_activity(Some(conversation_id), "file", &format!("{verb} {}", path.display()));

    Ok(result)
}
