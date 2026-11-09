//! End-to-end tests of the diff viewer.

use std::path::Path;

use eframe::egui::{Key, Modifiers, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use ferrum_core::DiffSource;

use crate::app::FerrumApp;
use crate::tab::View;
use crate::ui_tests::{frames, git, harness, repo, settle};

/// Opens the local changes with `path` selected.
fn select(h: &mut Harness<'static, FerrumApp>, path: &str) {
    let tab = &mut h.state_mut().tabs[0];
    tab.view = View::Changes;
    tab.select_change(DiffSource::Unstaged, path.into());
    frames(h);
}

fn write(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
}

#[test]
fn side_by_side_stages_hunks_and_selected_lines() {
    let dir = repo();
    write(dir.path(), "a.txt", "one\ntwo\nthree\n");
    let mut h = harness(dir.path());
    h.state_mut().settings.diff.side_by_side = true;
    select(&mut h, "a.txt");

    // Lines 1 and 2 of the hunk are "+two" and "+three"; pick only "two".
    h.state_mut().tabs[0].changes.selection.lines.insert((0, 1));
    frames(&mut h);
    // The toolbar and the floating buttons next to the selection.
    let buttons: Vec<_> = h.get_all_by_label("Stage lines").collect();
    assert_eq!(buttons.len(), 2);
    buttons[1].click();
    settle(&mut h);
    let staged = git(dir.path(), &["diff", "--cached"]);
    assert!(
        staged.contains("+two") && !staged.contains("+three"),
        "{staged}"
    );

    // The rest as a hunk, still side by side.
    h.get_by_label("Stage hunk").click();
    settle(&mut h);
    assert_eq!(git(dir.path(), &["diff", "--name-only"]), "");
    assert!(h.state().settings.diff.side_by_side);
}

#[test]
fn ctrl_f_finds_in_the_diff_under_the_pointer() {
    let dir = repo();
    let mut h = harness(dir.path());
    select(&mut h, "a.txt");

    // Over the diff: its find bar opens with the focus.
    h.hover_at(pos2(1000.0, 500.0));
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    frames(&mut h);
    let field = h
        .get_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .find(|n| n.is_focused())
        .expect("find field");
    field.type_text("two");
    frames(&mut h);
    assert_eq!(h.state().tabs[0].view, View::Changes);
    h.get_by_label("✖").click();
    frames(&mut h);

    // Elsewhere it still searches the commits.
    h.hover_at(pos2(100.0, 500.0));
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    frames(&mut h);
    assert_eq!(h.state().tabs[0].view, View::History);
}

#[test]
fn space_opens_the_large_diff_window() {
    let dir = repo();
    let mut h = harness(dir.path());
    select(&mut h, "a.txt");
    assert!(
        h.query_by_label("Space or Esc closes this window")
            .is_none()
    );
    h.key_press(Key::Space);
    frames(&mut h);
    assert!(
        h.query_by_label("Space or Esc closes this window")
            .is_some()
    );
    // Hunk staging works from the large window too.
    assert_eq!(h.get_all_by_label("Stage hunk").count(), 2);
    h.key_press(Key::Space);
    frames(&mut h);
    assert!(
        h.query_by_label("Space or Esc closes this window")
            .is_none()
    );
}

#[test]
fn large_untracked_files_load_on_request() {
    let dir = repo();
    write(dir.path(), "big.log", &"line\n".repeat(600_000));
    let mut h = harness(dir.path());
    select(&mut h, "big.log");
    assert!(h.query_by_label("Stage hunk").is_none());
    h.get_by_label("Load diff").click();
    frames(&mut h);
    assert!(h.query_by_label("Stage hunk").is_some());
    let diff = h.state().tabs[0].changes.diff.as_ref().unwrap();
    assert_eq!(diff.additions, 600_000);
}

#[test]
fn diff_options_reload_the_diff() {
    let dir = repo();
    write(dir.path(), "a.txt", "one  \ntwo\n");
    let mut h = harness(dir.path());
    select(&mut h, "a.txt");
    assert_eq!(
        h.state().tabs[0].changes.diff.as_ref().unwrap().additions,
        2
    );
    h.get_by_label(crate::icons::PARAGRAPH).click();
    frames(&mut h);
    assert_eq!(
        h.state().tabs[0].changes.diff.as_ref().unwrap().additions,
        1
    );
    assert!(h.state().settings.diff.ignore_whitespace);
    // Whitespace-free diffs cannot be staged line by line.
    assert!(h.query_by_label("Stage hunk").is_none());
}
