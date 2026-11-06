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

    fn label(self) -> String {
        match self {
            Section::General => t!("⚙ General"),
            Section::Profiles => t!("👤 Profiles"),
            Section::Accounts => t!("☁ Accounts"),
            Section::Shortcuts => t!("⌘ Shortcuts"),
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
                for (t, name) in [
                    (ThemeChoice::System, t!("System")),
                    (ThemeChoice::Light, t!("Light")),
                    (ThemeChoice::Dark, t!("Dark")),
                ] {
                    ui.selectable_value(&mut settings.theme, t, name);
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

fn accounts(ui: &mut egui::Ui, settings: &mut Settings, cmds: &mut Commands) {
    heading(ui, &t!("Accounts"));
    let note = t!("Used to list and create pull requests and merge requests.");
    ui.weak(note);
    ui.add_space(6.0);
    let mut remove = None;
    if settings.accounts.is_empty() {
        ui.weak(t!("No accounts yet."));
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
                    if ui.small_button(t!("Remove")).clicked() {
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
        if ui.button(t!("Add {} account…", "GitHub")).clicked() {
            cmds.push(Command::Dialog(Dialog::Forge(forge::login(
                ForgeKind::GitHub,
                "github.com",
            ))));
        }
        if ui.button(t!("Add {} account…", "GitLab")).clicked() {
            cmds.push(Command::Dialog(Dialog::Forge(forge::login(
                ForgeKind::GitLab,
                "gitlab.com",
            ))));
        }
    });
    ui.add_space(10.0);
    let stored = t!("Tokens are stored in Ferrum's settings file on this computer.");
    let cli = t!("Without a saved GitHub account, the GitHub CLI login (gh auth login) is used.");
    ui.weak(stored);
    ui.weak(cli);
}

/// Mirrors `FerrumApp::shortcuts` in app.rs.
fn shortcut_list() -> [(&'static str, String); 16] {
    [
        ("Mod+P", t!("Command palette")),
        ("Mod+O", t!("Open repository")),
        ("Mod+,", t!("Settings")),
        ("F5 / Mod+R", t!("Refresh")),
        ("Mod+F", t!("Search commits")),
        ("Mod+1", t!("History")),
        ("Mod+2", t!("Local changes")),
        ("Mod+B", t!("New branch")),
        ("Mod+Shift+S", t!("Stash changes")),
        ("Mod+Shift+F", t!("Fetch all remotes")),
        ("Mod+Shift+D", "Pull".to_owned()),
        ("Mod+Shift+U", "Push".to_owned()),
        ("Mod+Z", t!("Undo the last operation")),
        ("Mod+W", t!("Close tab")),
        ("Mod+Tab", t!("Next tab")),
        ("Mod+Shift+Tab", t!("Previous tab")),
    ]
}

/// The pull strategy as shown in the picker (Merge and Rebase are git terms).
fn pull_label(p: PullChoice) -> String {
    match p {
        PullChoice::Default => t!("Default (git config)"),
        PullChoice::FastForwardOnly => t!("Fast-forward only"),
        PullChoice::Merge | PullChoice::Rebase => p.label().to_owned(),
    }
}

fn shortcuts(ui: &mut egui::Ui) {
    heading(ui, &t!("Keyboard shortcuts"));
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
            for (keys, action) in shortcut_list() {
                ui.label(RichText::new(keys.replace("Mod", m)).monospace());
                ui.label(action);
                ui.end_row();
            }
        });
}

fn about(ui: &mut egui::Ui, win: &mut SettingsWindow, cmds: &mut Commands) {
    heading(ui, &t!("About Ferrum"));
    let git = win.git_version.get_or_insert_with(|| {
        std::process::Command::new("git")
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().trim_start_matches("git version ").to_owned())
    });
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
