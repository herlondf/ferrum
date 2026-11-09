//! UI tests of fetch, pull, push, fast-forward, tracking and remotes.

use std::path::Path;
use std::time::Instant;

use eframe::egui::accesskit::Role;
use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::app::FerrumApp;
use crate::command::Command;
use crate::ui::dialogs::Dialog;
use crate::ui::sync_dialogs;
use crate::ui_tests::{frames, git, harness, repo, settle};

/// A clean repository whose `main` is pushed to a bare `origin`.
fn with_remote() -> (tempfile::TempDir, tempfile::TempDir) {
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    let url = remote.path().to_string_lossy().into_owned();
    git(p, &["remote", "add", "origin", &url]);
    git(p, &["push", "-q", "-u", "origin", "main"]);
    (dir, remote)
}

/// Someone else commits `file` on `branch` and pushes it.
fn push_from_elsewhere(remote: &Path, branch: &str, file: &str) {
    let other = tempfile::tempdir().unwrap();
    let o = other.path();
    git(o, &["clone", "-q", &remote.to_string_lossy(), "."]);
    git(o, &["config", "user.name", "Other"]);
    git(o, &["config", "user.email", "other@example.com"]);
    git(o, &["config", "commit.gpgsign", "false"]);
    git(
        o,
        &["checkout", "-q", "-B", branch, &format!("origin/{branch}")],
    );
    std::fs::write(o.join(file), "x\n").unwrap();
    git(o, &["add", "-A"]);
    git(o, &["commit", "-qm", file]);
    git(o, &["push", "-q", "origin", branch]);
}

fn open(h: &mut Harness<'_, FerrumApp>, cmd: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(cmd, &ctx);
    frames(h);
}

fn combo<'a>(h: &'a Harness<'_, FerrumApp>, value: &str) -> egui_kittest::Node<'a> {
    h.get_all_by_role(Role::ComboBox)
        .rfind(|n| n.accesskit_node().value().as_deref() == Some(value))
        .unwrap_or_else(|| panic!("no combo box showing {value}"))
}

/// Opens a filter combo box, types `text` and presses Enter.
fn pick(h: &mut Harness<'_, FerrumApp>, value: &str, text: &str) {
    combo(h, value).click();
    frames(h);
    let field = h
        .get_all_by_role(Role::TextInput)
        .find(|n| n.accesskit_node().placeholder() == Some("Filter…"))
        .expect("filter field");
    field.type_text(text);
    frames(h);
    h.key_press(Key::Enter);
    frames(h);
}

#[test]
fn fetch_dialog_options_are_remembered() {
    let (dir, remote) = with_remote();
    let p = dir.path();
    git(remote.path(), &["branch", "gone", "main"]);
    let mut h = harness(p);
    frames(&mut h);
    // Alt+click opens the dialog instead of fetching.
    h.get_by_label(&crate::icons::text(crate::icons::FETCH, "Fetch"))
        .click_modifiers(Modifiers::ALT);
    frames(&mut h);
    h.get_by_label("Prune remote branches that were deleted")
        .click();
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Fetch").click();
    settle(&mut h);
    assert!(!h.state().settings.sync.fetch_prune);
    git(p, &["rev-parse", "--verify", "origin/gone"]);

    // A plain click fetches with the remembered options: nothing is pruned.
    git(remote.path(), &["branch", "-D", "gone"]);
    h.get_by_label(&crate::icons::text(crate::icons::FETCH, "Fetch"))
        .click();
    settle(&mut h);
    git(p, &["rev-parse", "--verify", "origin/gone"]);
    let last = crate::jobs::operations()
        .into_iter()
        .rev()
        .find(|o| o.repo.as_deref().is_some_and(|r| same_path(r, p)));
    let cmd = last.unwrap().commands()[0].command.clone();
    assert!(!cmd.contains("--prune"), "{cmd}");
}

#[test]
fn pull_dialog_rebases_and_reapplies_local_changes() {
    let (dir, remote) = with_remote();
    let p = dir.path();
    push_from_elsewhere(remote.path(), "main", "b.txt");
    git(p, &["fetch", "-q"]);
    std::fs::write(p.join("a.txt"), "local edit\n").unwrap();
    let mut h = harness(p);
    frames(&mut h);
    let d = Dialog::pull(&h.state().tabs[0]);
    open(&mut h, Command::Dialog(d));
    h.get_by_role_and_label(Role::RadioButton, "Rebase").click();
    h.get_by_label("Stash and reapply local changes").click();
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Pull").click();
    settle(&mut h);
    assert!(p.join("b.txt").exists());
    assert_eq!(
        std::fs::read_to_string(p.join("a.txt")).unwrap(),
        "local edit\n"
    );
    assert!(h.state().settings.sync.pull_autostash);
}

