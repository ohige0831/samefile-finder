use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;
use rfd::FileDialog;

use crate::adapters::sqlite_cache::CacheDb;
use crate::app::pipeline::run_pipeline;
use crate::core::cache::{global_cache_db_path, local_cache_db_path};
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

    fn parse_excluded_extensions(raw: &str) -> Vec<String> {
        raw.split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_start_matches('.').to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
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

        let excluded_extensions = Self::parse_excluded_extensions(&self.exclude_extensions_input);

        self.logs.clear();
        self.duplicate_rows.clear();
        self.duplicate_row_index_by_path.clear();
        self.folder_buckets_cache = None;
        self.selected_duplicate_index = None;
        self.is_running = true;
        self.status_text = "Running...".to_string();

        self.last_summary = None;
        self.last_fp_stats = Default::default();
        self.last_hash_stats = Default::default();

        self.push_log(format!("[Start] {}", normalized));
        if excluded_extensions.is_empty() {
            self.push_log("[Config] Excluded extensions: (none)");
        } else {
            self.push_log(format!(
                "[Config] Excluded extensions: {}",
                excluded_extensions.join(", ")
            ));
        }

        let config = ScanConfig {
            target_root: PathBuf::from(normalized),
            follow_symlinks: false,
            min_file_size_bytes: 1,
            excluded_extensions,
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

    fn current_cache_db_path(&self) -> Option<PathBuf> {
        if let Some(p) = global_cache_db_path() {
            return Some(p);
        }

        let normalized = Self::normalize_input_path(&self.target_path);
        if normalized.is_empty() {
            return None;
        }

        Some(local_cache_db_path(Path::new(&normalized)))
    }

    pub fn refresh_cache_stats(&mut self) {
        let Some(db_path) = self.current_cache_db_path() else {
            self.push_log("[Info] Cache DB path is not available yet.");
            return;
        };

        match CacheDb::open(&db_path) {
            Ok(db) => {
                let entries = db.count_entries().ok();
                let size = std::fs::metadata(&db_path).map(|m| m.len()).ok();
                self.cache_entries = entries;
                self.cache_db_size_bytes = size;
                self.cache_db_path = db_path.to_string_lossy().to_string();
                self.push_log(format!(
                    "[Cache] entries={:?}, size={:?} bytes ({})",
                    entries,
                    size,
                    db_path.display()
                ));
            }
            Err(e) => self.push_log(format!("[Error] Cache DB open failed: {}", e)),
        }
    }

    pub fn gc_cache_missing_paths(&mut self) {
        let Some(db_path) = self.current_cache_db_path() else {
            self.push_log("[Info] Cache DB path is not available yet.");
            return;
        };

        self.push_log(format!("[CacheGC] start: {}", db_path.display()));
        match CacheDb::open(&db_path).and_then(|db| db.gc_missing_paths()) {
            Ok(removed) => {
                self.push_log(format!("[CacheGC] removed {} missing paths", removed));
                self.refresh_cache_stats();
            }
            Err(e) => self.push_log(format!("[Error] CacheGC failed: {}", e)),
        }
    }

    pub fn vacuum_cache_db(&mut self) {
        let Some(db_path) = self.current_cache_db_path() else {
            self.push_log("[Info] Cache DB path is not available yet.");
            return;
        };

        self.push_log(format!("[Cache] VACUUM start: {}", db_path.display()));
        match CacheDb::open(&db_path).and_then(|db| db.vacuum()) {
            Ok(_) => {
                self.push_log("[Cache] VACUUM done".to_string());
                self.refresh_cache_stats();
            }
            Err(e) => self.push_log(format!("[Error] VACUUM failed: {}", e)),
        }
    }

    // ----- v2.3.2: Keep / Reclaim -----

    pub fn is_kept(&self, path: &Path) -> bool {
        self.keep_paths.contains(path)
    }

    pub fn toggle_keep(&mut self, path: &Path) {
        if self.keep_paths.contains(path) {
            self.keep_paths.remove(path);
            self.push_log(format!("[Keep] OFF: {}", path.display()));
        } else {
            self.keep_paths.insert(path.to_path_buf());
            self.push_log(format!("[Keep] ON : {}", path.display()));
        }
    }

    pub fn clear_keeps_all(&mut self) {
        let n = self.keep_paths.len();
        self.keep_paths.clear();
        self.push_log(format!("[Keep] cleared all ({} entries)", n));
    }

    pub fn clear_keeps_in_group(&mut self, files: &[PathBuf]) {
        let mut removed = 0usize;
        for p in files {
            if self.keep_paths.remove(p) {
                removed += 1;
            }
        }
        self.push_log(format!("[Keep] cleared in group ({} entries)", removed));
    }

    pub fn keep_only_one_in_group(&mut self, keep: &Path, files: &[PathBuf]) {
        self.clear_keeps_in_group(files);
        self.keep_paths.insert(keep.to_path_buf());
        self.push_log(format!("[Keep] keep-only: {}", keep.display()));
    }

    /// Move all non-kept files from duplicate groups into a reclaim folder.
    /// - Non-destructive: uses rename/move (same volume), falls back to copy+delete.
    /// - Keeps are honored only if the path exists in `keep_paths`.
    pub fn reclaim_move_non_kept(&mut self) {
        let Some(summary) = &self.last_summary else {
            self.push_log("[Info] No results to reclaim.".to_string());
            return;
        };

        let target_root = Self::normalize_input_path(&self.target_path);
        if target_root.is_empty() {
            self.push_log("[Error] Target path is empty.".to_string());
            return;
        }

        let target_root = PathBuf::from(target_root);
        if !target_root.exists() {
            self.push_log(format!(
                "[Error] Target path does not exist: {}",
                target_root.display()
            ));
            return;
        }

        let ts = now_compact_timestamp();
        let default_dest = target_root.join("_SFF_reclaim").join(ts);

        let dest = FileDialog::new()
            .set_title("Pick reclaim destination folder (or Cancel for default)")
            .pick_folder()
            .unwrap_or(default_dest);

        if !self.reclaim_dry_run {
            if let Err(e) = fs::create_dir_all(&dest) {
                self.push_log(format!(
                    "[Error] Failed to create reclaim folder: {} ({})",
                    dest.display(),
                    e
                ));
                return;
            }
        }

        let mut planned: Vec<PathBuf> = Vec::new();
        for g in &summary.duplicate_groups {
            for p in &g.files {
                if self.keep_paths.contains(p) {
                    continue;
                }
                planned.push(p.clone());
            }
        }

        if planned.is_empty() {
            self.push_log("[Reclaim] Nothing to move (all files are kept?)".to_string());
            return;
        }

        self.push_log(format!("[Reclaim] dest: {}", dest.display()));
        self.push_log(format!("[Reclaim] dry-run: {}", self.reclaim_dry_run));
        self.push_log(format!("[Reclaim] moving {} file(s)...", planned.len()));

        let mut moved_ok = 0usize;
        let mut moved_err = 0usize;

        for src in planned {
            let rel = src.strip_prefix(&target_root).ok();
            let mut dst = match rel {
                Some(r) => dest.join(r),
                None => {
                    // outside target root (MIXED): put into a special bucket
                    let safe = sanitize_for_filename(&src.to_string_lossy());
                    let name = src
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "file".to_string());
                    dest.join("_outside_target").join(format!("{}__{}", safe, name))
                }
            };

            // ensure parent dirs
            if let Some(parent) = dst.parent() {
                if !self.reclaim_dry_run {
                    let _ = fs::create_dir_all(parent);
                }
            }

            dst = avoid_collision(dst);

            if self.reclaim_dry_run {
                self.push_log(format!(
                    "[Reclaim][DRY] {} -> {}",
                    src.display(),
                    dst.display()
                ));
                moved_ok += 1;
                continue;
            }

            match move_file_best_effort(&src, &dst) {
                Ok(_) => {
                    self.push_log(format!("[Reclaim] {} -> {}", src.display(), dst.display()));
                    moved_ok += 1;
                }
                Err(e) => {
                    self.push_log(format!("[Error] Reclaim failed: {} ({})", src.display(), e));
                    moved_err += 1;
                }
            }
        }

        self.push_log(format!("[Reclaim] done: ok={}, err={}", moved_ok, moved_err));
        if !self.reclaim_dry_run {
            self.push_log("[Reclaim] Tip: re-run scan to refresh results.".to_string());
        }
    }

    pub fn request_cancel(&mut self) {
        if let Some(flag) = &self.cancel_flag {
            flag.store(true, Ordering::Relaxed);
            self.push_log("[Info] Cancel requested...");
            self.status_text = "Canceling...".to_string();
        }
    }
}

