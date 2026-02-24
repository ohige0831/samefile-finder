mod app;
mod core;
mod adapters;
mod ui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();

    eframe::run_native(
        "SameFile_Finder v2 (Rust)",
        options,
        Box::new(|_cc| Ok(Box::new(ui::egui_app::SameFileFinderApp::default()))),
    )
}