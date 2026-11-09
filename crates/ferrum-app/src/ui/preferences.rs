//! Settings sections for appearance, git and external tools.

use eframe::egui::{self, Color32, RichText};
use ferrum_core::config::GlobalIdentity;

use crate::command::{Command, Commands};
use crate::external::{self, DiffTool, Editor, Terminal};
use crate::prefs::{self, DateFormat, MergeChoice, ToolbarButton};
use crate::settings::Settings;

/// Values loaded when the Git section is first shown.
#[derive(Default)]
pub struct State {
    /// The global identity as saved, and as being edited.
    identity: Option<(GlobalIdentity, GlobalIdentity)>,
    /// The git executable last checked and its version (`None` = not working).
    git_check: Option<(String, Option<String>)>,
    show_log: bool,
}

fn grid(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, add);
}

fn subheading(ui: &mut egui::Ui, text: String) {
    ui.add_space(10.0);
    ui.label(RichText::new(text).strong());
    ui.add_space(4.0);
}

// ----- Appearance -----

pub fn appearance(ui: &mut egui::Ui, settings: &mut Settings) {
    let p = &mut settings.prefs;
    grid(ui, "settings-appearance", |ui| {
        ui.label(t!("Dates"));
        egui::ComboBox::from_id_salt("settings-date")
            .selected_text(p.date_format.label())
            .width(300.0)
            .show_ui(ui, |ui| {
                for f in DateFormat::ALL {
                    ui.selectable_value(&mut p.date_format, f, f.label());
                }
            });
        ui.end_row();

        ui.label(t!("Code font"));
        ui.horizontal(|ui| {
            let builtin = t!("Built-in ({})", crate::fonts::CODE_FONT_NAME);
            let current = if p.code_font.is_empty() {
                builtin.clone()
            } else {
                p.code_font.clone()
            };
            egui::ComboBox::from_id_salt("settings-code-font")
                .selected_text(current)
                .width(220.0)
                .height(320.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut p.code_font, String::new(), builtin);
                    match prefs::monospace_families(ui.ctx()) {
                        Some(list) => {
                            for f in list {
                                ui.selectable_value(&mut p.code_font, f.clone(), f);
                            }
                        }
                        None => {
                            ui.spinner();
                        }
                    }
                });
            ui.add(
                egui::DragValue::new(&mut p.code_font_size)
                    .range(8.0..=20.0)
                    .speed(0.1)
                    .fixed_decimals(1)
                    .suffix(" pt"),
            );
        });
        ui.end_row();

        ui.label(t!("Density"));
        ui.horizontal(|ui| {
            for d in crate::theme::Density::ALL {
                ui.selectable_value(&mut p.density, d, d.label());
            }
        })
        .response
        .on_hover_text(t!("Row heights and padding"));
        ui.end_row();

        ui.label(t!("Tab width"));
        ui.add(egui::Slider::new(&mut p.tab_width, 1..=8));
        ui.end_row();

        ui.label(t!("Accent color"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut p.system_accent, t!("Use the system accent color"));
            if p.system_accent {
                match prefs::system_accent(ui.ctx()) {
                    Some(c) => swatch(ui, c),
                    None => {
                        ui.weak(t!("(not available on this system)"));
                    }
                }
            }
        });
        ui.end_row();
    });

    subheading(ui, t!("Window"));
    ui.checkbox(&mut p.hide_menu_bar, t!("Hide the menu bar"))
        .on_hover_text(t!("F10 and Alt+letter only work while it is shown"));

    subheading(ui, t!("Toolbar buttons"));
    ui.horizontal_wrapped(|ui| {
        for b in ToolbarButton::ALL {
            let mut shown = p.shows(b);
            if ui.checkbox(&mut shown, b.label()).changed() {
                p.hidden_toolbar.retain(|h| *h != b);
                if !shown {
                    p.hidden_toolbar.push(b);
                }
            }
        }
    });
}

fn swatch(ui: &mut egui::Ui, c: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 4.0, c);
}

// ----- Git -----

