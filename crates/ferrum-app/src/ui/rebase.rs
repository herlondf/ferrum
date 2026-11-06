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

    fn hint(self) -> &'static str {
        match self {
            Kind::Pick => "keep the commit as it is",
            Kind::Reword => "keep the commit, change its message",
            Kind::Edit => "stop after this commit so you can amend it",
            Kind::Squash => "meld into the commit above, keep both messages",
            Kind::Fixup => "meld into the commit above, discard this message",
            Kind::Drop => "remove the commit",
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
}

/// Builds a plan for the commits after `base` (`None` = from the root).
pub fn plan(tab: &RepoTab, base: Option<Oid>) -> Result<RebasePlan, String> {
    if tab.state != RepoState::Clean {
        return Err(format!(
            "Cannot start an interactive rebase: {}. Finish or abort it first.",
            tab.state.label().to_lowercase()
        ));
    }
    let Some(branch) = tab.head.clone() else {
        return Err("HEAD is detached: check out a branch before rebasing.".into());
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
        return Err("There are no commits to rebase.".into());
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
        None => "the root commit".into(),
    };
    Ok(RebasePlan {
        base,
        base_label,
        branch,
        original: rows.iter().map(|r| r.commit.id).collect(),
        rows,
        pushed,
        merges,
    })
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
        return Err("Every commit is dropped: keep at least one.".into());
    }
    if let Some(r) = rows
        .iter()
        .find(|r| r.kind == Kind::Reword && r.message.as_deref().unwrap_or("").trim().is_empty())
    {
        return Err(format!(
            "The new message for {} is empty.",
            r.commit.short_id()
        ));
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

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
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
        parts.push("reordered".to_owned());
    }
    for (kind, word) in [
        (Kind::Drop, "dropped"),
        (Kind::Reword, "reworded"),
        (Kind::Squash, "squashed"),
        (Kind::Fixup, "fixed up"),
        (Kind::Edit, "to edit"),
    ] {
        let n = count(kind);
        if n > 0 {
            parts.push(format!("{n} {word}"));
        }
    }
    let mut text = format!("{} will become {}", plural(rows.len(), "commit"), result);
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
    egui::Window::new("Interactive rebase")
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
                RichText::new(format!(
                    "Rebase {} of {} onto {}",
                    plural(plan.rows.len(), "commit"),
                    plan.branch,
                    plan.base_label
                ))
                .strong(),
            );
            ui.weak(
                "Commits are applied from top to bottom (oldest first). Drag ☰ or use ⬆ ⬇ to reorder.",
            );
            if let Some((n, upstream)) = &plan.pushed {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "⚠ {} already on {upstream}: rewriting them means you will need to force push.",
                        if *n == 1 {
                            "1 of these commits is".to_owned()
                        } else {
                            format!("{n} of these commits are")
                        }
                    ),
                );
            }
            if plan.merges > 0 {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "⚠ {} left out: the rebase replays only regular commits.",
                        if plan.merges == 1 {
                            "1 merge commit is".to_owned()
                        } else {
                            format!("{} merge commits are", plan.merges)
                        }
                    ),
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
                    ui.weak("Nothing to do yet: change an action or the order of the commits.");
                }
                Ok(()) => {}
            }
            ui.weak("Local changes are stashed automatically and restored afterwards.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let can_start = result.is_ok() && !same && !busy;
                if ui
                    .add_enabled(can_start, egui::Button::new("Start rebase"))
                    .clicked()
                {
                    cmds.run(Op::InteractiveRebase {
                        base: plan.base,
                        steps: steps(&plan.rows),
                    });
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
    if !open || close {
        tab.rebase = None;
    }
}

fn rows_view(ui: &mut egui::Ui, plan: &mut RebasePlan, repo: &ferrum_core::Repo) {
    let len = plan.rows.len();
    let dragged = egui::DragAndDrop::payload::<usize>(ui.ctx()).map(|p| *p);
    let mut moved = None;
    let weak = ui.visuals().weak_text_color();
    for i in 0..len {
        let row = &mut plan.rows[i];
        let attached = row.kind.attached();
        let dropped = row.kind == Kind::Drop;
        let fill = if dragged == Some(i) {
            ui.visuals().selection.bg_fill.gamma_multiply(0.4)
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
                            .on_hover_text("Melded into the commit above");
                    }
                    let handle = ui
                        .add(
                            egui::Label::new(RichText::new("☰").color(weak))
                                .sense(egui::Sense::drag()),
                        )
                        .on_hover_text("Drag to reorder");
                    handle.dnd_set_drag_payload(i);
                    if ui
                        .add_enabled(i > 0, egui::Button::new("⬆").small())
                        .on_hover_text("Move up")
                        .clicked()
                    {
                        moved = Some((i, i - 1));
                    }
                    if ui
                        .add_enabled(i + 1 < len, egui::Button::new("⬇").small())
                        .on_hover_text("Move down")
                        .clicked()
                    {
                        moved = Some((i, i + 2));
                    }
                    let before = row.kind;
                    egui::ComboBox::from_id_salt(("rebase-action", row.commit.id))
                        .selected_text(row.kind.label())
                        .width(78.0)
                        .show_ui(ui, |ui| {
                            ui.set_min_width(380.0);
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
                        row.message = Some(
                            repo.commit_details(row.commit.id)
                                .map(|d| d.message.trim_end().to_owned())
                                .unwrap_or_else(|_| row.commit.summary.clone()),
                        );
                    }
                    let mut hash = RichText::new(row.commit.short_id()).monospace().color(weak);
                    let mut text = RichText::new(&row.commit.summary);
                    if dropped {
                        hash = hash.strikethrough();
                        text = text.strikethrough().color(weak);
                    }
                    ui.label(hash);
                    let summary = ui
                        .add(egui::Label::new(text).truncate().sense(egui::Sense::drag()))
                        .on_hover_text(format!("{}\n{}", row.commit.author, row.commit.summary));
                    summary.dnd_set_drag_payload(i);
                });
                if row.kind == Kind::Reword
                    && let Some(msg) = row.message.as_mut()
                {
                    ui.add(
                        egui::TextEdit::multiline(msg)
                            .desired_width(f32::INFINITY)
                            .desired_rows(3)
                            .hint_text("Commit message"),
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
