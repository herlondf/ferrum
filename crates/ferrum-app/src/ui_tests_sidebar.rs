//! Sidebar: folders, pins, filter, stale branches, keyboard, drag and drop.

use std::path::Path;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use super::{frames, git, harness, repo, settle};
use crate::app::FerrumApp;
use crate::ui::focus::Pane;

/// main, feature/alpha, feature/beta and solo, plus an annotated tag.
fn branches_repo() -> tempfile::TempDir {
    let dir = repo();
    let p = dir.path();
    git(p, &["stash", "-q"]);
    for b in ["feature/alpha", "feature/beta", "solo"] {
        git(p, &["branch", b]);
    }
    git(p, &["tag", "-a", "v1", "-m", "First release"]);
    dir
}

/// The sidebar row of a branch: its label is the name, then icons.
fn row<'a>(h: &'a Harness<'_, FerrumApp>, name: &str) -> egui_kittest::Node<'a> {
    h.get_all_by_role(Role::Button)
        .find(|n| {
            let label = n.accesskit_node().label().unwrap_or_default();
            let label = label.trim_start_matches("✔ ");
            label.split_whitespace().next() == Some(name)
        })
        .unwrap_or_else(|| panic!("no row {name}"))
}

fn has_row(h: &Harness<'_, FerrumApp>, name: &str) -> bool {
    h.get_all_by_role(Role::Button).any(|n| {
        let label = n.accesskit_node().label().unwrap_or_default();
        label.split_whitespace().next() == Some(name)
    })
}

fn filter_field<'a>(h: &'a Harness<'_, FerrumApp>) -> egui_kittest::Node<'a> {
    h.get_all_by_role(Role::TextInput)
        .find(|n| n.accesskit_node().placeholder() == Some("🔍 Filter"))
        .expect("filter field")
}

fn current_branch(p: &Path) -> String {
    git(p, &["rev-parse", "--abbrev-ref", "HEAD"])
}

#[test]
fn folders_pins_filter_and_move() {
    let dir = branches_repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    // Branches are grouped in a folder, shown by their last part.
    assert!(has_row(&h, "alpha"));
    h.get_by_label_contains("📁 feature");

    // Move "solo" into the folder.
    row(&h, "solo").click_secondary();
    frames(&mut h);
    h.get_by_label("Move to folder").click();
    frames(&mut h);
    h.get_by_label("📁 feature").click();
    settle(&mut h);
    assert_eq!(
        git(dir.path(), &["branch", "--list", "feature/solo"]),
        "feature/solo"
    );

    // Pin a branch: it shows in its own section and is saved per repository.
    row(&h, "alpha").click_secondary();
    frames(&mut h);
    h.get_by_label("📌 Pin to the top").click();
    frames(&mut h);
    h.get_by_label_contains("📌 Pinned");
    assert!(has_row(&h, "feature/alpha"));
    let key = crate::ui::sidebar_model::repo_key(h.state().tabs[0].path());
    let saved = &h.state().settings.repos[&key];
    assert_eq!(saved.pinned, ["refs/heads/feature/alpha"]);

    // The filter keeps only matching refs.
    filter_field(&h).focus();
    frames(&mut h);
    filter_field(&h).type_text("beta");
    frames(&mut h);
    assert!(has_row(&h, "beta"));
    assert!(!has_row(&h, "alpha"));
    assert!(!has_row(&h, "feature/alpha"));
}

#[test]
fn select_stale_branches_and_delete() {
    let dir = branches_repo();
    let p = dir.path();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    let url = remote.path().to_string_lossy().into_owned();
    git(p, &["remote", "add", "origin", &url]);
    git(p, &["push", "-q", "-u", "origin", "main", "solo"]);
    git(p, &["push", "-q", "origin", "--delete", "solo"]);

    let mut h = harness(p);
    frames(&mut h);
    h.get_by_label_contains("Branches").click_secondary();
    frames(&mut h);
    h.get_by_label("Select stale local branches (1)").click();
    frames(&mut h);
    h.get_by_label("1 selected");
    h.get_by_label("Delete…").click();
    frames(&mut h);
    h.get_by_label("Yes, do it").click();
    settle(&mut h);
    assert_eq!(git(p, &["branch", "--list", "solo"]), "");
    assert_eq!(git(p, &["branch", "--list", "main"]), "* main");
}

#[test]
fn double_click_tag_and_keyboard_checkout() {
    let dir = branches_repo();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);

    // Tab moves the keyboard to the sidebar; arrows and Enter check out.
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    frames(&mut h);
    assert_eq!(h.state().tabs[0].pane.pane, Pane::Sidebar);
    let rows = &h.state().tabs[0].sidebar.rows;
    let target = rows
        .iter()
        .position(|r| r.key == "ref:refs/heads/solo")
        .expect("solo row");
    for _ in 0..=target {
        h.key_press(egui::Key::ArrowDown);
        h.step();
    }
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert_eq!(current_branch(p), "solo");
    // Tab goes on to the commit list, then the details.
    h.key_press(egui::Key::Tab);
    frames(&mut h);
    assert_eq!(h.state().tabs[0].pane.pane, Pane::Commits);
    h.key_press(egui::Key::Tab);
    frames(&mut h);
    assert_eq!(h.state().tabs[0].pane.pane, Pane::Details);

    // Double-clicking a tag checks it out (detached).
    // The harness spends a step (0.25 s) per input event: allow slower
    // double-clicks, but not so slow that the header click counts.
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 1.5);
    h.get_by_label_contains("Tags (1)").click();
    frames(&mut h);
    frames(&mut h);
    h.get_by_label("🏷 v1").click();
    h.get_by_label("🏷 v1").click();
    settle(&mut h);
    assert_eq!(current_branch(p), "HEAD");
    let tags = &h.state().tabs[0].tags;
    assert_eq!(tags[0].message.as_deref(), Some("First release"));
}

#[test]
fn drag_branch_onto_another_to_merge() {
    let dir = branches_repo();
    let p = dir.path();
    git(p, &["commit", "-q", "--allow-empty", "-m", "on main"]);
    let mut h = harness(p);
    frames(&mut h);

    let from = row(&h, "main").rect().center();
    let to = row(&h, "solo").rect().center();
    h.drag_at(from);
    h.step();
    for i in 1..=5 {
        let t = i as f32 / 5.0;
        h.hover_at(from + (to - from) * t);
        h.step();
    }
    h.drop_at(to);
    frames(&mut h);
    h.get_by_label("Merge main into solo…").click();
    frames(&mut h);
    h.get_by_label("OK").click();
    settle(&mut h);
    assert_eq!(current_branch(p), "solo");
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "on main");
}
