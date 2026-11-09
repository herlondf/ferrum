//! Moves selected hunks or lines of a commit into another commit of the
//! current branch, or splits them into a new commit right after it.
//!
//! The source commit S is split first, without touching the working copy:
//! S' (S without the selection) and P (just the selection, on top of S').
//! Then one interactive rebase picks S' in S's place and either melds P into
//! the destination (`fixup` right after it) or keeps P as a new commit.
//! Conflicts stop the rebase like any other; aborting it changes nothing.

use std::path::{Path, PathBuf};

use crate::cli::Git;
use crate::rebase::{self, Action, Step};
use crate::{Error, Oid, Repo, Result};

/// Part of a commit's change to move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Selected lines: a patch from `diff::partial_patch(.., reverse = true)`
    /// of the commit's diff, so it applies in reverse to the commit.
    Lines(String),
    /// Whole files (both paths of a rename).
    Files(Vec<String>),
}

/// Where the selection goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dest {
    /// Meld into this commit (keeping its message).
    Commit(Oid),
    /// A new commit with this message, right after the source.
    New(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Move other branches pointing into the rewritten commits (git 2.38+).
    pub update_refs: bool,
}

/// Checks that `source` and the destination are commits of the current
/// branch (see `absorb::branch_commits`). Returns the commit the rebase
/// starts after (`None`: the root).
pub fn check(repo: &Repo, source: Oid, dest: &Dest) -> std::result::Result<Option<Oid>, String> {
    let mine = crate::absorb::branch_commits(repo, 500).map_err(|e| e.to_string())?;
    let pos = |id: Oid| mine.iter().position(|c| c.id == id);
    let s = pos(source).ok_or("The commit is not on the current branch.")?;
    let oldest = match dest {
        Dest::Commit(d) if *d == source => {
            return Err("Pick another commit than the one the changes come from.".into());
        }
        Dest::Commit(d) => s.max(pos(*d).ok_or("The target commit is not on the current branch.")?),
        Dest::New(msg) if msg.trim().is_empty() => {
            return Err("The new commit needs a message.".into());
        }
        Dest::New(_) => s,
    };
    Ok(mine[oldest].parents.first().copied())
}

