//! Finding repositories on disk, summary statistics for the overview, and
//! fixes for common errors (stale `index.lock`, `safe.directory`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::cli::Git;
use crate::{Error, Result, lossy};

/// Folders never worth looking into for repositories.
const SKIP: &[&str] = &["node_modules", "target", "build", "dist", "vendor"];

/// `dir` holds a repository: a working folder with `.git`, or a bare repository.
pub fn is_repository(dir: &Path) -> bool {
    dir.join(".git").exists()
        || (dir.join("HEAD").is_file() && dir.join("objects").is_dir() && dir.join("refs").is_dir())
}

/// Repositories under `root`, looking at most `depth` folders deep (`root`
/// itself is depth 0). Does not look inside repositories or hidden folders.
pub fn find_repositories(root: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, depth, &mut found);
    found.sort();
    found
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if is_repository(dir) {
        found.push(dir.to_path_buf());
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && !SKIP.contains(&name.as_ref())
        })
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for d in dirs {
        walk(&d, depth - 1, found);
    }
}

/// Numbers shown in the repository overview.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// Commits reachable from HEAD.
    pub commits: usize,
    /// Distinct author emails of those commits.
    pub contributors: usize,
    pub local_branches: usize,
    pub remote_branches: usize,
    pub tags: usize,
    /// Size of the git folder in bytes.
    pub git_size: u64,
    /// Time of the first and the last commit (unix seconds).
    pub first_commit: Option<i64>,
    pub last_commit: Option<i64>,
}

/// Counts commits, contributors, refs and the size of the repository. Walks
/// the whole history: call it off the UI thread.
pub fn summary(path: &Path) -> Result<Summary> {
    let repo = git2::Repository::discover(path)?;
    let mut s = Summary::default();
    if let Ok(head) = repo.head().and_then(|h| h.peel_to_commit()) {
        let mut walk = repo.revwalk()?;
        walk.push(head.id())?;
        let mut authors = HashSet::new();
        for id in walk.flatten() {
            let Ok(c) = repo.find_commit(id) else {
                continue;
            };
            s.commits += 1;
            let t = c.time().seconds();
            s.last_commit = Some(s.last_commit.map_or(t, |l| l.max(t)));
            s.first_commit = Some(s.first_commit.map_or(t, |f| f.min(t)));
            authors.insert(lossy(c.author().email_bytes()).to_lowercase());
        }
        s.contributors = authors.len();
    }
    for b in repo.branches(None)?.flatten() {
        match b.1 {
            git2::BranchType::Local => s.local_branches += 1,
            git2::BranchType::Remote => {
                // `origin/HEAD` is not a branch of its own.
                if !b.0.name_bytes().is_ok_and(|n| n.ends_with(b"/HEAD")) {
                    s.remote_branches += 1;
                }
            }
        }
    }
    s.tags = repo.tag_names(None)?.len();
    s.git_size = folder_size(repo.commondir());
    Ok(s)
}

fn folder_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => folder_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// The README of a working folder (README.md first), with its file name.
pub fn readme(workdir: &Path) -> Option<(String, String)> {
    let entries = std::fs::read_dir(workdir).ok()?;
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_lowercase().starts_with("readme"))
        .collect();
    // README.md, then README.markdown, then anything else (README, README.txt…).
    names.sort_by_key(|n| {
        let l = n.to_lowercase();
        (!l.ends_with(".md"), !l.ends_with(".markdown"), l.len())
    });
    let name = names.into_iter().next()?;
    let bytes = std::fs::read(workdir.join(&name)).ok()?;
    Some((name, lossy(&bytes)))
}

// ----- Fixes for common errors -----

/// The lock file named in a git error like "Unable to create
/// '/repo/.git/index.lock': File exists".
pub fn lock_file_in_error(message: &str) -> Option<PathBuf> {
    let end = message.find(".lock'")? + ".lock".len();
    let start = message[..end].rfind('\'')? + 1;
    let path = &message[start..end];
    path.ends_with("index.lock").then(|| PathBuf::from(path))
}

/// The error is git refusing a repository owned by another user.
pub fn is_safe_directory_error(message: &str) -> bool {
    let m = message.to_lowercase();
    m.contains("dubious ownership")
        || m.contains("not owned by current user")
        || m.contains("safe.directory")
}

/// Running git processes (best effort: empty when it cannot be told).
pub fn running_git_processes() -> Vec<u32> {
    #[cfg(target_os = "linux")]
    {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|e| {
                let pid: u32 = e.file_name().to_str()?.parse().ok()?;
                let comm = std::fs::read_to_string(e.path().join("comm")).ok()?;
                (comm.trim() == "git").then_some(pid)
            })
            .collect()
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let out = std::process::Command::new("pgrep")
            .args(["-x", "git"])
            .output();
        out.map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default()
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq git.exe", "/FO", "CSV", "/NH"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output();
        out.map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.split(',').nth(1)?.trim_matches('"').parse().ok())
                .collect()
        })
        .unwrap_or_default()
    }
}

