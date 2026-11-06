//! UI tests of the branch, tag and stash dialogs.

use std::path::Path;

use eframe::egui::accesskit::Role;
use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::app::FerrumApp;
use crate::ui_tests::{frames, git, harness, repo, settle};

/// main, feature/alpha, feature/beta and solo, with a clean working copy.
fn branches_repo() -> tempfile::TempDir {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "."]);
    for b in ["feature/alpha", "feature/beta", "solo"] {
        git(p, &["branch", b]);
    }
    dir
}

fn current_branch(p: &Path) -> String {
    git(p, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// The text field with this placeholder.
fn field<'a>(h: &'a Harness<'_, FerrumApp>, placeholder: &str) -> egui_kittest::Node<'a> {
    let placeholder = placeholder.to_owned();
    h.get_all_by_role(Role::TextInput)
        .find(|n| n.accesskit_node().placeholder() == Some(placeholder.as_str()))
        .unwrap_or_else(|| panic!("no field {placeholder}"))
}

fn type_into(h: &mut Harness<'_, FerrumApp>, placeholder: &str, text: &str) {
    field(h, placeholder).focus();
    frames(h);
    field(h, placeholder).type_text(text);
    frames(h);
}

/// The sidebar row of a branch.
fn row<'a>(h: &'a Harness<'_, FerrumApp>, name: &str) -> egui_kittest::Node<'a> {
    h.get_all_by_role(Role::Button)
        .find(|n| {
            let label = n.accesskit_node().label().unwrap_or_default();
            let label = label.trim_start_matches("✔ ");
            label.split_whitespace().next() == Some(name)
        })
        .unwrap_or_else(|| panic!("no row {name}"))
}

/// The text field showing this text.
fn input<'a>(h: &'a Harness<'_, FerrumApp>, value: &str) -> egui_kittest::Node<'a> {
    let value = value.to_owned();
    h.get_all_by_role(Role::TextInput)
        .find(|n| n.accesskit_node().value().as_deref() == Some(value.as_str()))
        .unwrap_or_else(|| panic!("no field with {value}"))
}

fn button_enabled(h: &Harness<'_, FerrumApp>, label: &str) -> bool {
    !h.get_by_label(label).accesskit_node().is_disabled()
}

fn bare_remote(p: &Path) -> tempfile::TempDir {
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    let url = remote.path().to_string_lossy().into_owned();
    git(p, &["remote", "add", "origin", &url]);
    remote
}

#[test]
fn quick_checkout_and_new_branch_with_prefix() {
    let dir = branches_repo();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);

    // Ctrl+B: search and Enter checks out the best match.
    h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    frames(&mut h);
    type_into(&mut h, "Search branches", "beta");
    h.key_press(Key::Enter);
    settle(&mut h);
    assert_eq!(current_branch(p), "feature/beta");

    // Ctrl+Shift+B: Tab completes the folder, the name is checked.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::B);
    frames(&mut h);
    type_into(&mut h, "feature/my-change", "fe");
    h.get_by_label("📁 feature/");
    h.key_press(Key::Tab);
    frames(&mut h);
    input(&h, "feature/").type_text("alpha");
    frames(&mut h);
    h.get_by_label("A branch with this name already exists.");
    assert!(!button_enabled(&h, "Create branch"));
    input(&h, "feature/alpha").type_text(" two");
    frames(&mut h);
    h.get_by_label("Create branch").click();
    settle(&mut h);
    assert_eq!(current_branch(p), "feature/alpha-two");
    assert_eq!(h.state().settings.refs.last_branch_prefix, "feature/");

    // The prefix is filled in next time.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::B);
    frames(&mut h);
    input(&h, "feature/");
}

#[test]
fn rename_on_remote_with_f2_and_delete_several() {
    let dir = branches_repo();
    let p = dir.path();
    let remote = bare_remote(p);
    git(p, &["push", "-q", "-u", "origin", "main", "solo"]);
    let wt = tempfile::tempdir().unwrap();
    let wt_path = wt.path().join("wt");
    git(
        p,
        &[
            "worktree",
            "add",
            "-q",
            &wt_path.to_string_lossy(),
            "feature/beta",
        ],
    );
    let mut h = harness(p);
    frames(&mut h);

    // F2 renames the branch under the sidebar cursor, here also on origin.
    row(&h, "solo").click();
    frames(&mut h);
    h.key_press(Key::F2);
    frames(&mut h);
    h.get_by_label("Rename solo");
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    input(&h, "solo").type_text("duo");
    frames(&mut h);
    h.get_by_label("Also rename solo on origin").click();
    frames(&mut h);
    h.get_by_label("Rename").click();
    settle(&mut h);
    let heads = git(remote.path(), &["branch", "--format=%(refname:short)"]);
    assert_eq!(heads.lines().collect::<Vec<_>>(), ["duo", "main"]);
    assert_eq!(
        git(p, &["rev-parse", "--abbrev-ref", "duo@{upstream}"]),
        "origin/duo"
    );

    // Ctrl+click two branches and delete them, with the remote branch and
    // the worktree that has one checked out.
    for name in ["duo", "beta"] {
        row(&h, name).click_modifiers(Modifiers::COMMAND);
        frames(&mut h);
    }
    h.get_by_label("2 selected");
    h.get_by_label("Delete…").click();
    frames(&mut h);
    h.get_by_label("Delete 2 branches");
    h.get_by_label_contains("☁ origin/duo");
    h.get_by_label("Also delete their remote branches (1)")
        .click();
    h.get_by_label("Also remove the worktrees that have them checked out (1)")
        .click();
    frames(&mut h);
    h.get_by_label("Delete").click();
    settle(&mut h);
    assert_eq!(git(p, &["branch", "--list", "duo", "feature/beta"]), "");
    let heads = git(remote.path(), &["branch", "--format=%(refname:short)"]);
    assert_eq!(heads, "main");
    assert!(!wt_path.exists());
}

#[test]
fn tag_with_message_and_details() {
    let dir = branches_repo();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);
    h.get_by_label_contains("Tags (0)").click_secondary();
    frames(&mut h);
    h.get_by_label("New tag…").click();
    frames(&mut h);
    type_into(&mut h, "v1.0.0", "v2");
    let message = h
        .get_all_by_role(Role::MultilineTextInput)
        .next()
        .expect("message field");
    message.focus();
    frames(&mut h);
    h.get_all_by_role(Role::MultilineTextInput)
        .next()
        .unwrap()
        .type_text("Release two\n\nWith notes");
    frames(&mut h);
    h.get_by_label("Create tag").click();
    settle(&mut h);
    let msg = git(p, &["tag", "-l", "--format=%(contents)", "v2"]);
    assert_eq!(msg.trim(), "Release two\n\nWith notes");

    // Details from the tag's menu.
    h.get_by_label_contains("Tags (1)").click();
    frames(&mut h);
    h.get_by_label("🏷 v2").click_secondary();
    frames(&mut h);
    h.get_by_label("Details…").click();
    frames(&mut h);
    h.get_by_label("Tagger");
    h.get_by_label("Annotated");
    h.get_by_label("Test <test@example.com>");
    h.get_by_label("Close").click();
    frames(&mut h);
}

