use std::path::{Path, PathBuf};

use crate::adapters::sqlite_cache::CacheDb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheDbKind {
    Global,
    LocalPerTarget,
}

/// Windows: %LOCALAPPDATA%\SameFileFinder\cache.sqlite3
/// Other OS: $XDG_CACHE_HOME/SameFileFinder/cache.sqlite3 or ~/.cache/SameFileFinder/cache.sqlite3
pub fn global_cache_db_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(base).join("SameFileFinder").join("cache.sqlite3"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(base) = std::env::var_os("XDG_CACHE_HOME") {
            return Some(PathBuf::from(base).join("SameFileFinder").join("cache.sqlite3"));
        }
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".cache")
                .join("SameFileFinder")
                .join("cache.sqlite3"),
        )
    }
}

pub fn local_cache_db_path(target_root: &Path) -> PathBuf {
    target_root.join(".samefile_finder_cache.sqlite3")
}

/// Resolve which DB path to use.
///
/// v2.3.0: Prefer global DB to share cache across different targets.
/// Compatibility: if a per-target DB exists, we auto-merge it into the global DB (best-effort).
pub fn resolve_cache_db_path(target_root: &Path) -> (PathBuf, CacheDbKind) {
    let local = local_cache_db_path(target_root);
    let Some(global) = global_cache_db_path() else {
        return (local, CacheDbKind::LocalPerTarget);
    };

    // Best-effort migration: local -> global
    if local.exists() {
        if let Ok(global_db) = CacheDb::open(&global) {
            let _ = global_db.merge_from_db(&local);
        }
    }

    (global, CacheDbKind::Global)
}