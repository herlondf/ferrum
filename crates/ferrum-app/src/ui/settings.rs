//! Settings window: general options, identity profiles, forge accounts,
//! keyboard shortcuts and version information.

use eframe::egui::{self, RichText};

use crate::command::{Command, Commands, Op};
use crate::settings::{PullChoice, Settings, ThemeChoice};
use crate::ui::dialogs::dirs_home;

pub(crate) const REPO_URL: &str = "https://github.com/herlondf/ferrum";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Section {
    #[default]
    General,
    Appearance,
    Git,
    Tools,
    Profiles,
    Commit,
    Ai,
    Accounts,
    Ssh,
    Shortcuts,
    Commands,
    About,
}

impl Section {
    const ALL: [Section; 12] = [
        Section::General,
        Section::Appearance,
        Section::Git,
        Section::Tools,
        Section::Profiles,
        Section::Commit,
        Section::Ai,
        Section::Accounts,
        Section::Ssh,
        Section::Shortcuts,
        Section::Commands,
        Section::About,
    ];

    fn label(self) -> String {
        match self {
            Section::General => t!("⚙ General"),
            Section::Appearance => t!("🎨 Appearance"),
            Section::Git => t!("🔧 Git"),
            Section::Tools => t!("🛠 Tools"),
            Section::Profiles => t!("👤 Profiles"),
            Section::Commit => t!("✔ Commit"),
            Section::Ai => t!("💡 AI"),
            Section::Accounts => t!("☁ Accounts"),
            Section::Ssh => t!("🔐 SSH keys"),
            Section::Shortcuts => t!("⌘ Shortcuts"),
            Section::Commands => t!("▶ Custom commands"),
            Section::About => t!("📝 About"),
        }
    }
}

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    section: Section,
    /// The profile being edited.
    profile: Option<usize>,
    /// Output of `git --version`, looked up the first time About is shown
    /// (`None` inside when git is not found).
    git_version: Option<Option<String>>,
    prefs: crate::ui::preferences::State,
    accounts: crate::ui::accounts::State,
}

impl SettingsWindow {
    /// Shows a section by its English name, e.g. "appearance" (for `FERRUM_START`).
    pub fn select(&mut self, name: &str) {
        let name = name.to_lowercase();
        self.section = match name.as_str() {
            "appearance" => Section::Appearance,
            "git" => Section::Git,
            "tools" => Section::Tools,
            "profiles" => Section::Profiles,
            "accounts" => Section::Accounts,
            "ssh" => Section::Ssh,
            "shortcuts" => Section::Shortcuts,
            "commands" => Section::Commands,
            "about" => Section::About,
            _ => Section::General,
        };
    }
}

pub fn show(
    ctx: &egui::Context,
    win: &mut SettingsWindow,
    settings: &mut Settings,
    cmds: &mut Commands,
) {
    let mut open = win.open;
    egui::Window::new(t!("Settings"))
        .id(egui::Id::unique("settings-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([680.0, 440.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .min_size([520.0, 300.0])
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(130.0);
                    for s in Section::ALL {
                        let selected = win.section == s;
                        if ui
                            .add(
                                egui::Button::selectable(selected, s.label())
                                    .right_text("")
                                    .min_size(egui::vec2(130.0, 26.0)),
                            )
                            .clicked()
                        {
                            win.section = s;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| match win.section {
                            Section::General => general(ui, settings),
                            Section::Appearance => {
                                heading(ui, &t!("Appearance"));
                                crate::ui::preferences::appearance(ui, settings);
                            }
                            Section::Git => {
                                heading(ui, "Git");
                                crate::ui::preferences::git(ui, &mut win.prefs, settings, cmds);
                                crate::ui::submodules::settings_block(ui, settings);
                            }
                            Section::Tools => {
                                heading(ui, &t!("External tools"));
                                crate::ui::preferences::tools(ui, settings);
                                crate::ui::smart_merge::settings_block(ui, &mut settings.diff);
                            }
                            Section::Profiles => profiles(ui, win, settings, cmds),
                            Section::Commit => super::commit_settings::commit(ui, settings),
                            Section::Ai => super::commit_settings::ai(ui, settings),
                            Section::Accounts => {
                                crate::ui::accounts::accounts(ui, &mut win.accounts, settings, cmds)
                            }
                            Section::Ssh => {
                                crate::ui::accounts::ssh_keys(ui, &mut win.accounts, settings, cmds)
                            }
                            Section::Shortcuts => crate::ui::shortcuts::section(ui, settings),
                            Section::Commands => {
                                crate::ui::custom_commands::settings_section(ui, settings, cmds)
                            }
                            Section::About => about(ui, win, cmds),
                        });
                });
            });
        });
    win.open = open;
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(crate::theme::strong(text).size(16.0));
    ui.add_space(6.0);
}

