//! Absorb: folds local changes into the commits of the current branch that
//! last touched those lines, like git-absorb.
//!
//! For each hunk of the local changes (staged ones if anything is staged,
//! else all changes to tracked files) the lines it changes are blamed in
//! HEAD. When they all come from one commit of the current branch, the hunk
//! becomes a `fixup!` commit for it; then an optional autosquash rebase melds
//! the fixups in. A hunk replacing lines one for one is split when its lines
//! come from several commits. Everything else stays a local change, with the
//! reason shown in the preview.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use git2::{BlameOptions, DiffOptions, Patch};

use crate::cli::Git;
use crate::{CommitInfo, Error, Oid, Repo, Result};

/// Commits considered at most.
pub const MAX_COMMITS: usize = 50;

/// Why a hunk stays a local change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stay {
    /// A new (or untracked) file: no commit touched it yet.
    NewFile,
    /// Deleted, renamed, binary or mode change: only whole lines are absorbed.
    WholeFile,
    /// Added lines next to lines of different commits, or of none on the branch.
    NoOwner,
    /// The lines come from a commit that is not on the current branch.
    NotOnBranch,
    /// The lines come from several commits and cannot be split.
    SeveralCommits,
}

/// One hunk of the local changes and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsorbHunk {
    pub path: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// Removed and added lines, with their line endings.
    pub removed: Vec<Vec<u8>>,
    pub added: Vec<Vec<u8>>,
    pub target: std::result::Result<Oid, Stay>,
}

impl AbsorbHunk {
    pub fn removed_text(&self) -> Vec<String> {
        text_lines(&self.removed)
    }

    pub fn added_text(&self) -> Vec<String> {
        text_lines(&self.added)
    }
}

fn text_lines(lines: &[Vec<u8>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            let s = crate::lossy(l);
            s.trim_end_matches(['\n', '\r']).to_owned()
        })
        .collect()
}

/// What absorbing would do.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Only the staged changes are absorbed (something was staged).
    pub staged: bool,
    pub head: Oid,
    /// Commits of the current branch that can receive changes, newest first.
    pub commits: Vec<CommitInfo>,
    pub hunks: Vec<AbsorbHunk>,
}

impl Plan {
    /// Target commits, oldest first.
    pub fn targets(&self) -> Vec<Oid> {
        let used: HashSet<Oid> = self.hunks.iter().filter_map(|h| h.target.ok()).collect();
        self.commits
            .iter()
            .rev()
            .map(|c| c.id)
            .filter(|id| used.contains(id))
            .collect()
    }

    pub fn absorbed(&self) -> usize {
        self.hunks.iter().filter(|h| h.target.is_ok()).count()
    }

    pub fn commit(&self, id: Oid) -> Option<&CommitInfo> {
        self.commits.iter().find(|c| c.id == id)
    }

    /// The commit the autosquash rebase starts after (`None`: the root).
    pub fn base(&self) -> Option<Oid> {
        let oldest = *self.targets().first()?;
        self.commit(oldest)?.parents.first().copied()
    }
}

/// The commits of the current branch that history-rewriting tools may
/// change, newest first: those not on the trunk (or, on the trunk, not
/// pushed), following first parents and stopping at merges.
pub fn branch_commits(repo: &Repo, limit: usize) -> Result<Vec<CommitInfo>> {
    let raw = repo.raw();
    let Some(head) = repo.head_oid() else {
        return Ok(Vec::new());
    };
    let current = repo.head_branch();
    let mut hide = Vec::new();
    for t in crate::stack::trunk_names(repo) {
        if current.as_deref() == Some(t.as_str()) {
            continue;
        }
        for r in [
            format!("refs/heads/{t}"),
            format!("refs/remotes/origin/{t}"),
        ] {
            if let Ok(id) = repo.resolve(&r) {
                hide.push(id);
            }
        }
    }
    if hide.is_empty()
        && let Some(b) = &current
        && let Ok(branch) = raw.find_branch(b, git2::BranchType::Local)
        && let Ok(up) = branch.upstream()
        && let Some(id) = up.get().target()
    {
        hide.push(id);
    }
    let mut walk = raw.revwalk()?;
    walk.push(head)?;
    for h in &hide {
        walk.hide(*h)?;
    }
    let mine: HashSet<Oid> = walk.take(10_000).filter_map(|r| r.ok()).collect();
    let mut out = Vec::new();
    let mut at = Some(head);
    while let Some(id) = at {
        if !mine.contains(&id) || out.len() >= limit {
            break;
        }
        let c = raw.find_commit(id)?;
        if c.parent_count() > 1 {
            break;
        }
        at = c.parent_id(0).ok();
        out.push(crate::repo::commit_info(&c));
    }
    Ok(out)
}

