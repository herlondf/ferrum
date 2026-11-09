//! The explain panel on the right of a repository: "Explain this commit",
//! "Explain this diff" and "Explain this file's history" stream their
//! answer here, as Markdown.

use std::path::PathBuf;

use eframe::egui::{self, RichText};

use crate::ai::{Stream, Subject};
use crate::command::{Command, Commands};
use crate::icons;
use crate::tab::RepoTab;
use crate::theme;
use crate::ui::ai_common;

#[derive(Default)]
pub struct ExplainPanel {
    pub open: bool,
    /// The repository it is about.
    pub repo: PathBuf,
    pub subject: Option<Subject>,
    title: String,
    stream: Option<Stream>,
    cache: egui_commonmark::CommonMarkCache,
}

impl ExplainPanel {
    /// Opens the panel on `subject` and starts the request.
    pub fn explain(&mut self, ctx: &egui::Context, tab: &RepoTab, subject: Subject) {
        self.open = true;
        self.repo = tab.path().to_path_buf();
        self.title = subject.title(tab);
        self.subject = Some(subject);
        self.cache = Default::default();
        self.start(ctx);
    }

    fn start(&mut self, ctx: &egui::Context) {
        let Some(subject) = &self.subject else { return };
        let cfg = ai_common::config(ctx);
        self.stream = cfg
            .enabled
            .then(|| Stream::start(ctx, &cfg, subject.prompt(self.repo.clone(), cfg.max)));
    }

    pub fn shows(&self, repo: &std::path::Path) -> bool {
        self.open && self.repo == repo
    }
}

pub fn show(ui: &mut egui::Ui, p: &mut ExplainPanel, cmds: &mut Commands) {
    let enabled = ai_common::config(ui.ctx()).enabled;
    // Turned on while the panel waited: start now.
    if enabled && p.stream.is_none() && p.subject.is_some() {
        p.start(ui.ctx());
    }
    ui.add_space(theme::SPACE_S);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icons::AI).size(16.0).color(theme::accent(ui)));
        ui.label(theme::strong(t!("Explain")).size(15.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(icons::X).frame_when_inactive(false))
                .on_hover_text(t!("Close"))
                .clicked()
            {
                p.open = false;
                if let Some(s) = &p.stream {
                    s.cancel();
                }
            }
            ai_common::badge(ui);
        });
    });
    ui.add(
        egui::Label::new(RichText::new(&p.title).color(ui.visuals().weak_text_color())).truncate(),
    )
    .on_hover_text(&p.title);
    ui.add_space(theme::SPACE_XS);
    ui.separator();
    if !enabled {
        ui.add_space(theme::SPACE_S);
        ai_common::off_card(ui, cmds);
        return;
    }
    let Some(stream) = &p.stream else { return };
    let text = stream.text();
    let running = stream.running();
    let done = stream.finished();
    egui::Panel::bottom("ai-explain-actions")
        .show_separator_line(true)
        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6)))
        .show(ui, |ui| {
            if let Some(sent) = stream.sent() {
                ai_common::sent_line(ui, &sent);
            }
            ui.horizontal(|ui| {
                if running {
                    if ui
                        .add(theme::secondary(icons::text(icons::X, t!("Stop"))))
                        .clicked()
                    {
                        stream.cancel();
                    }
                } else if ui
                    .add(theme::secondary(icons::text(icons::REFRESH, t!("Again"))))
                    .on_hover_text(t!("Ask again"))
                    .clicked()
                {
                    cmds.push(Command::Ai(crate::ai::AiCmd::Explain(
                        p.subject.clone().expect("a subject"),
                    )));
                }
                let answer = ferrum_core::ai::strip_thinking(&text);
                if !answer.trim().is_empty()
                    && ui
                        .add(theme::secondary(icons::text(icons::COPY, t!("Copy"))))
                        .clicked()
                {
                    cmds.push(Command::Copy(answer));
                }
            });
        });
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(running)
        .show(ui, |ui| {
            ui.add_space(theme::SPACE_S);
            ai_common::answer(ui, &mut p.cache, &text, running);
            if let Some(Err(e)) = done {
                ui.add_space(theme::SPACE_S);
                ui.colored_label(ui.visuals().error_fg_color, icons::text(icons::WARNING, e));
            }
        });
}
