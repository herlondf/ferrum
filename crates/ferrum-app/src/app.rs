//! The main window: toolbar, tabs, sidebar, the current view and dialogs.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use ferrum_core::forge;
use ferrum_core::{RepoState, ops};

mod background;
mod hosted;
mod parts;
mod repos;
mod safety;

use crate::command::{Command, Commands, DevStep, Op};
use crate::icons;
use crate::jobs::{Job, JobOutput, Refresh};
use crate::settings::{STORAGE_KEY, Settings, ThemeChoice};
use crate::tab::{Message, RepoTab, View};
use crate::ui::dialogs::Dialog;
use crate::ui::{
    blame, changes, conflicts, details, forge as forge_ui, history, history_model, history_options,
    palette, rebase, settings as settings_ui, sidebar,
};
use crate::ui::{focus, sidebar_model};

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
    /// Password prompts, notifications, AI review and SSH windows.
    hosted: hosted::State,
    /// Dev aid: with `FERRUM_SCREENSHOT=out.ppm`, save the window after a few frames and quit.
    screenshot: Option<(PathBuf, u32)>,
    /// Dev aid: commands from `FERRUM_START`, run on the first frame.
    startup: Vec<Command>,
    /// Repository Manager tab, closed tabs, workspaces, tab windows.
    repos: repos::State,
    /// Notifications in the bottom-right corner.
    toasts: crate::ui::toasts::Toasts,
    /// Operation timeline, secrets warning, learning panel.
    safety: safety::State,
}

