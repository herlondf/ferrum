//! The main window: toolbar, tabs, sidebar, the current view and dialogs.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};
use ferrum_core::forge;
use ferrum_core::{RepoState, ops};

mod background;

use crate::command::{Command, Commands, DevStep, Op};
use crate::jobs::{Job, JobOutput, Refresh};
use crate::settings::{STORAGE_KEY, Settings, ThemeChoice};
use crate::tab::{Message, RepoTab, View};
use crate::ui::dialogs::{self, Dialog};
use crate::ui::{
    blame, changes, conflicts, details, forge as forge_ui, history, palette, rebase,
    settings as settings_ui, sidebar,
};

pub struct FerrumApp {
    pub(crate) tabs: Vec<RepoTab>,
    active: usize,
    pub(crate) settings: Settings,
    dialog: Option<Dialog>,
    settings_window: settings_ui::SettingsWindow,
    palette: palette::Palette,
    /// Jobs that do not belong to a tab (clone, init, account checks).
    global_job: Option<Job>,
    message: Option<Message>,
    applied: Option<(ThemeChoice, f32, crate::i18n::Language)>,
    was_focused: bool,
    /// Background reloads, update check and the Activity window.
    bg: background::State,
    /// Dev aid: with `FERRUM_SCREENSHOT=out.ppm`, save the window after a few frames and quit.
    screenshot: Option<(PathBuf, u32)>,
    /// Dev aid: commands from `FERRUM_START`, run on the first frame.
    startup: Vec<Command>,
}

/// Parses `FERRUM_START`, a `;`-separated list of steps used to open a specific
/// screen for screenshots: `changes`, `history`, `commit:N` (N-th commit),
/// `file:PATH`, `blame:PATH`, `conflict:PATH`, `prs`, `rebase:N` (last N
/// commits), `settings`, `palette`, `activity`, `theme:light|dark`, `dialog:branch|push|stash|clone|identity|pr|login`,
/// `select:unstaged:PATH`, `select:staged:PATH`, `search:TEXT`.
fn startup_commands(spec: &str, tabs: &[RepoTab]) -> Vec<Command> {
    let tab = tabs.first();
    let mut out = Vec::new();
    for step in spec.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (cmd, arg) = step.split_once(':').unwrap_or((step, ""));
        match cmd {
            "changes" => out.push(Command::SetView(View::Changes)),
            "history" => out.push(Command::SetView(View::History)),
            "prs" => out.push(Command::SetView(View::PullRequests)),
            "commit" => {
                if let Some(c) = tab.and_then(|t| t.commits.get(arg.parse::<usize>().unwrap_or(0)))
                {
                    out.push(Command::SelectCommit(c.id));
                }
            }
            "file" => out.push(Command::FileHistory(arg.into())),
            "blame" => out.push(Command::Blame(arg.into())),
            "conflict" => out.push(Command::ResolveConflict(arg.into())),
            "rebase" => {
                let n = arg.parse::<usize>().unwrap_or(3);
                let base = tab.and_then(|t| t.repo.resolve(&format!("HEAD~{n}")).ok());
                out.push(Command::PlanRebase(base));
            }
            "settings" => out.push(Command::ShowSettings),
            "theme" => out.push(Command::DevStep(DevStep::Theme(match arg {
                "light" => ThemeChoice::Light,
                "dark" => ThemeChoice::Dark,
                _ => ThemeChoice::System,
            }))),
            "palette" => out.push(Command::ShowPalette),
            "activity" => out.push(Command::ShowActivity),
            "fetch" => out.push(Command::Run(Op::Fetch { remote: None })),
            "search" => out.push(Command::DevStep(DevStep::Search(arg.into()))),
            "select" => {
                if let Some((list, path)) = arg.split_once(':') {
                    let source = if list == "staged" {
                        ferrum_core::DiffSource::Staged
                    } else {
                        ferrum_core::DiffSource::Unstaged
                    };
                    out.push(Command::DevStep(DevStep::Select(source, path.into())));
                }
            }
            "dialog" => {
                let Some(t) = tab else { continue };
                let head = t.head.clone().unwrap_or_else(|| "HEAD".into());
                let d = match arg {
                    "branch" => Dialog::create_branch(&head, &head),
                    "push" => Dialog::push(t, &head),
                    "stash" => Dialog::stash(),
                    "clone" => Dialog::Clone {
                        url: String::new(),
                        parent: String::new(),
                        name: String::new(),
                    },
                    "identity" => Dialog::identity(&t.identity),
                    "pr" => Dialog::Forge(forge_ui::new_pull_request(t, &head)),
                    "login" => {
                        Dialog::Forge(forge_ui::login(forge::ForgeKind::GitHub, "github.com"))
                    }
                    _ => continue,
                };
                out.push(Command::Dialog(d));
            }
            _ => eprintln!("FERRUM_START: unknown step {step}"),
        }
    }
    out
}

