//! Merge, rebase, cherry-pick and revert with their options, a conflict
//! preview through `git merge-tree`, and what a stopped operation is doing.

use std::path::Path;

use crate::cli::Git;
use crate::model::RepoState;
use crate::ops::MergeMode;
use crate::{Error, Oid, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeOptions {
    pub mode: MergeMode,
    /// Stop before committing (`--no-commit`). Implies a real merge, not a fast-forward.
    pub no_commit: bool,
    pub allow_unrelated: bool,
}

pub fn merge(git: &Git, rev: &str, opts: &MergeOptions) -> Result<()> {
    let mut args = vec!["merge", "--no-edit"];
    match opts.mode {
        MergeMode::Default if opts.no_commit => args.push("--no-ff"),
        MergeMode::Default => {}
        MergeMode::NoFastForward => args.push("--no-ff"),
        MergeMode::FastForwardOnly => args.push("--ff-only"),
        MergeMode::Squash => args.push("--squash"),
    }
    if opts.no_commit && opts.mode != MergeMode::Squash {
        args.push("--no-commit");
    }
    if opts.allow_unrelated {
        args.push("--allow-unrelated-histories");
    }
    args.push(rev);
    git.run(&args)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RebaseOptions {
    /// Stash local changes first and restore them afterwards.
    pub autostash: bool,
    /// Move branches that point at rebased commits along with them.
    pub update_refs: bool,
}

pub fn rebase(git: &Git, onto: &str, opts: &RebaseOptions) -> Result<()> {
    let mut args = vec!["rebase"];
    if opts.autostash {
        args.push("--autostash");
    }
    if opts.update_refs {
        args.push("--update-refs");
    }
    args.push(onto);
    git.run(&args)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickOptions {
    /// Parent number (1-based) of a merge commit to diff against.
    pub mainline: Option<u32>,
    /// Append "(cherry picked from commit …)" (`-x`).
    pub record_origin: bool,
    /// Commit the result; `false` only applies the changes.
    pub commit: bool,
}

impl Default for PickOptions {
    fn default() -> Self {
        Self {
            mainline: None,
            record_origin: false,
            commit: true,
        }
    }
}

/// Cherry-picks `ids` in order (oldest first).
pub fn cherry_pick(git: &Git, ids: &[Oid], opts: &PickOptions) -> Result<()> {
    let ids: Vec<String> = ids.iter().map(Oid::to_string).collect();
    let mainline = opts.mainline.map(|m| m.to_string());
    let mut args = vec!["cherry-pick"];
    if opts.record_origin {
        args.push("-x");
    }
    if !opts.commit {
        args.push("--no-commit");
    }
    if let Some(m) = &mainline {
        args.extend(["-m", m]);
    }
    args.extend(ids.iter().map(String::as_str));
    git.run(&args)?;
    Ok(())
}

pub fn revert(git: &Git, id: Oid, mainline: Option<u32>, commit: bool) -> Result<()> {
    let id = id.to_string();
    let mainline = mainline.map(|m| m.to_string());
    let mut args = vec!["revert", "--no-edit"];
    if !commit {
        args.push("--no-commit");
    }
    if let Some(m) = &mainline {
        args.extend(["-m", m]);
    }
    args.push(&id);
    git.run(&args)?;
    Ok(())
}

/// Recreates the conflict of files that were marked resolved (`git checkout -m`).
pub fn unresolve(git: &Git, paths: &[String]) -> Result<()> {
    let mut args = vec!["checkout", "-m", "--"];
    args.extend(paths.iter().map(String::as_str));
    git.run(&args)?;
    Ok(())
}

/// One entry of the index for a conflicted path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageEntry {
    pub mode: u32,
    pub id: Oid,
    /// 1 = base, 2 = ours, 3 = theirs.
    pub stage: u8,
}

pub const GITLINK: u32 = 0o160000;

/// The conflict stages of `path` in the index.
pub fn stages(git: &Git, path: &str) -> Result<Vec<StageEntry>> {
    let out = git.run(&["ls-files", "-u", "-z", "--", path])?;
    Ok(parse_stages(&out))
}

fn parse_stages(out: &str) -> Vec<StageEntry> {
    out.split('\0')
        .filter_map(|rec| {
            let (info, _path) = rec.split_once('\t')?;
            let mut it = info.split(' ');
            let mode = u32::from_str_radix(it.next()?, 8).ok()?;
            let id = Oid::from_str(it.next()?).ok()?;
            let stage = it.next()?.parse().ok()?;
            Some(StageEntry { mode, id, stage })
        })
        .collect()
}

/// Resolves a submodule conflict by recording the commit of one side
/// (`stage` 2 = ours, 3 = theirs), or removing it when that side deleted it.
pub fn resolve_gitlink(git: &Git, path: &str, stage: u8) -> Result<()> {
    let entry = stages(git, path)?.into_iter().find(|e| e.stage == stage);
    match entry {
        Some(e) => {
            let info = format!("{:o},{},{}", e.mode, e.id, path);
            git.run(&["update-index", "--cacheinfo", &info])?;
        }
        None => {
            git.run(&["rm", "-q", "--cached", "--", path])?;
        }
    }
    Ok(())
}

/// What to do with local changes before a checkout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalChanges {
    /// Leave them; git refuses if they would be overwritten.
    Keep,
    /// Stash them, run the operation, then reapply them.
    Stash,
    /// Throw away changes to tracked files.
    Discard,
}

const STASH_MESSAGE: &str = "Ferrum: local changes before checkout";

/// Runs `op` after handling local changes as asked.
pub fn with_local_changes(
    git: &Git,
    mode: LocalChanges,
    op: impl FnOnce() -> Result<()>,
) -> Result<()> {
    match mode {
        LocalChanges::Keep => op(),
        LocalChanges::Discard => {
            git.run(&["reset", "-q", "--hard"])?;
            op()
        }
        LocalChanges::Stash => {
            let before = git.run(&["stash", "list", "-n1", "--format=%H"])?;
            git.run(&[
                "stash",
                "push",
                "-q",
                "--include-untracked",
                "-m",
                STASH_MESSAGE,
            ])?;
            let after = git.run(&["stash", "list", "-n1", "--format=%H"])?;
            let stashed = after.trim() != before.trim() && !after.trim().is_empty();
            let result = op();
            if stashed {
                // Reapply even when the operation failed, so nothing seems lost.
                if let Err(e) = git.run(&["stash", "pop", "-q"]) {
                    return Err(Error::Other(format!(
                        "Your changes could not be reapplied and were kept in the stash ({STASH_MESSAGE}): {e}"
                    )));
                }
            }
            result
        }
    }
}

/// Paths that differ between the commits `a` and `b` (revisions), to tell
/// whether a checkout would touch locally changed files.
pub fn changed_between(repo_dir: &Path, a: &str, b: &str) -> Result<Vec<String>> {
    let repo = git2::Repository::open(repo_dir)?;
    let tree = |rev: &str| -> Result<git2::Tree<'_>> {
        Ok(repo.revparse_single(rev)?.peel_to_commit()?.tree()?)
    };
    let (ta, tb) = (tree(a)?, tree(b)?);
    let diff = repo.diff_tree_to_tree(Some(&ta), Some(&tb), None)?;
    let mut out = std::collections::BTreeSet::new();
    for d in diff.deltas() {
        for f in [d.old_file(), d.new_file()] {
            if let Some(p) = f.path_bytes() {
                out.insert(crate::lossy(p));
            }
        }
    }
    Ok(out.into_iter().collect())
}