fn general(ui: &mut egui::Ui, settings: &mut Settings) {
    heading(ui, &t!("General"));
    egui::Grid::new("settings-general")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            ui.label(t!("Language"));
            egui::ComboBox::from_id_salt("settings-language")
                .selected_text(settings.language.native_name())
                .show_ui(ui, |ui| {
                    for lang in crate::i18n::Language::ALL {
                        ui.selectable_value(&mut settings.language, lang, lang.native_name());
                    }
                });
            ui.end_row();

            ui.label(t!("Theme"));
            ui.horizontal(|ui| {
                for t in ThemeChoice::ALL {
                    ui.selectable_value(&mut settings.theme, t, t.label());
                }
            });
            ui.end_row();

            ui.label(t!("Zoom"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::Slider::new(&mut settings.zoom, 0.7..=1.6)
                        .step_by(0.05)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                );
                if ui
                    .add_enabled(settings.zoom != 1.0, egui::Button::new(t!("Reset zoom")))
                    .clicked()
                {
                    settings.zoom = 1.0;
                }
            });
            ui.end_row();

            ui.label("Pull");
            let pull_tip = t!("How the toolbar's Pull button integrates remote changes");
            egui::ComboBox::from_id_salt("settings-pull")
                .selected_text(pull_label(settings.pull))
                .width(200.0)
                .show_ui(ui, |ui| {
                    for p in [
                        PullChoice::Default,
                        PullChoice::Merge,
                        PullChoice::Rebase,
                        PullChoice::FastForwardOnly,
                    ] {
                        ui.selectable_value(&mut settings.pull, p, pull_label(p));
                    }
                })
                .response
                .on_hover_text(pull_tip);
            ui.end_row();

            ui.label(t!("Clone into"));
            ui.horizontal(|ui| {
                let hint = dirs_home()
                    .map(|h| h.display().to_string())
                    .unwrap_or_default();
                ui.add(
                    egui::TextEdit::singleline(&mut settings.clone_dir)
                        .hint_text(hint)
                        .desired_width(260.0),
                );
                if ui.button(t!("Browse…")).clicked()
                    && let Some(dir) = rfd::FileDialog::new().pick_folder()
                {
                    settings.clone_dir = dir.display().to_string();
                }
            });
            ui.end_row();
        });
    crate::ui::background::settings_section(ui, &mut settings.background);
    crate::ui::sync_ui::settings_section(ui, &mut settings.sync);
}

