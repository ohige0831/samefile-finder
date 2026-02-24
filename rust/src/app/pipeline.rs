use crate::core::{
    group::build_size_candidates,
    hash::find_duplicate_groups_by_hash,
    scan::scan_files,
    types::{PipelineSummary, ScanConfig, ScanEvent},
};

pub fn run_pipeline<F>(config: ScanConfig, mut event_sink: F) -> Result<PipelineSummary, String>
where
    F: FnMut(ScanEvent),
{
    event_sink(ScanEvent::StageStarted("scan"));

    let scan_result = scan_files(&config)?;

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

    let candidate_files = build_size_candidates(&scan_result.files);

    event_sink(ScanEvent::Progress(format!(
        "Candidate files (same-size only): {}",
        candidate_files.len()
    )));

    event_sink(ScanEvent::StageStarted("hash"));

    let duplicate_groups = find_duplicate_groups_by_hash(&candidate_files, |path, current, total| {
        event_sink(ScanEvent::FileHashing {
            path: path.to_path_buf(),
            current,
            total,
        });
    })?;

    let summary = PipelineSummary {
        scanned_files: scan_result.files.len(),
        candidate_files: candidate_files.len(),
        skipped_files: scan_result.skipped.len(),
        duplicate_groups,
    };

    event_sink(ScanEvent::Summary(summary.clone()));

    Ok(summary)
}