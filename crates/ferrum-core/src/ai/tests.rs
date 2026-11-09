use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::search::{Day, Filters, from_json, parse};
use super::*;
use crate::Oid;
use crate::secrets::{Allowlist, Config, Scanner};
use crate::testutil::TestRepo;

/// A fake model server: answers each request with `route(method, path,
/// body)` -> (status, content type, chunks); chunks are written one by
/// one, so the client sees a stream. Records the request bodies.
pub(crate) struct Fake {
    pub base: String,
    pub bodies: Arc<Mutex<Vec<(String, Value)>>>,
}

pub(crate) fn serve(
    route: impl Fn(&str, &str, &Value) -> (u16, &'static str, Vec<String>) + Send + 'static,
) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let log = bodies.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_owned();
            let path = parts.next().unwrap_or_default().to_owned();
            let mut len = 0;
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                    break;
                }
                if let Some((k, v)) = h.trim_end().split_once(':')
                    && k.eq_ignore_ascii_case("content-length")
                {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            let _ = reader.read_exact(&mut body);
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let (status, kind, chunks) = route(&method, &path, &body);
            log.lock().unwrap().push((path, body));
            let head =
                format!("HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nConnection: close\r\n\r\n");
            let _ = stream.write_all(head.as_bytes());
            for c in chunks {
                let _ = stream.write_all(c.as_bytes());
                let _ = stream.flush();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    });
    Fake { base, bodies }
}

fn ollama() -> Fake {
    serve(|method, path, body| match (method, path) {
        ("GET", "/api/tags") => (
            200,
            "application/json",
            vec![
                json!({"models": [{"name": "llama3.1:8b"}, {"name": "qwen2.5-coder"}]}).to_string(),
            ],
        ),
        ("POST", "/api/chat") if body["model"] == "missing" => (
            404,
            "application/json",
            vec![r#"{"error":"model 'missing' not found"}"#.into()],
        ),
        ("POST", "/api/chat") => {
            let words = ["Adds ", "a login ", "form."];
            let mut out: Vec<String> = words
                .iter()
                .map(|w| {
                    json!({"message": {"role": "assistant", "content": w}, "done": false})
                        .to_string()
                        + "\n"
                })
                .collect();
            out.push(json!({"message": {"content": ""}, "done": true}).to_string() + "\n");
            (200, "application/x-ndjson", out)
        }
        _ => (404, "text/plain", vec!["no".into()]),
    })
}

#[test]
fn ollama_streams_the_answer_and_lists_models() {
    let fake = ollama();
    let p = Provider::Ollama {
        url: format!("{}/", fake.base),
        model: "llama3.1:8b".into(),
    };
    assert_eq!(
        models(&p).unwrap(),
        vec!["llama3.1:8b".to_owned(), "qwen2.5-coder".to_owned()]
    );
    assert_eq!(
        test(&p).unwrap(),
        Check::Models(vec!["llama3.1:8b".into(), "qwen2.5-coder".into()])
    );
    let mut pieces = Vec::new();
    let cancel = AtomicBool::new(false);
    let prompt = Prompt::new("Be brief.", "Explain");
    let out = complete(&p, &prompt, &cancel, &mut |t| pieces.push(t.to_owned())).unwrap();
    assert_eq!(out, "Adds a login form.");
    assert_eq!(pieces, ["Adds ", "a login ", "form."]);
    let (path, body) = fake.bodies.lock().unwrap().last().cloned().unwrap();
    assert_eq!(path, "/api/chat");
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["content"], "Explain");

    let missing = Provider::Ollama {
        url: fake.base.clone(),
        model: "missing".into(),
    };
    match complete(&missing, &prompt, &cancel, &mut |_| {}) {
        Err(AiError::Http { status, detail }) => {
            assert_eq!(status, 404);
            assert!(detail.contains("not found"), "{detail}");
        }
        other => panic!("{other:?}"),
    }
    let no_model = Provider::Ollama {
        url: fake.base.clone(),
        model: " ".into(),
    };
    assert_eq!(
        complete(&no_model, &prompt, &cancel, &mut |_| {}),
        Err(AiError::Missing("model"))
    );
}

