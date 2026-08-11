//! Folder indexing (Perception, IDX): turn a granted folder into searchable
//! chunks in the shared vector store, so `RET`'s `search_folder` tool has
//! something to search. Building is user-initiated (`IDX-UI-1`'s "Read it")
//! and runs on a background task — indexing must never block a chat turn
//! (`IDX-7`).
//!
//! Reuses `filesystem`'s ignore rules and binary sniff rather than forking
//! them (`IDX-2`): a folder should look the same to the indexer as it does to
//! `list_directory`/`search_files`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::db::vectors::NewVector;
use crate::db::Db;
use crate::runtime::proxy::CancelFlag;
use crate::runtime::{EmbedManager, RuntimeManager};

use super::filesystem::{is_ignored, looks_binary};

// ---- caps (IDX-2) ----

const MAX_FILES: usize = 500;
const MAX_DEPTH: usize = 6;
const MAX_CHUNKS_PER_FILE: usize = 60;

// ---- chunking (IDX-4) ----

const CHUNK_CHARS: usize = 1200;
const CHUNK_OVERLAP: usize = 200;

/// Files past this are skipped as "too large" outright — indexing has no
/// windowed-read escape hatch the way `read_file` does, so there's no partial
/// path worth taking.
const MAX_INDEX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Why one file didn't make it into the index (IDX-3), surfaced verbatim by
/// `IDX-UI-2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Image or scanned PDF — Part IV's job (`VIS`/`OCR`), deferred this phase.
    NeedsVision,
    TooLarge,
    NotText,
}

impl SkipReason {
    pub fn text(self) -> &'static str {
        match self {
            SkipReason::NeedsVision => "needs my eyes — no vision model loaded",
            SkipReason::TooLarge => "too large",
            SkipReason::NotText => "not text",
        }
    }
}

/// One skipped file, as stored in `index_roots.skipped` and rendered by
/// `IDX-UI-2`.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
}

/// `IDX-7`'s progress event: a plain counting line, never a percentage or bar.
#[derive(Debug, Clone, Serialize)]
pub struct IndexProgress {
    pub files_done: usize,
    pub files_total: usize,
}

/// What one build produced, for the caller to persist via
/// `Db::set_index_root_result`.
pub struct BuildOutcome {
    pub model: String,
    pub dim: i64,
    pub file_count: i64,
    pub chunk_count: i64,
    pub skipped: Vec<SkippedFile>,
}

/// Registry of in-flight builds, keyed by canonical root path, so
/// `cancel_index_cmd` can reach a build started by an earlier command
/// invocation. One `CancelFlag` per root — building the same root twice at
/// once isn't meaningful, so a new build simply replaces the old flag's entry
/// (the old build keeps running until its own flag is checked, but nothing
/// currently drives two builds of one root concurrently).
#[derive(Default)]
pub struct IndexManager {
    active: Mutex<HashMap<String, CancelFlag>>,
}

impl IndexManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a fresh flag for `path` and return it for the build loop to
    /// poll.
    pub fn start(&self, path: &str) -> CancelFlag {
        let flag = CancelFlag::new();
        self.active.lock().unwrap().insert(path.to_string(), flag.clone());
        flag
    }

    /// Build finished (however it ended) — stop tracking it.
    pub fn finish(&self, path: &str) {
        self.active.lock().unwrap().remove(path);
    }

    /// Signal a running build to stop. Returns whether one was actually
    /// running, so the caller can say so rather than claim a no-op succeeded.
    pub fn cancel(&self, path: &str) -> bool {
        match self.active.lock().unwrap().get(path) {
            Some(flag) => {
                flag.cancel();
                true
            }
            None => false,
        }
    }
}

// ---- walking ----

fn walk_dir(root: &Path, dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 || out.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if is_ignored(&name, false) {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            walk_dir(root, &p, depth - 1, out);
        } else {
            out.push(p);
        }
    }
}

/// Every file under `root` worth considering, capped at `MAX_FILES` (IDX-2).
/// Order is directory-walk order, not sorted — stable enough for progress
/// counting, not meant as a listing. `pub(crate)` so `agent::duplicates`
/// (`PHS`) can walk the same tree under the same ignore rules and cap,
/// rather than a second, independently-tuned walker.
pub(crate) fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_dir(root, root, MAX_DEPTH, &mut out);
    out
}

/// Visible so `agent::retrieval` can show the same relative form for a hit's
/// source file that `IDX-UI-2` uses for a skipped one.
pub(crate) fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

