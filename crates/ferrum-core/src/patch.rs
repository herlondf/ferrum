//! Patches: writing commits, the working copy or a stash as a patch, and
//! checking and applying patch files (`git apply` or `git am`).

use git2::Oid;

use crate::cli::Git;
use crate::{Error, Result, StashInfo};

#[derive(Debug, Clone)]
pub enum Source {
    /// Commits, oldest first, each as an email (`git format-patch`).
    Commits(Vec<Oid>),
    /// The commits after `from` up to `to`.
    Range {
        from: Oid,
        to: Oid,
    },
    /// All changes against HEAD, staged or not.
    WorkingCopy,
    Stash(StashInfo),
}

/// The text of a patch.
pub fn create(git: &Git, source: &Source) -> Result<String> {
    let out = match source {
        Source::Commits(ids) => {
            let mut out = String::new();
            for id in ids {
                out.push_str(&git.run(&[
                    "format-patch",
                    "--stdout",
                    "--binary",
                    "-1",
                    &id.to_string(),
                ])?);
            }
            out
        }
        Source::Range { from, to } => {
            let range = format!("{from}..{to}");
            git.run(&["format-patch", "--stdout", "--binary", &range])?
        }
        Source::WorkingCopy => git.run(&["diff", "--binary", "HEAD"])?,
        Source::Stash(s) => crate::refs::stash_patch(git, s)?,
    };
    if out.trim().is_empty() {
        return Err(Error::Other(
            "There are no changes to put in a patch.".into(),
        ));
    }
    Ok(out)
}

/// Text that looks like a diff (for paste and drop).
pub fn looks_like_patch(text: &str) -> bool {
    text.contains("diff --git ")
        || (text.contains("\n--- ") || text.starts_with("--- "))
            && text.contains("\n+++ ")
            && text.contains("\n@@ ")
}

/// An email patch (from `git format-patch`), which `git am` can commit.
pub fn is_mailbox(text: &str) -> bool {
    let head = text.split("\ndiff --git ").next().unwrap_or(text);
    let from = text.starts_with("From ") || head.lines().any(|l| l.starts_with("From: "));
    from && head.lines().any(|l| l.starts_with("Subject: "))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub path: String,
    /// `None` for binary files.
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

/// The commit subjects of an email patch, without "[PATCH n/m]".
pub fn subjects(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("Subject: "))
        .map(|s| {
            let s = s.trim();
            match s.strip_prefix('[').and_then(|r| r.split_once("] ")) {
                Some((tag, rest)) if tag.contains("PATCH") => rest.to_owned(),
                _ => s.to_owned(),
            }
        })
        .collect()
}

