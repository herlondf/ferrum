//! Settings > AI: turning AI on, the provider (Ollama, an OpenAI-compatible
//! server or a command line tool), its model, a connection test, where
//! data goes, and the commit message prompt.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};
use ferrum_core::ai::{self, Check, Provider};

use crate::commit_prefs::{AiProvider, DEFAULT_OLLAMA_URL, DEFAULT_OPENAI_URL};
use crate::hosted::Pending;
use crate::icons;
use crate::settings::Settings;
use crate::theme;

/// The connection test and the model list it found.
#[derive(Default)]
struct Probe {
    running: Option<Pending<Check>>,
    /// The provider tested, so results of an old one are not shown.
    tested: Option<Provider>,
    result: Option<Result<Check, String>>,
}

fn probe(ctx: &egui::Context) -> Arc<Mutex<Probe>> {
    let id = egui::Id::unique("ai-settings-probe");
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Arc<Mutex<Probe>>>(id).clone())
}

fn start_test(ctx: &egui::Context, probe: &mut Probe, provider: Provider) {
    let p = provider.clone();
    probe.running = Some(Pending::spawn(ctx, move || {
        ai::test(&p).map_err(|e| crate::ai::error_text(&e))
    }));
    probe.tested = Some(provider);
    probe.result = None;
}

pub fn section(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.label(theme::strong(t!("AI")).size(16.0));
    ui.add_space(theme::SPACE_S);
    ui.checkbox(&mut settings.ai.enabled, t!("Use AI features"))
        .on_hover_text(t!("Explain, conflict help, natural-language search, split into commits, pull request descriptions and commit messages"));
    ui.weak(t!("Nothing is sent to any model until this is on."));
    ui.add_space(theme::SPACE_L);

    ui.label(theme::strong(t!("Provider")));
    ui.add_space(theme::SPACE_XS);
    let p = &mut settings.ai;
    let choices = [
        (
            AiProvider::Ollama,
            icons::LOCK,
            t!("Ollama"),
            t!("Local models on this computer"),
        ),
        (
            AiProvider::OpenAi,
            icons::DESKTOP,
            t!("OpenAI-compatible server"),
            t!("LM Studio, llama.cpp server, vLLM… or a cloud API"),
        ),
        (
            AiProvider::Command,
            icons::TERMINAL_WINDOW,
            t!("Command line tool"),
            t!("Claude Code, Codex… (usually cloud)"),
        ),
    ];
    let width = ui.available_width().min(520.0);
    for (kind, icon, name, hint) in choices {
        let on = p.provider == kind;
        let text = format!("{icon}  {name}");
        if ui
            .add(
                egui::Button::selectable(on, text)
                    .right_text(RichText::new(hint).weak().size(11.0))
                    .min_size(egui::vec2(width, 28.0)),
            )
            .clicked()
        {
            p.provider = kind;
        }
    }
    ui.add_space(theme::SPACE_M);

    let probe = probe(ui.ctx());
    let mut probe = probe.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = crate::hosted::take_done(&mut probe.running) {
        probe.result = Some(r);
    }
    let models: Vec<String> = match (&probe.result, &probe.tested) {
        (Some(Ok(Check::Models(m))), Some(t)) if *t == crate::ai::provider(p) => m.clone(),
        _ => Vec::new(),
    };
    egui::Grid::new("ai-provider")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| match p.provider {
            AiProvider::Ollama => {
                ui.label(t!("Server"));
                url_field(ui, &mut p.ollama_url, DEFAULT_OLLAMA_URL);
                ui.end_row();
                ui.label(t!("Model"));
                model_field(ui, &mut p.ollama_model, &models, "llama3.1:8b");
                ui.end_row();
            }
            AiProvider::OpenAi => {
                ui.label(t!("Base URL"));
                url_field(ui, &mut p.openai_url, DEFAULT_OPENAI_URL);
                ui.end_row();
                ui.label(t!("API key"));
                ui.add(
                    egui::TextEdit::singleline(&mut p.openai_key)
                        .password(true)
                        .hint_text(t!("optional"))
                        .desired_width(320.0),
                );
                ui.end_row();
                ui.label(t!("Model"));
                model_field(ui, &mut p.openai_model, &models, "qwen2.5-coder-7b-instruct");
                ui.end_row();
            }
            AiProvider::Command => {
                ui.label(t!("Command"));
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut p.command)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(320.0),
                    );
                    ui.horizontal(|ui| {
                        ui.weak(t!("Examples:"));
                        for example in ["claude -p", "codex exec -", "ollama run llama3.1"] {
                            if ui.small_button(example).clicked() {
                                p.command = example.into();
                            }
                        }
                    });
                    ui.weak(t!("It gets the prompt on standard input and runs in a temporary folder, not in the repository."));
                });
                ui.end_row();
            }
        });
    ui.add_space(theme::SPACE_S);
    let provider = crate::ai::provider(p);
    ui.horizontal(|ui| {
        if probe.running.is_some() {
            ui.spinner();
            ui.label(t!("Testing…"));
        } else if ui
            .button(icons::text(icons::PULSE, t!("Test connection")))
            .on_hover_text(t!(
                "Checks that the provider answers, without sending any of your code"
            ))
            .clicked()
        {
            start_test(ui.ctx(), &mut probe, provider.clone());
        }
        if probe.tested.as_ref() == Some(&provider) {
            match &probe.result {
                Some(Ok(Check::Models(m))) if m.is_empty() => {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        t!("Connected, but the server has no models."),
                    );
                }
                Some(Ok(Check::Models(m))) => {
                    let text = if m.len() == 1 {
                        t!("Connected · 1 model")
                    } else {
                        t!("Connected · {} models", m.len())
                    };
                    ui.colored_label(
                        crate::util::added_color(ui),
                        icons::text(icons::CHECK, text),
                    );
                }
                Some(Ok(Check::Program(path))) => {
                    ui.colored_label(
                        crate::util::added_color(ui),
                        icons::text(icons::CHECK, t!("Found {}", path)),
                    );
                }
                Some(Err(e)) => {
                    ui.add(
                        egui::Label::new(
                            RichText::new(icons::text(icons::WARNING, e))
                                .color(ui.visuals().error_fg_color),
                        )
                        .wrap(),
                    );
                }
                None => {}
            }
        }
    });
    ui.add_space(theme::SPACE_M);
    destination(ui, &provider);
    drop(probe);

    ui.add_space(theme::SPACE_L);
    ui.label(theme::strong(t!("Privacy")));
    ui.add_space(theme::SPACE_XS);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(icons::KEY).color(ui.visuals().weak_text_color()));
        ui.label(t!("Lines the secret detector flags (Settings > Safety) are masked before anything is sent."));
    });
    ui.horizontal(|ui| {
        ui.label(t!("Context sent per request, at most"));
        ui.add(
            egui::DragValue::new(&mut settings.ai.max_kb)
                .range(4..=2048)
                .suffix(" KB"),
        );
    });
    ui.add_space(theme::SPACE_XL);
    super::commit_settings::ai(ui, settings);
}

