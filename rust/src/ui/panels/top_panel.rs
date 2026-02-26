use eframe::egui;

use crate::ui::state::SameFileApp;

pub fn draw_top_panel(app: &mut SameFileApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
        ui.heading("SameFile_Finder v2 (Rust / egui)");

        ui.horizontal(|ui| {
            ui.label("Target Path:");
            ui.add(egui::TextEdit::singleline(&mut app.target_path).desired_width(520.0));

            if ui
                .add_enabled(!app.is_running, egui::Button::new("Browse"))
                .clicked()
            {
                app.browse_folder();
            }
        });

        ui.horizontal(|ui| {
            ui.label("Exclude ext:");
            ui.add_enabled(
                !app.is_running,
                egui::TextEdit::singleline(&mut app.exclude_extensions_input)
                    .hint_text("comma-separated, e.g. lrc, txt, jpg")
                    .desired_width(320.0),
            );

            ui.label(
                egui::RichText::new("(comma-separated / '.' optional / case-insensitive)")
                    .small()
                    .color(egui::Color32::GRAY),
            );
        });

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!app.is_running, egui::Button::new("Run"))
                .clicked()
            {
                app.start_scan_async();
            }

            if ui
                .add_enabled(app.is_running, egui::Button::new("Cancel"))
                .clicked()
            {
                app.request_cancel();
            }

            if ui.button("Clear Logs").clicked() {
                app.logs.clear();
            }

            if ui.button("Export CSV").clicked() {
                app.export_csv();
            }

            let has_selection = app.selected_path().is_some();

            if ui
                .add_enabled(has_selection, egui::Button::new("Open Folder"))
                .clicked()
            {
                app.open_selected_folder();
            }

            if ui
                .add_enabled(has_selection, egui::Button::new("Open File"))
                .clicked()
            {
                app.open_selected_file();
            }

            if ui
                .add_enabled(has_selection, egui::Button::new("Copy Path"))
                .clicked()
            {
                app.copy_selected_path(ctx);
            }

            if ui
                .add_enabled(has_selection, egui::Button::new("Reveal"))
                .clicked()
            {
                app.reveal_selected_in_explorer();
            }
        });

        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Cache DB:");
            ui.label(
                egui::RichText::new(&app.cache_db_path)
                    .small()
                    .color(egui::Color32::GRAY),
            );

            if ui.button("Cache Stats").clicked() {
                app.refresh_cache_stats();
            }

            if ui.button("GC Missing").clicked() {
                app.gc_cache_missing_paths();
            }

            if ui.button("VACUUM").clicked() {
                app.vacuum_cache_db();
            }
        });

        ui.label(&app.status_text);
    });
}