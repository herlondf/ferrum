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
    include_str!("prefs.rs"),
    include_str!("external.rs"),
    include_str!("accent.rs"),
    include_str!("settings.rs"),
    include_str!("ui/activity.rs"),
    include_str!("ui/background.rs"),
    include_str!("app/background.rs"),
    include_str!("desktop_notify.rs"),
    include_str!("jobs.rs"),
    include_str!("refresh.rs"),
    include_str!("watcher.rs"),
    include_str!("benchmark.rs"),
    include_str!("ui/blame.rs"),
    include_str!("ui/blame_view.rs"),
    include_str!("ui/file_finder.rs"),
    include_str!("ui/file_lines.rs"),
    include_str!("ui/file_window.rs"),
    include_str!("i18n/pt_br/file_history.rs"),
    include_str!("ui/changes.rs"),
    include_str!("ui/changes_tools.rs"),
    include_str!("ui/commit_box.rs"),
    include_str!("ui/commit_settings.rs"),
    include_str!("ui/ai_message.rs"),
    include_str!("commit_prefs.rs"),
    include_str!("ui/conflicts.rs"),
    include_str!("ui/details.rs"),
    include_str!("ui/dialogs.rs"),
    include_str!("ui/diff_extra.rs"),
    include_str!("ui/diff_prefs.rs"),
    include_str!("ui/diff_text.rs"),
    include_str!("ui/diff_view.rs"),
    include_str!("i18n/pt_br/diff.rs"),
    include_str!("ui/forge.rs"),
    include_str!("ui/history.rs"),
    include_str!("ui/history_model.rs"),
    include_str!("ui/history_options.rs"),
    include_str!("ui/issue_links.rs"),
    include_str!("ui/avatars.rs"),
    include_str!("ui/stats.rs"),
    include_str!("ui/image_diff.rs"),
    include_str!("ui/palette.rs"),
    include_str!("ui/preferences.rs"),
    include_str!("ui/rebase.rs"),
    include_str!("ui/settings.rs"),
    include_str!("ui/sidebar.rs"),
    include_str!("ui/tools.rs"),
    include_str!("ui/sidebar_model.rs"),
    include_str!("ui/focus.rs"),
    include_str!("i18n/pt_br/sidebar.rs"),
    include_str!("i18n/mod.rs"),
    include_str!("i18n/pt_br/mod.rs"),
    include_str!("i18n/pt_br/common.rs"),
    include_str!("i18n/pt_br/main.rs"),
    include_str!("i18n/pt_br/editing.rs"),
    include_str!("i18n/pt_br/extras.rs"),
    include_str!("i18n/pt_br/preferences.rs"),
    include_str!("i18n/pt_br/activity.rs"),
    include_str!("i18n/pt_br/history.rs"),
    include_str!("i18n/pt_br/local.rs"),
    include_str!("ui/ref_dialogs.rs"),
    include_str!("ref_ops.rs"),
    include_str!("i18n/pt_br/refs.rs"),
    include_str!("repo_parts.rs"),
    include_str!("repo_settings.rs"),
    include_str!("app/parts.rs"),
    include_str!("ui/lfs.rs"),
    include_str!("ui/submodules.rs"),
    include_str!("ui/worktrees.rs"),
    include_str!("i18n/pt_br/parts.rs"),
    include_str!("sync.rs"),
    include_str!("ui/sync_dialogs.rs"),
    include_str!("ui/sync_ui.rs"),
    include_str!("ui/filter_combo.rs"),
    include_str!("i18n/pt_br/sync.rs"),
];

pub fn git(dir: &Path, args: &[&str]) -> String {
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
pub fn repo() -> tempfile::TempDir {
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

pub fn harness(path: &Path) -> Harness<'static, FerrumApp> {
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
pub fn frames(h: &mut Harness<'_, FerrumApp>) {
    for _ in 0..4 {
        h.step();
    }
}

/// Steps frames until no git operation is running.
pub fn settle(h: &mut Harness<'_, FerrumApp>) {
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

    type_into(&mut h, "Summary", "Second commit");
    type_into(&mut h, "Description (optional)", "More about it");
    h.get_by_label("Sign off").click();
    frames(&mut h);
    h.get_by_label("Commit 1 file").click();
    settle(&mut h);
    assert_eq!(
        git(dir.path(), &["log", "-1", "--format=%s"]),
        "Second commit"
    );
    let body = git(dir.path(), &["log", "-1", "--format=%b"]);
    assert!(body.starts_with("More about it"), "{body}");
    assert!(body.contains("Signed-off-by: Test <test@example.com>"));
    assert_eq!(git(dir.path(), &["status", "--porcelain"]), "");
    // The message box is cleared after a successful commit, and the message
    // goes to the recent list.
    assert_eq!(h.state().tabs[0].changes.message, "");
    assert_eq!(
        h.state().settings.commit.recent_messages,
        ["Second commit\n\nMore about it"]
    );
}

/// The text field with this placeholder.
fn field<'a>(h: &'a Harness<'_, FerrumApp>, placeholder: &'static str) -> egui_kittest::Node<'a> {
    h.get_by(move |n| n.placeholder() == Some(placeholder))
}