fn url_field(ui: &mut egui::Ui, url: &mut String, default: &str) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(url)
                .font(egui::TextStyle::Monospace)
                .hint_text(default)
                .desired_width(320.0),
        );
        if url != default && ui.small_button(t!("Default")).clicked() {
            *url = default.into();
        }
    });
}

fn model_field(ui: &mut egui::Ui, model: &mut String, models: &[String], example: &str) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(model)
                .hint_text(example)
                .desired_width(250.0),
        );
        let resp = egui::ComboBox::from_id_salt("ai-models")
            .selected_text("")
            .width(24.0)
            .show_ui(ui, |ui| {
                if models.is_empty() {
                    ui.weak(t!("Test the connection to list the models."));
                }
                for m in models {
                    ui.selectable_value(model, m.clone(), m);
                }
            });
        resp.response.on_hover_text(t!("Models of the server"));
    });
}

/// Where the data goes with this provider, in a colored line.
fn destination(ui: &mut egui::Ui, provider: &Provider) {
    let w = crate::ai::where_to(provider);
    let color = if w.local {
        crate::util::added_color(ui)
    } else {
        ui.visuals().warn_fg_color
    };
    egui::Frame::new()
        .fill(color.gamma_multiply(0.10))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(w.icon).color(color));
                ui.label(RichText::new(&w.long).color(color));
            });
        });
}
