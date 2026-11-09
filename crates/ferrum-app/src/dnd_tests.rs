//! UI tests of drag and drop (branches and files), the start screen cards,
//! the commit area and the command palette.

use std::path::Path;

use eframe::egui::{self, Key, Modifiers, Pos2, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::app::FerrumApp;
use crate::ui_tests::{frames, git, harness, repo, settle};

/// Drags with the mouse from `from` to `to` and releases there.
fn drag(h: &mut Harness<'_, FerrumApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.step();
    h.drag_at(from);
    h.step();
    for i in 1..=6 {
        h.hover_at(from + (to - from) * (i as f32 / 6.0));
        h.step();
    }
    // Release without taking the cursor away, as a real mouse does.
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    frames(h);
}

/// The sidebar row whose label starts with `name`.
fn row<'a>(h: &'a Harness<'_, FerrumApp>, name: &str) -> egui_kittest::Node<'a> {
    h.get_all_by_role(Role::Button)
        .find(|n| {
            let label = n.accesskit_node().label().unwrap_or_default();
            let label = label.trim_start_matches("✔ ");
            label.split_whitespace().next() == Some(name)
        })
        .unwrap_or_else(|| panic!("no row {name}"))
}

/// `main` with one commit and `topic` with two more; `main` checked out.
fn topic_repo() -> tempfile::TempDir {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    git(p, &["checkout", "-q", "-b", "topic"]);
    for f in ["b.txt", "c.txt"] {
        std::fs::write(p.join(f), "x\n").unwrap();
        git(p, &["add", f]);
        git(p, &["commit", "-qm", f]);
    }
    git(p, &["checkout", "-q", "main"]);
    dir
}

fn log(p: &Path) -> String {
    git(p, &["log", "--format=%s", "main"])
}

#[test]
fn dropping_a_branch_asks_first_then_cherry_picks() {
    let dir = topic_repo();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);
    let (from, to) = (
        row(&h, "topic").rect().center(),
        row(&h, "main").rect().center(),
    );
    drag(&mut h, from, to);
    // A menu asks what to do; nothing has run yet.
    h.get_by_label_contains("Merge topic into main…");
    h.get_by_label_contains("Rebase topic onto main…");
    assert_eq!(log(p), "first");
    // Escape closes it without doing anything.
    h.key_press(Key::Escape);
    frames(&mut h);
    assert!(
        h.query_by_label_contains("Merge topic into main…")
            .is_none()
    );

    drag(&mut h, from, to);
    h.get_by_label_contains("Cherry-pick topic onto main…")
        .click();
    frames(&mut h);
    // main is checked out: the cherry-pick dialog with both commits.
    assert!(h.state().has_dialog());
    assert_eq!(log(p), "first");
    h.get_by_role_and_label(Role::Button, "Cherry-pick").click();
    settle(&mut h);
    assert_eq!(log(p), "c.txt\nb.txt\nfirst");
}

#[test]
fn dropping_a_branch_on_a_remote_offers_to_push() {
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    let dir = topic_repo();
    let p = dir.path();
    let url = remote.path().to_string_lossy().into_owned();
    git(p, &["remote", "add", "origin", &url]);
    git(p, &["push", "-q", "-u", "origin", "main"]);
    let mut h = harness(p);
    frames(&mut h);
    let origin = h
        .get_all_by_role(Role::Button)
        .find(|n| {
            let l = n.accesskit_node().label().unwrap_or_default();
            l.contains("☁") && l.contains("origin")
        })
        .expect("origin row")
        .rect()
        .center();
    let topic = row(&h, "topic").rect().center();
    drag(&mut h, topic, origin);
    h.get_by_label_contains("Push topic to origin…").click();
    frames(&mut h);
    // The push dialog confirms it.
    assert!(h.state().has_dialog());
    h.get_by_role_and_label(Role::Button, "Push").click();
    settle(&mut h);
    git(remote.path(), &["rev-parse", "--verify", "topic"]);
}

