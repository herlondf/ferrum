//! Hosted services in the main window: the askpass server and its prompts,
//! the GitHub notifications indicator, the AI review window, the SSH host
//! and setup windows, and `Command::Hosted`.

use std::time::{Duration, Instant};

use eframe::egui;
use ferrum_core::askpass;
use ferrum_core::forge::{self, ForgeKind, Notification};

use super::FerrumApp;
use crate::command::{Command, Commands};
use crate::hosted::{HostedCmd, Pending, take_done};
use crate::tab::View;
use crate::ui::{accounts, ai_review, askpass as askpass_ui};

#[derive(Default)]
pub struct State {
    server: Option<askpass::Server>,
    /// The askpass setting the server was last set up for.
    askpass_on: Option<bool>,
    pub prompts: askpass_ui::Prompts,
    notifications: Notifications,
    review: Option<ai_review::ReviewWindow>,
    trust: Option<askpass_ui::TrustWindow>,
    setup: Option<accounts::SetupWizard>,
    /// Time of the last tab message looked at for ssh and auth errors.
    seen_message: Option<Instant>,
}

#[derive(Default)]
struct Notifications {
    /// (host, notification), newest first.
    list: Vec<(String, Notification)>,
    error: Option<String>,
    pending: Option<Pending<Vec<(String, Notification)>>>,
    last: Option<Instant>,
    open: bool,
}

impl State {
    /// Check again soon (accounts changed).
    pub fn notifications_changed(&mut self) {
        self.notifications.last = None;
    }
}

/// Dev and test runs never talk to real services by themselves.
fn dev_run() -> bool {
    cfg!(test)
        || std::env::var_os("FERRUM_SCREENSHOT").is_some()
        || std::env::var_os("FERRUM_NO_UPDATE_CHECK").is_some()
}

impl FerrumApp {
    fn notification_accounts(&self) -> Vec<(String, forge::Credentials)> {
        self.settings
            .accounts
            .iter()
            .filter(|a| a.kind == ForgeKind::GitHub && a.notifications)
            .map(|a| (a.host.clone(), a.credentials()))
            .collect()
    }

    pub(super) fn hosted_tick(&mut self, ctx: &egui::Context) {
        crate::hosted::sync_registry(&self.settings);
        self.askpass_tick(ctx);
        self.watch_errors(ctx);

        // GitHub notifications, every few minutes.
        let n = &mut self.hosted.notifications;
        if let Some(r) = take_done(&mut n.pending) {
            match r {
                Ok(list) => {
                    n.list = list;
                    n.error = None;
                }
                Err(e) => n.error = Some(e),
            }
        }
        let every =
            Duration::from_secs(60 * u64::from(self.settings.forge.notification_minutes.max(1)));
        let due = self
            .hosted
            .notifications
            .last
            .is_none_or(|t| t.elapsed() >= every);
        if due && !dev_run() && self.hosted.notifications.pending.is_none() {
            self.start_notifications(ctx);
        }
        if !self.notification_accounts().is_empty() {
            ctx.request_repaint_after(Duration::from_secs(30));
        }
    }

