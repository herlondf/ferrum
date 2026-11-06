//! Local changes: file lists (flat, tree or one combined list), the diff of
//! the selected file and the commit box.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use eframe::egui;
use ferrum_core::diff::{partial_patch, supports_line_staging};
use ferrum_core::ops::PatchTarget;
use ferrum_core::{ChangeKind, DiffSource, FileChange};

use crate::command::{Command, Commands, Op};
use crate::commit_prefs::ListMode;
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::changes_tools::{self, Tool};
use crate::ui::commit_box;
use crate::ui::dialogs::Dialog;
use crate::ui::diff_view::{Action, DiffAction, DiffMode};
use crate::ui::file_actions::{self, DiffSide};
use crate::util::change_label;

/// What the lists remember between frames.
#[derive(Default)]
pub struct ChangesUi {
    pub filter: String,
    /// Files picked with Ctrl/Shift-click, all from one list.
    pub picked: BTreeSet<String>,
    pub picked_source: Option<DiffSource>,
    anchor: Option<String>,
    /// Row order of each list in the last frame, for Shift-click ranges.
    order_unstaged: Vec<String>,
    order_staged: Vec<String>,
    ignored: Vec<String>,
    ignored_at: Option<Instant>,
    /// An open window of the changes view (ignore pattern, templates...).
    pub tool: Option<Tool>,
    pub commit: commit_box::CommitBox,
    /// A text field had focus in the last frame (Enter there is not a toggle).
    typing: bool,
}

/// Files dragged from one list to the other.
struct DragFiles {
    source: DiffSource,
    paths: Vec<String>,
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &mut Settings, cmds: &mut Commands) {
    egui::Panel::left("changes-lists")
        .resizable(true)
        .default_size(380.0)
        .show(ui, |ui| {
            egui::Panel::bottom("commit-box")
                .show(ui, |ui| commit_box::show(ui, tab, settings, cmds));
            egui::CentralPanel::default_margins().show(ui, |ui| lists(ui, tab, settings, cmds));
        });
    egui::CentralPanel::default_margins().show(ui, |ui| diff(ui, tab, cmds));
    changes_tools::show(ui.ctx(), tab, cmds);
    keys(ui, tab, settings, cmds);
    tab.changes.ui.typing = ui.ctx().memory(|m| m.focused().is_some());
    drag_preview(ui.ctx());
    if file_actions::external_diff_pressed(ui.ctx())
        && let Some(cmd) = external_diff(tab)
    {
        cmds.push(cmd);
    }
}

/// External diff of the selected change.
fn external_diff(tab: &RepoTab) -> Option<Command> {
    let (source, path) = tab.changes.selected.as_ref()?;
    Some(external_diff_of(tab, *source, path))
}

/// External diff of a change: the index (or HEAD, when staged) against the file.
fn external_diff_of(tab: &RepoTab, source: DiffSource, path: &str) -> Command {
    let cfg = ferrum_core::diff::DiffConfig {
        max_untracked_bytes: None,
        ..Default::default()
    };
    let file = match tab.repo.file_diff_with(source, path, &cfg) {
        Ok(Some(f)) => f,
        Ok(None) => return Command::Error(t!("No changes to show.")),
        Err(e) => return Command::Error(e.to_string()),
    };
    let right = if source == DiffSource::Staged {
        DiffSide::Blob
    } else {
        DiffSide::WorkDir
    };
    file_actions::external_diff(&tab.repo, &file, DiffSide::Blob, right)
}

/// Enter toggles staging of the selected files.
fn keys(ui: &egui::Ui, tab: &mut RepoTab, settings: &Settings, cmds: &mut Commands) {
    let blocked = ui
        .ctx()
        .memory(|m| m.focused().is_some() || m.top_modal_layer().is_some())
        || tab.changes.ui.typing
        || tab.changes.ui.tool.is_some()
        || tab.busy();
    if blocked || !ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.is_none()) {
        return;
    }
    let Some((source, path)) = tab.changes.selected.clone() else {
        return;
    };
    let paths = targets(tab, source, &path);
    toggle(tab, settings, cmds, source, paths);
}

