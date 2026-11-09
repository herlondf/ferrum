//! Repository overview (sidebar > Overview): the README, remotes and a
//! summary with commit, contributor, branch and tag counts and the size.

use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, RichText};
use ferrum_core::library::{self, Summary};

use crate::command::{Command, Commands};
use crate::repos::RepoCmd;
use crate::tab::{RepoTab, View};

#[derive(Default)]
pub struct OverviewState {
    summary: Option<Result<Summary, String>>,
    loading: Option<Receiver<Result<Summary, String>>>,
    /// README file name and text; `Some(None)` when there is none.
    readme: Option<Option<(String, String)>>,
    markdown: Option<egui_commonmark::CommonMarkCache>,
    /// HEAD when the summary was counted: a new commit counts again.
    counted_at: Option<ferrum_core::Oid>,
}

impl OverviewState {
    /// Loads everything again the next time the overview is shown.
    pub fn reset(&mut self) {
        self.summary = None;
        self.loading = None;
        self.readme = None;
    }

    #[cfg(test)]
    pub fn summary(&self) -> Option<&Summary> {
        self.summary.as_ref()?.as_ref().ok()
    }
}

pub fn open(tab: &mut RepoTab) {
    tab.overview.reset();
    tab.view = View::Overview;
}

fn load(tab: &mut RepoTab, ctx: &egui::Context) {
    let st = &mut tab.overview;
    if st.counted_at != tab.head_oid {
        st.counted_at = tab.head_oid;
        st.summary = None;
        st.loading = None;
    }
    if let Some(rx) = &st.loading
        && let Ok(r) = rx.try_recv()
    {
        st.summary = Some(r);
        st.loading = None;
    }
    if st.summary.is_none() && st.loading.is_none() {
        let (tx, rx) = mpsc::channel();
        let path = tab.repo.path().to_path_buf();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(library::summary(&path).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        st.loading = Some(rx);
    }
    if st.readme.is_none() {
        st.readme = Some(library::readme(tab.repo.path()));
    }
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    load(tab, ui.ctx());
    ui.horizontal(|ui| {
        ui.heading(RichText::new(&tab.name).strong());
        if let Some(b) = &tab.head {
            ui.label(format!("🌿 {b}"));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t!("Repository settings…")).clicked() {
                cmds.push(Command::Repos(RepoCmd::Settings));
            }
            if ui.button("↻").on_hover_text(t!("Reload")).clicked() {
                tab.overview.reset();
                crate::ui::readme_images::forget(ui.ctx());
            }
        });
    });
    ui.weak(tab.path().display().to_string());
    ui.separator();
    egui::Panel::right("overview-facts")
        .resizable(true)
        .default_size(300.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| facts(ui, tab, cmds));
        });
    egui::CentralPanel::default_margins().show(ui, |ui| {
        let root = tab.path().to_path_buf();
        let st = &mut tab.overview;
        match &st.readme {
            Some(Some((name, text))) => {
                ui.label(crate::icons::text(crate::icons::FILE_TEXT, name));
                ui.separator();
                let cache = st.markdown.get_or_insert_with(Default::default);
                // Relative image paths are read from the repository.
                crate::ui::readme_images::install(ui.ctx());
                let dir = std::path::Path::new(name.as_str())
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let prefix = crate::ui::readme_images::uri_prefix(&root, &dir);
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        egui_commonmark::CommonMarkViewer::new()
                            .default_implicit_uri_scheme(prefix)
                            .max_image_width(Some(ui.available_width() as usize))
                            .show(ui, cache, text);
                    });
            }
            _ => {
                crate::ui::empty_state::show(
                    ui,
                    crate::icons::FILE_DASHED,
                    &t!("This repository has no README."),
                    &t!("Add a README.md to describe the project; it will show up here."),
                    |_| {},
                );
            }
        }
    });
}

