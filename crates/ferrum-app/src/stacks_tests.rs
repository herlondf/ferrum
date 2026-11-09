//! UI tests of stacked branches (sidebar, restack with Undo, chained pull
//! requests against a fake server), absorb and moving changes between
//! commits.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use ferrum_core::forge::ForgeKind;

use crate::app::FerrumApp;
use crate::command::Command;
use crate::settings::ForgeAccount;
use crate::ui_tests::{frames, git, harness, repo, settle};

fn handle(h: &mut Harness<'_, FerrumApp>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(c, &ctx);
    frames(h);
}

fn wait_for(h: &mut Harness<'_, FerrumApp>, text: &str) {
    for _ in 0..1000 {
        h.step();
        if h.query_by_label_contains(text).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let last = h.state().tabs[0].message.as_ref().map(|m| m.text.clone());
    panic!("{text:?} never appeared (last message: {last:?})");
}

fn log(p: &Path, rev: &str) -> String {
    git(p, &["log", "--format=%s", rev])
}

fn commit(p: &Path, file: &str, content: &str, msg: &str) {
    std::fs::write(p.join(file), content).unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", msg]);
}

/// main ← api ← ui, then main moves on.
fn stacked() -> tempfile::TempDir {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    git(p, &["switch", "-q", "-c", "api"]);
    commit(p, "api.txt", "api\n", "add api");
    git(p, &["switch", "-q", "-c", "ui"]);
    commit(p, "ui.txt", "ui\n", "add ui");
    git(p, &["switch", "-q", "main"]);
    commit(p, "main.txt", "m\n", "main moved");
    dir
}

#[test]
fn sidebar_shows_the_stack_and_restack_can_be_undone() {
    let dir = stacked();
    let p = dir.path();
    let before = git(p, &["rev-parse", "ui"]);
    let mut h = harness(p);
    frames(&mut h);
    assert!(h.query_by_label_contains("Stacks").is_some());
    h.get_by_label_contains("Restack needed").click();
    frames(&mut h);
    assert!(
        h.query_by_label_contains("rebased onto the branch below")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Restack").click();
    settle(&mut h);
    assert_eq!(log(p, "ui"), "add ui\nadd api\nmain moved\nfirst");
    assert_eq!(git(p, &["branch", "--show-current"]), "main");
    let backups = git(
        p,
        &["for-each-ref", "--format=%(refname)", "refs/ferrum/backup/"],
    );
    assert!(
        backups.contains("/ui") && backups.contains("/api"),
        "{backups}"
    );
    wait_for(&mut h, "Restacked 2 branches on main");

    // The toast's Undo puts the branches back.
    h.get_by_role_and_label(Role::Button, "Undo").click();
    settle(&mut h);
    assert_eq!(git(p, &["rev-parse", "ui"]), before);
}

#[test]
fn new_branch_on_top_of_a_branch() {
    let dir = stacked();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);
    handle(
        &mut h,
        Command::Dialog(crate::ui::stacks::add_branch(
            ferrum_core::stack::Insert::Above("ui".into()),
        )),
    );
    h.get_by(|n| n.placeholder() == Some("feature/part-2"))
        .type_text("ui-2");
    frames(&mut h);
    h.get_by_role_and_label(Role::Button, "Create branch")
        .click();
    settle(&mut h);
    assert_eq!(git(p, &["branch", "--show-current"]), "ui-2");
    assert_eq!(git(p, &["config", "branch.ui-2.ferrumParent"]), "ui");
}

/// A fake GitHub: no open requests, numbered requests on POST, PATCH ok.
/// Requests received: (method, path, body).
type Log = Arc<Mutex<Vec<(String, String, String)>>>;

fn serve() -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log: Log = Arc::default();
    let seen = log.clone();
    let next = AtomicU64::new(10);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let mut parts = line.split_whitespace();
            let (method, path) = (
                parts.next().unwrap_or("").to_owned(),
                parts.next().unwrap_or("").to_owned(),
            );
            let mut len = 0;
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            let _ = reader.read_exact(&mut body);
            let reply = match method.as_str() {
                "GET" => "[]".to_owned(),
                _ => {
                    let n = if method == "POST" {
                        next.fetch_add(1, Ordering::SeqCst)
                    } else {
                        0
                    };
                    format!(
                        r#"{{"number": {n}, "title": "T", "user": {{"login": "ana"}}, "head": {{"ref": "x"}}, "base": {{"ref": "y"}}, "html_url": "u", "updated_at": "t"}}"#
                    )
                }
            };
            seen.lock()
                .unwrap()
                .push((method, path, String::from_utf8_lossy(&body).into_owned()));
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    (base, log)
}

