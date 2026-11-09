//! The Insights view (sidebar > Insights): hotspots and churn, who knows
//! which file (bus factor), contributors over time and CODEOWNERS, with
//! filters by period and path. Its other pages are the cleanup assistant
//! (`cleanup.rs`) and sparse checkout (`sparse.rs`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, Color32, Rect, RichText, Sense, pos2, vec2};
use ferrum_core::codeowners::CodeOwners;
use ferrum_core::insights::{self, Insights, TIMELINE_AUTHORS};

use crate::command::{Command, Commands};
use crate::icons;
use crate::tab::{RepoTab, View};
use crate::theme;
use crate::ui::dashboard::chip;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Insights,
    Cleanup,
    Sparse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    Month,
    Quarter,
    #[default]
    Year,
    All,
}

impl Period {
    const ALL: [Period; 4] = [Period::Month, Period::Quarter, Period::Year, Period::All];

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

type Loaded = Result<Insights, String>;

#[derive(Default)]
pub struct InsightsState {
    pub page: Page,
    period: Period,
    /// Path filter being typed, and the one applied.
    path: String,
    applied_path: String,
    result: Option<Loaded>,
    loading: Option<(Receiver<Loaded>, Arc<AtomicBool>)>,
    /// Period, path and HEAD of the shown result.
    loaded_for: Option<(Period, String, Option<ferrum_core::Oid>)>,
    owners: Option<Option<CodeOwners>>,
    owner_query: String,
    pub cleanup: crate::ui::cleanup::CleanupState,
    pub sparse: crate::ui::sparse::SparseUi,
}

impl InsightsState {
    #[cfg(test)]
    pub fn result(&self) -> Option<&Insights> {
        self.result.as_ref()?.as_ref().ok()
    }
}

/// Shows the view at a page (`cleanup`, `sparse`, else the insights).
pub fn open(tab: &mut RepoTab, page: &str) {
    tab.insights.page = match page {
        "cleanup" => Page::Cleanup,
        "rewrite" => {
            tab.insights.cleanup.demo_rewrite = true;
            Page::Cleanup
        }
        "sparse" => Page::Sparse,
        _ => Page::Insights,
    };
    tab.view = View::Insights;
}

fn load(tab: &mut RepoTab, ctx: &egui::Context) {
    let st = &mut tab.insights;
    if let Some((rx, _)) = &st.loading
        && let Ok(r) = rx.try_recv()
    {
        st.result = Some(r);
        st.loading = None;
    }
    let want = (st.period, st.applied_path.clone(), tab.head_oid);
    if st.loaded_for.as_ref() == Some(&want) {
        return;
    }
    // Stop a load for other options.
    if let Some((_, cancel)) = st.loading.take() {
        cancel.store(true, Ordering::Relaxed);
    }
    st.loaded_for = Some(want);
    let now = crate::util::now_secs();
    let opts = insights::Options {
        since: st.period.days().map(|d| now - d * 86_400),
        path: st.applied_path.clone(),
        now,
        ..Default::default()
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let (path, ctx, stop) = (tab.repo.path().to_path_buf(), ctx.clone(), cancel.clone());
    std::thread::spawn(move || {
        let r = insights::compute(&path, &opts, &stop).map_err(|e| e.to_string());
        if !stop.load(Ordering::Relaxed) {
            let _ = tx.send(r);
            ctx.request_repaint();
        }
    });
    st.loading = Some((rx, cancel));
    st.owners = Some(CodeOwners::load(tab.repo.path()));
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    header(ui, tab);
    ui.add_space(theme::SPACE_M);
    match tab.insights.page {
        Page::Insights => {
            load(tab, ui.ctx());
            insights_page(ui, tab, cmds);
        }
        Page::Cleanup => crate::ui::cleanup::show(ui, tab, cmds),
        Page::Sparse => crate::ui::sparse::show(ui, tab, cmds),
    }
}

/// Title and the page switcher.
fn header(ui: &mut egui::Ui, tab: &mut RepoTab) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icons::INSIGHTS)
                .size(20.0)
                .color(theme::accent(ui)),
        );
        ui.label(
            RichText::new(t!("Repository health"))
                .font(theme::strong_font(18.0))
                .color(ui.visuals().strong_text_color()),
        );
        ui.add_space(theme::SPACE_L);
        let st = &mut tab.insights;
        for (page, icon, label) in [
            (Page::Insights, icons::INSIGHTS, t!("Insights")),
            (Page::Cleanup, icons::CLEANUP, t!("Cleanup")),
            (Page::Sparse, icons::TREE_VIEW, t!("Sparse checkout")),
        ] {
            if chip(ui, st.page == page, &icons::text(icon, label), None).clicked() {
                st.page = page;
            }
        }
    });
}

