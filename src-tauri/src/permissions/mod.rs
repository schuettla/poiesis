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

/// How much the agent may do inside the conversation's attached working folder.
/// Reads are always silent inside it; this governs everything that changes bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trust {
    /// Look but don't touch. Writes and deletes are refused outright.
    ReadOnly,
    /// The default: every write, edit, move and delete raises one prompt.
    Confirm,
    /// Writes and edits go through silently. Deletes and moves still prompt —
    /// losing a file should never be something that just happens.
    Auto,
}

impl Trust {
    pub fn as_str(&self) -> &'static str {
        match self {
            Trust::ReadOnly => "read-only",
            Trust::Confirm => "confirm",
            Trust::Auto => "auto",
        }
    }

    /// Parse the stored string, defaulting to the safe middle setting.
    pub fn parse(s: &str) -> Trust {
        match s {
            "read-only" => Trust::ReadOnly,
            "auto" => Trust::Auto,
            _ => Trust::Confirm,
        }
    }

    /// Plain-language label, used in the system prompt so the model can explain
    /// its own constraints to the user.
    pub fn label(&self) -> &'static str {
        match self {
            Trust::ReadOnly => "read only",
            Trust::Confirm => "ask first",
            Trust::Auto => "full access",
        }
    }
}

/// What a single file operation does to the disk — the axis trust is graded on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impact {
    /// Reads nothing away: read, list, search.
    Read,
    /// Creates or changes bytes: write, edit, mkdir.
    Modify,
    /// Removes or relocates: delete, move.
    Destroy,
}

/// Whether an operation of this impact needs to stop and ask, given the trust
/// level on the folder it targets. `Err` means refuse outright.
pub fn gate(trust: Trust, impact: Impact) -> Result<bool, &'static str> {
    match (trust, impact) {
        (_, Impact::Read) => Ok(false),
        (Trust::ReadOnly, _) => Err(
            "this folder is attached read-only — ask the user to change access to \
             \"Ask first\" or \"Full\" in the Workbench panel if they want changes made",
        ),
        (Trust::Auto, Impact::Modify) => Ok(false),
        // Confirm-anything, and destructive work at any trust level, asks.
        _ => Ok(true),
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
    /// A diff or content excerpt to review, for in-folder operation confirms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// True when this is an operation confirm inside the attached working folder
    /// rather than a request to widen scope. The UI shows Allow / Deny / Don't
    /// ask again in this folder instead of the four-way scope choice.
    pub in_folder: bool,
}

impl PermissionRequest {
    /// A scope request: "may I reach into this folder at all?"
    pub fn scope(id: String, summary: String, path: String, mode: Mode) -> Self {
        Self { id, summary, path, mode, diff: None, in_folder: false }
    }

    /// An operation confirm inside the already-attached folder.
    pub fn operation(id: String, summary: String, path: String, mode: Mode, diff: Option<String>) -> Self {
        Self { id, summary, path, mode, diff, in_folder: true }
    }
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
    if root.canonicalize().is_err() {
        // A grant pointing at a folder that no longer exists fails closed.
        return false;
    }
    // Both sides must go through the *same* normalisation. Mixing raw
    // `canonicalize` (which keeps Windows' `\\?\` verbatim prefix) with
    // `canonicalize_lenient` (which strips it) silently fails every check.
    let root_canon = canonicalize_lenient(root);
    let resolved = canonicalize_lenient(requested);
    resolved.starts_with(&root_canon)
}

/// Strip Windows' `\\?\` verbatim prefix that `canonicalize` adds.
///
/// Without this, every canonical path both *looks* wrong in the UI
/// (`\\?\C:\Users\…`) and fails to string-match paths stored before this
/// change. Applied inside `canonicalize_lenient` so both sides of every
/// comparison get the same treatment.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        // Only a drive-letter path is safe to unwrap; anything else (device
        // namespace) stays verbatim rather than being silently mangled.
        let mut chars = rest.chars();
        if matches!((chars.next(), chars.next()), (Some(c), Some(':')) if c.is_ascii_alphabetic()) {
            return PathBuf::from(rest);
        }
    }
    path
}

