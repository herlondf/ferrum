//! Git logic for Ferrum, independent of the UI.
//!
//! Reads (history, diffs, status) go through libgit2 for speed. Writes go
//! through the `git` command line, so hooks, credential helpers, SSH config
//! and LFS behave exactly as in the terminal.

pub mod absorb;
pub mod askpass;
pub mod avatar;
pub mod backup;
pub mod bisect;
pub mod browse;
pub mod cleanup;
pub mod cli;
pub mod codeowners;
pub mod config;
pub mod conflict;
pub mod diff;
mod file_log;
pub mod forge;
pub mod gitflow;
pub mod gitignore;
pub mod glance;
pub mod graph;
pub mod hunk_move;
pub mod insights;
pub mod integrate;
pub mod lfs;
pub mod library;
pub mod local;
pub mod log;
pub mod message;
pub mod model;
pub mod ops;
pub mod patch;
pub mod rebase;
pub mod refs;
mod repo;
pub mod restore;
pub mod secrets;
pub mod semantic_merge;
pub mod shell;
pub mod sparse;
pub mod ssh;
pub mod stack;
pub mod stats;
pub mod structural;
pub mod submodule;
pub mod sync;
pub mod syntax;
pub mod update;

pub use git2::Oid;
pub use model::*;
pub use repo::Repo;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error("{0}")]
    Cli(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
    #[error("The operation was cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;

/// The local time zone's offset from UTC right now, in minutes.
pub fn local_offset_minutes() -> i32 {
    git2::Signature::now("ferrum", "ferrum")
        .map(|s| s.when().offset_minutes())
        .unwrap_or(0)
}

pub(crate) fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
pub(crate) mod testutil {
    use std::path::Path;

    use crate::cli::Git;

    /// A throwaway repository with an identity configured.
    pub struct TestRepo {
        pub dir: tempfile::TempDir,
        pub git: Git,
    }

    impl TestRepo {
        pub fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let git = Git::new(dir.path());
            git.run(&["init", "-q", "-b", "main"]).unwrap();
            git.run(&["config", "user.name", "Test"]).unwrap();
            git.run(&["config", "user.email", "test@example.com"])
                .unwrap();
            git.run(&["config", "commit.gpgsign", "false"]).unwrap();
            git.run(&["config", "core.autocrlf", "false"]).unwrap();
            Self { dir, git }
        }

        pub fn path(&self) -> &Path {
            self.dir.path()
        }

        pub fn write(&self, file: &str, content: &str) {
            let p = self.path().join(file);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, content).unwrap();
        }

        pub fn read(&self, file: &str) -> String {
            std::fs::read_to_string(self.path().join(file)).unwrap()
        }

        /// Writes `file`, stages everything and commits.
        pub fn commit(&self, file: &str, content: &str, msg: &str) {
            self.write(file, content);
            self.git.run(&["add", "-A"]).unwrap();
            self.git.run(&["commit", "-q", "-m", msg]).unwrap();
        }

        pub fn repo(&self) -> crate::Repo {
            crate::Repo::open(self.path()).unwrap()
        }
    }
}