/// Result of a conflict check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    Clean,
    /// Nothing would change.
    UpToDate,
    /// Conflicting files, and the commit being applied when they appear (rebase, cherry-pick).
    Conflicts {
        files: Vec<String>,
        at: Option<Oid>,
    },
    /// The check could not run (for example git older than 2.38).
    Unavailable(String),
}

struct MergeTree {
    tree: String,
    conflicts: Vec<String>,
}

/// Runs `git merge-tree --write-tree` on two commits or trees.
fn merge_tree(
    git: &Git,
    base: Option<&str>,
    ours: &str,
    theirs: &str,
    unrelated: bool,
) -> Result<MergeTree> {
    let base_arg = base.map(|b| format!("--merge-base={b}"));
    let mut args = vec![
        "merge-tree",
        "--write-tree",
        "--name-only",
        "--no-messages",
        "-z",
    ];
    if unrelated {
        args.push("--allow-unrelated-histories");
    }
    if let Some(b) = &base_arg {
        args.push(b);
    }
    args.extend([ours, theirs]);
    let out = git.command(&args).output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    match out.status.code() {
        Some(0) | Some(1) => {
            let mut parts = stdout.split('\0');
            let tree = parts.next().unwrap_or_default().trim().to_owned();
            if Oid::from_str(&tree).is_err() {
                return Err(Error::Cli(
                    String::from_utf8_lossy(&out.stderr).into_owned(),
                ));
            }
            let conflicts = parts
                .take_while(|p| !p.is_empty())
                .map(str::to_owned)
                .collect();
            Ok(MergeTree { tree, conflicts })
        }
        _ => Err(Error::Cli(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        )),
    }
}

