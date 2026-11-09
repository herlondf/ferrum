//! Moving hunks or lines of a commit into another commit of the current
//! branch, or splitting them into a new commit. In the commit details, a bar
//! above the diff of a commit of the current branch offers "Move to ▾" and a
//! handle to drag the selection (lines clicked in the diff, or the whole
//! file) onto a commit of the history. A dialog confirms before the rebase.

use std::sync::Arc;

use eframe::egui::{self, Rect, RichText};
use ferrum_core::diff::{partial_patch, supports_line_staging};
use ferrum_core::hunk_move::{self, Dest, Part};
use ferrum_core::{CommitInfo, FileDiff, Oid, Repo, short};

use crate::command::{Command, Commands, Op};
use crate::icons;
use crate::stack_ops::{self, StackOp};
use crate::tab::{LineSelection, RepoTab};
use crate::theme;
use crate::ui::dialogs::{Dialog, buttons};
use crate::ui::stacks::{StackDialog, backup_note, note};

/// What is being dragged onto a commit.
#[derive(Clone)]
pub struct DragHunks {
    pub source: Oid,
    pub parts: Vec<Part>,
    /// "3 lines of a.txt", "a.txt".
    pub what: String,
}

#[derive(Clone)]
pub struct MoveForm {
    source: Oid,
    parts: Vec<Part>,
    what: String,
    /// `None`: a new commit.
    target: Option<Oid>,
    message: String,
}

pub fn dialog(drag: &DragHunks, target: Option<Oid>) -> Dialog {
    Dialog::Stack(StackDialog::Move(Box::new(MoveForm {
        source: drag.source,
        parts: drag.parts.clone(),
        what: drag.what.clone(),
        target,
        message: String::new(),
    })))
}

// ----- State kept between frames -----

#[derive(Clone, Default)]
struct Picked {
    key: Option<(Oid, String)>,
    sel: LineSelection,
}

#[derive(Clone, Default)]
struct BranchCache {
    key: Option<(std::path::PathBuf, Option<Oid>, Option<String>)>,
    commits: Arc<Vec<CommitInfo>>,
}

/// Set by the `stack:pick` dev step.
pub static PICK_ALL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn picked_id() -> egui::Id {
    egui::Id::unique("move-hunks-selection")
}

/// Commits of the current branch that can be changed (cached per HEAD).
pub fn branch_commits(ctx: &egui::Context, repo: &Repo) -> Arc<Vec<CommitInfo>> {
    let key = Some((
        repo.path().to_path_buf(),
        repo.head_oid(),
        repo.head_branch(),
    ));
    let id = egui::Id::unique("move-hunks-branch");
    let cached: BranchCache = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    if cached.key == key {
        return cached.commits;
    }
    let commits = Arc::new(ferrum_core::absorb::branch_commits(repo, 500).unwrap_or_default());
    let fresh = BranchCache {
        key,
        commits: commits.clone(),
    };
    ctx.data_mut(|d| d.insert_temp(id, fresh));
    commits
}

/// The parts to move: the selected lines, or the whole file.
fn parts(file: &FileDiff, sel: Option<&LineSelection>) -> (Vec<Part>, String) {
    let name = file
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&file.path)
        .to_owned();
    if let Some(s) = sel.filter(|s| !s.lines.is_empty())
        && let Some(p) = partial_patch(file, |h, l| s.lines.contains(&(h, l)), true)
    {
        let n = s.lines.len();
        let what = if n == 1 {
            t!("1 line of {}", name)
        } else {
            t!("{} lines of {}", n, name)
        };
        return (vec![Part::Lines(p)], what);
    }
    let mut paths = vec![file.path.clone()];
    paths.extend(file.old_path.clone());
    (vec![Part::Files(paths)], name)
}

