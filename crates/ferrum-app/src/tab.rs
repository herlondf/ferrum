//! One open repository: its data, the current view and what is selected.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use ferrum_core::cli::Git;
use ferrum_core::forge::HostedRepo;
use ferrum_core::graph::Graph;
use ferrum_core::ops::ResetMode;
use ferrum_core::*;
use std::result::Result;

use crate::command::Op;
use crate::jobs::{Job, JobOutput, JobSpec, Refresh};
use crate::ui::{blame, conflicts, forge as forge_ui, history_model, rebase};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    History,
    Changes,
    Conflicts,
    /// File history and blame.
    File,
    PullRequests,
    /// README, remotes and statistics (`ui::overview`).
    Overview,
    Issues,
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
    /// Commits found by a search job (changes or paths).
    pub content_hits: Option<HashSet<Oid>>,
    /// Indices into `commits` that match the search; `None` shows everything.
    pub visible: Option<Vec<usize>>,
    pub focus_search: bool,
    pub ext: history_model::HistoryExt,
}

/// A commit shown in the details panel.
pub struct CommitView {
    pub details: CommitDetails,
    /// Changed files, without content (see `diff`).
    pub files: Vec<FileDiff>,
    pub selected_file: Option<usize>,
    /// Diff of the selected file, loaded when it is selected.
    pub diff: Option<FileDiff>,
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
    /// State of the file lists and the commit box.
    pub ui: crate::ui::changes::ChangesUi,
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
    /// Stacked branches (see `ui::stacks`).
    pub stacks: Vec<ferrum_core::stack::Stack>,
    pub identity: Identity,
    pub uses_lfs: bool,
    pub hosted: Option<HostedRepo>,
    pub status: WorkingStatus,
    /// Where a running bisect stands.
    pub bisect: Option<ferrum_core::bisect::BisectStatus>,

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
    pub file_tools: crate::ui::file_window::FileTools,
    pub prs: forge_ui::PrState,
    pub issues: crate::ui::issues::IssueState,
    pub sidebar: crate::ui::sidebar_model::SidebarState,
    pub pane: crate::ui::focus::PaneFocus,
    pub diff_options: crate::ui::diff_prefs::TabDiffOptions,
    /// Details tabs, file tree, compare view and commit windows.
    pub details: crate::ui::details_model::DetailsState,
    pub overview: crate::ui::overview::OverviewState,

    pub undo: Vec<UndoEntry>,
    pub job: Option<Job>,
    /// Undo entry for the running job, kept if it succeeds.
    pending_undo: Option<UndoEntry>,
    pub queue: VecDeque<Op>,
    pub message: Option<Message>,
    /// Background refresh and file watching (see `refresh.rs`).
    pub live: crate::refresh::Live,
    /// Automatic fetch (see `sync.rs`).
    pub sync: crate::sync::TabSync,
}

impl RepoTab {
    /// Opens a repository with its saved history options.
    pub fn open_with(
        path: PathBuf,
        history: &history_model::HistorySettings,
    ) -> Result<Self, String> {
        let mut tab = Self::new(path)?;
        tab.history.ext.prefs = crate::repo_settings::history_prefs(history, &tab);
        tab.refresh_all();
        Ok(tab)
    }