fn type_into(h: &mut Harness<'_, FerrumApp>, placeholder: &'static str, text: &str) {
    field(h, placeholder).focus();
    frames(h);
    field(h, placeholder).type_text(text);
    frames(h);
}

fn file_row<'a>(h: &'a Harness<'_, FerrumApp>, name: &'a str) -> egui_kittest::Node<'a> {
    h.get_all_by_label_contains(name)
        .find(|n| n.accesskit_node().role() == Role::Button)
        .expect("file row")
}

#[test]
fn enter_toggles_stage_and_amend_shows_last_commit() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label_contains("Local Changes").click();
    frames(&mut h);
    file_row(&h, "a.txt").click();
    frames(&mut h);
    h.key_press(eframe::egui::Key::Enter);
    settle(&mut h);
    assert_eq!(
        git(dir.path(), &["diff", "--cached", "--name-only"]),
        "a.txt"
    );
    // The selection follows the file into the staged list; Enter unstages it.
    h.key_press(eframe::egui::Key::Enter);
    settle(&mut h);
    assert_eq!(git(dir.path(), &["diff", "--cached", "--name-only"]), "");

    // Amending lists the files of the last commit as staged.
    h.get_by_label("Amend last commit").click();
    frames(&mut h);
    h.get_by_label_contains("Staged + last commit (1)");
    assert_eq!(h.state().tabs[0].changes.message, "first");
}

#[test]
fn drag_file_to_staged_list() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label_contains("Local Changes").click();
    frames(&mut h);
    let from = file_row(&h, "a.txt").rect().center();
    let to = h.get_by_label("Nothing staged yet.").rect().center();
    h.hover_at(from);
    h.step();
    h.drag_at(from);
    h.step();
    for i in 1..=5 {
        h.hover_at(from + (to - from) * (i as f32 / 5.0));
        h.step();
    }
    // Release without taking the cursor away, as a real mouse does.
    h.event(eframe::egui::Event::PointerButton {
        pos: to,
        button: eframe::egui::PointerButton::Primary,
        pressed: false,
        modifiers: eframe::egui::Modifiers::NONE,
    });
    settle(&mut h);
    assert_eq!(
        git(dir.path(), &["diff", "--cached", "--name-only"]),
        "a.txt"
    );
}

#[test]
fn ignore_untracked_file_from_menu() {
    let dir = repo();
    std::fs::write(dir.path().join("debug.log"), "x\n").unwrap();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label_contains("Local Changes").click();
    frames(&mut h);
    file_row(&h, "debug.log").click_secondary();
    frames(&mut h);
    h.get_by_label("Ignore").hover();
    frames(&mut h);
    h.get_by_label("All *.log files").click();
    settle(&mut h);
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
        "*.log\n"
    );
    assert!(!git(dir.path(), &["status", "--porcelain"]).contains("debug.log"));
}

#[cfg(unix)]
#[test]
fn failing_hook_offers_commit_without_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let dir = repo();
    let hook = dir.path().join(".git/hooks/pre-commit");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, "#!/bin/sh\necho lint found problems\nexit 1\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(dir.path(), &["add", "-A"]);

    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label_contains("Local Changes").click();
    frames(&mut h);
    type_into(&mut h, "Summary", "Change");
    h.get_by_label("Commit 1 file").click();
    settle(&mut h);
    h.get_by_label("The commit failed");
    assert!(h.query_all_by_label_contains("lint found problems").count() > 0);
    assert_eq!(git(dir.path(), &["log", "-1", "--format=%s"]), "first");

    h.get_by_label("Commit without hooks").click();
    settle(&mut h);
    assert_eq!(git(dir.path(), &["log", "-1", "--format=%s"]), "Change");
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
    settle(&mut h);
    assert_eq!(h.state().tabs[0].status.unstaged.len(), 1);
    // A commit made in a terminal reloads the history too.
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "from terminal"]);
    std::thread::sleep(Duration::from_millis(3200));
    settle(&mut h);
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
    // A plain click publishes main to origin and sets the upstream.
    h.get_by_label("⬆ Push").click();
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

#[test]
fn branch_names_get_spaces_replaced() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label("🌿 Branch").click();
    frames(&mut h);
    let field = h
        .get_all_by_role(Role::TextInput)
        .find(|n| n.is_focused())
        .expect("focused field");
    field.type_text("my new feature");
    frames(&mut h);
    h.get_by_label("Create branch").click();
    settle(&mut h);
    assert_eq!(
        git(dir.path(), &["branch", "--show-current"]),
        "my-new-feature"
    );
}