fn temp_dir() -> Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("ferrum-move-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The tree of `source` without `parts`.
fn tree_without(git: &Git, work: &Path, source: &git2::Commit, parts: &[Part]) -> Result<String> {
    let index = work.join("index");
    let ig = git
        .clone()
        .with_env("GIT_INDEX_FILE", index.to_string_lossy());
    ig.run(&["read-tree", &source.id().to_string()])?;
    let parent = source.parent_id(0).ok().map(|p| p.to_string());
    for (i, part) in parts.iter().enumerate() {
        match part {
            Part::Lines(patch) => {
                let file = work.join(format!("part-{i}.patch"));
                std::fs::write(&file, patch)?;
                let file = file.to_string_lossy().into_owned();
                ig.run(&["apply", "--cached", "-R", "--whitespace=nowarn", &file])?;
            }
            Part::Files(paths) => {
                let mut args: Vec<&str> = match &parent {
                    Some(p) => vec!["reset", "-q", p, "--"],
                    None => vec!["rm", "--cached", "-q", "--ignore-unmatch", "--"],
                };
                args.extend(paths.iter().map(String::as_str));
                ig.run(&args)?;
            }
        }
    }
    Ok(ig.run(&["write-tree"])?.trim().to_owned())
}

/// `git commit-tree` with a message file and, optionally, the author of `like`.
fn commit_tree(
    git: &Git,
    work: &Path,
    tree: &str,
    parent: Option<&str>,
    message: &[u8],
    like: Option<&git2::Commit>,
) -> Result<Oid> {
    let file = work.join(format!("msg-{tree}"));
    std::fs::write(&file, message)?;
    let file = file.to_string_lossy().into_owned();
    let mut args = vec!["commit-tree", tree];
    if let Some(p) = parent {
        args.extend(["-p", p]);
    }
    args.extend(["-F", &file]);
    let mut git = git.clone();
    if let Some(c) = like {
        let a = c.author();
        let when = a.when();
        let sign = if when.offset_minutes() < 0 { '-' } else { '+' };
        let off = when.offset_minutes().abs();
        git = git
            .with_env("GIT_AUTHOR_NAME", crate::lossy(a.name_bytes()))
            .with_env("GIT_AUTHOR_EMAIL", crate::lossy(a.email_bytes()))
            .with_env(
                "GIT_AUTHOR_DATE",
                format!("{} {sign}{:02}{:02}", when.seconds(), off / 60, off % 60),
            );
    }
    let out = git.run(&args)?;
    Oid::from_str(out.trim()).map_err(Error::from)
}

/// Moves `parts` of `source` to `dest`. Returns normally when the rebase
/// stops for a conflict (check the repository state).
pub fn run(git: &Git, source: Oid, parts: &[Part], dest: &Dest, opts: &Options) -> Result<()> {
    let repo = Repo::open(git.dir())?;
    let base = check(&repo, source, dest).map_err(Error::Other)?;
    if parts.is_empty() {
        return Err(Error::Other("Nothing is selected to move.".into()));
    }
    let raw = repo.raw();
    let s = raw.find_commit(source)?;
    let work = temp_dir()?;
    let result = (|| {
        let rest_tree = tree_without(git, &work, &s, parts)?;
        let s_tree = s.tree_id().to_string();
        if rest_tree == s_tree {
            return Err(Error::Other("Nothing is selected to move.".into()));
        }
        let parent = s.parent_id(0).ok();
        let parent_tree = match parent {
            Some(p) => raw.find_commit(p)?.tree_id().to_string(),
            None => git
                .run(&["hash-object", "-t", "tree", "--stdin"])
                .map(|o| o.trim().to_owned())?,
        };
        let parent_str = parent.map(|p| p.to_string());
        // S without the selection; nothing left when all of it moves.
        let rest = if rest_tree == parent_tree {
            None
        } else {
            Some(commit_tree(
                git,
                &work,
                &rest_tree,
                parent_str.as_deref(),
                s.message_bytes(),
                Some(&s),
            )?)
        };
        let below = rest.map(|r| r.to_string()).or(parent_str.clone());
        let message = match dest {
            Dest::New(m) => m.clone(),
            Dest::Commit(d) => {
                let d = repo.find_commit_info(*d)?;
                format!("fixup! {}", d.summary)
            }
        };
        let moved = commit_tree(
            git,
            &work,
            &s_tree,
            below.as_deref(),
            message.as_bytes(),
            None,
        )?;
        let rest_info = rest.map(|r| repo.find_commit_info(r)).transpose()?;
        let moved_info = repo.find_commit_info(moved)?;

        let mut steps: Vec<Step> = Vec::new();
        // Where each original commit ended up, for `update-ref`.
        let mut anchors: Vec<(Oid, Oid)> = Vec::new();
        for c in repo.commits_since(base)? {
            let id = c.id;
            if id == source {
                if let Some(r) = &rest_info {
                    steps.push(pick(r.clone()));
                }
                if matches!(dest, Dest::New(_)) {
                    steps.push(pick(moved_info.clone()));
                }
            } else {
                steps.push(pick(c));
            }
            if *dest == Dest::Commit(id) {
                steps.push(Step {
                    commit: moved_info.clone(),
                    action: Action::Fixup,
                });
            }
            if let Some(last) = steps.last() {
                anchors.push((id, last.commit.id));
            }
        }
        let mut update_refs = Vec::new();
        if opts.update_refs {
            let head = repo.head_branch();
            for b in repo.branches()? {
                if b.kind != crate::BranchKind::Local || Some(&b.name) == head.as_ref() {
                    continue;
                }
                if let Some((_, at)) = anchors.iter().find(|(orig, _)| Some(*orig) == b.target) {
                    update_refs.push((*at, b.name.clone()));
                }
            }
        }
        let ropts = rebase::Options {
            update_refs,
            backup: None,
        };
        rebase::run_with(git, base, &steps, &ropts)
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn pick(commit: crate::CommitInfo) -> Step {
    Step {
        commit,
        action: Action::Pick,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::partial_patch;
    use crate::model::{LineKind, RepoState};
    use crate::testutil::TestRepo;

    const TEN: &str = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\n";

    /// feature: A adds a.txt; B changes lines 2 and 9 of f.txt and adds b.txt.
    fn setup() -> TestRepo {
        let t = TestRepo::new();
        t.commit("f.txt", TEN, "base");
        t.git.run(&["checkout", "-q", "-b", "feature"]).unwrap();
        t.commit("a.txt", "a\n", "A");
        t.write("b.txt", "b\n");
        t.commit(
            "f.txt",
            &TEN.replace("b\n", "two\n").replace("m\n", "nine\n"),
            "B",
        );
        t
    }

    fn id(t: &TestRepo, rev: &str) -> Oid {
        t.repo().resolve(rev).unwrap()
    }

    fn log(t: &TestRepo) -> Vec<String> {
        t.git
            .run(&["log", "--format=%s"])
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn show(t: &TestRepo, rev: &str) -> String {
        t.git
            .run(&["show", "--format=", "--name-only", rev])
            .unwrap()
    }

    /// The second hunk of B's change to f.txt (line 9), as a part.
    fn second_hunk(t: &TestRepo, commit: Oid) -> Part {
        let file = t
            .repo()
            .commit_file_diff(commit, "f.txt", None)
            .unwrap()
            .unwrap();
        assert_eq!(file.hunks.len(), 2);
        Part::Lines(partial_patch(&file, |h, _| h == 1, true).unwrap())
    }

    #[test]
    fn moves_a_hunk_into_an_older_commit() {
        let t = setup();
        let (a, b) = (id(&t, "HEAD~1"), id(&t, "HEAD"));
        let part = second_hunk(&t, b);
        run(&t.git, b, &[part], &Dest::Commit(a), &Options::default()).unwrap();
        assert_eq!(t.repo().state(), RepoState::Clean);
        assert_eq!(log(&t), ["B", "A", "base"]);
        let a_f = t.git.run(&["show", "HEAD~1:f.txt"]).unwrap();
        assert!(a_f.contains("nine") && !a_f.contains("two"), "{a_f}");
        // The end result is the same.
        assert_eq!(
            t.read("f.txt"),
            TEN.replace("b\n", "two\n").replace("m\n", "nine\n")
        );
        let backup_free = t.git.run(&["branch", "--list", "backup/*"]).unwrap();
        assert!(backup_free.is_empty());
    }

    #[test]
    fn moves_a_file_into_a_newer_commit_and_splits() {
        let t = setup();
        t.commit("c.txt", "c\n", "C");
        let (a, c) = (id(&t, "HEAD~2"), id(&t, "HEAD"));
        // a.txt from A into C.
        let part = Part::Files(vec!["a.txt".into()]);
        run(&t.git, a, &[part], &Dest::Commit(c), &Options::default()).unwrap();
        // A had only a.txt: it is gone.
        assert_eq!(log(&t), ["C", "B", "base"]);
        assert!(show(&t, "HEAD").contains("a.txt"));

        // Split b.txt out of B into a new commit after it.
        let b = id(&t, "HEAD~1");
        let part = Part::Files(vec!["b.txt".into()]);
        let dest = Dest::New("Add b".into());
        run(&t.git, b, &[part], &dest, &Options::default()).unwrap();
        assert_eq!(log(&t), ["C", "Add b", "B", "base"]);
        assert_eq!(show(&t, "HEAD~1").trim(), "b.txt");
        assert_eq!(show(&t, "HEAD~2").trim(), "f.txt");
    }

    #[test]
    fn moves_single_lines() {
        let t = setup();
        let b = id(&t, "HEAD");
        let file = t
            .repo()
            .commit_file_diff(b, "f.txt", None)
            .unwrap()
            .unwrap();
        // Only the added "two" (keeping the removed "2" in B): a new commit
        // then holds the addition.
        let added = file.hunks[0]
            .lines
            .iter()
            .position(|l| l.kind == LineKind::Added)
            .unwrap();
        let patch = partial_patch(&file, |h, l| h == 0 && l == added, true).unwrap();
        let dest = Dest::New("Add two".into());
        run(&t.git, b, &[Part::Lines(patch)], &dest, &Options::default()).unwrap();
        let b_f = t.git.run(&["show", "HEAD~1:f.txt"]).unwrap();
        assert!(!b_f.contains("\nb\n") && !b_f.contains("two"), "{b_f}");
        assert_eq!(
            t.read("f.txt"),
            TEN.replace("b\n", "two\n").replace("m\n", "nine\n")
        );
    }

    #[test]
    fn keeps_other_branches_and_refuses_foreign_commits() {
        let t = setup();
        let (a, b) = (id(&t, "HEAD~1"), id(&t, "HEAD"));
        // A commit of main is not on the branch.
        let base = id(&t, "main");
        assert!(check(&t.repo(), base, &Dest::New("x".into())).is_err());
        assert!(check(&t.repo(), b, &Dest::Commit(b)).is_err());
        assert!(check(&t.repo(), b, &Dest::New(" ".into())).is_err());
        assert_eq!(check(&t.repo(), b, &Dest::Commit(a)), Ok(Some(base)));
        if !crate::stack::git_supports_update_refs() {
            return;
        }
        t.git.run(&["branch", "at-a", &a.to_string()]).unwrap();
        let part = second_hunk(&t, b);
        let opts = Options { update_refs: true };
        run(&t.git, b, &[part], &Dest::Commit(a), &opts).unwrap();
        assert_eq!(id(&t, "at-a"), id(&t, "HEAD~1"), "at-a moved along");
    }

    #[test]
    fn conflict_can_be_aborted() {
        let t = TestRepo::new();
        t.commit("f.txt", TEN, "base");
        t.git.run(&["checkout", "-q", "-b", "feature"]).unwrap();
        t.commit("g.txt", "g\n", "A");
        t.commit("f.txt", &TEN.replace("e\n", "five\n"), "B");
        t.commit("f.txt", &TEN.replace("e\n", "FIVE\n"), "C");
        let (a, c) = (id(&t, "HEAD~2"), id(&t, "HEAD"));
        // C's change depends on B's: moving it before B conflicts.
        let file = t
            .repo()
            .commit_file_diff(c, "f.txt", None)
            .unwrap()
            .unwrap();
        let part = Part::Lines(partial_patch(&file, |_, _| true, true).unwrap());
        run(&t.git, c, &[part], &Dest::Commit(a), &Options::default()).unwrap();
        assert_eq!(t.repo().state(), RepoState::Rebase);
        crate::ops::abort_operation(&t.git, RepoState::Rebase).unwrap();
        assert_eq!(id(&t, "HEAD"), c);
        assert_eq!(log(&t), ["C", "B", "A", "base"]);
    }
}