/// Works out where each hunk of the local changes goes.
pub fn plan(repo: &Repo) -> Result<Plan> {
    let raw = repo.raw();
    let head_commit = raw.head()?.peel_to_commit()?;
    let head = head_commit.id();
    let tree = head_commit.tree()?;
    let commits = branch_commits(repo, MAX_COMMITS)?;
    let mine: HashSet<Oid> = commits.iter().map(|c| c.id).collect();

    let mut opts = DiffOptions::new();
    opts.context_lines(0)
        .interhunk_lines(0)
        .ignore_submodules(true);
    let index = raw.index()?;
    let staged_diff = raw.diff_tree_to_index(Some(&tree), Some(&index), Some(&mut opts))?;
    let staged = staged_diff.deltas().len() > 0;
    let diff = if staged {
        staged_diff
    } else {
        let mut opts = DiffOptions::new();
        opts.context_lines(0)
            .interhunk_lines(0)
            .ignore_submodules(true)
            .include_untracked(true);
        raw.diff_tree_to_workdir_with_index(Some(&tree), Some(&mut opts))?
    };

    let mut hunks = Vec::new();
    for i in 0..diff.deltas().len() {
        let delta = diff.get_delta(i).expect("index in range");
        let path = delta
            .new_file()
            .path()
            .or(delta.old_file().path())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let whole = |target| AbsorbHunk {
            path: path.clone(),
            old_start: 0,
            old_lines: 0,
            new_start: 0,
            new_lines: 0,
            removed: Vec::new(),
            added: Vec::new(),
            target: Err(target),
        };
        match delta.status() {
            git2::Delta::Added | git2::Delta::Untracked => {
                hunks.push(whole(Stay::NewFile));
                continue;
            }
            git2::Delta::Modified => {}
            _ => {
                hunks.push(whole(Stay::WholeFile));
                continue;
            }
        }
        if delta.flags().is_binary() || delta.old_file().mode() != delta.new_file().mode() {
            hunks.push(whole(Stay::WholeFile));
            continue;
        }
        let Some(patch) = Patch::from_diff(&diff, i)? else {
            hunks.push(whole(Stay::WholeFile));
            continue;
        };
        if patch.delta().flags().is_binary() {
            hunks.push(whole(Stay::WholeFile));
            continue;
        }
        let owners = blame_lines(raw, head, &path)?;
        for h in 0..patch.num_hunks() {
            let (hunk, count) = patch.hunk(h)?;
            let mut removed = Vec::new();
            let mut added = Vec::new();
            for l in 0..count {
                let line = patch.line_in_hunk(h, l)?;
                match line.origin() {
                    '-' => removed.push(line.content().to_vec()),
                    '+' => added.push(line.content().to_vec()),
                    _ => {}
                }
            }
            let base = AbsorbHunk {
                path: path.clone(),
                old_start: hunk.old_start(),
                old_lines: hunk.old_lines(),
                new_start: hunk.new_start(),
                new_lines: hunk.new_lines(),
                removed,
                added,
                target: Err(Stay::NoOwner),
            };
            hunks.extend(assign(base, &owners, &mine));
        }
    }
    Ok(Plan {
        staged,
        head,
        commits,
        hunks,
    })
}

