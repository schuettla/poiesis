//! Perception (IDX): one row per indexed folder root — what built it, how it
//! went, and when. `skipped` is stored as raw JSON here (mirroring
//! `overrides_json`/`params_json` elsewhere in this file); typed access lives
//! in `agent::index`, which is the only place that needs to interpret it.

use rusqlite::{params, OptionalExtension, Row};

use super::{now_ms, Db, DbError};

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexRoot {
    pub path: String,
    pub model: String,
    pub dim: i64,
    pub file_count: i64,
    pub chunk_count: i64,
    /// JSON: `[{path, reason}]` — files a build couldn't read (IDX-UI-2).
    pub skipped: Option<String>,
    /// idle|building|stale|error.
    pub state: String,
    pub updated_at: i64,
}

fn map_index_root(r: &Row) -> rusqlite::Result<IndexRoot> {
    Ok(IndexRoot {
        path: r.get(0)?,
        model: r.get(1)?,
        dim: r.get(2)?,
        file_count: r.get(3)?,
        chunk_count: r.get(4)?,
        skipped: r.get(5)?,
        state: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

const COLUMNS: &str = "path, model, dim, file_count, chunk_count, skipped, state, updated_at";

impl Db {
    pub fn get_index_root(&self, path: &str) -> Result<Option<IndexRoot>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM index_roots WHERE path = ?1"),
            [path],
            map_index_root,
        )
        .optional()
        .map_err(Into::into)
    }

    /// For `IDX-UI-4`'s Settings list, newest build first.
    pub fn list_index_roots(&self) -> Result<Vec<IndexRoot>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare(&format!("SELECT {COLUMNS} FROM index_roots ORDER BY updated_at DESC"))?;
        let rows = stmt.query_map([], map_index_root)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Mark a root `building` right before a build starts (IDX-UI-1), whether
    /// this is its first build or a rebuild. Existing counts/skips are left in
    /// place until `set_index_root_result` overwrites them, so a crash mid-build
    /// doesn't erase the last successful build's numbers — only its state goes
    /// stale-looking (`building` with no matching progress) until retried.
    pub fn set_index_root_building(&self, path: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO index_roots(path, model, dim, file_count, chunk_count, skipped, state, updated_at)
             VALUES(?1, '', 0, 0, 0, NULL, 'building', ?2)
             ON CONFLICT(path) DO UPDATE SET state = 'building', updated_at = excluded.updated_at",
            params![path, ts],
        )?;
        Ok(())
    }

    /// Record how a build ended — success (`idle`) or `error`.
    #[allow(clippy::too_many_arguments)]
    pub fn set_index_root_result(
        &self,
        path: &str,
        model: &str,
        dim: i64,
        file_count: i64,
        chunk_count: i64,
        skipped_json: Option<&str>,
        state: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let ts = now_ms();
        conn.execute(
            "INSERT INTO index_roots(path, model, dim, file_count, chunk_count, skipped, state, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(path) DO UPDATE SET
               model = excluded.model, dim = excluded.dim, file_count = excluded.file_count,
               chunk_count = excluded.chunk_count, skipped = excluded.skipped,
               state = excluded.state, updated_at = excluded.updated_at",
            params![path, model, dim, file_count, chunk_count, skipped_json, state, ts],
        )?;
        Ok(())
    }

    /// Flip only `state`, leaving the last build's counts/skips untouched —
    /// used to revert a `building` row back to `idle` after a cancel or an
    /// error, without losing what the previous successful build found.
    pub fn set_index_root_state(&self, path: &str, state: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE index_roots SET state = ?2, updated_at = ?3 WHERE path = ?1",
            params![path, state, now_ms()],
        )?;
        Ok(())
    }

    /// `IDX-UI-4`'s "Forget this folder": drop the root and every chunk it
    /// produced. Both in one connection lock so a reader never sees the row
    /// gone with its vectors still lingering (or the reverse).
    pub fn forget_index_root(&self, path: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM index_roots WHERE path = ?1", [path])?;
        conn.execute(
            "DELETE FROM vectors WHERE owner_kind = 'file' AND scope_key = ?1",
            [path],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_with_no_row_is_never_built() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.get_index_root("/docs").unwrap().is_none());
    }

    #[test]
    fn building_then_result_round_trips() {
        let db = Db::open_in_memory().unwrap();
        db.set_index_root_building("/docs").unwrap();
        let mid = db.get_index_root("/docs").unwrap().unwrap();
        assert_eq!(mid.state, "building");

        db.set_index_root_result("/docs", "bge-small", 384, 12, 40, Some(r#"[{"path":"a.png","reason":"needs my eyes"}]"#), "idle")
            .unwrap();
        let done = db.get_index_root("/docs").unwrap().unwrap();
        assert_eq!(done.state, "idle");
        assert_eq!(done.file_count, 12);
        assert_eq!(done.chunk_count, 40);
        assert!(done.skipped.unwrap().contains("needs my eyes"));
    }

    #[test]
    fn forgetting_drops_the_row_and_its_vectors() {
        use crate::db::vectors::NewVector;
        let db = Db::open_in_memory().unwrap();
        db.set_index_root_result("/docs", "m", 2, 1, 1, None, "idle").unwrap();
        db.insert_vectors(&[NewVector {
            owner_kind: "file".into(),
            scope_key: "/docs".into(),
            ref_key: "/docs/a.md".into(),
            chunk_ix: 0,
            text: "x".into(),
            model: "m".into(),
            dim: 2,
            vec: vec![1.0, 0.0],
            mtime: Some(1),
        }])
        .unwrap();

        db.forget_index_root("/docs").unwrap();
        assert!(db.get_index_root("/docs").unwrap().is_none());
        let hits = db.file_mtimes_for_scope("/docs").unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn listing_orders_newest_first() {
        let db = Db::open_in_memory().unwrap();
        db.set_index_root_result("/a", "m", 1, 1, 1, None, "idle").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.set_index_root_result("/b", "m", 1, 1, 1, None, "idle").unwrap();
        let all = db.list_index_roots().unwrap();
        assert_eq!(all[0].path, "/b");
        assert_eq!(all[1].path, "/a");
    }
}
