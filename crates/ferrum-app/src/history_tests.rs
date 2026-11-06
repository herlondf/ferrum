//! UI tests of the history: collapsing merges, graph filters, reflog, path
//! search, go to commit and drag-and-drop rewriting.

use std::path::Path;

use eframe::egui::accesskit::Role;
use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;
use crate::ui::history::{self, Drop};
use crate::ui::history_model::SearchMode;
use crate::ui_tests::{frames, git, harness, repo, settle};

/// main: first - m1 - merge(feature: f1 - f2); side: first - s1.
fn merged_repo() -> tempfile::TempDir {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    git(p, &["switch", "-q", "-c", "feature"]);
    for (n, msg) in [("1", "f1"), ("2", "f2")] {
        std::fs::write(p.join("f.txt"), n).unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-qm", msg]);
    }
    git(p, &["switch", "-q", "-c", "side", "main"]);
    std::fs::write(p.join("s.txt"), "s").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "s1"]);
    git(p, &["switch", "-q", "main"]);
    std::fs::write(p.join("m.txt"), "m").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "m1"]);
    git(
        p,
        &["merge", "-q", "--no-ff", "-m", "merge feature", "feature"],
    );
    dir
}

fn summaries(h: &egui_kittest::Harness<'_, crate::app::FerrumApp>) -> Vec<String> {
    h.state().tabs[0]
        .commits
        .iter()
        .map(|c| c.summary.clone())
        .collect()
}

#[test]
fn collapse_merges_with_keys_and_view_menu() {
    let dir = merged_repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    assert_eq!(h.state().tabs[0].commits.len(), 6);
    h.key_press(Key::ArrowDown);
    frames(&mut h);
    let head = h.state().tabs[0].head_oid;
    assert_eq!(h.state().tabs[0].history.selected, head);
    h.key_press(Key::ArrowLeft);
    frames(&mut h);
    let list = summaries(&h);
    assert!(!list.contains(&"f1".to_owned()), "{list:?}");
    assert_eq!(list.len(), 4);
    assert_eq!(
        h.state().tabs[0].history.ext.hidden_under[&head.unwrap()],
        2
    );
    h.key_press(Key::ArrowRight);
    frames(&mut h);
    assert_eq!(summaries(&h).len(), 6);

    h.get_by_label("View").click();
    frames(&mut h);
    h.get_by_label("Collapse all merges").click();
    frames(&mut h);
    assert_eq!(summaries(&h).len(), 4);
    // Remembered with the repository's settings.
    let path = h.state().tabs[0].path().to_path_buf();
    assert!(h.state().settings.history.prefs_for(&path).collapse_all);
}

#[test]
fn sidebar_filters_and_active_branch_shortcut() {
    let dir = merged_repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label("feature").click_secondary();
    frames(&mut h);
    h.get_by_label("Show only this in the graph").click();
    frames(&mut h);
    assert_eq!(summaries(&h), ["f2", "f1", "first"]);
    // The status bar names the filter.
    h.get_by_label_contains("Only feature");

    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::A);
    frames(&mut h);
    assert_eq!(summaries(&h).len(), 5, "main only");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::A);
    frames(&mut h);
    assert_eq!(summaries(&h).len(), 6, "filter off again");

    h.get_by_label("side").click_secondary();
    frames(&mut h);
    h.get_by_label("Hide from the graph").click();
    frames(&mut h);
    assert!(!summaries(&h).contains(&"s1".to_owned()));
    h.get_by_label_contains("Hidden: side");
}

#[test]
fn reflog_toggle_shows_lost_commits() {
    let dir = repo();
    let p = dir.path();
    git(p, &["commit", "-qam", "lost"]);
    git(p, &["reset", "-q", "--hard", "HEAD~1"]);
    let mut h = harness(p);
    frames(&mut h);
    assert_eq!(summaries(&h), ["first"]);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Period);
    frames(&mut h);
    assert_eq!(summaries(&h), ["lost", "first"]);
    let tab = &h.state().tabs[0];
    assert!(tab.history.ext.unreachable.contains(&tab.commits[0].id));
}

#[test]
fn path_search_lists_matching_commits_and_files() {
    let dir = merged_repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.state_mut().tabs[0].history.ext.mode = SearchMode::Paths;
    frames(&mut h);
    let field = h.get_by_role(Role::TextInput);
    field.focus();
    frames(&mut h);
    h.get_by_role(Role::TextInput).type_text("f.TXT");
    frames(&mut h);
    h.key_press(Key::Enter);
    settle(&mut h);
    let tab = &h.state().tabs[0];
    let visible: Vec<&str> = tab
        .history
        .visible
        .as_ref()
        .unwrap()
        .iter()
        .map(|&i| tab.commits[i].summary.as_str())
        .collect();
    assert_eq!(visible, ["f2", "f1"]);
    assert_eq!(
        tab.history.ext.path_hits[&tab.commits[tab.history.visible.as_ref().unwrap()[0]].id],
        ["f.txt"]
    );
    assert_eq!(h.state().settings.history.recent_searches, ["f.TXT"]);
}

#[test]
fn go_to_commit_by_name() {
    let dir = merged_repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    frames(&mut h);
    // The box has the focus (the search field is also a text input).
    h.get_all_by_role(Role::TextInput)
        .find(|n| n.is_focused())
        .expect("focused field")
        .type_text("side");
    frames(&mut h);
    h.get_by_label("Go").click();
    frames(&mut h);
    let side = git(dir.path(), &["rev-parse", "side"]);
    let tab = &h.state().tabs[0];
    assert_eq!(tab.history.selected.map(|s| s.to_string()), Some(side));
    assert!(tab.history.ext.goto.is_none());
}

fn run_drop(tab: &RepoTab, src: usize, target: impl Fn(ferrum_core::Oid) -> Drop, onto: usize) {
    let chain = history::rewritable_chain(tab);
    let d = history::drag_plan(tab, &chain, chain[src], target(chain[onto])).expect("a plan");
    let Dialog::Confirm { op, .. } = d else {
        panic!("a confirmation")
    };
    op.execute(&tab.git, &|_| {}).unwrap();
}

fn log(p: &Path) -> String {
    git(p, &["log", "--format=%s"]).replace('\n', " ")
}

#[test]
fn drag_and_drop_reorders_and_squashes() {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    for name in ["A", "B", "C"] {
        std::fs::write(p.join(format!("{name}.txt")), name).unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-qm", name]);
    }
    let settings = Default::default();
    let mut tab = RepoTab::open_with(p.to_path_buf(), &settings).unwrap();
    // Chain is newest first: C, B, A, first. Drop C below A.
    run_drop(&tab, 0, Drop::Below, 2);
    assert_eq!(log(p), "B A C first");
    tab.refresh_all();
    // Squash B (newest) into A.
    run_drop(&tab, 0, Drop::Squash, 1);
    assert_eq!(git(p, &["log", "--format=%s", "-1"]), "A");
    assert_eq!(git(p, &["rev-list", "--count", "HEAD"]), "3");
    assert!(p.join("B.txt").exists());
    tab.refresh_all();
    // Dropping a commit where it already is changes nothing.
    let chain = history::rewritable_chain(&tab);
    assert!(history::drag_plan(&tab, &chain, chain[0], Drop::Above(chain[1])).is_none());
}
