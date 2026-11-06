//! Tabs and repositories: the Repository Manager tab, closing and reopening
//! tabs, workspaces, source folder scans, requests from other Ferrum
//! processes, dropped folders and tabs shown in windows of their own.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use ferrum_core::library;

use super::FerrumApp;
use crate::command::{Command, Commands};
use crate::repos::{RepoCmd, Request};
use crate::tab::{RepoTab, View};
use crate::ui::dialogs::Dialog;
use crate::ui::{open_quickly, repo_dialogs, repo_manager, tabs};

/// Most closed tabs remembered for Ctrl+Shift+T.
const MAX_CLOSED: usize = 20;

#[derive(Default)]
pub struct State {
    /// The Repository Manager tab (Ctrl+T) is open / selected.
    pub home_open: bool,
    pub home_active: bool,
    /// Paths of closed tabs, last closed at the end.
    pub closed: Vec<PathBuf>,
    pub manager: repo_manager::Manager,
    pub quick: open_quickly::OpenQuickly,
    scan: Option<Receiver<Vec<PathBuf>>>,
    pub listener: Option<crate::single_instance::Listener>,
    /// Requests from the command line, handled on the first frame.
    pub pending: Vec<Request>,
    /// Tabs shown in windows of their own.
    pub windows: Vec<PathBuf>,
    /// The open dialog belongs to this window's tab.
    pub dialog_window: Option<PathBuf>,
}

impl FerrumApp {
    #[cfg(test)]
    pub fn repo_state(&self) -> &State {
        &self.repos
    }

    #[cfg(test)]
    pub fn active_index(&self) -> usize {
        self.active
    }

    #[cfg(test)]
    pub fn has_dialog(&self) -> bool {
        self.dialog.is_some()
    }

    /// The Repository Manager fills the window (no tabs, or its tab is selected).
    pub(super) fn showing_home(&self) -> bool {
        self.tabs.is_empty() || self.repos.home_active
    }

    pub(super) fn window_title(&self) -> String {
        let ws = &self.settings.library.workspace;
        let ws = if ws.is_empty() {
            String::new()
        } else {
            format!(" — [{ws}]")
        };
        match self.tabs.get(self.active).filter(|_| !self.showing_home()) {
            Some(t) => format!(
                "{} — {}{ws} — Ferrum",
                crate::repos::base_label(&self.settings.library, t),
                t.head.clone().unwrap_or_else(|| t!("detached"))
            ),
            None if ws.is_empty() => "Ferrum".into(),
            None => format!("{}{ws} — Ferrum", t!("Repositories")),
        }
    }

    /// After a repository was opened (or selected again).
    pub(super) fn opened(&mut self, path: &Path) {
        self.settings.library.remember(path);
        self.repos.home_open = false;
        self.repos.home_active = false;
    }

    /// Opening failed: offers a fix for untrusted folders. Returns true when
    /// the error was handled.
    pub(super) fn open_failed(&mut self, path: &Path, error: &str) -> bool {
        if !library::is_safe_directory_error(error) || self.dialog.is_some() {
            return false;
        }
        self.dialog = Some(Dialog::Repo(repo_dialogs::RepoDialog::Trust {
            path: path.to_path_buf(),
            error: error.to_owned(),
        }));
        true
    }

