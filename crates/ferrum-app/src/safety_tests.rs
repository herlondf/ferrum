//! Restore points, secret detection and learning mode.

use std::path::Path;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use ferrum_core::ops::{PushOptions, ResetMode};

use crate::app::FerrumApp;
use crate::command::{Command, Op};
use crate::safety::SafetyCmd;
use crate::ui_tests::{frames, git, harness, repo, settle};

fn handle(h: &mut Harness<'_, FerrumApp>, cmd: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(cmd, &ctx);
    frames(h);
}

fn run(h: &mut Harness<'_, FerrumApp>, op: Op) {
    handle(h, Command::Run(op));
    settle(h);
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap_or_default()
}

fn points(dir: &Path) -> usize {
    let out = git(dir, &["for-each-ref", "refs/ferrum/restore"]);
    out.lines().filter(|l| !l.is_empty()).count()
}

fn commit(message: &str) -> Op {
    Op::Commit {
        message: message.into(),
        amend: false,
        signoff: false,
        no_verify: false,
        log: Default::default(),
    }
}

/// A fake AWS key, joined at run time.
fn secret_line() -> String {
    format!("const KEY: &str = \"{}{}\";\n", "AKIA", "IOSFODNN7QWERTY1")
}

#[test]
fn only_operations_that_drop_work_get_a_restore_point() {
    use crate::timeline::saves_point;
    let reset = Op::Reset {
        rev: "HEAD~1".into(),
        mode: ResetMode::Hard,
    };
    assert!(saves_point(&reset));
    assert!(saves_point(&Op::Discard(Vec::new())));
    assert!(saves_point(&Op::DeleteBranch {
        name: "x".into(),
        force: true
    }));
    assert!(saves_point(&Op::Rebase {
        onto: "main".into()
    }));
    let mut amend = commit("x");
    if let Op::Commit { amend: a, .. } = &mut amend {
        *a = true;
    }
    assert!(saves_point(&amend));
    assert!(!saves_point(&commit("x")));
    assert!(!saves_point(&Op::Fetch { remote: None }));
    assert!(!saves_point(&Op::Checkout("main".into())));
    assert!(!saves_point(&Op::StageAll));
    let push = |force| {
        Op::Push(PushOptions {
            remote: "origin".into(),
            branch: "main".into(),
            force_with_lease: force,
            ..Default::default()
        })
    };
    assert!(saves_point(&push(true)));
    assert!(!saves_point(&push(false)));
}

#[test]
fn discard_is_undone_with_ctrl_z() {
    let dir = repo();
    let p = dir.path();
    std::fs::write(p.join("new.txt"), "scratch\n").unwrap();
    let mut h = harness(p);
    settle(&mut h);
    let files = h.state().tabs[0].status.unstaged.clone();
    assert_eq!(files.len(), 2);
    run(&mut h, Op::Discard(files));
    assert_eq!(read(p, "a.txt"), "one\n");
    assert!(!p.join("new.txt").exists());
    assert_eq!(points(p), 1);
    // The toast offers to undo it.
    assert!(h.query_all_by_label("Undo").count() > 0);

    handle(&mut h, Command::Undo);
    assert!(h.query_all_by_label_contains("Restore to before").count() > 0);
    h.get_by_label("Restore").click();
    settle(&mut h);
    assert_eq!(read(p, "a.txt"), "one\ntwo\n");
    assert_eq!(read(p, "new.txt"), "scratch\n");
    // Restoring saved the state before it too.
    assert_eq!(points(p), 2);
}

