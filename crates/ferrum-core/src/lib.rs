//! Git logic for Ferrum, independent of the UI.

pub mod graph;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use git2::Oid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Git(#[from] git2::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchKind {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub kind: BranchKind,
    pub is_head: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

/// A branch or tag pointing at a commit, shown as a label in the history.
#[derive(Debug, Clone)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
    pub is_head: bool,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub id: Oid,
    pub summary: String,
    pub author: String,
    pub email: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
    /// Time zone of the commit, in minutes east of UTC.
    pub offset_minutes: i32,
    pub parents: Vec<Oid>,
}

impl CommitInfo {
    pub fn short_id(&self) -> String {
        self.id.to_string()[..7].to_string()
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub struct Repo {
    inner: git2::Repository,
    path: PathBuf,
}

impl Repo {
    /// Opens the repository that contains `path` (searches parent folders).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let inner = git2::Repository::discover(path)?;
        let path = inner
            .workdir()
            .unwrap_or_else(|| inner.path())
            .to_path_buf();
        Ok(Self { inner, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// Short name of the current branch, or `None` when HEAD is detached or unborn.
    pub fn head_branch(&self) -> Option<String> {
        let head = self.inner.head().ok()?;
        head.is_branch().then(|| lossy(head.shorthand_bytes()))
    }

    pub fn branches(&self) -> Result<Vec<BranchInfo>> {
        let mut out = Vec::new();
        for item in self.inner.branches(None)? {
            let (branch, kind) = item?;
            let Some(name) = branch.name()? else { continue };
            // Skip the "origin/HEAD" pointer.
            if kind == git2::BranchType::Remote && name.ends_with("/HEAD") {
                continue;
            }
            let upstream = branch
                .upstream()
                .ok()
                .and_then(|u| u.name().ok().flatten().map(str::to_owned));
            out.push(BranchInfo {
                name: name.to_owned(),
                kind: match kind {
                    git2::BranchType::Local => BranchKind::Local,
                    git2::BranchType::Remote => BranchKind::Remote,
                },
                is_head: branch.is_head(),
                upstream,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn tags(&self) -> Result<Vec<String>> {
        let mut tags: Vec<String> = self
            .inner
            .tag_names(None)?
            .iter()
            .flatten()
            .flatten()
            .map(str::to_owned)
            .collect();
        tags.sort();
        Ok(tags)
    }

    /// Branches and tags grouped by the commit they point at.
    pub fn ref_labels(&self) -> Result<HashMap<Oid, Vec<RefLabel>>> {
        let head = self
            .inner
            .head()
            .ok()
            .and_then(|h| h.name_bytes().to_vec().into());
        let mut map: HashMap<Oid, Vec<RefLabel>> = HashMap::new();
        for r in self.inner.references()? {
            let r = r?;
            let kind = if r.is_branch() {
                RefKind::Local
            } else if r.is_remote() {
                RefKind::Remote
            } else if r.is_tag() {
                RefKind::Tag
            } else {
                continue;
            };
            let name = lossy(r.shorthand_bytes());
            if kind == RefKind::Remote && name.ends_with("/HEAD") {
                continue;
            }
            // Annotated tags point at a tag object; peel to the commit.
            let Ok(commit) = r.peel_to_commit() else {
                continue;
            };
            let is_head = head.as_deref() == Some(r.name_bytes());
            map.entry(commit.id()).or_default().push(RefLabel {
                name,
                kind,
                is_head,
            });
        }
        for labels in map.values_mut() {
            labels.sort_by_key(|l| (!l.is_head, l.kind as u8, l.name.clone()));
        }
        Ok(map)
    }

    /// Commits reachable from all branches, newest first, up to `limit`.
    pub fn commits(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        let mut walk = self.inner.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
        // An empty repository has no HEAD yet.
        let _ = walk.push_head();
        walk.push_glob("refs/heads/*")?;
        walk.push_glob("refs/remotes/*")?;

        let mut out = Vec::new();
        for oid in walk.take(limit) {
            let commit = self.inner.find_commit(oid?)?;
            let author = commit.author();
            out.push(CommitInfo {
                id: commit.id(),
                summary: lossy(commit.summary_bytes().unwrap_or_default()),
                author: lossy(author.name_bytes()),
                email: lossy(author.email_bytes()),
                time: commit.time().seconds(),
                offset_minutes: commit.time().offset_minutes(),
                parents: commit.parent_ids().collect(),
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_own_repository() {
        // The crate lives inside the Ferrum git repository.
        let repo = Repo::open(env!("CARGO_MANIFEST_DIR")).unwrap();
        assert!(repo.path().join("Cargo.toml").exists());
        repo.branches().unwrap();
        repo.commits(10).unwrap();
    }
}
