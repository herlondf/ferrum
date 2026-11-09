//! Cleanup and sparse checkout operations (run through `Op::Cleanup`).
//! Removing paths from history saves backup refs (and optionally a bundle)
//! before git filter-repo runs; see `ferrum_core::cleanup`.

use std::path::PathBuf;

use ferrum_core::cleanup::{self, Merged};
use ferrum_core::cli::Git;
use ferrum_core::sparse;

use crate::jobs::{JobOutput, JobResult, Refresh};

#[derive(Debug, Clone)]
pub enum CleanupOp {
    /// Rewrite every local branch and tag without `paths`.
    RemovePaths {
        paths: Vec<String>,
        bundle: Option<PathBuf>,
    },
    /// Delete merged branches (local and remote).
    PruneBranches(Vec<Merged>),
    Gc {
        aggressive: bool,
    },
    /// Cone-mode sparse checkout of these folders.
    SparseSet(Vec<String>),
    SparseDisable,
}

impl CleanupOp {
    pub fn label(&self) -> String {
        match self {
            CleanupOp::RemovePaths { paths, .. } if paths.len() == 1 => {
                t!("Remove {} from history", paths[0])
            }
            CleanupOp::RemovePaths { paths, .. } => {
                t!("Remove {} paths from history", paths.len())
            }
            CleanupOp::PruneBranches(list) => t!("Delete {} merged branches", list.len()),
            CleanupOp::Gc { aggressive: true } => t!("Compact the repository (aggressive)"),
            CleanupOp::Gc { .. } => t!("Compact the repository"),
            CleanupOp::SparseSet(dirs) => t!("Sparse checkout of {} folders", dirs.len()),
            CleanupOp::SparseDisable => t!("Check out every folder"),
        }
    }

    pub fn refresh(&self) -> Refresh {
        match self {
            CleanupOp::Gc { .. } => Refresh::Status,
            _ => Refresh::All,
        }
    }

    /// Drops or rewrites work: the job takes a restore point first.
    pub fn rewrites(&self) -> bool {
        matches!(
            self,
            CleanupOp::RemovePaths { .. } | CleanupOp::PruneBranches(_)
        )
    }

    pub fn execute(&self, git: &Git, progress: &dyn Fn(&str)) -> JobResult {
        let e = |e: ferrum_core::Error| e.to_string();
        match self {
            CleanupOp::RemovePaths { paths, bundle } => {
                let done =
                    cleanup::remove_paths(git, paths, bundle.as_deref(), progress).map_err(e)?;
                let mut text = t!(
                    "History rewritten. Backup: refs/ferrum/backup/{}. Force push the branches to publish it.",
                    done.backup
                );
                if let Some(b) = done.bundle {
                    text.push(' ');
                    text.push_str(&t!("Bundle: {}", b.display()));
                }
                Ok(JobOutput::Message(text))
            }
            CleanupOp::Gc { aggressive } => {
                let (before, after) = cleanup::gc(git, *aggressive, progress).map_err(e)?;
                let saved = before.saturating_sub(after);
                Ok(JobOutput::Message(t!(
                    "Repository compacted: {} → {} ({} freed)",
                    size(before),
                    size(after),
                    size(saved)
                )))
            }
            _ => self.run(git, progress).map(|()| JobOutput::Done).map_err(e),
        }
    }

    pub fn run(&self, git: &Git, progress: &dyn Fn(&str)) -> ferrum_core::Result<()> {
        match self {
            CleanupOp::RemovePaths { paths, bundle } => {
                cleanup::remove_paths(git, paths, bundle.as_deref(), progress).map(drop)
            }
            CleanupOp::PruneBranches(list) => cleanup::delete_branches(git, list, progress),
            CleanupOp::Gc { aggressive } => cleanup::gc(git, *aggressive, progress).map(drop),
            CleanupOp::SparseSet(dirs) => sparse::set(git, dirs),
            CleanupOp::SparseDisable => sparse::disable(git),
        }
    }
}

/// Bytes as "12.3 MB".
pub fn size(bytes: u64) -> String {
    let b = bytes as f64;
    const K: f64 = 1024.0;
    if b >= K * K * K {
        format!("{:.1} GB", b / (K * K * K))
    } else if b >= K * K {
        format!("{:.1} MB", b / (K * K))
    } else if b >= K {
        format!("{:.0} KB", b / K)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_labels() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(2048), "2 KB");
        assert_eq!(size(5 * 1024 * 1024), "5.0 MB");
        let op = CleanupOp::RemovePaths {
            paths: vec!["a.bin".into()],
            bundle: None,
        };
        assert!(op.rewrites());
        assert!(!CleanupOp::Gc { aggressive: false }.rewrites());
        assert_eq!(op.refresh(), Refresh::All);
    }
}