fn unavailable(e: Error) -> Preview {
    Preview::Unavailable(e.to_string())
}

fn is_ancestor(git: &Git, a: &str, b: &str) -> bool {
    git.command(&["merge-base", "--is-ancestor", a, b])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Whether merging `rev` into HEAD would conflict.
pub fn preview_merge(git: &Git, rev: &str, unrelated: bool) -> Preview {
    if is_ancestor(git, rev, "HEAD") {
        return Preview::UpToDate;
    }
    match merge_tree(git, None, "HEAD", rev, unrelated) {
        Ok(m) if m.conflicts.is_empty() => Preview::Clean,
        Ok(m) => Preview::Conflicts {
            files: m.conflicts,
            at: None,
        },
        Err(e) => unavailable(e),
    }
}

/// Applies each commit's change in turn on top of `start`, like a cherry-pick
/// (or a revert), and stops at the first conflict.
fn chain(git: &Git, start: &str, ids: &[Oid], mainline: Option<u32>, reverse: bool) -> Preview {
    let mut cur = start.to_owned();
    for id in ids {
        let c = id.to_string();
        let parent = format!("{c}^{}", mainline.unwrap_or(1));
        let (base, theirs) = if reverse {
            (c.as_str(), parent.as_str())
        } else {
            (parent.as_str(), c.as_str())
        };
        match merge_tree(git, Some(base), &cur, theirs, false) {
            Ok(m) if m.conflicts.is_empty() => cur = m.tree,
            Ok(m) => {
                return Preview::Conflicts {
                    files: m.conflicts,
                    at: Some(*id),
                };
            }
            Err(e) => return unavailable(e),
        }
    }
    Preview::Clean
}

/// Whether cherry-picking `ids` (oldest first) onto HEAD would conflict.
pub fn preview_cherry_pick(git: &Git, ids: &[Oid], mainline: Option<u32>) -> Preview {
    chain(git, "HEAD", ids, mainline, false)
}

pub fn preview_revert(git: &Git, id: Oid, mainline: Option<u32>) -> Preview {
    chain(git, "HEAD", &[id], mainline, true)
}

/// Most commits replayed by a rebase preview.
const PREVIEW_LIMIT: usize = 300;

/// Whether rebasing HEAD onto `onto` would conflict.
pub fn preview_rebase(git: &Git, onto: &str) -> Preview {
    let range = format!("{onto}..HEAD");
    let list = match git.run(&["rev-list", "--reverse", "--no-merges", &range]) {
        Ok(l) => l,
        Err(e) => return unavailable(e),
    };
    let ids: Vec<Oid> = list
        .lines()
        .filter_map(|l| Oid::from_str(l.trim()).ok())
        .collect();
    if ids.is_empty() {
        return if is_ancestor(git, onto, "HEAD") {
            Preview::UpToDate
        } else {
            Preview::Clean
        };
    }
    if ids.len() > PREVIEW_LIMIT {
        return Preview::Unavailable(format!("{} commits to replay", ids.len()));
    }
    chain(git, onto, &ids, None, false)
}

/// What a stopped merge, rebase, cherry-pick or revert is doing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InProgress {
    /// The branch being rebased.
    pub head_name: Option<String>,
    /// The commit a rebase replays onto.
    pub onto: Option<Oid>,
    /// The commit being applied (rebase, cherry-pick, revert).
    pub current: Option<Oid>,
    pub merge_heads: Vec<Oid>,
    /// Name of what is merged, from the merge message ("feature" in "Merge branch 'feature'").
    pub merge_name: Option<String>,
    /// Step of a rebase: (done, total).
    pub step: Option<(usize, usize)>,
}

