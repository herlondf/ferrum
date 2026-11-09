//! UI tests of the toolbar, the tab bar, the status bar and the toasts.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::app::FerrumApp;
use crate::command::Command;
use crate::icons;
use crate::repos::RepoCmd;
use crate::ui_tests::{frames, git, harness, repo, settle};

fn handle(h: &mut Harness<'_, FerrumApp>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(c, &ctx);
    frames(h);
}

/// A repository with a bare remote, one commit ahead of it.
fn ahead_of_remote() -> (tempfile::TempDir, tempfile::TempDir) {
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    let url = remote.path().to_string_lossy().into_owned();
    git(p, &["remote", "add", "origin", &url]);
    git(p, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(p.join("b.txt"), "b\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", "second"]);
    (dir, remote)
}

fn height_of(h: &Harness<'_, FerrumApp>, label: &str) -> f64 {
    let node = h.get_by_label(label);
    let b = node.accesskit_node().raw_bounds().expect("bounds");
    b.y1 - b.y0
}

#[test]
fn push_from_the_toolbar_says_what_it_did() {
    let (dir, remote) = ahead_of_remote();
    let mut h = harness(dir.path());
    frames(&mut h);
    // The status bar shows the branch, its upstream and what is ahead.
    h.get_by_label_contains("origin/main");
    h.get_by_label(&icons::text(icons::PUSH, "Push")).click();
    frames(&mut h);
    settle(&mut h);
    h.get_by_label("Pushed 1 commit to origin/main");
    assert_eq!(
        git(remote.path(), &["log", "-1", "--format=%s", "main"]),
        "second"
    );
}

#[test]
fn error_toasts_stay_until_dismissed() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.state_mut().tabs[0].error("git failed: something broke");
    frames(&mut h);
    h.get_by_label("git failed: something broke");
    // Still there later; Copy is offered.
    for _ in 0..30 {
        h.step();
    }
    h.get_by_label("Copy");
    h.get_by_label("Dismiss").click();
    frames(&mut h);
    assert!(h.query_by_label("git failed: something broke").is_none());
}

#[test]
fn info_toasts_fade_out() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.state_mut().tabs[0].info("Refreshed");
    frames(&mut h);
    h.get_by_label("Refreshed");
    std::thread::sleep(std::time::Duration::from_millis(4700));
    frames(&mut h);
    assert!(h.query_by_label("Refreshed").is_none());
}

#[test]
fn tabs_move_close_and_keep_the_active_one() {
    let a = repo();
    let b = repo();
    let mut h = harness(a.path());
    handle(&mut h, Command::OpenRepo(b.path().to_path_buf()));
    assert_eq!(h.state().tabs.len(), 2);
    let active = h.state().tabs[h.state().active_index()]
        .path()
        .to_path_buf();
    handle(&mut h, Command::Repos(RepoCmd::MoveTab(1, 0)));
    let s = h.state();
    assert_eq!(s.tabs[s.active_index()].path(), active);
    assert_eq!(s.active_index(), 0);
    // Only the active tab shows its ×.
    assert_eq!(h.query_all_by_label(icons::CLOSE).count(), 1);
    h.get_by_label(icons::CLOSE).click();
    frames(&mut h);
    assert_eq!(h.state().tabs.len(), 1);
}

#[test]
fn status_bar_opens_the_identity_dialog_and_quick_checkout() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label(&icons::text(icons::IDENTITY, "Test"))
        .click();
    frames(&mut h);
    assert!(h.state().has_dialog());
    h.key_press(eframe::egui::Key::Escape);
    frames(&mut h);
    assert!(!h.state().has_dialog());
    h.get_by_label(&icons::text(icons::BRANCH, "main")).click();
    frames(&mut h);
    assert!(h.state().has_dialog());
}

#[test]
fn narrow_windows_hide_the_toolbar_labels() {
    let dir = repo();
    let wide = harness(dir.path());
    let fetch = icons::text(icons::FETCH, "Fetch");
    assert!(height_of(&wide, &fetch) >= 38.0);
    let path = dir.path().to_path_buf();
    let mut narrow = Harness::builder()
        .with_size([520.0, 700.0])
        .build_eframe(move |cc| FerrumApp::new(cc, vec![path]));
    narrow.state_mut().settings.language = crate::i18n::Language::English;
    frames(&mut narrow);
    assert!(height_of(&narrow, &fetch) < 34.0);
}

#[test]
fn toolbar_button_of_a_running_operation() {
    use crate::prefs::ToolbarButton as Tb;
    use crate::ui::toolbar::button_for;
    assert_eq!(button_for(&t!("Fetch all remotes")), Some(Tb::Fetch));
    assert_eq!(button_for(&t!("Fetch {}", "origin")), Some(Tb::Fetch));
    assert_eq!(button_for(&t!("Pull")), Some(Tb::Pull));
    assert_eq!(
        button_for(&t!("Push {} to {}", "main", "origin")),
        Some(Tb::Push)
    );
    assert_eq!(button_for(&t!("Undo: {}", "Commit")), Some(Tb::Undo));
    assert_eq!(button_for(&t!("Commit")), None);
}
