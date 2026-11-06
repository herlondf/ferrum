//! UI tests of custom commands, the command palette and customizable shortcuts.

use std::path::Path;

use eframe::egui::accesskit::Role;
use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::app::FerrumApp;
use crate::command::Command;
use crate::custom_commands::{CustomCommand, FieldKind, FormField, Invocation, Scope, Target};
use crate::ui_tests::{frames, git, harness, repo, settle};

fn handle(h: &mut Harness<'_, FerrumApp>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(c, &ctx);
    frames(h);
}

/// Waits until `file` exists in the repository and returns its content.
fn wait_file(h: &mut Harness<'_, FerrumApp>, p: &Path, file: &str) -> String {
    for _ in 0..500 {
        h.step();
        if let Ok(s) = std::fs::read_to_string(p.join(file))
            && !s.is_empty()
            && !h.state().is_busy()
        {
            return s.trim().to_owned();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("{file} was not written");
}

fn shell(name: &str, scope: Scope, command: &str) -> CustomCommand {
    CustomCommand {
        name: name.into(),
        scope,
        command: command.into(),
        ..Default::default()
    }
}

fn sidebar_row<'a>(h: &'a Harness<'_, FerrumApp>, name: &str) -> egui_kittest::Node<'a> {
    h.get_all_by_role(Role::Button)
        .find(|n| {
            let label = n.accesskit_node().label().unwrap_or_default();
            label.trim_start_matches("✔ ").split_whitespace().next() == Some(name)
        })
        .unwrap_or_else(|| panic!("no sidebar row {name}"))
}

fn text_field<'a>(h: &'a Harness<'_, FerrumApp>, placeholder: &str) -> egui_kittest::Node<'a> {
    let placeholder = placeholder.to_owned();
    h.get_all_by_role(Role::TextInput)
        .find(|n| n.accesskit_node().placeholder() == Some(placeholder.as_str()))
        .unwrap_or_else(|| panic!("no field {placeholder}"))
}

#[test]
fn branch_menu_runs_a_nested_custom_command() {
    let dir = repo();
    let p = dir.path();
    git(p, &["branch", "feature"]);
    let mut h = harness(p);
    h.state_mut().settings.custom.commands = vec![shell(
        "Tools/Write branch",
        Scope::Branch,
        "echo $branch $SHORT_SHA > out.txt",
    )];
    frames(&mut h);
    sidebar_row(&h, "feature").click_secondary();
    frames(&mut h);
    h.get_by_label("Custom commands").click();
    frames(&mut h);
    h.get_by_label("Tools").click();
    frames(&mut h);
    h.get_by_label("Write branch").click();
    let out = wait_file(&mut h, p, "out.txt");
    let short = git(p, &["rev-parse", "--short=7", "feature"]);
    assert_eq!(out, format!("feature {short}"));
    // Recorded in the activity log with its command line.
    let op = crate::jobs::operations()
        .into_iter()
        .rev()
        .find(|o| o.label == "Write branch")
        .expect("in the activity log");
    assert!(op.commands()[0].command.starts_with("echo feature"));
}

#[test]
fn form_values_become_variables() {
    let dir = repo();
    let p = dir.path().to_path_buf();
    let mut h = harness(&p);
    frames(&mut h);
    let mut cmd = shell("Say", Scope::Repository, "echo $word $loud > said.txt");
    cmd.form = vec![
        FormField {
            name: "word".into(),
            label: "Word".into(),
            ..Default::default()
        },
        FormField {
            name: "loud".into(),
            label: "Loud".into(),
            kind: FieldKind::Checkbox,
            on: "LOUD".into(),
            off: "quiet".into(),
            ..Default::default()
        },
    ];
    let target = Target::repository(&h.state().tabs[0]);
    handle(
        &mut h,
        Command::Custom(Invocation {
            cmd,
            shared: false,
            target,
        }),
    );
    let field = h
        .get_all_by_role(Role::TextInput)
        .last()
        .expect("the form's text field");
    field.focus();
    frames(&mut h);
    h.get_all_by_role(Role::TextInput)
        .last()
        .unwrap()
        .type_text("hello");
    frames(&mut h);
    h.get_by_label("Loud").click();
    frames(&mut h);
    h.get_by_label("Run").click();
    assert_eq!(wait_file(&mut h, &p, "said.txt"), "hello LOUD");
}

#[test]
fn shared_commands_ask_once_before_running() {
    let dir = repo();
    let p = dir.path().to_path_buf();
    std::fs::create_dir_all(p.join(".ferrum")).unwrap();
    std::fs::write(
        p.join(crate::custom_commands::SHARED_FILE),
        r#"{"commands": [{"name": "Team/Touch", "command": "echo team > team.txt"}]}"#,
    )
    .unwrap();
    let mut h = harness(&p);
    frames(&mut h);
    // Listed in the palette.
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    frames(&mut h);
    text_field(&h, "Type a command, branch or file…").type_text("touch");
    frames(&mut h);
    h.get_by_label_contains("Team › Touch").click();
    frames(&mut h);
    // First run: the command is shown for approval.
    h.get_by_label_contains("Run it only if you trust it");
    h.get_by_label("echo team > team.txt");
    h.get_by_label("Run").click();
    assert_eq!(wait_file(&mut h, &p, "team.txt"), "team");
    assert_eq!(h.state().settings.custom.approved.len(), 1);

    // Approved: runs right away the next time.
    std::fs::remove_file(p.join("team.txt")).unwrap();
    let cmd = crate::custom_commands::shared(&p).unwrap().remove(0);
    let target = Target::repository(&h.state().tabs[0]);
    handle(
        &mut h,
        Command::Custom(Invocation {
            cmd,
            shared: true,
            target,
        }),
    );
    assert!(h.query_by_label("Run").is_none());
    assert_eq!(wait_file(&mut h, &p, "team.txt"), "team");
}

#[test]
fn palette_finds_files_and_opens_their_history() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    frames(&mut h);
    // The tracked files load in the background.
    for _ in 0..100 {
        h.step();
        if !crate::ui::file_finder::cached(&h.state().tabs[0]).is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    text_field(&h, "Type a command, branch or file…").type_text("a.txt");
    frames(&mut h);
    h.get_by_label("📄 a.txt").click();
    frames(&mut h);
    assert_eq!(h.state().tabs[0].view, crate::tab::View::File);
}

#[test]
fn rebound_shortcut_replaces_the_default() {
    let dir = repo();
    let mut h = harness(dir.path());
    crate::keymap::set_keys(
        &mut h.state_mut().settings.keymap,
        crate::keymap::Action::NewTag,
        &[crate::keymap::Combo::parse("Mod+Alt+N").unwrap()],
    );
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
    frames(&mut h);
    assert!(h.query_by_label("New tag").is_none());
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::N);
    frames(&mut h);
    h.get_by_label("New tag");
}

#[test]
fn new_tag_and_delete_shortcuts() {
    let dir = repo();
    let p = dir.path();
    git(p, &["tag", "v1"]);
    let mut h = harness(p);
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
    frames(&mut h);
    h.get_by_label("New tag");
    h.key_press(Key::Escape);
    frames(&mut h);

    // Delete asks before removing the tag under the sidebar cursor.
    h.get_by_label_contains("Tags (1)").click();
    frames(&mut h);
    h.get_by_label("🏷 v1").click();
    frames(&mut h);
    assert_eq!(h.state().tabs[0].pane.pane, crate::ui::focus::Pane::Sidebar);
    h.key_press(Key::Delete);
    frames(&mut h);
    h.get_by_label("Delete").click();
    settle(&mut h);
    assert_eq!(git(p, &["tag"]), "");
}

#[test]
fn shortcuts_editor_records_a_key() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    handle(
        &mut h,
        Command::DevStep(crate::command::DevStep::SettingsSection("shortcuts".into())),
    );
    // The row of "Fetch all remotes": click its key, then press the new one.
    let fetch = crate::keymap::Combo::parse("Mod+Shift+F").unwrap().text();
    // Filter the list so the row is visible.
    text_field(&h, "Filter…").focus();
    frames(&mut h);
    text_field(&h, "Filter…").type_text("fetch all");
    frames(&mut h);
    h.get_by_label(&fetch).click();
    frames(&mut h);
    assert!(crate::keymap::capturing());
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::F);
    frames(&mut h);
    assert!(!crate::keymap::capturing());
    let keys = h.state().settings.keymap.get("fetch").cloned();
    assert_eq!(keys, Some(vec!["Mod+Alt+F".to_owned()]));
    // Reset all goes back to the defaults.
    h.get_by_label("Reset all").click();
    frames(&mut h);
    assert!(h.state().settings.keymap.is_empty());
}
