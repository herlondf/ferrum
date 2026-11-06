//! Diff conversion, diff options, partial patches for staging individual
//! lines, and extra information for the diff viewer (sizes, submodule
//! commits, Git LFS pointers).

use std::path::Path;

use git2::{DiffOptions, Oid};

use crate::cli::Git;
use crate::model::*;
use crate::repo::{Repo, commit_info};
use crate::{Error, Result, lossy};

/// Untracked files larger than this are not diffed until asked.
pub const LARGE_UNTRACKED_BYTES: u64 = 2 * 1024 * 1024;
/// Context used for "show entire file".
const FULL_CONTEXT: u32 = 1 << 24;
/// Commits listed per side of a submodule change.
const SUBMODULE_LIMIT: usize = 500;
pub const FILEMODE_LINK: u32 = 0o120000;
pub const FILEMODE_SUBMODULE: u32 = 0o160000;

/// Options that change how a diff is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffConfig {
    pub ignore_whitespace: bool,
    /// Unchanged lines shown around each change.
    pub context_lines: u32,
    /// Show the whole file, not just the changed parts.
    pub full_file: bool,
    /// Untracked files above this size are not diffed (`None`: no limit).
    pub max_untracked_bytes: Option<u64>,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            ignore_whitespace: false,
            context_lines: 3,
            full_file: false,
            max_untracked_bytes: Some(LARGE_UNTRACKED_BYTES),
        }
    }
}

impl DiffConfig {
    pub(crate) fn options(&self) -> DiffOptions {
        let mut opts = DiffOptions::new();
        opts.context_lines(if self.full_file {
            FULL_CONTEXT
        } else {
            self.context_lines
        });
        if self.ignore_whitespace {
            opts.ignore_whitespace(true);
        }
        opts
    }
}

impl Repo {
    /// Like [`Repo::file_diff`], with options.
    pub fn file_diff_with(
        &self,
        source: DiffSource,
        path: &str,
        cfg: &DiffConfig,
    ) -> Result<Option<FileDiff>> {
        let repo = self.raw();
        let mut opts = cfg.options();
        opts.pathspec(path).disable_pathspec_match(true);
        let diff = match source {
            DiffSource::Unstaged => {
                if let Some(limit) = cfg.max_untracked_bytes
                    && let Some(file) = self.large_untracked(path, limit)
                {
                    return Ok(Some(file));
                }
                opts.include_untracked(true)
                    .recurse_untracked_dirs(true)
                    .show_untracked_content(true);
                repo.diff_index_to_workdir(None, Some(&mut opts))?
            }
            DiffSource::Staged => {
                let head = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
                repo.diff_tree_to_index(head.as_ref(), None, Some(&mut opts))?
            }
            DiffSource::Commit(id) => return self.commit_file_diff_with(id, path, None, cfg),
        };
        Ok(collect(&diff)?.into_iter().next())
    }

