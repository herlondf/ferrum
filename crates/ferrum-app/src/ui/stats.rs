//! Repository statistics window: commits per weekday and hour, per author,
//! and a treemap of where the changes happened.

use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, pos2, vec2};
use ferrum_core::stats::Stats;

use crate::tab::RepoTab;
use crate::ui::history::lane_color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    #[default]
    All,
    Year,
    Quarter,
    Month,
}

impl Period {
    const ALL: [Period; 4] = [Period::All, Period::Year, Period::Quarter, Period::Month];

    fn label(self) -> String {
        match self {
            Period::All => t!("All time"),
            Period::Year => t!("Last 12 months"),
            Period::Quarter => t!("Last 3 months"),
            Period::Month => t!("Last 30 days"),
        }
    }

    fn days(self) -> Option<i64> {
        match self {
            Period::All => None,
            Period::Year => Some(365),
            Period::Quarter => Some(91),
            Period::Month => Some(30),
        }
    }
}

#[derive(Default)]
pub struct StatsState {
    pub open: bool,
    period: Period,
    all_branches: bool,
    result: Option<Result<Stats, String>>,
    rx: Option<Receiver<Result<Stats, String>>>,
    /// Options the shown result was computed for.
    loaded: Option<(Period, bool)>,
}

pub fn open(tab: &mut RepoTab) {
    tab.history.ext.stats.open = true;
}

fn start(tab: &mut RepoTab, ctx: &egui::Context) {
    let s = &mut tab.history.ext.stats;
    let since = s.period.days().map(|d| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        now - d * 86_400
    });
    let all = s.all_branches;
    let git = tab.git.clone();
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let r = ferrum_core::stats::collect(&git, since, all).map_err(|e| e.to_string());
        let _ = tx.send(r);
        ctx.request_repaint();
    });
    s.rx = Some(rx);
    s.loaded = Some((s.period, s.all_branches));
}

pub fn window(ctx: &egui::Context, tab: &mut RepoTab) {
    if !tab.history.ext.stats.open {
        return;
    }
    let s = &mut tab.history.ext.stats;
    if let Some(rx) = &s.rx
        && let Ok(r) = rx.try_recv()
    {
        s.result = Some(r);
        s.rx = None;
    }
    if s.rx.is_none() && s.loaded != Some((s.period, s.all_branches)) {
        start(tab, ctx);
    }
    let name = tab.name.clone();
    let s = &mut tab.history.ext.stats;
    let mut open = true;
    egui::Window::new(t!("Statistics of {}", name))
        .open(&mut open)
        .default_size([760.0, 640.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("stats-period")
                    .selected_text(s.period.label())
                    .show_ui(ui, |ui| {
                        for p in Period::ALL {
                            ui.selectable_value(&mut s.period, p, p.label());
                        }
                    });
                ui.checkbox(&mut s.all_branches, t!("All branches"));
                if s.rx.is_some() {
                    ui.spinner();
                }
            });
            ui.separator();
            match &s.result {
                None => {
                    ui.weak(t!("Counting commits…"));
                }
                Some(Err(e)) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                Some(Ok(stats)) => {
                    egui::ScrollArea::vertical().show(ui, |ui| show_stats(ui, stats));
                }
            }
        });
    if !open {
        s.open = false;
    }
}

