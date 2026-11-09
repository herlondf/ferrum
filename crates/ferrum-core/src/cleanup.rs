//! Repository cleanup: the largest files in history, removing paths from
//! history (with git filter-repo), merged branches, `git gc` and sizes.
//!
//! Rewriting history never runs without a backup: the branch tips are saved
//! under `refs/ferrum/backup/filter-<secs>/` (see [`crate::backup`]) and,
//! optionally, everything goes into a bundle file first. Only local branches
//! and tags are rewritten (`--refs`), so the backup refs, restore points and
//! remote-tracking branches keep the old history until they are removed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cli::Git;
use crate::{Error, Oid, Repo, Result, backup, lossy};

/// A path in history and the largest version of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFile {
    pub path: String,
    /// Size of the largest version (bytes).
    pub size: u64,
    /// The largest version.
    pub blob: String,
    /// Different versions in history.
    pub versions: usize,
    /// All versions together (bytes, uncompressed).
    pub total: u64,
    /// The path exists in HEAD.
    pub in_head: bool,
    /// Commits that changed the path (filled for the listed ones).
    pub commits: usize,
}

/// What to do about a large file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advice {
    /// Still used and binary-looking: move it to Git LFS.
    Lfs,
    /// No longer in HEAD: it only makes clones bigger; remove it from history.
    Remove,
    /// Large text that is still used: probably fine.
    Keep,
}

const BINARY_EXTENSIONS: &[&str] = &[
    "7z", "a", "ai", "apk", "avi", "bin", "bmp", "class", "dat", "db", "dll", "dmg", "doc", "docx",
    "dylib", "exe", "flac", "gif", "gz", "ico", "iso", "jar", "jpeg", "jpg", "lib", "mkv", "mov",
    "mp3", "mp4", "o", "obj", "otf", "pdf", "png", "ppt", "pptx", "psd", "rar", "so", "sqlite",
    "tar", "tgz", "tif", "tiff", "ttf", "wasm", "wav", "webm", "webp", "woff", "woff2", "xls",
    "xlsx", "xz", "zip", "zst",
];

impl BigFile {
    pub fn advice(&self) -> Advice {
        if !self.in_head {
            Advice::Remove
        } else if looks_binary(&self.path) && self.size >= 256 * 1024
            || self.size >= 5 * 1024 * 1024
        {
            Advice::Lfs
        } else {
            Advice::Keep
        }
    }
}

pub fn looks_binary(path: &str) -> bool {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .is_some_and(|e| BINARY_EXTENSIONS.contains(&e.as_str()))
}

