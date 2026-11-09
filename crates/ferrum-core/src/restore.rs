//! Restore points: before an operation that rewrites or drops work, the refs,
//! HEAD, the stash list, the index and the working tree are saved under
//! `refs/ferrum/restore/<id>`, without touching the working tree. Restoring
//! one brings them back (after saving the current state as a new point).
//!
//! A point is a commit whose tree holds `meta.json` (what was saved), the
//! `index` tree and the `worktree` tree (tracked files and, when asked,
//! untracked ones). Its parents keep the saved commits reachable, so `git gc`
//! does not drop them. These refs are never pushed: Ferrum only pushes named
//! branches and tags.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use git2::{ObjectType, Oid, StatusOptions};
use serde::{Deserialize, Serialize};

use crate::cli::Git;
use crate::{Error, RepoState, Result, lossy};

pub const PREFIX: &str = "refs/ferrum/restore/";
/// Extra refs keeping annotated tag objects of a point alive.
const KEEP_PREFIX: &str = "refs/ferrum/restore-keep/";
/// Backup refs written before history rewrites (stacks, absorb...).
pub const BACKUP_PREFIX: &str = "refs/ferrum/backup/";
/// At most this many dirty paths are listed in a point.
const MAX_FILES: usize = 200;
/// More parents than this are not added (command line length on Windows).
const MAX_PARENTS: usize = 250;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RefEntry {
    pub name: String,
    pub oid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StashEntry {
    pub oid: String,
    pub message: String,
}

/// A ref the operation changed: `None` = it did not exist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RefChange {
    pub name: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// What a restore point saved.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Meta {
    pub version: u32,
    /// The operation it was taken before, e.g. "Rebase onto main".
    pub label: String,
    /// Unix time, seconds.
    pub time: i64,
    /// Branch HEAD was on (`None`: detached).
    pub head: Option<String>,
    pub head_oid: Option<String>,
    /// Operation in progress then ("rebase", "merge"…; empty when none).
    pub state: String,
    /// Branches, tags and remote branches.
    pub refs: Vec<RefEntry>,
    /// Newest first.
    pub stashes: Vec<StashEntry>,
    /// Tree of the index (`None` when it had conflicts).
    pub index: Option<String>,
    /// Tree of the working tree.
    pub worktree: Option<String>,
    /// Changed and untracked paths saved (at most `MAX_FILES`).
    pub files: Vec<String>,
    pub files_total: usize,
    /// Untracked files were not saved (too big, or not asked).
    pub untracked_skipped: bool,
    /// Filled once the operation finished.
    pub finished: bool,
    pub changed: Vec<RefChange>,
    pub dropped_stashes: Vec<StashEntry>,
    /// The operation's error, when it failed.
    pub error: Option<String>,
}

impl Meta {
    pub fn head_short(&self) -> String {
        match (&self.head, &self.head_oid) {
            (Some(b), _) => b.clone(),
            (None, Some(id)) => id.chars().take(7).collect(),
            _ => String::new(),
        }
    }
}

/// How to take a snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotOptions {
    /// Save untracked (not ignored) files too.
    pub untracked: bool,
    /// Untracked files are skipped when they add up to more than this.
    pub max_untracked_bytes: u64,
    /// Untracked paths saved anyway (files an operation is about to delete).
    pub paths: Vec<String>,
}

impl Default for SnapshotOptions {
    fn default() -> Self {
        Self {
            untracked: true,
            max_untracked_bytes: 100 * 1024 * 1024,
            paths: Vec::new(),
        }
    }
}

/// A point being taken, returned by [`create`] and completed by [`finish`].
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub refname: String,
    pub commit: String,
    pub meta: Meta,
}

impl Snapshot {
    pub fn id(&self) -> &str {
        self.refname.trim_start_matches(PREFIX)
    }
}

/// A saved restore point.
#[derive(Debug, Clone)]
pub struct RestorePoint {
    pub id: String,
    pub refname: String,
    pub commit: Oid,
    pub meta: Meta,
}

