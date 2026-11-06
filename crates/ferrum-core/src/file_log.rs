//! History of files, folders and line ranges, blame at any revision, and
//! the content of a file at a revision.

use std::path::Path;

use git2::Oid;

use crate::diff::{DiffConfig, collect};
use crate::model::*;
use crate::repo::{FS, RS, Repo, parse_blame, parse_file_log, parse_offset};
use crate::{Error, Result};

impl Repo {
    /// Whether `path` is a folder at `rev`, or (when it is not there) in the
    /// working tree. The repository root ("") is a folder.
    pub fn is_folder(&self, rev: &str, path: &str) -> bool {
        let path = path.trim_end_matches('/');
        if path.is_empty() {
            return true;
        }
        let in_tree = self
            .raw()
            .revparse_single(rev)
            .and_then(|o| o.peel_to_tree())
            .ok()
            .and_then(|t| t.get_path(Path::new(path)).ok())
            .map(|e| e.kind() == Some(git2::ObjectType::Tree));
        in_tree.unwrap_or_else(|| self.path().join(path).is_dir())
    }

    /// Blame of `path` as of `rev` (a commit, branch or `HEAD`).
    pub fn blame_at(&self, path: &str, rev: &str) -> Result<Vec<BlameLine>> {
        check_rev(rev)?;
        let tree = self.raw().revparse_single(rev)?.peel_to_tree()?;
        let entry = tree.get_path(Path::new(path)).map_err(|_| {
            Error::Other(if rev == "HEAD" {
                format!("{path} is not in the last commit (HEAD).")
            } else {
                format!("{path} is not in {}.", short_rev(rev))
            })
        })?;
        let blob = entry.to_object(self.raw())?.peel_to_blob()?;
        if blob.is_binary() {
            return Err(Error::Other(format!("{path} is a binary file.")));
        }
        // `git blame` follows renames and moves, unlike libgit2's blame.
        let out = self.git().run(&["blame", "--porcelain", rev, "--", path])?;
        Ok(parse_blame(&out))
    }

    /// Commits reachable from `rev` that touched `path`, newest first. Files
    /// follow renames; a folder lists every commit that changed something in it.
    pub fn file_history_at(&self, path: &str, rev: &str) -> Result<Vec<FileCommit>> {
        check_rev(rev)?;
        let fmt = format!("--format={RS}%H{FS}%P{FS}%an{FS}%ae{FS}%at{FS}%ai{FS}%s");
        if !self.is_folder(rev, path) {
            let args = [
                "log",
                rev,
                "--follow",
                "--name-status",
                "-z",
                &fmt,
                "--",
                path,
            ];
            return Ok(parse_file_log(&self.git().run(&args)?, path));
        }
        let folder = path.trim_end_matches('/');
        let spec = if folder.is_empty() { "." } else { folder };
        let out = self.git().run(&["log", rev, "-z", &fmt, "--", spec])?;
        let mut list = parse_file_log(&out, folder);
        for c in &mut list {
            c.path = folder.to_owned();
            c.old_path = None;
        }
        Ok(list)
    }

    /// Commits reachable from `rev` that changed lines `start..=end`
    /// (1-based) of `path`, with how they changed them (`git log -L`).
    pub fn line_history(
        &self,
        path: &str,
        start: u32,
        end: u32,
        rev: &str,
    ) -> Result<Vec<LineCommit>> {
        check_rev(rev)?;
        let (start, end) = (start.max(1), end.max(start.max(1)));
        let range = format!("-L{start},{end}:{path}");
        let fmt = format!("--format={RS}%H{FS}%P{FS}%an{FS}%ae{FS}%at{FS}%ai{FS}%s");
        let out = self.git().run(&[
            "log",
            "--no-color",
            "--no-ext-diff",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            &fmt,
            &range,
            rev,
        ])?;
        Ok(parse_line_log(&out))
    }

    /// Content of `path` at commit `rev`, or `None` if it is not there.
    pub fn file_at(&self, rev: Oid, path: &str) -> Result<Option<Vec<u8>>> {
        let tree = self.raw().find_commit(rev)?.tree()?;
        let Ok(entry) = tree.get_path(Path::new(path)) else {
            return Ok(None);
        };
        let blob = entry.to_object(self.raw())?.peel_to_blob()?;
        Ok(Some(blob.content().to_vec()))
    }

