//! What the views ask the app to do. Views push commands; the app runs them
//! after drawing, which keeps borrowing simple.

use std::path::PathBuf;

use ferrum_core::cli::Git;
use ferrum_core::forge::{self, HostedRepo, NewPullRequest};
use ferrum_core::ops::{self, MergeMode, PatchTarget, PullMode, PushOptions, ResetMode, Side};
use ferrum_core::rebase::Step;
use ferrum_core::{FileChange, Identity, Oid, RepoState, StashInfo};

use crate::jobs::{JobOutput, JobResult, Refresh};
use crate::tab::View;
use crate::ui::dialogs::Dialog;
use crate::util::state_name;

pub enum Command {
    /// Run a git operation on the active repository.
    Run(Op),
    Undo,
    Refresh,
    OpenRepo(PathBuf),
    /// Ask for a folder and open it.
    OpenRepoDialog,
    CloseTab(usize),
    SelectTab(usize),
    Dialog(Dialog),
    SetView(View),
    /// Select a commit in the history (switching to the history view).
    SelectCommit(Oid),
    /// Show a commit that is not in the history list (e.g. a stash).
    ShowCommit(Oid),
    /// Open the file history / blame view for a path.
    FileHistory(String),
    Blame(String),
    /// Open the conflict resolver for a path.
    ResolveConflict(String),
    /// Start planning an interactive rebase of the commits after `base` (`None` = root).
    PlanRebase(Option<Oid>),
    LoadPullRequests,
    CreatePullRequest(NewPullRequest),
    VerifyAccount {
        kind: ferrum_core::forge::ForgeKind,
        host: String,
        token: String,
    },
    Clone {
        url: String,
        parent: PathBuf,
        name: String,
    },
    Init(PathBuf),
    Error(String),
    Copy(String),
    OpenUrl(String),
    ShowSettings,
    ShowPalette,
    /// Open or close the Activity window.
    ShowActivity,
    /// Stop the running operation (of the active tab, or the app-wide one).
    CancelJob,
    FocusSearch,
    /// Open something in another program (editor, terminal, diff tool...).
    External(crate::external::Launch),
    /// Write `user.name` / `user.email` to the global git config.
    SetGlobalIdentity(ferrum_core::config::GlobalIdentity),
    /// Change history options or graph filters (see `ui::history_options`).
    History(crate::ui::history_options::HistoryFn),
    /// Development aid from `FERRUM_START` (see `startup_commands` in app.rs).
    DevStep(DevStep),
}

pub enum DevStep {
    Search(String),
    Select(ferrum_core::DiffSource, String),
    Theme(crate::settings::ThemeChoice),
    /// Open Settings at a section, e.g. "appearance".
    SettingsSection(String),
    /// `sidebar:open:KEY`, `sidebar:pin:REF`, `sidebar:filter:TEXT`, `sidebar:focus`.
    Sidebar(String),
}

/// Collects commands while the UI is drawn.
#[derive(Default)]
pub struct Commands(Vec<Command>);

impl Commands {
    pub fn push(&mut self, c: Command) {
        self.0.push(c);
    }

    pub fn run(&mut self, op: Op) {
        self.0.push(Command::Run(op));
    }

    pub fn take(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.0)
    }
}

