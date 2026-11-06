//! Git operations of the branch, tag and stash dialogs (`Op::Refs`).

use std::path::PathBuf;

use ferrum_core::StashInfo;
use ferrum_core::cli::Git;
use ferrum_core::refs;

#[derive(Debug, Clone)]
pub enum RefOp {
    /// Rename a branch, and with `remote` (remote, branch there) on the remote too.
    RenameBranch {
        old: String,
        new: String,
        remote: Option<(String, String)>,
    },
    CreateTag {
        name: String,
        rev: String,
        message: String,
        sign: bool,
    },
    /// Stash only some files.
    StashPaths {
        message: String,
        untracked: bool,
        paths: Vec<String>,
    },
    /// Stash without cleaning the working copy.
    StashSnapshot(String),
    StashRename {
        stash: StashInfo,
        message: String,
    },
    StashApply {
        stash: StashInfo,
        pop: bool,
        index: bool,
    },
    StashSavePatch {
        stash: StashInfo,
        path: PathBuf,
    },
    StashDrop(Vec<StashInfo>),
}

impl RefOp {
    pub fn label(&self) -> String {
        match self {
            RefOp::RenameBranch { old, new, .. } => t!("Rename {} to {}", old, new),
            RefOp::CreateTag { name, .. } => t!("Create tag {}", name),
            RefOp::StashPaths { paths, .. } => {
                let n = if paths.len() == 1 {
                    t!("1 file")
                } else {
                    t!("{} files", paths.len())
                };
                t!("Stash {}", n)
            }
            RefOp::StashSnapshot(_) => t!("Save a snapshot"),
            RefOp::StashRename { stash, .. } => t!("Rename {}", stash.refname()),
            RefOp::StashApply {
                stash, pop: true, ..
            } => t!("Pop {}", stash.refname()),
            RefOp::StashApply { stash, .. } => t!("Apply {}", stash.refname()),
            RefOp::StashSavePatch { stash, .. } => t!("Save {} as a patch", stash.refname()),
            RefOp::StashDrop(list) => t!("Drop {} stashes", list.len()),
        }
    }

    pub fn run(&self, git: &Git, progress: &dyn Fn(&str)) -> ferrum_core::Result<()> {
        match self {
            RefOp::RenameBranch { old, new, remote } => {
                let remote = remote.as_ref().map(|(r, b)| (r.as_str(), b.as_str()));
                refs::rename_branch(git, old, new, remote, |l| progress(l))
            }
            RefOp::CreateTag {
                name,
                rev,
                message,
                sign,
            } => refs::create_tag(git, name, rev, message, *sign),
            RefOp::StashPaths {
                message,
                untracked,
                paths,
            } => refs::stash_paths(git, message, *untracked, paths),
            RefOp::StashSnapshot(message) => {
                if refs::stash_snapshot(git, message)? {
                    Ok(())
                } else {
                    Err(ferrum_core::Error::Other(t!(
                        "There are no changes to save."
                    )))
                }
            }
            RefOp::StashRename { stash, message } => refs::stash_rename(git, stash, message),
            RefOp::StashApply { stash, pop, index } => refs::stash_apply(git, stash, *pop, *index),
            RefOp::StashSavePatch { stash, path } => refs::save_stash_patch(git, stash, path),
            RefOp::StashDrop(list) => refs::stash_drop_many(git, list),
        }
    }
}
