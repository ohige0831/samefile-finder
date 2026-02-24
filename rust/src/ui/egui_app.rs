use crate::app::pipeline::run_pipeline;
use crate::core::types::{ScanConfig, ScanEvent, SkipReason};
use eframe::egui;
use std::path::PathBuf;

pub struct SameFileFinderApp {
    target_path: String,
    logs: Vec<String>,
    duplicate_lines: Vec<String>,
    is_running: bool,
}

impl Default for SameFileFinderApp {
    fn default() -> Self {
        Self {
            target_path: String::new(),
            logs: Vec::new(),
            duplicate_lines: Vec::new(),
            is_running: false,
        }
    }
}

impl SameFileFinderApp {
    fn push_log(&mut self, msg: impl Into<String>) {
        self.logs.push(msg.into());
        if self.logs.len() > 5000 {
            let drain_count = self.logs.len() - 5000;
            self.logs.drain(0..drain_count);
        }
    }

    fn format_skip_reason(reason: &SkipReason) -> String {
        match reason {
            SkipReason::MetadataReadFailed(msg) => format!("metadata read failed: {}", msg),
            SkipReason::DirReadFailed(msg) => format!("dir read failed: {}", msg),
            SkipReason::FileReadFailed(msg) => format!("file read failed: {}", msg),
            SkipReason::NotARegularFile => "not a regular file".to_string(),
        }
    }

    fn run_scan(&mut self) {
        // borrowを残さないよう、最初に owned String を作る
        let normalized: String = self
            .target_path
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        if normalized.is_empty() {
            self.push_log("[Error] Target path is empty.");
            return;
        }

        // 正規化した文字列をUIにも反映
        self.target_path = normalized.clone();

        self.logs.clear();
        self.duplicate_lines.clear();
        self.is_running = true;

        let config = ScanConfig {
            target_root: PathBuf::from(&normalized),
            follow_symlinks: false,
            min_file_size_bytes: 1,
        };

        self.push_log(format!("[Start] {}", normalized));

        let mut local_logs: Vec<String> = Vec::new();
        let mut local_duplicates: Vec<String> = Vec::new();

        let result = run_pipeline(config, |event| match event {
            ScanEvent::StageStarted(stage) => {
                local_logs.push(format!("[Stage] {}", stage));
            }
            ScanEvent::Progress(msg) => {
                local_logs.push(format!("[Info] {}", msg));
            }
            ScanEvent::FileScanned(_path) => {
                // 件数が多くなりやすいので今は表示しない
            }
            ScanEvent::FileHashing { path, current, total } => {
                local_logs.push(format!("[Hash] {}/{} {}", current, total, path.display()));
            }
            ScanEvent::FileSkipped { path, reason } => {
                local_logs.push(format!(
                    "[Skip] {} | {}",
                    path.display(),
                    Self::format_skip_reason(&reason)
                ));
            }
            ScanEvent::Summary(summary) => {
                local_logs.push(String::new());
                local_logs.push("=== Done ===".to_string());
                local_logs.push(format!("Scanned files    : {}", summary.scanned_files));
                local_logs.push(format!("Candidate files  : {}", summary.candidate_files));
                local_logs.push(format!("Skipped files    : {}", summary.skipped_files));
                local_logs.push(format!("Duplicate groups : {}", summary.duplicate_groups.len()));

                for (i, group) in summary.duplicate_groups.iter().enumerate() {
                    local_duplicates.push(format!(
                        "[Group {}] hash={} count={} size={} bytes",
                        i + 1,
                        group.hash_hex,
                        group.files.len(),
                        group.file_size_bytes
                    ));
                    for path in &group.files {
                        local_duplicates.push(path.display().to_string());
                    }
                    local_duplicates.push(String::new());
                }
            }
        });

        match result {
            Ok(_) => {
                self.logs.extend(local_logs);
                self.duplicate_lines = local_duplicates;
            }
            Err(err) => {
                self.logs.extend(local_logs);
                self.push_log(format!("[Error] {}", err));
            }
        }

        self.is_running = false;
    }
}

impl eframe::App for SameFileFinderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.heading("SameFile_Finder v2 (Rust / egui)");

            ui.horizontal(|ui| {
                ui.label("Target Path:");
                ui.text_edit_singleline(&mut self.target_path);

                let run_btn = ui.add_enabled(!self.is_running, egui::Button::new("Run"));
                if run_btn.clicked() {
                    self.run_scan();
                }

                if ui.button("Clear Logs").clicked() {
                    self.logs.clear();
                }
            });

            ui.label(if self.is_running { "Running..." } else { "Idle" });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |cols| {
                cols[0].group(|ui| {
                    ui.heading("Logs");
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .id_salt("logs_scroll")
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            for line in &self.logs {
                                ui.label(line);
                            }
                        });
                });

                cols[1].group(|ui| {
                    ui.heading("Duplicate Result");
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .id_salt("dup_scroll")
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            for line in &self.duplicate_lines {
                                if line.is_empty() {
                                    ui.separator();
                                } else {
                                    ui.label(line);
                                }
                            }
                        });
                });
            });
        });
    }
}