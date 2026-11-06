mod app;
mod command;
mod icon;
mod jobs;
mod settings;
mod tab;
mod ui;
#[cfg(test)]
mod ui_tests;
mod util;

fn main() -> eframe::Result {
    let paths: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Ferrum")
            .with_app_id("ferrum")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 560.0])
            .with_icon(std::sync::Arc::new(eframe::egui::IconData {
                rgba: icon::rgba(128),
                width: 128,
                height: 128,
            })),
        ..Default::default()
    };

    eframe::run_native(
        "Ferrum",
        options,
        Box::new(move |cc| Ok(Box::new(app::FerrumApp::new(cc, paths)))),
    )
}
