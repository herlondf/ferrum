//! Git logic for Ferrum, independent of the UI.
//!
//! Reads (history, diffs, status) go through libgit2 for speed. Writes go
//! through the `git` command line, so hooks, credential helpers, SSH config
//! and LFS behave exactly as in the terminal.

pub mod cli;
pub mod conflict;
pub mod diff;
pub mod forge;
pub mod graph;
pub mod model;
pub mod ops;
pub mod rebase;
mod repo;

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
}

pub type Result<T> = std::result::Result<T, Error>;

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
