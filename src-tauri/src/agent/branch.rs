//! `WRK-6`: two working copies of a folder, and keeping one.
//!
//! "Try two ways" needs two futures at once, *including the files*: two helpers
//! each work in a copy of the same folder, and one copy's changes are kept. This
//! is built on Poiesis's own copies and the same undo snapshots every other write
//! uses (`trash.rs`), not on git, because Poiesis is not only a coding agent and
//! has to do this in any folder, a git work tree or not.
//!
//! What is here is only the part that touches the disk and needs no model: make a
//! copy, say what changed in it, write the kept copy's changes into the real
//! folder (each file snapshotted first, so Undo works as for any other change),
//! and keep a record of what both ways did so the way not kept can still be read
//! afterwards. The copies themselves are deleted as soon as the choice is made:
//! what is worth keeping of the other way is its patch, and that is in the record.
//!
//! Copies skip what a person never means by "this folder": version control data,
//! dependencies and build output. A folder too big to copy is refused with a plain
//! reason, and the run goes on with one way.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::diff;
use super::ledger::Ledger;
use crate::db::Db;

/// Folders a copy leaves out.
const SKIPPED: &[&str] = &[
    ".git", "node_modules", "target", "dist", "build", ".next", ".nuxt", ".venv", "venv", "__pycache__", ".cache",
    ".turbo", ".gradle", "Pods", ".terraform",
];
/// The most a copy is allowed to hold. Past this a folder is a project of its own
/// and two copies of it are not a cheap experiment.
const MAX_FILES: usize = 8_000;
const MAX_BYTES: u64 = 250 * 1024 * 1024;
/// A side bigger than this is compared by size and time, not read.
const READ_CAP: u64 = 1024 * 1024;

pub fn root(data_dir: &Path) -> PathBuf {
    data_dir.join("branches")
}

/// An id is a uuid and nothing else: it ends up in a path.
pub fn clean_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn workdir(data_dir: &Path, id: &str) -> PathBuf {
    root(data_dir).join(id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Copied {
    pub files: usize,
    pub bytes: u64,
}

/// Every file under `root` a copy cares about, as `/`-separated paths relative
/// to it, sorted. Symbolic links are neither followed nor listed: a copy must not
/// reach outside the folder it was made from.
fn files_under(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !SKIPPED.contains(&name.as_str()) {
                    stack.push(path);
                }
            } else if meta.is_file() {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    out.sort();
    out
}

/// Copy `src` into `dst` (made if need be). `Err` says why it was not done.
pub fn copy_folder(src: &Path, dst: &Path) -> Result<Copied, String> {
    if !src.is_dir() {
        return Err("The working folder is not there.".into());
    }
    let files = files_under(src);
    if files.len() > MAX_FILES {
        return Err(format!("The folder has too many files to copy ({} past {MAX_FILES}).", files.len()));
    }
    let mut bytes = 0u64;
    for rel in &files {
        bytes += std::fs::metadata(src.join(rel)).map(|m| m.len()).unwrap_or(0);
        if bytes > MAX_BYTES {
            return Err("The folder is too big to copy twice.".into());
        }
    }
    std::fs::create_dir_all(dst).map_err(|e| format!("I could not make a copy: {e}"))?;
    for rel in &files {
        let to = dst.join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("I could not make a copy: {e}"))?;
        }
        std::fs::copy(src.join(rel), &to).map_err(|e| format!("I could not copy {rel}: {e}"))?;
    }
    Ok(Copied { files: files.len(), bytes })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// Relative to the folder, `/`-separated.
    pub rel: String,
    pub kind: Kind,
}

