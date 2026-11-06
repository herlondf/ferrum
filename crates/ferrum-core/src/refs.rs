//! Branches, tags and stashes beyond the basics in `ops`: renaming on the
//! remote, signed tags, tag details, partial stashes, snapshots, renaming.

use std::path::Path;

use crate::cli::Git;
use crate::{Error, Oid, Repo, Result, StashInfo, lossy};

/// `git check-ref-format --branch`: the name is a valid branch name.
pub fn check_branch_name(git: &Git, name: &str) -> bool {
    !name.trim().is_empty()
        && !name.starts_with('-')
        && git.run(&["check-ref-format", "--branch", name]).is_ok()
}

/// Renames a local branch. With `remote` (remote name, branch name there),
/// also pushes the new name, deletes the old one and tracks the new one.
pub fn rename_branch(
    git: &Git,
    old: &str,
    new: &str,
    remote: Option<(&str, &str)>,
    mut progress: impl FnMut(&str),
) -> Result<()> {
    git.run(&["branch", "-m", old, new])?;
    let Some((remote, old_remote)) = remote else {
        return Ok(());
    };
    let spec = format!("refs/heads/{new}:refs/heads/{new}");
    git.run_streaming(&["push", "--progress", "-u", remote, &spec], &mut progress)?;
    git.run_streaming(
        &["push", "--progress", remote, "--delete", old_remote],
        &mut progress,
    )?;
    Ok(())
}

/// Creates a tag; annotated when there is a message, signed with `sign`
/// (signed tags are always annotated).
pub fn create_tag(git: &Git, name: &str, rev: &str, message: &str, sign: bool) -> Result<()> {
    let msg = message.trim();
    if !sign && msg.is_empty() {
        git.run(&["tag", name, rev])?;
        return Ok(());
    }
    let flag = if sign { "-s" } else { "-a" };
    let msg = if msg.is_empty() { name } else { msg };
    git.run_with_input(&["tag", flag, name, rev, "-F", "-"], Some(msg.as_bytes()))?;
    Ok(())
}

/// What the tag details window shows.
#[derive(Debug, Clone)]
pub struct TagDetails {
    pub name: String,
    /// The commit the tag points to.
    pub target: Oid,
    pub target_summary: String,
    pub annotated: bool,
    pub tagger: Option<String>,
    pub email: Option<String>,
    /// Seconds since the Unix epoch.
    pub time: Option<i64>,
    /// Time zone of `time`, in minutes.
    pub offset: i32,
    pub message: Option<String>,
    /// The message carries a GPG or SSH signature.
    pub signed: bool,
}

pub fn tag_details(repo: &Repo, name: &str) -> Result<TagDetails> {
    let raw = repo.raw();
    let r = raw.find_reference(&format!("refs/tags/{name}"))?;
    let commit = r.peel_to_commit()?;
    let tag = r.peel_to_tag().ok();
    let tagger = tag.as_ref().and_then(|t| t.tagger());
    let message = tag.as_ref().and_then(|t| t.message_bytes().map(lossy));
    let signed = message.as_deref().is_some_and(|m| {
        m.contains("-----BEGIN PGP SIGNATURE-----") || m.contains("-----BEGIN SSH SIGNATURE-----")
    });
    // The signature is noise in the window.
    let message = message.map(|m| {
        let end = [
            "-----BEGIN PGP SIGNATURE-----",
            "-----BEGIN SSH SIGNATURE-----",
        ]
        .iter()
        .filter_map(|s| m.find(s))
        .min()
        .unwrap_or(m.len());
        m[..end].trim().to_owned()
    });
    Ok(TagDetails {
        name: name.to_owned(),
        target: commit.id(),
        target_summary: lossy(commit.summary_bytes().unwrap_or_default()),
        annotated: tag.is_some(),
        tagger: tagger.as_ref().map(|s| lossy(s.name_bytes())),
        email: tagger.as_ref().map(|s| lossy(s.email_bytes())),
        time: tagger.as_ref().map(|s| s.when().seconds()),
        offset: tagger.as_ref().map_or(0, |s| s.when().offset_minutes()),
        message,
        signed,
    })
}

