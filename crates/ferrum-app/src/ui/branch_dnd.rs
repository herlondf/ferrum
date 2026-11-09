//! Drag and drop of branches, like Fork: drag a branch (a sidebar row or a
//! label in the graph) onto another branch to merge, rebase or cherry-pick,
//! or onto a remote to push. Nothing runs on drop: a small menu asks what to
//! do, and the usual dialogs confirm.
//!
//! - `source(resp, branch)`: makes a row draggable.
//! - `target(ui, resp, target)`: highlights a row while a branch that can be
//!   dropped there hovers it; returns the menu to open on release.
//! - `graph_row(...)`: the same for the labels of a history row.
//! - `overlays(ctx, tab, menu, cmds)`: the dragged branch next to the
//!   pointer and the drop menu (called once a frame, by the sidebar).

use eframe::egui::{self, Color32, Rect};
use ferrum_core::RefKind;

use crate::command::{Command, Commands, Op};
use crate::flow::FlowOp;
use crate::icons;
use crate::tab::RepoTab;
use crate::theme;
use crate::ui::dialogs::Dialog;
pub use crate::ui::sidebar_model::{DragBranch, DropMenu};

/// Commits a cherry-pick by drag and drop copies at most.
const MAX_PICK: usize = 50;

/// What a branch was dropped on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Branch(DragBranch),
    /// A remote (its name), to push to.
    Remote(String),
}

impl Target {
    fn name(&self) -> &str {
        match self {
            Target::Branch(b) => &b.name,
            Target::Remote(r) => r,
        }
    }
}

/// An entry of the drop menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Merge the dragged branch into the target.
    Merge,
    /// Rebase the dragged branch onto the target.
    Rebase,
    /// Copy the commits of the dragged branch that the target lacks.
    CherryPick,
    /// Push the dragged branch to a remote (and a branch there).
    Push {
        remote: String,
        branch: Option<String>,
    },
}

/// What dropping `src` on `target` can do. Empty: not a drop target.
pub fn choices(src: &DragBranch, target: &Target) -> Vec<Choice> {
    let mut out = Vec::new();
    match target {
        Target::Branch(dst) => {
            if dst.name == src.name {
                return out;
            }
            // Merging and picking need a local target, rebasing a local source.
            if !dst.remote {
                out.push(Choice::Merge);
            }
            if !src.remote {
                out.push(Choice::Rebase);
            }
            if !dst.remote {
                out.push(Choice::CherryPick);
            }
            if !src.remote
                && dst.remote
                && let Some((remote, branch)) = dst.name.split_once('/')
            {
                out.push(Choice::Push {
                    remote: remote.to_owned(),
                    branch: Some(branch.to_owned()),
                });
            }
        }
        Target::Remote(remote) => {
            if !src.remote {
                out.push(Choice::Push {
                    remote: remote.clone(),
                    branch: None,
                });
            }
        }
    }
    out
}

fn choice_text(src: &str, dst: &str, c: &Choice) -> (&'static str, String) {
    match c {
        Choice::Merge => (icons::GIT_MERGE, t!("Merge {} into {}…", src, dst)),
        Choice::Rebase => (
            icons::ARROW_ELBOW_RIGHT_UP,
            t!("Rebase {} onto {}…", src, dst),
        ),
        Choice::CherryPick => (icons::CHERRIES, t!("Cherry-pick {} onto {}…", src, dst)),
        Choice::Push { remote, branch } => {
            let to = match branch {
                Some(b) => format!("{remote}/{b}"),
                None => remote.clone(),
            };
            (icons::PUSH, t!("Push {} to {}…", src, to))
        }
    }
}