fn insights_page(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    filters(ui, tab);
    ui.add_space(theme::SPACE_M);
    let mut owner_query = std::mem::take(&mut tab.insights.owner_query);
    body(ui, &tab.insights, &mut owner_query, cmds);
    tab.insights.owner_query = owner_query;
}

fn body(ui: &mut egui::Ui, st: &InsightsState, owner_query: &mut String, cmds: &mut Commands) {
    let ins = match &st.result {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(t!("Reading the history…"));
            });
            return;
        }
        Some(Err(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
            return;
        }
        Some(Ok(i)) => i,
    };
    if ins.commits == 0 {
        crate::ui::empty_state::show(
            ui,
            icons::INSIGHTS,
            &t!("No commits in this period"),
            &t!("Pick a longer period or another path."),
            |_| {},
        );
        return;
    }
    let owners = st.owners.clone().flatten();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // Room for the scroll bar.
            ui.set_max_width(ui.available_width() - 12.0);
            tiles(ui, ins, st.loading.is_some());
            ui.add_space(theme::SPACE_L);
            section(ui, &t!("Churn"), &t!("lines added and deleted"), |ui| {
                churn_chart(ui, ins)
            });
            ui.add_space(theme::SPACE_L);
            let wide = ui.available_width() > 900.0;
            let body = |ui: &mut egui::Ui, cmds: &mut Commands, q: &mut String, col: u8| {
                if col != 1 {
                    section(
                        ui,
                        &t!("Hotspots"),
                        &t!("changed often and recently"),
                        |ui| hotspots(ui, ins, owners.as_ref(), cmds),
                    );
                    ui.add_space(theme::SPACE_L);
                }
                if col != 0 {
                    section(
                        ui,
                        &t!("Knowledge risk"),
                        &t!("files mostly changed by one person"),
                        |ui| knowledge(ui, ins, cmds),
                    );
                    ui.add_space(theme::SPACE_L);
                    section(ui, &t!("Code owners"), "", |ui| {
                        code_owners(ui, owners.as_ref(), q)
                    });
                }
            };
            if wide {
                ui.columns(2, |cols| {
                    body(&mut cols[0], cmds, owner_query, 0);
                    body(&mut cols[1], cmds, owner_query, 1);
                });
            } else {
                body(ui, cmds, owner_query, 2);
            }
            ui.add_space(theme::SPACE_L);
            section(ui, &t!("Contributors over time"), "", |ui| {
                timeline(ui, ins);
                ui.add_space(theme::SPACE_M);
                contributors(ui, ins);
            });
            ui.add_space(theme::SPACE_XL);
        });
}

fn filters(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let st = &mut tab.insights;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_S + 2.0;
        ui.weak(t!("Period"));
        for p in Period::ALL {
            if chip(ui, st.period == p, &p.label(), None).clicked() {
                st.period = p;
            }
        }
        ui.add_space(theme::SPACE_L);
        ui.label(RichText::new(icons::FOLDER).color(ui.visuals().weak_text_color()));
        let r = ui.add(
            egui::TextEdit::singleline(&mut st.path)
                .hint_text(t!("Path, e.g. src/"))
                .desired_width(200.0),
        );
        if r.lost_focus() || (r.changed() && st.path.trim().is_empty()) {
            st.applied_path = st.path.trim().to_owned();
        }
        if !st.applied_path.is_empty()
            && ui
                .add(icons::icon_button(icons::CLOSE).frame_when_inactive(false))
                .on_hover_text(t!("Clear"))
                .clicked()
        {
            st.path.clear();
            st.applied_path.clear();
        }
        if st.loading.is_some() {
            ui.spinner();
        }
    });
}

/// A titled card.
fn section(ui: &mut egui::Ui, title: &str, hint: &str, body: impl FnOnce(&mut egui::Ui)) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(theme::strong(title).size(14.0));
            if !hint.is_empty() {
                ui.weak(hint);
            }
        });
        ui.add_space(theme::SPACE_M);
        body(ui);
    });
}