impl FerrumApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Vec<PathBuf>) -> Self {
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, STORAGE_KEY))
            .unwrap_or_default();
        let screenshot = std::env::var_os("FERRUM_SCREENSHOT").map(|p| (PathBuf::from(p), 0));
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            settings,
            dialog: None,
            settings_window: Default::default(),
            palette: Default::default(),
            global_job: None,
            message: None,
            applied: None,
            was_focused: true,
            bg: Default::default(),
            screenshot,
            startup: Vec::new(),
        };
        // Repositories given on the command line replace the saved session.
        let (to_open, active) = if paths.is_empty() {
            (app.settings.open_repos.clone(), app.settings.active_repo)
        } else {
            (paths, usize::MAX)
        };
        for p in to_open {
            if p.exists() {
                app.open(p);
            }
        }
        app.active = active.min(app.tabs.len().saturating_sub(1));
        if let Ok(spec) = std::env::var("FERRUM_START") {
            app.startup = startup_commands(&spec, &app.tabs);
        }
        app
    }

    fn open(&mut self, path: PathBuf) {
        match RepoTab::open(path) {
            Ok(tab) => {
                self.settings.add_recent(tab.path().to_path_buf());
                if let Some(i) = self.tabs.iter().position(|t| t.path() == tab.path()) {
                    self.active = i;
                } else {
                    self.tabs.push(tab);
                    self.active = self.tabs.len() - 1;
                }
            }
            Err(e) => self.error(e),
        }
    }

    /// A git operation or other job is running or queued.
    #[cfg(test)]
    pub fn is_busy(&self) -> bool {
        self.global_job.is_some()
            || self
                .tabs
                .iter()
                .any(|t| t.job.is_some() || !t.queue.is_empty() || t.live.refreshing())
    }

    fn tab(&mut self) -> Option<&mut RepoTab> {
        self.tabs.get_mut(self.active)
    }

    fn error(&mut self, text: impl Into<String>) {
        let text = text.into();
        match self.tabs.get_mut(self.active) {
            Some(t) => t.error(text),
            None => {
                self.message = Some(Message {
                    text,
                    error: true,
                    at: Instant::now(),
                })
            }
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        let text = text.into();
        match self.tabs.get_mut(self.active) {
            Some(t) => t.info(text),
            None => {
                self.message = Some(Message {
                    text,
                    error: false,
                    at: Instant::now(),
                })
            }
        }
    }

    fn apply_look(&mut self, ctx: &egui::Context) {
        let want = (
            self.settings.theme,
            self.settings.zoom.clamp(0.6, 2.0),
            self.settings.language,
        );
        if self.applied == Some(want) {
            return;
        }
        crate::i18n::set_language(want.2);
        ctx.set_theme(match want.0 {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        });
        ctx.set_zoom_factor(want.1);
        self.applied = Some(want);
    }

    fn maybe_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.screenshot else {
            return;
        };
        *frames = frames.saturating_add(1);
        ctx.request_repaint();
        // Wait at least FERRUM_SCREENSHOT_FRAMES frames (for loads outside jobs,
        // like blame), and for running jobs, up to ~600 frames.
        let min: u32 = std::env::var("FERRUM_SCREENSHOT_FRAMES")
            .ok()
            .and_then(|f| f.parse().ok())
            .unwrap_or(10);
        let busy =
            self.tabs.iter().any(|t| t.busy() || t.live.refreshing()) || self.global_job.is_some();
        if (min..u32::MAX).contains(frames) && (!busy || *frames >= min.max(600)) {
            *frames = u32::MAX;
            let (path, ctx2) = (path.clone(), ctx.clone());
            ctx.request_screenshot(move |img| {
                let mut out = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
                out.extend(img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b()]));
                if let Err(e) = std::fs::write(&path, out) {
                    eprintln!("screenshot: {e}");
                }
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
    }

    // ----- Jobs -----

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        let mut outputs = Vec::new();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if let Some(out) = tab.poll_job(ctx) {
                outputs.push((Some(i), out));
            }
        }
        if let Some(job) = &self.global_job
            && let Some(result) = job.poll()
        {
            let label = job.label.clone();
            let cancelled = job.cancel_requested();
            self.global_job = None;
            match result {
                Ok(out) => outputs.push((None, out)),
                Err(_) if cancelled => self.info(t!("{} cancelled", label)),
                Err(e) => self.error(t!("{} failed: {}", label, e)),
            }
        }
        for (tab, out) in outputs {
            self.handle_output(tab, out);
        }
    }

    fn handle_output(&mut self, tab: Option<usize>, out: JobOutput) {
        let t = tab.and_then(|i| self.tabs.get_mut(i));
        match out {
            JobOutput::Done => {}
            JobOutput::SearchHits(hits) => {
                if let Some(t) = t {
                    let n = hits.len();
                    t.history.content_hits = Some(hits);
                    t.apply_search();
                    t.info(if n == 1 {
                        t!("1 commit found")
                    } else {
                        t!("{} commits found", n)
                    });
                }
            }
            JobOutput::PullRequests(list) => {
                if let Some(t) = t {
                    t.prs.list = Some(list);
                    t.prs.error = None;
                }
            }
            JobOutput::PullRequestCreated(pr) => {
                if let Some(t) = t {
                    t.info(t!("Created #{} {}", pr.number, pr.title));
                    t.prs.list = None;
                }
                self.load_pull_requests();
            }
            JobOutput::Account(acc) => {
                let msg = t!("Logged in to {} as {}", acc.host, acc.username);
                self.settings
                    .accounts
                    .retain(|a| !(a.kind == acc.kind && a.host.eq_ignore_ascii_case(&acc.host)));
                self.settings.accounts.push(acc);
                self.info(msg);
                if self.tab().is_some_and(|t| t.view == View::PullRequests) {
                    self.load_pull_requests();
                }
            }
            JobOutput::OpenRepo(path) => self.open(path),
        }
    }

    fn spawn_global(
        &mut self,
        label: &str,
        ctx: &egui::Context,
        work: impl FnOnce(&dyn Fn(&str)) -> crate::jobs::JobResult + Send + 'static,
    ) {
        if self.global_job.is_some() {
            self.error(t!("Another operation is still running."));
            return;
        }
        self.global_job = Some(Job::spawn(label, Refresh::None, ctx, work));
    }

    fn load_pull_requests(&mut self) {
        let ctx = self.ctx_handle();
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let Some(hosted) = tab.hosted.clone() else {
            return;
        };
        let Some(token) = self.settings.token_for(hosted.kind, &hosted.host) else {
            let noun = hosted.kind.request_name();
            tab.prs.error = Some(t!("Log in to {} to see {}s.", hosted.host, noun));
            return;
        };
        tab.prs.error = None;
        tab.spawn_job(&t!("Load pull requests"), &ctx, move |_| {
            forge::list_pull_requests(&hosted, &token)
                .map(JobOutput::PullRequests)
                .map_err(|e| e.to_string())
        });
    }

    fn ctx_handle(&self) -> egui::Context {
        CTX.with(|c| c.borrow().clone())
            .expect("context is set every frame")
    }

    // ----- Commands -----

    pub(crate) fn handle(&mut self, cmd: Command, ctx: &egui::Context) {
        match cmd {
            Command::Run(op) => match self.tabs.get_mut(self.active) {
                Some(t) => t.run(op, ctx),
                None => self.error(t!("Open a repository first.")),
            },
            Command::Undo => {
                if let Some(t) = self.tab() {
                    if t.undo.is_empty() {
                        t.info(t!("Nothing to undo."));
                    } else {
                        t.run_undo(ctx);
                    }
                }
            }
            Command::Refresh => {
                if let Some(t) = self.tab() {
                    t.request_refresh(Refresh::All);
                    t.info(t!("Refreshed"));
                }
            }
            Command::OpenRepo(p) => self.open(p),
            Command::OpenRepoDialog => {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    self.open(p);
                }
            }
            Command::CloseTab(i) => {
                if i < self.tabs.len() {
                    self.tabs.remove(i);
                    if self.active >= i && self.active > 0 {
                        self.active -= 1;
                    }
                }
            }
            Command::SelectTab(i) => {
                if i < self.tabs.len() {
                    self.active = i;
                }
            }
            Command::Dialog(d) => self.dialog = Some(d),
            Command::SetView(v) => {
                let load_prs = if let Some(t) = self.tab() {
                    t.view = v;
                    if v == View::Changes && t.changes.selected.is_none() {
                        let first = t
                            .status
                            .unstaged
                            .first()
                            .map(|f| (ferrum_core::DiffSource::Unstaged, f.path.clone()))
                            .or_else(|| {
                                t.status
                                    .staged
                                    .first()
                                    .map(|f| (ferrum_core::DiffSource::Staged, f.path.clone()))
                            });
                        if let Some((s, p)) = first {
                            t.select_change(s, p);
                        }
                    }
                    v == View::PullRequests && t.prs.list.is_none()
                } else {
                    false
                };
                if load_prs {
                    self.load_pull_requests();
                }
            }
            Command::SelectCommit(id) => {
                if let Some(t) = self.tab() {
                    if t.index.contains_key(&id) {
                        t.select_commit(id);
                    } else {
                        t.view = View::History;
                        t.history.selected = Some(id);
                        t.load_commit(id, false);
                    }
                }
            }
            Command::ShowCommit(id) => {
                if let Some(t) = self.tab() {
                    t.view = View::History;
                    t.history.selected = None;
                    let is_stash = t.stashes.iter().any(|s| s.id == id);
                    t.load_commit(id, is_stash);
                }
            }
            Command::FileHistory(path) => {
                if let Some(t) = self.tab() {
                    blame::open(t, path, false);
                }
            }
            Command::Blame(path) => {
                if let Some(t) = self.tab() {
                    blame::open(t, path, true);
                }
            }
            Command::ResolveConflict(path) => {
                if let Some(t) = self.tab() {
                    conflicts::open(t, path);
                    t.view = View::Conflicts;
                }
            }
            Command::PlanRebase(base) => {
                if let Some(t) = self.tab() {
                    match rebase::plan(t, base) {
                        Ok(p) => t.rebase = Some(p),
                        Err(e) => t.error(e),
                    }
                }
            }
            Command::LoadPullRequests => self.load_pull_requests(),
            Command::CreatePullRequest(pr) => {
                // Runs as an `Op` so it waits for a push queued just before it.
                let hosted = self.tab().and_then(|t| t.hosted.clone());
                let token = hosted
                    .as_ref()
                    .and_then(|h| self.settings.token_for(h.kind, &h.host));
                match (hosted, token, self.tabs.get_mut(self.active)) {
                    (Some(hosted), Some(token), Some(t)) => {
                        t.run(Op::CreatePullRequest { hosted, token, pr }, ctx)
                    }
                    _ => self.error(t!("Log in first (Settings > Accounts).")),
                }
            }
            Command::VerifyAccount { kind, host, token } => {
                self.spawn_global(&t!("Check token"), ctx, move |_| {
                    let username =
                        forge::verify_token(kind, &host, &token).map_err(|e| e.to_string())?;
                    Ok(JobOutput::Account(crate::settings::ForgeAccount {
                        kind,
                        host,
                        username,
                        token,
                    }))
                });
            }
            Command::Clone { url, parent, name } => {
                self.spawn_global_cancellable(&t!("Clone {}", name), ctx, move |env| {
                    let git = env.git(&ferrum_core::cli::Git::new(&parent));
                    ops::clone_with(&git, &url, &name, |l| env.progress(l))
                        .map_err(|e| e.to_string())?;
                    Ok(JobOutput::OpenRepo(parent.join(&name)))
                });
            }
            Command::Init(path) => match ops::init(&path) {
                Ok(()) => self.open(path),
                Err(e) => self.error(e.to_string()),
            },
            Command::Error(e) => self.error(e),
            Command::DevStep(step) => match step {
                DevStep::Theme(theme) => self.settings.theme = theme,
                DevStep::Search(q) => {
                    if let Some(t) = self.tab() {
                        t.history.search = q;
                        t.apply_search();
                    }
                }
                DevStep::Select(source, path) => {
                    if let Some(t) = self.tab() {
                        t.view = View::Changes;
                        t.select_change(source, path);
                    }
                }
            },
            Command::Copy(text) => {
                ctx.copy_text(text);
                self.info(t!("Copied to the clipboard"));
            }
            Command::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            Command::ShowSettings => self.settings_window.open = true,
            Command::ShowPalette => self.palette.toggle(),
            Command::ShowActivity => self.bg.activity.toggle(),
            Command::CancelJob => match self.tabs.get_mut(self.active) {
                Some(t) if t.job.is_some() => t.cancel_job(),
                _ => {
                    if let Some(j) = self.global_job.as_ref().filter(|j| j.cancellable()) {
                        j.cancel();
                    }
                }
            },
            Command::FocusSearch => {
                if let Some(t) = self.tab() {
                    t.view = View::History;
                    t.history.focus_search = true;
                }
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, cmds: &mut Commands) {
        let cmd = Modifiers::COMMAND;
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let pressed = |m: Modifiers, k: Key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)))
        };
        if pressed(cmd, Key::P) || pressed(cs, Key::P) {
            cmds.push(Command::ShowPalette);
        }
        if pressed(cmd, Key::O) {
            cmds.push(Command::OpenRepoDialog);
        }
        if pressed(cmd, Key::Comma) {
            cmds.push(Command::ShowSettings);
        }
        if self.tabs.is_empty() || self.dialog.is_some() {
            return;
        }
        if pressed(Modifiers::NONE, Key::F5) || pressed(cmd, Key::R) {
            cmds.push(Command::Refresh);
        }
        if pressed(cmd, Key::F) {
            cmds.push(Command::FocusSearch);
        }
        if pressed(cmd, Key::Num1) {
            cmds.push(Command::SetView(View::History));
        }
        if pressed(cmd, Key::Num2) {
            cmds.push(Command::SetView(View::Changes));
        }
        if pressed(cmd, Key::B) {
            let head = self.tabs[self.active]
                .head
                .clone()
                .unwrap_or_else(|| "HEAD".into());
            cmds.push(Command::Dialog(Dialog::create_branch(&head, &head)));
        }
        if pressed(cs, Key::S) {
            cmds.push(Command::Dialog(Dialog::stash()));
        }
        if pressed(cs, Key::F) {
            cmds.run(Op::Fetch { remote: None });
        }
        if pressed(cs, Key::D) {
            cmds.run(Op::Pull(self.settings.pull.mode()));
        }
        if pressed(cs, Key::U)
            && let Some(b) = self.tabs[self.active].head.clone()
        {
            cmds.push(Command::Dialog(Dialog::push(&self.tabs[self.active], &b)));
        }
        if pressed(cmd, Key::W) {
            cmds.push(Command::CloseTab(self.active));
        }
        if pressed(cmd, Key::Tab) {
            cmds.push(Command::SelectTab((self.active + 1) % self.tabs.len()));
        }
        if pressed(cs, Key::Tab) {
            cmds.push(Command::SelectTab(
                (self.active + self.tabs.len() - 1) % self.tabs.len(),
            ));
        }
        // Ctrl+Z belongs to text fields while one is focused.
        let typing = ctx.memory(|m| m.focused().is_some());
        if !typing && pressed(cmd, Key::Z) {
            cmds.push(Command::Undo);
        }
    }

    // ----- Layout -----

    fn toolbar(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        ui.horizontal(|ui| {
            ui.menu_button(t!("📂 Repository"), |ui| {
                if ui.button(t!("Open…  (Ctrl+O)")).clicked() {
                    cmds.push(Command::OpenRepoDialog);
                    ui.close();
                }
                if ui.button(t!("Clone…")).clicked() {
                    cmds.push(Command::Dialog(Dialog::clone_repo(&self.settings)));
                    ui.close();
                }
                if ui.button(t!("New repository…")).clicked() {
                    cmds.push(Command::Dialog(Dialog::Init {
                        path: String::new(),
                    }));
                    ui.close();
                }
                if !self.settings.recent.is_empty() {
                    ui.separator();
                    ui.weak(t!("Recent"));
                    for p in self.settings.recent.clone() {
                        if ui.button(p.display().to_string()).clicked() {
                            cmds.push(Command::OpenRepo(p));
                            ui.close();
                        }
                    }
                }
            });
            let Some(tab) = self.tabs.get(self.active) else {
                return;
            };
            ui.separator();
            let busy = tab.busy();
            let has_remote = !tab.remotes.is_empty();
            ui.add_enabled_ui(!busy, |ui| {
                if ui
                    .add_enabled(has_remote, egui::Button::new("🔄 Fetch"))
                    .on_hover_text(t!("Fetch all remotes (Ctrl+Shift+F)"))
                    .clicked()
                {
                    cmds.run(Op::Fetch { remote: None });
                }
                let pull = ui.add_enabled(
                    has_remote && tab.head.is_some(),
                    egui::Button::new("⬇ Pull"),
                );
                let mode = self.settings.pull.label();
                let tip = t!("Pull — {} (Ctrl+Shift+D)\nRight-click for options", mode);
                if pull.on_hover_text(tip).clicked() {
                    cmds.run(Op::Pull(self.settings.pull.mode()));
                }
                if ui
                    .add_enabled(
                        has_remote && tab.head.is_some(),
                        egui::Button::new("⬆ Push"),
                    )
                    .on_hover_text(t!("Push (Ctrl+Shift+U)"))
                    .clicked()
                    && let Some(b) = &tab.head
                {
                    cmds.push(Command::Dialog(Dialog::push(tab, b)));
                }
                ui.menu_button("☰", |ui| {
                    if ui.button(t!("Pull with options…")).clicked() {
                        cmds.push(Command::Dialog(Dialog::Pull));
                        ui.close();
                    }
                    if ui.button(t!("Add remote…")).clicked() {
                        cmds.push(Command::Dialog(Dialog::AddRemote {
                            name: String::new(),
                            url: String::new(),
                        }));
                        ui.close();
                    }
                    if tab.uses_lfs {
                        ui.separator();
                        for a in ["fetch", "pull", "prune"] {
                            if ui.button(format!("Git LFS {a}")).clicked() {
                                cmds.run(Op::Lfs(a));
                                ui.close();
                            }
                        }
                    }
                    if !tab.submodules.is_empty() && ui.button(t!("Update submodules")).clicked() {
                        cmds.run(Op::SubmoduleUpdate);
                        ui.close();
                    }
                });
                ui.separator();
                if ui
                    .button("🌿 Branch")
                    .on_hover_text(t!("New branch (Ctrl+B)"))
                    .clicked()
                {
                    let head = tab.head.clone().unwrap_or_else(|| "HEAD".into());
                    cmds.push(Command::Dialog(Dialog::create_branch(&head, &head)));
                }
                if ui
                    .add_enabled(!tab.status.is_empty(), egui::Button::new("📦 Stash"))
                    .on_hover_text(t!("Stash changes (Ctrl+Shift+S)"))
                    .clicked()
                {
                    cmds.push(Command::Dialog(Dialog::stash()));
                }
                if let Some(s) = tab.stashes.first()
                    && ui
                        .button(t!("📤 Pop"))
                        .on_hover_text(t!("Apply and delete the latest stash: {}", s.message))
                        .clicked()
                {
                    cmds.run(Op::StashApply {
                        stash: s.clone(),
                        pop: true,
                    });
                }
                let undo = tab.undo.last().map(|u| u.label.clone());
                let resp = ui.add_enabled(undo.is_some(), egui::Button::new(t!("↩ Undo")));
                if let Some(label) = undo
                    && resp.on_hover_text(t!("Undo: {} (Ctrl+Z)", label)).clicked()
                {
                    cmds.push(Command::Undo);
                }
            });
            ui.separator();
            crate::ui::activity::status_control(ui, tab, self.global_job.as_ref(), cmds);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("⚙")
                    .on_hover_text(t!("Settings (Ctrl+,)"))
                    .clicked()
                {
                    cmds.push(Command::ShowSettings);
                }
                if ui
                    .button("⌘")
                    .on_hover_text(t!("Command palette (Ctrl+P)"))
                    .clicked()
                {
                    cmds.push(Command::ShowPalette);
                }
                self.update_link(ui);
            });
        });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        ui.horizontal_wrapped(|ui| {
            for (i, tab) in self.tabs.iter().enumerate() {
                let mut label = tab.name.clone();
                if let Some(b) = &tab.head {
                    label.push_str(&format!("  🌿 {b}"));
                }
                if tab.busy() {
                    label.push_str(" ⏳");
                }
                let resp = ui
                    .selectable_label(i == self.active, label)
                    .on_hover_text(tab.path().display().to_string());
                if resp.clicked() {
                    cmds.push(Command::SelectTab(i));
                }
                if resp.middle_clicked()
                    || ui
                        .small_button("×")
                        .on_hover_text(t!("Close (Ctrl+W)"))
                        .clicked()
                {
                    cmds.push(Command::CloseTab(i));
                }
                ui.separator();
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        ui.horizontal(|ui| {
            let tab = self.tabs.get(self.active);
            let job = tab
                .and_then(|t| t.job.as_ref())
                .or(self.global_job.as_ref());
            if let Some(job) = job {
                ui.spinner();
                let progress = job.progress();
                ui.label(if progress.is_empty() {
                    job.label.clone()
                } else {
                    format!("{} — {progress}", job.label)
                });
                if let Some(t) = tab
                    && !t.queue.is_empty()
                {
                    ui.weak(t!("(+{} queued)", t.queue.len()));
                }
            } else {
                let msg = tab
                    .and_then(|t| t.message.as_ref())
                    .or(self.message.as_ref());
                if let Some(m) = msg {
                    let fresh = m.at.elapsed() < Duration::from_secs(10);
                    if m.error {
                        let first = m.text.lines().next().unwrap_or_default();
                        let resp =
                            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {first}"));
                        if m.text.lines().count() > 1 {
                            resp.on_hover_text(&m.text);
                        }
                        if ui.small_button(t!("Copy")).clicked() {
                            cmds.push(Command::Copy(m.text.clone()));
                        }
                    } else if fresh {
                        ui.label(&m.text);
                    }
                }
            }
            let Some(tab) = tab else { return };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let id = &tab.identity;
                let who = match (&id.name, &id.email) {
                    (Some(n), Some(e)) => format!("👤 {n} <{e}>"),
                    _ => t!("👤 No identity"),
                };
                if ui
                    .small_button(who)
                    .on_hover_text(t!("Author identity for this repository"))
                    .clicked()
                {
                    cmds.push(Command::Dialog(Dialog::identity(&tab.identity)));
                }
                if tab.uses_lfs {
                    ui.weak("LFS");
                }
                if let Some(b) = tab.current_branch()
                    && let Some((a, bh)) = b.ahead_behind
                    && (a > 0 || bh > 0)
                {
                    ui.weak(format!("⬆{a} ⬇{bh}"));
                }
            });
        });
    }

    fn banner(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if tab.state == RepoState::Clean {
            return;
        }
        let conflicts = tab.status.conflicted.len();
        let fill = ui.visuals().warn_fg_color.gamma_multiply(0.25);
        egui::Frame::new()
            .fill(fill)
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut text = format!("⚠ {}", crate::util::state_label(tab.state));
                    if conflicts == 1 {
                        text.push_str(&t!(" — 1 file with conflicts"));
                    } else if conflicts > 1 {
                        text.push_str(&t!(" — {} files with conflicts", conflicts));
                    }
                    ui.strong(text);
                    ui.add_enabled_ui(!tab.busy(), |ui| {
                        if conflicts > 0 && ui.button(t!("Resolve conflicts")).clicked() {
                            cmds.push(Command::ResolveConflict(
                                tab.status.conflicted[0].path.clone(),
                            ));
                        }
                        let can_continue = conflicts == 0 && tab.state != RepoState::Bisect;
                        if ui
                            .add_enabled(can_continue, egui::Button::new(t!("Continue")))
                            .clicked()
                        {
                            cmds.run(Op::Continue(tab.state));
                        }
                        if matches!(
                            tab.state,
                            RepoState::Rebase | RepoState::CherryPick | RepoState::Revert
                        ) && ui.button(t!("Skip")).clicked()
                        {
                            cmds.run(Op::Skip(tab.state));
                        }
                        if ui.button(t!("Abort")).clicked() {
                            let name = crate::util::state_name(tab.state);
                            // One line, so the translation tests find the key.
                            #[rustfmt::skip]
                            let text = t!("Abort the {} in progress? Changes made by it will be thrown away.", name);
                            cmds.push(Command::Dialog(Dialog::confirm(
                                &t!("Abort"),
                                &text,
                                Op::Abort(tab.state),
                                true,
                            )));
                        }
                    });
                });
            });
    }

    fn welcome(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 5.0);
            ui.heading(RichText::new("Ferrum").size(32.0));
            ui.weak(t!("A fast Git client"));
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 360.0).max(0.0) / 2.0);
                if ui
                    .add(egui::Button::new(t!("📂 Open…")).min_size(egui::vec2(110.0, 32.0)))
                    .clicked()
                {
                    cmds.push(Command::OpenRepoDialog);
                }
                if ui
                    .add(egui::Button::new(t!("⬇ Clone…")).min_size(egui::vec2(110.0, 32.0)))
                    .clicked()
                {
                    cmds.push(Command::Dialog(Dialog::clone_repo(&self.settings)));
                }
                if ui
                    .add(egui::Button::new(t!("✚ New…")).min_size(egui::vec2(110.0, 32.0)))
                    .clicked()
                {
                    cmds.push(Command::Dialog(Dialog::Init {
                        path: String::new(),
                    }));
                }
            });
            if !self.settings.recent.is_empty() {
                ui.add_space(24.0);
                ui.strong(t!("Recent repositories"));
                for p in self.settings.recent.clone() {
                    if ui.link(p.display().to_string()).clicked() {
                        cmds.push(Command::OpenRepo(p));
                    }
                }
            }
        });
    }
}

