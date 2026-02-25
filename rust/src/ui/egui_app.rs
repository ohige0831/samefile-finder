use crate::app::pipeline::run_pipeline;
use crate::core::types::{
    FingerprintStats, HashStats, PipelineStatus, PipelineSummary, ScanConfig, ScanEvent, SkipReason,
};
use eframe::egui;
use rfd::FileDialog;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
    Arc,
};
use std::thread;

#[derive(Clone, Debug)]
struct DuplicateRow {
    text: String,
    path: Option<PathBuf>,
}

enum WorkerMessage {
    Event(ScanEvent),
    Finished(Result<PipelineStatus, String>),
}

pub struct SameFileFinderApp {
    target_path: String,
    logs: Vec<String>,
    duplicate_rows: Vec<DuplicateRow>,
    selected_duplicate_index: Option<usize>,
    is_running: bool,
    status_text: String,
    worker_rx: Option<Receiver<WorkerMessage>>,
    cancel_flag: Option<Arc<AtomicBool>>,

    // Step D: UI表示用サマリ
    last_summary: Option<PipelineSummary>,
    last_fp_stats: FingerprintStats,
    last_hash_stats: HashStats,
}

impl Default for SameFileFinderApp {
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

    fn normalize_input_path(raw: &str) -> String {
        raw.trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim()
            .to_string()
    }

    fn selected_path(&self) -> Option<&Path> {
        let idx = self.selected_duplicate_index?;
        self.duplicate_rows.get(idx)?.path.as_deref()
    }

    fn browse_folder(&mut self) {
        let mut dialog = FileDialog::new();

        let current = Self::normalize_input_path(&self.target_path);
        if !current.is_empty() {
            let p = PathBuf::from(&current);
            if p.exists() {
                let dir = if p.is_dir() {
                    p
                } else {
                    p.parent()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("."))
                };
                dialog = dialog.set_directory(dir);
            }
        }

