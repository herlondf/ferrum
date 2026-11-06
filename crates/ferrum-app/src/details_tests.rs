//! UI tests of the commit details: tabs, file tree, refs containing a commit,
//! the large popup, copying, comparing commits, branches and stashes.

use std::time::Duration;

use eframe::egui::{Key, Modifiers, OutputCommand};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use ferrum_core::browse::Rev;

use crate::app::FerrumApp;
use crate::command::Command;
use crate::ui::compare;
use crate::ui::details_model::DetailsTab;
use crate::ui_tests::{frames, git, harness, repo};

/// first (a.txt) - second (src/lib.rs, tag v1) on main; feature: third.
fn history_repo() -> tempfile::TempDir {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    std::fs::create_dir_all(p.join("src")).unwrap();
    std::fs::write(
        p.join("src/lib.rs"),
        "pub fn answer() -> u32 {\n    42\n}\n",
    )
    .unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "second"]);
    git(p, &["tag", "v1"]);
    git(p, &["switch", "-qc", "feature"]);
    std::fs::write(p.join("a.txt"), "one\nthree\n").unwrap();
    git(p, &["commit", "-qam", "third"]);
    git(p, &["switch", "-q", "main"]);
    dir
}

fn select(h: &mut Harness<'_, FerrumApp>, rev: &str) -> ferrum_core::Oid {
    let id = h.state().tabs[0].repo.resolve(rev).unwrap();
    let ctx = h.ctx.clone();
    h.state_mut().handle(Command::SelectCommit(id), &ctx);
    frames(h);
    id
}

/// Presses a key for one frame (the output of that frame stays readable).
fn press(h: &mut Harness<'_, FerrumApp>, modifiers: Modifiers, key: Key) {
    h.event(eframe::egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.step();
    let up = eframe::egui::Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    };
    // Released on the next frame.
    h.event(up);
}

fn link<'a>(h: &'a Harness<'_, FerrumApp>, text: &'a str) -> egui_kittest::Node<'a> {
    use egui_kittest::kittest::NodeT;
    h.get_all_by_label(text)
        .find(|n| n.accesskit_node().role() == eframe::egui::accesskit::Role::Link)
        .expect("link")
}

fn copied(h: &Harness<'_, FerrumApp>) -> Option<String> {
    h.output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
}

