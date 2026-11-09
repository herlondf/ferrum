//! Quick looks at repositories: what the start screen cards show, and the
//! commits a dragged branch would bring along.

use std::path::Path;

use git2::{Oid, StatusOptions};

use crate::{Repo, Result, lossy};

/// What a Repository Manager card shows about a repository.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Glance {
    /// The current branch (`None`: detached HEAD or not born yet).
    pub branch: Option<String>,
    /// Time of the HEAD commit (unix seconds).
    pub last_commit: Option<i64>,
    /// Summary of the HEAD commit.
    pub summary: String,
    /// The working copy has changes (untracked files included).
    pub dirty: bool,
}

/// Reads branch, last commit and whether there are local changes. Fast for
/// usual repositories, but call it off the UI thread.
pub fn glance(path: &Path) -> Result<Glance> {
    let repo = git2::Repository::open(path)?;
    let mut g = Glance::default();
    if let Ok(head) = repo.head() {
        if head.is_branch() {
            g.branch = Some(lossy(head.shorthand_bytes()));
        }
        if let Ok(c) = head.peel_to_commit() {
            g.last_commit = Some(c.time().seconds());
            g.summary = lossy(c.summary_bytes().unwrap_or_default());
        }
    } else if let Ok(r) = repo.find_reference("HEAD")
        && let Some(target) = r.symbolic_target_bytes()
    {
        // An unborn branch: HEAD names it, no commit yet.
        let name = lossy(target);
        g.branch = Some(name.trim_start_matches("refs/heads/").to_owned());
    }
    if !repo.is_bare() {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(false)
            .exclude_submodules(true);
        g.dirty = repo.statuses(Some(&mut opts)).is_ok_and(|s| !s.is_empty());
    }
    Ok(g)
}

/// Commits reachable from `from` but not from `onto`, oldest first, at most
/// `limit` (what cherry-picking `from` onto `onto` would copy).
pub fn commits_not_in(repo: &Repo, from: &str, onto: &str, limit: usize) -> Result<Vec<Oid>> {
    let raw = repo.raw();
    let from = raw.revparse_single(from)?.peel_to_commit()?.id();
    let onto = raw.revparse_single(onto)?.peel_to_commit()?.id();
    let mut walk = raw.revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(from)?;
    walk.hide(onto)?;
    let mut out = Vec::new();
    for id in walk {
        out.push(id?);
        if out.len() > limit {
            return Err(crate::Error::Cli(format!("more than {limit} commits")));
        }
    }
    out.reverse();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn glance_reads_branch_time_and_changes() {
        let t = TestRepo::new();
        let empty = glance(t.path()).unwrap();
        assert_eq!(empty.branch.as_deref(), Some("main"));
        assert_eq!(empty.last_commit, None);
        t.commit("a.txt", "a", "First commit");
        let g = glance(t.path()).unwrap();
        assert_eq!(g.branch.as_deref(), Some("main"));
        assert!(g.last_commit.is_some());
        assert_eq!(g.summary, "First commit");
        assert!(!g.dirty);
        t.write("b.txt", "new");
        assert!(glance(t.path()).unwrap().dirty);
        t.git.run(&["checkout", "-q", "--detach"]).unwrap();
        assert_eq!(glance(t.path()).unwrap().branch, None);
    }

    #[test]
    fn commits_not_in_lists_the_branch_commits() {
        let t = TestRepo::new();
        t.commit("a.txt", "a", "base");
        t.git.run(&["checkout", "-q", "-b", "topic"]).unwrap();
        t.commit("b.txt", "b", "one");
        t.commit("c.txt", "c", "two");
        t.git.run(&["checkout", "-q", "main"]).unwrap();
        t.commit("d.txt", "d", "main work");
        let repo = t.repo();
        let ids = commits_not_in(&repo, "topic", "main", 10).unwrap();
        let summaries: Vec<String> = ids
            .iter()
            .map(|id| repo.find_commit_info(*id).unwrap().summary)
            .collect();
        assert_eq!(summaries, ["one", "two"]);
        assert!(commits_not_in(&repo, "main", "topic", 10).unwrap().len() == 1);
        assert!(commits_not_in(&repo, "topic", "main", 1).is_err());
    }
}
