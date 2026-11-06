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
        let st = &mut tab.overview;
        match &st.readme {
            Some(Some((name, text))) => {
                ui.weak(name.as_str());
                ui.separator();
                let cache = st.markdown.get_or_insert_with(Default::default);
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        egui_commonmark::CommonMarkViewer::new().show(ui, cache, text);
                    });
            }
            _ => {
                ui.add_space(20.0);
                ui.weak(t!("This repository has no README."));
            }
        }
    });
}

fn facts(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
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
