//! UI tests of the Repository Manager, tabs, workspaces, the overview,
//! launch requests and the error fixes.

use std::path::{Path, PathBuf};

use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::FerrumApp;
use crate::command::{Command, Op};
use crate::repos::{RepoCmd, Request};
use crate::ui_tests::{frames, git, harness, repo, settle};

fn handle(h: &mut Harness<'_, FerrumApp>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(c, &ctx);
    frames(h);
}

/// The app without open tabs, started with `requests`.
fn empty_app(requests: Vec<Request>) -> Harness<'static, FerrumApp> {
    let mut h = Harness::builder()
        .with_size([1400.0, 900.0])
        .build_eframe(move |cc| FerrumApp::new(cc, Vec::new()).with_launch(requests, None));
    h.state_mut().settings.language = crate::i18n::Language::English;
    frames(&mut h);
    h
}

fn name_of(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn manager_renames_removes_and_opens() {
    let a = repo();
    let b = repo();
    let mut h = empty_app(Vec::new());
    {
        let lib = &mut h.state_mut().settings.library;
        lib.remember(a.path());
        lib.remember(b.path());
        lib.remember(Path::new("/no/such/repository"));
    }
    frames(&mut h);
    h.get_by_label_contains("repositories no longer exist");
    h.get_by_label("Remove missing").click();
    frames(&mut h);
    assert_eq!(h.state().settings.library.repos.len(), 2);

    // F2 renames the selected repository.
    h.get_by_label(&name_of(a.path())).click();
    frames(&mut h);
    h.key_press(Key::F2);
    frames(&mut h);
    h.get_by(|n| n.placeholder() == Some(&name_of(a.path())))
        .type_text("Renamed");
    frames(&mut h);
    h.key_press(Key::Enter);
    frames(&mut h);
    let lib = &h.state().settings.library;
    assert_eq!(lib.custom_name(a.path()), Some("Renamed"));

    // Delete removes it from the list only.
    h.get_by_label("Renamed").click();
    frames(&mut h);
    h.key_press(Key::Delete);
    frames(&mut h);
    assert!(h.state().settings.library.get(a.path()).is_none());
    assert!(a.path().exists());

    // Enter opens the selection.
    h.get_by_label(&name_of(b.path())).click();
    frames(&mut h);
    h.key_press(Key::Enter);
    frames(&mut h);
    assert_eq!(h.state().tabs.len(), 1);
    assert_eq!(real(h.state().tabs[0].path()), real(b.path()));
}

#[test]
fn new_tab_close_and_reopen() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    frames(&mut h);
    assert!(h.state().repo_state().home_active);
    h.get_by(|n| n.placeholder() == Some("Filter repositories"));
    // Ctrl+W closes the manager tab first, then the repository.
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    frames(&mut h);
    assert!(!h.state().repo_state().home_open);
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    frames(&mut h);
    assert!(h.state().tabs.is_empty());
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    frames(&mut h);
    assert_eq!(h.state().tabs.len(), 1);
}

#[test]
fn tab_menu_closes_others_and_same_names_are_told_apart() {
    let root = tempfile::tempdir().unwrap();
    let mk = |p: &str| {
        let d = root.path().join(p);
        std::fs::create_dir_all(&d).unwrap();
        git(&d, &["init", "-q", "-b", "main"]);
        d
    };
    let a = mk("client-a/app");
    let b = mk("client-b/app");
    let mut h = harness(&a);
    handle(&mut h, Command::OpenRepo(b.clone()));
    let labels = crate::ui::tabs::labels(&h.state().tabs, &h.state().settings.library);
    assert_eq!(labels, ["app — client-a", "app — client-b"]);
    h.get_by_label_contains("app — client-a").click_secondary();
    frames(&mut h);
    h.get_by_label("Close Others").click();
    frames(&mut h);
    assert_eq!(h.state().tabs.len(), 1);
    assert_eq!(real(h.state().tabs[0].path()), real(&a));
    // The closed one can come back.
    handle(&mut h, Command::Repos(RepoCmd::ReopenClosed));
    assert_eq!(h.state().tabs.len(), 2);
}

#[test]
fn workspaces_switch_their_tabs() {
    let a = repo();
    let b = repo();
    let mut h = harness(a.path());
    handle(
        &mut h,
        Command::Repos(RepoCmd::SwitchWorkspace("work/x".into())),
    );
    assert!(h.state().tabs.is_empty());
    handle(&mut h, Command::OpenRepo(b.path().into()));
    handle(
        &mut h,
        Command::Repos(RepoCmd::SwitchWorkspace(String::new())),
    );
    let paths: Vec<PathBuf> = h.state().tabs.iter().map(|t| real(t.path())).collect();
    assert_eq!(paths, [real(a.path())]);
    // Moving a tab to a workspace closes it here.
    handle(
        &mut h,
        Command::Repos(RepoCmd::MoveToWorkspace(0, "work/x".into())),
    );
    assert!(h.state().tabs.is_empty());
    // Switching from the palette.
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    frames(&mut h);
    h.get_by(|n| n.placeholder() == Some("Type a command, branch or file…"))
        .type_text("Workspace: work");
    frames(&mut h);
    h.key_press(Key::Enter);
    frames(&mut h);
    assert_eq!(h.state().settings.library.workspace, "work/x");
    let paths: Vec<PathBuf> = h.state().tabs.iter().map(|t| real(t.path())).collect();
    assert_eq!(paths, [real(b.path()), real(a.path())]);
}