/// Commits per week over the last year, as bars.
fn activity(ui: &mut egui::Ui, weeks: &[u32]) {
    use eframe::egui::{Rect, Sense, pos2, vec2};
    let total: u32 = weeks.iter().sum();
    let max = weeks.iter().copied().max().unwrap_or(0).max(1);
    ui.horizontal(|ui| {
        ui.label(crate::theme::strong(t!("Activity")));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(t!("last 12 months"));
        });
    });
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 64.0), Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    let accent = crate::theme::accent(ui);
    let faint = ui.visuals().widgets.noninteractive.bg_stroke.color;
    painter.hline(rect.x_range(), rect.bottom(), (1.0, faint));
    let n = weeks.len().max(1);
    let step = rect.width() / n as f32;
    let gap = (step * 0.25).clamp(1.0, 3.0);
    let hovered = resp
        .hover_pos()
        .map(|p| (((p.x - rect.left()) / step) as usize).min(n - 1));
    for (i, &c) in weeks.iter().enumerate() {
        let x0 = rect.left() + i as f32 * step;
        let h = if c == 0 {
            1.5
        } else {
            (c as f32 / max as f32 * (rect.height() - 4.0)).max(3.0)
        };
        let bar = Rect::from_min_max(
            pos2(x0, rect.bottom() - h),
            pos2(x0 + step - gap, rect.bottom()),
        );
        let color = match (c, hovered == Some(i)) {
            (0, _) => faint,
            (_, true) => accent,
            _ => accent.gamma_multiply(0.75),
        };
        painter.rect_filled(bar, 1.0, color);
    }
    if let Some(i) = hovered {
        let ago = (weeks.len() - 1 - i) as i64;
        let start = crate::util::now_secs() - (ago + 1) * 7 * 86_400 + 86_400;
        let day = crate::util::format_day(start, ferrum_core::local_offset_minutes());
        let c = weeks[i];
        resp.on_hover_text(if c == 1 {
            t!("Week of {}: 1 commit", day)
        } else {
            t!("Week of {}: {} commits", day, c)
        });
    }
    ui.horizontal(|ui| {
        let off = ferrum_core::local_offset_minutes();
        let now = crate::util::now_secs();
        let first = now - weeks.len() as i64 * 7 * 86_400;
        let small = |s: String| egui::RichText::new(s).small().weak();
        ui.label(small(crate::util::format_day(first, off)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(small(crate::util::format_day(now, off)));
        });
    });
    ui.label(if total == 0 {
        t!("No commits in the last 12 months.")
    } else if total == 1 {
        t!("1 commit in the last 12 months")
    } else {
        t!("{} commits in the last 12 months", total)
    });
}

fn facts(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    if let Some(Ok(s)) = &tab.overview.summary
        && !s.activity.is_empty()
    {
        activity(ui, &s.activity);
        ui.add_space(14.0);
    }
    ui.strong(t!("Summary"));
    match &tab.overview.summary {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(t!("Counting…"));
            });
        }
        Some(Err(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        Some(Ok(s)) => {
            egui::Grid::new("overview-summary")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    let mut line = |label: String, value: String| {
                        ui.weak(label);
                        ui.label(value);
                        ui.end_row();
                    };
                    line(t!("Commits"), s.commits.to_string());
                    line(t!("Contributors"), s.contributors.to_string());
                    line(t!("Local branches"), s.local_branches.to_string());
                    line(t!("Remote branches"), s.remote_branches.to_string());
                    line(t!("Tags"), s.tags.to_string());
                    line(t!("Stashes"), tab.stashes.len().to_string());
                    line(t!("Size on disk"), size(s.git_size));
                    if let Some(f) = s.first_commit {
                        line(
                            t!("First commit"),
                            crate::util::format_day(f, ferrum_core::local_offset_minutes()),
                        );
                    }
                    if let Some(l) = s.last_commit {
                        line(
                            t!("Last commit"),
                            crate::util::format_day(l, ferrum_core::local_offset_minutes()),
                        );
                    }
                });
        }
    }
    ui.add_space(12.0);
    ui.strong(t!("Remotes"));
    if tab.remotes.is_empty() {
        ui.weak(t!("No remotes."));
    }
    for r in &tab.remotes {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&r.name).strong());
            ui.weak(&r.url);
        });
        if let Some(h) = ferrum_core::forge::parse_remote_url(&r.url)
            && ui.link(h.web_url()).clicked()
        {
            cmds.push(Command::OpenUrl(h.web_url()));
        }
    }
    if let Some(h) = &tab.hosted {
        ui.weak(t!("Pull requests use {}.", h.host));
    }
    ui.add_space(12.0);
    ui.strong(t!("Working copy"));
    let n = tab.status.total();
    ui.label(if n == 0 {
        t!("No local changes.")
    } else {
        t!("{} changed files", n)
    });
    if !tab.worktrees.is_empty() {
        ui.label(t!("{} worktrees", tab.worktrees.len()));
    }
    if !tab.submodules.is_empty() {
        ui.label(t!("{} submodules", tab.submodules.len()));
    }
}

fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} GB", b / (1024.0 * 1024.0 * 1024.0))
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB", (b / 1024.0).ceil())
    }
}