    /// A tab with nothing loaded yet.
    pub fn new(path: PathBuf) -> Result<Self, String> {
        let repo = Repo::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let git = repo.git();
        let name = repo.name();
        Ok(Self {
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
            stacks: Vec::new(),
            identity: Identity::default(),
            uses_lfs: false,
            hosted: None,
            status: WorkingStatus::default(),
            bisect: None,
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
            file_tools: Default::default(),
            prs: Default::default(),
            issues: Default::default(),
            sidebar: Default::default(),
            pane: Default::default(),
            diff_options: Default::default(),
            details: Default::default(),
            overview: Default::default(),
            undo: Vec::new(),
            job: None,
            pending_undo: None,
            queue: VecDeque::new(),
            message: None,
            live: Default::default(),
            sync: Default::default(),
        })
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
        ok(history_model::load(self));
        self.stashes = self.repo.stashes().unwrap_or_default();
        self.submodules = self.repo.submodules().unwrap_or_default();
        self.worktrees = self.repo.worktrees().unwrap_or_default();
        self.stacks = ferrum_core::stack::detect(&self.repo).unwrap_or_default();
        self.uses_lfs = self.repo.uses_lfs();
        self.hosted = crate::hosted::hosted_for(self);
        self.apply_search();
        self.refresh_status();
        self.keep_selected_commit();
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

    /// Keeps the selected commit after a reload if it still exists.
    pub fn keep_selected_commit(&mut self) {
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
    }

    /// Reloads HEAD, the repository state and local changes.
    pub fn refresh_status(&mut self) {
        self.head = self.repo.head_branch();
        self.head_oid = self.repo.head_oid();
        self.state = self.repo.state();
        self.bisect = match self.state {
            RepoState::Bisect => self.repo.bisect_status(),
            _ => None,
        };
        self.identity = self.repo.identity();
        match self.repo.status() {
            Ok(s) => self.status = s,
            Err(e) => self.error(e.to_string()),
        }
        // When amending, the staged list also shows the last commit's changes.
        if self.changes.amend
            && let Ok(list) = self.repo.amend_staged()
        {
            self.status.staged = list;
        }
        self.reload_change_diff();
        if self.view == View::Conflicts && self.status.conflicted.is_empty() {
            self.view = View::Changes;
        }
    }

    // ----- History -----

    pub fn apply_search(&mut self) {
        history_model::apply_search(self);
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
        self.details.compare = None;
        self.details.picked.clear();
        // Only the file list here: diffs load per file, so huge commits stay fast.
        let result = self
            .repo
            .commit_details(id)
            .and_then(|d| Ok((d, self.repo.commit_files(id)?)));
        match result {
            Ok((details, files)) => {
                self.commit_view = Some(CommitView {
                    details,
                    files,
                    selected_file: None,
                    diff: None,
                    is_stash,
                });
                self.select_commit_file(0);
            }
            Err(e) => {
                self.commit_view = None;
                self.error(e.to_string());
            }
        }
    }

    /// Selects a file of the shown commit and loads its diff.
    pub fn select_commit_file(&mut self, i: usize) {
        let Some(view) = &mut self.commit_view else {
            return;
        };
        let Some(file) = view.files.get(i) else {
            return;
        };
        let id = view.details.info.id;
        view.selected_file = Some(i);
        let cfg = self.diff_options.config;
        match self
            .repo
            .commit_file_diff_with(id, &file.path, file.old_path.as_deref(), &cfg)
        {
            Ok(diff) => view.diff = diff,
            Err(e) => {
                view.diff = None;
                let msg = e.to_string();
                self.error(msg);
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
        let diff = if source == DiffSource::Staged && self.changes.amend {
            self.repo.amend_file_diff(&path)
        } else {
            let cfg = self.diff_options.for_path(&path);
            self.repo.file_diff_with(source, &path, &cfg)
        };
        match diff {
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
        // The background reload may not have landed yet; undo needs the real HEAD.
        self.head = self.repo.head_branch();
        self.head_oid = self.repo.head_oid();
        self.pending_undo = self.undo_for(&op);
        self.start(op, ctx);
    }

    pub fn run_undo(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        if let Some(entry) = self.undo.pop() {
            self.pending_undo = None;
            self.start(Op::Sequence(t!("Undo: {}", entry.label), entry.ops), ctx);
        }
    }

    /// Runs an operation on behalf of the app (e.g. auto-fetch): it is listed
    /// under Background in the activity window and fails quietly. Returns
    /// `false`, doing nothing, while another operation is running or queued.
    pub fn run_background(&mut self, op: Op, ctx: &egui::Context) -> bool {
        if self.job.is_some() || !self.queue.is_empty() {
            return false;
        }
        self.pending_undo = None;
        self.start_job(op, ctx, true);
        true
    }

    /// Stops the running operation and drops the queued ones.
    pub fn cancel_job(&mut self) {
        if let Some(job) = &self.job
            && job.cancellable()
        {
            job.cancel();
            self.queue.clear();
        }
    }

    fn start(&mut self, op: Op, ctx: &egui::Context) {
        self.start_job(op, ctx, false);
    }

    fn start_job(&mut self, op: Op, ctx: &egui::Context, background: bool) {
        let git = self.git.clone();
        let is_commit = matches!(op, Op::Commit { .. });
        let spec = JobSpec::new(op.label(), op.refresh())
            .repo(self.path())
            .background(background)
            .cancellable();
        let mut job = Job::start(spec, ctx, move |env| {
            op.execute(&env.git(&git), &|l| env.progress(l))
        });
        job.is_commit = is_commit;
        self.job = Some(job);
        self.live.job_started();
    }

    /// Starts a job that is not a git operation (forge requests, searches).
    pub fn spawn_job(
        &mut self,
        label: &str,
        ctx: &egui::Context,
        work: impl FnOnce(&dyn Fn(&str)) -> crate::jobs::JobResult + Send + 'static,
    ) {
        if self.job.is_some() {
            self.error(t!("Wait for the current operation to finish."));
            return;
        }
        self.pending_undo = None;
        let spec = JobSpec::new(label, Refresh::None).repo(self.path());
        self.job = Some(Job::start(spec, ctx, move |env| work(&|l| env.progress(l))));
    }

    /// Checks the running job. Returns its output when it has just finished.
    pub fn poll_job(&mut self, ctx: &egui::Context) -> Option<JobOutput> {
        let result = self.job.as_ref()?.poll()?;
        let job = self.job.take()?;
        // Reloads in the background; the state is read now for the undo check.
        self.request_refresh(job.refresh);
        self.state = self.repo.state();
        let output = match result {
            Err(_) if job.cancel_requested() => {
                self.pending_undo = None;
                self.changes.committing = false;
                self.queue.clear();
                self.info(t!("{} cancelled", job.label));
                None
            }
            // Shown in the activity window; no error in the status bar.
            Err(_) if job.background => None,
            Ok(out) if job.background => Some(out),
            Ok(out) => {
                if job.is_commit {
                    let was_amend = self.changes.amend;
                    self.changes.message.clear();
                    self.changes.amend = false;
                    self.changes.committing = false;
                    if was_amend {
                        self.refresh_status();
                    }
                }
                if let Some(entry) = self.pending_undo.take()
                    && self.state == RepoState::Clean
                {
                    self.undo.push(entry);
                }
                if job.refresh != Refresh::None {
                    self.info(t!("{} — done", job.label));
                }
                Some(out)
            }
            Err(e) => {
                self.pending_undo = None;
                self.changes.committing = false;
                self.error(t!("{} failed: {}", job.label, e));
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
        self.repo
            .resolve(&format!("refs/heads/{name}"))
            .ok()
            .or_else(|| {
                self.branches
                    .iter()
                    .find(|b| b.kind == BranchKind::Local && b.name == name)
                    .and_then(|b| b.target)
            })
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
            | Op::PullWith(_)
            | Op::Rebase { .. }
            | Op::InteractiveRebase { .. }
            | Op::Reset { .. } => vec![Op::Reset {
                rev: head?,
                mode: ResetMode::Keep,
            }],
            Op::Flow(f) if f.rewrites_head() => vec![Op::Reset {
                rev: head?,
                mode: ResetMode::Keep,
            }],
            Op::Flow(crate::flow::FlowOp::WithLocalChanges { op, .. }) => return self.undo_for(op),
            Op::Stack(s) => return s.undo(self),
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
            // Undo is simple only when the file had no changes of its own.
            Op::ResetFileTo { path, .. } if self.file_is_clean(path) => {
                self.repo.file_in_commit(self.head_oid?, path).ok()??;
                vec![Op::ResetFileTo {
                    path: path.clone(),
                    rev: head?,
                }]
            }
            Op::FastForward(pairs) => crate::sync::undo_fast_forward(self, pairs)?,
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

    /// The file has no staged or unstaged changes.
    fn file_is_clean(&self, path: &str) -> bool {
        let s = &self.status;
        !s.unstaged.iter().chain(&s.staged).any(|f| f.path == path)
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
