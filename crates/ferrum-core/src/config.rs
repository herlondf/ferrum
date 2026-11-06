//! Global git settings and files handed to external tools.

use std::path::{Path, PathBuf};

use crate::cli::Git;
use crate::{Error, Result};

/// `user.name` and `user.email` from the global (`~/.gitconfig`) config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalIdentity {
    pub name: String,
    pub email: String,
}

/// Reads the global identity. Missing keys are empty.
pub fn global_identity(git: &Git) -> GlobalIdentity {
    let get = |key: &str| {
        git.run(&["config", "--global", "--get", key])
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    GlobalIdentity {
        name: get("user.name"),
        email: get("user.email"),
    }
}

/// Writes the global identity. Empty values remove the key.
pub fn set_global_identity(git: &Git, id: &GlobalIdentity) -> Result<()> {
    for (key, value) in [("user.name", &id.name), ("user.email", &id.email)] {
        let value = value.trim();
        if value.is_empty() {
            // Unset fails when the key is not there; that is fine.
            let _ = git.run(&["config", "--global", "--unset", key]);
        } else {
            git.run(&["config", "--global", key, value])?;
        }
    }
    Ok(())
}

/// The versions of a conflicted file, written to a folder for a merge tool.
#[derive(Debug, Clone)]
pub struct ConflictFiles {
    /// Common ancestor (empty file when the file was added on both sides).
    pub base: PathBuf,
    /// Our version (the current branch).
    pub local: PathBuf,
    /// Their version (the branch being merged).
    pub remote: PathBuf,
    /// The file in the working tree, where the result goes.
    pub merged: PathBuf,
}

/// Writes the base, ours and theirs versions of the conflicted `path`
/// (relative to the repository) into `out_dir`.
pub fn write_conflict_files(repo_dir: &Path, path: &str, out_dir: &Path) -> Result<ConflictFiles> {
    let repo = git2::Repository::open(repo_dir)?;
    let index = repo.index()?;
    let conflict = index
        .conflicts()?
        .filter_map(|c| c.ok())
        .find(|c| {
            [&c.our, &c.their, &c.ancestor]
                .into_iter()
                .flatten()
                .any(|e| e.path == path.as_bytes())
        })
        .ok_or_else(|| Error::Other(format!("{path} has no conflict")))?;
    std::fs::create_dir_all(out_dir)?;
    let file_name = Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
        _ => (file_name.clone(), String::new()),
    };
    let write = |entry: &Option<git2::IndexEntry>, side: &str| -> Result<PathBuf> {
        // Keep the extension so tools pick the right syntax highlighting.
        let target = out_dir.join(format!("{stem}.{side}{ext}"));
        let data = match entry {
            Some(e) => repo.find_blob(e.id)?.content().to_vec(),
            None => Vec::new(),
        };
        std::fs::write(&target, data)?;
        Ok(target)
    };
    Ok(ConflictFiles {
        base: write(&conflict.ancestor, "BASE")?,
        local: write(&conflict.our, "LOCAL")?,
        remote: write(&conflict.their, "REMOTE")?,
        merged: repo_dir.join(path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn edits_the_global_identity() {
        let t = TestRepo::new();
        let cfg = t.path().join("global.gitconfig");
        std::fs::write(&cfg, "").unwrap();
        let git = t
            .git
            .clone()
            .with_env("GIT_CONFIG_GLOBAL", cfg.display().to_string());
        assert_eq!(global_identity(&git), GlobalIdentity::default());
        let id = GlobalIdentity {
            name: "Ada Lovelace".into(),
            email: "ada@example.com".into(),
        };
        set_global_identity(&git, &id).unwrap();
        assert_eq!(global_identity(&git), id);
        assert!(
            std::fs::read_to_string(&cfg)
                .unwrap()
                .contains("Ada Lovelace")
        );
        set_global_identity(
            &git,
            &GlobalIdentity {
                name: "Ada".into(),
                email: String::new(),
            },
        )
        .unwrap();
        assert_eq!(global_identity(&git).email, "");
    }

    #[test]
    fn writes_the_sides_of_a_conflict() {
        let t = TestRepo::new();
        t.commit("a.txt", "base\n", "base");
        t.git.run(&["switch", "-q", "-c", "other"]).unwrap();
        t.commit("a.txt", "theirs\n", "theirs");
        t.git.run(&["switch", "-q", "main"]).unwrap();
        t.commit("a.txt", "ours\n", "ours");
        assert!(t.git.run(&["merge", "other"]).is_err());

        let out = t.path().join(".tmp-merge");
        let files = write_conflict_files(t.path(), "a.txt", &out).unwrap();
        let read = |p: &Path| std::fs::read_to_string(p).unwrap();
        assert_eq!(read(&files.base), "base\n");
        assert_eq!(read(&files.local), "ours\n");
        assert_eq!(read(&files.remote), "theirs\n");
        assert!(files.local.display().to_string().ends_with("a.LOCAL.txt"));
        assert_eq!(files.merged, t.path().join("a.txt"));
        assert!(write_conflict_files(t.path(), "missing.txt", &out).is_err());
    }
}
