//! Submodules: the sidebar section with status, and the add / move / change
//! URL / update dialogs.

use eframe::egui::{self, RichText};
use ferrum_core::SubmoduleInfo;
use ferrum_core::submodule::UpdateOptions;

use crate::command::{Command, Commands, Op};
use crate::repo_parts::{AutoUpdate, PartOp};
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;
use crate::ui::sidebar::{children, header, leaf};
use crate::ui::sidebar_model::{NavAction, SidebarState};

fn run(cmds: &mut Commands, op: PartOp) {
    cmds.run(Op::Part(op));
}

fn dialog(cmds: &mut Commands, d: SubmoduleDialog) {
    cmds.push(Command::Dialog(Dialog::Submodule(d)));
}

/// Icons for a submodule's state, and the matching tooltip lines.
fn status(s: &SubmoduleInfo) -> (String, Vec<String>) {
    let mut icons = String::new();
    let mut lines = Vec::new();
    if s.moved {
        icons.push('⟳');
        lines.push(t!("Checked out at another commit than the one recorded"));
    }
    if s.dirty {
        icons.push('📝');
        lines.push(t!("Has uncommitted changes"));
    }
    if s.untracked {
        icons.push('+');
        lines.push(t!("Has untracked files"));
    }
    (icons, lines)
}

/// The sidebar section (always shown, so the "+" can add the first one).
pub fn section(ui: &mut egui::Ui, tab: &RepoTab, st: &mut SidebarState, cmds: &mut Commands) {
    let title = t!("Submodules ({})", tab.submodules.len());
    let h = header(
        ui,
        st,
        "sec:submodules",
        false,
        RichText::new(title),
        None,
        Some(t!("Add submodule…")),
    );
    if h.add {
        dialog(cmds, SubmoduleDialog::add());
    }
    h.resp.context_menu(|ui| {
        ui.add_enabled_ui(!tab.busy(), |ui| {
            if ui.button(t!("Add submodule…")).clicked() {
                dialog(cmds, SubmoduleDialog::add());
                ui.close();
            }
            if !tab.submodules.is_empty() {
                if ui.button(t!("Update submodules…")).clicked() {
                    dialog(cmds, SubmoduleDialog::update(tab, None));
                    ui.close();
                }
                if ui.button(t!("Update all submodules")).clicked() {
                    cmds.run(Op::SubmoduleUpdate);
                    ui.close();
                }
            }
        });
    });
    if !h.open {
        return;
    }
    children(ui, st, "sec:submodules", |ui, st| {
        if tab.submodules.is_empty() {
            ui.weak(t!("No submodules."));
        }
        for s in &tab.submodules {
            row(ui, tab, st, s, cmds);
        }
    });
}

