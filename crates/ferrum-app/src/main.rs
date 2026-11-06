mod app;
mod command;
mod jobs;
mod settings;
mod tab;
mod ui;
mod util;

fn main() -> eframe::Result {
    let paths: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Ferrum")
            .with_app_id("ferrum")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Ferrum",
        options,
        Box::new(move |cc| Ok(Box::new(app::FerrumApp::new(cc, paths)))),
    )
}