/// The current `stash@{n}` of a stash, found by its commit (indexes shift
/// when other stashes are added or dropped).
fn stash_ref(git: &Git, stash: &StashInfo) -> Result<String> {
    Ok(format!("stash@{{{}}}", stash_index(git, stash)?))
}

fn stash_index(git: &Git, stash: &StashInfo) -> Result<usize> {
    let out = git.run(&["stash", "list", "--format=%H"])?;
    let id = stash.id.to_string();
    out.lines()
        .position(|l| l.trim() == id)
        .ok_or_else(|| Error::Other(format!("The stash \"{}\" no longer exists.", stash.message)))
}

/// Stashes only `paths` (`git stash push -- paths`).
pub fn stash_paths(git: &Git, message: &str, untracked: bool, paths: &[String]) -> Result<()> {
    let mut args = vec!["stash", "push"];
    if untracked {
        args.push("--include-untracked");
    }
    if !message.trim().is_empty() {
        args.extend(["-m", message]);
    }
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    git.run(&args)?;
    Ok(())
}

/// Saves the local changes as a stash but leaves them in the working copy.
/// Returns false when there was nothing to save.
pub fn stash_snapshot(git: &Git, message: &str) -> Result<bool> {
    let id = git.run(&["stash", "create", message])?;
    let id = id.trim();
    if id.is_empty() {
        return Ok(false);
    }
    let msg = if message.trim().is_empty() {
        "Snapshot".to_owned()
    } else {
        message.trim().to_owned()
    };
    git.run(&["stash", "store", "-m", &msg, id])?;
    Ok(true)
}

/// Gives a stash a new message. The stash commit is kept; it moves to the top.
pub fn stash_rename(git: &Git, stash: &StashInfo, message: &str) -> Result<()> {
    let n = stash_index(git, stash)?;
    let id = stash.id.to_string();
    git.run(&["stash", "store", "-m", message.trim(), &id])?;
    // The old entry is now one further down.
    git.run(&["stash", "drop", "-q", &format!("stash@{{{}}}", n + 1)])?;
    Ok(())
}

/// Applies (or pops) a stash; with `index` the staged state is restored too.
pub fn stash_apply(git: &Git, stash: &StashInfo, pop: bool, index: bool) -> Result<()> {
    let r = stash_ref(git, stash)?;
    let mut args = vec!["stash", if pop { "pop" } else { "apply" }];
    if index {
        args.push("--index");
    }
    args.push(&r);
    git.run(&args)?;
    Ok(())
}

/// The changes of a stash as a patch, including new (untracked) files.
pub fn stash_patch(git: &Git, stash: &StashInfo) -> Result<String> {
    let id = stash.id.to_string();
    let mut patch = git.run(&["stash", "show", "-p", "--binary", &id])?;
    // Untracked files are in the third parent, when the stash has one.
    let untracked = format!("{id}^3");
    if git
        .run(&["rev-parse", "-q", "--verify", &untracked])
        .is_ok()
    {
        let empty = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
        let extra = git.run(&["diff", "--binary", empty, &untracked])?;
        if !patch.is_empty() && !patch.ends_with('\n') {
            patch.push('\n');
        }
        patch.push_str(&extra);
    }
    Ok(patch)
}

pub fn save_stash_patch(git: &Git, stash: &StashInfo, path: &Path) -> Result<()> {
    std::fs::write(path, stash_patch(git, stash)?)?;
    Ok(())
}