    /// Like [`Repo::commit_file_diff`], with options.
    pub fn commit_file_diff_with(
        &self,
        id: Oid,
        path: &str,
        old_path: Option<&str>,
        cfg: &DiffConfig,
    ) -> Result<Option<FileDiff>> {
        let repo = self.raw();
        let commit = repo.find_commit(id)?;
        let new_tree = commit.tree()?;
        let old_tree = commit.parent(0).ok().map(|p| p.tree()).transpose()?;
        let mut opts = cfg.options();
        opts.pathspec(path).disable_pathspec_match(true);
        if let Some(old) = old_path {
            opts.pathspec(old);
        }
        let mut diff =
            repo.diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), Some(&mut opts))?;
        diff.find_similar(None)?;
        Ok(collect(&diff)?.into_iter().find(|f| f.path == path))
    }

    /// An untracked file over `limit` bytes, as a diff without content.
    fn large_untracked(&self, path: &str, limit: u64) -> Option<FileDiff> {
        let status = self.raw().status_file(Path::new(path)).ok()?;
        if !status.contains(git2::Status::WT_NEW) {
            return None;
        }
        let meta = std::fs::metadata(self.path().join(path)).ok()?;
        if !meta.is_file() || meta.len() <= limit {
            return None;
        }
        Some(FileDiff {
            path: path.to_owned(),
            old_path: None,
            kind: ChangeKind::Untracked,
            binary: false,
            old_id: None,
            new_id: None,
            hunks: Vec::new(),
            additions: 0,
            deletions: 0,
            old_mode: 0,
            new_mode: 0o100644,
            skipped: Some(meta.len()),
        })
    }

    /// Sizes in bytes of the old and new version of a file.
    pub fn diff_sizes(&self, file: &FileDiff, source: DiffSource) -> (Option<u64>, Option<u64>) {
        let blob = |id: Option<Oid>| id.and_then(|id| self.blob_size(id).ok()).map(|s| s as u64);
        let old = blob(file.old_id);
        let new = if file.kind == ChangeKind::Deleted {
            None
        } else if source == DiffSource::Unstaged {
            std::fs::metadata(self.path().join(&file.path))
                .ok()
                .map(|m| m.len())
        } else {
            blob(file.new_id)
        };
        (old, new)
    }

    /// Commits between two revisions of the submodule at `path`.
    pub fn submodule_commits(
        &self,
        path: &str,
        old: Option<Oid>,
        new: Option<Oid>,
    ) -> Result<SubmoduleCommits> {
        let dir = self.path().join(path);
        // Without its own .git the folder is not checked out; opening it would find the parent.
        if !dir.join(".git").exists() {
            return Err(Error::Other(
                "The submodule is not checked out (run submodule update).".into(),
            ));
        }
        let sub = git2::Repository::open(&dir)?;
        for id in [old, new].into_iter().flatten() {
            if sub.find_commit(id).is_err() {
                return Err(Error::Other(format!(
                    "Commit {} is not in the submodule (fetch it first).",
                    short(id)
                )));
            }
        }
        // Commits reachable from `from` but not from `hide`, newest first.
        let walk = |from: Oid, hide: Option<Oid>| -> Result<(Vec<CommitInfo>, bool)> {
            let mut walk = sub.revwalk()?;
            walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
            walk.push(from)?;
            if let Some(h) = hide {
                walk.hide(h)?;
            }
            let mut list = Vec::new();
            for oid in walk {
                if list.len() == SUBMODULE_LIMIT {
                    return Ok((list, true));
                }
                list.push(commit_info(&sub.find_commit(oid?)?));
            }
            Ok((list, false))
        };
        let mut out = SubmoduleCommits::default();
        if let Some(new) = new {
            let (list, more) = walk(new, old)?;
            out.added = list;
            out.truncated |= more;
        }
        if let (Some(old), Some(new)) = (old, new) {
            let (list, more) = walk(old, Some(new))?;
            out.removed = list;
            out.truncated |= more;
        }
        Ok(out)
    }
}

/// Commits that a submodule change brings in (`added`) or drops (`removed`,
/// when the submodule moved back or to another branch).
#[derive(Debug, Clone, Default)]
pub struct SubmoduleCommits {
    pub added: Vec<CommitInfo>,
    pub removed: Vec<CommitInfo>,
    /// More commits than are listed.
    pub truncated: bool,
}

fn collect(diff: &git2::Diff) -> Result<Vec<FileDiff>> {
    let mut out = Vec::with_capacity(diff.deltas().len());
    for idx in 0..diff.deltas().len() {
        let patch = git2::Patch::from_diff(diff, idx)?;
        let delta = diff
            .get_delta(idx)
            .ok_or_else(|| Error::Other("missing diff delta".into()))?;
        out.push(patch_to_file_diff(&delta, patch.as_ref())?);
    }
    Ok(out)
}

/// Whether the change is to a submodule (a gitlink).
pub fn is_submodule(file: &FileDiff) -> bool {
    file.old_mode == FILEMODE_SUBMODULE || file.new_mode == FILEMODE_SUBMODULE
}

/// Submodule commit ids on each side, from the "Subproject commit" lines.
pub fn submodule_ids(file: &FileDiff) -> (Option<Oid>, Option<Oid>) {
    let (mut old, mut new) = (None, None);
    for line in file.hunks.iter().flat_map(|h| &h.lines) {
        let Some(id) = line
            .text
            .strip_prefix("Subproject commit ")
            .and_then(|s| Oid::from_str(s.trim().trim_end_matches("-dirty")).ok())
        else {
            continue;
        };
        match line.kind {
            LineKind::Removed => old = Some(id),
            LineKind::Added => new = Some(id),
            LineKind::Context => (old, new) = (Some(id), Some(id)),
        }
    }
    let side = |mode: u32, id: Option<Oid>| (mode == FILEMODE_SUBMODULE).then_some(id).flatten();
    (
        old.or(side(file.old_mode, file.old_id)),
        new.or(side(file.new_mode, file.new_id)),
    )
}