#[test]
fn openai_compatible_servers_stream_events() {
    let fake = serve(|method, path, body| match (method, path) {
        ("GET", "/v1/models") => (
            200,
            "application/json",
            vec![json!({"data": [{"id": "local-model"}]}).to_string()],
        ),
        ("POST", "/v1/chat/completions") if body["model"] == "plain" => (
            200,
            "application/json",
            vec![json!({"choices": [{"message": {"content": "All at once"}}]}).to_string()],
        ),
        ("POST", "/v1/chat/completions") => {
            let mut out: Vec<String> = ["Hel", "lo"]
                .iter()
                .map(|t| {
                    format!(
                        "data: {}\n\n",
                        json!({"choices": [{"delta": {"content": t}}]})
                    )
                })
                .collect();
            out.push(": keep-alive\n\n".into());
            out.push("data: [DONE]\n\n".into());
            (200, "text/event-stream", out)
        }
        _ => (
            401,
            "application/json",
            vec![r#"{"error":{"message":"bad key"}}"#.into()],
        ),
    });
    let p = Provider::OpenAi {
        url: format!("{}/v1", fake.base),
        key: String::new(),
        model: "local-model".into(),
    };
    assert_eq!(models(&p).unwrap(), vec!["local-model".to_owned()]);
    let cancel = AtomicBool::new(false);
    let mut seen = String::new();
    let out = complete(&p, &Prompt::new("", "Hi"), &cancel, &mut |t| {
        seen.push_str(t)
    })
    .unwrap();
    assert_eq!((out.as_str(), seen.as_str()), ("Hello", "Hello"));
    let (_, body) = fake.bodies.lock().unwrap().last().cloned().unwrap();
    // No system message when there is none.
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);

    let plain = Provider::OpenAi {
        url: format!("{}/v1", fake.base),
        key: "k".into(),
        model: "plain".into(),
    };
    assert_eq!(
        complete(&plain, &Prompt::new("", "Hi"), &cancel, &mut |_| {}).unwrap(),
        "All at once"
    );
    let wrong = Provider::OpenAi {
        url: format!("{}/nope", fake.base),
        key: String::new(),
        model: "x".into(),
    };
    match complete(&wrong, &Prompt::new("", "Hi"), &cancel, &mut |_| {}) {
        Err(AiError::Http {
            status: 401,
            detail,
        }) => assert_eq!(detail, "bad key"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn unreachable_servers_and_cancelling() {
    let p = Provider::Ollama {
        url: "http://127.0.0.1:1".into(),
        model: "m".into(),
    };
    assert!(matches!(models(&p), Err(AiError::Connect { .. })));
    let fake = ollama();
    let p = Provider::Ollama {
        url: fake.base.clone(),
        model: "m".into(),
    };
    let cancel = AtomicBool::new(true);
    assert_eq!(
        complete(&p, &Prompt::new("", "x"), &cancel, &mut |_| {}),
        Err(AiError::Cancelled)
    );
}

#[cfg(unix)]
#[test]
fn command_tools_get_the_prompt_on_stdin() {
    let cancel = AtomicBool::new(false);
    let p = Provider::Command {
        command: "tr a-z A-Z".into(),
    };
    let mut seen = String::new();
    let out = complete(&p, &Prompt::new("sys", "hello"), &cancel, &mut |t| {
        seen.push_str(t)
    })
    .unwrap();
    assert_eq!(out, "SYS\n\nHELLO");
    assert_eq!(seen, out);
    let failing = Provider::Command {
        command: "echo oops >&2; exit 3".into(),
    };
    match complete(&failing, &Prompt::new("", "x"), &cancel, &mut |_| {}) {
        Err(AiError::Failed { detail, .. }) => assert_eq!(detail, "oops"),
        other => panic!("{other:?}"),
    }
    let silent = Provider::Command {
        command: "cat >/dev/null".into(),
    };
    assert_eq!(
        complete(&silent, &Prompt::new("", "x"), &cancel, &mut |_| {}),
        Err(AiError::Empty)
    );
    assert!(matches!(
        test(&Provider::Command {
            command: "sh -c true".into()
        }),
        Ok(Check::Program(_))
    ));
    assert!(
        test(&Provider::Command {
            command: "no-such-tool-xyz".into()
        })
        .is_err()
    );
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let slow = Provider::Command {
        command: "sleep 5".into(),
    };
    assert_eq!(
        complete(&slow, &Prompt::new("", "x"), &cancel, &mut |_| {}),
        Err(AiError::Cancelled)
    );
}

#[test]
fn says_where_data_goes() {
    let ollama = |url: &str| Provider::Ollama {
        url: url.into(),
        model: "m".into(),
    };
    let openai = |url: &str| Provider::OpenAi {
        url: url.into(),
        key: String::new(),
        model: "m".into(),
    };
    let cmd = |c: &str| Provider::Command { command: c.into() };
    assert!(ollama("http://localhost:11434").destination().is_local());
    assert!(ollama("http://127.0.0.1:11434").destination().is_local());
    assert!(
        openai("http://192.168.1.20:1234/v1")
            .destination()
            .is_local()
    );
    assert!(openai("http://[::1]:8080/v1").destination().is_local());
    assert!(openai("http://gpu-box:8000/v1").destination().is_local());
    assert_eq!(
        openai("https://api.openai.com/v1").destination(),
        Destination::Remote {
            name: "OpenAI".into()
        }
    );
    assert_eq!(
        openai("https://llm.example.com/v1").destination(),
        Destination::Remote {
            name: "llm.example.com".into()
        }
    );
    assert_eq!(
        cmd("claude -p").destination(),
        Destination::Tool {
            program: "claude".into(),
            service: Some("Anthropic".into())
        }
    );
    assert_eq!(
        cmd("/usr/bin/codex exec -").destination(),
        Destination::Tool {
            program: "codex".into(),
            service: Some("OpenAI".into())
        }
    );
    assert_eq!(
        cmd("my-tool").destination(),
        Destination::Tool {
            program: "my-tool".into(),
            service: None
        }
    );
    assert!(cmd("ollama run llama3").destination().is_local());
}

#[test]
fn reads_json_out_of_chatty_answers() {
    let want = json!({"author": "Ana"});
    for text in [
        r#"{"author": "Ana"}"#,
        "```json\n{\"author\": \"Ana\"}\n```",
        "Sure! Here are the filters:\n```\n{\"author\": \"Ana\"}\n```\nLet me know if…",
        "The filters are {\"author\": \"Ana\"} — hope that helps.",
        "<think>The user wants {author}…</think>\n{\"author\": \"Ana\"}",
        "Using {braces} in prose first, then {\"author\": \"Ana\"} trailing",
        "```json\n{\"author\": \"Ana\"}",
    ] {
        assert_eq!(json::extract(text), Some(want.clone()), "{text}");
    }
    // Braces inside strings do not end the object.
    assert_eq!(
        json::extract(r#"x {"message": "use } and {", "n": 1} y"#),
        Some(json!({"message": "use } and {", "n": 1}))
    );
    assert_eq!(json::extract("[1, 2] and more"), Some(json!([1, 2])));
    assert_eq!(json::extract("no json here"), None);
    assert_eq!(json::extract("{broken"), None);
}

#[test]
fn strips_thinking_and_caps() {
    assert_eq!(strip_thinking("<think>hmm</think>\nAnswer"), "Answer");
    assert_eq!(strip_thinking("<think>still going"), "");
    assert!(is_thinking("<think>still"));
    assert!(!is_thinking("<think>a</think> b"));
    let (text, cut) = cap("one\ntwo\nthree\n", 9);
    assert!(cut);
    assert!(
        text.starts_with("one\ntwo\n") && !text.contains("three"),
        "{text}"
    );
    assert_eq!(cap("short", 10), ("short".into(), false));
}

fn day(s: &str) -> Day {
    Day::parse(s).unwrap()
}

#[test]
fn dates_are_real_calendar_days() {
    for s in ["1970-01-01", "2024-02-29", "2026-11-09", "2000-12-31"] {
        let d = day(s);
        assert_eq!(Day::from_days(d.to_days()), d, "{s}");
        assert_eq!(d.format(), s);
    }
    assert_eq!(Day::parse("2023-02-29"), None);
    assert_eq!(Day::parse("2024-13-01"), None);
    assert_eq!(Day::parse("yesterday"), None);
    assert_eq!(Day::parse("2024-03"), Some(day("2024-03-01")));
    assert_eq!(Day::parse("2024/03/05T10:00:00"), Some(day("2024-03-05")));
    assert_eq!(day("2024-03-31").add_months(-1), day("2024-02-29"));
    assert_eq!(day("2026-01-15").add_months(-1), day("2025-12-15"));
    assert_eq!(day("2026-11-09").weekday(), 0, "a Monday");
    assert_eq!(day("2026-11-15").weekday(), 6);
}

#[test]
fn fallback_parser_reads_simple_phrases() {
    let today = day("2026-11-12"); // a Thursday
    let f = |author: &str, since: &str, until: &str, path: &str, message: &str, content: &str| {
        Filters {
            author: author.into(),
            since: since.into(),
            until: until.into(),
            path: path.into(),
            message: message.into(),
            content: content.into(),
        }
    };
    let cases = [
        (
            "commits that touched login last month by Ana",
            f("Ana", "2026-10-12", "", "*login*", "", ""),
        ),
        ("last week", f("", "2026-11-05", "", "", "", "")),
        ("by Ana", f("Ana", "", "", "", "", "")),
        ("in src/", f("", "", "", "src/", "", "")),
        (
            "by Ana Silva in src/ui",
            f("Ana Silva", "", "", "src/ui", "", ""),
        ),
        ("yesterday", f("", "2026-11-11", "2026-11-11", "", "", "")),
        ("this week", f("", "2026-11-09", "", "", "", "")),
        ("this month", f("", "2026-11-01", "", "", "", "")),
        ("last 3 days", f("", "2026-11-09", "", "", "", "")),
        ("2 weeks ago", f("", "2026-10-29", "", "", "", "")),
        (
            "since 2026-01-01 before 2026-03-01",
            f("", "2026-01-01", "2026-02-28", "", "", ""),
        ),
        ("in 2024", f("", "2024-01-01", "2024-12-31", "", "", "")),
        ("changes to README.md", f("", "", "", "README.md", "", "")),
        ("containing TODO", f("", "", "", "", "", "TODO")),
        (
            "about \"login page\" by bob",
            f("bob", "", "", "", "\"login page\"", ""),
        ),
        ("fix crash", f("", "", "", "", "fix crash", "")),
        (
            "author:ana@example.com",
            f("ana@example.com", "", "", "", "", ""),
        ),
        // Portuguese
        (
            "commits que mexeram em login no mês passado por Ana",
            f("Ana", "2026-10-12", "", "*login*", "", ""),
        ),
        ("semana passada", f("", "2026-11-05", "", "", "", "")),
        ("da Ana em src/", f("Ana", "", "", "src/", "", "")),
        ("ontem", f("", "2026-11-11", "2026-11-11", "", "", "")),
        ("últimos 2 meses", f("", "2026-09-12", "", "", "", "")),
        (
            "desde 2025 contendo unwrap",
            f("", "2025-01-01", "", "", "", "unwrap"),
        ),
    ];
    for (query, want) in cases {
        assert_eq!(parse(query, today), want, "{query}");
    }
}

#[test]
fn model_filters_are_validated() {
    let (f, dropped) = from_json(&json!({
        "author": " Ana ",
        "since": "2026-10-01",
        "until": "last friday",
        "path": "/src/login",
        "message": null,
        "content": "",
    }));
    assert_eq!(f.author, "Ana");
    assert_eq!(f.since, "2026-10-01");
    assert_eq!(f.until, "");
    assert_eq!(f.path, "src/login");
    assert!(f.message.is_empty() && f.content.is_empty());
    assert_eq!(dropped, ["until: last friday"]);

    // Wrapped, synonyms, swapped dates, bare words, arrays.
    let (f, dropped) = from_json(&json!({"filters": {
        "after": "2026-05-01", "before": "2026-01-01", "file": "login", "author": ["Bob"],
        "grep": "fix"
    }}));
    assert!(dropped.is_empty());
    assert_eq!(
        (f.since.as_str(), f.until.as_str()),
        ("2026-01-01", "2026-05-01")
    );
    assert_eq!(f.path, "*login*");
    assert_eq!(f.author, "Bob");
    assert_eq!(f.message, "fix");
    let (f, dropped) = from_json(&json!({"path": "../../etc/passwd"}));
    assert!(f.path.is_empty());
    assert_eq!(dropped.len(), 1);
}

#[test]
fn filters_become_log_arguments_and_find_commits() {
    let r = TestRepo::new();
    r.commit("src/login.rs", "a\n", "Add login");
    r.git
        .run(&[
            "-c",
            "user.name=Ana",
            "-c",
            "user.email=ana@x.y",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Fix login crash",
        ])
        .unwrap();
    r.commit("README.md", "TODO\n", "Docs");
    let all = |f: &Filters| super::search::run(&r.git, f, true).unwrap().len();
    let mut f = Filters {
        author: "ana".into(),
        ..Default::default()
    };
    assert_eq!(all(&f), 1);
    f.author.clear();
    f.message = "fix login".into();
    assert_eq!(all(&f), 1);
    f.message = "login".into();
    assert_eq!(all(&f), 2);
    f.message.clear();
    f.path = "*login*".into();
    assert_eq!(all(&f), 1);
    f.path.clear();
    f.content = "TODO".into();
    assert_eq!(all(&f), 1);
    f.content.clear();
    f.since = Day::today().add_days(2).format();
    assert_eq!(all(&f), 0);
    let args = Filters {
        since: "2026-01-02".into(),
        until: "2026-01-03".into(),
        ..Default::default()
    }
    .log_args(false);
    assert!(args.contains(&"--since=2026-01-02 00:00:00".to_owned()));
    assert!(args.contains(&"--until=2026-01-03 23:59:59".to_owned()));
    assert!(args.contains(&"HEAD".to_owned()));
}

fn scanner() -> Scanner {
    Scanner::new(&Config::default(), Allowlist::default()).0
}

#[test]
fn secrets_are_masked_before_sending() {
    let key = "AKIA".to_owned() + "QWERTYUIOPASDFGH";
    let diff = format!(
        "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1,2 @@\n fn a() {{}}\n+let key = \"{key}\";\n\
diff --git a/.env b/.env\nnew file mode 100644\n--- /dev/null\n+++ b/.env\n@@ -0,0 +1 @@\n+PASSWORD=hunter2\n\
diff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-old\n+new\n"
    );
    let r = redact::redact(&scanner(), "", &diff);
    assert!(!r.text.contains(&key), "{}", r.text);
    assert!(!r.text.contains("hunter2"), "{}", r.text);
    assert!(
        r.text
            .contains("+[secret masked by Ferrum: AWS access key]"),
        "{}",
        r.text
    );
    assert!(r.text.contains("+new") && r.text.contains(" fn a() {}"));
    assert_eq!(r.masked, 2);
    assert_eq!(r.kinds.len(), 2, "{:?}", r.kinds);
    // Plain text is scanned too.
    let r = redact::redact(&scanner(), "notes.txt", &format!("token {key}\nfine\n"));
    assert_eq!(r.text, "[secret masked by Ferrum: AWS access key]\nfine\n");
    let r = redact::redact(&scanner(), "", "nothing secret\n");
    assert_eq!((r.masked, r.text.as_str()), (0, "nothing secret\n"));
}

#[test]
fn conflict_answers_give_explanation_and_file() {
    let answer =
        "<think>x</think>Both sides renamed `a`.\n\n```rust\nfn b() {}\nfn c() {}\n```\nDone.";
    let r = prompts::parse_resolution(answer, true).unwrap();
    assert_eq!(r.explanation, "Both sides renamed `a`.");
    assert_eq!(r.text, "fn b() {}\nfn c() {}\n");
    assert!(!r.has_markers);
    let r = prompts::parse_resolution("```\n<<<<<<< HEAD\na\n=======\nb\n>>>>>>> x\n```", false)
        .unwrap();
    assert!(r.has_markers);
    assert!(!r.text.ends_with('\n'));
    assert!(prompts::parse_resolution("no code block", true).is_none());
}

#[test]
fn prompts_carry_the_context() {
    let r = TestRepo::new();
    r.commit("a.txt", "one\n", "First");
    r.git.run(&["checkout", "-q", "-b", "feature"]).unwrap();
    r.commit("a.txt", "one\ntwo\n", "Add two");
    let head = r.git.run(&["rev-parse", "HEAD"]).unwrap();
    let id = Oid::from_str(head.trim()).unwrap();
    let p = prompts::explain_commit(&r.git, id, "English", 10_000).unwrap();
    assert!(
        p.user.contains("Add two") && p.user.contains("+two"),
        "{}",
        p.user
    );
    let p = prompts::explain_history(&r.git, "a.txt", "Portuguese", 10_000).unwrap();
    assert!(p.system.contains("Portuguese"));
    assert!(p.user.contains("First") && p.user.contains("Add two"));
    let p = prompts::pull_request(&r.git, "main", "feature", "English", 10_000).unwrap();
    assert!(
        p.user.contains("- Add two") && p.user.contains("+two"),
        "{}",
        p.user
    );
    r.write("a.txt", "one\ntwo\nthree\n");
    r.write("new.txt", "fresh\n");
    let local = |path: &str| prompts::DiffOf::Local {
        path: path.into(),
        staged: false,
    };
    let p = prompts::explain_diff(&r.git, &local("a.txt"), "English", 10_000).unwrap();
    assert!(p.user.contains("+three"));
    let p = prompts::explain_diff(&r.git, &local("new.txt"), "English", 10_000).unwrap();
    assert!(p.user.contains("+fresh"));
    let small = prompts::explain_commit(&r.git, id, "English", 50).unwrap();
    assert!(small.user.contains("[… cut: too long]"));
}

use crate::ai::split::{self, Group, Kind};

#[test]
fn split_cuts_changes_into_items_and_commits_groups() {
    let r = TestRepo::new();
    let long: String = (1..=30).map(|i| format!("line {i}\n")).collect();
    r.commit("src/a.txt", &long, "base");
    r.commit("docs/b.txt", "b\n", "base 2");
    // Two hunks in a.txt, a change in b.txt, a new file, one staged.
    let changed = long
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 28\n", "line 28 changed\n");
    r.write("src/a.txt", &changed);
    r.write("docs/b.txt", "b2\n");
    r.git.run(&["add", "docs/b.txt"]).unwrap();
    r.write("new.txt", "new\n");
    let changes = split::collect(&r.git).unwrap();
    let summary: Vec<(String, &str, bool)> = changes
        .items
        .iter()
        .map(|i| {
            (
                i.id.clone(),
                i.path.as_str(),
                matches!(i.kind, Kind::Hunk { .. }),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("H1".into(), "docs/b.txt", true),
            ("H2".into(), "src/a.txt", true),
            ("H3".into(), "src/a.txt", true),
            ("H4".into(), "new.txt", false),
        ]
    );
    assert_eq!((changes.items[1].added, changes.items[1].removed), (1, 1));

    // The model's answer, with mistakes: an unknown id, a repeat, H4 left out.
    let answer = json!({"commits": [
        {"message": "Change the second line", "changes": ["H2", "H9"]},
        {"message": "Docs and the end", "changes": ["H1", "H3", "H2"]},
        {"message": "Empty", "changes": []},
    ]});
    let mut groups = split::parse_groups(&answer, &changes);
    assert_eq!(
        groups,
        [
            Group {
                message: "Change the second line".into(),
                items: vec!["H2".into()]
            },
            Group {
                message: "Docs and the end".into(),
                items: vec!["H1".into(), "H3".into()]
            },
            Group {
                message: String::new(),
                items: vec!["H4".into()]
            },
        ]
    );
    // The user names the last group.
    groups[2].message = "Add new.txt".into();
    let made = split::apply(&r.git, &changes, &groups, |_| Ok(()), |_| {}).unwrap();
    assert_eq!(made, 3);
    let log = r.git.run(&["log", "--format=%s", "-n", "3"]).unwrap();
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        ["Add new.txt", "Docs and the end", "Change the second line"]
    );
    let first = r
        .git
        .run(&["show", "--format=", "--stat", "HEAD~2"])
        .unwrap();
    assert!(
        first.contains("src/a.txt") && !first.contains("b.txt"),
        "{first}"
    );
    // Nothing was lost or left over.
    assert_eq!(r.read("src/a.txt"), changed);
    assert!(
        r.git
            .run(&["status", "--porcelain"])
            .unwrap()
            .trim()
            .is_empty()
    );
}

#[test]
fn split_refuses_stale_plans_and_keeps_what_is_left_out() {
    let r = TestRepo::new();
    r.commit("a.txt", "a\n", "base");
    r.commit("b.txt", "b\n", "base 2");
    r.write("a.txt", "a2\n");
    r.write("b.txt", "b2\n");
    let changes = split::collect(&r.git).unwrap();
    let groups = vec![Group {
        message: "A".into(),
        items: vec!["H1".into()],
    }];
    // Changed since the proposal.
    r.write("b.txt", "b3\n");
    assert!(split::apply(&r.git, &changes, &groups, |_| Ok(()), |_| {}).is_err());
    let changes = split::collect(&r.git).unwrap();
    // The check before committing can stop it.
    let stop = split::apply(
        &r.git,
        &changes,
        &groups,
        |_| Err(crate::Error::Other("secret".into())),
        |_| {},
    );
    assert!(stop.is_err());
    r.git.run(&["reset", "-q"]).unwrap();
    assert_eq!(
        split::apply(&r.git, &changes, &groups, |_| Ok(()), |_| {}).unwrap(),
        1
    );
    assert_eq!(r.read("b.txt"), "b3\n");
    let status = r.git.run(&["status", "--porcelain"]).unwrap();
    assert_eq!(status.trim(), "M b.txt");
    let by_folder = split::by_folder(&split::collect(&r.git).unwrap());
    assert_eq!(by_folder.len(), 1);
    assert_eq!(by_folder[0].message, "Update top-level files");
}
