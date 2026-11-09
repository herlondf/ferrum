//! Operations on stacked branches, absorb and moving hunks between commits
//! (run through `Op::Stack`). Every one that rewrites history first saves a
//! backup ref (`ferrum_core::backup`), which its undo entry restores.

use ferrum_core::absorb;
use ferrum_core::backup;
use ferrum_core::cli::Git;
use ferrum_core::forge::chain::{self, ChainEntry};
use ferrum_core::forge::{Credentials, HostedRepo};
use ferrum_core::hunk_move::{self, Dest, Part};
use ferrum_core::ops::ResetMode;
use ferrum_core::stack::{self, Insert, Stack};
use ferrum_core::{Oid, short};

use crate::command::Op;
use crate::jobs::{JobOutput, JobResult, Refresh};
use crate::tab::{RepoTab, UndoEntry};

#[derive(Debug, Clone)]
pub enum StackOp {
    /// Rebase the branches of a stack onto their parents.
    Restack {
        stack: Stack,
        backup: String,
        return_to: Option<String>,
        update_refs: bool,
    },
    /// A new branch on top of a stack branch, or below one.
    AddBranch {
        name: String,
        at: Insert,
        checkout: bool,
    },
    /// Chained pull/merge requests.
    CreateRequests {
        hosted: HostedRepo,
        token: Credentials,
        entries: Vec<ChainEntry>,
    },
    /// Fixup commits for the local changes (then autosquash).
    Absorb {
        plan: Box<absorb::Plan>,
        opts: absorb::Options,
        backup: String,
        /// Branches the backup saves.
        branches: Vec<String>,
    },
    /// Selected changes of a commit into another one (or a new one).
    Move {
        source: Oid,
        parts: Vec<Part>,
        dest: Dest,
        update_refs: bool,
        backup: String,
        branches: Vec<String>,
        /// "3 lines", "a.txt": for messages.
        what: String,
        /// The destination, for messages.
        to: String,
    },
    /// Put the branches of a backup back (undo).
    Restore { backup: String, mode: ResetMode },
}

impl StackOp {
    pub fn label(&self) -> String {
        match self {
            StackOp::Restack { stack, .. } => t!("Restack {}", stack.top().name),
            StackOp::AddBranch { name, .. } => t!("Create branch {}", name),
            StackOp::CreateRequests {
                hosted, entries, ..
            } => t!(
                "Create {} {}s for the stack",
                entries.len(),
                hosted.kind.request_name()
            ),
            StackOp::Absorb { .. } => t!("Absorb local changes"),
            StackOp::Move { what, to, .. } => t!("Move {} to {}", what, to),
            StackOp::Restore { backup, .. } => t!("Restore {}", backup),
        }
    }

    pub fn refresh(&self) -> Refresh {
        match self {
            StackOp::CreateRequests { .. } => Refresh::None,
            _ => Refresh::All,
        }
    }

    /// Rewrites history: the toast offers Undo.
    pub fn rewrites(&self) -> bool {
        matches!(
            self,
            StackOp::Restack { .. } | StackOp::Absorb { .. } | StackOp::Move { .. }
        )
    }

    pub fn execute(&self, git: &Git, progress: &dyn Fn(&str)) -> JobResult {
        match self {
            StackOp::CreateRequests {
                hosted,
                token,
                entries,
            } => {
                let list =
                    chain::create_chain(hosted, token, entries).map_err(|e| e.to_string())?;
                let made: Vec<String> = list
                    .iter()
                    .filter(|r| r.created)
                    .map(|r| format!("#{}", r.pr.number))
                    .collect();
                let noun = hosted.kind.request_name();
                Ok(JobOutput::PullRequestsCreated(if made.is_empty() {
                    t!("Every branch of the stack already has a {}.", noun)
                } else {
                    t!("Created {} {}s: {}", made.len(), noun, made.join(", "))
                }))
            }
            _ => self
                .run(git, progress)
                .map(|()| JobOutput::Done)
                .map_err(|e| e.to_string()),
        }
    }