/// The numbers on top: commits, contributors, files, bus factor.
fn tiles(ui: &mut egui::Ui, ins: &Insights, loading: bool) {
    let n = 4.0;
    let gap = 10.0;
    let width = ((ui.available_width() - gap * (n - 1.0)) / n).max(120.0);
    let commits = if ins.truncated {
        format!("{}+", ins.commits)
    } else {
        ins.commits.to_string()
    };
    let bus_hint = t!(
        "The fewest people who together wrote most of more than half of the files. 1 means one person leaving would leave much of the code without anyone who knows it."
    );
    let list = [
        (icons::GIT_COMMIT, commits, t!("Commits"), String::new()),
        (
            icons::USERS_THREE,
            ins.contributors.len().to_string(),
            t!("Contributors"),
            String::new(),
        ),
        (
            icons::FILE,
            ins.files.len().to_string(),
            t!("Files changed"),
            String::new(),
        ),
        (
            icons::WARNING,
            ins.bus_factor.to_string(),
            t!("Bus factor"),
            bus_hint,
        ),
    ];
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (i, (icon, value, label, hint)) in list.into_iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(width, 64.0), Sense::hover());
            let v = ui.visuals();
            let p = ui.painter();
            p.rect(
                rect,
                theme::RADIUS_LARGE,
                v.faint_bg_color,
                v.widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Inside,
            );
            let risky = i == 3 && ins.bus_factor <= 1;
            let color = if risky {
                v.warn_fg_color
            } else {
                v.weak_text_color()
            };
            p.text(
                rect.left_top() + vec2(14.0, 18.0),
                egui::Align2::LEFT_CENTER,
                icons::text(icon, &label),
                egui::FontId::proportional(12.0),
                color,
            );
            p.text(
                rect.left_top() + vec2(14.0, 42.0),
                egui::Align2::LEFT_CENTER,
                &value,
                theme::strong_font(20.0),
                if risky {
                    v.warn_fg_color
                } else {
                    v.strong_text_color()
                },
            );
            if loading {
                p.text(
                    rect.right_top() + vec2(-14.0, 18.0),
                    egui::Align2::RIGHT_CENTER,
                    icons::ARROWS_CLOCKWISE,
                    egui::FontId::proportional(12.0),
                    v.weak_text_color(),
                );
            }
            if !hint.is_empty() {
                resp.on_hover_text(hint);
            }
        }
    });
    if ins.truncated {
        ui.weak(t!(
            "Only the newest {} commits were read; choose a shorter period for the rest.",
            ins.commits
        ));
    }
}

fn day(secs: i64) -> String {
    crate::util::format_day(secs, ferrum_core::local_offset_minutes())
}

