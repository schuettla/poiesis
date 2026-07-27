//! Undo trash for file operations inside the working folder.
//!
//! The agent writes to the user's real disk — there is no sandbox to throw away.
//! So before any operation that destroys bytes, we copy the current contents to
//! `<app_data>/trash/<uuid>` and record a `file_trash` row. That row is what the
//! Workbench's "Recent changes" strip lists, and what `undo` reverses.
//!
//! A create records a row with no blob: undoing it means deleting the file that
//! wasn't there before. This is what makes "Full access" a defensible setting
//! rather than a leap of faith.

use std::path::{Path, PathBuf};

use crate::db::{Db, TrashEntry};

/// Blobs older than this are pruned at startup. Long enough to catch "wait, what
/// did it do yesterday?", short enough not to grow without bound.
const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;

fn trash_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("trash")
}

/// Snapshot `path`'s current bytes, if it exists, and record the operation.
/// Returns the trash entry so callers can surface an undo affordance.
///
/// Failing to snapshot is not fatal — we would rather complete the user's
/// request without an undo than refuse the work — but it is logged.
pub fn record(
    db: &Db,
    data_dir: &Path,
    conversation_id: &str,
    op: &str,
    path: &Path,
    prev_path: Option<&Path>,
) -> Option<TrashEntry> {
    let blob = if path.is_file() {
        let dir = trash_dir(data_dir);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("trash: couldn't create {}: {e}", dir.display());
            return None;
        }
        let blob = dir.join(uuid::Uuid::new_v4().to_string());
        if let Err(e) = std::fs::copy(path, &blob) {
            eprintln!("trash: couldn't snapshot {}: {e}", path.display());
            return None;
        }
        Some(blob)
    } else {
        // Either brand new, or a directory (whose removal we don't snapshot —
        // recursive delete asks for confirmation at every trust level).
        None
    };

    db.add_trash_entry(
        conversation_id,
        op,
        &path.to_string_lossy(),
        prev_path.map(|p| p.to_string_lossy().to_string()).as_deref(),
        blob.as_ref().map(|b| b.to_string_lossy().to_string()).as_deref(),
    )
    .map_err(|e| eprintln!("trash: couldn't record {op} on {}: {e}", path.display()))
    .ok()
}

/// Reverse one recorded operation, putting the disk back the way it was.
pub fn undo(db: &Db, entry: &TrashEntry) -> Result<(), String> {
    if entry.undone {
        return Err("that change was already undone".to_string());
    }
    let path = PathBuf::from(&entry.path);

    match (&entry.blob_path, &entry.prev_path) {
        // A move: put it back where it came from.
        (_, Some(prev)) if entry.op == "move" => {
            let prev = PathBuf::from(prev);
            if let Some(parent) = prev.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::rename(&path, &prev)
                .map_err(|e| format!("couldn't move {} back: {e}", path.display()))?;
        }
        // Bytes existed before: restore them.
        (Some(blob), _) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::copy(blob, &path)
                .map_err(|e| format!("couldn't restore {}: {e}", path.display()))?;
        }
        // Nothing existed before: the undo is a removal.
        (None, _) => {
            if path.is_dir() {
                std::fs::remove_dir_all(&path)
                    .map_err(|e| format!("couldn't remove {}: {e}", path.display()))?;
            } else if path.exists() {
                std::fs::remove_file(&path)
                    .map_err(|e| format!("couldn't remove {}: {e}", path.display()))?;
            }
        }
    }

    db.mark_trash_undone(&entry.id).map_err(|e| e.to_string())?;
    Ok(())
}

/// Drop blobs (and their rows) past the retention window. Called once at startup.
pub fn prune(db: &Db, data_dir: &Path) {
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
        - RETENTION_MS;
    let Ok(expired) = db.expired_trash(cutoff) else {
        return;
    };
    for entry in expired {
        if let Some(blob) = &entry.blob_path {
            std::fs::remove_file(blob).ok();
        }
        db.delete_trash_entry(&entry.id).ok();
    }
    // Sweep any orphaned blobs a crash may have left behind.
    if let Ok(read) = std::fs::read_dir(trash_dir(data_dir)) {
        for e in read.flatten() {
            let stale = e
                .metadata()
                .and_then(|m| m.modified())
                .map(|m| m.elapsed().map(|d| d.as_millis() as i64 > RETENTION_MS).unwrap_or(false))
                .unwrap_or(false);
            if stale {
                std::fs::remove_file(e.path()).ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poiesis_trash_{name}_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn overwrite_then_undo_restores_exact_bytes() {
        let dir = scratch("overwrite");
        let db = Db::open_in_memory().unwrap();
        let file = dir.join("notes.md");
        std::fs::write(&file, "original contents").unwrap();

        let entry = record(&db, &dir, "conv1", "write", &file, None).unwrap();
        std::fs::write(&file, "clobbered").unwrap();

        undo(&db, &entry).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original contents");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn undoing_a_creation_removes_the_file() {
        let dir = scratch("create");
        let db = Db::open_in_memory().unwrap();
        let file = dir.join("new.txt");

        let entry = record(&db, &dir, "conv1", "write", &file, None).unwrap();
        assert!(entry.blob_path.is_none(), "nothing to snapshot for a new file");
        std::fs::write(&file, "fresh").unwrap();

        undo(&db, &entry).unwrap();
        assert!(!file.exists(), "undoing a creation deletes the file");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn undoing_a_delete_restores_it() {
        let dir = scratch("delete");
        let db = Db::open_in_memory().unwrap();
        let file = dir.join("gone.txt");
        std::fs::write(&file, "still here").unwrap();

        let entry = record(&db, &dir, "conv1", "delete", &file, None).unwrap();
        std::fs::remove_file(&file).unwrap();

        undo(&db, &entry).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "still here");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn undoing_a_move_returns_it_to_the_original_location() {
        let dir = scratch("move");
        let db = Db::open_in_memory().unwrap();
        let from = dir.join("a.txt");
        let to = dir.join("sub").join("b.txt");
        std::fs::write(&from, "payload").unwrap();
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();

        let entry = record(&db, &dir, "conv1", "move", &to, Some(&from)).unwrap();
        std::fs::rename(&from, &to).unwrap();

        undo(&db, &entry).unwrap();
        assert!(from.exists(), "the file is back where it started");
        assert!(!to.exists());
        assert_eq!(std::fs::read_to_string(&from).unwrap(), "payload");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn undo_is_not_repeatable() {
        let dir = scratch("twice");
        let db = Db::open_in_memory().unwrap();
        let file = dir.join("x.txt");
        std::fs::write(&file, "v1").unwrap();

        let entry = record(&db, &dir, "conv1", "write", &file, None).unwrap();
        std::fs::write(&file, "v2").unwrap();
        undo(&db, &entry).unwrap();

        let reloaded = db.get_trash_entry(&entry.id).unwrap().unwrap();
        assert!(reloaded.undone);
        assert!(undo(&db, &reloaded).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
