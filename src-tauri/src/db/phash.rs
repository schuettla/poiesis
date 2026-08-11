//! Perception (PHS-1): cached perceptual hashes, keyed by path + mtime so an
//! unchanged file is never rehashed on a repeat duplicate scan.

use rusqlite::{params, OptionalExtension};

use super::{now_ms, Db, DbError};

impl Db {
    /// The cached `(mtime, hash)` for a path, if any. The caller decides
    /// whether the cached `mtime` still matches the file on disk — this is a
    /// plain lookup, not a freshness check.
    pub fn get_image_hash(&self, path: &str) -> Result<Option<(i64, u64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT mtime, hash FROM image_hashes WHERE path = ?1",
            [path],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u64)),
        )
        .optional()
        .map_err(Into::into)
    }

    /// Store (or refresh) one path's hash. `hash` is a bit pattern, not a
    /// count — stored via its `i64` reinterpretation so a value with the top
    /// bit set round-trips exactly rather than overflowing.
    pub fn set_image_hash(&self, path: &str, mtime: i64, hash: u64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO image_hashes(path, mtime, hash, updated_at) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET mtime = excluded.mtime, hash = excluded.hash, updated_at = excluded.updated_at",
            params![path, mtime, hash as i64, now_ms()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_never_hashed_reads_as_absent() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.get_image_hash("/a.png").unwrap().is_none());
    }

    #[test]
    fn set_then_get_round_trips_including_the_top_bit() {
        let db = Db::open_in_memory().unwrap();
        let hash: u64 = 0xFFFF_FFFF_FFFF_FFFF;
        db.set_image_hash("/a.png", 100, hash).unwrap();
        assert_eq!(db.get_image_hash("/a.png").unwrap(), Some((100, hash)));
    }

    #[test]
    fn a_second_write_overwrites_rather_than_duplicating() {
        let db = Db::open_in_memory().unwrap();
        db.set_image_hash("/a.png", 100, 1).unwrap();
        db.set_image_hash("/a.png", 200, 2).unwrap();
        assert_eq!(db.get_image_hash("/a.png").unwrap(), Some((200, 2)));
    }
}
