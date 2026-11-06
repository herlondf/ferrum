//! Settings sections for committing and for AI commit messages.

use eframe::egui;

use crate::commit_prefs::{AiPrefs, DEFAULT_AI_PROMPT};
use crate::settings::Settings;

pub fn commit(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.label(egui::RichText::new(t!("Commit")).size(16.0).strong());
    ui.add_space(6.0);
    let p = &mut settings.commit;
    egui::Grid::new("settings-commit")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            ui.label(t!("Subject soft limit"));
            ui.add(egui::DragValue::new(&mut p.subject_soft_limit).range(0..=200))
                .on_hover_text(t!(
                    "The length counter turns yellow after this many characters"
                ));
            ui.end_row();

            ui.label(t!("Subject hard limit"));
            ui.add(egui::DragValue::new(&mut p.subject_hard_limit).range(1..=200))
                .on_hover_text(t!("Characters after this are marked in red"));
            ui.end_row();

            ui.label(t!("Description ruler"));
            ui.add(egui::DragValue::new(&mut p.ruler_column).range(0..=200))
                .on_hover_text(t!("Column of the ruler and of \"Wrap\" (0 hides them)"));
            ui.end_row();

            ui.label(t!("Large file warning"));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut p.large_file_mb).range(0..=10_000));
                ui.label("MB");
            })
            .response
            .on_hover_text(t!(
                "Warn about files bigger than this that are not in Git LFS (0 turns it off)"
            ));
            ui.end_row();
        });
    ui.add_space(6.0);
    ui.checkbox(&mut p.signoff, t!("Sign off commits (--signoff)"));
    ui.checkbox(&mut p.push_after_commit, t!("Push after every commit"));
    ui.add_space(6.0);
    let n = p.recent_messages.len();
    if ui
        .add_enabled(
            n > 0,
            egui::Button::new(t!("Forget recent messages ({})", n)),
        )
        .clicked()
    {
        p.recent_messages.clear();
    }
}

pub fn ai(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.label(
        egui::RichText::new(t!("AI commit messages"))
            .size(16.0)
            .strong(),
    );
    ui.add_space(6.0);
    ui.label(t!(
        "The AI button of the commit box runs this command with the prompt on standard input and uses its answer as the message."
    ));
    ui.add_space(6.0);
    let p: &mut AiPrefs = &mut settings.ai;
    ui.label(t!("Command"));
    ui.add(
        egui::TextEdit::singleline(&mut p.command)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY),
    );
    ui.horizontal(|ui| {
        ui.weak(t!("Examples:"));
        for example in ["claude -p", "codex exec -"] {
            if ui.small_button(example).clicked() {
                p.command = example.into();
            }
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(t!("Prompt"));
        ui.weak(t!("{diff} is replaced by the staged changes"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    p.prompt != DEFAULT_AI_PROMPT,
                    egui::Button::new(t!("Restore default")).small(),
                )
                .clicked()
            {
                p.prompt = DEFAULT_AI_PROMPT.into();
            }
        });
    });
    ui.add(
        egui::TextEdit::multiline(&mut p.prompt)
            .desired_rows(8)
            .desired_width(f32::INFINITY),
    );
}
