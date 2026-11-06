//! Local changes: staging area and commit box.

use eframe::egui;
use ferrum_core::diff::{partial_patch, supports_line_staging};
use ferrum_core::ops::PatchTarget;
use ferrum_core::{ChangeKind, DiffSource, FileChange, RepoState};

use crate::command::{Command, Commands, Op};
use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;
use crate::ui::diff_view::{self, Action, DiffAction, DiffMode};
use crate::ui::image_diff;
use crate::util::{change_label, is_image_path};

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    egui::Panel::left("changes-lists")
        .resizable(true)
        .default_size(380.0)
        .show(ui, |ui| {
            egui::Panel::bottom("commit-box").show(ui, |ui| commit_box(ui, tab, cmds));
            egui::CentralPanel::default_margins().show(ui, |ui| lists(ui, tab, cmds));
        });
    egui::CentralPanel::default_margins().show(ui, |ui| diff(ui, tab, cmds));
}

fn lists(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    let busy = tab.busy();
    if !tab.status.conflicted.is_empty() {
        ui.horizontal(|ui| {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("⚠ Conflicts ({})", tab.status.conflicted.len()),
            );
        });
        for f in &tab.status.conflicted {
            let job = change_label(ui, ChangeKind::Conflicted, &f.path, None);
            if ui
                .add(egui::Button::selectable(false, job).truncate())
                .on_hover_text("Open the conflict resolver")
                .clicked()
            {
                cmds.push(Command::ResolveConflict(f.path.clone()));
            }
        }
        ui.separator();
    }

    let half = (ui.available_height() / 2.0 - 30.0).max(80.0);

    ui.horizontal(|ui| {
        ui.strong(format!("Unstaged ({})", tab.status.unstaged.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(!busy && !tab.status.unstaged.is_empty(), |ui| {
                if ui.small_button("Discard all…").clicked() {
                    cmds.push(Command::Dialog(Dialog::confirm(
                        "Discard all changes",
                        "Throw away all unstaged changes, including new files? This cannot be undone.",
                        Op::Discard(tab.status.unstaged.clone()),
                        true,
                    )));
                }
                if ui.small_button("Stage all").clicked() {
                    cmds.run(Op::StageAll);
                }
            });
        });
    });
    egui::ScrollArea::vertical()
        .id_salt("unstaged")
        .max_height(half)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let files = tab.status.unstaged.clone();
            file_rows(ui, tab, cmds, &files, DiffSource::Unstaged);
            if files.is_empty() {
                ui.weak("No unstaged changes.");
            }
        });
    ui.separator();
    ui.horizontal(|ui| {
        ui.strong(format!("Staged ({})", tab.status.staged.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    !busy && !tab.status.staged.is_empty(),
                    egui::Button::new("Unstage all").small(),
                )
                .clicked()
            {
                cmds.run(Op::UnstageAll);
            }
        });
    });
    egui::ScrollArea::vertical()
        .id_salt("staged")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let files = tab.status.staged.clone();
            file_rows(ui, tab, cmds, &files, DiffSource::Staged);
            if files.is_empty() {
                ui.weak("Nothing staged yet.");
            }
        });
}

