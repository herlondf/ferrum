//! "Suggest resolution" in the conflict editor: the model gets the base
//! and both sides of the file and proposes a merged file with an
//! explanation. It is shown as a review card, like the semantic merge
//! suggestion; nothing goes in the result until the user takes it, and
//! nothing is staged until the file is marked as resolved.

use eframe::egui::{self, RichText};
use ferrum_core::ai::prompts::{self, ConflictSides, Resolution};
use ferrum_core::conflict;

use crate::ai::Stream;
use crate::icons;
use crate::theme;
use crate::ui::ai_common;

#[derive(Default)]
pub struct AiHelp {
    /// The file the suggestion is for.
    path: String,
    stream: Option<Stream>,
    result: Option<Result<Resolution, String>>,
    dismissed: bool,
    /// The suggestion's difference with ours (false) or theirs (true).
    compare: Option<bool>,
    cache: egui_commonmark::CommonMarkCache,
}

impl AiHelp {
    fn for_file(&mut self, path: &str) {
        if self.path != path {
            *self = Self {
                path: path.to_owned(),
                ..Default::default()
            };
        }
    }

    fn start(
        &mut self,
        ctx: &egui::Context,
        versions: &[Option<String>; 3],
        names: &(String, String),
    ) {
        let cfg = ai_common::config(ctx);
        let (path, base) = (self.path.clone(), versions[0].clone());
        let ours = versions[1].clone().unwrap_or_default();
        let theirs = versions[2].clone().unwrap_or_default();
        let names = names.clone();
        let max = cfg.max;
        self.result = None;
        self.dismissed = false;
        self.cache = Default::default();
        self.stream = Some(Stream::start(ctx, &cfg, move || {
            let sides = ConflictSides {
                path: &path,
                base: base.as_deref(),
                ours: &ours,
                theirs: &theirs,
                ours_name: &names.0,
                theirs_name: &names.1,
            };
            Ok(prompts::resolve_conflict(
                &sides,
                crate::ai::language(),
                max,
            ))
        }));
    }

    fn poll(&mut self, ends_with_newline: bool) {
        let Some(done) = self.stream.as_ref().and_then(Stream::finished) else {
            return;
        };
        self.stream = None;
        self.result = Some(done.and_then(|text| {
            prompts::parse_resolution(&text, ends_with_newline)
                .ok_or_else(|| t!("The answer had no merged file."))
        }));
    }
}

fn auto_id() -> egui::Id {
    egui::Id::unique("ai-conflict-auto")
}

/// Starts a suggestion for the open file on the next frame (`FERRUM_START`).
pub fn request(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(auto_id(), true));
}

/// Shows the button, the running request or the suggestion. Returns the
/// merged text when the user takes it.
pub fn show(
    ui: &mut egui::Ui,
    help: &mut AiHelp,
    path: &str,
    versions: &[Option<String>; 3],
    names: &(String, String),
    result: &str,
) -> Option<String> {
    help.for_file(path);
    let ends_nl = versions[1].as_deref().unwrap_or("").ends_with('\n');
    help.poll(ends_nl);
    if help.dismissed {
        return None;
    }
    let mut taken = None;
    let enabled = ai_common::config(ui.ctx()).enabled;
    if enabled && ui.ctx().data_mut(|d| d.remove_temp::<bool>(auto_id())).unwrap_or(false) {
        help.start(ui.ctx(), versions, names);
    }
    match (&help.stream, &help.result) {
        (Some(stream), _) => {
            theme::card(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spinner();
                    let lines = stream.text().lines().count();
                    ui.label(t!("Asking for a resolution… {} lines so far", lines));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t!("Cancel")).clicked() {
                            stream.cancel();
                        }
                        ai_common::badge(ui);
                    });
                });
                if let Some(sent) = stream.sent() {
                    ai_common::sent_line(ui, &sent);
                }
            });
        }
        (None, None) => {
            ui.horizontal(|ui| {
                let resp = ui
                    .add_enabled(enabled, egui::Button::new(icons::text(icons::AI, t!("Suggest resolution"))))
                    .on_hover_text(t!("The model proposes a merged file with an explanation; you review it before it goes in the result"))
                    .on_disabled_hover_text(t!("AI is off: turn it on in Settings > AI."));
                if resp.clicked() {
                    help.start(ui.ctx(), versions, names);
                }
                ai_common::badge(ui);
            });
        }
        (None, Some(Err(e))) => {
            let e = e.clone();
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().error_fg_color, icons::text(icons::WARNING, e));
                if ui.small_button(t!("Try again")).clicked() {
                    help.start(ui.ctx(), versions, names);
                }
                if ui
                    .small_button(icons::X)
                    .on_hover_text(t!("Dismiss the suggestion"))
                    .clicked()
                {
                    help.dismissed = true;
                }
            });
        }
        (None, Some(Ok(r))) => {
            let r = r.clone();
            let accent = theme::accent(ui);
            theme::card(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icons::AI).size(18.0).color(accent));
                    ui.label(theme::strong(t!("AI suggestion — review")).size(14.0));
                    ai_common::badge(ui);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(icons::X).frame_when_inactive(false))
                            .on_hover_text(t!("Dismiss the suggestion"))
                            .clicked()
                        {
                            help.dismissed = true;
                        }
                        if ui
                            .add(egui::Button::new(icons::REFRESH).frame_when_inactive(false))
                            .on_hover_text(t!("Ask again"))
                            .clicked()
                        {
                            help.start(ui.ctx(), versions, names);
                        }
                        if result == r.text {
                            ui.colored_label(
                                crate::util::added_color(ui),
                                icons::text(icons::CHECK, t!("In the result below")),
                            );
                        } else if ui
                            .add(theme::primary(icons::text(icons::CHECK, t!("Use this result"))))
                            .on_hover_text(t!("Put the merged file in the result editor; nothing is staged until you mark it as resolved"))
                            .clicked()
                        {
                            taken = Some(r.text.clone());
                        }
                    });
                });
                if r.has_markers {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        icons::text(icons::WARNING, t!("The suggestion still has conflict markers.")),
                    );
                }
                ui.add_space(theme::SPACE_S);
                if !r.explanation.is_empty() {
                    egui::ScrollArea::vertical()
                        .id_salt("ai-conflict-explanation")
                        .max_height(140.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| ai_common::answer(ui, &mut help.cache, &r.explanation, false));
                }
                ui.weak(t!("Generated by a model: check every line before marking the file as resolved."));
                ui.add_space(theme::SPACE_S);
                ui.horizontal(|ui| {
                    ui.label(t!("Compare with"));
                    for (theirs, name) in [(false, &names.0), (true, &names.1)] {
                        let on = help.compare == Some(theirs);
                        if ui.selectable_label(on, name.as_str()).clicked() {
                            help.compare = if on { None } else { Some(theirs) };
                        }
                    }
                });
                if let Some(theirs) = help.compare {
                    let side = versions[1 + usize::from(theirs)].as_deref().unwrap_or("");
                    crate::ui::smart_merge::diff_lines(ui, &conflict::diff_lines(side, &r.text, 2));
                }
            });
        }
    }
    ui.add_space(theme::SPACE_S);
    taken
}
