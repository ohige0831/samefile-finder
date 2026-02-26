use eframe::egui;

use crate::ui::state::SameFileApp;

pub fn draw_log_panel(app: &mut SameFileApp, ui: &mut egui::Ui) {
    ui.group(|ui| {
        ui.heading("Logs");
        ui.separator();

        egui::ScrollArea::both()
            .id_salt("logs_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for line in &app.logs {
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
}

pub fn draw_summary_panel(app: &SameFileApp, ui: &mut egui::Ui) {
    ui.group(|ui| {
        ui.heading("Run Summary");
        ui.separator();

        ui.label(egui::RichText::new("Cache (v2.3.0)").strong());
        ui.label(
            egui::RichText::new(format!("DB: {}", app.cache_db_path))
                .small()
                .color(egui::Color32::GRAY),
        );
        ui.horizontal(|ui| {
            ui.label("Entries:");
            ui.label(
                app.cache_entries
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "(unknown)".to_string()),
            );
            ui.add_space(12.0);
            ui.label("Size:");
            ui.label(
                app.cache_db_size_bytes
                    .map(|v| format!("{} bytes", v))
                    .unwrap_or_else(|| "(unknown)".to_string()),
            );
        });
        ui.separator();

        let Some(summary) = &app.last_summary else {
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