#[test]
fn push_dialog_renames_the_remote_branch_and_pushes_tags() {
    let (dir, remote) = with_remote();
    let p = dir.path();
    git(p, &["tag", "v1"]);
    let mut h = harness(p);
    frames(&mut h);
    h.get_all_by_label("⏷").nth(2).unwrap().click();
    frames(&mut h);
    pick(&mut h, "main", "review/main");
    combo(&h, "review/main");
    h.get_by_role_and_label(Role::RadioButton, "All tags")
        .click();
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Push").click();
    settle(&mut h);
    let r = remote.path();
    git(r, &["rev-parse", "--verify", "review/main"]);
    git(r, &["rev-parse", "--verify", "v1"]);
}

#[test]
fn fast_forwards_the_selected_branches() {
    let (dir, remote) = with_remote();
    let p = dir.path();
    git(p, &["push", "-q", "origin", "main:feature"]);
    git(p, &["branch", "--track", "feature", "origin/feature"]);
    push_from_elsewhere(remote.path(), "main", "m.txt");
    push_from_elsewhere(remote.path(), "feature", "f.txt");
    let mut h = harness(p);
    frames(&mut h);
    h.get_by_label(&crate::icons::text(crate::icons::FETCH, "Fetch"))
        .click();
    settle(&mut h);
    let main = h.state().tabs[0].current_branch().unwrap().ahead_behind;
    assert_eq!(main, Some((0, 1)), "the Pull button shows 1");
    for name in ["main", "feature"] {
        let row = h
            .get_all_by_role(Role::Button)
            .find(|n| {
                let l = n.accesskit_node().label().unwrap_or_default();
                l.trim_start_matches("✔ ").split_whitespace().next() == Some(name)
            })
            .unwrap();
        row.click_modifiers(Modifiers::COMMAND);
        frames(&mut h);
    }
    h.get_by_label("Fast-forward (2)").click();
    settle(&mut h);
    let rev = |r: &str| git(p, &["rev-parse", r]);
    assert_eq!(rev("main"), rev("origin/main"));
    assert_eq!(rev("feature"), rev("origin/feature"));
    assert!(p.join("m.txt").exists());
    assert_eq!(git(p, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
}

#[test]
fn checking_out_a_remote_branch_that_moved_on() {
    let (dir, remote) = with_remote();
    let p = dir.path();
    git(p, &["push", "-q", "origin", "main:feature"]);
    git(p, &["branch", "--track", "feature", "origin/feature"]);
    push_from_elsewhere(remote.path(), "feature", "f.txt");
    git(p, &["fetch", "-q"]);
    let mut h = harness(p);
    frames(&mut h);
    let cmd = {
        let tab = &h.state().tabs[0];
        let b = tab
            .branches
            .iter()
            .find(|b| b.name == "origin/feature")
            .unwrap();
        sync_dialogs::checkout_remote(tab, b)
    };
    open(&mut h, cmd);
    // Behind only: fast-forward is offered and chosen.
    h.get_by_role_and_label(Role::RadioButton, "Check out and fast-forward");
    h.get_by_role_and_label(Role::Button, "Check out").click();
    settle(&mut h);
    assert_eq!(git(p, &["rev-parse", "--abbrev-ref", "HEAD"]), "feature");
    assert!(p.join("f.txt").exists());

    // Diverged: rebase the local commit onto the remote branch.
    std::fs::write(p.join("mine.txt"), "m\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "mine"]);
    git(p, &["checkout", "-q", "main"]);
    push_from_elsewhere(remote.path(), "feature", "g.txt");
    git(p, &["fetch", "-q"]);
    settle(&mut h);
    h.state_mut().tabs[0].request_refresh(crate::jobs::Refresh::All);
    settle(&mut h);
    let cmd = {
        let tab = &h.state().tabs[0];
        let b = tab
            .branches
            .iter()
            .find(|b| b.name == "origin/feature")
            .unwrap();
        sync_dialogs::checkout_remote(tab, b)
    };
    open(&mut h, cmd);
    h.get_by_label_contains("have diverged");
    h.get_by_role_and_label(
        Role::RadioButton,
        "Check out and rebase onto origin/feature",
    )
    .click();
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Check out").click();
    settle(&mut h);
    assert_eq!(git(p, &["rev-parse", "--abbrev-ref", "HEAD"]), "feature");
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "mine");
    assert_eq!(git(p, &["log", "-1", "--format=%s", "HEAD~1"]), "g.txt");
}

#[test]
fn changes_the_tracking_branch_and_pulls() {
    let (dir, remote) = with_remote();
    let p = dir.path();
    git(p, &["push", "-q", "origin", "main:other"]);
    push_from_elsewhere(remote.path(), "other", "o.txt");
    git(p, &["fetch", "-q"]);
    std::fs::write(p.join("a.txt"), "dirty\n").unwrap();
    let mut h = harness(p);
    frames(&mut h);
    let cmd = {
        let tab = &h.state().tabs[0];
        Command::Dialog(Dialog::Sync(sync_dialogs::track(
            tab,
            tab.current_branch().unwrap(),
        )))
    };
    open(&mut h, cmd);
    pick(&mut h, "origin/main", "other");
    h.get_by_label("Pull from it now").click();
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Track").click();
    settle(&mut h);
    assert_eq!(
        git(p, &["rev-parse", "--abbrev-ref", "main@{upstream}"]),
        "origin/other"
    );
    assert!(p.join("o.txt").exists());
    assert_eq!(std::fs::read_to_string(p.join("a.txt")).unwrap(), "dirty\n");
}

#[test]
fn edits_and_tests_a_remote() {
    let (dir, remote) = with_remote();
    let p = dir.path();
    let url = remote.path().to_string_lossy().into_owned();
    let mut h = harness(p);
    frames(&mut h);
    open(
        &mut h,
        Command::Dialog(Dialog::Sync(sync_dialogs::edit_remote("origin", &url))),
    );
    h.get_by_role_and_label(Role::Button, "Test connection")
        .click();
    settle(&mut h);
    assert!(h.get_all_by_label_contains("— done").count() >= 1);
    let name = h
        .get_all_by_role(Role::TextInput)
        .find(|n| n.accesskit_node().value().as_deref() == Some("origin"))
        .unwrap();
    name.focus();
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_all_by_role(Role::TextInput)
        .find(|n| n.accesskit_node().value().as_deref() == Some("origin"))
        .unwrap()
        .type_text("upstream");
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Save").click();
    settle(&mut h);
    assert_eq!(git(p, &["remote"]), "upstream");
}

#[test]
fn fetches_automatically_in_the_background() {
    let (dir, remote) = with_remote();
    let p = dir.path();
    git(remote.path(), &["branch", "new", "main"]);
    let mut h = harness(p);
    frames(&mut h);
    // A remote left out is not fetched.
    {
        let app = h.state_mut();
        let tab = &app.tabs[0];
        app.settings.sync.no_auto_fetch.insert(
            crate::ui::sidebar_model::repo_key(tab.path()),
            ["origin".to_owned()].into(),
        );
        app.tabs[0].sync.next_fetch = Some(Instant::now());
    }
    frames(&mut h);
    assert!(!h.state().is_busy());
    assert!(git(p, &["branch", "-r"]).find("origin/new").is_none());

    h.state_mut().settings.sync.no_auto_fetch.clear();
    h.state_mut().tabs[0].sync.next_fetch = Some(Instant::now());
    frames(&mut h);
    settle(&mut h);
    assert!(git(p, &["branch", "-r"]).contains("origin/new"));
    let op = crate::jobs::operations()
        .into_iter()
        .rev()
        .find(|o| o.repo.as_deref().is_some_and(|r| same_path(r, p)))
        .unwrap();
    assert!(op.background);
    assert!(op.commands()[0].command.contains("--prune"));
    // The next one is a whole interval away.
    let next = h.state().tabs[0].sync.next_fetch.unwrap();
    assert!(next > Instant::now() + std::time::Duration::from_secs(60));
}

/// Paths compared after resolving links and spelling (macOS /private/var,
/// Windows short names and slashes).
fn same_path(a: &Path, b: &Path) -> bool {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    real(a) == real(b)
}