#[test]
fn creates_chained_pull_requests() {
    let (base, requests) = serve();
    let dir = stacked();
    let p = dir.path();
    git(
        p,
        &["remote", "add", "origin", &format!("{base}/acme/app.git")],
    );
    let mut h = harness(p);
    frames(&mut h);
    {
        let app = h.state_mut();
        // No automatic fetch: it would keep the tab busy.
        app.settings.sync.auto_fetch_minutes = 0;
        app.settings.accounts.push(ForgeAccount {
            kind: ForgeKind::GitHub,
            host: base.clone(),
            username: "ana".into(),
            token: "tok".into(),
            user: String::new(),
            notifications: false,
        });
        crate::hosted::sync_registry(&app.settings);
        let t = &mut app.tabs[0];
        t.remotes = t.repo.remotes().unwrap();
        // Set directly: the known-hosts registry is shared by parallel tests.
        t.hosted = Some(ferrum_core::forge::HostedRepo {
            kind: ForgeKind::GitHub,
            host: base.clone(),
            path: "acme/app".into(),
        });
    }
    let dialog = {
        let t = &h.state().tabs[0];
        crate::ui::stacks::requests(t, &t.stacks[0])
    };
    // An automatic fetch may be running: the dialog waits for it.
    settle(&mut h);
    handle(&mut h, Command::Dialog(dialog));
    // Nothing to push to in this test.
    h.get_by_label("Push the stack first").click();
    frames(&mut h);
    let enabled = |h: &Harness<'_, FerrumApp>| {
        !h.get_by_role_and_label(Role::Button, "Create pull requests")
            .accesskit_node()
            .is_disabled()
    };
    for _ in 0..300 {
        if enabled(&h) {
            break;
        }
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    let login = h
        .query_by_label_contains("Log in to")
        .map(|n| format!("{n:?}"));
    assert!(enabled(&h), "login: {login:?}");
    h.get_by_role_and_label(Role::Button, "Create pull requests")
        .click();
    wait_for(&mut h, "Created 2 pull requests: #10, #11");
    let all = requests.lock().unwrap().clone();
    let posts: Vec<&(String, String, String)> = all.iter().filter(|r| r.0 == "POST").collect();
    assert_eq!(posts.len(), 2);
    assert!(
        posts[0].2.contains(r#""head": "api""#) && posts[0].2.contains(r#""base": "main""#),
        "{all:?}"
    );
    assert!(posts[1].2.contains(r#""head": "ui""#) && posts[1].2.contains(r#""base": "api""#));
    let patches: Vec<_> = all.iter().filter(|r| r.0 == "PATCH").collect();
    assert_eq!(patches.len(), 2);
    assert!(patches[0].2.contains("#11"), "{}", patches[0].2);
}

/// feature: A changes line 2 of f.txt; locally line 2 changes again and a
/// new file appears.
fn absorbable() -> tempfile::TempDir {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    commit(p, "f.txt", "1\n2\n3\n4\n5\n6\n7\n8\n", "base f");
    git(p, &["switch", "-q", "-c", "feature"]);
    commit(p, "f.txt", "1\ntwo\n3\n4\n5\n6\n7\n8\n", "A");
    commit(p, "g.txt", "g\n", "B");
    std::fs::write(p.join("f.txt"), "1\nTWO\n3\n4\n5\n6\n7\n8\n").unwrap();
    std::fs::write(p.join("new.txt"), "new\n").unwrap();
    dir
}

#[test]
fn absorb_previews_and_squashes_into_the_commit() {
    let dir = absorbable();
    let p = dir.path();
    let mut h = harness(p);
    frames(&mut h);
    let d = crate::ui::absorb::open(&h.state().tabs[0]);
    handle(&mut h, Command::Dialog(d));
    wait_for(&mut h, "Absorb 1 change");
    assert!(
        h.query_by_label_contains("Stays as a local change")
            .is_some()
    );
    assert!(h.query_by_label_contains("new file").is_some());
    h.get_by_role_and_label(Role::Button, "Absorb 1 change")
        .click();
    settle(&mut h);
    assert_eq!(log(p, "HEAD"), "B\nA\nbase f\nfirst");
    assert!(git(p, &["show", "HEAD~1:f.txt"]).contains("TWO"));
    assert_eq!(git(p, &["status", "--porcelain"]), "?? new.txt");
    wait_for(&mut h, "Absorbed 1 change into 1 commit");
    // Undo brings the change back as a local change.
    h.get_by_role_and_label(Role::Button, "Undo").click();
    settle(&mut h);
    assert!(git(p, &["diff"]).contains("+TWO"));
    assert_eq!(log(p, "HEAD"), "B\nA\nbase f\nfirst");
    assert!(!git(p, &["show", "HEAD~1:f.txt"]).contains("TWO"));
}

#[test]
fn moves_a_file_to_an_older_commit_from_the_details() {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "a.txt"]);
    git(p, &["switch", "-q", "-c", "feature"]);
    commit(p, "x.txt", "x\n", "X");
    std::fs::write(p.join("y.txt"), "y\n").unwrap();
    commit(p, "z.txt", "z\n", "Y and Z");
    let mut h = harness(p);
    frames(&mut h);
    let head = h.state().tabs[0].head_oid.unwrap();
    handle(&mut h, Command::SelectCommit(head));
    h.get_by_label("y.txt").click();
    frames(&mut h);
    // The bar above the diff offers the move.
    assert!(h.query_by_label_contains("Move y.txt").is_some());
    let cmds = crate::ui::move_hunks::dev_step("y.txt>HEAD~1", &h.state().tabs[0]);
    for c in cmds {
        handle(&mut h, c);
    }
    assert!(
        h.query_by_label_contains("Move to another commit")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Move").click();
    settle(&mut h);
    assert_eq!(log(p, "HEAD"), "Y and Z\nX\nfirst");
    assert_eq!(
        git(p, &["show", "--format=", "--name-only", "HEAD~1"]),
        "x.txt\ny.txt"
    );
    wait_for(&mut h, "Moved y.txt to");
}