fn profiles(
    ui: &mut egui::Ui,
    win: &mut SettingsWindow,
    settings: &mut Settings,
    cmds: &mut Commands,
) {
    heading(ui, &t!("Profiles"));
    let note = t!("Identities you can apply to a repository, e.g. personal and work.");
    ui.weak(note);
    ui.add_space(6.0);
    if win.profile.is_some_and(|i| i >= settings.profiles.len()) {
        win.profile = None;
    }
    for (i, p) in settings.profiles.iter().enumerate() {
        let label = if p.label.trim().is_empty() {
            t!("(unnamed)")
        } else {
            p.label.clone()
        };
        let text = format!("{label}  ·  {} <{}>", p.name, p.email);
        if ui
            .add(egui::Button::selectable(win.profile == Some(i), text))
            .clicked()
        {
            win.profile = (win.profile != Some(i)).then_some(i);
        }
    }
    if settings.profiles.is_empty() {
        ui.weak(t!("No profiles yet."));
    }
    ui.add_space(4.0);
    if ui.button(t!("➕ Add profile")).clicked() {
        settings.profiles.push(crate::settings::Profile {
            label: t!("New profile"),
            ..Default::default()
        });
        win.profile = Some(settings.profiles.len() - 1);
    }

    let Some(i) = win.profile else { return };
    ui.add_space(8.0);
    ui.separator();
    let p = &mut settings.profiles[i];
    let mut delete = false;
    egui::Grid::new("settings-profile")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            let field = |ui: &mut egui::Ui, label: String, value: &mut String, hint: String| {
                ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(value)
                        .hint_text(hint)
                        .desired_width(300.0),
                );
                ui.end_row();
            };
            field(ui, t!("Label"), &mut p.label, t!("Work"));
            field(ui, t!("Name"), &mut p.name, t!("Your Name"));
            field(ui, t!("Email"), &mut p.email, "you@example.com".into());
            ui.label(t!("SSH key"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut p.ssh_key)
                        .hint_text(t!("~/.ssh/id_ed25519_work (optional)"))
                        .desired_width(220.0),
                );
                if ui.button(t!("Browse…")).clicked() {
                    let mut dialog = rfd::FileDialog::new();
                    if let Some(ssh) = dirs_home().map(|h| h.join(".ssh")).filter(|d| d.is_dir()) {
                        dialog = dialog.set_directory(ssh);
                    }
                    if let Some(f) = dialog.pick_file() {
                        p.ssh_key = f.display().to_string();
                    }
                }
            });
            ui.end_row();
            field(
                ui,
                t!("Signing key"),
                &mut p.signing_key,
                t!("GPG key id or ~/.ssh/id_ed25519.pub"),
            );
            ui.label("");
            ui.add_enabled(
                !p.signing_key.trim().is_empty(),
                egui::Checkbox::new(&mut p.sign_commits, t!("Sign commits")),
            );
            ui.end_row();
        });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let valid = !p.name.trim().is_empty() && p.email.contains('@');
        let tip = t!("Sets user.name, user.email and signing in this repository's config");
        if ui
            .add_enabled(valid, egui::Button::new(t!("Apply to current repository")))
            .on_hover_text(tip)
            .on_disabled_hover_text(t!("Enter a name and an email first"))
            .clicked()
        {
            cmds.run(Op::SetIdentity(p.identity()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(RichText::new(t!("Delete")).color(ui.visuals().error_fg_color))
                .clicked()
            {
                delete = true;
            }
        });
    });
    if delete {
        settings.profiles.remove(i);
        win.profile = None;
    }
}

/// Mirrors `FerrumApp::shortcuts` in app.rs.
/// The pull strategy as shown in the picker (Merge and Rebase are git terms).
fn pull_label(p: PullChoice) -> String {
    match p {
        PullChoice::Default => t!("Default (git config)"),
        PullChoice::FastForwardOnly => t!("Fast-forward only"),
        PullChoice::Merge | PullChoice::Rebase => p.label().to_owned(),
    }
}

fn about(ui: &mut egui::Ui, win: &mut SettingsWindow, cmds: &mut Commands) {
    heading(ui, &t!("About Ferrum"));
    let git = win
        .git_version
        .get_or_insert_with(ferrum_core::cli::version);
    let git = git.clone().unwrap_or_else(|| t!("not found on PATH"));
    ui.label(t!("A fast, cross-platform Git client written in Rust."));
    ui.add_space(6.0);
    egui::Grid::new("settings-about")
        .num_columns(2)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            ui.label(t!("Version"));
            ui.label(env!("CARGO_PKG_VERSION"));
            ui.end_row();
            ui.label("Git");
            ui.label(git);
            ui.end_row();
            ui.label(t!("Source"));
            if ui.link(REPO_URL).clicked() {
                cmds.push(Command::OpenUrl(REPO_URL.into()));
            }
            ui.end_row();
        });
}