/// Added lines up, deleted lines down, one bar pair per bucket.
fn churn_chart(ui: &mut egui::Ui, ins: &Insights) {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 120.0), Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    let (add_c, del_c) = (crate::util::added_color(ui), crate::util::removed_color(ui));
    let faint = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let n = ins.buckets.len().max(1);
    let max_add = ins
        .buckets
        .iter()
        .map(|b| b.added)
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    let max_del = ins
        .buckets
        .iter()
        .map(|b| b.deleted)
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    // The axis sits where both halves get room in proportion to their peaks.
    let mid = rect.top() + rect.height() * (max_add / (max_add + max_del)).clamp(0.3, 0.7);
    painter.hline(rect.x_range(), mid, (1.0, faint));
    let step = rect.width() / n as f32;
    let gap = (step * 0.25).clamp(1.0, 4.0);
    let hovered = resp
        .hover_pos()
        .map(|p| (((p.x - rect.left()) / step) as usize).min(n - 1));
    let up = mid - rect.top() - 2.0;
    let down = rect.bottom() - mid - 2.0;
    let bw = (step - gap).min(MAX_BAR);
    for (i, b) in ins.buckets.iter().enumerate() {
        let x0 = rect.left() + i as f32 * step + (step - gap - bw) / 2.0;
        let hot = hovered == Some(i);
        let alpha = if hot || hovered.is_none() { 1.0 } else { 0.55 };
        if b.added > 0 {
            let h = (b.added as f32 / max_add * up).max(2.0);
            let r = Rect::from_min_max(pos2(x0, mid - h - 1.0), pos2(x0 + bw, mid - 1.0));
            painter.rect_filled(
                r,
                egui::CornerRadius {
                    nw: 2,
                    ne: 2,
                    ..Default::default()
                },
                add_c.gamma_multiply(alpha),
            );
        }
        if b.deleted > 0 {
            let h = (b.deleted as f32 / max_del * down).max(2.0);
            let r = Rect::from_min_max(pos2(x0, mid + 1.0), pos2(x0 + bw, mid + h + 1.0));
            painter.rect_filled(
                r,
                egui::CornerRadius {
                    sw: 2,
                    se: 2,
                    ..Default::default()
                },
                del_c.gamma_multiply(alpha),
            );
        }
    }
    if let Some(i) = hovered
        && let Some(b) = ins.buckets.get(i)
    {
        resp.on_hover_ui(|ui| {
            ui.label(theme::strong(day(b.start)));
            ui.label(t!("{} commits", b.commits));
            ui.colored_label(add_c, format!("+{}", b.added));
            ui.colored_label(del_c, format!("−{}", b.deleted));
        });
    }
    axis_dates(ui, ins);
    ui.horizontal(|ui| {
        legend(ui, add_c, &t!("Added"));
        legend(ui, del_c, &t!("Deleted"));
        let total_add: u64 = ins.buckets.iter().map(|b| b.added).sum();
        let total_del: u64 = ins.buckets.iter().map(|b| b.deleted).sum();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let per = match ins.bucket_days {
                1 => t!("one bar per day"),
                7 => t!("one bar per week"),
                30 => t!("one bar per month"),
                n => t!("one bar per {} days", n),
            };
            ui.weak(format!(
                "{} · {per}",
                t!("+{} −{} lines", total_add, total_del)
            ));
        });
    });
}

fn axis_dates(ui: &mut egui::Ui, ins: &Insights) {
    let (Some(first), Some(_)) = (ins.buckets.first(), ins.buckets.last()) else {
        return;
    };
    ui.horizontal(|ui| {
        let small = |s: String| RichText::new(s).small().weak();
        ui.label(small(day(first.start)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(small(day(crate::util::now_secs())));
        });
    });
}

fn legend(ui: &mut egui::Ui, color: Color32, label: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().rect_filled(r, 2.0, color);
    ui.weak(label);
    ui.add_space(theme::SPACE_S);
}

/// A thin horizontal meter.
fn meter(ui: &mut egui::Ui, frac: f32, color: Color32, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, ui.visuals().widgets.inactive.weak_bg_fill);
    let mut fill = rect;
    fill.set_width((rect.width() * frac.clamp(0.0, 1.0)).max(3.0));
    p.rect_filled(fill, 3.0, color);
}

/// A file path that opens its history.
fn file_link(ui: &mut egui::Ui, path: &str, width: f32, cmds: &mut Commands) {
    let (dir, name) = match path.rsplit_once('/') {
        Some((d, n)) => (format!("{d}/"), n),
        None => (String::new(), path),
    };
    let mut job = egui::text::LayoutJob::default();
    let v = ui.visuals();
    job.append(
        &dir,
        0.0,
        egui::TextFormat::simple(egui::FontId::proportional(12.5), v.weak_text_color()),
    );
    job.append(
        name,
        0.0,
        egui::TextFormat::simple(egui::FontId::proportional(12.5), v.text_color()),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    let r = ui
        .add(egui::Label::new(job).sense(Sense::click()).truncate())
        .on_hover_text(t!("{}\nClick for the file history", path));
    if r.clicked() {
        cmds.push(Command::FileHistory(path.to_owned()));
    }
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

const ROWS: usize = 12;
/// Widest bar of the charts.
const MAX_BAR: f32 = 28.0;

fn hotspots(ui: &mut egui::Ui, ins: &Insights, owners: Option<&CodeOwners>, cmds: &mut Commands) {
    let max = ins.files.first().map_or(1.0, |f| f.score.max(0.001)) as f32;
    let accent = theme::accent(ui);
    let path_w = (ui.available_width() - 250.0).max(120.0);
    egui::Grid::new("hotspots")
        .num_columns(4)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            for h in [t!("File"), t!("Heat"), t!("Commits"), t!("Lines")] {
                ui.label(RichText::new(h).small().weak());
            }
            ui.end_row();
            for f in ins.files.iter().take(ROWS) {
                ui.horizontal(|ui| {
                    ui.set_width(path_w);
                    file_link(ui, &f.path, path_w - 8.0, cmds);
                });
                ui.vertical(|ui| {
                    ui.add_space(5.0);
                    meter(ui, f.score as f32 / max, accent, 70.0);
                });
                ui.label(f.commits.to_string());
                let lines = ui.label(RichText::new(format!("+{} −{}", f.added, f.deleted)).weak());
                let mut tip = t!("Last changed {}", day(f.last_change));
                if let Some(co) = owners {
                    let o = co.owners(&f.path);
                    if !o.is_empty() {
                        tip.push('\n');
                        tip.push_str(&t!("Owners: {}", o.join(", ")));
                    }
                }
                lines.on_hover_text(tip);
                ui.end_row();
            }
        });
}

