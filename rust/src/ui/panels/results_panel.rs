use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eframe::egui;

use crate::ui::state::{
    FolderBucketView, GroupBadge, GroupBadgeFilter, GroupSortMode, GroupView, SameFileApp,
};

pub fn draw_results_panel(app: &mut SameFileApp, ui: &mut egui::Ui) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.heading(if app.show_folder_grouping {
                "Duplicate Result (by folder)"
            } else {
                "Duplicate Result"
            });

            ui.separator();
            ui.checkbox(&mut app.show_folder_grouping, "Folder grouping");

            ui.separator();
            ui.label("Sort:");
            egui::ComboBox::from_id_salt("dup_group_sort")
                .selected_text(app.group_sort_mode.label())
                .show_ui(ui, |ui| {
                    for mode in [
                        GroupSortMode::GroupIndexAsc,
                        GroupSortMode::FileCountDesc,
                        GroupSortMode::SizeDesc,
                        GroupSortMode::PathAsc,
                    ] {
                        ui.selectable_value(&mut app.group_sort_mode, mode, mode.label());
                    }
                });

            ui.label("Filter:");
            egui::ComboBox::from_id_salt("dup_group_badge_filter")
                .selected_text(app.group_badge_filter.label())
                .show_ui(ui, |ui| {
                    for f in [
                        GroupBadgeFilter::All,
                        GroupBadgeFilter::Mixed,
                        GroupBadgeFilter::Shared,
                        GroupBadgeFilter::Internal,
                    ] {
                        ui.selectable_value(&mut app.group_badge_filter, f, f.label());
                    }
                });

            if ui.button("Clear Selection").clicked() {
                app.selected_duplicate_index = None;
            }
        });

        ui.horizontal(|ui| {
            ui.label("Search file/path:");
            ui.add(
                egui::TextEdit::singleline(&mut app.group_name_filter)
                    .hint_text("contains...")
                    .desired_width(240.0),
            );
            if ui.button("Clear Search").clicked() {
                app.group_name_filter.clear();
            }
        });

        ui.separator();

        if app.last_summary.is_none() {
            ui.label("No results yet.");
            return;
        }

        let summary = app.last_summary.take().expect("summary checked above");
        let target_root = normalize_target_root(&app.target_path);

        let mut folder_cache = app.folder_buckets_cache.take();
        if app.show_folder_grouping && folder_cache.is_none() {
            folder_cache = Some(build_folder_buckets(
                &summary.duplicate_groups,
                target_root.as_deref(),
            ));
        }

        egui::ScrollArea::both()
            .id_salt("dup_scroll_card_style")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if app.show_folder_grouping {
                    if let Some(buckets) = folder_cache.as_deref() {
                        draw_folder_bucket_view(app, ui, buckets, target_root.as_deref());
                    } else {
                        ui.label("No grouped results yet.");
                    }
                } else {
                    draw_flat_group_view(app, ui, &summary.duplicate_groups, target_root.as_deref());
                }
            });

        app.folder_buckets_cache = folder_cache;
        app.last_summary = Some(summary);
    });
}

fn draw_folder_bucket_view(
    app: &mut SameFileApp,
    ui: &mut egui::Ui,
    buckets: &[FolderBucketView],
    target_root: Option<&Path>,
) {
    for bucket in buckets {
        let mut groups = bucket.groups.clone();
        sort_group_views(&mut groups, app.group_sort_mode, target_root);
        groups.retain(|g| group_matches_filters(g, app, target_root));
        if groups.is_empty() {
            continue;
        }

        let file_count_total: usize = groups.iter().map(|g| g.files.len()).sum();
        let header = format!(
            "{} ({} groups / {} files)",
            bucket.folder,
            groups.len(),
            file_count_total
        );

        egui::CollapsingHeader::new(
            egui::RichText::new(header)
                .monospace()
                .color(egui::Color32::from_rgb(205, 205, 205)),
        )
        .id_salt(("folder_bucket", &bucket.folder))
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for badge in [GroupBadge::Mixed, GroupBadge::Shared, GroupBadge::Internal] {
                    if bucket.badges.contains(&badge) {
                        draw_badge_chip(ui, badge);
                    }
                }
            });

            let shares_text = if bucket.related_folders.is_empty() {
                "↔ shares duplicate files within this folder only".to_string()
            } else {
                let related = bucket
                    .related_folders
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("↔ shares duplicate files with: {}", related)
            };

            ui.label(
                egui::RichText::new(shares_text)
                    .italics()
                    .color(egui::Color32::from_rgb(165, 165, 165)),
            );

            ui.add_space(4.0);

            for gv in &groups {
                draw_group_card(app, ui, gv, target_root);
                ui.add_space(4.0);
            }
        });

        ui.separator();
    }
}

