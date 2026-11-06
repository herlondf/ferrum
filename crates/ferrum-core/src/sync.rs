//! Talking to remotes: fetch, pull and push with options, fast-forwarding
//! branches that are not checked out, editing and testing remotes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::cli::{CancelToken, Git};
use crate::ops::PullMode;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchOptions {
    /// Remotes to fetch; empty fetches all of them.
    pub remotes: Vec<String>,
    /// Delete remote-tracking branches that are gone on the remote.
    pub prune: bool,
    /// Fetch every tag, not only those pointing into fetched history.
    pub tags: bool,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            remotes: Vec::new(),
            prune: true,
            tags: true,
        }
    }
}

pub fn fetch(git: &Git, opts: &FetchOptions, progress: impl FnMut(&str)) -> Result<()> {
    let mut args = vec!["fetch", "--progress"];
    if opts.prune {
        args.push("--prune");
    }
    if opts.tags {
        args.push("--tags");
    }
    match opts.remotes.len() {
        0 => args.push("--all"),
        1 => {}
        _ => args.push("--multiple"),
    }
    args.extend(opts.remotes.iter().map(String::as_str));
    git.run_streaming(&args, progress)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullOptions {
    pub mode: PullMode,
    /// Stash local changes before and reapply them after (`--autostash`).
    pub autostash: bool,
    /// Pull this remote and branch instead of the upstream.
    pub remote: Option<String>,
    pub branch: Option<String>,
}

impl Default for PullOptions {
    fn default() -> Self {
        Self {
            mode: PullMode::Default,
            autostash: false,
            remote: None,
            branch: None,
        }
    }
}

pub fn pull(git: &Git, opts: &PullOptions, progress: impl FnMut(&str)) -> Result<()> {
    let mut args = vec!["pull", "--progress"];
    match opts.mode {
        PullMode::Default => {}
        PullMode::Merge => args.push("--no-rebase"),
        PullMode::Rebase => args.push("--rebase"),
        PullMode::FastForwardOnly => args.push("--ff-only"),
    }
    if opts.autostash {
        args.push("--autostash");
    }
    if let Some(r) = &opts.remote {
        args.push(r);
        if let Some(b) = &opts.branch {
            args.push(b);
        }
    }
    git.run_streaming(&args, progress)?;
    Ok(())
}

/// A push of any number of branches and tags to one remote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushRequest {
    pub remote: String,
    /// (local branch, remote branch) pairs.
    pub branches: Vec<(String, String)>,
    pub tags: Vec<String>,
    /// Push every tag (`--tags`).
    pub all_tags: bool,
    /// Push annotated tags reachable from the pushed branches (`--follow-tags`).
    pub follow_tags: bool,
    pub set_upstream: bool,
    pub force_with_lease: bool,
    /// All refs are updated, or none (`--atomic`).
    pub atomic: bool,
    /// Refuse to push when submodule commits are not on a remote.
    pub check_submodules: bool,
}

impl PushRequest {
    pub fn args(&self) -> Vec<String> {
        let mut args: Vec<String> = vec!["push".into(), "--progress".into()];
        let flags = [
            (self.set_upstream, "--set-upstream"),
            (self.force_with_lease, "--force-with-lease"),
            (self.follow_tags, "--follow-tags"),
            (self.all_tags, "--tags"),
            (
                self.atomic && self.branches.len() + self.tags.len() > 1,
                "--atomic",
            ),
            (self.check_submodules, "--recurse-submodules=check"),
        ];
        args.extend(flags.iter().filter(|f| f.0).map(|f| f.1.to_owned()));
        args.push(self.remote.clone());
        for (local, remote) in &self.branches {
            args.push(format!("refs/heads/{local}:refs/heads/{remote}"));
        }
        for t in &self.tags {
            args.push(format!("refs/tags/{t}:refs/tags/{t}"));
        }
        args
    }
}

pub fn push(git: &Git, req: &PushRequest, progress: impl FnMut(&str)) -> Result<()> {
    let args = req.args();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    git.run_streaming(&args, progress)?;
    Ok(())
}