pub fn git(ui: &mut egui::Ui, state: &mut State, settings: &mut Settings, cmds: &mut Commands) {
    subheading(ui, t!("Global identity"));
    ui.weak(t!(
        "Used for commits in repositories without their own identity (git config --global)."
    ));
    ui.add_space(6.0);
    let (saved, edit) = state.identity.get_or_insert_with(|| {
        let id =
            ferrum_core::config::global_identity(&ferrum_core::cli::Git::new(std::env::temp_dir()));
        (id.clone(), id)
    });
    ui.horizontal(|ui| {
        avatar(ui, &edit.name, &edit.email, 44.0);
        ui.add_space(8.0);
        grid(ui, "settings-global-identity", |ui| {
            ui.label(t!("Name"));
            ui.add(
                egui::TextEdit::singleline(&mut edit.name)
                    .hint_text(t!("Your Name"))
                    .desired_width(260.0),
            );
            ui.end_row();
            ui.label(t!("Email"));
            ui.add(
                egui::TextEdit::singleline(&mut edit.email)
                    .hint_text("you@example.com")
                    .desired_width(260.0),
            );
            ui.end_row();
        });
    });
    let changed = saved != edit;
    let valid = edit.email.trim().is_empty() || edit.email.contains('@');
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                changed && valid,
                egui::Button::new(t!("Save global identity")),
            )
            .clicked()
        {
            cmds.push(Command::SetGlobalIdentity(edit.clone()));
            *saved = edit.clone();
        }
        if changed && ui.button(t!("Revert")).clicked() {
            *edit = saved.clone();
        }
    });

    let p = &mut settings.prefs;
    subheading(ui, t!("Git executable"));
    grid(ui, "settings-git", |ui| {
        ui.label(t!("Program"));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut p.git_path)
                    .hint_text(t!("git (from the PATH)"))
                    .desired_width(260.0),
            );
            if ui.button(t!("Browse…")).clicked()
                && let Some(f) = rfd::FileDialog::new().pick_file()
            {
                p.git_path = f.display().to_string();
            }
        });
        ui.end_row();

        ui.label(t!("Version"));
        ui.horizontal(|ui| {
            let checked = state
                .git_check
                .as_ref()
                .filter(|(path, _)| *path == p.git_path);
            match checked {
                Some((_, Some(v))) => {
                    ui.label(v);
                }
                Some((_, None)) => {
                    ui.colored_label(ui.visuals().error_fg_color, t!("not found"));
                }
                None => {
                    ui.weak("—");
                }
            }
            if ui.small_button(t!("Check")).clicked() || state.git_check.is_none() {
                // Uses the program from the field, not the one applied last frame.
                let path = p.git_path.trim();
                let program = if path.is_empty() { "git" } else { path };
                let version = ferrum_core::cli::Git::new(std::env::temp_dir())
                    .with_program(program)
                    .run(&["--version"])
                    .ok()
                    .map(|v| v.trim().trim_start_matches("git version ").to_owned());
                state.git_check = Some((p.git_path.clone(), version));
            }
        });
        ui.end_row();

        ui.label("");
        ui.checkbox(&mut p.verbose_git, t!("Verbose git output"))
            .on_hover_text(t!(
                "Keep every git command and its output, and show the command in error messages"
            ));
        ui.end_row();
    });
    if p.verbose_git {
        ui.horizontal(|ui| {
            ui.toggle_value(&mut state.show_log, t!("Show recent git commands"));
            if state.show_log && ui.small_button(t!("Clear")).clicked() {
                ferrum_core::cli::clear_log();
            }
        });
        if state.show_log {
            git_log(ui);
        }
    }

    subheading(ui, t!("Branches and merges"));
    grid(ui, "settings-branches", |ui| {
        ui.label(t!("Spaces in new branch names"));
        let current = p.branch_space.clone();
        let label = |s: &str| match s {
            "" => t!("Keep (git refuses them)"),
            "-" => t!("Replace with -"),
            "_" => t!("Replace with _"),
            other => other.to_owned(),
        };
        egui::ComboBox::from_id_salt("settings-branch-space")
            .selected_text(label(&current))
            .show_ui(ui, |ui| {
                for s in ["-", "_", ""] {
                    ui.selectable_value(&mut p.branch_space, s.to_owned(), label(s));
                }
            });
        ui.end_row();

        ui.label(t!("Default merge type"));
        egui::ComboBox::from_id_salt("settings-merge")
            .selected_text(p.merge.label())
            .width(240.0)
            .show_ui(ui, |ui| {
                for m in MergeChoice::ALL {
                    ui.selectable_value(&mut p.merge, m, m.label());
                }
            });
        ui.end_row();

        ui.label(t!("Local changes on checkout"));
        ui.checkbox(
            &mut p.ask_local_changes,
            t!("Always ask to stash or discard them"),
        )
        .on_hover_text(t!(
            "Otherwise Ferrum only asks when the checkout would overwrite them"
        ));
        ui.end_row();
    });
}

