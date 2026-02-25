use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub target_root: PathBuf,
    pub follow_symlinks: bool,
    pub min_file_size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime_ns: i64,
}

#[derive(Debug, Clone)]
pub enum SkipReason {
    MetadataReadFailed(String),
    DirReadFailed(String),
    FileReadFailed(String),
    NotARegularFile,
}

#[derive(Debug, Clone)]
pub struct SkippedEntry {
    pub path: PathBuf,
    pub reason: SkipReason,
}

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub files: Vec<FileEntry>,
    pub skipped: Vec<SkippedEntry>,
}

#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub hash_hex: String,
    pub file_size_bytes: u64,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct FingerprintStats {
    pub total_inputs: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub computed: usize,
    pub narrowed_outputs: usize,
}

#[derive(Debug, Clone, Default)]
pub struct HashStats {
    pub total_inputs: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub computed: usize,
}

#[derive(Debug, Clone)]
pub struct PipelineSummary {
    pub scanned_files: usize,
    pub candidate_files: usize,        // same-size candidates
    pub fingerprint_candidates: usize, // after fingerprint
    pub skipped_files: usize,
    pub duplicate_groups: Vec<DuplicateGroup>,
    pub fingerprint_stats: FingerprintStats,
    pub hash_stats: HashStats,
}

#[derive(Debug, Clone)]
pub enum ScanEvent {
    StageStarted(&'static str),
    Progress(String),

    FileScanned(PathBuf),

    FileFingerprinting {
        path: PathBuf,
        current: usize,
        total: usize,
    },

    FileHashing {
        path: PathBuf,
        current: usize,
        total: usize,
    },

    FileSkipped {
        path: PathBuf,
        reason: SkipReason,
    },

    FingerprintStats(FingerprintStats),
    HashStats(HashStats),

    Summary(PipelineSummary),
}

#[derive(Debug, Clone)]
pub enum PipelineStatus {
    Completed(PipelineSummary),
    Canceled,
}