    pub fn run(&self, git: &Git, _progress: &dyn Fn(&str)) -> ferrum_core::Result<()> {
        match self {
            StackOp::Restack {
                stack,
                backup: id,
                return_to,
                update_refs,
            } => {
                backup::create(git, id, &stack.names())?;
                let opts = stack::RestackOptions {
                    update_refs: *update_refs,
                    return_to: return_to.clone(),
                };
                stack::restack(git, stack, &opts)
            }
            StackOp::AddBranch { name, at, checkout } => {
                stack::add_branch(git, name, at, *checkout)
            }
            StackOp::CreateRequests {
                hosted,
                token,
                entries,
            } => chain::create_chain(hosted, token, entries).map(drop),
            StackOp::Absorb {
                plan,
                opts,
                backup: id,
                branches,
            } => {
                backup::create(git, id, branches)?;
                absorb::run(git, plan, opts)
            }
            StackOp::Move {
                source,
                parts,
                dest,
                update_refs,
                backup: id,
                branches,
                ..
            } => {
                backup::create(git, id, branches)?;
                let opts = hunk_move::Options {
                    update_refs: *update_refs,
                };
                hunk_move::run(git, *source, parts, dest, &opts)
            }
            StackOp::Restore { backup: id, mode } => backup::restore(git, id, *mode),
        }
    }

    /// How to undo it: restore the backup it saves.
    pub fn undo(&self, _tab: &RepoTab) -> Option<UndoEntry> {
        let (id, mode) = match self {
            StackOp::Restack { backup, .. } | StackOp::Move { backup, .. } => {
                (backup, ResetMode::Keep)
            }
            // The absorbed changes go back to the working copy.
            StackOp::Absorb { backup, .. } => (backup, ResetMode::Mixed),
            _ => return None,
        };
        Some(UndoEntry {
            label: self.label(),
            ops: vec![Op::Stack(StackOp::Restore {
                backup: id.clone(),
                mode,
            })],
            // Set when the job finishes (see `RepoTab`).
            seq: 0,
        })
    }

    /// What it did, for the toast (`None`: "… — done").
    pub fn describe(&self) -> Option<String> {
        Some(match self {
            StackOp::Restack { stack, .. } => {
                let n = stack::restack_preview(stack).len();
                t!("Restacked {} on {}", branches(n), stack.trunk)
            }
            StackOp::Absorb { plan, opts, .. } => {
                let n = plan.absorbed();
                let c = plan.targets().len();
                if opts.autosquash {
                    t!("Absorbed {} into {}", hunks(n), commits(c))
                } else {
                    t!("Created {} fixup commits for {}", c, hunks(n))
                }
            }
            StackOp::Move { what, to, .. } => t!("Moved {} to {}", what, to),
            _ => return None,
        })
    }
}

/// A fresh backup id, checked against the existing backups.
pub fn backup_id(tab: &RepoTab, op: &str) -> String {
    let base = backup::new_id(op);
    let taken = ferrum_core::backup::list(&tab.repo).unwrap_or_default();
    let mut id = base.clone();
    let mut n = 1;
    while taken.iter().any(|b| b.id == id) {
        n += 1;
        id = format!("{base}.{n}");
    }
    id
}

/// The current branch and the other local branches pointing at `commits`
/// (those a rebase with `--update-refs` may move): what a backup saves.
pub fn branches_at(tab: &RepoTab, commits: &[Oid]) -> Vec<String> {
    let mut out: Vec<String> = tab.head.iter().cloned().collect();
    for b in &tab.branches {
        if b.kind == ferrum_core::BranchKind::Local
            && b.target.is_some_and(|t| commits.contains(&t))
            && !out.contains(&b.name)
        {
            out.push(b.name.clone());
        }
    }
    out
}

/// "short summary" of a commit for messages.
pub fn commit_title(tab: &RepoTab, id: Oid) -> String {
    tab.repo
        .find_commit_info(id)
        .map(|c| {
            let mut s: String = c.summary.chars().take(40).collect();
            if c.summary.chars().count() > 40 {
                s.push('…');
            }
            format!("{} “{}”", short(id), s)
        })
        .unwrap_or_else(|_| short(id))
}

pub fn branches(n: usize) -> String {
    if n == 1 {
        t!("1 branch")
    } else {
        t!("{} branches", n)
    }
}

pub fn hunks(n: usize) -> String {
    if n == 1 {
        t!("1 change")
    } else {
        t!("{} changes", n)
    }
}

pub fn commits(n: usize) -> String {
    if n == 1 {
        t!("1 commit")
    } else {
        t!("{} commits", n)
    }
}
