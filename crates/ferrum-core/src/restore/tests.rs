use super::*;
use crate::testutil::TestRepo;

fn head(r: &TestRepo) -> String {
    r.git.run(&["rev-parse", "HEAD"]).unwrap().trim().to_owned()
}

fn status(r: &TestRepo) -> String {
    r.git.run(&["status", "--porcelain"]).unwrap()
}

/// Takes a point, runs `op`, records the result; returns the point id.
fn guarded(r: &TestRepo, label: &str, opts: &SnapshotOptions, op: impl FnOnce(&Git)) -> String {
    let snap = create(&r.git, label, opts).unwrap();
    op(&r.git);
    finish(&r.git, &snap, None).unwrap();
    snap.id().to_owned()
}

fn restore_all(r: &TestRepo, id: &str) {
    let opts = SnapshotOptions::default();
    restore(&r.git, id, RestoreMode::Everything, "Restore", &opts).unwrap();
}

#[test]
fn hard_reset_comes_back_with_local_changes() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    r.commit("a.txt", "two\n", "second");
    let before = head(&r);
    r.write("a.txt", "two\nlocal edit\n");
    r.write("b.txt", "staged\n");
    r.git.run(&["add", "b.txt"]).unwrap();
    r.write("new.txt", "untracked\n");
    let status_before = status(&r);

    let id = guarded(&r, "Reset to HEAD~1", &Default::default(), |g| {
        g.run(&["reset", "-q", "--hard", "HEAD~1"]).unwrap();
        g.run(&["clean", "-fq"]).unwrap();
    });
    assert_ne!(head(&r), before);
    assert_eq!(r.read("a.txt"), "one\n");

    restore_all(&r, &id);
    assert_eq!(head(&r), before);
    assert_eq!(r.read("a.txt"), "two\nlocal edit\n");
    assert_eq!(r.read("new.txt"), "untracked\n");
    // Staged and unstaged changes are where they were.
    assert_eq!(status(&r), status_before);
    assert_eq!(r.repo().head_branch().as_deref(), Some("main"));
}

#[test]
fn bad_rebase_in_progress_is_undone() {
    let r = TestRepo::new();
    r.commit("f.txt", "base\n", "base");
    r.git.run(&["checkout", "-q", "-b", "feature"]).unwrap();
    r.commit("f.txt", "feature\n", "feature change");
    let feature = head(&r);
    r.git.run(&["checkout", "-q", "main"]).unwrap();
    r.commit("f.txt", "main\n", "main change");
    r.git.run(&["checkout", "-q", "feature"]).unwrap();

    let snap = create(&r.git, "Rebase onto main", &Default::default()).unwrap();
    let err = r.git.run(&["rebase", "main"]).err();
    assert!(err.is_some(), "the rebase conflicts");
    finish(&r.git, &snap, err.map(|e| e.to_string())).unwrap();
    assert_eq!(r.repo().state(), RepoState::Rebase);

    restore_all(&r, snap.id());
    assert_eq!(r.repo().state(), RepoState::Clean);
    assert_eq!(r.repo().head_branch().as_deref(), Some("feature"));
    assert_eq!(head(&r), feature);
    assert_eq!(r.read("f.txt"), "feature\n");
    assert_eq!(status(&r), "");
    // The point saw the failure.
    let p = load(&r.repo(), snap.id()).unwrap();
    assert!(p.meta.error.is_some());
}

#[test]
fn discarded_changes_and_deleted_untracked_files_come_back() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    r.write("a.txt", "edited\n");
    r.write("junk.log", "keep me\n");
    let opts = SnapshotOptions {
        untracked: false,
        paths: vec!["junk.log".into()],
        ..Default::default()
    };
    let id = guarded(&r, "Discard", &opts, |g| {
        g.run(&["checkout", "--", "a.txt"]).unwrap();
        g.run(&["clean", "-fq", "--", "junk.log"]).unwrap();
    });
    assert_eq!(r.read("a.txt"), "one\n");
    assert!(!r.path().join("junk.log").exists());

    restore_all(&r, &id);
    assert_eq!(r.read("a.txt"), "edited\n");
    assert_eq!(r.read("junk.log"), "keep me\n");
    // Still unstaged / untracked, not added.
    assert_eq!(status(&r), " M a.txt\n?? junk.log\n");
}

