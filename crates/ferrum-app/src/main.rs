#[macro_use]
mod i18n;

mod accent;
mod app;
mod benchmark;
mod command;
#[cfg(test)]
mod commands_tests;
mod commit_prefs;
mod custom_commands;
mod desktop_notify;
#[cfg(test)]
mod details_tests;
#[cfg(test)]
mod diff_tests;
mod external;
#[cfg(test)]
mod file_history_tests;
mod flow;
#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod fork_tools_tests;
#[cfg(test)]
mod history_tests;
mod icon;
mod jobs;
mod keymap;
#[cfg(test)]
mod parts_tests;
mod prefs;
mod ref_ops;
mod refresh;
#[cfg(test)]
mod refs_tests;
mod repo_parts;
mod repo_settings;
mod repos;
#[cfg(test)]
mod repos_tests;
mod settings;
mod single_instance;
mod sync;
#[cfg(test)]
mod sync_tests;
mod tab;
mod ui;
#[cfg(test)]
mod ui_tests;
mod util;
mod watcher;

fn main() -> eframe::Result {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let paths: Vec<std::path::PathBuf> = args.iter().map(Into::into).collect();
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

    // `ferrum PATH`, `ferrum log -- FILE` and "Open in Desktop" links.
    let cwd = std::env::current_dir().unwrap_or_default();
    let requests = repos::parse_args(&args, &cwd);
    let mut listener = None;
    if single_instance::enabled()
        && let Some(dir) = instance_dir()
    {
        match single_instance::start(&dir, &requests) {
            single_instance::Start::Forwarded => return Ok(()),
            single_instance::Start::Serve(l) => listener = Some(l),
            single_instance::Start::Alone => {}
        }
    }
    let (open, other): (Vec<repos::Request>, Vec<repos::Request>) = requests
        .into_iter()
        .partition(|r| matches!(r, repos::Request::Open(p) if ferrum_core::Repo::open(p).is_ok()));
    let paths: Vec<std::path::PathBuf> = open
        .into_iter()
        .filter_map(|r| match r {
            repos::Request::Open(p) => Some(p),
            _ => None,
        })
        .collect();

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
        Box::new(move |cc| {
            let app = app::FerrumApp::new(cc, paths).with_launch(other, listener);
            Ok(Box::new(app))
        }),
    )
}

/// Folder of the single-instance socket: the data folder.
fn instance_dir() -> Option<std::path::PathBuf> {
    match data_dir() {
        Some(file) => file.parent().map(Into::into),
        None => eframe::storage_dir("Ferrum"),
    }
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