fn drag_preview(ctx: &egui::Context) {
    let Some(drag) = egui::DragAndDrop::payload::<DragFiles>(ctx) else {
        return;
    };
    let Some(pos) = ctx.pointer_interact_pos() else {
        return;
    };
    let text = match drag.paths.as_slice() {
        [one] => one.rsplit('/').next().unwrap_or(one).to_owned(),
        many => t!("{} files", many.len()),
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("changes-drag"),
    ));
    let visuals = ctx.global_style().visuals.clone();
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(13.0),
        visuals.strong_text_color(),
    );
    let rect = egui::Rect::from_min_size(pos + egui::vec2(14.0, 8.0), galley.size())
        .expand2(egui::vec2(6.0, 3.0));
    painter.rect(
        rect,
        4.0,
        visuals.window_fill,
        visuals.window_stroke,
        egui::StrokeKind::Outside,
    );
    painter.galley(
        rect.min + egui::vec2(6.0, 3.0),
        galley,
        visuals.text_color(),
    );
}

// ----- Acting on files -----

/// The files a row action applies to: all picked files when the row is one
/// of them, otherwise just the row.
fn targets(tab: &RepoTab, source: DiffSource, path: &str) -> Vec<String> {
    let st = &tab.changes.ui;
    if st.picked_source == Some(source) && st.picked.len() > 1 && st.picked.contains(path) {
        st.picked.iter().cloned().collect()
    } else {
        vec![path.to_owned()]
    }
}

/// Stages files, asking first when some are big and not in Git LFS.
pub(crate) fn stage(tab: &RepoTab, settings: &Settings, cmds: &mut Commands, paths: Vec<String>) {
    let all = paths.is_empty();
    let check: Vec<String> = if all {
        tab.status.unstaged.iter().map(|f| f.path.clone()).collect()
    } else {
        paths.clone()
    };
    let op = if all { Op::StageAll } else { Op::Stage(paths) };
    let big = settings
        .commit
        .large_file_threshold()
        .map(|limit| tab.repo.large_files(&check, limit))
        .unwrap_or_default();
    if big.is_empty() {
        cmds.run(op);
        return;
    }
    let list: Vec<String> = big
        .iter()
        .map(|(p, size)| format!("{p} ({})", commit_box::format_size(*size)))
        .collect();
    cmds.push(Command::Dialog(Dialog::confirm(
        &t!("Large files"),
        &t!(
            "These files are bigger than {} MB and are not tracked by Git LFS:\n\n{}\n\nStage them anyway?",
            settings.commit.large_file_mb,
            list.join("\n")
        ),
        op,
        false,
    )));
}

fn unstage_op(tab: &RepoTab, paths: Vec<String>) -> Op {
    if tab.changes.amend {
        Op::UnstageAmend(paths)
    } else {
        Op::Unstage(paths)
    }
}

fn toggle(
    tab: &RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
    source: DiffSource,
    paths: Vec<String>,
) {
    if source == DiffSource::Staged {
        cmds.run(unstage_op(tab, paths));
    } else {
        stage(tab, settings, cmds, paths);
    }
}

/// Moves files dropped on a list into it.
fn drop_files(
    tab: &RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
    drag: &DragFiles,
    onto: DiffSource,
) {
    if drag.source == onto || tab.busy() {
        return;
    }
    toggle(tab, settings, cmds, drag.source, drag.paths.clone());
}

fn click(tab: &mut RepoTab, source: DiffSource, path: &str, mods: egui::Modifiers) {
    let order = if source == DiffSource::Staged {
        tab.changes.ui.order_staged.clone()
    } else {
        tab.changes.ui.order_unstaged.clone()
    };
    let st = &mut tab.changes.ui;
    if st.picked_source != Some(source) {
        st.picked.clear();
        st.picked_source = Some(source);
    }
    if mods.command {
        if !st.picked.remove(path) {
            st.picked.insert(path.to_owned());
        }
        st.anchor = Some(path.to_owned());
    } else if mods.shift
        && let Some(a) = st
            .anchor
            .as_ref()
            .and_then(|a| order.iter().position(|p| p == a))
        && let Some(b) = order.iter().position(|p| p == path)
    {
        let (from, to) = (a.min(b), a.max(b));
        st.picked = order[from..=to].iter().cloned().collect();
    } else {
        st.picked = BTreeSet::from([path.to_owned()]);
        st.anchor = Some(path.to_owned());
    }
    tab.select_change(source, path.to_owned());
}