    fn start_notifications(&mut self, ctx: &egui::Context) {
        let accounts = self.notification_accounts();
        let n = &mut self.hosted.notifications;
        n.last = Some(Instant::now());
        if accounts.is_empty() {
            n.list.clear();
            return;
        }
        n.pending = Some(Pending::spawn(ctx, move || {
            let mut all = Vec::new();
            for (host, creds) in accounts {
                let list = forge::list_notifications(&host, &creds).map_err(|e| e.to_string())?;
                all.extend(list.into_iter().map(|x| (host.clone(), x)));
            }
            all.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));
            Ok(all)
        }));
    }

    /// Starts or stops the askpass server as the setting says, and queues
    /// the prompts it receives.
    fn askpass_tick(&mut self, ctx: &egui::Context) {
        let want = self.settings.forge.askpass && !cfg!(test);
        if self.hosted.askpass_on != Some(want) {
            self.hosted.askpass_on = Some(want);
            self.hosted.server = None;
            ferrum_core::cli::set_global_env(Vec::new());
            if want {
                let repaint = ctx.clone();
                match (
                    askpass::Server::start(move || repaint.request_repaint()),
                    std::env::current_exe(),
                ) {
                    (Ok(server), Ok(exe)) => {
                        ferrum_core::cli::set_global_env(server.env(&exe));
                        self.hosted.server = Some(server);
                    }
                    (Err(e), _) | (_, Err(e)) => eprintln!("askpass: {e}"),
                }
            }
        }
        if let Some(server) = &self.hosted.server {
            while let Some(req) = server.try_recv() {
                self.hosted.prompts.push(req);
            }
        }
    }

    /// Offers to trust an unknown SSH host, and forgets remembered answers
    /// after an authentication error.
    fn watch_errors(&mut self, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let Some(m) = tab.message.as_ref().filter(|m| m.error) else {
            return;
        };
        if self.hosted.seen_message.is_some_and(|t| t >= m.at) {
            return;
        }
        self.hosted.seen_message = Some(m.at);
        let text = m.text.clone();
        let lower = text.to_lowercase();
        if lower.contains("authentication failed")
            || lower.contains("permission denied")
            || lower.contains("invalid username or password")
        {
            self.hosted.prompts.forget();
        }
        if ferrum_core::ssh::is_unknown_host_error(&text) && self.hosted.trust.is_none() {
            let from_remote = tab
                .remotes
                .iter()
                .find_map(|r| ferrum_core::ssh::ssh_host(&r.url));
            let host = ferrum_core::ssh::host_in_error(&text)
                .map(|h| {
                    let port = from_remote
                        .as_ref()
                        .filter(|(rh, _)| *rh == h)
                        .and_then(|r| r.1);
                    (h, port)
                })
                .or(from_remote);
            if let Some((host, port)) = host {
                self.hosted.trust = Some(askpass_ui::TrustWindow::new(ctx, host, port));
            }
        }
    }

    /// The notifications button in the toolbar (GitHub accounts only).
    pub(super) fn notifications_button(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        if self.notification_accounts().is_empty() {
            return;
        }
        let n = &self.hosted.notifications;
        let count = n.list.len();
        let tip = match (&n.error, count) {
            (Some(e), _) => t!("Notifications could not be loaded: {}", e),
            (None, 0) => t!("No unread notifications"),
            (None, 1) => t!("1 unread notification"),
            (None, c) => t!("{} unread notifications", c),
        };
        let badge = (count > 0).then(|| count.to_string());
        let button = crate::ui::toolbar::Tool::icon(crate::icons::BELL, "✉").badge_text(badge);
        if button.show(ui).main.on_hover_text(tip).clicked() {
            cmds.push(Command::Hosted(HostedCmd::ToggleNotifications));
        }
    }

    pub(super) fn hosted_windows(&mut self, ctx: &egui::Context, cmds: &mut Commands) {
        askpass_ui::show(ctx, &mut self.hosted.prompts, &mut self.settings.forge);
        if let Some(w) = &mut self.hosted.trust
            && !askpass_ui::trust_window(ctx, w)
        {
            self.hosted.trust = None;
        }
        if let Some(w) = &mut self.hosted.review
            && !ai_review::show(ctx, w, &mut self.settings, cmds)
        {
            self.hosted.review = None;
        }
        if let Some(w) = &mut self.hosted.setup
            && !accounts::setup_window(ctx, w, &self.settings, cmds)
        {
            self.hosted.setup = None;
        }
        self.notifications_window(ctx, cmds);
    }

    fn notifications_window(&mut self, ctx: &egui::Context, cmds: &mut Commands) {
        let n = &mut self.hosted.notifications;
        if !n.open {
            return;
        }
        let mut open = true;
        let mut refresh = false;
        let mut mark_read = false;
        egui::Window::new(t!("Notifications"))
            .id(egui::Id::unique("gh-notifications"))
            .open(&mut open)
            .default_size([520.0, 420.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    refresh = ui.button(t!("⟳ Refresh")).clicked();
                    if ui
                        .add_enabled(
                            !n.list.is_empty(),
                            egui::Button::new(t!("Mark all as read")),
                        )
                        .clicked()
                    {
                        mark_read = true;
                    }
                    if n.pending.is_some() {
                        ui.spinner();
                    }
                });
                if let Some(e) = &n.error {
                    ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
                }
                ui.separator();
                if n.list.is_empty() {
                    ui.weak(t!("No unread notifications"));
                }
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        for (_, x) in &n.list {
                            let resp = ui
                                .vertical(|ui| {
                                    ui.label(egui::RichText::new(&x.title).strong());
                                    ui.weak(format!("{} · {} · {}", x.repo, x.kind, x.reason));
                                })
                                .response
                                .interact(egui::Sense::click())
                                .on_hover_text(&x.url)
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if resp.clicked() {
                                cmds.push(Command::OpenUrl(x.url.clone()));
                            }
                            ui.separator();
                        }
                    });
            });
        n.open = open;
        if mark_read {
            let accounts = self.notification_accounts();
            let n = &mut self.hosted.notifications;
            n.list.clear();
            n.pending = Some(Pending::spawn(ctx, move || {
                for (host, creds) in &accounts {
                    forge::mark_notifications_read(host, creds).map_err(|e| e.to_string())?;
                }
                Ok(Vec::new())
            }));
        } else if refresh {
            self.start_notifications(ctx);
        }
    }

    pub(super) fn handle_hosted(&mut self, cmd: HostedCmd, ctx: &egui::Context) {
        match cmd {
            HostedCmd::Review { base, head } => {
                let Some(tab) = self.tabs.get(self.active) else {
                    return;
                };
                let prompt = self.settings.forge.review_prompt.clone();
                self.hosted.review = Some(ai_review::ReviewWindow::new(
                    tab.path().to_path_buf(),
                    base,
                    head,
                    prompt,
                ));
            }
            HostedCmd::ToggleNotifications => {
                let n = &mut self.hosted.notifications;
                n.open = !n.open;
            }
            HostedCmd::TrustHost { host, port } => {
                self.hosted.trust = Some(askpass_ui::TrustWindow::new(ctx, host, port));
            }
            HostedCmd::SshSetup => {
                self.hosted.setup = Some(accounts::SetupWizard::new(&self.settings));
            }
            HostedCmd::SetRemote(name) => {
                let Some(tab) = self.tabs.get_mut(self.active) else {
                    return;
                };
                let key = crate::repo_settings::shared_key(tab);
                self.settings.forge.remotes.insert(key, name);
                crate::hosted::sync_registry(&self.settings);
                tab.hosted = crate::hosted::hosted_for(tab);
                tab.prs = Default::default();
                tab.issues.reset();
                if tab.view == View::PullRequests {
                    self.handle(Command::LoadPullRequests, ctx);
                }
            }
            HostedCmd::Clone(url) => {
                let mut d = crate::ui::dialogs::Dialog::clone_repo(&self.settings);
                if let crate::ui::dialogs::Dialog::Clone { url: u, name, .. } = &mut d {
                    *name = crate::ui::dialogs::clone_dir_name(&url);
                    *u = url;
                }
                self.settings_window.open = false;
                self.dialog = Some(d);
            }
            HostedCmd::Dev(arg) => self.hosted_dev_step(&arg, ctx),
        }
    }

    /// `FERRUM_START=hosted:…` steps, for screenshots.
    fn hosted_dev_step(&mut self, arg: &str, ctx: &egui::Context) {
        let (step, value) = arg.split_once('=').unwrap_or((arg, ""));
        match step {
            "notifications" => self.hosted.notifications.open = true,
            "review" => {
                let (base, head) = value.split_once("..").unwrap_or(("main", "HEAD"));
                let cmd = HostedCmd::Review {
                    base: base.into(),
                    head: head.into(),
                };
                self.handle_hosted(cmd, ctx);
            }
            "askpass" => {
                let prompt = if value.is_empty() {
                    "Password for 'https://user@example.com': "
                } else {
                    value
                };
                let (req, _answer) = askpass::Request::new(prompt);
                self.hosted.prompts.push(req);
            }
            "trust" => {
                let (host, port) = ferrum_core::ssh::ssh_host(&format!("ssh://{value}/x"))
                    .unwrap_or_else(|| (value.to_owned(), None));
                self.hosted.trust = Some(askpass_ui::TrustWindow::new(ctx, host, port));
            }
            "ssh-setup" => self.handle_hosted(HostedCmd::SshSetup, ctx),
            "new-issue" => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.view = View::Issues;
                    crate::ui::issues::open_new(t);
                }
            }
            _ => eprintln!("FERRUM_START: unknown hosted step {arg}"),
        }
    }

    /// For tests: answer the prompt shown now.
    #[cfg(test)]
    pub(crate) fn prompts(&mut self) -> &mut askpass_ui::Prompts {
        &mut self.hosted.prompts
    }

    #[cfg(test)]
    pub(crate) fn prompts_shown(&self) -> Option<String> {
        self.hosted.prompts.shown().map(str::to_owned)
    }
}