/// The commit that last changed each line of `path` in `head` (index 0 = line 1).
fn blame_lines(raw: &git2::Repository, head: Oid, path: &str) -> Result<Vec<Oid>> {
    let mut opts = BlameOptions::new();
    opts.newest_commit(head);
    let blame = raw.blame_file(Path::new(path), Some(&mut opts))?;
    let mut out = Vec::new();
    for h in blame.iter() {
        for _ in 0..h.lines_in_hunk() {
            out.push(h.final_commit_id());
        }
    }
    Ok(out)
}

/// Decides the target of a hunk, splitting it when it replaces lines of
/// several commits one for one.
fn assign(mut h: AbsorbHunk, owners: &[Oid], mine: &HashSet<Oid>) -> Vec<AbsorbHunk> {
    let owner = |line: u32| owners.get(line as usize - 1).copied();
    if h.old_lines == 0 {
        // Added lines go to the commit of the lines around them.
        let after = h.old_start;
        let near: Vec<Oid> = [after, after + 1]
            .into_iter()
            .filter(|&l| l >= 1)
            .filter_map(owner)
            .collect();
        h.target = match near.first() {
            Some(c) if near.iter().all(|o| o == c) && mine.contains(c) => Ok(*c),
            _ => Err(Stay::NoOwner),
        };
        return vec![h];
    }
    let lines: Vec<Option<Oid>> = (h.old_start..h.old_start + h.old_lines)
        .map(owner)
        .collect();
    let first = lines[0];
    if lines.iter().all(|o| *o == first) {
        h.target = match first {
            Some(c) if mine.contains(&c) => Ok(c),
            Some(_) => Err(Stay::NotOnBranch),
            None => Err(Stay::NoOwner),
        };
        return vec![h];
    }
    if h.old_lines != h.new_lines {
        h.target = Err(Stay::SeveralCommits);
        return vec![h];
    }
    // One for one: each run of lines of one commit is a hunk of its own.
    let mut out = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let mut end = start + 1;
        while end < lines.len() && lines[end] == lines[start] {
            end += 1;
        }
        let target = match lines[start] {
            Some(c) if mine.contains(&c) => Ok(c),
            Some(_) => Err(Stay::NotOnBranch),
            None => Err(Stay::NoOwner),
        };
        out.push(AbsorbHunk {
            path: h.path.clone(),
            old_start: h.old_start + start as u32,
            old_lines: (end - start) as u32,
            new_start: h.new_start + start as u32,
            new_lines: (end - start) as u32,
            removed: h.removed[start..end].to_vec(),
            added: h.added[start..end].to_vec(),
            target,
        });
        start = end;
    }
    out
}

