//! `COD-11`/`PRJ-UI-3`: what the agent changed on disk, as patches.
//!
//! Built from the undo snapshots `trash.rs` takes before every write, never
//! from `git diff`. That departs from the plan's text, deliberately: a work
//! tree's diff against `HEAD` also holds whatever the user had not committed
//! before the run started, and showing that as the agent's patch — with an
//! Undo beside it — would be the most dangerous screen in the app. The
//! snapshots hold exactly the bytes the agent replaced, so a git project and a
//! plain folder give the same answer for the same edits, and it is the right one.
//!
//! The same set feeds the model's `changes` tool, the Changes sub-view and the
//! dirty dot on a file tab, so the patch the agent reviews is the patch the
//! user reviews.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::diff::{self, Hunk};
use crate::db::{Db, TrashEntry};

/// A side past this size gets a line count but no patch.
const DIFF_BYTES_CAP: u64 = 1024 * 1024;
/// The `changes` tool's output cap: past this the model is told to narrow.
const TOOL_OUTPUT_CAP: usize = 48 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    /// Absolute, as recorded.
    pub path: String,
    /// Relative to the working folder when inside it, `/`-separated.
    pub display: String,
    /// `added` | `modified` | `deleted` | `moved`.
    pub status: String,
    /// For a move, where it came from (display form).
    pub from: Option<String>,
    pub added: usize,
    pub removed: usize,
    pub hunks: Vec<Hunk>,
    /// No patch was computed: one side is binary, or too large.
    pub binary: bool,
    pub too_large: bool,
    /// The undo rows behind this file, newest first — the order they undo in.
    pub entry_ids: Vec<String>,
    pub last_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeSet {
    pub files: Vec<FileChange>,
    pub added: usize,
    pub removed: usize,
    /// The time the set starts from.
    pub since: i64,
    /// True when every change in it was made by the conversation's latest run,
    /// which is what lets the header say "this run" honestly.
    pub this_run: bool,
}

fn display(path: &str, folder: Option<&str>) -> String {
    if let Some(root) = folder {
        let p = Path::new(path);
        if let Ok(rel) = p.strip_prefix(root) {
            let s = rel.to_string_lossy().replace('\\', "/");
            if !s.is_empty() {
                return s;
            }
        }
    }
    path.to_string()
}

fn read_side(path: &Path) -> Result<Option<String>, (bool, bool)> {
    let Ok(meta) = std::fs::metadata(path) else { return Ok(None) };
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > DIFF_BYTES_CAP {
        return Err((false, true));
    }
    if super::filesystem::looks_binary(path) {
        return Err((true, false));
    }
    std::fs::read_to_string(path).map(Some).map_err(|_| (true, false))
}

/// Replay a conversation's recorded changes since `since` into one entry per
/// file. A file changed and then changed back is not a change.
pub fn change_set(db: &Db, conversation_id: &str, since: i64) -> ChangeSet {
    let folder = db.conversation_folder(conversation_id).ok().and_then(|(f, _)| f);
    let entries: Vec<TrashEntry> = db
        .trash_since(conversation_id, since)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.undone)
        .collect();

    // Grouped by the path the file ends up at, in first-seen order.
    let mut groups: BTreeMap<String, Vec<TrashEntry>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for e in entries {
        if !groups.contains_key(&e.path) {
            order.push(e.path.clone());
        }
        groups.entry(e.path.clone()).or_default().push(e);
    }

    let mut files = Vec::new();
    for path in order {
        let group = &groups[&path];
        let first = &group[0];
        let target = Path::new(&path);
        // A folder the agent made is not a patch.
        if target.is_dir() {
            continue;
        }
        let moved_from = group.iter().find(|e| e.op == "move").and_then(|e| e.prev_path.clone());
        let existed_before = first.blob_path.is_some() || first.op == "move";
        let before = match (&first.blob_path, &moved_from) {
            (Some(blob), _) => read_side(Path::new(blob)),
            (None, Some(from)) if first.op == "move" => {
                // The bytes moved rather than changed; compare against the
                // file at its new place as it was when it arrived.
                read_side(target)
            }
            _ => Ok(None),
        };
        let after = read_side(target);
        let exists_now = target.is_file();

        let status = match (existed_before, exists_now) {
            (_, _) if moved_from.is_some() && exists_now => "moved",
            (false, true) => "added",
            (true, false) | (false, false) => "deleted",
            (true, true) => "modified",
        };
        // Created and then deleted again inside the window: nothing to show.
        if !existed_before && !exists_now {
            continue;
        }

        let mut change = FileChange {
            display: display(&path, folder.as_deref()),
            path: path.clone(),
            status: status.to_string(),
            from: moved_from.as_deref().map(|f| display(f, folder.as_deref())),
            added: 0,
            removed: 0,
            hunks: Vec::new(),
            binary: false,
            too_large: false,
            entry_ids: group.iter().rev().map(|e| e.id.clone()).collect(),
            last_at: group.last().map(|e| e.created_at).unwrap_or(0),
        };
        match (before, after) {
            (Ok(b), Ok(a)) => {
                let (b, a) = (b.unwrap_or_default(), a.unwrap_or_default());
                if b == a && status == "modified" {
                    continue;
                }
                change.hunks = diff::hunks(&b, &a);
                let (added, removed) = diff::counts(&change.hunks);
                change.added = added;
                change.removed = removed;
            }
            (Err((binary, large)), _) | (_, Err((binary, large))) => {
                change.binary = binary;
                change.too_large = large;
            }
        }
        files.push(change);
    }

    let latest_run_start = db
        .last_logged_run(conversation_id)
        .ok()
        .flatten()
        .and_then(|(run_id, _)| db.run_started_at(&run_id).ok().flatten());
    let this_run = match latest_run_start {
        Some(start) => files.iter().all(|f| {
            groups[&f.path].iter().all(|e| e.created_at >= start)
        }),
        None => false,
    };
    ChangeSet {
        added: files.iter().map(|f| f.added).sum(),
        removed: files.iter().map(|f| f.removed).sum(),
        files,
        since,
        this_run,
    }
}

