use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eframe::egui;

use crate::ui::state::{FolderBucketView, GroupBadge, GroupView, SameFileApp};

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

            if ui.button("Clear Selection").clicked() {
                app.selected_duplicate_index = None;
            }
        });

        ui.separator();

        if app.last_summary.is_none() {
            ui.label("No results yet.");
            return;
        }

        // v2.1.3: last_summary.clone() を避けるため一時的にムーブして描画後に戻す
        let summary = app.last_summary.take().expect("summary checked above");

        let target_root = normalize_target_root(&app.target_path);

        // v2.1.3: フォルダ集計は毎フレーム再構築せずキャッシュ（必要時のみ構築）
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
                        draw_folder_bucket_view(app, ui, buckets);
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

fn draw_folder_bucket_view(app: &mut SameFileApp, ui: &mut egui::Ui, buckets: &[FolderBucketView]) {
    for bucket in buckets {
        let header = format!(
            "{} ({} groups / {} files)",
            bucket.folder, bucket.group_count, bucket.file_count_total
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

            for gv in &bucket.groups {
                draw_group_card(app, ui, gv);
                ui.add_space(4.0);
            }
        });

        ui.separator();
    }
}

fn draw_group_card(app: &mut SameFileApp, ui: &mut egui::Ui, gv: &GroupView) {
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
                            "Group {} | hash={} | {} file(s) | {} bytes",
                            gv.group_index,
                            gv.hash_hex,
                            gv.files.len(),
                            gv.file_size_bytes
                        ))
                        .monospace()
                        .color(egui::Color32::from_rgb(210, 210, 210)),
                    );

                    for badge in &gv.badges {
                        draw_badge_chip(ui, *badge);
                    }
                });

                ui.add_space(2.0);

                for file_path in &gv.files {
                    draw_file_row(app, ui, file_path);
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
    for (idx, group) in groups.iter().enumerate() {
        let files = group.files();
        let badge = classify_group_badge(files, target_root);

        let header_color = match badge {
            GroupBadge::Internal => egui::Color32::from_rgb(120, 210, 140),
            GroupBadge::Shared => egui::Color32::from_rgb(120, 180, 255),
            GroupBadge::Mixed => egui::Color32::from_rgb(230, 200, 110),
        };

        let header_text = format!(
            "[Group {}] {}  hash={}  count={}  size={} bytes",
            idx + 1,
            badge_label(badge),
            group.hash_hex(),
            files.len(),
            group.file_size_bytes()
        );

        egui::CollapsingHeader::new(
            egui::RichText::new(header_text)
                .monospace()
                .strong()
                .color(header_color),
        )
        .id_salt(("flat_group", idx))
        .default_open(false)
        .show(ui, |ui| {
            for file_path in files {
                draw_file_row(app, ui, file_path);
            }
        });

        ui.add_space(4.0);
    }
}

fn draw_file_row(app: &mut SameFileApp, ui: &mut egui::Ui, file_path: &PathBuf) {
    let selected = is_selected_path(app, file_path);

    let response = ui.selectable_label(
        selected,
        egui::RichText::new(file_path.display().to_string())
            .monospace()
            .color(if selected {
                egui::Color32::from_rgb(230, 245, 255)
            } else {
                egui::Color32::from_rgb(190, 190, 190)
            }),
    );

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
        let files = group.files();
        let group_badge = classify_group_badge(files, target_root);

        let all_parent_folders: BTreeSet<String> = files
            .iter()
            .filter_map(|p| p.parent().map(|d| d.display().to_string()))
            .collect();

        for folder in &all_parent_folders {
            let files_in_this_folder: Vec<PathBuf> = files
                .iter()
                .filter(|p| p.parent().map(|d| d.display().to_string()) == Some(folder.clone()))
                .cloned()
                .collect();

            if files_in_this_folder.is_empty() {
                continue;
            }

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

            bucket.groups.push(GroupView {
                group_index: idx + 1,
                hash_hex: group.hash_hex().to_string(),
                file_size_bytes: group.file_size_bytes(),
                files: files_in_this_folder,
                badges: {
                    let mut s = BTreeSet::new();
                    s.insert(group_badge);
                    s
                },
            });
        }
    }

    let mut out: Vec<_> = buckets.into_values().collect();
    for b in &mut out {
        b.groups.sort_by_key(|g| g.group_index);
    }
    out
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

fn badge_label(badge: GroupBadge) -> &'static str {
    match badge {
        GroupBadge::Internal => "INTERNAL",
        GroupBadge::Shared => "SHARED",
        GroupBadge::Mixed => "MIXED",
    }
}

fn normalize_target_root(raw: &str) -> Option<PathBuf> {
    let s = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if s.is_empty() {
        return None;
    }
    Some(PathBuf::from(s))
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