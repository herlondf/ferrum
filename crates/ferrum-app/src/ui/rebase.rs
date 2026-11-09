//! Interactive rebase planner: reorder, reword, squash, edit or drop the
//! commits after a base, then run the plan with `Op::InteractiveRebase`.

use eframe::egui::{self, RichText};
use ferrum_core::rebase::{self, Action, Step};
use ferrum_core::{BranchKind, CommitInfo, Oid, RepoState};

use crate::command::{Commands, Op};
use crate::tab::RepoTab;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
}

const KINDS: [Kind; 6] = [
    Kind::Pick,
    Kind::Reword,
    Kind::Edit,
    Kind::Squash,
    Kind::Fixup,
    Kind::Drop,
];

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Pick => "pick",
            Kind::Reword => "reword",
            Kind::Edit => "edit",
            Kind::Squash => "squash",
            Kind::Fixup => "fixup",
            Kind::Drop => "drop",
        }
    }

    fn hint(self) -> String {
        match self {
            Kind::Pick => t!("keep the commit as it is"),
            Kind::Reword => t!("keep the commit, change its message"),
            Kind::Edit => t!("stop after this commit so you can amend it"),
            Kind::Squash => t!("meld into the commit above, keep both messages"),
            Kind::Fixup => t!("meld into the commit above, discard this message"),
            Kind::Drop => t!("remove the commit"),
        }
    }

    /// Keyboard shortcut (the first letter).
    fn key(self) -> egui::Key {
        match self {
            Kind::Pick => egui::Key::P,
            Kind::Reword => egui::Key::R,
            Kind::Edit => egui::Key::E,
            Kind::Squash => egui::Key::S,
            Kind::Fixup => egui::Key::F,
            Kind::Drop => egui::Key::D,
        }
    }

    /// Melds into the previous commit.
    fn attached(self) -> bool {
        matches!(self, Kind::Squash | Kind::Fixup)
    }
}

struct Row {
    commit: CommitInfo,
    kind: Kind,
    /// New message for `reword`, loaded the first time it is chosen.
    message: Option<String>,
}

pub struct RebasePlan {
    pub base: Option<Oid>,
    /// "abc1234 Summary" or "the root commit".
    base_label: String,
    branch: String,
    rows: Vec<Row>,
    /// Commit order when the planner opened.
    original: Vec<Oid>,
    /// How many of the commits are already on the upstream branch, and its name.
    pushed: Option<(usize, String)>,
    /// Merge commits in the range, which the rebase leaves out.
    merges: usize,
    /// Selected rows (by commit), for keyboard actions and moving.
    selected: Vec<Oid>,
    /// Where a Shift+click range starts.
    anchor: Option<Oid>,
    /// Create a backup branch before starting.
    backup: bool,
    /// Move the branches in `dependents` along.
    update_refs: bool,
    /// Other local branches pointing at commits of the plan.
    dependents: Vec<(Oid, String)>,
    /// Closing was asked while the plan has changes.
    confirm_close: bool,
    /// Show avatars, and download them.
    pub avatars: (bool, bool),
}

