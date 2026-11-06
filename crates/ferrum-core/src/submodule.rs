//! Submodule operations: add, remove, move, change URL, update, discard.

use std::path::{Component, Path, PathBuf};

use crate::cli::Git;
use crate::{Error, Result};

/// Adds a submodule from `url` at `path`, following `branch` when given.
pub fn add(
    git: &Git,
    url: &str,
    path: &str,
    branch: Option<&str>,
    progress: impl FnMut(&str),
) -> Result<()> {
    let mut args = vec!["submodule", "add", "--progress"];
    if let Some(b) = branch.filter(|b| !b.trim().is_empty()) {
        args.extend(["-b", b]);
    }
    args.extend(["--", url, path]);
    git.run_streaming(&args, progress)?;
    Ok(())
}

/// Removes a submodule: deinit, `git rm`, and its clone in `.git/modules`.
pub fn remove(git: &Git, name: &str, path: &str) -> Result<()> {
    git.run(&["submodule", "deinit", "-f", "--", path])?;
    git.run(&["rm", "-f", "-q", "--", path])?;
    if let Some(dir) = modules_dir(git, name)?
        && dir.exists()
    {
        std::fs::remove_dir_all(&dir)?;
    }
    // `git rm` keeps the section in .git/config when deinit already dropped it,
    // but an older git may leave it behind.
    let _ = git.run(&[
        "config",
        "--local",
        "--remove-section",
        &format!("submodule.{name}"),
    ]);
    Ok(())
}

/// Where git keeps the clone of submodule `name`; `None` for odd names.
fn modules_dir(git: &Git, name: &str) -> Result<Option<PathBuf>> {
    let safe = !name.is_empty()
        && Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if !safe {
        return Ok(None);
    }
    let out = git.run(&["rev-parse", "--git-path", &format!("modules/{name}")])?;
    Ok(Some(git.dir().join(out.trim())))
}

/// Moves a submodule to another folder (updates .gitmodules too).
pub fn move_to(git: &Git, from: &str, to: &str) -> Result<()> {
    if let Some(parent) = Path::new(to).parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(git.dir().join(parent))?;
    }
    git.run(&["mv", "--", from, to])?;
    Ok(())
}

/// Changes the URL in .gitmodules and syncs it into the submodule's config.
pub fn set_url(git: &Git, name: &str, path: &str, url: &str) -> Result<()> {
    if url.trim().is_empty() {
        return Err(Error::Other("The URL is empty.".into()));
    }
    git.run(&[
        "config",
        "-f",
        ".gitmodules",
        &format!("submodule.{name}.url"),
        url.trim(),
    ])?;
    git.run(&["add", "--", ".gitmodules"])?;
    git.run(&["submodule", "sync", "--", path])?;
    Ok(())
}

/// How `update` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateOptions {
    /// Clone submodules that are not initialized yet.
    pub init: bool,
    /// Also update submodules inside submodules.
    pub recursive: bool,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            init: true,
            recursive: true,
        }
    }
}

/// Checks out the recorded commit of the submodules at `paths` (all when empty).
pub fn update(
    git: &Git,
    paths: &[String],
    opts: UpdateOptions,
    progress: impl FnMut(&str),
) -> Result<()> {
    let mut args = vec!["submodule", "update", "--progress"];
    if opts.init {
        args.push("--init");
    }
    if opts.recursive {
        args.push("--recursive");
    }
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    git.run_streaming(&args, progress)?;
    Ok(())
}

/// Throws away uncommitted changes in a submodule and checks out the commit
/// the parent records. Untracked files are kept.
pub fn discard(git: &Git, path: &str) -> Result<()> {
    let inner = Git::new(git.dir().join(path));
    inner.run(&["reset", "--hard", "-q"])?;
    git.run(&["submodule", "update", "--init", "--force", "--", path])?;
    Ok(())
}

