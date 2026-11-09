//! The multi-repository dashboard of the Repository Manager: filters
//! (changes, ahead, behind, problems), a list mode with branch, ahead/behind,
//! local changes and last fetch, and "Fetch all" / "Pull all" run in the
//! background (see `batch.rs`) with a result per repository.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Rect, RichText, Sense, vec2};
use ferrum_core::glance::Glance;

use crate::batch::{self, ItemState, Kind};
use crate::command::{Command, Commands};
use crate::icons;
use crate::repos::{Library, RepoCmd};
use crate::theme;

pub const ROW_HEIGHT: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Changes,
    Ahead,
    Behind,
    Problems,
}

impl Filter {
    const ALL: [Filter; 5] = [
        Filter::All,
        Filter::Changes,
        Filter::Ahead,
        Filter::Behind,
        Filter::Problems,
    ];

    fn label(self) -> String {
        match self {
            Filter::All => t!("All"),
            Filter::Changes => t!("Changes"),
            Filter::Ahead => t!("Ahead"),
            Filter::Behind => t!("Behind"),
            Filter::Problems => t!("Problems"),
        }
    }
}

/// Dashboard state of the Repository Manager.
#[derive(Default)]
pub struct Dash {
    pub filter: Filter,
    /// The last "Fetch all" / "Pull all", while it runs and after.
    pub batch: Option<batch::Shared>,
    /// The per-repository results are shown.
    pub show_results: bool,
}

/// What the dashboard knows about one repository.
pub struct Look<'a> {
    pub glance: Option<&'a Glance>,
    /// Reading it failed (or it no longer exists).
    pub error: Option<&'a str>,
    pub batch: Option<ItemState>,
}

impl Look<'_> {
    pub fn problem(&self) -> Option<String> {
        if let Some(e) = self.error {
            return Some(e.to_owned());
        }
        if let Some(ItemState::Failed(e)) = &self.batch {
            return Some(e.clone());
        }
        if self.glance.is_some_and(|g| g.in_progress) {
            return Some(t!("A merge, rebase or similar is in progress"));
        }
        None
    }

    pub fn keep(&self, f: Filter) -> bool {
        let ab = self.glance.and_then(|g| g.ahead_behind);
        match f {
            Filter::All => true,
            Filter::Changes => self.glance.is_some_and(|g| g.dirty),
            Filter::Ahead => ab.is_some_and(|a| a.0 > 0),
            Filter::Behind => ab.is_some_and(|a| a.1 > 0),
            Filter::Problems => self.problem().is_some(),
        }
    }
}

pub fn look<'a>(
    dash: &Dash,
    glances: &'a HashMap<PathBuf, Glance>,
    errors: &'a HashMap<PathBuf, String>,
    p: &Path,
) -> Look<'a> {
    let batch = dash
        .batch
        .as_ref()
        .and_then(|b| b.lock().ok()?.state_of(p).cloned());
    Look {
        glance: glances.get(p),
        error: errors.get(p).map(String::as_str),
        batch,
    }
}

/// Glances read by the batch are newer than the manager's.
pub fn take_glances(dash: &Dash, glances: &mut HashMap<PathBuf, Glance>) {
    let Some(b) = &dash.batch else { return };
    let Ok(mut b) = b.lock() else { return };
    for item in &mut b.items {
        if let Some(g) = item.glance.take() {
            glances.insert(item.path.clone(), g);
        }
    }
}