/// The dialog that confirms a choice (nothing runs without it).
pub fn command(tab: &RepoTab, src: &DragBranch, target: &Target, choice: &Choice) -> Command {
    let (s, d) = (src.name.as_str(), target.name());
    let head = tab.head.as_deref();
    match choice {
        Choice::Merge if head == Some(d) => Command::Dialog(Dialog::merge(s, s)),
        Choice::Merge => {
            let ops = vec![
                Op::Checkout(d.to_owned()),
                Op::Merge {
                    rev: s.to_owned(),
                    mode: ferrum_core::ops::MergeMode::Default,
                },
            ];
            Command::Dialog(Dialog::confirm(
                &t!("Merge"),
                &t!("Check out {} and merge {} into it?", d, s),
                Op::Sequence(t!("Merge {} into {}", s, d), ops),
                false,
            ))
        }
        Choice::Rebase if head == Some(s) => {
            Command::Dialog(crate::ui::flow_dialogs::rebase(tab, d))
        }
        Choice::Rebase => {
            #[rustfmt::skip]
            let text = t!("Rebase {} onto {}? The commits of {} will be rewritten.", s, d, s);
            let ops = vec![
                Op::Checkout(s.to_owned()),
                Op::Rebase { onto: d.to_owned() },
            ];
            Command::Dialog(Dialog::confirm(
                &t!("Rebase"),
                &text,
                Op::Sequence(t!("Rebase {} onto {}", s, d), ops),
                false,
            ))
        }
        Choice::CherryPick => {
            let ids = match ferrum_core::glance::commits_not_in(&tab.repo, s, d, MAX_PICK) {
                Ok(ids) if ids.is_empty() => {
                    return Command::Error(t!("{} has no commits that {} lacks.", s, d));
                }
                Ok(ids) => ids,
                Err(_) => {
                    return Command::Error(t!(
                        "{} has more than {} commits that {} lacks: merge or rebase instead.",
                        s,
                        MAX_PICK,
                        d
                    ));
                }
            };
            if head == Some(d) {
                return Command::Dialog(crate::ui::flow_dialogs::cherry_pick(tab, &ids));
            }
            let n = ids.len();
            let pick = Op::Flow(FlowOp::CherryPick {
                ids,
                opts: Default::default(),
            });
            Command::Dialog(Dialog::confirm(
                &t!("Cherry-pick"),
                &t!("Check out {} and copy {} commits of {} onto it?", d, n, s),
                Op::Sequence(
                    t!("Cherry-pick {} onto {}", s, d),
                    vec![Op::Checkout(d.to_owned()), pick],
                ),
                false,
            ))
        }
        Choice::Push { remote, branch } => Command::Dialog(Dialog::Sync(
            crate::ui::sync_dialogs::push_to(tab, s, remote, branch.as_deref()),
        )),
    }
}

// ----- Sources and targets -----

/// Makes `resp` (it must sense drags) start dragging `branch`.
pub fn source(resp: &egui::Response, branch: DragBranch) {
    resp.dnd_set_drag_payload(branch);
}

/// Highlights `resp` while a branch that can be dropped on `target` hovers
/// it, and returns the drop menu when it is released there.
pub fn target(ui: &egui::Ui, resp: &egui::Response, target: Target) -> Option<DropMenu> {
    let src = resp.dnd_hover_payload::<DragBranch>()?;
    if choices(&src, &target).is_empty() {
        return None;
    }
    highlight(ui, resp.rect);
    let src = resp.dnd_release_payload::<DragBranch>()?;
    let pos = ui
        .ctx()
        .pointer_interact_pos()
        .unwrap_or(resp.rect.left_bottom());
    Some(DropMenu::new((*src).clone(), target, pos))
}

/// The look of a row that accepts the dragged branch.
pub fn highlight(ui: &egui::Ui, rect: Rect) {
    let accent = theme::accent(ui);
    let fill = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 40);
    ui.painter().rect_filled(rect, theme::RADIUS, fill);
    ui.painter().rect_stroke(
        rect,
        theme::RADIUS,
        egui::Stroke::new(1.5, accent),
        egui::StrokeKind::Inside,
    );
}

/// Branch labels of a history row as drag sources and drop targets.
/// `labels` are their rects, kinds and full names. Returns true when a drag
/// started on a label (the row must not start its own drag then).
pub fn graph_row(
    ui: &egui::Ui,
    tab: &mut RepoTab,
    resp: &egui::Response,
    labels: &[(Rect, RefKind, &str)],
) -> bool {
    let branch = |kind: RefKind, name: &str| {
        (kind != RefKind::Tag).then(|| DragBranch {
            name: name.to_owned(),
            remote: kind == RefKind::Remote,
        })
    };
    let at = |p: Option<egui::Pos2>| {
        p.and_then(|p| labels.iter().find(|(r, _, _)| r.contains(p)))
            .and_then(|(r, k, n)| Some((*r, branch(*k, n)?)))
    };
    let mut started = false;
    if resp.drag_started()
        && let Some((_, b)) = at(ui.input(|i| i.pointer.press_origin()))
    {
        egui::DragAndDrop::set_payload(ui.ctx(), b);
        started = true;
    }
    if !resp.contains_pointer() {
        return started;
    }
    let Some(src) = egui::DragAndDrop::payload::<DragBranch>(ui.ctx()) else {
        return started;
    };
    let Some((rect, dst)) = at(ui.ctx().pointer_hover_pos()) else {
        return started;
    };
    let target = Target::Branch(dst);
    if choices(&src, &target).is_empty() {
        return started;
    }
    highlight(ui, rect.expand(1.0));
    if ui.input(|i| i.pointer.any_released())
        && let Some(src) = egui::DragAndDrop::take_payload::<DragBranch>(ui.ctx())
    {
        let pos = ui
            .ctx()
            .pointer_interact_pos()
            .unwrap_or(rect.left_bottom());
        tab.sidebar.drop_menu = Some(DropMenu::new((*src).clone(), target, pos));
    }
    started
}