/// Deletes a stale `index.lock`, refusing while a git process is running.
pub fn delete_index_lock(lock: &Path) -> Result<()> {
    if lock.file_name().is_none_or(|n| n != "index.lock") {
        return Err(Error::Other(format!("{} is not an index.lock", lock.display())));
    }
    let running = running_git_processes();
    if !running.is_empty() {
        let pids: Vec<String> = running.iter().map(u32::to_string).collect();
        return Err(Error::Other(format!(
            "A git process is still running (pid {}). Wait for it to finish, or close it, and try again.",
            pids.join(", ")
        )));
    }
    match std::fs::remove_file(lock) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        r => r.map_err(Error::from),
    }
}

/// `git config --global --add safe.directory <path>`.
pub fn add_safe_directory(git: &Git, path: &Path) -> Result<()> {
    // Git compares paths with forward slashes, also on Windows.
    let p = path.to_string_lossy().replace('\\', "/");
    git.run(&["config", "--global", "--add", "safe.directory", &p])
        .map(drop)
}

/// Sets (or with `None` removes) `commit.template` in the repository config.
pub fn set_commit_template(git: &Git, path: Option<&str>) -> Result<()> {
    match path {
        Some(p) => git.run(&["config", "commit.template", p]).map(drop),
        // `--unset` fails when the key is not set.
        None if git.run(&["config", "--local", "--get", "commit.template"]).is_err() => Ok(()),
        None => git
            .run(&["config", "--local", "--unset", "commit.template"])
            .map(drop),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn finds_repositories_to_a_depth() {
        let root = tempfile::tempdir().unwrap();
        let mk = |p: &str| {
            let d = root.path().join(p);
            std::fs::create_dir_all(&d).unwrap();
            Git::new(&d).run(&["init", "-q"]).unwrap();
            d
        };
        let a = mk("a");
        let b = mk("group/b");
        mk("deep/er/still/c");
        mk(".hidden/d");
        // Repositories inside repositories are not listed.
        mk("a/inner");
        std::fs::create_dir_all(root.path().join("empty")).unwrap();
        assert_eq!(find_repositories(root.path(), 2), vec![a.clone(), b.clone()]);
        assert_eq!(find_repositories(root.path(), 1), vec![a]);
        assert_eq!(find_repositories(&b, 0), vec![b]);
        assert_eq!(find_repositories(root.path(), 4).len(), 3);
    }

    #[test]
    fn summarizes_a_repository() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        t.git
            .run(&["-c", "user.email=other@example.com", "commit", "-q", "--allow-empty", "-m", "second"])
            .unwrap();
        t.git.run(&["branch", "topic"]).unwrap();
        t.git.run(&["tag", "v1"]).unwrap();
        let s = summary(t.path()).unwrap();
        assert_eq!(s.commits, 2);
        assert_eq!(s.contributors, 2);
        assert_eq!(s.local_branches, 2);
        assert_eq!(s.remote_branches, 0);
        assert_eq!(s.tags, 1);
        assert!(s.git_size > 0);
        assert!(s.first_commit <= s.last_commit);
        // An empty repository has nothing to count.
        let empty = TestRepo::new();
        assert_eq!(summary(empty.path()).unwrap().commits, 0);
    }

    #[test]
    fn picks_the_readme() {
        let t = TestRepo::new();
        assert_eq!(readme(t.path()), None);
        t.write("README", "plain");
        t.write("README.md", "# Title");
        assert_eq!(
            readme(t.path()),
            Some(("README.md".into(), "# Title".into()))
        );
    }

    #[test]
    fn reads_lock_and_ownership_errors() {
        let msg = "fatal: Unable to create '/home/u/r/.git/index.lock': File exists.\n\nAnother git process seems to be running";
        assert_eq!(
            lock_file_in_error(msg),
            Some(PathBuf::from("/home/u/r/.git/index.lock"))
        );
        assert_eq!(lock_file_in_error("error: could not lock config file"), None);
        assert_eq!(
            lock_file_in_error("Unable to create 'C:/r/.git/refs/heads/x.lock'"),
            None
        );
        assert!(is_safe_directory_error(
            "fatal: detected dubious ownership in repository at '/r'"
        ));
        assert!(is_safe_directory_error(
            "repository path '/r/' is not owned by current user; class=Config (7); code=Owner (-36)"
        ));
        assert!(!is_safe_directory_error("fatal: not a git repository"));
    }

    #[test]
    fn deletes_only_index_locks() {
        let t = TestRepo::new();
        let lock = t.path().join(".git/index.lock");
        std::fs::write(&lock, "").unwrap();
        assert!(delete_index_lock(&t.path().join(".git/config")).is_err());
        // Other tests may be running git right now; then the lock stays.
        match delete_index_lock(&lock) {
            Ok(()) => assert!(!lock.exists()),
            Err(e) => assert!(e.to_string().contains("still running"), "{e}"),
        }
    }

    #[test]
    fn sets_and_unsets_commit_template() {
        let t = TestRepo::new();
        t.write("tpl.txt", "feat: ");
        set_commit_template(&t.git, Some("tpl.txt")).unwrap();
        assert_eq!(t.repo().commit_template().as_deref(), Some("feat:"));
        set_commit_template(&t.git, None).unwrap();
        assert_eq!(t.repo().commit_template(), None);
        // Unsetting twice is fine.
        set_commit_template(&t.git, None).unwrap();
    }
}