/// A rounded toggle with an optional count, for filters.
pub fn chip(ui: &mut egui::Ui, on: bool, label: &str, count: Option<usize>) -> egui::Response {
    let font = egui::FontId::proportional(12.5);
    let v = ui.visuals();
    let accent = theme::accent(ui);
    let text_color = if on { accent } else { v.text_color() };
    let g = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), text_color);
    let count_g = count.map(|n| {
        ui.painter()
            .layout_no_wrap(n.to_string(), theme::strong_font(10.5), v.weak_text_color())
    });
    let extra = count_g.as_ref().map_or(0.0, |c| c.size().x + 8.0);
    let size = vec2(g.size().x + 20.0 + extra, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, label));
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let (fill, stroke) = if on {
            (
                tint(accent, 34),
                egui::Stroke::new(1.0, accent.gamma_multiply(0.8)),
            )
        } else if resp.hovered() {
            (v.widgets.hovered.weak_bg_fill, v.widgets.hovered.bg_stroke)
        } else {
            (v.faint_bg_color, v.widgets.noninteractive.bg_stroke)
        };
        let p = ui.painter();
        p.rect(rect, 13.0, fill, stroke, egui::StrokeKind::Inside);
        let x = rect.left() + 10.0;
        p.galley(
            egui::pos2(x, rect.center().y - g.size().y / 2.0),
            g.clone(),
            text_color,
        );
        if let Some(c) = count_g {
            let cx = x + g.size().x + 6.0;
            p.galley(
                egui::pos2(cx, rect.center().y - c.size().y / 2.0),
                c,
                v.weak_text_color(),
            );
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Two or more icon buttons joined, one of them selected.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: &[(T, &str, String)],
) {
    let v = ui.visuals().clone();
    egui::Frame::new()
        .fill(v.faint_bg_color)
        .stroke(v.widgets.noninteractive.bg_stroke)
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (opt, icon, label) in options {
                let on = *value == *opt;
                let text = RichText::new(*icon).size(14.0).color(if on {
                    theme::accent(ui)
                } else {
                    v.weak_text_color()
                });
                let b = egui::Button::new(text)
                    .fill(if on {
                        v.widgets.active.weak_bg_fill
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(egui::Stroke::NONE)
                    .min_size(vec2(28.0, 22.0));
                let r = ui.add(b);
                r.widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::Button, true, on, label.as_str())
                });
                if r.on_hover_text(label.as_str()).clicked() {
                    *value = *opt;
                }
            }
        });
}

fn tint(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// The bar above the repositories: view mode, filters and the batch
/// actions. `counts` has one number per filter; `targets` are the
/// repositories shown (what "Fetch all" works on).
pub fn bar(
    ui: &mut egui::Ui,
    dash: &mut Dash,
    lib: &mut Library,
    counts: [usize; 5],
    targets: &[PathBuf],
    busy: bool,
    cmds: &mut Commands,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_S + 2.0;
        segmented(
            ui,
            &mut lib.list_view,
            &[
                (false, icons::SQUARES_FOUR, t!("Cards")),
                (true, icons::ROWS, t!("List")),
            ],
        );
        ui.add_space(theme::SPACE_S);
        for (f, n) in Filter::ALL.into_iter().zip(counts) {
            if f != Filter::All && f != dash.filter && n == 0 {
                continue;
            }
            let count = (f != Filter::All).then_some(n);
            if chip(ui, dash.filter == f, &f.label(), count).clicked() {
                dash.filter = if dash.filter == f { Filter::All } else { f };
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let running = dash
                .batch
                .as_ref()
                .is_some_and(|b| b.lock().is_ok_and(|b| b.running()));
            let enabled = !targets.is_empty() && !running && !busy;
            let hint = |k: Kind| {
                if busy && !running {
                    t!("Another operation is still running.")
                } else {
                    k.label(targets.len())
                }
            };
            let pull = ui
                .add_enabled(
                    enabled,
                    theme::secondary(icons::text(icons::PULL, t!("Pull all"))),
                )
                .on_hover_text(hint(Kind::Pull))
                .on_disabled_hover_text(hint(Kind::Pull));
            if pull.clicked() {
                cmds.push(Command::Repos(RepoCmd::Batch(Kind::Pull, targets.to_vec())));
            }
            let fetch = ui
                .add_enabled(
                    enabled,
                    theme::secondary(icons::text(icons::FETCH, t!("Fetch all"))),
                )
                .on_hover_text(hint(Kind::Fetch))
                .on_disabled_hover_text(hint(Kind::Fetch));
            if fetch.clicked() {
                cmds.push(Command::Repos(RepoCmd::Batch(
                    Kind::Fetch,
                    targets.to_vec(),
                )));
            }
        });
    });
}