/// Builds a plan for the commits after `base` (`None` = from the root).
pub fn plan(tab: &RepoTab, base: Option<Oid>) -> Result<RebasePlan, String> {
    if tab.state != RepoState::Clean {
        return Err(t!(
            "Cannot start an interactive rebase: {}. Finish or abort it first.",
            tab.state.label().to_lowercase()
        ));
    }
    let Some(branch) = tab.head.clone() else {
        return Err(t!("HEAD is detached: check out a branch before rebasing."));
    };
    let commits = tab.repo.commits_since(base).map_err(|e| e.to_string())?;
    let merges = commits.iter().filter(|c| c.parents.len() > 1).count();
    let rows: Vec<Row> = commits
        .into_iter()
        .filter(|c| c.parents.len() <= 1)
        .map(|commit| Row {
            commit,
            kind: Kind::Pick,
            message: None,
        })
        .collect();
    if rows.is_empty() {
        return Err(t!("There are no commits to rebase."));
    }

    let upstream = tab.current_branch().and_then(|b| b.upstream.clone());
    let upstream_target = upstream.as_ref().and_then(|u| {
        tab.branches
            .iter()
            .find(|b| b.kind == BranchKind::Remote && &b.name == u)
            .and_then(|b| b.target)
    });
    let pushed = upstream.zip(upstream_target).and_then(|(name, target)| {
        let n = rows
            .iter()
            .filter(|r| tab.repo.is_ancestor(r.commit.id, target))
            .count();
        (n > 0).then_some((n, name))
    });

    let base_label = match base {
        Some(id) => match tab.repo.find_commit_info(id) {
            Ok(c) => format!("{} {}", c.short_id(), c.summary),
            Err(_) => ferrum_core::short(id),
        },
        None => t!("the root commit"),
    };
    // Branches checked out in other worktrees cannot be moved.
    let elsewhere: Vec<&str> = tab
        .worktrees
        .iter()
        .filter_map(|w| w.branch.as_deref())
        .collect();
    let dependents = tab
        .branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local && !b.is_head)
        .filter(|b| !elsewhere.contains(&b.name.as_str()))
        .filter_map(|b| {
            let t = b.target?;
            rows.iter()
                .any(|r| r.commit.id == t)
                .then(|| (t, b.name.clone()))
        })
        .collect();
    Ok(RebasePlan {
        base,
        base_label,
        branch,
        original: rows.iter().map(|r| r.commit.id).collect(),
        rows,
        pushed,
        merges,
        selected: Vec::new(),
        anchor: None,
        backup: false,
        update_refs: false,
        dependents,
        confirm_close: false,
        avatars: (true, false),
    })
}

/// Sets the action of the selected rows. A message is loaded for `reword`.
fn set_kind(plan: &mut RebasePlan, kind: Kind, repo: &ferrum_core::Repo) {
    for row in plan
        .rows
        .iter_mut()
        .filter(|r| plan.selected.contains(&r.commit.id))
    {
        row.kind = kind;
        if kind == Kind::Reword && row.message.is_none() {
            row.message = Some(load_message(repo, &row.commit));
        }
    }
}

fn load_message(repo: &ferrum_core::Repo, c: &CommitInfo) -> String {
    repo.commit_details(c.id)
        .map(|d| d.message.trim_end().to_owned())
        .unwrap_or_else(|_| c.summary.clone())
}

/// Moves the selected rows one place up (or down), keeping their order.
fn move_selected<T>(rows: &mut [T], selected: impl Fn(&T) -> bool, up: bool) {
    let picked: Vec<usize> = (0..rows.len()).filter(|&i| selected(&rows[i])).collect();
    let blocked = if up {
        picked.first() == Some(&0)
    } else {
        picked.last() == Some(&(rows.len().wrapping_sub(1)))
    };
    if picked.is_empty() || blocked {
        return;
    }
    if up {
        for i in picked {
            rows.swap(i, i - 1);
        }
    } else {
        for i in picked.into_iter().rev() {
            rows.swap(i, i + 1);
        }
    }
}

/// Click, Ctrl+click (toggle) and Shift+click (range) on a row.
fn select(plan: &mut RebasePlan, id: Oid, mods: egui::Modifiers) {
    if mods.shift
        && let Some(anchor) = plan.anchor
    {
        let pos = |x: Oid| plan.rows.iter().position(|r| r.commit.id == x);
        if let (Some(a), Some(b)) = (pos(anchor), pos(id)) {
            let (lo, hi) = (a.min(b), a.max(b));
            plan.selected = plan.rows[lo..=hi].iter().map(|r| r.commit.id).collect();
            return;
        }
    }
    if mods.command {
        match plan.selected.iter().position(|x| *x == id) {
            Some(p) => {
                plan.selected.remove(p);
            }
            None => plan.selected.push(id),
        }
    } else {
        plan.selected = vec![id];
    }
    plan.anchor = Some(id);
}