fn file_mtime_secs(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// How many files changed (added, edited, or removed) since the last build —
/// `IDX-UI-3`'s "N files changed since I read this". A stat-only walk, no
/// reading or embedding, so it's cheap enough to run on every status check of
/// a `stale` root rather than only during a rebuild.
pub fn count_changed(db: &Db, root: &Path, scope_key: &str) -> usize {
    let existing = db.file_mtimes_for_scope(scope_key).unwrap_or_default();
    let files = walk_files(root);
    let mut seen = HashSet::new();
    let mut changed = 0;
    for path in &files {
        let ref_key = path.to_string_lossy().to_string();
        let mtime = file_mtime_secs(path);
        match (mtime, existing.get(&ref_key)) {
            (Some(m), Some(prev)) if *prev == m => {}
            _ => changed += 1,
        }
        seen.insert(ref_key);
    }
    changed + existing.keys().filter(|k| !seen.contains(k.as_str())).count()
}

// ---- extraction + chunking ----

/// Collapse runs of whitespace (including newlines) to one space, then cut
/// into overlapping windows (IDX-4). Collapsing first means a chunk boundary
/// never lands mid-sentence just because of how a file happened to be
/// line-wrapped.
fn chunk_text(text: &str) -> Vec<String> {
    let mut collapsed = String::with_capacity(text.len());
    let mut last_was_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                collapsed.push(' ');
            }
            last_was_space = true;
        } else {
            collapsed.push(c);
            last_was_space = false;
        }
    }
    let collapsed = collapsed.trim();
    if collapsed.is_empty() {
        return Vec::new();
    }

    let chars: Vec<char> = collapsed.chars().collect();
    let step = CHUNK_CHARS - CHUNK_OVERLAP;
    let mut chunks = Vec::new();
    let mut start = 0;
    loop {
        let end = (start + CHUNK_CHARS).min(chars.len());
        chunks.push(chars[start..end].iter().collect::<String>());
        if end == chars.len() || chunks.len() >= MAX_CHUNKS_PER_FILE {
            break;
        }
        start += step;
    }
    chunks
}

enum Extracted {
    Chunks(Vec<String>),
    Skip(SkipReason),
}

/// Shared with `agent::duplicates` (`PHS-1`) — an image duplicate scan should
/// recognise exactly the files IDX itself would flag as `NeedsVision`.
pub(crate) const IMAGE_EXTS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff"];

pub(crate) fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| exts.iter().any(|want| e.eq_ignore_ascii_case(want)))
        .unwrap_or(false)
}

/// Text sniffing + PDF text layer (IDX-3). Images and text-less (scanned)
/// PDFs are always `NeedsVision` this phase — `VIS`/`OCR` are deferred, and
/// this is the one branch that changes when they land.
fn extract(path: &Path) -> Extracted {
    let Ok(meta) = std::fs::metadata(path) else {
        return Extracted::Skip(SkipReason::NotText);
    };
    if meta.len() > MAX_INDEX_FILE_BYTES {
        return Extracted::Skip(SkipReason::TooLarge);
    }
    if has_ext(path, &IMAGE_EXTS) {
        return Extracted::Skip(SkipReason::NeedsVision);
    }
    if has_ext(path, &["pdf"]) {
        let text = pdf_extract::extract_text(path.to_string_lossy().to_string()).unwrap_or_default();
        return if text.trim().is_empty() {
            Extracted::Skip(SkipReason::NeedsVision)
        } else {
            Extracted::Chunks(chunk_text(&text))
        };
    }
    if looks_binary(path) {
        return Extracted::Skip(SkipReason::NotText);
    }
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let chunks = chunk_text(&text);
            if chunks.is_empty() {
                Extracted::Skip(SkipReason::NotText)
            } else {
                Extracted::Chunks(chunks)
            }
        }
        Err(_) => Extracted::Skip(SkipReason::NotText),
    }
}

// ---- build ----

