//! Browsing commits: comparing two revisions (or a revision and the working
//! copy), the files of a revision, the refs that contain a commit, web links
//! to a commit and commit ids mentioned in text.

use std::ops::Range;
use std::path::Path;

use git2::{ObjectType, Oid};

use crate::cli::Git;
use crate::diff::{DiffConfig, FILEMODE_LINK, FILEMODE_SUBMODULE, patch_to_file_diff};
use crate::model::FileDiff;
use crate::repo::Repo;
use crate::{Error, Result, lossy};

/// One side of a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rev {
    Commit(Oid),
    /// The files on disk (staged and unstaged changes).
    WorkDir,
}

/// What a tree entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    Link,
    Submodule,
}

/// One entry of a folder at a revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub name: String,
    /// Path from the repository root.
    pub path: String,
    pub kind: EntryKind,
    pub id: Oid,
}

/// Branches and tags that contain a commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Containing {
    /// Local branches, then remote ones (short names).
    pub branches: Vec<String>,
    pub tags: Vec<String>,
    /// Refs found beyond the listed ones.
    pub more: usize,
}

impl Repo {
    /// Files that differ between two revisions, without their content.
    pub fn compare_files(&self, from: Rev, to: Rev) -> Result<Vec<FileDiff>> {
        let diff = self.compare_diff(from, to, &[], &DiffConfig::default())?;
        let mut out = Vec::with_capacity(diff.deltas().len());
        for delta in diff.deltas() {
            let mut f = patch_to_file_diff(&delta, None)?;
            f.binary = delta.flags().is_binary();
            out.push(f);
        }
        Ok(out)
    }

    /// The diff of one file between two revisions. Give `old_path` for renames.
    pub fn compare_file_diff(
        &self,
        from: Rev,
        to: Rev,
        path: &str,
        old_path: Option<&str>,
        cfg: &DiffConfig,
    ) -> Result<Option<FileDiff>> {
        let mut paths = vec![path];
        paths.extend(old_path);
        let diff = self.compare_diff(from, to, &paths, cfg)?;
        for idx in 0..diff.deltas().len() {
            let patch = git2::Patch::from_diff(&diff, idx)?;
            let delta = diff
                .get_delta(idx)
                .ok_or_else(|| Error::Other("missing diff delta".into()))?;
            let f = patch_to_file_diff(&delta, patch.as_ref())?;
            if f.path == path {
                return Ok(Some(f));
            }
        }
        Ok(None)
    }

