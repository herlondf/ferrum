//! "Fetch all" and "Pull all (fast-forward only)" of the Repository Manager:
//! one app-wide job (listed in the Activity window, cancellable) that works
//! on several repositories at a time and reports each one's progress and
//! result to the dashboard through a shared [`Batch`].

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ferrum_core::cli::{CancelToken, Git};
use ferrum_core::glance::{self, Glance};
use ferrum_core::ops::{self, PullMode};

use crate::jobs::{JobEnv, JobOutput, JobResult};

/// Repositories worked on at the same time.
const WORKERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Fetch,
    /// `git pull --ff-only` of the current branch.
    Pull,
}

impl Kind {
    pub fn label(self, n: usize) -> String {
        match self {
            Kind::Fetch => t!("Fetch {} repositories", n),
            Kind::Pull => t!("Pull {} repositories (fast-forward only)", n),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemState {
    Waiting,
    /// Running, with git's last progress line.
    Running(String),
    Done(String),
    /// Nothing to do (no remote, no upstream…), with the reason.
    Skipped(String),
    Failed(String),
    Cancelled,
}

impl ItemState {
    pub fn finished(&self) -> bool {
        !matches!(self, ItemState::Waiting | ItemState::Running(_))
    }
}

pub struct Item {
    pub path: PathBuf,
    pub state: ItemState,
    /// The repository's glance after the operation.
    pub glance: Option<Glance>,
}

pub struct Batch {
    pub kind: Kind,
    pub items: Vec<Item>,
    /// Stops the job (set once it started).
    pub cancel: Option<CancelToken>,
}

pub type Shared = Arc<Mutex<Batch>>;

impl Batch {
    pub fn new(kind: Kind, paths: Vec<PathBuf>) -> Shared {
        Arc::new(Mutex::new(Batch {
            kind,
            items: paths
                .into_iter()
                .map(|path| Item {
                    path,
                    state: ItemState::Waiting,
                    glance: None,
                })
                .collect(),
            cancel: None,
        }))
    }

    pub fn done(&self) -> usize {
        self.items.iter().filter(|i| i.state.finished()).count()
    }

    pub fn running(&self) -> bool {
        self.done() < self.items.len()
    }

    pub fn failed(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i.state, ItemState::Failed(_)))
            .count()
    }

    pub fn state_of(&self, path: &std::path::Path) -> Option<&ItemState> {
        self.items.iter().find(|i| i.path == path).map(|i| &i.state)
    }

    /// "5 fetched, 1 skipped, 1 failed".
    pub fn summary(&self) -> String {
        let count = |f: fn(&ItemState) -> bool| self.items.iter().filter(|i| f(&i.state)).count();
        let done = count(|s| matches!(s, ItemState::Done(_)));
        let skipped = count(|s| matches!(s, ItemState::Skipped(_)));
        let failed = self.failed();
        let cancelled = count(|s| matches!(s, ItemState::Cancelled));
        let mut parts = vec![match self.kind {
            Kind::Fetch => t!("{} fetched", done),
            Kind::Pull => t!("{} pulled", done),
        }];
        if skipped > 0 {
            parts.push(t!("{} skipped", skipped));
        }
        if failed > 0 {
            parts.push(t!("{} failed", failed));
        }
        if cancelled > 0 {
            parts.push(t!("{} cancelled", cancelled));
        }
        parts.join(", ")
    }
}

fn set(shared: &Shared, i: usize, state: ItemState, glance: Option<Glance>) {
    if let Ok(mut b) = shared.lock() {
        let item = &mut b.items[i];
        item.state = state;
        if glance.is_some() {
            item.glance = glance;
        }
    }
}

/// The job's work: every repository of the batch, `WORKERS` at a time.
pub fn run(shared: &Shared, env: &JobEnv, repaint: &(dyn Fn() + Sync)) -> JobResult {
    let (kind, paths) = {
        let b = shared.lock().map_err(|e| e.to_string())?;
        let paths: Vec<PathBuf> = b.items.iter().map(|i| i.path.clone()).collect();
        (b.kind, paths)
    };
    // Each repository's git runs with the job's command log and cancel token.
    let gits: Vec<Git> = paths.iter().map(|p| env.git(&Git::new(p))).collect();
    let next = AtomicUsize::new(0);
    let total = paths.len();
    std::thread::scope(|s| {
        for _ in 0..WORKERS.min(total) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(git) = gits.get(i) else { break };
                    let cancelled = git.cancel_token().is_some_and(|c| c.is_cancelled());
                    if cancelled {
                        set(shared, i, ItemState::Cancelled, None);
                        continue;
                    }
                    set(shared, i, ItemState::Running(String::new()), None);
                    repaint();
                    let state = one(kind, git, &|line| {
                        set(shared, i, ItemState::Running(line.trim().to_owned()), None);
                        repaint();
                    });
                    let state = match state {
                        ItemState::Failed(_)
                            if git.cancel_token().is_some_and(|c| c.is_cancelled()) =>
                        {
                            ItemState::Cancelled
                        }
                        s => s,
                    };
                    let after = glance::glance(&paths[i]).ok();
                    set(shared, i, state, after);
                    repaint();
                }
            });
        }
    });
    if gits
        .first()
        .and_then(Git::cancel_token)
        .is_some_and(CancelToken::is_cancelled)
    {
        return Err(ferrum_core::Error::Cancelled.to_string());
    }
    let b = shared.lock().map_err(|e| e.to_string())?;
    Ok(JobOutput::Repositories {
        paths,
        message: b.summary(),
        failed: b.failed() > 0,
    })
}