// ----- Lists -----

fn visible<'a>(files: &'a [FileChange], filter: &str, hide_untracked: bool) -> Vec<&'a FileChange> {
    let filter = filter.trim().to_lowercase();
    files
        .iter()
        .filter(|f| !(hide_untracked && f.kind == ChangeKind::Untracked))
        .filter(|f| filter.is_empty() || f.path.to_lowercase().contains(&filter))
        .collect()
}

fn lists(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &mut Settings, cmds: &mut Commands) {
    let busy = tab.busy();
    if !tab.status.conflicted.is_empty() {
        ui.horizontal(|ui| {
            ui.colored_label(
                ui.visuals().error_fg_color,
                t!("⚠ Conflicts ({})", tab.status.conflicted.len()),
            );
        });
        for f in &tab.status.conflicted {
            let job = change_label(ui, ChangeKind::Conflicted, &f.path, None);
            if ui
                .add(egui::Button::selectable(false, job).truncate())
                .on_hover_text(t!("Open the conflict resolver"))
                .clicked()
            {
                cmds.push(Command::ResolveConflict(f.path.clone()));
            }
        }
        ui.separator();
    }

    toolbar(ui, tab, settings);
    let prefs = &settings.commit;
    let unstaged_all = tab.status.unstaged.clone();
    let staged_all = tab.status.staged.clone();
    let unstaged = visible(&unstaged_all, &tab.changes.ui.filter, prefs.hide_untracked);
    let staged = visible(&staged_all, &tab.changes.ui.filter, false);
    let mode = prefs.list_mode;
    refresh_ignored(tab, settings);

    if mode == ListMode::Combined {
        combined(ui, tab, settings, cmds, &unstaged, &staged);
        return;
    }

    let half = (ui.available_height() / 2.0 - 30.0).max(80.0);
    ui.horizontal(|ui| {
        ui.strong(t!("Unstaged ({})", tab.status.unstaged.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(!busy && !tab.status.unstaged.is_empty(), |ui| {
                if ui.small_button(t!("Discard all…")).clicked() {
                    cmds.push(Command::Dialog(Dialog::confirm(
                        &t!("Discard all changes"),
                        &t!("Throw away all unstaged changes, including new files? This cannot be undone."),
                        Op::Discard(tab.status.unstaged.clone()),
                        true,
                    )));
                }
                if ui.small_button(t!("Stage all")).clicked() {
                    stage(tab, settings, cmds, Vec::new());
                }
            });
        });
    });
    let mut order = Vec::new();
    let top = ui.cursor().top();
    let out = egui::ScrollArea::vertical()
        .id_salt("unstaged")
        .max_height(half)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            file_list(
                ui,
                tab,
                settings,
                cmds,
                &unstaged,
                DiffSource::Unstaged,
                &mut order,
            );
            if unstaged.is_empty() {
                ui.weak(t!("No unstaged changes."));
            }
            ignored_section(ui, tab, settings, cmds);
        });
    tab.changes.ui.order_unstaged = order;
    drop_zone(
        ui,
        tab,
        settings,
        cmds,
        shown(ui, top, out.inner_rect),
        DiffSource::Unstaged,
    );

    ui.separator();
    ui.horizontal(|ui| {
        let title = if tab.changes.amend {
            t!("Staged + last commit ({})", tab.status.staged.len())
        } else {
            t!("Staged ({})", tab.status.staged.len())
        };
        ui.strong(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    !busy && !tab.status.staged.is_empty(),
                    egui::Button::new(t!("Unstage all")).small(),
                )
                .clicked()
            {
                if tab.changes.amend {
                    let all = tab.status.staged.iter().map(|f| f.path.clone()).collect();
                    cmds.run(Op::UnstageAmend(all));
                } else {
                    cmds.run(Op::UnstageAll);
                }
            }
        });
    });
    let mut order = Vec::new();
    let top = ui.cursor().top();
    let out = egui::ScrollArea::vertical()
        .id_salt("staged")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            file_list(
                ui,
                tab,
                settings,
                cmds,
                &staged,
                DiffSource::Staged,
                &mut order,
            );
            if staged.is_empty() {
                ui.weak(t!("Nothing staged yet."));
            }
        });
    tab.changes.ui.order_staged = order;
    drop_zone(
        ui,
        tab,
        settings,
        cmds,
        shown(ui, top, out.inner_rect),
        DiffSource::Staged,
    );
}

