//! Buttons that open files in other programs (see `external.rs`); the
//! toolbar's terminal and folder buttons are in `toolbar.rs`.

use std::path::Path;

use eframe::egui;

use crate::command::{Command, Commands};
use crate::external::Launch;

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