fn draw_group_card(app: &mut SameFileApp, ui: &mut egui::Ui, gv: &GroupView, target_root: Option<&Path>) {
    let representative = gv.files.first().cloned();
    let badge = gv
        .badges
        .iter()
        .next()
        .copied()
        .unwrap_or(GroupBadge::Internal);
    let meaning = badge_explain_text(badge);

    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 70, 70)))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| {
                    let chip_text = format!("G{}", gv.group_index);
                    let chip = egui::RichText::new(chip_text)
                        .monospace()
                        .color(egui::Color32::BLACK)
                        .strong();

                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(120, 195, 255))
                        .corner_radius(4.0)
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.label(chip);
                        });

                    ui.label(
                        egui::RichText::new(format!(
                            "hash={} | {} file(s) | {}",
                            gv.hash_hex,
                            gv.files.len(),
                            human_readable_bytes(gv.file_size_bytes)
                        ))
                        .monospace()
                        .color(egui::Color32::from_rgb(210, 210, 210)),
                    );

                    for badge in &gv.badges {
                        draw_badge_chip(ui, *badge);
                    }

                    ui.label(
                        egui::RichText::new(meaning)
                            .italics()
                            .color(egui::Color32::from_rgb(170, 170, 170)),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Open folder").clicked() {
                            if let Some(path) = representative.as_deref() {
                                open_parent_folder(app, path);
                            }
                        }
                        if ui.small_button("Open first").clicked() {
                            if let Some(path) = representative.as_deref() {
                                open_file_direct(app, path);
                            }
                        }
                        if ui.small_button("Copy paths").clicked() {
                            copy_group_paths(app, ui.ctx(), &gv.files);
                        }
                    });
                });

                ui.add_space(2.0);

                for file_path in &gv.files {
                    draw_file_row(app, ui, file_path, target_root);
                }
            });
        });
}

fn draw_flat_group_view(
    app: &mut SameFileApp,
    ui: &mut egui::Ui,
    groups: &[impl GroupLike],
    target_root: Option<&Path>,
) {
    let mut items: Vec<GroupView> = groups
        .iter()
        .enumerate()
        .map(|(idx, group)| {
            let badge = classify_group_badge(group.files(), target_root);
            let mut badges = BTreeSet::new();
            badges.insert(badge);
            GroupView {
                group_index: idx + 1,
                hash_hex: group.hash_hex().to_string(),
                file_size_bytes: group.file_size_bytes(),
                files: sort_group_files(group.files().to_vec(), target_root),
                badges,
            }
        })
        .collect();

    sort_group_views(&mut items, app.group_sort_mode, target_root);
    items.retain(|g| group_matches_filters(g, app, target_root));

    for gv in &items {
        draw_group_card(app, ui, gv, target_root);
        ui.add_space(4.0);
    }
}

fn draw_file_row(app: &mut SameFileApp, ui: &mut egui::Ui, file_path: &Path, target_root: Option<&Path>) {
    let selected = is_selected_path(app, file_path);
    let (_display_rel, file_name, parent_line) = display_parts(file_path, target_root);

    let button_label = format!("{}\n{}", file_name, parent_line);
    let button = egui::Button::new(
        egui::RichText::new(button_label)
            .monospace()
            .color(if selected {
                egui::Color32::from_rgb(235, 246, 255)
            } else {
                egui::Color32::from_rgb(198, 198, 198)
            }),
    )
    .selected(selected)
    .fill(if selected {
        egui::Color32::from_rgba_unmultiplied(90, 140, 210, 90)
    } else {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 4)
    });

    let response = ui
        .add_sized([ui.available_width(), 38.0], button)
        .on_hover_text(file_path.display().to_string());

    if response.clicked() {
        select_path_in_duplicate_rows(app, file_path);
    }

    if response.double_clicked() {
        select_path_in_duplicate_rows(app, file_path);
        app.reveal_selected_in_explorer();
    }
}

fn draw_badge_chip(ui: &mut egui::Ui, badge: GroupBadge) {
    let (label, fill) = match badge {
        GroupBadge::Mixed => ("MIXED", egui::Color32::from_rgb(140, 90, 220)),
        GroupBadge::Shared => ("SHARED", egui::Color32::from_rgb(65, 115, 235)),
        GroupBadge::Internal => ("INTERNAL", egui::Color32::from_rgb(235, 110, 35)),
    };

    egui::Frame::new()
        .fill(fill)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .monospace()
                    .size(11.0)
                    .color(egui::Color32::WHITE),
            );
        });
}

