//! Details of the selected commit: message, files and diff.

use eframe::egui;
use ferrum_core::{DiffSource, short};

use crate::command::{Command, Commands, Op};
use crate::tab::RepoTab;
use crate::ui::{diff_extra, diff_view};
use crate::util::{change_label, format_time};

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    let Some(view) = &tab.commit_view else {
        ui.centered_and_justified(|ui| ui.weak(t!("Select a commit to see its details.")));
        return;
    };
    let is_stash = view.is_stash;

    egui::Panel::left("details-info")
        .resizable(true)
        .default_size(340.0)
        .show(ui, |ui| {
            info(ui, tab, cmds);
            if is_stash {
                stash_actions(ui, tab, cmds);
            }
            ui.separator();
            file_list(ui, tab, cmds);
        });
    egui::CentralPanel::default_margins().show(ui, |ui| {
        let Some(view) = &tab.commit_view else { return };
        let Some(file) = view.selected_file.and(view.diff.as_ref()) else {
            ui.centered_and_justified(|ui| ui.weak(t!("No file selected.")));
            return;
        };
        ui.horizontal(|ui| {
            ui.strong(&file.path);
        });
        let id = view.details.info.id;
        diff_extra::show_file(
            ui,
            "commit-diff",
            &tab.repo,
            DiffSource::Commit(id),
            file,
            diff_view::DiffMode::ReadOnly,
            false,
            None,
        );
    });
}

fn info(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let Some(view) = &tab.commit_view else { return };
    let d = &view.details;
    let c = &d.info;
    egui::Grid::new("commit-info")
        .num_columns(2)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            // Same word in every language; "Commit" alone is the button's key.
            ui.weak("Commit");
            ui.horizontal(|ui| {
                ui.monospace(c.short_id()).on_hover_text(c.id.to_string());
                if ui.small_button(t!("Copy")).clicked() {
                    cmds.push(Command::Copy(c.id.to_string()));
                }
            });
            ui.end_row();
            ui.weak(t!("Author"));
            ui.label(format!("{} <{}>", c.author, c.email));
            ui.end_row();
            ui.weak(t!("Date"));
            ui.label(format_time(c.time, c.offset_minutes));
            ui.end_row();
            if d.committer != c.author || d.commit_time != c.time {
                ui.weak(t!("Committer"));
                ui.label(format!(
                    "{} — {}",
                    d.committer,
                    format_time(d.commit_time, d.commit_offset_minutes)
                ));
                ui.end_row();
            }
            if !c.parents.is_empty() {
                ui.weak(if c.parents.len() > 1 {
                    t!("Parents")
                } else {
                    t!("Parent")
                });
                ui.horizontal(|ui| {
                    for p in &c.parents {
                        if ui
                            .link(egui::RichText::new(short(*p)).monospace())
                            .clicked()
                        {
                            cmds.push(Command::SelectCommit(*p));
                        }
                    }
                });
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .id_salt("commit-message")
        .max_height(120.0)
        .show(ui, |ui| {
            let mut lines = d.message.trim_end().splitn(2, '\n');
            ui.strong(lines.next().unwrap_or_default());
            if let Some(body) = lines.next().map(str::trim).filter(|b| !b.is_empty()) {
                ui.label(body);
            }
        });
}

fn stash_actions(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let Some(view) = &tab.commit_view else { return };
    let Some(stash) = tab.stashes.iter().find(|s| s.id == view.details.info.id) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!tab.busy(), |ui| {
            if ui.button(t!("Apply")).clicked() {
                cmds.run(Op::StashApply {
                    stash: stash.clone(),
                    pop: false,
                });
            }
            if ui.button(t!("Pop")).clicked() {
                cmds.run(Op::StashApply {
                    stash: stash.clone(),
                    pop: true,
                });
            }
            if ui.button(t!("Drop…")).clicked() {
                cmds.push(Command::Dialog(crate::ui::dialogs::Dialog::confirm(
                    &t!("Drop stash"),
                    &t!("Delete {}? This cannot be undone.", stash.refname()),
                    Op::StashDrop(stash.clone()),
                    true,
                )));
            }
        });
    });
}

fn file_list(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    let Some(view) = &mut tab.commit_view else {
        return;
    };
    ui.weak(if view.files.len() == 1 {
        t!("1 file changed")
    } else {
        t!("{} files changed", view.files.len())
    });
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt("commit-files")
        .auto_shrink(false)
        .show(ui, |ui| {
            for (i, f) in view.files.iter().enumerate() {
                let job = change_label(ui, f.kind, &f.path, f.old_path.as_deref());
                let resp =
                    ui.add(egui::Button::selectable(view.selected_file == Some(i), job).truncate());
                if resp.clicked() {
                    clicked = Some(i);
                }
                resp.on_hover_text(&f.path).context_menu(|ui| {
                    if ui.button(t!("Copy path")).clicked() {
                        cmds.push(Command::Copy(f.path.clone()));
                        ui.close();
                    }
                    if ui.button(t!("File history")).clicked() {
                        cmds.push(Command::FileHistory(f.path.clone()));
                        ui.close();
                    }
                    if ui.button(t!("Blame")).clicked() {
                        cmds.push(Command::Blame(f.path.clone()));
                        ui.close();
                    }
                });
            }
        });
    if let Some(i) = clicked {
        tab.select_commit_file(i);
    }
}