fn file_rows(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    cmds: &mut Commands,
    files: &[FileChange],
    source: DiffSource,
) {
    let busy = tab.busy();
    for f in files {
        let selected = tab
            .changes
            .selected
            .as_ref()
            .is_some_and(|(s, p)| *s == source && p == &f.path);
        let job = change_label(ui, f.kind, &f.path, f.old_path.as_deref());
        let resp = ui.add(egui::Button::selectable(selected, job).truncate());
        if resp.clicked() {
            tab.select_change(source, f.path.clone());
        }
        let toggle = || match source {
            DiffSource::Staged => Op::Unstage(vec![f.path.clone()]),
            _ => Op::Stage(vec![f.path.clone()]),
        };
        if resp.double_clicked() && !busy {
            cmds.run(toggle());
        }
        resp.on_hover_text(format!(
            "{}\nDouble-click to {}",
            f.path,
            if source == DiffSource::Staged {
                "unstage"
            } else {
                "stage"
            }
        ))
        .context_menu(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                let label = if source == DiffSource::Staged {
                    "Unstage"
                } else {
                    "Stage"
                };
                if ui.button(label).clicked() {
                    cmds.run(toggle());
                    ui.close();
                }
                if source == DiffSource::Unstaged && ui.button("Discard changes…").clicked() {
                    let what = if f.kind == ChangeKind::Untracked {
                        "Delete the new file"
                    } else {
                        "Discard the changes in"
                    };
                    cmds.push(Command::Dialog(Dialog::confirm(
                        "Discard changes",
                        &format!("{what} {}? This cannot be undone.", f.path),
                        Op::Discard(vec![f.clone()]),
                        true,
                    )));
                    ui.close();
                }
            });
            ui.separator();
            if ui.button("Copy path").clicked() {
                cmds.push(Command::Copy(f.path.clone()));
                ui.close();
            }
            if f.kind != ChangeKind::Untracked && f.kind != ChangeKind::Added {
                if ui.button("File history").clicked() {
                    cmds.push(Command::FileHistory(f.path.clone()));
                    ui.close();
                }
                if ui.button("Blame").clicked() {
                    cmds.push(Command::Blame(f.path.clone()));
                    ui.close();
                }
            }
            if f.kind == ChangeKind::Untracked && ui.button("Add to .gitignore").clicked() {
                add_to_gitignore(tab, &f.path, cmds);
                ui.close();
            }
        });
    }
}

fn add_to_gitignore(tab: &RepoTab, path: &str, cmds: &mut Commands) {
    let file = tab.path().join(".gitignore");
    let mut content = std::fs::read_to_string(&file).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&format!("/{path}\n"));
    match std::fs::write(&file, content) {
        Ok(()) => cmds.push(Command::Refresh),
        Err(e) => cmds.push(Command::Error(format!("Could not update .gitignore: {e}"))),
    }
}

fn diff(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    let Some((source, path)) = tab.changes.selected.clone() else {
        ui.centered_and_justified(|ui| {
            ui.weak(if tab.status.is_empty() {
                "No local changes."
            } else {
                "Select a file to see its changes."
            })
        });
        return;
    };
    let Some(file) = tab.changes.diff.clone() else {
        ui.weak("No changes to show.");
        return;
    };
    ui.horizontal(|ui| {
        ui.strong(&path);
        ui.weak(if source == DiffSource::Staged {
            "staged"
        } else {
            "unstaged"
        });
    });
    if file.binary && is_image_path(&file.path) {
        image_diff::show(ui, &tab.repo, &file, source);
        return;
    }
    let mode = if source == DiffSource::Staged {
        DiffMode::Staged
    } else {
        DiffMode::Unstaged
    };
    let line_staging = supports_line_staging(&file);
    let action = diff_view::show(
        ui,
        "changes-diff",
        &file,
        mode,
        line_staging,
        Some(&mut tab.changes.selection),
    );
    let Some(action) = action else { return };
    if tab.busy() {
        tab.error("Wait for the current operation to finish.");
        return;
    }

    let change = FileChange {
        path: file.path.clone(),
        old_path: file.old_path.clone(),
        kind: file.kind,
    };
    let selection = tab.changes.selection.lines.clone();
    let (act, patch) = match action {
        DiffAction::Hunk(a, h) => (
            a,
            line_staging
                .then(|| partial_patch(&file, |hh, _| hh == h, a != Action::Stage))
                .flatten(),
        ),
        DiffAction::Lines(a) => (
            a,
            partial_patch(
                &file,
                |h, l| selection.contains(&(h, l)),
                a != Action::Stage,
            ),
        ),
    };
    let whole_file = !line_staging;
    let op = match (act, patch) {
        (Action::Stage, Some(patch)) => Op::ApplyPatch {
            patch,
            target: PatchTarget::Index,
            reverse: false,
            label: "Stage selection",
        },
        (Action::Unstage, Some(patch)) => Op::ApplyPatch {
            patch,
            target: PatchTarget::Index,
            reverse: true,
            label: "Unstage selection",
        },
        (Action::Discard, Some(patch)) => {
            let op = Op::ApplyPatch {
                patch,
                target: PatchTarget::WorkingCopy,
                reverse: true,
                label: "Discard selection",
            };
            cmds.push(Command::Dialog(Dialog::confirm(
                "Discard changes",
                "Discard the selected changes? This cannot be undone.",
                op,
                true,
            )));
            return;
        }
        (Action::Stage, None) if whole_file => Op::Stage(vec![change.path]),
        (Action::Unstage, None) if whole_file => Op::Unstage(vec![change.path]),
        (Action::Discard, None) if whole_file => {
            cmds.push(Command::Dialog(Dialog::confirm(
                "Discard changes",
                &format!(
                    "Discard the changes in {}? This cannot be undone.",
                    change.path
                ),
                Op::Discard(vec![change]),
                true,
            )));
            return;
        }
        _ => return,
    };
    tab.changes.selection.clear();
    cmds.run(op);
}