#[test]
fn bad_rebase_is_restored_from_the_timeline() {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    git(p, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(p.join("a.txt"), "feature\n").unwrap();
    git(p, &["commit", "-qam", "feature work"]);
    let feature = git(p, &["rev-parse", "HEAD"]);
    git(p, &["checkout", "-q", "main"]);
    std::fs::write(p.join("a.txt"), "main\n").unwrap();
    git(p, &["commit", "-qam", "main work"]);
    git(p, &["checkout", "-q", "feature"]);

    let mut h = harness(p);
    settle(&mut h);
    run(
        &mut h,
        Op::Rebase {
            onto: "main".into(),
        },
    );
    assert_eq!(
        h.state().tabs[0].repo.state(),
        ferrum_core::RepoState::Rebase
    );

    handle(&mut h, Command::Safety(SafetyCmd::ToggleTimeline));
    assert!(h.query_all_by_label("Rebase onto main").count() > 0);
    h.get_all_by_label("Restore…").next().unwrap().click();
    frames(&mut h);
    h.get_by_label("Restore").click();
    settle(&mut h);
    let tab = &h.state().tabs[0];
    assert_eq!(tab.repo.state(), ferrum_core::RepoState::Clean);
    assert_eq!(git(p, &["branch", "--show-current"]), "feature");
    assert_eq!(git(p, &["rev-parse", "HEAD"]), feature);
    assert_eq!(read(p, "a.txt"), "feature\n");
}

#[test]
fn hard_reset_and_branch_delete_are_listed_and_undone() {
    let dir = repo();
    let p = dir.path();
    git(p, &["commit", "-qam", "second"]);
    let second = git(p, &["rev-parse", "HEAD"]);
    std::fs::write(p.join("a.txt"), "uncommitted\n").unwrap();
    git(p, &["branch", "topic"]);

    let mut h = harness(p);
    settle(&mut h);
    let first = git(p, &["rev-parse", "HEAD~1"]);
    run(
        &mut h,
        Op::Reset {
            rev: first.clone(),
            mode: ResetMode::Hard,
        },
    );
    run(
        &mut h,
        Op::DeleteBranch {
            name: "topic".into(),
            force: true,
        },
    );
    assert_eq!(read(p, "a.txt"), "one\n");
    handle(&mut h, Command::Safety(SafetyCmd::ToggleTimeline));
    assert!(h.query_all_by_label_contains("Delete branch topic").count() > 0);
    assert!(h.query_all_by_label_contains("Reset to").count() > 0);

    // Ctrl+Z: the branch delete first (its quick undo), then the reset.
    handle(&mut h, Command::Undo);
    settle(&mut h);
    assert_eq!(git(p, &["rev-parse", "topic"]), second);

    // The reset, from its restore point: commit and uncommitted change back.
    let n = h.get_all_by_label("Restore…").count();
    h.get_all_by_label("Restore…").nth(n - 1).unwrap().click();
    frames(&mut h);
    h.get_by_label("Restore").click();
    settle(&mut h);
    assert_eq!(git(p, &["rev-parse", "HEAD"]), second);
    assert_eq!(read(p, "a.txt"), "uncommitted\n");
}

#[test]
fn restore_points_can_be_turned_off() {
    let dir = repo();
    let p = dir.path();
    let mut h = harness(p);
    h.state_mut().settings.safety.restore_points = false;
    settle(&mut h);
    let files = h.state().tabs[0].status.unstaged.clone();
    run(&mut h, Op::Discard(files));
    assert_eq!(points(p), 0);
}

#[test]
fn staged_secret_stops_the_commit_and_can_be_unstaged() {
    let dir = repo();
    let p = dir.path();
    std::fs::write(p.join("config.rs"), secret_line()).unwrap();
    git(p, &["add", "config.rs"]);
    let mut h = harness(p);
    settle(&mut h);
    run(&mut h, commit("Add config"));
    assert!(h.query_all_by_label("Possible secret found").count() > 0);
    assert!(h.query_all_by_label("AWS access key").count() > 0);
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "first");

    h.get_by_label("Unstage").click();
    settle(&mut h);
    assert_eq!(git(p, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "first");
}

#[test]
fn ignore_once_and_allowlist_let_the_commit_through() {
    let dir = repo();
    let p = dir.path();
    std::fs::write(p.join("config.rs"), secret_line()).unwrap();
    git(p, &["add", "config.rs"]);
    let mut h = harness(p);
    settle(&mut h);
    run(&mut h, commit("Add config"));
    h.get_by_label("Ignore once").click();
    settle(&mut h);
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "Add config");

    // The same value elsewhere: allowed for good this time.
    std::fs::write(p.join("other.rs"), secret_line()).unwrap();
    git(p, &["add", "other.rs"]);
    run(&mut h, commit("Copy config"));
    h.get_by_label("Add to allowlist").click();
    settle(&mut h);
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "Copy config");
    assert_eq!(h.state().settings.safety.allowlists.len(), 1);

    std::fs::write(p.join("third.rs"), secret_line()).unwrap();
    git(p, &["add", "third.rs"]);
    run(&mut h, commit("Third"));
    assert_eq!(h.query_all_by_label("Possible secret found").count(), 0);
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "Third");
}

#[test]
fn commit_and_push_are_both_held_and_a_push_is_scanned() {
    let dir = repo();
    let p = dir.path();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(
        p,
        &["remote", "add", "origin", &remote.path().to_string_lossy()],
    );
    git(p, &["push", "-q", "origin", "main"]);
    std::fs::write(p.join("config.rs"), secret_line()).unwrap();
    git(p, &["add", "config.rs"]);
    let mut h = harness(p);
    settle(&mut h);
    let push = Op::Push(PushOptions {
        remote: "origin".into(),
        branch: "main".into(),
        ..Default::default()
    });
    // Commit and push: both wait for the answer.
    handle(&mut h, Command::Run(commit("Config")));
    handle(&mut h, Command::Run(push.clone()));
    settle(&mut h);
    assert!(h.query_all_by_label("Possible secret found").count() > 0);
    h.get_by_label("Cancel").click();
    settle(&mut h);
    let local = git(p, &["rev-parse", "main"]);
    let pushed = git(remote.path(), &["rev-parse", "main"]);
    assert_eq!(local, pushed);

    // Committed outside: the push itself is checked.
    git(p, &["commit", "-qm", "Config"]);
    run(&mut h, push);
    assert!(h.query_all_by_label("Push anyway").count() > 0);
    h.get_by_label("Push anyway").click();
    settle(&mut h);
    assert_eq!(
        git(remote.path(), &["rev-parse", "main"]),
        git(p, &["rev-parse", "main"])
    );
}

#[test]
fn learning_mode_shows_the_git_commands() {
    let dir = repo();
    let mut h = harness(dir.path());
    h.state_mut().settings.safety.learning = true;
    settle(&mut h);
    assert!(h.query_all_by_label("Learning mode").count() > 0);
    run(&mut h, Op::StageAll);
    assert!(h.query_all_by_label("$ git add -A").count() > 0);
    assert!(h.query_all_by_label_contains("Stages changes").count() > 0);
    // Tooltips name the command too.
    let cmd = crate::ui::learning::command_for(&Op::Rebase {
        onto: "main".into(),
    });
    assert_eq!(cmd.as_deref(), Some("git rebase main"));
    let ex = crate::ui::learning::explain("git reset -q --hard HEAD~1").unwrap();
    assert!(ex.contains("throws away"));
}