/// Fetches or pulls one repository.
fn one(kind: Kind, git: &Git, progress: &dyn Fn(&str)) -> ItemState {
    let Ok(repo) = ferrum_core::Repo::open(git.dir()) else {
        return ItemState::Failed(t!("Not a git repository"));
    };
    let remotes = repo.remotes().unwrap_or_default();
    if remotes.is_empty() {
        return ItemState::Skipped(t!("No remotes"));
    }
    match kind {
        Kind::Fetch => match ops::fetch(git, None, progress) {
            Ok(()) => ItemState::Done(t!("Fetched")),
            Err(e) => ItemState::Failed(first_line(&e.to_string())),
        },
        Kind::Pull => {
            let before = glance::glance(git.dir()).ok();
            if before.as_ref().is_some_and(|g| g.branch.is_none()) {
                return ItemState::Skipped(t!("Detached HEAD"));
            }
            if before.as_ref().is_some_and(|g| g.ahead_behind.is_none()) {
                return ItemState::Skipped(t!("No upstream branch"));
            }
            if before.as_ref().is_some_and(|g| g.in_progress) {
                return ItemState::Skipped(t!("An operation is in progress"));
            }
            let head = repo.head_oid();
            match ops::pull(git, PullMode::FastForwardOnly, progress) {
                Ok(()) if repo.head_oid() == head => ItemState::Done(t!("Already up to date")),
                Ok(()) => {
                    let n = before.and_then(|g| g.ahead_behind).map_or(0, |a| a.1);
                    ItemState::Done(if n > 0 {
                        t!("Fast-forwarded {} commits", n)
                    } else {
                        t!("Fast-forwarded")
                    })
                }
                Err(e) => ItemState::Failed(first_line(&e.to_string())),
            }
        }
    }
}

/// The most telling line of a git error (`fatal:` / `error:` first).
fn first_line(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines
        .iter()
        .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
        .or(lines.first())
        .map(|l| l.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_states() {
        let shared = Batch::new(Kind::Fetch, vec!["a".into(), "b".into(), "c".into()]);
        {
            let mut b = shared.lock().unwrap();
            b.items[0].state = ItemState::Done("ok".into());
            b.items[1].state = ItemState::Skipped("no".into());
            assert!(b.running());
            b.items[2].state = ItemState::Failed("x".into());
            assert!(!b.running());
        }
        let b = shared.lock().unwrap();
        assert_eq!(b.summary(), "1 fetched, 1 skipped, 1 failed");
        assert_eq!(b.failed(), 1);
        assert_eq!(
            first_line("hint: x\nfatal: Not possible to fast-forward\n"),
            "fatal: Not possible to fast-forward"
        );
    }
}
