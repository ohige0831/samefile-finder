use std::sync::atomic::AtomicBool;

use crate::core::{
    fingerprint::build_fingerprint_candidates,
    group::build_size_candidates,
    hash::find_duplicate_groups_by_hash,
    scan::scan_files,
    types::{PipelineStatus, PipelineSummary, ScanConfig, ScanEvent},
};

pub fn run_pipeline<F>(
    config: ScanConfig,
    cancel_flag: &AtomicBool,
    mut event_sink: F,
) -> Result<PipelineStatus, String>
where
    F: FnMut(ScanEvent),
{
    event_sink(ScanEvent::StageStarted("scan"));

    let scan_result = match scan_files(&config, cancel_flag) {
        Ok(v) => v,
        Err(e) if e == "__CANCELED__" => return Ok(PipelineStatus::Canceled),
        Err(e) => return Err(e),
    };

    for file in &scan_result.files {
        event_sink(ScanEvent::FileScanned(file.path.clone()));
    }

    for skipped in &scan_result.skipped {
        event_sink(ScanEvent::FileSkipped {
            path: skipped.path.clone(),
            reason: skipped.reason.clone(),
        });
    }

    event_sink(ScanEvent::Progress(format!(
        "Scanned: {} files, skipped: {}",
        scan_result.files.len(),
        scan_result.skipped.len()
    )));

    event_sink(ScanEvent::StageStarted("group_by_size"));
    let size_candidates = build_size_candidates(&scan_result.files);
    event_sink(ScanEvent::Progress(format!(
        "Candidate files (same-size only): {}",
        size_candidates.len()
    )));

    let cache_db_path = config.target_root.join(".samefile_finder_cache.sqlite3");
    event_sink(ScanEvent::Progress(format!(
        "Cache DB: {}",
        cache_db_path.display()
    )));

    event_sink(ScanEvent::StageStarted("fingerprint"));
    let fp_stage = match build_fingerprint_candidates(
        &size_candidates,
        cancel_flag,
        &cache_db_path,
        |path: &std::path::Path, current, total| {
            event_sink(ScanEvent::FileFingerprinting {
                path: path.to_path_buf(),
                current,
                total,
            });
        },
    ) {
        Ok(v) => v,
        Err(e) if e == "__CANCELED__" => return Ok(PipelineStatus::Canceled),
        Err(e) => return Err(e),
    };

    event_sink(ScanEvent::Progress(format!(
        "Fingerprint candidates: {} -> {}",
        fp_stage.stats.total_inputs,
        fp_stage.stats.narrowed_outputs
    )));
    event_sink(ScanEvent::FingerprintStats(fp_stage.stats.clone()));

    event_sink(ScanEvent::StageStarted("hash"));
    let hash_stage = match find_duplicate_groups_by_hash(
        &fp_stage.candidates,
        cancel_flag,
        &cache_db_path,
        |path: &std::path::Path, current, total| {
            event_sink(ScanEvent::FileHashing {
                path: path.to_path_buf(),
                current,
                total,
            });
        },
    ) {
        Ok(v) => v,
        Err(e) if e == "__CANCELED__" => return Ok(PipelineStatus::Canceled),
        Err(e) => return Err(e),
    };

    event_sink(ScanEvent::HashStats(hash_stage.stats.clone()));
    event_sink(ScanEvent::Progress(format!(
        "Duplicate groups found: {}",
        hash_stage.duplicate_groups.len()
    )));

    let summary = PipelineSummary {
        scanned_files: scan_result.files.len(),
        candidate_files: size_candidates.len(),
        fingerprint_candidates: fp_stage.candidates.len(),
        skipped_files: scan_result.skipped.len(),
        duplicate_groups: hash_stage.duplicate_groups,
        fingerprint_stats: fp_stage.stats,
        hash_stats: hash_stage.stats,
    };

    event_sink(ScanEvent::Summary(summary.clone()));
    Ok(PipelineStatus::Completed(summary))
}