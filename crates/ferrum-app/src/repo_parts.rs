//! Git LFS and submodule operations (run as jobs through `Op::Part`), and
//! the automatic submodule update after checkouts.

use ferrum_core::cli::Git;
use ferrum_core::lfs::{self, Unlock};
use ferrum_core::submodule::{self, UpdateOptions};
use serde::{Deserialize, Serialize};

use crate::command::Op;
use crate::jobs::{JobOutput, JobResult, Refresh};
use crate::tab::RepoTab;

#[derive(Debug, Clone)]
pub enum PartOp {
    LfsInstall,
    LfsTrack(String),
    LfsUntrack(String),
    LfsLock(Vec<String>),
    LfsUnlock {
        which: Vec<Unlock>,
        force: bool,
    },
    SubmoduleAdd {
        url: String,
        path: String,
        branch: Option<String>,
    },
    SubmoduleRemove {
        name: String,
        path: String,
    },
    SubmoduleMove {
        from: String,
        to: String,
    },
    SubmoduleSetUrl {
        name: String,
        path: String,
        url: String,
    },
    /// Update the submodules at `paths` (all when empty).
    SubmoduleUpdate {
        paths: Vec<String>,
        opts: UpdateOptions,
    },
    SubmoduleDiscard(String),
    /// Looks for submodules that need an update, to ask about it.
    SubmoduleCheck,
}

impl PartOp {
    pub fn label(&self) -> String {
        match self {
            PartOp::LfsInstall => t!("Install Git LFS"),
            PartOp::LfsTrack(p) => t!("Track {} with Git LFS", p),
            PartOp::LfsUntrack(p) => t!("Stop tracking {} with Git LFS", p),
            PartOp::LfsLock(p) => t!("Lock {}", names(p)),
            PartOp::LfsUnlock { which, .. } => t!("Unlock {}", unlock_names(which)),
            PartOp::SubmoduleAdd { path, .. } => t!("Add submodule {}", path),
            PartOp::SubmoduleRemove { path, .. } => t!("Remove submodule {}", path),
            PartOp::SubmoduleMove { from, to } => t!("Move submodule {} to {}", from, to),
            PartOp::SubmoduleSetUrl { path, .. } => t!("Change URL of {}", path),
            PartOp::SubmoduleUpdate { paths, .. } if paths.is_empty() => {
                t!("Update submodules")
            }
            PartOp::SubmoduleUpdate { paths, .. } => t!("Update submodule {}", names(paths)),
            PartOp::SubmoduleDiscard(p) => t!("Discard changes in {}", p),
            PartOp::SubmoduleCheck => t!("Check submodules"),
        }
    }

    pub fn refresh(&self) -> Refresh {
        match self {
            PartOp::LfsLock(_) | PartOp::LfsUnlock { .. } | PartOp::SubmoduleCheck => Refresh::None,
            _ => Refresh::All,
        }
    }

    pub fn execute(&self, git: &Git, progress: &dyn Fn(&str)) -> JobResult {
        match self {
            PartOp::SubmoduleCheck => submodule::out_of_date(git)
                .map(JobOutput::SubmodulesOutOfDate)
                .map_err(|e| e.to_string()),
            _ => self
                .run(git, progress)
                .map(|()| JobOutput::Done)
                .map_err(|e| e.to_string()),
        }
    }

    pub fn run(&self, git: &Git, progress: &dyn Fn(&str)) -> ferrum_core::Result<()> {
        let p = |l: &str| progress(l);
        match self {
            PartOp::LfsInstall => lfs::install(git, false),
            PartOp::LfsTrack(pattern) => lfs::track(git, pattern),
            PartOp::LfsUntrack(pattern) => lfs::untrack(git, pattern),
            PartOp::LfsLock(paths) => lfs::lock(git, paths),
            PartOp::LfsUnlock { which, force } => lfs::unlock(git, which, *force),
            PartOp::SubmoduleAdd { url, path, branch } => {
                submodule::add(git, url, path, branch.as_deref(), p)
            }
            PartOp::SubmoduleRemove { name, path } => submodule::remove(git, name, path),
            PartOp::SubmoduleMove { from, to } => submodule::move_to(git, from, to),
            PartOp::SubmoduleSetUrl { name, path, url } => submodule::set_url(git, name, path, url),
            PartOp::SubmoduleUpdate { paths, opts } => submodule::update(git, paths, *opts, p),
            PartOp::SubmoduleDiscard(path) => submodule::discard(git, path),
            PartOp::SubmoduleCheck => submodule::out_of_date(git).map(drop),
        }
    }
}

fn names(paths: &[String]) -> String {
    match paths {
        [one] => one.clone(),
        _ => t!("{} files", paths.len()),
    }
}

fn unlock_names(which: &[Unlock]) -> String {
    match which {
        [Unlock::Path(p)] => p.clone(),
        [Unlock::Id(id)] => format!("#{id}"),
        _ => t!("{} locks", which.len()),
    }
}

/// Whether to update submodules after a checkout, cherry-pick or revert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AutoUpdate {
    #[default]
    Off,
    Ask,
    Always,
}

impl AutoUpdate {
    pub const ALL: [AutoUpdate; 3] = [AutoUpdate::Off, AutoUpdate::Ask, AutoUpdate::Always];

    pub fn label(self) -> String {
        match self {
            AutoUpdate::Off => t!("Never"),
            AutoUpdate::Ask => t!("Ask"),
            AutoUpdate::Always => t!("Always"),
        }
    }
}

/// Submodule settings (`Settings::submodules`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubmodulePrefs {
    pub auto_update: AutoUpdate,
}

/// The operation to queue after `op` so submodules follow the new commit.
pub fn after(op: &Op, tab: &RepoTab, prefs: &SubmodulePrefs) -> Option<Op> {
    use crate::flow::FlowOp;
    fn moves_head(op: &Op) -> bool {
        match op {
            Op::Checkout(_) | Op::CheckoutRemote { .. } | Op::CheckoutDetached(_) => true,
            Op::Flow(FlowOp::CherryPick { .. } | FlowOp::Revert { .. }) => true,
            // A checkout after stashing or discarding local changes.
            Op::Flow(FlowOp::WithLocalChanges { op, .. }) => moves_head(op),
            _ => false,
        }
    }
    let moves_head = moves_head(op);
    if !moves_head || tab.submodules.is_empty() {
        return None;
    }
    match prefs.auto_update {
        AutoUpdate::Off => None,
        AutoUpdate::Ask => Some(Op::Part(PartOp::SubmoduleCheck)),
        AutoUpdate::Always => Some(update_all()),
    }
}

pub fn update_all() -> Op {
    Op::Part(PartOp::SubmoduleUpdate {
        paths: Vec::new(),
        opts: UpdateOptions::default(),
    })
}
