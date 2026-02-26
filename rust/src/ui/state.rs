use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, mpsc::Receiver, Arc};

use crate::core::types::{FingerprintStats, HashStats, PipelineStatus, PipelineSummary, ScanEvent};
use crate::core::cache::global_cache_db_path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroupBadge {
    Mixed,
    Shared,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSortMode {
    GroupIndexAsc,
    FileCountDesc,
    SizeDesc,
    PathAsc,
}

impl GroupSortMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::GroupIndexAsc => "Group #",
            Self::FileCountDesc => "File count (desc)",
            Self::SizeDesc => "Size (desc)",
            Self::PathAsc => "Path (asc)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupBadgeFilter {
    All,
    Mixed,
    Shared,
    Internal,
}

impl GroupBadgeFilter {
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Mixed => "MIXED",
            Self::Shared => "SHARED",
            Self::Internal => "INTERNAL",
        }
    }
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
    pub exclude_extensions_input: String, // 例: "lrc,txt,jpg"
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

    // v2.1.4: 重複リスト表示オプション
    pub group_sort_mode: GroupSortMode,
    pub group_badge_filter: GroupBadgeFilter,
    pub group_name_filter: String,

    // v2.3.0: cache DB info
    pub cache_db_path: String,
    pub cache_entries: Option<u64>,
    pub cache_db_size_bytes: Option<u64>,
}

impl Default for SameFileApp {
    fn default() -> Self {
        let cache_db_path = global_cache_db_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "(per-target local cache)".to_string());

        Self {
            target_path: String::new(),
            exclude_extensions_input: "lrc,txt".to_string(),
            logs: Vec::new(),
            duplicate_rows: Vec::new(),
            selected_duplicate_index: None,
            duplicate_row_index_by_path: HashMap::new(),
            is_running: false,
            status_text: "Idle".to_string(),
            worker_rx: None,
            cancel_flag: None,
            last_summary: None,
            last_fp_stats: Default::default(),
            last_hash_stats: Default::default(),
            show_folder_grouping: true,
            folder_buckets_cache: None,
            group_sort_mode: GroupSortMode::GroupIndexAsc,
            group_badge_filter: GroupBadgeFilter::All,
            group_name_filter: String::new(),

            cache_db_path,
            cache_entries: None,
            cache_db_size_bytes: None,
        }
    }
}