fn now_compact_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // simple base-10 timestamp (good enough for folder name)
    secs.to_string()
}

fn sanitize_for_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let ok = ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.';
        out.push(if ok { ch } else { '_' });
    }
    if out.len() > 64 {
        out.truncate(64);
    }
    out
}

fn avoid_collision(mut dst: PathBuf) -> PathBuf {
    if !dst.exists() {
        return dst;
    }

    let stem = dst
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let ext = dst.extension().map(|s| s.to_string_lossy().to_string());

    for i in 1..=9999u32 {
        let file_name = match &ext {
            Some(e) => format!("{stem}__dup{i}.{e}"),
            None => format!("{stem}__dup{i}"),
        };
        dst.set_file_name(file_name);
        if !dst.exists() {
            return dst;
        }
    }

    dst
}

fn move_file_best_effort(src: &Path, dst: &Path) -> Result<(), String> {
    // Try rename first (fast, same volume)
    if fs::rename(src, dst).is_err() {
        // fallback copy + delete
        fs::copy(src, dst).map_err(|e| format!("copy failed: {e}"))?;
        fs::remove_file(src).map_err(|e| format!("remove failed: {e}"))?;
        return Ok(());
    }
    Ok(())
}

fn csv_escape(s: &str) -> String {
    s.replace('"', "\"\"")
}