/// Submodules whose commit recorded in HEAD is not on any of their remotes,
/// with the number of such commits.
pub fn unpushed_submodules(git: &Git) -> Result<Vec<(String, usize)>> {
    if !git.dir().join(".gitmodules").exists() {
        return Ok(Vec::new());
    }
    let list = git
        .run(&[
            "config",
            "--file",
            ".gitmodules",
            "--get-regexp",
            r"^submodule\..*\.path$",
        ])
        .unwrap_or_default();
    let mut out = Vec::new();
    for path in list.lines().filter_map(|l| l.split_once(' ').map(|p| p.1)) {
        let dir = git.dir().join(path);
        if !dir.join(".git").exists() {
            continue;
        }
        // "160000 commit <sha>\t<path>"
        let entry = git.run(&["ls-tree", "HEAD", "--", path])?;
        let Some(sha) = entry.split_whitespace().nth(2) else {
            continue;
        };
        let sub = Git::new(&dir);
        let count = sub
            .run(&["rev-list", "--count", sha, "--not", "--remotes"])
            .ok()
            .and_then(|c| c.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if count > 0 {
            out.push((path.to_owned(), count));
        }
    }
    Ok(out)
}

/// Moves `branch` forward to `target` without checking it out. Fails when
/// `branch` has commits that `target` does not contain.
pub fn fast_forward(git: &Git, branch: &str, target: &str) -> Result<()> {
    let head = git.run(&["symbolic-ref", "-q", "--short", "HEAD"]).ok();
    if head.as_deref().map(str::trim) == Some(branch) {
        git.run(&["merge", "--ff-only", "-q", target])?;
        return Ok(());
    }
    let local = format!("refs/heads/{branch}");
    if git
        .run(&["merge-base", "--is-ancestor", &local, target])
        .is_err()
    {
        return Err(Error::Other(format!(
            "{branch} cannot be fast-forwarded to {target}: they have diverged"
        )));
    }
    let full = git
        .run(&["rev-parse", "--symbolic-full-name", target])
        .unwrap_or_default();
    let full = full.trim();
    if full.starts_with("refs/") {
        // Like `git fetch . origin/x:x`: refuses branches checked out elsewhere.
        git.run(&["fetch", "-q", ".", &format!("{full}:{local}")])?;
    } else {
        let new = git.run(&["rev-parse", "--verify", &format!("{target}^{{commit}}")])?;
        let old = git.run(&["rev-parse", "--verify", &local])?;
        git.run(&[
            "update-ref",
            "-m",
            &format!("fast-forward to {target}"),
            &local,
            new.trim(),
            old.trim(),
        ])?;
    }
    Ok(())
}

/// Fast-forwards several branches, each to its target. The others are still
/// tried when one fails; the error lists the failures.
pub fn fast_forward_many(git: &Git, pairs: &[(String, String)]) -> Result<()> {
    let errors: Vec<String> = pairs
        .iter()
        .filter_map(|(b, t)| fast_forward(git, b, t).err().map(|e| e.to_string()))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::Other(errors.join("\n")))
    }
}

/// Renames a remote and/or changes its URL.
pub fn edit_remote(git: &Git, old: &str, new: &str, url: &str) -> Result<()> {
    if old != new {
        git.run(&["remote", "rename", old, new])?;
    }
    let current = git.run(&["remote", "get-url", new]).unwrap_or_default();
    if current.trim() != url.trim() {
        git.run(&["remote", "set-url", new, url.trim()])?;
    }
    Ok(())
}

