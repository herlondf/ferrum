//! UI tests of blame and file history: Go to line, back/forward, line
//! history, comparing revisions, reset with undo, the file finder and
//! separate windows.

use std::path::Path;
use std::time::Duration;

use eframe::egui::{self, Event, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::FerrumApp;
use crate::command::{Command, Op};
use crate::tab::View;
use crate::ui::blame::{self, FileView, Mode};
use crate::ui::blame_view::Pos;
use crate::ui_tests::{frames, git, harness, repo, settle};

/// `f.txt` with 40 lines in three commits: "first", "second" (line 5) and
/// "third" (line 20).
fn history_repo() -> tempfile::TempDir {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    let mut lines: Vec<String> = (1..=40).map(|i| format!("line {i}")).collect();
    for (msg, change) in [("first", None), ("second", Some(4)), ("third", Some(19))] {
        if let Some(i) = change {
            lines[i] = format!("changed {}", i + 1);
        }
        std::fs::write(p.join("f.txt"), lines.join("\n") + "\n").unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-qm", msg]);
    }
    dir
}

fn view<'a>(h: &'a Harness<'_, FerrumApp>) -> &'a FileView {
    h.state().tabs[0].file_view.as_ref().expect("file view")
}

/// Steps frames until `done` holds (background loads).
fn wait(h: &mut Harness<'_, FerrumApp>, done: impl Fn(&FerrumApp) -> bool) {
    for _ in 0..500 {
        h.step();
        if done(h.state()) {
            frames(h);
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out");
}

fn blame_loaded(app: &FerrumApp) -> bool {
    let v = app.tabs[0].file_view.as_ref();
    v.is_some_and(|v| v.blame.line_count().is_some())
}

fn rev(p: &Path, spec: &str) -> ferrum_core::Oid {
    ferrum_core::Oid::from_str(&git(p, &["rev-parse", spec])).unwrap()
}

#[test]
fn blame_go_to_line_navigation_and_line_history() {
    let dir = history_repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    blame::open(&mut h.state_mut().tabs[0], "f.txt".into(), true);
    wait(&mut h, blame_loaded);
    assert_eq!(view(&h).blame.line_count(), Some(40));

    // Ctrl+G over the blame is Go to line, not "go to commit".
    h.hover_at(egui::pos2(800.0, 500.0));
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    frames(&mut h);
    assert!(view(&h).blame.goto.is_some());
    assert!(h.state().tabs[0].history.ext.goto.is_none());
    h.event(Event::Text("30".into()));
    h.key_press(Key::Enter);
    frames(&mut h);
    assert!(view(&h).blame.goto.is_none());
    assert_eq!(view(&h).blame.selected_lines(), Some((30, 30)));

    // Blame at an older revision, then back and forward.
    let first = rev(dir.path(), "HEAD~2");
    h.state_mut().tabs[0]
        .file_view
        .as_mut()
        .unwrap()
        .blame
        .navigate(Pos {
            rev: Some(first),
            path: "f.txt".into(),
            line: 0,
        });
    wait(&mut h, blame_loaded);
    assert_eq!(view(&h).blame.at.rev, Some(first));
    h.get_by_label("⬅").click();
    frames(&mut h);
    assert_eq!(view(&h).blame.at.rev, None);
    h.get_by_label("➡").click();
    frames(&mut h);
    assert_eq!(view(&h).blame.at.rev, Some(first));
    h.get_by_label("⬅").click();
    wait(&mut h, blame_loaded);

    // History of the selected line (changed in "third").
    h.state_mut().tabs[0]
        .file_view
        .as_mut()
        .unwrap()
        .blame
        .go_to_line(20);
    frames(&mut h);
    h.get_by_label("History of line 20").click();
    wait(&mut h, |a| {
        let v = a.tabs[0].file_view.as_ref().unwrap();
        v.lines.as_ref().is_some_and(|l| l.count().is_some())
    });
    let v = view(&h);
    assert_eq!(v.mode, Mode::Lines);
    let lines = v.lines.as_ref().unwrap();
    assert_eq!(
        (lines.start, lines.end, lines.rev.as_str()),
        (20, 20, "HEAD")
    );
    // "third" changed it, "first" added it.
    assert_eq!(lines.count(), Some(2));
}

#[test]
fn compare_revisions_and_reset_file_with_undo() {
    let dir = history_repo();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);
    blame::open(&mut h.state_mut().tabs[0], "f.txt".into(), false);
    wait(&mut h, |a| {
        a.tabs[0]
            .file_view
            .as_ref()
            .is_some_and(|v| v.diff_key.is_some())
    });
    let (first, third) = (rev(p, "HEAD~2"), rev(p, "HEAD"));
    let key = view(&h).diff_key.clone().unwrap();
    assert_eq!((key.old, key.new), (Some(rev(p, "HEAD~1")), third));

    // Ctrl+click picked "first": compare it with "third".
    h.state_mut().tabs[0].file_view.as_mut().unwrap().other = Some(2);
    frames(&mut h);
    let key = view(&h).diff_key.clone().unwrap();
    assert_eq!((key.old, key.new), (Some(first), third));
    h.get_by_label_contains("Comparing");

    // Reset the file to "first", then undo.
    let ctx = h.ctx.clone();
    h.state_mut().tabs[0].run(
        Op::ResetFileTo {
            path: "f.txt".into(),
            rev: first.to_string(),
        },
        &ctx,
    );
    settle(&mut h);
    let content = std::fs::read_to_string(p.join("f.txt")).unwrap();
    assert!(content.contains("line 5\n") && content.contains("line 20\n"));
    h.state_mut().handle(Command::Undo, &ctx);
    settle(&mut h);
    let content = std::fs::read_to_string(p.join("f.txt")).unwrap();
    assert!(content.contains("changed 5\n") && content.contains("changed 20\n"));
    assert_eq!(git(p, &["status", "--porcelain"]), "");
}

#[test]
fn file_history_from_the_palette() {
    let dir = history_repo();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/guide.md"), "g\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "docs"]);
    let mut h = harness(dir.path());
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    frames(&mut h);
    h.event(Event::Text("File history".into()));
    frames(&mut h);
    h.key_press(Key::Enter);
    frames(&mut h);
    assert!(h.state().tabs[0].file_tools.finder.is_some());
    h.event(Event::Text("guid".into()));
    wait(&mut h, |a| !a.tabs[0].file_tools.candidates.is_empty());
    h.key_press(Key::Enter);
    frames(&mut h);
    assert_eq!(h.state().tabs[0].view, View::File);
    assert_eq!(view(&h).path, "docs/guide.md");

    // Folders have a history too.
    let ctx = h.ctx.clone();
    h.state_mut()
        .handle(Command::FileHistory("docs/".into()), &ctx);
    wait(&mut h, |a| {
        let v = a.tabs[0].file_view.as_ref().unwrap();
        v.is_folder && v.diff_key.is_some()
    });
    assert_eq!(view(&h).path, "docs");
    assert_eq!(view(&h).diff_key.as_ref().unwrap().path, "docs/guide.md");
}

