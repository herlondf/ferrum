//! Stacked branches: branches based on each other (`main` ← `api` ← `ui`),
//! like Graphite or git-town.
//!
//! A branch's parent is read from `branch.<name>.ferrumParent` when set
//! (Ferrum writes it when it creates a branch in a stack or restacks one), or
//! else inferred: the nearest other local branch whose tip is in its history,
//! or the trunk (`main`, `master`…) it forked from.
//!
//! Restacking rebases the branches whose parent moved with one
//! `git rebase --update-refs` (git 2.38 or newer), so a conflict in the
//! middle is resolved, continued or aborted like any rebase. Older git
//! rebases one branch at a time.

use std::collections::{HashMap, HashSet};

use crate::cli::Git;
use crate::{BranchKind, Error, Oid, Repo, Result};

/// The config key holding a branch's parent in a stack.
pub const PARENT_KEY: &str = "ferrumParent";

/// Branches compared when inferring parents, at most (pairs grow fast).
const MAX_BRANCHES: usize = 250;

/// One branch of a stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackBranch {
    pub name: String,
    /// The branch below (the trunk for the first one).
    pub parent: String,
    pub tip: Oid,
    /// Commits on this branch that its parent does not have.
    pub commits: usize,
    /// The parent has commits this branch is not based on: restack it.
    pub behind_parent: bool,
}

/// Branches stacked on each other, bottom first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    /// The branch the stack grows from, e.g. `main`.
    pub trunk: String,
    pub branches: Vec<StackBranch>,
}

impl Stack {
    pub fn top(&self) -> &StackBranch {
        self.branches.last().expect("a stack has branches")
    }

    pub fn names(&self) -> Vec<String> {
        self.branches.iter().map(|b| b.name.clone()).collect()
    }

    pub fn contains(&self, branch: &str) -> bool {
        self.branches.iter().any(|b| b.name == branch)
    }

    /// Some branch needs a restack.
    pub fn needs_restack(&self) -> bool {
        self.branches.iter().any(|b| b.behind_parent)
    }

    /// Position (1 = bottom) of `branch`.
    pub fn position(&self, branch: &str) -> Option<usize> {
        self.branches
            .iter()
            .position(|b| b.name == branch)
            .map(|i| i + 1)
    }
}

/// Branch names that usually are the trunk, best first.
pub fn trunk_names(repo: &Repo) -> Vec<String> {
    let mut out = Vec::new();
    // The remote's default branch (`origin/HEAD`) first.
    if let Ok(r) = repo.raw().find_reference("refs/remotes/origin/HEAD")
        && let Some(t) = r.symbolic_target_bytes()
    {
        let t = crate::lossy(t);
        if let Some(name) = t.strip_prefix("refs/remotes/origin/") {
            out.push(name.to_owned());
        }
    }
    for n in ["main", "master", "develop", "trunk"] {
        if !out.iter().any(|o| o == n) {
            out.push(n.to_owned());
        }
    }
    out
}

/// The parent recorded in the config for `branch`.
pub fn recorded_parent(repo: &Repo, branch: &str) -> Option<String> {
    let cfg = repo.raw().config().ok()?.snapshot().ok()?;
    let v = cfg
        .get_bytes(&format!("branch.{branch}.{PARENT_KEY}"))
        .ok()
        .map(crate::lossy)?;
    let v = v.trim().to_owned();
    (!v.is_empty() && v != branch).then_some(v)
}

/// Records `parent` as the parent of `branch`.
pub fn set_parent(git: &Git, branch: &str, parent: &str) -> Result<()> {
    git.run(&["config", &format!("branch.{branch}.{PARENT_KEY}"), parent])?;
    Ok(())
}

