//! UI tests of hosted services: password prompts, issues, the remote used
//! for pull requests, templates and the AI review. A local HTTP server
//! stands in for the service; nothing reaches the internet.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use ferrum_core::forge::ForgeKind;

use crate::app::FerrumApp;
use crate::command::Command;
use crate::hosted::HostedCmd;
use crate::settings::ForgeAccount;
use crate::tab::View;
use crate::ui_tests::{frames, git, harness, repo};

/// Requests received by the fake server: (method, path, body).
type Log = Arc<Mutex<Vec<(String, String, String)>>>;

/// A fake API answering GET with `list` and POST with `created`.
fn serve(list: &'static str, created: &'static str) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log: Log = Arc::default();
    let seen = log.clone();
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
            let reply = if method == "POST" { created } else { list };
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

fn run(h: &mut Harness<'_, FerrumApp>, cmd: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(cmd, &ctx);
    frames(h);
}

/// Steps until `cond` holds (requests run on other threads).
fn wait(h: &mut Harness<'_, FerrumApp>, cond: impl Fn(&Harness<'_, FerrumApp>) -> bool) {
    for _ in 0..500 {
        h.step();
        if cond(h) {
            frames(h);
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out");
}

fn type_in(h: &mut Harness<'_, FerrumApp>, placeholder: &'static str, text: &str) {
    h.get_by(move |n| n.placeholder() == Some(placeholder))
        .focus();
    frames(h);
    h.get_by(move |n| n.placeholder() == Some(placeholder))
        .type_text(text);
    frames(h);
}

fn has_label(h: &Harness<'_, FerrumApp>, text: &str) -> bool {
    h.query_by_label_contains(text).is_some()
}

#[test]
fn password_prompt_answers_git() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    let (req, answer) =
        ferrum_core::askpass::Request::new("Password for 'https://ana@example.com': ");
    h.state_mut().prompts().push(req);
    frames(&mut h);
    assert_eq!(
        h.state().prompts_shown().as_deref(),
        Some("Password for 'https://ana@example.com': ")
    );
    type_in(&mut h, "Password", "hunter2");
    h.get_by_label("OK").click();
    frames(&mut h);
    assert_eq!(answer.try_recv().unwrap().as_deref(), Some("hunter2"));
    assert!(h.state().prompts_shown().is_none());

    // ssh's host question gets yes or no.
    let (req, answer) = ferrum_core::askpass::Request::new(
        "The authenticity of host 'h (1.2.3.4)' can't be established.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ",
    );
    h.state_mut().prompts().push(req);
    frames(&mut h);
    h.get_by_label("Trust and connect").click();
    frames(&mut h);
    assert_eq!(answer.try_recv().unwrap().as_deref(), Some("yes"));
}

/// A repository whose origin is a GitHub Enterprise server at `base`.
fn hosted_repo(h: &mut Harness<'_, FerrumApp>, dir: &std::path::Path, base: &str) {
    git(
        dir,
        &["remote", "add", "origin", &format!("{base}/acme/app.git")],
    );
    let app = h.state_mut();
    app.settings.accounts.push(ForgeAccount {
        kind: ForgeKind::GitHub,
        host: base.into(),
        username: "ana".into(),
        token: "tok".into(),
        user: String::new(),
        notifications: false,
    });
    crate::hosted::sync_registry(&app.settings);
    let t = &mut app.tabs[0];
    t.remotes = t.repo.remotes().unwrap();
    t.hosted = crate::hosted::hosted_for(t);
    assert_eq!(t.hosted.as_ref().map(|h| h.kind), Some(ForgeKind::GitHub));
}

#[test]
fn lists_filters_and_creates_issues() {
    let (base, log) = serve(
        r#"[{"number": 4, "title": "Crash on start", "user": {"login": "bo"}, "state": "open",
             "assignees": [], "labels": [{"name": "bug"}], "html_url": "u", "updated_at": "2026-01-01T00:00:00Z"}]"#,
        r#"{"number": 5, "title": "New one", "user": {"login": "ana"}, "state": "open",
            "html_url": "u5", "updated_at": "2026-01-01T00:00:00Z"}"#,
    );
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    hosted_repo(&mut h, dir.path(), &base);
    frames(&mut h);
    h.get_by_label("📋 Issues").click();
    wait(&mut h, |h| has_label(h, "Crash on start"));
    assert_eq!(h.state().tabs[0].view, View::Issues);
    assert!(
        log.lock().unwrap()[0]
            .1
            .starts_with("/api/v3/repos/acme/app/issues?state=open")
    );

    // Filter by author; the search is remembered.
    type_in(&mut h, "Author", "bo");
    h.get_by_label("Search").click();
    wait(&mut h, |_| log.lock().unwrap().len() >= 2);
    wait(&mut h, |h| !h.state().tabs[0].issues.loading());
    assert!(
        log.lock().unwrap()[1].1.contains("&creator=bo"),
        "{:?}",
        log.lock().unwrap()
    );
    assert_eq!(
        h.state().settings.forge.recent_issue_searches[0].author,
        "bo"
    );

    // A new issue.
    h.get_by_label("New issue…").click();
    frames(&mut h);
    type_in(&mut h, "Issue title", "New one");
    h.get_by_label("Create issue").click();
    wait(&mut h, |h| {
        has_label(h, "New one")
            && h.state().tabs[0]
                .issues
                .list
                .as_ref()
                .is_some_and(|l| l.len() == 2)
    });
    let post = log
        .lock()
        .unwrap()
        .iter()
        .find(|r| r.0 == "POST")
        .cloned()
        .unwrap();
    assert_eq!(post.1, "/api/v3/repos/acme/app/issues");
    let sent: serde_json::Value = serde_json::from_str(&post.2).unwrap();
    assert_eq!(sent["title"], "New one");
}

