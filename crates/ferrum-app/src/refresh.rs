//! Reloads repositories on a background thread, so big repositories never
//! freeze the window, and watches the working folder for changes.
//!
//! A reload builds a fresh `RepoTab` off the UI thread (running the same
//! `refresh_all` / `refresh_status`) and then moves its data into the open
//! tab, keeping the view state (selection, scroll, messages) as it was.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::jobs::Refresh;
use crate::settings::BackgroundSettings;
use crate::tab::{RepoTab, View};
use crate::watcher::RepoWatcher;

/// Per-tab state of background reloads and file watching.
#[derive(Default)]
pub struct Live {
    task: Option<Task>,
    /// Asked for while a job or another reload was running.
    pending: Option<Refresh>,
    /// Bumped when a job starts: a reload that began before is out of date.
    job_gen: u64,
    watcher: Option<RepoWatcher>,
    /// Watching failed (e.g. too many folders); polling is used instead.
    watch_failed: bool,
    last_poll: Option<Instant>,
}

struct Task {
    kind: Refresh,
    job_gen: u64,
    rx: Receiver<Result<RepoTab, String>>,
}

impl Live {
    /// A reload is running or waiting to run.
    pub fn refreshing(&self) -> bool {
        self.task.is_some() || self.pending.is_some()
    }

    pub fn job_started(&mut self) {
        self.job_gen += 1;
    }
}

impl RepoTab {
    /// Reloads in the background. Requests made meanwhile are merged.
    pub fn request_refresh(&mut self, kind: Refresh) {
        if kind != Refresh::None {
            self.live.pending = self.live.pending.max(Some(kind));
        }
    }

    /// Called every frame: starts and applies background reloads, and reacts
    /// to file changes (or polls when watching is off).
    /// Only the `active` tab is polled.
    pub fn poll_refresh(
        &mut self,
        ctx: &egui::Context,
        settings: &BackgroundSettings,
        active: bool,
    ) {
        self.poll_changes(ctx, settings, active);

        if let Some(task) = &self.live.task {
            match task.rx.try_recv() {
                Ok(result) => {
                    let task = self.live.task.take().expect("checked above");
                    // A job started after this reload began: its result is
                    // already out of date and another reload follows.
                    let stale = task.job_gen != self.live.job_gen
                        && self.live.pending.is_some_and(|p| p >= task.kind);
                    match result {
                        Ok(fresh) if !stale => self.apply(fresh, task.kind),
                        Ok(_) => {}
                        Err(e) => self.error(e),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.live.task = None,
            }
        }

        // Jobs change the repository; reload once they are done.
        if self.live.task.is_none()
            && self.job.is_none()
            && let Some(kind) = self.live.pending.take()
        {
            self.spawn_refresh(kind, ctx);
        }
    }

    fn spawn_refresh(&mut self, kind: Refresh, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let path = self.path().to_path_buf();
        let ctx = ctx.clone();
        let (prefs, limit) = self.history.ext.load_options();
        let amend = self.changes.amend;
        std::thread::spawn(move || {
            let result = RepoTab::new(path).map(|mut t| {
                // The staged list depends on amend mode.
                t.changes.amend = amend;
                t.history.ext.prefs = prefs;
                t.history.ext.limit = limit;
                match kind {
                    Refresh::All => t.refresh_all(),
                    _ => t.refresh_status(),
                }
                t
            });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.live.task = Some(Task {
            kind,
            job_gen: self.live.job_gen,
            rx,
        });
    }

    /// Takes the data loaded by `fresh`, keeping this tab's view state.
    fn apply(&mut self, mut fresh: RepoTab, kind: Refresh) {
        use std::mem::swap;
        if kind == Refresh::All {
            swap(&mut self.branches, &mut fresh.branches);
            swap(&mut self.tags, &mut fresh.tags);
            swap(&mut self.stashes, &mut fresh.stashes);
            swap(&mut self.remotes, &mut fresh.remotes);
            swap(&mut self.submodules, &mut fresh.submodules);
            swap(&mut self.worktrees, &mut fresh.worktrees);
            swap(&mut self.stacks, &mut fresh.stacks);
            swap(&mut self.uses_lfs, &mut fresh.uses_lfs);
            swap(&mut self.hosted, &mut fresh.hosted);
            swap(&mut self.commits, &mut fresh.commits);
            swap(&mut self.graph, &mut fresh.graph);
            swap(&mut self.labels, &mut fresh.labels);
            swap(&mut self.index, &mut fresh.index);
            self.history.ext.take_loaded(&mut fresh.history.ext);
            self.apply_search();
            self.keep_selected_commit();
        }
        swap(&mut self.head, &mut fresh.head);
        swap(&mut self.head_oid, &mut fresh.head_oid);
        swap(&mut self.state, &mut fresh.state);
        swap(&mut self.bisect, &mut fresh.bisect);
        swap(&mut self.identity, &mut fresh.identity);
        swap(&mut self.status, &mut fresh.status);
        self.reload_change_diff();
        if self.view == View::Conflicts && self.status.conflicted.is_empty() {
            self.view = View::Changes;
        }
        if let Some(m) = fresh.message.filter(|m| m.error) {
            self.message = Some(m);
        }
    }

    /// Watches the working folder, or polls it every few seconds.
    fn poll_changes(&mut self, ctx: &egui::Context, settings: &BackgroundSettings, active: bool) {
        if settings.watch_files && self.live.watcher.is_none() && !self.live.watch_failed {
            match RepoWatcher::start(&self.repo, ctx) {
                Ok(w) => self.live.watcher = Some(w),
                Err(e) => {
                    eprintln!("watching {} failed, polling instead: {e}", self.name);
                    self.live.watch_failed = true;
                }
            }
        } else if !settings.watch_files {
            self.live.watcher = None;
            self.live.watch_failed = false;
        }

        if let Some(w) = &self.live.watcher {
            if let Some(change) = w.take() {
                self.request_refresh(if change.refs {
                    Refresh::All
                } else {
                    Refresh::Status
                });
            }
            return;
        }

        let interval = Duration::from_secs(settings.poll_secs.max(1) as u64);
        ctx.request_repaint_after(interval);
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let due = self.live.last_poll.is_none_or(|t| t.elapsed() >= interval);
        if !active || !due || minimized || self.busy() || self.live.refreshing() {
            return;
        }
        if self.live.last_poll.is_none() {
            // Just opened: everything was loaded already.
            self.live.last_poll = Some(Instant::now());
            return;
        }
        self.live.last_poll = Some(Instant::now());
        // Everything when HEAD moved (e.g. a commit made in a terminal).
        let head_moved =
            self.repo.head_oid() != self.head_oid || self.repo.head_branch() != self.head;
        self.request_refresh(if head_moved {
            Refresh::All
        } else {
            Refresh::Status
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn tabs_can_load_on_another_thread() {
        assert_send::<RepoTab>();
    }
}
