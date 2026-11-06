//! UI tests of submodules, worktrees, Git LFS and the settings shared
//! between worktrees.

use std::path::Path;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::FerrumApp;
use crate::command::{Command, Op};
use crate::repo_parts::AutoUpdate;
use crate::ui::dialogs::Dialog;
use crate::ui::submodules::SubmoduleDialog;
use crate::ui::worktrees::WorktreeDialog;
use crate::ui_tests::{frames, git, harness, repo, settle};

/// Commits in a submodule's folder (it has no identity of its own).
fn commit_in(dir: &Path, msg: &str) {
    git(
        dir,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qam",
            msg,
        ],
    );
}

/// A repository with a submodule at `sub`, and a branch `other` at HEAD.
fn with_submodule() -> (tempfile::TempDir, tempfile::TempDir) {
    let lib = repo();
    git(lib.path(), &["commit", "-qam", "two"]);
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    let url = lib.path().to_string_lossy().replace('\\', "/");
    git(
        p,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &url,
            "sub",
        ],
    );
    git(p, &["commit", "-qm", "add sub"]);
    git(p, &["branch", "other"]);
    (dir, lib)
}

fn open_section(h: &mut Harness<'_, FerrumApp>, title: &str) {
    h.get_by_label_contains(title).click();
    frames(h);
}

fn run(h: &mut Harness<'_, FerrumApp>, cmd: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(cmd, &ctx);
    frames(h);
}

#[test]
fn submodule_status_discard_move_and_remove() {
    let (dir, _lib) = with_submodule();
    let p = dir.path();
    std::fs::write(p.join("sub/a.txt"), "changed\n").unwrap();
    let mut h = harness(p);
    settle(&mut h);
    let s = &h.state().tabs[0].submodules[0];
    assert!(s.dirty && !s.moved);
    open_section(&mut h, "Submodules (1)");
    // The row shows the uncommitted changes.
    h.get_by_label_contains("▣ sub").click_secondary();
    frames(&mut h);
    h.get_by_label("Discard changes…").click();
    frames(&mut h);
    h.get_by_label("Yes, do it").click();
    settle(&mut h);
    assert_eq!(
        std::fs::read_to_string(p.join("sub/a.txt")).unwrap(),
        "one\ntwo\n"
    );
    assert!(!h.state().tabs[0].submodules[0].dirty);

    // Move it with the dialog.
    run(
        &mut h,
        Command::Dialog(Dialog::Submodule(SubmoduleDialog::Move {
            from: "sub".into(),
            to: "libs/sub".into(),
        })),
    );
    h.get_by_label("Move").click();
    settle(&mut h);
    assert!(p.join("libs/sub/a.txt").exists());
    assert!(
        std::fs::read_to_string(p.join(".gitmodules"))
            .unwrap()
            .contains("path = libs/sub")
    );
    git(p, &["commit", "-qam", "move"]);
    settle(&mut h);

    // Remove it from the context menu.
    h.get_by_label_contains("▣ libs/sub").click_secondary();
    frames(&mut h);
    h.get_by_label("Remove…").click();
    frames(&mut h);
    h.get_by_label("Yes, do it").click();
    settle(&mut h);
    assert!(!p.join("libs/sub").exists());
    assert!(!git(p, &["status", "--porcelain"]).is_empty());
}

#[test]
fn submodules_follow_checkouts_when_asked() {
    let (dir, _lib) = with_submodule();
    let p = dir.path();
    // Move the submodule to a newer commit without recording it.
    std::fs::write(p.join("sub/a.txt"), "three\n").unwrap();
    commit_in(&p.join("sub"), "three");
    let mut h = harness(p);
    settle(&mut h);
    assert!(h.state().tabs[0].submodules[0].moved);

    // Ask: a checkout asks, and OK updates.
    h.state_mut().settings.submodules.auto_update = AutoUpdate::Ask;
    run(&mut h, Command::Run(Op::Checkout("other".into())));
    settle(&mut h);
    h.get_by_label_contains("not at the commit recorded");
    h.get_by_label("OK").click();
    settle(&mut h);
    assert_eq!(
        std::fs::read_to_string(p.join("sub/a.txt")).unwrap(),
        "one\ntwo\n"
    );

    // Always: updated without asking.
    std::fs::write(p.join("sub/a.txt"), "four\n").unwrap();
    commit_in(&p.join("sub"), "four");
    h.state_mut().settings.submodules.auto_update = AutoUpdate::Always;
    run(&mut h, Command::Run(Op::Checkout("main".into())));
    settle(&mut h);
    assert_eq!(
        std::fs::read_to_string(p.join("sub/a.txt")).unwrap(),
        "one\ntwo\n"
    );
    assert!(!h.state().tabs[0].submodules[0].moved);
}