/// Drops several stashes.
pub fn stash_drop_many(git: &Git, stashes: &[StashInfo]) -> Result<()> {
    for s in stashes {
        let r = stash_ref(git, s)?;
        git.run(&["stash", "drop", "-q", &r])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    fn stashes(t: &TestRepo) -> Vec<StashInfo> {
        t.repo().stashes().unwrap()
    }

    #[test]
    fn validates_branch_names() {
        let t = TestRepo::new();
        assert!(check_branch_name(&t.git, "feature/x"));
        assert!(!check_branch_name(&t.git, "bad name"));
        assert!(!check_branch_name(&t.git, "a..b"));
        assert!(!check_branch_name(&t.git, "-x"));
        assert!(!check_branch_name(&t.git, "x.lock"));
    }

    #[test]
    fn renames_branch_on_remote() {
        let remote = TestRepo::new();
        remote.commit("a.txt", "a\n", "first");
        remote.git.run(&["branch", "old"]).unwrap();
        let t = TestRepo::new();
        let url = remote.path().to_string_lossy().into_owned();
        t.git.run(&["remote", "add", "origin", &url]).unwrap();
        t.git.run(&["fetch", "-q", "origin"]).unwrap();
        t.git
            .run(&["switch", "-q", "-c", "main", "origin/main"])
            .unwrap();
        t.git
            .run(&["branch", "old", "--track", "origin/old"])
            .unwrap();
        rename_branch(&t.git, "old", "new", Some(("origin", "old")), |_| {}).unwrap();
        let heads = remote
            .git
            .run(&["branch", "--format=%(refname:short)"])
            .unwrap();
        assert!(heads.lines().any(|l| l == "new"), "{heads}");
        assert!(!heads.lines().any(|l| l == "old"), "{heads}");
        let up = t
            .git
            .run(&["rev-parse", "--abbrev-ref", "new@{upstream}"])
            .unwrap();
        assert_eq!(up.trim(), "origin/new");
    }

    #[test]
    fn tags_with_message_and_details() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        create_tag(&t.git, "v1", "HEAD", "Release\n\nSecond line", false).unwrap();
        create_tag(&t.git, "light", "HEAD", "  ", false).unwrap();
        let d = tag_details(&t.repo(), "v1").unwrap();
        assert!(d.annotated && !d.signed);
        assert_eq!(d.message.as_deref(), Some("Release\n\nSecond line"));
        assert_eq!(d.tagger.as_deref(), Some("Test"));
        assert_eq!(d.target_summary, "first");
        let l = tag_details(&t.repo(), "light").unwrap();
        assert!(!l.annotated && l.tagger.is_none());
        assert_eq!(l.target, d.target);
    }

    #[test]
    fn partial_stash_snapshot_and_rename() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        t.commit("b.txt", "b\n", "second");
        t.write("a.txt", "a2\n");
        t.write("b.txt", "b2\n");
        stash_paths(&t.git, "only a", false, &["a.txt".into()]).unwrap();
        assert_eq!(t.read("a.txt"), "a\n");
        assert_eq!(t.read("b.txt"), "b2\n");

        // A snapshot keeps the working copy as it is.
        assert!(stash_snapshot(&t.git, "snap").unwrap());
        assert_eq!(t.read("b.txt"), "b2\n");
        let list = stashes(&t);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].message, "snap");

        // Renaming keeps the commit.
        let old = list[1].clone();
        stash_rename(&t.git, &old, "renamed").unwrap();
        let list = stashes(&t);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].message, "renamed");
        assert_eq!(list[0].id, old.id);
        assert_eq!(list[1].message, "snap");

        let patch = stash_patch(&t.git, &list[0]).unwrap();
        assert!(patch.contains("+a2"), "{patch}");

        stash_drop_many(&t.git, &list).unwrap();
        assert!(stashes(&t).is_empty());
        // Nothing to snapshot in a clean tree.
        t.git.run(&["checkout", "-q", "--", "."]).unwrap();
        assert!(!stash_snapshot(&t.git, "").unwrap());
    }

    #[test]
    fn apply_restores_staged_state() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        t.write("a.txt", "a2\n");
        t.git.run(&["add", "a.txt"]).unwrap();
        t.write("new.txt", "n\n");
        t.git.run(&["stash", "push", "-u", "-q"]).unwrap();
        let s = stashes(&t)[0].clone();
        let patch = stash_patch(&t.git, &s).unwrap();
        assert!(patch.contains("new.txt"), "{patch}");
        stash_apply(&t.git, &s, true, true).unwrap();
        let staged = t.git.run(&["diff", "--cached", "--name-only"]).unwrap();
        assert_eq!(staged.trim(), "a.txt");
        assert!(stashes(&t).is_empty());
    }
}
