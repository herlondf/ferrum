//! UI tests of the AI features with a fake local model: a tiny Ollama
//! server on 127.0.0.1 that streams canned answers. Nothing reaches the
//! internet.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

use crate::ai::{AiCmd, Subject};
use crate::app::FerrumApp;
use crate::command::Command;
use crate::commit_prefs::AiProvider;
use crate::icons;
use crate::tab::View;
use crate::ui_tests::{frames, git, harness, repo, settle};

/// The requests received: (path, body).
type Log = Arc<Mutex<Vec<(String, Value)>>>;

/// A fake Ollama: `/api/tags` lists one model, `/api/chat` streams the
/// answer `answer(system prompt, user prompt)` word by word.
fn ollama(answer: fn(&str, &str) -> String) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log: Log = Arc::default();
    let seen = log.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
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
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            seen.lock().unwrap().push((path.clone(), body.clone()));
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n"
            );
            if path == "/api/tags" {
                let _ = write!(stream, "{}", json!({"models": [{"name": "tiny-model"}]}));
                continue;
            }
            let text = |role: &str| {
                body["messages"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|m| m["role"] == role)
                    .filter_map(|m| m["content"].as_str())
                    .collect::<String>()
            };
            let reply = answer(&text("system"), &text("user"));
            for word in reply.split_inclusive(' ') {
                let chunk =
                    json!({"message": {"role": "assistant", "content": word}, "done": false});
                let _ = writeln!(stream, "{chunk}");
                let _ = stream.flush();
            }
            let _ = writeln!(stream, "{}", json!({"done": true}));
        }
    });
    (base, log)
}

fn run(h: &mut Harness<'_, FerrumApp>, cmd: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(cmd, &ctx);
    frames(h);
}

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

fn has(h: &Harness<'_, FerrumApp>, text: &str) -> bool {
    h.query_all_by_label_contains(text).next().is_some()
}

/// Turns AI on with the fake server.
fn use_ollama(h: &mut Harness<'_, FerrumApp>, base: &str) {
    let ai = &mut h.state_mut().settings.ai;
    ai.enabled = true;
    ai.provider = AiProvider::Ollama;
    ai.ollama_url = base.into();
    ai.ollama_model = "tiny-model".into();
    frames(h);
}

#[test]
fn explains_a_commit_in_the_side_panel() {
    let (base, log) = ollama(|_, user| {
        assert!(user.contains("first"), "the commit is sent: {user}");
        "This commit **adds** a.txt with one line.".into()
    });
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    use_ollama(&mut h, &base);
    let id = h.state().tabs[0].commits[0].id;
    run(&mut h, Command::Ai(AiCmd::Explain(Subject::Commit(id))));
    wait(&mut h, |h| has(h, "with one line."));
    assert!(has(&h, "Local · tiny-model"), "says where data goes");
    assert!(has(&h, "KB sent"));
    let (path, body) = log.lock().unwrap().last().cloned().unwrap();
    assert_eq!(path, "/api/chat");
    assert_eq!(body["model"], "tiny-model");
    assert!(
        body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("English")
    );
}

#[test]
fn nothing_is_sent_while_ai_is_off() {
    let (base, log) = ollama(|_, _| "should not be asked".into());
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    use_ollama(&mut h, &base);
    h.state_mut().settings.ai.enabled = false;
    frames(&mut h);
    let id = h.state().tabs[0].commits[0].id;
    run(&mut h, Command::Ai(AiCmd::Explain(Subject::Commit(id))));
    frames(&mut h);
    assert!(has(&h, "AI is off"));
    assert!(h.query_by_label_contains("AI settings…").is_some());
    std::thread::sleep(Duration::from_millis(100));
    frames(&mut h);
    assert!(log.lock().unwrap().is_empty(), "no request while off");
}

#[test]
fn secrets_are_masked_in_what_is_sent() {
    let (base, log) = ollama(|_, _| "Adds a key.".into());
    let dir = repo();
    let p = dir.path();
    let key = "AKIA".to_owned() + "QWERTYUIOPASDFGH";
    std::fs::write(p.join("a.txt"), format!("one\nkey = {key}\n")).unwrap();
    let mut h = harness(p);
    frames(&mut h);
    use_ollama(&mut h, &base);
    let of = ferrum_core::ai::prompts::DiffOf::Local {
        path: "a.txt".into(),
        staged: false,
    };
    run(&mut h, Command::Ai(AiCmd::Explain(Subject::Diff(of))));
    wait(&mut h, |h| has(h, "Adds a key."));
    let (_, body) = log.lock().unwrap().last().cloned().unwrap();
    let sent = body.to_string();
    assert!(!sent.contains(&key), "{sent}");
    assert!(sent.contains("secret masked by Ferrum"));
    assert!(has(&h, "1 line with a secret masked"));
}