#[test]
fn submodule_opens_next_to_its_parent() {
    let (dir, _lib) = with_submodule();
    let other = repo();
    let mut h = harness(dir.path());
    settle(&mut h);
    run(&mut h, Command::OpenRepo(other.path().to_path_buf()));
    run(&mut h, Command::SelectTab(0));
    open_section(&mut h, "Submodules (1)");
    h.get_by_label_contains("▣ sub").click_secondary();
    frames(&mut h);
    h.get_by_label("Open in new tab").click();
    settle(&mut h);
    let tabs = &h.state().tabs;
    assert_eq!(tabs.len(), 3);
    assert!(
        tabs[1].path().ends_with("sub"),
        "{}",
        tabs[1].path().display()
    );
    assert_eq!(tabs[2].path().file_name(), other.path().file_name());
}

#[test]
fn branch_in_another_worktree_opens_it() {
    let dir = repo();
    let p = dir.path();
    git(p, &["branch", "feature"]);
    git(p, &["branch", "solo"]);
    let out = tempfile::tempdir().unwrap();
    let wt = out.path().join("wt-feature");
    git(
        p,
        &["worktree", "add", "-q", &wt.to_string_lossy(), "feature"],
    );
    let mut h = harness(p);
    settle(&mut h);
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 1.5);

    // The branch row has the worktree icon; double-click opens the worktree.
    let label = "feature 📂";
    h.get_by_label_contains(label).click();
    h.get_by_label_contains(label).click();
    settle(&mut h);
    let tabs = &h.state().tabs;
    assert_eq!(tabs.len(), 2);
    assert_eq!(
        std::fs::canonicalize(tabs[1].path()).unwrap(),
        std::fs::canonicalize(&wt).unwrap()
    );
    assert_eq!(git(p, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");

    // Linked worktrees are listed under the main one.
    run(&mut h, Command::SelectTab(0));
    open_section(&mut h, "Worktrees (2)");
    h.get_by_label_contains("(feature)");

    // Check out another branch as a worktree.
    h.get_by_label_contains("solo").click_secondary();
    frames(&mut h);
    h.get_by_label("Check out as worktree…").click();
    frames(&mut h);
    h.get_by_label("Check out as worktree");
    // The suggested folder sits next to the repository; use a temporary one.
    let d = WorktreeDialog::checkout(&h.state().tabs[0], "solo", false);
    assert!(d.path().ends_with("-solo"), "{}", d.path());
    let dest = out.path().join("wt-solo");
    let d = d.with_path(dest.to_string_lossy().into_owned());
    run(&mut h, Command::Dialog(Dialog::Worktree(d)));
    h.get_by_label("Add worktree").click();
    settle(&mut h);
    assert!(dest.join("a.txt").exists());
    let list = git(p, &["worktree", "list", "--porcelain"]);
    assert!(list.contains("branch refs/heads/solo"), "{list}");
}

#[test]
fn worktrees_share_pins_but_not_view_state() {
    let dir = repo();
    let p = dir.path();
    git(p, &["branch", "feature"]);
    let out = tempfile::tempdir().unwrap();
    let wt = out.path().join("wt");
    git(
        p,
        &["worktree", "add", "-q", &wt.to_string_lossy(), "feature"],
    );
    let mut h = harness(p);
    settle(&mut h);
    run(&mut h, Command::OpenRepo(wt.clone()));
    settle(&mut h);
    run(&mut h, Command::SelectTab(0));

    // Pin in the main worktree: the other tab gets the pin too.
    h.state_mut().tabs[0]
        .sidebar
        .toggle_pin("refs/heads/feature");
    h.state_mut().tabs[0].sidebar.set_open("sec:tags", true);
    frames(&mut h);
    let app = h.state();
    assert_eq!(app.tabs[1].sidebar.prefs.pinned, ["refs/heads/feature"]);
    assert!(!app.tabs[1].sidebar.is_open("sec:tags", false));
    let shared = crate::repo_settings::shared_key(&app.tabs[0]);
    assert_eq!(shared, crate::repo_settings::shared_key(&app.tabs[1]));
    assert_eq!(app.settings.repos[&shared].pinned, ["refs/heads/feature"]);

    // A new tab of the worktree reads the shared pins.
    let prefs = crate::repo_settings::sidebar_prefs(&app.settings, &app.tabs[1]);
    assert_eq!(prefs.pinned, ["refs/heads/feature"]);
}

#[test]
fn lfs_dialog_shows_patterns_or_missing_lfs() {
    let dir = repo();
    let p = dir.path();
    std::fs::write(
        p.join(".gitattributes"),
        "*.psd filter=lfs diff=lfs merge=lfs -text\n",
    )
    .unwrap();
    let mut h = harness(p);
    settle(&mut h);
    let ctx = h.ctx.clone();
    let available = loop {
        if let Some(a) = crate::ui::lfs::available(&ctx) {
            break a;
        }
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let d = crate::ui::lfs::LfsDialog::track(&h.state().tabs[0]);
    run(&mut h, Command::Dialog(Dialog::Lfs(d)));
    if available {
        h.get_by_label("*.psd");
    } else {
        h.get_by_label("Git LFS is not installed");
    }
}