fn build_folder_buckets<T>(groups: &[T], target_root: Option<&Path>) -> Vec<FolderBucketView>
where
    T: GroupLike,
{
    let mut buckets: BTreeMap<String, FolderBucketView> = BTreeMap::new();

    for (idx, group) in groups.iter().enumerate() {
        let files = sort_group_files(group.files().to_vec(), target_root);
        let group_badge = classify_group_badge(&files, target_root);

        let all_parent_folders: BTreeSet<String> = files
            .iter()
            .filter_map(|p| p.parent().map(|d| d.display().to_string()))
            .collect();

        for folder in &all_parent_folders {
            let mut files_in_this_folder: Vec<PathBuf> = files
                .iter()
                .filter(|p| p.parent().map(|d| d.display().to_string()) == Some(folder.clone()))
                .cloned()
                .collect();

            if files_in_this_folder.is_empty() {
                continue;
            }

            files_in_this_folder = sort_group_files(files_in_this_folder, target_root);

            let bucket = buckets
                .entry(folder.clone())
                .or_insert_with(|| FolderBucketView {
                    folder: folder.clone(),
                    groups: Vec::new(),
                    file_count_total: 0,
                    group_count: 0,
                    related_folders: BTreeSet::new(),
                    badges: BTreeSet::new(),
                });

            let mut related = all_parent_folders.clone();
            related.remove(folder);
            bucket.related_folders.extend(related);

            bucket.badges.insert(group_badge);
            bucket.file_count_total += files_in_this_folder.len();
            bucket.group_count += 1;

            let mut badges = BTreeSet::new();
            badges.insert(group_badge);
            bucket.groups.push(GroupView {
                group_index: idx + 1,
                hash_hex: group.hash_hex().to_string(),
                file_size_bytes: group.file_size_bytes(),
                files: files_in_this_folder,
                badges,
            });
        }
    }

    let mut out: Vec<_> = buckets.into_values().collect();
    for b in &mut out {
        b.groups.sort_by_key(|g| g.group_index);
    }
    out
}

fn group_matches_filters(gv: &GroupView, app: &SameFileApp, target_root: Option<&Path>) -> bool {
    let badge = classify_group_badge(&gv.files, target_root);
    let badge_ok = match app.group_badge_filter {
        GroupBadgeFilter::All => true,
        GroupBadgeFilter::Mixed => badge == GroupBadge::Mixed,
        GroupBadgeFilter::Shared => badge == GroupBadge::Shared,
        GroupBadgeFilter::Internal => badge == GroupBadge::Internal,
    };
    if !badge_ok {
        return false;
    }

    let q = app.group_name_filter.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }

    gv.files.iter().any(|p| {
        let s = p.to_string_lossy().to_lowercase();
        s.contains(&q)
    })
}

fn sort_group_views(groups: &mut [GroupView], mode: GroupSortMode, target_root: Option<&Path>) {
    groups.sort_by(|a, b| match mode {
        GroupSortMode::GroupIndexAsc => a.group_index.cmp(&b.group_index),
        GroupSortMode::FileCountDesc => b
            .files
            .len()
            .cmp(&a.files.len())
            .then_with(|| b.file_size_bytes.cmp(&a.file_size_bytes))
            .then_with(|| a.group_index.cmp(&b.group_index)),
        GroupSortMode::SizeDesc => b
            .file_size_bytes
            .cmp(&a.file_size_bytes)
            .then_with(|| b.files.len().cmp(&a.files.len()))
            .then_with(|| a.group_index.cmp(&b.group_index)),
        GroupSortMode::PathAsc => {
            let ap = representative_sort_key(a.files.first().map(PathBuf::as_path), target_root);
            let bp = representative_sort_key(b.files.first().map(PathBuf::as_path), target_root);
            ap.cmp(&bp).then_with(|| a.group_index.cmp(&b.group_index))
        }
    });
}

fn sort_group_files(mut files: Vec<PathBuf>, target_root: Option<&Path>) -> Vec<PathBuf> {
    files.sort_by(|a, b| representative_sort_key(Some(a.as_path()), target_root)
        .cmp(&representative_sort_key(Some(b.as_path()), target_root)));
    files
}

fn representative_sort_key(path: Option<&Path>, target_root: Option<&Path>) -> (u8, String, String) {
    let Some(path) = path else {
        return (9, String::new(), String::new());
    };

    let under_root_rank = match target_root {
        Some(root) if path.starts_with(root) => 0,
        Some(_) => 1,
        None => 0,
    };

    let rel = relative_display_string(path, target_root);
    let lower = rel.to_lowercase();
    let parent = path
        .parent()
        .map(|p| relative_display_string(p, target_root))
        .unwrap_or_default()
        .to_lowercase();

    (under_root_rank, parent, lower)
}

