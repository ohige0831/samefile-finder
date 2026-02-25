use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::adapters::sqlite_cache::CacheDb;
use crate::core::types::{DuplicateGroup, FileEntry, HashStats};

pub struct HashStageResult {
    pub duplicate_groups: Vec<DuplicateGroup>,
    pub stats: HashStats,
}

pub fn find_duplicate_groups_by_hash<F>(
    candidates: &[FileEntry],
    cancel_flag: &AtomicBool,
    cache_db_path: &Path,
    mut on_hashing: F,
) -> Result<HashStageResult, String>
where
    F: FnMut(&Path, usize, usize),
{
    let mut by_key: HashMap<(u64, String), Vec<&FileEntry>> = HashMap::new();
    let total = candidates.len();
    let mut stats = HashStats {
        total_inputs: total,
        ..Default::default()
    };

    let cache_db = CacheDb::open(cache_db_path).ok();

    for (idx, file) in candidates.iter().enumerate() {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err("__CANCELED__".to_string());
        }

        on_hashing(&file.path, idx + 1, total);

        let hash_hex = if let Some(db) = cache_db.as_ref() {
            match db.get_reusable_full_hash(&file.path, file.size_bytes, file.mtime_ns) {
                Ok(Some(cached_hash)) => {
                    stats.cache_hits += 1;
                    cached_hash
                }
                Ok(None) => {
                    stats.cache_misses += 1;
                    stats.computed += 1;
                    let h = compute_md5_hex(file, cancel_flag)?;
                    let _ = db.upsert_full_hash(&file.path, file.size_bytes, file.mtime_ns, &h);
                    h
                }
                Err(_) => {
                    stats.cache_misses += 1;
                    stats.computed += 1;
                    compute_md5_hex(file, cancel_flag)?
                }
            }
        } else {
            stats.cache_misses += 1;
            stats.computed += 1;
            compute_md5_hex(file, cancel_flag)?
        };

        by_key
            .entry((file.size_bytes, hash_hex))
            .or_default()
            .push(file);
    }

    let mut groups: Vec<DuplicateGroup> = Vec::new();
    for ((size_bytes, hash_hex), files) in by_key {
        if files.len() >= 2 {
            groups.push(DuplicateGroup {
                hash_hex,
                file_size_bytes: size_bytes,
                files: files.iter().map(|f| f.path.clone()).collect(),
            });
        }
    }

    groups.sort_by(|a, b| {
        b.files
            .len()
            .cmp(&a.files.len())
            .then_with(|| b.file_size_bytes.cmp(&a.file_size_bytes))
    });

    Ok(HashStageResult {
        duplicate_groups: groups,
        stats,
    })
}

fn compute_md5_hex(file: &FileEntry, cancel_flag: &AtomicBool) -> Result<String, String> {
    let f = File::open(&file.path)
        .map_err(|e| format!("Failed to open file {}: {}", file.path.display(), e))?;

    let mut reader = BufReader::new(f);
    let mut context = md5::Context::new();
    let mut buffer = [0u8; 1024 * 1024];

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err("__CANCELED__".to_string());
        }

        let n = reader
            .read(&mut buffer)
            .map_err(|e| format!("Failed to read file {}: {}", file.path.display(), e))?;
        if n == 0 {
            break;
        }
        context.consume(&buffer[..n]);
    }

    let digest = context.compute();
    Ok(format!("{:x}", digest))
}