fn row(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    st: &mut SidebarState,
    s: &SubmoduleInfo,
    cmds: &mut Commands,
) {
    let label = if s.initialized {
        format!("▣ {}", s.path)
    } else {
        t!("☐ {} (not initialized)", s.path)
    };
    let (icons, lines) = status(s);
    let badge = RichText::new(icons).color(ui.visuals().warn_fg_color);
    let key = format!("sub:{}", s.path);
    let button = egui::Button::selectable(false, label).right_text(badge);
    let (resp, action) = leaf(ui, st, &key, button);
    let hint = if s.initialized {
        t!("Double-click to open in a new tab")
    } else {
        t!("Double-click to initialize")
    };
    let mut tip = s.url.clone().unwrap_or_default();
    if let Some(b) = &s.branch {
        tip.push_str(&format!("\n{}", t!("Branch: {}", b)));
    }
    for l in lines {
        tip.push_str(&format!("\n{l}"));
    }
    tip.push_str(&format!("\n{hint}"));
    let resp = resp.on_hover_text(tip);
    let full = tab.path().join(&s.path);
    let update_one = || {
        Op::Part(PartOp::SubmoduleUpdate {
            paths: vec![s.path.clone()],
            opts: UpdateOptions::default(),
        })
    };
    if resp.double_clicked() || action == Some(NavAction::Activate) {
        if s.initialized {
            cmds.push(Command::OpenBeside(full.clone()));
        } else if !tab.busy() {
            cmds.run(update_one());
        }
    }
    resp.context_menu(|ui| {
        ui.set_min_width(200.0);
        if s.initialized && ui.button(t!("Open in new tab")).clicked() {
            cmds.push(Command::OpenBeside(full.clone()));
            ui.close();
        }
        ui.add_enabled_ui(!tab.busy(), |ui| {
            let label = if s.initialized {
                t!("Update")
            } else {
                t!("Initialize")
            };
            if ui
                .button(label)
                .on_hover_text(t!("Check out the commit recorded by the repository"))
                .clicked()
            {
                cmds.run(update_one());
                ui.close();
            }
            if ui.button(t!("Update submodules…")).clicked() {
                dialog(cmds, SubmoduleDialog::update(tab, Some(&s.path)));
                ui.close();
            }
            if s.initialized && (s.dirty || s.moved) && ui.button(t!("Discard changes…")).clicked()
            {
                #[rustfmt::skip]
                let text = t!("Reset {} to the commit recorded by the repository? Its uncommitted changes will be lost (untracked files are kept).", s.path);
                cmds.push(Command::Dialog(Dialog::confirm(
                    &t!("Discard submodule changes"),
                    &text,
                    Op::Part(PartOp::SubmoduleDiscard(s.path.clone())),
                    true,
                )));
                ui.close();
            }
            ui.separator();
            if ui.button(t!("Move…")).clicked() {
                dialog(
                    cmds,
                    SubmoduleDialog::Move {
                        from: s.path.clone(),
                        to: s.path.clone(),
                    },
                );
                ui.close();
            }
            if ui.button(t!("Change URL…")).clicked() {
                dialog(
                    cmds,
                    SubmoduleDialog::SetUrl {
                        name: s.name.clone(),
                        path: s.path.clone(),
                        url: s.url.clone().unwrap_or_default(),
                    },
                );
                ui.close();
            }
            if ui.button(t!("Remove…")).clicked() {
                #[rustfmt::skip]
                let text = t!("Remove the submodule {}? Its folder and its clone in .git/modules are deleted; commit the change afterwards.", s.path);
                cmds.push(Command::Dialog(Dialog::confirm(
                    &t!("Remove submodule"),
                    &text,
                    Op::Part(PartOp::SubmoduleRemove {
                        name: s.name.clone(),
                        path: s.path.clone(),
                    }),
                    true,
                )));
                ui.close();
            }
        });
        ui.separator();
        if s.initialized && ui.button(t!("Open in terminal")).clicked() {
            cmds.push(Command::External(crate::external::Launch::Terminal {
                dir: full.clone(),
            }));
            ui.close();
        }
        crate::ui::custom_commands::submodule_menu(ui, tab, &s.path, cmds);
        if ui.button(t!("Copy path")).clicked() {
            cmds.push(Command::Copy(s.path.clone()));
            ui.close();
        }
        if let Some(url) = &s.url
            && ui.button(t!("Copy URL")).clicked()
        {
            cmds.push(Command::Copy(url.clone()));
            ui.close();
        }
    });
}

// ----- Dialogs -----

pub enum SubmoduleDialog {
    Add {
        url: String,
        path: String,
        branch: String,
        /// The path was typed by hand: stop deriving it from the URL.
        path_edited: bool,
    },
    Move {
        from: String,
        to: String,
    },
    SetUrl {
        name: String,
        path: String,
        url: String,
    },
    Update {
        /// Each submodule path and whether to update it.
        list: Vec<(String, bool)>,
        init: bool,
        recursive: bool,
    },
}

impl SubmoduleDialog {
    pub fn add() -> Self {
        SubmoduleDialog::Add {
            url: String::new(),
            path: String::new(),
            branch: String::new(),
            path_edited: false,
        }
    }

    /// Update dialog with every submodule, or only `only`, checked.
    pub fn update(tab: &RepoTab, only: Option<&str>) -> Self {
        SubmoduleDialog::Update {
            list: tab
                .submodules
                .iter()
                .map(|s| (s.path.clone(), only.is_none_or(|o| o == s.path)))
                .collect(),
            init: true,
            recursive: true,
        }
    }
}

/// A path inside the repository: relative, without `..`.
fn valid_path(p: &str) -> bool {
    let p = p.trim();
    !p.is_empty()
        && std::path::Path::new(p)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) -> egui::Response {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    )
}

/// Puts the cursor in the first field when the dialog opens.
fn focus_first(ui: &egui::Ui, resp: &egui::Response) {
    if ui.memory(|m| m.focused().is_none()) {
        resp.request_focus();
    }
}

/// OK / Cancel row. Some(true) for OK, Some(false) for Cancel.
fn buttons(ui: &mut egui::Ui, ok: &str, enabled: bool, enter: bool) -> Option<bool> {
    ui.add_space(8.0);
    let mut result = None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.add_enabled(enabled, egui::Button::new(ok)).clicked() || (enter && enabled) {
            result = Some(true);
        }
        if ui.button(t!("Cancel")).clicked() {
            result = Some(false);
        }
    });
    result
}

