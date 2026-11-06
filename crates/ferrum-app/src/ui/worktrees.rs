//! Worktrees: the sidebar section (linked worktrees nested under the main
//! one), "check out as worktree" for branches, and branches that are checked
//! out in another worktree.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use ferrum_core::WorktreeInfo;

use crate::command::{Command, Commands, Op};
use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;
use crate::ui::sidebar::{children, header, leaf};
use crate::ui::sidebar_model::{NavAction, SidebarState};
use crate::util::valid_ref_name;

/// Marks branches checked out in another worktree.
pub const ICON: &str = "📂";

fn is_current(tab: &RepoTab, w: &WorktreeInfo) -> bool {
    same_path(Path::new(&w.path), tab.path())
}

fn same_path(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    a == b || canon(a) == canon(b)
}

/// The other worktree where `branch` is checked out, if any.
pub fn other_worktree<'a>(tab: &'a RepoTab, branch: &str) -> Option<&'a WorktreeInfo> {
    // A branch can only be checked out in one worktree.
    if tab.head.as_deref() == Some(branch) || tab.worktrees.len() < 2 {
        return None;
    }
    tab.worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some(branch) && !is_current(tab, w))
}

/// A local branch's label, with the worktree icon when it is checked out elsewhere.
pub fn branch_label(tab: &RepoTab, branch: &str, label: &str) -> String {
    if other_worktree(tab, branch).is_some() {
        format!("{label} {ICON}")
    } else {
        label.to_owned()
    }
}

/// Double-click on a branch: opens its worktree when it is checked out in
/// another one. Returns `false` when the branch should be checked out here.
pub fn activate_branch(tab: &RepoTab, branch: &str, cmds: &mut Commands) -> bool {
    match other_worktree(tab, branch) {
        Some(w) => {
            cmds.push(Command::OpenBeside(PathBuf::from(&w.path)));
            true
        }
        None => false,
    }
}

/// Worktree entries for a branch's context menu. `remote` for remote branches.
pub fn branch_menu(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    branch: &str,
    remote: bool,
    cmds: &mut Commands,
) {
    if !remote && let Some(w) = other_worktree(tab, branch) {
        if ui
            .button(t!("Open worktree of {}", branch))
            .on_hover_text(&w.path)
            .clicked()
        {
            cmds.push(Command::OpenBeside(PathBuf::from(&w.path)));
            ui.close();
        }
        return;
    }
    let checked_out_here = !remote && tab.head.as_deref() == Some(branch);
    if !checked_out_here
        && ui
            .add_enabled(!tab.busy(), egui::Button::new(t!("Check out as worktree…")))
            .clicked()
    {
        let d = WorktreeDialog::checkout(tab, branch, remote);
        cmds.push(Command::Dialog(Dialog::Worktree(d)));
        ui.close();
    }
}

// ----- Sidebar -----