#[test]
fn partial_stash_snapshot_rename_and_drop() {
    let dir = branches_repo();
    let p = dir.path();
    std::fs::write(p.join("a.txt"), "one\nchanged\n").unwrap();
    std::fs::write(p.join("b.txt"), "new file\n").unwrap();
    let mut h = harness(p);
    frames(&mut h);

    // Stash only b.txt.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    frames(&mut h);
    h.get_by_label("Only some files").click();
    frames(&mut h);
    h.get_by_label("a.txt").click();
    frames(&mut h);
    h.get_by_label("1 of 2 files");
    h.get_by_label("Stash").click();
    settle(&mut h);
    assert!(!p.join("b.txt").exists());
    assert_eq!(
        std::fs::read_to_string(p.join("a.txt")).unwrap(),
        "one\nchanged\n"
    );

    // A snapshot from the toolbar dropdown keeps the working copy.
    h.get_by_label("⏷").click();
    frames(&mut h);
    h.get_by_label("Save a snapshot").click();
    settle(&mut h);
    assert_eq!(h.state().tabs[0].stashes.len(), 2);
    assert_eq!(
        std::fs::read_to_string(p.join("a.txt")).unwrap(),
        "one\nchanged\n"
    );

    // Rename the older stash.
    let older = h.state().tabs[0].stashes[1].clone();
    let label = format!("📦 {}", older.message);
    h.get_by_label(&label).click_secondary();
    frames(&mut h);
    h.get_by_label("Rename…").click();
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_all_by_role(Role::TextInput)
        .find(|n| n.accesskit_node().value().as_deref() == Some(older.message.as_str()))
        .expect("message field")
        .type_text("only b");
    frames(&mut h);
    h.get_by_label("Rename").click();
    settle(&mut h);
    let stashes = &h.state().tabs[0].stashes;
    assert_eq!(stashes[0].message, "only b");
    assert_eq!(stashes[0].id, older.id);

    // Drop both at once.
    h.get_by_label_contains("Stashes (2)").click_secondary();
    frames(&mut h);
    h.get_by_label("Drop several stashes…").click();
    frames(&mut h);
    h.get_by_label("All").click();
    frames(&mut h);
    h.get_by_label("Drop 2 stashes").click();
    settle(&mut h);
    assert!(h.state().tabs[0].stashes.is_empty());
}