/// Build (or rebuild) the index for `root`. Reused rows are left untouched;
/// changed or new files are (re-)embedded; files that vanished from disk lose
/// their rows. A model change from the root's last build forces a full
/// rebuild rather than an incremental one (`IDX-6`) — chunks embedded under a
/// different model aren't comparable to what's kept.
///
/// `on_progress` fires once per file, after that file is settled (embedded,
/// skipped, or reused). Checked for cancellation between files, never mid-file
/// — a file's chunks are embedded as one request, so there's no smaller unit
/// to cancel inside.
pub async fn build_index<F: FnMut(IndexProgress)>(
    client: &reqwest::Client,
    mgr: &RuntimeManager,
    embed_mgr: &EmbedManager,
    db: &Db,
    root: &Path,
    cancel: &CancelFlag,
    mut on_progress: F,
) -> Result<BuildOutcome, String> {
    let model = db
        .default_model_by_role("embed")
        .map_err(|e| e.to_string())?
        .ok_or("No recall model is installed yet — set one up in Settings first.")?;
    let model_path = PathBuf::from(&model.path);
    if !model_path.exists() {
        return Err("The recall model file is missing — reinstall it in Settings.".into());
    }
    let server_binary = crate::commands::embedgen::engine_binary_path(mgr, db)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("The recall engine isn't installed yet — set it up in Settings first.")?;

    let scope_key = root.to_string_lossy().to_string();

    // IDX-6: a stored root under a different model can't be updated in
    // place — the vectors it already has mean nothing next to a fresh one.
    let stored = db.get_index_root(&scope_key).map_err(|e| e.to_string())?;
    let full_rebuild = stored.map(|r| r.model != model.name).unwrap_or(true);
    if full_rebuild {
        db.delete_vectors_for_scope("file", &scope_key).map_err(|e| e.to_string())?;
    }
    let existing_mtimes: HashMap<String, i64> = if full_rebuild {
        HashMap::new()
    } else {
        db.file_mtimes_for_scope(&scope_key).map_err(|e| e.to_string())?
    };

    let files = walk_files(root);
    let files_total = files.len();
    let mut seen_refs: HashSet<String> = HashSet::new();
    let mut skipped = Vec::new();
    let mut file_count: i64 = 0;
    let mut chunk_count: i64 = 0;
    let mut dim: i64 = stored_dim(db, &scope_key).unwrap_or(0);

    for (i, path) in files.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err("cancelled".into());
        }
        let ref_key = path.to_string_lossy().to_string();
        let display = display_path(path, root);
        seen_refs.insert(ref_key.clone());

        let mtime = file_mtime_secs(path);
        if !full_rebuild {
            if let (Some(m), Some(prev)) = (mtime, existing_mtimes.get(&ref_key)) {
                if *prev == m {
                    // IDX-5: unchanged since the last build — reuse it.
                    file_count += 1;
                    on_progress(IndexProgress { files_done: i + 1, files_total });
                    continue;
                }
            }
            let _ = db.delete_vectors_for_ref("file", &scope_key, &ref_key);
        }

        match extract(path) {
            Extracted::Skip(reason) => {
                skipped.push(SkippedFile { path: display, reason: reason.text().to_string() });
            }
            Extracted::Chunks(chunks) => {
                match embed_mgr.embed_texts(client, server_binary.clone(), model_path.clone(), &chunks).await {
                    Ok(vectors) => {
                        dim = vectors.first().map(|v| v.len() as i64).unwrap_or(dim);
                        let rows: Vec<NewVector> = chunks
                            .iter()
                            .zip(vectors.iter())
                            .enumerate()
                            .map(|(ix, (text, v))| NewVector {
                                owner_kind: "file".into(),
                                scope_key: scope_key.clone(),
                                ref_key: ref_key.clone(),
                                chunk_ix: ix as i64,
                                text: text.clone(),
                                model: model.name.clone(),
                                dim,
                                vec: v.clone(),
                                mtime,
                            })
                            .collect();
                        chunk_count += rows.len() as i64;
                        let _ = db.insert_vectors(&rows);
                        file_count += 1;
                    }
                    Err(_) => {
                        skipped.push(SkippedFile {
                            path: display,
                            reason: "the recall engine couldn't read it".to_string(),
                        });
                    }
                }
            }
        }
        on_progress(IndexProgress { files_done: i + 1, files_total });
    }

    // IDX-5: a file that vanished from disk loses its rows too.
    if !full_rebuild {
        for old_ref in existing_mtimes.keys() {
            if !seen_refs.contains(old_ref) {
                let _ = db.delete_vectors_for_ref("file", &scope_key, old_ref);
            }
        }
    }

    Ok(BuildOutcome { model: model.name, dim, file_count, chunk_count, skipped })
}

