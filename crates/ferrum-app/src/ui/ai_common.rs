//! Pieces every AI feature shows: where the data goes (next to each AI
//! button), the "AI is off" card, what was sent, and the streamed answer
//! as Markdown. The current settings are shared through the context each
//! frame, so views without the settings can start requests.

use std::sync::Arc;

use eframe::egui::{self, RichText};
use ferrum_core::ai;

use crate::ai::{Config, Sent, Where};
use crate::command::{Command, Commands};
use crate::icons;
use crate::settings::Settings;
use crate::theme;

fn id() -> egui::Id {
    egui::Id::unique("ai-config")
}

/// Every frame: shares the AI settings with the views.
pub fn publish(ctx: &egui::Context, settings: &Settings) {
    let cfg = Config::from_settings(settings);
    let same = ctx
        .data(|d| d.get_temp::<Arc<Config>>(id()))
        .is_some_and(|c| {
            c.enabled == cfg.enabled
                && c.provider == cfg.provider
                && c.max == cfg.max
                && c.secrets == cfg.secrets
        });
    if !same {
        ctx.data_mut(|d| d.insert_temp(id(), Arc::new(cfg)));
    }
}

/// The settings shared by [`publish`] (defaults before the first frame).
pub fn config(ctx: &egui::Context) -> Arc<Config> {
    ctx.data(|d| d.get_temp::<Arc<Config>>(id()))
        .unwrap_or_else(|| Arc::new(Config::from_settings(&Settings::default())))
}

pub fn where_now(ctx: &egui::Context) -> Where {
    crate::ai::where_to(&config(ctx).provider)
}

fn where_color(ui: &egui::Ui, w: &Where) -> egui::Color32 {
    if w.local {
        crate::util::added_color(ui)
    } else {
        ui.visuals().warn_fg_color
    }
}

/// A small pill: "🔒 Local · llama3.1" or "☁ Cloud · Anthropic", with the
/// full sentence on hover. Shows "AI is off" when it is.
pub fn badge(ui: &mut egui::Ui) -> egui::Response {
    let cfg = config(ui.ctx());
    let w = crate::ai::where_to(&cfg.provider);
    let (icon, text, color, hover) = if cfg.enabled {
        (w.icon, w.short.clone(), where_color(ui, &w), w.long.clone())
    } else {
        (
            icons::PROHIBIT,
            t!("AI is off"),
            ui.visuals().weak_text_color(),
            t!("Nothing is sent until you turn AI on in Settings > AI."),
        )
    };
    let frame = egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::symmetric(6, 1));
    frame
        .show(ui, |ui| {
            // One label, so it reads left to right in right-aligned bars too.
            ui.label(
                RichText::new(format!("{icon} {text}"))
                    .color(color)
                    .size(11.0),
            );
        })
        .response
        .on_hover_text(hover)
}

/// The text after an AI menu item: where it goes.
pub fn menu_hint(ctx: &egui::Context) -> String {
    let cfg = config(ctx);
    if !cfg.enabled {
        return t!("off");
    }
    let w = crate::ai::where_to(&cfg.provider);
    format!("{} {}", w.icon, w.short)
}

/// A context menu entry for an AI action: the destination on its right,
/// the sentence on hover.
pub fn menu_item(ui: &mut egui::Ui, label: &str) -> bool {
    let hover = if config(ui.ctx()).enabled {
        where_now(ui.ctx()).long
    } else {
        t!("AI is off: this opens the AI panel, where you can turn it on.")
    };
    let hint = menu_hint(ui.ctx());
    let clicked = ui
        .add(
            egui::Button::new(icons::text(icons::AI, label))
                .right_text(RichText::new(hint).weak().size(11.0)),
        )
        .on_hover_text(hover)
        .clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// "AI is off" with the way to turn it on.
pub fn off_card(ui: &mut egui::Ui, cmds: &mut Commands) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(icons::PROHIBIT).size(16.0).color(ui.visuals().weak_text_color()));
            ui.label(theme::strong(t!("AI is off")).size(14.0));
        });
        ui.add_space(theme::SPACE_S);
        ui.add(
            egui::Label::new(t!(
                "Nothing is sent to any model until you turn AI on. Choose a local model (Ollama, LM Studio…) to keep your code on this computer."
            ))
            .wrap(),
        );
        ui.add_space(theme::SPACE_M);
        if ui
            .add(theme::primary(icons::text(icons::SETTINGS, t!("AI settings…"))))
            .clicked()
        {
            cmds.push(Command::Ai(crate::ai::AiCmd::OpenSettings));
        }
    });
}

/// "12 KB sent · 1 line with a secret masked".
pub fn sent_line(ui: &mut egui::Ui, sent: &Sent) {
    let weak = ui.visuals().weak_text_color();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_XS;
        let kb = (sent.bytes as f32 / 1024.0).max(0.1);
        ui.label(
            RichText::new(t!("{} KB sent", format!("{kb:.1}")))
                .color(weak)
                .size(11.0),
        );
        if sent.masked > 0 {
            ui.label(RichText::new("·").color(weak).size(11.0));
            let text = if sent.masked == 1 {
                t!("1 line with a secret masked")
            } else {
                t!("{} lines with secrets masked", sent.masked)
            };
            ui.label(
                RichText::new(icons::text(icons::KEY, text))
                    .color(ui.visuals().warn_fg_color)
                    .size(11.0),
            )
            .on_hover_text(sent.kinds.join(", "));
        }
        if sent.cut {
            ui.label(RichText::new("·").color(weak).size(11.0));
            ui.label(
                RichText::new(t!("cut to the size limit"))
                    .color(weak)
                    .size(11.0),
            )
            .on_hover_text(t!("Change the limit in Settings > AI."));
        }
    });
}

/// The answer so far as Markdown; "Thinking…" while a model reasons.
pub fn answer(
    ui: &mut egui::Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    text: &str,
    running: bool,
) {
    let visible = ai::strip_thinking(text);
    if visible.trim().is_empty() {
        if running {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(if ai::is_thinking(text) {
                    t!("Thinking…")
                } else {
                    t!("Waiting for the model…")
                });
            });
        }
        return;
    }
    egui_commonmark::CommonMarkViewer::new()
        .max_image_width(Some(ui.available_width() as usize))
        .show(ui, cache, &visible);
    if running {
        ui.spinner();
    }
}