    /// Files under `prefix` (all files when empty) that differ between two
    /// commits, without their content. `old: None` compares with nothing.
    pub fn changed_files_between(
        &self,
        old: Option<Oid>,
        new: Oid,
        prefix: &str,
    ) -> Result<Vec<FileDiff>> {
        let diff = self.tree_diff(old, new, &[prefix], &DiffConfig::default())?;
        let mut out = Vec::new();
        for delta in diff.deltas() {
            let mut f = crate::diff::patch_to_file_diff(&delta, None)?;
            f.binary = delta.flags().is_binary();
            out.push(f);
        }
        Ok(out)
    }

    /// Diff of one file between two commits. `old_path` is its name in `old`
    /// when it was renamed in between.
    pub fn file_diff_between(
        &self,
        old: Option<Oid>,
        new: Oid,
        path: &str,
        old_path: Option<&str>,
        cfg: &DiffConfig,
    ) -> Result<Option<FileDiff>> {
        let mut paths = vec![path];
        paths.extend(old_path);
        let diff = self.tree_diff(old, new, &paths, cfg)?;
        let files = collect(&diff)?;
        let pos = files
            .iter()
            .position(|f| f.path == path)
            .or_else(|| files.iter().position(|f| old_path == Some(f.path.as_str())));
        Ok(pos.and_then(|i| files.into_iter().nth(i)))
    }

    fn tree_diff(
        &self,
        old: Option<Oid>,
        new: Oid,
        paths: &[&str],
        cfg: &DiffConfig,
    ) -> Result<git2::Diff<'_>> {
        let repo = self.raw();
        let new_tree = repo.find_commit(new)?.tree()?;
        let old_tree = old
            .map(|o| repo.find_commit(o).and_then(|c| c.tree()))
            .transpose()?;
        let mut opts = cfg.options();
        for p in paths.iter().map(|p| p.trim_end_matches('/')) {
            if !p.is_empty() {
                opts.pathspec(p);
            }
        }
        opts.disable_pathspec_match(true);
        let mut diff =
            repo.diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), Some(&mut opts))?;
        diff.find_similar(None)?;
        Ok(diff)
    }

    /// Paths of every file in the index (`git ls-files`).
    pub fn tracked_files(&self) -> Result<Vec<String>> {
        let out = self.git().run(&["ls-files", "-z"])?;
        Ok(out
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect())
    }
}

/// Revisions are passed to git as arguments: never let one be an option.
fn check_rev(rev: &str) -> Result<()> {
    if rev.is_empty() || rev.starts_with('-') {
        return Err(Error::Other(format!("Invalid revision: {rev}")));
    }
    Ok(())
}

fn short_rev(rev: &str) -> &str {
    if rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        &rev[..7]
    } else {
        rev
    }
}

/// Parses `git log -L` output: each record is a header in our format, then
/// the diff of the traced lines.
fn parse_line_log(out: &str) -> Vec<LineCommit> {
    let mut list = Vec::new();
    for rec in out.split(RS).filter(|r| !r.trim().is_empty()) {
        let (header, patch) = rec.split_once('\n').unwrap_or((rec, ""));
        let f: Vec<&str> = header.splitn(7, FS).collect();
        let [id, parents, author, email, time, date, summary] = f.as_slice() else {
            continue;
        };
        let (Ok(id), Ok(time)) = (Oid::from_str(id), time.parse()) else {
            continue;
        };
        let commit = CommitInfo {
            id,
            parents: parents
                .split_whitespace()
                .filter_map(|p| Oid::from_str(p).ok())
                .collect(),
            author: (*author).to_owned(),
            email: (*email).to_owned(),
            time,
            offset_minutes: parse_offset(date),
            summary: (*summary).to_owned(),
        };
        list.push(LineCommit {
            commit,
            diff: parse_patch(patch),
        });
    }
    list
}

/// Path from a `--- a/x` or `+++ b/x` line (`None` for /dev/null).
fn patch_path(rest: &str) -> Option<String> {
    let rest = rest.trim_end();
    if rest == "/dev/null" {
        return None;
    }
    let unquoted = rest
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(rest);
    let path = unquoted
        .strip_prefix("a/")
        .or_else(|| unquoted.strip_prefix("b/"))
        .unwrap_or(unquoted);
    Some(path.to_owned())
}