/// Parses `FERRUM_START`, a `;`-separated list of steps used to open a specific
/// screen for screenshots: `changes`, `history`, `commit:N` (N-th commit),
/// `file:PATH`, `blame:PATH`, `lines:PATH:A-B`, `finder:TEXT`, `file-window`,
/// `conflict:PATH`, `prs`, `rebase:N` (last N
/// commits), `settings`, `settings:SECTION` (appearance, git, tools, shortcuts,
/// commands...), `palette`, `palette:QUERY`, `menu:N` (main menu N),
/// `theme:light|dark`, `dialog:branch|push|stash|clone|identity|pr|login|
/// submodule|submodule-update|lfs`,
/// `dialog:fetch|pull|push-many|push-tags|track|remote`,
/// `select:unstaged:PATH`, `select:staged:PATH`, `search:TEXT`,
/// `sidebar:open:KEY|pin:REF|filter:TEXT|focus|drop:SRC>DST` (see `sidebar::dev_step`),
/// `activity`, `fetch`, `stats`, `collapse-all`, `vertical`,
/// `graph:reflog|first-parent|topo|no-remotes|only=REF|hide=REF`,
/// `local:tree|flat|combined|amend|templates|msg=TEXT` (local changes view),
/// `diff:OPTS` (see `diff_prefs::dev_step`), `details:STEP` (see `details::dev_step`),
/// `flow:merge=REV|rebase=REV|pick=REV,REV|revert=REV|reword=REV|select=REV,REV` (see `flow::dev_steps`),
/// `gitflow:init|start=KIND|finish=BRANCH`, `bisect:dialog|start=BAD,GOOD`, `patch:FILE`,
/// `manager`, `quick:TEXT`, `overview`, `repo-settings`, `workspace:NAME`, `vertical-tabs` (see `app/repos.rs`),
/// `toast:success|info|error` (see `toasts::dev_step`),
/// `timeline`, `restore:N`, `learning`, `secrets:scan` (see `app/safety.rs`).
fn startup_commands(spec: &str, tabs: &[RepoTab]) -> Vec<Command> {
    let tab = tabs.first();
    let mut out = Vec::new();
    for step in spec.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (cmd, arg) = step.split_once(':').unwrap_or((step, ""));
        match cmd {
            "changes" => out.push(Command::SetView(View::Changes)),
            "history" => out.push(Command::SetView(View::History)),
            "prs" => out.push(Command::SetView(View::PullRequests)),
            "issues" => out.push(Command::SetView(View::Issues)),
            // hosted:notifications|review=BASE..HEAD|askpass=PROMPT|trust=HOST|ssh-setup…
            "hosted" => out.push(Command::Hosted(crate::hosted::HostedCmd::Dev(arg.into()))),
            "commit" => {
                if let Some(c) = tab.and_then(|t| t.commits.get(arg.parse::<usize>().unwrap_or(0)))
                {
                    out.push(Command::SelectCommit(c.id));
                }
            }
            "file" => out.push(Command::FileHistory(arg.into())),
            "blame" => out.push(Command::Blame(arg.into())),
            "lines" | "finder" | "file-window" => {
                out.push(crate::ui::file_window::dev_step(cmd, arg))
            }
            "conflict" => out.push(Command::ResolveConflict(arg.into())),
            "rebase" => {
                let n = arg.parse::<usize>().unwrap_or(3);
                let base = tab.and_then(|t| t.repo.resolve(&format!("HEAD~{n}")).ok());
                out.push(Command::PlanRebase(base));
            }
            "settings" if arg.is_empty() => out.push(Command::ShowSettings),
            // settings:appearance|git|tools|profiles|...
            "settings" => out.push(Command::DevStep(DevStep::SettingsSection(arg.into()))),
            "density" => out.push(Command::DevStep(DevStep::Density(match arg {
                "compact" => crate::theme::Density::Compact,
                _ => crate::theme::Density::Comfortable,
            }))),
            "theme" => out.push(Command::DevStep(DevStep::Theme(match arg {
                "light" => ThemeChoice::Light,
                "dark" => ThemeChoice::Dark,
                "contrast" => ThemeChoice::HighContrast,
                _ => ThemeChoice::System,
            }))),
            "palette" if arg.is_empty() => out.push(Command::ShowPalette),
            "palette" => out.push(Command::DevStep(DevStep::Palette(arg.into()))),
            "activity" => out.push(Command::ShowActivity),
            "menu" => out.push(Command::DevStep(DevStep::Menu(arg.parse().unwrap_or(0)))),
            "toast" => out.push(Command::DevStep(DevStep::Toast(step.into()))),
            "timeline" | "restore" | "learning" | "secrets" => out.push(safety::dev_command(step)),
            "fetch" => out.push(Command::Run(Op::Fetch { remote: None })),
            "stats" => out.push(history_options::cmd(|t, _| crate::ui::stats::open(t))),
            "collapse-all" => out.push(history_options::cmd(|t, _| {
                history_model::set_all_collapsed(t, true)
            })),
            "vertical" => out.push(history_options::cmd(|_, s| s.vertical_layout = true)),
            "relative-dates" => out.push(history_options::cmd(|_, s| s.relative_dates = true)),
            "graph" => {
                // graph:reflog, graph:first-parent, graph:only=refs/heads/x…
                let arg = arg.to_owned();
                out.push(history_options::change_log(move |o| {
                    match arg.split_once('=') {
                        Some(("only", r)) => o.include = vec![r.to_owned()],
                        Some(("hide", r)) => o.hidden.push(r.to_owned()),
                        _ => match arg.as_str() {
                            "reflog" => o.reflog = true,
                            "first-parent" => o.first_parent = true,
                            "topo" => o.topo_order = true,
                            "no-remotes" => o.hide_remotes = true,
                            _ => eprintln!("FERRUM_START: unknown graph option {arg}"),
                        },
                    }
                }));
            }
            "search" => out.push(Command::DevStep(DevStep::Search(arg.into()))),
            "sidebar" => out.push(Command::DevStep(DevStep::Sidebar(arg.into()))),
            "local" => out.push(Command::DevStep(DevStep::Changes(arg.into()))),
            "diff" => out.push(Command::DevStep(DevStep::Diff(arg.into()))),
            "details" => out.push(details::dev_step(arg)),
            "flow" => out.extend(crate::flow::dev_steps(arg, tab)),
            "gitflow" => out.extend(crate::ui::gitflow::dev_step(arg, tab)),
            "bisect" => out.extend(crate::ui::bisect::dev_step(arg, tab)),
            "patch" => out.extend(crate::ui::patches::dev_step(arg, tab)),
            "manager" | "quick" | "overview" | "repo-settings" | "workspace" | "vertical-tabs" => {
                out.push(Command::Repos(crate::repos::RepoCmd::Dev(step.into())))
            }
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
                    "pull" => Dialog::pull(t),
                    "fetch" => Dialog::Sync(crate::ui::sync_dialogs::fetch(t, None)),
                    "push-tags" => Dialog::Sync(crate::ui::sync_dialogs::push_tags(t, &[])),
                    "push-many" => {
                        let names = t
                            .branches
                            .iter()
                            .filter(|b| b.kind == ferrum_core::BranchKind::Local);
                        let names = names.map(|b| b.name.clone()).collect();
                        Dialog::Sync(crate::ui::sync_dialogs::push_many(t, names))
                    }
                    "track" => match t.current_branch() {
                        Some(b) => Dialog::Sync(crate::ui::sync_dialogs::track(t, b)),
                        None => continue,
                    },
                    "remote" => match t.remotes.first() {
                        Some(r) => {
                            Dialog::Sync(crate::ui::sync_dialogs::edit_remote(&r.name, &r.url))
                        }
                        None => continue,
                    },
                    "stash" => Dialog::stash(),
                    "checkout" => crate::ui::ref_dialogs::quick_checkout(),
                    "tag" => Dialog::create_tag("HEAD", &head),
                    "rename" => crate::ui::ref_dialogs::rename_branch(t, &head),
                    "stashes" => crate::ui::ref_dialogs::drop_stashes(t),
                    a if a.starts_with("tag=") => crate::ui::ref_dialogs::tag_details(t, &a[4..]),
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
                    "submodule" => Dialog::Submodule(crate::ui::submodules::SubmoduleDialog::add()),
                    "submodule-update" => {
                        Dialog::Submodule(crate::ui::submodules::SubmoduleDialog::update(t, None))
                    }
                    "lfs" => Dialog::Lfs(crate::ui::lfs::LfsDialog::track(t)),
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
            hosted: Default::default(),
            screenshot,
            startup: Vec::new(),
            repos: Default::default(),
            toasts: Default::default(),
            safety: Default::default(),
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

    /// Requests from the command line besides repositories to open, and the
    /// listener for other Ferrum processes.
    pub fn with_launch(
        mut self,
        requests: Vec<crate::repos::Request>,
        listener: Option<crate::single_instance::Listener>,
    ) -> Self {
        self.repos.pending = requests;
        self.repos.listener = listener;
        self
    }

    fn open(&mut self, path: PathBuf) {
        match RepoTab::open_with(path.clone(), &self.settings.history) {
            Ok(mut tab) => {
                let prefs = crate::repo_settings::sidebar_prefs(&self.settings, &tab);
                tab.sidebar = sidebar_model::SidebarState::with_prefs(prefs);
                self.settings.add_recent(tab.path().to_path_buf());
                self.opened(tab.path());
                if let Some(i) = self.tabs.iter().position(|t| t.path() == tab.path()) {
                    self.active = i;
                } else {
                    self.tabs.push(tab);
                    self.active = self.tabs.len() - 1;
                }
            }
            Err(e) => {
                if !self.open_failed(&path, &e) {
                    self.error(e)
                }
            }
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
        let p = &self.settings.prefs;
        crate::prefs::apply(ctx, p);
        let accent = p
            .system_accent
            .then(|| crate::prefs::system_accent(ctx))
            .flatten();
        crate::theme::apply(ctx, self.settings.theme, p.density, accent);
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
            ThemeChoice::Dark | ThemeChoice::HighContrast => egui::ThemePreference::Dark,
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
            JobOutput::PathHits(hits) => {
                if let Some(t) = t {
                    let n = hits.len();
                    history_model::set_path_hits(t, hits);
                    t.info(t!("{} commits found", n));
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
                // Several accounts per service; logging in again replaces the token.
                self.settings.accounts.retain(|a| {
                    !(a.kind == acc.kind
                        && a.host.eq_ignore_ascii_case(&acc.host)
                        && a.username == acc.username)
                });
                self.settings.accounts.push(acc);
                self.hosted.notifications_changed();
                self.info(msg);
                if self.tab().is_some_and(|t| t.view == View::PullRequests) {
                    self.load_pull_requests();
                }
            }
            JobOutput::OpenRepo(path) => self.open(path),
            JobOutput::Secrets(held) => self.secrets_found(tab, *held),
            JobOutput::Clipboard(text) => {
                self.ctx_handle().copy_text(text);
                self.info(t!("Copied to the clipboard"));
            }
            JobOutput::SubmodulesOutOfDate(list) => {
                if tab == Some(self.active) {
                    self.ask_submodule_update(list);
                }
            }
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
            // Checkouts first ask what to do with local changes when needed.
            Command::Run(op) => match self
                .tabs
                .get(self.active)
                .map(|t| crate::flow::checkout(t, op, self.settings.prefs.ask_local_changes))
            {
                Some(Command::Run(op)) => {
                    let t = &mut self.tabs[self.active];
                    let op = crate::sync::with_options(op, &self.settings.sync);
                    self.toasts.started(t, &op);
                    let then = crate::repo_parts::after(&op, t, &self.settings.submodules);
                    let op = crate::safety::wrap(op, &self.settings, t);
                    t.run(op, ctx);
                    if let Some(next) = then {
                        t.run(next, ctx);
                    }
                }
                Some(prompt) => self.handle(prompt, ctx),
                None => self.error(t!("Open a repository first.")),
            },
            Command::Undo => self.undo(ctx),
            Command::Refresh => {
                if let Some(t) = self.tab() {
                    t.request_refresh(Refresh::All);
                    t.info(t!("Refreshed"));
                }
            }
            Command::OpenRepo(p) => self.open(p),
            Command::OpenBeside(p) => self.open_beside(p),
            Command::OpenRepoDialog => {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    self.open(p);
                }
            }
            Command::CloseTab(i) => self.close_tab(i),
            Command::SelectTab(i) => {
                if i < self.tabs.len() {
                    self.active = i;
                    self.repos.home_active = false;
                }
            }
            Command::Repos(c) => self.handle_repos(c, ctx),
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
                let h = &self.settings.history;
                let avatars = (h.show_avatars, h.network_avatars);
                if let Some(t) = self.tab() {
                    match rebase::plan(t, base) {
                        Ok(mut p) => {
                            p.avatars = avatars;
                            t.rebase = Some(p);
                        }
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
                        token: token.token,
                        user: token.user,
                        notifications: true,
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
            Command::External(launch) => {
                if let Err(e) = crate::external::launch(&self.settings.external, &launch) {
                    self.error(e);
                }
            }
            Command::SetGlobalIdentity(id) => {
                self.spawn_global(&t!("Set global identity"), ctx, move |_| {
                    let git = ferrum_core::cli::Git::new(std::env::temp_dir());
                    ferrum_core::config::set_global_identity(&git, &id)
                        .map(|()| JobOutput::Done)
                        .map_err(|e| e.to_string())
                });
            }
            Command::DevStep(step) => match step {
                DevStep::SettingsSection(name) => {
                    self.settings_window.open = true;
                    self.settings_window.select(&name);
                }
                DevStep::Theme(theme) => self.settings.theme = theme,
                DevStep::Density(d) => self.settings.prefs.density = d,
                DevStep::Menu(i) => crate::ui::main_menu::open(ctx, i),
                DevStep::Toast(step) => crate::ui::toasts::dev_step(&mut self.toasts, &step),
                DevStep::Palette(q) => self.palette.open_with(&q),
                DevStep::Search(q) => {
                    if let Some(t) = self.tab() {
                        t.history.search = q;
                        t.apply_search();
                    }
                }
                DevStep::Diff(arg) => {
                    crate::ui::diff_prefs::dev_step(ctx, &mut self.settings.diff, &arg)
                }
                DevStep::Select(source, path) => {
                    if let Some(t) = self.tab() {
                        t.view = View::Changes;
                        t.select_change(source, path);
                    }
                }
                DevStep::Sidebar(arg) => {
                    if let Some(t) = self.tab() {
                        sidebar::dev_step(t, &arg);
                    }
                }
                DevStep::Changes(arg) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        changes::dev_step(t, &mut self.settings, &arg);
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
            Command::History(f) => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    f(t, &mut self.settings.history);
                    let history = &mut self.settings.history;
                    crate::repo_settings::store_history(history, &mut self.tabs, self.active);
                }
            }
            Command::Sync(f) => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    f(t, &mut self.settings.sync);
                }
            }
            Command::Custom(inv) => {
                let tab = self.tabs.get(self.active);
                for c in crate::custom_commands::resolve(inv, &self.settings, tab) {
                    self.handle(c, ctx);
                }
            }
            Command::Hosted(c) => self.handle_hosted(c, ctx),
            Command::Safety(c) => self.handle_safety(c, ctx),
            Command::FocusSearch => {
                if let Some(t) = self.tab() {
                    t.view = View::History;
                    t.history.focus_search = true;
                }
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, cmds: &mut Commands) {
        use crate::keymap::Action as A;
        let pressed = |a| crate::keymap::pressed(ctx, a);
        // Tabs, Open Quickly and closing the Repository Manager tab.
        self.repo_shortcuts(ctx, cmds);
        if pressed(A::Palette) {
            cmds.push(Command::ShowPalette);
        }
        if pressed(A::OpenRepo) {
            cmds.push(Command::OpenRepoDialog);
        }
        if pressed(A::Settings) {
            cmds.push(Command::ShowSettings);
        }
        if pressed(A::Quit) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.showing_home() || self.dialog.is_some() {
            return;
        }
        if pressed(A::Refresh) {
            cmds.push(Command::Refresh);
        }
        // Ctrl+F searches the diff when it has the pointer or focus, else the history.
        if pressed(A::Find) && !crate::ui::diff_view::claim_find(ctx) {
            cmds.push(Command::FocusSearch);
        }
        // Ctrl+G is Go to line in the blame under the pointer.
        let _ = crate::ui::blame_view::claim_goto(ctx);
        history_options::shortcuts(ctx, &self.tabs[self.active], cmds);
        if pressed(A::ShowHistory) {
            cmds.push(Command::SetView(View::History));
        }
        if pressed(A::ShowChanges) {
            cmds.push(Command::SetView(View::Changes));
        }
        crate::ui::ref_dialogs::shortcuts(ctx, &self.tabs[self.active], cmds);
        if pressed(A::Stash) {
            cmds.push(Command::Dialog(Dialog::stash()));
        }
        if pressed(A::Fetch) {
            cmds.run(Op::Fetch { remote: None });
        }
        if pressed(A::Pull) {
            cmds.run(Op::Pull(self.settings.pull.mode()));
        }
        if pressed(A::Push)
            && let Some(b) = self.tabs[self.active].head.clone()
        {
            cmds.push(Command::Dialog(Dialog::push(&self.tabs[self.active], &b)));
        }
        let dir = self.tabs[self.active].path().to_path_buf();
        if pressed(A::Terminal) {
            cmds.push(Command::External(crate::external::Launch::Terminal {
                dir: dir.clone(),
            }));
        }
        if pressed(A::FileManager) {
            cmds.push(Command::External(crate::external::Launch::Reveal {
                path: dir,
            }));
        }
        if pressed(A::CloseTab) {
            cmds.push(Command::CloseTab(self.active));
        }
        if pressed(A::NextTab) {
            cmds.push(Command::SelectTab((self.active + 1) % self.tabs.len()));
        }
        if pressed(A::PrevTab) {
            cmds.push(Command::SelectTab(
                (self.active + self.tabs.len() - 1) % self.tabs.len(),
            ));
        }
        // Ctrl+Z belongs to text fields while one is focused.
        let typing = ctx.memory(|m| m.focused().is_some());
        if !typing && pressed(A::Undo) {
            cmds.push(Command::Undo);
        }
        if pressed(A::Timeline) {
            cmds.push(Command::Safety(crate::safety::SafetyCmd::ToggleTimeline));
        }
    }

    // ----- Layout -----

    fn toolbar(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        use crate::ui::toolbar;
        // Room for ☰ and the buttons on the right (with Update, when shown).
        let mut reserve = 3.0 * 36.0;
        if self.settings.prefs.hide_menu_bar {
            reserve += 40.0;
        }
        if self.settings.background.update_available.is_some() {
            reserve += 70.0;
        }
        let tab = self
            .tabs
            .get(self.active)
            .filter(|_| !self.repos.home_active);
        let labels = toolbar::labels_fit(ui, tab, &self.settings, reserve);
        ui.horizontal(|ui| {
            // The same height without a repository (Repository Manager tab).
            ui.set_min_height(toolbar::height(labels));
            if self.settings.prefs.hide_menu_bar {
                crate::ui::main_menu::compact(ui, self.menu_bar(), cmds);
                toolbar::group_separator(ui);
            }
            if let Some(tab) = self
                .tabs
                .get(self.active)
                .filter(|_| !self.repos.home_active)
            {
                toolbar::repo_buttons(ui, tab, &self.settings, labels, cmds);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if toolbar::icon_button(ui, icons::SETTINGS, t!("Settings (Ctrl+,)")).clicked() {
                    cmds.push(Command::ShowSettings);
                }
                let tip = t!("Command palette (Ctrl+P)");
                if toolbar::icon_button(ui, icons::COMMAND_PALETTE, tip).clicked() {
                    cmds.push(Command::ShowPalette);
                }
                self.notifications_button(ui, cmds);
                self.update_link(ui, labels);
            });
        });
    }

    fn menu_bar(&mut self) -> crate::ui::main_menu::Bar<'_> {
        crate::ui::main_menu::Bar {
            settings: &mut self.settings,
            tabs: &self.tabs,
            active: self.active,
            home: self.repos.home_active || self.tabs.is_empty(),
            can_reopen: !self.repos.closed.is_empty(),
        }
    }

    fn main_menu(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        crate::ui::main_menu::show(ui, self.menu_bar(), cmds);
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        let tab = self
            .tabs
            .get(self.active)
            .filter(|_| !self.repos.home_active);
        crate::ui::status_bar::show(ui, tab, self.global_job.as_ref(), cmds);
    }

    fn banner(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            crate::ui::banner::show(ui, tab, cmds);
        }
    }
}