/// Forgets the recorded parent of `branch`.
pub fn clear_parent(git: &Git, branch: &str) -> Result<()> {
    // Exit code 5: the key was not set.
    match git.run(&[
        "config",
        "--unset",
        &format!("branch.{branch}.{PARENT_KEY}"),
    ]) {
        Ok(_) => Ok(()),
        Err(Error::Cli(_)) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Finds the stacks among the local branches. A stack has at least two
/// branches above its trunk; branches that fork into several make one stack
/// per top branch.
pub fn detect(repo: &Repo) -> Result<Vec<Stack>> {
    let locals: Vec<(String, Oid)> = repo
        .branches()?
        .into_iter()
        .filter(|b| b.kind == BranchKind::Local)
        .filter_map(|b| Some((b.name, b.target?)))
        .collect();
    let tips: HashMap<&str, Oid> = locals.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    let trunks: Vec<String> = trunk_names(repo)
        .into_iter()
        .filter(|t| tips.contains_key(t.as_str()))
        .collect();
    let infer = locals.len() <= MAX_BRANCHES;

    let mut parents: HashMap<String, String> = HashMap::new();
    for (name, tip) in &locals {
        if trunks.contains(name) {
            continue;
        }
        let recorded = recorded_parent(repo, name).filter(|p| tips.contains_key(p.as_str()));
        let parent = match recorded {
            Some(p) => Some(p),
            None if infer => infer_parent(repo, name, *tip, &locals, &trunks),
            None => None,
        };
        if let Some(p) = parent {
            parents.insert(name.clone(), p);
        }
    }
    // Drop cycles (only possible with a hand-edited config).
    let names: Vec<String> = parents.keys().cloned().collect();
    for n in names {
        let mut seen = HashSet::new();
        let mut at = n.clone();
        while let Some(p) = parents.get(&at) {
            if !seen.insert(at.clone()) {
                parents.remove(&n);
                break;
            }
            at = p.clone();
        }
    }

    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for (c, p) in &parents {
        children.entry(p.as_str()).or_default().push(c.as_str());
    }
    for list in children.values_mut() {
        list.sort_unstable();
    }
    // Roots: trunks and parents that have no parent themselves.
    let mut roots: Vec<&str> = children
        .keys()
        .copied()
        .filter(|p| !parents.contains_key(*p))
        .collect();
    roots.sort_by_key(|r| (!trunks.iter().any(|t| t == r), r.to_string()));

    let mut stacks = Vec::new();
    for root in roots {
        let mut paths = Vec::new();
        collect_paths(root, &children, &mut Vec::new(), &mut paths);
        for path in paths {
            if path.len() < 2 {
                continue;
            }
            let mut branches = Vec::new();
            let mut parent = root.to_owned();
            for name in path {
                let tip = tips[name];
                let ptip = tips[parent.as_str()];
                branches.push(StackBranch {
                    name: name.to_owned(),
                    parent: parent.clone(),
                    tip,
                    commits: count_between(repo, ptip, tip),
                    behind_parent: !repo.is_ancestor(ptip, tip),
                });
                parent = name.to_owned();
            }
            stacks.push(Stack {
                trunk: root.to_owned(),
                branches,
            });
        }
    }
    Ok(stacks)
}

fn collect_paths<'a>(
    at: &'a str,
    children: &HashMap<&'a str, Vec<&'a str>>,
    path: &mut Vec<&'a str>,
    out: &mut Vec<Vec<&'a str>>,
) {
    match children.get(at) {
        Some(list) if !list.is_empty() && path.len() < 64 => {
            for c in list {
                path.push(c);
                collect_paths(c, children, path, out);
                path.pop();
            }
        }
        _ => out.push(path.clone()),
    }
}

/// The nearest local branch in `name`'s history, or the trunk it forked from.
fn infer_parent(
    repo: &Repo,
    name: &str,
    tip: Oid,
    locals: &[(String, Oid)],
    trunks: &[String],
) -> Option<String> {
    let tip_of = |n: &str| locals.iter().find(|(b, _)| b == n).map(|(_, t)| *t);
    // Merged into a trunk: not part of a stack.
    if trunks
        .iter()
        .filter_map(|t| tip_of(t))
        .any(|t| repo.is_ancestor(tip, t))
    {
        return None;
    }
    let below: Vec<(&str, Oid)> = locals
        .iter()
        .filter(|(b, t)| b != name && *t != tip && repo.is_ancestor(*t, tip))
        .map(|(b, t)| (b.as_str(), *t))
        .collect();
    // The nearest: every other candidate is in its history.
    let nearest = below
        .iter()
        .find(|(_, t)| below.iter().all(|(_, o)| repo.is_ancestor(*o, *t)));
    if let Some((_, at)) = nearest {
        // Several branches at that commit: the trunk wins, then the first name.
        let mut same: Vec<&str> = below
            .iter()
            .filter(|(_, t)| t == at)
            .map(|(b, _)| *b)
            .collect();
        same.sort_by_key(|b| (!trunks.iter().any(|t| t == b), b.to_string()));
        return same.first().map(|b| (*b).to_owned());
    }
    if let Some((b, _)) = below.first() {
        return Some((*b).to_owned());
    }
    // The trunk moved on since the branch forked from it.
    trunks
        .iter()
        .find(|t| {
            tip_of(t).is_some_and(|tt| repo.raw().merge_base(tt, tip).is_ok()) && t.as_str() != name
        })
        .cloned()
}

fn count_between(repo: &Repo, from: Oid, to: Oid) -> usize {
    let Ok(mut walk) = repo.raw().revwalk() else {
        return 0;
    };
    if walk.push(to).is_err() || walk.hide(from).is_err() {
        return 0;
    }
    walk.take(1000).count()
}

/// The stack `branch` belongs to (the first one, when it forks).
pub fn stack_of<'a>(stacks: &'a [Stack], branch: &str) -> Option<&'a Stack> {
    stacks
        .iter()
        .find(|s| s.contains(branch))
        .or_else(|| stacks.iter().find(|s| s.trunk == branch))
}

