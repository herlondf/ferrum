//! Sparse checkout in cone mode: only some folders of a large repository
//! are checked out (files at the root always are).

use std::path::Path;

use crate::cli::Git;
use crate::{Repo, Result, lossy};

/// Most folders listed for the picker.
pub const MAX_FOLDERS: usize = 20_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SparseState {
    pub enabled: bool,
    pub cone: bool,
    /// Folders checked out (cone mode), or the patterns otherwise.
    pub folders: Vec<String>,
}

pub fn state(repo: &Repo) -> Result<SparseState> {
    let cfg = repo.raw().config()?.snapshot()?;
    let on = |key: &str| cfg.get_bool(key).unwrap_or(false);
    let mut s = SparseState {
        enabled: on("core.sparseCheckout"),
        cone: on("core.sparseCheckoutCone"),
        folders: Vec::new(),
    };
    if s.enabled {
        let git = repo.git();
        if let Ok(out) = git.run(&["sparse-checkout", "list"]) {
            s.folders = out
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect();
        }
    }
    Ok(s)
}

/// Every folder of HEAD's tree (`a`, `a/b`…), sorted, at most `MAX_FOLDERS`.
pub fn folders(path: &Path) -> Result<Vec<String>> {
    let repo = git2::Repository::open(path)?;
    let Ok(tree) = repo.head().and_then(|h| h.peel_to_tree()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        if out.len() >= MAX_FOLDERS {
            return git2::TreeWalkResult::Skip;
        }
        if entry.kind() == Some(git2::ObjectType::Tree) {
            out.push(format!("{root}{}", lossy(entry.name_bytes())));
        }
        git2::TreeWalkResult::Ok
    })?;
    out.sort();
    Ok(out)
}

/// Checks out only `folders` (and the files at the root), in cone mode.
pub fn set(git: &Git, folders: &[String]) -> Result<()> {
    let input: String = folders.iter().map(|f| format!("{f}\n")).collect();
    git.run_with_input(
        &["sparse-checkout", "set", "--cone", "--stdin"],
        Some(input.as_bytes()),
    )?;
    Ok(())
}

/// Checks out everything again.
pub fn disable(git: &Git) -> Result<()> {
    git.run(&["sparse-checkout", "disable"])?;
    Ok(())
}

/// Keeps only the outermost folders: in cone mode a folder brings all of
/// its subfolders, so `a` and `a/b` is just `a`.
pub fn outermost(folders: &[String]) -> Vec<String> {
    let mut sorted: Vec<String> = folders
        .iter()
        .map(|f| f.trim_matches('/').to_owned())
        .filter(|f| !f.is_empty())
        .collect();
    sorted.sort();
    sorted.dedup();
    let mut out: Vec<String> = Vec::new();
    for f in sorted {
        if !out.iter().any(|o| f.starts_with(&format!("{o}/"))) {
            out.push(f);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn folders_and_cone_mode() {
        let t = TestRepo::new();
        t.write("root.txt", "r");
        t.write("app/a.txt", "a");
        t.write("app/sub/b.txt", "b");
        t.commit("lib/c.txt", "c", "files");
        assert_eq!(folders(t.path()).unwrap(), ["app", "app/sub", "lib"]);
        let repo = t.repo();
        assert!(!state(&repo).unwrap().enabled);

        set(&t.git, &["lib".into()]).unwrap();
        let s = state(&t.repo()).unwrap();
        assert!(s.enabled && s.cone);
        assert_eq!(s.folders, ["lib"]);
        assert!(t.path().join("root.txt").exists());
        assert!(t.path().join("lib/c.txt").exists());
        assert!(!t.path().join("app/a.txt").exists());

        disable(&t.git).unwrap();
        assert!(t.path().join("app/sub/b.txt").exists());
        assert!(!state(&t.repo()).unwrap().enabled);
    }

    #[test]
    fn outermost_drops_nested_folders() {
        let list = ["a/b".into(), "a".into(), "ab".into(), "c/d/".into()];
        assert_eq!(outermost(&list), ["a", "ab", "c/d"]);
    }
}
