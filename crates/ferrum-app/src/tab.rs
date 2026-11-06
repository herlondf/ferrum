//! One open repository: its data, the current view and what is selected.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use ferrum_core::cli::Git;
use ferrum_core::forge::{self, HostedRepo};
use ferrum_core::graph::{self, Graph};
use ferrum_core::ops::ResetMode;
use ferrum_core::*;
use std::result::Result;

use crate::command::Op;
use crate::jobs::{Job, JobOutput, Refresh};
use crate::ui::{blame, conflicts, forge as forge_ui, rebase};

const COMMIT_LIMIT: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    History,
    Changes,
    Conflicts,
    /// File history and blame.
    File,
    PullRequests,
}

pub struct Message {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

#[derive(Default)]
pub struct HistoryState {
    pub selected: Option<Oid>,
    /// Scroll the list so the selected commit is visible (after a jump).
    pub scroll_to_selected: bool,
    pub search: String,
    /// Search inside the changes of each commit (`git log -S`).
    pub content_search: bool,
    pub content_hits: Option<HashSet<Oid>>,
    /// Indices into `commits` that match the search; `None` shows everything.
    pub visible: Option<Vec<usize>>,
    pub focus_search: bool,
}

/// A commit shown in the details panel.
pub struct CommitView {
    pub details: CommitDetails,
    pub files: Vec<FileDiff>,
    pub selected_file: Option<usize>,
    /// Shown from the stash list rather than the history.
    pub is_stash: bool,
}

/// Lines picked in a diff, as (hunk, line) pairs.
#[derive(Default, Clone)]
pub struct LineSelection {
    pub lines: BTreeSet<(usize, usize)>,
    pub anchor: Option<(usize, usize)>,
}

impl LineSelection {
    pub fn clear(&mut self) {
        self.lines.clear();
        self.anchor = None;
    }
}

#[derive(Default)]
pub struct ChangesState {
    /// Which list (unstaged or staged) and which file is selected.
    pub selected: Option<(DiffSource, String)>,
    pub diff: Option<FileDiff>,
    pub selection: LineSelection,
    pub message: String,
    pub amend: bool,
    /// A commit job is running; clear the message when it succeeds.
    pub committing: bool,
    /// The message was filled from MERGE_MSG and similar.
    pub prefilled: bool,
}

pub struct UndoEntry {
    pub label: String,
    pub ops: Vec<Op>,
}

pub struct RepoTab {
    pub repo: Repo,
    pub git: Git,
    pub name: String,

    pub head: Option<String>,
    pub head_oid: Option<Oid>,
    pub state: RepoState,
    pub branches: Vec<BranchInfo>,
    pub tags: Vec<TagInfo>,
    pub stashes: Vec<StashInfo>,
    pub remotes: Vec<RemoteInfo>,
    pub submodules: Vec<SubmoduleInfo>,
    pub worktrees: Vec<WorktreeInfo>,
    pub identity: Identity,
    pub uses_lfs: bool,
    pub hosted: Option<HostedRepo>,
    pub status: WorkingStatus,

    pub commits: Vec<CommitInfo>,
    pub graph: Graph,
    pub labels: HashMap<Oid, Vec<RefLabel>>,
    /// Position of each commit in `commits`.
    pub index: HashMap<Oid, usize>,

    pub view: View,
    pub history: HistoryState,
    pub commit_view: Option<CommitView>,
    pub changes: ChangesState,
    pub conflicts: conflicts::ConflictState,
    pub rebase: Option<rebase::RebasePlan>,
    pub file_view: Option<blame::FileView>,
    pub prs: forge_ui::PrState,

    pub undo: Vec<UndoEntry>,
    pub job: Option<Job>,
    /// Undo entry for the running job, kept if it succeeds.
    pending_undo: Option<UndoEntry>,
    pub queue: VecDeque<Op>,
    pub message: Option<Message>,
}

impl RepoTab {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let repo = Repo::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let git = repo.git();
        let name = repo.name();
        let mut tab = Self {
            repo,
            git,
            name,
            head: None,
            head_oid: None,
            state: RepoState::Clean,
            branches: Vec::new(),
            tags: Vec::new(),
            stashes: Vec::new(),
            remotes: Vec::new(),
            submodules: Vec::new(),
            worktrees: Vec::new(),
            identity: Identity::default(),
            uses_lfs: false,
            hosted: None,
            status: WorkingStatus::default(),
            commits: Vec::new(),
            graph: Graph::default(),
            labels: HashMap::new(),
            index: HashMap::new(),
            view: View::History,
            history: HistoryState::default(),
            commit_view: None,
            changes: ChangesState::default(),
            conflicts: Default::default(),
            rebase: None,
            file_view: None,
            prs: Default::default(),
            undo: Vec::new(),
            job: None,
            pending_undo: None,
            queue: VecDeque::new(),
            message: None,
        };
        tab.refresh_all();
        Ok(tab)
    }

