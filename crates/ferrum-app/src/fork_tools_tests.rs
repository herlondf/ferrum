//! UI tests of git-flow, bisect and patches.

use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::FerrumApp;
use crate::command::Command;
use crate::ui_tests::{frames, git, harness, repo, settle};

fn handle(h: &mut Harness<'_, FerrumApp>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(c, &ctx);
    frames(h);
}

fn wait_for(h: &mut Harness<'_, FerrumApp>, text: &str) {
    for _ in 0..300 {
        h.step();
        if h.query_all_by_label_contains(text).next().is_some() {
            // Let the dialog settle where it is drawn before clicking.
            frames(h);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("{text:?} never appeared");
}

fn start_steps(
    h: &mut Harness<'_, FerrumApp>,
    f: fn(&str, Option<&crate::tab::RepoTab>) -> Vec<Command>,
    arg: &str,
) {
    for c in f(arg, h.state().tabs.first()) {
        handle(h, c);
    }
}

/// `repo()` with its change committed, so the working copy is clean.
fn clean_repo() -> tempfile::TempDir {
    let dir = repo();
    git(dir.path(), &["commit", "-qam", "second"]);
    dir
}

#[test]
fn git_flow_init_start_and_finish_feature() {
    let dir = clean_repo();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);
    start_steps(&mut h, crate::ui::gitflow::dev_step, "init");
    h.get_by_role_and_label(Role::Button, "Initialize").click();
    settle(&mut h);
    assert_eq!(git(p, &["branch", "--show-current"]), "develop");
    assert_eq!(git(p, &["config", "gitflow.branch.master"]), "main");

    start_steps(&mut h, crate::ui::gitflow::dev_step, "start=feature");
    let name = h.get_by(|n| n.placeholder() == Some("my-feature"));
    name.focus();
    frames(&mut h);
    h.get_by(|n| n.placeholder() == Some("my-feature"))
        .type_text("login");
    frames(&mut h);
    assert!(h.query_by_label("Branch: feature/login").is_some());
    h.get_by_role_and_label(Role::Button, "Start").click();
    settle(&mut h);
    assert_eq!(git(p, &["branch", "--show-current"]), "feature/login");

    std::fs::write(p.join("login.txt"), "x\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "login"]);
    handle(&mut h, Command::Refresh);
    settle(&mut h);
    start_steps(&mut h, crate::ui::gitflow::dev_step, "finish=feature/login");
    assert!(
        h.query_by_label("Merges feature/login into develop.")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Finish").click();
    settle(&mut h);
    assert_eq!(git(p, &["branch", "--show-current"]), "develop");
    assert!(git(p, &["branch", "--list", "feature/login"]).is_empty());
    assert_eq!(
        git(p, &["log", "-1", "--format=%s"]),
        "Merge branch 'feature/login' into develop"
    );
}

#[test]
fn bisect_from_the_banner_finds_the_first_bad_commit() {
    let dir = clean_repo();
    let p = dir.path();
    for i in 3..=6 {
        let content = if i >= 5 { "bug\n" } else { "ok\n" };
        std::fs::write(p.join("a.txt"), format!("{content}{i}\n")).unwrap();
        git(p, &["commit", "-qam", &format!("c{i}")]);
    }
    let mut h = harness(p);
    frames(&mut h);
    // The dialog, filled with HEAD as bad and the first commit as good.
    let first = git(p, &["rev-parse", "HEAD~5"]);
    let d = crate::ui::bisect::start(&h.state().tabs[0], "HEAD", &first);
    handle(&mut h, Command::Dialog(d));
    assert!(
        h.query_by_label("5 commits to search, about 3 steps.")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Start bisect")
        .click();
    settle(&mut h);
    assert!(h.query_by_label("Bisecting").is_some());
    assert!(h.query_by_label_contains("revisions left").is_some());
    for _ in 0..5 {
        if h.query_by_label("— first bad commit:").is_some() {
            break;
        }
        let bad = std::fs::read_to_string(p.join("a.txt"))
            .unwrap()
            .starts_with("bug");
        let button = if bad { "✖ Bad" } else { "✔ Good" };
        h.get_by_label(button).click();
        settle(&mut h);
    }
    assert!(h.query_by_label("— first bad commit:").is_some());
    assert!(h.query_all_by_label_contains("c5").next().is_some());
    h.get_by_role_and_label(Role::Button, "Reset").click();
    settle(&mut h);
    assert!(h.query_by_label("Bisecting").is_none());
    assert!(!p.join(".git/BISECT_LOG").exists());
}

#[test]
fn apply_patch_file_commits_with_am() {
    let dir = clean_repo();
    let p = dir.path();
    std::fs::write(p.join("b.txt"), "b\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "add b"]);
    let file = p.join("../b.patch");
    let file = file.with_file_name(format!(
        "{}-b.patch",
        p.file_name().unwrap().to_string_lossy()
    ));
    let text = git(p, &["format-patch", "--stdout", "-1", "HEAD"]);
    std::fs::write(&file, text + "\n").unwrap();
    git(p, &["reset", "-q", "--hard", "HEAD~1"]);

    let mut h = harness(p);
    frames(&mut h);
    handle(&mut h, crate::ui::patches::open_file(&file));
    wait_for(&mut h, "Applies cleanly");
    assert!(h.query_by_label("• add b").is_some());
    assert!(h.query_by_label("b.txt").is_some());
    h.get_by_role_and_label(Role::Button, "Apply").click();
    settle(&mut h);
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "add b");
    let _ = std::fs::remove_file(file);
}

#[test]
fn pasted_patch_applies_to_working_copy_and_am_conflict_shows_banner() {
    let dir = clean_repo();
    let p = dir.path();
    std::fs::write(p.join("a.txt"), "pasted\n").unwrap();
    let diff = git(p, &["diff"]) + "\n";
    git(p, &["checkout", "--", "a.txt"]);
    let mut h = harness(p);
    frames(&mut h);
    h.event(eframe::egui::Event::Paste(diff));
    frames(&mut h);
    wait_for(&mut h, "Applies cleanly");
    // Not an email: it can only change the files.
    h.get_by_label("Change the files only (git apply)").click();
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Apply").click();
    settle(&mut h);
    assert_eq!(
        std::fs::read_to_string(p.join("a.txt")).unwrap(),
        "pasted\n"
    );
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "second");

    // A series that does not fit stops `git am`: the banner offers Abort.
    git(p, &["commit", "-qam", "pasted"]);
    let mail = git(p, &["format-patch", "--stdout", "-1", "HEAD"]) + "\n";
    git(p, &["reset", "-q", "--hard", "HEAD~1"]);
    std::fs::write(p.join("a.txt"), "other\n").unwrap();
    git(p, &["commit", "-qam", "other"]);
    handle(&mut h, Command::Refresh);
    settle(&mut h);
    handle(
        &mut h,
        Command::Dialog(crate::ui::patches::apply_dialog(mail, "test".into())),
    );
    wait_for(&mut h, "Applies only with a 3-way merge");
    h.get_by_role_and_label(Role::Button, "Apply").click();
    settle(&mut h);
    assert!(h.query_by_label("Applying patches").is_some());
    h.get_by_role_and_label(Role::Button, "Abort").click();
    frames(&mut h);
    h.get_by_label("Yes, do it").click();
    settle(&mut h);
    assert!(!p.join(".git/rebase-apply").exists());
    assert_eq!(git(p, &["log", "-1", "--format=%s"]), "other");
}
