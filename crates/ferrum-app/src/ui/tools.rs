//! Buttons and menu entries that open the repository in other programs
//! (see `external.rs`).

use std::path::Path;

use eframe::egui;

use crate::command::{Command, Commands};
use crate::external::Launch;
use crate::prefs::{Prefs, ToolbarButton};

/// Entries of the Repository menu for the open repository.
pub fn repository_menu(ui: &mut egui::Ui, repo: &Path, cmds: &mut Commands) {
    let entries = [
        (
            t!("Open in terminal"),
            Launch::Terminal { dir: repo.into() },
        ),
        (
            t!("Show in file manager"),
            Launch::Reveal { path: repo.into() },
        ),
        (
            t!("Open in editor"),
            Launch::Editor {
                path: repo.into(),
                line: None,
            },
        ),
    ];
    for (label, launch) in entries {
        if ui.button(label).clicked() {
            cmds.push(Command::External(launch));
            ui.close();
        }
    }
}

/// The terminal and file manager toolbar buttons (each can be hidden in Settings).
pub fn toolbar_buttons(ui: &mut egui::Ui, prefs: &Prefs, repo: &Path, cmds: &mut Commands) {
    let terminal = prefs.shows(ToolbarButton::Terminal);
    let files = prefs.shows(ToolbarButton::FileManager);
    if !terminal && !files {
        return;
    }
    ui.separator();
    if terminal
        && ui
            .button(">_")
            .on_hover_text(t!("Open in terminal"))
            .clicked()
    {
        cmds.push(Command::External(Launch::Terminal { dir: repo.into() }));
    }
    if files
        && ui
            .button("📁")
            .on_hover_text(t!("Show in file manager"))
            .clicked()
    {
        cmds.push(Command::External(Launch::Reveal { path: repo.into() }));
    }
}

/// "Open in merge tool" for a conflicted file (relative to `repo`).
pub fn merge_tool_button(ui: &mut egui::Ui, repo: &Path, path: &str, cmds: &mut Commands) {
    if ui
        .button(t!("Open in merge tool"))
        .on_hover_text(t!(
            "Resolve the file in the external merge tool chosen in Settings > Tools"
        ))
        .clicked()
    {
        cmds.push(Command::External(Launch::Merge {
            repo: repo.into(),
            path: path.into(),
        }));
    }
}
