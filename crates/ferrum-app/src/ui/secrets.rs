//! The warning shown when a commit or push would add secrets: the lines
//! found (masked), and Unstage / Ignore once / Add to allowlist / Cancel.

use eframe::egui::{self, RichText};

use crate::command::{Command, Commands};
use crate::icons;
use crate::safety::{Held, SafetyCmd};
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Unstage the files with findings; the commit does not run.
    Unstage,
    /// Run the held operations this time.
    IgnoreOnce,
    /// Remember the findings as accepted, then run.
    Allow,
    Cancel,
}

fn answer(cmds: &mut Commands, a: Answer) {
    cmds.push(Command::Safety(SafetyCmd::Secrets(a)));
}

/// Draws the warning for `held`.
pub fn show(ctx: &egui::Context, held: &Held, cmds: &mut Commands) {
    let modal = egui::Modal::new(egui::Id::unique("secrets-warning"))
        .frame(theme::dialog_frame(&ctx.global_style()))
        .show(ctx, |ui| {
            ui.set_width(640.0);
            let v = ui.visuals().clone();
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::WARNING).size(22.0).color(v.warn_fg_color));
                ui.vertical(|ui| {
                    let n = held.findings.len();
                    let title = if n == 1 {
                        t!("Possible secret found")
                    } else {
                        t!("{} possible secrets found", n)
                    };
                    ui.label(theme::strong(title).size(16.0));
                    let hint = if held.push {
                        t!("These lines in the commits to push look like passwords, keys or tokens. Once pushed, others can read them.")
                    } else {
                        t!("These staged lines look like passwords, keys or tokens. Once committed, they stay in the history.")
                    };
                    ui.add(egui::Label::new(RichText::new(hint).color(v.weak_text_color())).wrap());
                });
            });
            ui.add_space(theme::SPACE_M);
            theme::card(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt("secret-findings")
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for f in &held.findings {
                            finding(ui, f);
                        }
                    });
            });
            ui.add_space(theme::SPACE_S);
            ui.label(
                RichText::new(t!("Add “ferrum:allow” to a line to accept it there."))
                    .size(11.5)
                    .color(v.weak_text_color()),
            );
            ui.add_space(theme::SPACE_M);
            ui.horizontal(|ui| {
                if ui.add(theme::secondary(t!("Cancel"))).clicked() {
                    answer(cmds, Answer::Cancel);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Push: nothing to unstage, so going on is the main action.
                    let go = if held.push { t!("Push anyway") } else { t!("Ignore once") };
                    if held.push {
                        if ui.add(theme::danger(go)).clicked() {
                            answer(cmds, Answer::IgnoreOnce);
                        }
                    } else {
                        let unstage = ui
                            .add(theme::primary(t!("Unstage")))
                            .on_hover_text(t!("Unstage the files with secrets and do not commit"));
                        if unstage.clicked() {
                            answer(cmds, Answer::Unstage);
                        }
                        if ui.add(theme::secondary(go)).clicked() {
                            answer(cmds, Answer::IgnoreOnce);
                        }
                    }
                    let allow = ui
                        .add(theme::secondary(t!("Add to allowlist")))
                        .on_hover_text(t!("Accept these values in this repository from now on, then continue"));
                    if allow.clicked() {
                        answer(cmds, Answer::Allow);
                    }
                });
            });
        });
    if modal.should_close() {
        answer(cmds, Answer::Cancel);
    }
}

fn finding(ui: &mut egui::Ui, f: &ferrum_core::secrets::Finding) {
    let v = ui.visuals().clone();
    ui.horizontal(|ui| {
        egui::Frame::new()
            .fill(v.warn_fg_color.gamma_multiply(0.18))
            .corner_radius(4)
            .inner_margin(egui::Margin::symmetric(6, 1))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(rule_name(f))
                        .size(11.5)
                        .color(v.strong_text_color()),
                );
            });
        let place = if f.line > 0 {
            format!("{}:{}", f.path, f.line)
        } else {
            f.path.clone()
        };
        ui.label(RichText::new(place).font(crate::util::code_font(12.0)));
        if let Some(c) = f.commit {
            let id = c.to_string();
            ui.label(
                RichText::new(&id[..7])
                    .font(crate::util::code_font(11.5))
                    .color(v.weak_text_color()),
            );
        }
    });
    if !f.text.is_empty() {
        ui.add(
            egui::Label::new(
                RichText::new(&f.text)
                    .font(crate::util::code_font(11.5))
                    .color(v.weak_text_color()),
            )
            .truncate(),
        );
    }
    ui.add_space(theme::SPACE_S);
}

/// The rule's name in the user's language.
pub fn rule_name(f: &ferrum_core::secrets::Finding) -> String {
    builtin_name(&f.rule, &f.name)
}

/// The name of a rule by id; custom rules keep their own name.
pub fn builtin_name(id: &str, name: &str) -> String {
    match id {
        "aws-access-key" => t!("AWS access key"),
        "aws-secret-key" => t!("AWS secret key"),
        "github-token" => t!("GitHub token"),
        "github-pat" => t!("GitHub fine-grained token"),
        "gitlab-token" => t!("GitLab token"),
        "slack-token" => t!("Slack token"),
        "slack-webhook" => t!("Slack webhook"),
        "stripe-key" => t!("Stripe key"),
        "google-api-key" => t!("Google API key"),
        "google-oauth-secret" => t!("Google OAuth client secret"),
        "openai-key" => t!("OpenAI API key"),
        "anthropic-key" => t!("Anthropic API key"),
        "private-key" => t!("Private key"),
        "jwt" => t!("JSON Web Token"),
        "npm-token" => t!("npm token"),
        "pypi-token" => t!("PyPI token"),
        "sendgrid-key" => t!("SendGrid key"),
        "huggingface-token" => t!("Hugging Face token"),
        "digitalocean-token" => t!("DigitalOcean token"),
        "azure-storage-key" => t!("Azure storage key"),
        "url-password" => t!("Password in a URL"),
        "secret-assignment" => t!("Hard-coded secret"),
        "entropy" => t!("High-entropy string"),
        "sensitive-file" => match name {
            "Environment file" => t!("Environment file"),
            "Key file" => t!("Key file"),
            _ => t!("Credentials file"),
        },
        _ => name.to_owned(),
    }
}