        if let Some(folder) = dialog.pick_folder() {
            self.target_path = folder.to_string_lossy().to_string();
            self.push_log(format!("[Browse] {}", folder.display()));
        }
    }

    fn export_csv(&mut self) {
        if self.duplicate_rows.is_empty() {
            self.push_log("[Info] No duplicate result to export.");
            return;
        }

        let suggested = "duplicate_report.csv";
        let save_path = FileDialog::new()
            .set_file_name(suggested)
            .add_filter("CSV", &["csv"])
            .save_file();

        let Some(path) = save_path else {
            self.push_log("[Info] CSV export canceled.");
            return;
        };

        match self.write_csv(&path) {
            Ok(_) => self.push_log(format!("[ExportCSV] {}", path.display())),
            Err(e) => self.push_log(format!("[Error] CSV export failed: {}", e)),
        }
    }

    fn write_csv(&self, path: &Path) -> Result<(), String> {
        let mut file = File::create(path).map_err(|e| e.to_string())?;
        file.write_all(&[0xEF, 0xBB, 0xBF]).map_err(|e| e.to_string())?;
        file.write_all(b"group_index,row_type,path_or_text\r\n")
            .map_err(|e| e.to_string())?;

        let mut group_index: usize = 0;
        for row in &self.duplicate_rows {
            if row.text.is_empty() {
                continue;
            }

            if row.path.is_none() && row.text.starts_with("[Group ") {
                group_index += 1;
                let line = format!("{},group,\"{}\"\r\n", group_index, csv_escape(&row.text));
                file.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
                continue;
            }

            if let Some(p) = &row.path {
                let line = format!(
                    "{},file,\"{}\"\r\n",
                    group_index,
                    csv_escape(&p.to_string_lossy())
                );
                file.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
            } else {
                let line = format!("{},text,\"{}\"\r\n", group_index, csv_escape(&row.text));
                file.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
            }
        }

        Ok(())
    }

    fn open_selected_folder(&mut self) {
        let Some(path) = self.selected_path().map(PathBuf::from) else {
            self.push_log("[Info] No file row selected.");
            return;
        };

        let target_dir = if path.is_dir() {
            path
        } else {
            path.parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        };

        #[cfg(target_os = "windows")]
        {
            match Command::new("explorer").arg(&target_dir).spawn() {
                Ok(_) => self.push_log(format!("[OpenFolder] {}", target_dir.display())),
                Err(e) => self.push_log(format!(
                    "[Error] Failed to open folder: {} ({})",
                    target_dir.display(),
                    e
                )),
            }
        }
    }

    fn open_selected_file(&mut self) {
        let Some(path) = self.selected_path().map(PathBuf::from) else {
            self.push_log("[Info] No file row selected.");
            return;
        };

        if !path.exists() {
            self.push_log(format!("[Error] File not found: {}", path.display()));
            return;
        }

        #[cfg(target_os = "windows")]
        {
            match Command::new("cmd")
                .arg("/C")
                .arg("start")
                .arg("")
                .arg(&path)
                .spawn()
            {
                Ok(_) => self.push_log(format!("[OpenFile] {}", path.display())),
                Err(e) => self.push_log(format!(
                    "[Error] Failed to open file: {} ({})",
                    path.display(),
                    e
                )),
            }
        }
    }

    fn reveal_selected_in_explorer(&mut self) {
        let Some(path) = self.selected_path().map(PathBuf::from) else {
            self.push_log("[Info] No file row selected.");
            return;
        };

        #[cfg(target_os = "windows")]
        {
            match Command::new("explorer").arg("/select,").arg(&path).spawn() {
                Ok(_) => self.push_log(format!("[Reveal] {}", path.display())),
                Err(e) => self.push_log(format!(
                    "[Error] Failed to reveal in Explorer: {} ({})",
                    path.display(),
                    e
                )),
            }
        }
    }

    fn copy_selected_path(&mut self, ctx: &egui::Context) {
        let Some(path) = self.selected_path() else {
            self.push_log("[Info] No file row selected.");
            return;
        };

        let text = path.to_string_lossy().to_string();
        ctx.copy_text(text.clone());
        self.push_log(format!("[CopyPath] {}", text));
    }

    fn start_scan_async(&mut self) {
        let normalized = Self::normalize_input_path(&self.target_path);
        if normalized.is_empty() {
            self.push_log("[Error] Target path is empty.");
            return;
        }

        self.target_path = normalized.clone();
        self.logs.clear();
        self.duplicate_rows.clear();
        self.selected_duplicate_index = None;
        self.is_running = true;
        self.status_text = "Running...".to_string();

        self.last_summary = None;
        self.last_fp_stats = FingerprintStats::default();
        self.last_hash_stats = HashStats::default();

        self.push_log(format!("[Start] {}", normalized));

        let config = ScanConfig {
            target_root: PathBuf::from(normalized),
            follow_symlinks: false,
            min_file_size_bytes: 1,
        };

        let (tx, rx) = mpsc::channel::<WorkerMessage>();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel_flag);

        thread::spawn(move || {
            let tx_event = tx.clone();
            let result = run_pipeline(config, &cancel_for_thread, |event| {
                let _ = tx_event.send(WorkerMessage::Event(event));
            });
            let _ = tx.send(WorkerMessage::Finished(result));
        });

        self.worker_rx = Some(rx);
        self.cancel_flag = Some(cancel_flag);
    }

    fn request_cancel(&mut self) {
        if let Some(flag) = &self.cancel_flag {
            flag.store(true, Ordering::Relaxed);
            self.push_log("[Info] Cancel requested...");
            self.status_text = "Canceling...".to_string();
        }
    }

    fn poll_worker_messages(&mut self) {
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
            ScanEvent::FileScanned(_path) => {}

            ScanEvent::FileFingerprinting { path, current, total } => {
                self.status_text = format!("Fingerprinting {}/{}", current, total);
                // ログは多すぎるので間引く（先頭/末尾/50件ごと）
                if current <= 3 || current == total || current % 50 == 0 {
                    self.push_log(format!("[FP] {}/{} {}", current, total, path.display()));
                }
            }

            ScanEvent::FileHashing { path, current, total } => {
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
                self.push_log(format!("Scanned files       : {}", summary.scanned_files));
                self.push_log(format!("Size candidates     : {}", summary.candidate_files));
                self.push_log(format!(
                    "Fingerprint cand.   : {}",
                    summary.fingerprint_candidates
                ));
                self.push_log(format!("Skipped files       : {}", summary.skipped_files));
                self.push_log(format!(
                    "Duplicate groups    : {}",
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

    fn draw_summary_panel(&self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.heading("Run Summary");
            ui.separator();

            let Some(summary) = &self.last_summary else {
                ui.label("No results yet.");
                return;
            };

            egui::Grid::new("summary_grid")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Scanned files");
                    ui.label(summary.scanned_files.to_string());
                    ui.end_row();

                    ui.label("Skipped files");
                    ui.label(summary.skipped_files.to_string());
                    ui.end_row();

                    ui.label("Same-size candidates");
                    ui.label(summary.candidate_files.to_string());
                    ui.end_row();

                    ui.label("Fingerprint candidates");
                    ui.label(summary.fingerprint_candidates.to_string());
                    ui.end_row();

                    ui.label("Duplicate groups");
                    ui.label(summary.duplicate_groups.len().to_string());
                    ui.end_row();
                });

            ui.separator();
            ui.label(egui::RichText::new("Fingerprint cache").strong());

            egui::Grid::new("fp_stats_grid")
                .num_columns(4)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Hit");
                    ui.label(summary.fingerprint_stats.cache_hits.to_string());
                    ui.label("Miss");
                    ui.label(summary.fingerprint_stats.cache_misses.to_string());
                    ui.end_row();

                    ui.label("Computed");
                    ui.label(summary.fingerprint_stats.computed.to_string());
                    ui.label("Narrowed");
                    ui.label(summary.fingerprint_stats.narrowed_outputs.to_string());
                    ui.end_row();
                });

            ui.separator();
            ui.label(egui::RichText::new("Hash cache").strong());

            egui::Grid::new("hash_stats_grid")
                .num_columns(4)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Hit");
                    ui.label(summary.hash_stats.cache_hits.to_string());
                    ui.label("Miss");
                    ui.label(summary.hash_stats.cache_misses.to_string());
                    ui.end_row();

                    ui.label("Computed");
                    ui.label(summary.hash_stats.computed.to_string());
                    ui.label("Inputs");
                    ui.label(summary.hash_stats.total_inputs.to_string());
                    ui.end_row();
                });
        });
    }
}

