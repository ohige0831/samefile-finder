use std::sync::mpsc;

use crate::core::types::{PipelineStatus, ScanEvent};

use crate::ui::state::{DuplicateRow, SameFileApp, WorkerMessage};

impl SameFileApp {
    pub fn poll_worker_messages(&mut self) {
        let mut pending = Vec::new();
        let mut disconnected = false;

        if let Some(rx) = &self.worker_rx {
            loop {
                match rx.try_recv() {
                    Ok(msg) => pending.push(msg),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        for msg in pending {
            match msg {
                WorkerMessage::Event(event) => self.handle_scan_event(event),
                WorkerMessage::Finished(result) => {
                    self.finalize_worker(result);
                }
            }
        }

        if disconnected && self.is_running {
            self.is_running = false;
            self.worker_rx = None;
            self.cancel_flag = None;
            self.status_text = "Worker disconnected".to_string();
            self.push_log("[Error] Worker channel disconnected.");
        }
    }

    fn handle_scan_event(&mut self, event: ScanEvent) {
        match event {
            ScanEvent::StageStarted(stage) => self.push_log(format!("[Stage] {}", stage)),
            ScanEvent::Progress(msg) => self.push_log(format!("[Info] {}", msg)),
            ScanEvent::FileScanned(_path) => {
                // ここはログが多すぎるので表示しない（旧実装準拠）
            }
            ScanEvent::FileFingerprinting {
                path,
                current,
                total,
            } => {
                self.status_text = format!("Fingerprinting {}/{}", current, total);

                // ログは多すぎるので間引く（先頭/末尾/50件ごと）
                if current <= 3 || current == total || current % 50 == 0 {
                    self.push_log(format!("[FP] {}/{} {}", current, total, path.display()));
                }
            }
            ScanEvent::FileHashing {
                path,
                current,
                total,
            } => {
                self.push_log(format!("[Hash] {}/{} {}", current, total, path.display()));
                self.status_text = format!("Hashing {}/{}", current, total);
            }
            ScanEvent::FileSkipped { path, reason } => {
                self.push_log(format!(
                    "[Skip] {} | {}",
                    path.display(),
                    Self::format_skip_reason(&reason)
                ));
            }
            ScanEvent::FingerprintStats(stats) => {
                self.last_fp_stats = stats.clone();
                self.push_log(format!(
                    "[Info] Fingerprint cache: hit={}, miss={}, computed={}, narrowed={}",
                    stats.cache_hits, stats.cache_misses, stats.computed, stats.narrowed_outputs
                ));
            }
            ScanEvent::HashStats(stats) => {
                self.last_hash_stats = stats.clone();
                self.push_log(format!(
                    "[Info] Hash cache: hit={}, miss={}, computed={}",
                    stats.cache_hits, stats.cache_misses, stats.computed
                ));
            }
            ScanEvent::Summary(summary) => {
                self.last_summary = Some(summary.clone());

                self.push_log(String::new());
                self.push_log("=== Done ===");
                self.push_log(format!("Scanned files : {}", summary.scanned_files));
                self.push_log(format!("Size candidates : {}", summary.candidate_files));
                self.push_log(format!(
                    "Fingerprint candidates : {}",
                    summary.fingerprint_candidates
                ));
                self.push_log(format!("Skipped files : {}", summary.skipped_files));
                self.push_log(format!(
                    "Duplicate groups : {}",
                    summary.duplicate_groups.len()
                ));

                self.duplicate_rows.clear();
                for (i, group) in summary.duplicate_groups.iter().enumerate() {
                    self.duplicate_rows.push(DuplicateRow {
                        text: format!(
                            "[Group {}] hash={} count={} size={} bytes",
                            i + 1,
                            group.hash_hex,
                            group.files.len(),
                            group.file_size_bytes
                        ),
                        path: None,
                    });

                    for path in &group.files {
                        self.duplicate_rows.push(DuplicateRow {
                            text: path.display().to_string(),
                            path: Some(path.clone()),
                        });
                    }

                    self.duplicate_rows.push(DuplicateRow {
                        text: String::new(),
                        path: None,
                    });
                }
            }
        }
    }

    fn finalize_worker(&mut self, result: Result<PipelineStatus, String>) {
        match result {
            Ok(PipelineStatus::Completed(_summary)) => {
                self.status_text = "Done".to_string();
            }
            Ok(PipelineStatus::Canceled) => {
                self.status_text = "Canceled".to_string();
                self.push_log("[Canceled] Scan canceled by user.");
            }
            Err(err) => {
                self.status_text = "Error".to_string();
                self.push_log(format!("[Error] {}", err));
            }
        }

        self.is_running = false;
        self.worker_rx = None;
        self.cancel_flag = None;
    }
}