/// Asks a remote (name or URL) for its branches. Returns how many it has.
/// Gives up after `timeout`.
pub fn test_connection(git: &Git, target: &str, timeout: Duration) -> Result<usize> {
    let token = CancelToken::new();
    let parent = git.cancel_token().cloned();
    let done = Arc::new(AtomicBool::new(false));
    let timed_out = Arc::new(AtomicBool::new(false));
    {
        let (token, done, timed_out) = (token.clone(), done.clone(), timed_out.clone());
        let start = Instant::now();
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if parent.as_ref().is_some_and(CancelToken::is_cancelled) {
                    token.cancel();
                    break;
                }
                if start.elapsed() >= timeout {
                    timed_out.store(true, Ordering::Relaxed);
                    token.cancel();
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        });
    }
    let result = git
        .clone()
        .with_cancel(token)
        .run(&["ls-remote", "--heads", target]);
    done.store(true, Ordering::Relaxed);
    match result {
        Ok(out) => Ok(out.lines().filter(|l| !l.trim().is_empty()).count()),
        Err(Error::Cancelled) if timed_out.load(Ordering::Relaxed) => Err(Error::Other(format!(
            "No answer from {target} after {} seconds",
            timeout.as_secs()
        ))),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    /// A repository with a bare remote `origin` that has `main` pushed.
    fn with_remote() -> (TestRepo, tempfile::TempDir) {
        let remote = tempfile::tempdir().unwrap();
        Git::new(remote.path())
            .run(&["init", "-q", "--bare", "-b", "main"])
            .unwrap();
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        let url = remote.path().to_string_lossy().into_owned();
        t.git.run(&["remote", "add", "origin", &url]).unwrap();
        t.git.run(&["push", "-q", "-u", "origin", "main"]).unwrap();
        (t, remote)
    }

    fn rev(git: &Git, r: &str) -> String {
        git.run(&["rev-parse", r]).unwrap().trim().to_owned()
    }

    /// Another clone of `remote` that commits `file` on `branch` and pushes it.
    fn push_from_elsewhere(remote: &std::path::Path, branch: &str, file: &str) {
        let other = tempfile::tempdir().unwrap();
        let url = remote.to_string_lossy().into_owned();
        let g = Git::new(other.path());
        g.run(&["clone", "-q", &url, "."]).unwrap();
        g.run(&["config", "user.name", "O"]).unwrap();
        g.run(&["config", "user.email", "o@example.com"]).unwrap();
        g.run(&["config", "commit.gpgsign", "false"]).unwrap();
        g.run(&["checkout", "-q", "-B", branch, &format!("origin/{branch}")])
            .unwrap();
        std::fs::write(other.path().join(file), "x\n").unwrap();
        g.run(&["add", "-A"]).unwrap();
        g.run(&["commit", "-qm", file]).unwrap();
        g.run(&["push", "-q", "origin", branch]).unwrap();
    }

    #[test]
    fn fetch_prunes_and_takes_tags() {
        let (t, remote) = with_remote();
        let bare = Git::new(remote.path());
        bare.run(&["branch", "gone", "main"]).unwrap();
        bare.run(&["tag", "v1", "main"]).unwrap();
        fetch(&t.git, &FetchOptions::default(), |_| {}).unwrap();
        assert!(t.git.run(&["rev-parse", "origin/gone"]).is_ok());
        assert!(t.git.run(&["rev-parse", "v1"]).is_ok());
        bare.run(&["branch", "-D", "gone"]).unwrap();
        let keep = FetchOptions {
            remotes: vec!["origin".into()],
            prune: false,
            tags: false,
        };
        fetch(&t.git, &keep, |_| {}).unwrap();
        assert!(t.git.run(&["rev-parse", "origin/gone"]).is_ok());
        fetch(&t.git, &FetchOptions::default(), |_| {}).unwrap();
        assert!(
            t.git
                .run(&["rev-parse", "--verify", "origin/gone"])
                .is_err()
        );
    }

    #[test]
    fn pull_with_autostash_keeps_local_changes() {
        let (t, remote) = with_remote();
        push_from_elsewhere(remote.path(), "main", "b.txt");
        t.write("a.txt", "local edit\n");
        let opts = PullOptions {
            mode: PullMode::Rebase,
            autostash: true,
            ..Default::default()
        };
        pull(&t.git, &opts, |_| {}).unwrap();
        assert!(t.path().join("b.txt").exists());
        assert_eq!(t.read("a.txt"), "local edit\n");
        assert!(t.repo().stashes().unwrap().is_empty());
    }

    #[test]
    fn fast_forwards_branches_without_checking_out() {
        let (t, remote) = with_remote();
        t.git
            .run(&["push", "-q", "origin", "main:feature"])
            .unwrap();
        t.git.run(&["fetch", "-q", "origin"]).unwrap();
        t.git
            .run(&["branch", "--track", "feature", "origin/feature"])
            .unwrap();
        push_from_elsewhere(remote.path(), "feature", "f.txt");
        push_from_elsewhere(remote.path(), "main", "m.txt");
        t.git.run(&["fetch", "-q", "origin"]).unwrap();
        let pairs = vec![
            ("feature".to_owned(), "origin/feature".to_owned()),
            ("main".to_owned(), "origin/main".to_owned()),
        ];
        fast_forward_many(&t.git, &pairs).unwrap();
        assert_eq!(rev(&t.git, "feature"), rev(&t.git, "origin/feature"));
        assert_eq!(rev(&t.git, "main"), rev(&t.git, "origin/main"));
        // HEAD (main) was merged, so the working copy has the new file.
        assert!(t.path().join("m.txt").exists());
        assert!(!t.path().join("f.txt").exists());
        // To a plain commit too.
        t.git.run(&["branch", "old", "main~1"]).unwrap();
        let tip = rev(&t.git, "main");
        fast_forward(&t.git, "old", &tip).unwrap();
        assert_eq!(rev(&t.git, "old"), tip);
    }

    #[test]
    fn refuses_to_fast_forward_diverged_branches() {
        let (t, _remote) = with_remote();
        t.git.run(&["branch", "side"]).unwrap();
        t.commit("b.txt", "b\n", "on main");
        t.git.run(&["checkout", "-q", "side"]).unwrap();
        t.commit("c.txt", "c\n", "on side");
        t.git.run(&["checkout", "-q", "main"]).unwrap();
        let before = rev(&t.git, "side");
        let err = fast_forward(&t.git, "side", "main").unwrap_err();
        assert!(err.to_string().contains("diverged"), "{err}");
        assert_eq!(rev(&t.git, "side"), before);
    }

    #[test]
    fn pushes_renamed_branches_atomically_and_tags() {
        let (t, remote) = with_remote();
        let bare = Git::new(remote.path());
        t.git.run(&["branch", "topic"]).unwrap();
        t.git.run(&["tag", "v1"]).unwrap();
        t.git.run(&["tag", "v2"]).unwrap();
        let req = PushRequest {
            remote: "origin".into(),
            branches: vec![
                ("topic".into(), "review/topic".into()),
                ("main".into(), "main".into()),
            ],
            tags: vec!["v1".into()],
            atomic: true,
            set_upstream: true,
            ..Default::default()
        };
        assert!(req.args().contains(&"--atomic".to_owned()));
        push(&t.git, &req, |_| {}).unwrap();
        assert!(bare.run(&["rev-parse", "review/topic"]).is_ok());
        assert!(bare.run(&["rev-parse", "v1"]).is_ok());
        assert!(bare.run(&["rev-parse", "--verify", "v2"]).is_err());
        assert_eq!(
            t.git
                .run(&["rev-parse", "--abbrev-ref", "topic@{upstream}"])
                .unwrap()
                .trim(),
            "origin/review/topic"
        );
        let all = PushRequest {
            remote: "origin".into(),
            all_tags: true,
            ..Default::default()
        };
        push(&t.git, &all, |_| {}).unwrap();
        assert!(bare.run(&["rev-parse", "v2"]).is_ok());
    }

    #[test]
    fn atomic_push_updates_nothing_when_one_ref_is_rejected() {
        let (t, remote) = with_remote();
        push_from_elsewhere(remote.path(), "main", "theirs.txt");
        t.commit("ours.txt", "o\n", "ours");
        t.git.run(&["branch", "extra"]).unwrap();
        let req = PushRequest {
            remote: "origin".into(),
            branches: vec![
                ("extra".into(), "extra".into()),
                ("main".into(), "main".into()),
            ],
            atomic: true,
            ..Default::default()
        };
        assert!(push(&t.git, &req, |_| {}).is_err());
        let bare = Git::new(remote.path());
        assert!(bare.run(&["rev-parse", "--verify", "extra"]).is_err());
    }

    #[test]
    fn finds_unpushed_submodule_commits() {
        let (sub_repo, _sub_remote) = with_remote();
        let (t, _remote) = with_remote();
        let url = sub_repo.path().to_string_lossy().into_owned();
        t.git
            .run(&[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                &url,
                "lib",
            ])
            .unwrap();
        t.git.run(&["commit", "-qm", "add lib"]).unwrap();
        // The new submodule clone has its origin (sub_repo) with that commit.
        assert!(unpushed_submodules(&t.git).unwrap().is_empty());
        let lib = Git::new(t.path().join("lib"));
        lib.run(&["config", "user.name", "T"]).unwrap();
        lib.run(&["config", "user.email", "t@example.com"]).unwrap();
        lib.run(&["config", "commit.gpgsign", "false"]).unwrap();
        std::fs::write(t.path().join("lib/new.txt"), "n\n").unwrap();
        lib.run(&["add", "-A"]).unwrap();
        lib.run(&["commit", "-qm", "local only"]).unwrap();
        t.git.run(&["commit", "-qam", "bump lib"]).unwrap();
        assert_eq!(
            unpushed_submodules(&t.git).unwrap(),
            vec![("lib".to_owned(), 1)]
        );
    }

    #[test]
    fn edits_and_tests_remotes() {
        let (t, remote) = with_remote();
        let url = remote.path().to_string_lossy().into_owned();
        edit_remote(&t.git, "origin", "upstream", &url).unwrap();
        let remotes = t.repo().remotes().unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "upstream");
        assert_eq!(
            test_connection(&t.git, "upstream", Duration::from_secs(20)).unwrap(),
            1
        );
        let missing = remote.path().join("nope").to_string_lossy().into_owned();
        edit_remote(&t.git, "upstream", "upstream", &missing).unwrap();
        assert!(test_connection(&t.git, "upstream", Duration::from_secs(20)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn connection_test_times_out() {
        let t = TestRepo::new();
        let git = t
            .git
            .clone()
            .with_env("GIT_CONFIG_COUNT", "1")
            .with_env("GIT_CONFIG_KEY_0", "protocol.ext.allow")
            .with_env("GIT_CONFIG_VALUE_0", "always");
        let started = Instant::now();
        let err = test_connection(&git, "ext::sleep 30", Duration::from_millis(500)).unwrap_err();
        assert!(err.to_string().contains("No answer"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
