//! Git operations running on a background thread so the window stays responsive.
//!
//! Every job is also recorded as an [`Operation`] in the activity log, with the
//! git commands it ran, so the Activity window can show them.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use ferrum_core::cli::{CancelToken, CommandLog, Git};
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

/// How a finished operation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Succeeded,
    Failed(String),
    Cancelled,
}

/// One entry of the activity log: a job and the git commands it ran.
pub struct Operation {
    pub id: u64,
    pub label: String,
    /// Repository it ran in (`None` for clone, account checks…).
    pub repo: Option<PathBuf>,
    /// Started by the app itself (e.g. auto-fetch), not by the user.
    pub background: bool,
    /// Its git commands can be stopped.
    pub cancellable: bool,
    pub started: SystemTime,
    pub log: CommandLog,
    pub cancel: CancelToken,
    progress: Mutex<String>,
    outcome: Mutex<Option<(SystemTime, Outcome)>>,
}

impl Operation {
    pub fn progress(&self) -> String {
        self.progress.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// `None` while it runs.
    pub fn outcome(&self) -> Option<(SystemTime, Outcome)> {
        self.outcome.lock().ok().and_then(|o| o.clone())
    }

    pub fn running(&self) -> bool {
        self.outcome().is_none()
    }

    pub fn commands(&self) -> Vec<ferrum_core::cli::CommandRecord> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

const MAX_OPERATIONS: usize = 300;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OPERATIONS: Mutex<VecDeque<Arc<Operation>>> = Mutex::new(VecDeque::new());

/// The activity log, oldest first.
pub fn operations() -> Vec<Arc<Operation>> {
    OPERATIONS
        .lock()
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default()
}

/// Forgets finished operations.
pub fn clear_finished() {
    if let Ok(mut l) = OPERATIONS.lock() {
        l.retain(|o| o.running());
    }
}

fn record(op: Arc<Operation>) {
    if let Ok(mut l) = OPERATIONS.lock() {
        l.push_back(op);
        while l.len() > MAX_OPERATIONS {
            // Drop the oldest finished one; running ones stay visible.
            match l.iter().position(|o| !o.running()) {
                Some(i) => {
                    l.remove(i);
                }
                None => break,
            }
        }
    }
}

static WINDOW_FOCUSED: AtomicBool = AtomicBool::new(true);
static NOTIFY: AtomicBool = AtomicBool::new(false);

/// Called every frame: whether the window has focus, and whether finished
/// long operations should raise a desktop notification while it does not.
pub fn set_window_state(focused: bool, notifications: bool) {
    WINDOW_FOCUSED.store(focused, Ordering::Relaxed);
    NOTIFY.store(notifications, Ordering::Relaxed);
}

/// Operations that take longer than this raise a notification.
const LONG_OPERATION: Duration = Duration::from_secs(5);

/// How to start a job.
pub struct JobSpec {
    pub label: String,
    pub refresh: Refresh,
    pub repo: Option<PathBuf>,
    pub background: bool,
    pub cancellable: bool,
}

impl JobSpec {
    pub fn new(label: impl Into<String>, refresh: Refresh) -> Self {
        Self {
            label: label.into(),
            refresh,
            repo: None,
            background: false,
            cancellable: false,
        }
    }

    pub fn repo(mut self, path: impl Into<PathBuf>) -> Self {
        self.repo = Some(path.into());
        self
    }

    /// Run on behalf of the app (auto-fetch…): listed apart and quieter.
    pub fn background(mut self, yes: bool) -> Self {
        self.background = yes;
        self
    }

    /// The work runs git through [`JobEnv::git`], so it can be cancelled.
    pub fn cancellable(mut self) -> Self {
        self.cancellable = true;
        self
    }
}

/// What a job's work gets: progress reports and a git runner tied to the job.
pub struct JobEnv<'a> {
    op: &'a Operation,
    report: &'a dyn Fn(&str),
}

impl JobEnv<'_> {
    pub fn progress(&self, line: &str) {
        (self.report)(line)
    }

    /// `git` with this job's command log and cancel token attached.
    pub fn git(&self, git: &Git) -> Git {
        git.clone()
            .with_log(self.op.log.clone())
            .with_cancel(self.op.cancel.clone())
    }
}

pub struct Job {
    pub label: String,
    pub refresh: Refresh,
    /// The job is a commit; the message box is cleared when it succeeds.
    pub is_commit: bool,
    pub background: bool,
    pub operation: Arc<Operation>,
    rx: Receiver<JobResult>,
}

impl Job {
    pub fn spawn(
        label: impl Into<String>,
        refresh: Refresh,
        ctx: &egui::Context,
        work: impl FnOnce(&dyn Fn(&str)) -> JobResult + Send + 'static,
    ) -> Self {
        Self::start(JobSpec::new(label, refresh), ctx, move |env| {
            work(&|l| env.progress(l))
        })
    }

    pub fn start(
        spec: JobSpec,
        ctx: &egui::Context,
        work: impl FnOnce(&JobEnv) -> JobResult + Send + 'static,
    ) -> Self {
        let op = Arc::new(Operation {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            label: spec.label.clone(),
            repo: spec.repo,
            background: spec.background,
            cancellable: spec.cancellable,
            started: SystemTime::now(),
            log: CommandLog::default(),
            cancel: CancelToken::new(),
            progress: Mutex::new(String::new()),
            outcome: Mutex::new(None),
        });
        record(op.clone());
        let (tx, rx) = mpsc::channel();
        let (o, ctx) = (op.clone(), ctx.clone());
        std::thread::spawn(move || {
            let started = Instant::now();
            let report = |line: &str| {
                if let Ok(mut s) = o.progress.lock() {
                    *s = line.trim().to_owned();
                }
                ctx.request_repaint();
            };
            let env = JobEnv {
                op: &o,
                report: &report,
            };
            let result = work(&env);
            let outcome = match &result {
                Ok(_) => Outcome::Succeeded,
                Err(_) if o.cancel.is_cancelled() => Outcome::Cancelled,
                Err(e) => Outcome::Failed(e.clone()),
            };
            if let Ok(mut out) = o.outcome.lock() {
                *out = Some((SystemTime::now(), outcome.clone()));
            }
            if !o.background
                && outcome != Outcome::Cancelled
                && started.elapsed() >= LONG_OPERATION
                && NOTIFY.load(Ordering::Relaxed)
                && !WINDOW_FOCUSED.load(Ordering::Relaxed)
            {
                crate::desktop_notify::operation_finished(&o.label, &outcome);
            }
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        Self {
            label: spec.label,
            refresh: spec.refresh,
            is_commit: false,
            background: spec.background,
            operation: op,
            rx,
        }
    }

    pub fn progress(&self) -> String {
        self.operation.progress()
    }

    pub fn cancellable(&self) -> bool {
        self.operation.cancellable
    }

    /// Asks the job to stop; its running git command is killed.
    pub fn cancel(&self) {
        self.operation.cancel.cancel();
    }

    pub fn cancel_requested(&self) -> bool {
        self.operation.cancel.is_cancelled()
    }

    /// The result, once the job has finished.
    pub fn poll(&self) -> Option<JobResult> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                let msg = String::from("the operation stopped unexpectedly");
                if let Ok(mut out) = self.operation.outcome.lock()
                    && out.is_none()
                {
                    *out = Some((SystemTime::now(), Outcome::Failed(msg.clone())));
                }
                Some(Err(msg))
            }
        }
    }
}
