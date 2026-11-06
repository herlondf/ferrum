#[macro_use]
mod i18n;

mod accent;
mod app;
mod benchmark;
mod command;
mod commit_prefs;
mod desktop_notify;
#[cfg(test)]
mod diff_tests;
mod external;
#[cfg(test)]
mod history_tests;
mod icon;
mod jobs;
mod prefs;
mod ref_ops;
mod refresh;
#[cfg(test)]
mod refs_tests;
mod settings;
mod tab;
mod ui;
#[cfg(test)]
mod ui_tests;
mod util;
mod watcher;

fn main() -> eframe::Result {
    let paths: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    if paths
        .first()
        .is_some_and(|a| a.as_os_str() == "--benchmark")
    {
        let repo = paths.get(1).cloned().unwrap_or_else(|| ".".into());
        let runs = paths
            .get(2)
            .and_then(|r| r.to_str()?.parse().ok())
            .unwrap_or(3);
        std::process::exit(benchmark::main(&repo, runs));
    }

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
        persistence_path: data_dir(),
        ..Default::default()
    };

    eframe::run_native(
        "Ferrum",
        options,
        Box::new(move |cc| Ok(Box::new(app::FerrumApp::new(cc, paths)))),
    )
}

/// Where settings are saved. `FERRUM_DATA_DIR` overrides it, and screenshot
/// runs use a throwaway folder so development never touches real settings.
fn data_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("FERRUM_DATA_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("FERRUM_SCREENSHOT")
                .map(|_| std::env::temp_dir().join("ferrum-screenshot"))
        })?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("app.ron"))
}