/// Filter field and the options menu.
fn toolbar(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &mut Settings) {
    ui.horizontal(|ui| {
        let prefs = &mut settings.commit;
        ui.menu_button("⚙", |ui| {
            ui.label(t!("Show files as"));
            for (mode, label) in [
                (ListMode::Flat, t!("Flat list")),
                (ListMode::Tree, t!("Tree")),
                (ListMode::Combined, t!("Single list")),
            ] {
                if ui.radio(prefs.list_mode == mode, label).clicked() {
                    prefs.list_mode = mode;
                }
            }
            ui.separator();
            ui.checkbox(&mut prefs.hide_untracked, t!("Hide untracked files"));
            if ui
                .checkbox(&mut prefs.show_ignored, t!("Show ignored files"))
                .changed()
            {
                tab.changes.ui.ignored_at = None;
            }
            ui.separator();
            if ui.button(t!("Add .gitignore template…")).clicked() {
                tab.changes.ui.tool = Some(Tool::templates());
                ui.close();
            }
        })
        .response
        .on_hover_text(t!("List options"));
        ui.add(
            egui::TextEdit::singleline(&mut tab.changes.ui.filter)
                .hint_text(t!("Filter files"))
                .desired_width(f32::INFINITY),
        );
    });
    ui.add_space(2.0);
}

fn refresh_ignored(tab: &mut RepoTab, settings: &Settings) {
    let st = &mut tab.changes.ui;
    if !settings.commit.show_ignored {
        st.ignored.clear();
        st.ignored_at = None;
        return;
    }
    if st
        .ignored_at
        .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
    {
        return;
    }
    st.ignored = tab.repo.ignored_files().unwrap_or_default();
    st.ignored_at = Some(Instant::now());
}

fn ignored_section(ui: &mut egui::Ui, tab: &RepoTab, settings: &Settings, cmds: &mut Commands) {
    if !settings.commit.show_ignored {
        return;
    }
    let filter = tab.changes.ui.filter.trim().to_lowercase();
    let list: Vec<&String> = tab
        .changes
        .ui
        .ignored
        .iter()
        .filter(|p| filter.is_empty() || p.to_lowercase().contains(&filter))
        .collect();
    ui.add_space(4.0);
    egui::CollapsingHeader::new(t!("Ignored ({})", list.len()))
        .id_salt("ignored-files")
        .default_open(false)
        .show(ui, |ui| {
            for p in list {
                ui.weak(p).context_menu(|ui| {
                    if ui.button(t!("Copy relative path")).clicked() {
                        cmds.push(Command::Copy(p.clone()));
                        ui.close();
                    }
                    if ui.button(t!("Copy full path")).clicked() {
                        cmds.push(Command::Copy(full_path(tab, p)));
                        ui.close();
                    }
                });
            }
        });
}

fn full_path(tab: &RepoTab, path: &str) -> String {
    tab.path().join(path).display().to_string()
}

/// The part of the panel a list takes: from `top` down to where drawing
/// stopped (a scroll area's `inner_rect` can reach further).
fn shown(ui: &egui::Ui, top: f32, inner: egui::Rect) -> egui::Rect {
    egui::Rect::from_x_y_ranges(inner.x_range(), top..=ui.cursor().top())
}

/// Accepts files dragged from the other list.
fn drop_zone(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
    rect: egui::Rect,
    onto: DiffSource,
) {
    let id = ui.scope_id().with(("drop", onto == DiffSource::Staged));
    let resp = ui.interact(rect, id, egui::Sense::hover());
    if let Some(drag) = resp.dnd_hover_payload::<DragFiles>()
        && drag.source != onto
    {
        ui.painter().rect_stroke(
            rect,
            4.0,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Inside,
        );
    }
    // Only take the payload when it comes from the other list.
    let from_other = resp
        .dnd_hover_payload::<DragFiles>()
        .is_some_and(|d| d.source != onto);
    if from_other && let Some(drag) = resp.dnd_release_payload::<DragFiles>() {
        drop_files(tab, settings, cmds, &drag, onto);
    }
}

