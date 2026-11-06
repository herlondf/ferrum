//! End-to-end tests that drive the real app with simulated clicks and typing.

use std::path::Path;
use std::process::Command as Proc;
use std::time::Duration;

use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::app::FerrumApp;

/// UI source files, for checks over every string the app shows.
pub const SOURCES: &[&str] = &[
    include_str!("app.rs"),
    include_str!("command.rs"),
    include_str!("tab.rs"),
    include_str!("util.rs"),
    include_str!("settings.rs"),
    include_str!("ui/blame.rs"),
    include_str!("ui/changes.rs"),
    include_str!("ui/conflicts.rs"),
    include_str!("ui/details.rs"),
    include_str!("ui/dialogs.rs"),
    include_str!("ui/diff_view.rs"),
    include_str!("ui/forge.rs"),
    include_str!("ui/history.rs"),
    include_str!("ui/image_diff.rs"),
    include_str!("ui/palette.rs"),
    include_str!("ui/rebase.rs"),
    include_str!("ui/settings.rs"),
    include_str!("ui/sidebar.rs"),
    include_str!("i18n/mod.rs"),
    include_str!("i18n/pt_br/mod.rs"),
    include_str!("i18n/pt_br/common.rs"),
    include_str!("i18n/pt_br/main.rs"),
    include_str!("i18n/pt_br/editing.rs"),
    include_str!("i18n/pt_br/extras.rs"),
];

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Proc::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A repository with one commit and a modified file.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.name", "Test"]);
    git(p, &["config", "user.email", "test@example.com"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    git(p, &["config", "core.autocrlf", "false"]);
    std::fs::write(p.join("a.txt"), "one\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", "first"]);
    std::fs::write(p.join("a.txt"), "one\ntwo\n").unwrap();
    dir
}

fn harness(path: &Path) -> Harness<'static, FerrumApp> {
    let path = path.to_path_buf();
    let mut h = Harness::builder()
        .with_size([1400.0, 900.0])
        .build_eframe(move |cc| FerrumApp::new(cc, vec![path]));
    // Tests look up English labels, whatever the machine's language.
    h.state_mut().settings.language = crate::i18n::Language::English;
    h.step();
    h
}

/// Runs a few frames so clicks are handled and the next screen is drawn.
fn frames(h: &mut Harness<'_, FerrumApp>) {
    for _ in 0..4 {
        h.step();
    }
}

/// Steps frames until no git operation is running.
fn settle(h: &mut Harness<'_, FerrumApp>) {
    for _ in 0..1000 {
        h.step();
        if !h.state().is_busy() {
            frames(h);
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("operation did not finish");
}

#[test]
fn stage_all_and_commit() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label_contains("Local Changes").click();
    frames(&mut h);
    h.get_by_label("Stage all").click();
    settle(&mut h);
    assert_eq!(
        git(dir.path(), &["diff", "--cached", "--name-only"]),
        "a.txt"
    );

    let message = h.get_by_role(Role::MultilineTextInput);
    message.focus();
    frames(&mut h);
    h.get_by_role(Role::MultilineTextInput)
        .type_text("Second commit");
    frames(&mut h);
    h.get_by_label("Commit 1 file").click();
    settle(&mut h);
    assert_eq!(
        git(dir.path(), &["log", "-1", "--format=%s"]),
        "Second commit"
    );
    assert_eq!(git(dir.path(), &["status", "--porcelain"]), "");
    // The message box is cleared after a successful commit.
    assert_eq!(
        h.get_by_role(Role::MultilineTextInput)
            .value()
            .unwrap_or_default(),
        ""
    );
}

#[test]
fn create_branch_then_undo() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label("🌿 Branch").click();
    frames(&mut h);
    // The dialog's name field has the focus (the history search box is also a text input).
    let field = h
        .get_all_by_role(Role::TextInput)
        .find(|n| n.is_focused())
        .expect("focused field");
    field.type_text("feature-x");
    frames(&mut h);
    h.get_by_label("Create branch").click();
    settle(&mut h);
    assert_eq!(git(dir.path(), &["branch", "--show-current"]), "feature-x");

    h.get_by_label_contains("Undo").click();
    settle(&mut h);
    assert_eq!(git(dir.path(), &["branch", "--show-current"]), "main");
    assert_eq!(git(dir.path(), &["branch", "--list", "feature-x"]), "");
    // The local change survived both checkouts.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
}