// ----- git version -----

/// `--update-refs` needs git 2.38.
pub fn supports_update_refs(version: &str) -> bool {
    let mut parts = version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    (major, minor) >= (2, 38)
}

/// Whether the configured git supports `--update-refs` (asked once).
pub fn git_supports_update_refs() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| crate::cli::version().is_some_and(|v| supports_update_refs(&v)))
}

// ----- Restack -----

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestackOptions {
    /// One `git rebase --update-refs` for the whole stack (git 2.38+);
    /// otherwise one rebase per branch.
    pub update_refs: bool,
    /// Check this branch out again when done (the one checked out before).
    pub return_to: Option<String>,
}

/// What a restack will do, for the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestackStep {
    pub branch: String,
    /// Rebased onto this branch.
    pub onto: String,
    pub commits: usize,
}

/// The branches a restack rebases, bottom first: from the lowest one whose
/// parent moved up to the top.
pub fn restack_preview(stack: &Stack) -> Vec<RestackStep> {
    let Some(first) = stack.branches.iter().position(|b| b.behind_parent) else {
        return Vec::new();
    };
    stack.branches[first..]
        .iter()
        .map(|b| RestackStep {
            branch: b.name.clone(),
            onto: b.parent.clone(),
            commits: b.commits,
        })
        .collect()
}

fn tip(git: &Git, branch: &str) -> Result<String> {
    Ok(git
        .run(&[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ])?
        .trim()
        .to_owned())
}

/// Where the commits of `branch` on top of `parent` start: the fork point
/// (from the parent's reflog, so an amended parent is handled), else the
/// merge base.
fn upstream_of(git: &Git, parent: &str, branch: &str, old_parent: Option<&str>) -> Result<String> {
    let repo = Repo::open(git.dir())?;
    let btip = repo.resolve(&format!("refs/heads/{branch}"))?;
    if let Some(old) = old_parent
        && let Ok(o) = repo.resolve(old)
        && repo.is_ancestor(o, btip)
    {
        return Ok(old.to_owned());
    }
    if let Ok(out) = git.run(&[
        "merge-base",
        "--fork-point",
        &format!("refs/heads/{parent}"),
        &format!("refs/heads/{branch}"),
    ]) && !out.trim().is_empty()
    {
        return Ok(out.trim().to_owned());
    }
    Ok(git
        .run(&[
            "merge-base",
            &format!("refs/heads/{parent}"),
            &format!("refs/heads/{branch}"),
        ])?
        .trim()
        .to_owned())
}

