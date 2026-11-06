//! Comparing two revisions in the details pane: two commits picked with
//! Ctrl+click, two branches from the sidebar, or a commit or stash and the
//! working copy. Shows the changed files and the diff of the selected one,
//! with a button to swap the sides.

use std::hash::{Hash, Hasher};

use eframe::egui;
use ferrum_core::browse::Rev;
use ferrum_core::{DiffSource, FileDiff, Oid, short};

use crate::command::{Command, Commands};
use crate::tab::{RepoTab, View};
use crate::ui::details_model::DetailsSettings;
use crate::ui::file_actions::{self, DiffSide};
use crate::ui::{diff_extra, diff_view, file_tree};

/// One side of a comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    pub rev: Rev,
    pub label: String,
}

pub struct CompareView {
    pub from: Side,
    pub to: Side,
    pub files: Vec<FileDiff>,
    pub selected: Option<usize>,
    pub diff: Option<FileDiff>,
    pub error: Option<String>,
    pub list: file_tree::ListState,
    /// Local changes when the files were listed (to reload them).
    status_key: u64,
}

pub fn commit_side(tab: &RepoTab, id: Oid) -> Side {
    let summary = tab
        .index
        .get(&id)
        .map(|&i| tab.commits[i].summary.clone())
        .or_else(|| tab.repo.find_commit_info(id).ok().map(|c| c.summary))
        .unwrap_or_default();
    let mut label = format!("{} {summary}", short(id));
    if label.chars().count() > 48 {
        label = label.chars().take(47).collect::<String>() + "…";
    }
    Side {
        rev: Rev::Commit(id),
        label,
    }
}

pub fn workdir_side() -> Side {
    Side {
        rev: Rev::WorkDir,
        label: t!("Working copy"),
    }
}

fn status_key(tab: &RepoTab) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    for f in tab.status.unstaged.iter().chain(&tab.status.staged) {
        f.path.hash(&mut h);
        (f.kind as u8).hash(&mut h);
    }
    h.finish()
}

/// Shows the comparison of two revisions in the details pane.
pub fn open(tab: &mut RepoTab, from: Side, to: Side) {
    tab.view = View::History;
    tab.details.reveal = true;
    let mut view = CompareView {
        from,
        to,
        files: Vec::new(),
        selected: None,
        diff: None,
        error: None,
        list: Default::default(),
        status_key: 0,
    };
    reload(tab, &mut view);
    tab.details.compare = Some(view);
}

/// Compares two branches (or other revisions) by name, oldest first.
pub fn open_refs(tab: &mut RepoTab, a: &str, b: &str) {
    let resolve = |name: &str| tab.repo.resolve(name).map_err(|e| e.to_string());
    match (resolve(a), resolve(b)) {
        (Ok(ia), Ok(ib)) => {
            let side = |rev, name: &str| Side {
                rev: Rev::Commit(rev),
                label: name.to_owned(),
            };
            let (from, to) = if tab.repo.is_ancestor(ib, ia) {
                (side(ib, b), side(ia, a))
            } else {
                (side(ia, a), side(ib, b))
            };
            open(tab, from, to);
        }
        (Err(e), _) | (_, Err(e)) => tab.error(e),
    }
}

/// Compares two commits, the older one first (an ancestor, or the one lower
/// in the list).
fn compare_commits(tab: &mut RepoTab, a: Oid, b: Oid) {
    let older_first = if tab.repo.is_ancestor(a, b) {
        true
    } else if tab.repo.is_ancestor(b, a) {
        false
    } else {
        tab.index.get(&a) > tab.index.get(&b)
    };
    let (from, to) = if older_first { (a, b) } else { (b, a) };
    let (from, to) = (commit_side(tab, from), commit_side(tab, to));
    open(tab, from, to);
}

/// After a Ctrl/Shift+click in the history: exactly two selected commits are
/// compared; with more, the selection is for the multi-commit actions.
pub fn show_selection(tab: &mut RepoTab) {
    let many = tab.history.ext.selected_many.clone();
    match many.as_slice() {
        [a, b] => {
            tab.details.picked = many.clone();
            compare_commits(tab, *a, *b);
        }
        _ => {
            tab.details.picked.clear();
            tab.details.compare = None;
        }
    }
}

/// What a Ctrl+click on a commit row does (for tests).
#[cfg(test)]
pub fn pick_commit(tab: &mut RepoTab, id: Oid) {
    crate::ui::history_flow::click(tab, id, eframe::egui::Modifiers::COMMAND);
    show_selection(tab);
}

fn reload(tab: &mut RepoTab, view: &mut CompareView) {
    view.status_key = status_key(tab);
    let keep = view
        .selected
        .and_then(|i| view.files.get(i))
        .map(|f| f.path.clone());
    match tab.repo.compare_files(view.from.rev, view.to.rev) {
        Ok(files) => {
            view.files = files;
            view.error = None;
        }
        Err(e) => {
            view.files.clear();
            view.error = Some(e.to_string());
        }
    }
    let i = keep
        .and_then(|p| view.files.iter().position(|f| f.path == p))
        .unwrap_or(0);
    view.selected = None;
    view.diff = None;
    select(tab, view, i);
}

