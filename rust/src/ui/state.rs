use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, mpsc::Receiver, Arc};

use crate::core::types::{FingerprintStats, HashStats, PipelineStatus, PipelineSummary, ScanEvent};

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
}

impl Default for SameFileApp {
    fn default() -> Self {
        Self {
            target_path: String::new(),
            logs: Vec::new(),
            duplicate_rows: Vec::new(),
            selected_duplicate_index: None,
            is_running: false,
            status_text: "Idle".to_string(),
            worker_rx: None,
            cancel_flag: None,
            last_summary: None,
            last_fp_stats: FingerprintStats::default(),
            last_hash_stats: HashStats::default(),
            show_folder_grouping: true,
        }
    }
}
