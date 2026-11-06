//! Settings window: general options, identity profiles, forge accounts,
//! keyboard shortcuts and version information.

use eframe::egui::{self, RichText};
use ferrum_core::forge::ForgeKind;

use crate::command::{Command, Commands, Op};
use crate::settings::{PullChoice, Settings, ThemeChoice};
use crate::ui::dialogs::{Dialog, dirs_home};
use crate::ui::forge;

const REPO_URL: &str = "https://github.com/herlondf/ferrum";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Section {
    #[default]
    General,
    Profiles,
    Accounts,
    Shortcuts,
    About,
}

impl Section {
    const ALL: [Section; 5] = [
        Section::General,
        Section::Profiles,
        Section::Accounts,
        Section::Shortcuts,
        Section::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::General => "⚙ General",
            Section::Profiles => "👤 Profiles",
            Section::Accounts => "☁ Accounts",
            Section::Shortcuts => "⌘ Shortcuts",
            Section::About => "📝 About",
        }
    }
}

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    section: Section,
    /// The profile being edited.
    profile: Option<usize>,
    /// Output of `git --version`, looked up the first time About is shown.
    git_version: Option<String>,
}

pub fn show(
    ctx: &egui::Context,
    win: &mut SettingsWindow,
    settings: &mut Settings,
    cmds: &mut Commands,
) {
    let mut open = win.open;
    egui::Window::new("Settings")
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
                            Section::Profiles => profiles(ui, win, settings, cmds),
                            Section::Accounts => accounts(ui, settings, cmds),
                            Section::Shortcuts => shortcuts(ui),
                            Section::About => about(ui, win, cmds),
                        });
                });
            });
        });
    win.open = open;
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(16.0).strong());
    ui.add_space(6.0);
}

fn general(ui: &mut egui::Ui, settings: &mut Settings) {
    heading(ui, "General");
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

            ui.label("Theme");
            ui.horizontal(|ui| {
                for (t, name) in [
                    (ThemeChoice::System, "System"),
                    (ThemeChoice::Light, "Light"),
                    (ThemeChoice::Dark, "Dark"),
                ] {
                    ui.selectable_value(&mut settings.theme, t, name);
                }
            });
            ui.end_row();

            ui.label("Zoom");
            ui.horizontal(|ui| {
                ui.add(
                    egui::Slider::new(&mut settings.zoom, 0.7..=1.6)
                        .step_by(0.05)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                );
                if ui
                    .add_enabled(settings.zoom != 1.0, egui::Button::new("Reset"))
                    .clicked()
                {
                    settings.zoom = 1.0;
                }
            });
            ui.end_row();

            ui.label("Pull");
            egui::ComboBox::from_id_salt("settings-pull")
                .selected_text(settings.pull.label())
                .width(200.0)
                .show_ui(ui, |ui| {
                    for p in [
                        PullChoice::Default,
                        PullChoice::Merge,
                        PullChoice::Rebase,
                        PullChoice::FastForwardOnly,
                    ] {
                        ui.selectable_value(&mut settings.pull, p, p.label());
                    }
                })
                .response
                .on_hover_text("How the toolbar's Pull button integrates remote changes");
            ui.end_row();

            ui.label("Clone into");
            ui.horizontal(|ui| {
                let hint = dirs_home()
                    .map(|h| h.display().to_string())
                    .unwrap_or_default();
                ui.add(
                    egui::TextEdit::singleline(&mut settings.clone_dir)
                        .hint_text(hint)
                        .desired_width(260.0),
                );
                if ui.button("Browse…").clicked()
                    && let Some(dir) = rfd::FileDialog::new().pick_folder()
                {
                    settings.clone_dir = dir.display().to_string();
                }
            });
            ui.end_row();
        });
}