#[test]
fn deleted_branch_comes_back_with_refs_only() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    r.git.run(&["checkout", "-q", "-b", "topic"]).unwrap();
    r.commit("t.txt", "topic\n", "topic work");
    let topic = head(&r);
    r.git.run(&["checkout", "-q", "main"]).unwrap();
    r.write("a.txt", "dirty\n");

    let id = guarded(&r, "Delete branch topic", &Default::default(), |g| {
        g.run(&["branch", "-D", "topic"]).unwrap();
    });
    let p = load(&r.repo(), &id).unwrap();
    assert_eq!(p.meta.changed.len(), 1);
    assert_eq!(p.meta.changed[0].name, "refs/heads/topic");
    assert_eq!(p.meta.changed[0].new, None);

    let opts = SnapshotOptions::default();
    restore(&r.git, &id, RestoreMode::RefsOnly, "Restore", &opts).unwrap();
    let back = r.git.run(&["rev-parse", "topic"]).unwrap();
    assert_eq!(back.trim(), topic);
    // Refs only: the local edit stays.
    assert_eq!(r.read("a.txt"), "dirty\n");
}

#[test]
fn amend_is_undone_and_the_change_is_staged_again() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    let original = head(&r);
    r.write("a.txt", "one\ntwo\n");
    r.git.run(&["add", "a.txt"]).unwrap();
    let id = guarded(&r, "Amend commit", &Default::default(), |g| {
        g.run(&["commit", "-q", "--amend", "-m", "amended"])
            .unwrap();
    });
    assert_ne!(head(&r), original);

    restore_all(&r, &id);
    assert_eq!(head(&r), original);
    assert_eq!(status(&r), "M  a.txt\n");
}

#[test]
fn dropped_stash_comes_back() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    r.write("a.txt", "stashed\n");
    r.git
        .run(&["stash", "push", "-q", "-m", "my work"])
        .unwrap();
    let id = guarded(&r, "Drop stash@{0}", &Default::default(), |g| {
        g.run(&["stash", "drop", "-q"]).unwrap();
    });
    assert!(r.repo().stashes().unwrap().is_empty());
    let p = load(&r.repo(), &id).unwrap();
    assert_eq!(p.meta.dropped_stashes.len(), 1);

    let opts = SnapshotOptions::default();
    restore(&r.git, &id, RestoreMode::RefsOnly, "Restore", &opts).unwrap();
    let stashes = r.repo().stashes().unwrap();
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].message.contains("my work"));
    r.git.run(&["stash", "pop", "-q"]).unwrap();
    assert_eq!(r.read("a.txt"), "stashed\n");
}

#[test]
fn a_restore_can_itself_be_restored() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    r.commit("a.txt", "two\n", "second");
    let second = head(&r);
    let id = guarded(&r, "Reset", &Default::default(), |g| {
        g.run(&["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    });
    let first = head(&r);
    restore_all(&r, &id);
    assert_eq!(head(&r), second);

    // The restore saved the state before it: going back to it is a redo.
    let points = list(&r.repo()).unwrap();
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].meta.label, "Restore");
    restore_all(&r, &points[0].id);
    assert_eq!(head(&r), first);
}

#[test]
fn branches_created_by_the_operation_are_removed() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    let id = guarded(&r, "Start feature", &Default::default(), |g| {
        g.run(&["checkout", "-q", "-b", "feature/x"]).unwrap();
        r.commit("x.txt", "x\n", "x");
    });
    restore_all(&r, &id);
    assert_eq!(r.repo().head_branch().as_deref(), Some("main"));
    assert!(
        r.git
            .run(&["rev-parse", "--verify", "-q", "feature/x"])
            .is_err()
    );
    assert!(!r.path().join("x.txt").exists());
}