    pub(super) fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(i);
        let path = tab.path().to_path_buf();
        self.repos.closed.retain(|p| p != &path);
        self.repos.closed.push(path.clone());
        if self.repos.closed.len() > MAX_CLOSED {
            self.repos.closed.remove(0);
        }
        self.repos.windows.retain(|p| p != &path);
        if self.active >= i && self.active > 0 {
            self.active -= 1;
        }
        if self.tabs.is_empty() {
            self.repos.home_active = false;
            self.repos.home_open = false;
        }
    }

    pub(super) fn repo_shortcuts(&mut self, ctx: &egui::Context, cmds: &mut Commands) {
        use crate::keymap::{Action, pressed};
        for (a, c) in [
            (Action::ReopenTab, RepoCmd::ReopenClosed),
            (Action::OpenQuickly, RepoCmd::OpenQuickly),
            (Action::NewTab, RepoCmd::NewTab),
        ] {
            if pressed(ctx, a) {
                cmds.push(Command::Repos(c));
            }
        }
        // Close tab closes the Repository Manager tab when it is shown.
        if self.repos.home_active && !self.tabs.is_empty() && pressed(ctx, Action::CloseTab) {
            cmds.push(Command::Repos(RepoCmd::CloseHome));
        }
    }

    pub(super) fn handle_repos(&mut self, cmd: RepoCmd, ctx: &egui::Context) {
        match cmd {
            RepoCmd::NewTab => {
                self.repos.home_open = true;
                self.repos.home_active = true;
            }
            RepoCmd::CloseHome => {
                self.repos.home_open = false;
                self.repos.home_active = false;
            }
            RepoCmd::ReopenClosed => {
                while let Some(p) = self.repos.closed.pop() {
                    if p.exists() {
                        self.open(p);
                        break;
                    }
                }
            }
            RepoCmd::OpenQuickly => self.repos.quick.toggle(),
            RepoCmd::CloseAll => {
                for i in (0..self.tabs.len()).rev() {
                    self.close_tab(i);
                }
            }
            RepoCmd::CloseOthers(keep) => {
                let Some(path) = self.tabs.get(keep).map(|t| t.path().to_path_buf()) else {
                    return;
                };
                for i in (0..self.tabs.len()).rev() {
                    if self.tabs[i].path() != path {
                        self.close_tab(i);
                    }
                }
                self.active = 0;
            }
            RepoCmd::MoveToWorkspace(i, name) => {
                if let Some(t) = self.tabs.get(i) {
                    let path = t.path().to_path_buf();
                    self.settings.library.add_to_workspace(&name, &path);
                    self.close_tab(i);
                    let shown = crate::repos::clean_workspace_name(&name);
                    let shown = if shown.is_empty() {
                        t!("Default workspace")
                    } else {
                        shown
                    };
                    self.info(t!("Moved to workspace {}", shown));
                }
            }
            RepoCmd::SwitchWorkspace(name) => self.switch_workspace(&name),
            RepoCmd::Scan => self.scan(ctx),
            RepoCmd::OpenInWindow(i) => {
                if let Some(t) = self.tabs.get(i) {
                    let p = t.path().to_path_buf();
                    if !self.repos.windows.contains(&p) {
                        self.repos.windows.push(p);
                    }
                    // The main window moves on to another tab.
                    if self.active == i && self.tabs.len() > 1 {
                        self.active = if i == 0 { 1 } else { i - 1 };
                    }
                }
            }
            RepoCmd::TrustFolder(path) => {
                let git = ferrum_core::cli::Git::new(std::env::temp_dir());
                match library::add_safe_directory(&git, &path) {
                    Ok(()) => self.open(path),
                    Err(e) => self.error(e.to_string()),
                }
            }
            RepoCmd::Settings => {
                if let Some(t) = self.tabs.get(self.active) {
                    self.dialog = Some(repo_dialogs::settings(t, &self.settings));
                }
            }
            RepoCmd::Dev(step) => self.dev_step(&step, ctx),
        }
    }

    /// `FERRUM_START` steps: `manager`, `quick:TEXT`, `overview`,
    /// `repo-settings`, `workspace:NAME`, `vertical-tabs`.
    fn dev_step(&mut self, step: &str, ctx: &egui::Context) {
        let (cmd, arg) = step.split_once(':').unwrap_or((step, ""));
        match cmd {
            "manager" => self.handle_repos(RepoCmd::NewTab, ctx),
            "quick" => {
                self.repos.quick.toggle();
                self.repos.quick.set_query(arg);
            }
            "overview" => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    crate::ui::overview::open(t);
                }
            }
            "repo-settings" => self.handle_repos(RepoCmd::Settings, ctx),
            "workspace" => self.switch_workspace(arg),
            "vertical-tabs" => self.settings.library.vertical_tabs = true,
            _ => eprintln!("FERRUM_START: unknown step {step}"),
        }
    }

    fn switch_workspace(&mut self, name: &str) {
        let tabs: Vec<PathBuf> = self.tabs.iter().map(|t| t.path().to_path_buf()).collect();
        let Some((open, active)) = self
            .settings
            .library
            .switch_workspace(name, tabs, self.active)
        else {
            return;
        };
        for i in 0..self.tabs.len() {
            crate::repo_settings::store_history(&mut self.settings.history, &mut self.tabs, i);
        }
        self.tabs.clear();
        self.repos.windows.clear();
        self.active = 0;
        for p in open {
            if p.exists() {
                self.open(p);
            }
        }
        self.active = active.min(self.tabs.len().saturating_sub(1));
        // An empty workspace starts at the Repository Manager.
        self.repos.home_open = self.tabs.is_empty();
        self.repos.home_active = self.tabs.is_empty();
    }

    fn scan(&mut self, ctx: &egui::Context) {
        if self.repos.scan.is_some() {
            return;
        }
        let sources = self.settings.library.sources.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut found = Vec::new();
            for s in sources {
                for p in library::find_repositories(&s.path, s.depth) {
                    if !found.contains(&p) {
                        found.push(p);
                    }
                }
            }
            let _ = tx.send(found);
            ctx.request_repaint();
        });
        self.repos.scan = Some(rx);
    }

    pub(super) fn scanning(&self) -> bool {
        self.repos.scan.is_some()
    }

    fn launch(&mut self, r: Request, ctx: &egui::Context) {
        match r {
            Request::Open(p) => {
                if !p.exists() {
                    self.error(t!("{} does not exist.", p.display()));
                } else if ferrum_core::Repo::open(&p).is_ok() {
                    self.open(p);
                } else if !self.open_failed_any(&p) {
                    self.offer_init(p);
                }
            }
            Request::FileLog(file) => self.open_file_log(&file, ctx),
            Request::Clone(url) => {
                let mut d = Dialog::clone_repo(&self.settings);
                if let Dialog::Clone {
                    url: u, name: n, ..
                } = &mut d
                {
                    *n = crate::ui::dialogs::clone_dir_name(&url);
                    *u = url;
                }
                self.dialog = Some(d);
            }
            Request::Focus => {}
        }
    }

    /// Opening `p` fails for another reason than "not a repository".
    fn open_failed_any(&mut self, p: &Path) -> bool {
        match ferrum_core::Repo::open(p) {
            Err(e) => self.open_failed(p, &e.to_string()),
            Ok(_) => false,
        }
    }

    fn offer_init(&mut self, p: PathBuf) {
        if self.dialog.is_none() {
            self.info(t!("{} is not a git repository.", p.display()));
            self.dialog = Some(Dialog::Init {
                path: p.to_string_lossy().into_owned(),
            });
        }
    }

    /// `ferrum log -- FILE`: opens the file's repository and its history.
    fn open_file_log(&mut self, file: &Path, ctx: &egui::Context) {
        let dir = if file.is_dir() {
            file
        } else {
            file.parent().unwrap_or(file)
        };
        let repo = match ferrum_core::Repo::open(dir) {
            Ok(r) => r,
            Err(e) => {
                if !self.open_failed(dir, &e.to_string()) {
                    self.error(t!("{} is not in a git repository.", file.display()));
                }
                return;
            }
        };
        let root = repo.path().to_path_buf();
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let rel = canon(file)
            .strip_prefix(canon(&root))
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        self.open(root);
        if !rel.is_empty() {
            self.handle(Command::FileHistory(rel), ctx);
        }
    }

    /// Every frame: requests from other processes, scan results, dropped
    /// folders and the pull request remote of each repository.
    pub(super) fn repos_tick(&mut self, ctx: &egui::Context) {
        let mut requests = std::mem::take(&mut self.repos.pending);
        if let Some(l) = &self.repos.listener {
            l.set_context(ctx);
            let got = l.poll();
            if !got.is_empty() {
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::ROOT,
                    egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Informational,
                    ),
                );
            }
            requests.extend(got);
        }
        for r in requests {
            self.launch(r, ctx);
        }
        if let Some(rx) = &self.repos.scan
            && let Ok(found) = rx.try_recv()
        {
            self.repos.scan = None;
            let n = found.len();
            self.settings.library.found = found;
            self.info(t!("{} repositories found in the source folders", n));
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| p.is_dir())
                .collect()
        });
        for p in dropped {
            self.launch(Request::Open(p), ctx);
        }
        self.apply_pr_remotes();
    }

    /// Uses the remote chosen in the repository settings for pull requests.
    fn apply_pr_remotes(&mut self) {
        let lib = &self.settings.library;
        if lib.config.is_empty() {
            return;
        }
        for t in &mut self.tabs {
            let want = &lib.config(&crate::repo_settings::shared_key(t)).pr_remote;
            if want.is_empty() {
                continue;
            }
            let hosted = t
                .remotes
                .iter()
                .find(|r| &r.name == want)
                .and_then(|r| ferrum_core::forge::parse_remote_url(&r.url));
            if hosted.is_some() && hosted != t.hosted {
                t.hosted = hosted;
                t.prs.list = None;
            }
        }
    }

    // ----- Drawing -----

    /// The tab row under the toolbar (when tabs are not in a list).
    pub(super) fn tab_bar(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        if self.settings.library.vertical_tabs {
            return;
        }
        let bar = tabs::Bar {
            tabs: &self.tabs,
            active: (!self.showing_home()).then_some(self.active),
            home: self.repos.home_open,
            in_window: &self.repos.windows,
        };
        tabs::row(ui, &bar, &mut self.settings.library, cmds);
    }

    /// The tab list on the left (Settings: vertical tabs).
    pub(super) fn tab_list(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        if !self.settings.library.vertical_tabs || self.tabs.is_empty() {
            return;
        }
        let bar = tabs::Bar {
            tabs: &self.tabs,
            active: (!self.showing_home()).then_some(self.active),
            home: self.repos.home_open,
            in_window: &self.repos.windows,
        };
        egui::Panel::left("tab-list")
            .resizable(true)
            .default_size(190.0)
            .show(ui, |ui| {
                tabs::list(ui, &bar, &mut self.settings.library, cmds);
            });
    }

    /// The Repository Manager in the central area.
    pub(super) fn home(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        let open: Vec<PathBuf> = self.tabs.iter().map(|t| t.path().to_path_buf()).collect();
        let scanning = self.scanning();
        egui::CentralPanel::default_margins().show(ui, |ui| {
            repo_manager::show(
                ui,
                &mut self.repos.manager,
                &mut self.settings,
                &open,
                scanning,
                cmds,
            );
        });
    }

    /// Open Quickly and the tab windows.
    pub(super) fn repo_windows(&mut self, ctx: &egui::Context, cmds: &mut Commands) {
        open_quickly::show(ctx, &mut self.repos.quick, &self.settings, cmds);
        self.tab_windows(ctx);
    }

    /// The modal dialog, unless it belongs to a tab window.
    pub(super) fn main_dialog(&mut self, ctx: &egui::Context, cmds: &mut Commands) {
        if self.repos.dialog_window.is_some() {
            return;
        }
        if let Some(mut d) = self.dialog.take() {
            let keep = crate::ui::dialogs::show(
                ctx,
                &mut d,
                self.tabs.get(self.active),
                &mut self.settings,
                cmds,
            );
            if keep && self.dialog.is_none() {
                self.dialog = Some(d);
            }
        }
    }

    /// Each tab opened with "Open in new window" gets a native window showing
    /// its sidebar and current view. Its commands run with that tab active.
    fn tab_windows(&mut self, ctx: &egui::Context) {
        let open: Vec<PathBuf> = self.tabs.iter().map(|t| t.path().to_path_buf()).collect();
        self.repos.windows.retain(|p| open.contains(p));
        if self
            .repos
            .dialog_window
            .as_ref()
            .is_some_and(|p| !self.repos.windows.contains(p))
        {
            self.repos.dialog_window = None;
        }
        for path in self.repos.windows.clone() {
            let Some(i) = self.tabs.iter().position(|t| t.path() == path) else {
                continue;
            };
            let id = egui::ViewportId::from_hash_of(("tab-window", &path));
            let title = format!(
                "{} — Ferrum",
                tabs::labels(&self.tabs, &self.settings.library)[i]
            );
            let builder = egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size([1100.0, 760.0]);
            let mut wcmds = Commands::default();
            let mut close = false;
            let owns_dialog = self.repos.dialog_window.as_ref() == Some(&path);
            ctx.show_viewport_immediate(id, builder, |ui, _class| {
                let vctx = ui.ctx().clone();
                if vctx.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
                let settings = &mut self.settings;
                let tab = &mut self.tabs[i];
                tab_view(&vctx, ui, tab, settings, owns_dialog, &mut wcmds);
                if owns_dialog && let Some(mut d) = self.dialog.take() {
                    let keep = crate::ui::dialogs::show(
                        &vctx,
                        &mut d,
                        self.tabs.get(i),
                        &mut self.settings,
                        &mut wcmds,
                    );
                    if keep && self.dialog.is_none() {
                        self.dialog = Some(d);
                    }
                }
            });
            if close {
                self.repos.windows.retain(|p| p != &path);
            }
            // Run the window's commands as if its tab were the active one.
            let cmds = wcmds.take();
            if cmds.is_empty() {
                if owns_dialog && self.dialog.is_none() {
                    self.repos.dialog_window = None;
                }
                continue;
            }
            let main_active = self.active;
            let had_dialog = self.dialog.is_some();
            self.active = i;
            for c in cmds {
                self.handle(c, ctx);
            }
            let now = self.tabs.iter().position(|t| t.path() == path);
            if self.dialog.is_some() && (!had_dialog || owns_dialog) {
                self.repos.dialog_window = Some(path.clone());
            } else if self.dialog.is_none() {
                self.repos.dialog_window = None;
            }
            // Tab switches asked for in the window land in the main window.
            if self.active == now.unwrap_or(usize::MAX) {
                self.active = main_active.min(self.tabs.len().saturating_sub(1));
            }
        }
    }
}

