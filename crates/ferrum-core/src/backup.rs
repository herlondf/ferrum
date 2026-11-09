//! Backup refs taken before Ferrum rewrites history (restack, absorb,
//! moving hunks between commits).
//!
//! Layout: `refs/ferrum/backup/<op>-<unix seconds>/<branch>`, one ref per
//! branch the operation may move, pointing at the branch's tip before it ran.
//! `<op>` is a short word (`restack`, `absorb`, `move`). Anything that lists
//! restore points can read them with `git for-each-ref refs/ferrum/backup/`
//! or [`list`]; [`restore`] puts the branches back.

use crate::cli::Git;
use crate::ops::ResetMode;
use crate::{Error, Oid, Repo, Result};

pub const PREFIX: &str = "refs/ferrum/backup/";

/// How many backups are kept; older ones are removed when a new one is made.
pub const KEEP: usize = 50;

/// One backup: the tips of some branches at a moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    /// `<op>-<secs>`, the folder under [`PREFIX`].
    pub id: String,
    pub op: String,
    /// Unix seconds.
    pub time: i64,
    /// (branch, tip) pairs.
    pub refs: Vec<(String, Oid)>,
}

/// A fresh backup id for `op`, e.g. `restack-1731000000`.
pub fn new_id(op: &str) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    format!("{op}-{secs}")
}

/// Saves the current tips of `branches` under `id`. An id already used gets
/// a suffix; the id actually used is returned.
pub fn create(git: &Git, id: &str, branches: &[String]) -> Result<String> {
    let mut id = id.to_owned();
    let taken = |id: &str| {
        git.run(&["for-each-ref", "--count=1", &format!("{PREFIX}{id}/")])
            .is_ok_and(|o| !o.trim().is_empty())
    };
    let base = id.clone();
    let mut n = 1;
    while taken(&id) {
        n += 1;
        id = format!("{base}.{n}");
    }
    for b in branches {
        let tip = git.run(&[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{b}^{{commit}}"),
        ])?;
        git.run(&["update-ref", &format!("{PREFIX}{id}/{b}"), tip.trim()])?;
    }
    prune(git, KEEP);
    Ok(id)
}

/// All backups, newest first.
pub fn list(repo: &Repo) -> Result<Vec<Backup>> {
    let mut out: Vec<Backup> = Vec::new();
    let refs = repo.raw().references_glob(&format!("{PREFIX}*"))?;
    for r in refs.flatten() {
        let name = crate::lossy(r.name_bytes());
        let Some(rest) = name.strip_prefix(PREFIX) else {
            continue;
        };
        let Some((id, branch)) = rest.split_once('/') else {
            continue;
        };
        let Some(tip) = r.target() else { continue };
        let (op, time) = parse_id(id);
        match out.iter_mut().find(|b| b.id == id) {
            Some(b) => b.refs.push((branch.to_owned(), tip)),
            None => out.push(Backup {
                id: id.to_owned(),
                op,
                time,
                refs: vec![(branch.to_owned(), tip)],
            }),
        }
    }
    out.sort_by(|a, b| b.time.cmp(&a.time).then(b.id.cmp(&a.id)));
    Ok(out)
}

fn parse_id(id: &str) -> (String, i64) {
    let main = id.split('.').next().unwrap_or(id);
    match main.rsplit_once('-') {
        Some((op, secs)) => (op.to_owned(), secs.parse().unwrap_or(0)),
        None => (main.to_owned(), 0),
    }
}

/// Puts the branches of backup `id` back. The checked-out branch is reset
/// with `mode` (`Keep` keeps local changes and refuses to lose them; `Mixed`
/// keeps the working copy as it is); the others are moved.
pub fn restore(git: &Git, id: &str, mode: ResetMode) -> Result<()> {
    let repo = Repo::open(git.dir())?;
    let backup = list(&repo)?
        .into_iter()
        .find(|b| b.id == id)
        .ok_or_else(|| Error::Other(format!("The backup {id} no longer exists.")))?;
    let head = repo.head_branch();
    for (branch, tip) in &backup.refs {
        let tip = tip.to_string();
        if head.as_deref() == Some(branch.as_str()) {
            crate::ops::reset(git, &tip, mode)?;
        } else {
            git.run(&["update-ref", &format!("refs/heads/{branch}"), &tip])?;
        }
    }
    Ok(())
}

/// Removes backup `id`.
pub fn delete(git: &Git, id: &str) -> Result<()> {
    let out = git.run(&[
        "for-each-ref",
        "--format=%(refname)",
        &format!("{PREFIX}{id}/"),
    ])?;
    for r in out.lines().filter(|l| !l.is_empty()) {
        git.run(&["update-ref", "-d", r])?;
    }
    Ok(())
}

/// Keeps the newest `keep` backups.
pub fn prune(git: &Git, keep: usize) {
    let Ok(repo) = Repo::open(git.dir()) else {
        return;
    };
    let Ok(all) = list(&repo) else { return };
    for b in all.iter().skip(keep) {
        let _ = delete(git, &b.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn creates_lists_and_restores() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "A");
        t.git.run(&["branch", "side"]).unwrap();
        let before = t.repo().head_oid().unwrap();
        let id = create(&t.git, "restack-100", &["main".into(), "side".into()]).unwrap();
        assert_eq!(id, "restack-100");
        // The same id again gets a suffix.
        let again = create(&t.git, "restack-100", &["main".into()]).unwrap();
        assert_eq!(again, "restack-100.2");

        t.commit("b.txt", "b\n", "B");
        t.git.run(&["branch", "-f", "side", "HEAD"]).unwrap();
        let list = list(&t.repo()).unwrap();
        let b = list.iter().find(|b| b.id == "restack-100").unwrap();
        assert_eq!((b.op.as_str(), b.time), ("restack", 100));
        assert_eq!(b.refs.len(), 2);

        restore(&t.git, "restack-100", ResetMode::Keep).unwrap();
        assert_eq!(t.repo().head_oid(), Some(before));
        assert_eq!(t.repo().resolve("side").unwrap(), before);
        delete(&t.git, "restack-100").unwrap();
        assert!(
            super::list(&t.repo())
                .unwrap()
                .iter()
                .all(|b| b.id != "restack-100")
        );
    }

    #[test]
    fn prunes_old_backups() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "A");
        for i in 0..4 {
            create(&t.git, &format!("absorb-{i}"), &["main".into()]).unwrap();
        }
        prune(&t.git, 2);
        let ids: Vec<String> = list(&t.repo()).unwrap().into_iter().map(|b| b.id).collect();
        assert_eq!(ids, ["absorb-3", "absorb-2"]);
    }
}
