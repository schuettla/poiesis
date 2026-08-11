//! Duplicate and near-duplicate detection (Perception, `PHS`). Images cluster
//! by perceptual hash (`phash`); documents cluster by whole-file centroid
//! cosine over whatever `IDX` has already embedded — this module never reads
//! or hashes a document's bytes itself. Grouping only: nothing here deletes
//! anything (`PHS-UI-1`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::db::vectors::similarity;
use crate::db::Db;

use super::index::{has_ext, walk_files, IMAGE_EXTS};
use super::phash::{dhash, hamming, IDENTICAL_MAX, NEAR_MAX};

/// Two document centroids at or above this cosine count as the same document
/// (`PHS-3`). Set well above `RET`'s 0.40 retrieval floor on purpose: that
/// floor asks "is this relevant", this asks "is this the same file".
const DOC_SIMILARITY_MIN: f32 = 0.93;

/// A scan reads as "here's what I found", not a wall of output.
const MAX_GROUPS: usize = 30;

/// One cluster of files that look like the same thing (`PHS-UI-1`).
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateGroup {
    pub kind: String,
    /// "identical" | "near-duplicate" (images) | "similar" (documents).
    pub relation: String,
    pub files: Vec<String>,
}

/// Plain union-find over a fixed `0..n` universe — every cluster in this
/// module is small enough (bounded by `IDX`'s own `MAX_FILES`) that path
/// compression alone is plenty, no union-by-rank needed.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect() }
    }
    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

/// A file's dHash, from the cache when its mtime hasn't moved (`PHS-1`).
/// Shared with `find_similar` (`PHS-3`) so the two halves fill one cache
/// rather than each re-decoding the folder.
pub(crate) fn cached_or_computed_hash(db: &Db, path: &Path) -> Option<u64> {
    let mtime = std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let key = path.to_string_lossy().to_string();
    if let Ok(Some((cached_mtime, hash))) = db.get_image_hash(&key) {
        if cached_mtime == mtime {
            return Some(hash);
        }
    }
    let hash = dhash(path)?;
    let _ = db.set_image_hash(&key, mtime, hash);
    Some(hash)
}

/// Image duplicates under `dir` (`PHS-1`/`PHS-2`). Any pair within
/// `NEAR_MAX` Hamming distance clusters together transitively; a cluster is
/// only called "identical" if every pair inside it is within the tighter
/// `IDENTICAL_MAX` — one distant pair is enough to fall back to
/// "near-duplicate", since "identical" is a promise the UI's copy leans on.
pub fn image_groups(db: &Db, dir: &Path) -> Vec<DuplicateGroup> {
    let files: Vec<PathBuf> = walk_files(dir).into_iter().filter(|p| has_ext(p, &IMAGE_EXTS)).collect();

    let hashes: Vec<(PathBuf, u64)> = files
        .into_iter()
        .filter_map(|path| cached_or_computed_hash(db, &path).map(|h| (path, h)))
        .collect();

    let mut uf = UnionFind::new(hashes.len());
    let mut pair_dist: HashMap<(usize, usize), u32> = HashMap::new();
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            let d = hamming(hashes[i].1, hashes[j].1);
            if d <= NEAR_MAX {
                uf.union(i, j);
                pair_dist.insert((i, j), d);
            }
        }
    }

    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..hashes.len() {
        clusters.entry(uf.find(i)).or_default().push(i);
    }

    let mut groups: Vec<DuplicateGroup> = clusters
        .into_values()
        .filter(|members| members.len() > 1)
        .map(|members| {
            // Transitively-joined pairs with no direct edge (never explicitly
            // measured within the threshold) default to "worst case" so a
            // cluster can't be mislabelled "identical" on an unmeasured pair.
            let worst = members
                .iter()
                .enumerate()
                .flat_map(|(a, &i)| members[a + 1..].iter().map(move |&j| (i.min(j), i.max(j))))
                .map(|key| pair_dist.get(&key).copied().unwrap_or(u32::MAX))
                .max()
                .unwrap_or(0);
            let relation = if worst <= IDENTICAL_MAX { "identical" } else { "near-duplicate" };
            DuplicateGroup {
                kind: "image".into(),
                relation: relation.into(),
                files: members.into_iter().map(|i| hashes[i].0.to_string_lossy().to_string()).collect(),
            }
        })
        .collect();
    rank(&mut groups);
    groups
}

