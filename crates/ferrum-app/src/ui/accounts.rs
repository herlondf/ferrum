//! Settings > Accounts (logins on hosted services, their repositories) and
//! Settings > SSH keys (list, generate, ssh-agent, upload), plus the
//! first-time SSH setup assistant.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use ferrum_core::forge::{self, ForgeKind, RemoteRepo};
use ferrum_core::ssh::{self, SshKey};

use crate::command::{Command, Commands};
use crate::hosted::{HostedCmd, Pending, take_done};
use crate::settings::{ForgeAccount, Settings};
use crate::ui::dialogs::Dialog;
use crate::ui::forge as forge_ui;

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum AccountTab {
    #[default]
    Account,
    Repositories,
}

#[derive(Default)]
pub struct State {
    selected: usize,
    tab: AccountTab,
    /// Token check of the selected account: (index, request) and its result.
    check: Option<(usize, Pending<String>)>,
    status: Option<(usize, Result<String, String>)>,
    repos: Option<(usize, Pending<Vec<RemoteRepo>>)>,
    repo_list: Option<(usize, Result<Vec<RemoteRepo>, String>)>,
    search: String,
    ssh_urls: bool,
    pub ssh: SshState,
}

#[derive(Default)]
pub struct SshState {
    keys: Option<Vec<SshKey>>,
    selected: Option<PathBuf>,
    generate: Option<GenerateForm>,
    /// ssh-add, upload: what runs and its outcome.
    task: Option<(String, Pending<()>)>,
    outcome: Option<Result<String, String>>,
    agent: Option<bool>,
}

pub struct GenerateForm {
    pub path: String,
    pub comment: String,
    pub passphrase: String,
    pub confirm: String,
    pub error: Option<String>,
}

impl GenerateForm {
    pub fn new() -> Self {
        let dir = ssh::ssh_dir().unwrap_or_default();
        // The first free id_ed25519[_N].
        let path = (0..)
            .map(|i| match i {
                0 => dir.join("id_ed25519"),
                n => dir.join(format!("id_ed25519_{n}")),
            })
            .find(|p| !p.exists())
            .unwrap_or_else(|| dir.join("id_ed25519"));
        Self {
            path: path.to_string_lossy().into_owned(),
            comment: computer_name(),
            passphrase: String::new(),
            confirm: String::new(),
            error: None,
        }
    }

    /// Shows the fields; returns the generated key when "Generate" worked.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<SshKey> {
        egui::Grid::new("ssh-generate")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(t!("Location"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(300.0));
                    if ui.button(t!("Browse…")).clicked()
                        && let Some(p) = rfd::FileDialog::new()
                            .set_file_name("id_ed25519")
                            .save_file()
                    {
                        self.path = p.to_string_lossy().into_owned();
                    }
                });
                ui.end_row();
                ui.label(t!("Comment"));
                ui.add(egui::TextEdit::singleline(&mut self.comment).desired_width(300.0));
                ui.end_row();
                ui.label(t!("Passphrase"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.passphrase)
                        .password(true)
                        .hint_text(t!("Optional"))
                        .desired_width(300.0),
                );
                ui.end_row();
                ui.label(t!("Repeat"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.confirm)
                        .password(true)
                        .desired_width(300.0),
                );
                ui.end_row();
            });
        let mismatch = self.passphrase != self.confirm;
        if mismatch {
            ui.colored_label(ui.visuals().warn_fg_color, t!("The passphrases differ."));
        }
        if let Some(e) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
        }
        let ready = !mismatch && !self.path.trim().is_empty();
        if ui
            .add_enabled(ready, egui::Button::new(t!("Generate ED25519 key")))
            .clicked()
        {
            match ssh::generate(
                std::path::Path::new(self.path.trim()),
                &self.passphrase,
                self.comment.trim(),
            ) {
                Ok(k) => return Some(k),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        None
    }
}

