use std::collections::HashMap;
use std::path::{Path, PathBuf};

use git2::{DiffOptions, Oid, StatusOptions};

use crate::cli::Git;
use crate::diff::patch_to_file_diff;
use crate::model::*;
use crate::{Error, Result, lossy};

/// Field and record separators for parsing `git log --format` output.
const FS: char = '\x1f';
const RS: char = '\x1e';

pub struct Repo {
    inner: git2::Repository,
    path: PathBuf,
}

impl Repo {
    /// Opens the repository that contains `path` (searches parent folders).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let inner = git2::Repository::discover(path)?;
        let path = inner
            .workdir()
            .unwrap_or_else(|| inner.path())
            .to_path_buf();
        Ok(Self { inner, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn git(&self) -> Git {
        Git::new(&self.path)
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// Short name of the current branch, or `None` when HEAD is detached or unborn.
    pub fn head_branch(&self) -> Option<String> {
        let head = self.inner.head().ok()?;
        head.is_branch().then(|| lossy(head.shorthand_bytes()))
    }

    pub fn head_oid(&self) -> Option<Oid> {
        self.inner.head().ok()?.target()
    }

    /// Branch HEAD points at, even when it has no commits yet.
    pub fn head_ref_name(&self) -> Option<String> {
        let head = self.inner.find_reference("HEAD").ok()?;
        let target = head.symbolic_target_bytes()?;
        Some(lossy(target).trim_start_matches("refs/heads/").to_owned())
    }

    pub fn state(&self) -> RepoState {
        use git2::RepositoryState as S;
        match self.inner.state() {
            S::Clean => RepoState::Clean,
            S::Merge => RepoState::Merge,
            S::Revert | S::RevertSequence => RepoState::Revert,
            S::CherryPick | S::CherryPickSequence => RepoState::CherryPick,
            S::Bisect => RepoState::Bisect,
            S::Rebase | S::RebaseInteractive | S::RebaseMerge => RepoState::Rebase,
            _ => RepoState::Other,
        }
    }

    pub fn branches(&self) -> Result<Vec<BranchInfo>> {
        let mut out = Vec::new();
        for item in self.inner.branches(None)? {
            let (branch, kind) = item?;
            let name = lossy(branch.name_bytes()?);
            // Skip the "origin/HEAD" pointer.
            if kind == git2::BranchType::Remote && name.ends_with("/HEAD") {
                continue;
            }
            let target = branch.get().target();
            let upstream = branch.upstream().ok();
            let ahead_behind = match (target, upstream.as_ref().and_then(|u| u.get().target())) {
                (Some(local), Some(remote)) => self.inner.graph_ahead_behind(local, remote).ok(),
                _ => None,
            };
            out.push(BranchInfo {
                name,
                kind: match kind {
                    git2::BranchType::Local => BranchKind::Local,
                    git2::BranchType::Remote => BranchKind::Remote,
                },
                is_head: branch.is_head(),
                target,
                upstream: upstream.and_then(|u| u.name_bytes().ok().map(lossy)),
                ahead_behind,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn tags(&self) -> Result<Vec<TagInfo>> {
        let mut tags = Vec::new();
        for r in self.inner.references_glob("refs/tags/*")? {
            let r = r?;
            let Ok(commit) = r.peel_to_commit() else {
                continue;
            };
            let annotated = r.peel_to_tag().is_ok();
            tags.push(TagInfo {
                name: lossy(r.shorthand_bytes()),
                target: commit.id(),
                annotated,
            });
        }
        tags.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(tags)
    }

    /// Branches and tags grouped by the commit they point at.
    pub fn ref_labels(&self) -> Result<HashMap<Oid, Vec<RefLabel>>> {
        let head = self.inner.head().ok().map(|h| h.name_bytes().to_vec());
        let mut map: HashMap<Oid, Vec<RefLabel>> = HashMap::new();
        for r in self.inner.references()? {
            let r = r?;
            let kind = if r.is_branch() {
                RefKind::Local
            } else if r.is_remote() {
                RefKind::Remote
            } else if r.is_tag() {
                RefKind::Tag
            } else {
                continue;
            };
            let name = lossy(r.shorthand_bytes());
            if kind == RefKind::Remote && name.ends_with("/HEAD") {
                continue;
            }
            // Annotated tags point at a tag object; peel to the commit.
            let Ok(commit) = r.peel_to_commit() else {
                continue;
            };
            let is_head = head.as_deref() == Some(r.name_bytes());
            map.entry(commit.id()).or_default().push(RefLabel {
                name,
                kind,
                is_head,
            });
        }
        for labels in map.values_mut() {
            labels.sort_by_key(|l| (!l.is_head, l.kind as u8, l.name.clone()));
        }
        Ok(map)
    }

    /// Commits reachable from all branches and HEAD, newest first, up to `limit`.
    pub fn commits(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        let mut walk = self.inner.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
        // An empty repository has no HEAD yet.
        let _ = walk.push_head();
        walk.push_glob("refs/heads/*")?;
        walk.push_glob("refs/remotes/*")?;
        walk.push_glob("refs/tags/*")?;

        let mut out = Vec::new();
        for oid in walk.take(limit) {
            let commit = self.inner.find_commit(oid?)?;
            out.push(commit_info(&commit));
        }
        Ok(out)
    }

    pub fn commit_details(&self, id: Oid) -> Result<CommitDetails> {
        let commit = self.inner.find_commit(id)?;
        let committer = commit.committer();
        Ok(CommitDetails {
            info: commit_info(&commit),
            message: lossy(commit.message_bytes()),
            committer: lossy(committer.name_bytes()),
            committer_email: lossy(committer.email_bytes()),
            commit_time: commit.time().seconds(),
            commit_offset_minutes: commit.time().offset_minutes(),
        })
    }

    /// Changes introduced by a commit, compared with its first parent.
    pub fn commit_diff(&self, id: Oid) -> Result<Vec<FileDiff>> {
        let commit = self.inner.find_commit(id)?;
        let new_tree = commit.tree()?;
        let old_tree = commit.parent(0).ok().map(|p| p.tree()).transpose()?;
        let mut opts = DiffOptions::new();
        let mut diff =
            self.inner
                .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), Some(&mut opts))?;
        diff.find_similar(None)?;
        collect_diffs(&diff)
    }

    pub fn status(&self) -> Result<WorkingStatus> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true)
            .include_ignored(false);
        let statuses = self.inner.statuses(Some(&mut opts))?;
        let mut out = WorkingStatus::default();
        for entry in statuses.iter() {
            let s = entry.status();
            let path = lossy(entry.path_bytes());
            if s.is_conflicted() {
                out.conflicted.push(FileChange {
                    path,
                    old_path: None,
                    kind: ChangeKind::Conflicted,
                });
                continue;
            }
            let staged = if s.is_index_new() {
                Some(ChangeKind::Added)
            } else if s.is_index_modified() {
                Some(ChangeKind::Modified)
            } else if s.is_index_deleted() {
                Some(ChangeKind::Deleted)
            } else if s.is_index_renamed() {
                Some(ChangeKind::Renamed)
            } else if s.is_index_typechange() {
                Some(ChangeKind::TypeChange)
            } else {
                None
            };
            if let Some(kind) = staged {
                let (path, old_path) = match entry.head_to_index() {
                    Some(d) if kind == ChangeKind::Renamed => (
                        d.new_file()
                            .path()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or(path.clone()),
                        d.old_file()
                            .path()
                            .map(|p| p.to_string_lossy().into_owned()),
                    ),
                    _ => (path.clone(), None),
                };
                out.staged.push(FileChange {
                    path,
                    old_path,
                    kind,
                });
            }
            let unstaged = if s.is_wt_new() {
                Some(ChangeKind::Untracked)
            } else if s.is_wt_modified() {
                Some(ChangeKind::Modified)
            } else if s.is_wt_deleted() {
                Some(ChangeKind::Deleted)
            } else if s.is_wt_typechange() {
                Some(ChangeKind::TypeChange)
            } else if s.is_wt_renamed() {
                Some(ChangeKind::Renamed)
            } else {
                None
            };
            if let Some(kind) = unstaged {
                out.unstaged.push(FileChange {
                    path,
                    old_path: None,
                    kind,
                });
            }
        }
        for list in [&mut out.unstaged, &mut out.staged, &mut out.conflicted] {
            list.sort_by(|a, b| a.path.cmp(&b.path));
        }
        Ok(out)
    }

    /// Diff of one file in the working directory or the index.
    pub fn file_diff(&self, source: DiffSource, path: &str) -> Result<Option<FileDiff>> {
        let mut opts = DiffOptions::new();
        opts.pathspec(path).disable_pathspec_match(true);
        let diff = match source {
            DiffSource::Unstaged => {
                opts.include_untracked(true)
                    .recurse_untracked_dirs(true)
                    .show_untracked_content(true);
                self.inner.diff_index_to_workdir(None, Some(&mut opts))?
            }
            DiffSource::Staged => {
                let head = self.inner.head().ok().and_then(|h| h.peel_to_tree().ok());
                self.inner
                    .diff_tree_to_index(head.as_ref(), None, Some(&mut opts))?
            }
            DiffSource::Commit(id) => {
                let files = self.commit_diff(id)?;
                return Ok(files.into_iter().find(|f| f.path == path));
            }
        };
        Ok(collect_diffs(&diff)?.into_iter().next())
    }

    pub fn stashes(&self) -> Result<Vec<StashInfo>> {
        let out = self
            .git()
            .run(&["stash", "list", &format!("--format=%H{FS}%gs{RS}")])?;
        Ok(out
            .split(RS)
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .enumerate()
            .filter_map(|(index, rec)| {
                let (id, message) = rec.split_once(FS)?;
                Some(StashInfo {
                    index,
                    id: Oid::from_str(id).ok()?,
                    message: message.to_owned(),
                })
            })
            .collect())
    }

    pub fn remotes(&self) -> Result<Vec<RemoteInfo>> {
        let names = self.inner.remotes()?;
        let mut out = Vec::new();
        for name in names.iter().flatten().flatten() {
            let remote = self.inner.find_remote(name)?;
            out.push(RemoteInfo {
                name: name.to_owned(),
                url: lossy(remote.url_bytes()),
            });
        }
        Ok(out)
    }

    pub fn submodules(&self) -> Result<Vec<SubmoduleInfo>> {
        Ok(self
            .inner
            .submodules()?
            .iter()
            .map(|s| SubmoduleInfo {
                name: s.name().ok().unwrap_or_default().to_owned(),
                path: s.path().to_string_lossy().into_owned(),
                url: s.url().ok().flatten().map(str::to_owned),
                head: s.head_id(),
                initialized: s.workdir_id().is_some(),
            })
            .collect())
    }

    pub fn worktrees(&self) -> Result<Vec<WorktreeInfo>> {
        let out = self.git().run(&["worktree", "list", "--porcelain"])?;
        Ok(parse_worktrees(&out))
    }

    pub fn blame(&self, path: &str) -> Result<Vec<BlameLine>> {
        let blame = self.inner.blame_file(Path::new(path), None)?;
        let head = self.inner.head()?.peel_to_tree()?;
        let blob = head
            .get_path(Path::new(path))?
            .to_object(&self.inner)?
            .peel_to_blob()?;
        let content = lossy(blob.content());
        let mut out = Vec::new();
        let mut last = None;
        for (i, text) in content.lines().enumerate() {
            let Some(hunk) = blame.get_line(i + 1) else {
                continue;
            };
            let sig = hunk.final_signature();
            let commit = hunk.final_commit_id();
            out.push(BlameLine {
                commit,
                author: sig
                    .as_ref()
                    .map(|s| lossy(s.name_bytes()))
                    .unwrap_or_default(),
                time: sig.as_ref().map(|s| s.when().seconds()).unwrap_or_default(),
                offset_minutes: sig
                    .as_ref()
                    .map(|s| s.when().offset_minutes())
                    .unwrap_or_default(),
                text: text.to_owned(),
                block_start: last != Some(commit),
            });
            last = Some(commit);
        }
        Ok(out)
    }

    /// Commits that touched `path`, following renames.
    pub fn file_history(&self, path: &str) -> Result<Vec<CommitInfo>> {
        let fmt = format!("--format=%H{FS}%P{FS}%an{FS}%ae{FS}%at{FS}%s{RS}");
        let out = self.git().run(&["log", "--follow", &fmt, "--", path])?;
        Ok(parse_log(&out))
    }

    /// Commits whose changes add or remove `term` (`git log -S`).
    pub fn search_content(&self, term: &str) -> Result<Vec<Oid>> {
        let out = self
            .git()
            .run(&["log", "--all", "--format=%H", &format!("-S{term}")])?;
        Ok(out
            .lines()
            .filter_map(|l| Oid::from_str(l.trim()).ok())
            .collect())
    }

    pub fn blob(&self, id: Oid) -> Result<Vec<u8>> {
        Ok(self.inner.find_blob(id)?.content().to_vec())
    }

    pub fn read_workdir_file(&self, path: &str) -> Result<Vec<u8>> {
        Ok(std::fs::read(self.path.join(path))?)
    }

    /// Base, ours and theirs versions of a conflicted file (any can be missing).
    pub fn conflict_versions(&self, path: &str) -> Result<[Option<String>; 3]> {
        let index = self.inner.index()?;
        let mut out = [None, None, None];
        for conflict in index.conflicts()? {
            let conflict = conflict?;
            let entries = [conflict.ancestor, conflict.our, conflict.their];
            let matches = entries.iter().flatten().any(|e| lossy(&e.path) == path);
            if !matches {
                continue;
            }
            for (slot, entry) in out.iter_mut().zip(entries) {
                if let Some(e) = entry {
                    *slot = Some(lossy(self.inner.find_blob(e.id)?.content()));
                }
            }
        }
        Ok(out)
    }

    pub fn identity(&self) -> Identity {
        let Ok(cfg) = self.inner.config() else {
            return Identity::default();
        };
        let get = |k: &str| cfg.get_string(k).ok().filter(|s| !s.is_empty());
        Identity {
            name: get("user.name"),
            email: get("user.email"),
            signing_key: get("user.signingkey"),
            gpg_sign: cfg.get_bool("commit.gpgsign").unwrap_or(false),
            ssh_command: get("core.sshCommand"),
        }
    }

    /// Whether the repository tracks files with Git LFS.
    pub fn uses_lfs(&self) -> bool {
        std::fs::read_to_string(self.path.join(".gitattributes"))
            .is_ok_and(|s| s.contains("filter=lfs"))
    }

    /// Message git prepared for the next commit (merge, revert, cherry-pick).
    pub fn prepared_message(&self) -> Option<String> {
        let msg = self.inner.message().ok()?;
        let cleaned: Vec<&str> = msg.lines().filter(|l| !l.starts_with('#')).collect();
        Some(cleaned.join("\n").trim().to_owned()).filter(|m| !m.is_empty())
    }

    pub fn find_commit_info(&self, id: Oid) -> Result<CommitInfo> {
        Ok(commit_info(&self.inner.find_commit(id)?))
    }

    /// Resolves a revision like `HEAD~2` or a branch name to a commit id.
    pub fn resolve(&self, rev: &str) -> Result<Oid> {
        Ok(self.inner.revparse_single(rev)?.peel_to_commit()?.id())
    }

    /// Commits from `base` (exclusive) to HEAD, oldest first, for interactive rebase.
    /// Commits after `base` up to HEAD, oldest first. `None` means all commits from the root.
    pub fn commits_since(&self, base: Option<Oid>) -> Result<Vec<CommitInfo>> {
        let mut walk = self.inner.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;
        walk.push_head()?;
        if let Some(base) = base {
            walk.hide(base)?;
        }
        let mut out = Vec::new();
        for oid in walk {
            out.push(commit_info(&self.inner.find_commit(oid?)?));
        }
        Ok(out)
    }

    pub fn is_ancestor(&self, ancestor: Oid, of: Oid) -> bool {
        ancestor == of
            || self
                .inner
                .graph_descendant_of(of, ancestor)
                .unwrap_or(false)
    }

    pub fn last_commit_message(&self) -> Option<String> {
        let commit = self.inner.head().ok()?.peel_to_commit().ok()?;
        Some(lossy(commit.message_bytes()).trim_end().to_owned())
    }
}

fn commit_info(commit: &git2::Commit) -> CommitInfo {
    let author = commit.author();
    CommitInfo {
        id: commit.id(),
        summary: lossy(commit.summary_bytes().unwrap_or_default()),
        author: lossy(author.name_bytes()),
        email: lossy(author.email_bytes()),
        time: author.when().seconds(),
        offset_minutes: author.when().offset_minutes(),
        parents: commit.parent_ids().collect(),
    }
}

fn collect_diffs(diff: &git2::Diff) -> Result<Vec<FileDiff>> {
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

fn parse_log(out: &str) -> Vec<CommitInfo> {
    out.split(RS)
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .filter_map(|rec| {
            let f: Vec<&str> = rec.splitn(6, FS).collect();
            let [id, parents, author, email, time, summary] = f.as_slice() else {
                return None;
            };
            Some(CommitInfo {
                id: Oid::from_str(id).ok()?,
                parents: parents
                    .split_whitespace()
                    .filter_map(|p| Oid::from_str(p).ok())
                    .collect(),
                author: (*author).to_owned(),
                email: (*email).to_owned(),
                time: time.parse().ok()?,
                offset_minutes: 0,
                summary: (*summary).to_owned(),
            })
        })
        .collect()
}

fn parse_worktrees(out: &str) -> Vec<WorktreeInfo> {
    let mut list = Vec::new();
    for block in out.split("\n\n").filter(|b| !b.trim().is_empty()) {
        let mut wt = WorktreeInfo {
            path: String::new(),
            branch: None,
            head: None,
            is_main: list.is_empty(),
            locked: false,
        };
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                wt.path = p.to_owned();
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                wt.head = Some(h.to_owned());
            } else if let Some(b) = line.strip_prefix("branch ") {
                wt.branch = Some(b.trim_start_matches("refs/heads/").to_owned());
            } else if line.starts_with("locked") {
                wt.locked = true;
            }
        }
        list.push(wt);
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn opens_own_repository() {
        let repo = Repo::open(env!("CARGO_MANIFEST_DIR")).unwrap();
        assert!(repo.path().join("Cargo.toml").exists());
    }

    #[test]
    fn reads_status_and_diffs() {
        let t = TestRepo::new();
        t.commit("a.txt", "one\ntwo\nthree\n", "first");
        t.write("a.txt", "one\nTWO\nthree\n");
        t.write("new.txt", "hello\n");
        t.write("b.txt", "staged\n");
        t.git.run(&["add", "b.txt"]).unwrap();

        let repo = t.repo();
        let st = repo.status().unwrap();
        let unstaged: Vec<_> = st
            .unstaged
            .iter()
            .map(|f| (f.path.as_str(), f.kind))
            .collect();
        assert_eq!(
            unstaged,
            [
                ("a.txt", ChangeKind::Modified),
                ("new.txt", ChangeKind::Untracked)
            ]
        );
        assert_eq!(st.staged[0].path, "b.txt");
        assert_eq!(st.staged[0].kind, ChangeKind::Added);

        let d = repo
            .file_diff(DiffSource::Unstaged, "a.txt")
            .unwrap()
            .unwrap();
        assert_eq!((d.additions, d.deletions), (1, 1));
        let untracked = repo
            .file_diff(DiffSource::Unstaged, "new.txt")
            .unwrap()
            .unwrap();
        assert_eq!(untracked.additions, 1);
        let staged = repo
            .file_diff(DiffSource::Staged, "b.txt")
            .unwrap()
            .unwrap();
        assert_eq!(staged.hunks[0].lines[0].text, "staged");
    }

    #[test]
    fn reads_commit_details_and_diff() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        t.commit("a.txt", "a\nb\n", "second\n\nbody text");
        let repo = t.repo();
        let head = repo.head_oid().unwrap();
        let d = repo.commit_details(head).unwrap();
        assert!(d.message.contains("body text"));
        let files = repo.commit_diff(head).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].additions, 1);
        // Root commit diffs against an empty tree.
        let root = d.info.parents[0];
        assert_eq!(repo.commit_diff(root).unwrap()[0].kind, ChangeKind::Added);
    }

