//! UI tests of the multi-repository dashboard, the Insights view, the
//! cleanup assistant and sparse checkout.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::app::FerrumApp;
use crate::command::Command;
use crate::repos::RepoCmd;
use crate::tab::View;
use crate::ui_tests::{frames, git, harness, settle};

fn handle(h: &mut Harness<'_, FerrumApp>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(c, &ctx);
    frames(h);
}

/// Steps frames until `cond` holds.
fn wait(h: &mut Harness<'_, FerrumApp>, what: &str, cond: impl Fn(&FerrumApp) -> bool) {
    for _ in 0..1000 {
        h.step();
        if cond(h.state()) {
            frames(h);
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

fn init(p: &Path) {
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.name", "Test"]);
    git(p, &["config", "user.email", "test@example.com"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    git(p, &["config", "core.autocrlf", "false"]);
}

fn commit(p: &Path, file: &str, text: &str, msg: &str) {
    let f = p.join(file);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", msg]);
}

/// A server repository and a clone of it.
fn clone_pair(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let server = root.join(format!("{name}-server"));
    std::fs::create_dir_all(&server).unwrap();
    init(&server);
    commit(&server, "a.txt", "a\n", "first");
    let clone = root.join(name);
    git(
        root,
        &[
            "clone",
            "-q",
            &server.to_string_lossy(),
            &clone.to_string_lossy(),
        ],
    );
    init(&clone);
    (server, clone)
}

#[test]
fn dashboard_filters_fetches_and_pulls_all() {
    let root = tempfile::tempdir().unwrap();
    let (server_a, a) = clone_pair(root.path(), "alpha");
    let (_server_b, b) = clone_pair(root.path(), "beta");
    std::fs::write(b.join("a.txt"), "changed\n").unwrap();
    let mut h = harness(&a);
    h.state_mut().settings.library.remember(&b);
    handle(&mut h, Command::Repos(RepoCmd::NewTab));
    wait(&mut h, "glances", |app| {
        let m = &app.repo_state().manager;
        m.glance(&b).is_some_and(|g| g.dirty) && m.glance(&a).is_some()
    });

    // The "Changes" filter shows only the dirty one (tabs keep their name).
    let count = |h: &Harness<'_, FerrumApp>, n: &str| h.query_all_by_label(n).count();
    let (alpha, beta) = (count(&h, "alpha"), count(&h, "beta"));
    h.get_by_label("Changes").click();
    frames(&mut h);
    assert_eq!(count(&h, "alpha"), alpha - 1);
    assert_eq!(count(&h, "beta"), beta);
    h.get_by_label("Changes").click();
    frames(&mut h);
    assert_eq!(count(&h, "alpha"), alpha);

    // New commits on the server: Fetch all sees them, Pull all brings them.
    commit(&server_a, "b.txt", "b\n", "second");
    commit(&server_a, "c.txt", "c\n", "third");
    h.get_by_label_contains("Fetch all").click();
    frames(&mut h);
    settle(&mut h);
    wait(&mut h, "behind", |app| {
        app.repo_state()
            .manager
            .glance(&a)
            .is_some_and(|g| g.ahead_behind == Some((0, 2)))
    });
    h.get_by_label_contains("Fetch all finished");
    h.get_by_label("Behind").click();
    frames(&mut h);
    assert_eq!(count(&h, "beta"), beta - 1);
    h.get_by_label_contains("Pull all").click();
    frames(&mut h);
    settle(&mut h);
    assert_eq!(git(&a, &["log", "-1", "--format=%s"]), "third");
    // The open tab reloads; the list mode shows the result.
    h.get_by_label("List").click();
    frames(&mut h);
    assert!(h.state().settings.library.list_view);
    h.get_by_label("alpha");
}

#[test]
fn insights_view_reads_hotspots_and_owners() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    for i in 0..4 {
        commit(p, "src/hot.rs", &"x\n".repeat(i + 1), &format!("hot {i}"));
    }
    commit(p, "docs/readme.md", "doc\n", "docs");
    commit(
        p,
        ".github/CODEOWNERS",
        "* @team\nsrc/ @rustaceans\n",
        "owners",
    );
    let mut h = harness(p);
    handle(&mut h, Command::SetView(View::Insights));
    wait(&mut h, "insights", |app| {
        app.tabs[0].insights.result().is_some()
    });
    let ins = h.state().tabs[0].insights.result().unwrap();
    assert_eq!(ins.commits, 6);
    assert_eq!(ins.files[0].path, "src/hot.rs");
    assert_eq!(ins.bus_factor, 1);
    h.get_by_label_contains("Hotspots");
    h.get_by_label_contains("Contributors over time");
    // CODEOWNERS lookup.
    let field = h.get_by(|n| n.placeholder() == Some("Who owns a path? e.g. src/main.rs"));
    field.focus();
    frames(&mut h);
    h.get_by(|n| n.placeholder() == Some("Who owns a path? e.g. src/main.rs"))
        .type_text("src/main.rs");
    frames(&mut h);
    h.get_by_label_contains("@rustaceans");
    // The path filter reloads for that path only.
    let f = h.get_by(|n| n.placeholder() == Some("Path, e.g. src/"));
    f.focus();
    frames(&mut h);
    h.get_by(|n| n.placeholder() == Some("Path, e.g. src/"))
        .type_text("docs");
    frames(&mut h);
    h.key_press(eframe::egui::Key::Enter);
    wait(&mut h, "filtered", |app| {
        app.tabs[0]
            .insights
            .result()
            .is_some_and(|i| i.files.len() == 1)
    });
    assert_eq!(
        h.state().tabs[0].insights.result().unwrap().files[0].path,
        "docs/readme.md"
    );
}

#[test]
fn cleanup_lists_big_files_and_deletes_merged_branches() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    commit(p, "small.txt", "s\n", "small");
    commit(p, "blob.bin", &"z".repeat(20_000), "big");
    git(p, &["rm", "-q", "blob.bin"]);
    git(p, &["commit", "-q", "-m", "drop big"]);
    git(p, &["branch", "merged-one", "HEAD~1"]);
    git(p, &["checkout", "-q", "-b", "open-work"]);
    commit(p, "w.txt", "w\n", "work");
    git(p, &["checkout", "-q", "main"]);
    let mut h = harness(p);
    // Tall enough for every card of the page.
    h.set_size(eframe::egui::vec2(1400.0, 1600.0));
    handle(&mut h, Command::SetView(View::Insights));
    h.get_by_label_contains("Cleanup").click();
    frames(&mut h);
    wait(&mut h, "scan", |app| {
        app.tabs[0].insights.cleanup.scan().is_some()
    });
    let scan = h.state().tabs[0].insights.cleanup.scan().unwrap();
    assert_eq!(scan.files[0].path, "blob.bin");
    assert!(!scan.files[0].in_head);
    assert!(!h.state().tabs[0].insights.cleanup.scanning());

    // Removing from history asks for the repository name (or shows the
    // commands to run when git filter-repo is missing).
    let boxes: Vec<_> = h.get_all_by_role(Role::CheckBox).collect();
    boxes[0].click();
    frames(&mut h);
    h.get_by_label_contains("Remove 1 from history").click();
    frames(&mut h);
    if h.query_by_label_contains("git filter-repo is not installed")
        .is_some()
    {
        h.get_by_label_contains("git filter-repo --force --invert-paths --path blob.bin");
        h.get_by_label("Close").click();
        frames(&mut h);
    } else {
        assert!(
            h.get_by_label("Rewrite history")
                .accesskit_node()
                .is_disabled()
        );
        h.get_by_label("Cancel").click();
    }
    frames(&mut h);
    frames(&mut h);
    assert!(!h.state().tabs[0].insights.cleanup.dialog_open());

    h.get_by_label_contains("Delete 1 branches").click();
    frames(&mut h);
    assert!(h.state().tabs[0].insights.cleanup.dialog_open(), "confirm");
    frames(&mut h);
    h.get_by_label("Delete").click();
    frames(&mut h);
    settle(&mut h);
    let branches = git(p, &["branch", "--format=%(refname:short)"]);
    assert_eq!(branches.lines().collect::<Vec<_>>(), ["main", "open-work"]);
}

#[test]
fn sparse_checkout_picks_folders() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    std::fs::create_dir_all(p.join("app")).unwrap();
    std::fs::write(p.join("app/a.txt"), "a").unwrap();
    commit(p, "lib/b.txt", "b", "files");
    let mut h = harness(p);
    handle(
        &mut h,
        Command::Repos(RepoCmd::Dev("insights:sparse".into())),
    );
    wait(&mut h, "folders", |app| {
        app.tabs[0].insights.sparse.folders().is_some()
    });
    assert_eq!(
        h.state().tabs[0].insights.sparse.folders().unwrap(),
        ["app", "lib"]
    );
    h.state_mut().tabs[0]
        .insights
        .sparse
        .picked
        .insert("lib".into());
    frames(&mut h);
    h.get_by_label_contains("Check out 1 folders").click();
    frames(&mut h);
    settle(&mut h);
    assert!(!p.join("app/a.txt").exists());
    assert!(p.join("lib/b.txt").exists());
    wait(&mut h, "state", |app| {
        app.tabs[0].insights.sparse.picked.contains("lib") && app.tabs[0].job.is_none()
    });
    h.get_by_label_contains("Check out everything").click();
    frames(&mut h);
    settle(&mut h);
    assert!(p.join("app/a.txt").exists());
}
