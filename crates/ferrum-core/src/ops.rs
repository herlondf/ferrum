//! Operations that change a repository. They all run the `git` command line.

use crate::cli::Git;
use crate::model::*;
use crate::{Oid, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchTarget {
    Index,
    WorkingCopy,
}

/// Applies a patch from [`crate::diff::partial_patch`].
pub fn apply_patch(git: &Git, patch: &str, target: PatchTarget, reverse: bool) -> Result<()> {
    let mut args = vec!["apply", "--whitespace=nowarn", "--recount"];
    if target == PatchTarget::Index {
        args.push("--cached");
    }
    if reverse {
        args.push("--reverse");
    }
    args.push("-");
    git.run_with_input(&args, Some(patch.as_bytes()))?;
    Ok(())
}

fn with_paths<'a>(mut args: Vec<&'a str>, paths: &'a [String]) -> Vec<&'a str> {
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    args
}

pub fn stage(git: &Git, paths: &[String]) -> Result<()> {
    git.run(&with_paths(vec!["add", "-A"], paths))?;
    Ok(())
}

pub fn stage_all(git: &Git) -> Result<()> {
    git.run(&["add", "-A"])?;
    Ok(())
}

pub fn unstage(git: &Git, paths: &[String]) -> Result<()> {
    // `reset` also works before the first commit, unlike `restore --staged`.
    git.run(&with_paths(vec!["reset", "-q"], paths))?;
    Ok(())
}

pub fn unstage_all(git: &Git) -> Result<()> {
    git.run(&["reset", "-q"])?;
    Ok(())
}

/// Throws away working copy changes. Untracked files are deleted.
pub fn discard(git: &Git, files: &[FileChange]) -> Result<()> {
    let (untracked, tracked): (Vec<_>, Vec<_>) =
        files.iter().partition(|f| f.kind == ChangeKind::Untracked);
    if !tracked.is_empty() {
        let paths: Vec<String> = tracked.iter().map(|f| f.path.clone()).collect();
        git.run(&with_paths(vec!["checkout"], &paths))?;
    }
    if !untracked.is_empty() {
        let paths: Vec<String> = untracked.iter().map(|f| f.path.clone()).collect();
        git.run(&with_paths(vec!["clean", "-f", "-q"], &paths))?;
    }
    Ok(())
}

pub fn commit(git: &Git, message: &str, amend: bool) -> Result<()> {
    let mut args = vec!["commit", "-q", "--cleanup=strip", "-F", "-"];
    if amend {
        args.push("--amend");
    }
    git.run_with_input(&args, Some(message.as_bytes()))?;
    Ok(())
}

pub fn fetch(git: &Git, remote: Option<&str>, progress: impl FnMut(&str)) -> Result<()> {
    let mut args = vec!["fetch", "--prune", "--progress", "--tags"];
    match remote {
        Some(r) => args.push(r),
        None => args.push("--all"),
    }
    git.run_streaming(&args, progress)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullMode {
    /// Whatever the repository's `pull.rebase` / `pull.ff` config says.
    Default,
    Merge,
    Rebase,
    FastForwardOnly,
}

pub fn pull(git: &Git, mode: PullMode, progress: impl FnMut(&str)) -> Result<()> {
    let mut args = vec!["pull", "--progress"];
    match mode {
        PullMode::Default => {}
        PullMode::Merge => args.push("--no-rebase"),
        PullMode::Rebase => args.push("--rebase"),
        PullMode::FastForwardOnly => args.push("--ff-only"),
    }
    git.run_streaming(&args, progress)?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct PushOptions {
    pub remote: String,
    /// Local branch to push; it is pushed to a branch with the same name.
    pub branch: String,
    pub set_upstream: bool,
    pub force_with_lease: bool,
    pub tags: bool,
}

pub fn push(git: &Git, opts: &PushOptions, progress: impl FnMut(&str)) -> Result<()> {
    let refspec = format!("refs/heads/{0}:refs/heads/{0}", opts.branch);
    let mut args = vec!["push", "--progress"];
    if opts.set_upstream {
        args.push("--set-upstream");
    }
    if opts.force_with_lease {
        args.push("--force-with-lease");
    }
    if opts.tags {
        args.push("--follow-tags");
    }
    args.push(&opts.remote);
    args.push(&refspec);
    git.run_streaming(&args, progress)?;
    Ok(())
}

pub fn push_tag(git: &Git, remote: &str, tag: &str, progress: impl FnMut(&str)) -> Result<()> {
    let refspec = format!("refs/tags/{tag}");
    git.run_streaming(&["push", "--progress", remote, &refspec], progress)?;
    Ok(())
}

pub fn delete_remote_ref(
    git: &Git,
    remote: &str,
    name: &str,
    progress: impl FnMut(&str),
) -> Result<()> {
    git.run_streaming(&["push", "--progress", remote, "--delete", name], progress)?;
    Ok(())
}

pub fn checkout(git: &Git, branch: &str) -> Result<()> {
    git.run(&["switch", "-q", branch])?;
    Ok(())
}

/// Creates a local branch tracking `remote_branch` (e.g. `origin/feature`) and switches to it.
pub fn checkout_remote(git: &Git, remote_branch: &str, local: &str) -> Result<()> {
    git.run(&["switch", "-q", "-c", local, "--track", remote_branch])?;
    Ok(())
}

pub fn checkout_detached(git: &Git, rev: &str) -> Result<()> {
    git.run(&["switch", "-q", "--detach", rev])?;
    Ok(())
}

pub fn create_branch(git: &Git, name: &str, start: &str, checkout: bool) -> Result<()> {
    if checkout {
        git.run(&["switch", "-q", "-c", name, start])?;
    } else {
        git.run(&["branch", name, start])?;
    }
    Ok(())
}

pub fn rename_branch(git: &Git, old: &str, new: &str) -> Result<()> {
    git.run(&["branch", "-m", old, new])?;
    Ok(())
}

pub fn delete_branch(git: &Git, name: &str, force: bool) -> Result<()> {
    git.run(&["branch", if force { "-D" } else { "-d" }, name])?;
    Ok(())
}

pub fn set_upstream(git: &Git, branch: &str, upstream: &str) -> Result<()> {
    git.run(&["branch", "--set-upstream-to", upstream, branch])?;
    Ok(())
}

/// Moves a branch that is not checked out to another commit.
pub fn move_branch(git: &Git, branch: &str, target: &str) -> Result<()> {
    git.run(&["branch", "-f", branch, target])?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    Default,
    NoFastForward,
    FastForwardOnly,
    Squash,
}

pub fn merge(git: &Git, rev: &str, mode: MergeMode) -> Result<()> {
    let mut args = vec!["merge", "--no-edit"];
    match mode {
        MergeMode::Default => {}
        MergeMode::NoFastForward => args.push("--no-ff"),
        MergeMode::FastForwardOnly => args.push("--ff-only"),
        MergeMode::Squash => args.push("--squash"),
    }
    args.push(rev);
    git.run(&args)?;
    Ok(())
}

pub fn rebase(git: &Git, onto: &str) -> Result<()> {
    git.run(&["rebase", onto])?;
    Ok(())
}

pub fn cherry_pick(git: &Git, id: Oid, is_merge: bool) -> Result<()> {
    let id = id.to_string();
    let mut args = vec!["cherry-pick"];
    if is_merge {
        args.extend(["-m", "1"]);
    }
    args.push(&id);
    git.run(&args)?;
    Ok(())
}

pub fn revert(git: &Git, id: Oid, is_merge: bool) -> Result<()> {
    let id = id.to_string();
    let mut args = vec!["revert", "--no-edit"];
    if is_merge {
        args.extend(["-m", "1"]);
    }
    args.push(&id);
    git.run(&args)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    /// Keep changes staged.
    Soft,
    /// Keep changes in the working copy.
    Mixed,
    /// Discard all changes.
    Hard,
    /// Like hard, but refuses to throw away local changes.
    Keep,
}

pub fn reset(git: &Git, rev: &str, mode: ResetMode) -> Result<()> {
    let flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
        ResetMode::Keep => "--keep",
    };
    git.run(&["reset", "-q", flag, rev])?;
    Ok(())
}

pub fn create_tag(git: &Git, name: &str, rev: &str, message: Option<&str>) -> Result<()> {
    match message.filter(|m| !m.trim().is_empty()) {
        Some(msg) => {
            git.run_with_input(&["tag", "-a", name, rev, "-F", "-"], Some(msg.as_bytes()))?
        }
        None => git.run(&["tag", name, rev])?,
    };
    Ok(())
}

pub fn delete_tag(git: &Git, name: &str) -> Result<()> {
    git.run(&["tag", "-d", name])?;
    Ok(())
}

pub fn stash_push(
    git: &Git,
    message: &str,
    include_untracked: bool,
    staged_only: bool,
) -> Result<()> {
    let mut args = vec!["stash", "push"];
    if include_untracked {
        args.push("--include-untracked");
    }
    if staged_only {
        args.push("--staged");
    }
    if !message.trim().is_empty() {
        args.extend(["-m", message]);
    }
    git.run(&args)?;
    Ok(())
}

pub fn stash_apply(git: &Git, stash: &StashInfo, pop: bool) -> Result<()> {
    git.run(&["stash", if pop { "pop" } else { "apply" }, &stash.refname()])?;
    Ok(())
}

pub fn stash_drop(git: &Git, stash: &StashInfo) -> Result<()> {
    git.run(&["stash", "drop", &stash.refname()])?;
    Ok(())
}

fn state_command(state: RepoState) -> Option<&'static str> {
    match state {
        RepoState::Merge => Some("merge"),
        RepoState::Rebase => Some("rebase"),
        RepoState::CherryPick => Some("cherry-pick"),
        RepoState::Revert => Some("revert"),
        _ => None,
    }
}

/// Continues a paused merge, rebase, cherry-pick or revert.
pub fn continue_operation(git: &Git, state: RepoState) -> Result<()> {
    if let Some(cmd) = state_command(state) {
        git.run(&[cmd, "--continue"])?;
    }
    Ok(())
}

pub fn abort_operation(git: &Git, state: RepoState) -> Result<()> {
    match state {
        RepoState::Bisect => git.run(&["bisect", "reset"])?,
        s => match state_command(s) {
            Some(cmd) => git.run(&[cmd, "--abort"])?,
            None => String::new(),
        },
    };
    Ok(())
}

/// Skips the current commit of a rebase or cherry-pick.
pub fn skip_operation(git: &Git, state: RepoState) -> Result<()> {
    if let Some(cmd) = state_command(state).filter(|c| *c != "merge") {
        git.run(&[cmd, "--skip"])?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Ours,
    Theirs,
}

/// Resolves a whole conflicted file with one side.
pub fn resolve_with(git: &Git, path: &str, side: Side) -> Result<()> {
    let flag = match side {
        Side::Ours => "--ours",
        Side::Theirs => "--theirs",
    };
    match git.run(&["checkout", flag, "--", path]) {
        Ok(_) => {
            git.run(&["add", "--", path])?;
        }
        // The chosen side deleted the file.
        Err(_) => {
            git.run(&["rm", "-q", "--", path])?;
        }
    }
    Ok(())
}

/// Writes the resolved content of a conflicted file and marks it resolved.
pub fn save_resolution(git: &Git, path: &str, content: &str) -> Result<()> {
    std::fs::write(git.dir().join(path), content)?;
    git.run(&["add", "--", path])?;
    Ok(())
}

pub fn add_remote(git: &Git, name: &str, url: &str) -> Result<()> {
    git.run(&["remote", "add", name, url])?;
    Ok(())
}

pub fn remove_remote(git: &Git, name: &str) -> Result<()> {
    git.run(&["remote", "remove", name])?;
    Ok(())
}

pub fn lfs(git: &Git, action: &str, progress: impl FnMut(&str)) -> Result<()> {
    git.run_streaming(&["lfs", action], progress)?;
    Ok(())
}

pub fn submodule_update(git: &Git, progress: impl FnMut(&str)) -> Result<()> {
    git.run_streaming(
        &["submodule", "update", "--init", "--recursive", "--progress"],
        progress,
    )?;
    Ok(())
}

/// Adds a worktree at `path`. With `new_branch`, creates that branch from `rev`.
pub fn worktree_add(git: &Git, path: &str, rev: &str, new_branch: Option<&str>) -> Result<()> {
    let mut args = vec!["worktree", "add"];
    if let Some(b) = new_branch {
        args.extend(["-b", b]);
    }
    args.extend([path, rev]);
    git.run(&args)?;
    Ok(())
}

pub fn worktree_remove(git: &Git, path: &str, force: bool) -> Result<()> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(path);
    git.run(&args)?;
    Ok(())
}

/// Writes the identity into the repository's own config (not the global one).
pub fn set_identity(git: &Git, id: &Identity) -> Result<()> {
    let set = |key: &str, value: &Option<String>| -> Result<()> {
        match value.as_deref().filter(|v| !v.trim().is_empty()) {
            Some(v) => git.run(&["config", "--local", key, v.trim()]).map(drop),
            // Unset is allowed to fail when the key is not there.
            None => git
                .run(&["config", "--local", "--unset", key])
                .map(drop)
                .or(Ok(())),
        }
    };
    set("user.name", &id.name)?;
    set("user.email", &id.email)?;
    set("user.signingkey", &id.signing_key)?;
    set("core.sshCommand", &id.ssh_command)?;
    git.run(&[
        "config",
        "--local",
        "commit.gpgsign",
        if id.gpg_sign { "true" } else { "false" },
    ])?;
    // SSH keys sign with the ssh format.
    let ssh_key = id
        .signing_key
        .as_deref()
        .is_some_and(|k| k.ends_with(".pub") || k.starts_with("ssh-"));
    if id.gpg_sign && ssh_key {
        git.run(&["config", "--local", "gpg.format", "ssh"])?;
    }
    Ok(())
}

pub fn clone(
    parent: &std::path::Path,
    url: &str,
    dir_name: &str,
    progress: impl FnMut(&str),
) -> Result<()> {
    clone_with(&Git::new(parent), url, dir_name, progress)
}

/// Clones into `dir_name` inside `git.dir()`. `git` may carry a log and a
/// cancel token; a cancelled clone removes the folder it had started.
pub fn clone_with(git: &Git, url: &str, dir_name: &str, progress: impl FnMut(&str)) -> Result<()> {
    let target = git.dir().join(dir_name);
    let existed = target.exists();
    let result = git.run_streaming(&["clone", "--progress", url, dir_name], progress);
    if matches!(result, Err(crate::Error::Cancelled)) && !existed {
        // git may still hold files for a moment on Windows; retry briefly.
        for _ in 0..20 {
            if std::fs::remove_dir_all(&target).is_ok() || !target.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    result.map(|_| ())
}

pub fn init(path: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    Git::new(path).run(&["init", "-q"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn commits_and_amends() {
        let t = TestRepo::new();
        t.write("a.txt", "a\n");
        stage_all(&t.git).unwrap();
        commit(&t.git, "first\n\nbody", false).unwrap();
        t.write("b.txt", "b\n");
        stage(&t.git, &["b.txt".into()]).unwrap();
        commit(&t.git, "first, amended", true).unwrap();
        let log = t.git.run(&["log", "--format=%s"]).unwrap();
        assert_eq!(log.trim(), "first, amended");
        assert!(t.repo().status().unwrap().is_empty());
    }

    #[test]
    fn stages_unstages_and_discards() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        t.write("a.txt", "changed\n");
        t.write("new.txt", "new\n");
        stage(&t.git, &["a.txt".into()]).unwrap();
        assert_eq!(t.repo().status().unwrap().staged.len(), 1);
        unstage(&t.git, &["a.txt".into()]).unwrap();
        let st = t.repo().status().unwrap();
        assert!(st.staged.is_empty());
        discard(&t.git, &st.unstaged).unwrap();
        assert!(t.repo().status().unwrap().is_empty());
        assert_eq!(t.read("a.txt"), "a\n");
    }

    #[test]
    fn unstages_before_first_commit() {
        let t = TestRepo::new();
        t.write("a.txt", "a\n");
        stage_all(&t.git).unwrap();
        unstage(&t.git, &["a.txt".into()]).unwrap();
        assert!(t.repo().status().unwrap().staged.is_empty());
    }

    #[test]
    fn branches() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        create_branch(&t.git, "feature", "HEAD", true).unwrap();
        assert_eq!(t.repo().head_branch().as_deref(), Some("feature"));
        checkout(&t.git, "main").unwrap();
        rename_branch(&t.git, "feature", "feat").unwrap();
        delete_branch(&t.git, "feat", false).unwrap();
        let names: Vec<_> = t
            .repo()
            .branches()
            .unwrap()
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, ["main"]);
    }

    #[test]
    fn merge_conflict_then_abort() {
        let t = TestRepo::new();
        t.commit("a.txt", "base\n", "base");
        create_branch(&t.git, "other", "HEAD", true).unwrap();
        t.commit("a.txt", "theirs\n", "theirs");
        checkout(&t.git, "main").unwrap();
        t.commit("a.txt", "ours\n", "ours");
        assert!(merge(&t.git, "other", MergeMode::Default).is_err());
        let repo = t.repo();
        assert_eq!(repo.state(), RepoState::Merge);
        assert_eq!(repo.status().unwrap().conflicted.len(), 1);
        let [base, ours, theirs] = repo.conflict_versions("a.txt").unwrap();
        assert_eq!(
            (base.as_deref(), ours.as_deref(), theirs.as_deref()),
            (Some("base\n"), Some("ours\n"), Some("theirs\n"))
        );
        abort_operation(&t.git, RepoState::Merge).unwrap();
        assert_eq!(t.repo().state(), RepoState::Clean);
    }

    #[test]
    fn resolves_conflict_and_continues() {
        let t = TestRepo::new();
        t.commit("a.txt", "base\n", "base");
        create_branch(&t.git, "other", "HEAD", true).unwrap();
        t.commit("a.txt", "theirs\n", "theirs");
        checkout(&t.git, "main").unwrap();
        t.commit("a.txt", "ours\n", "ours");
        let _ = merge(&t.git, "other", MergeMode::Default);
        resolve_with(&t.git, "a.txt", Side::Theirs).unwrap();
        continue_operation(&t.git, RepoState::Merge).unwrap();
        assert_eq!(t.repo().state(), RepoState::Clean);
        assert_eq!(t.read("a.txt"), "theirs\n");
    }

    #[test]
    fn stash_cherry_pick_revert_reset_tags() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        t.write("a.txt", "wip\n");
        stash_push(&t.git, "my stash", false, false).unwrap();
        let stashes = t.repo().stashes().unwrap();
        stash_apply(&t.git, &stashes[0], true).unwrap();
        assert_eq!(t.read("a.txt"), "wip\n");
        assert!(t.repo().stashes().unwrap().is_empty());
        discard(&t.git, &t.repo().status().unwrap().unstaged).unwrap();

        create_branch(&t.git, "other", "HEAD", true).unwrap();
        t.commit("b.txt", "b\n", "add b");
        let picked = t.repo().head_oid().unwrap();
        checkout(&t.git, "main").unwrap();
        cherry_pick(&t.git, picked, false).unwrap();
        assert_eq!(t.read("b.txt"), "b\n");
        let head = t.repo().head_oid().unwrap();
        revert(&t.git, head, false).unwrap();
        assert!(!t.path().join("b.txt").exists());

        reset(&t.git, "HEAD~1", ResetMode::Hard).unwrap();
        assert_eq!(t.read("b.txt"), "b\n");

        create_tag(&t.git, "v1", "HEAD", Some("release")).unwrap();
        create_tag(&t.git, "light", "HEAD", None).unwrap();
        let tags = t.repo().tags().unwrap();
        assert_eq!(tags.len(), 2);
        assert!(tags.iter().any(|t| t.name == "v1" && t.annotated));
        delete_tag(&t.git, "light").unwrap();
        assert_eq!(t.repo().tags().unwrap().len(), 1);
    }

    #[test]
    fn fetch_pull_push_with_local_remote() {
        let remote = tempfile::tempdir().unwrap();
        Git::new(remote.path())
            .run(&["init", "-q", "--bare", "-b", "main"])
            .unwrap();
        let url = remote.path().to_string_lossy().into_owned();

        let a = TestRepo::new();
        a.commit("a.txt", "a\n", "first");
        add_remote(&a.git, "origin", &url).unwrap();
        let opts = PushOptions {
            remote: "origin".into(),
            branch: "main".into(),
            set_upstream: true,
            ..Default::default()
        };
        let mut lines = 0;
        push(&a.git, &opts, |_| lines += 1).unwrap();

        // A second clone pushes a commit; the first one pulls it.
        let parent = tempfile::tempdir().unwrap();
        clone(parent.path(), &url, "b", |_| {}).unwrap();
        let b = Git::new(parent.path().join("b"));
        b.run(&["config", "user.name", "B"]).unwrap();
        b.run(&["config", "user.email", "b@example.com"]).unwrap();
        std::fs::write(parent.path().join("b/b.txt"), "b\n").unwrap();
        stage_all(&b).unwrap();
        commit(&b, "from b", false).unwrap();
        push(
            &b,
            &PushOptions {
                remote: "origin".into(),
                branch: "main".into(),
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();

        fetch(&a.git, None, |_| {}).unwrap();
        let main = a
            .repo()
            .branches()
            .unwrap()
            .into_iter()
            .find(|b| b.name == "main")
            .unwrap();
        assert_eq!(main.ahead_behind, Some((0, 1)));
        pull(&a.git, PullMode::FastForwardOnly, |_| {}).unwrap();
        assert_eq!(a.read("b.txt"), "b\n");
    }

    #[test]
    fn identity_is_written_locally() {
        let t = TestRepo::new();
        let id = Identity {
            name: Some("Work Name".into()),
            email: Some("work@example.com".into()),
            ..Default::default()
        };
        set_identity(&t.git, &id).unwrap();
        let got = t.repo().identity();
        assert_eq!(got.name.as_deref(), Some("Work Name"));
        assert_eq!(got.email.as_deref(), Some("work@example.com"));
    }

    #[test]
    fn worktrees() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        let wt = tempfile::tempdir().unwrap();
        let path = wt.path().join("wt").to_string_lossy().into_owned();
        worktree_add(&t.git, &path, "HEAD", Some("wt-branch")).unwrap();
        let list = t.repo().worktrees().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].branch.as_deref(), Some("wt-branch"));
        worktree_remove(&t.git, &path, false).unwrap();
        assert_eq!(t.repo().worktrees().unwrap().len(), 1);
    }

    /// A clone stuck on a slow remote is cancelled and its folder removed.
    #[cfg(unix)]
    #[test]
    fn cancelled_clone_removes_the_folder() {
        let parent = tempfile::tempdir().unwrap();
        let cancel = crate::cli::CancelToken::new();
        // The ext:: transport runs a command that never answers.
        let git = Git::new(parent.path())
            .with_env("GIT_CONFIG_COUNT", "1")
            .with_env("GIT_CONFIG_KEY_0", "protocol.ext.allow")
            .with_env("GIT_CONFIG_VALUE_0", "always")
            .with_cancel(cancel.clone());
        let target = parent.path().join("slow");
        let watch = target.clone();
        let c = cancel.clone();
        std::thread::spawn(move || {
            for _ in 0..100 {
                if watch.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
            c.cancel();
        });
        let err = clone_with(&git, "ext::sleep 30", "slow", |_| {}).unwrap_err();
        assert!(matches!(err, crate::Error::Cancelled), "{err}");
        assert!(!target.exists());
    }
}