// ----- Overlays -----

/// The dragged branch next to the pointer, and the drop menu.
pub fn overlays(
    ctx: &egui::Context,
    tab: &RepoTab,
    menu: &mut Option<DropMenu>,
    cmds: &mut Commands,
) {
    ghost(ctx, tab);
    if let Some(m) = menu
        && drop_menu(ctx, tab, m, cmds)
    {
        *menu = None;
    }
}

/// A pill with the dragged branch next to the pointer.
fn ghost(ctx: &egui::Context, tab: &RepoTab) {
    let Some(p) = egui::DragAndDrop::payload::<DragBranch>(ctx) else {
        return;
    };
    let Some(pos) = ctx.pointer_interact_pos() else {
        return;
    };
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let icon = if p.remote {
        icons::REMOTE
    } else {
        icons::BRANCH
    };
    let local_head = !p.remote && tab.head.as_deref() == Some(p.name.as_str());
    egui::Area::new(egui::Id::unique("branch-drag-ghost"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + egui::vec2(16.0, 10.0))
        .interactable(false)
        .show(ctx, |ui| {
            pill(ui, icon, &p.name, local_head);
        });
}

/// The dragged item: an accent pill with an icon and a name.
pub fn pill(ui: &mut egui::Ui, icon: &str, text: &str, strong: bool) {
    let accent = theme::accent(ui);
    let frame = egui::Frame::new()
        .fill(accent)
        .corner_radius(theme::RADIUS_LARGE)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .shadow(ui.visuals().popup_shadow);
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE_S + 2.0;
            ui.label(egui::RichText::new(icon).color(theme::on_accent()));
            let text = egui::RichText::new(text).color(theme::on_accent());
            ui.label(if strong {
                text.font(theme::strong_font(13.0))
            } else {
                text
            });
        });
    });
}

/// Draws the drop menu. Returns true when it should close.
fn drop_menu(ctx: &egui::Context, tab: &RepoTab, m: &mut DropMenu, cmds: &mut Commands) -> bool {
    let list = choices(&m.source, &m.target);
    let mut close = list.is_empty();
    let src = m.source.name.clone();
    let dst = m.target.name().to_owned();
    let area = egui::Area::new(egui::Id::unique("branch-drop-menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(m.pos)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(240.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme::SPACE_S;
                    let weak = ui.visuals().weak_text_color();
                    ui.label(egui::RichText::new(icons::BRANCH).color(weak));
                    ui.label(theme::strong(&src));
                    ui.label(egui::RichText::new(icons::ARROW_RIGHT).color(weak));
                    let icon = match m.target {
                        Target::Remote(_) => icons::REMOTE,
                        Target::Branch(_) => icons::BRANCH,
                    };
                    ui.label(egui::RichText::new(icon).color(weak));
                    ui.label(theme::strong(&dst));
                });
                ui.separator();
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    ui.add_enabled_ui(!tab.busy(), |ui| {
                        for c in &list {
                            let (icon, text) = choice_text(&src, &dst, c);
                            let button = egui::Button::new(icons::text(icon, text))
                                .frame_when_inactive(false)
                                .min_size(egui::vec2(0.0, theme::row_height()));
                            if ui.add(button).clicked() {
                                cmds.push(command(tab, &m.source, &m.target, c));
                                close = true;
                            }
                        }
                    });
                    if tab.busy() {
                        ui.weak(t!("Wait for the current operation to finish."));
                    }
                    ui.separator();
                    let cancel = egui::Button::new(icons::text(icons::CLOSE, t!("Cancel")))
                        .frame_when_inactive(false);
                    if ui.add(cancel).clicked() {
                        close = true;
                    }
                });
            });
        });
    let (pressed, esc) = ctx.input(|i| (i.pointer.any_pressed(), i.key_pressed(egui::Key::Escape)));
    if esc || (pressed && !m.just_opened && !area.response.contains_pointer()) {
        close = true;
    }
    m.just_opened = false;
    close
}

