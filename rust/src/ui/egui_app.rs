use eframe::egui;

use crate::ui::panels::{
    log_panel::{draw_log_panel, draw_summary_panel},
    results_panel::draw_results_panel,
    top_panel::draw_top_panel,
};
use crate::ui::state::SameFileApp;

impl eframe::App for SameFileApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker_messages();

        if self.is_running {
            ctx.request_repaint();
        }

        draw_top_panel(self, ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |cols| {
                draw_log_panel(self, &mut cols[0]);

                cols[1].vertical(|ui| {
                    draw_summary_panel(self, ui);
                    ui.add_space(8.0);
                    draw_results_panel(self, ui);
                });
            });
        });
    }
}
