//! Which pane gets the keyboard: sidebar, commit list or commit details.
//! Tab and Shift+Tab cycle between them (when no text field is focused);
//! clicking in a pane focuses it.

use eframe::egui::{self, Key, Modifiers};

use crate::tab::{RepoTab, View};
use crate::ui::sidebar_model::NavKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    Sidebar,
    /// The commit list (or the main view outside the history).
    #[default]
    Commits,
    Details,
}

#[derive(Default)]
pub struct PaneFocus {
    pub pane: Pane,
    /// Draw a ring around the focused pane (after Tab, until the next click).
    pub ring: bool,
    pub sidebar_rect: Option<egui::Rect>,
    pub details_rect: Option<egui::Rect>,
}

/// A text field has the keyboard focus.
fn typing(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.focused())
        .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some())
}

/// Handles Tab, clicks and the arrow keys of the sidebar and details. Call
/// before the panes are drawn.
pub fn handle(ctx: &egui::Context, tab: &mut RepoTab) {
    let history = tab.view == View::History;
    let (press, pos) = ctx.input(|i| (i.pointer.any_pressed(), i.pointer.interact_pos()));
    if press && let Some(p) = pos {
        let f = &mut tab.pane;
        f.ring = false;
        f.pane = if f.sidebar_rect.is_some_and(|r| r.contains(p)) {
            Pane::Sidebar
        } else if history && f.details_rect.is_some_and(|r| r.contains(p)) {
            Pane::Details
        } else {
            Pane::Commits
        };
    }
    if egui::Popup::is_any_open(ctx) || typing(ctx) {
        tab.sidebar.has_focus = false;
        return;
    }
    // Shift+Tab first: a plain Tab check also matches it.
    let (prev, next) = ctx.input_mut(|i| {
        let prev = i.consume_key(Modifiers::SHIFT, Key::Tab);
        (prev, !prev && i.consume_key(Modifiers::NONE, Key::Tab))
    });
    if next || prev {
        // Keep egui from moving the focus to a button as well.
        ctx.memory_mut(|m| {
            m.move_focus(egui::FocusDirection::None);
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        let order: &[Pane] = if history {
            &[Pane::Sidebar, Pane::Commits, Pane::Details]
        } else {
            &[Pane::Sidebar, Pane::Commits]
        };
        let i = order.iter().position(|p| *p == tab.pane.pane).unwrap_or(1);
        let n = order.len();
        tab.pane.pane = order[if next { (i + 1) % n } else { (i + n - 1) % n }];
        tab.pane.ring = true;
    }
    if tab.pane.pane == Pane::Details && !history {
        tab.pane.pane = Pane::Commits;
    }
    tab.sidebar.has_focus = tab.pane.pane == Pane::Sidebar;
    match tab.pane.pane {
        Pane::Sidebar => sidebar_keys(ctx, tab),
        Pane::Details => details_keys(ctx, tab),
        Pane::Commits => {}
    }
}

fn sidebar_keys(ctx: &egui::Context, tab: &mut RepoTab) {
    let keys = [
        (Key::ArrowUp, NavKey::Up),
        (Key::ArrowDown, NavKey::Down),
        (Key::ArrowLeft, NavKey::Left),
        (Key::ArrowRight, NavKey::Right),
        (Key::Home, NavKey::Home),
        (Key::End, NavKey::End),
        (Key::Enter, NavKey::Enter),
        (Key::Space, NavKey::Space),
    ];
    for (k, nav) in keys {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k)) {
            tab.sidebar.navigate(nav);
        }
    }
}

/// Up/Down pick the previous / next changed file of the shown commit.
fn details_keys(ctx: &egui::Context, tab: &mut RepoTab) {
    let (up, down) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
        )
    });
    let Some(view) = &tab.commit_view else {
        return;
    };
    if view.files.is_empty() || !(up || down) {
        return;
    }
    let cur = view.selected_file.unwrap_or(0);
    let next = if up {
        cur.saturating_sub(1)
    } else {
        (cur + 1).min(view.files.len() - 1)
    };
    if Some(next) != view.selected_file {
        tab.select_commit_file(next);
    }
}

/// Paints the focus ring after the panes are drawn.
pub fn paint(ctx: &egui::Context, tab: &RepoTab) {
    let f = &tab.pane;
    if !f.ring {
        return;
    }
    let screen = ctx.content_rect();
    let rect = match f.pane {
        Pane::Sidebar => f.sidebar_rect,
        Pane::Details => f.details_rect,
        Pane::Commits => f.sidebar_rect.map(|s| {
            let bottom = f
                .details_rect
                .filter(|_| tab.view == View::History)
                .map_or(s.bottom(), |d| d.top());
            egui::Rect::from_min_max(
                egui::pos2(s.right(), s.top()),
                egui::pos2(screen.right(), bottom),
            )
        }),
    };
    if let Some(r) = rect {
        let color = ctx.global_style().visuals.selection.stroke.color;
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("pane-focus"),
        ))
        .rect_stroke(
            r.shrink(1.0),
            2.0,
            egui::Stroke::new(1.5, color),
            egui::StrokeKind::Inside,
        );
    }
}
