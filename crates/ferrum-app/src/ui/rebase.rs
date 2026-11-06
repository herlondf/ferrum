//! Interactive rebase planner. (Placeholder — to be implemented.)

use eframe::egui;
use ferrum_core::Oid;

use crate::command::Commands;
use crate::tab::RepoTab;

pub struct RebasePlan {
    pub base: Option<Oid>,
}

/// Builds a plan for the commits after `base` (`None` = from the root).
pub fn plan(_tab: &RepoTab, base: Option<Oid>) -> Result<RebasePlan, String> {
    Ok(RebasePlan { base })
}

/// Shows the planner window while `tab.rebase` is set.
pub fn show(ctx: &egui::Context, tab: &mut RepoTab, _cmds: &mut Commands) {
    let mut open = true;
    egui::Window::new("Interactive rebase")
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("Not implemented yet");
        });
    if !open {
        tab.rebase = None;
    }
}
