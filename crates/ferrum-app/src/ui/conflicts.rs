//! Three-way conflict resolver. (Placeholder — to be implemented.)

use eframe::egui;

use crate::command::Commands;
use crate::tab::RepoTab;

#[derive(Default)]
pub struct ConflictState {
    pub path: Option<String>,
}

/// Opens the resolver for `path`.
pub fn open(tab: &mut RepoTab, path: String) {
    tab.conflicts.path = Some(path);
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, _cmds: &mut Commands) {
    ui.label(format!(
        "Conflict resolver for {:?} — not implemented yet",
        tab.conflicts.path
    ));
}