/// Progress of the running batch, or its results once it finished.
pub fn batch_panel(ui: &mut egui::Ui, dash: &mut Dash, lib: &Library) {
    let Some(shared) = dash.batch.clone() else {
        return;
    };
    let Ok(b) = shared.lock() else { return };
    let (done, total, running) = (b.done(), b.items.len(), b.running());
    let mut close = false;
    ui.add_space(theme::SPACE_M);
    let width = ui.available_width();
    theme::card(ui)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.horizontal(|ui| {
                let (icon, color) = if running {
                    (icons::ARROWS_CLOCKWISE, theme::accent(ui))
                } else if b.failed() > 0 {
                    (icons::WARNING, ui.visuals().warn_fg_color)
                } else {
                    (icons::CHECK_CIRCLE, crate::util::added_color(ui))
                };
                if running {
                    ui.spinner();
                } else {
                    ui.label(RichText::new(icon).size(16.0).color(color));
                }
                let title = match (b.kind, running) {
                    (Kind::Fetch, true) => t!("Fetching {} of {}…", done + 1, total),
                    (Kind::Pull, true) => t!("Pulling {} of {}…", done + 1, total),
                    (Kind::Fetch, false) => t!("Fetch all finished"),
                    (Kind::Pull, false) => t!("Pull all finished"),
                };
                ui.label(theme::strong(title));
                ui.weak(b.summary());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if running {
                        let cancelling = b.cancel.as_ref().is_some_and(|c| c.is_cancelled());
                        if ui
                            .add_enabled(!cancelling, egui::Button::new(t!("Cancel")))
                            .clicked()
                            && let Some(c) = &b.cancel
                        {
                            c.cancel();
                        }
                    } else if ui
                        .add(icons::icon_button(icons::CLOSE).frame_when_inactive(false))
                        .on_hover_text(t!("Dismiss"))
                        .clicked()
                    {
                        close = true;
                    }
                    let label = if dash.show_results {
                        t!("Hide details")
                    } else {
                        t!("Details")
                    };
                    if ui.small_button(label).clicked() {
                        dash.show_results = !dash.show_results;
                    }
                });
            });
            let frac = if total == 0 {
                1.0
            } else {
                done as f32 / total as f32
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_height(4.0)
                    .fill(theme::accent(ui)),
            );
            if dash.show_results {
                ui.add_space(theme::SPACE_S);
                for item in &b.items {
                    ui.horizontal(|ui| {
                        let name = crate::ui::repo_manager::display_name(lib, &item.path);
                        let name_w = 170.0;
                        ui.allocate_ui(vec2(name_w, 18.0), |ui| {
                            ui.set_width(name_w);
                            ui.add(egui::Label::new(name).truncate());
                        });
                        let (text, color) = state_text(ui, &item.state);
                        ui.add(egui::Label::new(RichText::new(&text).color(color)).truncate())
                            .on_hover_text(text);
                    });
                }
            }
        });
    drop(b);
    if close {
        dash.batch = None;
        dash.show_results = false;
    }
}

/// A batch state as an icon and text, with its color.
pub fn state_text(ui: &egui::Ui, s: &ItemState) -> (String, Color32) {
    let v = ui.visuals();
    match s {
        ItemState::Waiting => (
            icons::text(icons::CLOCK, t!("Waiting")),
            v.weak_text_color(),
        ),
        ItemState::Running(line) if line.is_empty() => (
            icons::text(icons::ARROWS_CLOCKWISE, t!("Running…")),
            theme::accent(ui),
        ),
        ItemState::Running(line) => (
            icons::text(icons::ARROWS_CLOCKWISE, line),
            theme::accent(ui),
        ),
        ItemState::Done(t) => (icons::text(icons::CHECK, t), crate::util::added_color(ui)),
        ItemState::Skipped(t) => (icons::text(icons::MINUS, t), v.weak_text_color()),
        ItemState::Failed(t) => (icons::text(icons::WARNING, t), v.error_fg_color),
        ItemState::Cancelled => (icons::text(icons::X, t!("Cancelled")), v.weak_text_color()),
    }
}

// ----- List mode -----

struct Columns {
    name: f32,
    branch: f32,
    sync: f32,
    changes: f32,
    fetch: f32,
    commit: f32,
}

fn columns(width: f32) -> Columns {
    let fixed = 150.0 + 115.0 + 100.0 + 110.0 + 110.0;
    let name = (width - fixed - 24.0).max(140.0);
    Columns {
        name,
        branch: 150.0,
        sync: 115.0,
        changes: 100.0,
        fetch: 110.0,
        commit: 110.0,
    }
}

/// Column titles of the list.
pub fn header(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let c = columns(width);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    let p = ui.painter();
    let font = theme::strong_font(11.0);
    let color = ui.visuals().weak_text_color();
    let mut x = rect.left() + 14.0;
    for (title, w) in [
        (t!("Repository"), c.name),
        (t!("Branch"), c.branch),
        (t!("Sync"), c.sync),
        (t!("Changes"), c.changes),
        (t!("Last fetch"), c.fetch),
        (t!("Last commit"), c.commit),
    ] {
        p.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            title.to_uppercase(),
            font.clone(),
            color,
        );
        x += w;
    }
}

