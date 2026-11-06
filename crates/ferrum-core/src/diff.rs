//! Diff conversion and partial patches for staging individual lines.

use crate::model::*;
use crate::{Result, lossy};

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
}
