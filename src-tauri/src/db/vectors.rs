//! The vector store (Perception, VEC): one table serving both durable-memory
//! recall (SEM) and folder retrieval (RET). Vectors are stored pre-normalised
//! to unit length (`EMB-3` does the normalising), so similarity is a plain
//! dot product — no sqrt at query time.

use rusqlite::params;

use super::{new_id, now_ms, Db, DbError};

/// A vector row to insert, before it has an id.
#[derive(Debug, Clone)]
pub struct NewVector {
    /// "memory" | "file"
    pub owner_kind: String,
    /// memory: collection name; file: canonical index root.
    pub scope_key: String,
    /// memory: entry slug; file: absolute path.
    pub ref_key: String,
    pub chunk_ix: i64,
    pub text: String,
    pub model: String,
    pub dim: i64,
    pub vec: Vec<f32>,
    pub mtime: Option<i64>,
}

/// One search result (VEC-3).
#[derive(Debug, Clone, PartialEq)]
pub struct VecHit {
    pub ref_key: String,
    pub chunk_ix: i64,
    pub text: String,
    pub score: f32,
    /// The chunk's own (normalised) embedding. Unused by plain top-k callers,
    /// but `RET-2`'s MMR diversification needs pairwise similarity between
    /// candidates, which can't be recovered from `score` alone.
    pub vec: Vec<f32>,
}

/// The outcome of a scoped vector search (VEC-4). A scope that was embedded
/// under a different model than the caller expects is never partially
/// searched or silently mixed across spaces — the caller must rebuild it.
#[derive(Debug, Clone, PartialEq)]
pub enum ScopeSearch {
    Hits(Vec<VecHit>),
    Stale,
}