/// Paints one row of the list. Returns where the name is (for renaming).
#[allow(clippy::too_many_arguments)]
pub fn paint_row(
    ui: &egui::Ui,
    resp: &egui::Response,
    rect: Rect,
    color: Option<Color32>,
    name: &str,
    path_text: &str,
    look: &Look,
    selected: bool,
    is_open: bool,
    exists: bool,
) -> Rect {
    let v = ui.visuals();
    let accent = theme::accent(ui);
    let (fill, stroke) = if selected {
        (v.selection.bg_fill, egui::Stroke::new(1.0, accent))
    } else if resp.hovered() {
        (v.widgets.hovered.weak_bg_fill, egui::Stroke::NONE)
    } else {
        (Color32::TRANSPARENT, egui::Stroke::NONE)
    };
    let p = ui.painter();
    p.rect(rect, theme::RADIUS, fill, stroke, egui::StrokeKind::Inside);
    if !selected {
        p.hline(
            rect.x_range().shrink(6.0),
            rect.bottom(),
            (
                1.0,
                v.widgets.noninteractive.bg_stroke.color.gamma_multiply(0.6),
            ),
        );
    }
    if let Some(c) = color {
        let bar = Rect::from_min_size(rect.min + vec2(4.0, 9.0), vec2(3.0, rect.height() - 18.0));
        p.rect_filled(bar, 1.5, c);
    }
    let (strong, text, weak) = if selected {
        let c = v.selection.stroke.color;
        (c, c, c.gamma_multiply(0.75))
    } else {
        (v.strong_text_color(), v.text_color(), v.weak_text_color())
    };
    let c = columns(rect.width());
    let cy = rect.center().y;
    let small = egui::FontId::proportional(12.0);
    let clip =
        |x: f32, w: f32| p.with_clip_rect(Rect::from_x_y_ranges(x..=x + w - 10.0, rect.y_range()));
    let mut x = rect.left() + 14.0;

    // Name and folder, with a problem icon.
    let problem = look.problem().or_else(|| (!exists).then(|| t!("missing")));
    let name_painter = clip(x, c.name);
    let mut nx = x;
    if let Some(prob) = &problem {
        let g = name_painter.layout_no_wrap(icons::WARNING.into(), small.clone(), v.warn_fg_color);
        let r = Rect::from_min_size(egui::pos2(nx, cy - 15.0), g.size());
        name_painter.galley(r.min, g, v.warn_fg_color);
        ui.interact(r.expand(3.0), resp.id.with("problem"), Sense::hover())
            .on_hover_text(prob);
        nx += 18.0;
    }
    let ng = name_painter.layout_no_wrap(name.to_owned(), theme::strong_font(13.5), strong);
    let name_rect = Rect::from_min_size(egui::pos2(nx, cy - 17.0), vec2(c.name - 30.0, 20.0));
    let name_w = ng.size().x;
    name_painter.galley(egui::pos2(nx, cy - 8.0 - ng.size().y / 2.0), ng, strong);
    if is_open {
        let g = name_painter.layout_no_wrap(t!("open"), theme::strong_font(10.0), weak);
        let r = Rect::from_min_size(
            egui::pos2(nx + name_w + 8.0, cy - 8.0 - g.size().y / 2.0 - 1.0),
            g.size() + vec2(10.0, 2.0),
        );
        name_painter.rect_filled(r, 7.0, tint(weak, 30));
        name_painter.galley(r.min + vec2(5.0, 1.0), g, weak);
    }
    // Long paths keep their end, where the repository is.
    let path_font = egui::FontId::proportional(11.0);
    let mut chars: Vec<char> = path_text.chars().collect();
    let mut shown = path_text.to_owned();
    while chars.len() > 4
        && name_painter
            .layout_no_wrap(shown.clone(), path_font.clone(), weak)
            .size()
            .x
            > c.name - 16.0
    {
        chars.drain(..4.min(chars.len() - 4));
        shown = format!("…{}", chars.iter().collect::<String>());
    }
    let pg = name_painter.layout_no_wrap(shown, path_font, weak);
    name_painter.galley(egui::pos2(x, cy + 8.0 - pg.size().y / 2.0), pg, weak);
    x += c.name;

    let Some(g) = look.glance else {
        return name_rect;
    };
    // Branch.
    let bp = clip(x, c.branch);
    let branch = g.branch.clone().unwrap_or_else(|| t!("detached HEAD"));
    let bg = bp.layout_no_wrap(icons::text(icons::BRANCH, branch), small.clone(), text);
    bp.galley(egui::pos2(x, cy - bg.size().y / 2.0), bg, text);
    x += c.branch;

    // Ahead / behind.
    let sp = clip(x, c.sync);
    match g.ahead_behind {
        Some((0, 0)) => {
            let sg = sp.layout_no_wrap(
                icons::text(icons::CHECK, t!("in sync")),
                small.clone(),
                weak,
            );
            sp.galley(egui::pos2(x, cy - sg.size().y / 2.0), sg, weak);
        }
        Some((a, b)) => {
            let mut sx = x;
            for (icon, n, color) in [
                (icons::ARROW_UP, a, accent),
                (icons::ARROW_DOWN, b, v.warn_fg_color),
            ] {
                if n == 0 {
                    continue;
                }
                let sg = sp.layout_no_wrap(format!("{icon}{n}"), theme::strong_font(12.0), color);
                let w = sg.size().x;
                sp.galley(egui::pos2(sx, cy - sg.size().y / 2.0), sg, color);
                sx += w + 8.0;
            }
        }
        None => {
            let sg = sp.layout_no_wrap(t!("no upstream"), small.clone(), weak);
            sp.galley(egui::pos2(x, cy - sg.size().y / 2.0), sg, weak);
        }
    }
    x += c.sync;

    // Local changes.
    let cp = clip(x, c.changes);
    if g.dirty {
        cp.circle_filled(
            egui::pos2(x + 4.0, cy),
            4.0,
            crate::util::modified_color(ui),
        );
        let cg = cp.layout_no_wrap(t!("Changes"), small.clone(), text);
        cp.galley(egui::pos2(x + 13.0, cy - cg.size().y / 2.0), cg, text);
    } else {
        let cg = cp.layout_no_wrap(t!("Clean"), small.clone(), weak);
        cp.galley(egui::pos2(x, cy - cg.size().y / 2.0), cg, weak);
    }
    x += c.changes;

    // Last fetch and last commit (or the running batch).
    let now = crate::util::now_secs();
    let when =
        |t: Option<i64>| t.map_or_else(|| t!("never"), |t| crate::util::relative_time(t, now));
    match &look.batch {
        Some(s @ (ItemState::Running(_) | ItemState::Waiting)) => {
            let fp = clip(x, c.fetch + c.commit);
            let (t, color) = state_text(ui, s);
            let fg = fp.layout_no_wrap(t, small.clone(), color);
            fp.galley(egui::pos2(x, cy - fg.size().y / 2.0), fg, color);
        }
        _ => {
            let fp = clip(x, c.fetch);
            let fg = fp.layout_no_wrap(when(g.last_fetch), small.clone(), text);
            fp.galley(egui::pos2(x, cy - fg.size().y / 2.0), fg, text);
            x += c.fetch;
            let lp = clip(x, c.commit);
            let lg = lp.layout_no_wrap(when(g.last_commit), small, text);
            lp.galley(egui::pos2(x, cy - lg.size().y / 2.0), lg, text);
        }
    }
    name_rect
}

/// Rows hidden by the filter, as a hint under the list.
pub fn hidden_hint(ui: &mut egui::Ui, dash: &mut Dash, hidden: usize) {
    if hidden == 0 || dash.filter == Filter::All {
        return;
    }
    ui.horizontal(|ui| {
        ui.weak(t!("{} repositories hidden by the filter.", hidden));
        if ui.link(t!("Show all")).clicked() {
            dash.filter = Filter::All;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_follow_the_glance() {
        let g = Glance {
            dirty: true,
            ahead_behind: Some((2, 0)),
            ..Default::default()
        };
        let l = Look {
            glance: Some(&g),
            error: None,
            batch: None,
        };
        assert!(l.keep(Filter::All) && l.keep(Filter::Changes) && l.keep(Filter::Ahead));
        assert!(!l.keep(Filter::Behind) && !l.keep(Filter::Problems));
        let l = Look {
            glance: None,
            error: None,
            batch: Some(ItemState::Failed("fatal: x".into())),
        };
        assert!(l.keep(Filter::Problems));
        assert_eq!(l.problem().as_deref(), Some("fatal: x"));
    }
}