/// Keyboard shortcuts: P R E S F D set the action of the selected commits,
/// Alt+Up / Alt+Down (or Ctrl) move them, Esc closes. Not while typing.
fn keys(ctx: &egui::Context, plan: &mut RebasePlan, repo: &ferrum_core::Repo) -> bool {
    if ctx.egui_wants_keyboard_input() {
        return false;
    }
    let mut close = false;
    ctx.input_mut(|i| {
        if !plan.selected.is_empty() {
            for k in KINDS {
                if i.consume_key(egui::Modifiers::NONE, k.key()) {
                    set_kind(plan, k, repo);
                }
            }
            for m in [egui::Modifiers::ALT, egui::Modifiers::COMMAND] {
                for (key, up) in [(egui::Key::ArrowUp, true), (egui::Key::ArrowDown, false)] {
                    if i.consume_key(m, key) {
                        let sel = plan.selected.clone();
                        move_selected(&mut plan.rows, |r| sel.contains(&r.commit.id), up);
                    }
                }
            }
        }
        close = i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
    });
    close
}

fn steps(rows: &[Row]) -> Vec<Step> {
    rows.iter()
        .map(|r| Step {
            commit: r.commit.clone(),
            action: match r.kind {
                Kind::Pick => Action::Pick,
                Kind::Reword => Action::Reword(r.message.clone().unwrap_or_default()),
                Kind::Edit => Action::Edit,
                Kind::Squash => Action::Squash,
                Kind::Fixup => Action::Fixup,
                Kind::Drop => Action::Drop,
            },
        })
        .collect()
}

/// Checks the plan, with the same rules as git plus a non-empty reword message.
fn check(rows: &[Row]) -> Result<(), String> {
    if rows.iter().all(|r| r.kind == Kind::Drop) {
        return Err(t!("Every commit is dropped: keep at least one."));
    }
    if let Some(r) = rows
        .iter()
        .find(|r| r.kind == Kind::Reword && r.message.as_deref().unwrap_or("").trim().is_empty())
    {
        return Err(t!("The new message for {} is empty.", r.commit.short_id()));
    }
    rebase::validate(&steps(rows))
}

fn unchanged(rows: &[Row], original: &[Oid]) -> bool {
    rows.iter().all(|r| r.kind == Kind::Pick)
        && rows
            .iter()
            .map(|r| r.commit.id)
            .eq(original.iter().copied())
}

/// "4 commits will become 3 (1 dropped, 1 reworded)".
fn summary(rows: &[Row], original: &[Oid]) -> String {
    let count = |k: Kind| rows.iter().filter(|r| r.kind == k).count();
    let result = rows
        .iter()
        .filter(|r| r.kind != Kind::Drop && !r.kind.attached())
        .count();
    let mut parts = Vec::new();
    if !rows
        .iter()
        .map(|r| r.commit.id)
        .eq(original.iter().copied())
    {
        parts.push(t!("reordered"));
    }
    for kind in [
        Kind::Drop,
        Kind::Reword,
        Kind::Squash,
        Kind::Fixup,
        Kind::Edit,
    ] {
        let n = count(kind);
        let part = match (kind, n) {
            (_, 0) => continue,
            (Kind::Drop, 1) => t!("1 dropped"),
            (Kind::Drop, n) => t!("{} dropped", n),
            (Kind::Reword, 1) => t!("1 reworded"),
            (Kind::Reword, n) => t!("{} reworded", n),
            (Kind::Squash, 1) => t!("1 squashed"),
            (Kind::Squash, n) => t!("{} squashed", n),
            (Kind::Fixup, 1) => t!("1 fixed up"),
            (Kind::Fixup, n) => t!("{} fixed up", n),
            (_, 1) => t!("1 to edit"),
            (_, n) => t!("{} to edit", n),
        };
        parts.push(part);
    }
    let mut text = if rows.len() == 1 {
        t!("1 commit will become {}", result)
    } else {
        t!("{} commits will become {}", rows.len(), result)
    };
    if !parts.is_empty() {
        text.push_str(&format!(" ({})", parts.join(", ")));
    }
    text
}

/// Moves row `from` so it lands before the row that was at `to` (`to == len` = last).
fn move_row<T>(rows: &mut Vec<T>, from: usize, to: usize) {
    if from >= rows.len() || to > rows.len() {
        return;
    }
    let row = rows.remove(from);
    let to = if to > from { to - 1 } else { to };
    rows.insert(to, row);
}