fn file_list(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
    files: &[&FileChange],
    source: DiffSource,
    order: &mut Vec<String>,
) {
    if settings.commit.list_mode == ListMode::Tree {
        let root = Node::build(files);
        tree(ui, tab, settings, cmds, &root, "", source, order);
    } else {
        for f in files {
            order.push(f.path.clone());
            file_row(ui, tab, settings, cmds, f, source, false, None);
        }
    }
}

/// Folders of the tree view.
#[derive(Default)]
struct Node<'a> {
    dirs: BTreeMap<&'a str, Node<'a>>,
    files: Vec<&'a FileChange>,
}

impl<'a> Node<'a> {
    fn build(files: &[&'a FileChange]) -> Self {
        let mut root = Node::default();
        for f in files {
            let mut node = &mut root;
            let mut parts: Vec<&str> = f.path.split('/').collect();
            parts.pop();
            for p in parts {
                node = node.dirs.entry(p).or_default();
            }
            node.files.push(f);
        }
        root
    }

    fn all_paths(&self, out: &mut Vec<String>) {
        for d in self.dirs.values() {
            d.all_paths(out);
        }
        out.extend(self.files.iter().map(|f| f.path.clone()));
    }
}

#[allow(clippy::too_many_arguments)]
fn tree(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
    node: &Node,
    prefix: &str,
    source: DiffSource,
    order: &mut Vec<String>,
) {
    for (name, child) in &node.dirs {
        // Folders with a single subfolder and no files are shown as one row.
        let mut label = (*name).to_owned();
        let mut child = child;
        while child.files.is_empty() && child.dirs.len() == 1 {
            let (n, c) = child.dirs.iter().next().expect("one folder");
            label = format!("{label}/{n}");
            child = c;
        }
        let dir = format!("{prefix}{label}");
        let header = egui::CollapsingHeader::new(format!("{label}/"))
            .id_salt(("changes-tree", source == DiffSource::Staged, &dir))
            .default_open(true)
            .show(ui, |ui| {
                tree(
                    ui,
                    tab,
                    settings,
                    cmds,
                    child,
                    &format!("{dir}/"),
                    source,
                    order,
                );
            });
        let mut paths = Vec::new();
        child.all_paths(&mut paths);
        header.header_response.context_menu(|ui| {
            folder_menu(ui, tab, settings, cmds, source, &dir, paths);
        });
    }
    for f in &node.files {
        order.push(f.path.clone());
        file_row(ui, tab, settings, cmds, f, source, true, None);
    }
}

fn folder_menu(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
    source: DiffSource,
    dir: &str,
    paths: Vec<String>,
) {
    ui.add_enabled_ui(!tab.busy(), |ui| {
        let label = if source == DiffSource::Staged {
            t!("Unstage folder")
        } else {
            t!("Stage folder")
        };
        if ui.button(label).clicked() {
            toggle(tab, settings, cmds, source, paths);
            ui.close();
        }
        if ui.button(t!("Ignore folder")).clicked() {
            changes_tools::ignore(tab, cmds, &format!("/{dir}/"), vec![dir.to_owned()]);
            ui.close();
        }
    });
    ui.separator();
    if ui.button(t!("Copy relative path")).clicked() {
        cmds.push(Command::Copy(dir.to_owned()));
        ui.close();
    }
    if ui.button(t!("Copy full path")).clicked() {
        cmds.push(Command::Copy(full_path(tab, dir)));
        ui.close();
    }
}

/// One list with a checkbox per file: checked when fully staged.
fn combined(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    settings: &mut Settings,
    cmds: &mut Commands,
    unstaged: &[&FileChange],
    staged: &[&FileChange],
) {
    let busy = tab.busy();
    let mut rows: BTreeMap<&str, (Option<&FileChange>, Option<&FileChange>)> = BTreeMap::new();
    for f in unstaged {
        rows.entry(f.path.as_str()).or_default().0 = Some(f);
    }
    for f in staged {
        rows.entry(f.path.as_str()).or_default().1 = Some(f);
    }
    ui.horizontal_wrapped(|ui| {
        ui.strong(t!("Changes ({})", rows.len()));
        ui.add_enabled_ui(!busy, |ui| {
            if !tab.status.unstaged.is_empty() && ui.small_button(t!("Stage all")).clicked() {
                stage(tab, settings, cmds, Vec::new());
            }
            if !tab.status.staged.is_empty() && ui.small_button(t!("Unstage all")).clicked() {
                let all = tab.status.staged.iter().map(|f| f.path.clone()).collect();
                cmds.run(unstage_op(tab, all));
            }
        });
    });
    let mut order = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt("combined")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (unstaged, staged) in rows.values() {
                let (f, source) = match (unstaged, staged) {
                    (Some(u), _) => (*u, DiffSource::Unstaged),
                    (None, Some(s)) => (*s, DiffSource::Staged),
                    (None, None) => continue,
                };
                order.push(f.path.clone());
                let state = match (unstaged.is_some(), staged.is_some()) {
                    (false, true) => Some(true),
                    (true, true) => None,
                    _ => Some(false),
                };
                file_row(ui, tab, settings, cmds, f, source, false, Some(state));
            }
            if rows.is_empty() {
                ui.weak(t!("No local changes."));
            }
            ignored_section(ui, tab, settings, cmds);
        });
    // Picks in the combined list all count as unstaged rows.
    tab.changes.ui.order_unstaged = order.clone();
    tab.changes.ui.order_staged = order;
}