thread_local! {
    /// The egui context, so helpers without a `ctx` parameter can start jobs.
    static CTX: std::cell::RefCell<Option<egui::Context>> = const { std::cell::RefCell::new(None) };
}

impl eframe::App for FerrumApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        CTX.with(|c| *c.borrow_mut() = Some(ctx.clone()));
        self.maybe_screenshot(&ctx);
        self.apply_look(&ctx);
        self.poll_jobs(&ctx);
        self.background_tick(&ctx);

        let mut cmds = Commands::default();
        for c in std::mem::take(&mut self.startup) {
            cmds.push(c);
        }
        self.shortcuts(&ctx, &mut cmds);

        let title = match self.tabs.get(self.active) {
            Some(t) => format!(
                "{} — {} — Ferrum",
                t.name,
                t.head.clone().unwrap_or_else(|| t!("detached"))
            ),
            None => "Ferrum".into(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(2.0);
            self.toolbar(ui, &mut cmds);
            if !self.tabs.is_empty() {
                ui.separator();
                self.tab_bar(ui, &mut cmds);
            }
            ui.add_space(2.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui, &mut cmds));

        if self.tabs.is_empty() {
            egui::CentralPanel::default_margins().show(ui, |ui| self.welcome(ui, &mut cmds));
        } else {
            let state_banner = self.tabs[self.active].state != RepoState::Clean;
            if state_banner {
                egui::Panel::top("banner").show(ui, |ui| self.banner(ui, &mut cmds));
            }
            let settings = &self.settings;
            let tab = &mut self.tabs[self.active];
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(230.0)
                .show(ui, |ui| {
                    sidebar::show(ui, tab, &mut cmds);
                });
            match tab.view {
                View::History => {
                    egui::Panel::bottom("details")
                        .resizable(true)
                        .default_size(300.0)
                        .show(ui, |ui| {
                            details::show(ui, tab, &mut cmds);
                        });
                    egui::CentralPanel::default_margins().show(ui, |ui| {
                        history::search_bar(ui, tab, &mut cmds);
                        history::commit_list(ui, tab, &mut cmds);
                    });
                }
                View::Changes => {
                    egui::CentralPanel::default_margins()
                        .show(ui, |ui| changes::show(ui, tab, &mut cmds));
                }
                View::Conflicts => {
                    egui::CentralPanel::default_margins()
                        .show(ui, |ui| conflicts::show(ui, tab, &mut cmds));
                }
                View::File => {
                    egui::CentralPanel::default_margins()
                        .show(ui, |ui| blame::show(ui, tab, &mut cmds));
                }
                View::PullRequests => {
                    egui::CentralPanel::default_margins()
                        .show(ui, |ui| forge_ui::show_view(ui, tab, settings, &mut cmds));
                }
            }
            if tab.rebase.is_some() {
                rebase::show(&ctx, tab, &mut cmds);
            }
        }

        if let Some(mut d) = self.dialog.take() {
            let keep = dialogs::show(
                &ctx,
                &mut d,
                self.tabs.get(self.active),
                &mut self.settings,
                &mut cmds,
            );
            if keep && self.dialog.is_none() {
                self.dialog = Some(d);
            }
        }
        settings_ui::show(
            &ctx,
            &mut self.settings_window,
            &mut self.settings,
            &mut cmds,
        );
        let names: Vec<String> = self.tabs.iter().map(|t| t.name.clone()).collect();
        palette::show(
            &ctx,
            &mut self.palette,
            self.tabs.get(self.active),
            &names,
            &self.settings,
            &mut cmds,
        );

        self.show_activity(&ctx);

        for c in cmds.take() {
            self.handle(c, &ctx);
        }
        if self.tabs.iter().any(|t| t.job.is_some()) || self.global_job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.open_repos = self.tabs.iter().map(|t| t.path().to_path_buf()).collect();
        self.settings.active_repo = self.active;
        eframe::set_value(storage, STORAGE_KEY, &self.settings);
    }
}