fn computer_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|h| !h.trim().is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|h| h.trim().to_owned())
        })
        .unwrap_or_else(|| "computer".into());
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    if user.is_empty() {
        host
    } else {
        format!("{user}@{host}")
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(16.0).strong());
    ui.add_space(6.0);
}

fn login(kind: ForgeKind, host: &str) -> Command {
    Command::Dialog(Dialog::Forge(forge_ui::login(kind, host)))
}

// ----- Accounts -----

pub fn accounts(ui: &mut egui::Ui, st: &mut State, settings: &mut Settings, cmds: &mut Commands) {
    heading(ui, &t!("Accounts"));
    ui.weak(t!("Used to list and create pull requests and issues, clone your repositories and add SSH keys."));
    ui.add_space(6.0);
    if let Some((i, p)) = st.check.take() {
        let mut slot = Some(p);
        match take_done(&mut slot) {
            Some(r) => st.status = Some((i, r)),
            None => st.check = slot.map(|p| (i, p)),
        }
    }
    if let Some((i, p)) = st.repos.take() {
        let mut slot = Some(p);
        match take_done(&mut slot) {
            Some(r) => st.repo_list = Some((i, r)),
            None => st.repos = slot.map(|p| (i, p)),
        }
    }
    let mut remove = None;
    ui.horizontal_top(|ui| {
        // The list.
        ui.vertical(|ui| {
            ui.set_width(200.0);
            if settings.accounts.is_empty() {
                ui.weak(t!("No accounts yet."));
            }
            for (i, a) in settings.accounts.iter().enumerate() {
                let text = format!("{}\n{} · {}", a.kind.name(), a.username, a.host);
                if ui
                    .add(
                        egui::Button::selectable(st.selected == i, text)
                            .min_size(egui::vec2(200.0, 0.0)),
                    )
                    .clicked()
                {
                    st.selected = i;
                }
            }
            ui.horizontal(|ui| {
                if ui.button("✚").on_hover_text(t!("Add account…")).clicked() {
                    cmds.push(login(ForgeKind::GitHub, "github.com"));
                }
                if ui
                    .add_enabled(!settings.accounts.is_empty(), egui::Button::new("−"))
                    .on_hover_text(t!("Remove"))
                    .clicked()
                {
                    remove = Some(st.selected);
                }
            });
        });
        ui.separator();
        ui.vertical(|ui| {
            st.selected = st.selected.min(settings.accounts.len().saturating_sub(1));
            if settings.accounts.is_empty() {
                ui.weak(t!("Add an account to see its details and repositories."));
                return;
            }
            ui.horizontal(|ui| {
                ui.selectable_value(&mut st.tab, AccountTab::Account, t!("Account"));
                ui.selectable_value(&mut st.tab, AccountTab::Repositories, t!("Repositories"));
            });
            ui.separator();
            match st.tab {
                AccountTab::Account => account_details(ui, st, settings, cmds),
                AccountTab::Repositories => account_repos(ui, st, settings, cmds),
            }
        });
    });
    if let Some(i) = remove.filter(|i| *i < settings.accounts.len()) {
        settings.accounts.remove(i);
        st.status = None;
        st.repo_list = None;
    }
    ui.add_space(10.0);
    let stored = t!("Tokens are stored in Ferrum's settings file on this computer.");
    let cli = t!("Without a saved GitHub account, the GitHub CLI login (gh auth login) is used.");
    ui.weak(stored);
    ui.weak(cli);
}

