use std::fs;
use std::path::{Path, PathBuf};

use crate::core::types::{FileEntry, ScanConfig, ScanResult, SkipReason, SkippedEntry};

pub fn scan_files(config: &ScanConfig) -> Result<ScanResult, String> {
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
    );

    Ok(ScanResult { files, skipped })
}

fn walk_dir(
    dir: &Path,
    config: &ScanConfig,
    files: &mut Vec<FileEntry>,
    skipped: &mut Vec<SkippedEntry>,
) {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            skipped.push(SkippedEntry {
                path: dir.to_path_buf(),
                reason: SkipReason::DirReadFailed(e.to_string()),
            });
            return;
        }
    };

    for entry_result in read_dir {
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
            walk_dir(&path, config, files, skipped);
            continue;
        }

        if metadata.is_file() {
            let size = metadata.len();
            if size >= config.min_file_size_bytes {
                files.push(FileEntry {
                    path,
                    size_bytes: size,
                });
            }
            continue;
        }

        skipped.push(SkippedEntry {
            path,
            reason: SkipReason::NotARegularFile,
        });
    }
}