//! Repository settings dialog, and the fixes offered for a stale
//! `index.lock` and for folders git does not trust (`safe.directory`).

use std::path::PathBuf;

use eframe::egui;
use ferrum_core::library;

use crate::command::{Command, Commands, Op};
use crate::repos::{RepoCmd, RepoConfig, RepoOp};
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::dialogs::{Dialog, buttons};

pub enum RepoDialog {
    /// Settings of a repository, shared by its worktrees.
    Settings {
        key: String,
        remotes: Vec<String>,
        pr_remote: String,
        template: String,
        template_before: String,
    },
    /// Git refused a folder owned by someone else.
    Trust { path: PathBuf, error: String },
}

pub fn settings(tab: &RepoTab, s: &Settings) -> Dialog {
    let key = crate::repo_settings::shared_key(tab);
    let template = tab
        .git
        .run(&["config", "--local", "--get", "commit.template"])
        .unwrap_or_default()
        .trim()
        .to_owned();
    Dialog::Repo(RepoDialog::Settings {
        pr_remote: s.library.config(&key).pr_remote,
        key,
        remotes: tab.remotes.iter().map(|r| r.name.clone()).collect(),
        template_before: template.clone(),
        template,
    })
}

pub fn show(
    ui: &mut egui::Ui,
    d: &mut RepoDialog,
    tab: Option<&RepoTab>,
    settings: &mut Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    match d {
        RepoDialog::Settings {
            key,
            remotes,
            pr_remote,
            template,
            template_before,
        } => {
            ui.heading(t!("Repository settings"));
            if let Some(t) = tab {
                ui.weak(t.path().display().to_string());
            }
            ui.label(t!("Shared by all worktrees of this repository."));
            ui.add_space(8.0);
            ui.strong(t!("Issue tracker"));
            ui.horizontal(|ui| {
                ui.label(t!("Links from commit messages to issues."));
                if ui.button(t!("Edit issue link rules…")).clicked() {
                    cmds.push(crate::ui::history_options::cmd(|t, _| {
                        t.history.ext.issue_window = true
                    }));
                }
            });
            ui.add_space(8.0);
            ui.strong(t!("Pull requests"));
            ui.horizontal(|ui| {
                ui.label(t!("Remote"));
                let shown = if pr_remote.is_empty() {
                    t!("origin (default)")
                } else {
                    pr_remote.clone()
                };
                egui::ComboBox::from_id_salt("pr-remote")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(pr_remote, String::new(), t!("origin (default)"));
                        for r in remotes.iter() {
                            ui.selectable_value(pr_remote, r.clone(), r);
                        }
                    });
            });
            ui.weak(t!(
                "Its host is used for pull requests and issue links like #12."
            ));
            ui.add_space(8.0);
            ui.strong(t!("Commit template"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(template)
                        .hint_text(t!("No template"))
                        .desired_width(340.0),
                );
                if ui.button(t!("Browse…")).clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_file()
                {
                    *template = p.to_string_lossy().into_owned();
                }
            });
            ui.weak(t!(
                "Fills an empty commit message (git config commit.template)."
            ));
            match buttons(ui, &t!("Save"), true, false, enter) {
                Some(true) => {
                    let c = RepoConfig {
                        pr_remote: pr_remote.clone(),
                    };
                    settings.library.set_config(key.clone(), c);
                    if template.trim() != template_before.trim() {
                        let t = Some(template.trim().to_owned()).filter(|t| !t.is_empty());
                        cmds.run(Op::Repo(RepoOp::CommitTemplate(t)));
                    }
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        RepoDialog::Trust { path, error } => {
            ui.heading(t!("Trust this folder?"));
            ui.label(t!(
                "Git refuses to open {} because it belongs to another user.",
                path.display()
            ));
            ui.add(egui::Label::new(egui::RichText::new(error.as_str()).monospace().weak()).wrap());
            ui.label(t!(
                "Adding it to safe.directory in your global git config lets git work in it."
            ));
            match buttons(ui, &t!("Trust and open"), true, false, enter) {
                Some(true) => {
                    cmds.push(Command::Repos(RepoCmd::TrustFolder(path.clone())));
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
    }
}

/// Buttons next to an error in the status bar that fix it.
pub fn error_actions(ui: &mut egui::Ui, error: &str, cmds: &mut Commands) {
    if let Some(lock) = library::lock_file_in_error(error)
        && lock.exists()
        && ui
            .small_button(t!("Delete the lock file"))
            .on_hover_text(lock.display().to_string())
            .clicked()
    {
        let text = t!(
            "Git left {} behind, probably after a crash. Delete it only when no other git program is working on this repository.",
            lock.display()
        );
        cmds.push(Command::Dialog(Dialog::confirm(
            &t!("Delete the lock file"),
            &text,
            Op::Repo(RepoOp::DeleteLock(lock)),
            true,
        )));
    }
}
