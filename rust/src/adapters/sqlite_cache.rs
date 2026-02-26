use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct CacheRecord {
    pub path: String,
    pub size_bytes: u64,
    pub mtime_ns: i64,
    pub full_hash: Option<String>,
    pub fingerprint: Option<Vec<u8>>,
}

pub struct CacheDb {
    conn: Connection,
}

impl CacheDb {
    pub fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Failed to create cache DB directory {}: {}",
                    parent.display(),
                    e
                )
            })?;
        }

        let conn = Connection::open(db_path)
            .map_err(|e| format!("Failed to open cache DB {}: {}", db_path.display(), e))?;

        // DBロック耐性: read/writeがぶつかっても短時間は待つ
        // (rusqlite::Connection::busy_timeout exists, but keep it PRAGMA-based for simplicity)
        let _ = conn.execute_batch("PRAGMA busy_timeout = 5000;");

        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                r#"
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = NORMAL;
                PRAGMA busy_timeout = 5000;

                CREATE TABLE IF NOT EXISTS file_cache (
                    path TEXT PRIMARY KEY,
                    size_bytes INTEGER NOT NULL,
                    mtime_ns INTEGER NOT NULL,
                    fingerprint BLOB,
                    full_hash TEXT,
                    hash_algo TEXT NOT NULL DEFAULT 'md5',
                    updated_at_unix INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_file_cache_size
                    ON file_cache(size_bytes);

                CREATE INDEX IF NOT EXISTS idx_file_cache_mtime
                    ON file_cache(mtime_ns);
                "#,
            )
            .map_err(|e| format!("Failed to initialize cache schema: {}", e))?;

        Ok(())
    }

    pub fn get_record(&self, path: &Path) -> Result<Option<CacheRecord>, String> {
        let path_str = path.to_string_lossy().to_string();

        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT path, size_bytes, mtime_ns, fingerprint, full_hash
                FROM file_cache
                WHERE path = ?1
                "#,
            )
            .map_err(|e| format!("Failed to prepare cache query: {}", e))?;

        let row = stmt
            .query_row(params![path_str], |row| {
                let path: String = row.get(0)?;
                let size_i64: i64 = row.get(1)?;
                let mtime_ns: i64 = row.get(2)?;
                let fingerprint: Option<Vec<u8>> = row.get(3)?;
                let full_hash: Option<String> = row.get(4)?;

                Ok(CacheRecord {
                    path,
                    size_bytes: size_i64.max(0) as u64,
                    mtime_ns,
                    full_hash,
                    fingerprint,
                })
            })
            .optional()
            .map_err(|e| format!("Failed to read cache row: {}", e))?;

        Ok(row)
    }

    pub fn get_reusable_fingerprint(
        &self,
        path: &Path,
        size_bytes: u64,
        mtime_ns: i64,
    ) -> Result<Option<Vec<u8>>, String> {
        let Some(rec) = self.get_record(path)? else {
            return Ok(None);
        };

        if rec.size_bytes == size_bytes && rec.mtime_ns == mtime_ns {
            return Ok(rec.fingerprint);
        }

        Ok(None)
    }

    pub fn get_reusable_full_hash(
        &self,
        path: &Path,
        size_bytes: u64,
        mtime_ns: i64,
    ) -> Result<Option<String>, String> {
        let Some(rec) = self.get_record(path)? else {
            return Ok(None);
        };

        if rec.size_bytes == size_bytes && rec.mtime_ns == mtime_ns {
            return Ok(rec.full_hash);
        }

        Ok(None)
    }

    pub fn upsert_fingerprint(
        &self,
        path: &Path,
        size_bytes: u64,
        mtime_ns: i64,
        fingerprint: &[u8],
    ) -> Result<(), String> {
        let path_str = path.to_string_lossy().to_string();
        let now_unix = now_unix();

        self.conn
            .execute(
                r#"
                INSERT INTO file_cache (
                    path, size_bytes, mtime_ns, fingerprint, full_hash, hash_algo, updated_at_unix
                )
                VALUES (?1, ?2, ?3, ?4, NULL, 'md5', ?5)
                ON CONFLICT(path) DO UPDATE SET
                    size_bytes = excluded.size_bytes,
                    mtime_ns = excluded.mtime_ns,
                    fingerprint = excluded.fingerprint,
                    updated_at_unix = excluded.updated_at_unix
                "#,
                params![path_str, size_bytes as i64, mtime_ns, fingerprint, now_unix],
            )
            .map_err(|e| format!("Failed to upsert fingerprint cache row: {}", e))?;

        Ok(())
    }

    pub fn upsert_full_hash(
        &self,
        path: &Path,
        size_bytes: u64,
        mtime_ns: i64,
        full_hash: &str,
    ) -> Result<(), String> {
        let path_str = path.to_string_lossy().to_string();
        let now_unix = now_unix();

        self.conn
            .execute(
                r#"
                INSERT INTO file_cache (
                    path, size_bytes, mtime_ns, fingerprint, full_hash, hash_algo, updated_at_unix
                )
                VALUES (?1, ?2, ?3, NULL, ?4, 'md5', ?5)
                ON CONFLICT(path) DO UPDATE SET
                    size_bytes = excluded.size_bytes,
                    mtime_ns = excluded.mtime_ns,
                    full_hash = excluded.full_hash,
                    hash_algo = excluded.hash_algo,
                    updated_at_unix = excluded.updated_at_unix
                "#,
                params![path_str, size_bytes as i64, mtime_ns, full_hash, now_unix],
            )
            .map_err(|e| format!("Failed to upsert full-hash cache row: {}", e))?;

        Ok(())
    }

    /// Return number of cached entries.
    pub fn count_entries(&self) -> Result<u64, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(1) FROM file_cache")
            .map_err(|e| format!("Failed to prepare count query: {}", e))?;
        let n: i64 = stmt
            .query_row([], |row| row.get(0))
            .map_err(|e| format!("Failed to run count query: {}", e))?;
        Ok(n.max(0) as u64)
    }

    /// Migrate rows from another DB (same schema) into this DB.
    /// This keeps the source DB intact.
    pub fn merge_from_db(&self, other_db_path: &Path) -> Result<u64, String> {
        let other = other_db_path.to_string_lossy().to_string();

        self.conn
            .execute("ATTACH DATABASE ?1 AS otherdb", params![other])
            .map_err(|e| format!("Failed to attach DB for merge: {}", e))?;

        let copied = self
            .conn
            .execute(
                r#"
                INSERT OR REPLACE INTO file_cache(
                    path, size_bytes, mtime_ns, fingerprint, full_hash, hash_algo, updated_at_unix
                )
                SELECT
                    path, size_bytes, mtime_ns, fingerprint, full_hash, hash_algo, updated_at_unix
                FROM otherdb.file_cache
                "#,
                [],
            )
            .map_err(|e| format!("Failed to merge cache rows: {}", e))?;

        let _ = self.conn.execute("DETACH DATABASE otherdb", []);

        Ok(copied as u64)
    }

    /// GC: delete rows whose path no longer exists.
    pub fn gc_missing_paths(&self) -> Result<u64, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM file_cache")
            .map_err(|e| format!("Failed to prepare GC scan: {}", e))?;

        let iter = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Failed to query GC scan: {}", e))?;

        let mut removed: u64 = 0;
        for item in iter {
            let path_str = item.map_err(|e| format!("Failed to read GC row: {}", e))?;
            let p = Path::new(&path_str);
            if !p.exists() {
                self.conn
                    .execute("DELETE FROM file_cache WHERE path = ?1", params![path_str])
                    .map_err(|e| format!("Failed to delete GC row: {}", e))?;
                removed += 1;
            }
        }

        Ok(removed)
    }

    /// Optional GC: delete rows not updated in the last `days` days.
    pub fn gc_older_than_days(&self, days: u64) -> Result<u64, String> {
        let now = now_unix();
        let threshold = now - (days as i64) * 86400;
        let removed = self
            .conn
            .execute(
                "DELETE FROM file_cache WHERE updated_at_unix < ?1",
                params![threshold],
            )
            .map_err(|e| format!("Failed to delete old cache rows: {}", e))?;
        Ok(removed as u64)
    }

    /// Manual VACUUM.
    pub fn vacuum(&self) -> Result<(), String> {
        self.conn
            .execute_batch("VACUUM;")
            .map_err(|e| format!("Failed to VACUUM cache DB: {}", e))?;
        Ok(())
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}