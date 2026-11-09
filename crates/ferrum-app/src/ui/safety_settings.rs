//! Settings > Safety: restore points, secret detection and learning mode.

use eframe::egui::{self, RichText};
use ferrum_core::secrets::{self, CustomPattern};

use crate::command::{Command, Commands};
use crate::safety::SafetyCmd;
use crate::settings::Settings;
use crate::theme;

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(theme::SPACE_M);
    ui.label(theme::strong(text).size(14.0));
    ui.add_space(theme::SPACE_XS);
}

fn hint(ui: &mut egui::Ui, text: &str) {
    let weak = ui.visuals().weak_text_color();
    ui.add(egui::Label::new(RichText::new(text).size(12.0).color(weak)).wrap());
}

pub fn section(ui: &mut egui::Ui, settings: &mut Settings, cmds: &mut Commands) {
    ui.label(theme::strong(t!("Safety")).size(16.0));
    let s = &mut settings.safety;

    heading(ui, &t!("Restore points"));
    ui.checkbox(
        &mut s.restore_points,
        t!("Save a restore point before operations that rewrite or drop work"),
    );
    hint(
        ui,
        &t!(
            "Rebase, reset, merge, cherry-pick, amend, discard, stash drop, branch delete, force push and history rewrites. Points live in hidden refs (refs/ferrum/restore) and are never pushed."
        ),
    );
    ui.add_enabled_ui(s.restore_points, |ui| {
        ui.horizontal(|ui| {
            ui.label(t!("Keep them for"));
            ui.add(egui::DragValue::new(&mut s.keep_days).range(1..=365));
            ui.label(t!("days"));
        });
        ui.checkbox(
            &mut s.untracked,
            t!("Include untracked files (up to 100 MB)"),
        );
    });
    if ui.button(t!("Open the timeline")).clicked() {
        cmds.push(Command::Safety(SafetyCmd::ToggleTimeline));
    }

    heading(ui, &t!("Secret detection"));
    ui.checkbox(
        &mut s.secrets,
        t!("Scan staged changes for secrets before committing"),
    );
    ui.add_enabled_ui(s.secrets, |ui| {
        ui.checkbox(&mut s.scan_push, t!("Scan the commits before pushing"));
        let r = &mut s.secret_rules;
        ui.checkbox(&mut r.entropy, t!("Look for random-looking strings (high entropy)"));
        ui.checkbox(&mut r.sensitive_files, t!("Warn about .env, key and credential files"));
        egui::CollapsingHeader::new(t!("Built-in rules"))
            .id_salt("secret-rules")
            .show(ui, |ui| {
                for (id, name) in secrets::builtin_rules() {
                    let mut on = !r.disabled.iter().any(|d| d == id);
                    let label = crate::ui::secrets::builtin_name(id, name);
                    if ui.checkbox(&mut on, label).changed() {
                        r.disabled.retain(|d| d != id);
                        if !on {
                            r.disabled.push(id.to_owned());
                        }
                    }
                }
            });
        egui::CollapsingHeader::new(t!("Custom patterns"))
            .id_salt("secret-custom")
            .default_open(!r.custom.is_empty())
            .show(ui, |ui| {
                hint(ui, &t!("Regular expressions; the first group (or the whole match) is the secret."));
                let mut remove = None;
                for (i, p) in r.custom.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut p.name)
                                .hint_text(t!("Name"))
                                .desired_width(120.0),
                        );
                        let bad = !p.regex.is_empty() && regex::Regex::new(&p.regex).is_err();
                        let mut edit = egui::TextEdit::singleline(&mut p.regex)
                            .hint_text("ACME-[0-9]{6}")
                            .font(egui::TextStyle::Monospace)
                            .desired_width(ui.available_width() - 34.0);
                        if bad {
                            edit = edit.text_color(ui.visuals().error_fg_color);
                        }
                        ui.add(edit);
                        if ui.small_button(crate::icons::TRASH).on_hover_text(t!("Remove")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    r.custom.remove(i);
                }
                if ui.button(t!("Add pattern")).clicked() {
                    r.custom.push(CustomPattern::default());
                }
            });
        egui::CollapsingHeader::new(t!("Allowlists"))
            .id_salt("secret-allow")
            .show(ui, |ui| {
                hint(ui, &t!("Values accepted with “Add to allowlist”, and path patterns (one per line, e.g. tests/fixtures/** or *.pem) skipped, per repository."));
                if s.allowlists.is_empty() {
                    ui.label(RichText::new(t!("Nothing allowed yet.")).color(ui.visuals().weak_text_color()));
                }
                let mut clear = None;
                for (key, list) in s.allowlists.iter_mut() {
                    let name = key.trim_start_matches("common:");
                    ui.add_space(theme::SPACE_S);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).font(crate::util::code_font(12.0)));
                        ui.label(
                            RichText::new(t!("{} values", list.secrets.len()))
                                .color(ui.visuals().weak_text_color()),
                        );
                        if ui.small_button(t!("Clear")).clicked() {
                            clear = Some(key.clone());
                        }
                    });
                    let mut text = list.paths.join("\n");
                    let resp = ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .hint_text(t!("Paths to skip"))
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    );
                    if resp.changed() {
                        list.paths = text.split('\n').map(str::to_owned).collect();
                    }
                }
                if let Some(k) = clear {
                    s.allowlists.remove(&k);
                }
            });
    });

    heading(ui, &t!("Learning mode"));
    ui.checkbox(
        &mut s.learning,
        t!("Show the git commands of each action, with a short explanation"),
    );
    hint(
        ui,
        &t!(
            "A panel lists the commands as they run, and button tooltips show the command they use."
        ),
    );
}