/// The drop menu for `SRC>DST` (a `FERRUM_START` step); a DST without a
/// matching branch is taken as a remote.
pub fn dev_menu(tab: &RepoTab, arg: &str) -> Option<DropMenu> {
    let (src, dst) = arg.split_once('>')?;
    let branch = |name: &str| {
        tab.branches
            .iter()
            .find(|b| b.name == name)
            .map(|b| DragBranch {
                name: b.name.clone(),
                remote: b.kind == ferrum_core::BranchKind::Remote,
            })
    };
    let target = match branch(dst) {
        Some(b) => Target::Branch(b),
        None => Target::Remote(dst.to_owned()),
    };
    Some(DropMenu::new(
        branch(src)?,
        target,
        egui::pos2(120.0, 300.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, remote: bool) -> DragBranch {
        DragBranch {
            name: name.into(),
            remote,
        }
    }

    #[test]
    fn drop_choices_follow_the_branch_kinds() {
        let local = |n| Target::Branch(b(n, false));
        assert_eq!(
            choices(&b("topic", false), &local("main")),
            [Choice::Merge, Choice::Rebase, Choice::CherryPick]
        );
        assert!(choices(&b("main", false), &local("main")).is_empty());
        // A remote branch can be merged or picked into a local one, not rebased.
        assert_eq!(
            choices(&b("origin/x", true), &local("main")),
            [Choice::Merge, Choice::CherryPick]
        );
        // A local branch on a remote branch: rebase onto it or push to it.
        assert_eq!(
            choices(&b("topic", false), &Target::Branch(b("origin/dev", true))),
            [
                Choice::Rebase,
                Choice::Push {
                    remote: "origin".into(),
                    branch: Some("dev".into())
                }
            ]
        );
        assert!(choices(&b("origin/x", true), &Target::Branch(b("origin/y", true))).is_empty());
        // On a remote: push.
        assert_eq!(
            choices(&b("topic", false), &Target::Remote("upstream".into())),
            [Choice::Push {
                remote: "upstream".into(),
                branch: None
            }]
        );
        assert!(choices(&b("origin/x", true), &Target::Remote("origin".into())).is_empty());
    }

    /// A history row per branch, its label at a known place.
    struct Rows {
        tab: RepoTab,
        labels: Vec<Rect>,
        started: bool,
    }

    #[test]
    fn graph_labels_drag_onto_each_other() {
        use egui_kittest::Harness;
        let dir = crate::ui_tests::repo();
        crate::ui_tests::git(dir.path(), &["branch", "topic"]);
        let tab = RepoTab::open_with(dir.path().into(), &Default::default()).unwrap();
        let state = Rows {
            tab,
            labels: Vec::new(),
            started: false,
        };
        let mut h = Harness::new_ui_state(
            |ui, st: &mut Rows| {
                st.labels.clear();
                for name in ["topic", "main"] {
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(300.0, 26.0),
                        egui::Sense::click_and_drag(),
                    );
                    let label = Rect::from_min_size(
                        rect.min + egui::vec2(100.0, 3.0),
                        egui::vec2(60.0, 20.0),
                    );
                    st.started |=
                        graph_row(ui, &mut st.tab, &resp, &[(label, RefKind::Local, name)]);
                    st.labels.push(label);
                }
            },
            state,
        );
        h.run();
        let (from, to) = (h.state().labels[0].center(), h.state().labels[1].center());
        h.hover_at(from);
        h.step();
        h.drag_at(from);
        h.step();
        for i in 1..=5 {
            h.hover_at(from + (to - from) * (i as f32 / 5.0));
            h.step();
        }
        assert!(h.state().started);
        h.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
        let menu = h.state().tab.sidebar.drop_menu.clone().expect("drop menu");
        assert_eq!(menu.source, b("topic", false));
        assert_eq!(menu.target, Target::Branch(b("main", false)));
    }
}
