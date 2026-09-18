//! `COD-4`/`COD-12`: what one run has read, changed and checked.
//!
//! Two rules hang off this, both about the agent guessing:
//!
//! - **Read before edit.** An edit to a file the run never read, or one that
//!   changed on disk since it read it, is an edit against a guess. It is refused
//!   with a message that says exactly what to do instead.
//! - **Check after edit.** A run that changed code and never ran the project's
//!   check is asked once, before its answer, to run it or to say it did not.
//!
//! One per run, shared by every tool call in it, like `extra_read_roots`.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What a file looked like when it was last read: `(mtime ms, size)`.
pub type Stamp = (i64, u64);

#[derive(Default)]
struct Inner {
    reads: HashMap<PathBuf, Stamp>,
    /// Files this run changed, in the order of the ledger's clock.
    edited: BTreeSet<PathBuf>,
    /// The ledger's clock value at the last edit and at the last check.
    last_edit: u64,
    last_check: u64,
    clock: u64,
    nudged: bool,
}

#[derive(Default)]
pub struct Ledger(Mutex<Inner>);

/// The file's stamp as it is now. `None` for a file that does not exist.
pub fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some((mtime, meta.len()))
}

impl Ledger {
    /// `read_file` saw this file as it is now.
    pub fn record_read(&self, path: &Path) {
        if let Some(stamp) = stamp_of(path) {
            self.0.lock().unwrap().reads.insert(path.to_path_buf(), stamp);
        }
    }

    /// `COD-4`: may this run change `path`? A file that does not exist yet
    /// needs no read — there is nothing to have guessed about.
    pub fn check_before_edit(&self, path: &Path, shown: &str) -> Result<(), String> {
        let Some(now) = stamp_of(path) else { return Ok(()) };
        match self.0.lock().unwrap().reads.get(path) {
            None => Err(format!(
                "You have not read {shown} in this run, so this change would be made against a guess. \
                 Call read_file on it first, then make the change against what is actually there."
            )),
            Some(seen) if *seen != now => Err(format!(
                "{shown} changed on disk after you read it (something else wrote to it). \
                 Call read_file on it again, then make the change against the new contents."
            )),
            Some(_) => Ok(()),
        }
    }

    /// This run wrote `path`. Its own write is not a change behind its back,
    /// so the stamp moves with it and a second edit needs no second read.
    pub fn record_edit(&self, path: &Path, in_project: bool) {
        let mut inner = self.0.lock().unwrap();
        match stamp_of(path) {
            Some(stamp) => {
                inner.reads.insert(path.to_path_buf(), stamp);
            }
            None => {
                inner.reads.remove(path);
            }
        }
        if in_project {
            inner.clock += 1;
            inner.last_edit = inner.clock;
            inner.edited.insert(path.to_path_buf());
        }
    }

    /// A check-shaped task ran (`COD-12`). Whether it passed is the model's to
    /// read; the nudge is only about whether it looked.
    pub fn record_check(&self) {
        let mut inner = self.0.lock().unwrap();
        inner.clock += 1;
        inner.last_check = inner.clock;
    }

    /// `COD-12`: files changed since the last check, once per run. `Some(n)`
    /// means ask now; every later call answers `None`.
    pub fn take_unverified(&self) -> Option<usize> {
        let mut inner = self.0.lock().unwrap();
        if inner.nudged || inner.edited.is_empty() || inner.last_check > inner.last_edit {
            return None;
        }
        inner.nudged = true;
        Some(inner.edited.len())
    }

    pub fn edited_count(&self) -> usize {
        self.0.lock().unwrap().edited.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_file(body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("poiesis_ledger_{}.txt", uuid::Uuid::new_v4().simple()));
        std::fs::write(&p, body).unwrap();
        p
    }

    /// `COD-4-T`: refuses an unread file; refuses a moved mtime; succeeds after
    /// a re-read.
    #[test]
    fn an_edit_needs_a_current_read() {
        let ledger = Ledger::default();
        let f = scratch_file("one");

        let err = ledger.check_before_edit(&f, "a.txt").unwrap_err();
        assert!(err.contains("have not read a.txt"), "{err}");

        ledger.record_read(&f);
        assert!(ledger.check_before_edit(&f, "a.txt").is_ok());

        // Something else writes it: a different size is a different stamp
        // even on a filesystem whose clock is too coarse to move the mtime.
        std::fs::write(&f, "something longer").unwrap();
        let err = ledger.check_before_edit(&f, "a.txt").unwrap_err();
        assert!(err.contains("changed on disk after you read it"), "{err}");

        ledger.record_read(&f);
        assert!(ledger.check_before_edit(&f, "a.txt").is_ok());
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn a_new_file_needs_no_read_and_the_runs_own_write_needs_no_second_one() {
        let ledger = Ledger::default();
        let missing = std::env::temp_dir().join(format!("poiesis_ledger_new_{}.txt", uuid::Uuid::new_v4().simple()));
        assert!(ledger.check_before_edit(&missing, "new.txt").is_ok());

        std::fs::write(&missing, "created").unwrap();
        ledger.record_edit(&missing, true);
        std::fs::write(&missing, "created and grown").unwrap();
        // That second write was not recorded, so it is a change behind its back.
        assert!(ledger.check_before_edit(&missing, "new.txt").is_err());
        ledger.record_edit(&missing, true);
        assert!(ledger.check_before_edit(&missing, "new.txt").is_ok());
        std::fs::remove_file(&missing).ok();
    }

    /// `COD-12-T`: once, only when files were edited and no check ran after.
    #[test]
    fn the_unverified_note_fires_once_and_only_after_unchecked_edits() {
        let ledger = Ledger::default();
        assert_eq!(ledger.take_unverified(), None, "nothing edited");

        let f = scratch_file("x");
        ledger.record_check();
        ledger.record_edit(&f, true);
        assert_eq!(ledger.take_unverified(), Some(1), "a check before the edit does not count");
        assert_eq!(ledger.take_unverified(), None, "once per run");

        let checked = Ledger::default();
        checked.record_edit(&f, true);
        checked.record_check();
        assert_eq!(checked.take_unverified(), None, "checked after editing");

        let outside = Ledger::default();
        outside.record_edit(&f, false);
        assert_eq!(outside.take_unverified(), None, "a file outside the project is not project code");
        std::fs::remove_file(&f).ok();
    }
}