#[test]
fn untracked_file_in_the_way_is_overwritten_after_being_saved() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    r.commit("b.txt", "tracked\n", "add b");
    let id = guarded(&r, "Reset", &Default::default(), |g| {
        g.run(&["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    });
    // b.txt is now an untracked file with other content.
    r.write("b.txt", "mine\n");
    restore_all(&r, &id);
    assert_eq!(r.read("b.txt"), "tracked\n");
    // ...and the restore point taken first kept "mine".
    let pre = &list(&r.repo()).unwrap()[0];
    let tree = pre.meta.worktree.clone().unwrap();
    let saved = r.git.run(&["show", &format!("{tree}:b.txt")]).unwrap();
    assert_eq!(saved, "mine\n");
}

#[test]
fn merge_conflict_state_and_unborn_branch() {
    let r = TestRepo::new();
    // Nothing committed yet: a point still works.
    r.write("a.txt", "draft\n");
    let snap = create(&r.git, "Discard", &Default::default()).unwrap();
    finish(&r.git, &snap, None).unwrap();
    std::fs::remove_file(r.path().join("a.txt")).unwrap();
    restore_all(&r, snap.id());
    assert_eq!(r.read("a.txt"), "draft\n");

    r.commit("a.txt", "base\n", "base");
    r.git.run(&["checkout", "-q", "-b", "other"]).unwrap();
    r.commit("a.txt", "other\n", "other");
    r.git.run(&["checkout", "-q", "main"]).unwrap();
    r.commit("a.txt", "main\n", "main");
    let main = head(&r);
    assert!(r.git.run(&["merge", "other"]).is_err());
    // Conflicts: the index cannot be written as a tree; files are kept.
    let snap = create(&r.git, "Abort merge", &Default::default()).unwrap();
    assert!(snap.meta.index.is_none());
    assert_eq!(snap.meta.state, "merge");
    r.git.run(&["merge", "--abort"]).unwrap();
    finish(&r.git, &snap, None).unwrap();
    restore_all(&r, snap.id());
    assert_eq!(head(&r), main);
    assert!(r.read("a.txt").contains("<<<<<<<"));
}

#[test]
fn prune_and_delete() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    let a = guarded(&r, "A", &Default::default(), |_| {});
    let _b = guarded(&r, "B", &Default::default(), |_| {});
    assert_eq!(list(&r.repo()).unwrap().len(), 2);
    delete(&r.git, &a).unwrap();
    assert_eq!(list(&r.repo()).unwrap().len(), 1);
    assert_eq!(prune(&r.git, 30).unwrap(), 0);
    assert_eq!(prune_before(&r.git, i64::MAX).unwrap(), 1);
    assert!(list(&r.repo()).unwrap().is_empty());
}

#[test]
fn points_stay_out_of_history_and_backups_are_listed() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    guarded(&r, "A", &Default::default(), |_| {});
    let log = r.git.run(&["log", "--all", "--format=%s"]).unwrap();
    // `--all` sees them, but the graph only walks branches, tags and remotes.
    assert!(log.contains("Ferrum restore point"));
    let repo = r.repo();
    let commits = repo.commits(100).unwrap();
    assert_eq!(commits.len(), 1);

    let id = head(&r);
    r.git
        .run(&["update-ref", "refs/ferrum/backup/main/1", &id])
        .unwrap();
    let b = backups(&r.repo());
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].name, "main/1");
    assert_eq!(b[0].summary, "first");
}

#[test]
fn annotated_tag_deleted_comes_back() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "first");
    r.git.run(&["tag", "-a", "v1", "-m", "release"]).unwrap();
    let tag = r.git.run(&["rev-parse", "v1"]).unwrap();
    let id = guarded(&r, "Delete tag v1", &Default::default(), |g| {
        g.run(&["tag", "-d", "v1"]).unwrap();
    });
    let opts = SnapshotOptions::default();
    restore(&r.git, &id, RestoreMode::RefsOnly, "Restore", &opts).unwrap();
    assert_eq!(r.git.run(&["rev-parse", "v1"]).unwrap(), tag);
    let kind = r.git.run(&["cat-file", "-t", "v1"]).unwrap();
    assert_eq!(kind.trim(), "tag");
}