/// A file row. `check` adds the stage checkbox of the combined list:
/// `Some(staged)`, or `None` when the file is partly staged.
#[allow(clippy::too_many_arguments)]
fn file_row(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
    f: &FileChange,
    source: DiffSource,
    name_only: bool,
    check: Option<Option<bool>>,
) {
    let busy = tab.busy();
    let selected = tab
        .changes
        .selected
        .as_ref()
        .is_some_and(|(s, p)| p == &f.path && (*s == source || check.is_some()))
        || (tab.changes.ui.picked_source == Some(source)
            && tab.changes.ui.picked.len() > 1
            && tab.changes.ui.picked.contains(&f.path));
    let shown = if name_only {
        f.path.rsplit('/').next().unwrap_or(&f.path)
    } else {
        &f.path
    };
    let job = change_label(ui, f.kind, shown, f.old_path.as_deref());
    let resp = ui
        .horizontal(|ui| {
            if let Some(state) = check {
                let mut on = state.unwrap_or(false);
                let resp = ui.add_enabled(
                    !busy,
                    egui::Checkbox::without_text(&mut on).indeterminate(state.is_none()),
                );
                if resp.clicked() {
                    if state == Some(true) {
                        cmds.run(unstage_op(tab, vec![f.path.clone()]));
                    } else {
                        stage(tab, settings, cmds, vec![f.path.clone()]);
                    }
                }
            }
            ui.add(egui::Button::selectable(selected, job).truncate())
        })
        .inner
        .interact(egui::Sense::drag());
    if check.is_none() && resp.drag_started() {
        let paths = targets(tab, source, &f.path);
        resp.dnd_set_drag_payload(DragFiles { source, paths });
    }
    if resp.clicked() {
        let mods = ui.input(|i| i.modifiers);
        click(tab, source, &f.path, mods);
    }
    if resp.double_clicked() && !busy {
        if check == Some(Some(true)) || (check.is_none() && source == DiffSource::Staged) {
            cmds.run(unstage_op(tab, vec![f.path.clone()]));
        } else {
            stage(tab, settings, cmds, vec![f.path.clone()]);
        }
    }
    let hover = if check.is_some() {
        t!(
            "{}\nClick the box or press Enter to stage or unstage",
            f.path
        )
    } else if source == DiffSource::Staged {
        t!(
            "{}\nDouble-click, press Enter or drag up to unstage",
            f.path
        )
    } else {
        t!(
            "{}\nDouble-click, press Enter or drag down to stage",
            f.path
        )
    };
    resp.on_hover_text(hover)
        .context_menu(|ui| file_menu(ui, tab, settings, cmds, f, source));
}