#[test]
fn discard_from_context_menu_asks_first() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label_contains("Local Changes").click();
    frames(&mut h);
    // The file row in the list (the diff header also shows the path).
    let row = h
        .get_all_by_label_contains("a.txt")
        .find(|n| n.accesskit_node().role() == Role::Button)
        .expect("file row");
    row.click_secondary();
    frames(&mut h);
    h.get_by_label("Discard changes…").click();
    frames(&mut h);
    // Nothing happens until the dialog is confirmed.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    h.get_by_label("Yes, do it").click();
    settle(&mut h);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\n"
    );
}

/// Every non-ASCII character in the UI code must exist in the bundled fonts,
/// so icons look the same on every system (no "tofu" boxes).
#[test]
fn ui_characters_have_glyphs() {
    let sources = SOURCES;
    // The harness sets up fonts the same way the app does.
    let harness = Harness::new_ui(|_| {});
    let ctx = harness.ctx.clone();
    let mut missing: Vec<char> = Vec::new();
    for src in sources {
        // Skip comments: only string contents reach the screen.
        for line in src.lines().filter(|l| !l.trim_start().starts_with("//")) {
            for c in line.chars().filter(|c| !c.is_ascii() && !c.is_control()) {
                let has = ctx.fonts_mut(|f| {
                    f.has_glyph(&eframe::egui::FontId::proportional(13.0), c)
                        && f.has_glyph(&eframe::egui::FontId::monospace(12.0), c)
                });
                if !has && !missing.contains(&c) {
                    missing.push(c);
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "characters without a glyph: {missing:?}"
    );
}

#[test]
#[ignore]
fn print_glyph_candidates() {
    let harness = Harness::new_ui(|_| {});
    let candidates = "→←↑↓⬆⬇⬅➡⤓⤒⤵⤴⇄⇅⇆🔀🔁🔄🔃⟳⟲↻↺↶↷↩↪⋯…⋮☰✕✖×❌🗙✗✓✔⑂⎇🌿▤▦▣▢☐☑📦🗃📥📤⏎⇧⇩⇪⬍⏏✚➕＋⚠⏳⌛👤🔍⚙⌘📝🕓📂📁🏷☁🔖⬤●○◯◉·•";
    let mut have = String::new();
    let mut missing = String::new();
    for c in candidates.chars() {
        let ok = harness
            .ctx
            .fonts_mut(|f| f.has_glyph(&eframe::egui::FontId::proportional(13.0), c));
        if ok { have.push(c) } else { missing.push(c) }
    }
    println!("HAVE: {have}\nMISSING: {missing}");
}

/// A merge stopped by a conflict in a.txt (ours = "ours", theirs = "theirs").
fn conflicted_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.name", "Test"]);
    git(p, &["config", "user.email", "test@example.com"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    git(p, &["config", "core.autocrlf", "false"]);
    std::fs::write(p.join("a.txt"), "top\nbase\nbottom\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", "base"]);
    git(p, &["switch", "-q", "-c", "other"]);
    std::fs::write(p.join("a.txt"), "top\ntheirs\nbottom\n").unwrap();
    git(p, &["commit", "-qam", "theirs"]);
    git(p, &["switch", "-q", "main"]);
    std::fs::write(p.join("a.txt"), "top\nours\nbottom\n").unwrap();
    git(p, &["commit", "-qam", "ours"]);
    let merge = Proc::new("git")
        .current_dir(p)
        .args(["merge", "other"])
        .output()
        .unwrap();
    assert!(
        !merge.status.success(),
        "the merge should stop on a conflict"
    );
    dir
}

#[test]
fn resolve_conflict_and_continue_merge() {
    let dir = conflicted_repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label("Resolve conflicts").click();
    frames(&mut h);
    h.get_all_by_label("Theirs")
        .next()
        .expect("Theirs button")
        .click();
    frames(&mut h);
    h.get_by_label("Mark as resolved").click();
    settle(&mut h);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "top\ntheirs\nbottom\n"
    );
    assert_eq!(
        git(dir.path(), &["diff", "--name-only", "--diff-filter=U"]),
        ""
    );

    h.get_by_label("Continue").click();
    settle(&mut h);
    assert_eq!(
        git(dir.path(), &["rev-list", "--parents", "-n1", "HEAD"])
            .split(' ')
            .count(),
        3,
        "a merge commit"
    );
    assert_eq!(git(dir.path(), &["status", "--porcelain"]), "");
}

#[test]
fn interactive_rebase_reorder_from_window() {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    let base = git(p, &["rev-parse", "HEAD"]);
    for name in ["A", "B", "C"] {
        std::fs::write(p.join(format!("{name}.txt")), name).unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-qm", name]);
    }
    let mut h = harness(p);
    frames(&mut h);
    let ctx = h.ctx.clone();
    let base = ferrum_core::Oid::from_str(&base).unwrap();
    h.state_mut()
        .handle(crate::command::Command::PlanRebase(Some(base)), &ctx);
    frames(&mut h);
    // Move the first commit (A) one step down: B, A, C.
    h.get_all_by_label("⬇")
        .next()
        .expect("move down button")
        .click();
    frames(&mut h);
    h.get_by_label("Start rebase").click();
    settle(&mut h);
    assert_eq!(git(p, &["log", "--format=%s", "-3"]), "C\nA\nB");
    assert!(h.state().tabs[0].rebase.is_none(), "the window closes");
}

#[test]
fn picks_up_changes_made_outside_the_app() {
    let dir = repo();
    git(dir.path(), &["checkout", "-q", "--", "a.txt"]);
    let mut h = harness(dir.path());
    frames(&mut h);
    assert!(h.state().tabs[0].status.is_empty());
    std::fs::write(dir.path().join("new.txt"), "from an editor\n").unwrap();
    std::thread::sleep(Duration::from_millis(3200));
    frames(&mut h);
    assert_eq!(h.state().tabs[0].status.unstaged.len(), 1);
    // A commit made in a terminal reloads the history too.
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "from terminal"]);
    std::thread::sleep(Duration::from_millis(3200));
    frames(&mut h);
    assert_eq!(h.state().tabs[0].commits[0].summary, "from terminal");
}

