//! Plain data types shared with the UI.

use git2::Oid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchKind {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
pub struct BranchInfo {
    /// Short name, e.g. `main` or `origin/main`.
    pub name: String,
    pub kind: BranchKind,
    pub is_head: bool,
    pub target: Option<Oid>,
    pub upstream: Option<String>,
    /// Commits ahead of / behind the upstream.
    pub ahead_behind: Option<(usize, usize)>,
    /// Upstream set in the config whose branch no longer exists (or whose
    /// remote is missing), e.g. after the remote branch was deleted.
    pub missing_upstream: Option<String>,
    /// Commit time of the tip, in seconds since the Unix epoch.
    pub time: i64,
}

impl BranchInfo {
    /// For a remote branch `origin/feature/x`, returns (`origin`, `feature/x`).
    pub fn split_remote(&self) -> Option<(&str, &str)> {
        (self.kind == BranchKind::Remote)
            .then(|| self.name.split_once('/'))
            .flatten()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

/// A branch or tag pointing at a commit, shown as a label in the history.
#[derive(Debug, Clone)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
    pub is_head: bool,
}

#[derive(Debug, Clone)]
pub struct TagInfo {
    pub name: String,
    pub target: Oid,
    pub annotated: bool,
    /// Message of an annotated tag.
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub id: Oid,
    pub summary: String,
    pub author: String,
    pub email: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
    /// Time zone of the commit, in minutes east of UTC.
    pub offset_minutes: i32,
    pub parents: Vec<Oid>,
}

impl CommitInfo {
    pub fn short_id(&self) -> String {
        short(self.id)
    }
}

pub fn short(id: Oid) -> String {
    id.to_string()[..7].to_string()
}

/// Full details of one commit.
#[derive(Debug, Clone)]
pub struct CommitDetails {
    pub info: CommitInfo,
    pub message: String,
    pub committer: String,
    pub committer_email: String,
    pub commit_time: i64,
    pub commit_offset_minutes: i32,
}

#[derive(Debug, Clone)]
pub struct StashInfo {
    pub index: usize,
    pub id: Oid,
    pub message: String,
}

impl StashInfo {
    pub fn refname(&self) -> String {
        format!("stash@{{{}}}", self.index)
    }
}

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct SubmoduleInfo {
    pub name: String,
    pub path: String,
    pub url: Option<String>,
    pub head: Option<Oid>,
    pub initialized: bool,
}

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub is_main: bool,
    pub locked: bool,
}

/// An operation git is in the middle of, which needs Continue or Abort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoState {
    Clean,
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
    Other,
}

impl RepoState {
    pub fn label(self) -> &'static str {
        match self {
            RepoState::Clean => "",
            RepoState::Merge => "Merge in progress",
            RepoState::Rebase => "Rebase in progress",
            RepoState::CherryPick => "Cherry-pick in progress",
            RepoState::Revert => "Revert in progress",
            RepoState::Bisect => "Bisect in progress",
            RepoState::Other => "Operation in progress",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChange,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    pub fn letter(self) -> &'static str {
        match self {
            ChangeKind::Added => "A",
            ChangeKind::Modified => "M",
            ChangeKind::Deleted => "D",
            ChangeKind::Renamed => "R",
            ChangeKind::Copied => "C",
            ChangeKind::TypeChange => "T",
            ChangeKind::Untracked => "?",
            ChangeKind::Conflicted => "!",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
}

/// Local changes, split like the staging area of a Git client.
#[derive(Debug, Clone, Default)]
pub struct WorkingStatus {
    pub unstaged: Vec<FileChange>,
    pub staged: Vec<FileChange>,
    pub conflicted: Vec<FileChange>,
}

impl WorkingStatus {
    pub fn is_empty(&self) -> bool {
        self.unstaged.is_empty() && self.staged.is_empty() && self.conflicted.is_empty()
    }

    pub fn total(&self) -> usize {
        // A file can be both staged and unstaged; count it once.
        let mut paths: Vec<&str> = self
            .unstaged
            .iter()
            .chain(&self.staged)
            .chain(&self.conflicted)
            .map(|f| f.path.as_str())
            .collect();
        paths.sort_unstable();
        paths.dedup();
        paths.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Line text without the trailing newline.
    pub text: String,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    /// The line is the last one in its file and has no newline.
    pub no_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

/// Which side of a change a diff compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSource {
    /// A commit against its first parent.
    Commit(Oid),
    /// Working directory against the index (unstaged changes).
    Unstaged,
    /// Index against HEAD (staged changes).
    Staged,
}

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    pub binary: bool,
    pub old_id: Option<Oid>,
    pub new_id: Option<Oid>,
    pub hunks: Vec<Hunk>,
    pub additions: usize,
    pub deletions: usize,
    /// Git file modes (`0o100644`, `0o100755`, `0o120000` link, `0o160000`
    /// submodule); 0 when the side does not exist.
    pub old_mode: u32,
    pub new_mode: u32,
    /// Size in bytes of an untracked file whose diff was not loaded (too large).
    pub skipped: Option<u64>,
}

impl FileDiff {
    pub fn line_count(&self) -> usize {
        self.hunks.iter().map(|h| h.lines.len() + 1).sum()
    }
}

/// A commit in the history of one file.
#[derive(Debug, Clone)]
pub struct FileCommit {
    pub commit: CommitInfo,
    /// Path of the file in this commit (it may have been renamed since).
    pub path: String,
    /// Previous path, when this commit renamed or copied the file.
    pub old_path: Option<String>,
}

/// A commit in the history of a range of lines (`git log -L`).
#[derive(Debug, Clone)]
pub struct LineCommit {
    pub commit: CommitInfo,
    /// How the commit changed the lines (`None` if git showed no diff).
    pub diff: Option<FileDiff>,
}

/// One line of `git blame`.
#[derive(Debug, Clone)]
pub struct BlameLine {
    pub commit: Oid,
    pub author: String,
    pub time: i64,
    pub offset_minutes: i32,
    pub text: String,
    /// First line of a block from the same commit.
    pub block_start: bool,
    /// Line number (1-based) in the commit that last changed the line.
    pub orig_line: u32,
    /// Path of the file in that commit.
    pub path: String,
    /// The commit's parent and the file's path there, when the file existed
    /// before (`previous` in `git blame --porcelain`).
    pub previous: Option<(Oid, String)>,
}

/// Name and email that commits in a repository will use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    pub name: Option<String>,
    pub email: Option<String>,
    pub signing_key: Option<String>,
    pub gpg_sign: bool,
    pub ssh_command: Option<String>,
}