fn is_selected_path(app: &SameFileApp, target: &Path) -> bool {
    let Some(idx) = app.selected_duplicate_index else {
        return false;
    };
    let Some(row) = app.duplicate_rows.get(idx) else {
        return false;
    };
    let Some(path) = &row.path else {
        return false;
    };
    path == target
}

fn select_path_in_duplicate_rows(app: &mut SameFileApp, target: &Path) {
    if let Some(idx) = app.duplicate_row_index_by_path.get(target).copied() {
        app.selected_duplicate_index = Some(idx);
    } else {
        app.selected_duplicate_index = None;
    }
}

fn classify_group_badge(files: &[PathBuf], target_root: Option<&Path>) -> GroupBadge {
    if files.is_empty() {
        return GroupBadge::Internal;
    }

    let mut parent_set = BTreeSet::<String>::new();
    let mut has_under_root = false;
    let mut has_outside_root = false;

    for p in files {
        if let Some(parent) = p.parent() {
            parent_set.insert(parent.display().to_string());
        }

        if let Some(root) = target_root {
            if p.starts_with(root) {
                has_under_root = true;
            } else {
                has_outside_root = true;
            }
        } else {
            has_under_root = true;
        }
    }

    if has_under_root && has_outside_root {
        return GroupBadge::Mixed;
    }

    if parent_set.len() <= 1 {
        GroupBadge::Internal
    } else {
        GroupBadge::Shared
    }
}

fn badge_explain_text(badge: GroupBadge) -> &'static str {
    match badge {
        GroupBadge::Internal => "same folder",
        GroupBadge::Shared => "across folders",
        GroupBadge::Mixed => "includes outside target",
    }
}

fn normalize_target_root(raw: &str) -> Option<PathBuf> {
    let s = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if s.is_empty() {
        return None;
    }
    Some(PathBuf::from(s))
}

fn human_readable_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut idx = 0usize;
    while v >= 1024.0 && idx < UNITS.len() - 1 {
        v /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", bytes, UNITS[idx])
    } else {
        format!("{:.1} {}", v, UNITS[idx])
    }
}

fn relative_display_string(path: &Path, target_root: Option<&Path>) -> String {
    if let Some(root) = target_root {
        if let Ok(rel) = path.strip_prefix(root) {
            let rel_text = rel.display().to_string();
            if rel_text.is_empty() {
                return ".".to_string();
            }
            return rel_text;
        }
    }
    path.display().to_string()
}

fn display_parts(path: &Path, target_root: Option<&Path>) -> (String, String, String) {
    let rel = relative_display_string(path, target_root);
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| rel.clone());
    let parent = path
        .parent()
        .map(|p| relative_display_string(p, target_root))
        .unwrap_or_else(|| ".".to_string());
    (rel, file_name, parent)
}

fn copy_group_paths(app: &mut SameFileApp, ctx: &egui::Context, files: &[PathBuf]) {
    if files.is_empty() {
        app.push_log("[Info] Group has no files.");
        return;
    }
    let text = files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("\r\n");
    ctx.copy_text(text);
    app.push_log(format!("[CopyPaths] {} file(s)", files.len()));
}

fn open_parent_folder(app: &mut SameFileApp, file_path: &Path) {
    let Some(parent) = file_path.parent() else {
        app.push_log(format!("[Info] No parent folder: {}", file_path.display()));
        return;
    };

    #[cfg(target_os = "windows")]
    {
        match std::process::Command::new("explorer").arg(parent).spawn() {
            Ok(_) => app.push_log(format!("[OpenFolder] {}", parent.display())),
            Err(e) => app.push_log(format!(
                "[Error] Failed to open folder: {} ({})",
                parent.display(),
                e
            )),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        app.push_log(format!(
            "[Info] Open folder is only implemented for Windows: {}",
            parent.display()
        ));
    }
}

fn open_file_direct(app: &mut SameFileApp, path: &Path) {
    #[cfg(target_os = "windows")]
    {
        match std::process::Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(path)
            .spawn()
        {
            Ok(_) => app.push_log(format!("[OpenFile] {}", path.display())),
            Err(e) => app.push_log(format!(
                "[Error] Failed to open file: {} ({})",
                path.display(),
                e
            )),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        app.push_log(format!(
            "[Info] Open file is only implemented for Windows: {}",
            path.display()
        ));
    }
}

trait GroupLike {
    fn hash_hex(&self) -> &str;
    fn file_size_bytes(&self) -> u64;
    fn files(&self) -> &[PathBuf];
}

impl GroupLike for crate::core::types::DuplicateGroup {
    fn hash_hex(&self) -> &str {
        &self.hash_hex
    }
    fn file_size_bytes(&self) -> u64 {
        self.file_size_bytes
    }
    fn files(&self) -> &[PathBuf] {
        &self.files
    }
}