/// A Git LFS pointer file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsPointer {
    /// `sha256:...`
    pub oid: String,
    pub size: u64,
    /// The pointer text, to hand to `git lfs smudge`.
    pub text: String,
}

pub fn parse_lfs_pointer(text: &str) -> Option<LfsPointer> {
    let mut lines = text.lines();
    if !lines
        .next()?
        .starts_with("version https://git-lfs.github.com/spec/")
    {
        return None;
    }
    let (mut oid, mut size) = (None, None);
    for l in lines {
        if let Some(v) = l.strip_prefix("oid ") {
            oid = Some(v.trim().to_owned());
        } else if let Some(v) = l.strip_prefix("size ") {
            size = v.trim().parse().ok();
        }
    }
    Some(LfsPointer {
        oid: oid?,
        size: size?,
        text: text.to_owned(),
    })
}

/// Old and new LFS pointer of a file whose diff shows pointer files.
pub fn lfs_pointers(file: &FileDiff) -> (Option<LfsPointer>, Option<LfsPointer>) {
    // Pointers are a few lines, so one hunk holds both whole files.
    if file.binary || file.hunks.len() != 1 {
        return (None, None);
    }
    let side = |keep: LineKind| {
        let text: String = file.hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Context || l.kind == keep)
            .map(|l| format!("{}\n", l.text))
            .collect();
        parse_lfs_pointer(&text)
    };
    (side(LineKind::Removed), side(LineKind::Added))
}

/// Whether the `git lfs` command is installed.
pub fn lfs_available(git: &Git) -> bool {
    git.run(&["lfs", "version"]).is_ok()
}

/// Content of an LFS object, downloading it if needed (`git lfs smudge`).
pub fn lfs_smudge(git: &Git, path: &str, pointer: &LfsPointer) -> Result<Vec<u8>> {
    use std::io::Write;
    use std::process::Stdio;
    let mut cmd = git.command(&["lfs", "smudge", "--", path]);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Cli(format!("git lfs: {e}")))?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let data = pointer.text.clone().into_bytes();
    std::thread::spawn(move || stdin.write_all(&data));
    let out = child.wait_with_output()?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(Error::Cli(lossy(&out.stderr).trim().to_owned()))
    }
}

pub(crate) fn patch_to_file_diff(
    delta: &git2::DiffDelta,
    patch: Option<&git2::Patch>,
) -> Result<FileDiff> {
    use git2::Delta;
    let path_of = |f: git2::DiffFile| f.path().map(|p| p.to_string_lossy().into_owned());
    let new_path = path_of(delta.new_file());
    let old_path = path_of(delta.old_file());
    let kind = match delta.status() {
        Delta::Added => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Renamed => ChangeKind::Renamed,
        Delta::Copied => ChangeKind::Copied,
        Delta::Typechange => ChangeKind::TypeChange,
        Delta::Untracked => ChangeKind::Untracked,
        Delta::Conflicted => ChangeKind::Conflicted,
        _ => ChangeKind::Modified,
    };
    let id = |f: git2::DiffFile| Some(f.id()).filter(|id| !id.is_zero());
    let mut file = FileDiff {
        path: new_path
            .clone()
            .or_else(|| old_path.clone())
            .unwrap_or_default(),
        old_path: if matches!(kind, ChangeKind::Renamed | ChangeKind::Copied) {
            old_path
        } else {
            None
        },
        kind,
        binary: delta.flags().is_binary(),
        old_id: id(delta.old_file()),
        new_id: id(delta.new_file()),
        hunks: Vec::new(),
        additions: 0,
        deletions: 0,
        old_mode: u32::from(delta.old_file().mode()),
        new_mode: u32::from(delta.new_file().mode()),
        skipped: None,
    };
    let Some(patch) = patch else {
        file.binary = true;
        return Ok(file);
    };
    for h in 0..patch.num_hunks() {
        let (hunk, count) = patch.hunk(h)?;
        let mut out = Hunk {
            header: lossy(hunk.header()).trim_end().to_owned(),
            old_start: hunk.old_start(),
            old_lines: hunk.old_lines(),
            new_start: hunk.new_start(),
            new_lines: hunk.new_lines(),
            lines: Vec::with_capacity(count),
        };
        for l in 0..count {
            let line = patch.line_in_hunk(h, l)?;
            let kind = match line.origin() {
                '+' => LineKind::Added,
                '-' => LineKind::Removed,
                ' ' => LineKind::Context,
                // "\ No newline at end of file" markers belong to the previous line.
                _ => {
                    if let Some(prev) = out.lines.last_mut() {
                        prev.no_newline = true;
                    }
                    continue;
                }
            };
            match kind {
                LineKind::Added => file.additions += 1,
                LineKind::Removed => file.deletions += 1,
                LineKind::Context => {}
            }
            let text = lossy(line.content());
            out.lines.push(DiffLine {
                kind,
                text: text
                    .strip_suffix('\n')
                    .unwrap_or(&text)
                    .trim_end_matches('\r')
                    .to_owned(),
                old_no: line.old_lineno(),
                new_no: line.new_lineno(),
                no_newline: false,
            });
        }
        file.hunks.push(out);
    }
    Ok(file)
}

