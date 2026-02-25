use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;

use eframe::egui;
use rfd::FileDialog;

use crate::app::pipeline::run_pipeline;
use crate::core::types::{ScanConfig, SkipReason};

use crate::ui::state::{SameFileApp, WorkerMessage};

impl SameFileApp {
    pub fn push_log(&mut self, msg: impl Into<String>) {
        self.logs.push(msg.into());
        if self.logs.len() > 5000 {
            let drain_count = self.logs.len() - 5000;
            self.logs.drain(0..drain_count);
        }
    }

    pub fn format_skip_reason(reason: &SkipReason) -> String {
        match reason {
            SkipReason::MetadataReadFailed(msg) => format!("metadata read failed: {}", msg),
            SkipReason::DirReadFailed(msg) => format!("dir read failed: {}", msg),
            SkipReason::FileReadFailed(msg) => format!("file read failed: {}", msg),
            SkipReason::NotARegularFile => "not a regular file".to_string(),
        }
    }

    pub fn normalize_input_path(raw: &str) -> String {
        raw.trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim()
            .to_string()
    }

    pub fn selected_path(&self) -> Option<&Path> {
        let idx = self.selected_duplicate_index?;
        self.duplicate_rows.get(idx)?.path.as_deref()
    }

    pub fn browse_folder(&mut self) {
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

    pub fn export_csv(&mut self) {
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

        // UTF-8 BOM (Excel対策)
        file.write_all(&[0xEF, 0xBB, 0xBF])
            .map_err(|e| e.to_string())?;

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

    pub fn open_selected_folder(&mut self) {
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

        #[cfg(not(target_os = "windows"))]
        {
            self.push_log(format!(
                "[Info] Open Folder is only implemented for Windows: {}",
                target_dir.display()
            ));
        }
    }

    pub fn open_selected_file(&mut self) {
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

        #[cfg(not(target_os = "windows"))]
        {
            self.push_log(format!(
                "[Info] Open File is only implemented for Windows: {}",
                path.display()
            ));
        }
    }

    pub fn reveal_selected_in_explorer(&mut self) {
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

        #[cfg(not(target_os = "windows"))]
        {
            self.push_log(format!(
                "[Info] Reveal is only implemented for Windows: {}",
                path.display()
            ));
        }
    }

    pub fn copy_selected_path(&mut self, ctx: &egui::Context) {
        let Some(path) = self.selected_path() else {
            self.push_log("[Info] No file row selected.");
            return;
        };

        let text = path.to_string_lossy().to_string();
        ctx.copy_text(text.clone());
        self.push_log(format!("[CopyPath] {}", text));
    }

    pub fn start_scan_async(&mut self) {
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
        self.last_fp_stats = Default::default();
        self.last_hash_stats = Default::default();

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

    pub fn request_cancel(&mut self) {
        if let Some(flag) = &self.cancel_flag {
            flag.store(true, Ordering::Relaxed);
            self.push_log("[Info] Cancel requested...");
            self.status_text = "Canceling...".to_string();
        }
    }
}

fn csv_escape(s: &str) -> String {
    s.replace('"', "\"\"")
}