    pub fn path(&self) -> &std::path::Path {
        self.repo.path()
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: false,
            at: Instant::now(),
        });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: true,
            at: Instant::now(),
        });
    }

    pub fn refresh(&mut self, kind: Refresh) {
        match kind {
            Refresh::None => {}
            Refresh::Status => self.refresh_status(),
            Refresh::All => self.refresh_all(),
        }
    }

    /// Reloads everything: refs, history and local changes.
    pub fn refresh_all(&mut self) {
        let started = Instant::now();
        let mut errors = Vec::new();
        let mut ok = |r: Result<(), ferrum_core::Error>| {
            if let Err(e) = r {
                errors.push(e.to_string());
            }
        };
        ok(self.repo.branches().map(|v| self.branches = v));
        ok(self.repo.tags().map(|v| self.tags = v));
        ok(self.repo.remotes().map(|v| self.remotes = v));
        ok(self.repo.ref_labels().map(|v| self.labels = v));
        ok(self.repo.commits(COMMIT_LIMIT).map(|v| self.commits = v));
        self.stashes = self.repo.stashes().unwrap_or_default();
        self.submodules = self.repo.submodules().unwrap_or_default();
        self.worktrees = self.repo.worktrees().unwrap_or_default();
        self.graph = graph::layout(&self.commits);
        self.index = self
            .commits
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id, i))
            .collect();
        self.uses_lfs = self.repo.uses_lfs();
        self.hosted = self
            .remotes
            .iter()
            .find(|r| r.name == "origin")
            .or_else(|| self.remotes.first())
            .and_then(|r| forge::parse_remote_url(&r.url));
        self.apply_search();
        self.refresh_status();

        // Keep the selected commit if it still exists.
        if let Some(id) = self.history.selected {
            if self.index.contains_key(&id) {
                if self
                    .commit_view
                    .as_ref()
                    .is_none_or(|v| v.details.info.id != id)
                {
                    self.load_commit(id, false);
                }
            } else {
                self.history.selected = None;
                self.commit_view = None;
            }
        }
        if let Some(e) = errors.into_iter().next() {
            self.error(e);
        }
        if std::env::var_os("FERRUM_TIMING").is_some() {
            eprintln!(
                "refresh_all {}: {:?} ({} commits)",
                self.name,
                started.elapsed(),
                self.commits.len()
            );
        }
    }

    /// Reloads HEAD, the repository state and local changes.
    pub fn refresh_status(&mut self) {
        self.head = self.repo.head_branch();
        self.head_oid = self.repo.head_oid();
        self.state = self.repo.state();
        self.identity = self.repo.identity();
        match self.repo.status() {
            Ok(s) => self.status = s,
            Err(e) => self.error(e.to_string()),
        }
        self.reload_change_diff();
        if self.view == View::Conflicts && self.status.conflicted.is_empty() {
            self.view = View::Changes;
        }
    }

    // ----- History -----

    pub fn apply_search(&mut self) {
        let q = self.history.search.trim().to_lowercase();
        if q.is_empty() {
            self.history.visible = None;
            return;
        }
        let visible = if self.history.content_search {
            match &self.history.content_hits {
                Some(hits) => (0..self.commits.len())
                    .filter(|&i| hits.contains(&self.commits[i].id))
                    .collect(),
                None => Vec::new(),
            }
        } else {
            self.commits
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.summary.to_lowercase().contains(&q)
                        || c.author.to_lowercase().contains(&q)
                        || c.email.to_lowercase().contains(&q)
                        || c.id.to_string().starts_with(&q)
                })
                .map(|(i, _)| i)
                .collect()
        };
        self.history.visible = Some(visible);
    }

    pub fn select_commit(&mut self, id: Oid) {
        self.view = View::History;
        if self.history.selected != Some(id) || self.commit_view.is_none() {
            self.history.selected = Some(id);
            self.load_commit(id, false);
        }
        self.history.scroll_to_selected = true;
    }

    /// Loads a commit into the details panel.
    pub fn load_commit(&mut self, id: Oid, is_stash: bool) {
        let result = self
            .repo
            .commit_details(id)
            .and_then(|d| Ok((d, self.repo.commit_diff(id)?)));
        match result {
            Ok((details, files)) => {
                let selected_file = (!files.is_empty()).then_some(0);
                self.commit_view = Some(CommitView {
                    details,
                    files,
                    selected_file,
                    is_stash,
                });
            }
            Err(e) => {
                self.commit_view = None;
                self.error(e.to_string());
            }
        }
    }

    // ----- Local changes -----

    pub fn select_change(&mut self, source: DiffSource, path: String) {
        self.changes.selected = Some((source, path));
        self.changes.selection.clear();
        self.reload_change_diff();
    }

    pub fn reload_change_diff(&mut self) {
        let Some((source, path)) = self.changes.selected.clone() else {
            self.changes.diff = None;
            return;
        };
        // The file may have moved between lists, or disappeared.
        let list = match source {
            DiffSource::Staged => &self.status.staged,
            _ => &self.status.unstaged,
        };
        if !list.iter().any(|f| f.path == path) {
            let other = match source {
                DiffSource::Staged => (DiffSource::Unstaged, &self.status.unstaged),
                _ => (DiffSource::Staged, &self.status.staged),
            };
            if other.1.iter().any(|f| f.path == path) {
                self.changes.selected = Some((other.0, path.clone()));
                self.changes.selection.clear();
                return self.reload_change_diff();
            }
            self.changes.selected = None;
            self.changes.diff = None;
            return;
        }
        match self.repo.file_diff(source, &path) {
            Ok(d) => {
                // Line numbers shift after staging; a stale selection would be misleading.
                if self.changes.diff.as_ref().map(|old| &old.hunks)
                    != d.as_ref().map(|new| &new.hunks)
                {
                    self.changes.selection.clear();
                }
                self.changes.diff = d;
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    // ----- Jobs and undo -----

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Runs an operation now, or after the current one finishes.
    pub fn run(&mut self, op: Op, ctx: &egui::Context) {
        if self.job.is_some() {
            self.queue.push_back(op);
            return;
        }
        self.pending_undo = self.undo_for(&op);
        self.start(op, ctx);
    }

    pub fn run_undo(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        if let Some(entry) = self.undo.pop() {
            self.pending_undo = None;
            self.start(
                Op::Sequence(format!("Undo: {}", entry.label), entry.ops),
                ctx,
            );
        }
    }

    fn start(&mut self, op: Op, ctx: &egui::Context) {
        let git = self.git.clone();
        let label = op.label();
        let refresh = op.refresh();
        let is_commit = matches!(op, Op::Commit { .. });
        let mut job = Job::spawn(label, refresh, ctx, move |progress| {
            op.execute(&git, progress)
        });
        job.is_commit = is_commit;
        self.job = Some(job);
    }

    /// Starts a job that is not a git operation (forge requests, searches).
    pub fn spawn_job(
        &mut self,
        label: &str,
        ctx: &egui::Context,
        work: impl FnOnce(&dyn Fn(&str)) -> crate::jobs::JobResult + Send + 'static,
    ) {
        if self.job.is_some() {
            self.error("Wait for the current operation to finish.");
            return;
        }
        self.pending_undo = None;
        self.job = Some(Job::spawn(label, Refresh::None, ctx, work));
    }

    /// Checks the running job. Returns its output when it has just finished.
    pub fn poll_job(&mut self, ctx: &egui::Context) -> Option<JobOutput> {
        let result = self.job.as_ref()?.poll()?;
        let job = self.job.take()?;
        self.refresh(job.refresh);
        let output = match result {
            Ok(out) => {
                if job.is_commit {
                    self.changes.message.clear();
                    self.changes.amend = false;
                    self.changes.committing = false;
                }
                if let Some(entry) = self.pending_undo.take()
                    && self.state == RepoState::Clean
                {
                    self.undo.push(entry);
                }
                if job.refresh != Refresh::None {
                    self.info(format!("{} — done", job.label));
                }
                Some(out)
            }
            Err(e) => {
                self.pending_undo = None;
                self.changes.committing = false;
                self.error(format!("{} failed: {e}", job.label));
                // Do not run queued operations on top of a failure.
                self.queue.clear();
                None
            }
        };
        if let Some(next) = self.queue.pop_front() {
            self.run(next, ctx);
        }
        output
    }

    fn back_to_current(&self) -> Option<Op> {
        match (&self.head, self.head_oid) {
            (Some(b), _) => Some(Op::Checkout(b.clone())),
            (None, Some(id)) => Some(Op::CheckoutDetached(id.to_string())),
            _ => None,
        }
    }

    fn branch_target(&self, name: &str) -> Option<Oid> {
        self.branches
            .iter()
            .find(|b| b.kind == BranchKind::Local && b.name == name)
            .and_then(|b| b.target)
    }

    /// How to undo `op`, based on the state before it runs.
    fn undo_for(&self, op: &Op) -> Option<UndoEntry> {
        let head = self.head_oid.map(|h| h.to_string());
        let ops = match op {
            Op::Commit { .. } => vec![Op::Reset {
                rev: head?,
                mode: ResetMode::Soft,
            }],
            Op::Merge { .. }
            | Op::Pull(_)
            | Op::Rebase { .. }
            | Op::InteractiveRebase { .. }
            | Op::CherryPick { .. }
            | Op::Revert { .. }
            | Op::Reset { .. } => vec![Op::Reset {
                rev: head?,
                mode: ResetMode::Keep,
            }],
            Op::Checkout(_) | Op::CheckoutDetached(_) => vec![self.back_to_current()?],
            Op::CheckoutRemote { local, .. } => {
                vec![
                    self.back_to_current()?,
                    Op::DeleteBranch {
                        name: local.clone(),
                        force: true,
                    },
                ]
            }
            Op::CreateBranch { name, checkout, .. } => {
                let mut v = Vec::new();
                if *checkout {
                    v.push(self.back_to_current()?);
                }
                v.push(Op::DeleteBranch {
                    name: name.clone(),
                    force: true,
                });
                v
            }
            Op::RenameBranch { old, new } => vec![Op::RenameBranch {
                old: new.clone(),
                new: old.clone(),
            }],
            Op::DeleteBranch { name, .. } => vec![Op::CreateBranch {
                name: name.clone(),
                start: self.branch_target(name)?.to_string(),
                checkout: false,
            }],
            Op::MoveBranch { name, .. } => {
                vec![Op::MoveBranch {
                    name: name.clone(),
                    target: self.branch_target(name)?.to_string(),
                }]
            }
            Op::CreateTag { name, .. } => vec![Op::DeleteTag(name.clone())],
            Op::DeleteTag(name) => {
                let tag = self.tags.iter().find(|t| &t.name == name)?;
                vec![Op::CreateTag {
                    name: name.clone(),
                    rev: tag.target.to_string(),
                    message: None,
                }]
            }
            Op::StashPush { .. } => vec![Op::StashApply {
                stash: StashInfo {
                    index: 0,
                    id: Oid::ZERO_SHA1,
                    message: String::new(),
                },
                pop: true,
            }],
            _ => return None,
        };
        Some(UndoEntry {
            label: op.label(),
            ops,
        })
    }

    /// The upstream remote of the current branch, or the first remote.
    pub fn default_remote(&self) -> Option<String> {
        let current = self
            .branches
            .iter()
            .find(|b| b.is_head && b.kind == BranchKind::Local);
        current
            .and_then(|b| b.upstream.as_deref())
            .and_then(|u| u.split_once('/'))
            .map(|(r, _)| r.to_owned())
            .or_else(|| {
                self.remotes
                    .iter()
                    .find(|r| r.name == "origin")
                    .map(|r| r.name.clone())
            })
            .or_else(|| self.remotes.first().map(|r| r.name.clone()))
    }

    pub fn current_branch(&self) -> Option<&BranchInfo> {
        self.branches
            .iter()
            .find(|b| b.is_head && b.kind == BranchKind::Local)
    }
}