/// Submodules whose checked-out commit differs from the recorded one, or that
/// are not initialized (`git submodule status`).
pub fn out_of_date(git: &Git) -> Result<Vec<String>> {
    let out = git.run(&["submodule", "status"])?;
    Ok(out
        .lines()
        .filter(|l| l.starts_with('+') || l.starts_with('-'))
        .filter_map(|l| l[1..].split_whitespace().nth(1).map(str::to_owned))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    /// A parent repository with a submodule `sub` cloned from a local folder.
    fn with_submodule() -> (TestRepo, TestRepo, Git) {
        let sub = TestRepo::new();
        sub.commit("s.txt", "1\n", "one");
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "base");
        // Local clones need this since git 2.38.1.
        let git = t
            .git
            .clone()
            .with_env("GIT_CONFIG_COUNT", "1")
            .with_env("GIT_CONFIG_KEY_0", "protocol.file.allow")
            .with_env("GIT_CONFIG_VALUE_0", "always");
        let url = sub.path().to_string_lossy().replace('\\', "/");
        add(&git, &url, "libs/sub", Some("main"), |_| {}).unwrap();
        t.git.run(&["commit", "-q", "-m", "add sub"]).unwrap();
        (t, sub, git)
    }

    fn inner(t: &TestRepo, path: &str) -> Git {
        let g = Git::new(t.path().join(path));
        g.run(&["config", "user.name", "T"]).unwrap();
        g.run(&["config", "user.email", "t@example.com"]).unwrap();
        g.run(&["config", "commit.gpgsign", "false"]).unwrap();
        g
    }

    #[test]
    fn adds_and_reports_status() {
        let (t, _sub, _) = with_submodule();
        let list = t.repo().submodules().unwrap();
        assert_eq!(list.len(), 1);
        let s = &list[0];
        assert_eq!(s.path, "libs/sub");
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert!(s.initialized && !s.moved && !s.dirty && !s.untracked);

        // Uncommitted and untracked changes inside the submodule.
        std::fs::write(t.path().join("libs/sub/s.txt"), "changed\n").unwrap();
        std::fs::write(t.path().join("libs/sub/new.txt"), "x\n").unwrap();
        let s = &t.repo().submodules().unwrap()[0];
        assert!(s.dirty && s.untracked && !s.moved);

        // Discarding resets tracked files and keeps untracked ones.
        discard(&t.git, "libs/sub").unwrap();
        let s = &t.repo().submodules().unwrap()[0];
        assert!(!s.dirty && s.untracked);
        assert_eq!(t.read("libs/sub/s.txt"), "1\n");
    }

    #[test]
    fn moved_submodule_is_out_of_date_until_updated() {
        let (t, _sub, git) = with_submodule();
        let g = inner(&t, "libs/sub");
        std::fs::write(t.path().join("libs/sub/s.txt"), "2\n").unwrap();
        g.run(&["commit", "-q", "-am", "two"]).unwrap();
        assert!(t.repo().submodules().unwrap()[0].moved);
        assert_eq!(out_of_date(&t.git).unwrap(), ["libs/sub"]);
        update(&git, &["libs/sub".into()], UpdateOptions::default(), |_| {}).unwrap();
        assert!(out_of_date(&t.git).unwrap().is_empty());
        assert_eq!(t.read("libs/sub/s.txt"), "1\n");
    }

    #[test]
    fn moves_changes_url_and_removes() {
        let (t, _sub, _) = with_submodule();
        move_to(&t.git, "libs/sub", "deps/sub").unwrap();
        assert!(t.path().join("deps/sub/s.txt").exists());
        assert!(t.read(".gitmodules").contains("path = deps/sub"));
        t.git.run(&["commit", "-q", "-am", "move"]).unwrap();

        let name = t.repo().submodules().unwrap()[0].name.clone();
        set_url(&t.git, &name, "deps/sub", "https://example.com/sub.git").unwrap();
        assert!(
            t.read(".gitmodules")
                .contains("https://example.com/sub.git")
        );
        let synced = inner(&t, "deps/sub")
            .run(&["config", "remote.origin.url"])
            .unwrap();
        assert_eq!(synced.trim(), "https://example.com/sub.git");
        t.git.run(&["commit", "-q", "-am", "url"]).unwrap();

        remove(&t.git, &name, "deps/sub").unwrap();
        assert!(!t.path().join("deps/sub").exists());
        assert!(!t.repo().common_dir().join("modules").join(&name).exists());
        assert!(!t.read(".gitmodules").contains("deps/sub"));
        t.git.run(&["commit", "-q", "-m", "remove"]).unwrap();
        assert!(t.repo().submodules().unwrap().is_empty());
        let config = t.git.run(&["config", "--local", "--list"]).unwrap();
        assert!(!config.contains("submodule."), "{config}");
    }

    #[test]
    fn rejects_unsafe_module_names() {
        let t = TestRepo::new();
        assert!(modules_dir(&t.git, "../x").unwrap().is_none());
        assert!(modules_dir(&t.git, "").unwrap().is_none());
        assert!(modules_dir(&t.git, "a/b").unwrap().is_some());
    }
}