/// The bar above the diff of `commit`'s `file`. Returns the line selection
/// to give the diff (and to `store` afterwards) when lines can be picked.
pub fn bar(
    ui: &mut egui::Ui,
    repo: &Repo,
    commit: Oid,
    file: &FileDiff,
    ignore_whitespace: bool,
    busy: bool,
    cmds: &mut Commands,
) -> Option<LineSelection> {
    let ctx = ui.ctx().clone();
    let mine = branch_commits(&ctx, repo);
    let pos = mine.iter().position(|c| c.id == commit)?;
    let key = Some((commit, file.path.clone()));
    let mut p: Picked = ctx.data(|d| d.get_temp(picked_id())).unwrap_or_default();
    if p.key != key {
        p = Picked {
            key,
            sel: LineSelection::default(),
        };
    }
    let lines_ok = supports_line_staging(file) && !ignore_whitespace;
    let sel = lines_ok.then_some(&p.sel);
    let (parts, what) = parts(file, sel);
    let drag = DragHunks {
        source: commit,
        parts,
        what: what.clone(),
    };
    let mut clear = false;
    // `stack:pick` (FERRUM_START) selects every changed line once.
    let mut select: Option<Option<usize>> = PICK_ALL
        .swap(false, std::sync::atomic::Ordering::Relaxed)
        .then_some(None);
    let frame = egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::symmetric(6, 3));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE_S;
            let weak = ui.visuals().weak_text_color();
            let handle = ui
                .add(
                    egui::Label::new(RichText::new(icons::DOTS_SIX_VERTICAL).color(weak))
                        .sense(egui::Sense::drag()),
                )
                .on_hover_text(t!("Drag onto a commit in the history to move it there"));
            if handle.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            if handle.drag_started() && !busy {
                egui::DragAndDrop::set_payload(ui.ctx(), drag.clone());
            }
            ui.label(RichText::new(icons::ARROWS_LEFT_RIGHT).color(theme::accent(ui)));
            ui.label(theme::strong(t!("Move {}", what)).size(12.5));
            ui.add_enabled_ui(!busy, |ui| {
                ui.menu_button(format!("{} {}", t!("to…"), icons::CARET_DOWN), |ui| {
                    ui.set_min_width(300.0);
                    let new = icons::text(icons::PLUS, t!("A new commit right after it…"));
                    if ui.button(new).clicked() {
                        cmds.push(Command::Dialog(dialog(&drag, None)));
                        ui.close();
                    }
                    ui.separator();
                    ui.weak(t!("Into a commit of this branch"));
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for (i, c) in mine.iter().enumerate() {
                                if i == pos {
                                    continue;
                                }
                                let label = format!("{}  {}", short(c.id), c.summary);
                                let b = egui::Button::new(RichText::new(icons::text(
                                    icons::GIT_COMMIT,
                                    label,
                                )))
                                .truncate();
                                if ui.add(b).clicked() {
                                    cmds.push(Command::Dialog(dialog(&drag, Some(c.id))));
                                    ui.close();
                                }
                            }
                        });
                });
            });
            if lines_ok {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(icons::text(icons::CURSOR_CLICK, t!("Select")), |ui| {
                        if ui.button(t!("All changes")).clicked() {
                            select = Some(None);
                            ui.close();
                        }
                        for (h, hunk) in file.hunks.iter().enumerate() {
                            let label = t!(
                                "Hunk {}: lines {}–{}",
                                h + 1,
                                hunk.new_start,
                                hunk.new_start + hunk.new_lines.max(1) - 1
                            );
                            if ui.button(label).clicked() {
                                select = Some(Some(h));
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button(t!("Clear selection")).clicked() {
                            clear = true;
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text(t!("Or click lines in the diff, Shift+click for a range"));
                });
            } else if ignore_whitespace && supports_line_staging(file) {
                ui.weak(icons::INFO)
                    .on_hover_text(t!("Turn off “Ignore whitespace” to move single lines"));
            }
        });
    });
    if clear {
        p.sel.clear();
    }
    if let Some(which) = select {
        p.sel.clear();
        for (h, hunk) in file.hunks.iter().enumerate() {
            if which.is_some_and(|w| w != h) {
                continue;
            }
            for (l, line) in hunk.lines.iter().enumerate() {
                if line.kind != ferrum_core::LineKind::Context {
                    p.sel.lines.insert((h, l));
                }
            }
        }
    }
    ghost(&ctx);
    let out = lines_ok.then(|| p.sel.clone());
    ctx.data_mut(|d| d.insert_temp(picked_id(), p));
    out
}

/// Keeps the selection the diff changed.
pub fn store(ctx: &egui::Context, sel: LineSelection) {
    ctx.data_mut(|d| {
        let p: &mut Picked = d.get_temp_mut_or_default(picked_id());
        p.sel = sel;
    });
}

/// The dragged changes next to the pointer.
fn ghost(ctx: &egui::Context) {
    let Some(p) = egui::DragAndDrop::payload::<DragHunks>(ctx) else {
        return;
    };
    let Some(pos) = ctx.pointer_interact_pos() else {
        return;
    };
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    egui::Area::new(egui::Id::unique("move-hunks-ghost"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + egui::vec2(16.0, 10.0))
        .interactable(false)
        .show(ctx, |ui| {
            crate::ui::branch_dnd::pill(ui, icons::ARROWS_LEFT_RIGHT, &p.what, false);
        });
}

/// A history row as a drop target for dragged changes.
pub fn graph_row(
    ui: &egui::Ui,
    tab: &RepoTab,
    resp: &egui::Response,
    rect: Rect,
    id: Oid,
    cmds: &mut Commands,
) {
    let Some(p) = egui::DragAndDrop::payload::<DragHunks>(ui.ctx()) else {
        return;
    };
    let over = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|pos| rect.contains(pos));
    if !over || !resp.contains_pointer() || p.source == id {
        return;
    }
    let mine = branch_commits(ui.ctx(), &tab.repo);
    if !mine.iter().any(|c| c.id == id) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::NoDrop);
        return;
    }
    crate::ui::branch_dnd::highlight(ui, rect);
    if ui.input(|i| i.pointer.any_released())
        && let Some(p) = egui::DragAndDrop::take_payload::<DragHunks>(ui.ctx())
    {
        cmds.push(Command::Dialog(dialog(&p, Some(id))));
    }
}