fn account_details(
    ui: &mut egui::Ui,
    st: &mut State,
    settings: &mut Settings,
    cmds: &mut Commands,
) {
    let i = st.selected;
    let ctx = ui.ctx().clone();
    let a = settings.accounts[i].clone();
    egui::Grid::new("account-details")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.weak(t!("Service"));
            ui.label(a.kind.name());
            ui.end_row();
            ui.weak(t!("Server"));
            ui.label(&a.host);
            ui.end_row();
            ui.weak(t!("Username"));
            ui.label(RichText::new(&a.username).strong());
            ui.end_row();
            if let Some(label) = a.kind.user_label().filter(|_| !a.user.is_empty()) {
                ui.weak(if label == "Organization" {
                    t!("Organization")
                } else {
                    t!("Login")
                });
                ui.label(&a.user);
                ui.end_row();
            }
            ui.weak(t!("Status"));
            ui.horizontal(|ui| {
                match &st.status {
                    _ if st.check.as_ref().is_some_and(|(j, _)| *j == i) => {
                        ui.spinner();
                    }
                    Some((j, Ok(name))) if *j == i => {
                        ui.label(t!("✔ Token works ({})", name));
                    }
                    Some((j, Err(e))) if *j == i => {
                        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
                    }
                    _ => {
                        ui.weak(t!("Not checked"));
                    }
                }
                if ui.small_button(t!("Check")).clicked() {
                    let (kind, host, creds) = (a.kind, a.host.clone(), a.credentials());
                    st.check = Some((
                        i,
                        Pending::spawn(&ctx, move || {
                            forge::verify_token(kind, &host, &creds).map_err(|e| e.to_string())
                        }),
                    ));
                }
            });
            ui.end_row();
        });
    ui.add_space(6.0);
    if a.kind == ForgeKind::GitHub {
        ui.checkbox(
            &mut settings.accounts[i].notifications,
            t!("Enable notifications"),
        )
        .on_hover_text(t!("Show the unread GitHub notifications in the toolbar"));
    }
    ui.horizontal(|ui| {
        if ui.button(t!("Update token…")).clicked() {
            cmds.push(login(a.kind, &a.host));
        }
        let first_for_host = settings
            .accounts
            .iter()
            .position(|b| b.kind == a.kind && b.host.eq_ignore_ascii_case(&a.host))
            == Some(i);
        if !first_for_host
            && ui
                .button(t!("Use for {}", a.host))
                .on_hover_text(t!("Several accounts on one server: the first one is used for pull requests and issues"))
                .clicked()
        {
            let acc = settings.accounts.remove(i);
            settings.accounts.insert(0, acc);
            st.selected = 0;
        }
        if a.kind.can_upload_ssh_key() && ui.button(t!("Set up SSH…")).clicked() {
            cmds.push(Command::Hosted(HostedCmd::SshSetup));
        }
    });
}

fn account_repos(ui: &mut egui::Ui, st: &mut State, settings: &Settings, cmds: &mut Commands) {
    let i = st.selected;
    let ctx = ui.ctx().clone();
    let loading = st.repos.as_ref().is_some_and(|(j, _)| *j == i);
    let loaded = st.repo_list.as_ref().is_some_and(|(j, _)| *j == i);
    if !loading && !loaded {
        let a = &settings.accounts[i];
        st.repos = Some((i, crate::ui::clone_repos::start_load(&ctx, a)));
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut st.search)
                .hint_text(t!("Search repositories"))
                .desired_width(220.0),
        );
        ui.radio_value(&mut st.ssh_urls, false, "HTTPS");
        ui.radio_value(&mut st.ssh_urls, true, "SSH");
        if ui.small_button("⟳").on_hover_text(t!("Reload")).clicked() {
            st.repo_list = None;
        }
    });
    match &st.repo_list {
        Some((j, Ok(repos))) if *j == i => {
            let list = crate::ui::clone_repos::matching(repos, &st.search);
            egui::ScrollArea::vertical()
                .id_salt("account-repos")
                .max_height(260.0)
                .show(ui, |ui| {
                    if list.is_empty() {
                        ui.weak(t!("No repositories match"));
                    }
                    for r in list {
                        ui.horizontal(|ui| {
                            let url = if st.ssh_urls && !r.ssh_url.is_empty() {
                                r.ssh_url.clone()
                            } else {
                                r.https_url.clone()
                            };
                            if ui.small_button(t!("Clone…")).clicked() {
                                cmds.push(Command::Hosted(HostedCmd::Clone(url.clone())));
                            }
                            ui.label(&r.name).on_hover_text(&url);
                            if !r.description.is_empty() {
                                ui.weak(&r.description);
                            }
                        });
                    }
                });
        }
        Some((j, Err(e))) if *j == i => {
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
        }
        _ => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(t!("Loading repositories…"));
            });
        }
    }
}