/// Like `canonicalize`, but tolerant of trailing path components that don't
/// exist yet (needed to validate a write target before creating it).
pub fn canonicalize_lenient(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return strip_verbatim(c);
    }
    let mut tail = Vec::new();
    let mut cur = path.to_path_buf();
    while let Some(parent) = cur.parent().map(|p| p.to_path_buf()) {
        if let Some(name) = cur.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(c) = parent.canonicalize() {
            // Strip here too, not just on the fast path — otherwise a
            // not-yet-existing target normalises differently from an existing
            // one and never matches the root it lives under.
            let mut result = strip_verbatim(c);
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

/// Folders that must never become a working folder, however the user asks.
/// A drive root would put every file on the machine one relative path away, and
/// the system directories are never what someone means by "the folder I'm
/// working in". `app_data` is passed in so the agent can't be pointed at its own
/// database and memory store.
pub fn refuse_as_working_folder(path: &Path, app_data: Option<&Path>) -> Option<String> {
    let canon = canonicalize_lenient(path);

    if !canon.is_dir() {
        return Some("that isn't a folder".to_string());
    }

    // A drive root ("C:\", "\\server\share\", "/") has no parent.
    if canon.parent().is_none() {
        return Some("a whole drive is too broad — pick a project folder inside it".to_string());
    }

    if let Some(app_data) = app_data {
        if path_within_root(&canon, app_data) || canonicalize_lenient(app_data).starts_with(&canon) {
            return Some("that's Poiesis's own data folder".to_string());
        }
    }

    let lower = canon.to_string_lossy().to_lowercase();
    // Trailing separator so "C:\Windows" matches but "C:\WindowsApps" doesn't.
    let sep = std::path::MAIN_SEPARATOR;
    let probe = format!("{lower}{sep}");

    let matches = |var: &str, recursive: bool| -> bool {
        let Ok(v) = std::env::var(var) else { return false };
        if v.is_empty() {
            return false;
        }
        let v = canonicalize_lenient(Path::new(&v)).to_string_lossy().to_lowercase();
        lower == v || (recursive && probe.starts_with(&format!("{v}{sep}")))
    };

    // Nothing under these is ever someone's project.
    for var in ["WINDIR", "SYSTEMROOT", "ProgramFiles", "ProgramFiles(x86)"] {
        if matches(var, true) {
            return Some("that's a system folder".to_string());
        }
    }
    // These roots are far too broad to attach wholesale, but folders *inside*
    // them are ordinary — %LOCALAPPDATA%\Temp is where scratch work lives.
    for var in ["APPDATA", "LOCALAPPDATA", "ProgramData", "USERPROFILE"] {
        if matches(var, false) {
            return Some("that folder is too broad — pick a project folder inside it".to_string());
        }
    }
    for unix_root in ["/etc", "/usr", "/bin", "/sbin", "/sys", "/proc", "/var"] {
        if lower == unix_root || probe.starts_with(&format!("{unix_root}/")) {
            return Some("that's a system folder".to_string());
        }
    }
    None
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

    #[test]
    fn a_root_contains_itself_however_it_is_spelled() {
        // The Workbench asks to list the attached folder itself, so the root
        // must pass its own check — and it must pass whether the caller hands
        // over the raw path or an already-canonicalised one.
        let root = std::env::temp_dir().join(format!("poiesis_root_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let canon = canonicalize_lenient(&root);

        assert!(path_within_root(&root, &root), "raw against raw");
        assert!(path_within_root(&canon, &root), "canonical against raw");
        assert!(path_within_root(&root, &canon), "raw against canonical");
        assert!(path_within_root(&canon, &canon), "canonical against canonical");
        assert!(path_within_root(&canon.join("sub").join("f.txt"), &canon));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn canonical_paths_stay_human_readable() {
        let dir = std::env::temp_dir();
        let canon = canonicalize_lenient(&dir);
        let shown = canon.to_string_lossy();
        assert!(
            !shown.starts_with(r"\\?\"),
            "the verbatim prefix would show up in the UI and break path matching: {shown}"
        );
        // Round-tripping an already-clean path must be a no-op.
        assert_eq!(canonicalize_lenient(&canon), canon);
    }

    #[test]
    fn reads_never_prompt_but_read_only_refuses_changes() {
        for trust in [Trust::ReadOnly, Trust::Confirm, Trust::Auto] {
            assert_eq!(gate(trust, Impact::Read), Ok(false), "reads are silent at {trust:?}");
        }
        assert!(gate(Trust::ReadOnly, Impact::Modify).is_err());
        assert!(gate(Trust::ReadOnly, Impact::Destroy).is_err());
    }

    #[test]
    fn confirm_asks_for_changes_and_auto_still_guards_deletes() {
        assert_eq!(gate(Trust::Confirm, Impact::Modify), Ok(true));
        assert_eq!(gate(Trust::Confirm, Impact::Destroy), Ok(true));
        assert_eq!(gate(Trust::Auto, Impact::Modify), Ok(false));
        assert_eq!(
            gate(Trust::Auto, Impact::Destroy),
            Ok(true),
            "deleting is never silent, whatever the trust level"
        );
    }

    #[test]
    fn trust_round_trips_and_defaults_safely() {
        for t in [Trust::ReadOnly, Trust::Confirm, Trust::Auto] {
            assert_eq!(Trust::parse(t.as_str()), t);
        }
        assert_eq!(Trust::parse("nonsense"), Trust::Confirm);
        assert_eq!(Trust::parse(""), Trust::Confirm);
    }

    #[test]
    fn refuses_drive_roots_and_system_folders() {
        let mut root = std::env::temp_dir();
        while let Some(parent) = root.parent().map(|p| p.to_path_buf()) {
            root = parent;
        }
        assert!(refuse_as_working_folder(&root, None).is_some(), "drive root refused");

        if let Ok(windir) = std::env::var("WINDIR") {
            assert!(refuse_as_working_folder(Path::new(&windir), None).is_some());
            // …and anything inside it.
            assert!(refuse_as_working_folder(&Path::new(&windir).join("System32"), None).is_some());
        }
        // The user's home is too broad wholesale, but folders inside it are fine.
        if let Ok(home) = std::env::var("USERPROFILE") {
            assert!(refuse_as_working_folder(Path::new(&home), None).is_some());
        }

        let ok = std::env::temp_dir().join("poiesis_folder_ok_test");
        std::fs::create_dir_all(&ok).unwrap();
        assert!(refuse_as_working_folder(&ok, None).is_none(), "an ordinary folder is fine");

        // The app-data dir and anything under it is off limits.
        assert!(refuse_as_working_folder(&ok, Some(&ok)).is_some());
        std::fs::remove_dir_all(&ok).ok();
    }

    #[test]
    fn refuses_a_file_as_a_working_folder() {
        let f = std::env::temp_dir().join("poiesis_not_a_folder.txt");
        std::fs::write(&f, "x").unwrap();
        assert!(refuse_as_working_folder(&f, None).is_some());
        std::fs::remove_file(&f).ok();
    }
}