#[test]
fn line_history_from_a_commit_diff_and_separate_window() {
    let dir = history_repo();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);
    let ctx = h.ctx.clone();
    let head = rev(p, "HEAD");
    h.state_mut().handle(Command::SelectCommit(head), &ctx);
    frames(&mut h);
    // What the line menu of the commit diff asks for.
    crate::ui::file_lines::request(&ctx, "commit-diff", "f.txt", (20, 20), false);
    frames(&mut h);
    let v = view(&h);
    assert_eq!(h.state().tabs[0].view, View::File);
    assert_eq!(v.mode, Mode::Lines);
    assert_eq!(v.lines.as_ref().unwrap().rev, head.to_string());

    // A removed line is counted in the parent.
    crate::ui::file_lines::request(&ctx, "commit-diff-large", "f.txt", (20, 20), true);
    frames(&mut h);
    let rev = &view(&h).lines.as_ref().unwrap().rev;
    assert_eq!(rev, &format!("{head}^"));

    // Open in a window: the view moves out of the main area.
    h.get_by_label("Open in a window").click();
    frames(&mut h);
    let tab = &h.state().tabs[0];
    assert!(tab.file_view.is_none());
    assert_eq!(tab.file_tools.windows.len(), 1);
    assert_eq!(tab.view, View::History);
    h.get_by_label("Close").click();
    frames(&mut h);
    assert!(h.state().tabs[0].file_tools.windows.is_empty());
}
