use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::core::types::{DuplicateGroup, FileEntry};

pub fn find_duplicate_groups_by_hash<F>(
    candidates: &[FileEntry],
    mut on_hashing: F,
) -> Result<Vec<DuplicateGroup>, String>
where
    F: FnMut(&Path, usize, usize),
{
    let mut by_key: HashMap<(u64, String), Vec<&FileEntry>> = HashMap::new();
    let total = candidates.len();

    for (idx, file) in candidates.iter().enumerate() {
        on_hashing(&file.path, idx + 1, total);

        let hash_hex = compute_md5_hex(file)?;
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

    Ok(groups)
}

fn compute_md5_hex(file: &FileEntry) -> Result<String, String> {
    let f = File::open(&file.path)
        .map_err(|e| format!("Failed to open file {}: {}", file.path.display(), e))?;

    let mut reader = BufReader::new(f);
    let mut context = md5::Context::new();
    let mut buffer = [0u8; 1024 * 1024]; // 1MB

    loop {
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