fn git_log(ui: &mut egui::Ui) {
    let log = ferrum_core::cli::recent_log();
    if log.is_empty() {
        ui.weak(t!("No git commands yet."));
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("settings-git-log")
        .max_height(220.0)
        .show(ui, |ui| {
            for e in log.iter().rev() {
                let color = if e.success {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().error_fg_color
                };
                ui.label(
                    RichText::new(format!("$ {}", e.command))
                        .monospace()
                        .color(color),
                )
                .on_hover_text(e.dir.display().to_string());
                if !e.output.is_empty() {
                    ui.label(RichText::new(&e.output).monospace().small().weak());
                }
                ui.add_space(4.0);
            }
        });
}

/// A round avatar with the person's initials, colored from the email.
pub fn avatar(ui: &mut egui::Ui, name: &str, email: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let hash = email
        .trim()
        .to_lowercase()
        .bytes()
        .fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    let hue = (hash % 360) as f32 / 360.0;
    let fill: Color32 = egui::ecolor::Hsva::new(hue, 0.45, 0.7, 1.0).into();
    ui.painter().circle_filled(rect.center(), size / 2.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials(name, email),
        egui::FontId::proportional(size * 0.4),
        Color32::WHITE,
    );
}

/// "Ada Lovelace" -> "AL"; falls back to the email's first letter.
fn initials(name: &str, email: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let first = |w: &str| w.chars().next().map(|c| c.to_uppercase().to_string());
    match words.as_slice() {
        [] => email
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".into()),
        [one] => first(one).unwrap_or_default(),
        [a, .., b] => format!(
            "{}{}",
            first(a).unwrap_or_default(),
            first(b).unwrap_or_default()
        ),
    }
}

// ----- Tools -----

pub fn tools(ui: &mut egui::Ui, settings: &mut Settings) {
    let x = &mut settings.external;
    ui.weak(t!(
        "Programs used by \"Open in…\" and the external diff and merge commands."
    ));
    ui.add_space(6.0);
    grid(ui, "settings-tools", |ui| {
        ui.label(t!("Editor"));
        egui::ComboBox::from_id_salt("settings-editor")
            .selected_text(x.editor.label())
            .width(300.0)
            .show_ui(ui, |ui| {
                for e in Editor::ALL {
                    ui.selectable_value(&mut x.editor, e, e.label());
                }
            });
        ui.end_row();
        if x.editor == Editor::Custom {
            custom_row(ui, &mut x.editor_command, external::EDITOR_HINT);
        }

        ui.label(t!("Terminal"));
        egui::ComboBox::from_id_salt("settings-terminal")
            .selected_text(x.terminal.label())
            .width(300.0)
            .show_ui(ui, |ui| {
                for term in Terminal::choices() {
                    ui.selectable_value(&mut x.terminal, term, term.label());
                }
            });
        ui.end_row();
        if x.terminal == Terminal::Custom {
            custom_row(ui, &mut x.terminal_command, external::TERMINAL_HINT);
        }

        ui.label(t!("Diff tool"));
        tool_combo(ui, "settings-diff-tool", &mut x.diff_tool);
        ui.end_row();
        if x.diff_tool == DiffTool::Custom {
            custom_row(ui, &mut x.diff_command, external::DIFF_HINT);
        }

        ui.label(t!("Merge tool"));
        tool_combo(ui, "settings-merge-tool", &mut x.merge_tool);
        ui.end_row();
        if x.merge_tool == DiffTool::Custom {
            custom_row(ui, &mut x.merge_command, external::MERGE_HINT);
        }
    });
    ui.add_space(10.0);
    ui.weak(t!("Custom commands can use $FILE, $LINE and $DIR (editor, terminal), $LOCAL and $REMOTE (diff), and $BASE, $LOCAL, $REMOTE and $MERGED (merge). Quote words with spaces."));
}

fn tool_combo(ui: &mut egui::Ui, id: &str, tool: &mut DiffTool) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(tool.label())
        .width(300.0)
        .show_ui(ui, |ui| {
            for d in DiffTool::ALL {
                ui.selectable_value(tool, d, d.label());
            }
        });
}

fn custom_row(ui: &mut egui::Ui, command: &mut String, hint: &str) {
    ui.label("");
    ui.add(
        egui::TextEdit::singleline(command)
            .hint_text(hint)
            .font(egui::TextStyle::Monospace)
            .desired_width(300.0),
    );
    ui.end_row();
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn makes_initials() {
        assert_eq!(initials("Ada Lovelace", ""), "AL");
        assert_eq!(initials("ada b. lovelace", ""), "AL");
        assert_eq!(initials("Ada", ""), "A");
        assert_eq!(initials("", "zoe@x.org"), "Z");
        assert_eq!(initials("", ""), "?");
    }
}