// ----- SSH keys -----

fn all_keys(settings: &Settings) -> Vec<SshKey> {
    let mut keys = ssh::ssh_dir()
        .map(|d| ssh::list_keys(&d))
        .unwrap_or_default();
    for p in &settings.forge.extra_keys {
        if let Ok(k) = ssh::read_key(std::path::Path::new(p))
            && !keys.iter().any(|x| x.private == k.private)
        {
            keys.push(k);
        }
    }
    keys
}

/// Accounts that can receive an SSH key through the API.
fn upload_targets(settings: &Settings) -> Vec<(usize, &ForgeAccount)> {
    settings
        .accounts
        .iter()
        .enumerate()
        .filter(|(_, a)| a.kind.can_upload_ssh_key())
        .collect()
}

fn upload(ctx: &egui::Context, a: &ForgeAccount, key: &SshKey) -> Pending<()> {
    let (kind, host, creds) = (a.kind, a.host.clone(), a.credentials());
    let (title, text) = (
        format!("Ferrum ({})", computer_name()),
        key.public_key.clone(),
    );
    Pending::spawn(ctx, move || {
        forge::upload_ssh_key(kind, &host, &creds, &title, &text).map_err(|e| e.to_string())
    })
}

pub fn ssh_keys(ui: &mut egui::Ui, st: &mut State, settings: &mut Settings, cmds: &mut Commands) {
    heading(ui, &t!("SSH keys"));
    let s = &mut st.ssh;
    let ctx = ui.ctx().clone();
    if let Some((what, p)) = s.task.take() {
        let mut slot = Some(p);
        match take_done(&mut slot) {
            Some(r) => s.outcome = Some(r.map(|()| what)),
            None => s.task = slot.map(|p| (what, p)),
        }
    }
    let agent = *s.agent.get_or_insert_with(ssh::agent_running);
    ui.weak(if agent {
        t!("SSH configuration: system ssh, with a running ssh-agent.")
    } else {
        t!("SSH configuration: system ssh; no ssh-agent is running.")
    });
    ui.add_space(4.0);
    let keys = s.keys.get_or_insert_with(|| all_keys(settings)).clone();
    ui.horizontal(|ui| {
        if ui.button(t!("Generate new key…")).clicked() {
            s.generate = Some(GenerateForm::new());
        }
        if ui.button(t!("Add existing key…")).clicked()
            && let Some(p) = rfd::FileDialog::new()
                .set_directory(ssh::ssh_dir().unwrap_or_default())
                .pick_file()
        {
            match ssh::read_key(&p) {
                Ok(k) => {
                    let text = k.private.to_string_lossy().into_owned();
                    if !settings.forge.extra_keys.contains(&text) {
                        settings.forge.extra_keys.push(text);
                    }
                    s.selected = Some(k.private);
                    s.keys = None;
                }
                Err(e) => s.outcome = Some(Err(e.to_string())),
            }
        }
        if ui.button("⟳").on_hover_text(t!("Reload")).clicked() {
            s.keys = None;
            s.agent = None;
        }
        if ui.button(t!("Set up SSH for an account…")).clicked() {
            cmds.push(Command::Hosted(HostedCmd::SshSetup));
        }
    });
    if let Some(form) = &mut s.generate {
        let made = egui::Frame::group(ui.style())
            .show(ui, |ui| form.show(ui))
            .inner;
        if let Some(k) = made {
            s.selected = Some(k.private.clone());
            s.outcome = Some(Ok(t!("Created {}", k.private.display())));
            s.generate = None;
            s.keys = None;
        }
    }
    ui.add_space(4.0);
    if keys.is_empty() {
        ui.weak(t!("No SSH keys found in ~/.ssh."));
    }
    for k in &keys {
        let name = k.private.to_string_lossy();
        let text = format!("{name}   {}", k.kind);
        let on = s.selected.as_ref() == Some(&k.private);
        if ui.selectable_label(on, text).clicked() {
            s.selected = Some(k.private.clone());
            s.outcome = None;
        }
    }
    if let Some(key) = keys
        .iter()
        .find(|k| Some(&k.private) == s.selected.as_ref())
    {
        ui.separator();
        egui::Grid::new("ssh-key")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.weak(t!("Location"));
                ui.label(key.private.to_string_lossy());
                ui.end_row();
                ui.weak(t!("Fingerprint"));
                ui.label(RichText::new(&key.fingerprint).monospace());
                ui.end_row();
                if !key.comment.is_empty() {
                    ui.weak(t!("Comment"));
                    ui.label(&key.comment);
                    ui.end_row();
                }
            });
        ui.weak(t!("Public key"));
        let mut text = key.public_key.clone();
        ui.add(
            egui::TextEdit::multiline(&mut text)
                .font(egui::TextStyle::Monospace)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal_wrapped(|ui| {
            if ui.button(t!("Copy to clipboard")).clicked() {
                cmds.push(Command::Copy(key.public_key.clone()));
            }
            let busy = s.task.is_some();
            if ui
                .add_enabled(!busy, egui::Button::new(t!("Add to ssh-agent")))
                .on_hover_text(t!("The agent remembers the passphrase until you log out"))
                .clicked()
            {
                let (path, env) = (key.private.clone(), ferrum_core::cli::global_env());
                let what = t!("Added {} to the ssh-agent", key.private.display());
                s.task = Some((
                    what,
                    Pending::spawn(&ctx, move || {
                        ssh::add_to_agent(&path, &env).map_err(|e| e.to_string())
                    }),
                ));
            }
            let targets = upload_targets(settings);
            ui.add_enabled_ui(!busy && !targets.is_empty(), |ui| {
                ui.menu_button(t!("Add to account ⏷"), |ui| {
                    for (_, a) in &targets {
                        if ui
                            .button(format!("{} · {} · {}", a.kind.name(), a.host, a.username))
                            .clicked()
                        {
                            let what = t!("Added the key to {} on {}", a.username, a.host);
                            s.task = Some((what, upload(&ctx, a, key)));
                            ui.close();
                        }
                    }
                });
            });
            let text = key.private.to_string_lossy().into_owned();
            if settings.forge.extra_keys.contains(&text)
                && ui.button(t!("Remove from list")).clicked()
            {
                settings.forge.extra_keys.retain(|p| *p != text);
                s.keys = None;
            }
            if busy {
                ui.spinner();
            }
        });
    }
    match &s.outcome {
        Some(Ok(m)) => {
            ui.label(format!("✔ {m}"));
        }
        Some(Err(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
        }
        None => {}
    }
    ui.add_space(10.0);
    ui.weak(t!("1. Select or generate a key.\n2. Copy its public key, or add it to an account directly.\n3. Use SSH URLs (git@…) for your remotes."));
}

