//! Permissions model (PRD §6): least privilege with explicit, legible consent.
//!
//! Folders the user has granted are persisted (DB whitelist). When the agent
//! needs access it hasn't been granted, it raises an interactive request that
//! the UI answers (Allow once / Allow for this chat / Deny, §5.4.4); the loop
//! awaits that decision. Path-traversal and symlink escapes are blocked here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Read,
    ReadWrite,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Read => "read",
            Mode::ReadWrite => "read-write",
        }
    }
    pub fn satisfies(&self, required: Mode) -> bool {
        // Read-write covers read; read does not cover write.
        matches!((self, required), (Mode::ReadWrite, _) | (Mode::Read, Mode::Read))
    }
}

/// The user's answer to a permission request (§5.4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Deny,
    Once,
    Chat,
    Forever,
}

/// A pending request surfaced to the UI side panel.
#[derive(Debug, Clone, Serialize)]
pub struct PermissionRequest {
    pub id: String,
    /// One-sentence plain-language explanation (§5.4.4).
    pub summary: String,
    pub path: String,
    pub mode: Mode,
}

#[derive(Default)]
pub struct PermissionManager {
    pending: Mutex<HashMap<String, oneshot::Sender<Decision>>>,
    /// Per-conversation "allow for this chat" grants.
    chat_grants: Mutex<HashMap<String, Vec<(PathBuf, Mode)>>>,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a pending request and return its receiver; the agent loop awaits
    /// this until the UI resolves it (or the channel drops → treated as Deny).
    pub fn open_request(&self, id: &str) -> oneshot::Receiver<Decision> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.to_string(), tx);
        rx
    }

    /// Resolve a pending request with the user's decision.
    pub fn resolve(&self, id: &str, decision: Decision) {
        if let Some(tx) = self.pending.lock().unwrap().remove(id) {
            let _ = tx.send(decision);
        }
    }

    pub fn add_chat_grant(&self, conversation_id: &str, root: PathBuf, mode: Mode) {
        self.chat_grants
            .lock()
            .unwrap()
            .entry(conversation_id.to_string())
            .or_default()
            .push((root, mode));
    }

    /// Whether a per-chat grant already covers this path+mode.
    pub fn chat_allows(&self, conversation_id: &str, path: &Path, required: Mode) -> bool {
        let grants = self.chat_grants.lock().unwrap();
        grants
            .get(conversation_id)
            .map(|gs| gs.iter().any(|(root, mode)| mode.satisfies(required) && path_within_root(path, root)))
            .unwrap_or(false)
    }

    /// Clear a conversation's chat-scoped grants (e.g. on delete).
    #[allow(dead_code)]
    pub fn clear_chat(&self, conversation_id: &str) {
        self.chat_grants.lock().unwrap().remove(conversation_id);
    }
}

/// Resolve `requested` to a canonical path and test that it lives within
/// `root` — collapsing `..` and following symlinks so neither can escape (§6.1).
/// Works for not-yet-existing files by canonicalizing the nearest existing
/// ancestor and re-appending the remaining tail.
pub fn path_within_root(requested: &Path, root: &Path) -> bool {
    let Ok(root_canon) = root.canonicalize() else {
        return false;
    };
    let resolved = canonicalize_lenient(requested);
    resolved.starts_with(&root_canon)
}

/// Like `canonicalize`, but tolerant of trailing path components that don't
/// exist yet (needed to validate a write target before creating it).
pub fn canonicalize_lenient(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    let mut tail = Vec::new();
    let mut cur = path.to_path_buf();
    while let Some(parent) = cur.parent().map(|p| p.to_path_buf()) {
        if let Some(name) = cur.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(c) = parent.canonicalize() {
            let mut result = c;
            for part in tail.iter().rev() {
                result.push(part);
            }
            return result;
        }
        if parent.as_os_str().is_empty() {
            break;
        }
        cur = parent;
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_traversal_escape() {
        let root = std::env::temp_dir();
        let inside = root.join("nexus_test_file.txt");
        assert!(path_within_root(&inside, &root));
        let escape = root.join("..").join("somewhere_else.txt");
        assert!(!path_within_root(&escape, &root));
    }
}
