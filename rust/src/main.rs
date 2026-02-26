use eframe::egui;

mod adapters;
mod app;
mod core;
mod ui;

fn try_load_japanese_font_bytes() -> Option<Vec<u8>> {
    let candidates = [
        // プロジェクト/配布物同梱（推奨）
        "assets/fonts/NotoSansJP-Regular.ttf",
        "assets/fonts/BIZUDGothic-Regular.ttf",
        "assets/fonts/Meiryo.ttf",
        // 実行ファイル相対の候補
        "./assets/fonts/NotoSansJP-Regular.ttf",
        // Windows標準/Office系でTTFがある場合の候補（一部環境のみ）
        r"C:\Windows\Fonts\arialuni.ttf",
        r"F:\code_vault\30_tools\SameFile_Finder\samefile-finder\rust\src\assets\fonts\NotoSansJP-VF.ttf",
    ];

    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }
    }

    None
}

fn setup_japanese_font(cc: &eframe::CreationContext<'_>) {
    let mut fonts = egui::FontDefinitions::default();

    if let Some(bytes) = try_load_japanese_font_bytes() {
        fonts.font_data.insert(
            "jp_font".to_owned(),
            egui::FontData::from_owned(bytes).into(),
        );

        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "jp_font".to_owned());

        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "jp_font".to_owned());

        cc.egui_ctx.set_fonts(fonts);
    }
}

fn main() -> eframe::Result<()> {
    let title = "SameFile_Finder v2.3.1 (Rust)";
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        title,
        options,
        Box::new(|cc| {
            setup_japanese_font(cc);
            Ok(Box::new(ui::SameFileApp::default()))
        }),
    )
}