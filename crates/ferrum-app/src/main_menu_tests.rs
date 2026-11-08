//! UI tests of the main menu bar.

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::tab::View;
use crate::ui_tests::{frames, harness, repo};

/// Clicks a menu of the bar (the first button with that name).
fn open_menu(h: &mut egui_kittest::Harness<'_, crate::app::FerrumApp>, name: &str) {
    h.query_all_by_label(name).next().unwrap().click();
    frames(h);
}

#[test]
fn menu_items_run_their_actions() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    assert_eq!(h.state().tabs[0].view, View::History);
    open_menu(&mut h, "View");
    h.get_by_label_contains("Local changes").click();
    frames(&mut h);
    assert_eq!(h.state().tabs[0].view, View::Changes);
    // The menu shows the key from the keymap.
    open_menu(&mut h, "View");
    assert!(h.query_by_label_contains("Ctrl+1").is_some() || cfg!(target_os = "macos"));
    h.get_by_label_contains("History").click();
    frames(&mut h);
    assert_eq!(h.state().tabs[0].view, View::History);
}

#[test]
fn menu_toggles_settings() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    let before = h.state().settings.details.collapsed;
    open_menu(&mut h, "View");
    h.get_by_label_contains("Commit details").click();
    frames(&mut h);
    assert_eq!(h.state().settings.details.collapsed, !before);
    open_menu(&mut h, "View");
    h.get_by_label_contains("Vertical tab list").click();
    frames(&mut h);
    assert!(h.state().settings.library.vertical_tabs);
}

#[test]
fn keyboard_opens_and_switches_menus() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    assert!(h.query_by_label_contains("Open Quickly…").is_none());
    h.key_press(Key::F10);
    frames(&mut h);
    assert!(h.query_by_label_contains("Open Quickly…").is_some());
    // Right: the Edit menu.
    h.key_press(Key::ArrowRight);
    frames(&mut h);
    assert!(h.query_by_label_contains("Open Quickly…").is_none());
    assert!(h.query_by_label_contains("Find previous").is_some());
    h.key_press(Key::F10);
    frames(&mut h);
    assert!(h.query_by_label_contains("Find previous").is_none());
    // Alt+H: Help.
    h.key_press_modifiers(Modifiers::ALT, Key::H);
    frames(&mut h);
    assert!(h.query_by_label_contains("Report an issue").is_some());
}

#[test]
fn menu_bar_can_be_hidden() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.state_mut().settings.prefs.hide_menu_bar = true;
    frames(&mut h);
    assert!(h.query_by_label("Help").is_none());
    h.state_mut().settings.prefs.hide_menu_bar = false;
    frames(&mut h);
    assert!(h.query_by_label("Help").is_some());
}