/// `-12,3` or `+5` from a hunk header: (start, count).
fn hunk_range(s: &str) -> (u32, u32) {
    let s = &s[1..];
    match s.split_once(',') {
        Some((a, b)) => (a.parse().unwrap_or(0), b.parse().unwrap_or(0)),
        None => (s.parse().unwrap_or(0), 1),
    }
}

/// Parses a unified diff of one file.
pub(crate) fn parse_patch(text: &str) -> Option<FileDiff> {
    let mut old_path = None;
    let mut new_path = None;
    let mut hunks: Vec<Hunk> = Vec::new();
    let (mut old_no, mut new_no) = (0, 0);
    let (mut additions, mut deletions) = (0, 0);
    let mut in_hunk = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            let ranges: Vec<&str> = rest.split(' ').take(2).collect();
            let [old, new] = ranges.as_slice() else {
                continue;
            };
            let (old_start, old_lines) = hunk_range(old);
            let (new_start, new_lines) = hunk_range(new);
            (old_no, new_no) = (old_start, new_start);
            hunks.push(Hunk {
                header: line.to_owned(),
                old_start,
                old_lines,
                new_start,
                new_lines,
                lines: Vec::new(),
            });
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            if let Some(rest) = line.strip_prefix("--- ") {
                old_path = patch_path(rest);
            } else if let Some(rest) = line.strip_prefix("+++ ") {
                new_path = patch_path(rest);
            }
            continue;
        }
        let Some(hunk) = hunks.last_mut() else {
            continue;
        };
        let (kind, text) = match line.chars().next() {
            Some('+') => (LineKind::Added, &line[1..]),
            Some('-') => (LineKind::Removed, &line[1..]),
            Some(' ') => (LineKind::Context, &line[1..]),
            Some('\\') => {
                if let Some(last) = hunk.lines.last_mut() {
                    last.no_newline = true;
                }
                continue;
            }
            // `git log -L` separates commits with an empty line; the next
            // file header starts a new section.
            _ => {
                in_hunk = line.is_empty();
                continue;
            }
        };
        let (o, n) = match kind {
            LineKind::Added => {
                additions += 1;
                new_no += 1;
                (None, Some(new_no - 1))
            }
            LineKind::Removed => {
                deletions += 1;
                old_no += 1;
                (Some(old_no - 1), None)
            }
            LineKind::Context => {
                old_no += 1;
                new_no += 1;
                (Some(old_no - 1), Some(new_no - 1))
            }
        };
        hunk.lines.push(DiffLine {
            kind,
            text: text.to_owned(),
            old_no: o,
            new_no: n,
            no_newline: false,
        });
    }
    let path = new_path.clone().or_else(|| old_path.clone())?;
    let kind = match (&old_path, &new_path) {
        (None, _) => ChangeKind::Added,
        (_, None) => ChangeKind::Deleted,
        (Some(o), Some(n)) if o != n => ChangeKind::Renamed,
        _ => ChangeKind::Modified,
    };
    Some(FileDiff {
        old_path: old_path.filter(|o| *o != path),
        path,
        kind,
        binary: false,
        old_id: None,
        new_id: None,
        hunks,
        additions,
        deletions,
        old_mode: 0,
        new_mode: 0,
        skipped: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn blames_at_a_revision_with_previous_commits() {
        let t = TestRepo::new();
        t.commit("a.txt", "one\ntwo\n", "first");
        t.commit("a.txt", "one\nTWO\nthree\n", "second");
        let repo = t.repo();
        let head = repo.blame_at("a.txt", "HEAD").unwrap();
        assert_eq!(head.len(), 3);
        let first = repo.resolve("HEAD~1").unwrap();
        // Line 2 changed in "second"; its parent is "first".
        assert_eq!(head[1].previous.as_ref().map(|p| p.0), Some(first));
        assert_eq!(head[1].previous.as_ref().unwrap().1, "a.txt");
        assert_eq!(head[1].orig_line, 2);
        assert_eq!(head[0].path, "a.txt");
        assert!(head[0].previous.is_none());

        let old = repo.blame_at("a.txt", &first.to_string()).unwrap();
        assert_eq!(old.len(), 2);
        assert!(old.iter().all(|l| l.commit == first));
        let err = repo.blame_at("a.txt", "--all").unwrap_err().to_string();
        assert!(err.contains("Invalid"), "{err}");
        let missing = repo.blame_at("nope.txt", "HEAD~1").unwrap_err().to_string();
        assert!(missing.contains("is not in HEAD~1"), "{missing}");
    }

    #[test]
    fn folder_history_on_a_branch() {
        let t = TestRepo::new();
        t.commit("src/a.rs", "a\n", "add a");
        t.commit("other.txt", "x\n", "unrelated");
        t.git.run(&["switch", "-q", "-c", "feature"]).unwrap();
        t.commit("src/deep/b.rs", "b\n", "add b");
        t.git.run(&["switch", "-q", "main"]).unwrap();
        t.commit("src/a.rs", "a2\n", "edit a");
        let repo = t.repo();
        assert!(repo.is_folder("HEAD", "src"));
        assert!(repo.is_folder("feature", "src/deep"));
        assert!(!repo.is_folder("HEAD", "src/a.rs"));

        let summaries = |rev: &str| -> Vec<String> {
            repo.file_history_at("src", rev)
                .unwrap()
                .into_iter()
                .map(|c| c.commit.summary)
                .collect()
        };
        assert_eq!(summaries("HEAD"), ["edit a", "add a"]);
        assert_eq!(summaries("feature"), ["add b", "add a"]);
        let all = repo.file_history_at("src", "feature").unwrap();
        assert!(all.iter().all(|c| c.path == "src"));

        let tip = repo.resolve("feature").unwrap();
        let first = repo.resolve("main~2").unwrap();
        let files = repo.changed_files_between(Some(first), tip, "src").unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["src/deep/b.rs"]);
        // A file's history on another branch.
        let h = repo.file_history_at("src/deep/b.rs", "feature").unwrap();
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn traces_a_line_range() {
        let t = TestRepo::new();
        t.commit("f.txt", "a\nb\nc\nd\n", "create");
        t.commit("f.txt", "a\nB\nc\nd\n", "change b");
        t.commit("f.txt", "a\nB\nc\nD\n", "change d");
        let repo = t.repo();
        let list = repo.line_history("f.txt", 2, 2, "HEAD").unwrap();
        let summaries: Vec<&str> = list.iter().map(|c| c.commit.summary.as_str()).collect();
        assert_eq!(summaries, ["change b", "create"]);
        let diff = list[0].diff.as_ref().unwrap();
        assert_eq!(diff.path, "f.txt");
        assert_eq!((diff.additions, diff.deletions), (1, 1));
        let added = diff.hunks[0]
            .lines
            .iter()
            .find(|l| l.kind == LineKind::Added)
            .unwrap();
        assert_eq!((added.text.as_str(), added.new_no), ("B", Some(2)));
        assert_eq!(list[1].diff.as_ref().unwrap().kind, ChangeKind::Added);
    }

    #[test]
    fn diffs_a_file_between_revisions() {
        let t = TestRepo::new();
        t.commit("a.txt", "1\n2\n3\n4\n5\n6\n", "v1");
        t.git.run(&["mv", "a.txt", "b.txt"]).unwrap();
        t.git.run(&["commit", "-q", "-m", "rename"]).unwrap();
        t.commit("b.txt", "1\n2\n3\n4\n5\n6\n7\n", "v2");
        let repo = t.repo();
        let v1 = repo.resolve("HEAD~2").unwrap();
        let v2 = repo.resolve("HEAD").unwrap();
        let cfg = DiffConfig::default();
        let d = repo
            .file_diff_between(Some(v1), v2, "b.txt", Some("a.txt"), &cfg)
            .unwrap()
            .unwrap();
        assert_eq!(d.path, "b.txt");
        assert_eq!(d.old_path.as_deref(), Some("a.txt"));
        assert_eq!(d.additions, 1);
        assert_eq!(
            repo.file_at(v1, "a.txt").unwrap().unwrap(),
            b"1\n2\n3\n4\n5\n6\n"
        );
        assert!(repo.file_at(v1, "b.txt").unwrap().is_none());
        let tracked = repo.tracked_files().unwrap();
        assert_eq!(tracked, ["b.txt"]);
    }

    #[test]
    fn parses_a_patch() {
        let p = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n\\ No newline at end of file\n";
        let d = parse_patch(p).unwrap();
        assert_eq!(d.hunks.len(), 1);
        let lines = &d.hunks[0].lines;
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1].old_no, Some(2));
        assert_eq!(lines[2].new_no, Some(2));
        assert!(lines[2].no_newline);
        assert!(parse_patch("").is_none());
    }
}