// ----- First-time setup -----

pub struct SetupWizard {
    account: Option<usize>,
    use_existing: Option<PathBuf>,
    generate: GenerateForm,
    task: Option<Pending<()>>,
    result: Option<Result<(), String>>,
    keys: Vec<SshKey>,
}

impl SetupWizard {
    pub fn new(settings: &Settings) -> Self {
        let keys = all_keys(settings);
        Self {
            account: upload_targets(settings).first().map(|(i, _)| *i),
            use_existing: keys.first().map(|k| k.private.clone()),
            generate: GenerateForm::new(),
            task: None,
            result: None,
            keys,
        }
    }
}

/// The assistant window. Returns `false` when closed.
pub fn setup_window(
    ctx: &egui::Context,
    w: &mut SetupWizard,
    settings: &Settings,
    cmds: &mut Commands,
) -> bool {
    if let Some(r) = take_done(&mut w.task) {
        w.result = Some(r);
    }
    let mut open = true;
    egui::Window::new(t!("Set up SSH"))
        .id(egui::Id::unique("ssh-setup"))
        .open(&mut open)
        .default_width(560.0)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(t!("Ferrum creates or picks an SSH key and adds it to your account, so git can use SSH URLs without passwords."));
            ui.add_space(6.0);
            ui.strong(t!("1. Account"));
            let targets = upload_targets(settings);
            if targets.is_empty() {
                ui.label(t!("Add a GitHub, GitLab or Gitea account first."));
                if ui.button(t!("Add account…")).clicked() {
                    cmds.push(login(ForgeKind::GitHub, "github.com"));
                }
                return;
            }
            let label = |a: &ForgeAccount| format!("{} · {} · {}", a.kind.name(), a.host, a.username);
            let current = w
                .account
                .and_then(|i| settings.accounts.get(i))
                .map(label)
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("ssh-setup-account")
                .selected_text(current)
                .width(360.0)
                .show_ui(ui, |ui| {
                    for (i, a) in &targets {
                        ui.selectable_value(&mut w.account, Some(*i), label(a));
                    }
                });
            let scope = match w.account.and_then(|i| settings.accounts.get(i)).map(|a| a.kind) {
                Some(ForgeKind::GitHub) => "write:public_key",
                Some(ForgeKind::GitLab) => "api",
                _ => "write:user",
            };
            ui.weak(t!("The token needs the \"{}\" scope to add keys.", scope));
            ui.add_space(6.0);

            ui.strong(t!("2. Key"));
            ui.radio_value(&mut w.use_existing, None, t!("Generate a new ED25519 key"));
            for k in &w.keys {
                let text = t!("Use {}", k.private.display());
                ui.radio_value(&mut w.use_existing, Some(k.private.clone()), text);
            }
            if w.use_existing.is_none() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    if let Some(k) = w.generate.show(ui) {
                        w.use_existing = Some(k.private.clone());
                        w.keys.push(k);
                    }
                });
            }
            ui.add_space(6.0);

            ui.strong(t!("3. Add it to the account"));
            let key = w
                .keys
                .iter()
                .find(|k| Some(&k.private) == w.use_existing.as_ref())
                .cloned();
            let busy = w.task.is_some();
            ui.horizontal(|ui| {
                let ready = key.is_some() && w.account.is_some() && !busy;
                if ui
                    .add_enabled(ready, egui::Button::new(t!("Add key to account")))
                    .clicked()
                    && let (Some(k), Some(a)) = (&key, w.account.and_then(|i| settings.accounts.get(i)))
                {
                    w.result = None;
                    w.task = Some(upload(ctx, a, k));
                }
                if busy {
                    ui.spinner();
                }
            });
            match &w.result {
                Some(Ok(())) => {
                    ui.label(t!("✔ Done. The account accepts this key now."));
                    if let Some(a) = w.account.and_then(|i| settings.accounts.get(i)) {
                        let host = ferrum_core::ssh::ssh_host(&format!("git@{}:x", a.host))
                            .map(|(h, _)| h)
                            .unwrap_or_else(|| a.host.clone());
                        if ui
                            .button(t!("Trust the host key of {}…", host))
                            .clicked()
                        {
                            cmds.push(Command::Hosted(HostedCmd::TrustHost { host, port: None }));
                        }
                    }
                }
                Some(Err(e)) => {
                    ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
                    if let Some(k) = &key
                        && ui.button(t!("Copy public key")).clicked()
                    {
                        cmds.push(Command::Copy(k.public_key.clone()));
                    }
                }
                None => {}
            }
        });
    open
}