#[test]
fn natural_language_search_offline_shows_chips_then_filters() {
    let dir = repo();
    let p = dir.path();
    git(p, &["commit", "-qam", "second"]);
    git(
        p,
        &[
            "-c",
            "user.name=Ana",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Ana's work",
        ],
    );
    let mut h = harness(p);
    frames(&mut h);
    {
        let t = &mut h.state_mut().tabs[0];
        t.history.ext.mode = crate::ui::history_model::SearchMode::Ask;
        t.history.search = "by Ana".into();
    }
    frames(&mut h);
    h.get_by_label(&icons::text(icons::AI, "Interpret")).click();
    frames(&mut h);
    assert!(has(&h, "Read offline (AI is off)"));
    assert_eq!(
        h.state().tabs[0]
            .history
            .ext
            .ask
            .filters
            .as_ref()
            .unwrap()
            .author,
        "Ana"
    );
    // Before searching, nothing is filtered.
    assert!(h.state().tabs[0].history.visible.is_none());
    h.get_by_label(&icons::text(icons::SEARCH, "Search"))
        .click();
    settle(&mut h);
    let t = &h.state().tabs[0];
    let visible = t.history.visible.clone().unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(t.commits[visible[0]].summary, "Ana's work");
}

#[test]
fn natural_language_search_with_the_model() {
    let (base, _) = ollama(|system, user| {
        assert!(system.contains("YYYY-MM-DD"));
        assert_eq!(user, "what did Ana change");
        "Sure:\n```json\n{\"author\": \"Ana\", \"until\": \"not a date\"}\n```".into()
    });
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    use_ollama(&mut h, &base);
    {
        let t = &mut h.state_mut().tabs[0];
        t.history.ext.mode = crate::ui::history_model::SearchMode::Ask;
        t.history.search = "what did Ana change".into();
    }
    frames(&mut h);
    h.get_by_label(&icons::text(icons::AI, "Interpret")).click();
    wait(&mut h, |h| has(h, "Read by the model"));
    let f = h.state().tabs[0].history.ext.ask.filters.clone().unwrap();
    assert_eq!(f.author, "Ana");
    assert!(f.until.is_empty(), "invalid dates are dropped");
    assert!(has(&h, "until: not a date"));
}

#[test]
fn splits_local_changes_into_reviewed_commits() {
    let (base, _) = ollama(|system, user| {
        assert!(system.contains("split"));
        assert!(user.contains("### H1") && user.contains("### H2"), "{user}");
        "Here you go: {\"commits\": [{\"message\": \"Change a\", \"changes\": [\"H1\"]}, \
         {\"message\": \"Add b\", \"changes\": [\"H2\"]}]}"
            .into()
    });
    let dir = repo();
    let p = dir.path();
    std::fs::write(p.join("b.txt"), "new\n").unwrap();
    let mut h = harness(p);
    frames(&mut h);
    use_ollama(&mut h, &base);
    run(&mut h, Command::Ai(AiCmd::Split));
    wait(&mut h, |h| has(h, "Make 2 commits"));
    h.get_by_label(&icons::text(icons::CHECK, "Make 2 commits"))
        .click();
    frames(&mut h);
    settle(&mut h);
    let log = git(p, &["log", "--format=%s", "-n", "3"]);
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        ["Add b", "Change a", "first"]
    );
    assert!(git(p, &["status", "--porcelain"]).is_empty());
    assert_eq!(h.state().tabs[0].view, View::History);
}

#[test]
fn suggests_a_conflict_resolution_for_review() {
    let (base, _) = ollama(|system, user| {
        assert!(system.contains("merge conflicts"));
        assert!(
            user.contains("Ours (main)") && user.contains("Theirs (feature)"),
            "{user}"
        );
        "Kept both lines.\n\n```\nmain line\nfeature line\n```\n".into()
    });
    let dir = repo();
    let p = dir.path();
    git(p, &["commit", "-qam", "second"]);
    git(p, &["checkout", "-qb", "feature"]);
    std::fs::write(p.join("a.txt"), "feature line\n").unwrap();
    git(p, &["commit", "-qam", "feature"]);
    git(p, &["checkout", "-q", "main"]);
    std::fs::write(p.join("a.txt"), "main line\n").unwrap();
    git(p, &["commit", "-qam", "main"]);
    let out = std::process::Command::new("git")
        .current_dir(p)
        .args(["merge", "-q", "feature"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "the merge conflicts");
    let mut h = harness(p);
    frames(&mut h);
    use_ollama(&mut h, &base);
    run(&mut h, Command::ResolveConflict("a.txt".into()));
    h.get_by_label(&icons::text(icons::AI, "Suggest resolution"))
        .click();
    wait(&mut h, |h| has(h, "Use this result"));
    assert!(has(&h, "Kept both lines."));
    // Nothing is written until the user takes it and marks it resolved.
    assert!(
        std::fs::read_to_string(p.join("a.txt"))
            .unwrap()
            .contains("<<<<<<<")
    );
    h.get_by_label(&icons::text(icons::CHECK, "Use this result"))
        .click();
    frames(&mut h);
    assert!(has(&h, "In the result below"));
    assert!(
        std::fs::read_to_string(p.join("a.txt"))
            .unwrap()
            .contains("<<<<<<<")
    );
}

#[test]
fn settings_test_the_connection_and_list_models() {
    let (base, log) = ollama(|_, _| String::new());
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    use_ollama(&mut h, &base);
    run(&mut h, Command::Ai(AiCmd::OpenSettings));
    assert!(has(&h, "Nothing leaves your machine."));
    h.get_by_label_contains("Test connection").click();
    wait(&mut h, |h| has(h, "Connected · 1 model"));
    let paths: Vec<String> = log.lock().unwrap().iter().map(|(p, _)| p.clone()).collect();
    assert_eq!(paths, ["/api/tags"], "the test sends no code");
}