/// Shows the planner window while `tab.rebase` is set.
pub fn show(ctx: &egui::Context, tab: &mut RepoTab, cmds: &mut Commands) {
    let busy = tab.busy();
    let repo = &tab.repo;
    let Some(plan) = tab.rebase.as_mut() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    // Esc (or the window's ✕) asks first when the plan has changes.
    let mut ask_close = keys(ctx, plan, repo);
    egui::Window::new(t!("Interactive rebase"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(780.0)
        .default_pos({
            let c = ctx.content_rect().center();
            egui::pos2((c.x - 390.0).max(0.0), (c.y - 280.0).max(40.0))
        })
        .min_width(560.0)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(if plan.rows.len() == 1 {
                    t!("Rebase 1 commit of {} onto {}", plan.branch, plan.base_label)
                } else {
                    t!("Rebase {} commits of {} onto {}", plan.rows.len(), plan.branch, plan.base_label)
                })
                .strong(),
            );
            ui.weak(t!("Commits are applied from top to bottom (oldest first). Drag ☰ or use ⬆ ⬇ to reorder."));
            ui.weak(t!("Click to select (Ctrl/Shift for several), then press P R E S F D to pick, reword, edit, squash, fix up or drop, and Alt+⬆ ⬇ to move."));
            if let Some((n, upstream)) = &plan.pushed {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    if *n == 1 {
                        t!("⚠ 1 of these commits is already on {}: rewriting it means you will need to force push.", upstream)
                    } else {
                        t!("⚠ {} of these commits are already on {}: rewriting them means you will need to force push.", n, upstream)
                    },
                );
            }
            if plan.merges > 0 {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    if plan.merges == 1 {
                        t!("⚠ 1 merge commit is left out: the rebase replays only regular commits.")
                    } else {
                        t!("⚠ {} merge commits are left out: the rebase replays only regular commits.", plan.merges)
                    },
                );
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height((ui.ctx().content_rect().height() - 300.0).max(200.0))
                .auto_shrink([false, true])
                .show(ui, |ui| rows_view(ui, plan, repo));
            ui.separator();

            let result = check(&plan.rows);
            let same = unchanged(&plan.rows, &plan.original);
            ui.label(summary(&plan.rows, &plan.original));
            match &result {
                Err(e) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                Ok(()) if same => {
                    ui.weak(t!("Nothing to do yet: change an action or the order of the commits."));
                }
                Ok(()) => {}
            }
            ui.weak(t!("Local changes are stashed automatically and restored afterwards."));
            ui.checkbox(&mut plan.backup, t!("Back up the current state in a branch first"))
                .on_hover_text(t!("Creates a backup/… branch at the current commit, to get back to it later"));
            if !plan.dependents.is_empty() {
                let names: Vec<&str> = plan.dependents.iter().map(|(_, n)| n.as_str()).collect();
                ui.checkbox(&mut plan.update_refs, t!("Update dependent branches: {}", names.join(", ")))
                    .on_hover_text(t!("Move other branches that point at the rebased commits along with them"));
            }
            ui.add_space(4.0);
            if plan.confirm_close {
                ui.horizontal(|ui| {
                    ui.colored_label(ui.visuals().warn_fg_color, t!("Discard the changes to this plan?"));
                    if ui.button(t!("Discard")).clicked() {
                        close = true;
                    }
                    if ui.button(t!("Keep editing")).clicked() {
                        plan.confirm_close = false;
                    }
                });
                return;
            }
            ui.horizontal(|ui| {
                let can_start = result.is_ok() && !same && !busy;
                if ui
                    .add_enabled(can_start, crate::theme::primary(t!("Start rebase")))
                    .clicked()
                {
                    cmds.run(start_op(plan));
                    close = true;
                }
                if ui.add(crate::theme::secondary(t!("Cancel"))).clicked() {
                    ask_close = true;
                }
            });
        });
    if !open {
        ask_close = true;
    }
    if ask_close {
        if unchanged(&plan.rows, &plan.original) || plan.confirm_close {
            close = true;
        } else {
            plan.confirm_close = true;
        }
    }
    if close {
        tab.rebase = None;
    }
}