#[test]
fn picks_the_remote_for_pull_requests() {
    let dir = repo();
    let p = dir.path();
    git(
        p,
        &["remote", "add", "origin", "https://github.com/me/app.git"],
    );
    git(
        p,
        &[
            "remote",
            "add",
            "upstream",
            "https://gitlab.com/team/app.git",
        ],
    );
    let mut h = harness(p);
    frames(&mut h);
    {
        let t = &mut h.state_mut().tabs[0];
        t.remotes = t.repo.remotes().unwrap();
        t.hosted = crate::hosted::hosted_for(t);
        assert_eq!(t.hosted.as_ref().unwrap().kind, ForgeKind::GitHub);
    }
    run(
        &mut h,
        Command::Hosted(HostedCmd::SetRemote("upstream".into())),
    );
    let app = h.state();
    assert_eq!(app.tabs[0].hosted.as_ref().unwrap().path, "team/app");
    assert_eq!(
        app.settings
            .forge
            .remotes
            .values()
            .next()
            .map(String::as_str),
        Some("upstream")
    );
    // A reload keeps the choice.
    let t = &h.state().tabs[0];
    assert_eq!(
        crate::hosted::hosted_for(t).unwrap().kind,
        ForgeKind::GitLab
    );
}

#[test]
fn new_pull_request_starts_from_the_template() {
    let dir = repo();
    let p = dir.path();
    git(
        p,
        &["remote", "add", "origin", "https://github.com/me/app.git"],
    );
    std::fs::create_dir_all(p.join(".github")).unwrap();
    std::fs::write(
        p.join(".github/pull_request_template.md"),
        "## Why\n\nTemplate text",
    )
    .unwrap();
    let mut h = harness(p);
    frames(&mut h);
    let d = {
        let t = &mut h.state_mut().tabs[0];
        t.remotes = t.repo.remotes().unwrap();
        t.hosted = crate::hosted::hosted_for(t);
        crate::ui::forge::new_pull_request(t, "main")
    };
    run(
        &mut h,
        Command::Dialog(crate::ui::dialogs::Dialog::Forge(d)),
    );
    assert!(
        h.get_all_by(|n| n.value().is_some_and(|v| v.contains("Template text")))
            .next()
            .is_some()
    );
    let picker = h.get_all_by(|n| {
        n.value()
            .is_some_and(|v| v == "Template: pull_request_template")
    });
    assert!(picker.count() > 0);
}

#[cfg(unix)]
#[test]
fn ai_review_shows_the_answer() {
    let dir = repo();
    let p = dir.path();
    git(p, &["commit", "-qam", "second"]);
    let mut h = harness(p);
    frames(&mut h);
    h.state_mut().settings.ai.command = "grep '^+two'".into();
    run(
        &mut h,
        Command::Hosted(HostedCmd::Review {
            base: "HEAD~1".into(),
            head: "HEAD".into(),
        }),
    );
    h.get_by_label("Start review").click();
    wait(&mut h, |h| has_label(h, "+two"));
}

#[test]
fn settings_show_accounts_and_ssh_keys() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    h.state_mut().settings.accounts.push(ForgeAccount {
        kind: ForgeKind::Bitbucket,
        host: "bitbucket.org".into(),
        username: "ana".into(),
        token: "t".into(),
        user: "ana".into(),
        notifications: false,
    });
    run(
        &mut h,
        Command::DevStep(crate::command::DevStep::SettingsSection("accounts".into())),
    );
    h.get_by_label_contains("Bitbucket\nana · bitbucket.org");
    h.get_by_label("Update token…");
    run(
        &mut h,
        Command::DevStep(crate::command::DevStep::SettingsSection("ssh".into())),
    );
    h.get_by_label("Generate new key…").click();
    frames(&mut h);
    h.get_by_label("Generate ED25519 key");
}
