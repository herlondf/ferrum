//! End-to-end tests of the structural diff, the semantic merge suggestions
//! and the conflict editor.

use std::path::Path;
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use ferrum_core::DiffSource;

use crate::app::FerrumApp;
use crate::icons;
use crate::tab::View;
use crate::ui_tests::{frames, git, harness, repo, settle};

fn select(h: &mut Harness<'static, FerrumApp>, path: &str) {
    let tab = &mut h.state_mut().tabs[0];
    tab.view = View::Changes;
    tab.select_change(DiffSource::Unstaged, path.into());
    frames(h);
}

/// Steps frames until a label containing `text` shows (background work).
fn wait_for(h: &mut Harness<'static, FerrumApp>, text: &str) {
    for _ in 0..500 {
        h.step();
        if h.query_by_label_contains(text).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{text:?} did not show");
}

fn commit(dir: &Path, file: &str, text: &str, msg: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
}

#[test]
fn structural_toggle_ignores_formatting() {
    let dir = repo();
    let p = dir.path();
    commit(p, "lib.rs", "fn f(a: u32) -> u32 {\n    a + 1\n}\n", "f");
    std::fs::write(
        p.join("lib.rs"),
        "fn f(a: u32)\n    -> u32\n{\n        a + 2\n}\n",
    )
    .unwrap();
    let mut h = harness(p);
    select(&mut h, "lib.rs");

    // Remembered for this repository only.
    h.get_by_label(icons::TREE_STRUCTURE).click();
    wait_for(&mut h, "Structural · Rust");
    let key = h.state().tabs[0].path().to_string_lossy().into_owned();
    assert!(h.state().settings.diff.structural_repos.contains(&key));
    // Only the literal changed.
    assert!(h.query_by_label("1 removed").is_some());
    assert!(h.query_by_label("1 added").is_some());

    // A pure reformat has no syntax changes.
    std::fs::write(p.join("lib.rs"), "fn f(a: u32) -> u32\n{\n  a + 1\n}\n").unwrap();
    h.state_mut().tabs[0].reload_change_diff();
    select(&mut h, "lib.rs");
    wait_for(&mut h, "No syntax changes");

    // Files without a grammar keep the line diff, with no toggle.
    select(&mut h, "a.txt");
    assert!(h.query_by_label(icons::TREE_STRUCTURE).is_none());
    assert!(h.query_by_label("No syntax changes").is_none());

    // Settings can turn the feature off.
    select(&mut h, "lib.rs");
    h.state_mut().settings.diff.structural_diff = false;
    frames(&mut h);
    assert!(h.query_by_label(icons::TREE_STRUCTURE).is_none());
}

const BASE: &str = "pub struct Circle {\n    pub r: f64,\n}\n\nimpl Circle {\n    pub fn area(&self) -> f64 {\n        3.14 * self.r * self.r\n    }\n}\n";

/// A merge stopped on two files: shapes.rs, where both branches added a
/// method (the semantic merge resolves it), and config.rs, where both
/// changed the same function (a real conflict).
fn conflicted() -> tempfile::TempDir {
    let dir = repo();
    let p = dir.path();
    std::fs::write(p.join("shapes.rs"), BASE).unwrap();
    commit(
        p,
        "config.rs",
        "pub fn timeout() -> u64 {\n    30\n}\n",
        "base",
    );
    git(p, &["checkout", "-q", "-b", "feature"]);
    let add = |name: &str, body: &str| {
        BASE.replace(
            "    }\n}\n",
            &format!("    }}\n\n    pub fn {name}(&self) -> f64 {{\n        {body}\n    }}\n}}\n"),
        )
    };
    std::fs::write(p.join("shapes.rs"), add("diameter", "2.0 * self.r")).unwrap();
    commit(
        p,
        "config.rs",
        "pub fn timeout() -> u64 {\n    60\n}\n",
        "feature",
    );
    git(p, &["checkout", "-q", "main"]);
    std::fs::write(p.join("shapes.rs"), add("perimeter", "6.28 * self.r")).unwrap();
    commit(
        p,
        "config.rs",
        "pub fn timeout() -> u64 {\n    10\n}\n",
        "main",
    );
    let out = std::process::Command::new("git")
        .current_dir(p)
        .args(["merge", "feature"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    dir
}

#[test]
fn semantic_merge_is_offered_for_review_and_real_conflicts_are_left_alone() {
    let dir = conflicted();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);

    // The banner offers the file the semantic merge resolved.
    wait_for(&mut h, "1 file can be resolved automatically");
    h.get_by_label_contains("1 file can be resolved automatically")
        .click();
    frames(&mut h);
    assert_eq!(h.state().tabs[0].view, View::Conflicts);
    wait_for(&mut h, "Resolved automatically — review");
    assert!(
        h.query_by_label("impl Circle › pub fn diameter(&self) -> f64")
            .is_some()
    );
    // Nothing was written or staged yet.
    let shapes = std::fs::read_to_string(p.join("shapes.rs")).unwrap();
    assert!(shapes.contains("<<<<<<<"));
    assert!(git(p, &["ls-files", "-u"]).contains("shapes.rs"));

    h.get_by_label_contains("Use this result").click();
    frames(&mut h);
    h.get_by_label_contains("Mark as resolved").click();
    settle(&mut h);
    let shapes = std::fs::read_to_string(p.join("shapes.rs")).unwrap();
    assert!(!shapes.contains("<<<<<<<"), "{shapes}");
    assert!(shapes.contains("fn perimeter") && shapes.contains("fn diameter"));
    assert!(!git(p, &["ls-files", "-u"]).contains("shapes.rs"));

    // config.rs is a real conflict: explained, no suggestion.
    wait_for(&mut h, "The semantic merge left this file to you");
    assert!(
        h.query_by_label_contains("both sides changed pub fn timeout() -> u64")
            .is_some()
    );
    assert!(
        h.query_by_label("Resolved automatically — review")
            .is_none()
    );
    assert!(h.query_by_label("0 of 1 conflicts resolved").is_some());
    h.get_by_label("All feature").click();
    frames(&mut h);
    assert!(h.query_by_label("1 of 1 conflicts resolved").is_some());
}

#[test]
fn semantic_merge_can_be_turned_off() {
    let dir = conflicted();
    let mut h = harness(dir.path());
    h.state_mut().settings.diff.semantic_merge = false;
    frames(&mut h);
    h.state_mut().tabs[0].view = View::Conflicts;
    crate::ui::conflicts::open(&mut h.state_mut().tabs[0], "shapes.rs".into());
    for _ in 0..30 {
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        h.query_by_label("Resolved automatically — review")
            .is_none()
    );
    assert!(
        h.query_by_label_contains("can be resolved automatically")
            .is_none()
    );
}
