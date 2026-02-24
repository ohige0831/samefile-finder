use std::collections::HashMap;

use crate::core::types::FileEntry;

pub fn build_size_candidates(files: &[FileEntry]) -> Vec<FileEntry> {
    let mut by_size: HashMap<u64, Vec<&FileEntry>> = HashMap::new();

    for file in files {
        by_size.entry(file.size_bytes).or_default().push(file);
    }

    let mut candidates: Vec<FileEntry> = Vec::new();

    for group in by_size.values() {
        if group.len() >= 2 {
            for file in group {
                candidates.push((*file).clone());
            }
        }
    }

    candidates
}