/// A backup ref (`refs/ferrum/backup/...`).
#[derive(Debug, Clone)]
pub struct Backup {
    pub refname: String,
    /// The part after the prefix, e.g. `feature/1700000000`.
    pub name: String,
    pub target: Oid,
    pub summary: String,
    /// When the backup was written (reflog), or the commit's time.
    pub time: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreMode {
    /// Branches, tags, HEAD and stashes; files stay as they are.
    RefsOnly,
    /// Also the index and the working tree.
    Everything,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Plumbing commands with a fixed identity and no signing.
fn plumbing(git: &Git) -> Git {
    git.clone()
        .with_env("GIT_AUTHOR_NAME", "Ferrum")
        .with_env("GIT_AUTHOR_EMAIL", "ferrum@localhost")
        .with_env("GIT_COMMITTER_NAME", "Ferrum")
        .with_env("GIT_COMMITTER_EMAIL", "ferrum@localhost")
}

fn state_word(s: RepoState) -> &'static str {
    match s {
        RepoState::Clean => "",
        RepoState::Merge => "merge",
        RepoState::Rebase => "rebase",
        RepoState::CherryPick => "cherry-pick",
        RepoState::Revert => "revert",
        RepoState::Bisect => "bisect",
        RepoState::Am => "am",
        RepoState::Other => "other",
    }
}

/// Branches, tags and remote branches with their targets.
fn read_refs(repo: &git2::Repository) -> Result<BTreeMap<String, Oid>> {
    let mut out = BTreeMap::new();
    for r in repo.references()? {
        let r = r?;
        let name = lossy(r.name_bytes());
        let keep = name.starts_with("refs/heads/")
            || name.starts_with("refs/tags/")
            || (name.starts_with("refs/remotes/") && !name.ends_with("/HEAD"));
        if let (true, Some(t)) = (keep, r.target()) {
            out.insert(name, t);
        }
    }
    Ok(out)
}

fn read_stashes(git: &Git) -> Vec<StashEntry> {
    let Ok(repo) = crate::Repo::open(git.dir()) else {
        return Vec::new();
    };
    repo.stashes()
        .unwrap_or_default()
        .into_iter()
        .map(|s| StashEntry {
            oid: s.id.to_string(),
            message: s.message,
        })
        .collect()
}

fn peel_commit(repo: &git2::Repository, id: Oid) -> Option<Oid> {
    repo.find_object(id, None)
        .ok()?
        .peel_to_commit()
        .ok()
        .map(|c| c.id())
}

/// Changed and untracked files, and the size of the untracked ones.
fn dirty_files(repo: &git2::Repository) -> (Vec<String>, usize, Vec<String>, u64) {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .exclude_submodules(true);
    let Ok(list) = repo.statuses(Some(&mut opts)) else {
        return (Vec::new(), 0, Vec::new(), 0);
    };
    let workdir = repo.workdir().map(Path::to_path_buf);
    let (mut files, mut untracked, mut bytes) = (Vec::new(), Vec::new(), 0u64);
    let mut total = 0;
    for e in list.iter() {
        let path = lossy(e.path_bytes());
        total += 1;
        if files.len() < MAX_FILES {
            files.push(path.clone());
        }
        if e.status().contains(git2::Status::WT_NEW) {
            if let Some(w) = &workdir {
                bytes += std::fs::metadata(w.join(&path))
                    .map(|m| m.len())
                    .unwrap_or(0);
            }
            untracked.push(path);
        }
    }
    (files, total, untracked, bytes)
}

/// Tree of the working tree, built in a copy of the index so the real one
/// is not touched.
fn worktree_tree(
    git: &Git,
    repo: &git2::Repository,
    all_untracked: bool,
    paths: &[String],
    tag: &str,
) -> Result<String> {
    let index = repo.path().join("index");
    let tmp = repo.path().join(format!("ferrum-snapshot-{tag}.index"));
    if index.exists() {
        std::fs::copy(&index, &tmp)?;
    }
    let tmp_text = tmp.to_string_lossy().into_owned();
    let g = git.clone().with_env("GIT_INDEX_FILE", tmp_text);
    let result = (|| {
        g.run(&["add", if all_untracked { "-A" } else { "-u" }])?;
        let wd = repo.workdir().map(Path::to_path_buf).unwrap_or_default();
        let extra: Vec<&str> = paths
            .iter()
            .filter(|p| wd.join(p).exists())
            .map(String::as_str)
            .collect();
        if !all_untracked && !extra.is_empty() {
            let mut args = vec!["add", "-f", "--"];
            args.extend(extra);
            g.run(&args)?;
        }
        Ok(g.run(&["write-tree"])?.trim().to_owned())
    })();
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Writes the point's commit: tree with `meta.json`, `index` and `worktree`.
fn write_commit(git: &Git, meta: &Meta, parents: &[String], message: &str) -> Result<String> {
    let g = plumbing(git);
    let json = serde_json::to_vec_pretty(meta).map_err(|e| Error::Other(e.to_string()))?;
    let blob = g.run_with_input(&["hash-object", "-w", "--stdin"], Some(&json))?;
    let mut tree = format!("100644 blob {}\tmeta.json\n", blob.trim());
    if let Some(t) = &meta.index {
        tree.push_str(&format!("040000 tree {t}\tindex\n"));
    }
    if let Some(t) = &meta.worktree {
        tree.push_str(&format!("040000 tree {t}\tworktree\n"));
    }
    let tree = g.run_with_input(&["mktree"], Some(tree.as_bytes()))?;
    let tree = tree.trim().to_owned();
    let mut args: Vec<String> = vec!["commit-tree".into(), "--no-gpg-sign".into(), tree];
    let mut seen = HashSet::new();
    for p in parents
        .iter()
        .filter(|p| seen.insert(p.as_str()))
        .take(MAX_PARENTS)
    {
        args.push("-p".into());
        args.push(p.clone());
    }
    args.push("-m".into());
    args.push(message.into());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(g.run(&args)?.trim().to_owned())
}

/// Saves a restore point before an operation called `label`.
pub fn create(git: &Git, label: &str, opts: &SnapshotOptions) -> Result<Snapshot> {
    let repo = git2::Repository::discover(git.dir())?;
    let wrapped = crate::Repo::open(git.dir())?;
    let head = repo.head().ok();
    let head_oid = head.as_ref().and_then(|h| h.target());
    let head_branch = repo.find_reference("HEAD").ok().and_then(|h| {
        h.symbolic_target_bytes()
            .map(|t| lossy(t).trim_start_matches("refs/heads/").to_owned())
    });
    let refs = read_refs(&repo)?;
    let stashes = read_stashes(git);
    let mut meta = Meta {
        version: 1,
        label: label.to_owned(),
        time: now_secs(),
        head: head_branch,
        head_oid: head_oid.map(|o| o.to_string()),
        state: state_word(wrapped.state()).to_owned(),
        refs: refs
            .iter()
            .map(|(n, o)| RefEntry {
                name: n.clone(),
                oid: o.to_string(),
            })
            .collect(),
        stashes: stashes.clone(),
        ..Default::default()
    };
    let tag = format!("{}", now_millis());
    if !repo.is_bare() {
        let (files, total, untracked, bytes) = dirty_files(&repo);
        let all = opts.untracked && bytes <= opts.max_untracked_bytes;
        meta.files = files;
        meta.files_total = total;
        meta.untracked_skipped = !untracked.is_empty() && !all;
        // Fails with conflicts (unmerged entries): then only the files are kept.
        meta.index = git.run(&["write-tree"]).ok().map(|s| s.trim().to_owned());
        meta.worktree = Some(worktree_tree(git, &repo, all, &opts.paths, &tag)?);
    }
    let mut parents: Vec<String> = head_oid.map(|o| o.to_string()).into_iter().collect();
    for (name, oid) in &refs {
        if name.starts_with("refs/heads/") {
            parents.extend(peel_commit(&repo, *oid).map(|o| o.to_string()));
        }
    }
    parents.extend(stashes.iter().map(|s| s.oid.clone()));
    let commit = write_commit(
        git,
        &meta,
        &parents,
        &format!("Ferrum restore point: {label}"),
    )?;
    // A new ref; another one taken in the same millisecond gets a suffix.
    let zero = Oid::ZERO_SHA1.to_string();
    for n in 0..50 {
        let id = if n == 0 {
            tag.clone()
        } else {
            format!("{tag}-{n}")
        };
        let refname = format!("{PREFIX}{id}");
        let msg = format!("ferrum: restore point before {label}");
        if git
            .run(&["update-ref", "-m", &msg, &refname, &commit, &zero])
            .is_ok()
        {
            return Ok(Snapshot {
                refname,
                commit,
                meta,
            });
        }
    }
    Err(Error::Other("could not create a restore point ref".into()))
}

/// Records what the operation changed. `error` is its error, if it failed.
pub fn finish(git: &Git, snap: &Snapshot, error: Option<String>) -> Result<()> {
    let repo = git2::Repository::discover(git.dir())?;
    let after = read_refs(&repo)?;
    let mut meta = snap.meta.clone();
    let before: BTreeMap<&str, &str> = meta
        .refs
        .iter()
        .map(|r| (r.name.as_str(), r.oid.as_str()))
        .collect();
    let mut changed = Vec::new();
    let names: BTreeSet<&str> = before
        .keys()
        .copied()
        .chain(after.keys().map(String::as_str))
        .collect();
    for name in names {
        let old = before.get(name).map(|s| s.to_string());
        let new = after.get(name).map(|o| o.to_string());
        if old != new {
            changed.push(RefChange {
                name: name.to_owned(),
                old,
                new,
            });
        }
    }
    let now: HashSet<String> = read_stashes(git).into_iter().map(|s| s.oid).collect();
    meta.dropped_stashes = meta
        .stashes
        .iter()
        .filter(|s| !now.contains(&s.oid))
        .cloned()
        .collect();
    meta.changed = changed;
    meta.finished = true;
    meta.error = error;

    let mut parents = vec![snap.commit.clone()];
    let mut keep = Vec::new();
    for c in &meta.changed {
        let Some(old) = c.old.as_deref().and_then(|o| Oid::from_str(o).ok()) else {
            continue;
        };
        let kind = repo.find_object(old, None).ok().and_then(|o| o.kind());
        match kind {
            Some(ObjectType::Commit) => parents.push(old.to_string()),
            Some(_) => {
                parents.extend(peel_commit(&repo, old).map(|o| o.to_string()));
                keep.push(old);
            }
            None => {}
        }
    }
    let commit = write_commit(
        git,
        &meta,
        &parents,
        &format!("Ferrum restore point: {}", meta.label),
    )?;
    for (i, id) in keep.iter().enumerate() {
        let r = format!("{KEEP_PREFIX}{}/{i}", snap.id());
        git.run(&["update-ref", &r, &id.to_string()])?;
    }
    git.run(&["update-ref", &snap.refname, &commit, &snap.commit])?;
    Ok(())
}

fn load_meta(repo: &git2::Repository, commit: Oid) -> Option<Meta> {
    let tree = repo.find_commit(commit).ok()?.tree().ok()?;
    let entry = tree.get_name("meta.json")?;
    let blob = repo.find_blob(entry.id()).ok()?;
    serde_json::from_slice(blob.content()).ok()
}

/// Restore points, newest first.
pub fn list(repo: &crate::Repo) -> Result<Vec<RestorePoint>> {
    let raw = repo.raw();
    let mut out = Vec::new();
    for r in raw.references_glob(&format!("{PREFIX}*"))? {
        let r = r?;
        let refname = lossy(r.name_bytes());
        let Some(commit) = r.target() else { continue };
        let Some(meta) = load_meta(raw, commit) else {
            continue;
        };
        out.push(RestorePoint {
            id: refname.trim_start_matches(PREFIX).to_owned(),
            refname,
            commit,
            meta,
        });
    }
    out.sort_by(|a, b| {
        b.meta
            .time
            .cmp(&a.meta.time)
            .then_with(|| id_order(&b.id).cmp(&id_order(&a.id)))
    });
    Ok(out)
}

/// Millisecond time and suffix of an id, for sorting.
fn id_order(id: &str) -> (u128, u32) {
    let (ms, n) = id.split_once('-').unwrap_or((id, "0"));
    (ms.parse().unwrap_or(0), n.parse().unwrap_or(0))
}

pub fn load(repo: &crate::Repo, id: &str) -> Result<RestorePoint> {
    let refname = format!("{PREFIX}{id}");
    let raw = repo.raw();
    let commit = raw
        .find_reference(&refname)?
        .target()
        .ok_or_else(|| Error::Other(format!("{refname} is not a commit")))?;
    let meta = load_meta(raw, commit)
        .ok_or_else(|| Error::Other(format!("{refname}: no restore information")))?;
    Ok(RestorePoint {
        id: id.to_owned(),
        refname,
        commit,
        meta,
    })
}

/// Backup refs, newest first.
pub fn backups(repo: &crate::Repo) -> Vec<Backup> {
    let raw = repo.raw();
    let mut out = Vec::new();
    let Ok(refs) = raw.references_glob(&format!("{BACKUP_PREFIX}*")) else {
        return out;
    };
    for r in refs.flatten() {
        let refname = lossy(r.name_bytes());
        let Ok(commit) = r.peel_to_commit() else {
            continue;
        };
        let logged = raw
            .reflog(&refname)
            .ok()
            .and_then(|l| l.get(0).map(|e| e.committer().when().seconds()));
        out.push(Backup {
            name: refname.trim_start_matches(BACKUP_PREFIX).to_owned(),
            refname,
            target: commit.id(),
            summary: lossy(commit.summary_bytes().unwrap_or_default()),
            time: logged.unwrap_or_else(|| commit.time().seconds()),
        });
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.time));
    out
}

fn delete_refs(git: &Git, repo: &crate::Repo, ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut input = String::new();
    for id in ids {
        input.push_str(&format!("delete {PREFIX}{id}\n"));
        if let Ok(keep) = repo.raw().references_glob(&format!("{KEEP_PREFIX}{id}/*")) {
            for r in keep.flatten() {
                input.push_str(&format!("delete {}\n", lossy(r.name_bytes())));
            }
        }
    }
    git.run_with_input(&["update-ref", "--stdin"], Some(input.as_bytes()))?;
    Ok(())
}

pub fn delete(git: &Git, id: &str) -> Result<()> {
    let repo = crate::Repo::open(git.dir())?;
    delete_refs(git, &repo, &[id.to_owned()])
}

/// Deletes points older than `days`. Returns how many went.
pub fn prune(git: &Git, days: u32) -> Result<usize> {
    prune_before(git, now_secs() - i64::from(days) * 86_400)
}

/// Deletes points taken before `limit` (Unix seconds).
pub fn prune_before(git: &Git, limit: i64) -> Result<usize> {
    let repo = crate::Repo::open(git.dir())?;
    let old: Vec<String> = list(&repo)?
        .into_iter()
        .filter(|p| p.meta.time < limit)
        .map(|p| p.id)
        .collect();
    delete_refs(git, &repo, &old)?;
    Ok(old.len())
}

/// Ends a merge, rebase, cherry-pick, revert or `am` in progress, leaving
/// HEAD, the index and the files as they are.
fn quit_operation(git: &Git, state: RepoState) -> Result<()> {
    let cmd = match state {
        RepoState::Merge => "merge",
        RepoState::Rebase => "rebase",
        RepoState::CherryPick => "cherry-pick",
        RepoState::Revert => "revert",
        RepoState::Am => "am",
        _ => return Ok(()),
    };
    git.run(&[cmd, "--quit"])?;
    Ok(())
}

/// Brings back the state saved in point `id`. The current state is saved as
/// a new point called `label` first, so a restore can be undone too; its id
/// is returned.
pub fn restore(
    git: &Git,
    id: &str,
    mode: RestoreMode,
    label: &str,
    opts: &SnapshotOptions,
) -> Result<String> {
    let repo = crate::Repo::open(git.dir())?;
    let point = load(&repo, id)?;
    let pre = create(git, label, opts)?;
    let result = apply(git, &repo, &point.meta, mode);
    let _ = finish(git, &pre, result.as_ref().err().map(|e| e.to_string()));
    result.map(|()| pre.id().to_owned())
}

/// What restoring `meta` would change now: branches and tags, from their
/// current target (`old`) to the saved one (`new`, `None` = deleted).
pub fn preview(repo: &crate::Repo, meta: &Meta) -> Vec<RefChange> {
    let Ok(current) = read_refs(repo.raw()) else {
        return Vec::new();
    };
    let local = |n: &str| n.starts_with("refs/heads/") || n.starts_with("refs/tags/");
    let mut out = Vec::new();
    for r in meta.refs.iter().filter(|r| local(&r.name)) {
        let now = current.get(&r.name).map(|o| o.to_string());
        if now.as_deref() != Some(r.oid.as_str()) {
            out.push(RefChange {
                name: r.name.clone(),
                old: now,
                new: Some(r.oid.clone()),
            });
        }
    }
    for c in meta
        .changed
        .iter()
        .filter(|c| c.old.is_none() && local(&c.name))
    {
        if let Some(now) = current.get(&c.name) {
            out.push(RefChange {
                name: c.name.clone(),
                old: Some(now.to_string()),
                new: None,
            });
        }
    }
    out
}

fn apply(git: &Git, repo: &crate::Repo, meta: &Meta, mode: RestoreMode) -> Result<()> {
    quit_operation(git, repo.state())?;
    let raw = repo.raw();
    let current = read_refs(raw)?;

    // Branches and tags as they were; refs the operation created go away.
    let mut input = String::new();
    let local = |n: &str| n.starts_with("refs/heads/") || n.starts_with("refs/tags/");
    for r in meta.refs.iter().filter(|r| local(&r.name)) {
        if current.get(&r.name).map(|o| o.to_string()).as_deref() != Some(r.oid.as_str()) {
            input.push_str(&format!("update {} {}\n", r.name, r.oid));
        }
    }
    for c in meta
        .changed
        .iter()
        .filter(|c| c.old.is_none() && local(&c.name))
    {
        if current.contains_key(&c.name) {
            input.push_str(&format!("delete {}\n", c.name));
        }
    }
    if !input.is_empty() {
        git.run_with_input(
            &["update-ref", "-m", "ferrum: restore", "--stdin"],
            Some(input.as_bytes()),
        )?;
    }

    match (&meta.head, &meta.head_oid) {
        (Some(b), _) => {
            git.run(&[
                "symbolic-ref",
                "-m",
                "ferrum: restore",
                "HEAD",
                &format!("refs/heads/{b}"),
            ])?;
        }
        (None, Some(id)) => {
            git.run(&[
                "update-ref",
                "--no-deref",
                "-m",
                "ferrum: restore",
                "HEAD",
                id,
            ])?;
        }
        _ => {}
    }

    // Dropped stashes come back, oldest first so the order is kept.
    let now: HashSet<String> = read_stashes(git).into_iter().map(|s| s.oid).collect();
    for s in meta.stashes.iter().rev().filter(|s| !now.contains(&s.oid)) {
        git.run(&["stash", "store", "-m", &s.message, &s.oid])?;
    }

    if mode == RestoreMode::Everything
        && let Some(tree) = &meta.worktree
    {
        git.run(&["read-tree", "--reset", "-u", tree])?;
        match (&meta.index, &meta.head_oid) {
            (Some(index), _) => git.run(&["read-tree", index])?,
            (None, Some(_)) => git.run(&["read-tree", "HEAD"])?,
            (None, None) => git.run(&["read-tree", "--empty"])?,
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests;
