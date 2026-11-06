//! Opening submodules and worktrees next to their parent tab, and the
//! question after a checkout when submodules need an update.

use std::path::PathBuf;

use super::FerrumApp;
use crate::ui::dialogs::Dialog;

impl FerrumApp {
    /// Opens `path` in a new tab placed right after the active one.
    pub(super) fn open_beside(&mut self, path: PathBuf) {
        let parent = self.active;
        let before = self.tabs.len();
        self.open(path);
        if self.tabs.len() > before
            && parent + 1 < self.tabs.len() - 1
            && let Some(tab) = self.tabs.pop()
        {
            self.tabs.insert(parent + 1, tab);
            self.active = parent + 1;
        }
    }

    /// Asks whether to update the submodules that moved with the last checkout.
    pub(super) fn ask_submodule_update(&mut self, list: Vec<String>) {
        if list.is_empty() || self.dialog.is_some() {
            return;
        }
        let text = t!(
            "These submodules are not at the commit recorded by the repository:\n\n{}\n\nUpdate them now?",
            list.join("\n")
        );
        self.dialog = Some(Dialog::confirm(
            &t!("Update submodules"),
            &text,
            crate::repo_parts::update_all(),
            false,
        ));
    }
}