    #[test]
    fn lists_stashes_history_and_blame() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        t.commit("a.txt", "a\nb\n", "second");
        t.write("a.txt", "changed\n");
        t.git.run(&["stash", "push", "-m", "wip"]).unwrap();
        let repo = t.repo();
        let stashes = repo.stashes().unwrap();
        assert_eq!(stashes.len(), 1);
        assert!(stashes[0].message.contains("wip"));

        let history = repo.file_history("a.txt").unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].summary, "second");

        let blame = repo.blame("a.txt").unwrap();
        assert_eq!(blame.len(), 2);
        assert_ne!(blame[0].commit, blame[1].commit);
        assert!(blame[1].block_start);
    }

    #[test]
    fn lists_commits_since_base_or_root() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        let base = t.repo().head_oid().unwrap();
        t.commit("b.txt", "b\n", "second");
        t.commit("c.txt", "c\n", "third");
        let repo = t.repo();
        let summaries = |base| -> Vec<String> {
            repo.commits_since(base)
                .unwrap()
                .into_iter()
                .map(|c| c.summary)
                .collect()
        };
        assert_eq!(summaries(Some(base)), ["second", "third"]);
        assert_eq!(summaries(None), ["first", "second", "third"]);
    }

    #[test]
    fn parses_worktree_list() {
        let out = "worktree /a\nHEAD abc\nbranch refs/heads/main\n\nworktree /b\nHEAD def\ndetached\nlocked\n";
        let w = parse_worktrees(out);
        assert_eq!(w.len(), 2);
        assert!(w[0].is_main);
        assert_eq!(w[0].branch.as_deref(), Some("main"));
        assert!(w[1].locked && w[1].branch.is_none());
    }
}
