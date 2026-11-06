//! UI tests of merge, rebase and cherry-pick dialogs, quick rebase actions,
//! the planner's keyboard, the conflict banner and the local-changes prompt.

use std::path::Path;

use eframe::egui::Key;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::FerrumApp;
use crate::command::{Command, Op};
use crate::ui_tests::{frames, git, harness, repo, settle};

fn handle(h: &mut Harness<'_, FerrumApp>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(c, &ctx);
    frames(h);
}

/// Waits for the background conflict check of a dialog.
fn wait_for(h: &mut Harness<'_, FerrumApp>, text: &str) {
    for _ in 0..300 {
        h.step();
        if h.query_by_label_contains(text).is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("{text:?} never appeared");
}

fn log(p: &Path) -> String {
    git(p, &["log", "--format=%s"])
}

/// main: first - main a; feature: first - add f - feature a (both change a.txt).
fn diverged() -> tempfile::TempDir {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    git(p, &["switch", "-q", "-c", "feature"]);
    std::fs::write(p.join("f.txt"), "f\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "add f"]);
    std::fs::write(p.join("a.txt"), "feature\n").unwrap();
    git(p, &["commit", "-qam", "feature a"]);
    git(p, &["switch", "-q", "main"]);
    std::fs::write(p.join("a.txt"), "main\n").unwrap();
    git(p, &["commit", "-qam", "main a"]);
    dir
}

#[test]
fn merge_dialog_previews_and_stops_before_committing() {
    let dir = diverged();
    let p = dir.path();
    git(p, &["branch", "clean", "feature~1"]);
    let mut h = harness(p);
    frames(&mut h);
    handle(
        &mut h,
        Command::Dialog(crate::ui::dialogs::Dialog::merge("feature", "feature")),
    );
    wait_for(&mut h, "Will conflict in: a.txt");
    h.get_by_label("Cancel").click();
    frames(&mut h);

    handle(
        &mut h,
        Command::Dialog(crate::ui::dialogs::Dialog::merge("clean", "clean")),
    );
    wait_for(&mut h, "Applies cleanly");
    h.get_by_label("Don't commit (--no-commit)").click();
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Merge").click();
    settle(&mut h);
    assert!(
        p.join(".git/MERGE_HEAD").exists(),
        "stopped before the commit"
    );
    assert_eq!(log(p).lines().next(), Some("main a"));
    // The banner names what is merged.
    assert!(h.query_by_label("Merging").is_some());
}

#[test]
fn rebase_dialog_onto_a_tag_with_autostash() {
    let dir = diverged();
    let p = dir.path();
    git(p, &["switch", "-q", "feature"]);
    git(p, &["reset", "-q", "--hard", "HEAD~1"]);
    git(p, &["tag", "v1", "main"]);
    std::fs::write(p.join("f.txt"), "local\n").unwrap();
    let mut h = harness(p);
    frames(&mut h);
    let tab = &h.state().tabs[0];
    let d = crate::ui::flow_dialogs::rebase(tab, "v1");
    handle(&mut h, Command::Dialog(d));
    wait_for(&mut h, "Applies cleanly");
    h.get_by_role_and_label(Role::Button, "Rebase").click();
    settle(&mut h);
    assert_eq!(log(p), "add f\nmain a\nfirst");
    assert_eq!(std::fs::read_to_string(p.join("f.txt")).unwrap(), "local\n");
}

#[test]
fn cherry_pick_several_with_origin() {
    let dir = diverged();
    let p = dir.path();
    git(p, &["switch", "-q", "-c", "two", "main~1"]);
    for n in ["x", "y"] {
        std::fs::write(p.join(format!("{n}.txt")), n).unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-qm", n]);
    }
    git(p, &["switch", "-q", "main"]);
    let mut h = harness(p);
    frames(&mut h);
    for c in crate::flow::dev_steps("select=two,two~1", h.state().tabs.first()) {
        handle(&mut h, c);
    }
    for c in crate::flow::dev_steps("pick=two,two~1", h.state().tabs.first()) {
        handle(&mut h, c);
    }
    wait_for(&mut h, "Applies cleanly");
    h.get_by_label("Append the origin to the message (-x)")
        .click();
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Cherry-pick").click();
    settle(&mut h);
    assert_eq!(log(p), "y\nx\nmain a\nfirst", "applied oldest first");
    assert!(git(p, &["log", "-1", "--format=%B"]).contains("cherry picked from commit"));
}

#[test]
fn quick_fixup_and_reword_from_the_commit_menu() {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    for n in ["A", "B", "C"] {
        std::fs::write(p.join(format!("{n}.txt")), n).unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-qm", n]);
    }
    let mut h = harness(p);
    frames(&mut h);
    let b = h.state().tabs[0].repo.resolve("HEAD~1").unwrap();
    let op = crate::ui::flow_dialogs::quick_op(
        &h.state().tabs[0],
        &[b],
        ferrum_core::rebase::Quick::Fixup,
    )
    .unwrap();
    handle(&mut h, Command::Run(op));
    settle(&mut h);
    assert_eq!(log(p), "C\nA\nfirst");

    for c in crate::flow::dev_steps("reword=HEAD", h.state().tabs.first()) {
        handle(&mut h, c);
    }
    h.get_by_role(Role::MultilineTextInput).focus();
    frames(&mut h);
    h.get_by_role(Role::MultilineTextInput)
        .type_text(" changed");
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Reword").click();
    settle(&mut h);
    assert!(log(p).starts_with("C"), "{}", log(p));
    assert_ne!(log(p).lines().next(), Some("C"), "message changed");
}

#[test]
fn planner_keyboard_and_confirm_on_escape() {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    let base = git(p, &["rev-parse", "HEAD"]);
    for n in ["A", "B", "C"] {
        std::fs::write(p.join(format!("{n}.txt")), n).unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-qm", n]);
    }
    let mut h = harness(p);
    frames(&mut h);
    let base = ferrum_core::Oid::from_str(&base).unwrap();
    handle(&mut h, Command::PlanRebase(Some(base)));
    // Select B and drop it with "d".
    h.get_by_label("B").click();
    frames(&mut h);
    h.key_press(Key::D);
    frames(&mut h);
    // Esc asks before throwing the plan away.
    h.key_press(Key::Escape);
    frames(&mut h);
    assert!(h.state().tabs[0].rebase.is_some());
    h.get_by_label("Keep editing").click();
    frames(&mut h);
    h.get_by_label("Back up the current state in a branch first")
        .click();
    frames(&mut h);
    h.get_by_label("Start rebase").click();
    settle(&mut h);
    assert_eq!(log(p), "C\nA\nfirst");
    assert!(git(p, &["branch", "--list", "backup/*"]).contains("backup/main-"));
}

#[test]
fn banner_undoes_a_resolution() {
    let dir = diverged();
    let p = dir.path();
    assert!(
        std::process::Command::new("git")
            .current_dir(p)
            .args(["merge", "feature"])
            .output()
            .is_ok()
    );
    let mut h = harness(p);
    frames(&mut h);
    assert!(h.query_by_label("Merging").is_some());
    h.get_by_label("Resolve conflicts").click();
    frames(&mut h);
    h.get_by_label("Use feature for whole file").click();
    settle(&mut h);
    assert_eq!(
        std::fs::read_to_string(p.join("a.txt")).unwrap(),
        "feature\n"
    );
    h.get_by_label("Resolved (1) ⏷").click();
    frames(&mut h);
    h.get_by_label("Undo a.txt").click();
    settle(&mut h);
    assert!(
        std::fs::read_to_string(p.join("a.txt"))
            .unwrap()
            .contains("<<<<<<<")
    );
    assert_eq!(h.state().tabs[0].status.conflicted.len(), 1);
}

#[test]
fn checkout_asks_about_local_changes() {
    let dir = diverged();
    let p = dir.path();
    // a.txt differs on feature: a plain checkout would be refused.
    std::fs::write(p.join("a.txt"), "local\n").unwrap();
    let mut h = harness(p);
    frames(&mut h);
    handle(&mut h, Command::Run(Op::Checkout("feature".into())));
    assert!(
        h.query_by_label("These local changes would be overwritten:")
            .is_some()
    );
    h.get_by_label("Discard them").click();
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Continue").click();
    settle(&mut h);
    assert_eq!(git(p, &["branch", "--show-current"]), "feature");
    assert_eq!(
        std::fs::read_to_string(p.join("a.txt")).unwrap(),
        "feature\n"
    );

    // With the setting on, it asks even when nothing would be overwritten.
    h.state_mut().settings.prefs.ask_local_changes = true;
    std::fs::write(p.join("f.txt"), "changed\n").unwrap();
    git(p, &["branch", "side", "feature~1"]);
    h.state_mut().tabs[0].refresh_all();
    settle(&mut h);
    handle(&mut h, Command::Run(Op::Checkout("side".into())));
    assert!(
        h.query_by_label("Stash and reapply them afterwards")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Continue").click();
    settle(&mut h);
    assert_eq!(git(p, &["branch", "--show-current"]), "side");
    assert_eq!(
        std::fs::read_to_string(p.join("f.txt")).unwrap(),
        "changed\n"
    );
}

#[test]
fn ctrl_and_shift_click_select_several_commits() {
    use eframe::egui::Modifiers;
    let dir = diverged();
    let mut tab = crate::tab::RepoTab::open_with(dir.path().into(), &Default::default()).unwrap();
    let ids: Vec<_> = tab.commits.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), 4);
    tab.history.selected = Some(ids[0]);
    let click = crate::ui::history_flow::click;
    assert!(click(&mut tab, ids[2], Modifiers::COMMAND));
    assert_eq!(tab.history.ext.selected_many, [ids[0], ids[2]]);
    assert!(click(&mut tab, ids[2], Modifiers::COMMAND), "toggles off");
    assert_eq!(tab.history.ext.selected_many, [ids[0]]);
    assert!(click(&mut tab, ids[3], Modifiers::SHIFT));
    assert_eq!(tab.history.ext.selected_many, ids);
    assert!(!click(&mut tab, ids[1], Modifiers::NONE));
    assert!(tab.history.ext.selected_many.is_empty());
}
