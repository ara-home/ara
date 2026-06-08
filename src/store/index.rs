use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("migration error: {0}")]
    Migration(String),
}

pub struct StoreIndex {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl StoreIndex {
    pub fn new(db_path: PathBuf) -> Result<Self, IndexError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
        )?;

        let index = Self {
            conn: Mutex::new(conn),
            path: db_path,
        };
        index.ensure_schema()?;

        let legacy = index.path.with_file_name("index.json");
        if legacy.exists() {
            index.migrate_from_json(&legacy)?;
        }

        Ok(index)
    }

    fn ensure_schema(&self) -> Result<(), IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS objects (
                cache_key   TEXT PRIMARY KEY,
                hash        TEXT NOT NULL,
                source      TEXT NOT NULL DEFAULT '',
                inserted_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_accessed TEXT,
                refcount    INTEGER NOT NULL DEFAULT 1,
                size        INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_objects_hash ON objects(hash);

            CREATE TABLE IF NOT EXISTS extracted (
                hash         TEXT PRIMARY KEY,
                refcount     INTEGER NOT NULL DEFAULT 1,
                extracted_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS store_meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    fn migrate_from_json(&self, legacy_path: &Path) -> Result<(), IndexError> {
        let content = std::fs::read_to_string(legacy_path).map_err(|e| {
            IndexError::Migration(format!("failed to read {}: {e}", legacy_path.display()))
        })?;

        let map: std::collections::HashMap<String, String> = serde_json::from_str(&content)
            .map_err(|e| {
                IndexError::Migration(format!(
                    "corrupt legacy index {}: {e}. Delete this file to recover.",
                    legacy_path.display()
                ))
            })?;

        if map.is_empty() {
            let bak = legacy_path.with_extension("json.empty");
            std::fs::rename(legacy_path, &bak)?;
            return Ok(());
        }

        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;

        for (cache_key, hash) in &map {
            let source = cache_key.split(':').next().unwrap_or("unknown").to_string();
            tx.execute(
                "INSERT OR IGNORE INTO objects (cache_key, hash, source, refcount, size)
                 VALUES (?1, ?2, ?3, 1, 0)",
                rusqlite::params![cache_key, hash, source],
            )?;
        }

        tx.commit()?;

        let bak = legacy_path.with_extension("json.migrated");
        std::fs::rename(legacy_path, &bak)?;

        Ok(())
    }

    pub fn lookup(&self, cache_key: &str) -> Result<Option<String>, IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare_cached("SELECT hash FROM objects WHERE cache_key = ?1")?;
        let mut rows = stmt.query(rusqlite::params![cache_key])?;
        match rows.next()? {
            Some(row) => {
                let hash: String = row.get(0)?;
                Ok(Some(hash))
            }
            None => Ok(None),
        }
    }

    pub fn insert(
        &self,
        cache_key: &str,
        hash: &str,
        source: &str,
        size: i64,
    ) -> Result<(), IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO objects (cache_key, hash, source, refcount, size)
             VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT(cache_key) DO UPDATE SET
                 hash = excluded.hash,
                 refcount = refcount + 1,
                 last_accessed = datetime('now')",
            rusqlite::params![cache_key, hash, source, size],
        )?;
        Ok(())
    }

    pub fn remove(&self, cache_key: &str) -> Result<(), IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE objects SET refcount = refcount - 1 WHERE cache_key = ?1",
            rusqlite::params![cache_key],
        )?;
        Ok(())
    }

    pub fn get_active_hashes(&self) -> Result<HashSet<String>, IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare_cached("SELECT DISTINCT hash FROM objects WHERE refcount > 0")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut hashes = HashSet::new();
        for row in rows {
            hashes.insert(row?);
        }
        Ok(hashes)
    }

    pub fn get_orphan_hashes(&self) -> Result<HashSet<String>, IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare_cached("SELECT DISTINCT hash FROM objects WHERE refcount <= 0")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut hashes = HashSet::new();
        for row in rows {
            hashes.insert(row?);
        }
        Ok(hashes)
    }

    pub fn get_all_hashes(&self) -> Result<HashSet<String>, IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare_cached("SELECT DISTINCT hash FROM objects")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut hashes = HashSet::new();
        for row in rows {
            hashes.insert(row?);
        }
        Ok(hashes)
    }

    pub fn increment_extracted(&self, hash: &str) -> Result<(), IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO extracted (hash, refcount)
             VALUES (?1, 1)
             ON CONFLICT(hash) DO UPDATE SET refcount = refcount + 1",
            rusqlite::params![hash],
        )?;
        Ok(())
    }

    pub fn decrement_extracted(&self, hash: &str) -> Result<(), IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE extracted SET refcount = refcount - 1 WHERE hash = ?1",
            rusqlite::params![hash],
        )?;
        Ok(())
    }

    pub fn get_active_extracted_hashes(&self) -> Result<HashSet<String>, IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare_cached("SELECT hash FROM extracted WHERE refcount > 0")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut hashes = HashSet::new();
        for row in rows {
            hashes.insert(row?);
        }
        Ok(hashes)
    }

    pub fn get_orphan_extracted_hashes(&self) -> Result<HashSet<String>, IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare_cached("SELECT hash FROM extracted WHERE refcount <= 0")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut hashes = HashSet::new();
        for row in rows {
            hashes.insert(row?);
        }
        Ok(hashes)
    }

    /// Batch insert multiple cache entries in a single transaction.
    /// Eliminates lock contention by replacing N individual insert() calls
    /// with one bulk operation.
    pub fn batch_insert(
        &self,
        entries: &[(String, String, String, i64)],
    ) -> Result<(), IndexError> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        for (cache_key, hash, source, size) in entries {
            tx.execute(
                "INSERT INTO objects (cache_key, hash, source, refcount, size)
                 VALUES (?1, ?2, ?3, 1, ?4)
                 ON CONFLICT(cache_key) DO UPDATE SET
                     hash = excluded.hash,
                     refcount = refcount + 1,
                     last_accessed = datetime('now')",
                rusqlite::params![cache_key, hash, source, size],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn clean_orphan_entries(&self) -> Result<(u64, u64), IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let removed_objects = conn.execute("DELETE FROM objects WHERE refcount <= 0", [])? as u64;
        let removed_extracted =
            conn.execute("DELETE FROM extracted WHERE refcount <= 0", [])? as u64;
        Ok((removed_objects, removed_extracted))
    }

    pub fn entry_count(&self) -> Result<u64, IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let count: u64 = conn.query_row("SELECT COUNT(*) FROM objects", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn total_refcount(&self) -> Result<i64, IndexError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let total: i64 = conn.query_row(
            "SELECT COALESCE(SUM(refcount), 0) FROM objects",
            [],
            |row| row.get(0),
        )?;
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, StoreIndex) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let db_path = dir.path().join("index.db");
        let index = StoreIndex::new(db_path).unwrap();
        (dir, index)
    }

    #[test]
    fn test_lookup_empty() {
        let (_dir, index) = setup();
        let result = index.lookup("npm:zod@3.23.8").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_insert_and_lookup() {
        let (_dir, index) = setup();
        index
            .insert("npm:zod@3.23.8", "sha256-abc123", "npm", 1024)
            .unwrap();
        let hash = index.lookup("npm:zod@3.23.8").unwrap();
        assert_eq!(hash, Some("sha256-abc123".to_string()));
    }

    #[test]
    fn test_refcount_increment_on_reinsert() {
        let (_dir, index) = setup();
        index
            .insert("pkg@1.0.0", "sha256-hash1", "npm", 100)
            .unwrap();
        index
            .insert("pkg@1.0.0", "sha256-hash1", "npm", 100)
            .unwrap();
        let total = index.total_refcount().unwrap();
        assert_eq!(total, 2);
    }

    #[test]
    fn test_remove_decrements_refcount() {
        let (_dir, index) = setup();
        index
            .insert("pkg@1.0.0", "sha256-hash1", "npm", 100)
            .unwrap();
        index
            .insert("pkg@1.0.0", "sha256-hash1", "npm", 100)
            .unwrap();
        index.remove("pkg@1.0.0").unwrap();
        let total = index.total_refcount().unwrap();
        assert_eq!(total, 1);
    }

    #[test]
    fn test_get_active_hashes() {
        let (_dir, index) = setup();
        index.insert("a@1", "sha256-a", "npm", 10).unwrap();
        index.insert("b@1", "sha256-b", "npm", 10).unwrap();
        index.remove("a@1").unwrap();

        let active = index.get_active_hashes().unwrap();
        assert!(!active.contains("sha256-a"));
        assert!(active.contains("sha256-b"));
    }

    #[test]
    fn test_clean_orphan_entries() {
        let (_dir, index) = setup();
        index.insert("a@1", "sha256-a", "npm", 10).unwrap();
        index.insert("b@1", "sha256-b", "npm", 10).unwrap();
        index.remove("a@1").unwrap();

        let (removed_objects, removed_extracted) = index.clean_orphan_entries().unwrap();
        assert_eq!(removed_objects, 1);
        assert_eq!(removed_extracted, 0);
    }

    #[test]
    fn test_extracted_refcount() {
        let (_dir, index) = setup();
        index.increment_extracted("sha256-ext1").unwrap();
        index.increment_extracted("sha256-ext1").unwrap();
        index.decrement_extracted("sha256-ext1").unwrap();

        let active = index.get_active_extracted_hashes().unwrap();
        assert!(active.contains("sha256-ext1"));
    }

    #[test]
    fn test_extracted_orphan() {
        let (_dir, index) = setup();
        index.increment_extracted("sha256-orphan").unwrap();
        index.decrement_extracted("sha256-orphan").unwrap();

        let orphans = index.get_orphan_extracted_hashes().unwrap();
        assert!(orphans.contains("sha256-orphan"));
    }

    #[test]
    fn test_migration_from_json() {
        let dir = TempDir::new().expect("failed to create temp dir");

        let json_path = dir.path().join("index.json");
        let mut old_index = std::collections::HashMap::new();
        old_index.insert("npm:react@18.3.1".to_string(), "sha256-react".to_string());
        old_index.insert("npm:zod@3.23.8".to_string(), "sha256-zod".to_string());
        std::fs::write(&json_path, serde_json::to_string(&old_index).unwrap()).unwrap();

        let db_path = dir.path().join("index.db");
        assert!(json_path.exists());

        let index = StoreIndex::new(db_path).unwrap();

        assert!(!json_path.exists());

        assert_eq!(
            index.lookup("npm:react@18.3.1").unwrap(),
            Some("sha256-react".to_string())
        );
        assert_eq!(
            index.lookup("npm:zod@3.23.8").unwrap(),
            Some("sha256-zod".to_string())
        );
    }
}