#[test]
fn drags_several_selected_files_to_the_staged_list() {
    let dir = repo();
    let p = dir.path();
    for f in ["b.txt", "c.txt"] {
        std::fs::write(p.join(f), "new\n").unwrap();
    }
    let mut h = harness(p);
    frames(&mut h);
    h.get_by_label_contains("Local Changes").click();
    frames(&mut h);
    // Pick two files with Ctrl+click.
    let node = |h: &'_ Harness<'_, FerrumApp>, name: &str| {
        h.get_all_by_role(Role::Button)
            .find(|n| n.accesskit_node().label().as_deref() == Some(name))
            .expect("file row")
            .rect()
            .center()
    };
    let c = node(&h, "c.txt");
    h.get_all_by_role(Role::Button)
        .find(|n| n.accesskit_node().label().as_deref() == Some("a.txt"))
        .unwrap()
        .click();
    frames(&mut h);
    h.get_all_by_role(Role::Button)
        .find(|n| n.accesskit_node().label().as_deref() == Some("c.txt"))
        .unwrap()
        .click_modifiers(Modifiers::COMMAND);
    frames(&mut h);
    let to = h.get_by_label("Nothing staged yet.").rect().center();
    drag(&mut h, c, to);
    settle(&mut h);
    let staged = git(p, &["diff", "--cached", "--name-only"]);
    assert_eq!(staged, "a.txt\nc.txt");
}

#[test]
fn commit_button_says_what_is_committed() {
    let dir = repo();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);
    h.get_by_label_contains("Local Changes").click();
    frames(&mut h);
    // Nothing staged: the main button stages everything first.
    h.get_by(|n| n.placeholder() == Some("Summary")).focus();
    frames(&mut h);
    h.get_by(|n| n.placeholder() == Some("Summary"))
        .type_text("Change a");
    frames(&mut h);
    assert_eq!(h.state().tabs[0].changes.message, "Change a");
    h.get_by_label_contains("Stage and commit 1 file").click();
    frames(&mut h);
    settle(&mut h);
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "Change a");
}

#[test]
fn start_screen_cards_show_branch_and_changes() {
    let a = repo();
    let mut h = Harness::builder()
        .with_size([1400.0, 900.0])
        .build_eframe(|cc| FerrumApp::new(cc, Vec::new()));
    h.state_mut().settings.language = crate::i18n::Language::English;
    h.state_mut().settings.library.remember(a.path());
    // The card's branch and changes dot load in the background.
    let glance =
        |h: &Harness<'_, FerrumApp>| h.state().repo_state().manager.glance(a.path()).cloned();
    for _ in 0..200 {
        h.step();
        if glance(&h).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let g = glance(&h).expect("loaded");
    assert_eq!(g.branch.as_deref(), Some("main"));
    assert!(g.dirty);
    let name = a.path().file_name().unwrap().to_string_lossy().into_owned();
    h.get_by_label(&name);
    // The search field has the focus: typing filters, Enter opens.
    h.get_by(|n| n.placeholder() == Some("Filter repositories"))
        .type_text(&name[..4]);
    frames(&mut h);
    h.key_press(Key::Enter);
    frames(&mut h);
    assert_eq!(h.state().tabs.len(), 1);
}

#[test]
fn palette_lists_recent_commands_first() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    frames(&mut h);
    h.get_by(|n| n.placeholder() == Some("Type a command, branch or file…"))
        .type_text("Show local changes");
    frames(&mut h);
    h.key_press(Key::Enter);
    frames(&mut h);
    assert_eq!(h.state().settings.palette_recent, ["Show local changes"]);
    h.state_mut().tabs[0].view = crate::tab::View::History;
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    frames(&mut h);
    // The first entry is the one run last.
    h.key_press(Key::Enter);
    frames(&mut h);
    assert_eq!(h.state().tabs[0].view, crate::tab::View::Changes);
}