/// The setting that remembers "Keep all" for a conversation.
fn kept_key(conversation_id: &str) -> String {
    format!("changes.kept_at.{conversation_id}")
}

/// Where the Changes view starts: after the last "Keep all", or everything
/// still in the undo window.
pub fn view_since(db: &Db, conversation_id: &str) -> i64 {
    db.get_setting(&kept_key(conversation_id))
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
}

/// "Keep all": the changes stay on disk and leave the review list.
pub fn keep_all(db: &Db, conversation_id: &str, now_ms: i64) {
    let _ = db.set_setting(&kept_key(conversation_id), &now_ms.to_string());
}

/// Put one file back as it was before the window, newest change first.
pub fn undo_file(db: &Db, change: &FileChange) -> Result<(), String> {
    for id in &change.entry_ids {
        let Some(entry) = db.get_trash_entry(id).map_err(|e| e.to_string())? else { continue };
        if entry.undone {
            continue;
        }
        super::trash::undo(db, &entry)?;
    }
    Ok(())
}

/// The model's view: a summary line per file, then every patch.
pub fn render_for_model(set: &ChangeSet) -> String {
    if set.files.is_empty() {
        return "You have not changed any files in this run.".to_string();
    }
    let mut out = format!(
        "{} file{} changed, +{} -{}\n",
        set.files.len(),
        if set.files.len() == 1 { "" } else { "s" },
        set.added,
        set.removed
    );
    for f in &set.files {
        let from = f.from.as_deref().map(|x| format!(" (from {x})")).unwrap_or_default();
        out.push_str(&format!("- {} {}{from}, +{} -{}\n", f.status, f.display, f.added, f.removed));
    }
    for f in &set.files {
        out.push('\n');
        if f.binary || f.too_large {
            out.push_str(&format!(
                "{}: {} — no patch shown.\n",
                f.display,
                if f.binary { "binary" } else { "too large to diff" }
            ));
            continue;
        }
        out.push_str(&diff::unified(&f.display, f.status != "added", f.status != "deleted", &f.hunks));
        if out.len() > TOOL_OUTPUT_CAP {
            out.truncate(TOOL_OUTPUT_CAP);
            out.push_str("\n…(more patches not shown; read the files you need)");
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = crate::permissions::canonicalize_lenient(&std::env::temp_dir())
            .join(format!("poiesis_changes_{name}_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Do what `filesystem.rs` does for a write: snapshot, then write.
    fn write(db: &Db, data: &Path, conv: &str, path: &Path, body: &str) {
        let op = if path.exists() { "edit" } else { "write" };
        super::super::trash::record(db, data, conv, op, path, None);
        std::fs::write(path, body).unwrap();
    }

    fn apply_edits(git: bool) -> String {
        let db = Db::open_in_memory().unwrap();
        let root = scratch(if git { "git" } else { "plain" });
        let data = scratch("data");
        if git {
            std::fs::create_dir_all(root.join(".git")).unwrap();
            std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        }
        let conv = db.create_conversation("c", None, false).unwrap();
        db.set_conversation_folder(&conv.id, Some(&root.to_string_lossy())).unwrap();
        std::fs::write(root.join("keep.txt"), "one\ntwo\nthree\n").unwrap();

        write(&db, &data, &conv.id, &root.join("keep.txt"), "one\n2\nthree\n");
        write(&db, &data, &conv.id, &root.join("new.txt"), "fresh\n");
        let set = change_set(&db, &conv.id, 0);
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&data).ok();
        render_for_model(&set)
    }

    /// `COD-11-T`: a git work tree and a plain folder give the same patch for
    /// the same edits.
    #[test]
    fn a_git_project_and_a_plain_folder_give_identical_diffs() {
        let plain = apply_edits(false);
        assert_eq!(plain, apply_edits(true));
        assert!(plain.starts_with("2 files changed, +2 -1\n- modified keep.txt, +1 -1\n- added new.txt, +1 -0\n"), "{plain}");
        assert!(plain.contains("--- a/keep.txt\n+++ b/keep.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+2\n three\n"), "{plain}");
        assert!(plain.contains("--- /dev/null\n+++ b/new.txt\n"), "{plain}");
    }

    #[test]
    fn undoing_a_file_puts_back_the_bytes_from_before_the_first_change() {
        let db = Db::open_in_memory().unwrap();
        let root = scratch("undo");
        let data = scratch("undo_data");
        let conv = db.create_conversation("c", None, false).unwrap();
        db.set_conversation_folder(&conv.id, Some(&root.to_string_lossy())).unwrap();
        let f = root.join("a.txt");
        std::fs::write(&f, "v0").unwrap();
        write(&db, &data, &conv.id, &f, "v1");
        write(&db, &data, &conv.id, &f, "v2");

        let set = change_set(&db, &conv.id, 0);
        assert_eq!(set.files.len(), 1, "two edits to one file are one change");
        undo_file(&db, &set.files[0]).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "v0");
        assert!(change_set(&db, &conv.id, 0).files.is_empty(), "undone changes leave the set");
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&data).ok();
    }

    #[test]
    fn keep_all_moves_the_window_past_what_was_kept() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(view_since(&db, "c1"), 0);
        keep_all(&db, "c1", 1234);
        assert_eq!(view_since(&db, "c1"), 1234);
        assert_eq!(view_since(&db, "c2"), 0, "per conversation");
    }
}
