//! What the window does by itself every frame: background reloads, file
//! watching, refresh on focus, the daily update check and the Activity window.

use std::sync::mpsc::{self, Receiver};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;
use ferrum_core::update;

use super::FerrumApp;
use crate::jobs::{Job, JobResult, JobSpec, Refresh};
use crate::ui::activity;

const RELEASES: &str = "herlondf/ferrum";
const DAY: u64 = 24 * 60 * 60;

#[derive(Default)]
pub struct State {
    pub activity: activity::ActivityWindow,
    update: Option<Receiver<Result<update::Release, String>>>,
    update_started: bool,
    pub updater: crate::ui::updater::Updater,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl FerrumApp {
    pub(super) fn background_tick(&mut self, ctx: &egui::Context) {
        let bg = &self.settings.background;
        let focused = ctx.input(|i| i.focused);
        crate::jobs::set_window_state(focused, bg.notifications);

        // Pick up changes made outside the app when the window regains focus.
        if focused
            && !self.was_focused
            && bg.refresh_on_focus
            && let Some(t) = self.tabs.get_mut(self.active)
        {
            t.request_refresh(Refresh::All);
        }
        self.was_focused = focused;

        let poll_active = self.dialog.is_none();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            tab.poll_refresh(ctx, bg, poll_active && i == self.active);
        }
        if self.tabs.iter().any(|t| t.live.refreshing()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        crate::sync::auto_fetch(&mut self.tabs, &self.settings.sync, ctx);
        self.check_for_update(ctx);
    }

    /// Once a day (off the UI thread), asks GitHub for the latest release.
    fn check_for_update(&mut self, ctx: &egui::Context) {
        let bg = &mut self.settings.background;
        if let Some(rx) = &self.bg.update
            && let Ok(result) = rx.try_recv()
        {
            self.bg.update = None;
            match result {
                Ok(r) if update::is_newer(&r.version, env!("CARGO_PKG_VERSION")) => {
                    bg.update_available = Some((r.version, r.url));
                }
                Ok(_) => bg.update_available = None,
                Err(e) => eprintln!("update check: {e}"),
            }
        }
        // Forget a release once it is installed.
        if bg
            .update_available
            .as_ref()
            .is_some_and(|(v, _)| !update::is_newer(v, env!("CARGO_PKG_VERSION")))
        {
            bg.update_available = None;
        }
        let dev_run = cfg!(test)
            || std::env::var_os("FERRUM_SCREENSHOT").is_some()
            || std::env::var_os("FERRUM_NO_UPDATE_CHECK").is_some();
        if self.bg.update_started
            || dev_run
            || !bg.check_updates
            || now().saturating_sub(bg.last_update_check) < DAY
        {
            return;
        }
        self.bg.update_started = true;
        bg.last_update_check = now();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::latest_release(RELEASES).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.bg.update = Some(rx);
    }

    /// "Version X available" in the toolbar, linking to the release page.
    pub(super) fn update_link(&self, ui: &mut egui::Ui, labels: bool) {
        if let Some((version, url)) = &self.settings.background.update_available {
            let tool = crate::ui::toolbar::Tool::new(crate::icons::ARROW_CIRCLE_UP, t!("Update"))
                .accent()
                .label_shown(labels);
            let tip = format!("{}\n{url}", t!("Version {} available", version));
            if tool.show(ui).main.on_hover_text(tip).clicked() {
                crate::ui::updater::request_open(ui.ctx());
            }
        }
    }

    /// The "Update Ferrum" window (download, install, restart).
    pub(super) fn show_updater(&mut self, ctx: &egui::Context) {
        let available = self.settings.background.update_available.as_ref();
        crate::ui::updater::show(ctx, &mut self.bg.updater, available, RELEASES);
    }

    pub(super) fn show_activity(&mut self, ctx: &egui::Context) {
        let repo = self.tabs.get(self.active).map(|t| t.path().to_path_buf());
        activity::show(ctx, &mut self.bg.activity, repo.as_deref());
    }

    /// Starts an app-wide job (clone…) whose git commands can be cancelled.
    pub(super) fn spawn_global_cancellable(
        &mut self,
        label: &str,
        ctx: &egui::Context,
        work: impl FnOnce(&crate::jobs::JobEnv) -> JobResult + Send + 'static,
    ) {
        if self.global_job.is_some() {
            self.error(t!("Another operation is still running."));
            return;
        }
        let spec = JobSpec::new(label, Refresh::None).cancellable();
        self.global_job = Some(Job::start(spec, ctx, work));
    }
}
