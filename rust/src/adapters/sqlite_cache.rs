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
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
