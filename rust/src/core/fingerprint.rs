use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use md5::Context;

use crate::adapters::sqlite_cache::CacheDb;
use crate::core::types::{FileEntry, FingerprintStats};

const FP_CHUNK_SIZE: usize = 64 * 1024; // 64 KiB
const FP_SMALL_FILE_THRESHOLD: u64 = (FP_CHUNK_SIZE as u64) * 2; // 128 KiB

pub struct FingerprintStageResult {
    pub candidates: Vec<FileEntry>,
    pub stats: FingerprintStats,
}

pub fn build_fingerprint_candidates<F>(
    size_candidates: &[FileEntry],
    cancel_flag: &AtomicBool,
    cache_db_path: &Path,
    mut on_fingerprinting: F,
) -> Result<FingerprintStageResult, String>
where
    F: FnMut(&Path, usize, usize),
{
    let mut by_fp: HashMap<(u64, Vec<u8>), Vec<FileEntry>> = HashMap::new();
    let mut stats = FingerprintStats {
        total_inputs: size_candidates.len(),
        ..Default::default()
    };

    let cache_db = CacheDb::open(cache_db_path).ok();
    let total = size_candidates.len();

    for (idx, file) in size_candidates.iter().enumerate() {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err("__CANCELED__".to_string());
        }

        on_fingerprinting(&file.path, idx + 1, total);

        let fp = if let Some(db) = cache_db.as_ref() {
            match db.get_reusable_fingerprint(&file.path, file.size_bytes, file.mtime_ns) {
                Ok(Some(cached_fp)) => {
                    stats.cache_hits += 1;
                    cached_fp
                }
                Ok(None) => {
                    stats.cache_misses += 1;
                    stats.computed += 1;
                    let fresh = compute_fingerprint(file, cancel_flag)?;
                    let _ = db.upsert_fingerprint(&file.path, file.size_bytes, file.mtime_ns, &fresh);
                    fresh
                }
                Err(_) => {
                    stats.cache_misses += 1;
                    stats.computed += 1;
                    compute_fingerprint(file, cancel_flag)?
                }
            }
        } else {
            stats.cache_misses += 1;
            stats.computed += 1;
            compute_fingerprint(file, cancel_flag)?
        };

        by_fp.entry((file.size_bytes, fp)).or_default().push(file.clone());
    }

    let mut narrowed: Vec<FileEntry> = Vec::new();
    for group in by_fp.into_values() {
        if group.len() >= 2 {
            narrowed.extend(group);
        }
    }

    stats.narrowed_outputs = narrowed.len();

    Ok(FingerprintStageResult {
        candidates: narrowed,
        stats,
    })
}

fn compute_fingerprint(file: &FileEntry, cancel_flag: &AtomicBool) -> Result<Vec<u8>, String> {
    if cancel_flag.load(Ordering::Relaxed) {
        return Err("__CANCELED__".to_string());
    }

    let mut f = File::open(&file.path)
        .map_err(|e| format!("Failed to open file for fingerprint {}: {}", file.path.display(), e))?;

    let mut ctx = Context::new();
    ctx.consume(&file.size_bytes.to_le_bytes());

    if file.size_bytes <= FP_SMALL_FILE_THRESHOLD {
        let mut buf = [0u8; 64 * 1024];
        loop {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err("__CANCELED__".to_string());
            }
            let n = f
                .read(&mut buf)
                .map_err(|e| format!("Failed to read small file {}: {}", file.path.display(), e))?;
            if n == 0 {
                break;
            }
            ctx.consume(&buf[..n]);
        }
    } else {
        let mut head = vec![0u8; FP_CHUNK_SIZE];
        f.read_exact(&mut head)
            .map_err(|e| format!("Failed to read head fingerprint {}: {}", file.path.display(), e))?;
        ctx.consume(&head);

        if cancel_flag.load(Ordering::Relaxed) {
            return Err("__CANCELED__".to_string());
        }

        let tail_pos = file.size_bytes.saturating_sub(FP_CHUNK_SIZE as u64);
        f.seek(SeekFrom::Start(tail_pos))
            .map_err(|e| format!("Failed to seek tail fingerprint {}: {}", file.path.display(), e))?;
        let mut tail = vec![0u8; FP_CHUNK_SIZE];
        f.read_exact(&mut tail)
            .map_err(|e| format!("Failed to read tail fingerprint {}: {}", file.path.display(), e))?;
        ctx.consume(&tail);
    }

    let digest = ctx.compute();
    Ok(digest.0.to_vec())
}