fn knowledge(ui: &mut egui::Ui, ins: &Insights, cmds: &mut Commands) {
    let islands = ins.knowledge_islands(0.8, 3);
    if islands.is_empty() {
        ui.weak(t!(
            "No file with 3 or more commits was mostly changed by one person."
        ));
        return;
    }
    let warn = ui.visuals().warn_fg_color;
    let path_w = (ui.available_width() - 290.0).max(120.0);
    egui::Grid::new("knowledge")
        .num_columns(3)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            for h in [t!("File"), t!("Main author"), t!("Share")] {
                ui.label(RichText::new(h).small().weak());
            }
            ui.end_row();
            for f in islands.iter().take(ROWS) {
                let Some((who, share)) = f.main_author() else {
                    continue;
                };
                ui.horizontal(|ui| {
                    ui.set_width(path_w);
                    file_link(ui, &f.path, path_w - 8.0, cmds);
                });
                let name = ins
                    .contributors
                    .get(who)
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.set_width(130.0);
                    ui.add(egui::Label::new(icons::text(icons::USER, name)).truncate());
                });
                ui.horizontal(|ui| {
                    meter(ui, share, warn, 50.0);
                    ui.weak(format!("{:.0}%", share * 100.0));
                });
                ui.end_row();
            }
        });
    if islands.len() > ROWS {
        ui.weak(t!("{} more files", islands.len() - ROWS));
    }
}

fn code_owners(ui: &mut egui::Ui, owners: Option<&CodeOwners>, query: &mut String) {
    let Some(co) = owners else {
        ui.weak(t!(
            "No CODEOWNERS file. Add one to .github/, docs/ or the root to name who reviews which paths."
        ));
        return;
    };
    ui.horizontal(|ui| {
        ui.label(icons::text(icons::FILE_TEXT, &co.file));
        ui.weak(t!("{} rules", co.rules.len()));
    });
    ui.add(
        egui::TextEdit::singleline(query)
            .hint_text(t!("Who owns a path? e.g. src/main.rs"))
            .desired_width(f32::INFINITY),
    );
    let q = query.trim();
    if q.is_empty() {
        return;
    }
    match co.rule_for(q) {
        Some(r) if !r.owners.is_empty() => {
            ui.horizontal_wrapped(|ui| {
                for o in &r.owners {
                    crate::ui::branch_dnd::pill(ui, icons::USER, o, true);
                }
            });
            ui.weak(t!("Line {}: {}", r.line, r.pattern));
        }
        Some(r) => {
            ui.weak(t!("No owners (line {}: {}).", r.line, r.pattern));
        }
        None => {
            ui.weak(t!("No rule matches this path."));
        }
    }
}

fn author_color(i: usize, ui: &egui::Ui) -> Color32 {
    if i >= TIMELINE_AUTHORS {
        ui.visuals().weak_text_color().gamma_multiply(0.6)
    } else {
        crate::ui::history::lane_color(i)
    }
}

