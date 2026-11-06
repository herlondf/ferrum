//! More about local changes: ignored files, the amend view, patches, large
//! files, and commits with options (sign-off, skipping hooks, live hook output).

use std::path::Path;

use git2::{AttrCheckFlags, DiffFormat, DiffOptions, Oid};

use crate::cli::Git;
use crate::diff::patch_to_file_diff;
use crate::model::*;
use crate::{Error, Repo, Result, lossy};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommitOptions {
    pub amend: bool,
    /// Adds a `Signed-off-by:` trailer (`--signoff`).
    pub signoff: bool,
    /// Skips the pre-commit and commit-msg hooks (`--no-verify`).
    pub no_verify: bool,
}

impl Repo {
    /// Ignored files and folders (folders are not listed file by file).
    pub fn ignored_files(&self) -> Result<Vec<String>> {
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(false)
            .include_ignored(true)
            .recurse_ignored_dirs(false);
        let statuses = self.raw().statuses(Some(&mut opts))?;
        let mut out: Vec<String> = statuses
            .iter()
            .filter(|e| e.status().is_ignored())
            .map(|e| lossy(e.path_bytes()))
            .collect();
        out.sort();
        Ok(out)
    }

    /// The tree an amended commit is compared with: the parent of HEAD
    /// (`None` when HEAD is the first commit).
    fn amend_base(&self) -> Result<Option<git2::Tree<'_>>> {
        let head = self.raw().head()?.peel_to_commit()?;
        Ok(match head.parent(0) {
            Ok(p) => Some(p.tree()?),
            Err(_) => None,
        })
    }

    fn amend_diff(&self, path: Option<&str>) -> Result<git2::Diff<'_>> {
        let base = self.amend_base()?;
        let mut opts = DiffOptions::new();
        if let Some(p) = path {
            opts.pathspec(p).disable_pathspec_match(true);
        }
        let mut diff = self
            .raw()
            .diff_tree_to_index(base.as_ref(), None, Some(&mut opts))?;
        if path.is_none() {
            diff.find_similar(None)?;
        }
        Ok(diff)
    }

    /// Staged files when amending: the index against HEAD's parent, so the
    /// changes of the last commit are listed too.
    pub fn amend_staged(&self) -> Result<Vec<FileChange>> {
        let diff = self.amend_diff(None)?;
        let mut out = Vec::new();
        for idx in 0..diff.deltas().len() {
            let delta = diff
                .get_delta(idx)
                .ok_or_else(|| Error::Other("missing diff delta".into()))?;
            let f = patch_to_file_diff(&delta, None)?;
            out.push(FileChange {
                path: f.path,
                old_path: f.old_path,
                kind: f.kind,
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Diff of one file in the amend view.
    pub fn amend_file_diff(&self, path: &str) -> Result<Option<FileDiff>> {
        let diff = self.amend_diff(Some(path))?;
        if diff.deltas().len() == 0 {
            return Ok(None);
        }
        let patch = git2::Patch::from_diff(&diff, 0)?;
        let delta = diff
            .get_delta(0)
            .ok_or_else(|| Error::Other("missing diff delta".into()))?;
        Ok(Some(patch_to_file_diff(&delta, patch.as_ref())?))
    }

    /// A patch with the staged or unstaged changes of `paths` (all files when
    /// empty), in the format `git apply` reads. New files are included.
    pub fn patch(&self, source: DiffSource, paths: &[String]) -> Result<String> {
        let mut opts = DiffOptions::new();
        opts.show_binary(true).disable_pathspec_match(true);
        for p in paths {
            opts.pathspec(p);
        }
        let diff = match source {
            DiffSource::Staged => {
                let head = self.raw().head().ok().and_then(|h| h.peel_to_tree().ok());
                self.raw()
                    .diff_tree_to_index(head.as_ref(), None, Some(&mut opts))?
            }
            DiffSource::Unstaged => {
                opts.include_untracked(true)
                    .recurse_untracked_dirs(true)
                    .show_untracked_content(true);
                self.raw().diff_index_to_workdir(None, Some(&mut opts))?
            }
            DiffSource::Commit(id) => {
                let commit = self.raw().find_commit(id)?;
                let old = commit.parent(0).ok().map(|p| p.tree()).transpose()?;
                self.raw().diff_tree_to_tree(
                    old.as_ref(),
                    Some(&commit.tree()?),
                    Some(&mut opts),
                )?
            }
        };
        let mut out = Vec::new();
        diff.print(DiffFormat::Patch, |_, _, line| {
            if matches!(line.origin(), '+' | '-' | ' ') {
                out.push(line.origin() as u8);
            }
            out.extend_from_slice(line.content());
            true
        })?;
        Ok(lossy(&out))
    }

    /// Whether Git LFS handles `path` (`filter=lfs` in `.gitattributes`).
    pub fn is_lfs_tracked(&self, path: &str) -> bool {
        self.raw()
            .get_attr_bytes(Path::new(path), "filter", AttrCheckFlags::FILE_THEN_INDEX)
            .ok()
            .flatten()
            .is_some_and(|v| v == b"lfs")
    }

    /// Files in the working directory bigger than `threshold` bytes that are
    /// not handled by Git LFS, with their sizes.
    pub fn large_files(&self, paths: &[String], threshold: u64) -> Vec<(String, u64)> {
        paths
            .iter()
            .filter_map(|p| {
                let meta = std::fs::metadata(self.path().join(p)).ok()?;
                (meta.is_file() && meta.len() > threshold && !self.is_lfs_tracked(p))
                    .then(|| (p.clone(), meta.len()))
            })
            .collect()
    }

    /// The message from `commit.template`, without comment lines.
    pub fn commit_template(&self) -> Option<String> {
        let cfg = self.raw().config().ok()?;
        let path = cfg.get_path("commit.template").ok()?;
        let path = if path.is_relative() {
            self.path().join(path)
        } else {
            path
        };
        let text = std::fs::read_to_string(path).ok()?;
        let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        Some(kept.join("\n").trim_end().to_owned()).filter(|t| !t.trim().is_empty())
    }

    /// Whether HEAD is already on a remote: in the upstream of the current
    /// branch, or (without one) in any remote branch.
    pub fn head_is_pushed(&self) -> bool {
        let Some(head) = self.head_oid() else {
            return false;
        };
        let repo = self.raw();
        let contains = |target: Option<Oid>| {
            target.is_some_and(|t| t == head || repo.graph_descendant_of(t, head).unwrap_or(false))
        };
        if let Ok(h) = repo.head()
            && h.is_branch()
            && let Ok(upstream) = git2::Branch::wrap(h).upstream()
        {
            return contains(upstream.get().target());
        }
        let Ok(branches) = repo.branches(Some(git2::BranchType::Remote)) else {
            return false;
        };
        branches.flatten().any(|(b, _)| contains(b.get().target()))
    }
}

/// Commits with `message`, sending each line of hook output to `output`.
/// The message goes through a file so `prepare-commit-msg` and `commit-msg`
/// hooks can read and change it, like with `git commit -F`.
pub fn commit(
    git: &Git,
    message: &str,
    opts: CommitOptions,
    output: impl FnMut(&str),
) -> Result<()> {
    let file = git.run(&["rev-parse", "--git-path", "FERRUM_COMMIT_MSG"])?;
    let file = git.dir().join(file.trim());
    std::fs::write(&file, message)?;
    let file_arg = file.to_string_lossy().into_owned();
    let mut args = vec!["commit", "-q", "--cleanup=strip", "-F", &file_arg];
    if opts.amend {
        args.push("--amend");
    }
    if opts.signoff {
        args.push("--signoff");
    }
    if opts.no_verify {
        args.push("--no-verify");
    }
    let result = git.run_streaming(&args, output);
    let _ = std::fs::remove_file(&file);
    result.map(|_| ())
}

/// Stops tracking files but keeps them on disk (`git rm --cached`).
pub fn untrack(git: &Git, paths: &[String]) -> Result<()> {
    let mut args = vec!["rm", "-r", "-q", "--cached", "--ignore-unmatch", "--"];
    args.extend(paths.iter().map(String::as_str));
    git.run(&args)?;
    Ok(())
}

/// Replaces a file with its content at `rev`, in the index and the working copy.
pub fn reset_file_to(git: &Git, rev: &str, path: &str) -> Result<()> {
    git.run(&["checkout", rev, "--", path])?;
    Ok(())
}

/// Unstages files in the amend view: back to HEAD's parent, so they leave
/// the amended commit. On the first commit they are removed from the index.
pub fn unstage_from_amend(git: &Git, paths: &[String]) -> Result<()> {
    let has_parent = git.run(&["rev-parse", "-q", "--verify", "HEAD~1"]).is_ok();
    let mut args = if has_parent {
        vec!["reset", "-q", "HEAD~1", "--"]
    } else {
        vec!["rm", "-q", "--cached", "--"]
    };
    args.extend(paths.iter().map(String::as_str));
    git.run(&args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn lists_ignored_files() {
        let t = TestRepo::new();
        t.commit(".gitignore", "*.log\ntarget/\n", "ignore");
        t.write("a.log", "x");
        t.write("target/out.bin", "x");
        t.write("keep.txt", "x");
        assert_eq!(t.repo().ignored_files().unwrap(), ["a.log", "target/"]);
    }

    #[test]
    fn amend_view_includes_last_commit() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        t.commit("b.txt", "b\n", "second");
        t.write("c.txt", "c\n");
        t.git.run(&["add", "c.txt"]).unwrap();
        let repo = t.repo();
        let paths: Vec<String> = repo
            .amend_staged()
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(paths, ["b.txt", "c.txt"]);
        assert!(repo.amend_file_diff("b.txt").unwrap().is_some());
        assert!(repo.amend_file_diff("a.txt").unwrap().is_none());

        unstage_from_amend(&t.git, &["b.txt".into()]).unwrap();
        let paths: Vec<String> = repo
            .amend_staged()
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(paths, ["c.txt"]);
    }

    #[test]
    fn patch_applies_elsewhere() {
        let t = TestRepo::new();
        t.commit("a.txt", "one\n", "first");
        t.write("a.txt", "one\ntwo\n");
        t.write("new.txt", "new\n");
        let patch = t.repo().patch(DiffSource::Unstaged, &[]).unwrap();
        assert!(patch.contains("+two"));
        assert!(patch.contains("new.txt"));

        let only = t
            .repo()
            .patch(DiffSource::Unstaged, &["a.txt".into()])
            .unwrap();
        assert!(!only.contains("new.txt"));

        // Undo the changes and apply the patch again.
        t.git.run(&["checkout", "--", "a.txt"]).unwrap();
        std::fs::remove_file(t.path().join("new.txt")).unwrap();
        t.git
            .run_with_input(&["apply", "-"], Some(patch.as_bytes()))
            .unwrap();
        assert_eq!(t.read("a.txt"), "one\ntwo\n");
        assert_eq!(t.read("new.txt"), "new\n");
    }

    #[test]
    fn large_files_skip_lfs() {
        let t = TestRepo::new();
        t.commit(
            ".gitattributes",
            "*.bin filter=lfs diff=lfs merge=lfs -text\n",
            "lfs",
        );
        t.write("big.dat", &"x".repeat(2000));
        t.write("big.bin", &"x".repeat(2000));
        t.write("small.dat", "x");
        let repo = t.repo();
        let paths = ["big.dat", "big.bin", "small.dat"].map(String::from);
        assert_eq!(repo.large_files(&paths, 1000), [("big.dat".into(), 2000)]);
    }

    #[test]
    fn commit_options_and_template() {
        let t = TestRepo::new();
        t.write("tpl.txt", "# comment\nfeat: \n");
        t.git
            .run(&["config", "commit.template", "tpl.txt"])
            .unwrap();
        assert_eq!(t.repo().commit_template().as_deref(), Some("feat:"));

        t.write("a.txt", "a\n");
        t.git.run(&["add", "a.txt"]).unwrap();
        let opts = CommitOptions {
            signoff: true,
            ..Default::default()
        };
        commit(&t.git, "Subject\n\nBody", opts, |_| {}).unwrap();
        let msg = t.git.run(&["log", "-1", "--format=%B"]).unwrap();
        assert!(msg.starts_with("Subject\n\nBody"));
        assert!(msg.contains("Signed-off-by: Test <test@example.com>"));
        assert!(!t.repo().head_is_pushed());
    }

    #[cfg(unix)]
    #[test]
    fn hooks_stream_and_can_be_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        let hook = t.path().join(".git/hooks/pre-commit");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\necho checking files\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let prepare = t.path().join(".git/hooks/prepare-commit-msg");
        std::fs::write(&prepare, "#!/bin/sh\necho \"[hook]\" >> \"$1\"\n").unwrap();
        std::fs::set_permissions(&prepare, std::fs::Permissions::from_mode(0o755)).unwrap();

        t.write("a.txt", "b\n");
        t.git.run(&["add", "a.txt"]).unwrap();
        let mut lines = Vec::new();
        let err = commit(&t.git, "Change", CommitOptions::default(), |l| {
            lines.push(l.to_owned())
        });
        assert!(err.is_err());
        assert!(lines.iter().any(|l| l.contains("checking files")));

        let opts = CommitOptions {
            no_verify: true,
            ..Default::default()
        };
        commit(&t.git, "Change", opts, |_| {}).unwrap();
        let msg = t.git.run(&["log", "-1", "--format=%B"]).unwrap();
        assert_eq!(msg.trim(), "Change\n[hook]");
    }

    #[test]
    fn untrack_and_reset_file() {
        let t = TestRepo::new();
        t.commit("a.txt", "one\n", "first");
        t.commit("a.txt", "two\n", "second");
        reset_file_to(&t.git, "HEAD~1", "a.txt").unwrap();
        assert_eq!(t.read("a.txt"), "one\n");
        untrack(&t.git, &["a.txt".into()]).unwrap();
        let status = t.git.run(&["status", "--porcelain"]).unwrap();
        assert!(status.contains("D  a.txt"));
        assert!(t.path().join("a.txt").exists());
    }
}