fn commit_box(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    ui.add_space(4.0);
    // Merges, reverts and cherry-picks come with a prepared message.
    if tab.changes.message.is_empty() && tab.state != RepoState::Clean && !tab.changes.prefilled {
        if let Some(msg) = tab.repo.prepared_message() {
            tab.changes.message = msg;
        }
        tab.changes.prefilled = true;
    }
    if tab.state == RepoState::Clean {
        tab.changes.prefilled = false;
    }

    let resp = ui.add(
        egui::TextEdit::multiline(&mut tab.changes.message)
            .hint_text("Commit summary\n\nDescription (optional)")
            .desired_rows(4)
            .desired_width(f32::INFINITY),
    );
    let ctrl_enter =
        resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));

    let summary_len = tab
        .changes
        .message
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .count();
    ui.horizontal(|ui| {
        let was = tab.changes.amend;
        ui.add_enabled_ui(tab.head_oid.is_some(), |ui| {
            ui.checkbox(&mut tab.changes.amend, "Amend last commit")
                .on_hover_text("Replace the last commit with the staged changes and this message");
        });
        if tab.changes.amend && !was && tab.changes.message.trim().is_empty() {
            tab.changes.message = tab.repo.last_commit_message().unwrap_or_default();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let color = if summary_len > 72 {
                ui.visuals().warn_fg_color
            } else {
                ui.visuals().weak_text_color()
            };
            ui.colored_label(color, summary_len.to_string())
                .on_hover_text("Summary length (72 or fewer is recommended)");
        });
    });

    let staged = tab.status.staged.len();
    let has_message = !tab.changes.message.trim().is_empty();
    let can_commit = has_message
        && (staged > 0 || tab.changes.amend)
        && !tab.busy()
        && tab.status.conflicted.is_empty();
    let label = match (tab.changes.amend, staged) {
        (true, _) => "Amend commit".to_owned(),
        (false, 1) => "Commit 1 file".to_owned(),
        (false, n) => format!("Commit {n} files"),
    };
    ui.horizontal(|ui| {
        let resp = ui.add_enabled(
            can_commit,
            egui::Button::new(label).min_size(egui::vec2(140.0, 26.0)),
        );
        let resp = if !has_message {
            resp.on_disabled_hover_text("Write a commit message first")
        } else if staged == 0 && !tab.changes.amend {
            resp.on_disabled_hover_text("Stage some changes first")
        } else {
            resp
        };
        if resp.clicked() || (ctrl_enter && can_commit) {
            tab.changes.committing = true;
            cmds.run(Op::Commit {
                message: tab.changes.message.clone(),
                amend: tab.changes.amend,
            });
        }
        if staged == 0
            && !tab.status.unstaged.is_empty()
            && has_message
            && !tab.changes.amend
            && ui
                .add_enabled(!tab.busy(), egui::Button::new("Stage all & commit"))
                .clicked()
        {
            tab.changes.committing = true;
            cmds.run(Op::StageAll);
            cmds.run(Op::Commit {
                message: tab.changes.message.clone(),
                amend: false,
            });
        }
    });

    match (&tab.identity.name, &tab.identity.email) {
        (Some(n), Some(e)) => {
            ui.weak(format!("as {n} <{e}>"))
                .on_hover_text("Change in the identity menu at the bottom right");
        }
        _ => {
            if ui
                .link("⚠ No author identity set — click to choose one")
                .clicked()
            {
                cmds.push(Command::Dialog(Dialog::identity(&tab.identity)));
            }
        }
    }
    ui.add_space(4.0);
}