/// Files a patch touches (`git apply --numstat`).
pub fn files(git: &Git, text: &str) -> Result<Vec<FileStat>> {
    let out = git.run_with_input(&["apply", "--numstat", "-"], Some(text.as_bytes()))?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            let (a, r, p) = (it.next()?, it.next()?, it.next()?);
            Some(FileStat {
                path: p.to_owned(),
                added: a.parse().ok(),
                removed: r.parse().ok(),
            })
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applies {
    Clean,
    /// Its changes are already in the working copy.
    AlreadyApplied,
    /// Only with a 3-way merge, which may leave conflicts.
    ThreeWay,
    No(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub files: Vec<FileStat>,
    pub subjects: Vec<String>,
    pub mailbox: bool,
    pub applies: Applies,
}

/// Whether the patch would apply to the working copy, without changing it.
pub fn check(git: &Git, text: &str) -> Check {
    let input = Some(text.as_bytes());
    let applies = match git.run_with_input(&["apply", "--check", "-"], input) {
        Ok(_) => Applies::Clean,
        Err(_)
            if git
                .run_with_input(&["apply", "--check", "-R", "-"], input)
                .is_ok() =>
        {
            Applies::AlreadyApplied
        }
        Err(e) => match git.run_with_input(&["apply", "--check", "--3way", "-"], input) {
            Ok(_) => Applies::ThreeWay,
            Err(_) => Applies::No(e.to_string()),
        },
    };
    Check {
        files: files(git, text).unwrap_or_default(),
        subjects: subjects(text),
        mailbox: is_mailbox(text),
        applies,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Commit each patch (`git am`).
    Commit,
    /// Change the files only (`git apply`).
    WorkingCopy,
    /// Change the files and stage them (`git apply --index`).
    Index,
}

pub fn apply(git: &Git, text: &str, mode: Mode, three_way: bool) -> Result<()> {
    let mut args = match mode {
        Mode::Commit => vec!["am"],
        Mode::WorkingCopy => vec!["apply"],
        Mode::Index => vec!["apply", "--index"],
    };
    if three_way {
        args.push("--3way");
    }
    if mode != Mode::Commit {
        args.push("-");
    }
    git.run_with_input(&args, Some(text.as_bytes()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    fn two_commits() -> (TestRepo, Vec<Oid>) {
        let t = TestRepo::new();
        t.commit("a.txt", "one\n", "first");
        let base = t.repo().head_oid().unwrap();
        t.commit("a.txt", "one\ntwo\n", "second");
        let second = t.repo().head_oid().unwrap();
        t.commit("b.txt", "b\n", "third");
        let third = t.repo().head_oid().unwrap();
        (t, vec![base, second, third])
    }

    #[test]
    fn commits_round_trip_through_am() {
        let (t, ids) = two_commits();
        let text = create(&t.git, &Source::Commits(vec![ids[1], ids[2]])).unwrap();
        assert!(is_mailbox(&text));
        assert!(looks_like_patch(&text));
        assert_eq!(subjects(&text), vec!["second", "third"]);
        let range = create(
            &t.git,
            &Source::Range {
                from: ids[0],
                to: ids[2],
            },
        )
        .unwrap();
        assert_eq!(subjects(&range), vec!["second", "third"]);

        t.git
            .run(&["reset", "-q", "--hard", &ids[0].to_string()])
            .unwrap();
        let c = check(&t.git, &text);
        assert_eq!(c.applies, Applies::Clean);
        let paths: Vec<&str> = c.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["a.txt", "b.txt"]);
        assert_eq!(c.files[0].added, Some(1));
        apply(&t.git, &text, Mode::Commit, true).unwrap();
        let log = t.git.run(&["log", "--format=%s"]).unwrap();
        assert_eq!(log, "third\nsecond\nfirst\n");
    }

    #[test]
    fn working_copy_patch_applies_without_commit() {
        let (t, _) = two_commits();
        t.write("a.txt", "changed\n");
        let text = create(&t.git, &Source::WorkingCopy).unwrap();
        assert!(!is_mailbox(&text));
        t.git.run(&["checkout", "--", "a.txt"]).unwrap();
        apply(&t.git, &text, Mode::Index, false).unwrap();
        assert_eq!(t.read("a.txt"), "changed\n");
        let staged = t.git.run(&["diff", "--cached", "--name-only"]).unwrap();
        assert_eq!(staged.trim(), "a.txt");
        // Applying again no longer fits.
        assert_eq!(check(&t.git, &text).applies, Applies::AlreadyApplied);
    }

    #[test]
    fn conflicting_patch_needs_three_way() {
        let (t, ids) = two_commits();
        let text = create(&t.git, &Source::Commits(vec![ids[1]])).unwrap();
        t.git
            .run(&["reset", "-q", "--hard", &ids[0].to_string()])
            .unwrap();
        t.commit("a.txt", "one\nother\n", "other");
        assert!(matches!(
            check(&t.git, &text).applies,
            Applies::ThreeWay | Applies::No(_)
        ));
        assert!(apply(&t.git, &text, Mode::Commit, true).is_err());
        assert!(t.repo().git_dir().join("rebase-apply").exists());
        assert_eq!(t.repo().state(), crate::RepoState::Am);
        crate::ops::abort_operation(&t.git, crate::RepoState::Am).unwrap();
        assert_eq!(t.repo().state(), crate::RepoState::Clean);
    }

    #[test]
    fn recognizes_patches() {
        assert!(looks_like_patch("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n"));
        assert!(!looks_like_patch("just some text"));
        assert!(!is_mailbox("diff --git a/x b/x\n"));
    }
}