/// Draws a submodule dialog. Returns `false` when it should close.
pub fn show_dialog(
    ui: &mut egui::Ui,
    d: &mut SubmoduleDialog,
    tab: Option<&RepoTab>,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    if tab.is_none() {
        return false;
    }
    let (op, answer) = match d {
        SubmoduleDialog::Add {
            url,
            path,
            branch,
            path_edited,
        } => {
            ui.heading(t!("Add submodule"));
            let resp = field(
                ui,
                &t!("Repository URL"),
                url,
                "https://github.com/user/lib.git",
            );
            focus_first(ui, &resp);
            if !*path_edited {
                *path = crate::ui::dialogs::clone_dir_name(url);
            }
            if field(ui, &t!("Path in this repository"), path, "libs/lib").changed() {
                *path_edited = true;
            }
            field(
                ui,
                &t!("Branch (optional)"),
                branch,
                &t!("leave empty for the default branch"),
            );
            let ok = !url.trim().is_empty() && valid_path(path);
            let op = PartOp::SubmoduleAdd {
                url: url.trim().to_owned(),
                path: path.trim().to_owned(),
                branch: Some(branch.trim().to_owned()).filter(|b| !b.is_empty()),
            };
            (op, buttons(ui, &t!("Add submodule"), ok, enter))
        }
        SubmoduleDialog::Move { from, to } => {
            ui.heading(t!("Move submodule"));
            ui.label(t!("Moves {} and updates .gitmodules.", from));
            let resp = field(ui, &t!("New path"), to, "libs/lib");
            focus_first(ui, &resp);
            let ok = valid_path(to) && to.trim() != from;
            let op = PartOp::SubmoduleMove {
                from: from.clone(),
                to: to.trim().to_owned(),
            };
            (op, buttons(ui, &t!("Move"), ok, enter))
        }
        SubmoduleDialog::SetUrl { name, path, url } => {
            ui.heading(t!("Change submodule URL"));
            ui.label(t!(
                "Saved in .gitmodules (staged) and synced into {} (git submodule sync).",
                path
            ));
            let resp = field(
                ui,
                &t!("Repository URL"),
                url,
                "https://github.com/user/lib.git",
            );
            focus_first(ui, &resp);
            let op = PartOp::SubmoduleSetUrl {
                name: name.clone(),
                path: path.clone(),
                url: url.trim().to_owned(),
            };
            (
                op,
                buttons(ui, &t!("Change URL"), !url.trim().is_empty(), enter),
            )
        }
        SubmoduleDialog::Update {
            list,
            init,
            recursive,
        } => {
            ui.heading(t!("Update submodules"));
            ui.weak(t!("Checks out the commit recorded by the repository."));
            ui.horizontal(|ui| {
                if ui.small_button(t!("Select all")).clicked() {
                    list.iter_mut().for_each(|(_, on)| *on = true);
                }
                if ui.small_button(t!("Select none")).clicked() {
                    list.iter_mut().for_each(|(_, on)| *on = false);
                }
            });
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for (path, on) in list.iter_mut() {
                        ui.checkbox(on, path.as_str());
                    }
                });
            ui.separator();
            ui.checkbox(init, t!("Initialize submodules that are not cloned yet"));
            ui.checkbox(recursive, t!("Also update nested submodules (recursive)"));
            let paths: Vec<String> = list
                .iter()
                .filter(|(_, on)| *on)
                .map(|(p, _)| p.clone())
                .collect();
            let ok = !paths.is_empty();
            let op = PartOp::SubmoduleUpdate {
                paths,
                opts: UpdateOptions {
                    init: *init,
                    recursive: *recursive,
                },
            };
            (op, buttons(ui, &t!("Update"), ok, enter))
        }
    };
    match answer {
        Some(true) => {
            run(cmds, op);
            false
        }
        Some(false) => false,
        None => true,
    }
}

/// The Settings > Git block.
pub fn settings_block(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(10.0);
    ui.label(RichText::new(t!("Submodules")).strong());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(t!(
            "Update submodules after checkout, cherry-pick and revert"
        ));
        let current = &mut settings.submodules.auto_update;
        egui::ComboBox::from_id_salt("submodule-auto-update")
            .selected_text(current.label())
            .show_ui(ui, |ui| {
                for a in AutoUpdate::ALL {
                    ui.selectable_value(current, a, a.label());
                }
            });
    });
}