/// `old` with `hunks` (all of one file, positions in `old`) applied.
fn rebuild(old: &[u8], hunks: &[&AbsorbHunk]) -> Result<Vec<u8>> {
    let lines: Vec<&[u8]> = old.split_inclusive(|b| *b == b'\n').collect();
    let mut sorted: Vec<&&AbsorbHunk> = hunks.iter().collect();
    sorted.sort_by_key(|h| h.old_start);
    let mut out = Vec::with_capacity(old.len());
    let mut at = 0usize;
    let changed = || Error::Other("The file changed since the preview.".into());
    for h in sorted {
        // A pure insertion goes after line `old_start`.
        let start = if h.old_lines == 0 {
            h.old_start as usize
        } else {
            h.old_start as usize - 1
        };
        if start < at || start > lines.len() {
            return Err(changed());
        }
        for l in &lines[at..start] {
            out.extend_from_slice(l);
        }
        let end = start + h.old_lines as usize;
        if end > lines.len()
            || lines[start..end] != h.removed.iter().map(Vec::as_slice).collect::<Vec<_>>()[..]
        {
            return Err(changed());
        }
        for a in &h.added {
            out.extend_from_slice(a);
        }
        at = end;
    }
    for l in &lines[at..] {
        out.extend_from_slice(l);
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Meld the fixup commits in right away (`rebase -i --autosquash`).
    pub autosquash: bool,
    /// Move other branches pointing into the rewritten commits (git 2.38+).
    pub update_refs: bool,
}

/// Creates the fixup commits of `plan` (and squashes them in). Returns
/// normally when the autosquash rebase stops for a conflict.
pub fn run(git: &Git, plan: &Plan, opts: &Options) -> Result<()> {
    let repo = Repo::open(git.dir())?;
    if repo.head_oid() != Some(plan.head) {
        return Err(Error::Other(
            "The branch changed since the preview: open Absorb again.".into(),
        ));
    }
    let targets = plan.targets();
    if targets.is_empty() {
        return Err(Error::Other("No change can be absorbed.".into()));
    }
    let head = repo.raw().find_commit(plan.head)?;
    let tree = head.tree()?;
    let saved = if plan.staged {
        Some(git.run(&["write-tree"])?.trim().to_owned())
    } else {
        None
    };
    git.run(&["reset", "-q"])?;
    let mut applied: Vec<&AbsorbHunk> = Vec::new();
    for target in &targets {
        let mine: Vec<&AbsorbHunk> = plan
            .hunks
            .iter()
            .filter(|h| h.target == Ok(*target))
            .collect();
        applied.extend(&mine);
        let mut paths: Vec<&str> = mine.iter().map(|h| h.path.as_str()).collect();
        paths.sort_unstable();
        paths.dedup();
        for path in paths {
            let entry = tree.get_path(Path::new(path))?;
            let old = repo.raw().find_blob(entry.id())?;
            let file: Vec<&AbsorbHunk> =
                applied.iter().copied().filter(|h| h.path == path).collect();
            let content = rebuild(old.content(), &file)?;
            let blob = git.run_with_input(
                &["hash-object", "-w", "--stdin", "--no-filters"],
                Some(&content),
            )?;
            let info = format!("{:o},{},{path}", entry.filemode(), blob.trim());
            git.run(&["update-index", "--cacheinfo", &info])?;
        }
        git.run(&[
            "commit",
            "-q",
            "--no-verify",
            "--fixup",
            &target.to_string(),
        ])?;
    }
    if let Some(tree) = saved {
        git.run(&["read-tree", &tree])?;
    }
    if !opts.autosquash {
        return Ok(());
    }
    let base = plan.base().map(|b| b.to_string());
    let git = git.clone().with_env("GIT_SEQUENCE_EDITOR", ":");
    let mut args = vec!["rebase", "-i", "--autosquash", "--autostash"];
    if opts.update_refs {
        args.push("--update-refs");
    }
    match &base {
        Some(b) => args.push(b),
        None => args.push("--root"),
    }
    match git.run(&args) {
        Ok(_) => Ok(()),
        Err(_) if repo.git_dir().join("rebase-merge").exists() => Ok(()),
        Err(e) => Err(e),
    }
}

/// Counts of a plan per target commit, for summaries.
pub fn per_commit(plan: &Plan) -> HashMap<Oid, usize> {
    let mut out = HashMap::new();
    for h in &plan.hunks {
        if let Ok(id) = h.target {
            *out.entry(id).or_default() += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RepoState;
    use crate::ops::ResetMode;
    use crate::testutil::TestRepo;

    const TEN: &str = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";

    /// main: f.txt with ten lines. feature: A changes line 2, B line 8.
    fn setup() -> TestRepo {
        let t = TestRepo::new();
        t.commit("f.txt", TEN, "base");
        t.git.run(&["checkout", "-q", "-b", "feature"]).unwrap();
        t.commit("f.txt", &TEN.replace("2\n", "two\n"), "A");
        t.commit(
            "f.txt",
            &TEN.replace("2\n", "two\n").replace("8\n", "eight\n"),
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

    fn targets(plan: &Plan) -> Vec<(String, std::result::Result<Oid, Stay>)> {
        plan.hunks
            .iter()
            .map(|h| (h.added_text().join(","), h.target))
            .collect()
    }

    #[test]
    fn absorbs_each_hunk_into_its_commit() {
        let t = setup();
        let (a, b) = (id(&t, "HEAD~1"), id(&t, "HEAD"));
        t.write(
            "f.txt",
            &TEN.replace("2\n", "TWO\n").replace("8\n", "EIGHT\n"),
        );
        let plan = plan(&t.repo()).unwrap();
        assert!(!plan.staged);
        assert_eq!(
            targets(&plan),
            [("TWO".into(), Ok(a)), ("EIGHT".into(), Ok(b))]
        );
        assert_eq!(plan.base(), Some(id(&t, "main")));
        // Only commits of the branch are candidates.
        assert_eq!(plan.commits.len(), 2);

        run(
            &t.git,
            &plan,
            &Options {
                autosquash: true,
                update_refs: false,
            },
        )
        .unwrap();
        assert_eq!(t.repo().state(), RepoState::Clean);
        assert_eq!(log(&t), ["B", "A", "base"]);
        let a_now = t.git.run(&["show", "HEAD~1:f.txt"]).unwrap();
        assert!(
            a_now.contains("TWO\n") && a_now.contains("\n8\n"),
            "{a_now}"
        );
        assert!(t.git.run(&["status", "--porcelain"]).unwrap().is_empty());
    }

    #[test]
    fn splits_a_hunk_touching_lines_of_two_commits() {
        let t = TestRepo::new();
        t.commit("f.txt", TEN, "base");
        t.git.run(&["checkout", "-q", "-b", "feature"]).unwrap();
        t.commit("f.txt", &TEN.replace("4\n", "four\n"), "A");
        t.commit(
            "f.txt",
            &TEN.replace("4\n", "four\n").replace("5\n", "five\n"),
            "B",
        );
        let (a, b) = (id(&t, "HEAD~1"), id(&t, "HEAD"));
        // Lines 4 and 5 replaced one for one: split.
        t.write(
            "f.txt",
            &TEN.replace("4\n", "FOUR\n").replace("5\n", "FIVE\n"),
        );
        let p = plan(&t.repo()).unwrap();
        assert_eq!(
            targets(&p),
            [("FOUR".into(), Ok(a)), ("FIVE".into(), Ok(b))]
        );
        // Three lines for two: cannot be split, stays.
        t.write("f.txt", &TEN.replace("4\n5\n", "x\ny\nz\n"));
        let p = plan(&t.repo()).unwrap();
        assert_eq!(targets(&p), [("x,y,z".into(), Err(Stay::SeveralCommits))]);
        assert!(p.targets().is_empty());
        assert!(run(&t.git, &p, &Options::default()).is_err());

        // The split hunks absorb without a squash: two fixup commits.
        t.write(
            "f.txt",
            &TEN.replace("4\n", "FOUR\n").replace("5\n", "FIVE\n"),
        );
        let p = plan(&t.repo()).unwrap();
        run(&t.git, &p, &Options::default()).unwrap();
        assert_eq!(log(&t), ["fixup! B", "fixup! A", "B", "A", "base"]);
    }

    #[test]
    fn new_files_and_lines_without_owner_stay() {
        let t = setup();
        let a = id(&t, "HEAD~1");
        t.write("new.txt", "new\n");
        // Line 1 comes from main's commit: not a target.
        t.write(
            "f.txt",
            &TEN.replace("1\n", "ONE\n")
                .replace("2\n", "TWO\n")
                .replace("8\n", "eight\n"),
        );
        let p = plan(&t.repo()).unwrap();
        let got = targets(&p);
        assert!(
            got.contains(&("ONE".into(), Err(Stay::NotOnBranch))),
            "{got:?}"
        );
        assert!(got.contains(&("TWO".into(), Ok(a))), "{got:?}");
        let new = p.hunks.iter().find(|h| h.path == "new.txt").unwrap();
        assert_eq!(new.target, Err(Stay::NewFile));

        run(
            &t.git,
            &p,
            &Options {
                autosquash: true,
                update_refs: false,
            },
        )
        .unwrap();
        assert_eq!(log(&t), ["B", "A", "base"]);
        // What could not be absorbed is still a local change.
        let diff = t.git.run(&["diff"]).unwrap();
        assert!(diff.contains("+ONE") && !diff.contains("+TWO"), "{diff}");
        assert!(t.path().join("new.txt").exists());
        let status = t.git.run(&["status", "--porcelain"]).unwrap();
        assert!(status.contains("?? new.txt"), "{status}");
    }

    #[test]
    fn added_lines_go_to_the_commit_around_them() {
        let t = TestRepo::new();
        t.commit("f.txt", "1\n2\n", "base");
        t.git.run(&["checkout", "-q", "-b", "feature"]).unwrap();
        t.commit("f.txt", "1\n2\nfn a\nend a\n", "A");
        let a = id(&t, "HEAD");
        t.write("f.txt", "1\n2\nfn a\nbody\nend a\n");
        let p = plan(&t.repo()).unwrap();
        assert_eq!(targets(&p), [("body".into(), Ok(a))]);
        // Between a line of main and one of A: no owner.
        t.write("f.txt", "1\n2\nbetween\nfn a\nend a\n");
        let p = plan(&t.repo()).unwrap();
        assert_eq!(targets(&p), [("between".into(), Err(Stay::NoOwner))]);
    }

    #[test]
    fn absorbs_only_staged_changes_when_some_are_staged() {
        let t = setup();
        let (a, b) = (id(&t, "HEAD~1"), id(&t, "HEAD"));
        t.write(
            "f.txt",
            &TEN.replace("2\n", "TWO\n").replace("8\n", "eight\n"),
        );
        t.git.run(&["add", "f.txt"]).unwrap();
        t.write("g.txt", "g\n");
        t.git.run(&["add", "g.txt"]).unwrap();
        // Unstaged on top: line 8.
        t.write(
            "f.txt",
            &TEN.replace("2\n", "TWO\n").replace("8\n", "EIGHT\n"),
        );
        let p = plan(&t.repo()).unwrap();
        assert!(p.staged);
        let got = targets(&p);
        assert!(got.contains(&("TWO".into(), Ok(a))), "{got:?}");
        assert!(!got.iter().any(|(_, t)| *t == Ok(b)), "{got:?}");
        run(&t.git, &p, &Options::default()).unwrap();
        assert_eq!(log(&t), ["fixup! A", "B", "A", "base"]);
        // g.txt is still staged; line 8 still unstaged.
        let staged = t.git.run(&["diff", "--cached", "--name-only"]).unwrap();
        assert_eq!(staged.trim(), "g.txt");
        assert!(t.git.run(&["diff"]).unwrap().contains("+EIGHT"));
    }

    #[test]
    fn autosquash_conflict_aborts_and_backup_restores() {
        let t = TestRepo::new();
        t.commit("f.txt", TEN, "base");
        t.git.run(&["checkout", "-q", "-b", "feature"]).unwrap();
        t.commit("f.txt", &TEN.replace("2\n", "two\n"), "A");
        // B changes the line right after A's: the fixup cannot move past it.
        t.commit(
            "f.txt",
            &TEN.replace("2\n", "two\n").replace("3\n", "three\n"),
            "B",
        );
        let before = id(&t, "HEAD");
        let backup = crate::backup::create(&t.git, "absorb-1", &["feature".into()]).unwrap();
        t.write(
            "f.txt",
            &TEN.replace("2\n", "TWO\n").replace("3\n", "three\n"),
        );
        let p = plan(&t.repo()).unwrap();
        assert_eq!(p.absorbed(), 1);
        run(
            &t.git,
            &p,
            &Options {
                autosquash: true,
                update_refs: false,
            },
        )
        .unwrap();
        assert_eq!(t.repo().state(), RepoState::Rebase);
        crate::ops::abort_operation(&t.git, RepoState::Rebase).unwrap();
        assert_eq!(log(&t)[0], "fixup! A");
        // Undo: back to the original commits, the change local again.
        crate::backup::restore(&t.git, &backup, ResetMode::Mixed).unwrap();
        assert_eq!(id(&t, "HEAD"), before);
        assert!(t.read("f.txt").contains("TWO\n"));
        assert!(t.git.run(&["diff"]).unwrap().contains("+TWO"));
    }

    #[test]
    fn rejects_a_stale_plan() {
        let t = setup();
        t.write(
            "f.txt",
            &TEN.replace("2\n", "TWO\n").replace("8\n", "eight\n"),
        );
        let p = plan(&t.repo()).unwrap();
        t.commit("other.txt", "o\n", "C");
        assert!(run(&t.git, &p, &Options::default()).is_err());
    }
}
