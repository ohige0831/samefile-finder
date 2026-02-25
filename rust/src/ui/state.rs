use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, mpsc::Receiver, Arc};

use crate::core::types::{FingerprintStats, HashStats, PipelineStatus, PipelineSummary, ScanEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroupBadge {
    Mixed,
    Shared,
    Internal,
}

#[derive(Clone, Debug)]
pub struct GroupView {
    pub group_index: usize, // 1-based
    pub hash_hex: String,
    pub file_size_bytes: u64,
    pub files: Vec<PathBuf>,
    pub badges: BTreeSet<GroupBadge>,
}

#[derive(Clone, Debug)]
pub struct FolderBucketView {
    pub folder: String,
    pub groups: Vec<GroupView>,
    pub file_count_total: usize,
    pub group_count: usize,
    pub related_folders: BTreeSet<String>,
    pub badges: BTreeSet<GroupBadge>,
}

#[derive(Clone, Debug)]
pub struct DuplicateRow {
    pub text: String,
    pub path: Option<PathBuf>,
}

pub enum WorkerMessage {
    Event(ScanEvent),
    Finished(Result<PipelineStatus, String>),
}

pub struct SameFileApp {
    pub target_path: String,
    pub logs: Vec<String>,
    pub duplicate_rows: Vec<DuplicateRow>,
    pub selected_duplicate_index: Option<usize>,
    pub duplicate_row_index_by_path: HashMap<PathBuf, usize>,

    pub is_running: bool,
    pub status_text: String,

    pub worker_rx: Option<Receiver<WorkerMessage>>,
    pub cancel_flag: Option<Arc<AtomicBool>>,

    // UI表示用サマリ
    pub last_summary: Option<PipelineSummary>,
    pub last_fp_stats: FingerprintStats,
    pub last_hash_stats: HashStats,

    // v2.1.2: UI表示切替（旧UIのフォルダトグル相当）
    pub show_folder_grouping: bool,

    // v2.1.3: 重い集計を毎フレームやらないための表示キャッシュ
    pub folder_buckets_cache: Option<Vec<FolderBucketView>>,
}

impl Default for SameFileApp {
    fn default() -> Self {
        Self {
            target_path: String::new(),
            logs: Vec::new(),
            duplicate_rows: Vec::new(),
            selected_duplicate_index: None,
            duplicate_row_index_by_path: HashMap::new(),
            is_running: false,
            status_text: "Idle".to_string(),
            worker_rx: None,
            cancel_flag: None,
            last_summary: None,
            last_fp_stats: FingerprintStats::default(),
            last_hash_stats: HashStats::default(),
            show_folder_grouping: true,
            folder_buckets_cache: None,
        }
    }
}