/// Encode a `f32` vector as little-endian bytes (VEC-2).
pub fn encode_vec(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for x in v {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    bytes
}

/// Decode little-endian bytes back into a `f32` vector. Trailing bytes that
/// don't form a complete `f32` are dropped — defensive against a truncated row.
pub fn decode_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Cosine similarity between two pre-normalised vectors is their dot product.
///
/// Vectors of different length are **not** comparable, and `zip` would happily
/// truncate to the shorter one and return a plausible-looking number, so this
/// returns `0.0` rather than a partial dot product. Callers that could hit a
/// mismatch (`search_vectors`) reject it up front instead of relying on this.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

impl Db {
    /// Insert vector rows. `(owner_kind, scope_key, ref_key, chunk_ix)` is
    /// unique, so re-embedding an entry overwrites its chunks in place rather
    /// than silently doubling them — a duplicated chunk would surface as a
    /// duplicated recall hit, not an error, which is exactly the kind of quiet
    /// wrongness the model guard exists to prevent elsewhere.
    ///
    /// Callers still delete first when the *shape* changes — via
    /// `delete_vectors_for_ref` (an entry that now has fewer chunks) or
    /// `delete_vectors_for_scope` (a whole root's model changed, VEC-4/IDX-6) —
    /// since an upsert alone can't remove chunks that no longer exist.
    pub fn insert_vectors(&self, rows: &[NewVector]) -> Result<(), DbError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO vectors(id, owner_kind, scope_key, ref_key, chunk_ix, text, model, dim, vec, mtime, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(owner_kind, scope_key, ref_key, chunk_ix) DO UPDATE SET
                   text = excluded.text, model = excluded.model, dim = excluded.dim,
                   vec = excluded.vec, mtime = excluded.mtime, created_at = excluded.created_at",
            )?;
            let ts = now_ms();
            for r in rows {
                stmt.execute(params![
                    new_id(),
                    r.owner_kind,
                    r.scope_key,
                    r.ref_key,
                    r.chunk_ix,
                    r.text,
                    r.model,
                    r.dim,
                    encode_vec(&r.vec),
                    r.mtime,
                    ts,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Drop every chunk belonging to one entry (a memory fact, a file) before
    /// re-embedding it.
    pub fn delete_vectors_for_ref(&self, owner_kind: &str, scope_key: &str, ref_key: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM vectors WHERE owner_kind = ?1 AND scope_key = ?2 AND ref_key = ?3",
            params![owner_kind, scope_key, ref_key],
        )?;
        Ok(())
    }

    /// Drop every row in a scope (a whole folder root, or a whole memory
    /// collection) — used for a full rebuild after a model change (VEC-4, IDX-6).
    pub fn delete_vectors_for_scope(&self, owner_kind: &str, scope_key: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM vectors WHERE owner_kind = ?1 AND scope_key = ?2",
            params![owner_kind, scope_key],
        )?;
        Ok(())
    }

    /// Discard every vector in every scope and mark all indexed folders stale
    /// (VEC-4, IDX-6). Called when the embedding model itself changes: nothing
    /// embedded under the old model can be compared with anything embedded
    /// under the new one, and mixing the two is silently wrong rather than
    /// merely out of date. Memory vectors come back on the next turn (SEM-2);
    /// folders are re-read on the user's word (IDX-UI-3).
    ///
    /// Returns how many vectors were discarded, so the caller can say so.
    pub fn invalidate_all_vectors(&self) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        let dropped = conn.execute("DELETE FROM vectors", [])?;
        conn.execute("UPDATE index_roots SET state = 'stale'", [])?;
        Ok(dropped)
    }

    /// Every `ref_key` already embedded in one scope under `model`, regardless
    /// of `dim` — a model name determines its dimension in practice, and this
    /// is only used to decide what still needs embedding (`SEM-1`/`SEM-2`), not
    /// to search, so it doesn't need `VEC-4`'s stricter dim guard.
    pub fn vector_ref_keys_for_scope(
        &self,
        owner_kind: &str,
        scope_key: &str,
        model: &str,
    ) -> Result<std::collections::HashSet<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT ref_key FROM vectors WHERE owner_kind = ?1 AND scope_key = ?2 AND model = ?3",
        )?;
        let rows = stmt
            .query_map(params![owner_kind, scope_key, model], |r| r.get::<_, String>(0))?
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        Ok(rows)
    }

    /// Every indexed file's stored `mtime`, by `ref_key`, for one folder root
    /// (`IDX-5`'s incremental check: an unchanged mtime means the file can be
    /// reused rather than re-embedded). One row per file — every chunk of a
    /// file carries the same `mtime` since they're inserted together, so `MIN`
    /// is just "any of them".
    pub fn file_mtimes_for_scope(
        &self,
        scope_key: &str,
    ) -> Result<std::collections::HashMap<String, i64>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT ref_key, MIN(mtime) FROM vectors
             WHERE owner_kind = 'file' AND scope_key = ?1 AND mtime IS NOT NULL
             GROUP BY ref_key",
        )?;
        let rows = stmt
            .query_map(params![scope_key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
        Ok(rows)
    }

    /// Rough on-disk footprint of one scope, for `IDX-UI-4`'s "size on disk" —
    /// the stored vector bytes plus the source text, which dwarfs the id/model
    /// bookkeeping columns enough to not bother summing those too.
    pub fn scope_size_bytes(&self, owner_kind: &str, scope_key: &str) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        let bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(vec) + LENGTH(text)), 0) FROM vectors
             WHERE owner_kind = ?1 AND scope_key = ?2",
            params![owner_kind, scope_key],
            |r| r.get(0),
        )?;
        Ok(bytes)
    }

    /// Linear-scan search within one scope (VEC-3). Guarded against a stale
    /// embedding model (VEC-4): if any row in the scope was embedded under a
    /// different model/dim than the caller expects, the whole scope is
    /// reported `Stale` rather than partially searched.
    ///
    /// The guard covers the *query* too. A query vector whose length disagrees
    /// with `dim` cannot be compared with anything in this scope — dot products
    /// over a truncated overlap look like ordinary scores and would rank
    /// silently wrong, so this reports `Stale` before touching a single row.
    pub fn search_vectors(
        &self,
        owner_kind: &str,
        scope_key: &str,
        model: &str,
        dim: i64,
        query: &[f32],
        k: usize,
    ) -> Result<ScopeSearch, DbError> {
        if query.len() as i64 != dim {
            return Ok(ScopeSearch::Stale);
        }
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT ref_key, chunk_ix, text, model, dim, vec FROM vectors
             WHERE owner_kind = ?1 AND scope_key = ?2",
        )?;
        let rows = stmt
            .query_map(params![owner_kind, scope_key], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Vec<u8>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut hits = Vec::with_capacity(rows.len());
        for (ref_key, chunk_ix, text, row_model, row_dim, vec_bytes) in rows {
            if row_model != model || row_dim != dim {
                return Ok(ScopeSearch::Stale);
            }
            let v = decode_vec(&vec_bytes);
            // A blob that doesn't hold `dim` floats is a truncated row: rebuild
            // the scope rather than score it against a partial vector.
            if v.len() as i64 != row_dim {
                return Ok(ScopeSearch::Stale);
            }
            let score = similarity(query, &v);
            hits.push(VecHit { ref_key, chunk_ix, text, score, vec: v });
        }
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(k);
        Ok(ScopeSearch::Hits(hits))
    }

    /// One centroid per file in a scope — the mean of its chunk vectors,
    /// re-normalised to unit length (`PHS-3`'s document half). Cosine between
    /// two centroids is then a plain dot product, like every other comparison
    /// in this store. Whole-file meaning on purpose: best-chunk matching would
    /// over-score two files that merely share one stray paragraph.
    pub fn file_centroids(&self, scope_key: &str) -> Result<Vec<(String, Vec<f32>)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT ref_key, vec FROM vectors WHERE owner_kind = 'file' AND scope_key = ?1",
        )?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map(params![scope_key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut sums: std::collections::HashMap<String, (Vec<f32>, usize)> = std::collections::HashMap::new();
        for (ref_key, bytes) in rows {
            let v = decode_vec(&bytes);
            let entry = sums.entry(ref_key).or_insert_with(|| (vec![0.0; v.len()], 0));
            // A dimension mismatch (a scope mid-way through a model change)
            // just stops accumulating for that file rather than panicking on
            // a length mismatch — the file's centroid comes out from whatever
            // chunks did agree, or is dropped below if none did.
            if entry.0.len() == v.len() {
                for (a, b) in entry.0.iter_mut().zip(&v) {
                    *a += b;
                }
                entry.1 += 1;
            }
        }

        Ok(sums
            .into_iter()
            .filter_map(|(ref_key, (mut sum, count))| {
                if count == 0 {
                    return None;
                }
                let norm: f32 = sum.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm <= 0.0 {
                    return None;
                }
                for x in sum.iter_mut() {
                    *x /= norm;
                }
                Some((ref_key, sum))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_1_SQRT_2;

    #[test]
    fn encode_decode_round_trips() {
        let v = vec![0.5_f32, -0.25, 1.0, 0.0];
        assert_eq!(decode_vec(&encode_vec(&v)), v);
    }

    #[test]
    fn similarity_of_identical_unit_vectors_is_one() {
        let v = vec![0.6_f32, 0.8];
        assert!((similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn similarity_of_orthogonal_vectors_is_zero() {
        assert!(similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
    }

    #[test]
    fn similarity_refuses_to_compare_different_dimensions() {
        // Truncating to the overlap would return 1.0 here, which is worse than
        // useless: it looks like a perfect match.
        assert_eq!(similarity(&[1.0, 0.0, 0.0], &[1.0]), 0.0);
    }

    /// A query embedded under a different model has a different length. That
    /// must read as "this scope needs rebuilding", never as a ranking over the
    /// components the two happen to share.
    #[test]
    fn a_query_of_the_wrong_dimension_is_stale_not_scored() {
        let db = Db::open_in_memory().unwrap();
        db.insert_vectors(&[NewVector {
            owner_kind: "memory".into(),
            scope_key: "facts".into(),
            ref_key: "a".into(),
            chunk_ix: 0,
            text: "x".into(),
            model: "m1".into(),
            dim: 2,
            vec: vec![1.0, 0.0],
            mtime: None,
        }])
        .unwrap();

        let result = db.search_vectors("memory", "facts", "m1", 2, &[1.0, 0.0, 0.0], 5).unwrap();
        assert_eq!(result, ScopeSearch::Stale);
    }

    /// Re-embedding an entry replaces its chunks. Without the unique index this
    /// silently returned two hits for one fact.
    #[test]
    fn re_inserting_the_same_chunk_overwrites_instead_of_duplicating() {
        let db = Db::open_in_memory().unwrap();
        let row = |text: &str, v: Vec<f32>| NewVector {
            owner_kind: "memory".into(),
            scope_key: "facts".into(),
            ref_key: "a".into(),
            chunk_ix: 0,
            text: text.into(),
            model: "m1".into(),
            dim: 2,
            vec: v,
            mtime: None,
        };
        db.insert_vectors(&[row("first", vec![1.0, 0.0])]).unwrap();
        db.insert_vectors(&[row("second", vec![0.0, 1.0])]).unwrap();

        let ScopeSearch::Hits(hits) = db.search_vectors("memory", "facts", "m1", 2, &[0.0, 1.0], 10).unwrap() else {
            panic!("expected hits");
        };
        assert_eq!(hits.len(), 1, "the chunk should have been replaced, not duplicated");
        assert_eq!(hits[0].text, "second");
    }

    #[test]
    fn search_ranks_by_similarity_and_respects_k() {
        let db = Db::open_in_memory().unwrap();
        let rows = vec![
            NewVector {
                owner_kind: "memory".into(),
                scope_key: "facts".into(),
                ref_key: "a".into(),
                chunk_ix: 0,
                text: "close".into(),
                model: "m1".into(),
                dim: 2,
                vec: vec![1.0, 0.0],
                mtime: None,
            },
            NewVector {
                owner_kind: "memory".into(),
                scope_key: "facts".into(),
                ref_key: "b".into(),
                chunk_ix: 0,
                text: "far".into(),
                model: "m1".into(),
                dim: 2,
                vec: vec![0.0, 1.0],
                mtime: None,
            },
            NewVector {
                owner_kind: "memory".into(),
                scope_key: "facts".into(),
                ref_key: "c".into(),
                chunk_ix: 0,
                text: "mid".into(),
                model: "m1".into(),
                dim: 2,
                vec: vec![FRAC_1_SQRT_2, FRAC_1_SQRT_2],
                mtime: None,
            },
        ];
        db.insert_vectors(&rows).unwrap();

        let ScopeSearch::Hits(hits) = db.search_vectors("memory", "facts", "m1", 2, &[1.0, 0.0], 2).unwrap() else {
            panic!("expected hits");
        };
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].ref_key, "a");
        assert_eq!(hits[1].ref_key, "c");
    }

    #[test]
    fn a_scope_with_a_stale_model_is_reported_not_partially_searched() {
        let db = Db::open_in_memory().unwrap();
        db.insert_vectors(&[NewVector {
            owner_kind: "file".into(),
            scope_key: "/docs".into(),
            ref_key: "a.md".into(),
            chunk_ix: 0,
            text: "old".into(),
            model: "old-model".into(),
            dim: 2,
            vec: vec![1.0, 0.0],
            mtime: None,
        }])
        .unwrap();

        let result = db.search_vectors("file", "/docs", "new-model", 2, &[1.0, 0.0], 5).unwrap();
        assert_eq!(result, ScopeSearch::Stale);
    }

    #[test]
    fn delete_for_ref_only_drops_that_entry() {
        let db = Db::open_in_memory().unwrap();
        db.insert_vectors(&[
            NewVector {
                owner_kind: "memory".into(),
                scope_key: "facts".into(),
                ref_key: "a".into(),
                chunk_ix: 0,
                text: "x".into(),
                model: "m1".into(),
                dim: 1,
                vec: vec![1.0],
                mtime: None,
            },
            NewVector {
                owner_kind: "memory".into(),
                scope_key: "facts".into(),
                ref_key: "b".into(),
                chunk_ix: 0,
                text: "y".into(),
                model: "m1".into(),
                dim: 1,
                vec: vec![1.0],
                mtime: None,
            },
        ])
        .unwrap();
        db.delete_vectors_for_ref("memory", "facts", "a").unwrap();
        let ScopeSearch::Hits(hits) = db.search_vectors("memory", "facts", "m1", 1, &[1.0], 10).unwrap() else {
            panic!("expected hits");
        };
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].ref_key, "b");
    }

    #[test]
    fn delete_for_scope_drops_everything_in_it() {
        let db = Db::open_in_memory().unwrap();
        db.insert_vectors(&[NewVector {
            owner_kind: "file".into(),
            scope_key: "/docs".into(),
            ref_key: "a.md".into(),
            chunk_ix: 0,
            text: "x".into(),
            model: "m1".into(),
            dim: 1,
            vec: vec![1.0],
            mtime: None,
        }])
        .unwrap();
        db.delete_vectors_for_scope("file", "/docs").unwrap();
        let ScopeSearch::Hits(hits) = db.search_vectors("file", "/docs", "m1", 1, &[1.0], 10).unwrap() else {
            panic!("expected hits");
        };
        assert!(hits.is_empty());
    }

    #[test]
    fn ref_keys_are_scoped_by_owner_scope_and_model() {
        let db = Db::open_in_memory().unwrap();
        db.insert_vectors(&[
            NewVector {
                owner_kind: "memory".into(),
                scope_key: "facts".into(),
                ref_key: "a".into(),
                chunk_ix: 0,
                text: "x".into(),
                model: "m1".into(),
                dim: 1,
                vec: vec![1.0],
                mtime: None,
            },
            NewVector {
                owner_kind: "memory".into(),
                scope_key: "lessons".into(),
                ref_key: "b".into(),
                chunk_ix: 0,
                text: "y".into(),
                model: "m1".into(),
                dim: 1,
                vec: vec![1.0],
                mtime: None,
            },
        ])
        .unwrap();
        let facts = db.vector_ref_keys_for_scope("memory", "facts", "m1").unwrap();
        assert_eq!(facts, std::collections::HashSet::from(["a".to_string()]));
        // A different model sees nothing — exactly the state right after a
        // model switch, before backfill has re-embedded anything.
        assert!(db.vector_ref_keys_for_scope("memory", "facts", "m2").unwrap().is_empty());
    }

    #[test]
    fn file_centroids_average_a_files_own_chunks_and_come_out_unit_length() {
        let db = Db::open_in_memory().unwrap();
        db.insert_vectors(&[
            NewVector {
                owner_kind: "file".into(),
                scope_key: "/docs".into(),
                ref_key: "a.md".into(),
                chunk_ix: 0,
                text: "x".into(),
                model: "m1".into(),
                dim: 2,
                vec: vec![1.0, 0.0],
                mtime: None,
            },
            NewVector {
                owner_kind: "file".into(),
                scope_key: "/docs".into(),
                ref_key: "a.md".into(),
                chunk_ix: 1,
                text: "y".into(),
                model: "m1".into(),
                dim: 2,
                vec: vec![0.0, 1.0],
                mtime: None,
            },
        ])
        .unwrap();
        let centroids = db.file_centroids("/docs").unwrap();
        assert_eq!(centroids.len(), 1);
        let (ref_key, v) = &centroids[0];
        assert_eq!(ref_key, "a.md");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "a centroid must come out unit-length");
        // The mean of (1,0) and (0,1), normalised, is the 45° unit vector.
        assert!((v[0] - FRAC_1_SQRT_2).abs() < 1e-3);
        assert!((v[1] - FRAC_1_SQRT_2).abs() < 1e-3);
    }

    #[test]
    fn file_centroids_only_covers_the_given_scope() {
        let db = Db::open_in_memory().unwrap();
        db.insert_vectors(&[NewVector {
            owner_kind: "file".into(),
            scope_key: "/other".into(),
            ref_key: "b.md".into(),
            chunk_ix: 0,
            text: "z".into(),
            model: "m1".into(),
            dim: 2,
            vec: vec![1.0, 0.0],
            mtime: None,
        }])
        .unwrap();
        assert!(db.file_centroids("/docs").unwrap().is_empty());
    }
}