fn label(w: &WorktreeInfo, current: bool) -> String {
    let name = Path::new(&w.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(w.path.clone());
    let branch = w.branch.clone().unwrap_or_else(|| t!("detached"));
    let mark = if current { "✔ " } else { "" };
    let lock = if w.locked { " 🔒" } else { "" };
    format!("{mark}{ICON} {name} ({branch}){lock}")
}

pub fn section(ui: &mut egui::Ui, tab: &RepoTab, st: &mut SidebarState, cmds: &mut Commands) {
    let title = t!("Worktrees ({})", tab.worktrees.len());
    let h = header(
        ui,
        st,
        "sec:worktrees",
        false,
        RichText::new(title),
        None,
        Some(t!("Add worktree…")),
    );
    if h.add {
        cmds.push(Command::Dialog(Dialog::worktree_add(tab)));
    }
    h.resp
        .on_hover_text(t!("Extra working folders that share this repository"));
    if !h.open {
        return;
    }
    children(ui, st, "sec:worktrees", |ui, st| {
        let main = tab.worktrees.iter().find(|w| w.is_main);
        let linked: Vec<&WorktreeInfo> = tab.worktrees.iter().filter(|w| !w.is_main).collect();
        match main {
            Some(m) => {
                row(ui, tab, st, m, cmds);
                if !linked.is_empty() {
                    // Linked worktrees sit under the main one.
                    children(ui, st, "wt-main", |ui, st| {
                        for w in &linked {
                            row(ui, tab, st, w, cmds);
                        }
                    });
                }
            }
            None => {
                for w in &linked {
                    row(ui, tab, st, w, cmds);
                }
            }
        }
    });
}

fn row(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    st: &mut SidebarState,
    w: &WorktreeInfo,
    cmds: &mut Commands,
) {
    let current = is_current(tab, w);
    let key = format!("wt:{}", w.path);
    let button = egui::Button::selectable(false, label(w, current));
    let (resp, action) = leaf(ui, st, &key, button);
    let mut tip = w.path.clone();
    if w.is_main {
        tip.push_str(&format!("\n{}", t!("Main worktree")));
    }
    if !current {
        tip.push_str(&format!("\n{}", t!("Double-click to open in a new tab")));
    }
    let resp = resp.on_hover_text(tip);
    let path = PathBuf::from(&w.path);
    if (resp.double_clicked() || action == Some(NavAction::Activate)) && !current {
        cmds.push(Command::OpenBeside(path.clone()));
    }
    resp.context_menu(|ui| {
        if !current && ui.button(t!("Open in new tab")).clicked() {
            cmds.push(Command::OpenBeside(path.clone()));
            ui.close();
        }
        if ui.button(t!("Open in terminal")).clicked() {
            cmds.push(Command::External(crate::external::Launch::Terminal {
                dir: path.clone(),
            }));
            ui.close();
        }
        if !w.is_main
            && !current
            && ui
                .add_enabled(!tab.busy(), egui::Button::new(t!("Remove…")))
                .clicked()
        {
            cmds.push(Command::Dialog(Dialog::confirm(
                &t!("Remove worktree"),
                &t!(
                    "Remove the worktree at {}? Uncommitted changes in it will be lost.",
                    w.path
                ),
                Op::WorktreeRemove {
                    path: w.path.clone(),
                    force: true,
                },
                true,
            )));
            ui.close();
        }
        if ui.button(t!("Copy path")).clicked() {
            cmds.push(Command::Copy(w.path.clone()));
            ui.close();
        }
    });
}

// ----- Dialog -----

pub struct WorktreeDialog {
    /// Branch to check out (`origin/x` for a remote one).
    branch: String,
    remote: bool,
    /// Local branch created for a remote branch.
    local: String,
    path: String,
}

impl WorktreeDialog {
    pub fn checkout(tab: &RepoTab, branch: &str, remote: bool) -> Self {
        let local = if remote {
            branch.split_once('/').map_or(branch, |(_, n)| n)
        } else {
            branch
        };
        WorktreeDialog {
            branch: branch.to_owned(),
            remote,
            local: local.to_owned(),
            path: suggested_path(tab, local),
        }
    }
}

#[cfg(test)]
impl WorktreeDialog {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn with_path(mut self, path: String) -> Self {
        self.path = path;
        self
    }
}

/// `<folder of the main worktree>/<repo>-<branch>`, next to the main worktree.
pub fn suggested_path(tab: &RepoTab, branch: &str) -> String {
    let main = tab
        .worktrees
        .iter()
        .find(|w| w.is_main)
        .map(|w| PathBuf::from(&w.path))
        .unwrap_or_else(|| tab.path().to_path_buf());
    let repo = main
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| tab.name.clone());
    let leaf: String = branch
        .chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect();
    main.parent()
        .unwrap_or(&main)
        .join(format!("{repo}-{leaf}"))
        .to_string_lossy()
        .into_owned()
}

/// Draws the "check out as worktree" dialog. Returns `false` when it should close.
pub fn show_dialog(
    ui: &mut egui::Ui,
    d: &mut WorktreeDialog,
    tab: Option<&RepoTab>,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let Some(tab) = tab else { return false };
    ui.heading(t!("Check out as worktree"));
    ui.label(t!(
        "Checks out {} in a new folder, next to this one.",
        d.branch
    ));
    ui.add_space(4.0);
    ui.label(t!("Folder"));
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut d.path).desired_width(340.0));
        if ui.button(t!("Browse…")).clicked()
            && let Some(p) = rfd::FileDialog::new().pick_folder()
        {
            d.path = p.to_string_lossy().into_owned();
        }
    });
    if d.remote {
        ui.label(t!("Local branch"));
        let before = d.local.clone();
        ui.add(egui::TextEdit::singleline(&mut d.local).desired_width(f32::INFINITY));
        if d.local != before && d.path == suggested_path(tab, &before) {
            d.path = suggested_path(tab, &d.local);
        }
        if !d.local.is_empty() && !valid_ref_name(&d.local) {
            ui.colored_label(ui.visuals().warn_fg_color, t!("Invalid branch name"));
        }
    }
    let exists = Path::new(d.path.trim())
        .read_dir()
        .is_ok_and(|mut r| r.next().is_some());
    if exists {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("The folder exists and is not empty."),
        );
    }
    let ok = !d.path.trim().is_empty() && !exists && (!d.remote || valid_ref_name(&d.local));
    ui.add_space(8.0);
    let mut result = None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let add = ui.add_enabled(ok, egui::Button::new(t!("Add worktree")));
        if add.clicked() || (enter && ok) {
            result = Some(true);
        }
        if ui.button(t!("Cancel")).clicked() {
            result = Some(false);
        }
    });
    match result {
        Some(true) => {
            cmds.run(Op::WorktreeAdd {
                path: d.path.trim().to_owned(),
                rev: d.branch.clone(),
                new_branch: d.remote.then(|| d.local.trim().to_owned()),
            });
            false
        }
        Some(false) => false,
        None => true,
    }
}