thread_local! {
    /// The egui context, so helpers without a `ctx` parameter can start jobs.
    static CTX: std::cell::RefCell<Option<egui::Context>> = const { std::cell::RefCell::new(None) };
}

impl eframe::App for FerrumApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        crate::dev_hover::apply(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        CTX.with(|c| *c.borrow_mut() = Some(ctx.clone()));
        self.maybe_screenshot(&ctx);
        self.apply_look(&ctx);
        self.poll_jobs(&ctx);
        self.safety_tick(&ctx);
        self.background_tick(&ctx);
        self.repos_tick(&ctx);
        self.hosted_tick(&ctx);

        let mut cmds = Commands::default();
        for c in std::mem::take(&mut self.startup) {
            cmds.push(c);
        }
        crate::keymap::begin_frame(&ctx, &self.settings.keymap);
        crate::custom_commands::set_user_commands(&self.settings.custom.commands);
        self.shortcuts(&ctx, &mut cmds);
        if self.dialog.is_none()
            && let Some(tab) = self.tabs.get(self.active)
        {
            crate::ui::patches::dropped_files(&ctx, &mut cmds);
            crate::ui::patches::paste(&ctx, tab, &mut cmds);
        }
        crate::ui::diff_prefs::share_with_prefs(
            &ctx,
            &mut self.settings.diff,
            &mut self.settings.prefs,
        );
        crate::ui::diff_prefs::sync(&ctx, &mut self.settings.diff, &mut self.tabs);

        let title = self.window_title();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        let chrome = |ui: &egui::Ui, x: i8, y: i8| {
            crate::ui::toolbar::chrome_frame(ui, egui::Margin::symmetric(x, y))
        };
        let strip = !self.tabs.is_empty() && !self.settings.library.vertical_tabs;
        if !self.settings.prefs.hide_menu_bar {
            egui::Panel::top("main-menu")
                .frame(chrome(ui, 8, 2))
                .show_separator_line(false)
                .show(ui, |ui| self.main_menu(ui, &mut cmds));
        }
        egui::Panel::top("toolbar")
            .frame(chrome(ui, 8, 4))
            .show_separator_line(!strip)
            .show(ui, |ui| self.toolbar(ui, &mut cmds));
        if strip {
            let frame = chrome(ui, 8, 0).inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 2,
                bottom: 0,
            });
            egui::Panel::top("tab-strip")
                .frame(frame)
                .show_separator_line(false)
                .show(ui, |ui| self.tab_bar(ui, &mut cmds));
        }
        let status = egui::Panel::bottom("status")
            .frame(chrome(ui, 8, 3))
            .show(ui, |ui| self.status_bar(ui, &mut cmds));
        let tab = self.tabs.get(self.active).map(|_| self.active).unwrap_or(0);
        self.toasts.observe(&self.tabs, tab, self.message.as_ref());
        self.safety_toasts();
        self.toasts
            .show(&ctx, status.response.rect.top(), &mut cmds);

        self.tab_list(ui, &mut cmds);
        if self.showing_home() {
            self.home(ui, &mut cmds);
        } else {
            let state_banner = self.tabs[self.active].state != RepoState::Clean;
            if state_banner {
                egui::Panel::top("banner").show(ui, |ui| self.banner(ui, &mut cmds));
            }
            let settings = &mut self.settings;
            let tab = &mut self.tabs[self.active];
            if self.dialog.is_none() {
                focus::handle(&ctx, tab);
                details::shortcuts(&ctx, tab, &mut settings.details, &mut cmds);
            }
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(230.0)
                .show(ui, |ui| {
                    sidebar::show(ui, tab, &mut cmds);
                });
            match tab.view {
                View::History => {
                    let panel = if settings.history.vertical_layout {
                        egui::Panel::right("details-side").default_size(480.0)
                    } else {
                        egui::Panel::bottom("details").default_size(300.0)
                    };
                    tab.pane.details_rect = None;
                    if !settings.details.collapsed {
                        let r = panel.resizable(true).show(ui, |ui| {
                            details::show(ui, tab, &mut settings.details, &mut cmds);
                        });
                        tab.pane.details_rect = Some(r.response.rect);
                    }
                    egui::CentralPanel::default_margins().show(ui, |ui| {
                        history::search_bar(ui, tab, settings, &mut cmds);
                        history::commit_list(ui, tab, settings, &mut cmds);
                    });
                    history_options::windows(&ctx, tab, &mut cmds);
                }
                View::Changes => {
                    egui::CentralPanel::default_margins()
                        .show(ui, |ui| changes::show(ui, tab, settings, &mut cmds));
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
                View::Overview => {
                    egui::CentralPanel::default_margins()
                        .show(ui, |ui| crate::ui::overview::show(ui, tab, &mut cmds));
                }
                View::Issues => {
                    egui::CentralPanel::default_margins().show(ui, |ui| {
                        crate::ui::issues::show_view(ui, tab, settings, &mut cmds)
                    });
                }
            }
            if tab.rebase.is_some() {
                rebase::show(&ctx, tab, &mut cmds);
            }
            details::windows(&ctx, tab, &mut settings.details, &mut cmds);
            focus::paint(&ctx, tab);
        }

        self.main_dialog(&ctx, &mut cmds);
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
            self.tabs.get_mut(self.active),
            &names,
            &mut self.settings,
            &mut cmds,
        );

        self.repo_windows(&ctx, &mut cmds);
        self.show_activity(&ctx);
        self.safety_windows(&ctx, &mut cmds);
        crate::ui::custom_commands::output_window(&ctx);
        crate::ui::file_window::show_all(&ctx, &mut self.tabs, self.active, &mut cmds);
        self.hosted_windows(&ctx, &mut cmds);

        for c in cmds.take() {
            self.handle(c, &ctx);
        }
        for i in 0..self.tabs.len() {
            if let Some(prefs) = self.tabs[i].sidebar.take_dirty() {
                crate::repo_settings::store_sidebar(&mut self.settings, &mut self.tabs, i, prefs);
            }
        }
        if self.tabs.iter().any(|t| t.job.is_some()) || self.global_job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.open_repos = self.tabs.iter().map(|t| t.path().to_path_buf()).collect();
        self.settings.active_repo = self.active;
        for i in 0..self.tabs.len() {
            crate::repo_settings::store_history(&mut self.settings.history, &mut self.tabs, i);
        }
        eframe::set_value(storage, STORAGE_KEY, &self.settings);
    }
}