impl eframe::App for SameFileFinderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker_messages();
        if self.is_running {
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.heading("SameFile_Finder v2 (Rust / egui)");

            ui.horizontal(|ui| {
                ui.label("Target Path:");
                ui.add(egui::TextEdit::singleline(&mut self.target_path).desired_width(520.0));

                if ui.add_enabled(!self.is_running, egui::Button::new("Browse")).clicked() {
                    self.browse_folder();
                }
            });

            ui.horizontal(|ui| {
                if ui.add_enabled(!self.is_running, egui::Button::new("Run")).clicked() {
                    self.start_scan_async();
                }

                if ui.add_enabled(self.is_running, egui::Button::new("Cancel")).clicked() {
                    self.request_cancel();
                }

                if ui.button("Clear Logs").clicked() {
                    self.logs.clear();
                }

                if ui.button("Export CSV").clicked() {
                    self.export_csv();
                }

                let has_selection = self.selected_path().is_some();

                if ui
                    .add_enabled(has_selection, egui::Button::new("Open Folder"))
                    .clicked()
                {
                    self.open_selected_folder();
                }
                if ui
                    .add_enabled(has_selection, egui::Button::new("Open File"))
                    .clicked()
                {
                    self.open_selected_file();
                }
                if ui
                    .add_enabled(has_selection, egui::Button::new("Copy Path"))
                    .clicked()
                {
                    self.copy_selected_path(ctx);
                }
                if ui
                    .add_enabled(has_selection, egui::Button::new("Reveal"))
                    .clicked()
                {
                    self.reveal_selected_in_explorer();
                }
            });

            ui.label(&self.status_text);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |cols| {
                cols[0].group(|ui| {
                    ui.heading("Logs");
                    ui.separator();
                    egui::ScrollArea::both()
                        .id_salt("logs_scroll")
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            for line in &self.logs {
                                if line.starts_with("[Error]") || line.starts_with("[Canceled]") {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(220, 90, 90),
                                        egui::RichText::new(line).monospace(),
                                    );
                                } else if line.starts_with("[Stage]") {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(100, 180, 255),
                                        egui::RichText::new(line).monospace(),
                                    );
                                } else if line.starts_with("[Info]") {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(180, 220, 180),
                                        egui::RichText::new(line).monospace(),
                                    );
                                } else if line.starts_with("[FP]") {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(220, 220, 120),
                                        egui::RichText::new(line).monospace(),
                                    );
                                } else {
                                    ui.label(egui::RichText::new(line).monospace());
                                }
                            }
                        });
                });

                cols[1].vertical(|ui| {
                    self.draw_summary_panel(ui);
                    ui.add_space(8.0);

                    ui.group(|ui| {
                        ui.heading("Duplicate Result");
                        ui.separator();

                        let mut clicked_index: Option<usize> = None;
                        let mut double_clicked_index: Option<usize> = None;

                        egui::ScrollArea::both()
                            .id_salt("dup_scroll")
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                for (i, row) in self.duplicate_rows.iter().enumerate() {
                                    if row.text.is_empty() {
                                        ui.separator();
                                        continue;
                                    }

                                    let is_selected = self.selected_duplicate_index == Some(i);
                                    let rt = if row.path.is_none() {
                                        egui::RichText::new(&row.text).monospace().strong()
                                    } else {
                                        egui::RichText::new(&row.text).monospace()
                                    };

                                    let response = ui.selectable_label(is_selected, rt);
                                    if response.clicked() {
                                        clicked_index = Some(i);
                                    }
                                    if response.double_clicked() && row.path.is_some() {
                                        double_clicked_index = Some(i);
                                    }
                                }
                            });

                        if let Some(i) = clicked_index {
                            self.selected_duplicate_index = Some(i);
                        }
                        if let Some(i) = double_clicked_index {
                            self.selected_duplicate_index = Some(i);
                            self.reveal_selected_in_explorer();
                        }
                    });
                });
            });
        });
    }
}

fn csv_escape(s: &str) -> String {
    s.replace('"', "\"\"")
}