#[test]
fn open_quickly_finds_a_known_repository() {
    let a = repo();
    let mut h = empty_app(Vec::new());
    h.state_mut().settings.library.entry(a.path()).name = "Zebra project".into();
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::O);
    frames(&mut h);
    h.get_by(|n| n.placeholder() == Some("Type a repository name…"))
        .type_text("zebra");
    frames(&mut h);
    frames(&mut h);
    h.key_press(Key::Enter);
    frames(&mut h);
    assert_eq!(h.state().tabs.len(), 1);
}

#[test]
fn overview_shows_readme_and_counts() {
    let dir = repo();
    std::fs::write(
        dir.path().join("README.md"),
        "# Hello overview\n\nSome *text*.\n",
    )
    .unwrap();
    git(dir.path(), &["add", "README.md"]);
    git(dir.path(), &["commit", "-qm", "readme"]);
    let mut h = harness(dir.path());
    frames(&mut h);
    h.get_by_label_contains("Overview").click();
    for _ in 0..200 {
        h.step();
        if h.state().tabs[0].overview.summary().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    frames(&mut h);
    let s = h.state().tabs[0].overview.summary().unwrap().clone();
    assert_eq!(s.commits, 2);
    h.get_by_label_contains("Hello overview");
    h.get_by_label("Contributors");
}

#[test]
fn launch_requests_open_files_folders_and_links() {
    let dir = repo();
    let plain = tempfile::tempdir().unwrap();
    let h = empty_app(vec![Request::FileLog(dir.path().join("a.txt"))]);
    assert_eq!(h.state().tabs.len(), 1);
    assert_eq!(h.state().tabs[0].view, crate::tab::View::File);

    // A folder that is not a repository: offer git init.
    let h = empty_app(vec![Request::Open(plain.path().into())]);
    h.get_by_label("New repository");
    assert!(h.state().has_dialog());

    // "Open in Desktop" links prefill the clone dialog.
    let link = "x-github-client://openRepo/https://github.com/o/ferrum-x";
    let reqs = crate::repos::parse_args(&[link.into()], Path::new("/"));
    let h = empty_app(reqs);
    h.get_by_label("Clone repository");
    let has = |v: &str| {
        h.get_all_by(|n| n.value().as_deref() == Some(v))
            .next()
            .is_some()
    };
    assert!(has("https://github.com/o/ferrum-x"));
    assert!(has("ferrum-x"));
}

#[test]
fn stale_index_lock_can_be_deleted() {
    let dir = repo();
    let lock = dir.path().join(".git/index.lock");
    std::fs::write(&lock, "").unwrap();
    let mut h = harness(dir.path());
    handle(&mut h, Command::Run(Op::StageAll));
    settle(&mut h);
    h.get_by_label("Delete the lock file").click();
    frames(&mut h);
    h.get_by_label("Yes, do it").click();
    settle(&mut h);
    // Other tests may run git at the same time; then the lock stays.
    let msg = h.state().tabs[0].message.as_ref().map(|m| m.text.clone());
    assert!(
        !lock.exists() || msg.as_deref().unwrap_or("").contains("still running"),
        "{msg:?}"
    );
}

#[test]
fn repository_settings_set_the_commit_template() {
    let dir = repo();
    std::fs::write(dir.path().join("tpl.txt"), "feat: ").unwrap();
    let mut h = harness(dir.path());
    handle(&mut h, Command::Repos(RepoCmd::Settings));
    h.get_by(|n| n.placeholder() == Some("No template")).focus();
    frames(&mut h);
    h.get_by(|n| n.placeholder() == Some("No template"))
        .type_text("tpl.txt");
    frames(&mut h);
    h.get_by_label("Save").click();
    settle(&mut h);
    assert_eq!(git(dir.path(), &["config", "commit.template"]), "tpl.txt");
}

#[test]
fn tab_opens_in_its_own_window() {
    let a = repo();
    let b = repo();
    let mut h = harness(a.path());
    handle(&mut h, Command::OpenRepo(b.path().into()));
    handle(&mut h, Command::Repos(RepoCmd::OpenInWindow(1)));
    frames(&mut h);
    let windows: Vec<PathBuf> = h
        .state()
        .repo_state()
        .windows
        .iter()
        .map(real)
        .collect();
    assert_eq!(windows, [real(b.path())]);
    // The main window moved on to the other tab.
    assert_eq!(
        real(h.state().tabs[h.state().active_index()].path()),
        real(a.path())
    );
    // A click in the window acts on the window's tab.
    let count = h.get_all_by_label_contains("All Commits").count();
    assert_eq!(count, 2, "sidebar of the main window and of the tab window");
    // Closing the tab closes its window.
    handle(&mut h, Command::CloseTab(1));
    assert!(h.state().repo_state().windows.is_empty());
}

/// A path with links resolved (macOS /private/var, Windows spellings).
fn real(p: impl AsRef<Path>) -> PathBuf {
    let p = p.as_ref();
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}