fn stored_dim(db: &Db, scope_key: &str) -> Option<i64> {
    db.get_index_root(scope_key).ok().flatten().map(|r| r.dim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_collapses_before_chunking() {
        let chunks = chunk_text("line one\n\n\tline   two");
        assert_eq!(chunks, vec!["line one line two".to_string()]);
    }

    #[test]
    fn empty_or_whitespace_only_text_yields_no_chunks() {
        assert!(chunk_text("   \n\t  ").is_empty());
        assert!(chunk_text("").is_empty());
    }

    #[test]
    fn chunks_overlap_by_200_chars() {
        let text: String = ('a'..='z').cycle().take(3000).collect();
        let chunks = chunk_text(&text);
        assert!(chunks.len() >= 2);
        assert_eq!(chunks[0].chars().count(), CHUNK_CHARS);
        // The tail of chunk 0 and the head of chunk 1 should be the same 200
        // characters — that's what "overlap" means operationally.
        let tail: String = chunks[0].chars().skip(CHUNK_CHARS - CHUNK_OVERLAP).collect();
        let head: String = chunks[1].chars().take(CHUNK_OVERLAP).collect();
        assert_eq!(tail, head);
    }

    #[test]
    fn a_file_cannot_produce_more_than_the_chunk_cap() {
        let text: String = ('a'..='z').cycle().take(200_000).collect();
        let chunks = chunk_text(&text);
        assert_eq!(chunks.len(), MAX_CHUNKS_PER_FILE);
    }

    #[test]
    fn image_extensions_need_vision_regardless_of_content() {
        let f = std::env::temp_dir().join(format!("poiesis_idx_{}.png", uuid::Uuid::new_v4()));
        std::fs::write(&f, b"not actually a png").unwrap();
        assert!(matches!(extract(&f), Extracted::Skip(SkipReason::NeedsVision)));
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn a_text_file_extracts_its_own_content_as_one_chunk() {
        let f = std::env::temp_dir().join(format!("poiesis_idx_{}.md", uuid::Uuid::new_v4()));
        std::fs::write(&f, "hello world").unwrap();
        match extract(&f) {
            Extracted::Chunks(chunks) => assert_eq!(chunks, vec!["hello world".to_string()]),
            Extracted::Skip(r) => panic!("expected chunks, got skip: {r:?}"),
        }
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn binary_files_are_skipped_as_not_text() {
        let f = std::env::temp_dir().join(format!("poiesis_idx_{}.dat", uuid::Uuid::new_v4()));
        std::fs::write(&f, [0x00, 0x01, 0x02, 0x00]).unwrap();
        assert!(matches!(extract(&f), Extracted::Skip(SkipReason::NotText)));
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn oversized_files_are_skipped_as_too_large() {
        let f = std::env::temp_dir().join(format!("poiesis_idx_{}.txt", uuid::Uuid::new_v4()));
        // Sparse-ish write: just needs metadata().len() past the cap, so write
        // real bytes in a small loop rather than seeking (portable, still fast).
        let chunk = "x".repeat(1024 * 1024);
        std::fs::write(&f, chunk.repeat(6)).unwrap();
        assert!(matches!(extract(&f), Extracted::Skip(SkipReason::TooLarge)));
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn walking_honours_the_shared_ignore_list_and_file_cap() {
        let dir = std::env::temp_dir().join(format!("poiesis_idx_walk_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("node_modules").join("junk.js"), "x").unwrap();
        std::fs::write(dir.join("a.md"), "x").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("b.md"), "x").unwrap();

        let files = walk_files(&dir);
        assert_eq!(files.len(), 2, "node_modules must be skipped like it is everywhere else");
        assert!(files.iter().any(|p| p.ends_with("a.md")));
        assert!(files.iter().any(|p| p.ends_with("b.md")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_cancel_registered_by_path_is_visible_to_a_second_lookup() {
        let mgr = IndexManager::new();
        let flag = mgr.start("/docs");
        assert!(!flag.is_cancelled());
        assert!(mgr.cancel("/docs"), "a running build should be found and cancelled");
        assert!(flag.is_cancelled());
        // The registry entry only clears on `finish` (the build loop noticing
        // it was cancelled and exiting) — cancelling again before that is a
        // harmless no-op on an already-cancelled flag, not "nothing found".
        assert!(mgr.cancel("/docs"));
        mgr.finish("/docs");
        assert!(!mgr.cancel("/docs"), "finished — nothing left to cancel");
    }

    #[test]
    fn cancelling_an_unknown_root_reports_nothing_to_cancel() {
        let mgr = IndexManager::new();
        assert!(!mgr.cancel("/never/built"));
    }

    #[test]
    fn finish_stops_a_later_cancel_from_finding_it() {
        let mgr = IndexManager::new();
        mgr.start("/docs");
        mgr.finish("/docs");
        assert!(!mgr.cancel("/docs"));
    }

    #[test]
    fn display_path_is_relative_and_forward_slashed() {
        let root = std::env::temp_dir();
        let p = root.join("a").join("b.md");
        assert_eq!(display_path(&p, &root), "a/b.md");
    }
}
