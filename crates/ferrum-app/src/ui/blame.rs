//! File history and blame. (Placeholder — to be implemented.)

use eframe::egui;

use crate::command::Commands;
use crate::tab::{RepoTab, View};

pub struct FileView {
    pub path: String,
    pub blame: bool,
}

pub fn open(tab: &mut RepoTab, path: String, blame: bool) {
    tab.file_view = Some(FileView { path, blame });
    tab.view = View::File;
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, _cmds: &mut Commands) {
    if let Some(v) = &tab.file_view {
        ui.label(format!(
            "{} — {} not implemented yet",
            v.path,
            if v.blame { "blame" } else { "history" }
        ));
    }
}
