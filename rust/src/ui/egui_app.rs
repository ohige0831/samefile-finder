use crate::app::pipeline::run_pipeline;
use crate::core::types::{ScanConfig, ScanEvent, SkipReason};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

enum UiMessage {
    Log(String),
    DuplicateLines(Vec<String>),
    Finished,
}

pub struct SameFileFinderApp {
    target_path: String,
    logs: Vec<String>,
    duplicate_lines: Vec<String>,
    is_running: bool,
    rx: Option<Receiver<UiMessage>>,
}

impl Default for SameFileFinderApp {
    fn default() -> Self {
        Self {
            target_path: String::new(),
            logs: Vec::new(),
            duplicate_lines: Vec::new(),
            is_running: false,
            rx: None,
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

    fn start_scan_async(&mut self) {
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

        self.target_path = normalized.clone();
        self.logs.clear();
        self.duplicate_lines.clear();
        self.is_running = true;

        let (tx, rx) = mpsc::channel::<UiMessage>();
        self.rx = Some(rx);

        let _ = tx.send(UiMessage::Log(format!("[Start] {}", normalized)));

        thread::spawn(move || {
            let config = ScanConfig {
                target_root: PathBuf::from(&normalized),
                follow_symlinks: false,
                min_file_size_bytes: 1,
            };

            let mut dup_lines: Vec<String> = Vec::new();

            let result = run_pipeline(config, |event| match event {
                ScanEvent::StageStarted(stage) => {
                    let _ = tx.send(UiMessage::Log(format!("[Stage] {}", stage)));
                }
                ScanEvent::Progress(msg) => {
                    let _ = tx.send(UiMessage::Log(format!("[Info] {}", msg)));
                }
                ScanEvent::FileScanned(_path) => {
                    // 多すぎるので今は送らない
                }
                ScanEvent::FileHashing { path, current, total } => {
                    let _ = tx.send(UiMessage::Log(format!(
                        "[Hash] {}/{} {}",
                        current,
                        total,
                        path.display()
                    )));
                }
                ScanEvent::FileSkipped { path, reason } => {
                    let _ = tx.send(UiMessage::Log(format!(
                        "[Skip] {} | {}",
                        path.display(),
                        SameFileFinderApp::format_skip_reason(&reason)
                    )));
                }
                ScanEvent::Summary(summary) => {
                    let _ = tx.send(UiMessage::Log(String::new()));
                    let _ = tx.send(UiMessage::Log("=== Done ===".to_string()));
                    let _ = tx.send(UiMessage::Log(format!(
                        "Scanned files    : {}",
                        summary.scanned_files
                    )));
                    let _ = tx.send(UiMessage::Log(format!(
                        "Candidate files  : {}",
                        summary.candidate_files
                    )));
                    let _ = tx.send(UiMessage::Log(format!(
                        "Skipped files    : {}",
                        summary.skipped_files
                    )));
                    let _ = tx.send(UiMessage::Log(format!(
                        "Duplicate groups : {}",
                        summary.duplicate_groups.len()
                    )));

                    for (i, group) in summary.duplicate_groups.iter().enumerate() {
                        dup_lines.push(format!(
                            "[Group {}] hash={} count={} size={} bytes",
                            i + 1,
                            group.hash_hex,
                            group.files.len(),
                            group.file_size_bytes
                        ));
                        for path in &group.files {
                            dup_lines.push(path.display().to_string());
                        }
                        dup_lines.push(String::new());
                    }
                }
            });

            match result {
                Ok(_) => {
                    let _ = tx.send(UiMessage::DuplicateLines(dup_lines));
                }
                Err(err) => {
                    let _ = tx.send(UiMessage::Log(format!("[Error] {}", err)));
                }
            }

            let _ = tx.send(UiMessage::Finished);
        });
    }

    fn poll_messages(&mut self) {
        let mut finished = false;
        let mut pending_logs: Vec<String> = Vec::new();
        let mut pending_dup: Option<Vec<String>> = None;

        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    UiMessage::Log(line) => pending_logs.push(line),
                    UiMessage::DuplicateLines(lines) => pending_dup = Some(lines),
                    UiMessage::Finished => finished = true,
                }
            }
        }

        for line in pending_logs {
            self.push_log(line);
        }

        if let Some(lines) = pending_dup {
            self.duplicate_lines = lines;
        }

        if finished {
            self.is_running = false;
            self.rx = None;
        }
    }
}

impl eframe::App for SameFileFinderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 非同期メッセージを毎フレーム回収
        self.poll_messages();

        // 実行中は定期的に再描画（ログを流すため）
        if self.is_running {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.heading("SameFile_Finder v2 (Rust / egui)");

            ui.horizontal(|ui| {
                ui.label("Target Path:");
                ui.text_edit_singleline(&mut self.target_path);

                let run_btn = ui.add_enabled(!self.is_running, egui::Button::new("Run"));
                if run_btn.clicked() {
                    self.start_scan_async();
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