/// A git operation. Runs on a background thread with only the repository path.
#[derive(Debug, Clone)]
pub enum Op {
    Stage(Vec<String>),
    StageAll,
    Unstage(Vec<String>),
    UnstageAll,
    Discard(Vec<FileChange>),
    ApplyPatch {
        patch: String,
        target: PatchTarget,
        reverse: bool,
        label: &'static str,
    },
    Commit {
        message: String,
        amend: bool,
    },
    Fetch {
        remote: Option<String>,
    },
    Pull(PullMode),
    Push(PushOptions),
    PushTag {
        remote: String,
        tag: String,
    },
    DeleteRemoteRef {
        remote: String,
        name: String,
    },
    Checkout(String),
    CheckoutRemote {
        remote_branch: String,
        local: String,
    },
    CheckoutDetached(String),
    CreateBranch {
        name: String,
        start: String,
        checkout: bool,
    },
    RenameBranch {
        old: String,
        new: String,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    MoveBranch {
        name: String,
        target: String,
    },
    SetUpstream {
        branch: String,
        upstream: String,
    },
    UnsetUpstream(String),
    Merge {
        rev: String,
        mode: MergeMode,
    },
    Rebase {
        onto: String,
    },
    InteractiveRebase {
        base: Option<Oid>,
        steps: Vec<Step>,
    },
    CherryPick {
        id: Oid,
        is_merge: bool,
    },
    Revert {
        id: Oid,
        is_merge: bool,
    },
    Reset {
        rev: String,
        mode: ResetMode,
    },
    CreateTag {
        name: String,
        rev: String,
        message: Option<String>,
    },
    DeleteTag(String),
    StashPush {
        message: String,
        untracked: bool,
        staged_only: bool,
    },
    StashApply {
        stash: StashInfo,
        pop: bool,
    },
    StashDrop(StashInfo),
    Continue(RepoState),
    Abort(RepoState),
    Skip(RepoState),
    ResolveWith {
        path: String,
        side: Side,
    },
    SaveResolution {
        path: String,
        content: String,
    },
    AddRemote {
        name: String,
        url: String,
    },
    RemoveRemote(String),
    Lfs(&'static str),
    SubmoduleUpdate,
    WorktreeAdd {
        path: String,
        rev: String,
        new_branch: Option<String>,
    },
    WorktreeRemove {
        path: String,
        force: bool,
    },
    SetIdentity(Identity),
    /// Opens a pull/merge request. An `Op` (not a plain job) so it can be
    /// queued behind the `Push` of its branch.
    CreatePullRequest {
        hosted: HostedRepo,
        token: String,
        pr: NewPullRequest,
    },
    /// Several operations in a row (used by undo).
    Sequence(String, Vec<Op>),
}

impl Op {
    pub fn label(&self) -> String {
        match self {
            Op::Stage(p) => t!("Stage {}", files(p.len())),
            Op::StageAll => t!("Stage all changes"),
            Op::Unstage(p) => t!("Unstage {}", files(p.len())),
            Op::UnstageAll => t!("Unstage all changes"),
            Op::Discard(f) => t!("Discard changes in {}", files(f.len())),
            // Labels come from the views; translated here if the table has them.
            Op::ApplyPatch { label, .. } => match *label {
                "Stage selection" => t!("Stage selection"),
                "Unstage selection" => t!("Unstage selection"),
                "Discard selection" => t!("Discard selection"),
                other => other.to_owned(),
            },
            Op::Commit { amend: true, .. } => t!("Amend commit"),
            Op::Commit { .. } => t!("Commit"),
            Op::Fetch { remote: Some(r) } => t!("Fetch {}", r),
            Op::Fetch { remote: None } => t!("Fetch all remotes"),
            Op::Pull(_) => t!("Pull"),
            Op::Push(o) => t!("Push {} to {}", o.branch, o.remote),
            Op::PushTag { tag, remote } => t!("Push tag {} to {}", tag, remote),
            Op::DeleteRemoteRef { remote, name } => t!("Delete {} from {}", name, remote),
            Op::Checkout(b) => t!("Check out {}", b),
            Op::CheckoutRemote { local, .. } => t!("Check out {}", local),
            Op::CheckoutDetached(r) => t!("Check out {}", short(r)),
            Op::CreateBranch { name, .. } => t!("Create branch {}", name),
            Op::RenameBranch { old, new } => t!("Rename {} to {}", old, new),
            Op::DeleteBranch { name, .. } => t!("Delete branch {}", name),
            Op::MoveBranch { name, .. } => t!("Move branch {}", name),
            Op::SetUpstream { branch, upstream } => t!("Track {} from {}", upstream, branch),
            Op::UnsetUpstream(b) => t!("Stop tracking for {}", b),
            Op::Merge { rev, .. } => t!("Merge {}", short(rev)),
            Op::Rebase { onto } => t!("Rebase onto {}", short(onto)),
            Op::InteractiveRebase { steps, .. } => {
                t!("Interactive rebase of {}", commits(steps.len()))
            }
            Op::CherryPick { id, .. } => t!("Cherry-pick {}", ferrum_core::short(*id)),
            Op::Revert { id, .. } => t!("Revert {}", ferrum_core::short(*id)),
            Op::Reset { rev, .. } => t!("Reset to {}", short(rev)),
            Op::CreateTag { name, .. } => t!("Create tag {}", name),
            Op::DeleteTag(n) => t!("Delete tag {}", n),
            Op::StashPush { .. } => t!("Stash changes"),
            Op::StashApply { stash, pop: true } => t!("Pop {}", stash.refname()),
            Op::StashApply { stash, .. } => t!("Apply {}", stash.refname()),
            Op::StashDrop(s) => t!("Drop {}", s.refname()),
            Op::Continue(s) => t!("Continue {}", state_name(*s)),
            Op::Abort(s) => t!("Abort {}", state_name(*s)),
            Op::Skip(s) => t!("Skip commit in {}", state_name(*s)),
            Op::ResolveWith { path, side } => {
                if *side == Side::Ours {
                    t!("Resolve {} with ours", path)
                } else {
                    t!("Resolve {} with theirs", path)
                }
            }
            Op::SaveResolution { path, .. } => t!("Resolve {}", path),
            Op::AddRemote { name, .. } => t!("Add remote {}", name),
            Op::RemoveRemote(n) => t!("Remove remote {}", n),
            Op::Lfs(a) => format!("Git LFS {a}"),
            Op::SubmoduleUpdate => t!("Update submodules"),
            Op::WorktreeAdd { path, .. } => t!("Add worktree {}", path),
            Op::WorktreeRemove { path, .. } => t!("Remove worktree {}", path),
            Op::SetIdentity(_) => t!("Set repository identity"),
            Op::CreatePullRequest { hosted, .. } => {
                t!("Create {}", hosted.kind.request_name())
            }
            Op::Sequence(label, _) => label.clone(),
        }
    }

    pub fn refresh(&self) -> Refresh {
        match self {
            Op::Stage(_)
            | Op::StageAll
            | Op::Unstage(_)
            | Op::UnstageAll
            | Op::Discard(_)
            | Op::ApplyPatch { .. }
            | Op::ResolveWith { .. }
            | Op::SaveResolution { .. } => Refresh::Status,
            Op::SetIdentity(_) => Refresh::Status,
            Op::CreatePullRequest { .. } => Refresh::None,
            _ => Refresh::All,
        }
    }

    /// Runs the operation as a job, passing back results the app uses.
    pub fn execute(&self, git: &Git, progress: &dyn Fn(&str)) -> JobResult {
        match self {
            Op::CreatePullRequest { hosted, token, pr } => {
                forge::create_pull_request(hosted, token, pr).map(JobOutput::PullRequestCreated)
            }
            _ => self.run(git, progress).map(|()| JobOutput::Done),
        }
        .map_err(|e| e.to_string())
    }

    pub fn run(&self, git: &Git, progress: &dyn Fn(&str)) -> ferrum_core::Result<()> {
        let p = |l: &str| progress(l);
        match self {
            Op::Stage(paths) => ops::stage(git, paths),
            Op::StageAll => ops::stage_all(git),
            Op::Unstage(paths) => ops::unstage(git, paths),
            Op::UnstageAll => ops::unstage_all(git),
            Op::Discard(files) => ops::discard(git, files),
            Op::ApplyPatch {
                patch,
                target,
                reverse,
                ..
            } => ops::apply_patch(git, patch, *target, *reverse),
            Op::Commit { message, amend } => ops::commit(git, message, *amend),
            Op::Fetch { remote } => ops::fetch(git, remote.as_deref(), p),
            Op::Pull(mode) => ops::pull(git, *mode, p),
            Op::Push(opts) => ops::push(git, opts, p),
            Op::PushTag { remote, tag } => ops::push_tag(git, remote, tag, p),
            Op::DeleteRemoteRef { remote, name } => ops::delete_remote_ref(git, remote, name, p),
            Op::Checkout(b) => ops::checkout(git, b),
            Op::CheckoutRemote {
                remote_branch,
                local,
            } => ops::checkout_remote(git, remote_branch, local),
            Op::CheckoutDetached(r) => ops::checkout_detached(git, r),
            Op::CreateBranch {
                name,
                start,
                checkout,
            } => ops::create_branch(git, name, start, *checkout),
            Op::RenameBranch { old, new } => ops::rename_branch(git, old, new),
            Op::DeleteBranch { name, force } => ops::delete_branch(git, name, *force),
            Op::MoveBranch { name, target } => ops::move_branch(git, name, target),
            Op::SetUpstream { branch, upstream } => ops::set_upstream(git, branch, upstream),
            Op::UnsetUpstream(b) => ops::unset_upstream(git, b),
            Op::Merge { rev, mode } => ops::merge(git, rev, *mode),
            Op::Rebase { onto } => ops::rebase(git, onto),
            Op::InteractiveRebase { base, steps } => ferrum_core::rebase::run(git, *base, steps),
            Op::CherryPick { id, is_merge } => ops::cherry_pick(git, *id, *is_merge),
            Op::Revert { id, is_merge } => ops::revert(git, *id, *is_merge),
            Op::Reset { rev, mode } => ops::reset(git, rev, *mode),
            Op::CreateTag { name, rev, message } => {
                ops::create_tag(git, name, rev, message.as_deref())
            }
            Op::DeleteTag(n) => ops::delete_tag(git, n),
            Op::StashPush {
                message,
                untracked,
                staged_only,
            } => ops::stash_push(git, message, *untracked, *staged_only),
            Op::StashApply { stash, pop } => ops::stash_apply(git, stash, *pop),
            Op::StashDrop(s) => ops::stash_drop(git, s),
            Op::Continue(s) => ops::continue_operation(git, *s),
            Op::Abort(s) => ops::abort_operation(git, *s),
            Op::Skip(s) => ops::skip_operation(git, *s),
            Op::ResolveWith { path, side } => ops::resolve_with(git, path, *side),
            Op::SaveResolution { path, content } => ops::save_resolution(git, path, content),
            Op::AddRemote { name, url } => ops::add_remote(git, name, url),
            Op::RemoveRemote(n) => ops::remove_remote(git, n),
            Op::Lfs(action) => ops::lfs(git, action, p),
            Op::SubmoduleUpdate => ops::submodule_update(git, p),
            Op::WorktreeAdd {
                path,
                rev,
                new_branch,
            } => ops::worktree_add(git, path, rev, new_branch.as_deref()),
            Op::WorktreeRemove { path, force } => ops::worktree_remove(git, path, *force),
            Op::SetIdentity(id) => ops::set_identity(git, id),
            Op::CreatePullRequest { hosted, token, pr } => {
                forge::create_pull_request(hosted, token, pr).map(|_| ())
            }
            Op::Sequence(_, list) => list.iter().try_for_each(|op| op.run(git, progress)),
        }
    }
}

fn files(n: usize) -> String {
    if n == 1 {
        t!("1 file")
    } else {
        t!("{} files", n)
    }
}

fn commits(n: usize) -> String {
    if n == 1 {
        t!("1 commit")
    } else {
        t!("{} commits", n)
    }
}

/// Shortens full hashes; leaves branch names alone.
fn short(rev: &str) -> &str {
    if rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
        &rev[..7]
    } else {
        rev
    }
}