/// Commits per bucket, stacked by contributor (the top ones, then others).
fn timeline(ui: &mut egui::Ui, ins: &Insights) {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 110.0), Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    let faint = ui.visuals().widgets.noninteractive.bg_stroke.color;
    painter.hline(rect.x_range(), rect.bottom(), (1.0, faint));
    let n = ins.buckets.len().max(1);
    let max = ins
        .buckets
        .iter()
        .map(|b| b.commits)
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    let step = rect.width() / n as f32;
    let gap = (step * 0.25).clamp(1.0, 4.0);
    let hovered = resp
        .hover_pos()
        .map(|p| (((p.x - rect.left()) / step) as usize).min(n - 1));
    let surface = ui.visuals().panel_fill;
    let bw = (step - gap).min(MAX_BAR);
    for (i, b) in ins.buckets.iter().enumerate() {
        let x0 = rect.left() + i as f32 * step + (step - gap - bw) / 2.0;
        let mut y = rect.bottom();
        let alpha = if hovered.is_none() || hovered == Some(i) {
            1.0
        } else {
            0.55
        };
        for (who, &c) in b.by_author.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let h = c as f32 / max * (rect.height() - 4.0);
            let r = Rect::from_min_max(pos2(x0, y - h), pos2(x0 + bw, y));
            painter.rect_filled(r, 1.0, author_color(who, ui).gamma_multiply(alpha));
            // A thin gap between stacked parts.
            painter.hline(r.x_range(), r.top(), (1.0, surface));
            y -= h;
        }
    }
    if let Some(i) = hovered
        && let Some(b) = ins.buckets.get(i)
    {
        resp.on_hover_ui(|ui| {
            ui.label(theme::strong(day(b.start)));
            ui.label(t!("{} commits", b.commits));
            for (who, &c) in b.by_author.iter().enumerate() {
                if c == 0 {
                    continue;
                }
                let name = if who >= TIMELINE_AUTHORS {
                    t!("Others")
                } else {
                    ins.contributors[who].name.clone()
                };
                ui.horizontal(|ui| {
                    legend(ui, author_color(who, ui), &format!("{name}: {c}"));
                });
            }
        });
    }
    axis_dates(ui, ins);
    ui.horizontal_wrapped(|ui| {
        for (i, c) in ins.contributors.iter().take(TIMELINE_AUTHORS).enumerate() {
            legend(ui, author_color(i, ui), &c.name);
        }
        if ins.contributors.len() > TIMELINE_AUTHORS {
            legend(ui, author_color(TIMELINE_AUTHORS, ui), &t!("Others"));
        }
    });
}

fn contributors(ui: &mut egui::Ui, ins: &Insights) {
    let max = ins.contributors.first().map_or(1, |c| c.commits.max(1)) as f32;
    let accent = theme::accent(ui);
    egui::Grid::new("contributors")
        .num_columns(5)
        .spacing([16.0, 6.0])
        .striped(false)
        .show(ui, |ui| {
            for h in [
                t!("Contributor"),
                t!("Commits"),
                t!("Lines"),
                t!("Main author of"),
                t!("Active"),
            ] {
                ui.label(RichText::new(h).small().weak());
            }
            ui.end_row();
            for (i, c) in ins.contributors.iter().take(15).enumerate() {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter()
                        .circle_filled(r.center(), 4.0, author_color(i, ui));
                    ui.label(&c.name).on_hover_text(&c.email);
                });
                ui.horizontal(|ui| {
                    meter(ui, c.commits as f32 / max, accent, 60.0);
                    ui.label(c.commits.to_string());
                });
                ui.weak(format!("+{} −{}", c.added, c.deleted));
                ui.label(t!("{} files", c.owned_files));
                ui.weak(format!("{} – {}", day(c.first), day(c.last)));
                ui.end_row();
            }
        });
    if ins.contributors.len() > 15 {
        ui.weak(t!("and {} more", ins.contributors.len() - 15));
    }
}

/// Command palette entries for the view.
pub fn palette_entries(
    _: &crate::settings::Settings,
    tab: Option<&RepoTab>,
    list: &mut Vec<crate::ui::palette::Entry>,
) {
    use crate::ui::palette::{Entry, Group};
    if tab.is_none() {
        return;
    }
    let go =
        |page: &'static str| move || crate::ui::history_options::cmd(move |t, _| open(t, page));
    for (label, page) in [
        (t!("Show repository insights"), ""),
        (t!("Clean up the repository…"), "cleanup"),
        (t!("Sparse checkout…"), "sparse"),
    ] {
        list.push(Entry::new(Group::Commands, label, "", go(page)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_have_days() {
        assert_eq!(Period::All.days(), None);
        assert_eq!(Period::Month.days(), Some(30));
        assert_eq!(Period::default(), Period::Year);
    }
}