#[test]
fn toolbar_buttons_can_be_hidden() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    assert!(h.query_by_label("🌿 Branch").is_some());
    assert!(h.query_by_label(">_").is_some());
    h.state_mut().settings.prefs.hidden_toolbar = vec![
        crate::prefs::ToolbarButton::Branch,
        crate::prefs::ToolbarButton::Terminal,
    ];
    frames(&mut h);
    assert!(h.query_by_label("🌿 Branch").is_none());
    assert!(h.query_by_label(">_").is_none());
    assert!(h.query_by_label("📁").is_some());
}

#[cfg(unix)]
#[test]
fn opens_the_repository_in_a_custom_editor() {
    let dir = repo();
    let out = tempfile::tempdir().unwrap();
    let marker = out.path().join("opened.txt");
    let mut h = harness(dir.path());
    h.state_mut().settings.external = crate::external::ExternalTools {
        editor: crate::external::Editor::Custom,
        editor_command: format!("sh -c 'echo \"$1\" > {}' sh $FILE", marker.display()),
        ..Default::default()
    };
    frames(&mut h);
    h.get_by_label("📂 Repository").click();
    frames(&mut h);
    h.get_by_label("Open in editor").click();
    frames(&mut h);
    for _ in 0..200 {
        if let Ok(s) = std::fs::read_to_string(&marker)
            && !s.is_empty()
        {
            // Compare real paths: on macOS /var is a link to /private/var.
            let real = |p: &Path| std::fs::canonicalize(p).unwrap();
            assert_eq!(real(Path::new(s.trim())), real(dir.path()));
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the editor was not started");
}

#[test]
fn settings_sections_show_preferences() {
    let dir = repo();
    let mut h = harness(dir.path());
    let ctx = h.ctx.clone();
    h.state_mut().handle(
        crate::command::Command::DevStep(crate::command::DevStep::SettingsSection(
            "appearance".into(),
        )),
        &ctx,
    );
    frames(&mut h);
    assert!(h.query_by_label("Use the system accent color").is_some());
    h.get_by_label_contains("🛠 Tools").click();
    frames(&mut h);
    assert!(h.query_by_label_contains("$MERGED").is_some());
    h.get_by_label_contains("🔧 Git").click();
    frames(&mut h);
    assert!(h.query_by_label("Verbose git output").is_some());
}

/// Sidebar flows (in their own file to keep this one small).
#[path = "ui_tests_sidebar.rs"]
mod sidebar;

#[test]
fn polls_for_changes_when_not_watching() {
    let dir = repo();
    git(dir.path(), &["checkout", "-q", "--", "a.txt"]);
    let mut h = harness(dir.path());
    h.state_mut().settings.background.watch_files = false;
    h.state_mut().settings.background.poll_secs = 1;
    frames(&mut h);
    std::fs::write(dir.path().join("new.txt"), "from an editor\n").unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    settle(&mut h);
    assert_eq!(h.state().tabs[0].status.unstaged.len(), 1);
}

/// The operations of a repository, as recorded in the activity log.
fn recorded(path: &Path) -> Vec<std::sync::Arc<crate::jobs::Operation>> {
    crate::jobs::operations()
        .into_iter()
        .filter(|o| o.repo.as_deref() == Some(path))
        .collect()
}

#[test]
fn activity_window_shows_the_git_commands() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label_contains("Local Changes").click();
    frames(&mut h);
    h.get_by_label("Stage all").click();
    settle(&mut h);
    let ctx = h.ctx.clone();
    h.state_mut()
        .handle(crate::command::Command::ShowActivity, &ctx);
    frames(&mut h);
    h.get_by_label_contains("$ git add -A");
    let ops = recorded(h.state().tabs[0].path());
    assert_eq!(ops.len(), 1);
    assert_eq!(
        ops[0].outcome().map(|o| o.1),
        Some(crate::jobs::Outcome::Succeeded)
    );
    assert!(!ops[0].background);
}

/// A fetch from a remote that never answers is cancelled from the toolbar.
#[cfg(unix)]
#[test]
fn cancel_a_running_fetch() {
    let dir = repo();
    let p = dir.path();
    git(p, &["config", "protocol.ext.allow", "always"]);
    git(p, &["remote", "add", "origin", "ext::sleep 30"]);
    let mut h = harness(p);
    frames(&mut h);
    h.get_by_label("🔄 Fetch").click();
    frames(&mut h);
    assert!(h.state().tabs[0].busy());
    h.get_by_label("✖").click();
    settle(&mut h);
    let msg = h.state().tabs[0].message.as_ref().unwrap();
    assert!(msg.text.contains("cancelled"), "{}", msg.text);
    let ops = recorded(h.state().tabs[0].path());
    let op = ops.last().unwrap();
    assert_eq!(
        op.outcome().map(|o| o.1),
        Some(crate::jobs::Outcome::Cancelled)
    );
    assert!(op.commands()[0].cancelled);
}
