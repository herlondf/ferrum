//! Git operations running on a background thread so the window stays responsive.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use eframe::egui;
use ferrum_core::forge::PullRequest;

use crate::settings::ForgeAccount;

/// What to reload once a job finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Refresh {
    None,
    Status,
    All,
}

pub enum JobOutput {
    Done,
    PullRequests(Vec<PullRequest>),
    PullRequestCreated(PullRequest),
    Account(ForgeAccount),
    /// A repository was cloned or created; open it.
    OpenRepo(std::path::PathBuf),
    /// Commits found by a content search.
    SearchHits(std::collections::HashSet<ferrum_core::Oid>),
    /// Commits and files found by a file path search.
    PathHits(Vec<ferrum_core::log::PathHit>),
}

pub type JobResult = Result<JobOutput, String>;

pub struct Job {
    pub label: String,
    pub refresh: Refresh,
    /// The job is a commit; the message box is cleared when it succeeds.
    pub is_commit: bool,
    progress: Arc<Mutex<String>>,
    rx: Receiver<JobResult>,
}

impl Job {
    pub fn spawn(
        label: impl Into<String>,
        refresh: Refresh,
        ctx: &egui::Context,
        work: impl FnOnce(&dyn Fn(&str)) -> JobResult + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let progress = Arc::new(Mutex::new(String::new()));
        let (p, ctx) = (progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let report = |line: &str| {
                if let Ok(mut s) = p.lock() {
                    *s = line.trim().to_owned();
                }
                ctx.request_repaint();
            };
            let result = work(&report);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        Self {
            label: label.into(),
            refresh,
            is_commit: false,
            progress,
            rx,
        }
    }

    pub fn progress(&self) -> String {
        self.progress.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// The result, once the job has finished.
    pub fn poll(&self) -> Option<JobResult> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("the operation stopped unexpectedly".into()))
            }
        }
    }
}