/// Clusters come out of a `HashMap` in arbitrary order, so two scans of an
/// unchanged folder would list the same findings differently and `MAX_GROUPS`
/// would keep an arbitrary subset. Biggest cluster first — that's the one
/// worth acting on — with the path as a tiebreak so the order is stable.
fn rank(groups: &mut Vec<DuplicateGroup>) {
    for g in groups.iter_mut() {
        g.files.sort();
    }
    groups.sort_by(|a, b| b.files.len().cmp(&a.files.len()).then_with(|| a.files[0].cmp(&b.files[0])));
    groups.truncate(MAX_GROUPS);
}

/// Document duplicates (`PHS-3`'s other half): centroid-to-centroid cosine
/// over `IDX`'s already-built chunk vectors, restricted to files under
/// `prefix` — nothing here re-reads or re-embeds anything. Returns nothing if
/// the scope was never indexed; that's an ordinary, expected state, not
/// an error.
pub fn document_groups(db: &Db, scope_key: &str, prefix: &Path) -> Vec<DuplicateGroup> {
    let Ok(all) = db.file_centroids(scope_key) else { return Vec::new() };
    let centroids: Vec<(String, Vec<f32>)> =
        all.into_iter().filter(|(ref_key, _)| Path::new(ref_key).starts_with(prefix)).collect();

    let mut uf = UnionFind::new(centroids.len());
    for i in 0..centroids.len() {
        for j in (i + 1)..centroids.len() {
            if similarity(&centroids[i].1, &centroids[j].1) >= DOC_SIMILARITY_MIN {
                uf.union(i, j);
            }
        }
    }

    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..centroids.len() {
        clusters.entry(uf.find(i)).or_default().push(i);
    }

    let mut groups: Vec<DuplicateGroup> = clusters
        .into_values()
        .filter(|members| members.len() > 1)
        .map(|members| DuplicateGroup {
            kind: "document".into(),
            relation: "similar".into(),
            files: members.into_iter().map(|i| centroids[i].0.clone()).collect(),
        })
        .collect();
    rank(&mut groups);
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::vectors::NewVector;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poiesis_dup_{name}_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_png(path: &Path, fill: [u8; 3]) {
        image::ImageBuffer::from_fn(32, 32, |_, _| image::Rgb(fill)).save(path).unwrap();
    }

    #[test]
    fn two_identical_images_group_as_identical() {
        let dir = scratch_dir("identical");
        write_png(&dir.join("a.png"), [10, 200, 30]);
        write_png(&dir.join("b.png"), [10, 200, 30]);
        let db = Db::open_in_memory().unwrap();

        let groups = image_groups(&db, &dir);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].relation, "identical");
        assert_eq!(groups[0].files.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unrelated_images_do_not_group() {
        let dir = scratch_dir("unrelated");
        write_png(&dir.join("a.png"), [10, 200, 30]);
        // A checkerboard, not a gradient: dHash only encodes the sign of each
        // row-wise step, so a flat fill and a smooth gradient can legitimately
        // hash identically (every step the same sign either way). A
        // checkerboard's steps flip sign constantly, which a flat fill's don't.
        image::ImageBuffer::from_fn(32, 32, |x, y| {
            let on = (x / 4 + y / 4) % 2 == 0;
            image::Rgb([if on { 250u8 } else { 5u8 }; 3])
        })
        .save(dir.join("b.png"))
        .unwrap();
        let db = Db::open_in_memory().unwrap();

        assert!(image_groups(&db, &dir).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_biggest_cluster_is_listed_first_and_each_group_is_sorted() {
        let dir = scratch_dir("ranked");
        for name in ["a.png", "b.png", "c.png"] {
            write_png(&dir.join(name), [10, 200, 30]);
        }
        for name in ["y.png", "z.png"] {
            image::ImageBuffer::from_fn(32, 32, |x, y| {
                let on = (x / 4 + y / 4) % 2 == 0;
                image::Rgb([if on { 250u8 } else { 5u8 }; 3])
            })
            .save(dir.join(name))
            .unwrap();
        }
        let db = Db::open_in_memory().unwrap();

        let groups = image_groups(&db, &dir);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].files.len(), 3, "the bigger cluster leads");
        assert_eq!(groups[1].files.len(), 2);
        let mut sorted = groups[0].files.clone();
        sorted.sort();
        assert_eq!(groups[0].files, sorted, "files within a group are in a stable order");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_second_scan_reuses_the_cached_hash_for_an_unchanged_file() {
        let dir = scratch_dir("cache");
        let path = dir.join("a.png");
        write_png(&path, [1, 2, 3]);
        let db = Db::open_in_memory().unwrap();

        image_groups(&db, &dir);
        let key = path.to_string_lossy().to_string();
        let (mtime, hash) = db.get_image_hash(&key).unwrap().unwrap();

        // Poison the cached hash directly — if the second scan actually
        // rehashes, it will overwrite this with the true value, not read it
        // back unchanged.
        db.set_image_hash(&key, mtime, hash.wrapping_add(1)).unwrap();
        let cached = cached_or_computed_hash(&db, &path).unwrap();
        assert_eq!(cached, hash.wrapping_add(1), "an unchanged mtime should reuse the cache as-is");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn row(scope: &str, ref_key: &str, chunk_ix: i64, vec: Vec<f32>) -> NewVector {
        NewVector {
            owner_kind: "file".into(),
            scope_key: scope.into(),
            ref_key: ref_key.into(),
            chunk_ix,
            text: "x".into(),
            model: "m1".into(),
            dim: vec.len() as i64,
            vec,
            mtime: None,
        }
    }

    #[test]
    fn documents_with_near_identical_centroids_group_together() {
        let db = Db::open_in_memory().unwrap();
        db.insert_vectors(&[
            row("/docs", "/docs/a.md", 0, vec![1.0, 0.0]),
            row("/docs", "/docs/b.md", 0, vec![0.99, 0.14]),
            row("/docs", "/docs/c.md", 0, vec![0.0, 1.0]),
        ])
        .unwrap();

        let groups = document_groups(&db, "/docs", Path::new("/docs"));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].kind, "document");
        let mut files = groups[0].files.clone();
        files.sort();
        assert_eq!(files, vec!["/docs/a.md".to_string(), "/docs/b.md".to_string()]);
    }

    #[test]
    fn document_groups_respect_the_subfolder_prefix() {
        let db = Db::open_in_memory().unwrap();
        db.insert_vectors(&[
            row("/docs", "/docs/sub/a.md", 0, vec![1.0, 0.0]),
            row("/docs", "/docs/sub/b.md", 0, vec![1.0, 0.0]),
            row("/docs", "/docs/other/c.md", 0, vec![1.0, 0.0]),
        ])
        .unwrap();

        let groups = document_groups(&db, "/docs", Path::new("/docs/sub"));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].files.len(), 2);
        assert!(groups[0].files.iter().all(|f| f.starts_with("/docs/sub")));
    }

    #[test]
    fn an_unindexed_scope_yields_no_document_groups() {
        let db = Db::open_in_memory().unwrap();
        assert!(document_groups(&db, "/never/indexed", Path::new("/never/indexed")).is_empty());
    }
}