fn show_stats(ui: &mut egui::Ui, s: &Stats) {
    ui.strong(t!("{} commits by {} authors", s.commits, s.authors.len()));
    if s.commits == 0 {
        return;
    }
    ui.add_space(8.0);
    ui.columns(2, |cols| {
        cols[0].label(t!("Commits per weekday"));
        let days = [
            t!("Mon"),
            t!("Tue"),
            t!("Wed"),
            t!("Thu"),
            t!("Fri"),
            t!("Sat"),
            t!("Sun"),
        ];
        bars(&mut cols[0], &s.weekday, |i| days[i].clone());
        cols[1].label(t!("Commits per hour"));
        bars(&mut cols[1], &s.hour, |i| {
            if i % 3 == 0 {
                format!("{i}")
            } else {
                String::new()
            }
        });
    });
    ui.add_space(12.0);
    ui.label(t!("Authors"));
    let max = s.authors.first().map_or(1, |a| a.commits).max(1);
    egui::Grid::new("stats-authors")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            for a in s.authors.iter().take(20) {
                ui.label(&a.name).on_hover_text(&a.email);
                ui.label(a.commits.to_string());
                ui.weak(format!("+{} -{}", a.added, a.removed));
                let (rect, _) = ui.allocate_exact_size(vec2(200.0, 12.0), Sense::hover());
                let w = rect.width() * a.commits as f32 / max as f32;
                ui.painter().rect_filled(
                    Rect::from_min_size(rect.min, vec2(w.max(1.0), rect.height())),
                    2.0,
                    lane_color(0),
                );
                ui.end_row();
            }
        });
    if s.authors.len() > 20 {
        ui.weak(t!("and {} more", s.authors.len() - 20));
    }
    ui.add_space(12.0);
    ui.label(t!("Changed lines per top-level folder"));
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 260.0), Sense::hover());
    let shown: Vec<&(String, usize)> = s.folders.iter().take(40).collect();
    let values: Vec<f32> = shown.iter().map(|(_, n)| *n as f32).collect();
    let painter = ui.painter_at(rect);
    let pointer = resp.hover_pos();
    for (i, (r, (folder, n))) in treemap(&values, rect).into_iter().zip(&shown).enumerate() {
        let r = r.shrink(1.0);
        painter.rect_filled(r, 2.0, lane_color(i).gamma_multiply(0.85));
        let name = if folder.is_empty() {
            t!("(root files)")
        } else {
            folder.clone()
        };
        if r.width() > 40.0 && r.height() > 16.0 {
            painter.with_clip_rect(r).text(
                r.min + vec2(4.0, 3.0),
                Align2::LEFT_TOP,
                &name,
                FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
        if pointer.is_some_and(|p| r.contains(p)) {
            resp.clone()
                .on_hover_text(t!("{}: {} changed lines", name, n));
        }
    }
}

/// A small bar chart, one bar per value, with a label under each.
fn bars(ui: &mut egui::Ui, values: &[usize], label: impl Fn(usize) -> String) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let max = values.iter().copied().max().unwrap_or(0).max(1) as f32;
    let slot = rect.width() / values.len() as f32;
    let chart_h = rect.height() - 16.0;
    let weak = ui.visuals().weak_text_color();
    for (i, &v) in values.iter().enumerate() {
        let h = chart_h * v as f32 / max;
        let x = rect.left() + slot * i as f32;
        let bar = Rect::from_min_max(
            pos2(x + slot * 0.15, rect.top() + chart_h - h),
            pos2(x + slot * 0.85, rect.top() + chart_h),
        );
        painter.rect_filled(bar, 2.0, lane_color(0));
        painter.text(
            pos2(x + slot / 2.0, rect.bottom()),
            Align2::CENTER_BOTTOM,
            label(i),
            FontId::proportional(11.0),
            weak,
        );
        if resp.hover_pos().is_some_and(|p| p.x >= x && p.x < x + slot) {
            painter.rect_stroke(
                bar.expand(1.0),
                2.0,
                (1.0, ui.visuals().text_color()),
                egui::StrokeKind::Outside,
            );
            resp.clone().on_hover_text(v.to_string());
        }
    }
}

/// Squarified treemap: splits `rect` into one rectangle per value, in order,
/// with areas proportional to the values.
pub fn treemap(values: &[f32], rect: Rect) -> Vec<Rect> {
    let total: f32 = values.iter().sum();
    if total <= 0.0 || values.is_empty() {
        return Vec::new();
    }
    let scale = rect.area() / total;
    let areas: Vec<f32> = values.iter().map(|v| v * scale).collect();
    let worst = |row: &[f32], side: f32| {
        let sum: f32 = row.iter().sum();
        let max = row.iter().copied().fold(0.0, f32::max);
        let min = row.iter().copied().fold(f32::INFINITY, f32::min);
        (side * side * max / (sum * sum)).max(sum * sum / (side * side * min))
    };
    let mut out = Vec::with_capacity(values.len());
    let mut rest = rect;
    let mut i = 0;
    while i < areas.len() {
        let side = rest.width().min(rest.height()).max(1.0);
        let mut end = i + 1;
        while end < areas.len() && worst(&areas[i..=end], side) <= worst(&areas[i..end], side) {
            end += 1;
        }
        let row = &areas[i..end];
        let sum: f32 = row.iter().sum();
        if rest.width() >= rest.height() {
            let w = sum / rest.height().max(1.0);
            let mut y = rest.top();
            for a in row {
                let h = a / w.max(f32::EPSILON);
                out.push(Rect::from_min_size(pos2(rest.left(), y), vec2(w, h)));
                y += h;
            }
            rest.min.x += w;
        } else {
            let h = sum / rest.width().max(1.0);
            let mut x = rest.left();
            for a in row {
                let w = a / h.max(f32::EPSILON);
                out.push(Rect::from_min_size(pos2(x, rest.top()), vec2(w, h)));
                x += w;
            }
            rest.min.y += h;
        }
        i = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treemap_fills_the_area() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 200.0));
        let rects = treemap(&[50.0, 25.0, 15.0, 10.0], rect);
        assert_eq!(rects.len(), 4);
        let area: f32 = rects.iter().map(|r| r.area()).sum();
        assert!((area - rect.area()).abs() < 1.0);
        assert!((rects[0].area() - rect.area() / 2.0).abs() < 1.0);
        for r in &rects {
            assert!(rect.expand(0.5).contains_rect(*r));
        }
        assert!(treemap(&[], rect).is_empty());
    }
}