    fn compare_diff(
        &self,
        from: Rev,
        to: Rev,
        paths: &[&str],
        cfg: &DiffConfig,
    ) -> Result<git2::Diff<'_>> {
        let repo = self.raw();
        let mut opts = cfg.options();
        for p in paths {
            opts.pathspec(p);
        }
        if !paths.is_empty() {
            opts.disable_pathspec_match(true);
        }
        let tree = |id: Oid| repo.find_commit(id).and_then(|c| c.tree());
        let mut diff = match (from, to) {
            (Rev::Commit(a), Rev::Commit(b)) => {
                repo.diff_tree_to_tree(Some(&tree(a)?), Some(&tree(b)?), Some(&mut opts))?
            }
            (Rev::Commit(a), Rev::WorkDir) => {
                repo.diff_tree_to_workdir_with_index(Some(&tree(a)?), Some(&mut opts))?
            }
            (Rev::WorkDir, Rev::Commit(b)) => {
                opts.reverse(true);
                repo.diff_tree_to_workdir_with_index(Some(&tree(b)?), Some(&mut opts))?
            }
            (Rev::WorkDir, Rev::WorkDir) => repo.diff_tree_to_tree(None, None, Some(&mut opts))?,
        };
        let mut find = git2::DiffFindOptions::new();
        find.renames(true).for_untracked(true);
        diff.find_similar(Some(&mut find))?;
        Ok(diff)
    }

    /// The entries of folder `dir` ("" for the root) at a commit: folders
    /// first, then by name.
    pub fn tree_dir(&self, commit: Oid, dir: &str) -> Result<Vec<TreeEntry>> {
        let repo = self.raw();
        let root = repo.find_commit(commit)?.tree()?;
        let tree = if dir.is_empty() {
            root
        } else {
            root.get_path(Path::new(dir))?
                .to_object(repo)?
                .peel_to_tree()?
        };
        let mut out: Vec<TreeEntry> = tree
            .iter()
            .map(|e| {
                let name = lossy(e.name_bytes());
                let mode = e.filemode() as u32;
                let kind = match e.kind() {
                    Some(ObjectType::Tree) => EntryKind::Dir,
                    _ if mode == FILEMODE_SUBMODULE => EntryKind::Submodule,
                    _ if mode == FILEMODE_LINK => EntryKind::Link,
                    _ => EntryKind::File,
                };
                let path = if dir.is_empty() {
                    name.clone()
                } else {
                    format!("{dir}/{name}")
                };
                TreeEntry {
                    name,
                    path,
                    kind,
                    id: e.id(),
                }
            })
            .collect();
        out.sort_by(|a, b| {
            (a.kind != EntryKind::Dir)
                .cmp(&(b.kind != EntryKind::Dir))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    /// Content of a file at a revision; `None` when it does not exist there.
    pub fn file_at(&self, rev: Rev, path: &str) -> Result<Option<Vec<u8>>> {
        match rev {
            Rev::WorkDir => match std::fs::read(self.path().join(path)) {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            },
            Rev::Commit(id) => {
                let repo = self.raw();
                let tree = repo.find_commit(id)?.tree()?;
                let Ok(entry) = tree.get_path(Path::new(path)) else {
                    return Ok(None);
                };
                if entry.kind() != Some(ObjectType::Blob) {
                    return Ok(None);
                }
                Ok(Some(repo.find_blob(entry.id())?.content().to_vec()))
            }
        }
    }

    /// Content of a file in the index (staged version).
    pub fn file_in_index(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let repo = self.raw();
        let index = repo.index()?;
        let Some(entry) = index.get_path(Path::new(path), 0) else {
            return Ok(None);
        };
        Ok(Some(repo.find_blob(entry.id)?.content().to_vec()))
    }
}

/// Branches and tags that contain `id`, listing at most `limit` of each.
/// Uses `git for-each-ref --contains`, which is fast on large repositories
/// (commit-graph); call it off the UI thread anyway.
pub fn refs_containing(git: &Git, id: Oid, limit: usize) -> Result<Containing> {
    let contains = format!("--contains={id}");
    let out = git.run(&[
        "for-each-ref",
        &contains,
        "--format=%(refname)",
        "refs/heads",
        "refs/remotes",
        "refs/tags",
    ])?;
    let mut c = Containing::default();
    let mut remotes = Vec::new();
    for r in out.lines().map(str::trim) {
        let (list, name) = if let Some(n) = r.strip_prefix("refs/heads/") {
            (&mut c.branches, n)
        } else if let Some(n) = r.strip_prefix("refs/remotes/") {
            if n.ends_with("/HEAD") {
                continue;
            }
            (&mut remotes, n)
        } else if let Some(n) = r.strip_prefix("refs/tags/") {
            (&mut c.tags, n)
        } else {
            continue;
        };
        list.push(name.to_owned());
    }
    c.branches.extend(remotes);
    for list in [&mut c.branches, &mut c.tags] {
        if list.len() > limit {
            c.more += list.len() - limit;
            list.truncate(limit);
        }
    }
    Ok(c)
}

/// Web page of a commit for a remote URL on GitHub, GitLab, Bitbucket or a
/// Gitea / Forgejo / Gogs style host (`/commit/<sha>`).
pub fn commit_web_url(remote_url: &str, sha: &str) -> Option<String> {
    let (web, host) = web_base(remote_url)?;
    let path = if host.contains("gitlab") {
        "-/commit"
    } else if host.contains("bitbucket") {
        "commits"
    } else if host.contains("dev.azure.com") || host.contains("visualstudio.com") {
        return None;
    } else {
        "commit"
    };
    Some(format!("{web}/{path}/{sha}"))
}

/// `https://host/owner/repo` and the host, from a clone URL.
fn web_base(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (scheme, host, path) = if let Some((scheme, rest)) = url.split_once("://") {
        if scheme == "file" {
            return None;
        }
        let (auth, path) = rest.split_once('/')?;
        let host = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
        let host = host.split(':').next()?;
        let scheme = if scheme == "http" { "http" } else { "https" };
        (scheme, host, path)
    } else {
        // git@host:owner/repo
        let rest = url.rsplit_once('@').map_or(url, |(_, r)| r);
        let (host, path) = rest.split_once(':')?;
        if host.contains('/') || host.len() == 1 {
            // A local path (or a Windows drive letter).
            return None;
        }
        ("https", host, path)
    };
    let path = path
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_matches('/');
    if host.is_empty() || path.split('/').count() < 2 {
        return None;
    }
    let host = host.to_ascii_lowercase();
    Some((format!("{scheme}://{host}/{path}"), host))
}

/// Byte ranges of words in `text` that look like commit ids (7 to 40 hex
/// digits, with at least one digit).
pub fn find_commit_ids(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let word = &bytes[start..i];
        let hex = word
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b));
        if hex && (7..=40).contains(&word.len()) && word.iter().any(u8::is_ascii_digit) {
            out.push(start..i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ChangeKind;
    use crate::testutil::TestRepo;

    #[test]
    fn compares_commits_and_the_working_copy() {
        let t = TestRepo::new();
        t.commit("a.txt", "one\n", "first");
        t.commit("b.txt", "bee\n", "second");
        let repo = t.repo();
        let first = repo.resolve("HEAD~1").unwrap();
        let second = repo.resolve("HEAD").unwrap();

        let files = repo
            .compare_files(Rev::Commit(first), Rev::Commit(second))
            .unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "b.txt");
        assert_eq!(files[0].kind, ChangeKind::Added);
        // Swapped, the file is deleted.
        let files = repo
            .compare_files(Rev::Commit(second), Rev::Commit(first))
            .unwrap();
        assert_eq!(files[0].kind, ChangeKind::Deleted);

        t.write("a.txt", "one\ntwo\n");
        let files = repo
            .compare_files(Rev::Commit(first), Rev::WorkDir)
            .unwrap();
        let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, ["a.txt", "b.txt"]);
        let d = repo
            .compare_file_diff(
                Rev::Commit(first),
                Rev::WorkDir,
                "a.txt",
                None,
                &DiffConfig::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!((d.additions, d.deletions), (1, 0));
        let d = repo
            .compare_file_diff(
                Rev::WorkDir,
                Rev::Commit(first),
                "a.txt",
                None,
                &DiffConfig::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!((d.additions, d.deletions), (0, 1));
    }

    #[test]
    fn lists_folders_and_reads_files_at_a_revision() {
        let t = TestRepo::new();
        t.commit("src/main.rs", "fn main() {}\n", "first");
        t.commit("README", "hi\n", "second");
        let repo = t.repo();
        let head = repo.resolve("HEAD").unwrap();
        let root = repo.tree_dir(head, "").unwrap();
        let names: Vec<(&str, EntryKind)> =
            root.iter().map(|e| (e.name.as_str(), e.kind)).collect();
        assert_eq!(
            names,
            [("src", EntryKind::Dir), ("README", EntryKind::File)]
        );
        let src = repo.tree_dir(head, "src").unwrap();
        assert_eq!(src[0].path, "src/main.rs");
        assert_eq!(
            repo.file_at(Rev::Commit(head), "src/main.rs").unwrap(),
            Some(b"fn main() {}\n".to_vec())
        );
        let first = repo.resolve("HEAD~1").unwrap();
        assert_eq!(repo.file_at(Rev::Commit(first), "README").unwrap(), None);
        t.write("README", "changed\n");
        assert_eq!(
            repo.file_at(Rev::WorkDir, "README").unwrap(),
            Some(b"changed\n".to_vec())
        );
        assert_eq!(
            repo.file_in_index("README").unwrap(),
            Some(b"hi\n".to_vec())
        );
    }

    #[test]
    fn finds_branches_and_tags_containing_a_commit() {
        let t = TestRepo::new();
        t.commit("a", "1", "first");
        let repo = t.repo();
        let first = repo.resolve("HEAD").unwrap();
        t.git.run(&["tag", "v1"]).unwrap();
        t.git.run(&["switch", "-q", "-c", "feature"]).unwrap();
        t.commit("a", "2", "second");
        let second = repo.resolve("HEAD").unwrap();
        let c = refs_containing(&t.git, first, 10).unwrap();
        assert_eq!(c.branches, ["feature", "main"]);
        assert_eq!(c.tags, ["v1"]);
        let c = refs_containing(&t.git, second, 10).unwrap();
        assert_eq!(c.branches, ["feature"]);
        assert!(c.tags.is_empty());
        let c = refs_containing(&t.git, first, 1).unwrap();
        assert_eq!((c.branches.len(), c.more), (1, 1));
    }

    #[test]
    fn builds_commit_web_links() {
        let sha = "abc1234";
        let cases = [
            (
                "git@github.com:o/r.git",
                Some("https://github.com/o/r/commit/abc1234"),
            ),
            (
                "https://gitlab.com/g/sub/r.git",
                Some("https://gitlab.com/g/sub/r/-/commit/abc1234"),
            ),
            (
                "https://user@bitbucket.org/o/r.git",
                Some("https://bitbucket.org/o/r/commits/abc1234"),
            ),
            (
                "ssh://git@codeberg.org:22/o/r.git",
                Some("https://codeberg.org/o/r/commit/abc1234"),
            ),
            (
                "http://gitea.local/o/r",
                Some("http://gitea.local/o/r/commit/abc1234"),
            ),
            ("/srv/git/r.git", None),
            ("C:/repos/r", None),
            ("file:///srv/r.git", None),
        ];
        for (url, want) in cases {
            assert_eq!(commit_web_url(url, sha).as_deref(), want, "{url}");
        }
    }

    #[test]
    fn finds_commit_ids_in_text() {
        let text = "Reverts 1a2b3c4d, see abcdef0 and deadbeef; not g1a2b3c4 or 12345.";
        let found: Vec<&str> = find_commit_ids(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(found, ["1a2b3c4d", "abcdef0"]);
    }
}