/// A tab's sidebar and current view, for a tab window.
fn tab_view(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    settings: &mut crate::settings::Settings,
    dialog_open: bool,
    cmds: &mut Commands,
) {
    use crate::ui::{blame, changes, conflicts, details, forge, history, history_options, rebase};
    if tab.state != ferrum_core::RepoState::Clean {
        egui::Panel::top("w-banner").show(ui, |ui| crate::ui::banner::show(ui, tab, cmds));
    }
    if !dialog_open {
        crate::ui::focus::handle(ctx, tab);
    }
    egui::Panel::left("w-sidebar")
        .resizable(true)
        .default_size(230.0)
        .show(ui, |ui| crate::ui::sidebar::show(ui, tab, cmds));
    match tab.view {
        View::History => {
            if !settings.details.collapsed {
                egui::Panel::bottom("w-details")
                    .resizable(true)
                    .default_size(300.0)
                    .show(ui, |ui| details::show(ui, tab, &mut settings.details, cmds));
            }
            egui::CentralPanel::default_margins().show(ui, |ui| {
                history::search_bar(ui, tab, settings, cmds);
                history::commit_list(ui, tab, settings, cmds);
            });
            history_options::windows(ctx, tab, cmds);
        }
        View::Changes => {
            egui::CentralPanel::default_margins()
                .show(ui, |ui| changes::show(ui, tab, settings, cmds));
        }
        View::Conflicts => {
            egui::CentralPanel::default_margins().show(ui, |ui| conflicts::show(ui, tab, cmds));
        }
        View::File => {
            egui::CentralPanel::default_margins().show(ui, |ui| blame::show(ui, tab, cmds));
        }
        View::PullRequests => {
            egui::CentralPanel::default_margins()
                .show(ui, |ui| forge::show_view(ui, tab, settings, cmds));
        }
        View::Overview => {
            egui::CentralPanel::default_margins()
                .show(ui, |ui| crate::ui::overview::show(ui, tab, cmds));
        }
        #[allow(unreachable_patterns)]
        _ => {
            egui::CentralPanel::default_margins().show(ui, |ui| {
                ui.weak(t!("This view is shown in the main window."));
            });
        }
    }
    if tab.rebase.is_some() {
        rebase::show(ctx, tab, cmds);
    }
    details::windows(ctx, tab, &mut settings.details, cmds);
}