#[test]
fn push_and_pull_through_the_toolbar() {
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    let url = remote.path().to_string_lossy().into_owned();
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    git(p, &["remote", "add", "origin", &url]);

    let mut h = harness(p);
    frames(&mut h);
    h.get_by_label("⬆ Push").click();
    frames(&mut h);
    // The dialog proposes origin/main and setting the upstream.
    h.get_by_role_and_label(Role::Button, "Push").click();
    settle(&mut h);
    assert_eq!(git(remote.path(), &["log", "--format=%s", "main"]), "first");
    assert_eq!(
        git(p, &["rev-parse", "--abbrev-ref", "main@{upstream}"]),
        "origin/main"
    );

    // Someone else pushes a commit.
    let other = tempfile::tempdir().unwrap();
    git(other.path(), &["clone", "-q", &url, "."]);
    git(other.path(), &["config", "user.name", "Other"]);
    git(other.path(), &["config", "user.email", "other@example.com"]);
    std::fs::write(other.path().join("b.txt"), "b\n").unwrap();
    git(other.path(), &["add", "-A"]);
    git(other.path(), &["commit", "-qm", "from someone else"]);
    git(other.path(), &["push", "-q"]);

    h.get_by_label("⬇ Pull").click();
    settle(&mut h);
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "from someone else");
    assert!(p.join("b.txt").exists());
}