fn select(tab: &RepoTab, view: &mut CompareView, i: usize) {
    let Some(f) = view.files.get(i) else { return };
    view.selected = Some(i);
    let cfg = tab.diff_options.for_path(&f.path);
    match tab.repo.compare_file_diff(
        view.from.rev,
        view.to.rev,
        &f.path,
        f.old_path.as_deref(),
        &cfg,
    ) {
        Ok(d) => view.diff = d,
        Err(e) => {
            view.diff = None;
            view.error = Some(e.to_string());
        }
    }
}

/// Swaps the two sides.
pub fn swap(tab: &mut RepoTab) {
    let Some(mut view) = tab.details.compare.take() else {
        return;
    };
    std::mem::swap(&mut view.from, &mut view.to);
    reload(tab, &mut view);
    tab.details.compare = Some(view);
}

fn source(view: &CompareView) -> DiffSource {
    match view.to.rev {
        Rev::WorkDir => DiffSource::Unstaged,
        Rev::Commit(id) => DiffSource::Commit(id),
    }
}

fn side_kind(rev: Rev) -> DiffSide {
    match rev {
        Rev::WorkDir => DiffSide::WorkDir,
        Rev::Commit(_) => DiffSide::Blob,
    }
}

/// External diff of the selected file (Ctrl+D).
pub fn external_diff(tab: &RepoTab) -> Option<Command> {
    let view = tab.details.compare.as_ref()?;
    let file = view.diff.as_ref()?;
    Some(file_actions::external_diff(
        &tab.repo,
        file,
        side_kind(view.from.rev),
        side_kind(view.to.rev),
    ))
}

pub fn show(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    prefs: &mut DetailsSettings,
    cmds: &mut Commands,
) {
    let Some(mut view) = tab.details.compare.take() else {
        return;
    };
    let has_workdir = view.from.rev == Rev::WorkDir || view.to.rev == Rev::WorkDir;
    if has_workdir && view.status_key != status_key(tab) {
        reload(tab, &mut view);
    }
    let mut close = false;
    let mut swap_sides = false;
    let mut refresh = false;
    ui.horizontal(|ui| {
        ui.strong(t!("Compare"));
        ui.label(egui::RichText::new(&view.from.label).monospace());
        ui.weak("➡");
        ui.label(egui::RichText::new(&view.to.label).monospace());
        swap_sides = ui
            .button(t!("🔃 Swap"))
            .on_hover_text(t!("Swap the order of the two sides"))
            .clicked();
        refresh = ui.button("↻").on_hover_text(t!("Reload")).clicked();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui.button(t!("Close")).clicked();
        });
    });
    ui.separator();
    if let Some(e) = &view.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }

    let mut clicked = None;
    let root = tab.path().to_path_buf();
    egui::Panel::left("compare-files")
        .resizable(true)
        .default_size(320.0)
        .show(ui, |ui| {
            file_tree::list_toolbar(ui, &view.files, &mut prefs.files_as_tree, &mut view.list);
            if view.files.is_empty() && view.error.is_none() {
                ui.weak(t!("No differences."));
            }
            egui::ScrollArea::vertical()
                .id_salt("compare-files-scroll")
                .auto_shrink(false)
                .show(ui, |ui| {
                    let mut menu = |ui: &mut egui::Ui, f: &FileDiff| {
                        if ui.button(t!("Copy path")).clicked() {
                            cmds.push(Command::Copy(f.path.clone()));
                            ui.close();
                        }
                        if ui.button(t!("File history")).clicked() {
                            cmds.push(Command::FileHistory(f.path.clone()));
                            ui.close();
                        }
                        ui.separator();
                        file_actions::workdir_menu(ui, &root, &f.path, cmds);
                    };
                    clicked = file_tree::changed_files(
                        ui,
                        &view.files,
                        view.selected,
                        prefs.files_as_tree,
                        &mut view.list,
                        &mut menu,
                    );
                });
        });
    if let Some(i) = clicked {
        select(tab, &mut view, i);
    }
    egui::CentralPanel::default_margins().show(ui, |ui| {
        let Some(file) = &view.diff else {
            ui.centered_and_justified(|ui| ui.weak(t!("No file selected.")));
            return;
        };
        ui.horizontal(|ui| {
            ui.strong(&file.path);
            if ui
                .small_button(t!("External diff"))
                .on_hover_text("Ctrl+D")
                .clicked()
            {
                cmds.push(file_actions::external_diff(
                    &tab.repo,
                    file,
                    side_kind(view.from.rev),
                    side_kind(view.to.rev),
                ));
            }
        });
        diff_view::offer_reveal(ui.ctx(), "compare-diff");
        let action = diff_extra::show_file(
            ui,
            "compare-diff",
            &tab.repo,
            source(&view),
            file,
            diff_view::DiffMode::ReadOnly,
            false,
            None,
        );
        if let Some(diff_view::DiffAction::Reveal(line)) = action {
            cmds.push(file_actions::reveal_line(&root, file, line));
        }
    });

    if close {
        tab.details.picked.clear();
        return;
    }
    tab.details.compare = Some(view);
    if swap_sides {
        swap(tab);
    } else if refresh && let Some(mut v) = tab.details.compare.take() {
        reload(tab, &mut v);
        tab.details.compare = Some(v);
    }
}