fn profiles(
    ui: &mut egui::Ui,
    win: &mut SettingsWindow,
    settings: &mut Settings,
    cmds: &mut Commands,
) {
    heading(ui, "Profiles");
    ui.weak("Identities you can apply to a repository, e.g. personal and work.");
    ui.add_space(6.0);
    if win.profile.is_some_and(|i| i >= settings.profiles.len()) {
        win.profile = None;
    }
    for (i, p) in settings.profiles.iter().enumerate() {
        let label = if p.label.trim().is_empty() {
            "(unnamed)"
        } else {
            &p.label
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
        ui.weak("No profiles yet.");
    }
    ui.add_space(4.0);
    if ui.button("➕ Add profile").clicked() {
        settings.profiles.push(crate::settings::Profile {
            label: "New profile".into(),
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
            let field = |ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str| {
                ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(value)
                        .hint_text(hint)
                        .desired_width(300.0),
                );
                ui.end_row();
            };
            field(ui, "Label", &mut p.label, "Work");
            field(ui, "Name", &mut p.name, "Your Name");
            field(ui, "Email", &mut p.email, "you@example.com");
            ui.label("SSH key");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut p.ssh_key)
                        .hint_text("~/.ssh/id_ed25519_work (optional)")
                        .desired_width(220.0),
                );
                if ui.button("Browse…").clicked() {
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
                "Signing key",
                &mut p.signing_key,
                "GPG key id or ~/.ssh/id_ed25519.pub",
            );
            ui.label("");
            ui.add_enabled(
                !p.signing_key.trim().is_empty(),
                egui::Checkbox::new(&mut p.sign_commits, "Sign commits"),
            );
            ui.end_row();
        });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let valid = !p.name.trim().is_empty() && p.email.contains('@');
        if ui
            .add_enabled(valid, egui::Button::new("Apply to current repository"))
            .on_hover_text("Sets user.name, user.email and signing in this repository's config")
            .on_disabled_hover_text("Enter a name and an email first")
            .clicked()
        {
            cmds.run(Op::SetIdentity(p.identity()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(RichText::new("Delete").color(ui.visuals().error_fg_color))
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

fn accounts(ui: &mut egui::Ui, settings: &mut Settings, cmds: &mut Commands) {
    heading(ui, "Accounts");
    ui.weak("Used to list and create pull requests and merge requests.");
    ui.add_space(6.0);
    let mut remove = None;
    if settings.accounts.is_empty() {
        ui.weak("No accounts yet.");
    } else {
        egui::Grid::new("settings-accounts")
            .num_columns(4)
            .striped(true)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                for (i, a) in settings.accounts.iter().enumerate() {
                    ui.label(a.kind.name());
                    ui.label(&a.host);
                    ui.label(RichText::new(&a.username).strong());
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
    }
    if let Some(i) = remove {
        settings.accounts.remove(i);
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Add GitHub account…").clicked() {
            cmds.push(Command::Dialog(Dialog::Forge(forge::login(
                ForgeKind::GitHub,
                "github.com",
            ))));
        }
        if ui.button("Add GitLab account…").clicked() {
            cmds.push(Command::Dialog(Dialog::Forge(forge::login(
                ForgeKind::GitLab,
                "gitlab.com",
            ))));
        }
    });
    ui.add_space(10.0);
    ui.weak("Tokens are stored in Ferrum's settings file on this computer.");
    ui.weak("Without a saved GitHub account, the GitHub CLI login (gh auth login) is used automatically.");
}

/// Mirrors `FerrumApp::shortcuts` in app.rs.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Mod+P", "Command palette"),
    ("Mod+O", "Open repository"),
    ("Mod+,", "Settings"),
    ("F5 / Mod+R", "Refresh"),
    ("Mod+F", "Search commits"),
    ("Mod+1", "History"),
    ("Mod+2", "Local changes"),
    ("Mod+B", "New branch"),
    ("Mod+Shift+S", "Stash changes"),
    ("Mod+Shift+F", "Fetch all remotes"),
    ("Mod+Shift+D", "Pull"),
    ("Mod+Shift+U", "Push"),
    ("Mod+Z", "Undo the last operation"),
    ("Mod+W", "Close tab"),
    ("Mod+Tab", "Next tab"),
    ("Mod+Shift+Tab", "Previous tab"),
];

fn shortcuts(ui: &mut egui::Ui) {
    heading(ui, "Keyboard shortcuts");
    let m = if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    };
    egui::Grid::new("settings-shortcuts")
        .num_columns(2)
        .striped(true)
        .spacing([24.0, 6.0])
        .show(ui, |ui| {
            for (keys, action) in SHORTCUTS {
                ui.label(RichText::new(keys.replace("Mod", m)).monospace());
                ui.label(*action);
                ui.end_row();
            }
        });
}

fn about(ui: &mut egui::Ui, win: &mut SettingsWindow, cmds: &mut Commands) {
    heading(ui, "About Ferrum");
    let git = win.git_version.get_or_insert_with(|| {
        std::process::Command::new("git")
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().trim_start_matches("git version ").to_owned())
            .unwrap_or_else(|| "not found on PATH".into())
    });
    ui.label("A fast, cross-platform Git client written in Rust.");
    ui.add_space(6.0);
    egui::Grid::new("settings-about")
        .num_columns(2)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            ui.label("Version");
            ui.label(env!("CARGO_PKG_VERSION"));
            ui.end_row();
            ui.label("Git");
            ui.label(git.as_str());
            ui.end_row();
            ui.label("Source");
            if ui.link(REPO_URL).clicked() {
                cmds.push(Command::OpenUrl(REPO_URL.into()));
            }
            ui.end_row();
        });
}
