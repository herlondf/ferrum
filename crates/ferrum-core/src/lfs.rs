//! Git LFS: tracked patterns, status, locks. Everything but `tracked_patterns`
//! needs the `git lfs` command.

use std::path::Path;

use serde::Deserialize;

use crate::cli::Git;
use crate::{Error, Result};

/// Whether the `git lfs` command is installed.
pub fn available(git: &Git) -> bool {
    crate::diff::lfs_available(git)
}

/// Sets up the LFS filters and this repository's hooks (`git lfs install`).
/// `local` writes the filters to the repository config instead of the global one.
pub fn install(git: &Git, local: bool) -> Result<()> {
    let mut args = vec!["lfs", "install"];
    if local {
        args.push("--local");
    }
    git.run(&args)?;
    Ok(())
}

/// Patterns handled by LFS, from the top-level `.gitattributes`.
pub fn tracked_patterns(workdir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(workdir.join(".gitattributes")).unwrap_or_default();
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let (pattern, attrs) = split_pattern(l)?;
            attrs
                .split_whitespace()
                .any(|a| a == "filter=lfs")
                .then_some(pattern)
        })
        .collect()
}

/// Splits a `.gitattributes` line into its pattern (unquoted) and attributes.
fn split_pattern(line: &str) -> Option<(String, &str)> {
    if let Some(rest) = line.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((rest[..end].to_owned(), &rest[end + 1..]));
    }
    let (p, attrs) = line.split_once(char::is_whitespace)?;
    Some((p.to_owned(), attrs))
}

pub fn track(git: &Git, pattern: &str) -> Result<()> {
    git.run(&["lfs", "track", "--", pattern.trim()])?;
    Ok(())
}

pub fn untrack(git: &Git, pattern: &str) -> Result<()> {
    git.run(&["lfs", "untrack", "--", pattern.trim()])?;
    Ok(())
}

/// `git lfs status` followed by the files in LFS (`git lfs ls-files`).
pub fn status(git: &Git) -> Result<(String, String)> {
    let status = git.run(&["lfs", "status"])?;
    let files = git.run(&["lfs", "ls-files"])?;
    Ok((status, files))
}

/// A file locked on the LFS server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lock {
    pub id: String,
    pub path: String,
    pub owner: String,
    pub locked_at: String,
}

#[derive(Deserialize)]
struct RawLock {
    #[serde(default)]
    id: String,
    path: String,
    #[serde(default)]
    owner: Option<RawOwner>,
    #[serde(default)]
    locked_at: String,
}

#[derive(Deserialize)]
struct RawOwner {
    #[serde(default)]
    name: String,
}

/// Locks on the server (`git lfs locks --json`).
pub fn locks(git: &Git) -> Result<Vec<Lock>> {
    parse_locks(&git.run(&["lfs", "locks", "--json"])?)
}

fn parse_locks(json: &str) -> Result<Vec<Lock>> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<RawLock> =
        serde_json::from_str(json).map_err(|e| Error::Other(format!("git lfs locks: {e}")))?;
    Ok(raw
        .into_iter()
        .map(|l| Lock {
            id: l.id,
            path: l.path,
            owner: l.owner.map(|o| o.name).unwrap_or_default(),
            locked_at: l.locked_at,
        })
        .collect())
}

pub fn lock(git: &Git, paths: &[String]) -> Result<()> {
    for p in paths {
        git.run(&["lfs", "lock", "--", p])?;
    }
    Ok(())
}

/// What to unlock: a path, or a lock id (from the locks list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unlock {
    Path(String),
    Id(String),
}

/// Releases locks. `force` releases locks held by other people.
pub fn unlock(git: &Git, which: &[Unlock], force: bool) -> Result<()> {
    for u in which {
        let mut args = vec!["lfs".to_owned(), "unlock".to_owned()];
        if force {
            args.push("--force".into());
        }
        match u {
            Unlock::Id(id) => args.push(format!("--id={id}")),
            Unlock::Path(p) => args.extend(["--".into(), p.clone()]),
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        git.run(&refs)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn reads_tracked_patterns() {
        let t = TestRepo::new();
        t.write(
            ".gitattributes",
            "# comment\n*.psd filter=lfs diff=lfs merge=lfs -text\n*.txt text\n\"my file.bin\" filter=lfs -text\n",
        );
        assert_eq!(tracked_patterns(t.path()), ["*.psd", "my file.bin"]);
        assert!(tracked_patterns(&t.path().join("missing")).is_empty());
    }

    #[test]
    fn parses_locks() {
        let json = r#"[{"id":"7","path":"art/a.psd","owner":{"name":"Ana"},"locked_at":"2024-05-01T10:00:00Z"},{"id":"8","path":"b.bin"}]"#;
        let list = parse_locks(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].owner, "Ana");
        assert_eq!(list[0].path, "art/a.psd");
        assert_eq!(list[1].owner, "");
        assert!(parse_locks("").unwrap().is_empty());
        assert!(parse_locks("not json").is_err());
    }

    /// Runs only where git-lfs is installed.
    #[test]
    fn tracks_and_untracks_with_git_lfs() {
        let t = TestRepo::new();
        if !available(&t.git) {
            eprintln!("git-lfs not installed; skipping");
            return;
        }
        install(&t.git, true).unwrap();
        track(&t.git, "*.psd").unwrap();
        assert_eq!(tracked_patterns(t.path()), ["*.psd"]);
        let (status, _) = status(&t.git).unwrap();
        assert!(!status.is_empty());
        untrack(&t.git, "*.psd").unwrap();
        assert!(tracked_patterns(t.path()).is_empty());
    }
}
