//! Watches a repository's working folder and `.git` folder for changes made
//! by other programs, so the views update without polling.

use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use eframe::egui;
use notify::event::{AccessKind, AccessMode, EventKind};
use notify::{RecursiveMode, Watcher};

/// What changed since the last look.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// Files in the working folder, or the index.
    pub work_tree: bool,
    /// HEAD, branches, tags or the operation in progress.
    pub refs: bool,
}

impl Change {
    fn any(self) -> bool {
        self.work_tree || self.refs
    }

    fn merge(&mut self, other: Change) {
        self.work_tree |= other.work_tree;
        self.refs |= other.refs;
    }
}

/// Waits this long after the last event before reporting, so a burst
/// (a build, a checkout) becomes one reload.
const QUIET: Duration = Duration::from_millis(300);
/// …but reports at least this often during a long burst.
const MAX_DELAY: Duration = Duration::from_secs(2);

pub struct RepoWatcher {
    _watcher: notify::RecommendedWatcher,
    rx: Receiver<Change>,
}

impl RepoWatcher {
    pub fn start(repo: &ferrum_core::Repo, ctx: &egui::Context) -> notify::Result<Self> {
        // Events carry real paths (e.g. /private/var on macOS for /var).
        let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let dirs = Dirs {
            work: real(repo.path()),
            git: real(repo.git_dir()),
            common: real(repo.common_dir()),
        };
        let (raw_tx, raw_rx) = mpsc::channel::<notify::Event>();
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let _ = raw_tx.send(event);
                }
            })?;
        watcher.watch(&dirs.work, RecursiveMode::Recursive)?;
        // Linked worktrees keep their git folders elsewhere.
        for extra in [&dirs.git, &dirs.common] {
            if !extra.starts_with(&dirs.work) {
                watcher.watch(extra, RecursiveMode::Recursive)?;
            }
        }

        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // For .gitignore checks; opened here as it is not shared across threads.
            let repo = ferrum_core::Repo::open(&dirs.work).ok();
            let ignored = |rel: &Path| repo.as_ref().is_some_and(|r| r.is_ignored(rel));
            let mut pending = Change::default();
            let mut first: Option<Instant> = None;
            loop {
                match raw_rx.recv_timeout(QUIET) {
                    Ok(event) => {
                        if !relevant(&event.kind) {
                            continue;
                        }
                        for p in &event.paths {
                            pending.merge(dirs.classify(p, &ignored));
                        }
                        if pending.any() {
                            first.get_or_insert_with(Instant::now);
                        }
                        if !first.is_some_and(|f| f.elapsed() >= MAX_DELAY) {
                            continue;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    // The watcher was dropped with its tab.
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                if pending.any() {
                    if tx.send(std::mem::take(&mut pending)).is_err() {
                        return;
                    }
                    first = None;
                    ctx.request_repaint();
                }
            }
        });
        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }

    /// Changes reported since the last call, merged.
    pub fn take(&self) -> Option<Change> {
        let mut all = Change::default();
        while let Ok(c) = self.rx.try_recv() {
            all.merge(c);
        }
        all.any().then_some(all)
    }
}

/// Reading a file is not a change; finishing a write is.
fn relevant(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

struct Dirs {
    work: PathBuf,
    git: PathBuf,
    common: PathBuf,
}

impl Dirs {
    fn classify(&self, path: &Path, ignored: &dyn Fn(&Path) -> bool) -> Change {
        for dir in [&self.git, &self.common] {
            if let Ok(rel) = path.strip_prefix(dir) {
                return git_dir_change(rel);
            }
        }
        let Ok(rel) = path.strip_prefix(&self.work) else {
            return Change::default();
        };
        // Nested repositories (submodules) and ignored files do not show in status.
        if rel.as_os_str().is_empty()
            || rel
                .components()
                .any(|c| c == Component::Normal(".git".as_ref()))
            || ignored(rel)
        {
            return Change::default();
        }
        Change {
            work_tree: true,
            refs: false,
        }
    }
}

/// What a change inside the `.git` folder means. Object writes, logs and
/// lock files are churn; the index, HEAD and refs are what the views show.
fn git_dir_change(rel: &Path) -> Change {
    let mut parts = rel.components().filter_map(|c| match c {
        Component::Normal(s) => s.to_str(),
        _ => None,
    });
    let Some(first) = parts.next() else {
        return Change::default();
    };
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if name.ends_with(".lock") {
        return Change::default();
    }
    match first {
        "index" | "config" => Change {
            work_tree: true,
            refs: false,
        },
        "HEAD" | "ORIG_HEAD" | "FETCH_HEAD" | "MERGE_HEAD" | "CHERRY_PICK_HEAD" | "REVERT_HEAD"
        | "BISECT_LOG" | "packed-refs" | "refs" | "rebase-merge" | "rebase-apply" => Change {
            work_tree: false,
            refs: true,
        },
        _ => Change::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> Dirs {
        Dirs {
            work: PathBuf::from("/r"),
            git: PathBuf::from("/r/.git"),
            common: PathBuf::from("/r/.git"),
        }
    }

    fn classify(p: &str) -> Change {
        dirs().classify(Path::new(p), &|rel| rel.starts_with("target"))
    }

    const WORK: Change = Change {
        work_tree: true,
        refs: false,
    };
    const REFS: Change = Change {
        work_tree: false,
        refs: true,
    };

    #[test]
    fn sorts_out_what_changed() {
        assert_eq!(classify("/r/src/main.rs"), WORK);
        assert_eq!(classify("/r/.git/index"), WORK);
        assert_eq!(classify("/r/.git/HEAD"), REFS);
        assert_eq!(classify("/r/.git/refs/heads/main"), REFS);
        assert_eq!(classify("/r/.git/packed-refs"), REFS);
        assert_eq!(classify("/r/.git/MERGE_HEAD"), REFS);
        // Churn that does not change what is shown.
        assert_eq!(classify("/r/.git/objects/ab/cdef"), Change::default());
        assert_eq!(classify("/r/.git/index.lock"), Change::default());
        assert_eq!(classify("/r/.git/refs/heads/main.lock"), Change::default());
        assert_eq!(classify("/r/.git/logs/HEAD"), Change::default());
        assert_eq!(classify("/r/target/debug/app"), Change::default());
        assert_eq!(classify("/r/sub/.git/index"), Change::default());
        assert_eq!(classify("/elsewhere/file"), Change::default());
    }

    #[test]
    fn reports_edits_and_commits() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let git = ferrum_core::cli::Git::new(p);
        git.run(&["init", "-q", "-b", "main"]).unwrap();
        git.run(&["config", "user.name", "T"]).unwrap();
        git.run(&["config", "user.email", "t@example.com"]).unwrap();
        git.run(&["config", "commit.gpgsign", "false"]).unwrap();
        git.run(&["config", "core.autocrlf", "false"]).unwrap();
        let repo = ferrum_core::Repo::open(p).unwrap();
        let w = RepoWatcher::start(&repo, &egui::Context::default()).unwrap();

        let wait = |want: fn(Change) -> bool| {
            let mut seen = Change::default();
            for _ in 0..100 {
                if let Some(c) = w.take() {
                    seen.merge(c);
                }
                if want(seen) {
                    return seen;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!("no change reported: {seen:?}");
        };
        std::fs::write(p.join("a.txt"), "a\n").unwrap();
        wait(|c| c.work_tree);
        git.run(&["add", "-A"]).unwrap();
        git.run(&["commit", "-qm", "first"]).unwrap();
        wait(|c| c.refs);
    }
}
