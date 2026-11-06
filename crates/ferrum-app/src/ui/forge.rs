//! Pull/merge requests on GitHub and GitLab. (Placeholder — to be implemented.)

use eframe::egui;
use ferrum_core::forge::{ForgeKind, PullRequest};

use crate::command::Commands;
use crate::settings::Settings;
use crate::tab::RepoTab;

#[derive(Default)]
pub struct PrState {
    pub list: Option<Vec<PullRequest>>,
    pub error: Option<String>,
}

pub enum ForgeDialog {
    NewPullRequest {
        branch: String,
    },
    Login {
        kind: ForgeKind,
        host: String,
        token: String,
    },
}

pub fn new_pull_request(_tab: &RepoTab, branch: &str) -> ForgeDialog {
    ForgeDialog::NewPullRequest {
        branch: branch.into(),
    }
}

pub fn login(kind: ForgeKind, host: &str) -> ForgeDialog {
    ForgeDialog::Login {
        kind,
        host: host.into(),
        token: String::new(),
    }
}

/// The pull requests view (`View::PullRequests`).
pub fn show_view(
    ui: &mut egui::Ui,
    _tab: &mut RepoTab,
    _settings: &Settings,
    _cmds: &mut Commands,
) {
    ui.label("Pull requests — not implemented yet");
}

/// Body of a forge dialog inside the modal. Returns `false` to close it.
pub fn show_dialog(
    ui: &mut egui::Ui,
    _d: &mut ForgeDialog,
    _tab: Option<&RepoTab>,
    _settings: &mut Settings,
    _cmds: &mut Commands,
) -> bool {
    ui.label("Not implemented yet");
    !ui.button("Close").clicked()
}