/// The operation that runs the plan, with its options.
fn start_op(plan: &RebasePlan) -> Op {
    let opts = rebase::Options {
        update_refs: if plan.update_refs {
            plan.dependents.clone()
        } else {
            Vec::new()
        },
        backup: plan.backup.then(|| rebase::backup_name(&plan.branch)),
    };
    let op = Op::InteractiveRebase {
        base: plan.base,
        steps: steps(&plan.rows),
    };
    if opts == rebase::Options::default() {
        return op;
    }
    Op::Flow(crate::flow::FlowOp::Plan {
        label: op.label(),
        base: plan.base,
        steps: steps(&plan.rows),
        opts,
    })
}

fn rows_view(ui: &mut egui::Ui, plan: &mut RebasePlan, repo: &ferrum_core::Repo) {
    let len = plan.rows.len();
    let dragged = egui::DragAndDrop::payload::<usize>(ui.ctx()).map(|p| *p);
    let mut moved = None;
    let mut clicked = None;
    let weak = ui.visuals().weak_text_color();
    let (show_avatars, network) = plan.avatars;
    for i in 0..len {
        let is_selected = plan.selected.contains(&plan.rows[i].commit.id);
        let row = &mut plan.rows[i];
        let attached = row.kind.attached();
        let dropped = row.kind == Kind::Drop;
        let fill = if dragged == Some(i) {
            ui.visuals().selection.bg_fill.gamma_multiply(0.4)
        } else if is_selected {
            ui.visuals().selection.bg_fill.gamma_multiply(0.7)
        } else if dropped {
            egui::Color32::TRANSPARENT
        } else {
            ui.visuals().faint_bg_color
        };
        if !attached {
            ui.add_space(2.0);
        }
        let resp = egui::Frame::new()
            .fill(fill)
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if attached {
                        ui.add_space(18.0);
                        ui.label(RichText::new("⤴").size(16.0))
                            .on_hover_text(t!("Melded into the commit above"));
                    }
                    let handle = ui
                        .add(
                            egui::Label::new(RichText::new("☰").color(weak))
                                .sense(egui::Sense::drag()),
                        )
                        .on_hover_text(t!("Drag to reorder"));
                    handle.dnd_set_drag_payload(i);
                    if ui
                        .add_enabled(i > 0, egui::Button::new("⬆").small())
                        .on_hover_text(t!("Move up"))
                        .clicked()
                    {
                        moved = Some((i, i - 1));
                    }
                    if ui
                        .add_enabled(i + 1 < len, egui::Button::new("⬇").small())
                        .on_hover_text(t!("Move down"))
                        .clicked()
                    {
                        moved = Some((i, i + 2));
                    }
                    let before = row.kind;
                    egui::ComboBox::from_id_salt(("rebase-action", row.commit.id))
                        .selected_text(row.kind.label())
                        .width(78.0)
                        .show_ui(ui, |ui| {
                            ui.set_min_width(460.0);
                            for k in KINDS {
                                let text = (
                                    RichText::new(format!("{:<7}", k.label())).monospace(),
                                    RichText::new(k.hint()).color(weak),
                                );
                                if ui.selectable_label(row.kind == k, text).clicked() {
                                    row.kind = k;
                                }
                            }
                        })
                        .response
                        .on_hover_text(row.kind.hint());
                    if row.kind == Kind::Reword && before != Kind::Reword && row.message.is_none() {
                        row.message = Some(load_message(repo, &row.commit));
                    }
                    if show_avatars {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        let painter = ui.painter().clone();
                        crate::ui::avatars::paint(
                            ui,
                            &painter,
                            r,
                            &row.commit.author,
                            &row.commit.email,
                            network,
                        );
                    }
                    let mut hash = RichText::new(row.commit.short_id()).monospace().color(weak);
                    let mut text = RichText::new(&row.commit.summary);
                    if dropped {
                        hash = hash.strikethrough();
                        text = text.strikethrough().color(weak);
                    }
                    if ui
                        .add(egui::Label::new(hash).sense(egui::Sense::click()))
                        .clicked()
                    {
                        clicked = Some(row.commit.id);
                    }
                    let summary = ui
                        .add(
                            egui::Label::new(text)
                                .truncate()
                                .sense(egui::Sense::click_and_drag()),
                        )
                        .on_hover_text(format!("{}\n{}", row.commit.author, row.commit.summary));
                    if summary.clicked() {
                        clicked = Some(row.commit.id);
                    }
                    summary.dnd_set_drag_payload(i);
                });
                if row.kind == Kind::Reword
                    && let Some(msg) = row.message.as_mut()
                {
                    ui.add(
                        egui::TextEdit::multiline(msg)
                            .desired_width(f32::INFINITY)
                            .desired_rows(3)
                            .hint_text(t!("Commit message")),
                    );
                }
            })
            .response;

        // Drop target: insert above or below this row depending on the pointer.
        if let (Some(from), Some(pos)) = (
            resp.dnd_hover_payload::<usize>(),
            ui.input(|i| i.pointer.interact_pos()),
        ) {
            let below = pos.y > resp.rect.center().y;
            let y = if below {
                resp.rect.bottom() + 1.0
            } else {
                resp.rect.top() - 1.0
            };
            if *from != i {
                ui.painter().hline(
                    resp.rect.x_range(),
                    y,
                    egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                );
            }
            if let Some(from) = resp.dnd_release_payload::<usize>() {
                moved = Some((*from, if below { i + 1 } else { i }));
            }
        }
    }
    if let Some((from, to)) = moved {
        move_row(&mut plan.rows, from, to);
    }
    if let Some(id) = clicked {
        let mods = ui.input(|i| i.modifiers);
        select(plan, id, mods);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: u8) -> Vec<Row> {
        (0..n)
            .map(|i| Row {
                commit: CommitInfo {
                    id: Oid::from_bytes(&[i + 1; 20]).unwrap(),
                    summary: format!("c{i}"),
                    author: String::new(),
                    email: String::new(),
                    time: 0,
                    offset_minutes: 0,
                    parents: Vec::new(),
                },
                kind: Kind::Pick,
                message: None,
            })
            .collect()
    }

    fn ids(rows: &[Row]) -> Vec<Oid> {
        rows.iter().map(|r| r.commit.id).collect()
    }

    #[test]
    fn moves_rows() {
        let mut v = vec![0, 1, 2, 3];
        move_row(&mut v, 0, 2);
        assert_eq!(v, [1, 0, 2, 3]);
        move_row(&mut v, 3, 0);
        assert_eq!(v, [3, 1, 0, 2]);
        move_row(&mut v, 1, 4);
        assert_eq!(v, [3, 0, 2, 1]);
        move_row(&mut v, 2, 2);
        assert_eq!(v, [3, 0, 2, 1]);
    }

    #[test]
    fn summarizes_and_detects_changes() {
        let mut r = rows(4);
        let original = ids(&r);
        assert!(unchanged(&r, &original));
        assert_eq!(summary(&r, &original), "4 commits will become 4");

        r[1].kind = Kind::Drop;
        r[2].kind = Kind::Squash;
        r[3].kind = Kind::Reword;
        r[3].message = Some("new".into());
        assert!(!unchanged(&r, &original));
        assert_eq!(
            summary(&r, &original),
            "4 commits will become 2 (1 dropped, 1 reworded, 1 squashed)"
        );
        assert!(check(&r).is_ok());

        let mut moved = rows(2);
        let original = ids(&moved);
        move_row(&mut moved, 1, 0);
        assert!(!unchanged(&moved, &original));
        assert_eq!(
            summary(&moved, &original),
            "2 commits will become 2 (reordered)"
        );
    }

    #[test]
    fn rejects_invalid_plans() {
        let mut r = rows(3);
        r[0].kind = Kind::Drop;
        r[1].kind = Kind::Fixup;
        assert!(check(&r).is_err(), "first kept commit cannot be squashed");

        let mut r = rows(2);
        r[1].kind = Kind::Reword;
        r[1].message = Some("  ".into());
        assert!(check(&r).is_err(), "empty reword message");

        let mut r = rows(1);
        r[0].kind = Kind::Drop;
        assert!(check(&r).is_err(), "everything dropped");
    }
}