pub fn show(
    ui: &mut egui::Ui,
    f: &mut MoveForm,
    tab: &RepoTab,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.set_width(540.0);
    let new = f.target.is_none();
    ui.heading(if new {
        t!("Split into a new commit")
    } else {
        t!("Move to another commit")
    });
    let commit_row = |ui: &mut egui::Ui, label: String, id: Oid| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE_S;
            ui.label(RichText::new(label).weak());
            ui.label(RichText::new(icons::GIT_COMMIT).color(theme::accent(ui)));
            ui.label(
                RichText::new(short(id))
                    .font(crate::util::code_font(12.0))
                    .weak(),
            );
            let summary = tab
                .repo
                .find_commit_info(id)
                .map(|c| c.summary)
                .unwrap_or_default();
            ui.add(egui::Label::new(theme::strong(summary)).truncate());
        });
    };
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(icons::ARROWS_LEFT_RIGHT).color(theme::accent(ui)));
            ui.label(theme::strong(&f.what));
        });
        ui.add_space(theme::SPACE_XS);
        commit_row(ui, t!("From"), f.source);
        match f.target {
            Some(t) => commit_row(ui, t!("Into"), t),
            None => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t!("Into")).weak());
                    ui.label(t!("a new commit right after it"));
                });
            }
        }
    });
    if new {
        ui.add_space(theme::SPACE_S);
        let resp = crate::ui::dialogs::field(ui, &t!("Message"), &mut f.message, "");
        if f.message.is_empty() {
            resp.request_focus();
        }
    }
    ui.add_space(theme::SPACE_S);
    let dest = match f.target {
        Some(t) => Dest::Commit(t),
        None => Dest::New(f.message.trim().to_owned()),
    };
    let check = hunk_move::check(&tab.repo, f.source, &dest);
    match &check {
        Err(e) if !(new && f.message.trim().is_empty()) => {
            note(ui, icons::WARNING, e, Some(ui.visuals().warn_fg_color));
        }
        _ => {
            note(
                ui,
                icons::INFO,
                &t!(
                    "One interactive rebase rewrites the commits from there up. If it stops for a conflict, resolve it and continue, or abort to leave everything as it was."
                ),
                None,
            );
        }
    }
    if crate::ui::stacks::has_local_changes(tab) {
        note(
            ui,
            icons::INFO,
            &t!("Local changes are stashed during the rebase and put back afterwards."),
            None,
        );
    }
    // The commits the rebase rewrites: from the older of the two up.
    let all = branch_commits(ui.ctx(), &tab.repo);
    let oldest = [Some(f.source), f.target]
        .into_iter()
        .flatten()
        .filter_map(|id| all.iter().position(|c| c.id == id))
        .max()
        .map_or(0, |i| i + 1);
    let mine: Vec<Oid> = all.iter().take(oldest).map(|c| c.id).collect();
    let branches = stack_ops::branches_at(tab, &mine);
    let update_refs = ferrum_core::stack::git_supports_update_refs();
    if branches.len() > 1 {
        let others = branches[1..].join(", ");
        let text = if update_refs {
            t!("Branches on these commits move along: {}", others)
        } else {
            t!(
                "This git is older than 2.38: {} will not move along.",
                others
            )
        };
        note(ui, icons::STACK, &text, None);
    }
    backup_note(ui);
    let ok = check.is_ok() && !tab.busy() && tab.head.is_some();
    let label = if new { t!("Split") } else { t!("Move") };
    let r = buttons(ui, &label, ok, false, enter && ok);
    if r == Some(true) {
        let to = match f.target {
            Some(t) => stack_ops::commit_title(tab, t),
            None => t!("a new commit"),
        };
        cmds.run(Op::Stack(StackOp::Move {
            source: f.source,
            parts: f.parts.clone(),
            dest,
            update_refs,
            backup: stack_ops::backup_id(tab, "move"),
            branches,
            what: f.what.clone(),
            to,
        }));
        // The commit is rewritten: forget its selection.
        ui.ctx().data_mut(|d| d.remove::<Picked>(picked_id()));
    }
    r.is_none()
}

/// `stack:move=PATH>REV` (or `PATH>new`): the dialog moving PATH out of
/// HEAD into REV.
pub fn dev_step(arg: &str, tab: &RepoTab) -> Vec<Command> {
    let (path, to) = arg.split_once('>').unwrap_or((arg, "new"));
    let Some(head) = tab.head_oid else {
        return Vec::new();
    };
    let target = (to != "new").then(|| tab.repo.resolve(to).ok()).flatten();
    let file = tab.repo.commit_file_diff(head, path, None).ok().flatten();
    let Some(file) = file else {
        eprintln!("FERRUM_START: {path} is not changed in HEAD");
        return Vec::new();
    };
    let (parts, what) = parts(&file, None);
    let drag = DragHunks {
        source: head,
        parts,
        what,
    };
    vec![Command::Dialog(dialog(&drag, target))]
}