/// Changes to tracked files, staged or not.
pub fn has_local_changes(git: &Git) -> Result<bool> {
    let out = git.run(&["status", "--porcelain", "--untracked-files=no"])?;
    Ok(!out.trim().is_empty())
}

fn rebasing(git: &Git) -> bool {
    let Ok(repo) = Repo::open(git.dir()) else {
        return false;
    };
    let dir = repo.git_dir();
    dir.join("rebase-merge").exists() || dir.join("rebase-apply").exists()
}

/// Rebases the stack's branches onto their parents. Returns normally when a
/// rebase stops for a conflict (check the repository state): continuing it
/// finishes the stack with `--update-refs`; with older git, run the restack
/// again after continuing.
pub fn restack(git: &Git, stack: &Stack, opts: &RestackOptions) -> Result<()> {
    let top = stack.top().name.clone();
    let head = Repo::open(git.dir())?.head_branch();
    // `--autostash` would carry local changes over to another branch.
    if (head.as_deref() != Some(top.as_str()) || !opts.update_refs) && has_local_changes(git)? {
        return Err(Error::Other(
            "Commit or stash the local changes before restacking.".into(),
        ));
    }
    // Parents are recorded so the stack is still known once tips move.
    for b in &stack.branches {
        set_parent(git, &b.name, &b.parent)?;
    }
    let names = stack.names();
    if opts.update_refs {
        // One rebase per break in the stack; usually just one.
        for _ in 0..names.len() {
            let fresh = refresh(git, stack)?;
            let Some(b) = fresh.branches.iter().find(|b| b.behind_parent) else {
                break;
            };
            let upstream = upstream_of(git, &b.parent, &b.name, None)?;
            let args = [
                "rebase",
                "--update-refs",
                "--autostash",
                "--onto",
                &format!("refs/heads/{}", b.parent),
                &upstream,
                &top,
            ];
            if let Err(e) = git.run(&args) {
                if rebasing(git) {
                    return Ok(());
                }
                return Err(e);
            }
        }
    } else {
        let old: HashMap<String, String> = names
            .iter()
            .filter_map(|n| Some((n.clone(), tip(git, n).ok()?)))
            .collect();
        let fresh = refresh(git, stack)?;
        let first = fresh.branches.iter().position(|b| b.behind_parent);
        for b in &fresh.branches[first.unwrap_or(fresh.branches.len())..] {
            let old_parent = old.get(&b.parent).map(String::as_str);
            let upstream = upstream_of(git, &b.parent, &b.name, old_parent)?;
            let args = [
                "rebase",
                "--autostash",
                "--onto",
                &format!("refs/heads/{}", b.parent),
                &upstream,
                &b.name,
            ];
            if let Err(e) = git.run(&args) {
                if rebasing(git) {
                    return Ok(());
                }
                return Err(e);
            }
        }
    }
    if let Some(back) = &opts.return_to
        && !rebasing(git)
        && Repo::open(git.dir())?.head_branch().as_deref() != Some(back.as_str())
    {
        git.run(&["checkout", "-q", back])?;
    }
    Ok(())
}

/// The stack with its tips read again.
fn refresh(git: &Git, stack: &Stack) -> Result<Stack> {
    let repo = Repo::open(git.dir())?;
    let mut out = stack.clone();
    for b in &mut out.branches {
        let t = repo.resolve(&format!("refs/heads/{}", b.name))?;
        let p = repo.resolve(&format!("refs/heads/{}", b.parent))?;
        b.tip = t;
        b.behind_parent = !repo.is_ancestor(p, t);
        b.commits = count_between(&repo, p, t);
    }
    Ok(out)
}

/// Where a new branch goes in a stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Insert {
    /// On top of `branch` (`branch` becomes its parent).
    Above(String),
    /// Between `branch` and its parent: `branch` is restacked on it later.
    Below(String),
}