#[test]
fn tabs_file_tree_and_containing_refs() {
    let dir = history_repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    select(&mut h, "main");
    // Author and committer, and the refs that contain the commit.
    assert!(h.query_by_label("Committer").is_some());
    let mut found = false;
    for _ in 0..200 {
        h.step();
        if h.query_by_label("v1").is_some() {
            found = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(found, "tags containing the commit were not shown");
    assert!(h.query_by_label("main").is_some());

    // Files as a tree.
    h.get_by_label("Tree").click();
    frames(&mut h);
    assert!(h.state().settings.details.files_as_tree);
    assert!(h.query_by_label_contains("📁 src").is_some());
    let rows = |h: &Harness<'_, FerrumApp>| h.query_all_by_label_contains("lib.rs").count();
    let before = rows(&h);
    h.get_by_label_contains("📁 src").click();
    frames(&mut h);
    assert_eq!(rows(&h), before - 1);
    h.get_by_label("Expand all").click();
    frames(&mut h);
    assert_eq!(rows(&h), before);

    // The File Tree tab browses the commit and is remembered.
    h.get_by_label("File Tree").click();
    frames(&mut h);
    assert_eq!(h.state().settings.details.tab, DetailsTab::FileTree);
    h.get_by_label_contains("📁 src").click();
    frames(&mut h);
    h.get_by_label("📄 lib.rs").click();
    frames(&mut h);
    assert!(h.query_by_label_contains("pub fn answer()").is_some());
    assert!(h.query_by_label("3 lines").is_some());
    // The first commit has no src folder.
    select(&mut h, "main~1");
    assert!(
        h.query_by_label_contains("does not exist in this commit")
            .is_some()
    );
}

#[test]
fn space_opens_the_popup_and_shortcuts_copy() {
    let dir = history_repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    let id = select(&mut h, "main");
    h.key_press(Key::Space);
    frames(&mut h);
    assert!(h.state().tabs[0].details.popup);
    assert!(
        h.query_by_label("Space or Esc closes this window")
            .is_some()
    );
    h.key_press(Key::Space);
    frames(&mut h);
    assert!(!h.state().tabs[0].details.popup);
    // With the details focused, Space opens the large diff instead.
    h.state_mut().tabs[0].pane.pane = crate::ui::focus::Pane::Details;
    h.key_press(Key::Space);
    frames(&mut h);
    assert!(!h.state().tabs[0].details.popup);
    assert!(crate::ui::diff_view::large_open(&h.ctx, "commit-diff"));
    h.key_press(Key::Space);
    frames(&mut h);
    h.state_mut().tabs[0].pane.pane = crate::ui::focus::Pane::Commits;

    press(&mut h, Modifiers::COMMAND, Key::C);
    assert_eq!(copied(&h), Some(id.to_string()));
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
    let info = copied(&h).unwrap();
    assert!(info.starts_with(&format!("Commit: {id}")), "{info}");
    assert!(info.contains("Committer: Test <test@example.com>"));

    // Ctrl+Alt+D hides and shows the pane.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::D);
    frames(&mut h);
    assert!(h.state().settings.details.collapsed);
    assert!(h.query_by_label("File Tree").is_none());
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::D);
    frames(&mut h);
    assert!(h.query_by_label("File Tree").is_some());
}

#[test]
fn commit_ids_in_messages_preview_and_select() {
    let dir = history_repo();
    let p = dir.path();
    let first = git(p, &["rev-parse", "--short", "main~1"]);
    std::fs::write(p.join("a.txt"), "four\n").unwrap();
    let msg = format!("fourth\n\nUndoes part of {first}.");
    git(p, &["commit", "-qam", &msg]);
    let mut h = harness(p);
    frames(&mut h);
    select(&mut h, "main");
    link(&h, &first).hover();
    frames(&mut h);
    assert!(h.query_by_label("Click to select it").is_some());
    assert!(h.query_by_label("first").is_some());
    link(&h, &first).click();
    frames(&mut h);
    let id = h.state().tabs[0].repo.resolve(&first).unwrap();
    assert_eq!(h.state().tabs[0].history.selected, Some(id));
}

#[test]
fn compares_two_commits_and_swaps() {
    let dir = history_repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    let second = select(&mut h, "main");
    let third = h.state().tabs[0].repo.resolve("feature").unwrap();
    // Ctrl+click on a second commit.
    compare::pick_commit(&mut h.state_mut().tabs[0], third);
    frames(&mut h);
    let picked = h.state().tabs[0].details.picked.clone();
    assert_eq!(picked, [second, third]);
    let view = h.state().tabs[0].details.compare.as_ref().unwrap();
    assert_eq!(
        (view.from.rev, view.to.rev),
        (Rev::Commit(second), Rev::Commit(third))
    );
    // Only a.txt differs between second and third.
    let paths: Vec<&str> = view.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a.txt"]);
    assert!(h.query_by_label_contains("Compare").is_some());
    h.get_by_label("🔃 Swap").click();
    frames(&mut h);
    let view = h.state().tabs[0].details.compare.as_ref().unwrap();
    assert_eq!(view.from.rev, Rev::Commit(third));
    h.get_by_label("Close").click();
    frames(&mut h);
    assert!(h.state().tabs[0].details.compare.is_none());
    assert!(h.query_by_label("File Tree").is_some());
}

#[test]
fn compares_branches_and_a_stash_with_the_working_copy() {
    let dir = history_repo();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);
    // Two branches picked in the sidebar.
    {
        let tab = &mut h.state_mut().tabs[0];
        tab.sidebar.selection = ["feature".to_owned(), "main".to_owned()].into();
    }
    frames(&mut h);
    h.get_by_label("Compare").click();
    frames(&mut h);
    let view = h.state().tabs[0].details.compare.as_ref().unwrap();
    assert_eq!(view.files.len(), 1);
    assert_eq!(view.files[0].path, "a.txt");

    // A stash against the working copy.
    std::fs::write(p.join("a.txt"), "stashed\n").unwrap();
    git(p, &["stash", "-q"]);
    std::fs::write(p.join("b.txt"), "new\n").unwrap();
    git(p, &["add", "b.txt"]);
    let stash = h.state().tabs[0].repo.resolve("stash@{0}").unwrap();
    h.state_mut().tabs[0].stashes = h.state().tabs[0].repo.stashes().unwrap();
    let ctx = h.ctx.clone();
    h.state_mut().handle(Command::ShowCommit(stash), &ctx);
    frames(&mut h);
    assert!(h.state().tabs[0].details.compare.is_none());
    h.get_by_label("Compare with working copy").click();
    frames(&mut h);
    let view = h.state().tabs[0].details.compare.as_ref().unwrap();
    assert_eq!(view.to.rev, Rev::WorkDir);
    assert_eq!(view.from.label, "stash@{0}");
    let mut paths: Vec<&str> = view.files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, ["a.txt", "b.txt"]);
}

/// Uses a diff tool that writes both files, one after the other, to a file;
/// `act` starts it. Returns what it wrote.
#[cfg(unix)]
fn diff_tool_output(
    path: &std::path::Path,
    act: impl FnOnce(&mut Harness<'_, FerrumApp>),
) -> String {
    let out = tempfile::tempdir().unwrap();
    let marker = out.path().join("diff.txt");
    let mut h = harness(path);
    h.state_mut().settings.external = crate::external::ExternalTools {
        diff_tool: crate::external::DiffTool::Custom,
        diff_command: format!(
            "sh -c 'cat \"$1\" \"$2\" > {}' sh $LOCAL $REMOTE",
            marker.display()
        ),
        ..Default::default()
    };
    frames(&mut h);
    act(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    frames(&mut h);
    for _ in 0..200 {
        if let Ok(s) = std::fs::read_to_string(&marker)
            && !s.is_empty()
        {
            return s;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the diff tool was not started");
}

#[cfg(unix)]
#[test]
fn external_diff_of_a_commit_file_and_a_local_change() {
    let dir = history_repo();
    let s = diff_tool_output(dir.path(), |h| {
        select(h, "feature");
    });
    assert_eq!(s, "one\none\nthree\n");

    let dir = repo();
    let s = diff_tool_output(dir.path(), |h| {
        h.get_by_label_contains("Local Changes").click();
        frames(h);
    });
    // The staged version, then the file on disk.
    assert_eq!(s, "one\none\ntwo\n");
}