fn file_menu(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
    f: &FileChange,
    source: DiffSource,
) {
    let paths = targets(tab, source, &f.path);
    let many = paths.len() > 1;
    let files: Vec<FileChange> = if many {
        let list = if source == DiffSource::Staged {
            &tab.status.staged
        } else {
            &tab.status.unstaged
        };
        list.iter()
            .filter(|c| paths.contains(&c.path))
            .cloned()
            .collect()
    } else {
        vec![f.clone()]
    };
    let tracked: Vec<String> = files
        .iter()
        .filter(|c| c.kind != ChangeKind::Untracked)
        .map(|c| c.path.clone())
        .collect();
    if many {
        ui.weak(t!("{} files selected", paths.len()));
    }
    ui.add_enabled_ui(!tab.busy(), |ui| {
        let label = if source == DiffSource::Staged {
            t!("Unstage")
        } else {
            t!("Stage")
        };
        if ui.button(label).clicked() {
            toggle(tab, settings, cmds, source, paths.clone());
            ui.close();
        }
        if source == DiffSource::Unstaged && ui.button(t!("Discard changes…")).clicked() {
            let text = if many {
                t!(
                    "Discard the changes in {} files? This cannot be undone.",
                    files.len()
                )
            } else if f.kind == ChangeKind::Untracked {
                t!("Delete the new file {}? This cannot be undone.", f.path)
            } else {
                t!("Discard the changes in {}? This cannot be undone.", f.path)
            };
            cmds.push(Command::Dialog(Dialog::confirm(
                &t!("Discard changes"),
                &text,
                Op::Discard(files.clone()),
                true,
            )));
            ui.close();
        }
        if !many
            && f.kind != ChangeKind::Untracked
            && f.kind != ChangeKind::Added
            && ui.button(t!("Reset to revision…")).clicked()
        {
            tab.changes.ui.tool = Some(Tool::reset_to(&f.path));
            ui.close();
        }
        ignore_menu(ui, tab, cmds, &files, &tracked);
    });
    crate::ui::lfs::file_menu(ui, tab, cmds, &tracked);
    ui.separator();
    if ui.button(t!("Copy relative path")).clicked() {
        cmds.push(Command::Copy(paths.join("\n")));
        ui.close();
    }
    if ui.button(t!("Copy full path")).clicked() {
        let full: Vec<String> = paths.iter().map(|p| full_path(tab, p)).collect();
        cmds.push(Command::Copy(full.join("\n")));
        ui.close();
    }
    let patch_source = if source == DiffSource::Staged {
        DiffSource::Staged
    } else {
        DiffSource::Unstaged
    };
    if !many {
        ui.separator();
        file_actions::workdir_menu(ui, tab.path(), &f.path, cmds);
        let button = egui::Button::new(t!("External diff")).shortcut_text("Ctrl+D");
        if ui.add(button).clicked() {
            cmds.push(external_diff_of(tab, source, &f.path));
            ui.close();
        }
        ui.separator();
    }
    if ui.button(t!("Copy as patch")).clicked() {
        changes_tools::copy_patch(tab, cmds, patch_source, &paths);
        ui.close();
    }
    if ui.button(t!("Save as patch…")).clicked() {
        changes_tools::save_patch(tab, cmds, patch_source, &paths);
        ui.close();
    }
    if !many && f.kind != ChangeKind::Untracked && f.kind != ChangeKind::Added {
        ui.separator();
        if ui.button(t!("File history")).clicked() {
            cmds.push(Command::FileHistory(f.path.clone()));
            ui.close();
        }
        if ui.button("Blame").clicked() {
            cmds.push(Command::Blame(f.path.clone()));
            ui.close();
        }
    }
}