/// The `limit` paths with the largest versions anywhere in history
/// (`git rev-list --objects --all` + `git cat-file --batch-check`).
pub fn largest_files(git: &Git, limit: usize) -> Result<Vec<BigFile>> {
    let objects = git.run(&["rev-list", "--objects", "--all"])?;
    let mut paths: HashMap<&str, &str> = HashMap::new();
    let mut input = String::new();
    for line in objects.lines() {
        if let Some((oid, path)) = line.split_once(' ')
            && !path.is_empty()
        {
            paths.insert(oid, path);
            input.push_str(oid);
            input.push('\n');
        }
    }
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let sizes = git.run_with_input(
        &[
            "cat-file",
            "--batch-check=%(objecttype) %(objectname) %(objectsize)",
        ],
        Some(input.as_bytes()),
    )?;
    let mut by_path: HashMap<&str, BigFile> = HashMap::new();
    for line in sizes.lines() {
        let mut it = line.split(' ');
        let (Some("blob"), Some(oid), Some(size)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let Ok(size) = size.parse::<u64>() else {
            continue;
        };
        let Some(&path) = paths.get(oid) else {
            continue;
        };
        let f = by_path.entry(path).or_insert_with(|| BigFile {
            path: path.to_owned(),
            size: 0,
            blob: String::new(),
            versions: 0,
            total: 0,
            in_head: false,
            commits: 0,
        });
        f.versions += 1;
        f.total += size;
        if size > f.size {
            f.size = size;
            f.blob = oid.to_owned();
        }
    }
    let mut list: Vec<BigFile> = by_path.into_values().collect();
    list.sort_by(|a, b| b.size.cmp(&a.size).then(a.path.cmp(&b.path)));
    list.truncate(limit);
    let repo = Repo::open(git.dir())?;
    let head_tree = repo.raw().head().ok().and_then(|h| h.peel_to_tree().ok());
    for f in &mut list {
        f.in_head = head_tree
            .as_ref()
            .is_some_and(|t| t.get_path(Path::new(&f.path)).is_ok());
        let n = git.run(&["rev-list", "--all", "--count", "--", &f.path])?;
        f.commits = n.trim().parse().unwrap_or(0);
    }
    Ok(list)
}

/// `git lfs migrate` for files like `path` (by extension when it has one).
pub fn lfs_pattern(path: &str) -> String {
    match Path::new(path).extension() {
        Some(e) => format!("*.{}", e.to_string_lossy()),
        None => path.to_owned(),
    }
}

/// Whether `git filter-repo` can be run.
pub fn filter_repo_available(git: &Git) -> bool {
    git.run(&["filter-repo", "--version"]).is_ok()
}

/// The commands to run by hand when git filter-repo is not installed.
pub fn guided_commands(paths: &[String], bundle: &str) -> String {
    let quoted: Vec<String> = paths.iter().map(|p| shell_quote(p)).collect();
    let filter_paths: Vec<String> = quoted.iter().map(|p| format!("--path {p}")).collect();
    let rm = quoted.join(" ");
    format!(
        "# 1. Back up everything\n\
         git bundle create {bundle} --all\n\n\
         # 2a. With git filter-repo (pip install git-filter-repo)\n\
         git filter-repo --force --invert-paths {}\n\n\
         # 2b. Or with git itself (slower)\n\
         git filter-branch --force --prune-empty --index-filter \\\n  \
         'git rm -r -q --cached --ignore-unmatch -- {rm}' -- --branches --tags\n\n\
         # 3. Publish: this replaces the history on the server\n\
         git push --force-with-lease --all\n\
         git push --force-with-lease --tags",
        filter_paths.join(" "),
        bundle = shell_quote(bundle),
    )
}

fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./*".contains(c))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// How a rewrite went: where the backups are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewritten {
    pub backup: String,
    pub bundle: Option<PathBuf>,
}

/// A bundle path for a backup taken now, in the repository's git folder.
pub fn bundle_path(repo: &Repo) -> PathBuf {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    repo.common_dir()
        .join("ferrum-backups")
        .join(format!("before-rewrite-{secs}.bundle"))
}

/// Removes `paths` from the history of every local branch and tag with git
/// filter-repo. Refuses when there are uncommitted changes. Saves backup
/// refs first, and a bundle of everything when `bundle` is given.
pub fn remove_paths(
    git: &Git,
    paths: &[String],
    bundle: Option<&Path>,
    progress: &dyn Fn(&str),
) -> Result<Rewritten> {
    if paths.is_empty() {
        return Err(Error::Other("No paths to remove.".into()));
    }
    let status = git.run(&["status", "--porcelain", "--untracked-files=no"])?;
    if !status.trim().is_empty() {
        return Err(Error::Other(
            "Commit or stash your local changes before rewriting history.".into(),
        ));
    }
    if !filter_repo_available(git) {
        return Err(Error::Other("git filter-repo is not installed.".into()));
    }
    let refs = git.run(&[
        "for-each-ref",
        "--format=%(refname)",
        "refs/heads",
        "refs/tags",
    ])?;
    let refs: Vec<&str> = refs.lines().filter(|l| !l.is_empty()).collect();
    if refs.is_empty() {
        return Err(Error::Other("There is no history to rewrite.".into()));
    }
    let branches: Vec<String> = refs
        .iter()
        .filter_map(|r| r.strip_prefix("refs/heads/"))
        .map(str::to_owned)
        .collect();
    progress("backup");
    let id = backup::create(git, &backup::new_id("filter"), &branches)?;
    if let Some(b) = bundle {
        if let Some(dir) = b.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let b = b.to_string_lossy();
        git.run(&["bundle", "create", &b, "--all"])?;
    }
    let mut args: Vec<&str> = vec!["filter-repo", "--force", "--invert-paths"];
    for p in paths {
        args.push("--path");
        args.push(p);
    }
    args.push("--refs");
    args.extend(refs.iter().copied());
    git.run_streaming(&args, progress)?;
    // With --refs filter-repo may leave the index at the old commit: match
    // the new HEAD (files still on disk stay, untracked).
    if git.run(&["rev-parse", "--verify", "-q", "HEAD"]).is_ok() {
        git.run(&["reset", "-q", "--mixed", "HEAD"])?;
    }
    Ok(Rewritten {
        backup: id,
        bundle: bundle.map(Path::to_path_buf),
    })
}

/// A branch whose commits are all in the base branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    /// Short name (`topic`, or `feature/x` without the remote).
    pub name: String,
    /// `Some(remote)` for a remote branch.
    pub remote: Option<String>,
    pub tip: Oid,
    /// Time of the tip commit.
    pub time: i64,
    /// Points at the base itself (no commits of its own yet).
    pub at_base: bool,
}

impl Merged {
    pub fn full_name(&self) -> String {
        match &self.remote {
            Some(r) => format!("{r}/{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// The branch merged branches are compared with: the remote's default
/// branch, else main/master/develop/trunk, else the current branch.
pub fn default_base(repo: &Repo) -> Option<String> {
    let raw = repo.raw();
    crate::stack::trunk_names(repo)
        .into_iter()
        .find(|n| raw.find_branch(n, git2::BranchType::Local).is_ok())
        .or_else(|| repo.head_branch())
}

/// Local and remote branches fully merged into `base` (a branch or any
/// revision). The base, the current branch and `<remote>/HEAD` are left out,
/// and so are remote branches named like the base.
pub fn merged_branches(repo: &Repo, base: &str) -> Result<Vec<Merged>> {
    let raw = repo.raw();
    let base_id = raw.revparse_single(base)?.peel_to_commit()?.id();
    let base_short = base.rsplit('/').next().unwrap_or(base);
    let head = repo.head_branch();
    let mut out = Vec::new();
    for b in raw.branches(None)? {
        let (b, kind) = b?;
        let Some(tip) = b.get().target() else {
            continue;
        };
        let full = lossy(b.get().name_bytes());
        let (remote, name) = match kind {
            git2::BranchType::Local => {
                let name = full.trim_start_matches("refs/heads/").to_owned();
                if name == base || Some(&name) == head.as_ref() {
                    continue;
                }
                (None, name)
            }
            git2::BranchType::Remote => {
                let rest = full.trim_start_matches("refs/remotes/");
                let Some((r, n)) = rest.split_once('/') else {
                    continue;
                };
                if n == "HEAD" || n == base_short || rest == base {
                    continue;
                }
                (Some(r.to_owned()), n.to_owned())
            }
        };
        let merged = tip == base_id || raw.graph_descendant_of(base_id, tip)?;
        if !merged {
            continue;
        }
        let time = raw
            .find_commit(tip)
            .map(|c| c.time().seconds())
            .unwrap_or(0);
        out.push(Merged {
            name,
            remote,
            tip,
            time,
            at_base: tip == base_id,
        });
    }
    out.sort_by(|a, b| {
        a.remote
            .is_some()
            .cmp(&b.remote.is_some())
            .then(a.full_name().cmp(&b.full_name()))
    });
    Ok(out)
}

/// Deletes merged branches: local ones with `git branch -D`, remote ones
/// with one `git push --delete` per remote.
pub fn delete_branches(git: &Git, branches: &[Merged], progress: &dyn Fn(&str)) -> Result<()> {
    let local: Vec<&str> = branches
        .iter()
        .filter(|b| b.remote.is_none())
        .map(|b| b.name.as_str())
        .collect();
    if !local.is_empty() {
        let mut args = vec!["branch", "-D", "--"];
        args.extend(local);
        git.run(&args)?;
    }
    let mut remotes: Vec<&str> = branches
        .iter()
        .filter_map(|b| b.remote.as_deref())
        .collect();
    remotes.sort_unstable();
    remotes.dedup();
    for r in remotes {
        let mut args = vec!["push", "--progress", r, "--delete"];
        args.extend(
            branches
                .iter()
                .filter(|b| b.remote.as_deref() == Some(r))
                .map(|b| b.name.as_str()),
        );
        git.run_streaming(&args, progress)?;
    }
    Ok(())
}

/// Bytes used by the object database (loose objects and packs).
pub fn object_size(git: &Git) -> Result<u64> {
    // Sum the files under objects/ in bytes: `count-objects` rounds to KiB
    // (and reports 0 for small loose objects on Windows).
    let dir = git.run(&["rev-parse", "--git-common-dir"])?;
    let dir = git.dir().join(dir.trim()).join("objects");
    fn walk(p: &std::path::Path) -> u64 {
        let Ok(entries) = std::fs::read_dir(p) else {
            return 0;
        };
        entries
            .flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => walk(&e.path()),
                Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
                _ => 0,
            })
            .sum()
    }
    Ok(walk(&dir))
}

/// `git gc` (with `--aggressive`); returns the sizes before and after.
pub fn gc(git: &Git, aggressive: bool, progress: &dyn Fn(&str)) -> Result<(u64, u64)> {
    let before = object_size(git)?;
    let mut args = vec!["gc"];
    if aggressive {
        args.push("--aggressive");
    }
    git.run_streaming(&args, progress)?;
    Ok((before, object_size(git)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn largest_files_lists_history_blobs() {
        let t = TestRepo::new();
        t.commit("small.txt", "x", "small");
        t.commit("big.bin", &"a".repeat(5000), "big");
        t.commit("big.bin", &"b".repeat(8000), "bigger");
        t.git.run(&["rm", "-q", "big.bin"]).unwrap();
        t.git.run(&["commit", "-q", "-m", "drop"]).unwrap();
        t.commit("mid.png", &"c".repeat(3000), "png");
        let list = largest_files(&t.git, 2).unwrap();
        assert_eq!(list.len(), 2);
        let big = &list[0];
        assert_eq!(big.path, "big.bin");
        assert_eq!(big.size, 8000);
        assert_eq!(big.versions, 2);
        assert_eq!(big.total, 13000);
        assert!(!big.in_head);
        assert_eq!(big.commits, 3);
        assert_eq!(big.advice(), Advice::Remove);
        assert_eq!(list[1].path, "mid.png");
        assert!(list[1].in_head);
        assert_eq!(list[1].advice(), Advice::Keep, "small images are fine");
        let big_png = BigFile {
            size: 300 * 1024,
            ..list[1].clone()
        };
        assert_eq!(big_png.advice(), Advice::Lfs);
        assert_eq!(lfs_pattern("a/b.png"), "*.png");
        assert!(largest_files(&TestRepo::new().git, 5).unwrap().is_empty());
    }

    #[test]
    fn merged_branches_and_deleting_them() {
        let t = TestRepo::new();
        t.commit("a.txt", "a", "base");
        t.git.run(&["branch", "fresh"]).unwrap();
        t.git.run(&["checkout", "-q", "-b", "done"]).unwrap();
        t.commit("b.txt", "b", "done work");
        t.git.run(&["checkout", "-q", "main"]).unwrap();
        t.git
            .run(&["merge", "-q", "--no-ff", "done", "-m", "m"])
            .unwrap();
        t.git.run(&["branch", "same"]).unwrap();
        t.git.run(&["checkout", "-q", "-b", "open"]).unwrap();
        t.commit("c.txt", "c", "open work");
        t.git.run(&["checkout", "-q", "main"]).unwrap();
        let repo = t.repo();
        assert_eq!(default_base(&repo).as_deref(), Some("main"));
        let list = merged_branches(&repo, "main").unwrap();
        let names: Vec<String> = list.iter().map(Merged::full_name).collect();
        assert_eq!(names, ["done", "fresh", "same"]);
        assert!(!list[0].at_base && !list[1].at_base && list[2].at_base);
        delete_branches(&t.git, &list, &|_| {}).unwrap();
        let left = t.git.run(&["branch", "--format=%(refname:short)"]).unwrap();
        assert_eq!(left.lines().collect::<Vec<_>>(), ["main", "open"]);
    }

    #[test]
    fn remote_merged_branches_are_deleted_with_push() {
        let remote = TestRepo::new();
        remote.commit("a.txt", "a", "base");
        remote.git.run(&["branch", "old"]).unwrap();
        let t = TestRepo::new();
        t.git
            .run(&["remote", "add", "origin", &remote.path().to_string_lossy()])
            .unwrap();
        t.git.run(&["fetch", "-q", "origin"]).unwrap();
        t.git
            .run(&["checkout", "-q", "-b", "main", "origin/main"])
            .unwrap();
        let list = merged_branches(&t.repo(), "main").unwrap();
        let names: Vec<String> = list.iter().map(Merged::full_name).collect();
        assert_eq!(names, ["origin/old"]);
        delete_branches(&t.git, &list, &|_| {}).unwrap();
        let left = remote
            .git
            .run(&["branch", "--format=%(refname:short)"])
            .unwrap();
        assert_eq!(left.trim(), "main");
    }

    #[test]
    fn gc_reports_sizes_and_guided_commands_quote_paths() {
        let t = TestRepo::new();
        t.commit("a.txt", &"x".repeat(2000), "a");
        let (before, after) = gc(&t.git, false, &|_| {}).unwrap();
        assert!(before > 0 && after > 0);
        let text = guided_commands(&["my file.bin".into(), "b.zip".into()], "/tmp/x.bundle");
        assert!(text.contains("--path 'my file.bin' --path b.zip"));
        assert!(text.contains("git bundle create /tmp/x.bundle --all"));
        assert!(text.contains("--force-with-lease"));
    }

    #[test]
    fn remove_paths_refuses_local_changes() {
        let t = TestRepo::new();
        t.commit("a.txt", "a", "a");
        t.write("a.txt", "changed");
        let err = remove_paths(&t.git, &["a.txt".into()], None, &|_| {}).unwrap_err();
        assert!(err.to_string().contains("local changes"));
    }

    /// A stand-in `git-filter-repo` that removes the paths with filter-branch,
    /// to check the backup and the arguments (the real tool may be missing).
    #[cfg(unix)]
    #[test]
    fn remove_paths_backs_up_and_rewrites() {
        use std::os::unix::fs::PermissionsExt;
        let t = TestRepo::new();
        t.commit("keep.txt", "k", "keep");
        t.commit("secret.bin", "s", "oops");
        t.commit("keep.txt", "k2", "more");
        t.git.run(&["tag", "v1"]).unwrap();
        let bin = tempfile::tempdir().unwrap();
        let script = bin.path().join("git-filter-repo");
        let args_file = bin.path().join("args");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\necho \"$@\" > '{}'\n\
                 git filter-branch -f --prune-empty --index-filter \
                 'git rm -q --cached --ignore-unmatch secret.bin' --tag-name-filter cat -- --branches --tags >/dev/null 2>&1\n",
                args_file.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!(
            "{}:{}",
            bin.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let git = t.git.clone().with_env("PATH", path);
        let bundle = t.path().join(".git/ferrum-backups/b.bundle");
        let done = remove_paths(&git, &["secret.bin".into()], Some(&bundle), &|_| {}).unwrap();
        assert!(done.backup.starts_with("filter-"));
        assert!(bundle.exists());
        let args = std::fs::read_to_string(&args_file).unwrap();
        assert!(args.starts_with("--force --invert-paths --path secret.bin --refs"));
        assert!(args.contains("refs/heads/main") && args.contains("refs/tags/v1"));
        let rewritten = t
            .git
            .run(&["log", "--format=", "--name-only", "main"])
            .unwrap();
        assert!(!rewritten.contains("secret.bin"));
        // The backup still has the old history.
        let backups = backup::list(&t.repo()).unwrap();
        assert_eq!(backups[0].id, done.backup);
        let old = t
            .git
            .run(&[
                "log",
                "--format=",
                "--name-only",
                &backups[0].refs[0].1.to_string(),
            ])
            .unwrap();
        assert!(old.contains("secret.bin"));
        let staged = t.git.run(&["diff", "--cached", "--name-only"]).unwrap();
        assert!(staged.trim().is_empty(), "{staged}");
    }
}
