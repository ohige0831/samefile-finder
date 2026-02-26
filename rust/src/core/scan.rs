use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use crate::core::types::{FileEntry, ScanConfig, ScanResult, SkipReason, SkippedEntry};

pub fn scan_files(config: &ScanConfig, cancel_flag: &AtomicBool) -> Result<ScanResult, String> {
    if !config.target_root.exists() {
        return Err(format!(
            "Target path does not exist: {}",
            config.target_root.display()
        ));
    }

    let mut files: Vec<FileEntry> = Vec::new();
    let mut skipped: Vec<SkippedEntry> = Vec::new();

    walk_dir(
        &config.target_root,
        config,
        &mut files,
        &mut skipped,
        cancel_flag,
    )?;

    Ok(ScanResult { files, skipped })
}

fn should_skip_dir_name(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "target"
            | "build"
            | "dist"
            | "__pycache__"
            | "node_modules"
            | ".venv"
            | "venv"
            | ".idea"
            | ".vscode"
    )
}

fn should_skip_file_name(name: &str) -> bool {
    matches!(
        name,
        ".samefile_finder_cache.sqlite3"
            | ".samefile_finder_cache.sqlite3-shm"
            | ".samefile_finder_cache.sqlite3-wal"
    )
}

fn should_skip_file_extension(path: &Path, excluded_extensions: &[String]) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| {
            excluded_extensions
                .iter()
                .any(|deny| ext.eq_ignore_ascii_case(deny))
        })
        .unwrap_or(false)
}

fn metadata_mtime_ns(metadata: &fs::Metadata) -> i64 {
    match metadata.modified() {
        Ok(st) => match st.duration_since(UNIX_EPOCH) {
            Ok(dur) => {
                let secs = dur.as_secs() as i128;
                let nanos = dur.subsec_nanos() as i128;
                let total = secs.saturating_mul(1_000_000_000i128).saturating_add(nanos);
                total.clamp(i64::MIN as i128, i64::MAX as i128) as i64
            }
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

fn walk_dir(
    dir: &Path,
    config: &ScanConfig,
    files: &mut Vec<FileEntry>,
    skipped: &mut Vec<SkippedEntry>,
    cancel_flag: &AtomicBool,
) -> Result<(), String> {
    if cancel_flag.load(Ordering::Relaxed) {
        return Err("__CANCELED__".to_string());
    }

    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            skipped.push(SkippedEntry {
                path: dir.to_path_buf(),
                reason: SkipReason::DirReadFailed(e.to_string()),
            });
            return Ok(());
        }
    };

    for entry_result in read_dir {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err("__CANCELED__".to_string());
        }

        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                skipped.push(SkippedEntry {
                    path: dir.to_path_buf(),
                    reason: SkipReason::DirReadFailed(e.to_string()),
                });
                continue;
            }
        };

        let path: PathBuf = entry.path();

        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                skipped.push(SkippedEntry {
                    path: path.clone(),
                    reason: SkipReason::MetadataReadFailed(e.to_string()),
                });
                continue;
            }
        };

        let file_type = metadata.file_type();

        if file_type.is_symlink() && !config.follow_symlinks {
            skipped.push(SkippedEntry {
                path: path.clone(),
                reason: SkipReason::NotARegularFile,
            });
            continue;
        }

        if metadata.is_dir() {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if should_skip_dir_name(name) {
                    continue;
                }
            }

            walk_dir(&path, config, files, skipped, cancel_flag)?;
            continue;
        }

        if metadata.is_file() {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if should_skip_file_name(name) {
                    continue;
                }
            }

            // UI指定の拡張子除外（大小文字無視）
            if should_skip_file_extension(&path, &config.excluded_extensions) {
                continue;
            }

            let size = metadata.len();
            if size >= config.min_file_size_bytes {
                let mtime_ns = metadata_mtime_ns(&metadata);
                files.push(FileEntry {
                    path,
                    size_bytes: size,
                    mtime_ns,
                });
            }
            continue;
        }

        skipped.push(SkippedEntry {
            path,
            reason: SkipReason::NotARegularFile,
        });
    }

    Ok(())
}