/// Whether lines of this file can be staged one by one. New, deleted, renamed
/// and binary files are staged as a whole.
pub fn supports_line_staging(file: &FileDiff) -> bool {
    !file.binary && matches!(file.kind, ChangeKind::Modified)
}

/// Builds a patch with only the selected lines of `file`.
///
/// With `reverse = false` the patch applies forward to the old side (staging
/// unstaged changes into the index). With `reverse = true` it is meant for
/// `git apply -R` against the new side (unstaging from the index, or
/// discarding from the working directory). Returns `None` when no added or
/// removed line is selected.
pub fn partial_patch(
    file: &FileDiff,
    selected: impl Fn(usize, usize) -> bool,
    reverse: bool,
) -> Option<String> {
    let mut body = String::new();
    for (h, hunk) in file.hunks.iter().enumerate() {
        let mut lines = String::new();
        let (mut old, mut new, mut changes) = (0, 0, 0);
        for (l, line) in hunk.lines.iter().enumerate() {
            let sel = selected(h, l);
            let prefix = match (line.kind, sel, reverse) {
                (LineKind::Context, _, _) => ' ',
                (LineKind::Added, true, _) => '+',
                (LineKind::Removed, true, _) => '-',
                // Unselected lines that exist on the side the patch applies to stay as context.
                (LineKind::Added, false, true) | (LineKind::Removed, false, false) => ' ',
                (LineKind::Added, false, false) | (LineKind::Removed, false, true) => continue,
            };
            match prefix {
                '+' => {
                    new += 1;
                    changes += 1;
                }
                '-' => {
                    old += 1;
                    changes += 1;
                }
                _ => {
                    old += 1;
                    new += 1;
                }
            }
            lines.push(prefix);
            lines.push_str(&line.text);
            lines.push('\n');
            if line.no_newline {
                lines.push_str("\\ No newline at end of file\n");
            }
        }
        if changes == 0 {
            continue;
        }
        body.push_str(&format!(
            "@@ -{},{old} +{},{new} @@\n",
            hunk.old_start, hunk.new_start
        ));
        body.push_str(&lines);
    }
    if body.is_empty() {
        return None;
    }
    let old_path = file.old_path.as_deref().unwrap_or(&file.path);
    Some(format!(
        "diff --git a/{old_path} b/{p}\n--- a/{old_path}\n+++ b/{p}\n{body}",
        p = file.path
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops;
    use crate::testutil::TestRepo;

    fn setup() -> TestRepo {
        let t = TestRepo::new();
        t.commit("f.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n", "base");
        // Two separate hunks: change line 2 and line 11.
        t.write("f.txt", "1\nTWO\n3\n4\n5\n6\n7\n8\n9\n10\nELEVEN\n12\n");
        t
    }

    #[test]
    fn stages_one_line_of_a_hunk() {
        let t = setup();
        let repo = t.repo();
        let file = repo
            .file_diff(DiffSource::Unstaged, "f.txt")
            .unwrap()
            .unwrap();
        assert_eq!(file.hunks.len(), 2);
        // Select only the removed "2" in the first hunk (not the added "TWO").
        let first = &file.hunks[0];
        let removed = first
            .lines
            .iter()
            .position(|l| l.kind == LineKind::Removed)
            .unwrap();
        let patch = partial_patch(&file, |h, l| h == 0 && l == removed, false).unwrap();
        ops::apply_patch(&t.git, &patch, ops::PatchTarget::Index, false).unwrap();

        let staged = t.git.run(&["diff", "--cached"]).unwrap();
        assert!(staged.contains("-2\n"));
        assert!(!staged.contains("+TWO"));
        assert!(!staged.contains("ELEVEN"));
        // The working copy is untouched.
        assert!(t.read("f.txt").contains("TWO"));
    }

    #[test]
    fn unstages_and_discards_a_hunk() {
        let t = setup();
        t.git.run(&["add", "f.txt"]).unwrap();
        let repo = t.repo();
        let staged = repo
            .file_diff(DiffSource::Staged, "f.txt")
            .unwrap()
            .unwrap();
        let patch = partial_patch(&staged, |h, _| h == 1, true).unwrap();
        ops::apply_patch(&t.git, &patch, ops::PatchTarget::Index, true).unwrap();
        let cached = t.git.run(&["diff", "--cached"]).unwrap();
        assert!(cached.contains("TWO") && !cached.contains("ELEVEN"));

        // Discard the second hunk from the working copy.
        let unstaged = repo
            .file_diff(DiffSource::Unstaged, "f.txt")
            .unwrap()
            .unwrap();
        let patch = partial_patch(&unstaged, |_, _| true, true).unwrap();
        ops::apply_patch(&t.git, &patch, ops::PatchTarget::WorkingCopy, true).unwrap();
        assert!(!t.read("f.txt").contains("ELEVEN"));
        assert!(t.read("f.txt").contains("TWO"));
    }

    #[test]
    fn nothing_selected_means_no_patch() {
        let t = setup();
        let file = t
            .repo()
            .file_diff(DiffSource::Unstaged, "f.txt")
            .unwrap()
            .unwrap();
        assert!(partial_patch(&file, |_, _| false, false).is_none());
    }

    #[test]
    fn handles_missing_final_newline() {
        let t = TestRepo::new();
        t.commit("f.txt", "a\nb", "base");
        t.write("f.txt", "a\nc");
        let file = t
            .repo()
            .file_diff(DiffSource::Unstaged, "f.txt")
            .unwrap()
            .unwrap();
        let patch = partial_patch(&file, |_, _| true, false).unwrap();
        ops::apply_patch(&t.git, &patch, ops::PatchTarget::Index, false).unwrap();
        assert_eq!(t.git.run(&["show", ":f.txt"]).unwrap(), "a\nc");
    }

    fn lines(f: &FileDiff) -> usize {
        f.hunks.iter().map(|h| h.lines.len()).sum()
    }

    #[test]
    fn context_lines_and_full_file() {
        let t = TestRepo::new();
        let text: String = (1..=30).map(|i| format!("{i}\n")).collect();
        t.commit("f.txt", &text, "base");
        t.write("f.txt", &text.replace("\n15\n", "\nX\n"));
        let repo = t.repo();
        let get = |cfg: DiffConfig| {
            repo.file_diff_with(DiffSource::Unstaged, "f.txt", &cfg)
                .unwrap()
                .unwrap()
        };
        let one = DiffConfig {
            context_lines: 1,
            ..Default::default()
        };
        assert_eq!(lines(&get(one)), 4);
        assert_eq!(lines(&get(DiffConfig::default())), 8);
        let full = DiffConfig {
            full_file: true,
            ..Default::default()
        };
        assert_eq!(lines(&get(full)), 31);

        // Staging with the whole file as context still works.
        let file = get(full);
        let patch = partial_patch(&file, |_, _| true, false).unwrap();
        ops::apply_patch(&t.git, &patch, ops::PatchTarget::Index, false).unwrap();
        assert!(t.git.run(&["diff", "--cached"]).unwrap().contains("+X"));

        // Commits take the same options.
        t.git.run(&["commit", "-q", "-m", "x"]).unwrap();
        let head = repo.head_oid().unwrap();
        let f = repo
            .commit_file_diff_with(head, "f.txt", None, &one)
            .unwrap()
            .unwrap();
        assert_eq!(lines(&f), 4);
    }

    #[test]
    fn ignores_whitespace() {
        let t = TestRepo::new();
        t.commit("f.txt", "a b\nc\n", "base");
        t.write("f.txt", "a   b\nc\n");
        let repo = t.repo();
        let cfg = DiffConfig {
            ignore_whitespace: true,
            ..Default::default()
        };
        let f = repo.file_diff_with(DiffSource::Unstaged, "f.txt", &cfg);
        assert!(f.unwrap().is_none_or(|f| f.hunks.is_empty()));
        let f = repo.file_diff_with(DiffSource::Unstaged, "f.txt", &DiffConfig::default());
        assert_eq!(f.unwrap().unwrap().additions, 1);
    }

    #[test]
    fn skips_large_untracked_files_until_asked() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "base");
        t.write("big.txt", &"x\n".repeat(100));
        t.write("a.txt", &"y\n".repeat(100));
        let repo = t.repo();
        let small_limit = DiffConfig {
            max_untracked_bytes: Some(50),
            ..Default::default()
        };
        let f = repo
            .file_diff_with(DiffSource::Unstaged, "big.txt", &small_limit)
            .unwrap()
            .unwrap();
        assert_eq!((f.skipped, f.hunks.len()), (Some(200), 0));
        // Tracked files are always loaded.
        let f = repo
            .file_diff_with(DiffSource::Unstaged, "a.txt", &small_limit)
            .unwrap()
            .unwrap();
        assert_eq!(f.skipped, None);
        let unlimited = DiffConfig {
            max_untracked_bytes: None,
            ..Default::default()
        };
        let f = repo
            .file_diff_with(DiffSource::Unstaged, "big.txt", &unlimited)
            .unwrap()
            .unwrap();
        assert_eq!((f.skipped, f.additions), (None, 100));
        assert_eq!(repo.diff_sizes(&f, DiffSource::Unstaged), (None, Some(200)));
    }

    #[test]
    fn reports_file_modes() {
        let t = TestRepo::new();
        t.commit("run.sh", "echo\n", "base");
        t.git
            .run(&["update-index", "--chmod=+x", "run.sh"])
            .unwrap();
        let f = t
            .repo()
            .file_diff(DiffSource::Staged, "run.sh")
            .unwrap()
            .unwrap();
        assert_eq!((f.old_mode, f.new_mode), (0o100644, 0o100755));
        assert!(f.hunks.is_empty());
    }

    #[test]
    fn lists_submodule_commits() {
        let sub = TestRepo::new();
        sub.commit("s.txt", "1\n", "one");
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "base");
        let url = sub.path().to_string_lossy().replace('\\', "/");
        t.git
            .run(&[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                &url,
                "sub",
            ])
            .unwrap();
        t.git.run(&["commit", "-q", "-m", "add sub"]).unwrap();
        let inner = Git::new(t.path().join("sub"))
            .with_env("GIT_AUTHOR_NAME", "T")
            .with_env("GIT_AUTHOR_EMAIL", "t@example.com")
            .with_env("GIT_COMMITTER_NAME", "T")
            .with_env("GIT_COMMITTER_EMAIL", "t@example.com");
        inner.run(&["config", "commit.gpgsign", "false"]).unwrap();
        for (i, msg) in ["two", "three"].iter().enumerate() {
            std::fs::write(t.path().join("sub/s.txt"), format!("{i}\n")).unwrap();
            inner.run(&["commit", "-q", "-am", msg]).unwrap();
        }
        let repo = t.repo();
        let f = repo
            .file_diff(DiffSource::Unstaged, "sub")
            .unwrap()
            .unwrap();
        assert!(is_submodule(&f));
        let (old, new) = submodule_ids(&f);
        assert!(old.is_some() && new.is_some() && old != new);
        let commits = repo.submodule_commits("sub", old, new).unwrap();
        let names: Vec<_> = commits.added.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(names, ["three", "two"]);
        assert!(commits.removed.is_empty());
        // Going back lists the same commits as removed.
        let back = repo.submodule_commits("sub", new, old).unwrap();
        assert_eq!((back.added.len(), back.removed.len()), (0, 2));
        assert!(repo.submodule_commits("missing", old, new).is_err());
    }

    #[test]
    fn reads_lfs_pointers() {
        let t = TestRepo::new();
        let ptr = |oid: &str, size: u32| {
            format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n")
        };
        t.commit("big.bin", &ptr("aaaa", 10), "base");
        t.write("big.bin", &ptr("bbbb", 2048));
        let f = t
            .repo()
            .file_diff(DiffSource::Unstaged, "big.bin")
            .unwrap()
            .unwrap();
        let (old, new) = lfs_pointers(&f);
        assert_eq!(old.unwrap().size, 10);
        let new = new.unwrap();
        assert_eq!((new.oid.as_str(), new.size), ("sha256:bbbb", 2048));
        assert_eq!(new.text, ptr("bbbb", 2048));
        assert!(parse_lfs_pointer("hello\n").is_none());
    }
}