/// Creates `name` in a stack and records the parents. The new branch starts
/// at the tip of its parent and is checked out when `checkout`.
pub fn add_branch(git: &Git, name: &str, at: &Insert, checkout: bool) -> Result<()> {
    let repo = Repo::open(git.dir())?;
    let (parent, child) = match at {
        Insert::Above(b) => (b.clone(), None),
        Insert::Below(b) => {
            let p = recorded_parent(&repo, b)
                .or_else(|| {
                    detect(&repo).ok().and_then(|s| {
                        s.iter()
                            .flat_map(|s| s.branches.iter())
                            .find(|x| &x.name == b)
                            .map(|x| x.parent.clone())
                    })
                })
                .ok_or_else(|| Error::Other(format!("{b} has no branch below it.")))?;
            (p, Some(b.clone()))
        }
    };
    crate::ops::create_branch(git, name, &parent, checkout)?;
    set_parent(git, name, &parent)?;
    if let Some(c) = child {
        // Keep the old parent of the branch above recorded: restacking it
        // later onto the new branch finds its commits from there.
        set_parent(git, &c, name)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RepoState;
    use crate::testutil::TestRepo;

    /// main ← a ← b ← c, each with one commit.
    fn stacked() -> TestRepo {
        let t = TestRepo::new();
        t.commit("base.txt", "base\n", "base");
        for name in ["a", "b", "c"] {
            t.git.run(&["checkout", "-q", "-b", name]).unwrap();
            t.commit(&format!("{name}.txt"), &format!("{name}\n"), name);
        }
        t.git.run(&["checkout", "-q", "main"]).unwrap();
        t
    }

    fn names(s: &Stack) -> Vec<&str> {
        s.branches.iter().map(|b| b.name.as_str()).collect()
    }

    fn log(t: &TestRepo, rev: &str) -> Vec<String> {
        t.git
            .run(&["log", "--format=%s", rev])
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn detects_a_stack() {
        let t = stacked();
        // A lone feature branch is not a stack.
        t.git.run(&["branch", "lone", "main~0"]).unwrap();
        let stacks = detect(&t.repo()).unwrap();
        assert_eq!(stacks.len(), 1, "{stacks:?}");
        let s = &stacks[0];
        assert_eq!(s.trunk, "main");
        assert_eq!(names(s), ["a", "b", "c"]);
        assert_eq!(s.branches[1].parent, "a");
        assert!(
            s.branches
                .iter()
                .all(|b| b.commits == 1 && !b.behind_parent)
        );
        assert_eq!(
            stack_of(&stacks, "b").map(|s| s.top().name.as_str()),
            Some("c")
        );
    }

    #[test]
    fn version_check() {
        assert!(supports_update_refs("2.38.0"));
        assert!(supports_update_refs("2.47.1.windows.1"));
        assert!(supports_update_refs("3.0"));
        assert!(!supports_update_refs("2.37.9"));
        assert!(!supports_update_refs("1.9"));
        assert!(!supports_update_refs(""));
    }

    fn restack_after_trunk_moves(update_refs: bool) {
        let t = stacked();
        t.commit("main.txt", "m\n", "main moved");
        let stacks = detect(&t.repo()).unwrap();
        let s = &stacks[0];
        assert!(s.branches[0].behind_parent && s.needs_restack());
        assert_eq!(restack_preview(s).len(), 3);
        let opts = RestackOptions {
            update_refs,
            return_to: Some("main".into()),
        };
        restack(&t.git, s, &opts).unwrap();
        assert_eq!(t.repo().state(), RepoState::Clean);
        assert_eq!(t.repo().head_branch().as_deref(), Some("main"));
        assert_eq!(log(&t, "c"), ["c", "b", "a", "main moved", "base"]);
        assert_eq!(log(&t, "a"), ["a", "main moved", "base"]);
        let again = detect(&t.repo()).unwrap();
        assert!(!again[0].needs_restack(), "{again:?}");
    }

    #[test]
    fn restacks_with_update_refs() {
        if !git_supports_update_refs() {
            eprintln!("git without --update-refs: skipped");
            return;
        }
        restack_after_trunk_moves(true);
    }

    #[test]
    fn restacks_one_branch_at_a_time() {
        restack_after_trunk_moves(false);
    }

    #[test]
    fn restacks_after_a_middle_branch_gets_a_commit() {
        let t = stacked();
        // Record the parents first: b no longer contains a's tip afterwards.
        let s = detect(&t.repo()).unwrap().remove(0);
        for b in &s.branches {
            set_parent(&t.git, &b.name, &b.parent).unwrap();
        }
        t.git.run(&["checkout", "-q", "a"]).unwrap();
        t.commit("a2.txt", "a2\n", "a2");
        let s = detect(&t.repo()).unwrap().remove(0);
        assert_eq!(names(&s), ["a", "b", "c"]);
        assert!(!s.branches[0].behind_parent && s.branches[1].behind_parent);
        for update_refs in [git_supports_update_refs(), false] {
            let opts = RestackOptions {
                update_refs,
                return_to: None,
            };
            restack(&t.git, &s, &opts).unwrap();
        }
        assert_eq!(log(&t, "c"), ["c", "b", "a2", "a", "base"]);
    }

    #[test]
    fn restack_conflict_can_be_continued_or_aborted() {
        let t = TestRepo::new();
        t.commit("f.txt", "base\n", "base");
        t.git.run(&["checkout", "-q", "-b", "a"]).unwrap();
        t.commit("f.txt", "a\n", "a");
        t.git.run(&["checkout", "-q", "-b", "b"]).unwrap();
        t.commit("b.txt", "b\n", "b");
        t.git.run(&["checkout", "-q", "main"]).unwrap();
        t.commit("f.txt", "main\n", "main changes f");
        let s = detect(&t.repo()).unwrap().remove(0);
        let before_b = s.branches[1].tip;
        let opts = RestackOptions {
            update_refs: git_supports_update_refs(),
            return_to: Some("main".into()),
        };
        restack(&t.git, &s, &opts).unwrap();
        assert_eq!(t.repo().state(), RepoState::Rebase);
        // Abort: everything is back.
        crate::ops::abort_operation(&t.git, RepoState::Rebase).unwrap();
        assert_eq!(t.repo().resolve("b").unwrap(), before_b);

        // Again, resolving the conflict and continuing.
        restack(&t.git, &s, &opts).unwrap();
        assert_eq!(t.repo().state(), RepoState::Rebase);
        t.write("f.txt", "main and a\n");
        t.git.run(&["add", "f.txt"]).unwrap();
        crate::ops::continue_operation(&t.git, RepoState::Rebase).unwrap();
        if !opts.update_refs {
            // Older git: the rest of the stack is done by restacking again.
            let s = detect(&t.repo()).unwrap().remove(0);
            restack(&t.git, &s, &opts).unwrap();
        }
        assert_eq!(t.repo().state(), RepoState::Clean);
        assert_eq!(log(&t, "b"), ["b", "a", "main changes f", "base"]);
    }

    #[test]
    fn adds_branches_on_top_and_in_the_middle() {
        let t = stacked();
        add_branch(&t.git, "d", &Insert::Above("c".into()), false).unwrap();
        add_branch(&t.git, "ab", &Insert::Below("b".into()), true).unwrap();
        assert_eq!(t.repo().head_branch().as_deref(), Some("ab"));
        assert_eq!(recorded_parent(&t.repo(), "b").as_deref(), Some("ab"));
        let s = detect(&t.repo()).unwrap().remove(0);
        assert_eq!(names(&s), ["a", "ab", "b", "c", "d"]);
        // A commit on the inserted branch: b and above need a restack.
        t.commit("ab.txt", "ab\n", "ab");
        let s = detect(&t.repo()).unwrap().remove(0);
        assert_eq!(
            restack_preview(&s)
                .iter()
                .map(|r| r.branch.as_str())
                .collect::<Vec<_>>(),
            ["b", "c", "d"]
        );
        let opts = RestackOptions {
            update_refs: git_supports_update_refs(),
            return_to: Some("ab".into()),
        };
        restack(&t.git, &s, &opts).unwrap();
        assert_eq!(log(&t, "d"), ["c", "b", "ab", "a", "base"]);
        assert_eq!(t.repo().head_branch().as_deref(), Some("ab"));
        clear_parent(&t.git, "d").unwrap();
        clear_parent(&t.git, "d").unwrap();
    }
}