fn same_file(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else { return false };
    if ma.len() != mb.len() {
        return false;
    }
    if ma.len() > READ_CAP {
        // Too big to read twice for this: the same size and the same time is the same.
        return ma.modified().ok() == mb.modified().ok();
    }
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// What `copy` has that `orig` does not, and the reverse, and what differs.
pub fn changes(orig: &Path, copy: &Path) -> Vec<Changed> {
    let was = files_under(orig);
    let now = files_under(copy);
    let mut out = Vec::new();
    for rel in &now {
        if was.binary_search(rel).is_err() {
            out.push(Changed { rel: rel.clone(), kind: Kind::Added });
        } else if !same_file(&orig.join(rel), &copy.join(rel)) {
            out.push(Changed { rel: rel.clone(), kind: Kind::Modified });
        }
    }
    for rel in &was {
        if now.binary_search(rel).is_err() {
            out.push(Changed { rel: rel.clone(), kind: Kind::Deleted });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// The changes as unified diffs, for a model to judge and a person to read. A
/// file that is binary or large says so instead of a patch.
pub fn patch(orig: &Path, copy: &Path, changes: &[Changed]) -> String {
    let mut out = String::new();
    for c in changes {
        let before = std::fs::read(orig.join(&c.rel)).ok();
        let after = std::fs::read(copy.join(&c.rel)).ok();
        let text = |b: &Option<Vec<u8>>| -> Option<String> {
            match b {
                None => Some(String::new()),
                Some(b) if b.len() as u64 > READ_CAP => None,
                Some(b) => String::from_utf8(b.clone()).ok(),
            }
        };
        match (text(&before), text(&after)) {
            (Some(b), Some(a)) => {
                let hunks = diff::hunks(&b, &a);
                out.push_str(&diff::unified(&c.rel, c.kind != Kind::Added, c.kind != Kind::Deleted, &hunks));
            }
            _ => out.push_str(&format!("{}: binary or too large, no patch shown.\n", c.rel)),
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// Write the kept copy's changes into the real folder. Every file is snapshotted
/// first, the way every other write is, so each change can be undone; the ledger
/// is told, so the run's own count of what it changed is true. Returns how many
/// files were written or removed.
pub fn apply(
    db: &Db,
    data_dir: &Path,
    conversation_id: &str,
    ledger: &Ledger,
    orig: &Path,
    copy: &Path,
    changes: &[Changed],
) -> usize {
    let mut done = 0;
    for c in changes {
        let to = orig.join(&c.rel);
        match c.kind {
            Kind::Added | Kind::Modified => {
                super::trash::record(db, data_dir, conversation_id, if c.kind == Kind::Added { "write" } else { "edit" }, &to, None);
                if let Some(parent) = to.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::copy(copy.join(&c.rel), &to).is_ok() {
                    ledger.record_edit(&to, true);
                    done += 1;
                }
            }
            Kind::Deleted => {
                super::trash::record(db, data_dir, conversation_id, "delete", &to, None);
                if std::fs::remove_file(&to).is_ok() {
                    ledger.record_edit(&to, true);
                    done += 1;
                }
            }
        }
    }
    done
}

// `TEAM-13`: a team's members each work in a copy at the same time, so a copy is kept
// against what the folder held when it was made (`Base`), and what it changed is told
// apart from what the folder changed meanwhile. Same copies, same `apply` as above.

/// What every file held when a copy was made: a hash, by path.
pub type Base = std::collections::BTreeMap<String, String>;

pub fn hash_of(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// The base of a copy that was just made: what is in it now.
pub fn base_of(root: &Path) -> Base {
    files_under(root).into_iter().filter_map(|rel| hash_of(&root.join(&rel)).map(|h| (rel, h))).collect()
}

/// What `copy` changed since it was made.
pub fn changes_since(base: &Base, copy: &Path) -> Vec<Changed> {
    let now = files_under(copy);
    let mut out = Vec::new();
    for rel in &now {
        match (base.get(rel), hash_of(&copy.join(rel))) {
            (None, _) => out.push(Changed { rel: rel.clone(), kind: Kind::Added }),
            (Some(was), Some(is)) if *was != is => out.push(Changed { rel: rel.clone(), kind: Kind::Modified }),
            _ => {}
        }
    }
    for rel in base.keys() {
        if now.binary_search(rel).is_err() {
            out.push(Changed { rel: rel.clone(), kind: Kind::Deleted });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// A copy's changes, told apart by what the folder has done to the same file since the copy
/// was made.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Split {
    /// The folder still has what the copy started from: safe to write.
    pub clean: Vec<Changed>,
    /// The folder already has what the copy has: nothing to write.
    pub same: Vec<Changed>,
    /// The folder changed the file too, differently: never overwritten.
    pub conflicts: Vec<Changed>,
}

pub fn split(base: &Base, main: &Path, copy: &Path, changes: Vec<Changed>) -> Split {
    let mut out = Split::default();
    for c in changes {
        let (was, main_now) = (base.get(&c.rel), hash_of(&main.join(&c.rel)));
        let copy_now = if c.kind == Kind::Deleted { None } else { hash_of(&copy.join(&c.rel)) };
        if main_now.as_ref() == was {
            out.clean.push(c);
        } else if main_now == copy_now {
            out.same.push(c);
        } else {
            out.conflicts.push(c);
        }
    }
    out
}

/// The files of `done` are now the same in the folder and the copy: the next merge of this copy
/// starts from there.
pub fn advance(base: &mut Base, copy: &Path, done: &[Changed]) {
    for c in done {
        match (c.kind, hash_of(&copy.join(&c.rel))) {
            (Kind::Deleted, _) | (_, None) => {
                base.remove(&c.rel);
            }
            (_, Some(h)) => {
                base.insert(c.rel.clone(), h);
            }
        }
    }
}

/// What both ways did, kept after the copies are gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub item: usize,
    pub text: String,
    /// `a` | `b`.
    pub kept: String,
    pub reason: String,
    /// What each helper said it did, `[a, b]`.
    pub ways: Vec<String>,
    /// What each changed, as a patch, `[a, b]`.
    pub patches: Vec<String>,
    /// How many files each changed, `[a, b]`.
    pub files: Vec<usize>,
    /// The helpers' own chats, `[a, b]`, so a transcript can be opened.
    pub children: Vec<String>,
}

pub fn save(data_dir: &Path, record: &Record) -> Result<(), String> {
    if !clean_id(&record.id) {
        return Err("bad id".into());
    }
    let dir = workdir(data_dir, &record.id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("record.json"), json).map_err(|e| e.to_string())
}

pub fn load(data_dir: &Path, id: &str) -> Option<Record> {
    if !clean_id(id) {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(workdir(data_dir, id).join("record.json")).ok()?).ok()
}

/// Both copies are gone once the choice is made; the record stays.
pub fn discard_copies(data_dir: &Path, id: &str) {
    if !clean_id(id) {
        return;
    }
    for side in ["a", "b"] {
        let _ = std::fs::remove_dir_all(workdir(data_dir, id).join(side));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = crate::permissions::canonicalize_lenient(&std::env::temp_dir())
            .join(format!("poiesis_branch_{name}_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn project(name: &str) -> PathBuf {
        let root = scratch(name);
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(root.join("node_modules/dep")).unwrap();
        std::fs::write(root.join("node_modules/dep/x.js"), "x").unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref").unwrap();
        root
    }

    #[test]
    fn a_copy_holds_the_folder_without_what_nobody_means_by_it() {
        let src = project("copy_src");
        let dst = scratch("copy_dst").join("a");
        let got = copy_folder(&src, &dst).unwrap();
        assert_eq!(got.files, 2);
        assert!(dst.join("src/lib.rs").is_file());
        assert!(!dst.join("node_modules").exists() && !dst.join(".git").exists());
        assert!(changes(&src, &dst).is_empty(), "a fresh copy differs in nothing");
    }

    #[test]
    fn what_a_copy_changed_is_listed_and_patched() {
        let src = project("diff_src");
        let dst = scratch("diff_dst").join("a");
        copy_folder(&src, &dst).unwrap();
        std::fs::write(dst.join("a.txt"), "one\n2\nthree\n").unwrap();
        std::fs::write(dst.join("new.txt"), "fresh\n").unwrap();
        std::fs::remove_file(dst.join("src/lib.rs")).unwrap();

        let c = changes(&src, &dst);
        assert_eq!(
            c,
            vec![
                Changed { rel: "a.txt".into(), kind: Kind::Modified },
                Changed { rel: "new.txt".into(), kind: Kind::Added },
                Changed { rel: "src/lib.rs".into(), kind: Kind::Deleted },
            ]
        );
        let p = patch(&src, &dst, &c);
        assert!(p.contains("--- a/a.txt\n+++ b/a.txt") && p.contains("-two\n+2"), "{p}");
        assert!(p.contains("--- /dev/null\n+++ b/new.txt"), "{p}");
        assert!(p.contains("--- a/src/lib.rs\n+++ /dev/null"), "{p}");
    }

    #[test]
    fn keeping_a_copy_writes_its_changes_to_the_folder_and_each_can_be_undone() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("c", None, false).unwrap();
        let data = scratch("apply_data");
        let src = project("apply_src");
        db.set_conversation_folder(&conv.id, Some(&src.to_string_lossy())).unwrap();
        let dst = scratch("apply_dst").join("b");
        copy_folder(&src, &dst).unwrap();
        std::fs::write(dst.join("a.txt"), "one\n2\nthree\n").unwrap();
        std::fs::write(dst.join("new.txt"), "fresh\n").unwrap();
        std::fs::remove_file(dst.join("src/lib.rs")).unwrap();

        let ledger = Ledger::default();
        let c = changes(&src, &dst);
        assert_eq!(apply(&db, &data, &conv.id, &ledger, &src, &dst, &c), 3);
        assert_eq!(std::fs::read_to_string(src.join("a.txt")).unwrap(), "one\n2\nthree\n");
        assert_eq!(std::fs::read_to_string(src.join("new.txt")).unwrap(), "fresh\n");
        assert!(!src.join("src/lib.rs").exists());
        assert_eq!(ledger.edited_count(), 3, "the run's own count of what it changed is true");

        // The same undo as for any other change: the set the Changes view reads.
        let set = super::super::changes::change_set(&db, &conv.id, 0);
        assert_eq!(set.files.len(), 3);
        for f in &set.files {
            super::super::changes::undo_file(&db, f).unwrap();
        }
        assert_eq!(std::fs::read_to_string(src.join("a.txt")).unwrap(), "one\ntwo\nthree\n");
        assert!(src.join("src/lib.rs").exists() && !src.join("new.txt").exists());
    }

    /// `TEAM-13`: a copy is read against what it was made from, so a file the folder changed in the
    /// meantime is told apart from one only the copy changed.
    #[test]
    fn a_copy_is_split_by_what_the_folder_did_meanwhile() {
        let src = project("split_src");
        let dst = scratch("split_dst").join("a");
        copy_folder(&src, &dst).unwrap();
        let mut base = base_of(&dst);
        assert!(changes_since(&base, &dst).is_empty());

        std::fs::write(dst.join("a.txt"), "copy
").unwrap();
        std::fs::write(dst.join("new.txt"), "fresh
").unwrap();
        std::fs::write(dst.join("same.txt"), "both
").unwrap();
        std::fs::write(src.join("same.txt"), "both
").unwrap();
        // The folder changed a.txt too, and has a file the copy did not see.
        std::fs::write(src.join("a.txt"), "folder
").unwrap();
        std::fs::write(src.join("other.txt"), "x").unwrap();
        std::fs::remove_file(dst.join("src/lib.rs")).unwrap();

        let c = changes_since(&base, &dst);
        let kinds: Vec<(&str, Kind)> = c.iter().map(|c| (c.rel.as_str(), c.kind)).collect();
        assert_eq!(
            kinds,
            vec![("a.txt", Kind::Modified), ("new.txt", Kind::Added), ("same.txt", Kind::Added), ("src/lib.rs", Kind::Deleted)]
        );
        let s = split(&base, &src, &dst, c);
        let rels = |v: &[Changed]| v.iter().map(|c| c.rel.clone()).collect::<Vec<_>>();
        assert_eq!(rels(&s.clean), vec!["new.txt", "src/lib.rs"]);
        assert_eq!(rels(&s.same), vec!["same.txt"]);
        assert_eq!(rels(&s.conflicts), vec!["a.txt"]);

        // What was brought over is the new start; the conflict is still a difference.
        let done: Vec<Changed> = s.clean.iter().chain(&s.same).cloned().collect();
        advance(&mut base, &dst, &done);
        let left: Vec<String> = changes_since(&base, &dst).into_iter().map(|c| c.rel).collect();
        assert_eq!(left, vec!["a.txt"]);
    }

    #[test]
    fn a_folder_with_too_many_files_is_refused_with_a_reason() {
        let src = scratch("big_src");
        for i in 0..(MAX_FILES + 1) {
            std::fs::write(src.join(format!("f{i}.txt")), "x").unwrap();
        }
        let err = copy_folder(&src, &scratch("big_dst").join("a")).unwrap_err();
        assert!(err.contains("too many files"), "{err}");
        assert!(copy_folder(&src.join("nope"), &scratch("none").join("a")).is_err());
    }

    #[test]
    fn a_record_reads_back_and_an_id_that_is_a_path_does_not() {
        let data = scratch("record");
        let r = Record {
            id: uuid::Uuid::new_v4().to_string(),
            item: 1,
            text: "Export".into(),
            kept: "b".into(),
            reason: "no new dependency".into(),
            ways: vec!["did x".into(), "did y".into()],
            patches: vec!["+a".into(), "+b".into()],
            files: vec![1, 2],
            children: vec!["c1".into(), "c2".into()],
        };
        save(&data, &r).unwrap();
        assert_eq!(load(&data, &r.id), Some(r.clone()));
        assert!(load(&data, "../../etc/passwd").is_none());
        assert!(save(&data, &Record { id: "../x".into(), ..r.clone() }).is_err());
        std::fs::create_dir_all(workdir(&data, &r.id).join("a")).unwrap();
        discard_copies(&data, &r.id);
        assert!(!workdir(&data, &r.id).join("a").exists());
        assert!(load(&data, &r.id).is_some(), "the record stays when the copies go");
    }
}