fn read(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn read_oid(dir: &Path, name: &str) -> Option<Oid> {
    read(dir, name).and_then(|s| Oid::from_str(s.lines().next()?.trim()).ok())
}

/// Reads the state files git keeps in `git_dir` during an operation.
pub fn in_progress(git_dir: &Path, state: RepoState) -> InProgress {
    let mut info = InProgress::default();
    match state {
        RepoState::Merge => {
            info.merge_heads = read(git_dir, "MERGE_HEAD")
                .map(|s| {
                    s.lines()
                        .filter_map(|l| Oid::from_str(l.trim()).ok())
                        .collect()
                })
                .unwrap_or_default();
            info.merge_name = read(git_dir, "MERGE_MSG").and_then(|m| merge_name(&m));
        }
        RepoState::Rebase => {
            let (dir, done, total) = if git_dir.join("rebase-merge").is_dir() {
                (git_dir.join("rebase-merge"), "msgnum", "end")
            } else {
                (git_dir.join("rebase-apply"), "next", "last")
            };
            info.head_name = read(&dir, "head-name")
                .map(|h| h.strip_prefix("refs/heads/").unwrap_or(&h).to_owned())
                .filter(|h| h != "detached HEAD");
            info.onto = read_oid(&dir, "onto");
            let num = |n| read(&dir, n).and_then(|s| s.parse::<usize>().ok());
            info.step = num(done).zip(num(total));
            info.current = read_oid(git_dir, "REBASE_HEAD");
        }
        RepoState::CherryPick => info.current = read_oid(git_dir, "CHERRY_PICK_HEAD"),
        RepoState::Revert => info.current = read_oid(git_dir, "REVERT_HEAD"),
        _ => {}
    }
    info
}

/// "feature" from "Merge branch 'feature' into main" (also remote-tracking branches and tags).
fn merge_name(msg: &str) -> Option<String> {
    let first = msg.lines().next()?;
    let start = first.find('\'')? + 1;
    let end = first[start..].find('\'')? + start;
    Some(first[start..end].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    /// main and feature both change a.txt; feature also adds f.txt.
    fn diverged() -> TestRepo {
        let t = TestRepo::new();
        t.commit("a.txt", "one\n", "base");
        t.git.run(&["switch", "-q", "-c", "feature"]).unwrap();
        t.commit("f.txt", "f\n", "add f");
        t.commit("a.txt", "feature\n", "feature a");
        t.git.run(&["switch", "-q", "main"]).unwrap();
        t.commit("a.txt", "main\n", "main a");
        t
    }

    fn merge_tree_works(t: &TestRepo) -> bool {
        merge_tree(&t.git, None, "HEAD", "HEAD", false).is_ok()
    }

    #[test]
    fn previews_conflicts() {
        let t = diverged();
        if !merge_tree_works(&t) {
            eprintln!("git merge-tree --write-tree is not available; skipping");
            return;
        }
        let conflicts = |p: Preview| match p {
            Preview::Conflicts { files, .. } => files,
            other => panic!("expected conflicts, got {other:?}"),
        };
        assert_eq!(
            conflicts(preview_merge(&t.git, "feature", false)),
            ["a.txt"]
        );
        assert_eq!(preview_merge(&t.git, "main~1", false), Preview::UpToDate);

        let f = t.repo().resolve("feature~1").unwrap();
        assert_eq!(preview_cherry_pick(&t.git, &[f], None), Preview::Clean);
        let fa = t.repo().resolve("feature").unwrap();
        let p = preview_cherry_pick(&t.git, &[f, fa], None);
        assert_eq!(
            p,
            Preview::Conflicts {
                files: vec!["a.txt".into()],
                at: Some(fa)
            }
        );
        let head = t.repo().head_oid().unwrap();
        assert_eq!(preview_revert(&t.git, head, None), Preview::Clean);

        t.git.run(&["switch", "-q", "feature"]).unwrap();
        match preview_rebase(&t.git, "main") {
            Preview::Conflicts { files, at } => {
                assert_eq!(files, ["a.txt"]);
                assert_eq!(at, Some(fa));
            }
            Preview::Unavailable(e) => eprintln!("rebase preview unavailable: {e}"),
            other => panic!("expected conflicts, got {other:?}"),
        }
        // Nothing to replay.
        assert_eq!(preview_rebase(&t.git, "feature~1"), Preview::Clean);
        assert_eq!(preview_rebase(&t.git, "feature"), Preview::UpToDate);
    }

    #[test]
    fn merges_with_options() {
        let t = diverged();
        t.git.run(&["switch", "-q", "feature"]).unwrap();
        t.git.run(&["reset", "-q", "--hard", "feature~1"]).unwrap();
        t.git.run(&["switch", "-q", "main"]).unwrap();
        let opts = MergeOptions {
            mode: MergeMode::Default,
            no_commit: true,
            allow_unrelated: false,
        };
        merge(&t.git, "feature", &opts).unwrap();
        assert_eq!(t.repo().state(), RepoState::Merge);
        let info = in_progress(t.repo().git_dir(), RepoState::Merge);
        assert_eq!(info.merge_name.as_deref(), Some("feature"));
        assert_eq!(info.merge_heads, [t.repo().resolve("feature").unwrap()]);

        // Unrelated histories need the option.
        t.git.run(&["merge", "--abort"]).unwrap();
        t.git.run(&["switch", "-q", "--orphan", "other"]).unwrap();
        t.commit("o.txt", "o\n", "other root");
        t.git.run(&["switch", "-q", "main"]).unwrap();
        let mut opts = opts;
        opts.no_commit = false;
        assert!(merge(&t.git, "other", &opts).is_err());
        opts.allow_unrelated = true;
        merge(&t.git, "other", &opts).unwrap();
        assert!(t.path().join("o.txt").exists());
    }

    #[test]
    fn cherry_picks_several_with_origin_and_reverts_without_commit() {
        let t = diverged();
        let f = t.repo().resolve("feature~1").unwrap();
        let opts = PickOptions {
            record_origin: true,
            ..Default::default()
        };
        cherry_pick(&t.git, &[f], &opts).unwrap();
        let msg = t.git.run(&["log", "-1", "--format=%B"]).unwrap();
        assert!(msg.contains(&format!("(cherry picked from commit {f})")));

        let head = t.repo().head_oid().unwrap();
        revert(&t.git, head, None, false).unwrap();
        assert_eq!(t.repo().head_oid(), Some(head), "nothing committed");
        assert!(!t.path().join("f.txt").exists());
    }

    #[test]
    fn rebases_with_autostash_and_update_refs() {
        let t = diverged();
        t.git.run(&["switch", "-q", "feature"]).unwrap();
        t.git.run(&["branch", "part", "feature~1"]).unwrap();
        // Make feature apply cleanly: drop its change to a.txt.
        t.git.run(&["reset", "-q", "--hard", "feature~1"]).unwrap();
        t.commit("g.txt", "g\n", "add g");
        t.write("f.txt", "local\n");
        let opts = RebaseOptions {
            autostash: true,
            update_refs: true,
        };
        rebase(&t.git, "main", &opts).unwrap();
        assert_eq!(t.read("f.txt"), "local\n", "local change restored");
        let main = t.repo().resolve("main").unwrap();
        let part = t.repo().resolve("part").unwrap();
        assert!(t.repo().is_ancestor(main, part), "part moved along");
    }

    #[test]
    fn reads_rebase_progress_and_undoes_a_resolution() {
        let t = diverged();
        t.git.run(&["switch", "-q", "feature"]).unwrap();
        assert!(rebase(&t.git, "main", &RebaseOptions::default()).is_err());
        let repo = t.repo();
        assert_eq!(repo.state(), RepoState::Rebase);
        let info = in_progress(repo.git_dir(), RepoState::Rebase);
        assert_eq!(info.head_name.as_deref(), Some("feature"));
        assert_eq!(info.onto, Some(repo.resolve("main").unwrap()));
        assert_eq!(info.current, Some(repo.resolve("feature").unwrap()));
        assert_eq!(info.step, Some((2, 2)));

        let st = stages(&t.git, "a.txt").unwrap();
        assert_eq!(st.iter().map(|s| s.stage).collect::<Vec<_>>(), [1, 2, 3]);
        crate::ops::resolve_with(&t.git, "a.txt", crate::ops::Side::Theirs).unwrap();
        assert!(stages(&t.git, "a.txt").unwrap().is_empty());
        unresolve(&t.git, &["a.txt".into()]).unwrap();
        assert_eq!(stages(&t.git, "a.txt").unwrap().len(), 3);
        assert!(t.read("a.txt").contains("<<<<<<<"));
    }

    #[test]
    fn stashes_around_a_checkout() {
        let t = diverged();
        t.write("a.txt", "local edit\n");
        // A plain switch refuses: feature changes a.txt too.
        assert!(
            with_local_changes(&t.git, LocalChanges::Keep, || crate::ops::checkout(
                &t.git, "feature"
            ))
            .is_err()
        );
        let r = with_local_changes(&t.git, LocalChanges::Stash, || {
            crate::ops::checkout(&t.git, "feature")
        });
        // Reapplying conflicts with feature's a.txt: the changes stay stashed.
        assert!(r.is_err());
        assert_eq!(t.repo().head_branch().as_deref(), Some("feature"));
        t.git.run(&["reset", "-q", "--hard"]).unwrap();
        assert!(
            t.git
                .run(&["stash", "list"])
                .unwrap()
                .contains(STASH_MESSAGE)
        );

        t.write("f.txt", "edited\n");
        with_local_changes(&t.git, LocalChanges::Discard, || {
            crate::ops::checkout(&t.git, "main")
        })
        .unwrap();
        assert_eq!(t.repo().head_branch().as_deref(), Some("main"));
        assert!(!t.path().join("f.txt").exists());
    }

    #[test]
    fn parses_stage_lines_and_merge_names() {
        let id = "0123456789012345678901234567890123456789";
        let out = format!("160000 {id} 2\tsub\0100644 {id} 3\tsub\0");
        let st = parse_stages(&out);
        assert_eq!(st.len(), 2);
        assert_eq!(st[0].mode, GITLINK);
        assert_eq!(st[1].stage, 3);
        assert_eq!(
            merge_name("Merge branch 'feature' into main\n").as_deref(),
            Some("feature")
        );
        assert_eq!(
            merge_name("Merge remote-tracking branch 'origin/x'").as_deref(),
            Some("origin/x")
        );
        assert_eq!(merge_name("Merge abc123"), None);
    }
}