fn ignore_menu(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    cmds: &mut Commands,
    files: &[FileChange],
    tracked: &[String],
) {
    let title = if tracked.is_empty() {
        t!("Ignore")
    } else {
        t!("Ignore and stop tracking")
    };
    ui.menu_button(title, |ui| {
        let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        let label = if paths.len() == 1 {
            t!("This file")
        } else {
            t!("These {} files", paths.len())
        };
        if ui.button(label).clicked() {
            let patterns: Vec<String> = paths
                .iter()
                .map(|p| ferrum_core::gitignore::file_pattern(p))
                .collect();
            changes_tools::ignore(tab, cmds, &patterns.join("\n"), tracked.to_vec());
            ui.close();
        }
        if let [one] = paths.as_slice() {
            if let Some(dir) = ferrum_core::gitignore::folder_pattern(one)
                && ui.button(t!("Folder {}", dir)).clicked()
            {
                let folder = dir.trim_matches('/').to_owned();
                changes_tools::ignore(tab, cmds, &dir, vec![folder]);
                ui.close();
            }
            if let Some(ext) = ferrum_core::gitignore::extension_pattern(one)
                && ui.button(t!("All {} files", ext)).clicked()
            {
                changes_tools::ignore(tab, cmds, &ext, tracked.to_vec());
                ui.close();
            }
        }
        if ui.button(t!("Custom pattern…")).clicked() {
            let first = paths.first().cloned().unwrap_or_default();
            tab.changes.ui.tool = Some(Tool::pattern(&first, tracked.to_vec()));
            ui.close();
        }
    });
}

// ----- Diff -----

fn diff(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    let Some((source, path)) = tab.changes.selected.clone() else {
        ui.centered_and_justified(|ui| {
            ui.weak(if tab.status.is_empty() {
                t!("No local changes.")
            } else {
                t!("Select a file to see its changes.")
            })
        });
        return;
    };
    let Some(file) = tab.changes.diff.clone() else {
        ui.weak(t!("No changes to show."));
        return;
    };
    ui.horizontal(|ui| {
        ui.strong(&path);
        ui.weak(if source == DiffSource::Staged {
            t!("staged")
        } else {
            t!("unstaged")
        });
    });
    let mode = if source == DiffSource::Staged {
        DiffMode::Staged
    } else {
        DiffMode::Unstaged
    };
    // Lines of the last commit cannot be unstaged one by one while amending.
    let line_staging =
        supports_line_staging(&file) && !(tab.changes.amend && source == DiffSource::Staged);
    crate::ui::diff_view::offer_reveal(ui.ctx(), "changes-diff");
    let action = crate::ui::diff_extra::show_file(
        ui,
        "changes-diff",
        &tab.repo,
        source,
        &file,
        mode,
        line_staging,
        Some(&mut tab.changes.selection),
    );
    let Some(action) = action else { return };
    if let DiffAction::Reveal(line) = action {
        cmds.push(file_actions::reveal_line(tab.path(), &file, line));
        return;
    }
    if action == DiffAction::LoadFull {
        crate::ui::diff_prefs::load_large(tab, &file.path);
        return;
    }
    if tab.busy() {
        tab.error(t!("Wait for the current operation to finish."));
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
        DiffAction::LoadFull | DiffAction::Reveal(_) => return,
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
                &t!("Discard changes"),
                &t!("Discard the selected changes? This cannot be undone."),
                op,
                true,
            )));
            return;
        }
        (Action::Stage, None) if whole_file => Op::Stage(vec![change.path]),
        (Action::Unstage, None) if whole_file => unstage_op(tab, vec![change.path]),
        (Action::Discard, None) if whole_file => {
            cmds.push(Command::Dialog(Dialog::confirm(
                &t!("Discard changes"),
                &t!(
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

/// Handles a `local:` step of `FERRUM_START`.
pub fn dev_step(tab: &mut RepoTab, settings: &mut Settings, arg: &str) {
    tab.view = crate::tab::View::Changes;
    match arg {
        "tree" => settings.commit.list_mode = ListMode::Tree,
        "flat" => settings.commit.list_mode = ListMode::Flat,
        "combined" => settings.commit.list_mode = ListMode::Combined,
        "amend" => {
            tab.changes.amend = true;
            tab.refresh_status();
        }
        "templates" => tab.changes.ui.tool = Some(Tool::templates()),
        _ => {
            if let Some(msg) = arg.strip_prefix("msg=") {
                tab.changes.message = msg.replace("\\n", "\n");
            } else {
                eprintln!("FERRUM_START: unknown local step {arg}");
            }
        }
    }
}
